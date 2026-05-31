//! Events: state-change records emitted as actions resolve.
//!
//! When the engine applies an [`Action`], it produces a sequence of
//! [`Event`] values describing what changed. Events flow back to clients
//! over the websocket (clients update their local view by replaying
//! them) and are the substrate that triggered card abilities listen to.
//!
//! Events are NOT the source of truth for state — that's the action log.
//! Events are derived from action application and are useful as a
//! denormalized "what happened" stream.
//!
//! [`Action`]: crate::Action

use serde::{Deserialize, Serialize};

use card_dsl::card_data::CardType;

use crate::scenario::Resolution;
use crate::state::{
    CardCode, CardInstanceId, ChaosToken, DefeatCause, EnemyId, InvestigatorId, LocationId, Phase,
    SkillKind, TokenResolution, WindowKind, Zone,
};

/// One state-change record emitted by the engine.
///
/// Phase-1 minimal set. Later phases add events for skill-test
/// commits, card plays, ability triggers, encounter draws, doom changes,
/// trauma, scenario resolution, etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    /// A scenario session has begun.
    ScenarioStarted,
    /// A new phase began.
    PhaseStarted {
        /// The phase that just started.
        phase: Phase,
    },
    /// A phase ended.
    PhaseEnded {
        /// The phase that just ended.
        phase: Phase,
    },
    /// An investigator's turn ended (Investigation phase).
    TurnEnded {
        /// Whose turn it was.
        investigator: InvestigatorId,
    },
    /// An investigator's action point count changed.
    ActionsRemainingChanged {
        /// Whose action count changed.
        investigator: InvestigatorId,
        /// New count.
        new_count: u8,
    },
    /// An investigator moved between locations.
    InvestigatorMoved {
        /// Who moved.
        investigator: InvestigatorId,
        /// Origin location.
        from: LocationId,
        /// Destination location.
        to: LocationId,
    },
    /// A chaos token was revealed during a skill test.
    ChaosTokenRevealed {
        /// The token revealed.
        token: ChaosToken,
        /// How the token resolves against the scenario's modifier table:
        /// a numeric modifier, [`AutoFail`], or [`ElderSign`].
        ///
        /// [`AutoFail`]: TokenResolution::AutoFail
        /// [`ElderSign`]: TokenResolution::ElderSign
        resolution: TokenResolution,
    },
    /// One or more clues moved to an investigator.
    CluePlaced {
        /// Who received the clues.
        investigator: InvestigatorId,
        /// Number of clues placed.
        count: u8,
    },
    /// A location's clue count changed.
    LocationCluesChanged {
        /// The location.
        location: LocationId,
        /// New clue count.
        new_count: u8,
    },
    /// An investigator suffered physical damage.
    DamageTaken {
        /// Who was damaged.
        investigator: InvestigatorId,
        /// Amount of damage.
        amount: u8,
    },
    /// An investigator suffered horror.
    HorrorTaken {
        /// Who took horror.
        investigator: InvestigatorId,
        /// Amount of horror.
        amount: u8,
    },
    /// An investigator gained resources.
    ResourcesGained {
        /// Who received resources.
        investigator: InvestigatorId,
        /// Amount gained.
        amount: u8,
    },
    /// An investigator paid / lost resources (e.g. as a `Cost::Resources`
    /// payment for an activated ability).
    ResourcesPaid {
        /// Who paid resources.
        investigator: InvestigatorId,
        /// Amount paid.
        amount: u8,
    },
    /// A skill test was declared and resolution has begun.
    SkillTestStarted {
        /// Investigator taking the test.
        investigator: InvestigatorId,
        /// Skill the test is against.
        skill: SkillKind,
        /// Difficulty: total to meet or exceed for success.
        difficulty: i8,
    },
    /// A skill test succeeded. The investigator's total met or
    /// exceeded the difficulty.
    SkillTestSucceeded {
        /// Investigator who passed the test.
        investigator: InvestigatorId,
        /// Skill the test was against.
        skill: SkillKind,
        /// `total - difficulty`. Always `>= 0` for a success.
        margin: i8,
    },
    /// A skill test failed. Either the total fell short of the
    /// difficulty, or an `AutoFail` chaos token forced the total to 0.
    ///
    /// Note: per the Rules Reference, the investigator's total is
    /// clamped to a minimum of 0 before the margin is computed (a
    /// negative `skill + modifier` is treated as 0). `AutoFail` short-
    /// circuits to the same total = 0 regardless of skill or modifier.
    /// In both cases `by` reflects the clamped margin.
    SkillTestFailed {
        /// Investigator who failed the test.
        investigator: InvestigatorId,
        /// Skill the test was against.
        skill: SkillKind,
        /// Why the test failed.
        reason: FailureReason,
        /// Amount the test failed by (`difficulty - clamped_total`,
        /// always `>= 0`). Effects keying on "if you fail by X+" read
        /// this directly.
        by: i8,
    },
    /// The skill-test resolution sequence finished. Cleanup events
    /// (committed-card discards, etc.) precede this; downstream
    /// listeners use it as a "test is fully over" signal.
    SkillTestEnded {
        /// Investigator the test was for.
        investigator: InvestigatorId,
    },
    /// An enemy entered play at a location from the encounter deck.
    ///
    /// Emitted by `spawn_enemy` (in `engine::dispatch`) when an
    /// encounter card resolved as an enemy lands in
    /// [`GameState::enemies`](crate::state::GameState::enemies).
    /// `engaged_with` is `Some(investigator)` when the spawn caused
    /// engagement-on-spawn (Rules Reference p.10) and `None` when the
    /// enemy spawned at an empty location.
    ///
    /// When `engaged_with == Some(_)`, the spawn handler also emits
    /// [`EnemyEngaged`](Self::EnemyEngaged) immediately after this
    /// event, so listeners that key off engagement transitions see
    /// the on-spawn engagement uniformly with mid-game engagements.
    EnemySpawned {
        /// The newly-spawned enemy's stable id (freshly minted from
        /// [`GameState::next_enemy_id`](crate::state::GameState::next_enemy_id)).
        enemy: EnemyId,
        /// Printed code of the spawned enemy.
        code: CardCode,
        /// Where the enemy spawned on the location map.
        location: LocationId,
        /// If the spawn engaged an investigator on arrival, who.
        /// `None` if the enemy spawned at a location with no
        /// investigators.
        engaged_with: Option<InvestigatorId>,
    },
    /// An enemy became engaged with an investigator.
    EnemyEngaged {
        /// The engaged enemy.
        enemy: EnemyId,
        /// The investigator the enemy is now engaged with.
        investigator: InvestigatorId,
    },
    /// A hunter enemy moved one location during Enemy-phase step 3.2
    /// (Rules Reference p.12). Engagement on arrival, if any, emits a
    /// paired [`EnemyEngaged`](Self::EnemyEngaged) immediately after.
    EnemyMoved {
        /// The enemy that moved.
        enemy: EnemyId,
        /// Destination location.
        to: LocationId,
    },
    /// An enemy disengaged from an investigator (e.g. via a
    /// successful Evade).
    EnemyDisengaged {
        /// The enemy that disengaged.
        enemy: EnemyId,
        /// The investigator it was previously engaged with.
        investigator: InvestigatorId,
    },
    /// An enemy was exhausted (e.g. via a successful Evade or after
    /// attacking).
    EnemyExhausted {
        /// The enemy that exhausted.
        enemy: EnemyId,
    },
    /// An enemy was readied (e.g. during the Upkeep phase).
    EnemyReadied {
        /// The enemy that readied.
        enemy: EnemyId,
    },
    /// An enemy took damage.
    EnemyDamaged {
        /// The damaged enemy.
        enemy: EnemyId,
        /// Amount of damage applied.
        amount: u8,
        /// The enemy's new accumulated damage after the application.
        new_damage: u8,
    },
    /// An enemy was defeated (damage reached `max_health` or a card
    /// effect explicitly defeated it). The enemy is removed from
    /// `GameState::enemies` after this event fires.
    ///
    /// Per the Rules Reference, defeat takes the enemy out of play
    /// entirely — it does NOT emit a paired [`EnemyDisengaged`] for
    /// an enemy that was engaged at the time of defeat. Engagement
    /// implicitly terminates because the enemy is gone. Consumers
    /// tracking engagement via the event stream should treat
    /// `EnemyDefeated` as terminating any engagement the enemy had.
    ///
    /// [`EnemyDisengaged`]: Event::EnemyDisengaged
    EnemyDefeated {
        /// The defeated enemy.
        enemy: EnemyId,
        /// Who defeated it, if attributable. `None` for non-
        /// investigator-attributed defeats (e.g. effects that just
        /// say "defeat this enemy").
        by: Option<InvestigatorId>,
    },
    /// An investigator was defeated. The investigator's
    /// [`Status`](crate::state::Status) has been flipped from
    /// `Active` to `Killed` / `Insane` (or `Resigned` once the
    /// Resign action lands). The investigator entry stays in
    /// `state.investigators` so consumers can still identify them by
    /// id; they just can't take actions or be targeted as "active."
    InvestigatorDefeated {
        /// The defeated investigator.
        investigator: InvestigatorId,
        /// What caused the defeat.
        cause: DefeatCause,
    },
    /// An investigator's player deck was shuffled. State inspection
    /// has the new order; this event is the announcement.
    DeckShuffled {
        /// Whose deck was shuffled.
        investigator: InvestigatorId,
    },
    /// A shuffle of the shared encounter deck occurred. Emitted by
    /// `shuffle_encounter_deck` (in `engine::dispatch`) iff the deck
    /// had ≥ 2 cards (a 0- or 1-card shuffle is a no-op and emits
    /// nothing). Has no payload — the encounter deck is shared, so
    /// no investigator ID is needed.
    EncounterDeckShuffled,
    /// An investigator drew `count` cards from their player deck. The
    /// cards have already been moved from deck to hand by the time
    /// this event fires; the specific card codes are not in the event
    /// payload (state inspection has the post-draw hand). Cards are
    /// drawn from the deck front, i.e. top.
    CardsDrawn {
        /// The investigator who drew.
        investigator: InvestigatorId,
        /// How many cards were drawn.
        count: u8,
    },
    /// An investigator completed a mulligan. `redrawn_count` is the
    /// number of cards swapped (0 if the player kept their hand).
    /// State inspection has the new hand contents.
    MulliganPerformed {
        /// Who mulliganed.
        investigator: InvestigatorId,
        /// How many cards were redrawn.
        redrawn_count: u8,
    },
    /// Every investigator in `state.investigators` is now non-Active.
    /// Fires immediately after the [`InvestigatorDefeated`] that
    /// flipped the last active investigator. Scenario-resolution
    /// flow (#74) consumes this when it lands; for now, downstream
    /// listeners can use it as a "scenario lost" trigger.
    ///
    /// [`InvestigatorDefeated`]: Event::InvestigatorDefeated
    AllInvestigatorsDefeated,
    /// An investigator played a card from their hand. Fires before
    /// any `Trigger::OnPlay` effects resolve (the play *causes* the
    /// effects), and before the card lands in its destination zone.
    /// State inspection has the post-play hand / `cards_in_play` /
    /// discard contents.
    CardPlayed {
        /// Who played the card.
        investigator: InvestigatorId,
        /// The card code that was played.
        code: CardCode,
    },
    /// A card was discarded — moved from `from` to the investigator's
    /// discard pile. Fires for played events after their on-play
    /// effects resolve; future card effects ("discard a card from
    /// your hand", "discard top of deck") emit this with the
    /// matching `from` zone.
    CardDiscarded {
        /// The card's controller.
        investigator: InvestigatorId,
        /// The discarded card code.
        code: CardCode,
        /// Where the card came from before landing in discard.
        from: Zone,
    },
    /// An in-play card was exhausted (turned 90°). Fires as part of
    /// activation cost payment when a card's
    /// [`Cost::Exhaust`](crate::dsl::Cost::Exhaust) resolves, and
    /// from future ready/exhaust effects.
    CardExhausted {
        /// The card's controller.
        investigator: InvestigatorId,
        /// The exhausted in-play instance.
        instance_id: CardInstanceId,
        /// The card code (for log readability; redundant with state).
        code: CardCode,
    },
    /// An investigator's in-play card was readied (flipped from
    /// exhausted to ready) — e.g. during Upkeep step 4.3. Mirror of
    /// [`Event::CardExhausted`]. Enemies readying emit
    /// [`Event::EnemyReadied`] instead.
    CardReadied {
        /// The card's controller.
        investigator: InvestigatorId,
        /// The readied in-play instance.
        instance_id: CardInstanceId,
        /// The card code (for log readability; redundant with state).
        code: CardCode,
    },
    /// An encounter card was revealed from the encounter deck. Fires
    /// before any [`Trigger::Revelation`](card_dsl::dsl::Trigger::Revelation)
    /// effects on the card resolve — the card has been drawn off the
    /// deck and identified, but its Revelation effect has not yet
    /// applied. Before-timing reaction listeners (#52's machinery, not
    /// wired in Phase 4) hook this point to interpose or cancel.
    ///
    /// Emitted by `encounter_card_revealed` (in `engine::dispatch`) in
    /// response to `EngineRecord::EncounterCardRevealed`.
    CardRevealed {
        /// The investigator whose draw produced this reveal. For
        /// Phase-4 Mythos draws, this is the investigator taking their
        /// Mythos turn; for forced reveals (scenario effects), the
        /// scenario module names the controller.
        investigator: InvestigatorId,
        /// The revealed card's code.
        code: CardCode,
        /// The card's type, as resolved by the card registry at reveal
        /// time. Redundant with the metadata lookup but baked into the
        /// event so consumers don't need the registry to filter.
        card_type: CardType,
    },
    /// An activated ability resolved its costs and is about to apply
    /// its effect. Fires after every cost-payment event and before
    /// the ability's own effect events. Downstream reactions that
    /// key on "after an ability is activated" use this as their
    /// trigger point.
    AbilityActivated {
        /// Who activated the ability.
        investigator: InvestigatorId,
        /// The in-play source's instance.
        instance_id: CardInstanceId,
        /// The source card's code.
        code: CardCode,
        /// Which ability on the card fired.
        ability_index: u8,
    },
    /// A reaction window opened. Carries the [`WindowKind`] discriminant
    /// so listeners and replay tools know what kind of triggers can
    /// fire inside the window. Pairs with a later
    /// [`WindowClosed`](Self::WindowClosed) of the same kind.
    ///
    /// **Mid-action timing.** Per the Rules Reference's "after…"
    /// clause, this event fires immediately after the triggering
    /// condition's impact resolves — *inside* the surrounding action
    /// handler, before the action's other resolution steps continue.
    /// For a Fight that defeats an enemy, that means:
    /// `EnemyDefeated → WindowOpened → [reaction effects] →
    /// WindowClosed → OnSkillTestResolution → CardDiscarded →
    /// SkillTestEnded`.
    WindowOpened {
        /// The kind of window that opened.
        kind: WindowKind,
    },
    /// A reaction window closed — every forced trigger inside it has
    /// fired and the player has chosen to fire or skip each optional
    /// one. Pairs with the preceding [`WindowOpened`](Self::WindowOpened)
    /// of the same kind.
    ///
    /// A window with **no** matching triggers is not opened at all —
    /// no [`WindowOpened`]/`WindowClosed` pair fires for it. Listeners
    /// can treat the presence of a `WindowOpened` event as proof that
    /// at least one trigger was offered to the player; the matching
    /// `WindowClosed` follows once the player resolves the window.
    ///
    /// After this fires, the surrounding action's driver resumes
    /// (e.g. the skill-test driver advances from
    /// [`FinishContinuation::PostFollowUp`](crate::state::FinishContinuation::PostFollowUp)
    /// to the `OnSkillTestResolution` step).
    ///
    /// [`WindowOpened`]: Self::WindowOpened
    WindowClosed {
        /// The kind of window that closed.
        kind: WindowKind,
    },
    /// A scenario resolved (won or lost). Emitted by
    /// [`apply`](crate::engine::apply) after a `Done` outcome when the
    /// active scenario module's `detect_resolution` returns `Some`.
    /// Followed immediately by any events the scenario's
    /// `apply_resolution` pushes — XP / trauma changes will appear
    /// after this event once Phase 9 lands real bodies.
    ///
    /// This event is **terminal-ish** for the scenario, but the
    /// Phase-4 engine does not latch on it: a scenario whose
    /// `detect_resolution` keeps returning `Some` will keep re-emitting
    /// `ScenarioResolved` on each subsequent apply. Phase 9 will add
    /// the idempotency guard alongside the first non-trivial
    /// `apply_resolution` — tracked as #131.
    ScenarioResolved {
        /// The resolution returned by the scenario module.
        resolution: Resolution,
    },
}

