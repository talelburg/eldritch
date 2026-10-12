//! Integration: The Gathering's agenda reverses fire on advance, through
//! the real card registry (#281). Own process so it can install the
//! process-global registry against the real `cards` corpus.
//!
//! Drives the reverses by firing the `AgendaAdvanced` timing point
//! (`TestSession::fire_at`) rather than a full Mythos
//! doom-to-threshold cascade — the firing wiring lives in `advance_agenda`
//! and is unit-tested there; here we prove the *card effects* resolve.

use cards::REGISTRY;
use game_core::engine::{ApplyResult, EngineOutcome, PromptNature, TimingEvent};
use game_core::event::Event;
use game_core::state::{
    CardCode, DiscardPile, EnemyId, GameStateBuilder, InvestigatorId, LocationId, Zone,
};
use game_core::test_support::{self, TestSession};

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// 01105's reverse is the lead's interactive `ChooseOne` (Axis A #334): it
/// suspends with a two-option prompt, and picking branch 1 (the printed
/// "lead takes 2 horror") resolves it.
#[test]
fn agenda_01105_reverse_choice_lead_takes_two_horror() {
    let lead = InvestigatorId(1);
    let inv = test_support::test_investigator(1);
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([lead])
        .build();
    assert_eq!(state.investigators[&lead].horror(), 0);

    // Firing the reverse suspends on the lead's choice.
    let session = TestSession::new(state).fire_at(TimingEvent::AgendaAdvanced {
        code: CardCode::new("01105"),
    });
    assert_eq!(
        session.prompt().nature,
        PromptNature::Decision,
        "01105's reverse is a lead choice, not a deterministic effect",
    );

    // Pick branch 1: the lead takes 2 horror, and nothing is left pending.
    let result = session.pick_nth(1).finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.investigators[&lead].horror(),
        2,
        "branch 1 deals 2 horror to the lead investigator",
    );
}

/// Picking branch 0 (each investigator discards 1 random card from hand)
/// moves one card from the lead's seeded hand into their discard.
#[test]
fn agenda_01105_reverse_choice_random_discard_each() {
    let lead = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
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

    let result = TestSession::new(state)
        .fire_at(TimingEvent::AgendaAdvanced {
            code: CardCode::new("01105"),
        })
        .pick_nth(0)
        .finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    let lead_inv = &result.state.investigators[&lead];
    assert!(lead_inv.hand.is_empty(), "the one hand card was discarded");
    assert_eq!(
        lead_inv.discard,
        vec![CardCode::new("01088")],
        "the discarded card landed in the discard pile",
    );
    assert_eq!(
        result.state.investigators[&lead].horror(),
        0,
        "no horror branch"
    );
}

/// 01106's reverse digs past non-Ghoul cards to the Ghoul enemy and the
/// lead draws (spawns) it. The "shuffle the discard into the deck" step
/// randomizes order, so the order-independent invariant is: the Ghoul is
/// always reached and drawn, both non-Ghoul cards remain (in the deck if the
/// Ghoul sat above them, in the discard if they were dug through), and each
/// one dug through announces its own discard, in dig order. Seeded deck =
/// [Hunting Shadow (01135), False Lead (01136), Ghoul Minion (01160, the
/// Ghoul enemy)].
#[test]
fn agenda_01106_reverse_digs_until_a_ghoul_and_the_lead_draws_it() {
    let lead = InvestigatorId(1);
    let loc = test_support::test_location(20, "Here");
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), LocationId(20))
        .with_location(loc)
        .with_turn_order([lead])
        .build();
    state.encounter_deck.push_back(CardCode::new("01135")); // treachery (non-Ghoul)
    state.encounter_deck.push_back(CardCode::new("01136")); // treachery (non-Ghoul)
    state.encounter_deck.push_back(CardCode::new("01160")); // Ghoul Minion

    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = TestSession::new(state)
        .fire_at(TimingEvent::AgendaAdvanced {
            code: CardCode::new("01106"),
        })
        .finish();
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
    // Both non-Ghoul cards remain somewhere (deck if the Ghoul was above
    // them, discard if they were dug through).
    assert_eq!(
        state.encounter_deck.len() + state.encounter_discard.len(),
        2,
        "deck={:?} discard={:?}",
        state.encounter_deck,
        state.encounter_discard,
    );
    // One discard event per miss, in dig order.
    assert_eq!(encounter_deck_discards(&events), state.encounter_discard);
}

/// The codes of every `CardDiscarded` from the encounter deck to the
/// encounter discard, in emission order.
fn encounter_deck_discards(events: &[Event]) -> Vec<CardCode> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::CardDiscarded {
                code,
                from: Zone::EncounterDeck,
                to: DiscardPile::Encounter,
            } => Some(code.clone()),
            _ => None,
        })
        .collect()
}

/// 01106's dig discards non-Ghoul cards. A deck of two non-Ghoul cards: both
/// are discarded, each with its own discard event in dig order, nothing is
/// drawn, and the reverse resolves to `Done` (deck exhausted with no Ghoul
/// — a faithful no-op draw). The reshuffle randomizes which miss comes
/// first, so the order is read off the discard pile.
#[test]
fn agenda_01106_reverse_discards_non_ghoul_cards() {
    let lead = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([lead])
        .build();
    state.encounter_deck.push_back(CardCode::new("01135")); // treachery, no Ghoul
    state.encounter_deck.push_back(CardCode::new("01136")); // treachery, no Ghoul

    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = TestSession::new(state)
        .fire_at(TimingEvent::AgendaAdvanced {
            code: CardCode::new("01106"),
        })
        .finish();
    assert_eq!(outcome, EngineOutcome::Done);
    let mut discarded = state.encounter_discard.clone();
    discarded.sort_unstable();
    assert_eq!(
        discarded,
        vec![CardCode::new("01135"), CardCode::new("01136")],
        "both non-Ghoul cards are discarded",
    );
    assert_eq!(
        encounter_deck_discards(&events),
        state.encounter_discard,
        "one discard event per miss, in dig order",
    );
    assert!(state.encounter_deck.is_empty());
    assert!(
        state.enemies.is_empty(),
        "no Ghoul to draw → nothing spawns"
    );
}
