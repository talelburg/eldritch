use super::*;

#[test]
fn eval_context_defaults_clue_discovery_count_to_none() {
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    assert_eq!(ctx.clue_discovery_count(), None);
}

#[test]
fn eval_context_round_trips_with_grouped_bindings() {
    let mut ctx = EvalContext::for_controller(InvestigatorId(1));
    ctx.set_failed_by(3);
    ctx.set_chosen_investigator(InvestigatorId(2));
    let json = serde_json::to_string(&ctx).expect("serialize");
    let back: EvalContext = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.failed_by(), Some(3));
    assert_eq!(back.chosen_investigator(), Some(InvestigatorId(2)));
    assert_eq!(back.attacking_enemy(), None);
    assert_eq!(back.chosen_option(), None);
}

/// The context rides `GameState` inside a `Continuation::Effect` frame, so
/// the collapsed source is part of the serialized shape. #834 broke that
/// payload deliberately and without a migration — the #707 / #709 / #735
/// posture, since persisted games are discarded and schema versioning is
/// #581 — which is what makes the shape worth pinning.
///
/// A **board** source is the case the collapse exists for: an act carries
/// no `CardInstanceId`, so before #834 it was thrown away on the way in and
/// `source` came back `None`.
#[test]
fn eval_context_round_trips_a_board_source_the_instance_projection_would_lose() {
    let ctx = EvalContext::for_controller_with_source(InvestigatorId(1), AbilitySource::Act);
    let json = serde_json::to_string(&ctx).expect("serialize");
    let back: EvalContext = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.ability_source, Some(AbilitySource::Act));
    assert_eq!(
        back.source_instance(),
        None,
        "the act has no card instance, and the projection says so rather than inventing one",
    );
}

/// The in-play case: the projection is the instance the descriptor names,
/// so `DiscardSelf` and recorded-modifier provenance read exactly what they
/// read before the collapse.
#[test]
fn an_in_play_source_projects_to_its_own_instance() {
    let ctx = EvalContext::for_controller_with_source(
        InvestigatorId(1),
        AbilitySource::InPlay(CardInstanceId(7)),
    );
    assert_eq!(ctx.source_instance(), Some(CardInstanceId(7)));
}
