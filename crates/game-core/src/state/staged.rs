//! A board piece a test staged, whose printed fields may still have to be read
//! from its card.
//!
//! The test-only stagers (`test_support::enemy`, `test_support::location`)
//! record a printed code with [`as_card`](crate::test_support::StagedEnemy::as_card)
//! and the board state around it. The printed fields can't be read then: an
//! enemy's per-investigator health scales by the investigators on the board,
//! which are not all seated yet. [`GameStateBuilder::build`] (or `place`, on a
//! built state) resolves each [`Staged`] piece once that count is known. The
//! act and agenda deck stagers (`with_act_deck`, `with_agenda_deck`) record
//! each printed card's code the same way, and `build` reads its threshold.
//!
//! Plain data, so the production builder holds no fixture type.
//!
//! [`GameStateBuilder::build`]: crate::state::GameStateBuilder::build

use card_dsl::card_data::CardMetadata;

use crate::card_registry::CardRegistry;
use crate::state::{Act, Agenda, CardCode, Enemy, Location};

/// A board piece, and the printed card whose printed fields replace its own
/// when it is [resolved](Self::resolve).
#[derive(Debug, Clone)]
pub(crate) struct Staged<T> {
    /// The piece as staged: stock printed fields (overwritten if `printed`
    /// names a card) and the board state the test set.
    pub(crate) piece: T,
    /// The card `as_card` named, read at resolution. `None` for a stock piece.
    pub(crate) printed: Option<CardCode>,
}

impl<T> Staged<T> {
    /// A piece with nothing left to read: it is resolved as it stands.
    pub(crate) fn stock(piece: T) -> Self {
        Self {
            piece,
            printed: None,
        }
    }
}

/// A board piece whose printed fields can be read from its card while its
/// board state is kept.
pub(crate) trait Reprint: Sized {
    /// The card type `metadata` must print, for the panic naming a mismatch.
    const KIND: &'static str;

    /// The call that named the printed card, which a resolve panic names:
    /// `as_card` on a board stager, the deck setter for a deck card.
    const STAGER: &'static str;

    /// `self` with the printed fields `metadata` prints, scaled to
    /// `investigator_count` where the card prints a per-investigator value,
    /// and its board state unchanged. `None` when `metadata` is not a
    /// [`KIND`](Self::KIND) card.
    fn reprint(self, metadata: &CardMetadata, investigator_count: u8) -> Option<Self>;
}

impl<T: Reprint> Staged<T> {
    /// The piece, with its printed card's fields if it names one, read from
    /// `cards` and scaled to `investigator_count`.
    ///
    /// # Panics
    ///
    /// Naming the code, if the piece names a printed card and `cards` is
    /// `None` (the test named no registries), the registry does not know the
    /// code, or the code is not a [`KIND`](Reprint::KIND) card.
    pub(crate) fn resolve(self, cards: Option<&CardRegistry>, investigator_count: u8) -> T {
        let Some(code) = self.printed else {
            return self.piece;
        };
        let metadata = printed_metadata(cards, &code, T::STAGER);
        self.piece
            .reprint(metadata, investigator_count)
            .unwrap_or_else(|| panic!("{}({code}): {code} is not {} card", T::STAGER, T::KIND))
    }
}

/// The metadata `cards` prints for `code`, the card `stager` (the call that
/// named it, as in [`Reprint::STAGER`]) named.
///
/// # Panics
///
/// Naming `stager` and `code`, if `cards` is `None` (the test named no
/// registries) or the registry does not know `code`.
pub(crate) fn printed_metadata(
    cards: Option<&CardRegistry>,
    code: &CardCode,
    stager: &str,
) -> &'static CardMetadata {
    let cards = cards.unwrap_or_else(|| {
        panic!(
            "{stager}({code}): name the registries to read {code} from, with \
             `.with_registries(..)` on the builder"
        )
    });
    (cards.metadata_for)(code)
        .unwrap_or_else(|| panic!("{stager}({code}): the registry has no card {code}"))
}

impl Reprint for Enemy {
    const KIND: &'static str = "an enemy";
    const STAGER: &'static str = "as_card";

    /// The printed enemy, through [`Enemy::from_metadata`] — the constructor a
    /// spawn uses — keeping the staged id, location, owner, damage,
    /// exhaustion, engagement and attachments.
    fn reprint(self, metadata: &CardMetadata, investigator_count: u8) -> Option<Self> {
        let mut printed = Self::from_metadata(
            self.id,
            metadata,
            self.current_location,
            self.owner,
            investigator_count,
        )?;
        printed.damage = self.damage;
        printed.exhausted = self.exhausted;
        printed.engaged_with = self.engaged_with;
        printed.attachments = self.attachments;
        Some(printed)
    }
}

impl Reprint for Location {
    const KIND: &'static str = "a location";
    const STAGER: &'static str = "as_card";

    /// The printed location, through [`Location::from_metadata`] — the
    /// constructor `add_location` uses — keeping the staged id, clues,
    /// revealed state, connections, attachments and cards at it. A location
    /// prints no per-investigator field that resolves on entry, so
    /// `investigator_count` is unused: its printed clue value is kept as
    /// printed and placed only at reveal.
    fn reprint(self, metadata: &CardMetadata, _investigator_count: u8) -> Option<Self> {
        let mut printed = Self::from_metadata(self.id, metadata)?;
        printed.clues = self.clues;
        printed.revealed = self.revealed;
        printed.connections = self.connections;
        printed.attachments = self.attachments;
        printed.cards_at_location = self.cards_at_location;
        Some(printed)
    }
}

impl Reprint for Act {
    const KIND: &'static str = "an act";
    const STAGER: &'static str = "with_act_deck";

    /// The printed act, through [`Act::from_metadata`] — the constructor a
    /// scenario's `setup()` uses. An act in a deck carries no board state.
    fn reprint(self, metadata: &CardMetadata, _investigator_count: u8) -> Option<Self> {
        Self::from_metadata(metadata)
    }
}

impl Reprint for Agenda {
    const KIND: &'static str = "an agenda";
    const STAGER: &'static str = "with_agenda_deck";

    /// The printed agenda, through [`Agenda::from_metadata`] — the
    /// constructor a scenario's `setup()` uses. An agenda in a deck carries
    /// no board state: the doom on it is the board's
    /// ([`agenda_doom`](crate::state::GameState::agenda_doom)).
    fn reprint(self, metadata: &CardMetadata, _investigator_count: u8) -> Option<Self> {
        Self::from_metadata(metadata)
    }
}
