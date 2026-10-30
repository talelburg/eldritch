//! The board stagers: [`enemy`] and [`location`], beside
//! [`seat`](super::seat), each staging one piece of the board in one
//! expression.
//!
//! ```
//! use game_core::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId};
//! use game_core::test_support::{enemy, location, seat, BoardLookup as _, TEST_REGISTRIES};
//!
//! const STUDY: LocationId = LocationId(10);
//! let state = GameStateBuilder::new()
//!     .with_registries(TEST_REGISTRIES)
//!     .with_location(location(STUDY).clues(2))
//!     .with_seat(seat(1).at(STUDY))
//!     .with_enemy(enemy(3).hunter().engaged_with(InvestigatorId(1)).damage(1))
//!     .build();
//! assert_eq!(state.location(STUDY).clues, 2);
//! assert_eq!(state.enemy(EnemyId(3)).current_location, Some(STUDY));
//! ```
//!
//! A stager starts **stock**: a synthetic primitive with neutral printed
//! fields, which a test may set to isolate one mechanism (an enemy whose only
//! interesting property is Hunter). [`as_card`](StagedEnemy::as_card) makes it
//! **printed**: the card's printed fields, read from the registries at
//! [`build`](GameStateBuilder::build), and no setter for them — ADR 0016's
//! *"It may not impersonate a printed card"*, held by the type:
//!
//! ```compile_fail,E0599
//! use game_core::test_support::enemy;
//! let _ = enemy(1).as_card("_probe").fight(5);
//! ```
//!
//! ```compile_fail,E0599
//! use game_core::test_support::location;
//! let _ = location(1).as_card("_probe").shroud(5);
//! ```
//!
//! Board state — damage, location, engagement, clues, connections,
//! attachments — stays settable on both.

use std::marker::PhantomData;

use card_dsl::card_data::{ClueValue, Prey};

use crate::engine::Registries;
use crate::state::staged::Staged;
use crate::state::{
    CardCode, CardInPlay, Counter, Enemy, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    Location, LocationId, Owner,
};
use crate::test_support::fixtures::{test_enemy, test_location};

/// A stager's state before [`as_card`](StagedEnemy::as_card): its printed
/// fields are the stock fixture's, settable.
#[derive(Debug, Clone, Copy)]
pub struct Stock;

/// A stager's state after [`as_card`](StagedEnemy::as_card): its printed
/// fields are the card's, with no setter.
#[derive(Debug, Clone, Copy)]
pub struct Printed;

/// A stager that is [`Stock`] or [`Printed`], with only the board-state
/// setters both share.
///
/// It is what the builder takes, and the common type for a collection that
/// mixes the two: `vec![seat(1).into(), seat(2).as_card(..).into()]`.
#[derive(Debug, Clone, Copy)]
pub struct AnyKind;

/// Whether `code` has the shape of a printed `ArkhamDB` code — five digits
/// and an optional lowercase letter (`01160`, `01145b`).
fn is_printed_code(code: &str) -> bool {
    let digits = code
        .strip_suffix(|c: char| c.is_ascii_lowercase())
        .unwrap_or(code);
    digits.len() == 5 && digits.bytes().all(|b| b.is_ascii_digit())
}

/// The synthetic code a stock stager names, for a registry's ability lookup.
///
/// # Panics
///
/// If `code` has the shape of a printed `ArkhamDB` code: a stock piece may
/// model an engine primitive but not impersonate a printed card (ADR 0016), so
/// a printed card is staged the way that reads its printed fields. `call`
/// names the call for the message (`"enemy(3).code(01160)"`), and `remedy`
/// says how to stage the printed card instead.
pub(crate) fn synthetic_code(call: &str, code: CardCode, remedy: &str) -> CardCode {
    assert!(
        !is_printed_code(code.as_str()),
        "{call}: {code} is a printed ArkhamDB code, and a stock piece can't impersonate a \
         printed card (ADR 0016). {remedy}, or use a synthetic code"
    );
    code
}

