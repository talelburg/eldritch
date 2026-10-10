//! The Investigate basic action, its candidates, the investigate perform
//! shared with the designated Investigate, and its after-test clue discovery.

use card_dsl::dsl::{self, IntExpr, LocationTarget, SkillTestKind, Stat};

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::skill_test;
use crate::engine::dispatch::skill_test::InitiatorModifier;
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::state::{
    AbilitySource, ActionResume, DifficultyBasis, GameState, InvestigatorId, LocationId,
    ModifierTarget, SkillKind, SkillTestFollowUp,
};

/// The location an Investigate would test: `investigator`'s current location,
/// if it exists and is revealed. `glossary/Investigate_Action.md`: *"he or she
/// makes an intellect test against the shroud value of that location"*. At most
/// one, so an `Option`.
///
/// `None` is the ineligible or lapsed case: it reads as a rejection in the
/// basic action and pre-cost in the designator gate (`designator::can_perform`),
/// as a suppression on resume, and as no menu entry. It never panics; a
/// dangling `current_location` is the handler's to surface loudly, ahead of
/// asking this. Every Investigate path reads it, the designated
/// **Investigate**'s perform included, so they agree on what a location has to
/// be to investigate it (#805).
pub(crate) fn candidates(state: &GameState, investigator: InvestigatorId) -> Option<LocationId> {
    state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location)
        .filter(|id| state.locations.get(id).is_some_and(|loc| loc.revealed))
}

/// Handler for `TurnAction::Investigate`.
///
/// Spends 1 action, runs an intellect skill test against the location's
/// shroud, and on success applies [`Effect::DiscoverClue`] to move 1
/// clue from the location to the investigator. The discover-clue
/// evaluator handles the location-empty edge case as a silent no-op,
/// so an investigation at a 0-clue location costs the action and runs
/// the test but yields nothing. That is the printed rule, and in
/// particular **a clueless location is not an illegal target** — no
/// gate here counts clues, deliberately.
/// `data/official-faq/Frequently_Asked_Questions.md`:
///
/// > Q: Can I investigate a location with no clues on it? If I do, what
/// > happens?
/// >
/// > A: Yes. You can investigate a location even if there are no clues on
/// > it. However, you won't be able to discover any clues there, because
/// > there are no clues on the location to discover. Investigating a
/// > location with no clues might still be useful to trigger card
/// > abilities such as Burglary (\[core\] 45) or Scavenging (\[core\] 73).
///
/// The last sentence is why the "no potential to change the game state"
/// initiation gate (`glossary/Ability.md`) must not be read as a clue
/// check on the basic action: the test itself is the state change other
/// cards key off.
///
/// Card-derived investigate variants (Rite of Seeking's "Action:
/// Investigate using willpower instead of intellect", Working a
/// Hunch's discover-without-test) implement their own paths; this
/// handler is the bare turn-action.
///
/// Validate-first: the investigator may take the action ([`take::check`]),
/// then the location checks. Then take it ([`take::take`]). Investigate is not
/// on the attack-of-opportunity exempt list, so each ready engaged enemy
/// attacks before the skill test, which [`perform`] starts.
///
/// [`Effect::DiscoverClue`]: card_dsl::dsl::Effect::DiscoverClue
pub(in crate::engine::dispatch) fn handle(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let description = ActionDescription::basic(ActionKind::Investigate);
    if let Err(reason) = take::check(cx.state, investigator, &description) {
        return EngineOutcome::Rejected { reason };
    }
    let inv = &cx.state.investigators[&investigator];
    let Some(location_id) = inv.current_location else {
        return EngineOutcome::Rejected {
            reason: format!("Investigate: {investigator:?} has no current_location to investigate")
                .into(),
        };
    };
    // A `current_location` that doesn't exist in `state.locations` is
    // a state-corruption invariant violation, not a user-facing
    // rejection — match `end_turn` and `rotate_to_active` and surface
    // it loudly.
    // It panics ahead of the candidates check, which is empty on corrupt state
    // rather than panicking.
    assert!(
        cx.state.locations.contains_key(&location_id),
        "Investigate: location {location_id:?} (investigator's current_location) \
         is not in the locations map; this is a state-corruption invariant violation"
    );
    if candidates(cx.state, investigator).is_none() {
        return EngineOutcome::Rejected {
            reason: format!("Investigate: location {location_id:?} is not revealed").into(),
        };
    }

    take::take(cx, investigator, &description, |_| {
        Ok(ActionResume::Investigate)
    })
}

