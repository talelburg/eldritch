//! The [`Seat`] fixture: a seated investigator built in one expression.
//!
//! ```
//! use game_core::state::{GameStateBuilder, InvestigatorId};
//! use game_core::test_support::{seat, BoardLookup as _, TEST_ASSET, TEST_REGISTRIES};
//!
//! let state = GameStateBuilder::new()
//!     .with_registries(TEST_REGISTRIES)
//!     .with_seat(seat(1).in_play([TEST_ASSET]))
//!     .build();
//! let asset = state.in_play(InvestigatorId(1), TEST_ASSET);
//! assert_eq!(
//!     state.investigators[&InvestigatorId(1)].cards_in_play[0].instance_id,
//!     asset,
//! );
//! ```
//!
//! Cards a seat puts in play or in a threat area get instance ids minted by
//! the builder, never hand-picked, and a test finds one again by its code
//! through [`BoardLookup::in_play`](super::BoardLookup::in_play).
//!
//! A seat is [`Stock`] until [`as_card`](Seat::as_card) makes it
//! [`Printed`]; only a stock seat's skills can be set:
//!
//! ```compile_fail,E0599
//! use card_dsl::card_data::SkillKind;
//! use game_core::test_support::seat;
//! let _ = seat(1).as_card("_probe").skill(SkillKind::Combat, 5);
//! ```

use std::collections::BTreeMap;
use std::marker::PhantomData;

use card_dsl::card_data::{CardKind, CardType};

use crate::card_registry::CardRegistry;
use crate::engine::Registries;
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, GameStateBuilder, Investigator, InvestigatorId,
    LocationId, Owner, SkillKind, Skills, Status, UseKind,
};
use crate::test_support::{test_investigator, Either, Printed, Stock};

