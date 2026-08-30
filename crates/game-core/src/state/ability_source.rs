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
/// The enum is `#[non_exhaustive]`, and #709 filled in the last two kinds the
/// bullets name. It is **not** the activation side's private vocabulary: since
/// #735 the forced and reaction scans say where an ability comes from with the
/// same descriptor, through
/// [`CandidateSource::Ability`](crate::state::CandidateSource::Ability). The
/// one thing that stayed outside it is a Fast event played from hand, which is
/// not an ability source in the rules' sense — no bullet above names a card in
/// hand — which is why `CandidateSource` wraps this enum rather than the two
/// merging into one. See
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
    /// The **current** act — the third bullet's *"The current act … card"*
    /// (#709). Carries no id: there is exactly one current act, and it is
    /// `act_deck[act_index]`, so naming it by cursor position would let an
    /// action address an act that is no longer current.
    ///
    /// Unlike [`Location`](Self::Location) and [`Enemy`](Self::Enemy), this is
    /// **not** gated on where the investigator stands — Disrupting the Ritual
    /// 01148's ruling says so outright (<https://arkhamdb.com/card/01148>):
    /// *"Your investigator doesn't need to be at the Ritual Site in order to
    /// activate the ability of this act card."* Corpus cards printing such an
    /// ability: Uncovering the Conspiracy 01123 (*"\[action\] The investigators spend
    /// 2 clues per investigator, as a group: Draw the top card of the Cultist
    /// deck."*) and Disrupting the Ritual 01148 (*"\[action\] Spend 1 clue:
    /// Test \[willpower\] (3) or \[agility\] (3). If you succeed, place 1
    /// clue on this Act."*).
    Act,
    /// The **current** agenda — *"… or current agenda card"* (#709). Carries no
    /// id, and is not location-gated, for the same reasons as
    /// [`Act`](Self::Act). Corpus cards printing such an ability: the Core
    /// resigns on Predator or Prey? 01121a and Time Is Running Short 01122 (*"\[action\]:
    /// **Resign.** You don't want to risk taking too long, so you head to
    /// safety with the information you've gathered."*).
    Agenda,
}

impl AbilitySource {
    /// The in-play instance this source names, if any. `None` for a location,
    /// an enemy, the act and the agenda — none of them carries a
    /// [`CardInstanceId`]. Mirrors
    /// [`CandidateSource::instance`](crate::state::CandidateSource::instance).
    ///
    /// Callers that need per-instance state (an exhaust or uses cost, a usage
    /// counter) read this and reject when it is `None`, rather than assuming
    /// every source has a card instance behind it.
    #[must_use]
    pub fn instance(self) -> Option<CardInstanceId> {
        match self {
            AbilitySource::InPlay(id) => Some(id),
            AbilitySource::Location(_)
            | AbilitySource::Enemy(_)
            | AbilitySource::Act
            | AbilitySource::Agenda => None,
        }
    }
}

/// **How an ability is named across a suspension.**
///
/// An ability has no id: a scan mints a candidate naming one, and a resolve
/// looks it back up, possibly several suspensions later. Through #774 that name
/// was a bare index into the card's ability vector, and that worked only while
/// the vector was a pure function of the card.
///
/// It is not, once anything grants abilities to it (ADR 0014). **So an address
/// points at where the ability is *printed*, never at where it currently
/// appears.** Lita Chantler 01117 is the proof: taking control of her removes
/// her granted Parley and adds her two granted buffs *in the same instant,
/// inside the Parley's own resolution*, so a merged index 1 means
/// `[action] Parley` at scan time and a combat modifier at resolve time.
///
/// The address rides `GameState` across the wire — `ResolutionCandidate` sits
/// on an open window frame, and `protocol::ServerMessage::Hello` carries
/// `state: Box<GameState>` — so this is part of the serialized shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AbilityAddress {
    /// The ability at this index of the card's **printed** abilities, on the
    /// side of it that is in effect (#774).
    Printed(u8),
    /// An ability another card grants to this one — addressed by the
    /// *granter's* printed text, which is what stays put.
    ///
    /// The recipient is already named by the candidate's own source, and the
    /// granter's clause means the same thing whichever copy of the granter is
    /// in play, so the granter is a [`CardCode`](crate::state::CardCode) rather
    /// than an [`AbilitySource`].
    Granted {
        /// Printed code of the card declaring the grant.
        granter: super::card::CardCode,
        /// Index of the granter's printed `Effect::Grant` ability, on the side
        /// of the granter that is in effect.
        ability: u8,
        /// Position within that grant's `abilities` list.
        sub: u8,
    },
}

