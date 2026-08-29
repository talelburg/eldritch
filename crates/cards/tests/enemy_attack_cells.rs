//! #704: an enemy attack is **one** triggering condition, and the coordinator
//! resolves it between the `when` and `at` cells.
//!
//! `data/rules-reference/rules/glossary/Nested_Sequences.md`, verbatim:
//!
//! > Each time a triggering condition occurs, the following sequence is
//! > followed: 1) execute "when..." effects that interrupt that triggering
//! > condition, (2) resolve the triggering condition, and then, (3) execute
//! > "after..." effects in response to that triggering condition.
//!
//! Before this, the attack's interrupt point was a single-cell reaction window
//! the attack loop opened by hand, and there was no after-attack condition at
//! all: an ability declaring `at` or `after` on an attack was never collected,
//! never resolved and never rejected. The sibling migration for clue discovery
//! is `clue_discovery_cells.rs`, whose structure this file mirrors.
//!
//! The corpus halves live elsewhere and must not move: Dodge 01023's cancel in
//! `dodge.rs` / `dodge_aoo.rs`, Guard Dog 01021's soak reaction in
//! `guard_dog_soak.rs`, and Silver Twilight Acolyte 01102's forced
//! after-attack doom in `dodge.rs`.
//!
//! The cell order is the event order, so that is what these assert on: each
//! synthetic ability gains a distinct number of resources, and `DamageTaken` /
//! `HorrorTaken` mark the condition's own resolution.

use card_dsl::dsl::{
    forced_on_event, gain_resources, reaction_on_event, Ability, Effect, EventPattern, EventTiming,
    InvestigatorTarget,
};
use game_core::card_registry::CardRegistry;
use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, GameState, InvestigatorId, LocationId,
    Phase,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event, Action, InputResponse, PlayerAction, TurnAction};

/// `when`-tagged reaction on a card the attacked investigator controls: +4
/// resources. Declaring interrupt timing on this condition is *accepted* now
/// that it is coordinator-owned; a caller-owned condition rejects it.
const WHEN: &str = "_ea_when";
/// `at`-tagged reaction: +1 resource.
const AT: &str = "_ea_at";
/// `after`-tagged reaction: +2 resources.
const AFTER: &str = "_ea_after";
/// `when`-tagged reaction that cancels the attack — Dodge 01023's shape without
/// Dodge's Fast-event play.
const CANCEL: &str = "_ea_cancel";

/// An **enemy** whose own printed ability is `Forced - After … attacks`: +8
/// resources to the investigator it attacked. Silver Twilight Acolyte 01102's
/// shape; the forced scan for this condition reads the attacking enemy's card.
const ENEMY_FORCED_AFTER: &str = "_ea_enemy_forced_after";
/// An enemy whose ability is `Forced - When … attacks`: +16 resources. No
/// Chapter 1 enemy prints this, but the Dodge ruling names both trigger words,
/// so the cell has to work.
const ENEMY_FORCED_WHEN: &str = "_ea_enemy_forced_when";

/// A reaction in `timing`'s cell of the one condition under test, gaining
/// `amount` resources — the marker these tests read cell order off.
fn on_attack(timing: EventTiming, amount: u8) -> Ability {
    reaction_on_event(
        EventPattern::EnemyAttacks,
        timing,
        gain_resources(InvestigatorTarget::You, amount),
    )
}

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        WHEN => Some(vec![on_attack(EventTiming::When, 4)]),
        AT => Some(vec![on_attack(EventTiming::At, 1)]),
        AFTER => Some(vec![on_attack(EventTiming::After, 2)]),
        CANCEL => Some(vec![reaction_on_event(
            EventPattern::EnemyAttacks,
            EventTiming::When,
            Effect::Cancel,
        )]),
        ENEMY_FORCED_AFTER => Some(vec![forced_on_event(
            EventPattern::EnemyAttacks,
            EventTiming::After,
            gain_resources(InvestigatorTarget::You, 8),
        )]),
        ENEMY_FORCED_WHEN => Some(vec![forced_on_event(
            EventPattern::EnemyAttacks,
            EventTiming::When,
            gain_resources(InvestigatorTarget::You, 16),
        )]),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for: |code| game_core::test_support::metadata_for_test_inv(code),
        abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
        back_abilities_for: |_| None,
        native_condition_for: |_| None,
    });
}

