//! The **setup fingerprint**: what setup decided, recorded as card codes when
//! a game is created and compared with a re-run setup when it is loaded
//! (ADR 0022).
//!
//! Pure and I/O-free. It is built from card codes and token names only, never
//! from a serialized engine type, so a change to an engine type's shape can't
//! trip it. Only a change to what setup *decides* (another hand, another deck
//! order, another bag) does.

use std::fmt;

use game_core::state::card::CardCode;
use game_core::state::chaos_bag::ChaosToken;
use game_core::state::game_state::GameState;
use game_core::state::investigator::InvestigatorId;
use serde::{Deserialize, Serialize};

/// What setup decided: every investigator's hand and deck in order, the
/// encounter deck in order, and the chaos bag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupFingerprint {
    /// One entry per seated investigator, in id order.
    investigators: Vec<InvestigatorZones>,
    /// The encounter deck, top first.
    encounter_deck: Vec<String>,
    /// The chaos bag's tokens, as [`token_name`] names them.
    chaos_bag: Vec<String>,
}

/// One investigator's share of a [`SetupFingerprint`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InvestigatorZones {
    investigator: InvestigatorId,
    hand: Vec<String>,
    deck: Vec<String>,
}

/// A zone of the fingerprint, named so a mismatch says where setup diverged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Zone {
    /// Which investigators were seated.
    Investigators,
    /// An investigator's opening hand.
    Hand(InvestigatorId),
    /// An investigator's deck.
    Deck(InvestigatorId),
    /// The encounter deck.
    EncounterDeck,
    /// The chaos bag.
    ChaosBag,
}

impl fmt::Display for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Investigators => f.write_str("the seated investigators"),
            Self::Hand(id) => write!(f, "investigator {}'s hand", id.0),
            Self::Deck(id) => write!(f, "investigator {}'s deck", id.0),
            Self::EncounterDeck => f.write_str("the encounter deck"),
            Self::ChaosBag => f.write_str("the chaos bag"),
        }
    }
}

/// The first place a re-run setup differs from the stored fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FingerprintMismatch {
    /// The zone that differs.
    pub zone: Zone,
    /// The zero-based position of its first differing entry. Where one side
    /// is a prefix of the other, the length of the shorter.
    pub position: usize,
}

impl fmt::Display for FingerprintMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} differs at position {}", self.zone, self.position)
    }
}

impl SetupFingerprint {
    /// Record what setup decided in `state`.
    #[must_use]
    pub fn of(state: &GameState) -> Self {
        Self {
            investigators: state
                .investigators
                .values()
                .map(|inv| InvestigatorZones {
                    investigator: inv.id,
                    hand: codes(&inv.hand),
                    deck: codes(&inv.deck),
                })
                .collect(),
            encounter_deck: codes(&state.encounter_deck),
            chaos_bag: state
                .chaos_bag
                .tokens
                .iter()
                .copied()
                .map(token_name)
                .collect(),
        }
    }

    /// The first zone in which `current` differs from `self`, or `None` if
    /// setup decided the same things.
    #[must_use]
    pub fn first_difference(&self, current: &Self) -> Option<FingerprintMismatch> {
        if let Some(position) = first_differing(
            self.investigators.iter().map(|z| z.investigator),
            current.investigators.iter().map(|z| z.investigator),
        ) {
            return Some(FingerprintMismatch {
                zone: Zone::Investigators,
                position,
            });
        }
        for (stored, now) in self.investigators.iter().zip(&current.investigators) {
            let id = stored.investigator;
            for (zone, stored, now) in [
                (Zone::Hand(id), &stored.hand, &now.hand),
                (Zone::Deck(id), &stored.deck, &now.deck),
            ] {
                if let Some(position) = first_differing(stored.iter(), now.iter()) {
                    return Some(FingerprintMismatch { zone, position });
                }
            }
        }
        [
            (
                Zone::EncounterDeck,
                &self.encounter_deck,
                &current.encounter_deck,
            ),
            (Zone::ChaosBag, &self.chaos_bag, &current.chaos_bag),
        ]
        .into_iter()
        .find_map(|(zone, stored, now)| {
            first_differing(stored.iter(), now.iter())
                .map(|position| FingerprintMismatch { zone, position })
        })
    }
}

/// Card codes as plain strings.
fn codes<'a>(cards: impl IntoIterator<Item = &'a CardCode>) -> Vec<String> {
    cards
        .into_iter()
        .map(|code| code.as_str().to_owned())
        .collect()
}

/// A chaos token's name in the fingerprint: a numeric token as its signed
/// modifier (`"0"`, `"-2"`, `"+1"`), any other token by its variant name
/// (`"Skull"`, `"AutoFail"`).
fn token_name(token: ChaosToken) -> String {
    match token {
        ChaosToken::Numeric(0) => "0".to_owned(),
        ChaosToken::Numeric(n) => format!("{n:+}"),
        // `ChaosToken` is non-exhaustive: a symbol token added later is named
        // by its variant without a change here.
        other => format!("{other:?}"),
    }
}

/// The position of the first differing entry of two sequences, or `None` if
/// they are equal.
fn first_differing<T: PartialEq>(
    mut stored: impl Iterator<Item = T>,
    mut current: impl Iterator<Item = T>,
) -> Option<usize> {
    let mut position = 0;
    loop {
        match (stored.next(), current.next()) {
            (None, None) => return None,
            (Some(a), Some(b)) if a == b => position += 1,
            _ => return Some(position),
        }
    }
}
