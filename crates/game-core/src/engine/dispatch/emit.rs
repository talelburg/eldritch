//! The unified trigger-dispatch chokepoint (umbrella §2 / Axis-B T5a).
//!
//! [`queue_event`] is the single entry point for forced + reaction trigger
//! dispatch at a framework/game timing point. A [`TimingEvent`] names the
//! timing point and carries its binding context; `queue_event` pushes the
//! [coordinator](super::coordinator) that walks the condition's
//! `when → resolve → at → after` sequence, and each cell of that walk runs the
//! two phases — Rules Reference p.2 forced-then-reaction:
//!
//! 1. **forced** — mandatory abilities are queued (via
//!    `queue_forced_triggers` for a lone hit, or the lead-ordered run for
//!    2+ simultaneous ones).
//! 2. **reaction** — the optional player reaction window opens.
//!
//! **Queues; does not resolve.** Since the effect-frame migration (#423),
//! firing an ability means *pushing a frame*: nothing here is evaluated
//! synchronously, so a returned [`EngineOutcome::Done`] means **queued**, not
//! **resolved**. A caller with work to do after the emit must put that work on
//! a resumption frame and emit in tail position; checking the returned outcome
//! is not a substitute. See
//! `docs/adr/0003-emitting-a-timing-point-queues-abilities.md` — four call
//! sites read `Done` as "nothing happened" and ran their tails *above* the
//! abilities they had just queued (#569).
//!
//! Which abilities a cell holds is the one trigger scan's answer
//! ([`trigger_scan`](super::trigger_scan)): a walk over the whole board, the same
//! for every condition (ADR 0018). `queue_event` does **not** push the logged
//! [`Event`](crate::event::Event) — call sites still emit their own (e.g.
//! `EnemyDefeated`, `InvestigatorMoved`).

use card_dsl::dsl::{SkillTestKind, TestOutcome, TriggeringCondition};
use serde::{Deserialize, Serialize};

use crate::engine::dispatch::{actions, combat, encounter, skill_test};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{evaluator, Cx};
use crate::state::{
    Assignment, CardCode, CardInstanceId, DamageSource, EmitEventFrame, EmitStep, EnemyId,
    InvestigatorId, LocationId, Phase, RevealedToken,
};

