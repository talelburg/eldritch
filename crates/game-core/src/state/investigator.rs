//! Investigators: the players' avatars in the game.

use std::collections::BTreeSet;
use std::iter;

use card_dsl::card_data::CardKind;
use card_dsl::dsl::AdditionalActionRestriction;
use serde::{Deserialize, Serialize};

use crate::card_registry::CardRegistry;
use crate::engine::OptionTarget;
use crate::state::{CardCode, CardInPlay, CardInstanceId, LocationId, Skills};

/// One additional action an effect granted for this turn, held on
/// [`Investigator::additional_actions_this_turn`] until it is used or the turn
/// ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdditionalActionGrant {
    /// What the action may be used for; `None` is any action.
    pub restriction: Option<AdditionalActionRestriction>,
}

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
/// - Physical damage is tracked via
///   [`investigator_card.accumulated_damage`](CardInPlay::accumulated_damage);
///   when [`damage()`](Self::damage) reaches [`max_health()`](Self::max_health)
///   the apply loop flips `status` to [`Status::Defeated`] and emits
///   [`Event::InvestigatorEliminated`]. Symmetric for horror /
///   [`max_sanity()`](Self::max_sanity). The accessor methods are `u8` so they
///   don't wrap; the threshold check is what defines defeat.
/// - Once `status != Status::Active`, the investigator is "out of
///   play": damage / horror helpers no-op, the engine doesn't let
///   them take actions, and card effects targeting investigators
///   should filter by status.
///
/// [`Event::InvestigatorEliminated`]: crate::event::Event::InvestigatorEliminated
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
    /// Clues currently held by the investigator.
    pub clues: u8,
    /// Resources currently held.
    pub resources: u8,
    /// **Standard** actions remaining this turn (refreshed at the start of each
    /// investigation phase). Additional actions are not counted here: the
    /// actions the investigator can take are this plus their additional
    /// actions, which `engine::actions_available` works out (see **Standard
    /// action / additional action** in `GLOSSARY.md`).
    pub actions_remaining: u8,
    /// Additional actions pushed by an effect this turn and not yet used —
    /// "Skids" O'Toole 01003's *"You may take an additional action this
    /// turn."* Taking an action that one admits removes it; the end of the
    /// turn forfeits the rest (FAQ 1.31, `glossary/Action.md`: *"Any unused
    /// actions are forfeited at the end of an investigator's turn"*). A grant
    /// from a card in play is never stored here: it is read off the card.
    /// Required on the wire.
    pub additional_actions_this_turn: Vec<AdditionalActionGrant>,
    /// Card instances whose additional-action grant was spent this turn — Leo
    /// De Luca 01048's *"You may take an additional action during your
    /// turn."* Keyed by instance, so a second copy entering play
    /// grants again (his ruling: *"If you then play a second copy of the card,
    /// you gain a further additional action."*). Cleared as the turn begins.
    /// Required on the wire.
    pub grants_spent_this_turn: BTreeSet<CardInstanceId>,
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
    /// Weaknesses set aside from the opening hand during scenario setup
    /// (Rules Reference step 8: "Each weakness card drawn during this step
    /// is ignored, set aside (without resolving it), and replaced by
    /// drawing another card from the deck."). Transient: populated by
    /// `replace_opening_hand_weaknesses` (in `engine::dispatch::cards`)
    /// during the setup draw and mulligan redraw, then drained back into
    /// `deck` (and shuffled) once the mulligan loop fully drains.
    /// Empty for all investigators once the Investigation phase begins.
    ///
    /// `#[serde(default)]`: this field is always empty in wire-serialized
    /// game-in-progress states (setup is complete before any client sees the
    /// state), so older payloads without the field deserialize to the correct
    /// empty-vector default rather than failing.
    #[serde(default)]
    pub setaside: Vec<CardCode>,
    /// Cards this investigator owns that have been removed from the game.
    /// Mostly elimination's (Rules Reference p.10, "Elimination," step 1):
    /// when this investigator is eliminated, every card they control in play
    /// (`cards_in_play`) and every card they own in an out-of-play area
    /// (`hand`, `deck`, `discard`) is drained into this pile and removed from
    /// the game. A card of theirs that
    /// [`NativeCx::remove_from_game`](crate::engine::NativeCx::remove_from_game)
    /// takes out of play lands here too, by its owner, wherever it was.
    /// Required on the wire (#453).
    pub removed_from_game: Vec<CardCode>,
    /// Source instances whose [`ExtraActionCost`](card_dsl::dsl::Restriction::ExtraActionCost)
    /// with `first_each_round` has already surcharged an action this round
    /// (Frozen in Fear 01164). Cleared at the round boundary. Keyed by
    /// instance so multiple surcharge sources track independently. Required
    /// on the wire (#453).
    pub action_surcharge_spent_this_round: BTreeSet<CardInstanceId>,
    /// The investigator's own card as a real in-play permanent: it holds the
    /// investigator's health/sanity capacity (from `CardKind::Investigator`
    /// metadata) and is the default damage/horror soaker via its
    /// `accumulated_damage` / `accumulated_horror`. Lives here rather than in
    /// `cards_in_play` so loops over "cards the player played" never touch it
    /// (#448). Required on the wire.
    pub investigator_card: CardInPlay,
}

