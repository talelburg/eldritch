//! Integration: Flashlight 01087's `[action] Spend 1 supply:
//! Investigate. Your location gets -2 shroud for this investigation.`
//! end-to-end against the real `cards::REGISTRY`.
//!
//! Exercises the **Investigate** designator's shroud modifier: the -2 lowers
//! the location difficulty (clamped at 0), the test reuses the base
//! Investigate follow-up (so a success discovers a clue), and the
//! activation rejects before spending a supply when there is no revealed
//! location to investigate.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    GameState, GameStateBuilder, InvestigatorId, LocationId, Owner, SkillKind, UseKind,
};
use game_core::test_support::{self, seat, uses, SeatLookup as _, StagedLocation, TestSession};

const FLASHLIGHT: &str = "01087";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Board: Flashlight in play with 3 supplies; the active investigator at a
/// revealed `shroud`-shroud location holding 1 clue, with `intellect`
/// intellect. A `Numeric(0)` chaos bag makes the Investigate deterministic.
fn board(intellect: i8, shroud: u8, revealed: bool) -> GameStateBuilder {
    board_at(study(shroud, revealed), intellect)
}

/// The Study at `HERE`: `shroud` shroud, 1 clue, revealed or not.
fn study(shroud: u8, revealed: bool) -> StagedLocation {
    let study = test_support::location(HERE).shroud(shroud).clues(1);
    if revealed {
        study
    } else {
        study.unrevealed()
    }
}

/// [`board`] at `location`.
fn board_at(location: StagedLocation, intellect: i8) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Intellect, intellect)
                .in_play([(FLASHLIGHT, uses(UseKind::Supplies, 3))]),
        )
        .with_location(location)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
}

fn supplies(state: &GameState) -> Option<u8> {
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.code.as_str() == FLASHLIGHT)
        .map(|c| c.uses.get(&UseKind::Supplies).copied().unwrap_or(0))
}

/// Flashlight's `[action]` ability.
fn activation(state: &GameState) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, FLASHLIGHT)),
        address: AbilityAddress::Printed(0),
    }
}

/// Activate the Flashlight and commit nothing to its investigation; the
/// session comes back to the turn menu.
fn activate(board: GameStateBuilder) -> TestSession {
    let session = board.session();
    let activation = activation(session.state());
    let session = session.take(&activation).commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn minus_two_shroud_turns_a_failing_investigation_into_a_success() {
    // Intellect 2 + 0 = 2. Base difficulty (shroud 4) would fail; the -2
    // brings it to 2 → success by 0, discovering the location's clue.
    let r = activate(board(2, 4, true));
    assert_event!(
        r.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    assert_event!(r.events(), Event::CluePlaced { investigator, .. } if *investigator == INV);
    assert_eq!(r.state().investigators[&INV].clues, 1, "1 clue discovered");
    assert_eq!(
        r.state().locations[&HERE].clues,
        0,
        "clue left the location"
    );
    assert_eq!(supplies(r.state()), Some(2), "1 supply spent");
}

#[test]
fn reduced_shroud_clamps_at_zero() {
    // Shroud 1, -2 → difficulty (1 - 2).max(0) = 0, not -1 (which would
    // reject as a negative difficulty). Intellect 0 + 0 = 0 ≥ 0 → success.
    let r = activate(board(0, 1, true));
    assert_event!(
        r.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(supplies(r.state()), Some(2), "1 supply spent");
}

#[test]
fn rejects_without_a_revealed_location_before_spending_a_supply() {
    // Validate-first: an unrevealed location is not investigatable, so the
    // activation rejects before the supply cost is paid. The ability is not
    // offered (unrevealed location → not legal), so bypass the gate.
    let session = board(2, 4, false).session();
    let activation = activation(session.state());
    let r = session.submit_off_menu(&activation);
    let _ = r.expect_rejected();
    assert_eq!(
        supplies(r.state()),
        Some(3),
        "no supply spent on a rejected activation"
    );
}

// ---- composing with Obscuring Fog 01168 --------------------------

/// Obscuring Fog 01168: *"Attached location gets +2 shroud."* Attaching
/// it to the investigated location makes the shroud something two
/// different cards contribute to.
const FOG: &str = "01168";

/// The instance id of the Obscuring Fog [`fogged_board`] attaches. A seat
/// cannot place a location attachment, so the builder does not mint this one;
/// it is fixed here, and seated cards are minted past it.
const FOG_ID: CardInstanceId = CardInstanceId(7);

/// [`board`], with Obscuring Fog attached to the location.
fn fogged_board(intellect: i8, shroud: u8) -> GameStateBuilder {
    let location = study(shroud, true).attachments([CardInPlay::enter_play(
        CardCode::new(FOG),
        FOG_ID,
        Owner::EncounterDeck,
    )]);
    board_at(location, intellect)
}

/// The shroud is **one** modified quantity that both cards contribute
/// to, not two mechanisms applied at two different times:
/// printed 2, +2 from the fog, −2 from the flashlight, resolved against
/// a single difficulty of 2.
#[test]
fn fog_and_flashlight_compose_into_one_shroud() {
    let r = activate(fogged_board(2, 2));
    assert_event!(
        r.events(),
        Event::SkillTestStarted {
            skill: SkillKind::Intellect,
            difficulty: 2,
            ..
        }
    );
    // Intellect 2 + 0 = 2 against that difficulty → success by 0.
    assert_event!(
        r.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    assert!(
        r.state().recorded_modifiers.is_empty(),
        "the -2 shroud is scoped to the investigation and expires with it",
    );
}

/// The clamp is applied **once**, after every modifier — the ordering
/// `Modifiers.md` pins: *"after all active modifiers have been applied,
/// any resultant value below zero is treated as zero."*
///
/// Printed shroud 1, +2 fog, −2 flashlight = 1. Clamping the flashlight's
/// reduction on its own first would give (1 − 2 → 0) + 2 = 2, and the
/// investigation would fail by 1 instead of succeeding.
#[test]
fn the_clamp_applies_once_after_both_modifiers() {
    let r = activate(fogged_board(1, 1));
    assert_event!(
        r.events(),
        Event::SkillTestStarted {
            skill: SkillKind::Intellect,
            difficulty: 1,
            ..
        }
    );
    assert_event!(
        r.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
}
