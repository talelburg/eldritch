//! One firing path for triggered abilities (#964): a forced ability resolves
//! the same way whether it fires alone or in the lead's ordered run, and a
//! Fast event played from a reaction window gets the same binding from its
//! timing event as a reaction would.
//!
//! Synthetic probes only (ADR 0016). Each `_ti_*` code models one primitive —
//! a usage-limited forced ability at the end of the round, a second forced
//! ability at the same timing point, a Fast event that names the attacking
//! enemy — and none stands in for a printed card.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{self, EventPattern, EventTiming, UsageLimit, UsagePeriod};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{Cx, EngineOutcome, OptionTarget, TimingEvent};
use game_core::event::Event;
use game_core::state::{
    Assignment, CardCode, CardInPlay, CardInstanceId, DamageSource, EnemyId, GameState,
    GameStateBuilder, Investigator, InvestigatorId,
};
use game_core::test_support::{self, MockRegistry, TestSession};

/// `Forced - At the end of the round: mark 1. (Limit once per round.)`
const LIMITED: &str = "_ti_forced_limited";
/// `Forced - At the end of the round: mark 2.` A sibling at the same timing
/// point, so [`LIMITED`] fires in an ordered run rather than alone.
const SIBLING: &str = "_ti_forced_sibling";
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
