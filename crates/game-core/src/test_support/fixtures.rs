//! Stock fixture constructors behind the stagers, and sample prompts.
//!
//! A test stages its board with [`seat`](super::seat), [`enemy`](super::enemy)
//! and [`location`](super::location); the stock values they start from live
//! here. A printed code reaches a fixture only through `as_card` (ADR 0016).
//! The stock enemy and location are crate-private, so the stagers are the door.
//!
//! # `#[non_exhaustive]` interaction
//!
//! [`Investigator`], [`Enemy`] and [`Location`] are `#[non_exhaustive]`, so
//! downstream test crates cannot construct them via struct literal — they MUST
//! go through the stagers. That's deliberate (it forces a single source of
//! test defaults) but it also means **adding a field to those structs requires
//! updating these fixtures in the same PR**, otherwise the new field defaults
//! to whatever `test_investigator` / `test_enemy` / `test_location` set, which
//! may not match the field's intent. Reviewers: flag a missing fixture update
//! when a field addition lands.

use std::collections::BTreeSet;

use card_dsl::card_data::{CardKind, CardMetadata, Class, ClueValue, HealthValue, Prey};
use card_dsl::dsl::SkillTestKind;

use crate::engine::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, PickCount, Responder,
    ResumeToken,
};
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, Continuation, ContinuationStack, DifficultyBasis, Enemy,
    EnemyId, InFlightSkillTest, Investigator, InvestigatorId, Location, LocationId, Owner,
    SkillKind, SkillTestFollowUp, SkillTestId, SkillTestStep, Skills, Status,
};
use crate::test_support;

/// A stock investigator with reasonable defaults.
///
/// - 3/3/3/3 skills; capacity and harm live on `investigator_card` (`TEST_INV`: 8/8 health/sanity).
/// - 5 starting resources, 0 clues.
/// - 3 actions remaining.
/// - Not placed at any location (`current_location: None`).
///
/// To seat one with a printed card, zones, or cards in play, use
/// [`seat`](crate::test_support::seat), which starts from this.
#[must_use]
pub fn test_investigator(id: u32) -> Investigator {
    let investigator_card = CardInPlay::enter_play(
        CardCode::new(test_support::TEST_INV),
        CardInstanceId(u32::MAX - id),
        Owner::Investigator(InvestigatorId(id)),
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
        action_surcharge_spent_this_round: BTreeSet::new(),
        additional_actions_this_turn: Vec::new(),
        grants_spent_this_turn: BTreeSet::new(),
        investigator_card,
    }
}

/// Metadata for a synthetic card `code` named `name`, of `kind`: no traits, no
/// text, no back, not a weakness.
///
/// For an integration binary that registers its own synthetic cards on a
/// [`MockRegistry`](crate::test_support::MockRegistry). It builds metadata,
/// not a probe card: the abilities a probe exists for stay in the test file
/// that reads them.
#[must_use]
pub fn test_card_metadata(code: &str, name: &str, kind: CardKind) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: name.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: None,
        kind,
    }
}

/// Metadata for a stock investigator card printed as `code`: 3 in every
/// skill, 8 health, 8 sanity, Neutral, no traits, named after its code.
///
/// For an integration binary that registers its own synthetic investigators
/// on a [`MockRegistry`](crate::test_support::MockRegistry), each modelling
/// one ability shape on otherwise identical printed stats.
#[must_use]
pub fn test_investigator_card(code: &str) -> CardMetadata {
    test_card_metadata(
        code,
        code,
        CardKind::Investigator {
            class: Class::Neutral,
            skills: Skills {
                willpower: 3,
                intellect: 3,
                combat: 3,
                agility: 3,
            },
            health: 8,
            sanity: 8,
        },
    )
}

