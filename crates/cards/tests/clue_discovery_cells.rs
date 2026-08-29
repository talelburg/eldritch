//! #703: clue discovery is **one** triggering condition, and the coordinator
//! resolves it between the `when` and `at` cells.
//!
//! `data/rules-reference/rules/glossary/Nested_Sequences.md`, verbatim:
//!
//! > Each time a triggering condition occurs, the following sequence is
//! > followed: 1) execute "when..." effects that interrupt that triggering
//! > condition, (2) resolve the triggering condition, and then, (3) execute
//! > "after..." effects in response to that triggering condition.
//!
//! Before this, the interrupt point was its own timing event, reaction-only,
//! and there was no after-discovery condition at all: an ability declaring
//! `at` or `after` on a discovery was never collected, never resolved and
//! never rejected. No corpus card wants one yet, so the two new cells are
//! proved with a hand-built registry (prior art: `timing_cells.rs`,
//! `advance_act_interactive_reverse.rs`). Cover Up 01007 — the one card that
//! *does* declare the `when` cell — is covered against the **real** corpus in
//! `cover_up.rs`, which must not move.
//!
//! The cell order is the event order, so that is what these assert on: each
//! synthetic ability gains a distinct number of resources, and `CluePlaced`
//! marks the condition's own resolution.
//!
//! The count a replacement reads is **not** re-proved here — `discover_clue`
//! still caps it before emitting, and `cover_up.rs`'s Deduction 01039 pair
//! proves the cap end to end through two real cards.

use card_dsl::dsl::{
    gain_resources, reaction_on_event, Ability, Effect, EventPattern, EventTiming,
    InvestigatorTarget,
};
use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::engine::OptionId;
use game_core::event::{Event, LapseReason};
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameState, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder, TestSession};
use game_core::{assert_event, ApplyResult, TurnAction};

/// `when`-tagged reaction: +4 resources. Declaring interrupt timing on this
/// condition is accepted now that it is coordinator-owned — before #703 the
/// engine rejected it.
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
    reaction_on_event(
        EventPattern::DiscoverClues,
        timing,
        gain_resources(InvestigatorTarget::You, amount),
    )
}

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        WHEN => Some(vec![on_discovery(EventTiming::When, 4)]),
        AT => Some(vec![on_discovery(EventTiming::At, 1)]),
        AFTER => Some(vec![on_discovery(EventTiming::After, 2)]),
        CANCEL => Some(vec![reaction_on_event(
            EventPattern::DiscoverClues,
            EventTiming::When,
            Effect::Cancel,
        )]),
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
        back_abilities_for: |_| None,
        native_condition_for: |_| None,
    });
}

const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

/// Investigation-phase board: the active investigator at a `location_clues`-clue
/// location with `codes` in their threat area, 0 resources, and a single
/// `Numeric(0)` chaos token so the Intellect-3-vs-shroud-2 Investigate always
/// succeeds and discovers exactly 1 clue.
fn board_with(codes: &[&str], location_clues: u8) -> GameState {
    let mut inv = test_investigator(1);
    inv.resources = 0;
    for (i, code) in codes.iter().enumerate() {
        inv.threat_area.push(CardInPlay::enter_play(
            CardCode::new(*code),
            CardInstanceId(u32::try_from(i).expect("fixture card count fits u32")),
        ));
    }
    let mut location = test_location(10, "Study");
    location.clues = location_clues;
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(location)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_rng_seed(1)
        .build()
}

