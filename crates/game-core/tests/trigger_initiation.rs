//! One firing path for triggered abilities (#964): a forced ability resolves
//! the same way whether it fires alone or in the lead's ordered run, and a
//! Fast event played from a reaction window gets the same binding from its
//! timing event as a reaction would.
//!
//! Synthetic probes only (ADR 0016). Each `_ti_*` code models one primitive —
//! a usage-limited forced ability at the end of the round, a forced ability
//! that names the attacking enemy or the discovered count, a second forced
//! ability at the same timing point, a Fast event that names the attacking
//! enemy — and none stands in for a printed card.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{self, EventPattern, EventTiming, UsageLimit, UsagePeriod};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{Cx, EngineOutcome, OptionTarget, TimingEvent};
use game_core::event::Event;
use game_core::state::{
    Assignment, CardCode, CardInPlay, CardInstanceId, DamageSource, EnemyId, GameState,
    GameStateBuilder, Investigator, InvestigatorId, LocationId,
};
use game_core::test_support::{self, MockRegistry, TestSession};

/// `Forced - At the end of the round: mark 1. (Limit once per round.)`
const LIMITED: &str = "_ti_forced_limited";
/// `Forced - At the end of the round: mark 2.` A sibling at the same timing
/// point, so [`LIMITED`] fires in an ordered run rather than alone.
const SIBLING: &str = "_ti_forced_sibling";
/// `Forced - When an enemy attack deals damage to this card: mark the
/// attacking enemy.` / `Forced - When you discover clues at your location:
/// mark that many.` Each names what its timing event supplies.
const NAMES_WHAT_IT_HEARS: &str = "_ti_forced_names_what_it_hears";
/// `Forced - When an enemy attack deals damage to this card: mark 2.` /
/// `Forced - When you discover clues at your location: mark 2.` A sibling at
/// both of [`NAMES_WHAT_IT_HEARS`]'s timing points, so it fires in an ordered
/// run rather than alone.
const HEARS_THE_SAME: &str = "_ti_forced_hears_the_same";
/// Fast event. `[reaction] When an enemy attack deals damage: mark the
/// attacking enemy.`
const NAMES_ATTACKER: &str = "_ti_fast_names_attacker";

const INV: InvestigatorId = InvestigatorId(1);

/// A marker: one `ResourcesGained` whose amount names what fired.
fn mark(cx: &mut Cx, ctx: &EvalContext, amount: u8) -> EngineOutcome {
    cx.events.push(Event::ResourcesGained {
        investigator: ctx.controller,
        amount,
    });
    EngineOutcome::Done
}

/// Marks the bound clue-discovery count, or 0 when nothing was bound.
fn mark_count(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let amount = ctx.clue_discovery_count().unwrap_or(0);
    mark(cx, ctx, amount)
}

/// Marks the bound attacking enemy's id, or 0 when nothing was bound.
fn mark_attacker(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let amount = ctx
        .attacking_enemy()
        .map_or(0, |enemy| u8::try_from(enemy.0).expect("small enemy id"));
    mark(cx, ctx, amount)
}

fn fast_event_metadata() -> CardMetadata {
    CardMetadata {
        code: NAMES_ATTACKER.to_owned(),
        name: "Mock attacker-naming Fast event".to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Neutral,
            cost: Some(0),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
            play_only_during_turn: false,
        },
    }
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_abilities(LIMITED, || {
            vec![dsl::forced_on_event(
                EventPattern::RoundEnded,
                EventTiming::At,
                dsl::native("_ti:mark1"),
            )
            .with_usage_limit(UsageLimit {
                count: 1,
                period: UsagePeriod::Round,
            })]
        })
        .with_abilities(SIBLING, || {
            vec![dsl::forced_on_event(
                EventPattern::RoundEnded,
                EventTiming::At,
                dsl::native("_ti:mark2"),
            )]
        })
        .with_abilities(NAMES_WHAT_IT_HEARS, || {
            vec![
                dsl::forced_on_event(
                    EventPattern::EnemyAttackDamagedSelf,
                    EventTiming::When,
                    dsl::native("_ti:mark_attacker"),
                ),
                dsl::forced_on_event(
                    EventPattern::DiscoverClues,
                    EventTiming::When,
                    dsl::native("_ti:mark_count"),
                ),
            ]
        })
        .with_abilities(HEARS_THE_SAME, || {
            vec![
                dsl::forced_on_event(
                    EventPattern::EnemyAttackDamagedSelf,
                    EventTiming::When,
                    dsl::native("_ti:mark2"),
                ),
                dsl::forced_on_event(
                    EventPattern::DiscoverClues,
                    EventTiming::When,
                    dsl::native("_ti:mark2"),
                ),
            ]
        })
        .with_card(fast_event_metadata())
        .with_abilities(NAMES_ATTACKER, || {
            vec![dsl::reaction_on_event(
                EventPattern::EnemyAttackDamagedSelf,
                EventTiming::When,
                dsl::native("_ti:mark_attacker"),
            )]
        })
        .with_native_effect("_ti:mark1", |cx, ctx| mark(cx, ctx, 1))
        .with_native_effect("_ti:mark2", |cx, ctx| mark(cx, ctx, 2))
        .with_native_effect("_ti:mark_attacker", mark_attacker)
        .with_native_effect("_ti:mark_count", mark_count)
        .install();
}

fn instance(i: usize) -> CardInstanceId {
    CardInstanceId(u32::try_from(i + 1).expect("small index"))
}

/// The investigator, holding `codes` in their threat area, instance ids from 1
/// in order.
fn holding(codes: &[&str]) -> Investigator {
    let mut investigator = test_support::test_investigator(1);
    for (i, code) in codes.iter().enumerate() {
        investigator
            .threat_area
            .push(CardInPlay::enter_play(CardCode::new(*code), instance(i)));
    }
    investigator
}

