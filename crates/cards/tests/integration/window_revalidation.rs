//! Regression for #568: reaction-window candidates are re-validated at fire
//! time, against the real `cards::REGISTRY`.
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
//! Lives at `crates/cards/tests/` so it can run against
//! `cards::TEST_REGISTRIES`, which `game-core` cannot reach by crate direction.

use card_dsl::dsl::UsagePeriod;
use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::{Event, LapseReason};
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind, Status, UsageClock,
};
use game_core::test_support::{enemy, location, seat, BoardLookup as _, Either, Seat, TestSession};

use game_core::{assert_event, assert_event_count, assert_no_event};

/// `ArkhamDB` code for original-Core Evidence!.
const EVIDENCE: &str = "01022";
/// `ArkhamDB` code for Roland Banks.
const ROLAND: &str = "01001";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

/// A solo investigator (combat 4) engaged with a 1-HP enemy at a
/// `location_clues`-clue location, with a stacked bag so a Fight auto-succeeds
/// and the defeat opens the after-defeat reaction window. `shape` fills in what
/// each test varies — hand contents, wallet, whether Roland's investigator card
/// is in play.
fn after_defeat_board<S: Into<Seat<Either>>>(
    location_clues: u8,
    shape: impl FnOnce(Seat) -> S,
) -> GameState {
    let investigator = seat(1).at(HERE).skill(SkillKind::Combat, 4);

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_round(0)
        .open_turn(INV)
        .with_seat(shape(investigator))
        // Co-located: Fight is location-gated.
        .with_enemy_engaged(enemy(ENEMY).fight(1).health(1), INV)
        .with_location(location(HERE).clues(location_clues))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build()
}

/// Fight the enemy on `state`, committing nothing: the defeat opens the
/// after-defeat window, where the session rests.
fn defeat_the_enemy(state: GameState) -> TestSession {
    TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: ENEMY,
        })
        .commit(&[])
}

/// The anchor of the Evidence! copies in `INV`'s hand.
fn evidence_in_hand() -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(EVIDENCE),
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

/// Play one of the two Evidence! copies the after-defeat window offers.
///
/// The window offers each copy as its own option, both anchored to
/// [`evidence_in_hand`], so `pick` cannot name one; the copies are
/// interchangeable, so the first one at the anchor plays.
fn play_one_of_two_evidence(window: TestSession) -> TestSession {
    assert_eq!(window.offered_at(&evidence_in_hand()), 2);
    window.pick_nth_at(evidence_in_hand(), 0)
}

/// How many Evidence! plays `events` holds.
fn evidence_plays(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Event::PlayCommenced { code, .. } if code.as_str() == EVIDENCE))
        .count()
}

#[test]
fn a_second_evidence_is_withdrawn_once_the_first_empties_the_wallet() {
    // Two copies of Evidence! (cost 1 each) on a 1-resource
    // wallet: the scan offers both, because at scan time either one is payable.
    // Playing the first empties the wallet, so the second can no longer be
    // initiated — RR p.22, the cost must have the potential to be paid in full.
    //
    // Re-prompting the stored candidate unfiltered would let
    // `pay_play_cost`'s `saturating_sub` take 1 from 0, emitting a **false**
    // `ResourcesPaid { amount: 1 }` and resolving the card for free.
    let state = after_defeat_board(2, |inv| inv.hand([EVIDENCE, EVIDENCE]).resources(1));

    // Commit nothing to the Fight's skill test, then play the first Evidence!.
    // The window closes on its own afterwards: its only remaining option lapsed,
    // so play is back at the turn menu with no second offer to take.
    let session = play_one_of_two_evidence(defeat_the_enemy(state));
    session.assert_at_turn_menu(INV);
    let events = session.events();

    // Exactly one play, one payment, one clue — not two of anything.
    assert_eq!(
        evidence_plays(events),
        1,
        "only the affordable copy may be played; events = {events:?}",
    );
    assert_event_count!(events, 1, Event::ResourcesPaid { .. });
    assert_event!(
        events,
        Event::ReactionOptionLapsed { code, reason: LapseReason::CostUnpayable, investigator }
            if code.as_str() == EVIDENCE && *investigator == INV
    );
    assert_eq!(lapse_count(events, EVIDENCE), 1);

    // The wallet paid exactly the one cost it could afford, the unplayed copy is
    // still in hand, and only one clue moved.
    let inv = &session.state().investigators[&INV];
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
    assert_eq!(session.state().locations[&HERE].clues, 1);
}