impl Investigator {
    /// The board surface that names this investigator in a prompt: their own
    /// investigator card. An option that chooses an investigator — whom a
    /// hunter engages, a chosen investigator, the soak prompt's "take it
    /// yourself" — anchors here, so it renders on the investigator panel's
    /// card (ADR 0011, #950). The card, not the seat, because it is the
    /// rendered surface and where damage and horror are placed; a test picks
    /// such an option through this same method.
    #[must_use]
    pub fn card_anchor(&self) -> OptionTarget {
        OptionTarget::CardInstance(self.investigator_card.instance_id)
    }

    /// Every in-play card instance this investigator controls that can
    /// carry a triggerable ability: the **investigator card** first, then
    /// cards in play, then threat-area cards. The single definition both
    /// the reaction-window scan and the forced instance-scan walk, so the
    /// investigator card and threat area are covered by both dispatch paths
    /// without a duplicate walk (#448 cp3a folds the investigator card into
    /// this iterator, retiring the bespoke `scan_investigator_card_reactions`
    /// source). This is the shared scan source #212 later absorbs.
    ///
    /// **This is "cards at your seat with live abilities", not "cards you
    /// control", and the two are not the same set.** The threat area holds
    /// both: a *player* card there is the investigator's own (Cover Up 01007
    /// entered play from Roland's deck, so per
    /// `glossary/Ownership_and_Control.md` — *"Cards by default enter play
    /// under their owner's control"* — he controls it), but an *encounter*
    /// card there is owned and controlled by the scenario. Frozen in Fear
    /// 01164 sits in your threat area and is not one of your cards. The
    /// official FAQ draws the line the abilities need: *"In general, 'your
    /// cards' are the cards you currently control. If you own a card but do
    /// not control it, it is not 'yours' for the purposes of abilities."*
    /// (`data/official-faq/Frequently_Asked_Questions.md`.)
    ///
    /// Harmless today — no implemented ability reads "cards you control" as a
    /// *set* — but the first one that does must not reach for this helper
    /// unfiltered. The same conflation shows from the other end in
    /// `engine::ability_source::reachable_sources`, which called this iterator
    /// "the definition of 'a card in play and under his or her control'".
    ///
    /// The investigator card is deliberately *not* in `cards_in_play`, so
    /// loops over "cards the player played" (asset queries, elimination
    /// drain, discard-all, soak `build_soakers`) keep iterating
    /// `cards_in_play` directly and never touch it.
    pub fn controlled_card_instances(&self) -> impl Iterator<Item = &CardInPlay> {
        iter::once(&self.investigator_card)
            .chain(self.cards_in_play.iter())
            .chain(self.threat_area.iter())
    }

