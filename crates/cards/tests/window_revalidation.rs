//! Fire-time re-validation of reaction-window candidates (#568) against the
//! real `cards::REGISTRY`.
//!
//! A window's candidate list is a snapshot of one scan, and resolving one of its
//! options changes the board. The Rules Reference makes **initiation**, not
//! scanning, the moment that binds (Triggered Abilities):
//!
//! > A triggered ability can only be initiated if its effect has the potential to
//! > change the game state, and its cost (if any) has the potential to be paid in
//! > full, taking active cost modifiers into account.
//!
//! Card text (verbatim, <https://arkhamdb.com/card/01022>):
//!
//! > Fast. Play after you defeat an enemy.
//! > Discover 1 clue at your location.
//!
//! and (verbatim, <https://arkhamdb.com/card/01001>):
//!
//! > After you defeat an enemy: Discover 1 clue at your location. (Limit once
//! > per round.)
//!
//! Both carry the same FAQ ruling — *"You can only 'discover' a clue if there is
//! a clue on your location."* — which is what makes the sibling in each pair
//! lapse.
//!
//! Lives at `crates/cards/tests/` so it can install [`cards::REGISTRY`] in its
//! own integration-test process.

use game_core::action::InputResponse;
use game_core::dsl::EventTiming;
use game_core::engine::{EngineOutcome, OptionId, TimingEvent, TurnAction};
use game_core::event::{Event, LapseReason};
use game_core::state::{
    CandidateSource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation,
    EnemyId, GameState, Investigator, InvestigatorId, LocationId, Phase, ResolutionCandidate,
    TimingMode, TokenModifiers,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder, TestSession,
};
use game_core::{apply, assert_event, assert_no_event, Action, PlayerAction};

/// `ArkhamDB` code for original-Core Evidence!.
const EVIDENCE: &str = "01022";
/// `ArkhamDB` code for Roland Banks.
const ROLAND: &str = "01001";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// A solo investigator engaged with a 1-HP enemy at a `location_clues`-clue
/// location, with a stacked bag so a Fight auto-succeeds and the defeat opens the
/// after-defeat reaction window. `shape` fills in what each test varies — hand
/// contents, wallet, whether Roland's investigator card is in play.
fn after_defeat_board(
    location_clues: u8,
    shape: impl FnOnce(&mut Investigator),
) -> (InvestigatorId, EnemyId, LocationId, GameState) {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4;
    shape(&mut inv);

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

/// How many times `events` contains a [`ReactionOptionLapsed`] for `code`.
///
/// [`ReactionOptionLapsed`]: Event::ReactionOptionLapsed
fn lapse_count(events: &[Event], code: &str) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Event::ReactionOptionLapsed { code: c, .. } if c.as_str() == code))
        .count()
}

