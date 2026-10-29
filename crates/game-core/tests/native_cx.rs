//! The native contract, driven through `apply`.
//!
//! Regression for #571: a native that writes through an exit and then
//! suspends for a choice is `Rejected` in every build. A native chooses before
//! it writes; the apply's snapshot rollback undoes the rejected native's write.
//! One that suspends before any write suspends and resumes normally.
//!
//! And a native resolves in the order its author wrote it (ADR 0020): once an
//! exit leaves work pending (a deal of harm), every later exit waits for that
//! work to resolve, and a read of the game made while it waits is `Rejected`
//! and rolled back.
//!
//! Each native sits on its own mock act as a forced ability at the end of the
//! enemy phase, the `native_effect.rs` idiom. The natives model the contract,
//! not printed cards (ADR 0016).
//!
//! Its own test binary: the mock registry is built once per process, and the
//! sibling exit tests (`spend_uses.rs`, `native_exits.rs`) build their own.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot};
use card_dsl::dsl::{self, Ability, EventPattern, EventTiming, InvestigatorTarget};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx, OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    self, Act, Agenda, CardCode, EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId,
    Owner,
};
use game_core::test_support::{self, seat, MockRegistry, Seat, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const WRITE_THEN_SUSPEND_ACT: &str = "_ncx_act_write_then_suspend";
const SUSPEND_THEN_WRITE_ACT: &str = "_ncx_act_suspend_then_write";
const HARM_THEN_WRITE_ACT: &str = "_ncx_act_harm_then_write";
const HARM_THEN_HARM_ACT: &str = "_ncx_act_harm_then_harm";
const HARM_THEN_READ_ACT: &str = "_ncx_act_harm_then_read";
const HARM_THEN_TAKE_ACT: &str = "_ncx_act_harm_then_take";
const DEFEAT_THEN_WRITE_ACT: &str = "_ncx_act_defeat_then_write";
const THREE_HARMS_ACT: &str = "_ncx_act_three_harms";
const HARM_THEN_DOOM_ACT: &str = "_ncx_act_harm_then_doom";
const HARM_THEN_BAD_ENTRY_ACT: &str = "_ncx_act_harm_then_bad_entry";

const WRITE_THEN_SUSPEND: &str = "test:write-then-suspend";
const HARM_THEN_READ: &str = "test:harm-then-read";
const HARM_THEN_TAKE: &str = "test:harm-then-take";
const HARM_THEN_BAD_ENTRY: &str = "test:harm-then-bad-entry";

/// An ally with 2 health, so a deal of damage asks where its point goes.
const SOAKER: &str = "_ncx_soaker";
/// A code no set-aside zone holds.
const NOT_SET_ASIDE: &str = "_ncx_not_set_aside";

/// The two locations the natives offer as a choice.
const NEAR: LocationId = LocationId(10);
const FAR: LocationId = LocationId(11);

/// An enemy two damage defeats, out of everyone's way at [`FAR`].
const FOE: EnemyId = EnemyId(5);

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

/// An ally with `health` and no other text.
fn soaking_ally(code: &str, health: u8) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: "Soaker".to_owned(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_owned(),
        weakness: None,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![Slot::Ally],
            health: Some(health),
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

/// Offer the controller a pick between [`NEAR`] and [`FAR`].
fn pick_location(cx: &NativeCx, ctx: &EvalContext) -> Grounded<LocationId> {
    engine::resolve_grounded_choice(
        cx.state(),
        cx.cards(),
        ctx,
        &[NEAR, FAR],
        "Choose a location",
        |&loc| OptionTarget::Location(loc),
    )
}

/// The contract violation: place doom, *then* ask for a location.
fn write_then_suspend(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.place_doom_on_agenda(3);
    match pick_location(cx, ctx) {
        Grounded::Picked(loc) => {
            cx.place_investigator(ctx.controller, loc);
            EngineOutcome::Done
        }
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(suspension) => cx.suspend(suspension),
    }
}

/// The contract kept: ask for a location, then place doom and the
/// investigator there.
fn suspend_then_write(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    match pick_location(cx, ctx) {
        Grounded::Picked(loc) => {
            cx.place_doom_on_agenda(3);
            cx.place_investigator(ctx.controller, loc);
            EngineOutcome::Done
        }
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(suspension) => cx.suspend(suspension),
    }
}

/// Deal 1 damage, then discard every resource: "take harm, then X".
fn harm_then_write(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let dealt = cx.deal_harm(ctx.controller, 1, 0);
    cx.discard_all_resources(ctx.controller);
    dealt
}

/// Deal 1 horror, then 1 damage, written forward.
fn harm_then_harm(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let first = cx.deal_harm(ctx.controller, 0, 1);
    if !matches!(first, EngineOutcome::Done) {
        return first;
    }
    cx.deal_harm(ctx.controller, 1, 0)
}

/// Deal 1 damage, then read the board the pending deal hasn't settled.
fn harm_then_read(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let dealt = cx.deal_harm(ctx.controller, 1, 0);
    let _damage = cx.state().investigators[&ctx.controller].damage();
    dealt
}

/// Deal 1 damage, then take the encounter deck's top card: an exit that
/// returns a value nobody has computed yet.
fn harm_then_take(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let dealt = cx.deal_harm(ctx.controller, 1, 0);
    let _top = cx.take_encounter_top();
    dealt
}

/// Defeat [`FOE`], which queues its after-defeat window, then discard every
/// resource.
fn defeat_then_write(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.deal_damage_to_enemy(FOE, 2, Some(ctx.controller));
    cx.discard_all_resources(ctx.controller);
    EngineOutcome::Done
}

/// Deal 1 damage, 1 horror and 2 damage, in that order.
fn three_harms(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    for (damage, horror) in [(1, 0), (0, 1), (2, 0)] {
        let dealt = cx.deal_harm(ctx.controller, damage, horror);
        if !matches!(dealt, EngineOutcome::Done) {
            return dealt;
        }
    }
    EngineOutcome::Done
}

/// Deal 1 damage, then place 3 doom on the agenda.
fn harm_then_doom(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let dealt = cx.deal_harm(ctx.controller, 1, 0);
    cx.place_doom_on_agenda(3);
    dealt
}

/// Deal 1 damage, then put into play a card nothing set aside: an op that
/// turns out invalid only when it runs.
fn harm_then_bad_entry(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let dealt = cx.deal_harm(ctx.controller, 1, 0);
    let _entered = cx.put_set_aside_card_into_play(NOT_SET_ASIDE, None, Owner::EncounterDeck);
    dealt
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(soaking_ally(SOAKER, 2))
            .with_abilities(WRITE_THEN_SUSPEND_ACT, || forced_native(WRITE_THEN_SUSPEND))
            .with_native_effect(WRITE_THEN_SUSPEND, write_then_suspend)
            .with_abilities(SUSPEND_THEN_WRITE_ACT, || {
                forced_native("test:suspend-then-write")
            })
            .with_native_effect("test:suspend-then-write", suspend_then_write)
            .with_abilities(HARM_THEN_WRITE_ACT, || {
                forced_native("test:harm-then-write")
            })
            .with_native_effect("test:harm-then-write", harm_then_write)
            .with_abilities(HARM_THEN_HARM_ACT, || forced_native("test:harm-then-harm"))
            .with_native_effect("test:harm-then-harm", harm_then_harm)
            .with_abilities(HARM_THEN_READ_ACT, || forced_native(HARM_THEN_READ))
            .with_native_effect(HARM_THEN_READ, harm_then_read)
            .with_abilities(HARM_THEN_TAKE_ACT, || forced_native(HARM_THEN_TAKE))
            .with_native_effect(HARM_THEN_TAKE, harm_then_take)
            .with_abilities(DEFEAT_THEN_WRITE_ACT, || {
                let mut abilities = forced_native("test:defeat-then-write");
                abilities.push(dsl::forced_on_event(
                    EventPattern::EnemyDefeated {
                        by_controller: false,
                        code: None,
                    },
                    EventTiming::After,
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                ));
                abilities
            })
            .with_native_effect("test:defeat-then-write", defeat_then_write)
            .with_abilities(THREE_HARMS_ACT, || forced_native("test:three-harms"))
            .with_native_effect("test:three-harms", three_harms)
            .with_abilities(HARM_THEN_DOOM_ACT, || forced_native("test:harm-then-doom"))
            .with_native_effect("test:harm-then-doom", harm_then_doom)
            .with_abilities(HARM_THEN_BAD_ENTRY_ACT, || {
                forced_native(HARM_THEN_BAD_ENTRY)
            })
            .with_native_effect(HARM_THEN_BAD_ENTRY, harm_then_bad_entry)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
/// The investigator's starting location, the third on the board.
const HERE: LocationId = LocationId(1);

/// The lead's open turn at a third location, [`NEAR`] and [`FAR`] on the
/// board, an agenda in play, and `act` as the current act.
fn state_with_act(act: &str) -> GameState {
    state_with(act, seat(1), GameStateBuilder::new())
}

/// [`state_with_act`], seating `seat` (placed [`HERE`], with a filler deck) on
/// `builder`. The filler deck keeps Upkeep 4.4's draw from dealing empty-deck
/// horror once the enemy phase has ended.
fn state_with(act: &str, seat: Seat, builder: GameStateBuilder) -> GameState {
    let mut state = builder
        .with_registries(registries())
        .with_seat(seat.at(HERE).deck(["filler-card"]))
        .with_location(test_support::test_location(HERE.0, "Start"))
        .with_location(test_support::test_location(NEAR.0, "Near"))
        .with_location(test_support::test_location(FAR.0, "Far"))
        .open_turn(INV)
        .build();
    // No builder setter stages the act and agenda decks.
    state.act_deck = vec![Act {
        code: CardCode::new(act),
        clue_threshold: 99,
    }];
    state.act_index = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("TEST-AGENDA"),
        doom_threshold: 10,
    }];
    state
}

/// End the lead's turn, and with it the Investigation phase, so play runs into
/// the Enemy phase and its end, where the act's forced native fires.
fn end_enemy_phase(session: TestSession) -> TestSession {
    session.take(&TurnAction::EndTurn)
}

/// Answer the soak prompt of a 1-damage deal by placing the point on the
/// investigator.
fn take_the_point_myself(session: TestSession) -> TestSession {
    let me = session.state().investigators[&INV].card_anchor();
    session.pick(me)
}

#[test]
fn a_native_that_writes_then_suspends_is_rejected_and_its_write_rolled_back() {
    let session = TestSession::new(state_with_act(WRITE_THEN_SUSPEND_ACT), registries());
    let before = session.state().clone();

    let session = end_enemy_phase(session);

    let reason = session.expect_rejected();
    assert!(
        reason.contains(WRITE_THEN_SUSPEND),
        "the rejection names the native: {reason}"
    );
    assert_eq!(
        session.state(),
        &before,
        "the doom the native placed before suspending is rolled back"
    );
}

#[test]
fn a_native_that_suspends_before_writing_suspends_and_resumes() {
    let session = end_enemy_phase(TestSession::new(
        state_with_act(SUSPEND_THEN_WRITE_ACT),
        registries(),
    ));
    assert_eq!(session.prompt().options.len(), 2, "both locations offered");
    assert_eq!(session.state().agenda_doom, 0, "nothing written yet");

    let session = session.pick(OptionTarget::Location(FAR));

    // The enemy phase ends and play goes on into the next round's Mythos
    // phase, whose step 1.2 places 1 doom of its own.
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: state::Phase::Enemy
        }
    );
    assert_eq!(session.state().agenda_doom, 3 + 1);
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(FAR)
    );
}

