//! Skill-test result panel (#478): renders the just-resolved test — chaos token
//! drawn, final total vs difficulty, pass/fail by N — from the three things the
//! store latches for it:
//! [`last_skill_test_result`](crate::store::ClientState::last_skill_test_result),
//! [`last_skill_test_difficulty`](crate::store::ClientState::last_skill_test_difficulty)
//! and [`last_revealed_token`](crate::store::ClientState::last_revealed_token).
//! It reads no live event batch: the three are routinely determined in different
//! applies from each other and from the pause this renders on (#787, #853).
//! Since #541 it is a centred modal carrying **its own** Confirm — the
//! acknowledge pause's only one — rather than a panel beside a button in the
//! retired action bar.

use game_core::state::{ChaosToken, TokenResolution};
use game_core::{Event, FailureReason};
use leptos::prelude::*;

use crate::store::use_store;

/// The data the result panel renders: a display string for the drawn token, the
/// final total vs difficulty, and a player-facing outcome line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillTestSummary {
    pub token: String,
    pub total: i8,
    pub difficulty: i8,
    pub outcome: String,
}

/// Build a [`SkillTestSummary`] from the store's latched skill-test state, or
/// `None` if no resolution is being held or the difficulty is unknown. Pure — no
/// DOM, unit-tested on native.
///
/// `total` is reconstructed from the logged margin: `difficulty + margin` on a
/// success, `difficulty - by` on a failure (an `AutoFail` reports `by =
/// difficulty`, so the total clamps to 0).
///
/// **Every input is a latch, and none is read off the live batch.** A resolution
/// can span several apply batches, and the batch carrying the outcome is not
/// reliably the one the panel renders on: an ST.4 effect that suspends for input
/// strands the reveal an earlier batch back (#787), and a reaction on
/// `SkillTestResolved` — Lita Chantler 01117 — fires in the same apply as the
/// outcome, so the acknowledge pause arrives on an *empty* batch (#853).
/// [`crate::store::reduce`] owns holding all three across those boundaries and
/// releasing them at `SkillTestEnded`. `None` for the token means no token was
/// drawn at all, and renders the em dash.
#[must_use]
pub fn summarize(state: &crate::store::ClientState) -> Option<SkillTestSummary> {
    let difficulty = state.last_skill_test_difficulty?;
    let token = state
        .last_revealed_token
        .map(|(token, resolution)| token_display(token, resolution));
    match state.last_skill_test_result.as_ref()? {
        Event::SkillTestSucceeded { margin, .. } => Some(SkillTestSummary {
            token: token.unwrap_or_else(|| "—".to_string()),
            total: difficulty.saturating_add(*margin),
            difficulty,
            outcome: format!("Succeeded by {margin}"),
        }),
        Event::SkillTestFailed { reason, by, .. } => {
            let note = if matches!(reason, FailureReason::AutoFail) {
                " (auto-fail)"
            } else {
                ""
            };
            Some(SkillTestSummary {
                token: token.unwrap_or_else(|| "—".to_string()),
                total: difficulty.saturating_sub(*by),
                difficulty,
                outcome: format!("Failed by {by}{note}"),
            })
        }
        // The store latches nothing else into this slot.
        _ => None,
    }
}

/// A short display string for the drawn token and how it resolved (e.g.
/// `"+1"`, `"Skull (-2)"`, `"AutoFail (auto-fail)"`).
fn token_display(token: ChaosToken, resolution: TokenResolution) -> String {
    let suffix = match resolution {
        TokenResolution::Modifier(n) => format!("{n:+}"),
        TokenResolution::AutoFail => "auto-fail".to_string(),
        TokenResolution::ElderSign => "elder sign".to_string(),
        // `TokenResolution` is #[non_exhaustive]; a future kind gets a placeholder.
        _ => "?".to_string(),
    };
    match token {
        // A numeric token reads cleanly as just its signed value.
        ChaosToken::Numeric(n) => format!("{n:+}"),
        // `ChaosToken` is #[non_exhaustive]; render the symbol via Debug + suffix.
        other => format!("{other:?} ({suffix})"),
    }
}

