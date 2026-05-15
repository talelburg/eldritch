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

use crate::state::{
    CardCode, CardInstanceId, ChaosToken, DefeatCause, EnemyId, InvestigatorId, LocationId, Phase,
    SkillKind, TokenResolution, Zone,
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
    /// An enemy entered play at a location. (Spawning logic lands
    /// with the encounter deck and Mythos phase; this event slot
    /// exists so card effects that react to spawns have something to
    /// listen for from day one.)
    EnemySpawned {
        /// The newly-spawned enemy.
        enemy: EnemyId,
        /// Where it spawned.
        location: LocationId,
    },
    /// An enemy became engaged with an investigator.
    EnemyEngaged {
        /// The engaged enemy.
        enemy: EnemyId,
        /// The investigator the enemy is now engaged with.
        investigator: InvestigatorId,
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
