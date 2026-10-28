//! The [`Seat`] fixture: a seated investigator built in one expression.
//!
//! ```
//! use game_core::state::{GameStateBuilder, InvestigatorId};
//! use game_core::test_support::{seat, SeatLookup as _, TEST_ASSET, TEST_REGISTRIES};
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
//! through [`SeatLookup::in_play`].

use std::collections::BTreeMap;

use card_dsl::card_data::{CardKind, CardType};

use crate::card_registry::CardRegistry;
use crate::engine::{OptionTarget, Registries};
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, GameState, GameStateBuilder, Investigator,
    InvestigatorId, LocationId, Owner, SkillKind, Skills, Status, UseKind,
};
use crate::test_support::test_investigator;

/// A seated investigator under construction; see [`seat`].
#[derive(Debug, Clone)]
#[must_use = "a Seat does nothing until it is passed to GameStateBuilder::with_seat"]
pub struct Seat {
    investigator: Investigator,
    /// The printed investigator [`as_card`](Seat::as_card) named, with the
    /// skill overrides set after it in order. Resolved by
    /// [`with_seat`](GameStateBuilder::with_seat), the first point that
    /// knows the board's registries.
    printed: Option<Printed>,
    /// Cards for the play area and threat area, in the order the setters
    /// named them. Instance ids are minted by [`with_seat`](GameStateBuilder::with_seat).
    cards: Vec<Placed>,
}

#[derive(Debug, Clone)]
struct Printed {
    code: CardCode,
    /// [`skills`](Seat::skills) / [`skill`](Seat::skill) calls made after
    /// `as_card`, replayed over the printed skills so the later call wins.
    overrides: Vec<SkillOverride>,
}

#[derive(Debug, Clone, Copy)]
enum SkillOverride {
    All(Skills),
    One(SkillKind, i8),
}

impl SkillOverride {
    fn apply(self, skills: &mut Skills) {
        match self {
            Self::All(all) => *skills = all,
            Self::One(kind, value) => {
                let slot = match kind {
                    SkillKind::Willpower => &mut skills.willpower,
                    SkillKind::Intellect => &mut skills.intellect,
                    SkillKind::Combat => &mut skills.combat,
                    SkillKind::Agility => &mut skills.agility,
                };
                *slot = value;
            }
        }
    }
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
pub fn seat(id: u32) -> Seat {
    Seat {
        investigator: test_investigator(id),
        printed: None,
        cards: Vec::new(),
    }
}

impl Seat {
    /// Seat the printed investigator `code`: its investigator card, and the
    /// name and skills printed on it, read from the card registry's
    /// `metadata_for` — the source real seating reads. A later
    /// [`skills`](Self::skills) or [`skill`](Self::skill) wins.
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
    pub fn as_card(mut self, code: impl Into<CardCode>) -> Self {
        self.printed = Some(Printed {
            code: code.into(),
            overrides: Vec::new(),
        });
        self
    }

    /// Set the investigator's skills, overriding any printed ones.
    pub fn skills(self, skills: Skills) -> Self {
        self.override_skills(SkillOverride::All(skills))
    }

    /// Set one skill to `value`, keeping the other three: `seat(1).skill(SkillKind::Combat, 5)`
    /// is the stock 3/3/3/3 with combat 5. Like [`skills`](Self::skills), a
    /// later call wins over [`as_card`](Self::as_card)'s printed value.
    pub fn skill(self, kind: SkillKind, value: i8) -> Self {
        self.override_skills(SkillOverride::One(kind, value))
    }

    /// Apply `skill` now, or — after [`as_card`](Self::as_card), whose printed
    /// skills are not known until `with_seat` — record it to replay over them.
    fn override_skills(mut self, skill: SkillOverride) -> Self {
        match &mut self.printed {
            Some(printed) => printed.overrides.push(skill),
            None => skill.apply(&mut self.investigator.skills),
        }
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

    /// Set the investigator's status — [`Status::Defeated`] or
    /// [`Status::Resigned`] seats one already eliminated.
    pub fn status(mut self, status: Status) -> Self {
        self.investigator.status = status;
        self
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
    pub fn with_seat(self, seat: Seat) -> Self {
        let Seat {
            mut investigator,
            printed,
            cards,
        } = seat;
        let registry = self.registries().map(|registries| registries.cards);
        if let Some(Printed { code, overrides }) = printed {
            print(named(registry), &mut investigator, code);
            for skill in overrides {
                skill.apply(&mut investigator.skills);
            }
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
    pub fn with_two_seats_led_by_the_second(self, first: Seat, second: Seat) -> Self {
        let order = [first.investigator.id, second.investigator.id];
        self.with_seat(first)
            .with_seat(second)
            .with_turn_order(order)
            .with_lead(order[1])
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

    /// The [`CardInstance`](OptionTarget::CardInstance) anchor of that same
    /// card — what [`pick`](super::TestSession::pick) and
    /// [`offered_at`](super::TestSession::offered_at) take for a reaction,
    /// forced or zero-action ability on a seated card.
    ///
    /// # Panics
    ///
    /// As [`in_play`](Self::in_play).
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
        if let [id] = matches[..] {
            return id;
        }
        let there: Vec<String> = cards()
            .map(|card| format!("{} {:?}", card.code, card.instance_id))
            .collect();
        panic!(
            "in_play: {investigator:?} has {} cards {code} in play or in their threat area, \
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
}
