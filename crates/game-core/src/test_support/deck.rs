//! The act and agenda deck stagers: [`with_act_deck`] and
//! [`with_agenda_deck`] on the builder, each taking a deck in the order it is
//! written, top card first.
//!
//! A card in a deck is **printed** or **stock**. A printed card is its code,
//! and its threshold is the card's, read at [`build`] through the constructor
//! a scenario's `setup()` uses ([`Act::from_metadata`],
//! [`Agenda::from_metadata`]). A stock card ([`act`], [`agenda`]) is a
//! synthetic primitive with the threshold the test states, for a test that
//! can't reach the corpus — ADR 0016's *"It may not impersonate a printed
//! card"*, so it refuses a printed code.
//!
//! ```
//! use game_core::state::GameStateBuilder;
//! use game_core::test_support::{act, agenda};
//!
//! let state = GameStateBuilder::new()
//!     .with_act_deck([act("_act_1", 2), act("_act_2", 3)])
//!     .with_agenda_deck([agenda("_agenda", 4)])
//!     .with_agenda_doom(1)
//!     .build();
//! assert_eq!(state.act_deck[1].clue_threshold, 3);
//! assert_eq!(state.agenda_doom, 1);
//! ```
//!
//! [`with_act_deck`]: GameStateBuilder::with_act_deck
//! [`with_agenda_deck`]: GameStateBuilder::with_agenda_deck
//! [`build`]: GameStateBuilder::build

use crate::state::staged::Staged;
use crate::state::{Act, Agenda, CardCode, GameStateBuilder};
use crate::test_support::stage::synthetic_code;

/// An act in a staged act deck: printed (from its code) or stock
/// ([`act`]). See [`with_act_deck`](GameStateBuilder::with_act_deck).
#[derive(Debug, Clone)]
#[must_use = "a staged act does nothing until it is passed to with_act_deck"]
pub struct StagedAct(Staged<Act>);

/// An agenda in a staged agenda deck: printed (from its code) or stock
/// ([`agenda`]). See [`with_agenda_deck`](GameStateBuilder::with_agenda_deck).
#[derive(Debug, Clone)]
#[must_use = "a staged agenda does nothing until it is passed to with_agenda_deck"]
pub struct StagedAgenda(Staged<Agenda>);

/// A stock act: the synthetic `code` (for a registry's ability lookup), with
/// the clue threshold `clue_threshold`.
///
/// # Panics
///
/// If `code` has the shape of a printed `ArkhamDB` code: stage that card by
/// its code, which reads its printed threshold.
pub fn act(code: impl Into<CardCode>, clue_threshold: u8) -> StagedAct {
    let code = code.into();
    let code = synthetic_code(
        &format!("act({code}, {clue_threshold})"),
        code.clone(),
        &format!(
            "Stage the card by its code, `with_act_deck([\"{code}\"])`, which reads its \
             printed threshold"
        ),
    );
    StagedAct(Staged::stock(Act {
        code,
        clue_threshold,
    }))
}

/// A stock agenda: the synthetic `code` (for a registry's ability lookup),
/// with the doom threshold `doom_threshold`.
///
/// # Panics
///
/// If `code` has the shape of a printed `ArkhamDB` code: stage that card by
/// its code, which reads its printed threshold.
pub fn agenda(code: impl Into<CardCode>, doom_threshold: u8) -> StagedAgenda {
    let code = code.into();
    let code = synthetic_code(
        &format!("agenda({code}, {doom_threshold})"),
        code.clone(),
        &format!(
            "Stage the card by its code, `with_agenda_deck([\"{code}\"])`, which reads its \
             printed threshold"
        ),
    );
    StagedAgenda(Staged::stock(Agenda {
        code,
        doom_threshold,
    }))
}

/// The printed act `code`, its threshold read at
/// [`build`](GameStateBuilder::build).
impl From<CardCode> for StagedAct {
    fn from(code: CardCode) -> Self {
        Self(Staged {
            piece: Act {
                code: code.clone(),
                clue_threshold: 0,
            },
            printed: Some(code),
        })
    }
}

/// The printed act `code`, its threshold read at
/// [`build`](GameStateBuilder::build).
impl From<&str> for StagedAct {
    fn from(code: &str) -> Self {
        CardCode::from(code).into()
    }
}

/// The printed agenda `code`, its threshold read at
/// [`build`](GameStateBuilder::build).
impl From<CardCode> for StagedAgenda {
    fn from(code: CardCode) -> Self {
        Self(Staged {
            piece: Agenda {
                code: code.clone(),
                doom_threshold: 0,
            },
            printed: Some(code),
        })
    }
}

/// The printed agenda `code`, its threshold read at
/// [`build`](GameStateBuilder::build).
impl From<&str> for StagedAgenda {
    fn from(code: &str) -> Self {
        CardCode::from(code).into()
    }
}

impl GameStateBuilder {
    /// Set the act deck, [`GameState::act_deck`](crate::state::GameState::act_deck),
    /// in the order written: the first card is act 1. Replaces any set before.
    ///
    /// Each card is a printed act's code — its clue threshold read from the
    /// registries at [`build`](Self::build), a missing one reading as 0 — or
    /// a stock [`act`] with the threshold the test states. A deck mixing the
    /// two names its printed cards with `StagedAct::from(code)`.
    ///
    /// Test-only, like [`with_seat`](Self::with_seat): a scenario's
    /// `setup()` builds its deck with [`Act::from_metadata`].
    ///
    /// # Panics
    ///
    /// [`build`](Self::build) panics, naming the code, if a printed card
    /// can't be read: no registries were named, the registry doesn't know
    /// it, or it is not an act.
    pub fn with_act_deck(mut self, deck: impl IntoIterator<Item = impl Into<StagedAct>>) -> Self {
        self.stage_act_deck(deck.into_iter().map(|act| act.into().0).collect());
        self
    }

    /// Set the agenda deck,
    /// [`GameState::agenda_deck`](crate::state::GameState::agenda_deck), in
    /// the order written: the first card is agenda 1. Replaces any set
    /// before.
    ///
    /// Each card is a printed agenda's code — its doom threshold read from
    /// the registries at [`build`](Self::build) — or a stock [`agenda`] with
    /// the threshold the test states. A deck mixing the two names its printed
    /// cards with `StagedAgenda::from(code)`.
    ///
    /// Test-only, like [`with_seat`](Self::with_seat): a scenario's
    /// `setup()` builds its deck with [`Agenda::from_metadata`].
    ///
    /// # Panics
    ///
    /// [`build`](Self::build) panics, naming the code, if a printed card
    /// can't be read: no registries were named, the registry doesn't know
    /// it, or it is not an agenda.
    pub fn with_agenda_deck(
        mut self,
        deck: impl IntoIterator<Item = impl Into<StagedAgenda>>,
    ) -> Self {
        self.stage_agenda_deck(deck.into_iter().map(|agenda| agenda.into().0).collect());
        self
    }
}