/// A game/framework timing point at which forced and/or reaction triggers
/// may fire, with the binding context the fired effects need.
///
/// Each variant maps, through exhaustive matches, to the triggering condition
/// its patterns listen to (`condition`), the investigator it is about
/// (`subject`), and who resolves the condition itself (`condition_resolution`).
/// Which zones hold its abilities is **not** tabulated: every condition walks
/// the whole board, and whether a cell holds a forced or reaction ability is the
/// coordinator's per-cell scan's answer (#702, ADR 0018).
///
/// `SkillTestResolved` is the general skill-test-outcome timing point (RR
/// ST.6), of which "after you successfully investigate" (Obscuring Fog forced +
/// Dr. Milan reaction) is the `{ Investigate, Success }` narrowing. Routing the
/// forced and reaction phases through one `queue_event` keeps RR p.2
/// forced-before-reaction. Framework `PlayerWindow(PhaseStep)` windows are *not*
/// timing events — they have no `EventPattern` and stay on explicit
/// `open_fast_window` calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingEvent {
    /// An investigator entered a location (forced only).
    EnteredLocation {
        investigator: InvestigatorId,
        location: LocationId,
    },
    /// A phase began, `Appendix_II_Timing_and_Gameplay.md` steps 1.1 / 2.1 /
    /// 3.1 / 4.1 (forced only). A **bare milestone**: the phase's own opening
    /// work belongs to the driver frame that emitted this, and runs on its
    /// resume once the whole sequence has.
    PhaseStarted { phase: Phase },
    /// A phase ended (forced only).
    PhaseEnded { phase: Phase },
    /// An act advanced — its reverse resolves (forced only).
    ActAdvanced { code: CardCode },
    /// An agenda advanced — its reverse resolves (forced only).
    AgendaAdvanced { code: CardCode },
    /// An enemy was defeated. **Dual:** forced (act objectives keyed to the
    /// defeat) + the after-defeat reaction window (Roland 01001, Evidence!).
    EnemyDefeated {
        enemy: EnemyId,
        by: Option<InvestigatorId>,
        code: CardCode,
    },
    /// The round ended, step 4.6 (forced only).
    RoundEnded,
    /// An investigator's turn ended, step 2.2.2 (forced only).
    EndOfTurn { investigator: InvestigatorId },
    /// The game ended — a scenario resolution latched (forced only).
    GameEnd,
    /// The game has ended **for one eliminated investigator**, for the purpose
    /// of resolving weakness cards — Rules Reference p.10 Elimination step 0,
    /// quoted in full on [`Continuation::Elimination`](crate::state::Continuation::Elimination)
    /// (forced only, #638).
    ///
    /// Same card-facing pattern as [`GameEnd`](Self::GameEnd) (a weakness prints
    /// *"when the game ends"*, not two different triggers) but a different scan:
    /// one investigator, weaknesses only, and no `Status::Active` filter — the
    /// investigator being eliminated has already been flipped off `Active` when
    /// this fires.
    EliminationGameEnd {
        /// The investigator being eliminated.
        investigator: InvestigatorId,
    },
    /// Damage and/or horror has been **assigned** — Rules Reference step 1 of
    /// dealing damage, `glossary/Dealing_Damage_Horror.md`: *"Determine the
    /// amount of damage and/or horror being dealt. Place damage and/or horror
    /// tokens equal to the amount of damage and horror being dealt **next to**
    /// the cards that will be taking the damage/horror."* Nothing is on any card
    /// yet, so this is a **bare milestone** (ADR 0008): its resolve step is a
    /// documented no-op and the mutation the whole procedure has is
    /// [`DamagePlaced`](Self::DamagePlaced)'s.
    ///
    /// Its `when` cell is the window the Rules Reference names between the two
    /// steps — *"Abilities that prevent, reduce, or reassign damage and/or
    /// horror that is being dealt are resolved between steps 1 and 2."* — and is
    /// the cell Guard Dog 01021 prints: *"\[reaction\] **When** an enemy attack
    /// deals damage to Guard Dog: Deal 1 damage to the attacking enemy."*
    ///
    /// Reaction only. See `docs/adr/0009-damage-is-assigned-then-placed.md`.
    DamageAssigned {
        /// What is dealing the harm — the scoping Guard Dog's *"an enemy
        /// attack"* narrows on, and what binds the attacker its retaliate names.
        source: DamageSource,
        /// The investigator the harm is being dealt to.
        investigator: InvestigatorId,
        /// The assignment as this step left it: a snapshot of the live value the
        /// [`DealDamage`](crate::state::Continuation::DealDamage) frame owns.
        assignment: Assignment,
    },
    /// Assigned damage and/or horror is **placed** — Rules Reference step 2:
    /// *"Any assigned damage/horror that has not been prevented is now placed on
    /// each card to which it has been assigned, simultaneously."*
    ///
    /// **Coordinator-owned** with a real impact (ADR 0008): its resolve step is
    /// `place_assignment` — the simultaneous placement and the defeat
    /// determination that follows it (RR p.7). So a `when` ability here
    /// interrupts the placement with the assignment already settled (Baron
    /// Samedi 05019's *"Forced - When any amount of damage is placed on an
    /// investigator in Baron Samedi's location: Place 1 additional damage on
    /// that investigator."*), while an `at` or `after` one sees the tokens on
    /// the cards (Mark Harrigan 03001's *"After damage is placed on a card you
    /// control"*).
    ///
    /// Neither of those cards is in the Core or Dunwich corpus. The card-facing
    /// pattern is [`EventPattern::HarmPlaced`](card_dsl::dsl::EventPattern::HarmPlaced),
    /// whose first consumer is Agnes Baker 01004's *"After 1 or more horror is
    /// placed on Agnes Baker"* in the `after` cell. See
    /// `docs/adr/0009-damage-is-assigned-then-placed.md`.
    DamagePlaced {
        /// What dealt the harm.
        source: DamageSource,
        /// The investigator the harm is being dealt to.
        investigator: InvestigatorId,
        /// The assignment being placed — re-read from the frame after
        /// [`DamageAssigned`](Self::DamageAssigned)'s cells, so it is what
        /// actually lands.
        assignment: Assignment,
    },
    /// A skill test resolved (RR ST.6). **Dual:** forced + reaction. The
    /// general timing point of which "after you successfully investigate"
    /// (Obscuring Fog 01168 forced + Dr. Milan 01033 reaction) is the
    /// `{ Investigate, Success }` narrowing. Carries no location: the matcher
    /// reads the investigated location, for Obscuring Fog's *"attached
    /// location"*, from the still-live in-flight `SkillTest` frame
    /// (`current_skill_test().tested_location`) — teardown is at
    /// `PostOnResolution`, well after this fires. Both phases fire at one
    /// timing point, RR p.2 forced-before-reaction.
    SkillTestResolved {
        investigator: InvestigatorId,
        kind: SkillTestKind,
        outcome: TestOutcome,
    },
    /// An enemy attacks an investigator (RR p.25 step 3.3) — one triggering
    /// condition in all three cells, **coordinator-owned** (#704).
    ///
    /// The `when` cell is the cancel/replacement window (Dodge 01023); the
    /// coordinator deals the attack at its resolve step; the `at` and `after`
    /// cells see the damage and horror already landed (Silver Twilight Acolyte
    /// 01102's *"**Forced** - After Silver Twilight Acolyte attacks: Place 1
    /// doom on the current agenda."*). There is no separate would-attack
    /// condition: a card declares [`EventPattern::EnemyAttacks`] plus the trigger
    /// word it prints.
    ///
    /// [`EventPattern::EnemyAttacks`]: card_dsl::dsl::EventPattern::EnemyAttacks
    EnemyAttacks {
        /// The attacking enemy.
        enemy: EnemyId,
        /// The investigator being attacked.
        investigator: InvestigatorId,
    },
    /// An investigator discovers clues (reaction only, all three cells).
    /// **Coordinator-owned** (#703): the emitting `discover_clue` only caps the
    /// count and emits, and the coordinator moves the clues at its resolve step,
    /// between the `when` and `at` cells. So a `when` ability interrupts the
    /// discovery before the clues move — Cover Up 01007's replacement — while an
    /// `at` or `after` one sees them already moved.
    DiscoverClues {
        investigator: InvestigatorId,
        location: LocationId,
        /// Clues that will actually be discovered — already capped at the
        /// location's clue count by the emitting `discover_clue`, so a
        /// replacement effect's "discard that many" (Cover Up) reads the real
        /// quantity, not the requested one (#471).
        count: u8,
    },
    /// A card entered play (After). Opens the `AfterEnteredPlay` window —
    /// Research Librarian 01032's tutor (a reaction) and Arcane Initiate
    /// 01063's doom (a forced).
    EnteredPlay {
        /// The card instance that entered play (self-binding scope).
        instance: CardInstanceId,
        /// The investigator who controls it.
        controller: InvestigatorId,
    },
    /// An investigator left a location — Barricade 01038's self-discard, heard
    /// on the left location's attachments.
    ///
    /// Coordinator-owned since #721: the departure itself resolves at
    /// `resolve_left_location`, so a `when` ability sees the investigator
    /// still standing at `location`, while an `at` or `after` one sees them
    /// already at `destination`.
    LeftLocation {
        /// The investigator who left.
        investigator: InvestigatorId,
        /// The location they left.
        location: LocationId,
        /// The location they are moving to. Carried because the departure's own
        /// impact — the engaged-enemy drag and the location assignment — is
        /// resolved from this event's value at the coordinator's resolve step,
        /// where `move::perform`'s locals are long out of scope.
        destination: LocationId,
    },
    /// An investigator's play of a card has **completed** — announced once
    /// `Appendix_I_Initiation_Sequence.md` step 4 has: *"The card is regarded
    /// as played (and placed in play, or in its owner's discard pile if it's an
    /// event), and the ability is considered resolved simultaneously with the
    /// completion of this step."* So an event is already in the discard pile
    /// and an asset already in play. The 01012 ruling puts Heirloom of
    /// Hyperborea's *"After you play a \[\[Spell\]\] card"* here: *"This trigger
    /// occurs after the Spell card has resolved in full."*
    ///
    /// Not the logged [`Event::CardPlayed`](crate::event::Event::CardPlayed),
    /// which records step 3, the card *commencing* being played. A **bare
    /// milestone** (ADR 0008): the placement precedes it, so its resolve step
    /// is a no-op.
    ///
    /// Announced from both play paths, the play action and a Fast event played
    /// from a reaction window, because both dispose of the card through
    /// `dispose_play_from_hand`. A played card something else already placed
    /// (Barricade 01038 attaching itself) is not announced.
    CardPlayed {
        /// The investigator who played the card.
        investigator: InvestigatorId,
        /// The played card, which a [`CardFilter`](card_dsl::dsl::CardFilter)
        /// narrows on.
        code: CardCode,
    },
    /// An investigator **drew** an encounter card — framework step 1.4
    /// (`Appendix_II_Timing_and_Gameplay.md`), a card effect's draw (Drawn to
    /// the Flame 01064), or a scenario's (Rise of the Ghouls 01106's *"The lead
    /// investigator draws that enemy."*). Every one of them resolves the card
    /// through `encounter::resolve_encounter_card`, which announces this.
    ///
    /// **Coordinator-owned**, born so (#1056): its resolve step is the drawn
    /// card's revelation, step 3 of 1.4's *"3. Resolve the revelation ability
    /// on the drawn card."* So a `when` ability resolves before the revelation —
    /// `glossary/When.md`'s *"When you draw an enemy card" initiates
    /// immediately after you draw the enemy card, but before resolving its
    /// revelation ability, spawning it, etc."* — which is where Ward of
    /// Protection 01065's *"Cancel that card's revelation effect"* has to sit.
    /// A cancel abandons the revelation and nothing else: the card's disposal
    /// (step 4, the discard or the spawn) is on the `EncounterCard` frame
    /// beneath this one, and still runs — `glossary/Cancel.md`: *"If the
    /// effects of a treachery card are canceled, the card is still regarded as
    /// having been drawn, and it is still placed in the encounter discard
    /// pile."*
    ///
    /// The `after` cell therefore runs *before* the disposal, where
    /// `glossary/After.md` would put "after you draw" after *"resolving its
    /// revelation ability, spawning it, etc."* An interpretation with no
    /// consumer: no Core or Dunwich card prints an `after`-draw-encounter
    /// trigger.
    EncounterCardDrawn {
        /// The investigator who drew the card.
        investigator: InvestigatorId,
        /// The drawn card, which a [`CardFilter`](card_dsl::dsl::CardFilter)
        /// narrows on.
        code: CardCode,
    },
    /// An investigator **evaded** an enemy — announced by the one evade exit,
    /// `evade::evade_enemy`, which the Evade action's and a designated
    /// **Evade**'s success reach, and an automatic evade will (#780).
    ///
    /// **Coordinator-owned**, born so (#1075): its resolve step is the
    /// evasion's impact, `glossary/Evade_Action.md`'s *"the enemy is exhausted
    /// (if it was ready) and the engagement is broken."* So an `after` ability
    /// sees the enemy already evaded — Pickpocketing 01046's *"\[reaction\]
    /// After you evade an enemy, exhaust Pickpocketing: Draw 1 card."* — and a
    /// `when` one would resolve before it. No Core or Dunwich card listens in
    /// the `when` cell.
    EnemyEvaded {
        /// The investigator who evaded.
        investigator: InvestigatorId,
        /// The evaded enemy.
        enemy: EnemyId,
    },
    /// An investigator's turn began — Appendix II step 2.2, *"Next
    /// investigator's turn begins."* Announced once per turn by
    /// `phases::begin_investigator_turn`, after the active investigator has
    /// rotated to `investigator`.
    ///
    /// A **bare milestone** (ADR 0008): making the investigator active is the
    /// rotation's, done before this is announced, so its resolve step is a
    /// no-op. Its `after` cell is the first opening of the post-2.2 player
    /// window, where On the Lam 01010's *"Play after your turn begins."* is
    /// played; the window step 2.2.1 returns to after each action is not this
    /// cell, so the card is not playable there. The post-2.2 Fast window opens
    /// once this sequence has resolved.
    TurnBegan {
        /// The investigator whose turn began.
        investigator: InvestigatorId,
    },
    /// An investigator performing a skill test revealed a chaos token — Skill
    /// Test Timing step 3, announced by the test's draw step once the token is
    /// out and logged.
    ///
    /// **Coordinator-owned**, born so (#1106): its resolve step is what the
    /// token does to the test — its modifier row or automatic failure, the
    /// frame's revealed token, step 4's chaos symbol effect and the initiating
    /// card's reveal clause. So a `when` ability that cancels the token leaves
    /// the test as if it had never been revealed — Wendy Adams 01005's
    /// *"\[reaction\] When you reveal a chaos token, choose and discard 1 card
    /// from your hand: Cancel that chaos token and return it to the bag. Reveal
    /// a new chaos token."* — and an `after` one sees it resolved.
    ChaosTokenRevealed {
        /// The investigator performing the test.
        investigator: InvestigatorId,
        /// The token drawn, the token it counts as, and the resolution
        /// announced — everything the resolve step needs, carried here rather
        /// than parked on the test's frame before the `when` cell can cancel it.
        token: RevealedToken,
    },
}