/// An enemy under construction; see [`enemy`]. `K` is [`Stock`], [`Printed`]
/// or [`AnyKind`].
#[derive(Debug, Clone)]
#[must_use = "a staged enemy does nothing until it is passed to with_enemy, place or mint"]
pub struct StagedEnemy<K = Stock> {
    staged: Staged<Enemy>,
    /// The investigator `engaged_with` named, whose location the enemy takes
    /// once the board is known.
    engaged: Option<InvestigatorId>,
    kind: PhantomData<K>,
}

/// Start staging the enemy `id`, a stock enemy.
///
/// With no setters it is the stock enemy `test_enemy` builds, in
/// `test_support/fixtures.rs`, whose doc lists its values.
pub fn enemy(id: impl Into<EnemyId>) -> StagedEnemy<Stock> {
    let EnemyId(id) = id.into();
    StagedEnemy {
        staged: Staged::stock(test_enemy(id, format!("Test Enemy {id}"))),
        engaged: None,
        kind: PhantomData,
    }
}

impl StagedEnemy<Stock> {
    /// Stage the printed enemy `code`: every printed field — name, fight,
    /// evade, health, attack, keywords, Prey, victory — is the card's, read at
    /// [`build`](GameStateBuilder::build) (or [`mint`](Self::mint),
    /// [`place`](GameState::place)) through the constructor a spawn uses. A
    /// per-investigator health scales to every investigator on the board.
    ///
    /// A printed field set before this call is replaced, and none can be set
    /// after it. A test that needs other numbers stages a probe card.
    ///
    /// # Panics
    ///
    /// Where it is read, naming `code`: if the test named no registries, the
    /// registry does not know `code`, or `code` is not an enemy card.
    pub fn as_card(self, code: impl Into<CardCode>) -> StagedEnemy<Printed> {
        StagedEnemy {
            staged: Staged {
                printed: Some(code.into()),
                ..self.staged
            },
            engaged: self.engaged,
            kind: PhantomData,
        }
    }

    fn printed(mut self, set: impl FnOnce(&mut Enemy)) -> Self {
        set(&mut self.staged.piece);
        self
    }

    /// Set a synthetic code, for a registry's ability lookup.
    ///
    /// # Panics
    ///
    /// If `code` has the shape of a printed `ArkhamDB` code; stage that card
    /// with [`as_card`](Self::as_card).
    pub fn code(self, code: impl Into<CardCode>) -> Self {
        let code = code.into();
        let code = synthetic_code(
            &format!("enemy({}).code({code})", self.staged.piece.id.0),
            code.clone(),
            &format!("Stage the card with `.as_card(\"{code}\")`, which reads its printed fields"),
        );
        self.printed(|enemy| enemy.code = code)
    }

    /// Set the display name.
    pub fn name(self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.printed(|enemy| enemy.name = name)
    }

    /// Set the fight value.
    pub fn fight(self, fight: i8) -> Self {
        self.printed(|enemy| enemy.fight = fight)
    }

    /// Set the evade value.
    pub fn evade(self, evade: i8) -> Self {
        self.printed(|enemy| enemy.evade = evade)
    }

    /// Set the maximum health.
    pub fn health(self, health: u8) -> Self {
        self.printed(|enemy| enemy.max_health = health)
    }

    /// Set the damage an attack deals.
    pub fn attack_damage(self, damage: u8) -> Self {
        self.printed(|enemy| enemy.attack_damage = damage)
    }

    /// Set the horror an attack deals.
    pub fn attack_horror(self, horror: u8) -> Self {
        self.printed(|enemy| enemy.attack_horror = horror)
    }

    /// Give the enemy the Hunter keyword.
    pub fn hunter(self) -> Self {
        self.printed(|enemy| enemy.hunter = true)
    }

    /// Give the enemy the Retaliate keyword.
    pub fn retaliate(self) -> Self {
        self.printed(|enemy| enemy.retaliate = true)
    }

    /// Set the Prey instruction.
    pub fn prey(self, prey: Prey) -> Self {
        self.printed(|enemy| enemy.prey = prey)
    }

