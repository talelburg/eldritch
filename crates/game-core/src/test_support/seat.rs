//! The [`Seat`] fixture: a seated investigator built in one expression.

use card_dsl::card_data::CardKind;

use crate::card_registry;
use crate::state::{CardCode, GameStateBuilder, Investigator, LocationId, Skills};
use crate::test_support::test_investigator;

/// A seated investigator under construction; see [`seat`].
#[derive(Debug, Clone)]
#[must_use = "a Seat does nothing until it is passed to GameStateBuilder::with_seat"]
pub struct Seat {
    investigator: Investigator,
}

/// Start a [`Seat`] for investigator `id`. With no setters it seats exactly
/// [`test_investigator(id)`](test_investigator).
pub fn seat(id: u32) -> Seat {
    Seat {
        investigator: test_investigator(id),
    }
}

impl Seat {
    /// Seat the printed investigator `code`: its investigator card, and the
    /// name and skills printed on it, read from the installed registry's
    /// `metadata_for` — the source real seating reads. A later
    /// [`skills`](Self::skills) wins.
    ///
    /// # Panics
    ///
    /// Panics, naming `code`, if no registry is installed, the registry does
    /// not know `code`, or `code` is not an investigator card.
    pub fn as_card(mut self, code: impl Into<CardCode>) -> Self {
        let code = code.into();
        let meta = card_registry::current()
            .and_then(|reg| (reg.metadata_for)(&code))
            .unwrap_or_else(|| panic!("seat.as_card: the registry has no card {code}"));
        let CardKind::Investigator { skills, .. } = meta.kind else {
            panic!("seat.as_card: {code} is not an investigator card");
        };
        self.investigator.name.clone_from(&meta.name);
        self.investigator.skills = skills;
        self.investigator.investigator_card.code = code;
        self
    }

    /// Set the investigator's skills, overriding any printed ones.
    pub fn skills(mut self, skills: Skills) -> Self {
        self.investigator.skills = skills;
        self
    }

    /// Place the investigator at `location`. The location itself is added to
    /// the builder separately.
    pub fn at(mut self, location: LocationId) -> Self {
        self.investigator.current_location = Some(location);
        self
    }

    /// Set the hand, in order.
    pub fn hand<C: Into<CardCode>>(mut self, codes: impl IntoIterator<Item = C>) -> Self {
        self.investigator.hand = codes.into_iter().map(Into::into).collect();
        self
    }

    /// Set the deck, top card first.
    pub fn deck<C: Into<CardCode>>(mut self, codes: impl IntoIterator<Item = C>) -> Self {
        self.investigator.deck = codes.into_iter().map(Into::into).collect();
        self
    }

    /// Set the discard pile, in order.
    pub fn discard<C: Into<CardCode>>(mut self, codes: impl IntoIterator<Item = C>) -> Self {
        self.investigator.discard = codes.into_iter().map(Into::into).collect();
        self
    }

    /// Put `damage` damage and `horror` horror on the investigator card, where
    /// an investigator's harm lives.
    pub fn harm(mut self, damage: u8, horror: u8) -> Self {
        self.investigator.investigator_card.accumulated_damage = damage;
        self.investigator.investigator_card.accumulated_horror = horror;
        self
    }

    /// Set the investigator's resource pool.
    pub fn resources(mut self, n: u8) -> Self {
        self.investigator.resources = n;
        self
    }

    /// Set the clues the investigator holds.
    pub fn clues(mut self, n: u8) -> Self {
        self.investigator.clues = n;
        self
    }

    /// Set the actions the investigator has remaining.
    pub fn actions(mut self, n: u8) -> Self {
        self.investigator.actions_remaining = n;
        self
    }
}

impl GameStateBuilder {
    /// Add the investigator `seat` describes. Lives here (test-only), like
    /// [`session`](GameStateBuilder::session), so the production builder
    /// carries no test dependency.
    pub fn with_seat(self, seat: Seat) -> Self {
        self.with_investigator(seat.investigator)
    }
}
