//! The turn's player window reopens after each action.
//!
//! Appendix II 2.2.1 (`data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`):
//! *"Active investigator may take an action, if able. If an action was taken,
//! return to previous player window. If no action was taken, proceed to
//! 2.2.2."* The previous player window is the one after 2.2, so after each
//! action every investigator holding an eligible fast play may answer it
//! before the active investigator's next action. A fast play, a 0-action
//! ability, the act advance and End Turn are not actions, and open nothing.
//!
//! ## Verified card text (`data/arkhamdb-snapshot/pack/core/`)
//!
//! - **Beat Cop 01018:** "You get +1 [combat].\n[fast] Discard Beat Cop: Deal 1
//!   damage to an enemy at your location." Its ruling
//!   (<https://arkhamdb.com/card/01018>), that it can't be used between
//!   assigning and applying damage to it, is not exercised here.
//! - **Ghoul Minion 01160:** no text; health 2, 1 damage and 1 horror. No
//!   rulings (`data/arkhamdb-faq/no-rulings.txt`).
//! - **Daisy Walker 01002:** health 5. "You may take an additional action
//!   during your turn, which can only be used on [[Tome]] [action] abilities."
//!   Her ruling (<https://arkhamdb.com/card/01002>), on which actions a "lose
//!   actions" effect removes first, is not exercised here.
//! - **Leo De Luca 01048:** "You may take an additional action during your
//!   turn." Its rulings (<https://arkhamdb.com/card/01048>): the first action
//!   spent each turn takes up the additional designation, which
//!   `an_action_paid_with_an_additional_action_opens_the_window` relies on; the
//!   choice between two qualifying additional actions is not exercised here.
//! - **Old Book of Lore 01031:** "[action] Exhaust Old Book of Lore: Choose an
//!   investigator at your location. That investigator searches the top 3 cards
//!   of his or her deck for a card, draws it, and shuffles the remaining cards
//!   into his or her deck." Its ruling (<https://arkhamdb.com/card/01031>),
//!   that the search looks at the top 3 and draws the one you like, is the
//!   pick `take_searched` answers.
//! - **Magnifying Glass 01030:** "Fast.\nYou get +1 [intellect] while
//!   investigating." No rulings.

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId,
    LocationId, PhaseStep, Status,
};
use game_core::test_support::{
    self, act, seat, AnyKind, BoardLookup as _, Printed, Seat, TestSession,
};

/// Roland Banks 01001.
const ROLAND_BANKS: &str = "01001";
/// Daisy Walker 01002.
const DAISY_WALKER: &str = "01002";
/// Beat Cop 01018.
const BEAT_COP: &str = "01018";
/// Leo De Luca 01048.
const LEO_DE_LUCA: &str = "01048";
/// Old Book of Lore 01031.
const OLD_BOOK_OF_LORE: &str = "01031";
/// Magnifying Glass 01030.
const MAGNIFYING_GLASS: &str = "01030";
/// Emergency Cache 01088, filler for a deck to search.
const EMERGENCY_CACHE: &str = "01088";
/// Ghoul Minion 01160.
const GHOUL_MINION: &str = "01160";
/// Hallway 01112.
const HALLWAY_CODE: &str = "01112";

const ROLAND: InvestigatorId = InvestigatorId(1);
const DAISY: InvestigatorId = InvestigatorId(2);
const HALLWAY: LocationId = LocationId(101);
const GHOUL: EnemyId = EnemyId(100);

/// Roland Banks in the Hallway with nothing in hand or in play.
fn roland() -> Seat<Printed> {
    seat(1).as_card(ROLAND_BANKS).at(HALLWAY)
}

/// Roland Banks in the Hallway with Beat Cop in play.
fn roland_with_beat_cop() -> Seat<Printed> {
    roland().in_play([BEAT_COP])
}

/// Daisy Walker in the Hallway with nothing in hand or in play.
fn daisy() -> Seat<Printed> {
    seat(2).as_card(DAISY_WALKER).at(HALLWAY)
}