impl AbilityAddress {
    /// The printed index this address names, or `None` for a granted ability.
    ///
    /// The per-instance usage counter (`CardInPlay::ability_usage`) is keyed by
    /// printed index — a `BTreeMap<u8, _>`, because JSON object keys are
    /// strings and an enum key does not survive the wire. A granted ability
    /// therefore has nowhere to record a *"Limit X per \[period\]"* cap, which
    /// is why `reject_untrackable_usage_limit` refuses one before any cost is
    /// paid. No corpus card grants an ability with a printed limit; the Parlor
    /// 01115's Parley has none.
    #[must_use]
    pub fn printed_index(&self) -> Option<u8> {
        match self {
            AbilityAddress::Printed(index) => Some(*index),
            AbilityAddress::Granted { .. } => None,
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
            // Flashlight 01087's shape: the designated action rides the frame
            // beside the (empty) residual effect (#805).
            designator: Some(card_dsl::dsl::investigate(-2i8)),
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
        assert_eq!(
            AbilitySource::Act.instance(),
            None,
            "the current act is a scenario board card and has no card instance",
        );
        assert_eq!(
            AbilitySource::Agenda.instance(),
            None,
            "the current agenda is a scenario board card and has no card instance",
        );
    }

    /// The descriptor also rides the wire *inside* a
    /// [`CandidateSource`](crate::state::CandidateSource), on a
    /// `ResolutionCandidate` in an open window frame — the payload #735 broke,
    /// deliberately and without a migration (same posture as #707/#709).
    #[test]
    fn a_candidate_source_round_trips_through_serialization() {
        use crate::state::CandidateSource;
        for source in [
            CandidateSource::Ability(AbilitySource::InPlay(CardInstanceId(7))),
            CandidateSource::Ability(AbilitySource::Location(crate::state::LocationId(4))),
            CandidateSource::Ability(AbilitySource::Enemy(crate::state::EnemyId(5))),
            CandidateSource::Ability(AbilitySource::Act),
            CandidateSource::Ability(AbilitySource::Agenda),
            CandidateSource::Hand,
        ] {
            let json = serde_json::to_string(&source).expect("serializes");
            assert_eq!(
                serde_json::from_str::<CandidateSource>(&json).expect("deserializes"),
                source,
            );
        }
    }

    /// Every kind rides the wire, not just the one #707 shipped.
    #[test]
    fn the_location_enemy_act_and_agenda_kinds_round_trip_through_serialization() {
        for source in [
            AbilitySource::Location(crate::state::LocationId(4)),
            AbilitySource::Enemy(crate::state::EnemyId(5)),
            AbilitySource::Act,
            AbilitySource::Agenda,
        ] {
            let json = serde_json::to_string(&source).expect("serializes");
            assert_eq!(
                serde_json::from_str::<AbilitySource>(&json).expect("deserializes"),
                source,
            );
        }
    }

    /// The address rides `GameState` across the wire — a `ResolutionCandidate`
    /// on an open window frame, inside the `state: Box<GameState>` that
    /// `protocol::ServerMessage::Hello` carries — so both kinds want pinning.
    #[test]
    fn every_address_kind_round_trips_through_serialization() {
        for address in [
            AbilityAddress::Printed(3),
            AbilityAddress::Granted {
                granter: super::super::card::CardCode::new("01115"),
                ability: 1,
                sub: 0,
            },
        ] {
            let json = serde_json::to_string(&address).expect("serializes");
            assert_eq!(
                serde_json::from_str::<AbilityAddress>(&json).expect("deserializes"),
                address,
            );
        }
    }

    /// Only a printed address has an index the per-instance usage counter can
    /// be keyed by; a granted one is refused a *"Limit X per \[period\]"* cap
    /// rather than silently uncapped (`reject_untrackable_usage_limit`).
    #[test]
    fn only_a_printed_address_has_an_index_to_key_a_usage_limit_by() {
        assert_eq!(AbilityAddress::Printed(2).printed_index(), Some(2));
        assert_eq!(
            AbilityAddress::Granted {
                granter: super::super::card::CardCode::new("01115"),
                ability: 1,
                sub: 0,
            }
            .printed_index(),
            None,
        );
    }
}
