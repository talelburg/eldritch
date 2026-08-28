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

use crate::dsl::Determination;
use crate::scenario::ScenarioEnding;
use crate::state::{
    CardCode, CardInstanceId, ChaosToken, DefeatCause, EnemyId, InvestigatorId, LocationId, Phase,
    SkillKind, TokenResolution, UseKind, Zone,
};

/// One state-change record emitted by the engine.
///
/// Phase-1 minimal set. Later phases add events for skill-test
/// commits, card plays, ability triggers, encounter draws, doom changes,
/// trauma, scenario resolution, etc.
/// Which trauma track a [`Event::TraumaSuffered`] applies to. Trauma is a
/// cross-scenario campaign concept (Phase 9 owns persistence / max-stat
/// reduction); this event makes it observable now without modeling state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraumaKind {
    /// Physical trauma (reduces max health in campaign play).
    Physical,
    /// Mental trauma (reduces max sanity in campaign play).
    Mental,
}

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
    /// An investigator suffered trauma. Emitted by Cover Up 01007's
    /// game-end Forced ability. Observable + replay-visible; persistence
    /// (campaign log, max-stat reduction) is Phase 9 — no state mutation.
    TraumaSuffered {
        /// Who suffered the trauma.
        investigator: InvestigatorId,
        /// Physical or mental.
        kind: TraumaKind,
        /// How many trauma.
        amount: u8,
    },
    /// An investigator gained resources.
    ResourcesGained {
        /// Who received resources.
        investigator: InvestigatorId,
        /// Amount gained.
        amount: u8,
    },
    /// An investigator paid / lost resources (a `Cost::Resources` payment for an
    /// activated ability, or a card's printed play cost on `PlayCard`).
    ResourcesPaid {
        /// Who paid resources.
        investigator: InvestigatorId,
        /// Amount paid.
        amount: u8,
    },
    /// Tokens were spent from a source asset's runtime uses-pool to pay a
    /// `Cost::SpendUses` (".38 Special": "Spend 1 ammo").
    UsesSpent {
        /// Investigator who activated the ability.
        investigator: InvestigatorId,
        /// The source asset instance whose uses were spent.
        instance_id: CardInstanceId,
        /// Which uses-kind was spent.
        kind: UseKind,
        /// How many were spent.
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
    /// A skill test's [`Determination`] was latched by a card effect: the
    /// test now automatically fails, or automatically succeeds, whatever
    /// the numbers say.
    ///
    /// **Narrative, not adjudication.** The determination itself lives in
    /// the recorded-modifier population and is read through the test-level
    /// query (ADR 0007); nothing in the engine branches on this event. It
    /// exists because the event log is the client's only narrative channel,
    /// and an automatic success otherwise renders as a win with a dash
    /// where the chaos token would be and no stated cause.
    ///
    /// Not emitted for the determination an `[auto_fail]` chaos token
    /// latches: that cause is already on the log as the
    /// [`ChaosTokenRevealed`](Self::ChaosTokenRevealed) carrying
    /// [`TokenResolution::AutoFail`], and a second event for the same
    /// moment would read as two determinations.
    SkillTestDeterminationLatched {
        /// The investigator taking the determined test.
        investigator: InvestigatorId,
        /// Which determination was latched.
        determination: Determination,
        /// The in-play card instance whose ability latched it, when there
        /// is one. `None` for an effect with no in-play source — an event
        /// card resolving out of hand, or a card's commit-time effect —
        /// mirroring the recorded row's own
        /// [`source`](crate::state::RecordedModifier::source). Widening
        /// that attribution is the row's problem, not this event's.
        source: Option<CardInstanceId>,
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
    /// difficulty, or the test was determined to fail automatically.
    ///
    /// Note: per the Rules Reference, the investigator's total is
    /// clamped to a minimum of 0 before the margin is computed (a
    /// negative `skill + modifier` is treated as 0). An automatic failure
    /// substitutes the same total = 0 regardless of skill or modifier.
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
        /// [`GameState::enemy_ids`](crate::state::GameState::enemy_ids)).
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
    /// Defeat takes the enemy out of *play*, but not out of the *game*:
    /// its card is placed, eventlessly, in the pile the Rules Reference
    /// glossary entry "Defeat" names — "the encounter discard pile (or on
    /// its owner's discard pile if it is a weakness)", or the victory
    /// display instead for a Victory enemy. Consumers wanting the
    /// destination read state (`encounter_discard` / the owner's
    /// `discard` / `victory_display`) rather than the event stream.
    ///
    /// Defeat does NOT emit a paired [`EnemyDisengaged`] for an enemy
    /// that was engaged at the time of defeat. Engagement implicitly
    /// terminates because the enemy has left play. Consumers tracking
    /// engagement via the event stream should treat `EnemyDefeated` as
    /// terminating any engagement the enemy had.
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
    /// A card was found by a deck search and moved to an investigator's hand
    /// ([`Effect::SearchDeck`](crate::dsl::Effect::SearchDeck): Old Book of
    /// Lore 01031, Research Librarian 01032). Distinct from
    /// [`CardsDrawn`](Self::CardsDrawn) — a search is not a "draw" (no on-draw
    /// triggers key off it), and it names the specific card.
    CardSearchedToHand {
        /// The investigator who searched and now holds the card.
        investigator: InvestigatorId,
        /// The card moved to hand.
        code: CardCode,
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
    /// A weakness card was set aside from an investigator's opening hand
    /// during scenario setup (Rules Reference step 8: "Each weakness card
    /// drawn during this step is ignored, set aside (without resolving
    /// it), and replaced by drawing another card from the deck.").
    ///
    /// Emitted by `replace_opening_hand_weaknesses` (in
    /// `engine::dispatch::cards`) for each weakness pulled from the hand
    /// before drawing a replacement. The weakness moves to
    /// `investigator.setaside`; once the mulligan loop drains it is
    /// shuffled back into the owner's deck.
    WeaknessSetAside {
        /// The investigator whose opening hand contained the weakness.
        investigator: InvestigatorId,
        /// The weakness card code that was set aside.
        code: CardCode,
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
    /// An encounter card entered an investigator's threat area
    /// (persistent treachery / weakness). Mirror of the in-play entry
    /// path for player cards; the discard mirror reuses
    /// [`CardDiscarded`](Event::CardDiscarded) with
    /// `from: Zone::ThreatArea`.
    CardEnteredThreatArea {
        /// The investigator whose threat area the card entered.
        investigator: InvestigatorId,
        /// The card code that entered.
        code: CardCode,
        /// The minted in-play instance id.
        instance_id: CardInstanceId,
    },
    /// An encounter card was attached to a location (Obscuring Fog
    /// 01168's Revelation). The location-attachment mirror of
    /// [`CardEnteredThreatArea`](Event::CardEnteredThreatArea); the
    /// discard mirror reuses [`CardDiscarded`](Event::CardDiscarded) with
    /// `from: Zone::LocationAttachment`.
    CardAttachedToLocation {
        /// The location the card attached to.
        location: LocationId,
        /// The card code that attached.
        code: CardCode,
        /// The minted in-play instance id.
        instance_id: CardInstanceId,
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
    /// `amount` of `kind` (damage or horror) was healed from `investigator`
    /// (First Aid 01019, Medical Texts 01035). The inverse of the damage/horror
    /// events; emitted only when something was actually healed.
    Healed {
        /// The investigator healed.
        investigator: InvestigatorId,
        /// Which track was healed.
        kind: crate::dsl::HarmKind,
        /// How much was actually healed (≤ the amount requested).
        amount: u8,
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
        /// What carried the ability — a card instance in play, a location, or
        /// an enemy (#707, #708). Not every source has a
        /// [`CardInstanceId`]; a consumer that
        /// needs one reads
        /// [`AbilitySource::instance`](crate::state::AbilitySource::instance).
        source: crate::state::AbilitySource,
        /// The source card's code.
        code: CardCode,
        /// Which ability on the card fired.
        ability_index: u8,
    },
    /// A scenario ended — at a resolution point, or at none. Emitted by
    /// [`apply`](crate::engine::apply) when the scenario's *ending* finishes —
    /// not when the resolution latches. A dispatch site latches at a discrete
    /// trigger (act/agenda resolution point, or last-investigator elimination),
    /// which arms a
    /// [`ScenarioEnd`](crate::state::Continuation::ScenarioEnd) frame; the
    /// engine then cancels the opportunities and framework steps still on the
    /// continuation stack, runs the game-end Forced abilities (Cover Up 01007's
    /// mental trauma), and only then pushes this event. So it can land on a
    /// *later* apply than the latch — the interactive acknowledge of a game-end
    /// forced ability spans an apply boundary (#566).
    ///
    /// Followed immediately by any events the scenario's `apply_resolution`
    /// pushes — XP / trauma changes will appear after this event once
    /// Phase 9 lands real bodies.
    ///
    /// Fire-once: the latch is first-writer-wins and pushes the `ScenarioEnd`
    /// frame with it, and the apply boundary pops that frame as it finalizes —
    /// so this event fires exactly once per scenario.
    ScenarioResolved {
        /// How the scenario ended. Carries no win/loss verdict: that is a
        /// standalone-mode projection for the client to compute, not
        /// engine state.
        ending: ScenarioEnding,
    },
    /// A card was placed in the victory display (Rules Reference p.21).
    /// Emitted for each victory-point location at scenario resolution.
    EnteredVictoryDisplay {
        /// The placed card's printed code.
        code: CardCode,
        /// Its corpus victory value.
        victory: u8,
    },

    /// The agenda deck advanced: the agenda at `from` met its doom
    /// threshold and the next agenda became current. Doom was reset to
    /// 0. Not emitted when a *terminal* agenda is reached — that fires
    /// [`ScenarioResolved`] instead.
    ///
    /// [`ScenarioResolved`]: Self::ScenarioResolved
    AgendaAdvanced {
        /// The `agenda_index` of the agenda that advanced (before the
        /// cursor moved).
        from: usize,
    },
    /// The act deck advanced: the investigators spent the act at `from`'s
    /// clue threshold and the next act became current. Not emitted when
    /// a *terminal* act is reached — that fires [`ScenarioResolved`].
    ///
    /// [`ScenarioResolved`]: Self::ScenarioResolved
    ActAdvanced {
        /// The `act_index` of the act that advanced (before the cursor
        /// moved).
        from: usize,
    },
    /// An option the engine had already offered in an open reaction window was
    /// withdrawn: a sibling candidate resolved first and left it unable to
    /// initiate (Rules Reference, Triggered Abilities — *"A triggered ability
    /// can only be initiated if its effect has the potential to change the game
    /// state, and its cost (if any) has the potential to be paid in full, taking
    /// active cost modifiers into account."*). Emitted once per withdrawn
    /// option, immediately before the window re-prompts without it (#568).
    ///
    /// Purely explanatory: the withdrawal itself is visible as a shorter option
    /// list, and this event is what lets a client say *why* the option went away
    /// rather than having it silently vanish.
    ReactionOptionLapsed {
        /// The withdrawn option's controller — who was being offered it.
        investigator: InvestigatorId,
        /// The withdrawn option's card code.
        code: CardCode,
        /// Best-effort attribution of what stopped it from initiating.
        reason: LapseReason,
    },
    /// A location was revealed (turned face-up) on first investigator
    /// entry; `clues` were placed on it.
    LocationRevealed {
        /// The revealed location.
        location: LocationId,
        /// Clues placed at reveal, from the location's clue value. These
        /// are *added* to any clues already on the location, so this is
        /// not necessarily the location's resulting clue count.
        clues: u8,
    },
}

/// Why a skill test failed.
///
/// Both variants produce a `by` margin on the bracketing
/// [`SkillTestFailed`](Event::SkillTestFailed) event; this enum names
/// the *cause*, for **display attribution** rather than adjudication. No
/// Core + Dunwich card keys off a failure being automatic rather than
/// ordinary, and nothing in the engine branches on it (ADR 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FailureReason {
    /// The investigator's clamped total fell short of the difficulty.
    Total,
    /// The test carried a [`Determination::AutomaticFailure`], so the
    /// investigator's total skill value was considered 0 whatever their
    /// skills and modifiers said.
    ///
    /// Deliberately says nothing about *what* determined it: an
    /// `[auto_fail]` chaos token is one source, but a card latching an
    /// [`Effect::AutoResolve`](crate::dsl::Effect::AutoResolve) produces the
    /// same reason with no token drawn at all.
    AutoFail,
}

/// Why an already-offered reaction option could no longer be initiated
/// ([`Event::ReactionOptionLapsed`], #568).
///
/// **Attribution, not adjudication.** The decision to withdraw the option is
/// made by re-running the reaction scan and seeing that it no longer produces
/// the candidate; these variants are a second, cheaper pass over the withdrawn
/// candidate alone, run only to label the event. A mislabel is a cosmetic bug in
/// the client log, never a rules error — which is why the residual case
/// ([`NoLongerEligible`](Self::NoLongerEligible)) can be reported without
/// naming a specific gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LapseReason {
    /// The card is no longer where the scan found it — played out of hand by a
    /// sibling option, discarded, left play, or (for an act/agenda reaction) no
    /// longer the current act/agenda.
    SourceGone,
    /// The play cost can no longer be paid in full (Rules Reference p.22): a
    /// sibling option spent the resources. The wallet is shared, so two copies
    /// of a 1-cost Fast event are both offered on 1 resource and only the first
    /// can be played.
    CostUnpayable,
    /// The effect no longer has the potential to change the game state (Rules
    /// Reference, Triggered Abilities) — a sibling option consumed what it would
    /// have acted on, e.g. took the location's last clue before a second
    /// "discover 1 clue at your location" could resolve.
    NoStateChange,
    /// The triggering condition the window belongs to was **prevented** from
    /// resolving by a sibling `when`-cell ability, so the rest of its sequence
    /// is suppressed (#714 — Dodge 01023's ruling, quoted at the read site,
    /// `engine::dispatch::coordinator::prevented_in_the_when_cell`). Unlike the
    /// three probes above, this candidate is not withdrawn by a re-scan — it is
    /// still perfectly initiable; the condition it references is simply no
    /// longer happening.
    ConditionPrevented,
    /// The residual: the candidate no longer survives the scan, but none of the
    /// three named probes explains it. Reachable through the window's own
    /// scoping (a before-attack window admits only reactors co-located with the
    /// attacked investigator, so a reaction that moved someone can withdraw a
    /// sibling) or a per-round usage limit reached in the meantime.
    NoLongerEligible,
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

#[cfg(test)]
mod trauma_event_tests {
    use super::*;

    #[test]
    fn trauma_suffered_round_trips() {
        let e = Event::TraumaSuffered {
            investigator: InvestigatorId(1),
            kind: TraumaKind::Mental,
            amount: 1,
        };
        let json = serde_json::to_string(&e).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);
    }
}
