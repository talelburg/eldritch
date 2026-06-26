//! Skill-test result panel (#478): renders the just-resolved test — chaos token
//! drawn, final total vs difficulty, pass/fail by N — from the events the store
//! retained ([`crate::store::ClientState::last_events`] +
//! [`last_skill_test_difficulty`](crate::store::ClientState::last_skill_test_difficulty)).
//! Pairs with the Confirm button rendered by `AwaitingInputView` (the
//! wasm-only `crate::input` module, so not linked here — the host `cargo doc`
//! build excludes it).

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

/// Build a [`SkillTestSummary`] from a resolution event batch and the test's
/// difficulty, or `None` if the batch carries no skill-test result or the
/// difficulty is unknown. Pure — no DOM, unit-tested on native.
///
/// `total` is reconstructed from the logged margin: `difficulty + margin` on a
/// success, `difficulty - by` on a failure (an `AutoFail` reports `by =
/// difficulty`, so the total clamps to 0).
#[must_use]
pub fn summarize(events: &[Event], difficulty: Option<i8>) -> Option<SkillTestSummary> {
    let difficulty = difficulty?;
    let token = events.iter().find_map(|e| match e {
        Event::ChaosTokenRevealed { token, resolution } => Some(token_display(*token, *resolution)),
        _ => None,
    });
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

/// Result panel for the just-resolved skill test. Renders nothing unless the
/// store's retained batch carries a skill-test result and a known difficulty
/// (i.e. exactly while the #478 acknowledge pause is live). Reads the store
/// reactively.
#[component]
pub fn SkillTestResultView() -> impl IntoView {
    let store = use_store();
    view! {
        {move || {
            let st = store.get();
            let Some(s) = summarize(&st.last_events, st.last_skill_test_difficulty) else {
                return ().into_any();
            };
            view! {
                <section class="skill-test-result">
                    <p class="str-token">"Chaos token: " {s.token}</p>
                    <p class="str-total">
                        "Total " {s.total} " vs difficulty " {s.difficulty}
                    </p>
                    <p class="str-outcome">{s.outcome}</p>
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
        let s = summarize(&events, Some(3)).expect("a success summary");
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
        let s = summarize(&events, Some(4)).expect("a failure summary");
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
        let s = summarize(&events, Some(3)).expect("an autofail summary");
        assert_eq!(s.total, 0, "auto-fail clamps total to 0");
        assert!(
            s.outcome.contains("auto-fail"),
            "notes the auto-fail: {}",
            s.outcome
        );
    }

    #[test]
    fn no_summary_without_resolution_events() {
        assert!(summarize(&[], Some(3)).is_none());
    }

    #[test]
    fn no_summary_without_known_difficulty() {
        let events = vec![Event::SkillTestSucceeded {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            margin: 0,
        }];
        assert!(summarize(&events, None).is_none());
    }
}