/// A seated investigator under construction; see [`seat`]. `K` is
/// [`Stock`], [`Printed`] (after [`as_card`](Seat::as_card)) or [`Either`].
#[derive(Debug, Clone)]
#[must_use = "a Seat does nothing until it is passed to GameStateBuilder::with_seat"]
pub struct Seat<K = Stock> {
    investigator: Investigator,
    /// The printed investigator [`as_card`](Seat::as_card) named. Resolved
    /// by [`with_seat`](GameStateBuilder::with_seat), the first point that
    /// knows the board's registries.
    printed: Option<CardCode>,
    /// Cards for the play area and threat area, in the order the setters
    /// named them. Instance ids are minted by [`with_seat`](GameStateBuilder::with_seat).
    cards: Vec<Placed>,
    kind: PhantomData<K>,
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

/// The state a [`SeatCard`] enters with: [`uses`], [`clues`], [`doom`] or [`horror`]. A
/// plain code enters with none — no printed uses are seeded.
#[derive(Debug, Clone, Default)]
pub struct CardSetup {
    uses: BTreeMap<UseKind, u8>,
    clues: u8,
    doom: u8,
    horror: u8,
}

/// A card entering with `n` uses of `kind`: `(FIRST_AID, uses(UseKind::Supplies, 3))`.
#[must_use]
pub fn uses(kind: UseKind, n: u8) -> CardSetup {
    CardSetup {
        uses: BTreeMap::from([(kind, n)]),
        ..CardSetup::default()
    }
}

/// A card entering with `n` clues on it: `(COVER_UP, clues(3))`.
#[must_use]
pub fn clues(n: u8) -> CardSetup {
    CardSetup {
        clues: n,
        ..CardSetup::default()
    }
}

/// A card entering with `n` horror on it: `(NECRONOMICON, horror(3))`.
#[must_use]
pub fn horror(n: u8) -> CardSetup {
    CardSetup {
        horror: n,
        ..CardSetup::default()
    }
}

/// A card entering with `n` doom on it: `(ARCANE_INITIATE, doom(1))`.
#[must_use]
pub fn doom(n: u8) -> CardSetup {
    CardSetup {
        doom: n,
        ..CardSetup::default()
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
pub fn seat(id: u32) -> Seat<Stock> {
    Seat {
        investigator: test_investigator(id),
        printed: None,
        cards: Vec::new(),
        kind: PhantomData,
    }
}

impl Seat<Stock> {
    /// Seat the printed investigator `code`: its investigator card, and the
    /// name and skills printed on it, read from the card registry's
    /// `metadata_for` — the source real seating reads. Skills set before this
    /// call are replaced, and none can be set after it: a test that needs
    /// other skills seats a probe investigator.
    ///
    /// The lookup happens in [`with_seat`](GameStateBuilder::with_seat),
    /// against the registries named by
    /// [`with_registries`](GameStateBuilder::with_registries).
    ///
    /// # Panics
    ///
    /// `with_seat` panics if the test never named its registries, and, naming
    /// `code`, if the registry does not know `code` or `code` is not an
    /// investigator card.
    pub fn as_card(self, code: impl Into<CardCode>) -> Seat<Printed> {
        Seat {
            printed: Some(code.into()),
            ..self.kind()
        }
    }

    /// Set the investigator's skills.
    pub fn skills(mut self, skills: Skills) -> Self {
        self.investigator.skills = skills;
        self
    }

    /// Set one skill to `value`, keeping the other three: `seat(1).skill(SkillKind::Combat, 5)`
    /// is the stock 3/3/3/3 with combat 5.
    pub fn skill(mut self, kind: SkillKind, value: i8) -> Self {
        let skills = &mut self.investigator.skills;
        let slot = match kind {
            SkillKind::Willpower => &mut skills.willpower,
            SkillKind::Intellect => &mut skills.intellect,
            SkillKind::Combat => &mut skills.combat,
            SkillKind::Agility => &mut skills.agility,
        };
        *slot = value;
        self
    }
}

impl<K> Seat<K> {
    /// This seat as another kind, unchanged.
    fn kind<L>(self) -> Seat<L> {
        Seat {
            investigator: self.investigator,
            printed: self.printed,
            cards: self.cards,
            kind: PhantomData,
        }
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
    /// [`BoardLookup::in_play`](super::BoardLookup::in_play).
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

    /// Set the investigator's status — [`Status::Defeated`] or
    /// [`Status::Resigned`] seats one already eliminated.
    pub fn status(mut self, status: Status) -> Self {
        self.investigator.status = status;
        self
    }
}

impl From<Seat<Stock>> for Seat<Either> {
    fn from(seat: Seat<Stock>) -> Self {
        seat.kind()
    }
}

impl From<Seat<Printed>> for Seat<Either> {
    fn from(seat: Seat<Printed>) -> Self {
        seat.kind()
    }
}

/// The owner of a card a seat puts in `zone`: the investigator for the play
/// area, and for the threat area whoever the printed card says, read from the
/// named `cards`.
fn owner(
    cards: Option<&CardRegistry>,
    zone: SeatZone,
    code: &CardCode,
    investigator: InvestigatorId,
) -> Owner {
    let player_card = || {
        (named(cards).metadata_for)(code).is_some_and(|meta| {
            meta.is_weakness()
                || matches!(
                    meta.card_type(),
                    CardType::Investigator | CardType::Asset | CardType::Event | CardType::Skill
                )
        })
    };
    if zone == SeatZone::InPlay || player_card() {
        Owner::Investigator(investigator)
    } else {
        Owner::EncounterDeck
    }
}

/// The card registry a seat's lookup reads: the one the test named.
///
/// # Panics
///
/// Panics if the test never named its registries. A seat that looks nothing
/// up (no printed investigator, no threat-area card) never asks.
fn named(cards: Option<&CardRegistry>) -> &CardRegistry {
    cards.unwrap_or_else(|| {
        panic!(
            "with_seat: name the registries first, with `.with_registries(..)` on the \
             builder before the first `.with_seat(..)`"
        )
    })
}

/// Seat the printed investigator `code` on `investigator`, from `cards`.
fn print(cards: &CardRegistry, investigator: &mut Investigator, code: CardCode) {
    let meta = (cards.metadata_for)(&code)
        .unwrap_or_else(|| panic!("seat.as_card: the registry has no card {code}"));
    let CardKind::Investigator { skills, .. } = meta.kind else {
        panic!("seat.as_card: {code} is not an investigator card");
    };
    investigator.name.clone_from(&meta.name);
    investigator.skills = skills;
    investigator.investigator_card.code = code;
}

impl GameStateBuilder {
    /// Name the registries this board's test fixtures resolve against:
    /// [`with_seat`](Self::with_seat) reads a seat's printed investigator
    /// ([`Seat::as_card`]) and its threat-area owners from them, and
    /// [`session`](Self::session) drives the engine with them. Name them once,
    /// before the first `with_seat`.
    ///
    /// Test-only, like `with_seat`: a production builder never names
    /// registries, since the host hands them to `apply` directly. There is no
    /// default: a fixture that needs them before they are named panics.
    pub fn with_registries(mut self, registries: Registries) -> Self {
        self.set_registries(registries);
        self
    }

    /// Add the investigator `seat` describes, minting instance ids for the
    /// cards it places. Minting continues past every id already staged on the
    /// builder and every [`in_play_as`](Seat::in_play_as) id, and
    /// [`build`](GameStateBuilder::build) seeds the state's counter past them
    /// all. A card staged *after* this seat, with a hand-picked id a seat
    /// already minted, is caught at `build`, which panics on any two cards
    /// sharing an id.
    ///
    /// The seat's printed investigator ([`as_card`](Seat::as_card)) and its
    /// threat-area cards' owners are read from the registries named by
    /// [`with_registries`](Self::with_registries).
    ///
    /// Lives here (test-only), like [`session`](GameStateBuilder::session),
    /// because [`Seat`] is a test fixture: the production builder holds only
    /// the optional registries slot this reads, never a fixture type.
    ///
    /// # Panics
    ///
    /// Panics if an [`in_play_as`](Seat::in_play_as) id is already in use, if
    /// the seat needs a lookup (a printed investigator or a threat-area card)
    /// before the test named its registries, or as [`as_card`](Seat::as_card)
    /// describes.
    pub fn with_seat(self, seat: impl Into<Seat<Either>>) -> Self {
        let Seat {
            mut investigator,
            printed,
            cards,
            ..
        } = seat.into();
        let registry = self.registries().map(|registries| registries.cards);
        if let Some(code) = printed {
            print(named(registry), &mut investigator, code);
        }
        let chosen: Vec<CardInstanceId> = cards.iter().filter_map(|placed| placed.id).collect();
        let mut taken: Vec<CardInstanceId> = self.card_instance_ids_in_use().collect();
        for &id in &chosen {
            assert!(
                !taken.contains(&id),
                "with_seat: in_play_as({id:?}) is already in use on this board"
            );
            taken.push(id);
        }
        let mut next = chosen
            .iter()
            .map(|id| id.0.saturating_add(1))
            .fold(self.next_card_instance_id(), u32::max);
        for Placed { zone, id, card } in cards {
            let instance_id = id.unwrap_or_else(|| {
                let minted = CardInstanceId(next);
                next = next.saturating_add(1);
                minted
            });
            let owner = owner(registry, zone, &card.code, investigator.id);
            let mut in_play = CardInPlay::enter_play(card.code, instance_id, owner);
            in_play.uses = card.setup.uses;
            in_play.clues = card.setup.clues;
            in_play.doom = card.setup.doom;
            in_play.accumulated_horror = card.setup.horror;
            match zone {
                SeatZone::InPlay => investigator.cards_in_play.push(in_play),
                SeatZone::ThreatArea => investigator.threat_area.push(in_play),
            }
        }
        self.with_investigator(investigator)
    }

    /// Seat `first` then `second`, with `second` the lead, so player order
    /// runs `second`, `first`: the table a test needs to tell the lead from
    /// the first seat.
    pub fn with_two_seats_led_by_the_second(
        self,
        first: impl Into<Seat<Either>>,
        second: impl Into<Seat<Either>>,
    ) -> Self {
        let (first, second) = (first.into(), second.into());
        let order = [first.investigator.id, second.investigator.id];
        self.with_seat(first)
            .with_seat(second)
            .with_seating(order)
            .with_lead(order[1])
    }
}
