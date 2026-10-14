//! #702: **every** triggering condition walks the `when → at → after` sequence,
//! not just the round end.
//!
//! `data/rules-reference/rules/glossary/Nested_Sequences.md`, verbatim:
//!
//! > Each time a triggering condition occurs, the following sequence is
//! > followed: 1) execute "when..." effects that interrupt that triggering
//! > condition, (2) resolve the triggering condition, and then, (3) execute
//! > "after..." effects in response to that triggering condition.
//!
//! `data/rules-reference/rules/glossary/At.md` (identical in `glossary/If.md`)
//! adds the middle cell:
//!
//! > Some abilities have triggering conditions that use the words "at" or "if"
//! > instead of specifying "when" or "after," such as "at the end of the round,"
//! > or "if the Ghoul Priest is defeated." These abilities trigger in between any
//! > "when..." abilities and any "after..." abilities with the same triggering
//! > condition.
//!
//! Two conditions have their cells *contested* by the corpus, and both are
//! covered against the real corpus rather than here. The **round end** —
//! act 01109's `when`, agenda 01107's and Dissonant Voices 01165's `at` — in
//! `theyre_getting_out.rs` and `the_barrier_eligibility.rs`. The **enemy
//! defeat** — act 01110's `at` objective against Roland Banks 01001's
//! *"`[reaction]` After you defeat an enemy: Discover 1 clue at your
//! location."* — in `scenarios/tests/the_gathering_resolutions.rs`, whose C7b
//! gate defeats the real Ghoul Priest and asserts Roland's window never opens,
//! per 01110's ruling that the objective fires *"before any "After you defeat
//! an enemy" reactions can be used."* None of those must move.
//!
//! The corpus's two remaining `at`-tagged abilities — Frozen in Fear 01164's
//! end-of-turn test, agenda 01107's enemy-phase-end move — are alone on their
//! condition, so nothing orders against them and their own module tests pin
//! the declaration and nothing more. So the walk itself is proved with a
//! mock registry (prior art: `advance_act_interactive_reverse`).
//!
//! The condition under test is `SkillTestResolved` (RR ST.6), picked because
//! the trigger scan's board walk reaches a card in the investigator's threat
//! area for forced and reaction abilities alike, so one synthetic card there is
//! the whole fixture.
//! Since #773 it is **coordinator-owned** — a bare milestone, since a
//! determination mutates nothing and ST.7 applies the results on the re-exposed
//! `SkillTest` frame — so all three of its cells are walked here. The
//! caller-owned `when`-cell *reject* moved with it, and lives on a condition
//! that has not migrated (`PhaseEnded { Upkeep }`, in
//! `game-core/tests/timing_resolve_step.rs`).

use card_dsl::dsl::{
    self, Ability, Effect, EventPattern, EventTiming, InvestigatorTarget, TestOutcome,
    TestedLocationScope,
};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, MockRegistry, SeatLookup as _, TestSession};
use std::sync::LazyLock;

/// `at`-tagged forced: +1 resource.
const AT: &str = "_tc_at";
/// `after`-tagged forced: +2 resources.
const AFTER: &str = "_tc_after";
/// `when`-tagged forced: +4 resources. Reachable since #773 made
/// `SkillTestResolved` coordinator-owned; before that it was rejected.
const WHEN: &str = "_tc_when";
/// `at`-tagged *reaction*: +7 resources.
const REACT: &str = "_tc_react";
/// `at`-tagged forced that discards itself, plus an `after`-tagged forced on the
/// same card — the `after` cell's fresh re-scan must no longer find it.
const RESCAN: &str = "_tc_rescan";

/// The one triggering condition every ability in this file keys off: "you
/// succeeded at a skill test", unnarrowed by kind. Only the timing cell differs.
fn succeeded() -> EventPattern {
    EventPattern::SkillTestResolved {
        outcome: TestOutcome::Success,
        kind: None,
        by_controller: true,
        tested_location: TestedLocationScope::Any,
    }
}