/// Why a skill test failed.
///
/// Both variants produce a `by` margin on the bracketing
/// [`SkillTestFailed`](Event::SkillTestFailed) event; this enum names
/// the *cause*, which some card effects key off independently of the
/// numeric margin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FailureReason {
    /// The investigator's clamped total fell short of the difficulty.
    Total,
    /// An `AutoFail` chaos token forced the total to 0, regardless of
    /// skill value or other modifiers.
    AutoFail,
}

#[cfg(test)]
mod window_opened_event_tests {
    use super::*;
    use crate::state::{Phase, WindowKind};

    #[test]
    fn window_opened_serde_roundtrip() {
        let ev = Event::WindowOpened {
            kind: WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }
}

#[cfg(test)]
mod enemy_spawned_event_tests {
    use super::*;
    use crate::state::{CardCode, EnemyId, InvestigatorId, LocationId};

    #[test]
    fn enemy_spawned_with_engagement_serde_roundtrip() {
        let ev = Event::EnemySpawned {
            enemy: EnemyId(7),
            code: CardCode("_synth_enemy".into()),
            location: LocationId(10),
            engaged_with: Some(InvestigatorId(1)),
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }

    #[test]
    fn enemy_spawned_without_engagement_serde_roundtrip() {
        let ev = Event::EnemySpawned {
            enemy: EnemyId(8),
            code: CardCode("_synth_enemy".into()),
            location: LocationId(10),
            engaged_with: None,
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }
}

#[cfg(test)]
mod encounter_deck_event_tests {
    use super::*;

    #[test]
    fn encounter_deck_shuffled_serde_roundtrip() {
        let ev = Event::EncounterDeckShuffled;
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }
}

#[cfg(test)]
mod card_revealed_event_tests {
    use super::*;
    use crate::state::CardCode;
    use card_dsl::card_data::CardType;

    #[test]
    fn card_revealed_event_serde_roundtrip() {
        let ev = Event::CardRevealed {
            investigator: InvestigatorId(1),
            code: CardCode("_synth_treachery".into()),
            card_type: CardType::Treachery,
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }
}
