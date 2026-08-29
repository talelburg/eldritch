//! Which side of a card is in effect right now (#774).
//!
//! A double-sided card prints text on both faces, and only one of them is in
//! effect at a time. For a location the switch is [`Location::revealed`]: **a
//! location's `back_text` abilities apply while it is unrevealed, and its
//! front's while it is revealed.** The Parlor 01115 is the corpus's first
//! card to need the distinction — its unrevealed back reads *"The entrance to
//! the Parlor is blocked by a darkly glowing unfathomable barrier. You cannot
//! move into the Parlor."*, and act 01109b lifts it by revealing (*"The
//! barrier blocking passage into the parlor has vanished. Reveal the
//! Parlor."*).
//!
//! # Why a mechanism rather than a hardcode
//!
//! **20 locations in Core + Dunwich carry `back_text`, and 19 of the 20 are
//! movement restrictions.** Seven of those are unconditional and need nothing
//! beyond this module (Parlor 01115, Dormitories 02052, Faculty Offices
//! 02054/02055, Alchemy Labs 02057, Museum Halls 02127, Ascending Path
//! 02283); the other twelve carry a condition a constant `Restrict` cannot
//! yet hold, which is `#821`.
//!
//! # Why not "unrevealed ⇒ unenterable"
//!
//! That is not a rule. The Attic 01113 and the Cellar 01114 enter play
//! unrevealed and are revealed **by** being entered
//! (`dispatch::actions::resume_move_enter`). Only a location that prints a
//! barrier on its back has one.
//!
//! # Why every reader funnels through here
//!
//! An ability is addressed by `(code, ability_index)` — candidates minted by
//! a scan are re-resolved by index when they fire. Front and back are
//! different vectors, so a scan that picks a side and a resolve that picks
//! the other would fire the wrong ability. One choice, made in one place, is
//! what keeps the index meaningful.
//!
//! [`Location::revealed`]: crate::state::Location::revealed

use crate::card_registry;
use crate::dsl::Ability;
use crate::state::{AbilitySource, CandidateSource, CardCode, GameState, LocationId};

/// The abilities in effect on the location `id`: its back's while it is
/// unrevealed, its front's while it is revealed.
///
/// `None` when no registry is installed, when `id` is not in play, or when
/// the side in effect implements nothing — the same *"this card implements
/// nothing"* signal `abilities_for` gives, kept so the callers that reject on
/// it keep their reason. [`location_abilities_or_empty`] is the form for
/// callers that only want to scan.
#[must_use]
pub(crate) fn location_abilities(state: &GameState, id: LocationId) -> Option<Vec<Ability>> {
    let reg = card_registry::current()?;
    let location = state.locations.get(&id)?;
    let lookup = if location.revealed {
        reg.abilities_for
    } else {
        reg.back_abilities_for
    };
    lookup(&location.code)
}

/// [`location_abilities`] with the three "nothing to scan" cases folded into
/// the empty vector, for callers that iterate rather than reject.
#[must_use]
pub(crate) fn location_abilities_or_empty(state: &GameState, id: LocationId) -> Vec<Ability> {
    location_abilities(state, id).unwrap_or_default()
}

/// The abilities in effect on the card behind `source`.
///
/// Delegates to [`location_abilities`] for
/// [`AbilitySource::Location`] and to the registry's front-side lookup for
/// every other source — an act, an agenda, an enemy and an in-play instance
/// each have exactly one face in play, so their side never varies.
///
/// Returns `None` for an unimplemented card, preserving `abilities_for`'s
/// distinction between *"this card implements nothing"* and *"this card's
/// abilities are empty"* for the callers that reject on it. An unrevealed
/// location whose back implements nothing is `None` for the same reason a
/// card with no implementation is — there is nothing on the side in effect.
#[must_use]
pub(crate) fn for_source(
    state: &GameState,
    source: AbilitySource,
    code: &CardCode,
) -> Option<Vec<Ability>> {
    if let AbilitySource::Location(id) = source {
        return location_abilities(state, id);
    }
    let reg = card_registry::current()?;
    (reg.abilities_for)(code)
}

/// [`for_source`] for a [`CandidateSource`]. A [`Hand`](CandidateSource::Hand)
/// candidate is a card being *played*, never a board card with two faces, so
/// it reads the front.
#[must_use]
pub(crate) fn for_candidate_source(
    state: &GameState,
    source: CandidateSource,
    code: &CardCode,
) -> Option<Vec<Ability>> {
    match source {
        CandidateSource::Ability(source) => for_source(state, source, code),
        CandidateSource::Hand => (card_registry::current()?.abilities_for)(code),
    }
}
