//! Wendy's Amulet (Neutral signature asset, 01014).
//!
//! ```text
//! Wendy's Amulet.
//! Item. Relic.
//! Wendy Adams deck only.
//! You may play the topmost event in your discard pile as if it were in your hand.
//! <b>Forced</b> - After you play an event or discard an event from play: Place it on the bottom of your deck instead of in your discard pile.
//! ```
//!
//! The snapshot text carries the erratum (FAQ v1.9, June 2021) that added
//! *"or discard an event from play"*; the ruling it overturned, that an event
//! leaving play at another time is discarded as normal, is struck through in
//! the card's FAQ file (<https://arkhamdb.com/card/01014>).
//!
//! **The Forced is a routing rule, not an ability with a window**: an
//! [`Effect::PlaceEventsOnBottomOfDeck`] constant the engine reads where it
//! places the owner's event. The Official FAQ puts a played event's placement
//! inside play step 4: *"An event card is placed on top of its owner’s discard
//! pile during step 4 of the play/initiation sequence, so any effect that
//! modifies where the event goes (such as Parallel Agnes Baker or Crystallizer
//! of Dreams) must occur during that step."* So the "after you play" names no
//! cell of the `CardPlayed` condition: the event is already on the bottom of the
//! deck when that condition's after cell opens, as it is for Heirloom of
//! Hyperborea 01012. A discard from play is redirected where the leave-play
//! router files the card by its owner.
//!
//! Rulings (<https://arkhamdb.com/card/01014>) the engine meets by
//! construction: *"The **Forced** effect triggers after you play an event from
//! either your hand or your discard pile. It does not trigger after you commit
//! an event card from hand to a skill test."* — a committed card is discarded
//! at ST.8 by its own path, which never reads the constant. *"If your deck is
//! empty, and Wendy’s Amulet would have you place an event “on the bottom of
//! your deck,” that event becomes your new deck."* — the event is pushed onto
//! the deck's bottom, which an empty deck makes the whole deck.
//!
//! **The first ability is a play permission**: an
//! [`Effect::PlayTopmostEventInDiscardPile`] constant the play paths read
//! where they list what may be played from where — the turn menu, a Fast
//! window, and the reaction scan for a Fast event such as "Look what I
//! found!" 01079. `glossary/As_If.md` makes it a normal play throughout: *"The
//! game state is considered to be altered throughout the duration of the
//! indicated ability or action, from its initiation (including the paying of
//! its costs, attacks of opportunity, etc) through the resolution of each
//! aspect of its effect, and up until its completion."* — so the event pays
//! its cost, keeps its timing, and provokes attacks of opportunity as a hand
//! play does, and nothing else sees it as in hand. The option is offered on
//! the Amulet, labelled *"Play \<event\> (Wendy's Amulet)"*: the client has no
//! discard-pile view (TODO(#1101): show each investigator's discard pile).
//! Played that way, the event is then placed by the Forced, on the bottom of
//! the deck.
//!
//! The ruling (<https://arkhamdb.com/card/01014>) *"'Play' and 'Commit' are
//! two different game terms. Wendy's Amulet allows you to 'play' the topmost
//! event card in your discard pile, but it does not allow you to 'commit' it
//! to a skill test."* is met by construction: a commit names hand positions,
//! and no commit path reads the permission.
//!
//! "Wendy Adams deck only." is a deckbuilding restriction the engine does not
//! police.

use card_dsl::dsl::{self, Ability, Effect};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01014";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Wendy's Amulet's two abilities, both constants the engine reads.
///
/// *"You may play the topmost event in your discard pile as if it were in
/// your hand."*, then the Forced: *"After you play an event or discard an
/// event from play: Place it on the bottom of your deck instead of in your
/// discard pile."*
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::constant(Effect::PlayTopmostEventInDiscardPile),
        dsl::constant(Effect::PlaceEventsOnBottomOfDeck),
    ]
}