impl TimingEvent {
    /// The investigator this condition is **about** — who entered, left, was
    /// attacked, tested, discovered, was dealt harm, ended their turn, was
    /// eliminated, or defeated the enemy. `None` for the board-wide conditions:
    /// phase boundaries, advances, the round's and the game's end, and an
    /// enemy defeat no investigator is credited with.
    ///
    /// The forced-binding rule's third arm (ADR 0018): a forced ability on an
    /// uncontrolled card other than the act or agenda — a location, an enemy,
    /// an encounter attachment — is bound to the subject, falling back to the
    /// lead proxy when there is none. An **exhaustive** match, so a new timing
    /// event cannot compile without saying who it is about.
    pub(crate) fn subject(&self) -> Option<InvestigatorId> {
        match self {
            TimingEvent::EnteredLocation { investigator, .. }
            | TimingEvent::EndOfTurn { investigator }
            | TimingEvent::EliminationGameEnd { investigator }
            | TimingEvent::DamageAssigned { investigator, .. }
            | TimingEvent::DamagePlaced { investigator, .. }
            | TimingEvent::SkillTestResolved { investigator, .. }
            | TimingEvent::EnemyAttacks { investigator, .. }
            | TimingEvent::DiscoverClues { investigator, .. }
            | TimingEvent::LeftLocation { investigator, .. }
            | TimingEvent::CardPlayed { investigator, .. }
            | TimingEvent::EncounterCardDrawn { investigator, .. }
            | TimingEvent::EnemyEvaded { investigator, .. }
            | TimingEvent::TurnBegan { investigator }
            | TimingEvent::ChaosTokenRevealed { investigator, .. } => Some(*investigator),
            TimingEvent::EnteredPlay { controller, .. } => Some(*controller),
            TimingEvent::EnemyDefeated { by, .. } => *by,
            TimingEvent::PhaseStarted { .. }
            | TimingEvent::PhaseEnded { .. }
            | TimingEvent::ActAdvanced { .. }
            | TimingEvent::AgendaAdvanced { .. }
            | TimingEvent::RoundEnded
            | TimingEvent::GameEnd => None,
        }
    }