/// `roland` and `daisy` seated `[ROLAND, DAISY]` in the Hallway, Daisy's turn
/// open, a chaos bag of one `0`, and a Ghoul Minion in the Hallway, engaged
/// with Daisy when `ghoul_engaged`.
fn board(
    roland: impl Into<Seat<AnyKind>>,
    daisy: impl Into<Seat<AnyKind>>,
    ghoul_engaged: bool,
) -> GameStateBuilder {
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

/// `daisy` fights the engaged Ghoul Minion on a `0` and deals 1 damage, passing
/// the skill test's own windows (Roland's Beat Cop keeps them standing): before
/// the commit, the commit, and before the token.
fn daisy_fights_the_ghoul(daisy: Seat<Printed>) -> TestSession {
    board(roland_with_beat_cop(), daisy, true)
        .session()
        .take(&TurnAction::Fight {
            investigator: DAISY,
            enemy: GHOUL,
        })
        .skip()
        .commit(&[])
        .skip()
}

/// Take the `position`th card of the searched top 3, as Daisy.
///
/// Exempt from the anchor-pick rule, and sent raw: a deck search's options
/// are cards **in a deck**, with no board anchor and neither of ADR 0015's
/// natures, which that ADR knowingly excludes and defers. No session step can
/// name one, so the pick is positional (`old_book_of_lore.rs` does the same).
fn take_searched(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        investigator: DAISY,
        response: InputResponse::PickSingle(OptionId(position)),
    })
}

#[test]
fn after_a_fight_roland_finishes_the_ghoul_with_beat_cop_before_daisys_next_action() {
    let session = daisy_fights_the_ghoul(daisy());
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
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

#[test]
fn once_everyone_holding_a_play_passes_daisys_menu_is_back() {
    let session = daisy_fights_the_ghoul(daisy());
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);

    let session = session.skip_for(ROLAND);
    session.assert_at_turn_menu(DAISY);
    assert_eq!(session.state().enemies[&GHOUL].damage, 1);
    assert_eq!(session.state().investigators[&DAISY].actions_remaining, 2);
}

#[test]
fn a_pass_leaves_the_others_open_and_a_play_asks_the_passer_again() {
    let session = daisy_fights_the_ghoul(daisy().hand([MAGNIFYING_GLASS]).resources(5));
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    let glass = OptionTarget::HandCard {
        investigator: DAISY,
        hand_index: 0,
    };
    assert_eq!(session.offered_at(&glass), 1);

    // Roland's pass leaves Daisy's play open.
    let session = session.skip_for(ROLAND);
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(session.offered_to(), vec![Some(DAISY)]);

    // Her play asks Roland again.
    let session = session.pick_for(DAISY, glass);
    assert!(session.state().investigators[&DAISY]
        .cards_in_play
        .iter()
        .any(|card| card.code.as_str() == MAGNIFYING_GLASS));
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(session.offered_to(), vec![Some(ROLAND)]);
}

/// `glossary/Elimination.md` step 3: *"All enemies engaged with that player are
/// placed at the location the investigator was at when he or she was
/// eliminated, unengaged"*, so Roland can still reach the Ghoul with Beat Cop.
#[test]
fn when_an_attack_of_opportunity_defeats_daisy_the_window_still_opens_for_roland() {
    // 4 damage on health 5: the Ghoul Minion's 1 damage defeats her.
    let session = board(roland_with_beat_cop(), daisy().harm(4, 0), true)
        .session()
        .take(&TurnAction::Resource {
            investigator: DAISY,
        });
    assert_eq!(
        session.state().investigators[&DAISY].status,
        Status::Defeated
    );
    // The log announces her turn's end at the elimination, ahead of the window.
    assert_event!(session.events(), Event::TurnEnded { investigator } if *investigator == DAISY);
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(session.offered_to(), vec![Some(ROLAND)]);

    // Closing it runs the rotation (2.2.2), and Roland's turn begins (2.2): the
    // next prompt is the window after 2.2, on his turn.
    let session = session.skip();
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(session.state().active_investigator, Some(ROLAND));
}

