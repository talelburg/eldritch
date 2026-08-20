//! What an activation names: the **ability source**.

use serde::{Deserialize, Serialize};

use super::card::CardInstanceId;
use super::enemy::EnemyId;
use super::location::LocationId;

/// The thing whose ability is being used — the descriptor an activation
/// names instead of a bare card instance (#707).
///
/// The Rules Reference lists the sources an investigator may use a triggered
/// ability from, and it lists *things*, not zones —
/// `glossary/Triggered_Abilities.md`, verbatim:
///
/// > An investigator is permitted to use triggered abilities (\[free\],
/// > \[reaction\], and \[action\] abilities) from the following sources:
/// >
/// > - A card in play and under his or her control. This includes his or her
/// >   investigator card.
/// > - A scenario card that is in play and at the same location as the
/// >   investigator. This includes the location itself, encounter cards placed
/// >   at that location, and all encounter cards in the threat area of any
/// >   investigator at that location.
/// > - The current act or current agenda card.
/// > - Any card that explicitly allows the investigator to activate its ability.
///
/// **[`InPlay`](Self::InPlay) means any card instance in play, wherever it
/// sits** — the investigator card, a card in play, a threat-area card, and
/// later a location or enemy attachment all carry a [`CardInstanceId`], and the
/// descriptor deliberately does not record which collection holds it. *Whether*
/// a given investigator can reach it is the reachability predicate's answer
/// (`engine::ability_source`), not a fact about the address. Keeping zone
/// membership out of the addressing vocabulary is what stops the four bullets
/// becoming a pile of special cases.
///
/// The enum is `#[non_exhaustive]` and grows: an act kind and an agenda kind
/// with #709. It is the activation-side
/// peer of [`CandidateSource`](crate::state::CandidateSource), which the forced
/// and reaction scans use to say the same thing about an ability that fires on
/// its own; the two converge when those kinds land. See
/// `docs/adr/0010-an-activation-names-an-ability-source.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AbilitySource {
    /// A card instance in play, wherever it sits.
    InPlay(CardInstanceId),
    /// A location card itself — the second bullet's *"the location itself"*
    /// (#708). Locations carry a [`LocationId`] and no
    /// [`CardInstanceId`], which is why this
    /// cannot fold into [`InPlay`](Self::InPlay).
    Location(LocationId),
    /// An enemy in play — an *"encounter card placed at that location"* (#708).
    /// The Parley abilities the corpus prints sit here: Herman Collins 01138,
    /// Peter Warren 01139, Victoria Devereux 01140, Mob Enforcer 01101.
    Enemy(EnemyId),
}

impl AbilitySource {
    /// The in-play instance this source names, if any. `None` for a location
    /// or an enemy — neither carries a
    /// [`CardInstanceId`] — and `None` again for
    /// the act and agenda kinds #709 adds. Mirrors
    /// [`CandidateSource::instance`](crate::state::CandidateSource::instance).
    ///
    /// Callers that need per-instance state (an exhaust or uses cost, a usage
    /// counter) read this and reject when it is `None`, rather than assuming
    /// every source has a card instance behind it.
    #[must_use]
    pub fn instance(self) -> Option<CardInstanceId> {
        match self {
            AbilitySource::InPlay(id) => Some(id),
            AbilitySource::Location(_) | AbilitySource::Enemy(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ActionResume;

    /// The descriptor rides the wire twice: inside a parked
    /// [`ActionResume::ActivateAbility`] frame in serialized game state, and in
    /// the seed state the server replays its action log over. #707 broke that
    /// payload deliberately and without a migration (persisted games are
    /// discarded, and schema versioning is #581), so what the shape *is* wants
    /// pinning.
    #[test]
    fn a_source_round_trips_through_serialization() {
        let source = AbilitySource::InPlay(CardInstanceId(7));
        let json = serde_json::to_string(&source).expect("serializes");
        assert_eq!(
            serde_json::from_str::<AbilitySource>(&json).expect("deserializes"),
            source,
        );
    }

    #[test]
    fn a_parked_activation_frame_round_trips_through_serialization() {
        let resume = ActionResume::ActivateAbility {
            source: AbilitySource::InPlay(CardInstanceId(7)),
            effect: card_dsl::dsl::Effect::Seq(vec![]),
        };
        let json = serde_json::to_string(&resume).expect("serializes");
        assert_eq!(
            serde_json::from_str::<ActionResume>(&json).expect("deserializes"),
            resume,
        );
    }

    #[test]
    fn every_source_kind_reports_its_instance() {
        assert_eq!(
            AbilitySource::InPlay(CardInstanceId(3)).instance(),
            Some(CardInstanceId(3)),
        );
        assert_eq!(
            AbilitySource::Location(crate::state::LocationId(1)).instance(),
            None,
            "a location has a LocationId and no card instance",
        );
        assert_eq!(
            AbilitySource::Enemy(crate::state::EnemyId(1)).instance(),
            None,
            "an enemy has an EnemyId and no card instance",
        );
    }

    /// Every kind rides the wire, not just the one #707 shipped.
    #[test]
    fn the_location_and_enemy_kinds_round_trip_through_serialization() {
        for source in [
            AbilitySource::Location(crate::state::LocationId(4)),
            AbilitySource::Enemy(crate::state::EnemyId(5)),
        ] {
            let json = serde_json::to_string(&source).expect("serializes");
            assert_eq!(
                serde_json::from_str::<AbilitySource>(&json).expect("deserializes"),
                source,
            );
        }
    }
}
