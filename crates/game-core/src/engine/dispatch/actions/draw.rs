//! The Draw basic action: draw 1 card.

use crate::engine::dispatch::{cards, combat};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::state::{ActionResolutionFrame, ActionResume, InvestigatorId};

use super::{spend_one_action, validate_basic_action};

/// Handler for `TurnAction::Draw`.
///
/// Validate-first: Investigation phase, investigator is active and
/// `Status::Active`, has at least 1 action remaining. Then spend the
/// action and resolve the draw per the Rules Reference:
///
/// - **Non-empty deck**: draw 1 to hand.
/// - **Empty deck, non-empty discard**: shuffle discard into deck,
///   draw 1, then take 1 horror — the horror penalty fires when an
///   investigator with an empty deck needs to draw.
/// - **Both empty**: no shuffle (per the Rules Reference's "any
///   ability that would shuffle a discard pile of zero cards back
///   into a deck does not shuffle the deck"), no card drawn — but
///   the 1 horror still applies. The rules don't explicitly address
///   this corner case; we apply the horror as the safer reading
///   ("would-draw-from-empty triggers the penalty"), and the case
///   is rare enough in practice (only high-cycle decks burn through
///   both zones) that the difference is mostly theoretical.
///
/// The draw logic itself is delegated to [`draw_primary_effect`] after
/// the attack-of-opportunity loop runs as an
/// [`ActionResolution`](crate::state::Continuation::ActionResolution) frame (#293).
pub(in crate::engine::dispatch) fn draw(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if let Err(rejection) = validate_basic_action(cx.state, "Draw", investigator) {
        return rejection;
    }

    // Mutate-second: spend the action, then park the draw over its
    // attack-of-opportunity loop (#293). Push the resume frame, then
    // drive the AoO. Draw is NOT on the AoO-exempt list (only Fight,
    // Evade, Parley, Resign are), so each ready engaged enemy attacks
    // before the card is drawn (RR p.5).
    spend_one_action(cx, investigator);
    cx.state.continuations.push(ActionResolutionFrame {
        investigator,
        resume: ActionResume::Draw,
    });
    combat::drive_aoo(cx, investigator)
}

/// The draw half of a Draw action, run after its `AoO` loop (#293).
///
/// Draw has no target precondition (unlike Move or Investigate), so
/// there is no secondary precondition re-check here. The `resume_action_resolution`
/// `Status::Active` gate upstream already guarantees the investigator is
/// present and Active; a missing map entry here is therefore a
/// state-corruption invariant violation — it must panic (via
/// `draw_one_with_deckout`'s `expect`), never silently return `Done`.
pub(in crate::engine::dispatch) fn draw_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    cards::draw_one_with_deckout(cx, investigator);
    EngineOutcome::Done
}
