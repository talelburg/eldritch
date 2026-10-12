//! The one uses-spending path, [`engine::spend_uses`] (#1009, #353): spending
//! a card's uses through a native's **effect**, not a `Cost::SpendUses`.
//!
//! The depletion discard is a constant ability (#353): it fires the moment the
//! pool empties, wherever the uses leave from. A cost already reached it; an
//! effect did not, so a card whose last use leaves through its effect
//! (Forbidden Knowledge 01058) would have stayed in play with none.
//!
//! Each native sits on its own mock act as a forced ability at the end of the
//! enemy phase, the `native_effect.rs` idiom, and spends from a synthetic
//! asset ([`POUCH`]) printing `discard_when_empty` secrets. The fixtures model
//! the primitive, not a printed card (ADR 0016).
//!
//! Its own test binary: the mock registry installs once per process, and the
//! sibling exit tests (`native_exits.rs`, `native_cx.rs`) install their own.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Uses};
use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{ApplyResult, EngineOutcome, NativeCx, TimingEvent};
use game_core::event::Event;
use game_core::state::{
    self, Act, CardCode, CardInPlay, CardInstanceId, DiscardPile, GameState, GameStateBuilder,
    InvestigatorId, Owner, UseKind, Zone,
};
use game_core::test_support::{self, MockRegistry, TestSession};
use game_core::{assert_event, assert_no_event};

/// A synthetic asset printing "Uses (secrets)", discarded when they run out.
const POUCH: &str = "TEST-POUCH";

/// The [`POUCH`] instance the natives spend from.
const HELD: CardInstanceId = CardInstanceId(50);

const SPEND_SECRET_ACT: &str = "TEST-ACT-SPEND-SECRET";
const SPEND_CHARGE_ACT: &str = "TEST-ACT-SPEND-CHARGE";

/// The controller, who holds [`HELD`].
const HOLDER: InvestigatorId = InvestigatorId(1);

/// Who owns [`HELD`]: not its holder, so the discard's routing is visible.
const OWNER: InvestigatorId = InvestigatorId(2);

fn pouch_metadata() -> CardMetadata {
    CardMetadata {
        code: POUCH.to_string(),
        name: "Pouch".to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: Some(Uses {
                kind: UseKind::Secrets,
                count: 2,
                discard_when_empty: true,
            }),
            play_only_during_turn: false,
        },
    }
}

/// Forced at end of enemy phase -> the native effect tagged `tag`.
fn forced_native(tag: &'static str) -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::PhaseEnded {
            phase: dsl::Phase::Enemy,
        },
        EventTiming::After,
        dsl::native(tag),
    )]
}

/// Spend 1 `kind` from [`HELD`] as the controller; reject if the spend fails.
fn spend_one(cx: &mut NativeCx, ctx: &EvalContext, kind: UseKind) -> EngineOutcome {
    match cx.spend_uses(ctx.controller, HELD, kind, 1) {
        Ok(()) => EngineOutcome::Done,
        Err(err) => EngineOutcome::Rejected {
            reason: format!("spend_uses failed: {err:?}").into(),
        },
    }
}

fn spend_secret(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    spend_one(cx, ctx, UseKind::Secrets)
}

fn spend_charge(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    spend_one(cx, ctx, UseKind::Charges)
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_card(pouch_metadata())
        .with_abilities(SPEND_SECRET_ACT, || forced_native("test:spend-secret"))
        .with_native_effect("test:spend-secret", spend_secret)
        .with_abilities(SPEND_CHARGE_ACT, || forced_native("test:spend-charge"))
        .with_native_effect("test:spend-charge", spend_charge)
        .install();
}

/// [`HOLDER`] (first in turn order, so the forced dispatch binds the
/// controller to them) and [`OWNER`], `act` as the current act, and — when
/// `pool` is `Some` — [`HELD`] in the holder's play area with that uses pool.
fn state(act: &str, pool: Option<&[(UseKind, u8)]>) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(HOLDER.0))
        .with_investigator(test_support::test_investigator(OWNER.0))
        .with_turn_order([HOLDER, OWNER])
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(act),
        clue_threshold: 99,
    }];
    state.act_index = 0;
    if let Some(pool) = pool {
        let mut pouch =
            CardInPlay::enter_play(CardCode::new(POUCH), HELD, Owner::Investigator(OWNER));
        pouch.uses.extend(pool.iter().copied());
        state
            .investigators
            .get_mut(&HOLDER)
            .expect("holder")
            .cards_in_play
            .push(pouch);
    }
    state
}