/// Investigate, committing nothing and firing the single offered reaction in
/// each of the `windows` cells that opens one. An empty cell prompts for
/// nothing, so the script length is itself an assertion about how many cells
/// were populated (`ScriptedResolver` panics on an unscripted prompt).
fn investigate_firing(state: GameState, windows: usize) -> ApplyResult {
    TestSession::new(state)
        .take(&TurnAction::Investigate { investigator: INV })
        .resolve_choices(|c| {
            c.commit_cards(&[]);
            for _ in 0..windows {
                c.pick_single(OptionId(0));
            }
        })
        .run()
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

/// Whether the walk left nothing behind: no coordinator, timing point, window
/// or effect frame outlives the sequence. The idle `InvestigatorTurn` sentinel
/// stays on the stack for the whole turn and is not one.
fn no_queued_frames(r: &ApplyResult) -> bool {
    !r.state
        .continuations
        .iter()
        .any(game_core::state::Continuation::is_queued_ability)
}

/// The headline claim: the discovery happens *between* the `when` and `at`
/// cells. Before #703 the `when` ability's window was opened by the emit site
/// itself, the discovery was performed on that window's close, and the `at` and
/// `after` cells did not exist for this condition at all.
#[test]
fn the_discovery_resolves_between_the_when_and_at_cells() {
    let r = investigate_firing(board_with(&[WHEN, AT, AFTER], 2), 3);
    assert_eq!(
        timeline(&r.events),
        vec![Some(4), None, Some(1), Some(2)],
        "expected when → discovery → at → after; events = {:?}",
        r.events
    );
    assert_eq!(r.state.locations[&LOC].clues, 1, "location -1");
    assert_eq!(r.state.investigators[&INV].clues, 1, "investigator +1");
    assert_eq!(r.state.investigators[&INV].resources, 7, "4 + 1 + 2");
}

/// An after-discovery ability is declarable for the first time, and it resolves
/// once the clues have moved — `glossary/After.md` pins after as *"the moment
/// immediately after the specified timing point or triggering condition has
/// fully resolved"*.
#[test]
fn an_after_ability_resolves_once_the_clues_have_moved() {
    let r = investigate_firing(board_with(&[AFTER], 2), 1);
    assert_eq!(
        timeline(&r.events),
        vec![None, Some(2)],
        "the discovery must precede the `after` ability; events = {:?}",
        r.events
    );
    assert!(
        no_queued_frames(&r),
        "the empty `when`/`at` cells left frames behind: {:?}",
        r.state.continuations
    );
}

/// The `at` cell sits between the two — `glossary/At.md`: *"These abilities
/// trigger in between any "when..." abilities and any "after..." abilities with
/// the same triggering condition."* — and, per ADR 0008's interpretation, after
/// the condition's impact lands.
#[test]
fn an_at_ability_resolves_after_the_clues_move_and_before_any_after_ability() {
    let r = investigate_firing(board_with(&[AT, AFTER], 2), 2);
    assert_eq!(
        timeline(&r.events),
        vec![None, Some(1), Some(2)],
        "expected discovery → at → after; events = {:?}",
        r.events
    );
}

/// A prevented condition runs **none** of the rest of its sequence (#714).
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
    // and `ScriptedResolver` would panic on an unscripted prompt if they did.
    let r = investigate_firing(board_with(&[CANCEL, AT, AFTER], 2), 1);
    assert_eq!(
        timeline(&r.events),
        Vec::<Option<u8>>::new(),
        "neither the discovery nor any later cell may run; events = {:?}",
        r.events
    );
    assert_eq!(r.state.locations[&LOC].clues, 2, "location untouched");
    assert_eq!(r.state.investigators[&INV].clues, 0, "discovered nothing");
    assert_eq!(r.state.investigators[&INV].resources, 0, "no marker fired");
    assert!(
        !r.state.pending_cancellation,
        "the signal must be consumed at the resolve step, not left to catch the \
         next condition"
    );
    assert!(
        no_queued_frames(&r),
        "the abandoned sequence left frames behind: {:?}",
        r.state.continuations
    );
}

/// Suppression reaches the rest of the **current** cell too, per the same Dodge
/// ruling: a `when` forced ability *"does not trigger if an attack is Dodged"*,
/// and the ability that dodges is itself a `when`-cell one. So the sibling
/// still pending in the open window is withdrawn rather than offered.
#[test]
fn a_cancel_withdraws_the_remaining_when_cell_abilities() {
    // The `when` cell opens with two candidates (CANCEL first, in threat-area
    // order); firing CANCEL withdraws WHEN rather than re-prompting, so one
    // pick is the whole script.
    let r = investigate_firing(board_with(&[CANCEL, WHEN, AT, AFTER], 2), 1);
    assert_eq!(
        timeline(&r.events),
        Vec::<Option<u8>>::new(),
        "no sibling `when` ability may resolve after the cancel; events = {:?}",
        r.events
    );
    assert_event!(
        &r.events,
        Event::ReactionOptionLapsed {
            code,
            reason: LapseReason::ConditionPrevented,
            ..
        } if *code == CardCode::new(WHEN)
    );
    assert_eq!(r.state.locations[&LOC].clues, 2, "location untouched");
    assert_eq!(r.state.investigators[&INV].resources, 0, "no marker fired");
}

/// A discovery of nothing is not a discovery: `discover_clue` returns before
/// the emit when the location holds no clues, so no cell runs at all.
#[test]
fn no_condition_is_emitted_when_the_location_has_no_clues() {
    let r = investigate_firing(board_with(&[AT, AFTER], 0), 0);
    assert_eq!(
        timeline(&r.events),
        Vec::<Option<u8>>::new(),
        "no discovery, so no sequence; events = {:?}",
        r.events
    );
    assert_eq!(r.state.investigators[&INV].resources, 0);
    assert!(
        no_queued_frames(&r),
        "no frames left behind: {:?}",
        r.state.continuations
    );
}