const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const ATTACKER: EnemyId = EnemyId(7);

/// The attacker: engaged, ready, dealing 1 damage **and** 1 horror, so the
/// condition's own impact is visible on both tracks. `code` selects whether it
/// carries a forced ability of its own.
fn attacker(code: &str) -> Enemy {
    let mut e = test_enemy(7, "Attacker");
    e.code = CardCode::new(code);
    e.attack_damage = 1;
    e.attack_horror = 1;
    e.current_location = Some(LOC);
    e.engaged_with = Some(INV);
    e
}

/// Investigation-phase board: the active investigator at `LOC` with `codes` in
/// their threat area and 0 resources, engaged by one ready attacker. `EndTurn`
/// cascades into the Enemy phase, where the attack walks its cells.
///
/// No soaker assets, so the attack's damage/horror never prompts the
/// interactive distribution (#44/K5b) and the only prompts are the cells' own.
fn board_with(codes: &[&str], enemy_code: &str) -> GameState {
    let mut inv = test_investigator(1);
    inv.resources = 0;
    // A spare deck card so Upkeep step 4.4's draw does not hit an empty deck
    // (which would add a horror the damage/horror assertions would then read).
    inv.deck = vec![CardCode::new("_ea_filler")];
    for (i, code) in codes.iter().enumerate() {
        inv.threat_area.push(CardInPlay::enter_play(
            CardCode::new(*code),
            CardInstanceId(u32::try_from(i).expect("fixture card count fits u32")),
        ));
    }
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_location(10, "Study"))
        .with_investigator_at(inv, LOC)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_enemy(attacker(enemy_code))
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(INV)
        .build()
}

/// The board and the whole event stream after a scripted run.
struct Run {
    state: GameState,
    events: Vec<Event>,
}

/// End the turn and answer `picks` prompts in order, accumulating every event.
/// A prompt the script does not answer simply leaves the engine suspended, and
/// a script longer than the prompts panics — so the script length is itself an
/// assertion about how many cells opened.
fn end_turn_answering(state: GameState, picks: &[InputResponse]) -> Run {
    let mut result = take_turn_action(state, &TurnAction::EndTurn);
    let mut events = std::mem::take(&mut result.events);
    for pick in picks {
        assert!(
            matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
            "expected a prompt for {pick:?}, got {:?}",
            result.outcome
        );
        result = apply(
            result.state,
            Action::Player(PlayerAction::ResolveInput {
                response: pick.clone(),
            }),
        );
        events.extend(std::mem::take(&mut result.events));
    }
    Run {
        state: result.state,
        events,
    }
}

/// Fire the single offered candidate, `n` times.
fn fire(n: usize) -> Vec<InputResponse> {
    vec![InputResponse::PickSingle(OptionId(0)); n]
}

/// Every marker and every mark of the condition's own impact, in emission order
/// — `Some(amount)` for a marker ability's resource gain, `None` for a point of
/// damage or horror the attack placed.
///
/// Truncated at the end of the Enemy phase: the `EndTurn` cascade runs on into
/// Upkeep, whose card draw and resource gain are not this file's subject.
fn timeline(events: &[Event]) -> Vec<Option<u8>> {
    events
        .iter()
        .take_while(|e| {
            !matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Enemy
                }
            )
        })
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } => Some(Some(*amount)),
            Event::DamageTaken { .. } | Event::HorrorTaken { .. } => Some(None),
            _ => None,
        })
        .collect()
}

/// The headline claim: the attack lands *between* the `when` and `at` cells.
/// Before #704 the `when` ability's window was opened by the attack loop
/// itself, the damage was dealt on that window's close, and the `at` and
/// `after` cells did not exist for this condition at all.
#[test]
fn the_attack_resolves_between_the_when_and_at_cells() {
    let r = end_turn_answering(board_with(&[WHEN, AT, AFTER], "_plain"), &fire(3));
    assert_eq!(
        timeline(&r.events),
        vec![Some(4), None, None, Some(1), Some(2)],
        "expected when → damage+horror → at → after; events = {:?}",
        r.events
    );
    assert_eq!(r.state.investigators[&INV].damage(), 1);
    assert_eq!(r.state.investigators[&INV].horror(), 1);
    assert_eq!(
        r.state.investigators[&INV].resources, 8,
        "4 + 1 + 2 from the cells, plus Upkeep step 4.4's 1"
    );
}