fn run(state: GameState) -> ApplyResult {
    TestSession::new(state)
        .fire_at(TimingEvent::PhaseEnded {
            phase: state::Phase::Enemy,
        })
        .finish()
}

fn pouch_in_play(state: &GameState) -> Option<&CardInPlay> {
    state.investigators[&HOLDER]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == HELD)
}

#[test]
fn spending_the_last_use_through_an_effect_discards_the_card_to_its_owner() {
    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = run(state(SPEND_SECRET_ACT, Some(&[(UseKind::Secrets, 1)])));
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(pouch_in_play(&state).is_none(), "the empty pouch left play");
    assert_eq!(
        state.investigators[&OWNER].discard,
        vec![CardCode::new(POUCH)]
    );
    assert!(state.investigators[&HOLDER].discard.is_empty());

    let spent = events
        .iter()
        .position(|e| {
            *e == Event::UsesSpent {
                investigator: HOLDER,
                instance_id: HELD,
                kind: UseKind::Secrets,
                amount: 1,
            }
        })
        .expect("UsesSpent emitted");
    let discarded = events
        .iter()
        .position(|e| {
            *e == Event::CardDiscarded {
                code: CardCode::new(POUCH),
                from: Zone::InPlay,
                to: DiscardPile::Investigator(OWNER),
            }
        })
        .expect("CardDiscarded emitted");
    assert!(
        spent < discarded,
        "the spend is announced before the discard"
    );
}

#[test]
fn a_spend_that_leaves_uses_keeps_the_card_in_play() {
    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = run(state(SPEND_SECRET_ACT, Some(&[(UseKind::Secrets, 2)])));
    assert_eq!(outcome, EngineOutcome::Done);
    let pouch = pouch_in_play(&state).expect("the pouch still has a secret");
    assert_eq!(pouch.uses.get(&UseKind::Secrets), Some(&1));
    assert!(state.investigators[&OWNER].discard.is_empty());
    assert_event!(
        events,
        Event::UsesSpent {
            investigator: HOLDER,
            instance_id: HELD,
            kind: UseKind::Secrets,
            amount: 1,
        }
    );
    assert_no_event!(events, Event::CardDiscarded { .. });
}

#[test]
fn emptying_a_pool_of_another_kind_keeps_the_card_in_play() {
    // The printed discard-when-empty uses are secrets; the charges run out.
    let ApplyResult {
        state,
        outcome,
        events,
        ..
    } = run(state(
        SPEND_CHARGE_ACT,
        Some(&[(UseKind::Charges, 1), (UseKind::Secrets, 2)]),
    ));
    assert_eq!(outcome, EngineOutcome::Done);
    let pouch = pouch_in_play(&state).expect("only the printed kind discards it");
    assert_eq!(pouch.uses.get(&UseKind::Charges), Some(&0));
    assert_no_event!(events, Event::CardDiscarded { .. });
}

#[test]
fn spending_more_than_remains_saturates_and_discards() {
    let ApplyResult { state, outcome, .. } =
        run(state(SPEND_SECRET_ACT, Some(&[(UseKind::Secrets, 0)])));
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(pouch_in_play(&state).is_none(), "an empty pool is empty");
    assert_eq!(
        state.investigators[&OWNER].discard,
        vec![CardCode::new(POUCH)]
    );
}

#[test]
fn spending_from_a_card_not_in_play_is_an_error_the_native_sees() {
    let ApplyResult {
        outcome, events, ..
    } = run(state(SPEND_SECRET_ACT, None));
    assert!(
        matches!(&outcome, EngineOutcome::Rejected { reason } if reason.contains("NotInPlay")),
        "the native saw the error: {outcome:?}"
    );
    assert_no_event!(events, Event::UsesSpent { .. });
}
