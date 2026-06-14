//! Investigators: the players' avatars in the game.

use serde::{Deserialize, Serialize};

use super::card::{CardCode, CardInPlay};
use super::location::LocationId;
use super::Skills;

/// Stable identifier for an investigator within a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InvestigatorId(pub u32);

/// An investigator's full state during a scenario.
///
/// Phase-1 minimal shape; fields will grow as later phases need them
/// (mental/physical trauma carried in from the campaign log, traits,
/// passive ability flags, etc.).
///
/// # Invariants
///
/// - `damage` may exceed `max_health` transiently — when that happens
///   the apply loop's damage helpers flip `status` to [`Status::Killed`]
///   and emit [`Event::InvestigatorDefeated`]. Symmetric for `horror`
///   / `max_sanity` / [`Status::Insane`]. The numeric fields are
///   `u8` so they don't wrap; the threshold check is what defines
///   defeat.
/// - Once `status != Status::Active`, the investigator is "out of
///   play": damage / horror helpers no-op, the engine doesn't let
///   them take actions, and card effects targeting investigators
///   should filter by status.
///
/// [`Event::InvestigatorDefeated`]: crate::event::Event::InvestigatorDefeated
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Investigator {
    /// Stable identifier within this scenario.
    pub id: InvestigatorId,
    /// Display name.
    pub name: String,
    /// Location the investigator is currently at, or `None` if they are
    /// "between locations" (resigned, defeated, or in scenario setup
    /// before initial placement).
    pub current_location: Option<LocationId>,
    /// Skill values.
    pub skills: Skills,
    /// Maximum health (physical hit points).
    pub max_health: u8,
    /// Current physical damage suffered.
    pub damage: u8,
    /// Maximum sanity.
    pub max_sanity: u8,
    /// Current horror suffered.
    pub horror: u8,
    /// Clues currently held by the investigator.
    pub clues: u8,
    /// Resources currently held.
    pub resources: u8,
    /// Action points remaining this turn (refreshed at the start of each
    /// investigation phase).
    pub actions_remaining: u8,
    /// Active / Killed / Insane / Resigned. See [`Status`].
    pub status: Status,
    /// Player deck. Cards are listed top-to-bottom; the engine draws
    /// from the front. Populated at scenario setup (and re-shuffled
    /// when empty during a Draw, when that lands in the follow-up
    /// issue).
    pub deck: Vec<CardCode>,
    /// Cards currently in hand.
    pub hand: Vec<CardCode>,
    /// Player discard pile.
    pub discard: Vec<CardCode>,
    /// Cards currently in play under this investigator's control.
    ///
    /// Each entry is a [`CardInPlay`] carrying the card code plus
    /// per-instance state (exhaust, named-uses, accumulated horror /
    /// damage on the asset itself). Instance ids are assigned by the
    /// engine from
    /// [`GameState::next_card_instance_id`](crate::state::GameState::next_card_instance_id)
    /// at enter-play time so duplicate codes are still individually
    /// addressable.
    pub cards_in_play: Vec<CardInPlay>,
    /// Encounter cards in this investigator's threat area — persistent
    /// treacheries and weaknesses engaged with / affecting them (Rules
    /// Reference p.20: "a play area in which encounter cards currently
    /// engaged with and/or affecting an investigator are placed";
    /// cards there are at the investigator's location). Mirrors
    /// [`cards_in_play`](Self::cards_in_play) — same `CardInPlay`
    /// per-instance state — but holds scenario-bag content rather than
    /// player cards. Defaults to empty for backward-compat: states
    /// serialized before this field was added still deserialize.
    ///
    /// [`cards_in_play`]: Self::cards_in_play
    #[serde(default)]
    pub threat_area: Vec<CardInPlay>,
    /// Cards removed from the game (Rules Reference p.10, "Elimination,"
    /// step 1). When this investigator is eliminated, every card they
    /// control in play (`cards_in_play`) and every card they own in an
    /// out-of-play area (`hand`, `deck`, `discard`) is drained into this
    /// pile and removed from the game. Stays empty for Active
    /// investigators. Defaults to empty for backward-compat: states
    /// serialized before this field was added still deserialize.
    #[serde(default)]
    pub removed_from_game: Vec<CardCode>,
}