    /// The triggering condition this event announces: the engine's side of the
    /// pattern ↔ condition pairing whose card side is
    /// [`EventPattern::condition`](card_dsl::dsl::EventPattern::condition). An
    /// **exhaustive** match, so a new timing event cannot compile without
    /// naming the condition whose patterns it matches. The reaction matcher
    /// refuses a pattern of another condition before it reads any narrowing.
    pub(crate) fn condition(&self) -> TriggeringCondition {
        match self {
            TimingEvent::EnteredLocation { .. } => TriggeringCondition::EnteredLocation,
            TimingEvent::PhaseStarted { .. } => TriggeringCondition::PhaseStarted,
            TimingEvent::PhaseEnded { .. } => TriggeringCondition::PhaseEnded,
            TimingEvent::ActAdvanced { .. } => TriggeringCondition::ActAdvanced,
            TimingEvent::AgendaAdvanced { .. } => TriggeringCondition::AgendaAdvanced,
            TimingEvent::EnemyDefeated { .. } => TriggeringCondition::EnemyDefeated,
            TimingEvent::RoundEnded => TriggeringCondition::RoundEnded,
            TimingEvent::EndOfTurn { .. } => TriggeringCondition::EndOfTurn,
            // One card-facing condition with two scans: a weakness prints one
            // *"when the game ends"*, which elimination step 0 also triggers.
            TimingEvent::GameEnd | TimingEvent::EliminationGameEnd { .. } => {
                TriggeringCondition::GameEnd
            }
            TimingEvent::DamageAssigned { .. } => TriggeringCondition::DamageAssigned,
            TimingEvent::DamagePlaced { .. } => TriggeringCondition::DamagePlaced,
            TimingEvent::SkillTestResolved { .. } => TriggeringCondition::SkillTestResolved,
            TimingEvent::EnemyAttacks { .. } => TriggeringCondition::EnemyAttacks,
            TimingEvent::DiscoverClues { .. } => TriggeringCondition::DiscoverClues,
            TimingEvent::EnteredPlay { .. } => TriggeringCondition::EnteredPlay,
            TimingEvent::LeftLocation { .. } => TriggeringCondition::LeftLocation,
            TimingEvent::CardPlayed { .. } => TriggeringCondition::CardPlayed,
            TimingEvent::EncounterCardDrawn { .. } => TriggeringCondition::EncounterCardDrawn,
            TimingEvent::EnemyEvaded { .. } => TriggeringCondition::EnemyEvaded,
            TimingEvent::TurnBegan { .. } => TriggeringCondition::TurnBegan,
            TimingEvent::ChaosTokenRevealed { .. } => TriggeringCondition::ChaosTokenRevealed,
        }
    }

