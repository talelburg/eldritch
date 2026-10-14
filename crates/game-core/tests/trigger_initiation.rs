//! One firing path for triggered abilities (#964): a forced ability resolves
//! the same way whether it fires alone or in the lead's ordered run, and a
//! Fast event played from a reaction window gets the same binding from its
//! timing event as a reaction would.
//!
//! Synthetic probes only (ADR 0016). Each `_ti_*` code models one primitive —
//! a usage-limited forced ability that names the attacking enemy or the
//! discovered count, a second forced ability at the same timing point, a Fast
//! event that names the attacking enemy — and none stands in for a printed
//! card.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{self, EventPattern, EventTiming, InvestigatorTarget, UsageLimit, UsagePeriod};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, OptionTarget, Registries, TimingEvent};
use game_core::event::{Event, TraumaKind};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    Assignment, CardCode, CardInstanceId, DamageSource, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};

/// `Forced - When an enemy attack deals damage to this card: mark the
/// attacking enemy. (Limit once per round.)` / `Forced - When you discover
/// clues at your location: mark that many. (Limit once per round.)` Each names
/// what its timing event supplies, and each records its use.
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

/// A marker that names what its context bound: one `TraumaSuffered`, an exit
/// that only announces, whose amount is the bound value. A marker with a
/// constant amount is a plain `gain_resources` effect instead.
fn mark(cx: &mut NativeCx, ctx: &EvalContext, amount: u8) -> EngineOutcome {
    cx.suffer_trauma(ctx.controller, TraumaKind::Physical, amount);
    EngineOutcome::Done
}

/// Marks the bound clue-discovery count, or 0 when nothing was bound.
fn mark_count(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let amount = ctx.clue_discovery_count().unwrap_or(0);
    mark(cx, ctx, amount)
}

