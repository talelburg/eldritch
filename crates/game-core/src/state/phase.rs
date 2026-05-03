//! Game phases.

use serde::{Deserialize, Serialize};

/// One of the four phases in an Arkham Horror round.
///
/// Phases cycle in order: [`Mythos`] → [`Investigation`] → [`Enemy`] → [`Upkeep`] → [`Mythos`]…
///
/// [`Mythos`]: Phase::Mythos
/// [`Investigation`]: Phase::Investigation
/// [`Enemy`]: Phase::Enemy
/// [`Upkeep`]: Phase::Upkeep
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    /// Encounter cards drawn, doom advances.
    Mythos,
    /// Investigators take turns, spending action points.
    Investigation,
    /// Engaged enemies attack; investigators may resolve hunter movement.
    Enemy,
    /// Cards ready, hand size adjusted, agenda/act effects.
    Upkeep,
}

impl Phase {
    /// The phase that follows this one in normal round order.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Mythos => Self::Investigation,
            Self::Investigation => Self::Enemy,
            Self::Enemy => Self::Upkeep,
            Self::Upkeep => Self::Mythos,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Phase;

    #[test]
    fn phases_cycle_in_round_order() {
        let mut p = Phase::Mythos;
        for expected in [
            Phase::Investigation,
            Phase::Enemy,
            Phase::Upkeep,
            Phase::Mythos,
        ] {
            p = p.next();
            assert_eq!(p, expected);
        }
    }
}
