//! The Mythos-phase draws that no printed card can reach: the surge chain, and
//! a Revelation that suspends **directly** into a choice.
//!
//! The rest of the group runs against real Core Set cards in
//! `mythos_phase.rs`. These three tests need card *shapes* the corpus does not
//! supply, so their probes are declared inline here (ADR 0016: a probe card is
//! test-local, and belongs in the file that reads it). The binary installs a
//! [`MockRegistry`] rather than `cards::REGISTRY`; the two cannot coexist in
//! one process, which is why this is its own file.
//!
//! # Why each probe is not a real card
//!
//! **The surge pair — provisional, terminal condition [#138].** The corpus
//! contains no Surge card *at all*, and not because the content lacks one:
//! `crates/card-data-pipeline/src/main.rs` hardcodes `surge: false` in both
//! arms that can carry it, an unparsed stub. The snapshot has 156 cards whose
//! printed text carries Surge, including False Lead 01136 in the Core set —
//! *"**Revelation** - If you have no clues, False Lead gains surge. …"*
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`; no rulings, per
//! `data/arkhamdb-faq/no-rulings.txt`). **When #138 lands, flip these two to a
//! real card unless the probe is better tailored to what the test asks** —
//! ADR 0016's substrate criterion, applied per test, not an automatic sweep.
//!
//! **The choice Revelation — not provisional.** No implemented card has a
//! top-level choice Revelation. The nearest is Crypt Chill 01167 —
//! *"**Revelation** - Test \[willpower\] (4). If you fail, choose and discard 1
//! asset you control (if you cannot, take 2 damage instead)."* — whose choice
//! sits *under* a skill test, and stripping that wrapper is the entire point of
//! the probe (#380: before the `EncounterCard` frame, a choice-only
//! Revelation's disposal was stranded, because only the skill-test driver's
//! teardown flushed `pending_revelation_discard`). A real card that reaches the
//! bug would have to be a card the game does not print.
//!
//! [#138]: https://github.com/talelburg/eldritch/issues/138
//! [MockRegistry]: game_core::test_support::MockRegistry

use game_core::action::RosterEntry;
use game_core::card_data::{CardKind, CardMetadata, CardType};
use game_core::dsl::{choose_one, gain_resources, revelation, Ability, InvestigatorTarget};
use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::seat_and_open;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{
    take_turn_action, test_location, GameStateBuilder, MockRegistry, TEST_INV,
};
use game_core::{
    assert_event, assert_event_sequence, Action, InputResponse, PlayerAction, TurnAction,
};

/// Probe: a treachery whose Revelation is a bare *"You gain 1 resource"* — the
/// engine primitive "a Revelation that resolves inline and discards", with
/// nothing else attached that a draw-ordering assertion could pick up.
const PROBE_TREACHERY: &str = "_mpp_treachery";

/// Probe: the same Revelation with `surge: true`. The load-bearing difference
/// is the metadata bit, which drives the re-draw in the per-card sub-sequence
/// (Rules Reference p.19, p.24 step 1.4.5). See the module docs for #138.
const PROBE_SURGE_TREACHERY: &str = "_mpp_surge_treachery";

/// Probe: a treachery whose Revelation is a top-level `Effect::ChooseOne`
/// (gain 2 vs. gain 5 resources) — a Revelation that suspends *directly* into a
/// choice rather than under a skill test.
const PROBE_CHOICE_TREACHERY: &str = "_mpp_choice_treachery";

/// The probe location the roster is seated at.
const PROBE_LOCATION: &str = "_mpp_location";

fn treachery_metadata(code: &str, name: &str, text: &str, surge: bool) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: name.to_owned(),
        text: Some(text.to_owned()),
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_mpp".to_owned(),
        weakness: false,
        kind: CardKind::Treachery {
            surge,
            peril: false,
            quantity: 1,
        },
    }
}

fn gain_one_revelation() -> Vec<Ability> {
    vec![revelation(gain_resources(InvestigatorTarget::You, 1))]
}

fn choice_revelation() -> Vec<Ability> {
    vec![revelation(choose_one([
        (
            "Gain 2 resources",
            gain_resources(InvestigatorTarget::You, 2),
        ),
        (
            "Gain 5 resources",
            gain_resources(InvestigatorTarget::You, 5),
        ),
    ]))]
}

#[ctor::ctor(unsafe)]
fn install_probe_registry() {
    MockRegistry::new()
        .with_card(treachery_metadata(
            PROBE_TREACHERY,
            "Probe Treachery",
            "Revelation - You gain 1 resource. (Probe; not a printed card.)",
            false,
        ))
        .with_abilities(PROBE_TREACHERY, gain_one_revelation)
        .with_card(treachery_metadata(
            PROBE_SURGE_TREACHERY,
            "Probe Surge Treachery",
            "Revelation - You gain 1 resource. Surge. (Probe; not a printed card.)",
            true,
        ))
        .with_abilities(PROBE_SURGE_TREACHERY, gain_one_revelation)
        .with_card(treachery_metadata(
            PROBE_CHOICE_TREACHERY,
            "Probe Choice Treachery",
            "Revelation - Choose one: gain 2 resources; or gain 5 resources. \
             (Probe; not a printed card.)",
            false,
        ))
        .with_abilities(PROBE_CHOICE_TREACHERY, choice_revelation)
        .install();
}

/// The board every test starts from: one probe location, set as the starting
/// location, and a +0 chaos bag. No investigators — callers supply the roster
/// to [`seat_and_open`]. No act or agenda deck: nothing here discovers a clue
/// or places doom.
fn board() -> GameState {
    let mut location = test_location(10, "Probe Location");
    location.code = CardCode::new(PROBE_LOCATION);

    let mut state = GameStateBuilder::new()
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    state.starting_location = Some(LocationId(10));
    state
}

/// Seed the encounter deck in draw order (top of deck = index 0), replacing
/// whatever is there. Where the order matters, call this *after*
/// `seat_and_open` — its setup shuffles the deck.
fn with_encounter_deck(state: &mut GameState, codes: &[&str]) {
    state.encounter_deck = codes.iter().map(|c| CardCode::new(*c)).collect();
}

/// Close one investigator's mulligan prompt, keeping the whole opening hand.
fn keep_hand(state: GameState) -> GameState {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state
}

fn roster(seats: usize) -> Vec<RosterEntry> {
    (0..seats)
        .map(|_| RosterEntry {
            investigator: CardCode::new(TEST_INV),
            deck: vec![],
        })
        .collect()
}

/// Seat one investigator, close the mulligan, and end their turn — which ticks
/// through Investigation → Enemy → Upkeep into Mythos (round 2), pausing with
/// the encounter-draw cursor on inv1.
fn setup_at_mythos_draw(state: GameState) -> GameState {
    let state = keep_hand(seat_and_open(state, &roster(1)).state);
    take_turn_action(state, &TurnAction::EndTurn).state
}

/// The two-investigator equivalent: both mulliganed, both turns ended — the
/// second `EndTurn` is the last in `turn_order`, so it ticks into Mythos.
fn setup_two_investigators_at_mythos_draw(state: GameState) -> GameState {
    let mut state = keep_hand(keep_hand(seat_and_open(state, &roster(2)).state));
    // inv1 ends turn → rotates to inv2.
    state = take_turn_action(state, &TurnAction::EndTurn).state;
    // inv2 is last in turn_order → auto-advances into Mythos.
    take_turn_action(state, &TurnAction::EndTurn).state
}

/// The `Confirm` that answers the step-1.4 encounter-draw prompt.
fn draw_encounter_card(state: GameState) -> game_core::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    )
}

// ------------------------------------------------------------------
// Surge chain
// ------------------------------------------------------------------

#[test]
fn mythos_phase_surge_chains_into_next_card() {
    let mut state = setup_at_mythos_draw(board());
    assert_eq!(state.phase, Phase::Mythos);

    // Seed the controlled draw order *after* seat_and_open's shuffle:
    // surge treachery on top, plain treachery below.
    with_encounter_deck(&mut state, &[PROBE_SURGE_TREACHERY, PROBE_TREACHERY]);

    let result = draw_encounter_card(state);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert!(
        result.state.encounter_deck.is_empty(),
        "both cards consumed by surge chain"
    );
    assert_eq!(
        result.state.encounter_discard.len(),
        2,
        "both treacheries must be in discard after surge chain",
    );
    assert_eq!(result.state.phase, Phase::Investigation);

    // Both cards were revealed; surge treachery first.
    assert_event_sequence!(
        result.events,
        Event::CardRevealed { code, .. }
            if *code == CardCode::new(PROBE_SURGE_TREACHERY),
        Event::CardRevealed { code, .. }
            if *code == CardCode::new(PROBE_TREACHERY),
    );
}

// ------------------------------------------------------------------
// Multi-investigator surge isolation
// ------------------------------------------------------------------

#[test]
#[allow(clippy::too_many_lines)] // end-to-end multi-investigator surge-isolation walkthrough
fn mythos_phase_multi_investigator_surge_does_not_spill() {
    // Verifies that a surge in inv1's draw chain resolves entirely within
    // inv1's DrawEncounterCard apply — consuming two cards from the shared
    // encounter deck — without disrupting inv2's subsequent draw.
    //
    // Encounter deck (top → bottom):
    //   [PROBE_SURGE_TREACHERY, PROBE_TREACHERY, PROBE_TREACHERY]
    //
    // Drive: seat_and_open → mulligans → EndTurn(inv1) → EndTurn(inv2)
    //        → DrawEncounterCard(inv1) → DrawEncounterCard(inv2)
    //
    // Expected:
    //   - inv1's DrawEncounterCard: draws the surge treachery, which
    //     triggers an immediate chain-draw of the next card (plain
    //     treachery). Both cards resolve, 2× CardRevealed, discard grows
    //     by 2. Still Mythos after; the cursor sits on inv2.
    //   - inv2's DrawEncounterCard: draws the third card (plain treachery),
    //     1× CardRevealed, discard grows by 1 more (3 total).
    //     Phase transitions to Investigation; the cursor clears.
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);

    let mut state = setup_two_investigators_at_mythos_draw(board());

    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.current_encounter_drawer(),
        Some(inv1),
        "inv1 draws first"
    );

    // Seed the controlled draw order *after* seat_and_open's shuffle:
    // surge on top, then two plain treacheries.
    with_encounter_deck(
        &mut state,
        &[PROBE_SURGE_TREACHERY, PROBE_TREACHERY, PROBE_TREACHERY],
    );

    // inv1 draws: surge chain pulls TWO cards (surge + plain treachery), then
    // the loop re-prompts inv2 (AwaitingInput).
    let result1 = draw_encounter_card(state);
    assert!(matches!(
        result1.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    // The surge chain resolves within inv1's single apply; still Mythos
    // because inv2 still needs to draw.
    assert_eq!(result1.state.phase, Phase::Mythos);
    assert_eq!(
        result1.state.current_encounter_drawer(),
        Some(inv2),
        "cursor advances to inv2 after inv1's chain completes"
    );
    assert_eq!(
        result1.state.encounter_discard.len(),
        2,
        "surge + plain treachery both discarded after inv1's chain"
    );
    assert_eq!(
        result1.state.encounter_deck.len(),
        1,
        "one card remains for inv2"
    );
    // Both cards emitted CardRevealed attributed to inv1.
    assert_event!(
        result1.events,
        Event::CardRevealed { investigator, code, .. }
            if *investigator == inv1
                && *code == CardCode::new(PROBE_SURGE_TREACHERY)
    );
    assert_event!(
        result1.events,
        Event::CardRevealed { investigator, code, .. }
            if *investigator == inv1
                && *code == CardCode::new(PROBE_TREACHERY)
    );

    // inv2 draws: one plain treachery, no surge.
    let result2 = draw_encounter_card(result1.state);
    assert!(matches!(
        result2.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result2.state.phase, Phase::Investigation);
    assert_eq!(result2.state.current_encounter_drawer(), None);
    assert!(result2.state.encounter_deck.is_empty());
    assert_eq!(
        result2.state.encounter_discard.len(),
        3,
        "all three cards in discard after both investigators draw"
    );
    assert_event!(
        result2.events,
        Event::CardRevealed { investigator, code, .. }
            if *investigator == inv2
                && *code == CardCode::new(PROBE_TREACHERY)
    );
}

// ------------------------------------------------------------------
// Revelation suspending directly into a choice (#380)
// ------------------------------------------------------------------

#[test]
fn revelation_suspending_into_a_choice_discards_after_the_pick() {
    let inv = InvestigatorId(1);
    let mut state = setup_at_mythos_draw(board());
    with_encounter_deck(&mut state, &[PROBE_CHOICE_TREACHERY]);

    // Confirm the draw → the Revelation's ChooseOne suspends for the pick.
    let drawn = draw_encounter_card(state);
    assert!(
        matches!(drawn.outcome, EngineOutcome::AwaitingInput { .. }),
        "the Revelation choice suspends, got {:?}",
        drawn.outcome
    );
    let res_before = drawn.state.investigators[&inv].resources;

    // Pick branch 0 (gain 2 resources). The choice resolves, and the framework
    // disposes of the treachery to encounter_discard.
    let resolved = apply(
        drawn.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert_eq!(
        resolved.state.investigators[&inv].resources,
        res_before + 2,
        "branch 0 granted 2 resources",
    );
    assert!(
        resolved
            .state
            .encounter_discard
            .contains(&CardCode::new(PROBE_CHOICE_TREACHERY)),
        "the treachery discards once its directly-suspended choice resolves",
    );
    // The card was revealed as a treachery on the way in.
    assert_event!(
        drawn.events,
        Event::CardRevealed { investigator, code, card_type }
            if *investigator == inv
                && *code == CardCode::new(PROBE_CHOICE_TREACHERY)
                && *card_type == CardType::Treachery
    );
}
