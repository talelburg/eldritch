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
use crate::dsl::SkillTestKind;
use crate::engine::{ChoiceOption, EngineOutcome, InputRequest, OptionId, ResumeToken};
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, Continuation, Enemy, EnemyId, GameState, HandSizeDiscard,
    InFlightSkillTest, Investigator, InvestigatorId, Location, LocationId, SkillKind,
    SkillTestFollowUp, SkillTestId, SkillTestStep, Skills, Status,
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
        setaside: Vec::new(),
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
        cards_at_location: Vec::new(),
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
        attachments: Vec::new(),
    }
}

/// A stock [`InFlightSkillTest`] parked at its commit window.
///
/// Neutral in every direction the caller didn't name: nothing committed, no
/// tested location, no follow-up, no success/failure effect, no accumulated
/// bonuses, unresolved. Callers override what they care about with functional
/// update syntax:
///
/// ```ignore
/// InFlightSkillTest {
///     follow_up: SkillTestFollowUp::Investigate,
///     bonus_clues_discovered: 1,
///     ..test_skill_test(SkillTestId(0), inv, SkillKind::Intellect, SkillTestKind::Investigate, 2)
/// }
/// ```
///
/// The `#[non_exhaustive]` note at the top of this module applies: a new
/// field on `InFlightSkillTest` is defaulted here, so it must be reviewed
/// here too.
#[must_use]
pub fn test_skill_test(
    id: SkillTestId,
    investigator: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
    difficulty: i8,
) -> InFlightSkillTest {
    InFlightSkillTest {
        id,
        investigator,
        skill,
        kind,
        // A printed difficulty, the basis with no board quantity behind it
        // — a fixture that wants a location's shroud or an enemy's fight
        // sets `difficulty_basis` itself with functional update syntax.
        difficulty_basis: crate::state::DifficultyBasis::Fixed(difficulty),
        committed_by_active: Vec::new(),
        tested_location: None,
        follow_up: SkillTestFollowUp::None,
        on_fail: None,
        on_success: None,
        source: None,
        continuation: SkillTestStep::AwaitingCommit,
        bonus_attack_damage: 0,
        bonus_clues_discovered: 0,
        resolved: None,
        symbol_on_fail: None,
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
        request: InputRequest::pick_multiple(prompt),
        resume_token: ResumeToken(0),
    }
}

/// A skippable [`PickMultiple`](crate::InputKind::PickMultiple)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome — for UI tests of the
/// Pass/Skip control on a multi-select prompt.
#[must_use]
pub fn awaiting_skippable_commit_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_multiple(prompt).skippable(),
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
        request: InputRequest::pick_single(
            prompt,
            vec![
                ChoiceOption::new(OptionId(0), "End turn"),
                ChoiceOption::new(OptionId(1), "Investigate"),
            ],
        ),
        resume_token: ResumeToken(0),
    }
}

/// An [`AwaitingInput`](EngineOutcome::AwaitingInput) `PickSingle` outcome over
/// caller-supplied `options` — for host/UI tests that need a specific
/// [`OptionTarget`](crate::OptionTarget) anchor (the no-arg
/// [`awaiting_pick_single_input`] fixture is un-anchored only). `ResumeToken(0)`
/// matches the other fixtures (the UI never inspects it).
#[must_use]
pub fn awaiting_pick_single_with(
    prompt: impl Into<String>,
    options: Vec<ChoiceOption>,
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, options),
        resume_token: ResumeToken(0),
    }
}

/// A sample [`Confirm`](crate::InputKind::Confirm)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome, for client/UI
/// fixtures. Models the Mythos encounter-draw prompt.
#[must_use]
pub fn awaiting_confirm_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::confirm(prompt),
        resume_token: ResumeToken(0),
    }
}

/// A sample skippable [`PickSingle`](crate::InputKind::PickSingle)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome, for client/UI
/// fixtures. Models a non-forced reaction window: one option plus a Skip
/// affordance.
#[must_use]
pub fn awaiting_skippable_pick_single_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, vec![ChoiceOption::new(OptionId(0), "Resolve")])
            .skippable(),
        resume_token: ResumeToken(0),
    }
}

/// A skippable [`PickSingle`](crate::InputKind::PickSingle)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) over caller-supplied `options`
/// (so a test can mix anchored and un-anchored options). Like
/// [`awaiting_pick_single_with`] but with the Skip affordance a window carries.
#[must_use]
pub fn awaiting_skippable_pick_single_with(
    prompt: impl Into<String>,
    options: Vec<ChoiceOption>,
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, options).skippable(),
        resume_token: ResumeToken(0),
    }
}

/// Put `state` at the upkeep hand-size discard prompt by pushing a
/// [`Continuation::HandSizeDiscard`] frame for `remaining`. A fixture because
/// [`HandSizeDiscard`] is `#[non_exhaustive]`, so downstream test crates
/// (the web client's wasm tests, #468) can't build the frame directly.
#[must_use]
pub fn at_hand_size_discard(mut state: GameState, remaining: Vec<InvestigatorId>) -> GameState {
    state
        .continuations
        .push(Continuation::HandSizeDiscard(HandSizeDiscard { remaining }));
    state
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
