//! Clue discovery is **one** triggering condition, and the coordinator
//! resolves it between the `when` and `at` cells.
//!
//! `data/rules-reference/rules/glossary/Nested_Sequences.md`, verbatim:
//!
//! > Each time a triggering condition occurs, the following sequence is
//! > followed: 1) execute "when..." effects that interrupt that triggering
//! > condition, (2) resolve the triggering condition, and then, (3) execute
//! > "after..." effects in response to that triggering condition.
//!
//! No corpus card declares the `at` or `after` cell on a discovery, so those
//! two cells are proved with a mock registry (prior art: `timing_cells.rs`,
//! `advance_act_interactive_reverse.rs`). Cover Up 01007 — the one card that
//! *does* declare the `when` cell — is covered against the **real** corpus in
//! `cover_up.rs`.
//!
//! The cell order is the event order, so that is what these assert on: each
//! synthetic ability gains a distinct number of resources, and `CluePlaced`
//! marks the condition's own resolution.
//!
//! The count a replacement reads is **not** re-proved here — `discover_clue`
//! still caps it before emitting, and `cover_up.rs`'s Deduction 01039 pair
//! proves the cap end to end through two real cards.

use card_dsl::dsl::{self, Ability, Effect, EventPattern, EventTiming, InvestigatorTarget};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::event::{Event, LapseReason};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, Continuation, GameState, GameStateBuilder, InvestigatorId,
    LocationId,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};
use std::sync::LazyLock;

/// `when`-tagged reaction: +4 resources. Declaring interrupt timing on this
/// condition is accepted, because the coordinator owns it.
const WHEN: &str = "_cd_when";
/// `at`-tagged reaction: +1 resource.
const AT: &str = "_cd_at";
/// `after`-tagged reaction: +2 resources.
const AFTER: &str = "_cd_after";
/// `when`-tagged reaction that cancels the discovery — Cover Up 01007's shape
/// without Cover Up's discard.
const CANCEL: &str = "_cd_cancel";

/// A reaction in `timing`'s cell of the one condition under test, gaining
/// `amount` resources — the marker these tests read cell order off.
fn on_discovery(timing: EventTiming, amount: u8) -> Ability {
    dsl::reaction_on_event(
        EventPattern::DiscoverClues,
        timing,
        dsl::gain_resources(InvestigatorTarget::You, amount),
    )
}

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// The investigator's turn at a `location_clues`-clue Study with `codes` in
/// their threat area, 0 resources, and a single `Numeric(0)` chaos token so
/// the Intellect-3-vs-shroud-2 Investigate always succeeds and discovers
/// exactly 1 clue.
fn board_with(codes: &[&str], location_clues: u8) -> TestSession {
    let location = test_support::location(HERE).clues(location_clues);
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                .resources(0)
                .threat_area(codes.iter().copied()),
        )
        .with_location(location)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_rng_seed(1)
        .session()
}

/// Investigate, committing nothing, then fire the reaction of each card in
/// `fired`, in order, each in the window that offers it. After the last, the
/// investigation resolves back to the turn menu: an empty cell prompts for
/// nothing, so `fired` is itself an assertion about how many cells were
/// populated (any further window would stop the session short of the menu).
fn investigate_firing(session: TestSession, fired: &[&str]) -> TestSession {
    let mut session = session
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[]);
    for code in fired {
        let card = session.state().card(INV, *code);
        session = session.pick(card);
    }
    session.assert_at_turn_menu(INV);
    session
}

/// Every `ResourcesGained` amount and every `CluePlaced`, in emission order —
/// `Some(amount)` for a marker ability, `None` for the discovery itself.
fn timeline(events: &[Event]) -> Vec<Option<u8>> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } => Some(Some(*amount)),
            Event::CluePlaced { .. } => Some(None),
            _ => None,
        })
        .collect()
}

