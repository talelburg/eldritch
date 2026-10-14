//! C1a end-to-end: the-gathering `setup()` seats a roster at the Study and
//! reaches a resolution; the Attic/Cellar forced-on-enter abilities fire
//! through the real card registry, named as `scenarios::TEST_REGISTRIES` (the
//! real `cards` corpus with the synthetic test cards, plus this crate's
//! scenario modules).

use std::collections::BTreeSet;

use game_core::action::RosterEntry;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{self, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, DiscardPile, GameState, GameStateBuilder, InvestigatorId, LocationId,
    Owner, Phase, Zone,
};
use game_core::test_support;
use game_core::test_support::{seat, TestSession};
use scenarios::the_gathering;

const INV: InvestigatorId = InvestigatorId(1);

/// Seat solo Roland (01001, empty deck) and keep his opening hand at the
/// mulligan, leaving him at his first turn menu.
fn setup_and_seat() -> GameState {
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: vec![],
    }];
    // seat_and_open rests at the mulligan; the session settles there.
    let session = TestSession::new(
        engine::seat_and_open(the_gathering::setup(), &roster, scenarios::TEST_REGISTRIES).state,
        scenarios::TEST_REGISTRIES,
    )
    .choose_from_hand(&[]);
    session.assert_at_turn_menu(INV);
    session.finish().state
}

#[test]
fn roster_seating_places_investigator_at_study() {
    let state = setup_and_seat();
    let roland = state.investigators.get(&INV).expect("Roland seated");
    let study = state.starting_location;
    assert_eq!(
        roland.current_location, study,
        "seating must place investigators at setup()'s starting_location",
    );
    let study_loc = &state.locations[&state.starting_location.unwrap()];
    assert!(study_loc.revealed, "seating reveals the starting location");
    assert_eq!(study_loc.clues, 2, "1 investigator × 2 per-investigator");
}

/// The set-aside zone as printed codes, in order.
fn set_aside_codes(state: &GameState) -> Vec<String> {
    state
        .set_aside_cards
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect()
}

/// Act 2's reverse leaves the Parlor (01115) revealed with Lita Chantler
/// (01117) in play *at* it under nobody's control — lines 1 and 2 of the three
/// it prints (#772).
fn assert_parlor_revealed_with_lita(state: &GameState) {
    let parlor = state
        .locations
        .values()
        .find(|l| l.code.as_str() == "01115")
        .expect("Parlor (01115) in play");
    assert!(parlor.revealed, "act 2's reverse reveals the Parlor");
    assert_eq!(
        parlor
            .cards_at_location
            .iter()
            .map(|c| c.code.as_str())
            .collect::<Vec<_>>(),
        ["01117"],
        "Lita is put into play in the Parlor, under nobody's control",
    );
}