/// Perform an **investigate**: an Intellect test against `investigator`'s
/// location whose difficulty *is* that location's modified shroud, read live at
/// ST.6 rather than snapshotted here (#677), with the base Investigate follow-up
/// (so a success discovers a clue).
///
/// The one perform behind both ways of investigating (#805) — the basic action
/// (after its attack-of-opportunity loop, with no modification) and an ability
/// printing the bold **Investigate** designator (Flashlight 01087).
/// `glossary/Ability.md` is what makes them the same procedure: *"Activating
/// such an ability **performs the designated action** as described in the
/// rules, but modified in the manner described by the ability."* The
/// modification is `shroud_modifier`, and it is the *only* thing that differs.
///
/// `shroud_modifier` adjusts the **location difficulty** (shroud), not the
/// investigator's total, and it is one contribution among however many the
/// location carries: Flashlight's `-2` composes with Obscuring Fog 01168's `+2`
/// into a single shroud, clamped at 0 once at the end (RR p.4: game values can
/// never be reduced below 0). It travels into the test unevaluated so the row
/// it becomes is recalculated at every read (ADR 0005).
///
/// The location is re-read here through [`candidates`] — the §D precondition
/// re-check after the basic action's attacks of opportunity. If the
/// investigator is locationless or their location is gone or no longer
/// revealed, the precondition has lapsed and this suppresses (returns `Done`).
/// The designated path checks [`candidates`] itself first and rejects instead,
/// so it never reaches the suppression.
///
/// A missing investigator map entry panics — `resume_action_resolution`'s
/// `Status::Active` gate upstream already guarantees the investigator is
/// present, so absence here is a state-corruption invariant violation.
pub(crate) fn perform(
    cx: &mut Cx,
    investigator: InvestigatorId,
    shroud_modifier: Option<IntExpr>,
    source: Option<AbilitySource>,
) -> EngineOutcome {
    assert!(
        cx.state.investigators.contains_key(&investigator),
        "investigate::perform: investigator {investigator:?} not in map after the \
         Status::Active re-validation gate; this is a state-corruption invariant violation"
    );
    let Some(location_id) = candidates(cx.state, investigator) else {
        return EngineOutcome::Done;
    };
    skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Intellect,
        SkillTestKind::Investigate,
        DifficultyBasis::Shroud(location_id),
        SkillTestFollowUp::Investigate,
        None,
        None,
        source,
        // "Your location gets -2 shroud for this investigation" — a row over
        // the *location*, scoped to the test about to start.
        shroud_modifier.map(|delta| InitiatorModifier {
            target: ModifierTarget::Location(location_id),
            stat: Stat::Shroud,
            delta,
        }),
    )
}

/// An investigation's after-test step (RR ST.7), run on success only: push
/// **one** discovery of `1 + bonus_clues_discovered` clues at the tested
/// location, for the global drive loop (Slice D #423).
///
/// The discovery may suspend on a before-timing interrupt (Cover Up 01007); the
/// loop drives it to completion either way, then re-dispatches the skill test
/// at its `ApplyResultEffect` step. The "after you successfully investigate"
/// timing point already fired at the preceding `DetermineOutcome` step, on the
/// ST.6 success, before this discovery. The discovery has no source card, so
/// `for_controller` is correct.
///
/// **One** discovery, whose count carries any commit-time bonus (Deduction
/// 01039's `bonus_clues_discovered`, the clue-side twin of Fight's
/// `bonus_attack_damage`). "Discover 1 additional clue" raises this discovery's
/// count; it does not make a second one — see the **Discovery** entry in
/// `GLOSSARY.md` and #471. The in-flight test is still present here (torn down
/// only at the end of resolution), so the accumulator is readable.
///
/// `TestedLocation` — the test's start-of-test location snapshot — is the
/// **default** target, not an invariant: several cards replace or redirect
/// this discovery. Burglary 01045 (Core): "If you succeed, instead of
/// discovering clues, gain 3 resources." Seeking Answers 02023 (Dunwich)
/// discovers at a *connecting* location instead. It differs from
/// `YourLocation` only if the investigator moves mid-test — no in-corpus path
/// does today, but the snapshot is what "at that location" means for every
/// card that reads it.
///
/// A mid-test move is not a reason to abandon the test, which is what makes
/// the snapshot the right thing to keep rather than a case to reject.
/// `data/official-faq/Frequently_Asked_Questions.md`: *"Once you initiate a
/// skill test or ability, you'll resolve that test or ability as completely as
/// possible, regardless of your location (unless another effect cancels or
/// interrupts it)."*
pub(in crate::engine::dispatch) fn after_test(cx: &mut Cx, investigator: InvestigatorId) {
    let bonus = cx
        .state
        .current_skill_test()
        .map_or(0, |t| t.bonus_clues_discovered);
    let effect = dsl::discover_clue(LocationTarget::TestedLocation, 1u8.saturating_add(bonus));
    evaluator::push_effect(cx, &effect, EvalContext::for_controller(investigator));
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::Effect;

    use super::*;
    use crate::state::{
        Continuation, EffectFrame, GameStateBuilder, InFlightSkillTest, SkillTestId,
    };
    use crate::test_support;

    /// The `Investigate` follow-up pushes **one** `DiscoverClue` of
    /// `1 + bonus_clues_discovered` at the test's `tested_location`, reading the
    /// commit-time accumulator off the in-flight record (Deduction 01039). With
    /// `bonus_clues_discovered: 1` that is a single discovery of 2 — not two of
    /// 1, which is what Cover Up 01007 would replace twice (#471).
    #[test]
    fn investigate_follow_up_pushes_one_discovery_carrying_the_clue_bonus() {
        let inv = InvestigatorId(1);
        let loc = LocationId(10);
        let mut state = GameStateBuilder::new()
            .with_investigator_at(test_support::test_investigator(1), loc)
            .with_location(test_support::test_location(10, "Study"))
            .build();
        state
            .continuations
            .push(Continuation::SkillTest(InFlightSkillTest {
                tested_location: Some(loc),
                follow_up: SkillTestFollowUp::Investigate,
                bonus_clues_discovered: 1,
                ..test_support::test_skill_test(
                    SkillTestId(0),
                    inv,
                    SkillKind::Intellect,
                    SkillTestKind::Investigate,
                    2,
                )
            }));
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        after_test(&mut cx, inv);

        let Some(Continuation::Effect(EffectFrame::Leaf { effect, .. })) =
            state.continuations.top()
        else {
            panic!(
                "expected one pushed DiscoverClue leaf, got {:?}",
                state.continuations.top()
            );
        };
        assert_eq!(
            **effect,
            Effect::DiscoverClue {
                from: LocationTarget::TestedLocation,
                count: 2,
            },
            "one discovery of 1 base + 1 bonus, at the tested location",
        );
    }
}