/// Assert the walk left nothing behind: no coordinator, timing point, window
/// or effect frame outlives the sequence. The idle `InvestigatorTurn` sentinel
/// stays on the stack for the whole turn and is not one.
fn assert_no_queued_frames(state: &GameState, why: &str) {
    assert!(
        !state
            .continuations
            .iter()
            .any(Continuation::is_queued_ability),
        "{why}: {:?}",
        state.continuations,
    );
}

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(WHEN, || vec![on_discovery(EventTiming::When, 4)])
            .with_abilities(AT, || vec![on_discovery(EventTiming::At, 1)])
            .with_abilities(AFTER, || vec![on_discovery(EventTiming::After, 2)])
            .with_abilities(CANCEL, || {
                vec![dsl::reaction_on_event(
                    EventPattern::DiscoverClues,
                    EventTiming::When,
                    Effect::Cancel,
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The headline claim: the discovery happens *between* the `when` and `at`
/// cells.
#[test]
fn the_discovery_resolves_between_the_when_and_at_cells() {
    let session = investigate_firing(board_with(&[WHEN, AT, AFTER], 2), &[WHEN, AT, AFTER]);
    assert_eq!(
        timeline(session.events()),
        vec![Some(4), None, Some(1), Some(2)],
        "expected when → discovery → at → after; events = {:?}",
        session.events()
    );
    assert_eq!(session.state().locations[&HERE].clues, 1, "location -1");
    assert_eq!(
        session.state().investigators[&INV].clues,
        1,
        "investigator +1"
    );
    assert_eq!(
        session.state().investigators[&INV].resources,
        7,
        "4 + 1 + 2"
    );
}

/// An after-discovery ability is declarable for the first time, and it resolves
/// once the clues have moved — `glossary/After.md` pins after as *"the moment
/// immediately after the specified timing point or triggering condition has
/// fully resolved"*.
#[test]
fn an_after_ability_resolves_once_the_clues_have_moved() {
    let session = investigate_firing(board_with(&[AFTER], 2), &[AFTER]);
    assert_eq!(
        timeline(session.events()),
        vec![None, Some(2)],
        "the discovery must precede the `after` ability; events = {:?}",
        session.events()
    );
    assert_no_queued_frames(
        session.state(),
        "the empty `when`/`at` cells left frames behind",
    );
}

/// The `at` cell sits between the two — `glossary/At.md`: *"These abilities
/// trigger in between any "when..." abilities and any "after..." abilities with
/// the same triggering condition."* — and, per ADR 0008's interpretation, after
/// the condition's impact lands.
#[test]
fn an_at_ability_resolves_after_the_clues_move_and_before_any_after_ability() {
    let session = investigate_firing(board_with(&[AT, AFTER], 2), &[AT, AFTER]);
    assert_eq!(
        timeline(session.events()),
        vec![None, Some(1), Some(2)],
        "expected discovery → at → after; events = {:?}",
        session.events()
    );
}

/// A prevented condition runs **none** of the rest of its sequence.
/// Dodge 01023's ruling (`data/arkhamdb-faq/core/01023.md`) settles it for the
/// sibling condition: *"If the attacking enemy has a **Forced** ability that
/// says "When attacks" or "After attacks", that ability does not trigger if an
/// attack is Dodged."* — both later cells, not just the resolve step. And
/// `glossary/Instead.md` reaches the same place for this condition: Cover Up
/// 01007's *"when you **would** discover"* changes the nature of the discovery,
/// and *"No further abilities referencing the original triggering condition may
/// be used."*
#[test]
fn a_cancelled_discovery_suppresses_the_at_and_after_cells() {
    // One window only: the `when` cell. The `at` and `after` cells never open,
    // and the session would stop short of the turn menu if they did.
    let session = investigate_firing(board_with(&[CANCEL, AT, AFTER], 2), &[CANCEL]);
    assert_eq!(
        timeline(session.events()),
        Vec::<Option<u8>>::new(),
        "neither the discovery nor any later cell may run; events = {:?}",
        session.events()
    );
    assert_eq!(
        session.state().locations[&HERE].clues,
        2,
        "location untouched"
    );
    assert_eq!(
        session.state().investigators[&INV].clues,
        0,
        "discovered nothing"
    );
    assert_eq!(
        session.state().investigators[&INV].resources,
        0,
        "no marker fired"
    );
    assert!(
        !session
            .state()
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EmitEvent(e) if e.prevented)),
        "the cancelled condition must leave the stack at its resolve step; stack = {:?}",
        session.state().continuations
    );
    assert_no_queued_frames(session.state(), "the abandoned sequence left frames behind");
}

/// Suppression reaches the rest of the **current** cell too, per the same Dodge
/// ruling: a `when` forced ability *"does not trigger if an attack is Dodged"*,
/// and the ability that dodges is itself a `when`-cell one. So the sibling
/// still pending in the open window is withdrawn rather than offered.
#[test]
fn a_cancel_withdraws_the_remaining_when_cell_abilities() {
    // The `when` cell opens with two candidates (CANCEL first, in threat-area
    // order); firing CANCEL withdraws WHEN rather than re-prompting, so one
    // pick is the whole drive.
    let session = investigate_firing(board_with(&[CANCEL, WHEN, AT, AFTER], 2), &[CANCEL]);
    assert_eq!(
        timeline(session.events()),
        Vec::<Option<u8>>::new(),
        "no sibling `when` ability may resolve after the cancel; events = {:?}",
        session.events()
    );
    assert_event!(
        session.events(),
        Event::ReactionOptionLapsed {
            code,
            reason: LapseReason::ConditionPrevented,
            ..
        } if *code == CardCode::new(WHEN)
    );
    assert_eq!(
        session.state().locations[&HERE].clues,
        2,
        "location untouched"
    );
    assert_eq!(
        session.state().investigators[&INV].resources,
        0,
        "no marker fired"
    );
}

/// A discovery of nothing is not a discovery: `discover_clue` returns before
/// the emit when the location holds no clues, so no cell runs at all.
#[test]
fn no_condition_is_emitted_when_the_location_has_no_clues() {
    let session = investigate_firing(board_with(&[AT, AFTER], 0), &[]);
    assert_eq!(
        timeline(session.events()),
        Vec::<Option<u8>>::new(),
        "no discovery, so no sequence; events = {:?}",
        session.events()
    );
    assert_eq!(session.state().investigators[&INV].resources, 0);
    assert_no_queued_frames(session.state(), "no frames left behind");
}
