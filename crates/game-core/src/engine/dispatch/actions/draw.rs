//! The Draw basic action: draw 1 card.

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::cards;
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::state::{ActionResume, InvestigatorId};

/// Handler for `TurnAction::Draw`.
///
/// Draw has no target, so taking it ([`take::take`]) is the whole handler.
/// Draw is not on the attack-of-opportunity exempt list, so each ready engaged
/// enemy attacks before the card is drawn. The draw itself, in
/// [`draw_primary_effect`], resolves per the Rules Reference:
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
pub(in crate::engine::dispatch) fn draw(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    take::take(
        cx,
        investigator,
        &ActionDescription::basic(ActionKind::Draw),
        |_| Ok(ActionResume::Draw),
    )
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