/// An after-attack ability is declarable for the first time, and it resolves
/// once the damage and horror have landed — `glossary/After.md` pins after as
/// *"the moment immediately after the specified timing point or triggering
/// condition has fully resolved"*.
#[test]
fn an_after_ability_resolves_once_the_damage_and_horror_have_landed() {
    let r = end_turn_answering(board_with(&[AFTER], "_plain"), &fire(1));
    assert_eq!(
        timeline(&r.events),
        vec![None, None, Some(2)],
        "the attack must precede the `after` ability; events = {:?}",
        r.events
    );
    assert_eq!(r.state.investigators[&INV].damage(), 1);
    assert_eq!(r.state.investigators[&INV].horror(), 1);
}

/// The `at` cell sits between the two — `glossary/At.md`: *"These abilities
/// trigger in between any "when..." abilities and any "after..." abilities with
/// the same triggering condition."* — and, per ADR 0008's interpretation, after
/// the condition's impact lands.
#[test]
fn an_at_ability_resolves_after_the_impact_and_before_any_after_ability() {
    let r = end_turn_answering(board_with(&[AT, AFTER], "_plain"), &fire(2));
    assert_eq!(
        timeline(&r.events),
        vec![None, None, Some(1), Some(2)],
        "expected attack → at → after; events = {:?}",
        r.events
    );
}

/// A prevented attack runs **none** of the rest of its sequence (#714), but the
/// attacker still exhausts: `data/arkhamdb-faq/core/01023.md`, verbatim —
/// *"If an attack was cancelled during the Enemy phase, the attacking enemy
/// still exhausts."*
#[test]
fn a_cancelled_attack_suppresses_the_at_and_after_cells_but_still_exhausts() {
    // One prompt only: the `when` cell. The `at` and `after` cells never open.
    let r = end_turn_answering(board_with(&[CANCEL, AT, AFTER], "_plain"), &fire(1));
    assert_eq!(
        timeline(&r.events),
        Vec::<Option<u8>>::new(),
        "neither the attack nor any later cell may run; events = {:?}",
        r.events
    );
    assert_eq!(r.state.investigators[&INV].damage(), 0);
    assert_eq!(r.state.investigators[&INV].horror(), 0);
    assert_eq!(
        r.state.investigators[&INV].resources, 1,
        "no marker fired; the 1 is Upkeep step 4.4's"
    );
    assert_event!(&r.events, Event::EnemyExhausted { enemy } if *enemy == ATTACKER);
    assert!(
        !r.state.pending_cancellation,
        "the signal must be consumed at the resolve step, not left to catch the \
         next condition"
    );
}

/// The attacking enemy's own `Forced - After … attacks` resolves in the `after`
/// cell, once the damage and horror have landed. Silver Twilight Acolyte
/// 01102's shape; the corpus half is in `dodge.rs`.
#[test]
fn an_enemys_forced_after_attacks_ability_resolves_after_its_damage() {
    let r = end_turn_answering(board_with(&[], ENEMY_FORCED_AFTER), &[]);
    assert_eq!(
        timeline(&r.events),
        vec![None, None, Some(8)],
        "expected damage+horror → the enemy's forced after; events = {:?}",
        r.events
    );
}

/// …and its `Forced - When … attacks` resolves in the `when` cell, *before* the
/// damage. This is the acceptance the migration buys: on a caller-owned
/// condition the coordinator rejects an interrupt declaration outright, because
/// the caller has already mutated.
#[test]
fn an_enemys_forced_when_attacks_ability_resolves_before_its_damage() {
    let r = end_turn_answering(board_with(&[], ENEMY_FORCED_WHEN), &[]);
    assert_eq!(
        timeline(&r.events),
        vec![Some(16), None, None],
        "expected the enemy's forced when → damage+horror; events = {:?}",
        r.events
    );
}

/// The forced-before-reaction ordering inside one cell (RR p.2) survives the
/// migration: the enemy's forced `after` resolves before the investigator's
/// `after` reaction in the same cell.
#[test]
fn forced_resolves_before_reaction_within_the_after_cell() {
    let r = end_turn_answering(board_with(&[AFTER], ENEMY_FORCED_AFTER), &fire(1));
    assert_eq!(
        timeline(&r.events),
        vec![None, None, Some(8), Some(2)],
        "expected damage → forced (8) → reaction (2); events = {:?}",
        r.events
    );
}
