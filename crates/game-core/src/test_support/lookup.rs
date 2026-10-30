//! [`BoardLookup`]: find a staged piece of the board again by what it is.

use crate::engine::OptionTarget;
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, GameState, InvestigatorId, Location,
    LocationId,
};

/// Find a piece of the board by what it is: a seated card by its code, an
/// enemy or location by its id. Each panics with what *is* there. Import it as
/// `BoardLookup as _`.
pub trait BoardLookup {
    /// The one card with `code` in `investigator`'s play area or threat area
    /// (the threat area is a play area too) — to read its exhaustion, tokens
    /// or uses.
    ///
    /// # Panics
    ///
    /// Panics unless exactly one card matches, listing what is there. Seat two
    /// copies with [`Seat::in_play_as`](super::Seat::in_play_as) and address
    /// them by id.
    fn card_in_play(&self, investigator: InvestigatorId, code: impl Into<CardCode>) -> &CardInPlay;

    /// The instance id of that same card.
    ///
    /// # Panics
    ///
    /// As [`card_in_play`](Self::card_in_play).
    fn in_play(&self, investigator: InvestigatorId, code: impl Into<CardCode>) -> CardInstanceId {
        self.card_in_play(investigator, code).instance_id
    }

    /// The [`CardInstance`](OptionTarget::CardInstance) anchor of that same
    /// card — what [`pick`](super::TestSession::pick) and
    /// [`offered_at`](super::TestSession::offered_at) take for a reaction,
    /// forced or zero-action ability on a seated card.
    ///
    /// # Panics
    ///
    /// As [`card_in_play`](Self::card_in_play).
    fn card(&self, investigator: InvestigatorId, code: impl Into<CardCode>) -> OptionTarget {
        OptionTarget::CardInstance(self.in_play(investigator, code))
    }

    /// The name `investigator` is shown by, as their investigator card prints
    /// it.
    ///
    /// # Panics
    ///
    /// If `investigator` is not seated.
    fn name_of(&self, investigator: InvestigatorId) -> String;

    /// The enemy `id`.
    ///
    /// # Panics
    ///
    /// If no enemy `id` is on the board, listing the ids that are.
    fn enemy(&self, id: EnemyId) -> &Enemy;

    /// The location `id`.
    ///
    /// # Panics
    ///
    /// If no location `id` is on the board, listing the ids that are.
    fn location(&self, id: LocationId) -> &Location;
}

impl BoardLookup for GameState {
    fn card_in_play(&self, investigator: InvestigatorId, code: impl Into<CardCode>) -> &CardInPlay {
        let code = code.into();
        let inv = self
            .investigators
            .get(&investigator)
            .unwrap_or_else(|| panic!("card_in_play: no investigator {investigator:?}"));
        let cards = || inv.cards_in_play.iter().chain(&inv.threat_area);
        let matches: Vec<&CardInPlay> = cards().filter(|card| card.code == code).collect();
        if let [card] = matches[..] {
            return card;
        }
        let there: Vec<String> = cards()
            .map(|card| format!("{} {:?}", card.code, card.instance_id))
            .collect();
        panic!(
            "card_in_play: {investigator:?} has {} cards {code} in play or in their threat area, \
             not exactly one; there: [{}]",
            matches.len(),
            there.join(", "),
        );
    }

    fn name_of(&self, investigator: InvestigatorId) -> String {
        self.investigators
            .get(&investigator)
            .unwrap_or_else(|| panic!("name_of: no investigator {investigator:?}"))
            .name
            .clone()
    }

    fn enemy(&self, id: EnemyId) -> &Enemy {
        self.enemies.get(&id).unwrap_or_else(|| {
            panic!(
                "enemy: no {id:?} on the board; there: {:?}",
                self.enemies.keys().collect::<Vec<_>>()
            )
        })
    }

    fn location(&self, id: LocationId) -> &Location {
        self.locations.get(&id).unwrap_or_else(|| {
            panic!(
                "location: no {id:?} on the board; there: {:?}",
                self.locations.keys().collect::<Vec<_>>()
            )
        })
    }
}
