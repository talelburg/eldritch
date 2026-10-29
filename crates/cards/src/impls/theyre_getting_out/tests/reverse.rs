use super::*;

/// Run the act-3 body for `investigator`, bound the way the reverse's
/// `ForEach` binds it, assert it finished, and return its events.
fn run_body_for(state: &mut GameState, investigator: InvestigatorId) -> Vec<Event> {
    let mut ctx = EvalContext::for_controller(InvestigatorId(1));
    ctx.set_each(EachBinding::Investigator(investigator));
    let (out, events) = test_support::run_native(state, &REGISTRY, defeated_with_trauma, &ctx);
    assert_eq!(out, EngineOutcome::Done);
    events
}

/// The investigators announced defeated, in the order the events landed.
fn defeat_order(events: &[Event]) -> Vec<InvestigatorId> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::InvestigatorEliminated { investigator, .. } => Some(*investigator),
            _ => None,
        })
        .collect()
}

/// Investigators 1 and 2, seated in that order.
fn two_investigators() -> GameState {
    GameStateBuilder::new()
        .with_registries(crate::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_seating([InvestigatorId(1), InvestigatorId(2)])
        .build()
}

#[test]
fn abilities_are_the_two_forced_natives_then_the_reverse() {
    let abilities = abilities();
    assert_eq!(abilities.len(), 3);
    assert_eq!(
        abilities[0].trigger,
        Trigger::OnEvent {
            pattern: EventPattern::PhaseEnded {
                phase: Phase::Enemy
            },
            timing: EventTiming::At,
            kind: TriggerKind::Forced,
        }
    );
    assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == MOVE_GHOULS));
    assert_eq!(
        abilities[1].trigger,
        Trigger::OnEvent {
            pattern: EventPattern::RoundEnded,
            timing: EventTiming::At,
            kind: TriggerKind::Forced,
        }
    );
    assert!(matches!(&abilities[1].effect, Effect::Native { tag } if tag == ROUND_END_DOOM));
}

/// The reverse branches on the `after` cell of the agenda's own advance
/// (ADR 0013): at act 3 the ghouls run rampant, and **otherwise** — the
/// card's act-1-or-2 half — it reaches the printed `(→R3)`.
#[test]
fn reverse_branches_on_the_act_deck_after_the_agenda_advances() {
    let abilities = abilities();
    assert_eq!(
        abilities[2].trigger,
        Trigger::OnEvent {
            pattern: EventPattern::AgendaAdvanced,
            timing: EventTiming::After,
            kind: TriggerKind::Forced,
        }
    );
    let Effect::If {
        condition,
        then,
        else_,
    } = &abilities[2].effect
    else {
        panic!("the reverse is a branch, got {:?}", abilities[2].effect);
    };
    assert!(
        matches!(condition, Condition::Native { tag } if tag == AT_ACT_THREE),
        "branches on the card-local act-3 predicate, got {condition:?}",
    );
    let Effect::ForEach { targets, body } = &**then else {
        panic!("act 3 fans out over the investigators, got {then:?}");
    };
    assert_eq!(
        *targets,
        ForEachTargets::Investigators(InvestigatorTargetSet::All),
        "act 3: each investigator",
    );
    assert!(
        matches!(&**body, Effect::Native { tag } if tag == DEFEATED_WITH_TRAUMA),
        "act 3: each is defeated and suffers trauma, got {body:?}",
    );
    assert!(
        matches!(else_.as_deref(), Some(Effect::ReachResolution(3))),
        "act 1 or 2: the printed (→R3), got {else_:?}",
    );
}

/// *"If the investigators are at Act 3"* against a zero-based cursor.
#[test]
fn the_act_three_predicate_reads_the_zero_based_cursor() {
    let mut state = GameStateBuilder::new().build();
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    for (act_index, expected) in [(0, false), (1, false), (ACT_THREE_INDEX, true)] {
        state.act_index = act_index;
        assert_eq!(
            at_act_three(&state, &ctx),
            expected,
            "act_index {act_index} (the card's Act {})",
            act_index + 1,
        );
    }
}

/// The act-3 body, for the investigator it runs for: defeated by a card
/// ability, announcing 1 physical trauma, and nobody else touched. The walk
/// over the table — player order, the resigned seat skipped, and the
/// no-resolution ending — is pinned end to end in
/// `tests/theyre_getting_out.rs`.
#[test]
fn the_act_three_body_defeats_its_investigator_with_one_physical_trauma() {
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let mut state = two_investigators();

    let events = run_body_for(&mut state, b);

    assert_eq!(
        state.investigators[&b].status,
        Status::Defeated,
        "defeated by a card ability — neither killed nor driven insane",
    );
    assert_eq!(state.investigators[&a].status, Status::Active);
    assert_eq!(defeat_order(&events), vec![b]);
    assert!(
        events.iter().any(|e| matches!(e, Event::TraumaSuffered {
            investigator, kind, amount
        } if *investigator == b
            && *kind == TraumaKind::Physical
            && *amount == 1)),
        "the defeated investigator suffers 1 physical trauma",
    );
}

/// *"Each investigator that has not resigned"* — run for a resigned
/// investigator, the body leaves them alone and announces no trauma.
#[test]
fn an_investigator_who_has_resigned_is_left_alone() {
    let a = InvestigatorId(1);
    let mut state = two_investigators();
    state.investigators.get_mut(&a).expect("seated").status = Status::Resigned;

    let events = run_body_for(&mut state, a);

    assert_eq!(state.investigators[&a].status, Status::Resigned);
    assert!(defeat_order(&events).is_empty());
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::TraumaSuffered { investigator, .. } if *investigator == a
        )),
        "no trauma announced for an investigator this card never defeated",
    );
}
