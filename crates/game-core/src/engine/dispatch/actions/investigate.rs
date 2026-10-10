//! The Investigate basic action, and the investigate primary shared with
//! the designated Investigate.

use card_dsl::dsl::{IntExpr, SkillTestKind, Stat};

use crate::engine::dispatch::skill_test::InitiatorModifier;
use crate::engine::dispatch::{combat, skill_test};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{designator, Cx};
use crate::state::{
    AbilitySource, ActionResolutionFrame, ActionResume, DifficultyBasis, InvestigatorId,
    LocationId, ModifierTarget, SkillKind, SkillTestFollowUp,
};

use super::{spend_one_action, validate_basic_action};

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
/// The `AoO` loop now runs as an [`ActionResolution`] frame (#293): the
/// frame is pushed, then [`combat::drive_aoo`] drives the loop. If a
/// cancel/soak window opens the loop suspends; `drive` resumes the
/// frame once the window closes, calling [`investigate_primary_effect`].
///
/// [`Effect::DiscoverClue`]: card_dsl::dsl::Effect::DiscoverClue
/// [`ActionResolution`]: crate::state::Continuation::ActionResolution
pub(in crate::engine::dispatch) fn investigate(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    // Validate-first (the shared basic-action prefix, then the
    // location-specific checks).
    let inv = match validate_basic_action(cx.state, "Investigate", investigator) {
        Ok(inv) => inv,
        Err(rejection) => return rejection,
    };
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
    let location = cx.state.locations.get(&location_id).unwrap_or_else(|| {
        unreachable!(
            "Investigate: location {location_id:?} (investigator's current_location) \
             is not in the locations map; this is a state-corruption invariant violation"
        )
    });
    if !location.revealed {
        return EngineOutcome::Rejected {
            reason: format!("Investigate: location {location_id:?} is not revealed").into(),
        };
    }

    // Mutate-second: spend the action, then park the investigate over
    // its attack-of-opportunity loop (#293). Push the resume frame,
    // then drive the AoO. Investigate is NOT on the AoO-exempt list
    // (only Fight, Evade, Parley, Resign are), so each ready engaged
    // enemy attacks before the skill test resolves.
    spend_one_action(cx, investigator);
    cx.state.continuations.push(ActionResolutionFrame {
        investigator,
        resume: ActionResume::Investigate,
    });
    combat::drive_aoo(cx, investigator)
}

/// The skill-test half of an Investigate, run after its `AoO` loop (#293).
/// Re-reads the location + effective shroud live and re-checks the location
/// is still revealed (the §D precondition re-check); suppresses (returns
/// `Done`) if the precondition has lapsed.
///
/// A missing investigator map entry panics — `resume_action_resolution`'s
/// `Status::Active` gate upstream already guarantees the investigator is
/// present, so absence here is a state-corruption invariant violation. A
/// legitimately lapsed precondition (no `current_location`, or location
/// absent / not `revealed`) returns `Done` instead.
pub(in crate::engine::dispatch) fn investigate_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    assert!(
        cx.state.investigators.contains_key(&investigator),
        "investigate_primary_effect: investigator {investigator:?} not in map after the \
         Status::Active re-validation gate; this is a state-corruption invariant violation"
    );
    // Locationless after the AoO, or the location gone / no longer revealed:
    // the precondition lapsed, so suppress the primary rather than rejecting
    // (the §D contract). Read through the same helper `can_perform` uses, so
    // the basic action and a designated **Investigate** agree on what a
    // location has to be to investigate it (#805).
    let Some(location_id) = designator::investigate_location(cx.state, investigator) else {
        return EngineOutcome::Done;
    };
    // A basic investigation carries no modification — the designated one
    // (Flashlight 01087) reaches the same primary with its `-2 [shroud]`.
    perform_investigate(cx, investigator, location_id, None, None)
}

/// Perform an **investigate** against `location_id`: an Intellect test whose
/// difficulty *is* that location's modified shroud, read live at ST.6 rather
/// than snapshotted here (#677), with the base Investigate follow-up (so a
/// success discovers a clue).
///
/// The one primary behind both ways of investigating (#805) — the basic action
/// (via [`investigate_primary_effect`], after its attack-of-opportunity loop)
/// and an ability printing the bold **Investigate** designator (Flashlight
/// 01087). `glossary/Ability.md` is what makes them the same procedure:
/// *"Activating such an ability **performs the designated action** as described
/// in the rules, but modified in the manner described by the ability."* The
/// modification is `shroud_modifier`, and it is the *only* thing that differs.
///
/// `shroud_modifier` adjusts the **location difficulty** (shroud), not the
/// investigator's total, and it is one contribution among however many the
/// location carries: Flashlight's `-2` composes with Obscuring Fog 01168's `+2`
/// into a single shroud, clamped at 0 once at the end (RR p.4: game values can
/// never be reduced below 0). It travels into the test unevaluated so the row
/// it becomes is recalculated at every read (ADR 0005).
///
/// Callers validate that the location exists and is revealed; this takes the id
/// as given.
pub(crate) fn perform_investigate(
    cx: &mut Cx,
    investigator: InvestigatorId,
    location_id: LocationId,
    shroud_modifier: Option<IntExpr>,
    source: Option<AbilitySource>,
) -> EngineOutcome {
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
