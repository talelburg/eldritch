//! The chaos bag: pool of tokens drawn during skill tests.

use serde::{Deserialize, Serialize};

/// One token in the chaos bag.
///
/// Tokens are drawn during skill tests; each token's modifier (positive or
/// negative) is added to the test's skill total. Symbol tokens
/// ([`Skull`], [`Cultist`], [`Tablet`], [`ElderThing`]) have scenario- or
/// effect-specific modifiers that are resolved by separate logic, not by
/// this enum directly.
///
/// [`Skull`]: ChaosToken::Skull
/// [`Cultist`]: ChaosToken::Cultist
/// [`Tablet`]: ChaosToken::Tablet
/// [`ElderThing`]: ChaosToken::ElderThing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChaosToken {
    /// Numeric modifier token (+1, 0, −1, …, −8). The wrapped value is the
    /// modifier applied to the test's skill total when drawn.
    Numeric(i8),
    /// Skull symbol — modifier set per scenario, often `−1` to `−4`.
    Skull,
    /// Cultist symbol — scenario-specific effect.
    Cultist,
    /// Tablet symbol — scenario-specific effect.
    Tablet,
    /// Elder Thing symbol — scenario-specific effect.
    ElderThing,
    /// Auto-fail. Test fails regardless of skill total.
    AutoFail,
    /// Elder Sign. Investigator's own elder-sign ability triggers.
    ElderSign,
}

/// The pool of chaos tokens for the current scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosBag {
    /// Tokens currently in the bag. Order is not significant; the engine
    /// draws via the deterministic RNG, not list position.
    pub tokens: Vec<ChaosToken>,
}

impl ChaosBag {
    /// Build a chaos bag from an iterator of tokens.
    #[must_use]
    pub fn new(tokens: impl IntoIterator<Item = ChaosToken>) -> Self {
        Self {
            tokens: tokens.into_iter().collect(),
        }
    }

    /// Number of tokens currently in the bag.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// True if the bag has no tokens (e.g. all temporarily set aside).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}