/// Marks the bound attacking enemy's id, or 0 when nothing was bound.
fn mark_attacker(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
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

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(NAMES_WHAT_IT_HEARS, || {
                let once_per_round = UsageLimit {
                    count: 1,
                    period: UsagePeriod::Round,
                };
                vec![
                    dsl::forced_on_event(
                        EventPattern::EnemyAttackDamagedSelf,
                        EventTiming::When,
                        dsl::native("_ti:mark_attacker"),
                    )
                    .with_usage_limit(once_per_round),
                    dsl::forced_on_event(
                        EventPattern::DiscoverClues,
                        EventTiming::When,
                        dsl::native("_ti:mark_count"),
                    )
                    .with_usage_limit(once_per_round),
                ]
            })
            .with_abilities(HEARS_THE_SAME, || {
                vec![
                    dsl::forced_on_event(
                        EventPattern::EnemyAttackDamagedSelf,
                        EventTiming::When,
                        dsl::gain_resources(InvestigatorTarget::You, 2),
                    ),
                    dsl::forced_on_event(
                        EventPattern::DiscoverClues,
                        EventTiming::When,
                        dsl::gain_resources(InvestigatorTarget::You, 2),
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
            .with_native_effect("_ti:mark_attacker", mark_attacker)
            .with_native_effect("_ti:mark_count", mark_count)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const HERE: LocationId = LocationId(1);
const ATTACKER: EnemyId = EnemyId(7);

/// `INV`'s open turn at `HERE`, holding `codes` in their threat area, with
/// `HERE` holding `location_clues`. The probes' timing points are fired over
/// this turn rather than reached by a real attack or discovery (see each
/// test), so the session comes back to the turn menu afterwards.
fn session_holding(codes: &[&str], location_clues: u8) -> TestSession {
    let mut location = test_support::test_location(HERE.0, "Mock location");
    location.clues = location_clues;
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(INV.0).at(HERE).threat_area(codes.iter().copied()))
        .with_location(location)
        .open_turn(INV)
        .session()
}

/// The markers fired so far, in order, with the controller each bound to:
/// the context-naming markers' `TraumaSuffered` and the constant marker's
/// `ResourcesGained`.
fn marks(session: &TestSession) -> Vec<(InvestigatorId, u8)> {
    session
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::TraumaSuffered {
                investigator,
                amount,
                ..
            }
            | Event::ResourcesGained {
                investigator,
                amount,
            } => Some((*investigator, *amount)),
            _ => None,
        })
        .collect()
}

/// How many uses of `instance`'s printed ability `ability` are recorded this
/// round.
fn recorded_uses(state: &GameState, instance: CardInstanceId, ability: u8) -> u8 {
    let card = state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.instance_id == instance)
        .expect("the card is still in the threat area");
    card.ability_usage
        .get(&ability)
        .filter(|record| record.round == state.round)
        .map_or(0, |record| record.count)
}

/// An enemy attack by [`ATTACKER`] dealing 1 damage to each of `cards`.
fn enemy_7_damages(cards: &[CardInstanceId]) -> TimingEvent {
    TimingEvent::DamageAssigned {
        source: DamageSource::EnemyAttack { enemy: ATTACKER },
        investigator: INV,
        assignment: Assignment {
            asset_damage: cards.iter().map(|id| (*id, 1)).collect(),
            ..Assignment::default()
        },
    }
}

/// Fire an enemy attack's damage to the threat-area cards `codes` over the
/// open turn. Not a real attack: an attack assigns damage only to assets with
/// health, and these probes are healthless threat-area cards, so the timing
/// point is fired on its own.
fn attack_damages(session: TestSession, codes: &[&str]) -> TestSession {
    let ids: Vec<CardInstanceId> = codes
        .iter()
        .map(|code| session.state().in_play(INV, *code))
        .collect();
    session.fire_at(enemy_7_damages(&ids))
}

#[test]
fn a_forced_ability_fired_alone_names_the_attacking_enemy_and_records_its_use_like_one_in_an_ordered_run(
) {
    // Alone: the only forced ability the damage reaches.
    let alone = attack_damages(
        session_holding(&[NAMES_WHAT_IT_HEARS], 0),
        &[NAMES_WHAT_IT_HEARS],
    );
    alone.assert_at_turn_menu(INV);
    // In a run: the lead orders it alongside a sibling, and picks it first.
    let paused = attack_damages(
        session_holding(&[NAMES_WHAT_IT_HEARS, HEARS_THE_SAME], 0),
        &[NAMES_WHAT_IT_HEARS, HEARS_THE_SAME],
    );
    let (names, same) = (
        paused.state().in_play(INV, NAMES_WHAT_IT_HEARS),
        paused.state().in_play(INV, HEARS_THE_SAME),
    );
    let in_run = paused
        .pick(OptionTarget::CardInstance(names))
        .pick(OptionTarget::CardInstance(same));
    in_run.assert_at_turn_menu(INV);

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
    assert_eq!(
        recorded_uses(in_run.state(), names, 0),
        1,
        "the ability fired in a run counts one use"
    );
    assert_eq!(
        recorded_uses(
            alone.state(),
            alone.state().in_play(INV, NAMES_WHAT_IT_HEARS),
            0
        ),
        1,
        "the same ability fired alone counts the same one use"
    );
}

#[test]
fn a_forced_ability_at_its_usage_limit_does_not_initiate() {
    // The round has not ended between the two timing points, so the second is
    // in the same period as the first.
    let session = attack_damages(
        attack_damages(
            session_holding(&[NAMES_WHAT_IT_HEARS], 0),
            &[NAMES_WHAT_IT_HEARS],
        ),
        &[NAMES_WHAT_IT_HEARS],
    );

    session.assert_at_turn_menu(INV);
    assert_eq!(
        marks(&session),
        vec![(INV, 7)],
        "the second timing point found the once-per-round ability already used"
    );
}

/// The investigator holding `codes`, at a location with 5 clues, discovering 3
/// of them. Fired over the open turn rather than reached by an Investigate: a
/// basic investigate discovers 1 clue, and a count other than 1 is what tells
/// a bound count from a constant.
fn discovering_3(codes: &[&str]) -> TestSession {
    session_holding(codes, 5).fire_at(TimingEvent::DiscoverClues {
        investigator: INV,
        location: HERE,
        count: 3,
    })
}

#[test]
fn a_forced_ability_fired_alone_names_the_discovered_count_like_one_in_an_ordered_run() {
    let alone = discovering_3(&[NAMES_WHAT_IT_HEARS]);
    alone.assert_at_turn_menu(INV);
    let paused = discovering_3(&[NAMES_WHAT_IT_HEARS, HEARS_THE_SAME]);
    let (names, same) = (
        paused.state().in_play(INV, NAMES_WHAT_IT_HEARS),
        paused.state().in_play(INV, HEARS_THE_SAME),
    );
    let in_run = paused
        .pick(OptionTarget::CardInstance(names))
        .pick(OptionTarget::CardInstance(same));
    in_run.assert_at_turn_menu(INV);

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
    assert_eq!(
        (
            recorded_uses(
                alone.state(),
                alone.state().in_play(INV, NAMES_WHAT_IT_HEARS),
                1
            ),
            recorded_uses(in_run.state(), names, 1),
        ),
        (1, 1),
        "alone and in a run, the ability counts the same one use"
    );
}

#[test]
fn a_fast_event_played_in_an_enemy_attack_damage_window_names_the_attacking_enemy() {
    // A real attack: drawing a card while engaged provokes the enemy's attack
    // of opportunity, whose damage opens the window the Fast event is played
    // in. (Draw rather than Resource, whose resource gain `marks` would count.)
    let mut attacker = test_support::test_enemy(ATTACKER.0, "Attacker");
    attacker.attack_damage = 1;
    attacker.attack_horror = 0;
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(INV.0)
                .at(HERE)
                .hand([NAMES_ATTACKER])
                .deck(["filler-card"]),
        )
        .with_location(test_support::test_location(HERE.0, "Mock location"))
        .with_enemy_engaged(attacker, INV)
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });

    let session = session.pick(OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(NAMES_ATTACKER),
    });
    session.assert_at_turn_menu(INV);
    assert_eq!(
        marks(&session),
        vec![(INV, 7)],
        "the Fast event's effect was bound to the attacking enemy"
    );
}