#[test]
fn a_second_evidence_is_withdrawn_once_the_first_empties_the_wallet() {
    // #568 acceptance 1. Two copies of Evidence! (cost 1 each) on a 1-resource
    // wallet: the scan offers both, because at scan time either one is payable.
    // Playing the first empties the wallet, so the second can no longer be
    // initiated — RR p.22, the cost must have the potential to be paid in full.
    //
    // Pre-fix, the window re-prompted the stored candidate unfiltered and
    // `pay_play_cost`'s `saturating_sub` took 1 from 0, emitting a **false**
    // `ResourcesPaid { amount: 1 }` and resolving the card for free.
    let (inv_id, enemy_id, loc_id, state) = after_defeat_board(2, |inv| {
        inv.hand.push(CardCode::new(EVIDENCE));
        inv.hand.push(CardCode::new(EVIDENCE));
        inv.resources = 1;
    });

    // Commit nothing to the Fight's skill test, then play the first Evidence!.
    // The window closes on its own afterwards: its only remaining option lapsed.
    // The second `pick_single` is the pre-fix path — unused once the option is
    // withdrawn, and what makes this test fail on the *defect* (a second play, a
    // second `ResourcesPaid`) rather than on a missing scripted response.
    let result = TestSession::new(state)
        .take(&fight_action(inv_id, enemy_id))
        .resolve_choices(|c| {
            c.commit_cards(&[])
                .pick_single(OptionId(0))
                .pick_single(OptionId(0));
        })
        .run();

    // Exactly one play, one payment, one clue — not two of anything.
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, Event::CardPlayed { code, .. } if code.as_str() == EVIDENCE))
            .count(),
        1,
        "only the affordable copy may be played; events = {:?}",
        result.events,
    );
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, Event::ResourcesPaid { .. }))
            .count(),
        1,
        "a second ResourcesPaid would be the false event #568 describes; events = {:?}",
        result.events,
    );
    assert_event!(
        result.events,
        Event::ReactionOptionLapsed { code, reason: LapseReason::CostUnpayable, investigator }
            if code.as_str() == EVIDENCE && *investigator == inv_id
    );
    assert_eq!(lapse_count(&result.events, EVIDENCE), 1);

    // The wallet paid exactly the one cost it could afford, the unplayed copy is
    // still in hand, and only one clue moved.
    let inv = &result.state.investigators[&inv_id];
    assert_eq!(inv.resources, 0, "1 - cost 1 = 0, and nothing further");
    assert_eq!(
        inv.hand.iter().filter(|c| c.as_str() == EVIDENCE).count(),
        1,
        "the unaffordable copy stays in hand",
    );
    assert_eq!(
        inv.discard
            .iter()
            .filter(|c| c.as_str() == EVIDENCE)
            .count(),
        1,
    );
    assert_eq!(inv.clues, 1);
    assert_eq!(result.state.locations[&loc_id].clues, 1);
}

#[test]
fn evidence_is_withdrawn_when_rolands_reaction_takes_the_last_clue() {
    // #568 acceptance 2. Roland Banks and Evidence! both read "Discover 1 clue at
    // your location", and both are FAQ'd "You can only 'discover' a clue if there
    // is a clue on your location." At a 1-clue location the after-defeat window
    // offers both; firing Roland's reaction takes that clue, so Evidence! can no
    // longer change the game state and must not stay on offer.
    //
    // Pre-fix it did: playing it paid a resource and discarded the card, then
    // `discover_clue` found 0 clues and no-opped silently — one resource and one
    // card spent for nothing, with no error.
    let (inv_id, enemy_id, loc_id, state) = after_defeat_board(1, |inv| {
        inv.hand.push(CardCode::new(EVIDENCE));
        // Roland's investigator card in play → his after-defeat reaction matches
        // alongside the hand play.
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(ROLAND),
            CardInstanceId(1),
        ));
    });
    assert_eq!(
        state.investigators[&inv_id].resources, 5,
        "fixture invariant: 5 starting resources",
    );

    // OptionId(0) = Roland's in-play reaction (in-play triggers are offered
    // before hand plays); OptionId(1) would be the Evidence! play. The second
    // `pick_single` is the pre-fix path — unused once Evidence! is withdrawn, and
    // what makes this test fail on the *defect* (a resource and a card spent for
    // a no-op discovery) rather than on a missing scripted response.
    let result = TestSession::new(state)
        .take(&fight_action(inv_id, enemy_id))
        .resolve_choices(|c| {
            c.commit_cards(&[])
                .pick_single(OptionId(0))
                .pick_single(OptionId(0));
        })
        .run();

    assert_event!(
        result.events,
        Event::ReactionOptionLapsed { code, reason: LapseReason::NoStateChange, investigator }
            if code.as_str() == EVIDENCE && *investigator == inv_id
    );
    assert_eq!(lapse_count(&result.events, EVIDENCE), 1);
    // Roland's clue moved; Evidence! never resolved.
    assert_no_event!(result.events, Event::CardPlayed { .. });
    assert_no_event!(result.events, Event::ResourcesPaid { .. });
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, Event::CluePlaced { .. }))
            .count(),
        1,
    );

    let inv = &result.state.investigators[&inv_id];
    assert_eq!(inv.clues, 1);
    assert_eq!(inv.resources, 5, "Evidence! was never paid for");
    assert!(
        inv.hand.iter().any(|c| c.as_str() == EVIDENCE),
        "Evidence! stays in hand",
    );
    assert!(inv.discard.is_empty());
    assert_eq!(result.state.locations[&loc_id].clues, 0);
}

