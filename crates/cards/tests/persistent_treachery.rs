//! Integration: The Gathering's three persistent treacheries (C4c, #235)
//! resolved through the real `cards` registry — they stay in play, enforce
//! their constant restriction, and discard at a forced timing point. Own
//! process so it can install the process-global registry against the real
//! corpus.

use std::sync::Once;

use game_core::action::{EngineRecord, PlayerAction};
use game_core::state::{
    Agenda, CardCode, CardInPlay, CardInstanceId, ChaosToken, EnemyId, InvestigatorId, Location,
    LocationId, Phase,
};
use game_core::test_support::{
    drive, fire_forced_after_location_investigated, fire_forced_on_round_end, test_enemy,
    test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{apply, Action, EngineOutcome};

static INSTALL: Once = Once::new();

fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Reveal the top encounter card for investigator 1, committing no cards
/// at any skill-test commit window that opens.
fn reveal_top(state: game_core::GameState) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    )
}

/// One investigator at location 20 (printed shroud 2), with `treachery`
/// on top of the encounter deck.
fn board_with(treachery: &str) -> game_core::GameState {
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(20))
        .with_location(test_location(20, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.encounter_deck.push_back(CardCode::new(treachery));
    state
}

#[test]
fn obscuring_fog_attaches_raises_shroud_and_discards_on_investigate() {
    install_registry();

    // Reveal: attaches to the investigator's location, not discarded.
    let result = reveal_top(board_with("01168"));
    assert_eq!(result.outcome, EngineOutcome::Done);
    let loc = &result.state.locations[&LocationId(20)];
    assert_eq!(loc.attachments.len(), 1, "Obscuring Fog attached");
    assert_eq!(loc.attachments[0].code.as_str(), "01168");
    assert!(
        !result
            .state
            .encounter_discard
            .contains(&CardCode::new("01168")),
        "a persistent treachery is not auto-discarded after its Revelation",
    );

    // +2 shroud: printed 2 → effective 4.
    assert_eq!(
        game_core::effective_shroud(&cards::REGISTRY, loc),
        4,
        "attached Obscuring Fog grants +2 shroud (printed 2)",
    );

    // Forced — after the attached location is successfully investigated,
    // discard Obscuring Fog.
    let mut state = result.state;
    let mut events = Vec::new();
    let outcome = fire_forced_after_location_investigated(
        &mut state,
        &mut events,
        InvestigatorId(1),
        LocationId(20),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(
        state.locations[&LocationId(20)].attachments.is_empty(),
        "Obscuring Fog discards after its location is investigated",
    );
    assert!(state.encounter_discard.contains(&CardCode::new("01168")));
}

// ---- Dissonant Voices (01165) --------------------------------------

#[test]
fn dissonant_voices_enters_threat_area_and_discards_on_round_end() {
    install_registry();

    let result = reveal_top(board_with("01165"));
    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.threat_area.len(), 1, "Dissonant Voices in threat area");
    assert_eq!(inv.threat_area[0].code.as_str(), "01165");
    assert!(!result
        .state
        .encounter_discard
        .contains(&CardCode::new("01165")));

    // Forced — at the end of the round, discard Dissonant Voices.
    let mut state = result.state;
    let mut events = Vec::new();
    let outcome = fire_forced_on_round_end(&mut state, &mut events);
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(
        state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty(),
        "Dissonant Voices discards at end of round",
    );
    assert!(state.encounter_discard.contains(&CardCode::new("01165")));
}

#[test]
fn dissonant_voices_forbids_playing_an_asset() {
    install_registry();
    // Investigator mid-investigation with a playable asset (Holy Rosary,
    // 01059) in hand and Dissonant Voices in their threat area.
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(101));
    inv.hand = vec![CardCode::new("01059")];
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01165"),
        CardInstanceId(0),
    ));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_location(test_location(101, "Study"))
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        }),
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Dissonant Voices forbids playing assets; got {:?}",
        result.outcome,
    );
    // Validate-first: the asset stays in hand, nothing entered play.
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.hand, vec![CardCode::new("01059")]);
    assert!(inv.cards_in_play.is_empty());
}

#[test]
fn dissonant_voices_round_end_coexists_with_agenda_01107_doom() {
    install_registry();
    // Both Dissonant Voices (threat area) and agenda 01107 carry a
    // RoundEnded forced ability. They must resolve together (deterministic
    // multi-resolve), not reject: the agenda places doom per ghoul in the
    // Hallway/Parlor, and Dissonant Voices discards itself.
    let loc = |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
    let mut inv = test_investigator(1);
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01165"),
        CardInstanceId(0),
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_location(loc(2, "01112", "Hallway"))
        .with_location(loc(5, "01115", "Parlor"))
        .build();
    let mut ghoul = test_enemy(1, "Ghoul");
    ghoul.traits = vec!["Monster".into(), "Ghoul".into()];
    ghoul.current_location = Some(LocationId(2)); // Hallway
    state.enemies.insert(EnemyId(1), ghoul);
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 10,
        resolution: None,
    }];
    state.agenda_index = 0;

    let mut events = Vec::new();
    let outcome = fire_forced_on_round_end(&mut state, &mut events);
    assert_eq!(
        outcome,
        EngineOutcome::Done,
        "two simultaneous RoundEnded forced triggers resolve in order, not reject",
    );
    assert_eq!(
        state.agenda_deck[0_usize].doom_threshold, 10,
        "doom_threshold unchanged (sanity)",
    );
    assert!(
        state.agenda_doom >= 1,
        "agenda 01107 placed doom for the Ghoul in the Hallway; agenda_doom = {}",
        state.agenda_doom,
    );
    assert!(
        state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty(),
        "Dissonant Voices also discarded in the same round-end resolution",
    );
}