/// A forced ability in `timing`'s cell that gains `amount` resources — the
/// marker these tests read cell order off.
fn on_success(timing: EventTiming, amount: u8) -> Ability {
    dsl::forced_on_event(
        succeeded(),
        timing,
        dsl::gain_resources(InvestigatorTarget::You, amount),
    )
}

const INV: InvestigatorId = InvestigatorId(1);

/// `INV`'s open turn, with `codes` in their threat area (the zone
/// `Effect::DiscardSelf` can remove from), 0 resources, and a single
/// `Numeric(0)` chaos bag so a difficulty-0 test succeeds deterministically.
fn board_with(codes: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).resources(0).threat_area(codes.iter().copied()))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
}

/// A difficulty-0 Intellect test on `board`, committing nothing. The session
/// rests at the first prompt past the commit window, or back at the turn menu.
fn succeed_on(board: TestSession) -> TestSession {
    board.test_skill(INV, SkillKind::Intellect, 0).commit(&[])
}

/// [`succeed_on`] the [`board_with`] `codes`, with nothing but forced abilities
/// to resolve, so play comes back to the turn menu.
fn succeed_with(codes: &[&str]) -> TestSession {
    let session = succeed_on(board_with(codes));
    session.assert_at_turn_menu(INV);
    session
}

/// Every `ResourcesGained` amount, in the order the engine emitted it — the
/// cell order *is* the event order, which is what these tests assert on.
fn gains(events: &[Event]) -> Vec<u8> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect()
}