#[test]
fn a_direct_write_called_after_deal_harm_lands_only_after_the_soak_is_answered() {
    let session = end_enemy_phase(TestSession::new(
        state_with(
            HARM_THEN_WRITE_ACT,
            seat(1).in_play([SOAKER]).resources(2),
            GameStateBuilder::new(),
        ),
        registries(),
    ));

    // Resting at the soak: the discard written after the deal hasn't landed.
    assert!(session.rejection().is_none(), "{:?}", session.rejection());
    assert_eq!(session.state().investigators[&INV].resources, 2);
    assert_no_event!(session.events(), Event::ResourcesDiscarded { .. });

    let session = take_the_point_myself(session);

    assert_event_sequence!(
        session.events(),
        Event::DamagePlacedOnInvestigator {
            investigator: INV,
            amount: 1
        },
        Event::ResourcesDiscarded {
            investigator: INV,
            amount: 2
        },
    );
}

#[test]
fn two_deal_harms_resolve_in_the_order_they_were_called() {
    let session = end_enemy_phase(TestSession::new(
        state_with_act(HARM_THEN_HARM_ACT),
        registries(),
    ));

    assert!(session.rejection().is_none(), "{:?}", session.rejection());
    assert_event_sequence!(
        session.events(),
        Event::HorrorPlacedOnInvestigator {
            investigator: INV,
            amount: 1
        },
        Event::DamagePlacedOnInvestigator {
            investigator: INV,
            amount: 1
        },
    );
}