// ---- Frozen in Fear (01164) ----------------------------------------

#[test]
fn frozen_in_fear_surcharges_first_move_each_round_only() {
    install_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(1));
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(0),
    ));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_location(test_location(1, "A"))
        .with_location(test_location(2, "B"))
        .build();
    state.connect(LocationId(1), LocationId(2));
    assert_eq!(state.investigators[&InvestigatorId(1)].actions_remaining, 3);

    // First move this round costs 2 (base 1 + surcharge 1): 3 → 1.
    let r = apply(
        state,
        Action::Player(PlayerAction::Move {
            investigator: InvestigatorId(1),
            destination: LocationId(2),
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].actions_remaining,
        1,
        "first move/fight/evade each round costs +1 action",
    );

    // Second move this round costs 1 (surcharge already spent): 1 → 0.
    let r = apply(
        r.state,
        Action::Player(PlayerAction::Move {
            investigator: InvestigatorId(1),
            destination: LocationId(1),
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].actions_remaining,
        0,
        "subsequent actions that round cost the normal 1",
    );
}

/// Build a two-investigator Investigation-phase board with Frozen in Fear
/// in investigator 1's threat area and a single rigged chaos token.
fn frozen_in_fear_board(token: ChaosToken) -> game_core::GameState {
    let mut inv1 = test_investigator(1);
    inv1.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(0),
    ));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv1)
        .with_investigator(test_investigator(2))
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();
    state.chaos_bag.tokens = vec![token];
    state
}

fn end_turn_committing_nothing(state: game_core::GameState) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    drive(state, Action::Player(PlayerAction::EndTurn), resolver)
}

#[test]
fn frozen_in_fear_end_of_turn_success_discards_and_turn_resumes() {
    install_registry();
    // Willpower 3 + Numeric(0) = 3 vs difficulty 3 → success.
    let r = end_turn_committing_nothing(frozen_in_fear_board(ChaosToken::Numeric(0)));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert!(
        r.state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty(),
        "succeeded willpower(3) test discards Frozen in Fear",
    );
    assert!(r.state.encounter_discard.contains(&CardCode::new("01164")));
    // The suspending end-of-turn test did not strand the turn: rotation ran.
    assert_eq!(
        r.state.active_investigator,
        Some(InvestigatorId(2)),
        "end_turn resumed after the test and rotated to investigator 2",
    );
    assert!(r.state.pending_end_turn.is_none());
}

#[test]
fn frozen_in_fear_end_of_turn_failure_keeps_card_but_turn_still_resumes() {
    install_registry();
    // Willpower 3 + Numeric(-1) = 2 vs difficulty 3 → fail.
    let r = end_turn_committing_nothing(frozen_in_fear_board(ChaosToken::Numeric(-1)));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].threat_area.len(),
        1,
        "failed test leaves Frozen in Fear in the threat area",
    );
    assert!(!r.state.encounter_discard.contains(&CardCode::new("01164")));
    // Turn still progresses regardless of the test outcome.
    assert_eq!(r.state.active_investigator, Some(InvestigatorId(2)));
    assert!(r.state.pending_end_turn.is_none());
}

#[test]
fn two_frozen_in_fear_end_of_turn_tests_both_resolve_then_turn_resumes() {
    // #213 reentrancy: two Frozen in Fear copies on one investigator fire two
    // simultaneous `EndOfTurn` forced abilities, each a *suspending* willpower
    // test. The lead orders them; firing the first suspends on its commit
    // window, and once it resolves the forced run resumes the second sibling —
    // rather than abandoning it. After both resolve, the end-of-turn tail runs
    // (rotation to the next investigator).
    install_registry();

    let mut inv1 = test_investigator(1);
    inv1.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(0),
    ));
    inv1.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(1),
    ));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv1)
        .with_investigator(test_investigator(2))
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();
    // Two Numeric(0) draws → willpower 3 vs difficulty 3 → both succeed.
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0), ChaosToken::Numeric(0)];

    // Order the first forced, commit nothing to its test; order the second,
    // commit nothing to its test.
    let mut resolver = ScriptedResolver::new();
    resolver.pick(0).commit_cards(&[]).pick(0).commit_cards(&[]);
    let r = drive(state, Action::Player(PlayerAction::EndTurn), resolver);

    assert_eq!(r.outcome, EngineOutcome::Done);
    assert!(
        r.state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty(),
        "both succeeded willpower tests discard both Frozen in Fear copies",
    );
    assert_eq!(
        r.state
            .encounter_discard
            .iter()
            .filter(|c| **c == CardCode::new("01164"))
            .count(),
        2,
        "both copies land in the encounter discard",
    );
    // Neither sibling was abandoned, and the end-of-turn tail still ran:
    assert_eq!(
        r.state.active_investigator,
        Some(InvestigatorId(2)),
        "end_turn resumed after both tests and rotated to investigator 2",
    );
    assert!(r.state.pending_end_turn.is_none());
}

#[test]
fn obscuring_fog_limit_one_per_location_discards_the_second_copy() {
    install_registry();

    // First copy attaches.
    let result = reveal_top(board_with("01168"));
    let mut state = result.state;
    assert_eq!(state.locations[&LocationId(20)].attachments.len(), 1);

    // Second copy revealed at the same location: limit 1 → discarded, not
    // attached.
    state.encounter_deck.push_back(CardCode::new("01168"));
    let result = reveal_top(state);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.locations[&LocationId(20)].attachments.len(),
        1,
        "limit 1 per location: the second copy does not attach",
    );
    assert!(
        result
            .state
            .encounter_discard
            .contains(&CardCode::new("01168")),
        "the over-limit copy is discarded",
    );
}
