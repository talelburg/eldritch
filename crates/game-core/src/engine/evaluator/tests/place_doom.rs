use super::*;

/// A two-agenda fixture with the given doom threshold on the current one.
fn state_with_agenda(threshold: u8) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.agenda_deck = vec![
        Agenda {
            code: CardCode("ag1".into()),
            doom_threshold: threshold,
        },
        Agenda {
            code: CardCode("ag2".into()),
            doom_threshold: 9,
        },
    ];
    state
}

#[test]
fn place_doom_on_current_agenda_below_threshold_only_places() {
    let mut state = state_with_agenda(3);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::PlaceDoomOnCurrentAgenda {
            count: IntExpr::Lit(1),
            may_advance: true,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 1);
    assert_eq!(state.agenda_index, 0, "1 of 3 doom does not advance");
}

#[test]
fn place_doom_on_current_agenda_advances_at_threshold() {
    let mut state = state_with_agenda(1);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::PlaceDoomOnCurrentAgenda {
            count: IntExpr::Lit(1),
            may_advance: true,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    // The advance is deferred to an AdvanceReverse frame (#482); drive it
    // (no registry ⇒ the reverse fires nothing ⇒ it drives straight through).
    dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(
        state.agenda_index, 1,
        "Ancient Evils 01166: `This effect can cause the current agenda to advance`"
    );
    assert_eq!(state.agenda_doom, 0, "doom reset on advance");
}

/// The `may_advance: false` half — Silver Twilight Acolyte 01102's bare
/// *"Place 1 doom on the current agenda."*, which
/// `data/rules-reference/rules/glossary/Doom.md` leaves waiting for Mythos:
///
/// > Unless a card otherwise specifies that it can advance the agenda, this
/// > is the only time at which the agenda can advance.
///
/// So the doom lands and stays landed, even sitting on the threshold.
#[test]
fn place_doom_without_the_advance_clause_does_not_advance_at_threshold() {
    let mut state = state_with_agenda(1);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::PlaceDoomOnCurrentAgenda {
            count: IntExpr::Lit(1),
            may_advance: false,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(
        state.agenda_index, 0,
        "no printed advance clause ⇒ the threshold check waits for Mythos 1.3"
    );
    assert_eq!(state.agenda_doom, 1, "the doom is placed, and stays");
}

/// Offer of Power 01178's *"place 2 doom"* is one placement of two, so the
/// threshold is checked once, after both — the second doom is not placed on
/// an agenda the first already advanced past.
#[test]
fn place_doom_places_the_whole_count_before_checking_the_threshold() {
    let mut state = state_with_agenda(2);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::PlaceDoomOnCurrentAgenda {
            count: IntExpr::Lit(2),
            may_advance: true,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(state.agenda_index, 1, "both doom landed, then it advanced");
    assert_eq!(state.agenda_doom, 0);
}

/// A computed count is read at resolution, not at declaration — the reason
/// the variant carries an [`IntExpr`] rather than a `u8`.
#[test]
fn place_doom_evaluates_a_computed_count() {
    let mut state = state_with_agenda(9);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let mut eval_ctx = EvalContext::for_controller(InvestigatorId(1));
    eval_ctx.set_failed_by(3);
    let out = run(
        &mut cx,
        &Effect::PlaceDoomOnCurrentAgenda {
            count: IntExpr::Count(Quantity::SkillTestFailedBy),
            may_advance: true,
        },
        eval_ctx,
    );
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 3);
}

/// The point of the variant over a card-local `Native` tag (#716): it
/// nests. Saracenic Script 02240's act back places its doom as the last
/// step of a `Seq`; Offer of Power 01178 places 2 inside a `ChooseOne`
/// branch; Blood on the Altar 02195 places 1 under an `If`-on-failure.
#[test]
fn place_doom_composes_as_a_sub_effect() {
    let mut state = state_with_agenda(9);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::Seq(vec![
            Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(1),
                may_advance: true,
            },
            Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(2),
                may_advance: true,
            },
        ]),
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 3);
}

/// The `ChooseOne` and `If` halves of the nesting claim. `ChooseOne` with a
/// single branch auto-resolves, so this needs no input round-trip.
#[test]
fn place_doom_nests_in_choose_one_and_if() {
    let mut state = state_with_agenda(9);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    let place = |n| Effect::PlaceDoomOnCurrentAgenda {
        count: IntExpr::Lit(n),
        may_advance: true,
    };
    assert_eq!(
        run(&mut cx, &dsl::choose_one([("Place 1 doom", place(1))]), ctx),
        EngineOutcome::Done
    );
    assert_eq!(
        run(
            &mut cx,
            &Effect::If {
                // No `Condition::Always`; a tautology stands in.
                condition: Condition::Compare {
                    quantity: Quantity::CluesAtControllerLocation,
                    op: CmpOp::Ge,
                    value: 0,
                },
                then: Box::new(place(1)),
                else_: None,
            },
            ctx,
        ),
        EngineOutcome::Done
    );
    dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 2, "both nestings placed their doom");
}

/// A zero count is a full no-op, threshold check included: it must not
/// advance an agenda already sitting at its threshold on the strength of
/// doom it never placed.
#[test]
fn place_doom_of_zero_does_not_run_the_threshold_check() {
    let mut state = state_with_agenda(1);
    state.agenda_doom = 1;
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::PlaceDoomOnCurrentAgenda {
            count: IntExpr::Lit(0),
            may_advance: true,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(
        state.agenda_index, 0,
        "no placement ⇒ no check ⇒ no advance"
    );
    assert_eq!(state.agenda_doom, 1);
}

#[test]
fn place_doom_without_an_agenda_deck_is_a_no_op() {
    let mut state = GameStateBuilder::new()
        .with_turn_order([InvestigatorId(1)])
        .build();
    assert!(state.agenda_deck.is_empty());
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::PlaceDoomOnCurrentAgenda {
            count: IntExpr::Lit(1),
            may_advance: true,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done, "no agenda modeled ⇒ no rejection");
    assert_eq!(state.agenda_doom, 0);
}