#[test]
fn drives_act_1_then_act_2_via_round_end_window() {
    let inv = INV;
    let mut state = setup_and_seat();

    // Enough clues to clear act 1 (threshold 2, AdvanceAct) and then act 2
    // (threshold 3) via its round-end objective; a small deck so the upkeep
    // draw doesn't deck out. Act 3 (01110) advances on Ghoul-Priest-defeat —
    // covered by cards/tests/act_advancement.rs — so this test stops at
    // reaching act 3 through the real deck.
    {
        let i = state.investigators.get_mut(&inv).unwrap();
        i.clues = 7;
        i.deck = (0..5).map(|n| CardCode(format!("filler{n}"))).collect();
    }

    // Act 1: the normal Investigation-phase clue spend. Board builds and the
    // investigator relocates to the Hallway (01112) — the act-2 contributors.
    let session = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .take(&TurnAction::AdvanceAct { investigator: inv });
    session.assert_at_turn_menu(inv);
    let state = session.state();
    assert_eq!(state.act_index, 1, "act 1 advanced to act 2");

    // #774: the board act 1 builds puts the Parlor (01115) into play
    // **unrevealed**, and its unrevealed back reads *"The entrance to the
    // Parlor is blocked by a darkly glowing unfathomable barrier. You cannot
    // move into the Parlor."* — so the Hallway investigator is not offered it,
    // while the Attic and Cellar off the same Hallway stay reachable. Without
    // the barrier an investigator could walk in from round 1 and skip act 2's
    // gate entirely, which is exactly what this act-2 advance is.
    let parlor_id = location_id(state, "01115");
    assert!(
        !state.locations[&parlor_id].revealed,
        "the Parlor enters play unrevealed",
    );
    let offered = enumerate::legal_actions(state, scenarios::TEST_REGISTRIES.cards);
    assert!(
        !offered.contains(&TurnAction::Move {
            investigator: inv,
            destination: parlor_id,
        }),
        "the Parlor's barrier is up before act 2 advances; offered {offered:?}",
    );
    for code in ["01113", "01114"] {
        let id = location_id(state, code);
        assert!(
            offered.contains(&TurnAction::Move {
                investigator: inv,
                destination: id,
            }),
            "{code} is an ordinary Hallway neighbour and stays reachable; \
             offered {offered:?}",
        );
    }

    // End the round: the cascade reaches step 4.6 and opens act 2's round-end
    // window (Hallway investigator holds >= 3 clues). The window is the round
    // end's `when`: it sits in round 1's Upkeep, offering the act's clue spend
    // as a reaction the player may pass.
    let round_end = session.take(&TurnAction::EndTurn);
    assert_eq!(
        round_end.state().round,
        1,
        "the window sits on round 1's end"
    );
    assert_eq!(
        round_end.state().phase,
        Phase::Upkeep,
        "the round ends at the close of Upkeep"
    );
    assert_eq!(
        round_end.offered_at(&OptionTarget::Act),
        1,
        "round end opens the act-2 clue-spend window, got {:?}",
        round_end.prompt(),
    );
    assert!(round_end.prompt().skippable, "a reaction window");

    // Pick the act-advance candidate (the window's sole option): act 2 advances
    // to act 3 via 01109's `When`-RoundEnded group clue-spend (#434). A
    // rejected pick would leave the act where it was.
    let after_spend = round_end.pick(OptionTarget::Act);
    let r = after_spend.state();
    assert_eq!(r.act_index, 2, "act 2 advanced to act 3 (01110)");
    assert_eq!(
        r.act_deck.len(),
        3,
        "act 3 (01110) is the last act, which is what makes it terminal — its \
         resolution point is an effect on its reverse (ADR 0013), reached when \
         the Ghoul-Priest defeat advances it",
    );

    // Act 2's reverse (#280) fired on advance: all three of its printed lines
    // ran, so the Parlor (01115) is revealed, Lita Chantler (01117) is in play
    // *at* the Parlor under nobody's control (#772), and the set-aside Ghoul
    // Priest (01116) is in the Hallway (01112) — which makes act 3's "Ghoul
    // Priest defeated" objective reachable in real play.
    assert!(
        set_aside_codes(r).is_empty(),
        "the reverse emptied the set-aside zone, got {:?}",
        set_aside_codes(r),
    );
    let priest = r
        .enemies
        .values()
        .find(|e| e.code.as_str() == "01116")
        .expect("Ghoul Priest (01116) spawned");
    let hallway_id = r
        .locations
        .values()
        .find(|l| l.code.as_str() == "01112")
        .unwrap()
        .id;
    assert_eq!(
        priest.current_location,
        Some(hallway_id),
        "the Ghoul Priest spawns in the Hallway",
    );
    assert_parlor_revealed_with_lita(r);

    // …and revealing it is what lifts the barrier (#774), which is the whole
    // point of 01109b's *"The barrier blocking passage into the parlor has
    // vanished. Reveal the Parlor."* The Hallway investigator can now move in.
    assert!(
        engine::investigator_can_enter_location(
            r,
            scenarios::TEST_REGISTRIES.cards,
            location_id(r, "01115")
        ),
        "the reveal lifts the barrier",
    );
}

