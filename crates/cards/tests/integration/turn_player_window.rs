//! The turn's player window reopens after each action (#146).
//!
//! Appendix II 2.2.1 (`data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`):
//! *"Active investigator may take an action, if able. If an action was taken,
//! return to previous player window. If no action was taken, proceed to
//! 2.2.2."* The previous player window is the one after 2.2, so after each
//! action every investigator holding an eligible fast play may answer it
//! before the active investigator's next action.
//!
//! ## Verified card text (`data/arkhamdb-snapshot/pack/core/`)
//!
//! - **Beat Cop 01018:** "You get +1 [combat].\n[fast] Discard Beat Cop: Deal 1
//!   damage to an enemy at your location." Ruling
//!   (<https://arkhamdb.com/card/01018>): the ability can't be used between
//!   assigning and applying damage to Beat Cop; not exercised here.
//! - **Ghoul Minion 01160:** no text; health 2, 1 damage and 1 horror. No
//!   rulings (`data/arkhamdb-faq/no-rulings.txt`).
//! - **Daisy Walker 01002:** health 5.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Responder;
use game_core::state::{
    ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId, LocationId,
};
use game_core::test_support::{self, seat, AnyKind, BoardLookup as _, Seat, TestSession};

/// Beat Cop 01018.
const BEAT_COP: &str = "01018";
/// Ghoul Minion 01160.
const GHOUL_MINION: &str = "01160";
/// Hallway 01112.
const HALLWAY_CODE: &str = "01112";

const ROLAND: InvestigatorId = InvestigatorId(1);
const DAISY: InvestigatorId = InvestigatorId(2);
const HALLWAY: LocationId = LocationId(101);
const GHOUL: EnemyId = EnemyId(100);

/// Roland Banks 01001 in the Hallway with Beat Cop in play.
fn roland_with_beat_cop() -> Seat<AnyKind> {
    seat(1)
        .as_card("01001")
        .at(HALLWAY)
        .in_play([BEAT_COP])
        .into()
}

/// Daisy Walker 01002 in the Hallway with nothing in hand or in play.
fn daisy() -> Seat<AnyKind> {
    seat(2).as_card("01002").at(HALLWAY).into()
}

/// `roland` and `daisy` seated `[ROLAND, DAISY]` in the Hallway, Daisy's turn
/// open, a chaos bag of one `0`, and a Ghoul Minion in the Hallway, engaged
/// with Daisy when `ghoul_engaged`.
fn board(roland: Seat<AnyKind>, daisy: Seat<AnyKind>, ghoul_engaged: bool) -> GameStateBuilder {
    let ghoul = test_support::enemy(GHOUL).as_card(GHOUL_MINION).at(HALLWAY);
    let ghoul = if ghoul_engaged {
        ghoul.engaged_with(DAISY)
    } else {
        ghoul
    };
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HALLWAY).as_card(HALLWAY_CODE))
        .with_seat(roland)
        .with_seat(daisy)
        .with_seating([ROLAND, DAISY])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_enemy(ghoul)
        .open_turn(DAISY)
}

/// Assert `session` rests at the turn's player window during `active`'s turn:
/// a passable prompt several investigators answer, outside any skill test.
#[track_caller]
fn assert_at_turn_window(session: &TestSession, active: InvestigatorId) {
    let prompt = session.prompt();
    assert!(
        prompt.responder == Responder::Several && prompt.skippable,
        "expected a player window; the session rests at {prompt:?}",
    );
    assert!(session.state().current_skill_test().is_none());
    assert_eq!(session.state().active_investigator, Some(active));
}

#[test]
fn after_a_fight_roland_finishes_the_ghoul_with_beat_cop_before_daisys_next_action() {
    let session = board(roland_with_beat_cop(), daisy(), true)
        .session()
        .take(&TurnAction::Fight {
            investigator: DAISY,
            enemy: GHOUL,
        })
        // The skill test's own windows: before the commit, the commit, and
        // before the token.
        .skip()
        .commit(&[])
        .skip();
    assert_at_turn_window(&session, DAISY);
    assert_eq!(session.state().enemies[&GHOUL].damage, 1);
    assert_eq!(session.offered_to(), vec![Some(ROLAND)]);

    let beat_cop = session.state().card(ROLAND, BEAT_COP);
    let session = session.pick_for(ROLAND, beat_cop);
    assert!(
        !session.state().enemies.contains_key(&GHOUL),
        "Beat Cop's 1 damage defeats the Ghoul Minion",
    );
    // Nothing is left to play, so the window closes by itself.
    session.assert_at_turn_menu(DAISY);
}