    /// Who resolves this triggering condition's own impact — step 2 of the
    /// sequence in `glossary/Nested_Sequences.md`. An **exhaustive** match, so a
    /// new timing event cannot compile without choosing an arm; see
    /// [`ConditionResolution`] and
    /// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
    pub(crate) fn condition_resolution(&self) -> ConditionResolution {
        match self {
            // The **bare milestones**: nothing about the game state changes as
            // the condition itself resolves, so there is no impact for a caller
            // to have mutated ahead of the emit and none for the coordinator to
            // perform. Each owns a no-op resolve step and each `when` cell is
            // genuinely safe to walk. In all three the teardown that *looks*
            // like the impact runs after the whole sequence, on a frame the
            // `drive` loop re-exposes once the sequence has drained:
            //
            // - `RoundEnded` — Upkeep → Mythos runs on the Upkeep anchor's
            //   `AfterRoundEnd` resume, which is why act 01109 The Barrier's
            //   *"When the round ends"* objective has resolved in the `when`
            //   cell all along. "Until the end of the round" lasting effects
            //   expire just ahead of the emit, so no cell sees them (#643).
            // - `GameEnd` — the victory-display scan and `apply_resolution` run
            //   at the apply boundary, once the `ScenarioEnd` frame is re-exposed
            //   at `ScenarioEndStep::Finalize` (#720). The frame advances its own
            //   cursor before emitting, so the emit is in tail position and the
            //   ending finishes only after every ability it queued has drained —
            //   which is why Cover Up 01007's trauma, with its interactive
            //   acknowledge, can span `apply` calls.
            // - `EliminationGameEnd` — Rules Reference p.10 Elimination steps
            //   1–6 run on the re-exposed `Elimination` frame at
            //   `EliminationStep::RunSteps`, again after a tail-position emit.
            //   That includes the sentence that reads most like this condition's
            //   own impact, step 0's *"Then, remove those weaknesses from the
            //   game."* — the engine discharges it through step 1's threat-area
            //   partition, so it too runs after the whole sequence rather than
            //   between the cells. It migrates with `GameEnd` and for the same
            //   card: a weakness prints one *"when the game ends"* trigger, and
            //   Cover Up's `EventPattern::GameEnd` declaration is scanned by both
            //   points, so leaving this one caller-owned would reject the
            //   elimination that the retag was meant to serve.
            // - `DamageAssigned` — the assignment is a *proposal*, tokens
            //   sitting "next to the cards that will be taking the
            //   damage/horror" (`glossary/Dealing_Damage_Horror.md` step 1).
            //   Nothing is on any card, so there is nothing for this condition
            //   to resolve; the placement that looks like its impact is the next
            //   cursor step's own condition, `DamagePlaced`. It is the one
            //   member whose teardown is a second *condition* rather than a
            //   resume, which is what the named window between the two rules
            //   steps requires (ADR 0009).
            // - `PhaseStarted` — born coordinator-owned (#697), as
            //   `ConditionResolution::Caller` requires of a new event: there is
            //   no legacy emit site to invert. A phase beginning changes nothing
            //   by itself; the phase's opening *work* (the Mythos doom, the
            //   Enemy phase's hunter movement, each phase's first player window)
            //   belongs to the driver frame that emitted it and runs on that
            //   frame's resume, after the whole sequence. The one thing that
            //   does precede the emit is Mythos's round-counter bump — step 1.1
            //   *"also formalizes the beginning of a new game round"*
            //   (`Appendix_II_Timing_and_Gameplay.md`), so an ability firing at
            //   this milestone must already read the new round.
            // - `CardPlayed` — born coordinator-owned (#1048). The condition is
            //   the play's *completion*, not its placement: the 01012 ruling
            //   puts Heirloom of Hyperborea's *"After you play a [[Spell]]
            //   card"* *"after the Spell card has resolved in full"*, and
            //   Appendix I step 4 makes the card *"regarded as played"*
            //   simultaneously with the step's completion. A completion is a
            //   milestone like `PhaseStarted`, so the placement — the event to
            //   the discard pile, the asset into play — precedes the emit rather
            //   than being its resolve step. It could not be the resolve step
            //   anyway: between the emit and that step the card would sit in no
            //   zone, carried only by the event, where elimination's sweep could
            //   not find it (ADR 0002). Its `when` cell is walked and no corpus
            //   card listens there.
            // - `TurnBegan` — born coordinator-owned (#778), like
            //   `PhaseStarted`: a turn beginning changes nothing by itself. The
            //   rotation that makes the investigator active precedes the emit,
            //   and the post-2.2 player window opens on the Investigation
            //   anchor's resume, after the whole sequence. On the Lam 01010's
            //   *"Play after your turn begins."* is played in its `after` cell.
            TimingEvent::PhaseStarted { .. }
            | TimingEvent::TurnBegan { .. }
            | TimingEvent::RoundEnded
            | TimingEvent::GameEnd
            | TimingEvent::EliminationGameEnd { .. }
            | TimingEvent::DamageAssigned { .. }
            | TimingEvent::CardPlayed { .. } => {
                ConditionResolution::Coordinator(resolve_bare_milestone)
            }
            // Coordinator-owned for Lita Chantler 01117's *"[reaction] **When**
            // an investigator at your location successfully attacks a
            // [[Monster]] enemy: That investigator deals +1 damage."* — a
            // `when` whose point is to raise the damage before ST.7 applies it.
            // ST.7 itself belongs to the `SkillTest` frame, which advances its
            // cursor to `AcknowledgeOutcome` before emitting and applies the
            // results once the whole sequence drains.
            //
            // The resolve step is not a bare milestone. Lucky! 01080 prints
            // *"Fast. Play when you would fail a skill test. Get +2 to your
            // skill value for that test."*, so the verdict announced is
            // provisional until the `when` cell is done: the resolve step
            // re-totals, logs the `SkillTestSucceeded`/`SkillTestFailed`, and
            // voids the rest of the sequence when the verdict has flipped.
            TimingEvent::SkillTestResolved { .. } => {
                ConditionResolution::Coordinator(resolve_skill_test_resolved)
            }
            // The fourth migration (#727/#722), which cost a model rather than
            // an arm: `EnemyAttackDamagedSelf` could not be flipped to
            // coordinator-owned, because one condition emitted per damaged asset
            // *after* a single collapsed place-and-announce call was neither of
            // the Rules Reference's two steps. `DamagePlaced` is the second of
            // them, and its impact is the simultaneous placement plus the defeat
            // determination (RR p.7).
            TimingEvent::DamagePlaced { .. } => {
                ConditionResolution::Coordinator(resolve_damage_placed)
            }
            // The first migration (#703). Cover Up 01007 prints *"[reaction] When
            // you would discover 1 or more clues at your location: Discard that
            // many clues from Cover Up instead"*, so the discovery has to be able
            // to not happen — which it cannot be if the caller has already moved
            // the clues by the time the `when` cell runs.
            TimingEvent::DiscoverClues { .. } => {
                ConditionResolution::Coordinator(resolve_clue_discovery)
            }
            // The second migration (#704). Dodge 01023 prints *"Fast. Play when
            // an enemy attacks an investigator at your location. / Cancel that
            // attack."*, so the attack has to be able to not happen — which it
            // cannot be if the caller has already dealt the damage by the time
            // the `when` cell runs. The attack's impact is the damage and horror
            // it places, so that is what moves to the resolve step; the
            // attacker's *exhaust* does not (`data/arkhamdb-faq/core/01023.md`:
            // *"If an attack was cancelled during the Enemy phase, the
            // attacking enemy still exhausts."*), and lives on the parked
            // `AttackLoop` frame instead.
            TimingEvent::EnemyAttacks { .. } => {
                ConditionResolution::Coordinator(resolve_enemy_attack)
            }
            // The third migration (#721). Barricade 01038 prints *"**Forced** -
            // When an investigator leaves attached location: Discard
            // Barricade."*, so the discard has to resolve before the departure
            // lands — which it cannot if the caller has already moved the
            // investigator by the time the `when` cell runs. The departure's
            // impact is the engaged-enemy drag and the location assignment, so
            // that is what moves to the resolve step; the destination *reveal*
            // and the auto-engagement do not, being the arrival's business, and
            // stay on the `MoveEnter` frame parked beneath the emit (#569).
            TimingEvent::LeftLocation { .. } => {
                ConditionResolution::Coordinator(resolve_left_location)
            }
            // Born coordinator-owned (#1056). Ward of Protection 01065 prints
            // *"Fast. Play when you draw a non-weakness treachery card. / Cancel
            // that card's revelation effect."*, so the revelation has to be able
            // to not happen: it is the condition's resolve step.
            TimingEvent::EncounterCardDrawn { .. } => {
                ConditionResolution::Coordinator(resolve_encounter_draw)
            }
            // Born coordinator-owned (#1075). Evading is not a bare milestone:
            // it disengages and exhausts the enemy, so that is the resolve step,
            // and Pickpocketing 01046's after-evade reaction sees it done.
            TimingEvent::EnemyEvaded { .. } => {
                ConditionResolution::Coordinator(resolve_enemy_evaded)
            }
            // Born coordinator-owned (#1106). Wendy Adams 01005 cancels a
            // revealed chaos token in the `when` cell, so what the token does to
            // the test has to be able to not happen: it is the resolve step.
            TimingEvent::ChaosTokenRevealed { .. } => {
                ConditionResolution::Coordinator(resolve_chaos_token_revealed)
            }
            // Every other condition is still caller-owned: the emitting call site
            // mutates the board and *then* emits. Each flips to a coordinator-
            // owned arm when a card demands its `when` cell — what that costs,
            // and the terminal condition for the arm itself, is on
            // [`ConditionResolution::Caller`].
            TimingEvent::EnteredLocation { .. }
            | TimingEvent::PhaseEnded { .. }
            | TimingEvent::ActAdvanced { .. }
            | TimingEvent::AgendaAdvanced { .. }
            | TimingEvent::EnemyDefeated { .. }
            | TimingEvent::EndOfTurn { .. }
            | TimingEvent::EnteredPlay { .. } => ConditionResolution::Caller,
        }
    }
}

