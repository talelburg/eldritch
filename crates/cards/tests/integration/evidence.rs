//! End-to-end tests for Evidence! 01022, a Fast event played in a reaction
//! window, against the real `cards::REGISTRY`.
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Fast. Play after you defeat an enemy.
//! > Discover 1 clue at your location.
//!
//! Lives at `crates/cards/tests/` so it can run against
//! `cards::TEST_REGISTRIES`, which `game-core` cannot reach by crate direction.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{InputKind, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, DiscardPile, EnemyId, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind, Zone,
};
use game_core::test_support::{self, seat, Either, Seat, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

/// `ArkhamDB` code for original-Core Evidence!.
const EVIDENCE: &str = "01022";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

const FIGHT: TurnAction = TurnAction::Fight {
    investigator: INV,
    enemy: ENEMY,
};

/// Solo investigator (NOT Roland — no in-play reaction) at the Study with
/// combat 4, holding Evidence! in hand.
fn holding_evidence() -> Seat {
    seat(1)
        .at(HERE)
        .skill(SkillKind::Combat, 4)
        .hand([EVIDENCE])
}

/// `investigator`'s open turn, engaged with a 1-HP enemy at a location with
/// `location_clues` clues. A successful Combat test defeats the enemy and
/// opens the after-defeat window.
fn fight_board(investigator: impl Into<Seat<Either>>, location_clues: u8) -> GameStateBuilder {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;

    let mut loc = test_support::test_location(HERE.0, "Study");
    loc.clues = location_clues;

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_round(0)
        .with_seat(investigator)
        // Co-located by engagement: Fight is location-gated.
        .with_enemy_engaged(enemy, INV)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
}

/// Fight the enemy committing nothing: the session rests at the after-defeat
/// window, or back at the turn menu if none opened.
fn defeat_the_enemy(board: GameStateBuilder) -> TestSession {
    board.session().take(&FIGHT).commit(&[])
}

/// The anchor of the Evidence! in `INV`'s hand, as a reaction window offers it.
fn evidence_in_hand() -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(EVIDENCE),
    }
}

/// Defeat the enemy, then play Evidence! from the after-defeat window.
fn play_evidence(board: GameStateBuilder) -> TestSession {
    defeat_the_enemy(board).pick(evidence_in_hand())
}

#[test]
fn after_defeat_window_opens_and_offers_evidence_with_no_in_play_reaction() {
    // Apply the Fight, then commit nothing to the resulting skill-test prompt;
    // the engine must then SUSPEND on the after-defeat reaction window — that
    // suspend is how "a window opened" is observed, since no event marks a
    // window opening; the AwaitingInput channel does. The
    // window opens even though no in-play card reacts: the hand match alone
    // opens it, observable as the offered Evidence! hand play.
    let session = defeat_the_enemy(fight_board(holding_evidence(), 2));

    let request = session.prompt();
    assert_eq!(
        session.offered_at(&evidence_in_hand()),
        1,
        "after-defeat window must offer the Evidence! hand play; request = {request:?}",
    );
    // A non-forced reaction window is a PickSingle the player may pass:
    // the client must offer a Skip control.
    assert_eq!(request.kind, InputKind::PickSingle);
    assert!(
        request.skippable,
        "a non-forced reaction window must be skippable; request = {request:?}",
    );
    // The window is still open: no clue discovered yet, Evidence! still in hand.
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(session.state().locations[&HERE].clues, 2);
    assert!(session.state().investigators[&INV]
        .hand
        .iter()
        .any(|c| c.as_str() == EVIDENCE));
}

#[test]
fn evidence_not_offered_when_location_has_no_clues() {
    // Evidence!'s "Discover 1 clue at your location" can't change the game
    // state at a 0-clue location, so it must not be offered as a Fast play after
    // a defeat (RR: an event can't be played if its effect can't change the game
    // state). Mirror of the in-play Roland suppression — same ability, sourced
    // from hand.
    let session = defeat_the_enemy(fight_board(holding_evidence(), 0));

    // The defeat happened, but no after-defeat window offering Evidence! opened:
    // play is back at the open-turn menu, which never lists the hand play.
    assert_event!(session.events(), Event::EnemyDefeated { enemy: e, .. } if *e == ENEMY);
    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(session.state().locations[&HERE].clues, 0);
    assert!(
        session.state().investigators[&INV]
            .hand
            .iter()
            .any(|c| c.as_str() == EVIDENCE),
        "Evidence! stays in hand (never played)",
    );
}