/// The stock location [`location`](super::location) starts from.
///
/// - Shroud 2, a printed clue value of 0, 0 clues, revealed.
/// - No connections, nothing attached and no cards at it.
/// - `code` is `CardCode("_test_loc_{id}")` — underscore-prefixed so it
///   can't collide with real `ArkhamDB` codes.
#[must_use]
pub(crate) fn test_location(id: u32, name: impl Into<String>) -> Location {
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

/// The stock enemy [`enemy`](super::enemy) starts from.
///
/// - Fight 2, Evade 2, max-health 2, no damage.
/// - Attack pattern: 1 damage / 0 horror.
/// - Not spawned (`current_location: None`), ready, unengaged.
/// - A synthetic code (`_test_enemy_{id}`) with no printed card behind it, so
///   it has no traits.
/// - Owned by the **encounter deck** ([`Owner::EncounterDeck`]) — the owner of
///   an enemy drawn from the encounter deck, which is where every enemy but a
///   weakness comes from.
///
/// It is built by `Enemy::from_metadata`, the constructor a spawn uses, so a
/// field added there reaches this fixture too.
///
/// # Panics
///
/// Never: the metadata it builds is an enemy card's.
#[must_use]
pub(crate) fn test_enemy(id: u32, name: impl Into<String>) -> Enemy {
    let code = format!("_test_enemy_{id}");
    let metadata = CardMetadata {
        code: code.clone(),
        name: name.into(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: None,
        kind: CardKind::Enemy {
            fight: 2,
            evade: 2,
            damage: 1,
            horror: 0,
            health: Some(HealthValue::Fixed(2)),
            victory: None,
            spawn: None,
            surge: false,
            peril: false,
            hunter: false,
            retaliate: false,
            prey: Prey::Default,
            quantity: 1,
        },
    };
    // A fixed health ignores the investigator count.
    Enemy::from_metadata(EnemyId(id), &metadata, None, Owner::EncounterDeck, 1)
        .expect("the metadata is an enemy's")
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
        difficulty_basis: DifficultyBasis::Fixed(difficulty),
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
        revealed: None,
        reveal_clause: None,
        discard_after: None,
    }
}

/// A continuation stack built from raw frames, bottom first, **without**
/// checking any of the stack's invariants.
///
/// **Render-only.** A state built with it is for reading — a view, a decision
/// label, an eligibility condition — and is never applied. The stack may be one
/// the engine could never produce. To drive a state with a frame on top, stage
/// it with [`GameStateBuilder::with_frame`](crate::state::GameStateBuilder::with_frame),
/// which pushes it with the stack's checks, and settle it into a
/// [`TestSession`](crate::test_support::TestSession).
///
/// To stack a frame on a state the builder already staged, rebuild from its
/// frames: `from_frames_unchecked(state.continuations.iter().cloned().chain([frame]))`.
#[must_use]
pub fn from_frames_unchecked(frames: impl IntoIterator<Item = Continuation>) -> ContinuationStack {
    ContinuationStack::from_frames_unchecked(frames.into_iter().collect())
}

/// Who answers the `AwaitingInput` fixtures below, unless a `_for` variant
/// names someone else: seat 1.
const SEAT_ONE: Responder = Responder::Investigator(InvestigatorId(1));

/// A sample skill-test commit [`AwaitingInput`](EngineOutcome::AwaitingInput)
/// outcome, for client/UI fixtures.
///
/// Seat 1 picks any number of their hand cards at `hand_indices`, option `i`
/// anchored to `hand_indices[i]`. The `ResumeToken` value is irrelevant to
/// rendering.
#[must_use]
pub fn awaiting_commit_input(prompt: impl Into<String>, hand_indices: &[u8]) -> EngineOutcome {
    awaiting_commit_input_for(prompt, InvestigatorId(1), hand_indices)
}

/// [`awaiting_commit_input`] answered by `investigator`, from their hand.
#[must_use]
pub fn awaiting_commit_input_for(
    prompt: impl Into<String>,
    investigator: InvestigatorId,
    hand_indices: &[u8],
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: hand_pick(prompt, investigator, hand_indices),
        resume_token: ResumeToken(0),
    }
}

/// A skippable [`PickMultiple`](crate::engine::InputKind::PickMultiple)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome — for UI tests of the
/// Pass/Skip control on a multi-select prompt.
///
/// Offers seat 1's hand cards at `hand_indices`, as [`awaiting_commit_input`]
/// does.
#[must_use]
pub fn awaiting_skippable_commit_input(
    prompt: impl Into<String>,
    hand_indices: &[u8],
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: hand_pick(prompt, InvestigatorId(1), hand_indices).skippable(),
        resume_token: ResumeToken(0),
    }
}