#[test]
fn a_still_payable_second_evidence_is_not_withdrawn() {
    // The guard against over-withdrawing: with 2 resources both copies stay
    // payable, and with 2 clues both stay able to discover, so firing the first
    // must leave the second on offer. Same fixture as the wallet test, one
    // resource richer.
    let (inv_id, enemy_id, loc_id, state) = after_defeat_board(2, |inv| {
        inv.hand.push(CardCode::new(EVIDENCE));
        inv.hand.push(CardCode::new(EVIDENCE));
        inv.resources = 2;
    });

    let result = TestSession::new(state)
        .take(&fight_action(inv_id, enemy_id))
        .resolve_choices(|c| {
            c.commit_cards(&[])
                .pick_single(OptionId(0))
                .pick_single(OptionId(0));
        })
        .run();

    assert_eq!(lapse_count(&result.events, EVIDENCE), 0);
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, Event::CardPlayed { code, .. } if code.as_str() == EVIDENCE))
            .count(),
        2,
    );
    let inv = &result.state.investigators[&inv_id];
    assert_eq!(inv.resources, 0, "2 - two costs of 1 = 0");
    assert_eq!(inv.clues, 2);
    assert!(!inv.hand.iter().any(|c| c.as_str() == EVIDENCE));
    assert_eq!(result.state.locations[&loc_id].clues, 0);
}

#[test]
fn firing_a_candidate_whose_card_left_hand_rejects_instead_of_panicking() {
    // #568 acceptance 3. A hand candidate naming a card the investigator no
    // longer holds is the shape a sibling option leaves behind when it removes
    // that card from hand. The window frame is built directly because the two
    // prompt sites now withdraw such an option before it can be picked — what is
    // under test is the fire-time gate itself, reachable by a client replaying a
    // stale option id.
    //
    // Pre-fix this path was an `unreachable!` in `play_fast_event`: a reachable
    // panic that would take the session down.
    let (inv_id, enemy_id, loc_id, mut state) = after_defeat_board(2, |_inv| {
        // Deliberately no Evidence! in hand.
    });
    state.continuations.push(Continuation::TimingPointWindow {
        event: TimingEvent::EnemyDefeated {
            enemy: enemy_id,
            by: Some(inv_id),
            code: CardCode::new("_synth_enemy"),
        },
        bucket: EventTiming::After,
        mode: TimingMode::Reaction,
        candidates: vec![ResolutionCandidate::new(
            CardCode::new(EVIDENCE),
            inv_id,
            0,
            CandidateSource::Hand,
        )],
    });

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "a candidate that can no longer be initiated must reject, not panic; got {:?}",
        result.outcome,
    );
    // Rejection leaves state and events untouched (the `apply_via` contract):
    // nothing paid, nothing played, no clue moved.
    assert_no_event!(result.events, Event::ResourcesPaid { .. });
    assert_no_event!(result.events, Event::CardPlayed { .. });
    assert_eq!(result.state.investigators[&inv_id].resources, 5);
    assert_eq!(result.state.locations[&loc_id].clues, 2);
}

#[test]
fn a_lone_evidence_still_opens_and_resolves_its_window() {
    // Regression floor: re-validation must not disturb the ordinary single-option
    // window. Same board as the wallet test with one copy and a full wallet.
    let (inv_id, enemy_id, loc_id, state) = after_defeat_board(2, |inv| {
        inv.hand.push(CardCode::new(EVIDENCE));
    });

    let after_fight = take_turn_action(state, &fight_action(inv_id, enemy_id));
    let opened = apply(
        after_fight.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    let EngineOutcome::AwaitingInput { request, .. } = &opened.outcome else {
        panic!("after-defeat window must open; got {:?}", opened.outcome);
    };
    assert!(request.options.iter().any(|o| o.label.contains(EVIDENCE)));
    assert_eq!(lapse_count(&opened.events, EVIDENCE), 0);

    let result = apply(
        opened.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_eq!(result.state.locations[&loc_id].clues, 1);
}
