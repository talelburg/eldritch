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
#[non_exhaustive]
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
#[non_exhaustive]
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
}

/// Per-scenario numeric modifiers for the four symbol tokens.
///
/// Skull/Cultist/Tablet/ElderThing don't have intrinsic numeric values
/// — each scenario (and difficulty) sets its own. This struct is set
/// at scenario setup and is immutable for the duration of the scenario.
///
/// Symbol tokens may also trigger non-numeric scenario effects (e.g.
/// "Cultist: −2 and an enemy spawns"). Only the numeric modifier lives
/// here; effect triggering is a separate, downstream concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TokenModifiers {
    /// Numeric modifier for [`ChaosToken::Skull`].
    pub skull: i8,
    /// Numeric modifier for [`ChaosToken::Cultist`].
    pub cultist: i8,
    /// Numeric modifier for [`ChaosToken::Tablet`].
    pub tablet: i8,
    /// Numeric modifier for [`ChaosToken::ElderThing`].
    pub elder_thing: i8,
}

/// The result of resolving a drawn chaos token against the scenario's
/// modifier table.
///
/// Numeric/symbol tokens collapse to a [`Modifier`](Self::Modifier) (the
/// integer added to the test's skill total). [`AutoFail`](Self::AutoFail)
/// short-circuits the test to a guaranteed failure regardless of total.
/// [`ElderSign`](Self::ElderSign) hands control to the active
/// investigator's elder-sign ability, whose dispatch is downstream of
/// this resolution step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenResolution {
    /// Numeric modifier contributed to the skill-test total.
    Modifier(i8),
    /// Test fails regardless of skill total.
    AutoFail,
    /// Active investigator's elder-sign ability triggers.
    ElderSign,
}

/// Resolve a drawn chaos token to its [`TokenResolution`] given the
/// scenario's symbol-token modifiers.
#[must_use]
pub fn resolve_token(token: ChaosToken, modifiers: &TokenModifiers) -> TokenResolution {
    match token {
        ChaosToken::Numeric(n) => TokenResolution::Modifier(n),
        ChaosToken::Skull => TokenResolution::Modifier(modifiers.skull),
        ChaosToken::Cultist => TokenResolution::Modifier(modifiers.cultist),
        ChaosToken::Tablet => TokenResolution::Modifier(modifiers.tablet),
        ChaosToken::ElderThing => TokenResolution::Modifier(modifiers.elder_thing),
        ChaosToken::AutoFail => TokenResolution::AutoFail,
        ChaosToken::ElderSign => TokenResolution::ElderSign,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standard-difficulty Night of the Zealot symbol-token values, as
    /// a representative scenario configuration to exercise resolution
    /// against non-zero modifiers.
    fn night_of_the_zealot_standard() -> TokenModifiers {
        TokenModifiers {
            skull: -1,
            cultist: -2,
            tablet: -3,
            elder_thing: -4,
        }
    }

    #[test]
    fn numeric_token_resolves_to_its_value() {
        let mods = night_of_the_zealot_standard();
        assert_eq!(
            resolve_token(ChaosToken::Numeric(1), &mods),
            TokenResolution::Modifier(1),
        );
        assert_eq!(
            resolve_token(ChaosToken::Numeric(-2), &mods),
            TokenResolution::Modifier(-2),
        );
    }

    #[test]
    fn symbol_tokens_resolve_to_scenario_modifiers() {
        let mods = night_of_the_zealot_standard();
        assert_eq!(
            resolve_token(ChaosToken::Skull, &mods),
            TokenResolution::Modifier(-1),
        );
        assert_eq!(
            resolve_token(ChaosToken::Cultist, &mods),
            TokenResolution::Modifier(-2),
        );
        assert_eq!(
            resolve_token(ChaosToken::Tablet, &mods),
            TokenResolution::Modifier(-3),
        );
        assert_eq!(
            resolve_token(ChaosToken::ElderThing, &mods),
            TokenResolution::Modifier(-4),
        );
    }

    #[test]
    fn autofail_and_elder_sign_resolve_to_their_special_variants() {
        let mods = night_of_the_zealot_standard();
        assert_eq!(
            resolve_token(ChaosToken::AutoFail, &mods),
            TokenResolution::AutoFail,
        );
        assert_eq!(
            resolve_token(ChaosToken::ElderSign, &mods),
            TokenResolution::ElderSign,
        );
    }
}