#[test]
fn evidence_is_withdrawn_when_rolands_reaction_takes_the_last_clue() {
    // Roland Banks and Evidence! both read "Discover 1 clue at
    // your location", and both are FAQ'd "You can only 'discover' a clue if there
    // is a clue on your location." At a 1-clue location the after-defeat window
    // offers both; firing Roland's reaction takes that clue, so Evidence! can no
    // longer change the game state and must not stay on offer.
    //
    // Were it to stay, playing it would pay a resource and discard the card,
    // then `discover_clue` would find 0 clues and no-op silently — one resource
    // and one card spent for nothing, with no error.
    //
    // Roland's investigator card in play → his after-defeat reaction matches
    // alongside the hand play.
    let state = after_defeat_board(1, |inv| inv.hand([EVIDENCE]).in_play([ROLAND]));
    assert_eq!(
        state.investigators[&INV].resources, 5,
        "fixture invariant: 5 starting resources",
    );
    let roland = state.card(INV, ROLAND);

    // Fire Roland's reaction. Evidence! is then withdrawn, so the window closes
    // and play is back at the turn menu — not still offering a resource and a
    // card spent for a no-op discovery.
    let offered = defeat_the_enemy(state);
    assert_eq!(offered.offered_at(&evidence_in_hand()), 1);
    let session = offered.pick(roland);
    session.assert_at_turn_menu(INV);
    let events = session.events();

    assert_event!(
        events,
        Event::ReactionOptionLapsed { code, reason: LapseReason::NoStateChange, investigator }
            if code.as_str() == EVIDENCE && *investigator == INV
    );
    assert_eq!(lapse_count(events, EVIDENCE), 1);
    // Roland's clue moved; Evidence! never resolved.
    assert_no_event!(events, Event::PlayCommenced { .. });
    assert_no_event!(events, Event::ResourcesPaid { .. });
    assert_event_count!(events, 1, Event::CluePlaced { .. });

    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.clues, 1);
    assert_eq!(inv.resources, 5, "Evidence! was never paid for");
    assert!(
        inv.hand.iter().any(|c| c.as_str() == EVIDENCE),
        "Evidence! stays in hand",
    );
    assert!(inv.discard.is_empty());
    assert_eq!(session.state().locations[&HERE].clues, 0);
}

#[test]
fn a_still_payable_second_evidence_is_not_withdrawn() {
    // The guard against over-withdrawing: with 2 resources both copies stay
    // payable, and with 2 clues both stay able to discover, so firing the first
    // must leave the second on offer. Same fixture as the wallet test, one
    // resource richer.
    let state = after_defeat_board(2, |inv| inv.hand([EVIDENCE, EVIDENCE]).resources(2));

    let first = play_one_of_two_evidence(defeat_the_enemy(state));
    // The second copy is still on offer.
    assert_eq!(first.offered_at(&evidence_in_hand()), 1);
    let session = first.pick(evidence_in_hand());
    session.assert_at_turn_menu(INV);

    assert_eq!(lapse_count(session.events(), EVIDENCE), 0);
    assert_eq!(evidence_plays(session.events()), 2);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.resources, 0, "2 - two costs of 1 = 0");
    assert_eq!(inv.clues, 2);
    assert!(!inv.hand.iter().any(|c| c.as_str() == EVIDENCE));
    assert_eq!(session.state().locations[&HERE].clues, 0);
}