/// A `PickMultiple` answered by `investigator` over their hand cards at
/// `hand_indices`, any number of them.
fn hand_pick(
    prompt: impl Into<String>,
    investigator: InvestigatorId,
    hand_indices: &[u8],
) -> InputRequest {
    let options = hand_indices
        .iter()
        .enumerate()
        .map(|(i, &hand_index)| {
            ChoiceOption::new(
                OptionId(u32::try_from(i).expect("a hand fits in u32")),
                format!("hand card {hand_index}"),
            )
            .at(OptionTarget::HandCard {
                investigator,
                hand_index,
            })
        })
        .collect();
    InputRequest::pick_multiple(prompt, options, PickCount::up_to(hand_indices.len()))
        .answered_by(Responder::Investigator(investigator))
}

/// An [`AwaitingInput`](EngineOutcome::AwaitingInput) around an arbitrary
/// `request`, for a UI fixture that needs a prompt shape the named helpers do
/// not build — an anchored decision, say.
///
/// [`ResumeToken`]'s field is `pub(crate)`, so a host crate cannot wrap its own
/// request without this.
#[must_use]
pub fn awaiting_request(request: InputRequest) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request,
        resume_token: ResumeToken(0),
    }
}

/// A sample structured [`PickSingle`](crate::action::InputResponse::PickSingle)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome, for client/UI
/// fixtures. Carries two options:
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
        )
        .answered_by(SEAT_ONE),
        resume_token: ResumeToken(0),
    }
}

/// An [`AwaitingInput`](EngineOutcome::AwaitingInput) `PickSingle` outcome over
/// caller-supplied `options`.
///
/// For host/UI tests that need a specific
/// [`OptionTarget`](crate::engine::OptionTarget) anchor (the no-arg
/// [`awaiting_pick_single_input`] fixture is un-anchored only). `ResumeToken(0)`
/// matches the other fixtures (the UI never inspects it).
#[must_use]
pub fn awaiting_pick_single_with(
    prompt: impl Into<String>,
    options: Vec<ChoiceOption>,
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, options).answered_by(SEAT_ONE),
        resume_token: ResumeToken(0),
    }
}

/// A sample [`Confirm`](crate::engine::InputKind::Confirm)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome, for client/UI
/// fixtures. Models the Mythos encounter-draw prompt, asked of seat 1.
#[must_use]
pub fn awaiting_confirm_input(prompt: impl Into<String>) -> EngineOutcome {
    awaiting_confirm_input_for(prompt, InvestigatorId(1))
}

/// [`awaiting_confirm_input`] asked of `investigator`, for a UI test of who
/// the game is waiting on.
#[must_use]
pub fn awaiting_confirm_input_for(
    prompt: impl Into<String>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::confirm(prompt).answered_by(Responder::Investigator(investigator)),
        resume_token: ResumeToken(0),
    }
}

/// A sample skippable [`PickSingle`](crate::engine::InputKind::PickSingle)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome, for client/UI
/// fixtures.
///
/// Models a non-forced reaction window: one option plus a Skip affordance.
#[must_use]
pub fn awaiting_skippable_pick_single_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, vec![ChoiceOption::new(OptionId(0), "Resolve")])
            .skippable()
            .answered_by(SEAT_ONE),
        resume_token: ResumeToken(0),
    }
}

/// A skippable [`PickSingle`](crate::engine::InputKind::PickSingle)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) over caller-supplied
/// `options` (so a test can mix anchored and un-anchored options).
///
/// Like [`awaiting_pick_single_with`] but with the Skip affordance a window
/// carries.
#[must_use]
pub fn awaiting_skippable_pick_single_with(
    prompt: impl Into<String>,
    options: Vec<ChoiceOption>,
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, options)
            .skippable()
            .answered_by(SEAT_ONE),
        resume_token: ResumeToken(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awaiting_commit_input_carries_the_prompt() {
        let outcome = awaiting_commit_input("Commit cards for the test", &[0]);
        match outcome {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.prompt, "Commit cards for the test");
            }
            other => panic!("expected AwaitingInput, got {other:?}"),
        }
    }
}
