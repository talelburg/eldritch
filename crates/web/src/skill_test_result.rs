//! Skill-test result panel (#478): renders the just-resolved test — chaos token
//! drawn, final total vs difficulty, pass/fail by N — from the events the store
//! retained ([`crate::store::ClientState::last_events`] plus the latched
//! [`last_skill_test_difficulty`](crate::store::ClientState::last_skill_test_difficulty)
//! and [`last_revealed_token`](crate::store::ClientState::last_revealed_token)).
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

/// Build a [`SkillTestSummary`] from a resolution event batch, the test's
/// difficulty and the token it drew, or `None` if the batch carries no
/// skill-test result or the difficulty is unknown. Pure — no DOM, unit-tested
/// on native.
///
/// `total` is reconstructed from the logged margin: `difficulty + margin` on a
/// success, `difficulty - by` on a failure (an `AutoFail` reports `by =
/// difficulty`, so the total clamps to 0).
///
/// Both `difficulty` and `token` come from the store's latches rather than from
/// `events`, because a resolution can span several apply batches: any ST.4
/// effect that suspends for input — The Gathering's Tablet dealing damage with a
/// soak target in play (#787) — leaves the reveal in the batch *before* the one
/// carrying the outcome. `None` for `token` means no token was drawn at all, and
/// renders the em dash.
#[must_use]
pub fn summarize(
    events: &[Event],
    difficulty: Option<i8>,
    token: Option<(ChaosToken, TokenResolution)>,
) -> Option<SkillTestSummary> {
    let difficulty = difficulty?;
    let token = token.map(|(token, resolution)| token_display(token, resolution));
    for e in events {
        match e {
            Event::SkillTestSucceeded { margin, .. } => {
                return Some(SkillTestSummary {
                    token: token.unwrap_or_else(|| "—".to_string()),
                    total: difficulty.saturating_add(*margin),
                    difficulty,
                    outcome: format!("Succeeded by {margin}"),
                });
            }
            Event::SkillTestFailed { reason, by, .. } => {
                let note = if matches!(reason, FailureReason::AutoFail) {
                    " (auto-fail)"
                } else {
                    ""
                };
                return Some(SkillTestSummary {
                    token: token.unwrap_or_else(|| "—".to_string()),
                    total: difficulty.saturating_sub(*by),
                    difficulty,
                    outcome: format!("Failed by {by}{note}"),
                });
            }
            _ => {}
        }
    }
    None
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

/// True iff the skill-test result modal is up: the store's retained batch carries
/// a result *and* the live prompt is the un-anchored `Confirm` acknowledge pause.
///
/// Public because the prompt banner needs it — the modal carries that pause's only
/// Confirm, so the banner must not render a second one (#541). The rule lives here,
/// with the view it governs, rather than being re-derived by the banner. Pure.
#[must_use]
pub fn modal_is_live(state: &crate::store::ClientState) -> bool {
    summarize(
        &state.last_events,
        state.last_skill_test_difficulty,
        state.last_revealed_token,
    )
    .is_some()
        && matches!(
            &state.outcome,
            Some(game_core::EngineOutcome::AwaitingInput { request, .. })
                if request.kind == game_core::InputKind::Confirm && request.target.is_none()
        )
}

/// Result modal for the just-resolved skill test (#478, made a modal by #541).
/// Renders nothing unless the store's retained batch carries a skill-test result
/// *and* the live prompt is the un-anchored `Confirm` acknowledge pause — that
/// pairing is what guarantees the modal always has a way out, since the modal
/// carries the pause's only Confirm.
///
/// **Dismissible only by that button.** Clicking Confirm submits real engine
/// input, so a backdrop click or an Escape key would advance the game on the
/// player's behalf; neither is wired, deliberately. The backdrop is inert —
/// it exists to stop the player scrolling past the token that decided the test.
#[component]
pub fn SkillTestResultView() -> impl IntoView {
    let store = use_store();
    view! {
        {move || {
            let st = store.get();
            if !modal_is_live(&st) {
                return ().into_any();
            }
            let Some(s) = summarize(
                &st.last_events,
                st.last_skill_test_difficulty,
                st.last_revealed_token,
            ) else {
                return ().into_any();
            };
            view! {
                // No `on:click` on the backdrop: dismissal is engine input.
                <div class="str-backdrop"></div>
                <section class="skill-test-result">
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
    use game_core::state::{ChaosToken, InvestigatorId, SkillKind, TokenResolution};
    use game_core::{Event, FailureReason};

    fn reveal(modifier: i8) -> Event {
        Event::ChaosTokenRevealed {
            token: ChaosToken::Numeric(modifier),
            resolution: TokenResolution::Modifier(modifier),
        }
    }

    /// The store's latch for a numeric token of `modifier`.
    fn latched(modifier: i8) -> (ChaosToken, TokenResolution) {
        (
            ChaosToken::Numeric(modifier),
            TokenResolution::Modifier(modifier),
        )
    }

    #[test]
    fn summarizes_a_success() {
        let events = vec![
            reveal(1),
            Event::SkillTestSucceeded {
                investigator: InvestigatorId(1),
                skill: SkillKind::Willpower,
                margin: 2,
            },
        ];
        let s = summarize(&events, Some(3), Some(latched(1))).expect("a success summary");
        assert_eq!(s.difficulty, 3);
        assert_eq!(s.total, 5, "total = difficulty + margin");
        assert!(s.outcome.contains("Succeeded by 2"), "{}", s.outcome);
    }

    #[test]
    fn summarizes_a_failure() {
        let events = vec![
            reveal(-1),
            Event::SkillTestFailed {
                investigator: InvestigatorId(1),
                skill: SkillKind::Combat,
                reason: FailureReason::Total,
                by: 2,
            },
        ];
        let s = summarize(&events, Some(4), Some(latched(-1))).expect("a failure summary");
        assert_eq!(s.total, 2, "total = difficulty - by");
        assert!(s.outcome.contains("Failed by 2"), "{}", s.outcome);
    }

    #[test]
    fn summarizes_an_autofail() {
        let events = vec![
            Event::ChaosTokenRevealed {
                token: ChaosToken::AutoFail,
                resolution: TokenResolution::AutoFail,
            },
            Event::SkillTestFailed {
                investigator: InvestigatorId(1),
                skill: SkillKind::Agility,
                reason: FailureReason::AutoFail,
                by: 3,
            },
        ];
        let s = summarize(
            &events,
            Some(3),
            Some((ChaosToken::AutoFail, TokenResolution::AutoFail)),
        )
        .expect("an autofail summary");
        assert_eq!(s.total, 0, "auto-fail clamps total to 0");
        assert!(
            s.outcome.contains("auto-fail"),
            "notes the auto-fail: {}",
            s.outcome
        );
    }

    #[test]
    fn no_summary_without_resolution_events() {
        assert!(summarize(&[], Some(3), Some(latched(1))).is_none());
    }

    #[test]
    fn no_summary_without_known_difficulty() {
        let events = vec![Event::SkillTestSucceeded {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            margin: 0,
        }];
        assert!(summarize(&events, None, Some(latched(1))).is_none());
    }

    /// The #787 case: a symbol token whose ST.4 effect suspends strands the
    /// reveal in an earlier batch, so the resolution batch carries only the
    /// outcome. The latch is what names the token.
    #[test]
    fn names_a_token_revealed_in_an_earlier_batch() {
        let events = vec![Event::SkillTestFailed {
            investigator: InvestigatorId(1),
            skill: SkillKind::Combat,
            reason: FailureReason::Total,
            by: 1,
        }];
        let s = summarize(
            &events,
            Some(3),
            Some((ChaosToken::Tablet, TokenResolution::Modifier(-2))),
        )
        .expect("a failure summary");
        assert_eq!(s.token, "Tablet (-2)", "names the latched token");
    }

    #[test]
    fn falls_back_to_the_em_dash_when_no_token_was_drawn() {
        let events = vec![Event::SkillTestSucceeded {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            margin: 1,
        }];
        let s = summarize(&events, Some(3), None).expect("a success summary");
        assert_eq!(s.token, "—", "no token drawn at all keeps the fallback");
    }
}