/// Who resolves a triggering condition's own impact — step 2 of the sequence in
/// `glossary/Nested_Sequences.md`, which the Rules Reference puts *inside* the
/// sequence: *"1) execute "when..." effects that interrupt that triggering
/// condition, (2) resolve the triggering condition, and then, (3) execute
/// "after..." effects in response to that triggering condition."*
///
/// Returned by `TimingEvent::condition_resolution`, an exhaustive match: a new
/// timing event cannot compile without a decision — the discipline ADR 0004
/// established for classifying continuation frames. Never stored on a frame; the
/// coordinator recomputes it from the event's own value at
/// [`EmitStep::ResolveCondition`](crate::state::EmitStep::ResolveCondition),
/// because frames are serialized and cannot hold a fn pointer.
///
/// See `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ConditionResolution {
    /// The **coordinator** resolves the condition, between the `when` and `at`
    /// cells. The `when` cell is walked: an interrupt there resolves before the
    /// condition's impact lands, which is what the card prints.
    Coordinator(ResolveConditionFn),
    /// The **emitting caller** mutates the board and then emits, so the
    /// condition has already resolved by the time the coordinator runs.
    ///
    /// The migration arm. Its `when` cell is *not* walked and an interrupt
    /// declared on it is rejected rather than dropped: the caller has already
    /// mutated, so resolving the interrupt here would resolve it *after* the
    /// thing it was meant to interrupt — worse than not resolving it at all.
    ///
    /// **Migrating one event** means: move the caller's mutation into a named
    /// function (the emit site keeps only the emit, in tail position per ADR
    /// 0003), point a `Coordinator` arm at it, and let the coordinator call it at
    /// the resolve step. The worked example is `resolve_clue_discovery` (#703):
    /// that mutation was already factored into `perform_discovery` for a resume
    /// frame to call, so the migration moved the call site and deleted the
    /// resume. `resolve_enemy_attack` (#704) is the dearer worked example: the
    /// mutation was tangled into a loop that read the stack after emitting, so
    /// migrating it meant parking that loop on its own frame first.
    ///
    /// **This arm exists to migrate existing callers, not to accommodate new
    /// ones.** A new timing event has no legacy emit site to invert, so it is
    /// born `Coordinator`. When the last member migrates, this arm — and the
    /// classification, and the reject — are deleted together.
    Caller,
}