    /// Physical damage currently on the investigator — reads
    /// `investigator_card.accumulated_damage` (#448 cp2a).
    #[must_use]
    pub fn damage(&self) -> u8 {
        self.investigator_card.accumulated_damage
    }

    /// Horror currently on the investigator. See [`Self::damage`].
    #[must_use]
    pub fn horror(&self) -> u8 {
        self.investigator_card.accumulated_horror
    }

    /// Maximum health (printed) — reads `CardKind::Investigator` metadata from
    /// `cards` (#448 cp2a). Panics if the investigator card's code is not a
    /// `CardKind::Investigator` in `cards`.
    #[must_use]
    pub fn max_health(&self, cards: &CardRegistry) -> u8 {
        investigator_capacity(cards, &self.investigator_card.code).0
    }

    /// Maximum sanity (printed). See [`Self::max_health`].
    #[must_use]
    pub fn max_sanity(&self, cards: &CardRegistry) -> u8 {
        investigator_capacity(cards, &self.investigator_card.code).1
    }
}

/// (health, sanity) printed capacity for an investigator card, from `reg`.
/// Panics if the code is not an investigator card — a state-shape invariant violation, surfaced
/// rather than silently defaulted.
fn investigator_capacity(reg: &CardRegistry, code: &CardCode) -> (u8, u8) {
    match &(reg.metadata_for)(code)
        .expect("investigator card code absent from registry")
        .kind
    {
        CardKind::Investigator { health, sanity, .. } => (*health, *sanity),
        _ => panic!("investigator_card.code does not resolve to a CardKind::Investigator"),
    }
}

/// Whether an investigator is still active in the scenario, and if not,
/// how they left play.
///
/// # Three states, because the rules' scenario layer has three
///
/// `glossary/Elimination.md` is the union: *"A player is eliminated from a
/// scenario any time his or her investigator is defeated, or if he or she
/// resigns"*, and `glossary/Resign.md` draws the only line inside it: *"An
/// investigator who resigns is not considered to have been defeated."* So
/// non-`Active` splits exactly two ways, and *how* a defeat happened rides
/// [`EliminationCause`] on [`Event::InvestigatorEliminated`] rather than
/// fragmenting this enum (#814).
///
/// **Killed and driven insane are not on this axis.** They are campaign-log
/// states derived from accumulated trauma totals against *printed*
/// health/sanity — `glossary/Campaign_Play.md`: *"If an investigator has
/// physical trauma equal to his or her printed health, the investigator is
/// **killed**."* An investigator defeated by damage at zero prior trauma is
/// defeated and owes 1 physical trauma; they are not killed. The derivation
/// belongs to the campaign log (#766), which reads the recorded totals — not
/// to the defeat that ended the scenario.
///
/// Nor does the corpus ask for it. Cards read *eliminated* (33 cards, e.g.
/// Threads of Time 04315) and *undefeated* (38, e.g. All In 02068 *"If each
/// undefeated investigator has resigned, advance."*); killed and insane appear
/// only as outcomes a card **imposes**, never as a state a card tests. Chaos
/// Incarnate 06289 is the proof — *"Each investigator is defeated **and driven
/// insane**."* spells out both, because the defeat alone does not imply the
/// second.
///
/// [`Event::InvestigatorEliminated`]: crate::event::Event::InvestigatorEliminated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Status {
    /// Investigator is in play and can take actions.
    #[default]
    Active,
    /// Investigator resigned from the scenario, via an ability printing the
    /// [`Resign`](card_dsl::dsl::ActionDesignator::Resign) action designator —
    /// the Parlor 01115 is the corpus's first (#644).
    ///
    /// `glossary/Resign.md`: *"When an investigator resigns, the investigator
    /// is eliminated by resignation … An investigator who resigns is not
    /// considered to have been defeated."* So this is the one non-`Active`
    /// status that carries no defeat, and phase 9's campaign log reads that
    /// distinction — trauma is owed for defeat, not for walking out.
    Resigned,
    /// Investigator was defeated — however it happened.
    ///
    /// Every defeat lands here, whatever caused it: damage reaching
    /// `max_health`, horror reaching `max_sanity`, or a card ability defeating
    /// outright (`glossary/Defeat.md`: *"An investigator might also be defeated
    /// by a card ability."*). Which one it was is on
    /// [`EliminationCause`], carried by [`Event::InvestigatorEliminated`].
    ///
    /// Not killed and not driven insane: `glossary/Defeat.md` makes those two
    /// consequences of **trauma** — *"Taking trauma may cause an investigator
    /// to be killed or driven insane"* — so a defeat that hands out one trauma
    /// has taken the first step on that track, not the last. See the enum's
    /// own note.
    ///
    /// [`Event::InvestigatorEliminated`]: crate::event::Event::InvestigatorEliminated
    Defeated,
}

