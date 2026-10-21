//! Integration: The Gathering's agenda reverses fire on advance, through
//! the real card registry.
//!
//! Drives the reverses through the real advance: the round ends one doom
//! short of the agenda's threshold, so the next Mythos phase's step-1.2 doom
//! advances it and its reverse fires. The session then rests at that Mythos
//! phase's step-1.4 encounter draw, which is where the assertions read the
//! reverse's effect.

use cards::TEST_REGISTRIES;
use game_core::engine::PromptNature;
use game_core::event::Event;
use game_core::state::{
    Agenda, CardCode, DiscardPile, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    LocationId, Zone,
};
use game_core::test_support::{self, seat, Seat, TestSession};

const LEAD: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

/// What's Going On?! (01105), doom 3, then Rise of the Ghouls (01106), doom
/// 7, then They're Getting Out! (01107), doom 10 — The Gathering's agenda
/// deck (`data/arkhamdb-snapshot/pack/core/core_encounter.json`).
const AGENDAS: [(&str, u8); 3] = [("01105", 3), ("01106", 7), ("01107", 10)];

/// The lead seated as `lead` at the Study, at the end of the Upkeep phase —
/// after step 4.4's draw, so no card is drawn into hand — with agenda
/// `AGENDAS[index]` current and one doom short of its threshold, and
/// `encounter_deck` on the encounter deck, top first.
fn ending_the_round_on(index: usize, lead: Seat, encounter_deck: &[&str]) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_seat(lead.at(HERE))
        .with_turn_order([LEAD])
        .ending_upkeep_phase()
        .build();
    state.agenda_deck = AGENDAS
        .iter()
        .map(|&(code, doom_threshold)| Agenda {
            code: CardCode::new(code),
            doom_threshold,
        })
        .collect();
    state.agenda_index = index;
    state.agenda_doom = AGENDAS[index].1 - 1;
    for code in encounter_deck {
        state.encounter_deck.push_back(CardCode::new(*code));
    }
    state
}

/// 01105's reverse is an interactive `ChooseOne`, made by the first-seat proxy:
/// it suspends with a two-option prompt, and picking branch 1 (the printed
/// "the lead investigator takes 2 horror") resolves it.
#[test]
fn agenda_01105_reverse_choice_lead_takes_two_horror() {
    let state = ending_the_round_on(0, seat(1), &[]);
    assert_eq!(state.investigators[&LEAD].horror(), 0);

    // Advancing fires the reverse, which suspends on the lead's choice.
    let session = TestSession::new(state, TEST_REGISTRIES);
    assert_eq!(
        session.prompt().nature,
        PromptNature::Decision,
        "01105's reverse is a lead choice, not a deterministic effect",
    );

    // Pick branch 1: the lead takes 2 horror, and nothing is left pending.
    let session = session.pick_nth(1);
    session.assert_at_encounter_draw();
    assert_eq!(
        session.state().investigators[&LEAD].horror(),
        2,
        "branch 1 deals 2 horror to the first-seat proxy",
    );
}

/// Picking branch 0 (each investigator discards 1 random card from hand)
/// moves one card from the lead's seeded hand into their discard.
#[test]
fn agenda_01105_reverse_choice_random_discard_each() {
    // Seed a single known card into the lead's hand so the random discard is
    // deterministic (only one card to pick): Emergency Cache, any hand-legal
    // card.
    let state = ending_the_round_on(0, seat(1).hand(["01088"]), &[]);

    let session = TestSession::new(state, TEST_REGISTRIES).pick_nth(0);
    session.assert_at_encounter_draw();
    let lead_inv = &session.state().investigators[&LEAD];
    assert!(lead_inv.hand.is_empty(), "the one hand card was discarded");
    assert_eq!(
        lead_inv.discard,
        vec![CardCode::new("01088")],
        "the discarded card landed in the discard pile",
    );
    assert_eq!(lead_inv.horror(), 0, "no horror branch");
}

/// 01106's reverse digs past non-Ghoul cards to the Ghoul enemy and the
/// lead draws (spawns) it. The "Shuffle the encounter discard pile into the
/// encounter deck." step randomizes order, so the order-independent
/// invariant is: the Ghoul is always reached and drawn, both non-Ghoul cards
/// remain (in the deck if the Ghoul sat above them, in the discard if they
/// were dug through), and each one dug through announces its own discard, in
/// dig order. Seeded deck =
/// [Hunting Shadow (01135), False Lead (01136), Ghoul Minion (01160, the
/// Ghoul enemy)].
#[test]
fn agenda_01106_reverse_digs_until_a_ghoul_and_the_lead_draws_it() {
    let session = TestSession::new(
        ending_the_round_on(
            1,
            seat(1),
            &[
                "01135", // treachery (non-Ghoul)
                "01136", // treachery (non-Ghoul)
                "01160", // Ghoul Minion
            ],
        ),
        TEST_REGISTRIES,
    );
    session.assert_at_encounter_draw();
    let state = session.state();
    let events = session.events();

    // The Ghoul Minion was drawn → spawned into play (always reached,
    // whatever the shuffle order).
    let ghoul = state
        .enemies
        .values()
        .find(|e| e.code.as_str() == "01160")
        .expect("the dug-up Ghoul enemy (01160) spawned");
    assert_eq!(
        ghoul.current_location,
        Some(HERE),
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
    assert_eq!(encounter_deck_discards(events), state.encounter_discard);
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
/// drawn, and the reverse resolves (deck exhausted with no Ghoul — a faithful
/// no-op draw), Mythos going on to its step-1.4 draw. The reshuffle randomizes which miss comes
/// first, so the order is read off the discard pile.
#[test]
fn agenda_01106_reverse_discards_non_ghoul_cards() {
    let session = TestSession::new(
        ending_the_round_on(
            1,
            seat(1),
            &[
                "01135", // treachery, no Ghoul
                "01136", // treachery, no Ghoul
            ],
        ),
        TEST_REGISTRIES,
    );
    session.assert_at_encounter_draw();
    let state = session.state();
    let events = session.events();
    let mut discarded = state.encounter_discard.clone();
    discarded.sort_unstable();
    assert_eq!(
        discarded,
        vec![CardCode::new("01135"), CardCode::new("01136")],
        "both non-Ghoul cards are discarded",
    );
    assert_eq!(
        encounter_deck_discards(events),
        state.encounter_discard,
        "one discard event per miss, in dig order",
    );
    assert!(state.encounter_deck.is_empty());
    assert!(
        state.enemies.is_empty(),
        "no Ghoul to draw → nothing spawns"
    );
}
