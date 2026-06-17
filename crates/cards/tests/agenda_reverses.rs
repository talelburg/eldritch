//! Integration: The Gathering's agenda reverses fire on advance, through
//! the real card registry (#281). Own process so it can install the
//! process-global registry against the real `cards` corpus.
//!
//! Drives the reverses via `fire_forced_on_agenda_advance` (the
//! `ForcedTriggerPoint::AgendaAdvanced` path) rather than a full Mythos
//! doom-to-threshold cascade — the firing wiring lives in `advance_agenda`
//! and is unit-tested there; here we prove the *card effects* resolve.

use std::sync::Once;

use game_core::state::{CardCode, EnemyId, InvestigatorId, LocationId};
use game_core::test_support::{
    fire_forced_on_agenda_advance, test_investigator, test_location, GameStateBuilder,
};
use game_core::{apply, Action, EngineOutcome, InputResponse, OptionId, PlayerAction};

static INSTALL: Once = Once::new();

fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// 01105's reverse is the lead's interactive `ChooseOne` (Axis A #334): it
/// suspends with a two-option prompt, and picking branch 1 (the printed
/// "lead takes 2 horror") resolves through `apply` + `ResolveInput`.
#[test]
fn agenda_01105_reverse_choice_lead_takes_two_horror() {
    install_registry();
    let lead = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([lead])
        .build();
    assert_eq!(state.investigators[&lead].horror, 0);

    // Firing the reverse suspends on the lead's choice (Choice frame pushed).
    let mut events = Vec::new();
    let outcome = fire_forced_on_agenda_advance(&mut state, &mut events, CardCode::new("01105"));
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "01105's reverse is a lead choice, not a deterministic effect: {outcome:?}",
    );

    // Pick branch 1 (option id 1): the lead takes 2 horror.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(1)),
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.investigators[&lead].horror, 2,
        "branch 1 deals 2 horror to the lead investigator",
    );
    assert!(
        result.state.continuations.is_empty(),
        "the choice frame is consumed",
    );
}

/// Picking branch 0 (each investigator discards 1 random card from hand)
/// moves one card from the lead's seeded hand into their discard.
#[test]
fn agenda_01105_reverse_choice_random_discard_each() {
    install_registry();
    let lead = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([lead])
        .build();
    // Seed a single known card into the lead's hand so the random discard is
    // deterministic (only one card to pick).
    state
        .investigators
        .get_mut(&lead)
        .expect("lead present")
        .hand
        .push(CardCode::new("01088")); // Emergency Cache (any hand-legal card)

    let mut events = Vec::new();
    let outcome = fire_forced_on_agenda_advance(&mut state, &mut events, CardCode::new("01105"));
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    let lead_inv = &result.state.investigators[&lead];
    assert!(lead_inv.hand.is_empty(), "the one hand card was discarded");
    assert_eq!(
        lead_inv.discard,
        vec![CardCode::new("01088")],
        "the discarded card landed in the discard pile",
    );
    assert_eq!(
        result.state.investigators[&lead].horror, 0,
        "no horror branch"
    );
}

/// 01106's reverse digs past non-Ghoul cards to the Ghoul enemy and the
/// lead draws (spawns) it. The "shuffle the discard into the deck" step
/// randomizes order, so the order-independent invariant is: the Ghoul is
/// always reached and drawn, and exactly the one non-Ghoul card remains
/// (still in the deck if the Ghoul sat above it, or in the discard if it
/// was dug through). Seeded deck = [Hunting Shadow (01135, treachery),
/// Ghoul Minion (01160, the Ghoul enemy)].
#[test]
fn agenda_01106_reverse_digs_until_a_ghoul_and_the_lead_draws_it() {
    install_registry();
    let lead = InvestigatorId(1);
    let loc = test_location(20, "Here");
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(20))
        .with_location(loc)
        .with_turn_order([lead])
        .build();
    state.encounter_deck.push_back(CardCode::new("01135")); // treachery (non-Ghoul)
    state.encounter_deck.push_back(CardCode::new("01160")); // Ghoul Minion

    let mut events = Vec::new();
    let outcome = fire_forced_on_agenda_advance(&mut state, &mut events, CardCode::new("01106"));
    assert_eq!(outcome, EngineOutcome::Done);

    // The Ghoul Minion was drawn → spawned into play (always reached,
    // whatever the shuffle order).
    let ghoul = state
        .enemies
        .values()
        .find(|e| e.code.as_str() == "01160")
        .expect("the dug-up Ghoul enemy (01160) spawned");
    assert_eq!(
        ghoul.current_location,
        Some(LocationId(20)),
        "Ghoul Minion (no spawn rule) spawns at the lead's location",
    );
    assert!(state
        .enemies
        .contains_key(&EnemyId(state.enemy_ids.peek() - 1)));
    // The Ghoul left both deck and discard (it was drawn into play).
    assert!(!state.encounter_deck.contains(&CardCode::new("01160")));
    assert!(!state.encounter_discard.contains(&CardCode::new("01160")));
    // Exactly the one non-Ghoul card remains somewhere (deck if the Ghoul
    // was above it, discard if it was dug through).
    assert_eq!(
        state.encounter_deck.len() + state.encounter_discard.len(),
        1,
        "deck={:?} discard={:?}",
        state.encounter_deck,
        state.encounter_discard,
    );
}

/// 01106's dig discards non-Ghoul cards. A one-card deck (no shuffle, since
/// `len < 2`) with a single non-Ghoul card: it is discarded, nothing is
/// drawn, and the reverse resolves to `Done` (deck exhausted with no Ghoul
/// — a faithful no-op draw).
#[test]
fn agenda_01106_reverse_discards_non_ghoul_cards() {
    install_registry();
    let lead = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([lead])
        .build();
    state.encounter_deck.push_back(CardCode::new("01135")); // treachery, no Ghoul

    let mut events = Vec::new();
    let outcome = fire_forced_on_agenda_advance(&mut state, &mut events, CardCode::new("01106"));
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.encounter_discard,
        vec![CardCode::new("01135")],
        "the non-Ghoul card is discarded",
    );
    assert!(state.encounter_deck.is_empty());
    assert!(
        state.enemies.is_empty(),
        "no Ghoul to draw → nothing spawns"
    );
}