/// Why an investigator was eliminated from the scenario. Carried on
/// [`Event::InvestigatorEliminated`] so consumers (campaign log,
/// after-elimination triggers) know the cause without re-reading state.
///
/// Three of the four causes are defeats; [`Resigned`](Self::Resigned) is not
/// (`glossary/Resign.md`: *"An investigator who resigns is not considered to
/// have been defeated"*). `glossary/Elimination.md` is what unifies them —
/// *"A player is eliminated from a scenario any time his or her investigator
/// is defeated, or if he or she resigns"* — and the procedure it then
/// gives runs identically for all four.
///
/// [`Event::InvestigatorEliminated`]: crate::event::Event::InvestigatorEliminated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EliminationCause {
    /// Damage reached `max_health`. Flips status to [`Status::Defeated`].
    Damage,
    /// Horror reached `max_sanity`. Flips status to [`Status::Defeated`].
    Horror,
    /// Investigator resigned, via an ability printing the
    /// [`Resign`](card_dsl::dsl::ActionDesignator::Resign) action designator.
    /// Flips status to [`Status::Resigned`]. **Not a defeat** — see the enum's
    /// own note.
    Resigned,
    /// A card ability defeated the investigator outright, with no damage
    /// or horror threshold involved — `glossary/Defeat.md`: *"An
    /// investigator might also be defeated by a card ability."* Flips
    /// status to [`Status::Defeated`].
    CardAbility,
}

// `Skills` and `SkillKind` moved down to `card-dsl`
// (`card_dsl::card_data`) — pure data shared across the engine-corpus
// boundary. Re-exported at `game_core::state::{Skills, SkillKind}` (see
// `state/mod.rs`).

#[cfg(test)]
mod threat_area_tests {
    use super::*;
    use crate::state::Owner;
    use crate::test_support;

    #[test]
    fn new_investigator_has_empty_threat_area() {
        let inv = test_support::test_investigator(1);
        assert!(inv.threat_area.is_empty());
    }

    #[test]
    fn omitting_any_required_field_is_rejected() {
        // Every field is required on the wire (#453): a payload missing one
        // fails loudly rather than silently defaulting. `investigator_card`
        // in particular — an absent identity field must not degrade to some
        // default that silently disables elder-sign + seated reaction.
        let inv = test_support::test_investigator(1);
        let full = serde_json::to_value(&inv).expect("serialize");
        // The complete object still round-trips.
        serde_json::from_value::<Investigator>(full.clone()).expect("full object deserializes");
        // Each formerly-`#[serde(default)]` field is now mandatory.
        for field in [
            "investigator_card",
            "threat_area",
            "removed_from_game",
            "action_surcharge_spent_this_round",
            "additional_actions_this_turn",
            "grants_spent_this_turn",
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
    fn controlled_card_instances_yields_investigator_card_then_in_play_then_threat_area() {
        // #448 cp3a: the investigator card is yielded first (so the unified
        // scan fires its abilities), then `cards_in_play`, then `threat_area`.
        let mut inv = test_support::test_investigator(1);
        inv.investigator_card.code = CardCode::new("inv-card");
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new("in-play"),
            CardInstanceId(1),
            Owner::Investigator(InvestigatorId(1)),
        ));
        inv.threat_area.push(CardInPlay::enter_play(
            CardCode::new("threat"),
            CardInstanceId(2),
            Owner::EncounterDeck,
        ));
        let codes: Vec<&str> = inv
            .controlled_card_instances()
            .map(|c| c.code.as_str())
            .collect();
        assert_eq!(codes, vec!["inv-card", "in-play", "threat"]);
    }
}