/// The id of the in-play location with `code`.
fn location_id(state: &GameState, code: &str) -> LocationId {
    state
        .locations
        .values()
        .find(|l| l.code.as_str() == code)
        .unwrap_or_else(|| panic!("location {code} in play"))
        .id
}

/// The investigator's location: a plain room connected to the one entered.
const HERE: LocationId = LocationId(10);
/// The room entered, printed as the Attic or the Cellar.
const ENTERED: LocationId = LocationId(20);

/// `INV` moves from `HERE` into the room printed as `code` (named `name`):
/// the real Move whose arrival fires the room's `EnteredLocation` forced
/// ability.
fn move_into(code: &str, name: &str) -> TestSession {
    let mut here = test_support::test_location(HERE.0, "Hallway");
    here.connections = vec![ENTERED];
    let mut entered = test_support::test_location(ENTERED.0, name);
    entered.code = CardCode::new(code);
    GameStateBuilder::new()
        .with_registries(scenarios::TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE))
        .with_location(here)
        .with_location(entered)
        .open_turn(INV)
        .session()
        .take(&TurnAction::Move {
            investigator: INV,
            destination: ENTERED,
        })
}

/// Attic 01113: *"**Forced** - After you enter the Attic: Take 1 horror."*
#[test]
fn attic_forced_enter_deals_one_horror() {
    let session = move_into("01113", "Attic");
    session.assert_at_turn_menu(INV);
    let investigator = &session.state().investigators[&INV];
    assert_eq!(
        investigator.current_location,
        Some(ENTERED),
        "the Move arrived"
    );
    assert_eq!(
        investigator.horror(),
        1,
        "entering the Attic deals 1 horror to the entering investigator",
    );
}

/// Cellar 01114: *"**Forced** - After you enter the Cellar: Take 1 damage."*
#[test]
fn cellar_forced_enter_deals_one_damage() {
    let session = move_into("01114", "Cellar");
    session.assert_at_turn_menu(INV);
    let investigator = &session.state().investigators[&INV];
    assert_eq!(
        investigator.current_location,
        Some(ENTERED),
        "the Move arrived"
    );
    assert_eq!(
        investigator.damage(),
        1,
        "entering the Cellar deals 1 damage to the entering investigator",
    );
}

/// Solo Roland seated at the Study for real, at his first turn menu, holding
/// the 2 clues act 1 (01108) needs to advance.
fn act_1_ready_to_advance() -> GameState {
    let mut state = setup_and_seat();
    state
        .investigators
        .get_mut(&INV)
        .expect("Roland seated")
        .clues = 2;
    state
}

