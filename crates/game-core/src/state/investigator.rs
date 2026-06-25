//! Investigators: the players' avatars in the game.

use serde::{Deserialize, Serialize};

use super::card::{
    bump_usage, usage_exhausted, AbilityUsageRecord, CardCode, CardInPlay, CardInstanceId,
};
use super::location::LocationId;
use super::Skills;
use std::collections::BTreeMap;

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
    /// The investigator's own `ArkhamDB` card code (01001 for Roland
    /// Banks). Set at roster seating from `RosterEntry.investigator`;
    /// the elder-sign firing path and the seated-reaction scan look the
    /// investigator card's abilities up by this code
    /// (`abilities_for(card_code)`). An empty sentinel (`CardCode::new("")`)
    /// marks the pre-seated `test_support` / builder path — codepaths skip
    /// empty codes, so those investigators carry no investigator-card
    /// abilities. Required on the wire (#453): a payload omitting it is
    /// rejected rather than silently degrading to the empty sentinel.
    ///
    /// **Bridge (#118), sunset by #448:** when the investigator card
    /// becomes a real `CardInPlay` (health/sanity/soak), this field and
    /// [`ability_usage`](Self::ability_usage) fold into the uniform path.
    pub card_code: CardCode,
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
    /// [`GameState::card_instance_ids`](crate::state::GameState::card_instance_ids)
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
    /// player cards. Required on the wire (#453).
    ///
    /// [`cards_in_play`]: Self::cards_in_play
    pub threat_area: Vec<CardInPlay>,
    /// Cards removed from the game (Rules Reference p.10, "Elimination,"
    /// step 1). When this investigator is eliminated, every card they
    /// control in play (`cards_in_play`) and every card they own in an
    /// out-of-play area (`hand`, `deck`, `discard`) is drained into this
    /// pile and removed from the game. Stays empty for Active
    /// investigators. Required on the wire (#453).
    pub removed_from_game: Vec<CardCode>,
    /// Per-ability "Limit X per \[period\]" usage records for this
    /// investigator's **own card** abilities (Roland Banks's once-per-round
    /// `[reaction]`). Mirrors [`CardInPlay::ability_usage`] — the investigator
    /// card is not a `CardInPlay`, so it needs its own usage home. Keyed by
    /// ability index within the investigator card's `abilities()`. Lazy
    /// reset: a stale-round record reads as 0 (see [`CardInPlay::ability_usage`]
    /// docs). Required on the wire (#453).
    ///
    /// **Bridge (#118), sunset by #448:** retired when the investigator card
    /// becomes a real `CardInPlay`.
    ///
    /// [`CardInPlay::ability_usage`]: crate::state::CardInPlay::ability_usage
    pub ability_usage: BTreeMap<u8, AbilityUsageRecord>,
    /// Source instances whose [`ExtraActionCost`](crate::dsl::Restriction::ExtraActionCost)
    /// with `first_each_round` has already surcharged an action this round
    /// (Frozen in Fear 01164). Cleared at the round boundary. Keyed by
    /// instance so multiple surcharge sources track independently. Required
    /// on the wire (#453).
    pub action_surcharge_spent_this_round: std::collections::BTreeSet<CardInstanceId>,
    /// The investigator's own card as a real in-play permanent: it holds the
    /// investigator's health/sanity capacity (from `CardKind::Investigator`
    /// metadata) and is the default damage/horror soaker via its
    /// `accumulated_damage` / `accumulated_horror`. Lives here rather than in
    /// `cards_in_play` so loops over "cards the player played" never touch it
    /// (#448). Required on the wire.
    pub investigator_card: CardInPlay,
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

    /// Whether this investigator's own-card ability at `ability_index` has
    /// reached its per-period [`UsageLimit`](crate::dsl::UsageLimit). Mirrors
    /// [`CardInPlay::is_usage_exhausted`] over [`ability_usage`](Self::ability_usage).
    #[must_use]
    pub fn is_usage_exhausted(
        &self,
        ability_index: u8,
        limit: Option<crate::dsl::UsageLimit>,
        current_round: u32,
    ) -> bool {
        usage_exhausted(&self.ability_usage, ability_index, limit, current_round)
    }

    /// Record one firing of this investigator's own-card ability at
    /// `ability_index` against the current period. Mirrors
    /// [`CardInPlay::bump_ability_usage`].
    pub fn bump_ability_usage(&mut self, ability_index: u8, current_round: u32) {
        bump_usage(&mut self.ability_usage, ability_index, current_round);
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
    fn omitting_any_required_field_is_rejected() {
        // Every field is required on the wire (#453): a payload missing one
        // fails loudly rather than silently defaulting. `card_code` in
        // particular — an absent identity field must not degrade to the empty
        // sentinel (which would silently disable that investigator's
        // elder-sign + seated reaction).
        let inv = crate::test_support::test_investigator(1);
        let full = serde_json::to_value(&inv).expect("serialize");
        // The complete object still round-trips.
        serde_json::from_value::<Investigator>(full.clone()).expect("full object deserializes");
        // Each formerly-`#[serde(default)]` field is now mandatory.
        for field in [
            "card_code",
            "threat_area",
            "removed_from_game",
            "ability_usage",
            "action_surcharge_spent_this_round",
            "investigator_card",
        ] {
            let mut v = full.clone();
            v.as_object_mut()
                .expect("investigator serializes to a JSON object")
                .remove(field)
                .unwrap_or_else(|| panic!("`{field}` should be present in the serialized form"));
            assert!(
                serde_json::from_value::<Investigator>(v).is_err(),
                "omitting `{field}` must be rejected, not defaulted"
            );
        }
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
mod ability_usage_tests {
    use crate::dsl::{UsageLimit, UsagePeriod};
    use crate::state::AbilityUsageRecord;

    #[test]
    fn new_investigator_has_empty_ability_usage() {
        let inv = crate::test_support::test_investigator(1);
        assert!(inv.ability_usage.is_empty());
    }

    #[test]
    fn usage_exhausts_after_limit_within_a_round_and_resets_across_rounds() {
        let mut inv = crate::test_support::test_investigator(1);
        let limit = Some(UsageLimit {
            count: 1,
            period: UsagePeriod::Round,
        });
        // Ability 0, round 5: not yet fired → not exhausted.
        assert!(!inv.is_usage_exhausted(0, limit, 5));
        // Fire once in round 5 → now exhausted in round 5.
        inv.bump_ability_usage(0, 5);
        assert!(inv.is_usage_exhausted(0, limit, 5));
        assert_eq!(
            inv.ability_usage.get(&0),
            Some(&AbilityUsageRecord::new(5, 1))
        );
        // Round 6: lazy reset → not exhausted (stored record is stale).
        assert!(!inv.is_usage_exhausted(0, limit, 6));
        // No limit (None) is never exhausted.
        assert!(!inv.is_usage_exhausted(0, None, 5));
    }
}

#[cfg(test)]
mod removed_from_game_tests {
    #[test]
    fn new_investigator_has_empty_removed_pile() {
        let inv = crate::test_support::test_investigator(1);
        assert!(inv.removed_from_game.is_empty());
    }
}

#[cfg(test)]
mod investigator_card_tests {
    #[test]
    fn test_investigator_has_an_investigator_card_with_the_synthetic_code() {
        let inv = crate::test_support::test_investigator(1);
        assert_eq!(
            inv.investigator_card.code.as_str(),
            crate::test_support::TEST_INV
        );
        assert_eq!(inv.investigator_card.accumulated_damage, 0);
        assert_eq!(inv.investigator_card.accumulated_horror, 0);
    }
}