#[cfg(test)]
mod ability_usage_tests {
    use card_dsl::dsl::{UsageLimit, UsagePeriod};

    use crate::state::{AbilityUsageRecord, InvestigatorId, Phase, UsageClock};
    use crate::test_support;

    /// The investigator triggering the ability in these tests.
    const ME: InvestigatorId = InvestigatorId(1);

    /// The clock at `round` and `phase`, outside any investigator's turn.
    fn at(round: u32, phase: Phase) -> UsageClock {
        UsageClock::new(round, phase, None)
    }

    #[test]
    fn new_investigator_card_has_empty_ability_usage() {
        let inv = test_support::test_investigator(1);
        assert!(inv.investigator_card.ability_usage.is_empty());
    }

    #[test]
    fn usage_exhausts_after_limit_within_a_round_and_resets_across_rounds() {
        let mut inv = test_support::test_investigator(1);
        let limit = Some(UsageLimit {
            count: 1,
            period: UsagePeriod::Round,
        });
        let card = &mut inv.investigator_card;
        // Ability 0, round 5: not yet fired → not exhausted.
        assert!(!card.is_usage_exhausted(0, ME, limit, at(5, Phase::Investigation)));
        // Fire once in round 5 → now exhausted in round 5.
        card.bump_ability_usage(0, ME, UsagePeriod::Round, at(5, Phase::Investigation));
        assert!(card.is_usage_exhausted(0, ME, limit, at(5, Phase::Investigation)));
        assert_eq!(
            card.ability_usage.get(&0),
            Some(&std::collections::BTreeMap::from([(
                ME,
                AbilityUsageRecord::new(at(5, Phase::Investigation), 1)
            )]))
        );
        // A later phase of the same round is the same round: still exhausted.
        assert!(card.is_usage_exhausted(0, ME, limit, at(5, Phase::Enemy)));
        // Round 6: lazy reset → not exhausted (stored record is stale).
        assert!(!card.is_usage_exhausted(0, ME, limit, at(6, Phase::Investigation)));
        // No limit (None) is never exhausted.
        assert!(!card.is_usage_exhausted(0, ME, None, at(5, Phase::Investigation)));
    }

    /// A round limit with a count above 1 keeps counting across a phase change
    /// within the round: bumping in a later phase must not reset it.
    #[test]
    fn a_round_limit_does_not_reset_at_a_phase_change() {
        let mut inv = test_support::test_investigator(1);
        let limit = Some(UsageLimit {
            count: 2,
            period: UsagePeriod::Round,
        });
        let card = &mut inv.investigator_card;
        card.bump_ability_usage(0, ME, UsagePeriod::Round, at(3, Phase::Investigation));
        card.bump_ability_usage(0, ME, UsagePeriod::Round, at(3, Phase::Enemy));
        assert!(card.is_usage_exhausted(0, ME, limit, at(3, Phase::Upkeep)));
        assert_eq!(card.ability_usage[&0][&ME].count, 2);
    }