/// A coordinator-owned condition's resolution step: what the condition itself
/// does to the game state, run between the `when` and `at` cells.
///
/// A plain fn pointer rather than a closure — the coordinator frame is part of
/// serialized game state, so the step is dispatched from the timing event's own
/// value on every visit instead of being captured when the frame is pushed.
pub(crate) type ResolveConditionFn = fn(&mut Cx, &TimingEvent) -> EngineOutcome;

/// The resolve step of a condition that is a **bare milestone** — a phase,
/// round, turn, step or game boundary whose occurrence changes nothing by
/// itself. Nothing to resolve, so nothing happens here; the cells around it are
/// the whole of the sequence.
///
/// A milestone is bare when the teardown that follows it belongs to the frame
/// that emitted it rather than to the condition: it runs after the whole
/// sequence, on a resume, not between the `when` and `at` cells. See the arm in
/// [`TimingEvent::condition_resolution`] for that argument on each of its
/// members, and
/// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
fn resolve_bare_milestone(_cx: &mut Cx, _event: &TimingEvent) -> EngineOutcome {
    EngineOutcome::Done
}

/// The resolve step of [`TimingEvent::DiscoverClues`]: move the clues (#703).
///
/// The impact of "you discover clues" is the clues arriving on the investigator,
/// so this is where they move — after the `when` cell's replacement effects have
/// had their chance to cancel it, and before the `at` and `after` cells see the
/// board. `count` is the count fixed at the moment of the would-be discovery;
/// `perform_discovery`'s own `min` is the shrinkage backstop for a `when`
/// reaction that removed clues in between (#471).
fn resolve_clue_discovery(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::DiscoverClues {
        investigator,
        location,
        count,
    } = event
    else {
        unreachable!("resolve_clue_discovery: not a DiscoverClues event: {event:?}");
    };
    evaluator::perform_discovery(cx, *location, *count, *investigator);
    EngineOutcome::Done
}

/// The resolve step of [`TimingEvent::EnemyAttacks`]: deal the attack (#704).
///
/// The impact of "an enemy attacks an investigator" is the damage and horror it
/// places, so that is what happens here — after the `when` cell's cancel window
/// (Dodge 01023) has had its chance to prevent it, and before the `at` and
/// `after` cells see the board. May suspend on the interactive soak
/// distribution (#44/K5b); the coordinator has already advanced its cursor to
/// `At`, so the resumed prompt lands back on the `at` cell rather than dealing
/// the attack twice.
///
/// **Not** here: the attacker's exhaust. `data/arkhamdb-faq/core/01023.md`,
/// verbatim — *"If an attack was cancelled during the Enemy phase, the attacking
/// enemy still exhausts."* — so exhausting cannot be part of the impact a cancel
/// prevents. It belongs to the enemy phase's own step and lives on the parked
/// [`AttackLoop`](crate::state::Continuation::AttackLoop) frame, which the
/// `drive` loop re-exposes once this whole sequence has run. That also matches
/// `Appendix_II_Timing_and_Gameplay.md` step 3.3: *"Upon completion of dealing
/// the attack (and all abilities triggered by the attack), exhaust the enemy."*
/// — which is after the `after` cell, not before it.
fn resolve_enemy_attack(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::EnemyAttacks {
        enemy,
        investigator,
    } = event
    else {
        unreachable!("resolve_enemy_attack: not an EnemyAttacks event: {event:?}");
    };
    combat::deal_enemy_attack(cx, *investigator, *enemy)
}

/// The resolve step of [`TimingEvent::DamagePlaced`]: the tokens land (#727).
///
/// `glossary/Dealing_Damage_Horror.md` step 2, verbatim: *"Any assigned
/// damage/horror that has not been prevented is now placed on each card to which
/// it has been assigned, simultaneously."* — so this is one call,
/// [`place_assignment`](super::combat::place_assignment), placing every share at
/// once and then determining defeat (RR p.7). The assignment comes off the
/// event, which the [`DealDamage`](crate::state::Continuation::DealDamage) frame
/// snapshotted from its live value *after*
/// [`DamageAssigned`](TimingEvent::DamageAssigned)'s cells had their chance at
/// it.
///
/// **Not** here: the determination of *how much* is being dealt and to whom.
/// That is step 1, and it has already happened on the frame's `Distribute` step
/// — which is the whole point of the split, since the Rules Reference puts a
/// named window between the two.
fn resolve_damage_placed(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::DamagePlaced {
        investigator,
        assignment,
        ..
    } = event
    else {
        unreachable!("resolve_damage_placed: not a DamagePlaced event: {event:?}");
    };
    combat::place_assignment(cx, *investigator, assignment);
    EngineOutcome::Done
}

/// The resolve step of [`TimingEvent::LeftLocation`]: the departure lands
/// (#721).
///
/// The impact of "an investigator leaves a location" is the investigator (and
/// the enemies engaged with them) arriving at the destination, so that is what
/// happens here — after the `when` cell has had its chance to interrupt, and
/// before the `at` and `after` cells see the board. Barricade 01038's
/// *"**Forced** - When an investigator leaves attached location: Discard
/// Barricade."* is the `when`-cell ability this exists for.
///
/// **Not** here: the destination reveal and the entered location's
/// auto-engagement. Both belong to the arrival, not the departure, and stay on
/// the [`MoveEnter`](crate::state::Continuation::MoveEnter) frame parked beneath
/// the emit — which is also what keeps #569's ordering, entering after leaving.
fn resolve_left_location(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::LeftLocation {
        investigator,
        location,
        destination,
    } = event
    else {
        unreachable!("resolve_left_location: not a LeftLocation event: {event:?}");
    };
    actions::r#move::resolve_departure(cx, *investigator, *location, *destination);
    EngineOutcome::Done
}