#[test]
fn a_state_read_after_deal_harm_is_rejected_and_rolled_back() {
    let session = TestSession::new(state_with_act(HARM_THEN_READ_ACT), registries());
    let before = session.state().clone();

    let session = end_enemy_phase(session);

    let reason = session.expect_rejected();
    assert!(
        reason.contains(HARM_THEN_READ),
        "the rejection names the native: {reason}"
    );
    assert_eq!(session.state(), &before, "the enemy phase is rolled back");
}

#[test]
fn a_value_returning_exit_after_deal_harm_is_rejected_and_rolled_back() {
    let session = TestSession::new(state_with_act(HARM_THEN_TAKE_ACT), registries());
    let before = session.state().clone();

    let session = end_enemy_phase(session);

    let reason = session.expect_rejected();
    assert!(
        reason.contains(HARM_THEN_TAKE),
        "the rejection names the native: {reason}"
    );
    assert_eq!(session.state(), &before, "the enemy phase is rolled back");
}

#[test]
fn an_exit_after_an_enemy_defeat_lands_before_the_after_defeat_window_opens() {
    let mut foe = test_support::test_enemy(FOE.0, "Foe");
    foe.current_location = Some(FAR);
    let session = end_enemy_phase(TestSession::new(
        state_with(
            DEFEAT_THEN_WRITE_ACT,
            seat(1).resources(2),
            GameStateBuilder::new().with_enemy(foe),
        ),
        registries(),
    ));

    assert!(session.rejection().is_none(), "{:?}", session.rejection());
    // Both resources are discarded, and then the after-defeat Forced gains 1;
    // had the window opened first, the discard would have taken 3.
    assert_event_sequence!(
        session.events(),
        Event::EnemyDefeated { enemy: FOE, .. },
        Event::ResourcesDiscarded {
            investigator: INV,
            amount: 2
        },
        Event::ResourcesGained {
            investigator: INV,
            amount: 1
        },
    );
}

