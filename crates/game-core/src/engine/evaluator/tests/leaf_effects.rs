use super::*;
use crate::state::FastWindowFrame;

#[test]
fn cancel_effect_sets_pending_cancellation() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    // Effect::Cancel asserts an open window frame is present; push a minimal one.
    state.continuations.push(FastWindowFrame {
        candidates: Vec::new(),
        fast_actors: FastActorScope::Any,
        kind: FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
    });
    assert!(!state.pending_cancellation);
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &Effect::Cancel,
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(state.pending_cancellation);
}

/// `Effect::BoostAttackDamage` accumulates onto the in-flight test's
/// `bonus_attack_damage`; repeated applications stack. A no-op with no
/// in-flight test.
#[test]
fn boost_attack_damage_accumulates_on_in_flight_test() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();

    // No in-flight test: a clean no-op (no panic, nothing to mutate).
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::boost_attack_damage(1),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);

    state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            id: SkillTestId(0),
            investigator: InvestigatorId(1),
            skill: SkillKind::Combat,
            kind: SkillTestKind::Fight,
            difficulty_basis: DifficultyBasis::Fixed(3),
            committed_by_active: Vec::new(),
            tested_location: None,
            follow_up: SkillTestFollowUp::None,
            on_fail: None,
            on_success: None,
            source: None,
            continuation: SkillTestStep::AwaitingCommit,
            bonus_attack_damage: 0,
            bonus_clues_discovered: 0,
            resolved: None,
            symbol_on_fail: None,
        }));

    for _ in 0..2 {
        run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &dsl::boost_attack_damage(1),
            ctx(1),
        );
    }
    assert_eq!(
        state.current_skill_test().unwrap().bonus_attack_damage,
        2,
        "two BoostAttackDamage(1) applications should stack to 2"
    );
}

/// `Effect::DiscoverAdditionalClues` accumulates onto the in-flight test's
/// `bonus_clues_discovered`; repeated applications stack (two copies of
/// Deduction 01039 committed to one investigation). A no-op with no
/// in-flight test.
#[test]
fn discover_additional_clues_accumulates_on_in_flight_test() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();

    // No in-flight test: a clean no-op (no panic, nothing to mutate).
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::discover_additional_clues(1),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);

    state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            id: SkillTestId(0),
            investigator: InvestigatorId(1),
            skill: SkillKind::Intellect,
            kind: SkillTestKind::Investigate,
            difficulty_basis: DifficultyBasis::Fixed(3),
            committed_by_active: Vec::new(),
            tested_location: None,
            follow_up: SkillTestFollowUp::Investigate,
            on_fail: None,
            on_success: None,
            source: None,
            continuation: SkillTestStep::AwaitingCommit,
            bonus_attack_damage: 0,
            bonus_clues_discovered: 0,
            resolved: None,
            symbol_on_fail: None,
        }));

    for _ in 0..2 {
        run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &dsl::discover_additional_clues(1),
            ctx(1),
        );
    }
    assert_eq!(
        state.current_skill_test().unwrap().bonus_clues_discovered,
        2,
        "two DiscoverAdditionalClues(1) applications should stack to 2"
    );
}

/// `Effect::DrawCards` moves `count` cards deck→hand for the resolved
/// target and emits `CardsDrawn`; `count == 0` is a no-op.
#[test]
fn draw_cards_effect_draws_for_target() {
    let mut inv = test_support::test_investigator(1);
    inv.deck = vec![
        CardCode::new("d1"),
        CardCode::new("d2"),
        CardCode::new("d3"),
    ];
    inv.hand = Vec::new();
    let mut state = GameStateBuilder::new().with_investigator(inv).build();
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::draw_cards(InvestigatorTarget::You, 2),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    let inv_after = &state.investigators[&InvestigatorId(1)];
    assert_eq!(inv_after.hand.len(), 2, "two cards moved into hand");
    assert_eq!(inv_after.deck.len(), 1, "two cards left the deck");
    assert_event!(events, Event::CardsDrawn { count: 2, .. });

    // count == 0 → clean no-op (no further draw, no event).
    let mut events0 = Vec::new();
    run(
        &mut Cx {
            state: &mut state,
            events: &mut events0,
        },
        &dsl::draw_cards(InvestigatorTarget::You, 0),
        ctx(1),
    );
    assert_eq!(state.investigators[&InvestigatorId(1)].hand.len(), 2);
    assert!(events0.is_empty());
}

#[test]
fn attach_self_to_location_rejects_with_no_pending_event() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &Effect::AttachSelfToLocation,
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
}

#[test]
fn put_into_threat_area_with_clues_seeds_the_placed_instance() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = run(
        &mut cx,
        &dsl::put_into_threat_area_with_clues("01007", 3),
        EvalContext::for_revelation(id, Owner::Investigator(id)),
    );
    assert!(matches!(outcome, EngineOutcome::Done));
    let placed = state.investigators[&id]
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == "01007")
        .expect("Cover Up placed in threat area");
    assert_eq!(placed.clues, 3, "Cover Up enters with 3 clues");
}

/// The card a Revelation puts into play is the revealed card, so it takes the
/// revealed card's owner — whichever deck it was revealed from.
#[test]
fn put_into_threat_area_records_the_revealed_cards_owner() {
    for owner in [Owner::Investigator(InvestigatorId(1)), Owner::EncounterDeck] {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_support::test_investigator(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = run(
            &mut cx,
            &dsl::put_into_threat_area("01164"),
            EvalContext::for_revelation(InvestigatorId(1), owner),
        );
        assert!(matches!(outcome, EngineOutcome::Done));
        assert_eq!(
            state.investigators[&InvestigatorId(1)].threat_area[0].owner,
            owner
        );
    }
}

/// Outside a Revelation nothing says who owns the card, and an owner is never
/// guessed: the effect rejects rather than minting an ownerless instance.
#[test]
fn put_into_threat_area_outside_a_revelation_rejects() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::put_into_threat_area("01164"),
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(state.investigators[&InvestigatorId(1)]
        .threat_area
        .is_empty());
}