impl Investigator {
    /// Every in-play card instance this investigator controls that can
    /// carry a triggerable ability: cards in play, then threat-area
    /// cards. The single definition both the reaction-window scan and
    /// the forced instance-scan walk, so the threat area is covered by
    /// both dispatch paths without a duplicate walk. This is the
    /// shared scan source #212 later absorbs.
    pub fn controlled_card_instances(&self) -> impl Iterator<Item = &CardInPlay> {
        self.cards_in_play.iter().chain(self.threat_area.iter())
    }
}

/// Whether an investigator is still active in the scenario, and if not,
/// how they left play.
///
/// Resigned is a placeholder slot until the Resign action lands; the
/// engine doesn't currently produce that variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Status {
    /// Investigator is in play and can take actions.
    #[default]
    Active,
    /// Investigator was killed (`damage >= max_health`).
    Killed,
    /// Investigator was driven insane (`horror >= max_sanity`).
    Insane,
    /// Investigator chose to resign from the scenario. Not yet
    /// produced by the engine; the Resign action is downstream.
    Resigned,
}

/// Why an investigator was defeated. Carried on
/// [`Event::InvestigatorDefeated`] so consumers (campaign log,
/// after-defeat triggers) know the cause without re-reading state.
///
/// [`Event::InvestigatorDefeated`]: crate::event::Event::InvestigatorDefeated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DefeatCause {
    /// Damage reached `max_health`.
    Damage,
    /// Horror reached `max_sanity`.
    Horror,
    /// Investigator resigned. Not yet produced; reserved for the
    /// Resign action.
    Resigned,
}

// `Skills` and `SkillKind` moved down to `card-dsl`
// (`card_dsl::card_data`) — pure data shared across the engine-corpus
// boundary. Re-exported at `game_core::state::{Skills, SkillKind}` (see
// `state/mod.rs`).

#[cfg(test)]
mod threat_area_tests {
    use super::*;
    use crate::state::{CardCode, CardInPlay, CardInstanceId};

    #[test]
    fn new_investigator_has_empty_threat_area() {
        let inv = crate::test_support::test_investigator(1);
        assert!(inv.threat_area.is_empty());
    }

    #[test]
    fn deserializes_when_threat_area_field_absent() {
        // A state serialized before `threat_area` existed must still
        // parse (serde default), proving forward-compat.
        let json = r#"{
            "id": 1, "name": "Test", "current_location": null,
            "skills": {"willpower":3,"intellect":3,"combat":3,"agility":3},
            "max_health": 8, "damage": 0, "max_sanity": 8, "horror": 0,
            "clues": 0, "resources": 0, "actions_remaining": 3,
            "status": "Active", "deck": [], "hand": [], "discard": [],
            "cards_in_play": []
        }"#;
        let inv: Investigator = serde_json::from_str(json).expect("deserialize");
        assert!(inv.threat_area.is_empty());
    }

    #[test]
    fn controlled_card_instances_yields_in_play_then_threat_area() {
        let mut inv = crate::test_support::test_investigator(1);
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new("in-play"),
            CardInstanceId(1),
        ));
        inv.threat_area.push(CardInPlay::enter_play(
            CardCode::new("threat"),
            CardInstanceId(2),
        ));
        let codes: Vec<&str> = inv
            .controlled_card_instances()
            .map(|c| c.code.as_str())
            .collect();
        assert_eq!(codes, vec!["in-play", "threat"]);
    }
}

#[cfg(test)]
mod removed_from_game_tests {
    use super::*;

    #[test]
    fn new_investigator_has_empty_removed_pile() {
        let inv = crate::test_support::test_investigator(1);
        assert!(inv.removed_from_game.is_empty());
    }

    #[test]
    fn deserializes_when_field_absent() {
        // A JSON object missing `removed_from_game` must still parse
        // (serde default), proving forward-compat for pre-field states.
        let json = r#"{
            "id": 1, "name": "Test", "current_location": null,
            "skills": {"willpower":3,"intellect":3,"combat":3,"agility":3},
            "max_health": 8, "damage": 0, "max_sanity": 8, "horror": 0,
            "clues": 0, "resources": 0, "actions_remaining": 3,
            "status": "Active", "deck": [], "hand": [], "discard": [],
            "cards_in_play": []
        }"#;
        let inv: Investigator = serde_json::from_str(json).expect("deserialize");
        assert!(inv.removed_from_game.is_empty());
    }
}
