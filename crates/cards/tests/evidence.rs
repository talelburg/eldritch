//! End-to-end tests for Evidence! 01022 (Axis C reaction-event-play, #304)
//! against the real `cards::REGISTRY`.
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Fast. Play after you defeat an enemy.
//! > Discover 1 clue at your location.
//!
//! Lives at `crates/cards/tests/` so it can install [`cards::REGISTRY`] in its
//! own integration-test process.

use game_core::action::InputResponse;
use game_core::engine::TurnAction;
use game_core::engine::{EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, InvestigatorId, LocationId, Phase, TokenModifiers,
    Zone,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, take_turn_action, test_enemy, test_investigator, test_location,
    GameStateBuilder, TestSession,
};
use game_core::{apply, assert_event, assert_no_event, Action, PlayerAction};

/// `ArkhamDB` code for original-Core Evidence!.
const EVIDENCE: &str = "01022";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Solo investigator (NOT Roland — no in-play reaction) engaged with a 1-HP
/// enemy at a location with `location_clues` clues, holding Evidence! in hand.
/// A successful Combat test defeats the enemy and opens the after-defeat
/// window.
fn investigator_with_evidence_and_enemy(
    location_clues: u8,
) -> (InvestigatorId, EnemyId, LocationId, game_core::GameState) {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4;
    inv.hand.push(CardCode::new(EVIDENCE));

    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)

    let mut loc = test_location(10, "Study");
    loc.clues = location_clues;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(0)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, loc_id, state)
}

fn fight_action(inv: InvestigatorId, enemy: EnemyId) -> TurnAction {
    TurnAction::Fight {
        investigator: inv,
        enemy,
    }
}

#[test]
fn after_defeat_window_opens_and_offers_evidence_with_no_in_play_reaction() {
    let (inv_id, enemy_id, loc_id, state) = investigator_with_evidence_and_enemy(2);

    // Apply the Fight, then commit nothing to the resulting skill-test prompt;
    // the engine must then SUSPEND on the after-defeat reaction window — that
    // suspend is how "a window opened" is observed (the dedicated WindowOpened
    // event was removed as redundant with the AwaitingInput channel). The
    // window opens even though no in-play card reacts: the hand match alone
    // opens it, observable as the offered "Play <Evidence> from hand" option.
    let after_fight = take_turn_action(state, &fight_action(inv_id, enemy_id));
    let EngineOutcome::AwaitingInput { .. } = &after_fight.outcome else {
        panic!("Fight must suspend on the commit window; got {after_fight:?}");
    };
    let result = apply(
        after_fight.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );

    match &result.outcome {
        EngineOutcome::AwaitingInput { request, .. } => {
            assert!(
                request.options.iter().any(|o| o.label.contains(EVIDENCE)),
                "after-defeat window must offer the Evidence! hand play; request = {request:?}",
            );
            // A non-forced reaction window is a PickSingle the player may pass:
            // the client must offer a Skip control.
            assert_eq!(request.kind, game_core::InputKind::PickSingle);
            assert!(
                request.skippable,
                "a non-forced reaction window must be skippable; request = {request:?}",
            );
        }
        other => panic!("after-defeat window must open (AwaitingInput); got {other:?}"),
    }
    // The window is still open: no clue discovered yet, Evidence! still in hand.
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.locations[&loc_id].clues, 2);
    assert!(result.state.investigators[&inv_id]
        .hand
        .iter()
        .any(|c| c.as_str() == EVIDENCE));
}

#[test]
fn evidence_not_offered_when_location_has_no_clues() {
    // #495: Evidence!'s "Discover 1 clue at your location" can't change the game
    // state at a 0-clue location, so it must not be offered as a Fast play after
    // a defeat (RR: an event can't be played if its effect can't change the game
    // state). Mirror of the in-play Roland suppression — same ability, sourced
    // from hand.
    let (inv_id, enemy_id, loc_id, state) = investigator_with_evidence_and_enemy(0);

    let after_fight = take_turn_action(state, &fight_action(inv_id, enemy_id));
    let result = apply(
        after_fight.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );

    // The defeat happened, but no after-defeat window offering Evidence! opened:
    // the pending input is the open-turn menu, which never lists the hand play.
    assert_event!(result.events, Event::EnemyDefeated { enemy: e, .. } if *e == enemy_id);
    let EngineOutcome::AwaitingInput { request, .. } = &result.outcome else {
        panic!(
            "expected AwaitingInput (open turn), got {:?}",
            result.outcome
        );
    };
    assert!(
        !request.options.iter().any(|o| o.label.contains(EVIDENCE)),
        "Evidence! must not be offered at a 0-clue location; request = {request:?}",
    );
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.locations[&loc_id].clues, 0);
    assert!(
        result.state.investigators[&inv_id]
            .hand
            .iter()
            .any(|c| c.as_str() == EVIDENCE),
        "Evidence! stays in hand (never played)",
    );
}

