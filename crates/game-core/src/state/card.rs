//! Card identifiers used throughout state.

use serde::{Deserialize, Serialize};

/// `ArkhamDB` card code (e.g. `"01030"` for Magnifying Glass).
///
/// Newtype over `String` so we can't accidentally pass arbitrary strings
/// where a card code is expected. Serializes transparently as a string
/// so the wire format matches the upstream JSON.
///
/// The cards crate's lookups (`cards::by_code`, `cards::abilities_for`)
/// take `&str`; deref or call `.as_str()` to bridge.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardCode(pub String);

impl CardCode {
    /// Wrap a string into a [`CardCode`].
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the inner string. Use this to bridge to `&str`-taking APIs
    /// like `cards::by_code(code.as_str())`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CardCode {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for CardCode {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for CardCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// A card-bearing zone, used as the `from` field on movement events
/// (e.g. [`Event::CardDiscarded`](crate::Event::CardDiscarded)).
///
/// Phase-3 minimal set. Discard is a destination but never a `from`
/// in the current event set; encounter / weakness / out-of-game zones
/// land when they're needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Zone {
    /// A player's hand.
    Hand,
    /// A player's deck (top or bottom — events that need the
    /// distinction can record it separately).
    Deck,
    /// Cards currently in play under an investigator's control.
    InPlay,
}
