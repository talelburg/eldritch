//! Stock fixture constructors for tests.
//!
//! Tests constantly need a "reasonable" investigator or location to
//! place into a state. These helpers produce one with default-y values;
//! callers tweak fields after construction when something specific is
//! needed.
//!
//! # `#[non_exhaustive]` interaction
//!
//! [`Investigator`] and [`Location`] are `#[non_exhaustive]`, so
//! downstream test crates cannot construct them via struct literal —
//! they MUST go through these fixtures. That's deliberate (it forces
//! a single source of test defaults) but it also means **adding a
//! field to those structs requires updating these fixtures in the
//! same PR**, otherwise the new field defaults to whatever
//! `test_investigator` / `test_location` set, which may not match the
//! field's intent. Phase-2+ reviewers: flag missing fixture updates
//! when a field addition lands.

use crate::card_data::{ClueValue, Prey};
use crate::engine::{ChoiceOption, EngineOutcome, InputRequest, OptionId, ResumeToken};
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, Investigator, InvestigatorId, Location,
    LocationId, Skills, Status,
};

/// A stock investigator with reasonable defaults.
///
/// - 3/3/3/3 skills; capacity and harm live on `investigator_card` (`TEST_INV`: 8/8 health/sanity).
/// - 5 starting resources, 0 clues.
/// - 3 actions remaining.
/// - Not placed at any location (`current_location: None`).
///
/// Mutate fields directly after construction to customize.
#[must_use]
pub fn test_investigator(id: u32) -> Investigator {
    let investigator_card = CardInPlay::enter_play(
        CardCode::new(crate::test_support::TEST_INV),
        CardInstanceId(u32::MAX - id),
    );
    Investigator {
        id: InvestigatorId(id),
        name: format!("Test Investigator {id}"),
        current_location: None,
        skills: Skills {
            willpower: 3,
            intellect: 3,
            combat: 3,
            agility: 3,
        },
        clues: 0,
        resources: 5,
        actions_remaining: 3,
        status: Status::Active,
        deck: Vec::new(),
        hand: Vec::new(),
        discard: Vec::new(),
        cards_in_play: Vec::new(),
        threat_area: Vec::new(),
        removed_from_game: Vec::new(),
        action_surcharge_spent_this_round: std::collections::BTreeSet::new(),
        investigator_card,
    }
}

/// A stock location with reasonable defaults.
///
/// - Shroud 2, 0 clues, revealed.
/// - No connections (caller adds them).
/// - `code` defaults to `CardCode("_test_loc_{id}")` — underscore-
///   prefixed so it can't collide with real `ArkhamDB` codes. Callers
///   that care about the code (encounter-spawn tests, etc.) should
///   mutate it directly after construction.
#[must_use]
pub fn test_location(id: u32, name: impl Into<String>) -> Location {
    Location {
        id: LocationId(id),
        code: CardCode(format!("_test_loc_{id}")),
        name: name.into(),
        shroud: 2,
        clues: 0,
        printed_clues: ClueValue::Fixed(0),
        revealed: true,
        connections: Vec::new(),
        attachments: Vec::new(),
    }
}

/// A stock enemy with reasonable defaults.
///
/// - Fight 2, Evade 2, max-health 2, no damage.
/// - Attack pattern: 1 damage / 0 horror.
/// - Not spawned (`current_location: None`), ready, unengaged, no
///   traits.
///
/// Mutate fields directly after construction to customize. The
/// `#[non_exhaustive]` interaction note from the module-level docs
/// applies to `Enemy` as well — adding a field to the struct requires
/// updating this fixture in the same PR.
#[must_use]
pub fn test_enemy(id: u32, name: impl Into<String>) -> Enemy {
    Enemy {
        id: EnemyId(id),
        name: name.into(),
        code: CardCode::new(format!("_test_enemy_{id}")),
        fight: 2,
        evade: 2,
        max_health: 2,
        damage: 0,
        attack_damage: 1,
        attack_horror: 0,
        current_location: None,
        exhausted: false,
        traits: Vec::new(),
        engaged_with: None,
        hunter: false,
        prey: Prey::Default,
        retaliate: false,
        victory: None,
    }
}

/// A sample skill-test commit [`AwaitingInput`](EngineOutcome::AwaitingInput)
/// outcome, for client/UI fixtures. This is the only `AwaitingInput`
/// shape the engine emits today (the skill-test commit window). The
/// `ResumeToken` value is irrelevant to rendering — routing keys off
/// `state.in_flight_skill_test`, not the token.
#[must_use]
pub fn awaiting_commit_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::prompt(prompt),
        resume_token: ResumeToken(0),
    }
}

/// A sample structured [`PickSingle`](crate::InputResponse::PickSingle)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome, for client/UI
/// fixtures (#447). Carries two options:
///
/// - `OptionId(0)` → `"End turn"`
/// - `OptionId(1)` → `"Investigate"`
///
/// The `ResumeToken` value is irrelevant to rendering.
#[must_use]
pub fn awaiting_pick_single_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(
            prompt,
            vec![
                ChoiceOption {
                    id: OptionId(0),
                    label: "End turn".into(),
                },
                ChoiceOption {
                    id: OptionId(1),
                    label: "Investigate".into(),
                },
            ],
        ),
        resume_token: ResumeToken(0),
    }
}

#[cfg(test)]
mod tests {
    use super::awaiting_commit_input;
    use crate::EngineOutcome;

    #[test]
    fn awaiting_commit_input_carries_the_prompt() {
        let outcome = awaiting_commit_input("Commit cards for the test");
        match outcome {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.prompt, "Commit cards for the test");
            }
            other => panic!("expected AwaitingInput, got {other:?}"),
        }
    }
}