fn session_holding(codes: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_investigator(holding(codes))
        .with_turn_order([INV])
        .session()
}

/// The markers fired so far, in order, with the controller each bound to.
fn marks(session: &TestSession) -> Vec<(InvestigatorId, u8)> {
    session
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained {
                investigator,
                amount,
            } => Some((*investigator, *amount)),
            _ => None,
        })
        .collect()
}

/// How many uses of `instance`'s first printed ability are recorded this round.
fn recorded_uses(state: &GameState, instance: CardInstanceId) -> u8 {
    let card = state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.instance_id == instance)
        .expect("the card is still in the threat area");
    card.ability_usage
        .get(&0)
        .filter(|record| record.round == state.round)
        .map_or(0, |record| record.count)
}

#[test]
fn a_forced_ability_fired_alone_records_its_use_like_one_in_an_ordered_run() {
    // Alone: the only forced ability at the end of the round.
    let alone = session_holding(&[LIMITED]).fire_at(TimingEvent::RoundEnded);

    // In a run: the lead orders it alongside a sibling, and picks it first.
    let in_run = session_holding(&[LIMITED, SIBLING])
        .fire_at(TimingEvent::RoundEnded)
        .pick(OptionTarget::CardInstance(instance(0)))
        .pick(OptionTarget::CardInstance(instance(1)));

    assert_eq!(marks(&alone), vec![(INV, 1)], "the lone ability resolved");
    assert_eq!(
        marks(&in_run),
        vec![(INV, 1), (INV, 2)],
        "both abilities in the run resolved, in the lead's order"
    );
    assert_eq!(
        recorded_uses(in_run.state(), instance(0)),
        1,
        "the ability fired in a run counts one use"
    );
    assert_eq!(
        recorded_uses(alone.state(), instance(0)),
        1,
        "the same ability fired alone counts the same one use"
    );
}

#[test]
fn a_forced_ability_at_its_usage_limit_does_not_initiate() {
    // The round has not ended between the two timing points, so the second is
    // in the same period as the first.
    let session = session_holding(&[LIMITED])
        .fire_at(TimingEvent::RoundEnded)
        .fire_at(TimingEvent::RoundEnded);

    assert_eq!(
        marks(&session),
        vec![(INV, 1)],
        "the second timing point found the once-per-round ability already used"
    );
}

/// An enemy attack by enemy 7 dealing 1 damage to each of the first `cards`
/// instances.
fn enemy_7_damages(cards: usize) -> TimingEvent {
    TimingEvent::DamageAssigned {
        source: DamageSource::EnemyAttack { enemy: EnemyId(7) },
        investigator: INV,
        assignment: Assignment {
            asset_damage: (0..cards).map(|i| (instance(i), 1)).collect(),
            ..Assignment::default()
        },
    }
}

#[test]
fn a_forced_ability_fired_alone_names_the_attacking_enemy_like_one_in_an_ordered_run() {
    let alone = session_holding(&[NAMES_WHAT_IT_HEARS]).fire_at(enemy_7_damages(1));
    let in_run = session_holding(&[NAMES_WHAT_IT_HEARS, HEARS_THE_SAME])
        .fire_at(enemy_7_damages(2))
        .pick(OptionTarget::CardInstance(instance(0)))
        .pick(OptionTarget::CardInstance(instance(1)));

    assert_eq!(
        marks(&alone),
        vec![(INV, 7)],
        "the lone ability was bound to the attacking enemy"
    );
    assert_eq!(
        marks(&in_run),
        vec![(INV, 7), (INV, 2)],
        "the same ability in a run was bound to the same attacking enemy"
    );
}

/// The investigator holding `codes`, at a location with 5 clues, about to
/// discover 3 of them.
fn discovering_3(codes: &[&str]) -> TestSession {
    let mut location = test_support::test_location(1, "Mock location");
    location.clues = 5;
    GameStateBuilder::new()
        .with_location(location)
        .with_investigator_at(holding(codes), LocationId(1))
        .with_turn_order([INV])
        .session()
        .fire_at(TimingEvent::DiscoverClues {
            investigator: INV,
            location: LocationId(1),
            count: 3,
        })
}

#[test]
fn a_forced_ability_fired_alone_names_the_discovered_count_like_one_in_an_ordered_run() {
    let alone = discovering_3(&[NAMES_WHAT_IT_HEARS]);
    let in_run = discovering_3(&[NAMES_WHAT_IT_HEARS, HEARS_THE_SAME])
        .pick(OptionTarget::CardInstance(instance(0)))
        .pick(OptionTarget::CardInstance(instance(1)));

    assert_eq!(
        marks(&alone),
        vec![(INV, 3)],
        "the lone ability was bound to the discovered count"
    );
    assert_eq!(
        marks(&in_run),
        vec![(INV, 3), (INV, 2)],
        "the same ability in a run was bound to the same count"
    );
}

#[test]
fn a_fast_event_played_in_an_enemy_attack_damage_window_names_the_attacking_enemy() {
    let mut investigator = test_support::test_investigator(1);
    investigator.hand.push(CardCode::new(NAMES_ATTACKER));
    let session = GameStateBuilder::new()
        .with_investigator(investigator)
        .with_turn_order([INV])
        .session()
        .fire_at(TimingEvent::DamageAssigned {
            source: DamageSource::EnemyAttack { enemy: EnemyId(7) },
            investigator: INV,
            assignment: Assignment {
                investigator_damage: 1,
                ..Assignment::default()
            },
        });

    let session = session.pick(OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(NAMES_ATTACKER),
    });
    assert_eq!(
        marks(&session),
        vec![(INV, 7)],
        "the Fast event's effect was bound to the attacking enemy"
    );
}