#[test]
fn a_fast_play_or_a_zero_action_ability_from_the_menu_opens_no_window() {
    let session = board(
        roland_with_beat_cop(),
        daisy()
            .hand([MAGNIFYING_GLASS])
            .resources(5)
            .in_play([BEAT_COP]),
        false,
    )
    .session()
    .take(&TurnAction::PlayCard {
        investigator: DAISY,
        source: PlaySource::Hand(0),
    });
    session.assert_at_turn_menu(DAISY);
    assert!(session.state().investigators[&DAISY]
        .cards_in_play
        .iter()
        .any(|card| card.code.as_str() == MAGNIFYING_GLASS));

    let beat_cop = session.state().in_play(DAISY, BEAT_COP);
    let session = session.take(&TurnAction::ActivateAbility {
        investigator: DAISY,
        source: AbilitySource::InPlay(beat_cop),
        address: AbilityAddress::Printed(1),
    });
    session.assert_at_turn_menu(DAISY);
    assert_eq!(session.state().enemies[&GHOUL].damage, 1);
}

#[test]
fn an_action_paid_with_an_additional_action_opens_the_window() {
    let session = board(
        roland_with_beat_cop(),
        daisy().in_play([LEO_DE_LUCA]),
        false,
    )
    .session()
    .take(&TurnAction::Resource {
        investigator: DAISY,
    });
    // Leo De Luca's ruling (https://arkhamdb.com/card/01048): "When spending
    // actions during your turn, the first action you spend each turn takes up
    // the 'additional' designation." Daisy's own additional action is for Tome
    // abilities only, so the resource spends Leo's.
    assert_eq!(
        session.state().investigators[&DAISY].actions_remaining,
        3,
        "paid with Leo De Luca's additional action",
    );
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(session.offered_to(), vec![Some(ROLAND)]);
}

#[test]
fn an_action_ability_opens_the_window() {
    let session = board(
        roland_with_beat_cop(),
        daisy().in_play([OLD_BOOK_OF_LORE]).deck([
            EMERGENCY_CACHE,
            EMERGENCY_CACHE,
            EMERGENCY_CACHE,
        ]),
        false,
    )
    .session();
    let book = session.state().in_play(DAISY, OLD_BOOK_OF_LORE);
    // No investigator is asked for: Roland has no deck, so the search could not
    // change his state (`glossary/Target.md`: "A card is not an eligible target
    // for an ability if the resolution of that ability's effect could not
    // change the target's state."), and Daisy is chosen alone.
    let session = session
        .take(&TurnAction::ActivateAbility {
            investigator: DAISY,
            source: AbilitySource::InPlay(book),
            address: AbilityAddress::Printed(0),
        })
        .apply(take_searched(0));
    assert_eq!(
        session.state().investigators[&DAISY].hand.len(),
        1,
        "she drew one of the three",
    );
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(session.offered_to(), vec![Some(ROLAND)]);
}

#[test]
fn the_window_follows_the_third_action_too() {
    let resource = TurnAction::Resource {
        investigator: DAISY,
    };
    let session = board(roland_with_beat_cop(), daisy(), false)
        .session()
        .take(&resource)
        .skip_for(ROLAND)
        .take(&resource)
        .skip_for(ROLAND)
        .take(&resource);
    assert_eq!(session.state().investigators[&DAISY].actions_remaining, 0);
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(session.offered_to(), vec![Some(ROLAND)]);
}

#[test]
fn neither_the_act_advance_nor_end_turn_opens_the_window() {
    let session = board(roland_with_beat_cop(), daisy().clues(4), false)
        .with_act_deck([act("ACT-A", 2), act("ACT-B", 2)])
        .session()
        .take(&TurnAction::AdvanceAct {
            investigator: DAISY,
        });
    session.assert_at_turn_menu(DAISY);

    // The window that follows End Turn is Roland's, after 2.2 on his turn.
    let session = session.take(&TurnAction::EndTurn);
    assert_event!(session.events(), Event::TurnEnded { investigator } if *investigator == DAISY);
    session.assert_at_player_window(PhaseStep::TurnPlayerWindow);
    assert_eq!(session.state().active_investigator, Some(ROLAND));
}

#[test]
fn no_prompt_follows_an_action_when_nobody_holds_a_play() {
    let session = board(roland(), daisy(), false)
        .session()
        .take(&TurnAction::Resource {
            investigator: DAISY,
        });
    session.assert_at_turn_menu(DAISY);
    assert_eq!(session.state().investigators[&DAISY].actions_remaining, 2);
}