#[test]
fn advancing_act_1_rebuilds_the_board() {
    let inv = INV;
    let state = act_1_ready_to_advance();

    let session = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .take(&TurnAction::AdvanceAct { investigator: inv });
    session.assert_at_turn_menu(inv);
    let result = session.finish();

    // Board rebuilt: four locations in play, Study gone, set-aside empty.
    let codes: BTreeSet<String> = result
        .state
        .locations
        .values()
        .map(|l| l.code.as_str().to_owned())
        .collect();
    assert_eq!(
        codes,
        ["01112", "01113", "01114", "01115"]
            .into_iter()
            .map(String::from)
            .collect()
    );
    assert_eq!(
        set_aside_codes(&result.state),
        ["01116", "01117"],
        "the Ghoul Priest (Act 2 spawns it) and Lita Chantler (#772) are still \
         set aside",
    );

    // The layout table wired the rooms as they entered: Hallway hub,
    // Attic/Cellar/Parlor spokes. Each id is minted at entry, so this is
    // the first moment any of these connections could exist.
    let id_of = |code: &str| {
        result
            .state
            .locations
            .values()
            .find(|l| l.code.as_str() == code)
            .unwrap_or_else(|| panic!("{code} in play"))
            .id
    };
    let hallway_id = id_of("01112");
    let mut spokes = ["01113", "01114", "01115"].map(id_of);
    spokes.sort_unstable();
    let mut hall_conns = result.state.locations[&hallway_id].connections.clone();
    hall_conns.sort_unstable();
    assert_eq!(
        hall_conns,
        spokes.to_vec(),
        "Hallway connects to Attic/Cellar/Parlor",
    );
    for spoke in spokes {
        assert_eq!(
            result.state.locations[&spoke].connections,
            vec![hallway_id],
            "each spoke connects back to the Hallway",
        );
    }

    // Investigator relocated to the Hallway (01112).
    assert_eq!(
        result.state.investigators[&inv].current_location,
        Some(hallway_id)
    );

    // Act cursor moved to Act 2.
    assert_eq!(result.state.act_index, 1);

    // Hallway (01112) was revealed when the investigator was relocated there.
    assert!(
        result.state.locations[&hallway_id].revealed,
        "relocate-to-Hallway reveals it",
    );
}

/// **Removing the Study discards what is attached to it, each by its owner**
/// (#981). Trapped 01108's reverse prints *"Remove the Study from the game."*,
/// and `glossary/Leaves_Play.md` makes a card's attachments go with it: *"All
/// attachments on the card are discarded."* Obscuring Fog 01168 drawn in Act 1
/// attaches to the Study; it is the encounter deck's, so it lands in the
/// encounter discard, while a Barricade 01038 its player put there lands in that
/// player's discard (`glossary/Discard_Piles.md`: *"Any time a card is
/// discarded, it is placed faceup on top of its owner's discard pile. Encounter
/// cards are owned by the encounter deck."*).
#[test]
fn removing_the_study_discards_its_attachments_by_owner() {
    let inv = INV;
    let mut state = act_1_ready_to_advance();
    let study = state.starting_location.expect("the Study is in play");
    // No seat places a location attachment: attach both to the Study by hand,
    // with ids minted from the state's own counter.
    let fog = state.card_instance_ids.mint();
    let barricade = state.card_instance_ids.mint();
    let attachments = &mut state.locations.get_mut(&study).unwrap().attachments;
    attachments.push(CardInPlay::enter_play(
        CardCode::new("01168"),
        fog,
        Owner::EncounterDeck,
    ));
    attachments.push(CardInPlay::enter_play(
        CardCode::new("01038"),
        barricade,
        Owner::Investigator(inv),
    ));

    let session = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .take(&TurnAction::AdvanceAct { investigator: inv });
    session.assert_at_turn_menu(inv);
    let result = session.finish();

    assert!(!result.state.locations.contains_key(&study), "Study gone");
    assert_eq!(result.state.encounter_discard, vec![CardCode::new("01168")]);
    assert_eq!(
        result.state.investigators[&inv].discard,
        vec![CardCode::new("01038")],
    );
    assert_eq!(
        result.state.removed_from_game,
        vec![CardCode::new("01111")],
        "the Study itself is removed from the game",
    );
    let leaving: Vec<&Event> = result
        .events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::CardDiscarded { .. } | Event::CardRemovedFromGame { .. }
            )
        })
        .collect();
    assert_eq!(
        leaving,
        [
            &Event::CardRemovedFromGame {
                code: CardCode::new("01111"),
                from: Zone::Location,
            },
            &Event::CardDiscarded {
                code: CardCode::new("01168"),
                from: Zone::LocationAttachment,
                to: DiscardPile::Encounter,
            },
            &Event::CardDiscarded {
                code: CardCode::new("01038"),
                from: Zone::LocationAttachment,
                to: DiscardPile::Investigator(inv),
            },
        ],
        "the Study's own event, then one per attachment, in order",
    );
}