#[test]
fn evidence_cannot_be_played_as_a_standalone_action() {
    // #304 acceptance: Evidence! is illegal outside its named window. Playing
    // it via the ordinary PlayCard action (here, during the turn with no defeat)
    // must reject — otherwise it would run no OnPlay effect and silently fizzle.
    let (inv_id, _enemy_id, loc_id, state) = investigator_with_evidence_and_enemy(2);
    assert_eq!(
        state.investigators[&inv_id].hand[0].as_str(),
        EVIDENCE,
        "fixture invariant: Evidence! is the only hand card, at index 0",
    );

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: inv_id,
            hand_index: 0,
        },
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Evidence! must be illegal as a standalone play; got {:?}",
        result.outcome,
    );
    // State unchanged: still in hand, not discarded, no clue moved.
    assert!(result.state.investigators[&inv_id]
        .hand
        .iter()
        .any(|c| c.as_str() == EVIDENCE));
    assert!(result.state.investigators[&inv_id].discard.is_empty());
    assert_eq!(result.state.locations[&loc_id].clues, 2);
}

#[test]
fn picking_evidence_plays_it_and_discovers_a_clue() {
    let (inv_id, enemy_id, loc_id, state) = investigator_with_evidence_and_enemy(2);

    // Commit nothing, then pick the single offered option (OptionId(0) = the
    // hand Evidence! play; there is no in-play trigger).
    let result = TestSession::new(state)
        .take(&fight_action(inv_id, enemy_id))
        .resolve_choices(|c| {
            c.commit_cards(&[]).pick_single(OptionId(0));
        })
        .run();

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::CardPlayed { investigator, code } if *investigator == inv_id && code.as_str() == EVIDENCE
    );
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::CardDiscarded { investigator, code, from: Zone::Hand }
            if *investigator == inv_id && code.as_str() == EVIDENCE
    );

    // 1 clue moved from the Study to the investigator; Evidence! is in discard.
    assert_eq!(result.state.locations[&loc_id].clues, 1);
    assert_eq!(result.state.investigators[&inv_id].clues, 1);
    let inv = &result.state.investigators[&inv_id];
    assert!(!inv.hand.iter().any(|c| c.as_str() == EVIDENCE));
    assert!(inv.discard.iter().any(|c| c.as_str() == EVIDENCE));
}

#[test]
fn evidence_fast_event_discards_exactly_once() {
    // Defeat an enemy to open the after-defeat window, then play Evidence!
    // 01022 from hand in it. Invariant guard for the PlayFromHand migration
    // (Slice D #423): the event must be discarded exactly once — no double-flush.
    let (inv_id, enemy_id, _loc_id, state) = investigator_with_evidence_and_enemy(2);

    let result = TestSession::new(state)
        .take(&fight_action(inv_id, enemy_id))
        .resolve_choices(|c| {
            c.commit_cards(&[]).pick_single(OptionId(0));
        })
        .run();

    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::CardDiscarded {
                        from: Zone::Hand,
                        ..
                    }
                )
            })
            .count(),
        1,
    );
}

#[test]
fn window_offers_both_in_play_reaction_and_hand_evidence() {
    use game_core::state::{CardInPlay, CardInstanceId};
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4;
    inv.hand.push(CardCode::new(EVIDENCE));
    // Roland's investigator card in play → his after-defeat reaction also matches.
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("01001"),
        CardInstanceId(1),
    ));

    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)

    let mut loc = test_location(10, "Study");
    loc.clues = 2;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(0)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    // Two options: OptionId(0) = Roland's in-play reaction, OptionId(1) = hand
    // Evidence!. Pick the hand play, then skip the remaining reaction.
    let result = TestSession::new(state)
        .take(&fight_action(inv_id, enemy_id))
        .resolve_choices(|c| {
            c.commit_cards(&[]).pick_single(OptionId(1)).skip();
        })
        .run();

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::CardPlayed { investigator, code } if *investigator == inv_id && code.as_str() == EVIDENCE
    );
    // Evidence! discovered its clue; Roland's reaction was skipped.
    assert_event!(result.events, Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id);
    assert_eq!(result.state.investigators[&inv_id].clues, 1);
}