#[test]
fn three_deal_harms_resolve_in_the_order_they_were_called() {
    let session = end_enemy_phase(TestSession::new(
        state_with_act(THREE_HARMS_ACT),
        registries(),
    ));

    assert!(session.rejection().is_none(), "{:?}", session.rejection());
    assert_event_sequence!(
        session.events(),
        Event::DamagePlacedOnInvestigator {
            investigator: INV,
            amount: 1
        },
        Event::HorrorPlacedOnInvestigator {
            investigator: INV,
            amount: 1
        },
        Event::DamagePlacedOnInvestigator {
            investigator: INV,
            amount: 2
        },
    );
}

#[test]
fn the_rest_of_a_native_completes_after_its_deal_ends_the_scenario() {
    // One damage from defeat: the deal eliminates the sole investigator, and
    // the scenario's ending latches mid-native.
    let session = end_enemy_phase(TestSession::new(
        state_with(
            HARM_THEN_DOOM_ACT,
            seat(1).harm(7, 0),
            GameStateBuilder::new(),
        ),
        registries(),
    ));

    assert!(session.rejection().is_none(), "{:?}", session.rejection());
    assert_event!(session.events(), Event::AllInvestigatorsEliminated);
    assert_event!(session.events(), Event::ScenarioResolved { .. });
    assert_eq!(
        session.state().agenda_doom,
        3,
        "the doom written after the deal lands though the scenario has ended"
    );
}

#[test]
fn a_deferred_exit_that_turns_out_invalid_rejects_the_apply_and_rolls_back() {
    let session = end_enemy_phase(TestSession::new(
        state_with(
            HARM_THEN_BAD_ENTRY_ACT,
            seat(1).in_play([SOAKER]),
            GameStateBuilder::new(),
        ),
        registries(),
    ));
    // The entry waits behind the deal, so the soak is asked first.
    assert!(session.rejection().is_none(), "{:?}", session.rejection());
    let at_soak = session.state().clone();

    let session = take_the_point_myself(session);

    let reason = session.expect_rejected();
    assert!(
        reason.contains(NOT_SET_ASIDE),
        "the rejection names the card: {reason}"
    );
    assert_eq!(
        session.state(),
        &at_soak,
        "the damage placed by the answered soak is rolled back"
    );
}