    /// Set the printed victory value.
    pub fn victory(self, victory: u8) -> Self {
        self.printed(|enemy| enemy.victory = Some(victory))
    }
}

impl<K> StagedEnemy<K> {
    fn board(mut self, set: impl FnOnce(&mut Enemy)) -> Self {
        set(&mut self.staged.piece);
        self
    }

    /// Place the enemy at `location`.
    pub fn at(self, location: LocationId) -> Self {
        self.board(|enemy| enemy.current_location = Some(location))
    }

    /// Engage the enemy with `investigator`. Staged on a board, it is placed
    /// at that investigator's location, whichever order the two were staged
    /// in; [`mint`](Self::mint), which knows no board, leaves its location as
    /// [`at`](Self::at) set it.
    pub fn engaged_with(mut self, investigator: InvestigatorId) -> Self {
        self.engaged = Some(investigator);
        self.board(|enemy| enemy.engaged_with = Some(investigator))
    }

    /// Put `damage` damage on the enemy.
    pub fn damage(self, damage: u8) -> Self {
        self.board(|enemy| enemy.damage = damage)
    }

    /// Exhaust the enemy.
    pub fn exhausted(self) -> Self {
        self.board(|enemy| enemy.exhausted = true)
    }

    /// Attach `cards` to the enemy, in order.
    pub fn attachments(self, cards: impl IntoIterator<Item = CardInPlay>) -> Self {
        self.board(|enemy| enemy.attachments = cards.into_iter().collect())
    }

    /// Set who owns the enemy — the bearer, for a weakness enemy. Stock and
    /// printed alike start owned by the encounter deck.
    pub fn owner(self, owner: Owner) -> Self {
        self.board(|enemy| enemy.owner = owner)
    }

    /// The bare [`Enemy`], resolved against `registries` and
    /// `investigator_count` — for a test of a function that takes `&Enemy`,
    /// or of a component that renders one, with no board around it.
    ///
    /// # Panics
    ///
    /// As [`as_card`](StagedEnemy::as_card) describes, if it named a card.
    #[must_use]
    pub fn mint(self, registries: Registries, investigator_count: u8) -> Enemy {
        self.staged
            .resolve(Some(registries.cards), investigator_count)
    }

    fn any_kind(self) -> StagedEnemy<AnyKind> {
        StagedEnemy {
            staged: self.staged,
            engaged: self.engaged,
            kind: PhantomData,
        }
    }
}

impl From<StagedEnemy<Stock>> for StagedEnemy<AnyKind> {
    fn from(enemy: StagedEnemy<Stock>) -> Self {
        enemy.any_kind()
    }
}

impl From<StagedEnemy<Printed>> for StagedEnemy<AnyKind> {
    fn from(enemy: StagedEnemy<Printed>) -> Self {
        enemy.any_kind()
    }
}

/// A location under construction; see [`location`]. `K` is [`Stock`],
/// [`Printed`] or [`AnyKind`].
#[derive(Debug, Clone)]
#[must_use = "a staged location does nothing until it is passed to with_location, place or mint"]
pub struct StagedLocation<K = Stock> {
    staged: Staged<Location>,
    kind: PhantomData<K>,
}

/// Start staging the location `id`, a stock location.
///
/// With no setters it is the stock location `test_location` builds, in
/// `test_support/fixtures.rs`, whose doc lists its values.
///
/// A staged location is revealed with no clues whether stock or printed:
/// staging never places printed clues as revealing would. Say otherwise with
/// [`unrevealed`](StagedLocation::unrevealed) and
/// [`clues`](StagedLocation::clues).
pub fn location(id: impl Into<LocationId>) -> StagedLocation<Stock> {
    let LocationId(id) = id.into();
    StagedLocation {
        staged: Staged::stock(test_location(id, format!("Test Location {id}"))),
        kind: PhantomData,
    }
}

