use super::*;
use crate::state::{MythosPhaseFrame, MythosResume};

// Card actions can't use the enumeration form here: game-core's
// `TEST_INV`-only registry never enumerates a card-bearing PlayCard, so the
// menu never offers one. Instead we reach the handler with
// `submit_off_menu` (bypassing the enumeration gate) and assert it
// rejects — testing the handler's defensive validation, exactly as the
// pre-#447 typed-action `apply(PlayCard)` tests did. (The *positive*
// PlayCard enumeration + the registry-backed flow live in `enumerate.rs`
// and `crates/cards/tests/play_card.rs`.)
const CARD: &str = "01059";

fn play(hand_index: u8) -> TurnAction {
    TurnAction::PlayCard {
        investigator: INV,
        hand_index,
    }
}

#[test]
fn play_card_outside_investigation_phase_is_rejected() {
    // Mythos step 1.4: `INV` is to draw an encounter card.
    let session = GameStateBuilder::new()
        .with_seat(seat(1).hand([CARD]))
        .with_turn_order([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .session();
    session.assert_rejects(|s| s.submit_off_menu(&play(0)));
}

#[test]
fn play_card_by_non_active_investigator_is_rejected() {
    // Investigator 2's turn is open; investigator 1 tries to play.
    let session = GameStateBuilder::new()
        .with_seat(seat(1).hand([CARD]))
        .with_seat(seat(2))
        .with_turn_order([INV, InvestigatorId(2)])
        .open_turn(InvestigatorId(2))
        .session();
    session.assert_rejects(|s| s.submit_off_menu(&play(0)));
}

#[test]
fn play_card_by_defeated_investigator_is_rejected() {
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).hand([CARD]))
        .open_turn(INV)
        .build();
    // No seat setter for status: a defeated investigator whose turn is
    // still open is what the handler defends against.
    state.investigators.get_mut(&INV).expect("seated").status = Status::Defeated;
    TestSession::new(state).assert_rejects(|s| s.submit_off_menu(&play(0)));
}

#[test]
fn play_card_with_out_of_bounds_hand_index_is_rejected() {
    let session = GameStateBuilder::new()
        .with_seat(seat(1).hand([CARD]))
        .open_turn(INV)
        .session();
    session.assert_rejects(|s| s.submit_off_menu(&play(5)));
}

#[test]
fn play_card_with_empty_hand_is_rejected() {
    let session = GameStateBuilder::new()
        .with_seat(seat(1))
        .open_turn(INV)
        .session();
    session.assert_rejects(|s| s.submit_off_menu(&play(0)));
}