/// The resolve step of [`TimingEvent::EncounterCardDrawn`]: the drawn card's
/// revelation (#1056).
///
/// Framework step 1.4's *"3. Resolve the revelation ability on the drawn
/// card."*, after the `when` cell has had its chance to cancel it (Ward of
/// Protection 01065) and before the `at` and `after` cells. The revelation's
/// effects are pushed above the coordinator, whose cursor is already at `At`,
/// so one that suspends (a skill test, a choice) resumes into the `at` cell
/// rather than revealing twice.
///
/// **Not** here: the disposal. It is step 4, on the
/// [`EncounterCard`](crate::state::Continuation::EncounterCard) frame beneath
/// the coordinator, and a cancelled revelation still reaches it.
fn resolve_encounter_draw(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::EncounterCardDrawn { investigator, code } = event else {
        unreachable!("resolve_encounter_draw: not an EncounterCardDrawn event: {event:?}");
    };
    encounter::resolve_revelation(cx, *investigator, code);
    EngineOutcome::Done
}

/// The resolve step of [`TimingEvent::EnemyEvaded`]: the evasion lands
/// (#1075) — the enemy disengages and, if it was ready, exhausts
/// (`evade::resolve_evasion`). After the `when` cell, before the `at` and
/// `after` cells, so Pickpocketing 01046's *"After you evade an enemy"* sees it
/// done.
fn resolve_enemy_evaded(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::EnemyEvaded {
        investigator,
        enemy,
    } = event
    else {
        unreachable!("resolve_enemy_evaded: not an EnemyEvaded event: {event:?}");
    };
    actions::evade::resolve_evasion(cx, *investigator, *enemy);
    EngineOutcome::Done
}

/// The resolve step of [`TimingEvent::SkillTestResolved`]: re-total the test
/// and confirm or void the verdict the event announced (#1111,
/// `skill_test::resolve_verdict`). After the `when` cell, so a modifier played
/// there (Lucky! 01080) is counted; before the `at` and `after` cells, so a
/// verdict it flipped never reaches them.
fn resolve_skill_test_resolved(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::SkillTestResolved {
        investigator,
        outcome,
        ..
    } = event
    else {
        unreachable!("resolve_skill_test_resolved: not a SkillTestResolved event: {event:?}");
    };
    skill_test::resolve_verdict(cx, *investigator, *outcome)
}

/// The resolve step of [`TimingEvent::ChaosTokenRevealed`]: the token takes
/// effect on the test (#1106, `skill_test::resolve_reveal`) — its row, the
/// frame's revealed token, the ST.4 symbol effect and the reveal clause. After
/// the `when` cell, so a cancelled token (Wendy Adams 01005) never gets here.
/// The pushed effects sit above the coordinator, already at `At`, so one that
/// suspends (a soak) resumes into the `at` cell rather than resolving twice.
fn resolve_chaos_token_revealed(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::ChaosTokenRevealed {
        investigator,
        token,
    } = event
    else {
        unreachable!("resolve_chaos_token_revealed: not a ChaosTokenRevealed event: {event:?}");
    };
    skill_test::resolve_reveal(cx, *investigator, *token);
    EngineOutcome::Done
}

/// Dispatch a timing event: push its `when → resolve → at → after` coordinator
/// (#702). **Every** triggering condition goes through it — since #704 there is
/// no bypass and no per-condition table of any kind here.
///
/// # This queues; it does not resolve
///
/// Every path here pushes frames for the `drive` loop to own. So **`Done` means
/// queued, not resolved** — a caller that does synchronous work after this call
/// pushes that work *above* the abilities it just queued, and it runs *first*.
///
/// A caller with post-emit work therefore **emits in tail position and resumes
/// on its own frame**: arm the resume cursor (`enemy_phase_end` /
/// `upkeep_phase_end` re-park their phase anchor; `end_turn` arms
/// `InvestigatorTurn { ending: true }`), or push a dedicated frame beneath the
/// emit (`move::perform`'s [`MoveEnter`](crate::state::Continuation::MoveEnter)),
/// then return this outcome unexamined. See
/// `docs/adr/0003-emitting-a-timing-point-queues-abilities.md` (#569).
///
/// # The sequence
///
/// `glossary/Nested_Sequences.md` gives *every* triggering condition the same
/// three-part sequence, and `glossary/At.md` adds the middle cell — so there is
/// no per-event table of which cells an event supports and no single-cell fast
/// path — the last two bypasses, the enemy attack and its soak window, walk the
/// coordinator since #704. A cell is populated iff [the coordinator's per-cell
/// scan](super::coordinator::dispatch_emit_event) finds something in it, and an
/// empty cell is skipped without prompting. Forced-before-reaction within a cell
/// (RR p.2) is the [`TimingPoint`](crate::state::Continuation::TimingPoint)
/// frame's `sub` cursor.
///
/// Returns `Done`: the walk itself never suspends here, because pushing the
/// coordinator *is* the whole of the queueing. The suspensions it will produce —
/// a reaction window, or the lead's ordering pick for 2+ simultaneous forced
/// abilities (#213) — surface from the `drive` loop as it walks the cells, which
/// is why a call site's only obligation is to emit in tail position.
#[must_use = "queue_event only queues frames: post-emit work belongs on a resume \
              frame, not after this call (ADR 0003). Return the outcome, or bind \
              it to `_` at a site whose caller owns the suspension channel"]
pub(crate) fn queue_event(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    cx.state.continuations.push(EmitEventFrame {
        event: event.clone(),
        step: EmitStep::When,
        prevented: false,
    });
    EngineOutcome::Done
}