impl StagedLocation<Stock> {
    /// Stage the printed location `code`: its name, shroud and printed clue
    /// value are the card's, read at [`build`](GameStateBuilder::build) (or
    /// [`mint`](Self::mint), [`place`](GameState::place)) through the
    /// constructor `add_location` uses. Its board state — revealed with no
    /// clues unless set — is unchanged.
    ///
    /// A printed field set before this call is replaced, and none can be set
    /// after it.
    ///
    /// # Panics
    ///
    /// Where it is read, naming `code`: if the test named no registries, the
    /// registry does not know `code`, or `code` is not a location card.
    pub fn as_card(self, code: impl Into<CardCode>) -> StagedLocation<Printed> {
        StagedLocation {
            staged: Staged {
                printed: Some(code.into()),
                ..self.staged
            },
            kind: PhantomData,
        }
    }

    fn printed(mut self, set: impl FnOnce(&mut Location)) -> Self {
        set(&mut self.staged.piece);
        self
    }

    /// Set a synthetic code, for a registry's ability lookup.
    ///
    /// # Panics
    ///
    /// If `code` has the shape of a printed `ArkhamDB` code; stage that card
    /// with [`as_card`](Self::as_card).
    pub fn code(self, code: impl Into<CardCode>) -> Self {
        let code = code.into();
        let code = synthetic_code(
            &format!("location({}).code({code})", self.staged.piece.id.0),
            code.clone(),
            &format!("Stage the card with `.as_card(\"{code}\")`, which reads its printed fields"),
        );
        self.printed(|location| location.code = code)
    }

    /// Set the display name.
    pub fn name(self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.printed(|location| location.name = name)
    }

    /// Set the shroud.
    pub fn shroud(self, shroud: u8) -> Self {
        self.printed(|location| location.shroud = shroud)
    }

    /// Set the printed clue value, which revealing the location places.
    pub fn printed_clues(self, clues: ClueValue) -> Self {
        self.printed(|location| location.printed_clues = clues)
    }
}

impl<K> StagedLocation<K> {
    fn board(mut self, set: impl FnOnce(&mut Location)) -> Self {
        set(&mut self.staged.piece);
        self
    }

    /// Put `n` clues on the location.
    pub fn clues(self, n: u8) -> Self {
        self.board(|location| location.clues = n)
    }

    /// Stage the location face down.
    pub fn unrevealed(self) -> Self {
        self.board(|location| location.revealed = false)
    }

    /// Connect the location to `connections`, one way. A two-way connection
    /// names each location from the other.
    pub fn connections(self, connections: impl IntoIterator<Item = LocationId>) -> Self {
        self.board(|location| location.connections = connections.into_iter().collect())
    }

    /// Attach `cards` to the location, in order.
    pub fn attachments(self, cards: impl IntoIterator<Item = CardInPlay>) -> Self {
        self.board(|location| location.attachments = cards.into_iter().collect())
    }

    /// Put `cards` into play at the location, controlled by no one.
    pub fn cards_at_location(self, cards: impl IntoIterator<Item = CardInPlay>) -> Self {
        self.board(|location| location.cards_at_location = cards.into_iter().collect())
    }

    /// The bare [`Location`], resolved against `registries` — for a test of a
    /// function that takes `&Location`, or of a component that renders one,
    /// with no board around it.
    ///
    /// # Panics
    ///
    /// As [`as_card`](StagedLocation::as_card) describes, if it named a card.
    #[must_use]
    pub fn mint(self, registries: Registries) -> Location {
        // A location prints nothing that scales on entry.
        self.staged.resolve(Some(registries.cards), 1)
    }

    fn any_kind(self) -> StagedLocation<AnyKind> {
        StagedLocation {
            staged: self.staged,
            kind: PhantomData,
        }
    }
}

impl From<StagedLocation<Stock>> for StagedLocation<AnyKind> {
    fn from(location: StagedLocation<Stock>) -> Self {
        location.any_kind()
    }
}

impl From<StagedLocation<Printed>> for StagedLocation<AnyKind> {
    fn from(location: StagedLocation<Printed>) -> Self {
        location.any_kind()
    }
}

