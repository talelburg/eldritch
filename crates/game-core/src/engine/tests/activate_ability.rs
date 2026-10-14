use super::*;
use crate::state::{MythosPhaseFrame, MythosResume};
use crate::test_support::SeatLookup as _;
use crate::test_support::TEST_REGISTRIES;

// As with PlayCard (`play_card.rs`), the non-enumeration form is vacuous
// here (`TEST_INV`-only registry yields no abilities for "01059", so the
// menu never offers the activation). Reach the handler with
// `submit_off_menu` and assert it rejects — the handler's defensive
// validation, exactly as the pre-#447 typed-action tests did. The
// registry-backed activation flow lives in
// crates/game-core/tests/activate_ability.rs.
const ASSET: &str = "01059";

fn activate(source: CardInstanceId) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(source),
        address: AbilityAddress::Printed(0),
    }
}

#[test]
fn activate_ability_outside_investigation_phase_is_rejected() {
    // Mythos step 1.4: `INV` is to draw an encounter card.
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).in_play([ASSET]))
        .with_turn_order([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .session();
    let asset = session.state().in_play(INV, ASSET);
    session.assert_rejects(|s| s.submit_off_menu(&activate(asset)));
}

#[test]
fn activate_ability_by_non_active_investigator_is_rejected() {
    // Investigator 2's turn is open; investigator 1 tries to activate.
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).in_play([ASSET]))
        .with_seat(seat(2))
        .with_turn_order([INV, InvestigatorId(2)])
        .open_turn(InvestigatorId(2))
        .session();
    let asset = session.state().in_play(INV, ASSET);
    session.assert_rejects(|s| s.submit_off_menu(&activate(asset)));
}

#[test]
fn activate_ability_with_unknown_instance_id_is_rejected() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).in_play([ASSET]))
        .open_turn(INV)
        .session();
    // An id nothing on the board carries.
    session.assert_rejects(|s| s.submit_off_menu(&activate(CardInstanceId(9999))));
}

#[test]
fn activate_ability_when_defeated_is_rejected() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).in_play([ASSET]))
        .open_turn(INV)
        .build();
    // No seat setter for status: a defeated investigator whose turn is
    // still open is what the handler defends against.
    state.investigators.get_mut(&INV).expect("seated").status = Status::Defeated;
    let asset = state.in_play(INV, ASSET);
    TestSession::new(state, TEST_REGISTRIES)
        .assert_rejects(|s| s.submit_off_menu(&activate(asset)));
}