#[test]
fn evidence_cannot_be_played_as_a_standalone_action() {
    // Evidence! is illegal outside its named window. Playing
    // it via the ordinary PlayCard action (here, during the turn with no defeat)
    // must reject — otherwise it would run no OnPlay effect and silently fizzle.
    // The menu does not offer it, so it goes straight to the handler, which a
    // client can do over the wire.
    let session = fight_board(holding_evidence(), 2).session();
    assert_eq!(
        session.state().investigators[&INV].hand[0].as_str(),
        EVIDENCE,
        "fixture invariant: Evidence! is the only hand card, at index 0",
    );

    let session = session.submit_off_menu(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });

    let _ = session.expect_rejected();
    // State unchanged: still in hand, not discarded, no clue moved.
    let state = session.state();
    assert!(state.investigators[&INV]
        .hand
        .iter()
        .any(|c| c.as_str() == EVIDENCE));
    assert!(state.investigators[&INV].discard.is_empty());
    assert_eq!(state.locations[&HERE].clues, 2);
}

#[test]
fn picking_evidence_plays_it_and_discovers_a_clue() {
    // Commit nothing, then pick the hand Evidence! play; there is no in-play
    // trigger.
    let session = play_evidence(fight_board(holding_evidence(), 2));

    session.assert_at_turn_menu(INV);
    let events = session.events();
    assert_event!(
        events,
        Event::PlayCommenced { investigator, code } if *investigator == INV && code.as_str() == EVIDENCE
    );
    assert_event!(
        events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
    assert_event!(
        events,
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(investigator) }
            if *investigator == INV && code.as_str() == EVIDENCE
    );

    // 1 clue moved from the Study to the investigator; Evidence! is in discard.
    let state = session.state();
    assert_eq!(state.locations[&HERE].clues, 1);
    assert_eq!(state.investigators[&INV].clues, 1);
    let inv = &state.investigators[&INV];
    assert!(!inv.hand.iter().any(|c| c.as_str() == EVIDENCE));
    assert!(inv.discard.iter().any(|c| c.as_str() == EVIDENCE));
}

#[test]
fn evidence_fast_event_discards_exactly_once() {
    // Defeat an enemy to open the after-defeat window, then play Evidence!
    // 01022 from hand in it. The event must be discarded exactly once — no
    // double-flush.
    let session = play_evidence(fight_board(holding_evidence(), 2));

    assert_event_count!(
        session.events(),
        1,
        Event::CardDiscarded {
            from: Zone::Hand,
            ..
        }
    );
}

#[test]
fn playing_evidence_from_hand_pays_its_resource_cost() {
    // Evidence! (01022) costs 1 resource. Playing it from its after-defeat
    // window deducts the cost (RR p.22, Initiation Sequence) — a Fast play
    // skips the *action* cost, not the *resource* cost. The fixture starts at
    // the default 5 resources.
    let board = fight_board(holding_evidence(), 2);
    let session = board.session();
    assert_eq!(
        session.state().investigators[&INV].resources,
        5,
        "fixture invariant: 5 starting resources",
    );

    let session = session.take(&FIGHT).commit(&[]).pick(evidence_in_hand());

    assert_event!(
        session.events(),
        Event::ResourcesPaid { investigator, amount } if *investigator == INV && *amount == 1
    );
    assert_eq!(
        session.state().investigators[&INV].resources,
        4,
        "5 - cost 1 = 4 after playing Evidence!",
    );
}

#[test]
fn evidence_not_offered_when_resources_below_cost() {
    // Evidence! costs 1; with 0 resources the cost can't be paid, so the
    // after-defeat window must not offer it (RR p.22: the cost must be
    // established as payable before initiation). Mirror of the change-
    // game-state suppression above, but on affordability.
    let session = defeat_the_enemy(fight_board(holding_evidence().resources(0), 2));

    // No window offering Evidence! opened: play is back at the turn menu.
    session.assert_at_turn_menu(INV);
    assert!(
        session.state().investigators[&INV]
            .hand
            .iter()
            .any(|c| c.as_str() == EVIDENCE),
        "Evidence! stays in hand (never played)",
    );
    assert_no_event!(session.events(), Event::CluePlaced { .. });
}

#[test]
fn window_offers_both_in_play_reaction_and_hand_evidence() {
    // Roland Banks seated (01001, combat 4): his investigator card's
    // after-defeat reaction also matches.
    let offered = defeat_the_enemy(fight_board(
        seat(1).at(HERE).as_card("01001").hand([EVIDENCE]),
        2,
    ));
    let rolands_card = offered.state().investigators[&INV].card_anchor();
    assert_eq!(
        (
            offered.offered_at(&rolands_card),
            offered.offered_at(&evidence_in_hand()),
            offered.prompt().options.len(),
        ),
        (1, 1, 2),
        "the window offers Roland's reaction and the hand Evidence!: {:?}",
        offered.prompt(),
    );

    // Pick the hand play, then skip the remaining reaction.
    let session = offered.pick(evidence_in_hand()).skip();

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::PlayCommenced { investigator, code } if *investigator == INV && code.as_str() == EVIDENCE
    );
    // Evidence! discovered its clue; Roland's reaction was skipped.
    assert_event!(session.events(), Event::CluePlaced { investigator, count: 1 } if *investigator == INV);
    assert_eq!(session.state().investigators[&INV].clues, 1);
}
