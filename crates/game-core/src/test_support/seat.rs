//! The [`Seat`] fixture: a seated investigator built in one expression.
//!
//! ```ignore
//! let state = GameStateBuilder::new()
//!     .with_location(test_support::test_location(10, "Study"))
//!     .with_seat(
//!         seat(1)
//!             .at(LocationId(10))
//!             .as_card("01001")
//!             .hand([GUTS])
//!             .in_play([(FIRST_AID, uses(UseKind::Supplies, 3))])
//!             .harm(2, 2),
//!     )
//!     .open_turn(InvestigatorId(1))
//!     .build();
//! let first_aid = state.in_play(InvestigatorId(1), FIRST_AID);
//! ```
//!
//! Cards a seat puts in play or in a threat area get instance ids minted by
//! the builder, never hand-picked, and a test finds one again by its code
//! through [`SeatLookup::in_play`].

use std::collections::BTreeMap;

use card_dsl::card_data::{CardKind, CardType};

use crate::card_registry;
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, GameState, GameStateBuilder, Investigator,
    InvestigatorId, LocationId, Owner, Skills, UseKind,
};
use crate::test_support::test_investigator;

/// A seated investigator under construction; see [`seat`].
#[derive(Debug, Clone)]
#[must_use = "a Seat does nothing until it is passed to GameStateBuilder::with_seat"]
pub struct Seat {
    investigator: Investigator,
    /// Cards for the play area and threat area, in the order the setters
    /// named them. Instance ids are minted by [`with_seat`](GameStateBuilder::with_seat).
    cards: Vec<Placed>,
}

