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
//! The round end is the only condition whose cells the corpus *contests*: the
//! three other `at`-tagged corpus abilities — act 01110's objective, Frozen in
//! Fear 01164's end-of-turn test, agenda 01107's enemy-phase-end move — are
//! each alone on their condition, so no card orders against them and their own
//! module tests pin the declaration and nothing more. So the walk is proved
//! with a hand-built registry (prior art: `advance_act_interactive_reverse`).
//! The round end's own three consumers — act 01109, agenda 01107, Dissonant
//! Voices 01165 — are covered against the *real* corpus in
//! `theyre_getting_out.rs` and `the_barrier_eligibility.rs`, which must not
//! move.
//!
//! The condition under test is `SkillTestResolved` (RR ST.6), picked because
//! both its forced and its reaction scan read the investigator's own controlled
//! instances, so one synthetic card in the threat area is the whole fixture.

use card_dsl::dsl::{
    forced_on_event, gain_resources, reaction_on_event, Ability, Effect, EventPattern, EventTiming,
    InvestigatorTarget, TestOutcome,
};
use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::engine::EngineOutcome;
use game_core::engine::OptionId;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameState, InvestigatorId, Phase,
    SkillKind, TokenModifiers,
};
use game_core::test_support::{
    drive_skill_test, perform_skill_test_no_commits, test_investigator, GameStateBuilder,
    ScriptedResolver,
};

/// `at`-tagged forced: +1 resource.
const AT: &str = "_tc_at";
/// `after`-tagged forced: +2 resources.
const AFTER: &str = "_tc_after";
/// `when`-tagged forced — declared on a caller-owned condition, so it is
/// rejected rather than silently dropped.
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
    }
}

/// A forced ability in `timing`'s cell that gains `amount` resources — the
/// marker these tests read cell order off.
fn on_success(timing: EventTiming, amount: u8) -> Ability {
    forced_on_event(
        succeeded(),
        timing,
        gain_resources(InvestigatorTarget::You, amount),
    )
}

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        AT => Some(vec![on_success(EventTiming::At, 1)]),
        AFTER => Some(vec![on_success(EventTiming::After, 2)]),
        WHEN => Some(vec![on_success(EventTiming::When, 4)]),
        REACT => Some(vec![reaction_on_event(
            succeeded(),
            EventTiming::At,
            gain_resources(InvestigatorTarget::You, 7),
        )]),
        RESCAN => Some(vec![
            forced_on_event(
                succeeded(),
                EventTiming::At,
                Effect::Seq(vec![
                    gain_resources(InvestigatorTarget::You, 1),
                    Effect::DiscardSelf,
                ]),
            ),
            on_success(EventTiming::After, 2),
        ]),
        _ => None,
    }
}

fn metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for,
        abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
        native_condition_for: |_| None,
    });
}

const INV: InvestigatorId = InvestigatorId(1);