impl GameStateBuilder {
    /// Add the location `location` stages, replacing any with the same id.
    /// A printed location ([`as_card`](StagedLocation::as_card)) reads its
    /// card at [`build`](Self::build).
    ///
    /// Test-only, like [`with_seat`](Self::with_seat): a scenario puts its
    /// locations into play with
    /// [`add_location`](GameState::add_location).
    pub fn with_location(mut self, location: impl Into<StagedLocation<AnyKind>>) -> Self {
        self.stage_location(location.into().staged);
        self
    }

    /// Add the enemy `enemy` stages, replacing any with the same id. A
    /// printed enemy ([`as_card`](StagedEnemy::as_card)) reads its card at
    /// [`build`](Self::build), scaled to every investigator on the board. An
    /// enemy [`engaged_with`](StagedEnemy::engaged_with) an investigator is
    /// placed at that investigator's location at `build`.
    ///
    /// # Panics
    ///
    /// [`build`](Self::build) panics if the enemy is engaged with an
    /// investigator that was never added.
    pub fn with_enemy(mut self, enemy: impl Into<StagedEnemy<AnyKind>>) -> Self {
        let StagedEnemy {
            staged, engaged, ..
        } = enemy.into();
        let id = staged.piece.id;
        self.stage_enemy(staged);
        if let Some(investigator) = engaged {
            self.stage_engagement(id, investigator);
        }
        self
    }
}

/// A staged piece [`GameState::place`] can put on a built board:
/// [`StagedEnemy`] or [`StagedLocation`].
pub trait Place: sealed::Sealed {
    #[doc(hidden)]
    fn place_on(self, state: &mut GameState, registries: Registries);
}

mod sealed {
    pub trait Sealed {}
    impl<K> Sealed for super::StagedEnemy<K> {}
    impl<K> Sealed for super::StagedLocation<K> {}
}

/// Advance `counter` past `id`, if it isn't already.
fn advance_past<T>(counter: &mut Counter<T>, id: u32) {
    if counter.peek() <= id {
        *counter = Counter::at(id.saturating_add(1));
    }
}

impl<K> Place for StagedEnemy<K> {
    fn place_on(self, state: &mut GameState, registries: Registries) {
        let count = u8::try_from(state.investigators.len()).unwrap_or(u8::MAX);
        let mut enemy = self.staged.resolve(Some(registries.cards), count);
        if let Some(investigator) = self.engaged {
            enemy.current_location = state
                .investigators
                .get(&investigator)
                .unwrap_or_else(|| {
                    panic!("place: enemy {:?} is engaged with {investigator:?}, who is not on the board", enemy.id)
                })
                .current_location;
        }
        let id = enemy.id;
        assert!(
            !state.enemies.contains_key(&id),
            "place: {id:?} is already on the board"
        );
        advance_past(&mut state.enemy_ids, id.0);
        state.enemies.insert(id, enemy);
    }
}

impl<K> Place for StagedLocation<K> {
    fn place_on(self, state: &mut GameState, registries: Registries) {
        let location = self.staged.resolve(Some(registries.cards), 1);
        let id = location.id;
        assert!(
            !state.locations.contains_key(&id),
            "place: {id:?} is already on the board"
        );
        advance_past(&mut state.location_ids, id.0);
        state.locations.insert(id, location);
    }
}

impl GameState {
    /// Put the enemy or location `piece` stages onto this already-built
    /// board — a test that adds a piece mid-scenario, or to a board a
    /// scenario's `setup()` built. A printed piece reads its card from
    /// `registries` (a state carries none), scaled to the investigators on
    /// this board; an engaged enemy takes its investigator's location. The
    /// matching id counter advances past the piece's id, so nothing the
    /// engine mints later overwrites it.
    ///
    /// Test-only, like the stagers.
    ///
    /// # Panics
    ///
    /// If the board already has a piece with that id, if an engaged enemy's
    /// investigator is not on it, or as `as_card` describes.
    pub fn place(&mut self, piece: impl Place, registries: Registries) {
        piece.place_on(self, registries);
    }
}