/// Whether the test's determination — RR ST.6, logged as `SkillTestSucceeded`
/// — precedes the first ability the sequence fired. True of every cell,
/// including `when`: the verdict is stashed and logged *before* the condition
/// is emitted at all, which is why a `when` ability on this condition reads a
/// resolved test.
///
/// This is therefore not the *impact* ordering. The condition's impact is ST.7
/// — *"Apply skill test results"* — which a bare plain test has none of, so the
/// `when`-before-impact claim is pinned where it is observable: Lita Chantler
/// 01117's `crates/cards/tests/lita_chantler.rs`, where the `when` cell raises
/// an attack's damage before ST.7 deals it.
fn resolved_before_first_gain(events: &[Event]) -> bool {
    let position = |pred: fn(&Event) -> bool| events.iter().position(pred);
    let resolved = position(|e| matches!(e, Event::SkillTestSucceeded { .. }));
    let first_gain = position(|e| matches!(e, Event::ResourcesGained { .. }));
    match (resolved, first_gain) {
        (Some(r), Some(g)) => r < g,
        _ => false,
    }
}

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(AT, || vec![on_success(EventTiming::At, 1)])
            .with_abilities(AFTER, || vec![on_success(EventTiming::After, 2)])
            .with_abilities(WHEN, || vec![on_success(EventTiming::When, 4)])
            .with_abilities(REACT, || {
                vec![dsl::reaction_on_event(
                    succeeded(),
                    EventTiming::At,
                    dsl::gain_resources(InvestigatorTarget::You, 7),
                )]
            })
            .with_abilities(RESCAN, || {
                vec![
                    dsl::forced_on_event(
                        succeeded(),
                        EventTiming::At,
                        Effect::Seq(vec![
                            dsl::gain_resources(InvestigatorTarget::You, 1),
                            Effect::DiscardSelf,
                        ]),
                    ),
                    on_success(EventTiming::After, 2),
                ]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The headline claim: on a condition that is *not* the round end, an
/// `at`-tagged forced ability resolves, and it resolves before the `after`
/// cell. Before #702 the `at` cell was unreachable off the round end and the
/// +1 never fired at all.
#[test]
fn an_at_tagged_forced_resolves_before_the_after_cell() {
    let session = succeed_with(&[AT, AFTER]);
    let events = session.events();
    assert_eq!(
        gains(events),
        vec![1, 2],
        "the `at` cell must resolve before the `after` cell; events = {events:?}",
    );
    assert!(
        resolved_before_first_gain(events),
        "both cells sit past the resolve step, so the test's own resolution must be \
         logged first; events = {events:?}",
    );
    assert_eq!(session.state().investigators[&INV].resources, 3);
}

/// An `after`-tagged ability keeps resolving after the condition's impact, and
/// the two cells it does not occupy are skipped without a prompt: the test runs
/// back to the turn menu with exactly one ability resolved.
///
/// The impact of "you succeeded at a skill test" is the test's own resolution,
/// logged as `SkillTestSucceeded` — `glossary/After.md` pins after as *"the
/// moment immediately after the specified timing point or triggering condition
/// has fully resolved"* — so the assertion is that the gain follows that event,
/// not merely that it happened.
#[test]
fn an_empty_cell_is_skipped_without_prompting() {
    let board = board_with(&[AFTER]);
    let open_turn = board.state().continuations.clone();
    let session = succeed_on(board);
    session.assert_at_turn_menu(INV);
    let events = session.events();
    assert_eq!(gains(events), vec![2], "events = {events:?}");
    assert!(
        resolved_before_first_gain(events),
        "the `after` cell resolved before the condition's impact landed; events = {events:?}",
    );
    // A stack invariant, so read off the stack: nothing is left above the open
    // turn the test started from.
    assert_eq!(
        session.state().continuations,
        open_turn,
        "the empty `when`/`at` cells left frames behind",
    );
}

/// `data/rules-reference/rules/glossary/Ability.md`: *"For any given timing
/// point, all forced abilities initiated in reference to that timing point must
/// resolve before any \[reaction\] abilities ... referencing the same timing
/// point in the same manner may be initiated."* Within one cell, the forced +1
/// lands before the reaction is even offered.
#[test]
fn forced_resolves_before_reaction_within_one_cell() {
    let offered = succeed_on(board_with(&[AT, REACT]));
    // The reaction is offered with the forced +1 already resolved.
    assert_eq!(gains(offered.events()), vec![1]);
    let react = offered.state().card(INV, REACT);
    let session = offered.pick(react);
    session.assert_at_turn_menu(INV);
    assert_eq!(
        gains(session.events()),
        vec![1, 7],
        "the forced +1 must resolve before the reaction +7 is offered; events = {:?}",
        session.events()
    );
}

/// Each cell is scanned fresh, so a change the previous cell made is visible to
/// the next. The `at` cell discards the card that carries the `after` ability;
/// the `after` cell's own re-scan therefore finds nothing. A pre-computed grid
/// would still fire the +2.
#[test]
fn each_cell_is_scanned_fresh() {
    let session = succeed_with(&[RESCAN]);
    assert_eq!(
        gains(session.events()),
        vec![1],
        "the `after` cell re-scanned and must not have found the discarded card; events = {:?}",
        session.events()
    );
    assert!(
        session.state().investigators[&INV].threat_area.is_empty(),
        "the `at` cell's DiscardSelf did not take effect"
    );
}

/// **All three cells, in order.** Until #773 this condition was caller-owned
/// and the `when` card here was *rejected* rather than resolved; migrating it
/// to a coordinator-owned bare milestone opened the cell, and the sequence is
/// now the whole of `glossary/Nested_Sequences.md`'s walk.
///
/// The reject itself is still live scaffolding and still covered — on
/// `PhaseEnded { Upkeep }`, which has not migrated, in
/// `game-core/tests/timing_resolve_step.rs`.
#[test]
fn every_cell_resolves_in_order_when_at_after() {
    let session = succeed_with(&[AFTER, WHEN, AT]);
    assert_eq!(
        gains(session.events()),
        vec![4, 1, 2],
        "declaration order is after/when/at and resolution order must be \
         when/at/after regardless; events = {:?}",
        session.events()
    );
    assert_eq!(session.state().investigators[&INV].resources, 7);
}