/// True iff the skill-test result modal is up: the store is holding a resolution
/// *and* the live prompt is the un-anchored `Confirm` acknowledge pause.
///
/// Public because the prompt banner needs it — the modal carries that pause's only
/// Confirm, so the banner must not render a second one (#541). The rule lives here,
/// with the view it governs, rather than being re-derived by the banner. Pure.
#[must_use]
pub fn modal_is_live(state: &crate::store::ClientState) -> bool {
    summarize(state).is_some()
        && matches!(
            &state.outcome,
            Some(game_core::EngineOutcome::AwaitingInput { request, .. })
                if request.kind == game_core::InputKind::Confirm && request.target.is_none()
        )
}

/// Result modal for the just-resolved skill test (#478, made a modal by #541).
/// Renders nothing unless the store is holding a skill-test result
/// *and* the live prompt is the un-anchored `Confirm` acknowledge pause — that
/// pairing is what guarantees the modal always has a way out, since the modal
/// carries the pause's only Confirm.
///
/// **Dismissible only by that button.** Clicking Confirm submits real engine
/// input, so a backdrop click or an Escape key would advance the game on the
/// player's behalf; neither is wired, deliberately. The backdrop is inert —
/// it exists to stop the player scrolling past the token that decided the test.
///
/// **Draggable** (#857, [`crate::drag`]). Since dismissal is engine input, a
/// player who wants to check the board before acknowledging otherwise has no way
/// to get the modal out of the way; dragging is that way, and the backdrop fades
/// once the modal has been moved. The dismissal rule is untouched — moving the
/// modal is not dismissing it, and a press that lands on Confirm starts no drag.
#[component]
pub fn SkillTestResultView() -> impl IntoView {
    let store = use_store();
    // The prompt's fingerprint: which applied batch a live modal is up for. The
    // batch count is what tells one prompt from the next when neither liveness
    // nor the rendered content does — two identical tests running back to back
    // (#857).
    let drag = crate::drag::Drag::per_prompt(move || {
        let st = store.get();
        modal_is_live(&st).then_some(st.log.len())
    });
    view! {
        {move || {
            let st = store.get();
            if !modal_is_live(&st) {
                return ().into_any();
            }
            let Some(s) = summarize(&st) else {
                return ().into_any();
            };
            view! {
                // No `on:click` on the backdrop: dismissal is engine input.
                <div class="str-backdrop" style=move || drag.scrim_style()></div>
                <section
                    class="skill-test-result"
                    style=move || drag.transform_style()
                    on:pointerdown=move |ev| drag.down(&ev)
                    on:pointermove=move |ev| drag.movement(&ev)
                    on:pointerup=move |ev| drag.up(&ev)
                >
                    <p class="str-token">"Chaos token: " {s.token}</p>
                    <p class="str-total">
                        "Total " {s.total} " vs difficulty " {s.difficulty}
                    </p>
                    <p class="str-outcome">{s.outcome}</p>
                    <button
                        class="str-confirm"
                        on:click={
                            #[cfg(target_arch = "wasm32")]
                            {
                                move |_| {
                                    crate::controls::submit(
                                        game_core::InputResponse::Confirm,
                                        "Confirm",
                                    );
                                }
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            { |_| () }
                        }
                    >
                        "Confirm"
                    </button>
                </section>
            }
            .into_any()
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ClientState;
    use game_core::state::{ChaosToken, InvestigatorId, SkillKind, TokenResolution};
    use game_core::{Event, FailureReason};

    /// A store holding what the panel renders from: the difficulty announced at
    /// ST.1, the token drawn (or none), and the resolution. The three arrive in
    /// different batches often enough that the panel never sees them together
    /// except here (#787, #853).
    fn holding(
        difficulty: Option<i8>,
        token: Option<(ChaosToken, TokenResolution)>,
        result: Option<Event>,
    ) -> ClientState {
        ClientState {
            last_skill_test_difficulty: difficulty,
            last_revealed_token: token,
            last_skill_test_result: result,
            ..Default::default()
        }
    }

    /// The store's latch for a numeric token of `modifier`.
    fn latched(modifier: i8) -> (ChaosToken, TokenResolution) {
        (
            ChaosToken::Numeric(modifier),
            TokenResolution::Modifier(modifier),
        )
    }

    fn succeeded(margin: i8) -> Event {
        Event::SkillTestSucceeded {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            margin,
        }
    }

    fn failed(reason: FailureReason, by: i8) -> Event {
        Event::SkillTestFailed {
            investigator: InvestigatorId(1),
            skill: SkillKind::Combat,
            reason,
            by,
        }
    }

    #[test]
    fn summarizes_a_success() {
        let s = summarize(&holding(Some(3), Some(latched(1)), Some(succeeded(2))))
            .expect("a success summary");
        assert_eq!(s.difficulty, 3);
        assert_eq!(s.total, 5, "total = difficulty + margin");
        assert!(s.outcome.contains("Succeeded by 2"), "{}", s.outcome);
    }

    #[test]
    fn summarizes_a_failure() {
        let s = summarize(&holding(
            Some(4),
            Some(latched(-1)),
            Some(failed(FailureReason::Total, 2)),
        ))
        .expect("a failure summary");
        assert_eq!(s.total, 2, "total = difficulty - by");
        assert!(s.outcome.contains("Failed by 2"), "{}", s.outcome);
    }

    #[test]
    fn summarizes_an_autofail() {
        let s = summarize(&holding(
            Some(3),
            Some((ChaosToken::AutoFail, TokenResolution::AutoFail)),
            Some(Event::SkillTestFailed {
                investigator: InvestigatorId(1),
                skill: SkillKind::Agility,
                reason: FailureReason::AutoFail,
                by: 3,
            }),
        ))
        .expect("an autofail summary");
        assert_eq!(s.total, 0, "auto-fail clamps total to 0");
        assert!(
            s.outcome.contains("auto-fail"),
            "notes the auto-fail: {}",
            s.outcome
        );
    }

    #[test]
    fn no_summary_without_a_held_resolution() {
        assert!(summarize(&holding(Some(3), Some(latched(1)), None)).is_none());
    }

    #[test]
    fn no_summary_without_known_difficulty() {
        assert!(summarize(&holding(None, Some(latched(1)), Some(succeeded(0)))).is_none());
    }

    /// The #787 case: a symbol token whose ST.4 effect suspends strands the
    /// reveal in an earlier batch. The latch is what names the token.
    #[test]
    fn names_a_token_revealed_in_an_earlier_batch() {
        let s = summarize(&holding(
            Some(3),
            Some((ChaosToken::Tablet, TokenResolution::Modifier(-2))),
            Some(failed(FailureReason::Total, 1)),
        ))
        .expect("a failure summary");
        assert_eq!(s.token, "Tablet (-2)", "names the latched token");
    }

    /// The #853 case, at this seam: the resolution came in a batch of its own —
    /// the one Lita's reaction window suspended on — and the panel renders it all
    /// the same, because it reads the latch rather than the live batch.
    #[test]
    fn summarizes_a_resolution_determined_in_an_earlier_batch() {
        let state = holding(Some(3), Some(latched(0)), Some(succeeded(0)));
        let s = summarize(&state).expect("a success summary");
        assert_eq!(s.total, 3);
        assert!(s.outcome.contains("Succeeded by 0"), "{}", s.outcome);
    }

    #[test]
    fn falls_back_to_the_em_dash_when_no_token_was_drawn() {
        let s = summarize(&holding(Some(3), None, Some(succeeded(1)))).expect("a success summary");
        assert_eq!(s.token, "—", "no token drawn at all keeps the fallback");
    }
}