    #[test]
    fn a_phase_limit_resets_at_a_phase_change_and_at_a_round_change() {
        let mut inv = test_support::test_investigator(1);
        let limit = Some(UsageLimit {
            count: 1,
            period: UsagePeriod::Phase,
        });
        let card = &mut inv.investigator_card;
        card.bump_ability_usage(0, ME, UsagePeriod::Phase, at(3, Phase::Investigation));
        assert!(card.is_usage_exhausted(0, ME, limit, at(3, Phase::Investigation)));
        // A later phase of the same round is a new period.
        assert!(!card.is_usage_exhausted(0, ME, limit, at(3, Phase::Enemy)));
        // So is the same phase of the next round.
        assert!(!card.is_usage_exhausted(0, ME, limit, at(4, Phase::Investigation)));
        // Firing in the new phase restarts the count there.
        card.bump_ability_usage(0, ME, UsagePeriod::Phase, at(3, Phase::Enemy));
        assert_eq!(
            card.ability_usage.get(&0),
            Some(&std::collections::BTreeMap::from([(
                ME,
                AbilityUsageRecord::new(at(3, Phase::Enemy), 1)
            )]))
        );
    }

    /// A turn limit resets when another investigator's turn begins, within the
    /// same Investigation phase.
    #[test]
    fn a_turn_limit_resets_at_the_next_investigators_turn() {
        let mut inv = test_support::test_investigator(1);
        let limit = Some(UsageLimit {
            count: 1,
            period: UsagePeriod::Turn,
        });
        let turn = |id| UsageClock::new(3, Phase::Investigation, Some(InvestigatorId(id)));
        let card = &mut inv.investigator_card;
        card.bump_ability_usage(0, ME, UsagePeriod::Turn, turn(1));
        assert!(card.is_usage_exhausted(0, ME, limit, turn(1)));
        assert!(!card.is_usage_exhausted(0, ME, limit, turn(2)));
    }
}

#[cfg(test)]
mod removed_from_game_tests {
    use crate::test_support;

    #[test]
    fn new_investigator_has_empty_removed_pile() {
        let inv = test_support::test_investigator(1);
        assert!(inv.removed_from_game.is_empty());
    }
}

#[cfg(test)]
mod harm_accessor_tests {
    use crate::test_support;

    // Seam test from cp1b (which delegated to legacy fields) is removed by
    // cp2a: the accessors now read the investigator_card directly.
    #[test]
    fn harm_accessors_read_from_investigator_card() {
        let mut inv = test_support::test_investigator(1);
        // Set accumulated harm directly on the card.
        inv.investigator_card.accumulated_damage = 2;
        inv.investigator_card.accumulated_horror = 1;
        assert_eq!(inv.damage(), 2);
        assert_eq!(inv.horror(), 1);
        // Capacity reads from the registry (TEST_INV has 8/8).
        assert_eq!(inv.max_health(&test_support::TEST_REGISTRY), 8);
        assert_eq!(inv.max_sanity(&test_support::TEST_REGISTRY), 8);
    }
}

#[cfg(test)]
mod investigator_card_tests {
    use crate::test_support;

    #[test]
    fn test_investigator_has_an_investigator_card_with_the_synthetic_code() {
        let inv = test_support::test_investigator(1);
        assert_eq!(inv.investigator_card.code.as_str(), test_support::TEST_INV);
        assert_eq!(inv.investigator_card.accumulated_damage, 0);
        assert_eq!(inv.investigator_card.accumulated_horror, 0);
    }

    // `identity_is_read_from_the_investigator_card` removed: it was a
    // duplicate of `test_investigator_has_an_investigator_card_with_the_synthetic_code`
    // (cp4 cleanup per task-7-brief).

    #[test]
    fn defeat_reads_capacity_from_the_card_not_a_field() {
        let mut inv = test_support::test_investigator(1);
        inv.investigator_card.accumulated_damage = 8; // == TEST_INV health
        assert!(
            inv.damage() >= inv.max_health(&test_support::TEST_REGISTRY),
            "defeat threshold reached via accessors"
        );
    }
}