#[test]
fn firing_a_candidate_whose_card_left_hand_rejects_instead_of_panicking() {
    // A hand candidate naming a card the investigator no
    // longer holds is the shape a sibling option leaves behind when it removes
    // that card from hand. The two prompt sites withdraw such an option
    // before it can be picked, so the window is opened for real with Evidence!
    // in hand and the card is then taken out of hand behind the prompt's back —
    // what is under test is the fire-time gate itself, reachable by a client
    // replaying a stale option id.
    //
    // The gate rejects rather than reaching an `unreachable!` in
    // `play_fast_event`, a panic that would take the session down.
    let opened = defeat_the_enemy(after_defeat_board(2, |inv| inv.hand([EVIDENCE])));
    assert_eq!(
        (
            opened.offered_at(&evidence_in_hand()),
            opened.prompt().options.len()
        ),
        (1, 1),
        "Evidence! alone is on offer: {:?}",
        opened.prompt(),
    );

    // No step can empty the hand while the prompt is open, so the stale option
    // is made by editing the state under it.
    let result = opened
        .edit_state(|state| {
            state
                .investigators
                .get_mut(&INV)
                .expect("seated")
                .hand
                .clear();
        })
        .pick(evidence_in_hand());

    let reason = result.expect_rejected();
    assert!(
        reason.contains("can no longer be initiated"),
        "rejected by the initiation gate: {reason}",
    );
    // Rejection leaves state and events untouched (the `apply_via` contract):
    // nothing paid, nothing played, no clue moved.
    let result = result.finish();
    assert_no_event!(result.events, Event::ResourcesPaid { .. });
    assert_no_event!(result.events, Event::PlayCommenced { .. });
    assert_eq!(result.state.investigators[&INV].resources, 5);
    assert_eq!(result.state.locations[&HERE].clues, 2);
}

#[test]
fn a_lone_evidence_still_opens_and_resolves_its_window() {
    // Regression floor: re-validation must not disturb the ordinary single-option
    // window. Same board as the wallet test with one copy and a full wallet.
    let opened = defeat_the_enemy(after_defeat_board(2, |inv| inv.hand([EVIDENCE])));
    assert_eq!(opened.offered_at(&evidence_in_hand()), 1);
    assert_eq!(lapse_count(opened.events(), EVIDENCE), 0);

    let session = opened.pick(evidence_in_hand());
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
    assert_eq!(session.state().locations[&HERE].clues, 1);
}

/// Roland Banks, seated from his printed card, solo at a 1-clue Study, with the
/// after-defeat window for an enemy he defeated open and offering his
/// reaction. Returns the settled state.
fn rolands_window_open() -> GameState {
    let opened = defeat_the_enemy(after_defeat_board(1, |inv| inv.as_card(ROLAND)));
    let rolands_card = OptionTarget::CardInstance(
        opened.state().investigators[&INV]
            .investigator_card
            .instance_id,
    );
    let offered: Vec<_> = opened
        .prompt()
        .options
        .iter()
        .map(|option| option.target.clone())
        .collect();
    assert_eq!(
        offered,
        vec![Some(rolands_card)],
        "the window offers Roland's reaction alone",
    );
    opened.state().clone()
}

/// Roland's reaction is *"(Limit once per round.)"*
/// (`glossary/Limits_and_Maximums.md`). If the limit is used up between the
/// offer and the pick, the option lapses, and the log says the limit is why.
#[test]
fn rolands_reaction_lapses_as_usage_limit_reached_once_its_limit_is_used() {
    let mut state = rolands_window_open();
    let now = UsageClock::of(&state);
    // His reaction is the first ability printed on his card (`roland_banks.rs`).
    state
        .investigators
        .get_mut(&INV)
        .expect("seated")
        .investigator_card
        .bump_ability_usage(0, INV, UsagePeriod::Round, now);

    let resurfaced = TestSession::new(state, TEST_REGISTRIES);

    assert_event!(
        resurfaced.events(),
        Event::ReactionOptionLapsed { code, reason: LapseReason::UsageLimitReached, investigator }
            if code.as_str() == ROLAND && *investigator == INV
    );
    assert_no_event!(resurfaced.events(), Event::CluePlaced { .. });
}

/// An investigator eliminated while a window is open initiates nothing
/// (`glossary/Elimination.md`), so his offered reaction lapses as not Active.
#[test]
fn rolands_reaction_lapses_as_not_active_once_he_is_eliminated() {
    let mut state = rolands_window_open();
    state.investigators.get_mut(&INV).expect("seated").status = Status::Defeated;

    let resurfaced = TestSession::new(state, TEST_REGISTRIES);

    assert_event!(
        resurfaced.events(),
        Event::ReactionOptionLapsed { code, reason: LapseReason::NotActive, investigator }
            if code.as_str() == ROLAND && *investigator == INV
    );
    assert_no_event!(resurfaced.events(), Event::CluePlaced { .. });
}