#[derive(Debug, Clone)]
struct Placed {
    zone: SeatZone,
    id: Option<CardInstanceId>,
    card: SeatCard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeatZone {
    InPlay,
    ThreatArea,
}

/// One card a [`Seat`] places: a card code, with optionally the state it
/// enters with. Built from a plain code (`"01030"`), or a code paired with a
/// [`CardSetup`] (`("01030", uses(UseKind::Supplies, 3))`).
#[derive(Debug, Clone)]
pub struct SeatCard {
    code: CardCode,
    setup: CardSetup,
}

/// The state a [`SeatCard`] enters with: [`uses`] or [`clues`]. A plain code
/// enters with neither — no printed uses are seeded.
#[derive(Debug, Clone, Default)]
pub struct CardSetup {
    uses: BTreeMap<UseKind, u8>,
    clues: u8,
}

/// A card entering with `n` uses of `kind`: `(FIRST_AID, uses(UseKind::Supplies, 3))`.
#[must_use]
pub fn uses(kind: UseKind, n: u8) -> CardSetup {
    CardSetup {
        uses: BTreeMap::from([(kind, n)]),
        clues: 0,
    }
}

/// A card entering with `n` clues on it: `(COVER_UP, clues(3))`.
#[must_use]
pub fn clues(n: u8) -> CardSetup {
    CardSetup {
        uses: BTreeMap::new(),
        clues: n,
    }
}

macro_rules! seat_card_from {
    ($($code:ty),*) => {$(
        impl From<$code> for SeatCard {
            fn from(code: $code) -> Self {
                Self { code: code.into(), setup: CardSetup::default() }
            }
        }
        impl From<($code, CardSetup)> for SeatCard {
            fn from((code, setup): ($code, CardSetup)) -> Self {
                Self { code: code.into(), setup }
            }
        }
    )*};
}
seat_card_from!(&str, String, CardCode);

/// Start a [`Seat`] for investigator `id`. With no setters it seats exactly
/// [`test_investigator(id)`](test_investigator).
pub fn seat(id: u32) -> Seat {
    Seat {
        investigator: test_investigator(id),
        cards: Vec::new(),
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

    /// Put cards into the investigator's play area, owned by the investigator,
    /// each with a freshly minted instance id. Find one again with
    /// [`SeatLookup::in_play`].
    pub fn in_play<C: Into<SeatCard>>(mut self, cards: impl IntoIterator<Item = C>) -> Self {
        self.place(SeatZone::InPlay, cards);
        self
    }

    /// Put one card into the play area with the instance id `id` — for a test
    /// that seats two copies of a card and must address each. Minting skips
    /// past `id`.
    pub fn in_play_as(mut self, id: CardInstanceId, card: impl Into<SeatCard>) -> Self {
        self.cards.push(Placed {
            zone: SeatZone::InPlay,
            id: Some(id),
            card: card.into(),
        });
        self
    }

    /// Put cards into the investigator's threat area, each with a freshly
    /// minted instance id. A card printed as a player card or a weakness is
    /// owned by the investigator; anything else — an encounter card, or a
    /// code the registry does not know — by the encounter deck.
    pub fn threat_area<C: Into<SeatCard>>(mut self, cards: impl IntoIterator<Item = C>) -> Self {
        self.place(SeatZone::ThreatArea, cards);
        self
    }

    fn place<C: Into<SeatCard>>(&mut self, zone: SeatZone, cards: impl IntoIterator<Item = C>) {
        self.cards.extend(cards.into_iter().map(|card| Placed {
            zone,
            id: None,
            card: card.into(),
        }));
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

/// The owner of a card a seat puts in `zone`: the investigator for the play
/// area, and for the threat area whoever the printed card says.
fn owner(zone: SeatZone, code: &CardCode, investigator: InvestigatorId) -> Owner {
    let player_card = || {
        card_registry::current()
            .and_then(|reg| (reg.metadata_for)(code))
            .is_some_and(|meta| {
                meta.weakness
                    || matches!(
                        meta.card_type(),
                        CardType::Investigator
                            | CardType::Asset
                            | CardType::Event
                            | CardType::Skill
                    )
            })
    };
    if zone == SeatZone::InPlay || player_card() {
        Owner::Investigator(investigator)
    } else {
        Owner::EncounterDeck
    }
}

impl GameStateBuilder {
    /// Add the investigator `seat` describes, minting instance ids for the
    /// cards it places. Minting continues past every id already staged on the
    /// builder and every [`in_play_as`](Seat::in_play_as) id, so no two cards
    /// share one, and [`build`](GameStateBuilder::build) seeds the state's
    /// counter past them all.
    ///
    /// Lives here (test-only), like [`session`](GameStateBuilder::session), so
    /// the production builder carries no test dependency.
    ///
    /// # Panics
    ///
    /// Panics if an [`in_play_as`](Seat::in_play_as) id is already in use.
    pub fn with_seat(self, seat: Seat) -> Self {
        let Seat {
            mut investigator,
            cards,
        } = seat;
        let mut taken: Vec<CardInstanceId> = self.card_instance_ids_in_use().collect();
        for id in cards.iter().filter_map(|placed| placed.id) {
            assert!(
                !taken.contains(&id),
                "with_seat: in_play_as({id:?}) is already in use on this board"
            );
            taken.push(id);
        }
        let mut next = taken
            .iter()
            .map(|id| id.0.saturating_add(1))
            .max()
            .unwrap_or(0);
        for Placed { zone, id, card } in cards {
            let instance_id = id.unwrap_or_else(|| {
                let minted = CardInstanceId(next);
                next = next.saturating_add(1);
                minted
            });
            let owner = owner(zone, &card.code, investigator.id);
            let mut in_play = CardInPlay::enter_play(card.code, instance_id, owner);
            in_play.uses = card.setup.uses;
            in_play.clues = card.setup.clues;
            match zone {
                SeatZone::InPlay => investigator.cards_in_play.push(in_play),
                SeatZone::ThreatArea => investigator.threat_area.push(in_play),
            }
        }
        self.with_investigator(investigator)
    }
}

/// Find a seated card by what it is. Import it as `SeatLookup as _`.
pub trait SeatLookup {
    /// The instance id of the one card with `code` in `investigator`'s play
    /// area or threat area (the threat area is a play area too).
    ///
    /// # Panics
    ///
    /// Panics unless exactly one card matches, listing what is there. Seat two
    /// copies with [`Seat::in_play_as`] and address them by id.
    fn in_play(&self, investigator: InvestigatorId, code: impl Into<CardCode>) -> CardInstanceId;
}

impl SeatLookup for GameState {
    fn in_play(&self, investigator: InvestigatorId, code: impl Into<CardCode>) -> CardInstanceId {
        let code = code.into();
        let inv = self
            .investigators
            .get(&investigator)
            .unwrap_or_else(|| panic!("in_play: no investigator {investigator:?}"));
        let cards = || inv.cards_in_play.iter().chain(&inv.threat_area);
        let matches: Vec<CardInstanceId> = cards()
            .filter(|card| card.code == code)
            .map(|card| card.instance_id)
            .collect();
        match matches[..] {
            [id] => id,
            _ => {
                let there: Vec<String> = cards()
                    .map(|card| format!("{} {:?}", card.code, card.instance_id))
                    .collect();
                panic!(
                    "in_play: {investigator:?} has {} cards {code} in play or in their threat \
                     area, not exactly one; there: [{}]",
                    matches.len(),
                    there.join(", "),
                );
            }
        }
    }
}