/// One investigator with `codes` in their threat area (the zone
/// `Effect::DiscardSelf` can remove from), 0 resources, and a single
/// `Numeric(0)` chaos bag so a difficulty-0 test succeeds deterministically.
fn board_with(codes: &[&str]) -> GameState {
    let mut inv = test_investigator(1);
    inv.resources = 0;
    for (i, code) in codes.iter().enumerate() {
        inv.threat_area.push(CardInPlay::enter_play(
            CardCode::new(*code),
            CardInstanceId(u32::try_from(i).expect("fixture card count fits u32")),
        ));
    }
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
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

/// Whether the condition's own impact — the skill test resolving, logged as
/// `SkillTestSucceeded` — precedes the first ability the sequence fired. Every
/// cell in these tests is `at` or `after`, both of which sit past the resolve
/// step, so this must hold for all of them.
fn resolved_before_first_gain(events: &[Event]) -> bool {
    let position = |pred: fn(&Event) -> bool| events.iter().position(pred);
    let resolved = position(|e| matches!(e, Event::SkillTestSucceeded { .. }));
    let first_gain = position(|e| matches!(e, Event::ResourcesGained { .. }));
    match (resolved, first_gain) {
        (Some(r), Some(g)) => r < g,
        _ => false,
    }
}

/// The headline claim: on a condition that is *not* the round end, an
/// `at`-tagged forced ability resolves, and it resolves before the `after`
/// cell. Before #702 the `at` cell was unreachable off the round end and the
/// +1 never fired at all.
#[test]
fn an_at_tagged_forced_resolves_before_the_after_cell() {
    let r = perform_skill_test_no_commits(board_with(&[AT, AFTER]), INV, SkillKind::Intellect, 0);
    assert_eq!(
        gains(&r.events),
        vec![1, 2],
        "the `at` cell must resolve before the `after` cell; events = {:?}",
        r.events
    );
    assert!(
        resolved_before_first_gain(&r.events),
        "both cells sit past the resolve step, so the test's own resolution must be \
         logged first; events = {:?}",
        r.events
    );
    assert_eq!(r.state.investigators[&INV].resources, 3);
}

/// An `after`-tagged ability keeps resolving after the condition's impact, and
/// the two cells it does not occupy are skipped without a prompt: the drive
/// reaches its terminal outcome with exactly one ability resolved.
///
/// The impact of "you succeeded at a skill test" is the test's own resolution,
/// logged as `SkillTestSucceeded` — `glossary/After.md` pins after as *"the
/// moment immediately after the specified timing point or triggering condition
/// has fully resolved"* — so the assertion is that the gain follows that event,
/// not merely that it happened.
#[test]
fn an_empty_cell_is_skipped_without_prompting() {
    let r = perform_skill_test_no_commits(board_with(&[AFTER]), INV, SkillKind::Intellect, 0);
    assert_eq!(gains(&r.events), vec![2], "events = {:?}", r.events);
    assert!(
        resolved_before_first_gain(&r.events),
        "the `after` cell resolved before the condition's impact landed; events = {:?}",
        r.events
    );
    assert!(
        r.state.continuations.is_empty(),
        "the empty `when`/`at` cells left frames behind: {:?}",
        r.state.continuations
    );
}

/// `data/rules-reference/rules/glossary/Ability.md`: *"For any given timing
/// point, all forced abilities initiated in reference to that timing point must
/// resolve before any \[reaction\] abilities ... referencing the same timing
/// point in the same manner may be initiated."* Within one cell, the forced +1
/// lands before the reaction is even offered.
#[test]
fn forced_resolves_before_reaction_within_one_cell() {
    let mut script = ScriptedResolver::new();
    script.commit_cards(&[]).pick_single(OptionId(0));
    let r = drive_skill_test(
        board_with(&[AT, REACT]),
        INV,
        SkillKind::Intellect,
        0,
        script,
    );
    assert_eq!(
        gains(&r.events),
        vec![1, 7],
        "the forced +1 must resolve before the reaction +7 is offered; events = {:?}",
        r.events
    );
}

/// Each cell is scanned fresh, so a change the previous cell made is visible to
/// the next. The `at` cell discards the card that carries the `after` ability;
/// the `after` cell's own re-scan therefore finds nothing. A pre-computed grid
/// would still fire the +2.
#[test]
fn each_cell_is_scanned_fresh() {
    let r = perform_skill_test_no_commits(board_with(&[RESCAN]), INV, SkillKind::Intellect, 0);
    assert_eq!(
        gains(&r.events),
        vec![1],
        "the `after` cell re-scanned and must not have found the discarded card; events = {:?}",
        r.events
    );
    assert!(
        r.state.investigators[&INV].threat_area.is_empty(),
        "the `at` cell's DiscardSelf did not take effect"
    );
}

/// The caller-owned `when`-cell reject, now reachable: before #702 only the
/// round end walked its cells, and the round end is coordinator-owned, so no
/// ability could ever hit this path. An interrupt declared on a caller-owned
/// condition fails loudly rather than resolving in the wrong cell — the
/// scaffolding ADR 0008 describes, deleted when the last condition migrates.
#[test]
fn an_interrupt_on_a_caller_owned_condition_is_rejected() {
    let r = perform_skill_test_no_commits(board_with(&[WHEN]), INV, SkillKind::Intellect, 0);
    let EngineOutcome::Rejected { reason } = &r.outcome else {
        panic!("expected a rejection, got {:?}", r.outcome);
    };
    assert!(
        reason.contains("caller-owned") && reason.contains("0008"),
        "the reason must name the condition's ownership and the ADR: {reason}"
    );
    assert_eq!(
        gains(&r.events),
        Vec::<u8>::new(),
        "a rejected apply leaves no events behind"
    );
}
