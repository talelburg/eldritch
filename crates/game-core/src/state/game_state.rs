//! Top-level game state.

use std::collections::{BTreeMap, VecDeque};

use card_dsl::card_data::{CardKind, CardMetadata, SkillKind};
use card_dsl::dsl::{Determination, IntExpr, Stat};
use serde::{Deserialize, Serialize};

use crate::rng::RngState;
use crate::scenario::{ScenarioEnding, ScenarioId};
use crate::state::continuation::{
    Continuation, ContinuationStack, Frame, HandSizeDiscard, InFlightSkillTest,
};
use crate::state::continuation::{EncounterDrawFrame, MulliganFrame};
use crate::state::{
    CardCode, CardInstanceId, ChaosBag, Counter, Enemy, EnemyId, Investigator, InvestigatorId,
    Location, LocationId, Phase, TokenModifiers,
};

/// The full state of a scenario at a single point in time.
///
/// `GameState` is the world the engine mutates by applying actions.
/// In the event-sourced model, the canonical state is *derived* by
/// replaying the action log; `GameState` is the materialized cache.
///
/// Phase-1 minimal shape; later phases will add e.g. persistent
/// campaign-log facts and cross-scenario trauma tracking.
///
/// Investigators and locations are stored in [`BTreeMap`]s keyed by ID
/// rather than [`Vec`]s. This makes iteration order deterministic
/// (sorted by ID) regardless of insertion order — important for replay
/// equality — and gives O(log n) lookup. Turn order is tracked
/// separately in [`turn_order`](Self::turn_order); the storage map's
/// iteration order is *not* turn order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GameState {
    /// All investigators currently in the scenario, keyed by ID.
    pub investigators: BTreeMap<InvestigatorId, Investigator>,
    /// All locations laid out (revealed and unrevealed alike), keyed by ID.
    pub locations: BTreeMap<LocationId, Location>,
    /// Cards set aside, out of play (Rules Reference p.3, "set aside"),
    /// recorded by printed code only — one zone for every cardtype that
    /// can be set aside, because a card's stats are minted when it enters
    /// play, not at `setup()`. An enemy's per-investigator health depends
    /// on the live investigator count, and a location's [`LocationId`] is
    /// minted at entry, so nothing here can be pre-built.
    ///
    /// Brought into play by card effects, via
    /// [`put_set_aside_card_into_play`], which dispatches on the card's
    /// [`CardKind`]: The Gathering's Act-1 reverse puts the four house
    /// locations into play (the `01108:board-build` native effect) and its
    /// Act-2 reverse spawns the Ghoul Priest (01116) in the Hallway (the
    /// `01109:reverse` native effect).
    ///
    /// [`put_set_aside_card_into_play`]: crate::engine::put_set_aside_card_into_play
    pub set_aside_cards: Vec<CardCode>,
    /// Where roster-seated investigators are placed at scenario start.
    /// `setup()` sets it (e.g. The Gathering -> the Study); the
    /// scenario setup (via `seat_and_open`) reads it. `None` leaves seated
    /// investigators unplaced (`current_location: None`) — the legacy
    /// pre-seated test path, where `setup()` already placed them.
    pub starting_location: Option<LocationId>,
    /// All enemies currently in play, keyed by ID. Defeated enemies are
    /// removed; the map is the source of truth for "this enemy exists."
    pub enemies: BTreeMap<EnemyId, Enemy>,
    /// The chaos bag at this scenario's difficulty.
    pub chaos_bag: ChaosBag,
    /// Per-scenario numeric values for the four symbol tokens
    /// (Skull/Cultist/Tablet/ElderThing). Set at scenario setup,
    /// immutable for the scenario.
    pub token_modifiers: TokenModifiers,
    /// Current round phase.
    pub phase: Phase,
    /// 1-based round counter, incremented on each Mythos phase entry.
    pub round: u32,
    /// Whose turn it is during the [`Investigation`] phase, if any.
    /// `None` outside of Investigation.
    ///
    /// [`Investigation`]: Phase::Investigation
    pub active_investigator: Option<InvestigatorId>,
    /// Order in which investigators take their turns during the
    /// Investigation phase, as decided by the lead investigator each
    /// round. The first entry is the first to act.
    pub turn_order: Vec<InvestigatorId>,
    /// Deterministic RNG state. Carries `(seed, draws)` only; the
    /// underlying [`rand_chacha::ChaCha8Rng`] is reconstructed on
    /// demand by [`RngState`] methods.
    pub rng: RngState,
    // The setup mulligan loop now lives on its `Continuation::Mulligan`
    // frame (#348); read the prompted investigator via
    // [`Self::current_mulligan`]. The former `mulligan_pending:
    // Option<InvestigatorId>` cursor is removed — the continuation stack is the
    // single source of truth (mirroring the `in_flight_skill_test` fold).
    /// Allocator for [`CardInstanceId`]s, minted when cards enter play.
    /// Deterministic across replays; serializes as a bare `u32`.
    pub card_instance_ids: Counter<CardInstanceId>,
    /// Allocator for [`EnemyId`]s, minted when enemies enter play via the
    /// encounter deck (see `crate::engine::dispatch::spawn_enemy`).
    /// Independent of [`card_instance_ids`](Self::card_instance_ids) — the
    /// phantom-typed [`Counter`] mints only `EnemyId`s.
    pub enemy_ids: Counter<EnemyId>,
    /// Allocator for [`LocationId`]s, minted as scenarios build their board.
    pub location_ids: Counter<LocationId>,
    /// Allocator for [`SkillTestId`]s, minted at
    /// `engine::dispatch::start_skill_test`. Monotonic and never reused, so
    /// a [`RecordedModifier`] stamped with a test's id is inert once that
    /// test ends even if a later test occupies the same frame slot.
    pub skill_test_ids: Counter<SkillTestId>,
    /// The **recorded** half of the modifier population: rows whose
    /// lifetime is decoupled from any card's zone, and which therefore
    /// cannot be found by the modified-value sweep over the board.
    ///
    /// Written by the evaluator's `Modify` arm when a card declares a
    /// non-constant [`ModifierScope`], and by skill-test initiation for
    /// the one-shot modifier an initiating effect grants the test it
    /// starts (a weapon's *"+N \[combat\] for this attack"*, Flashlight
    /// 01087's *"-2 shroud for this investigation"*). Read by
    /// [`modified_value`](crate::engine::modified_value::modified_value)
    /// alongside the swept population, and expired by whichever boundary
    /// its [`Lifetime`] names — for
    /// [`Lifetime::SkillTest`], the teardown of the test whose id it
    /// carries.
    ///
    /// A row stores its delta as an **expression**, evaluated at read
    /// time like every swept modifier (ADR 0005): Esoteric Formula 02254's
    /// *"You get +2 \[willpower\] for this attack for each clue on the
    /// attacked enemy"* is why a resolved integer is not enough.
    ///
    /// [`ModifierScope`]: card_dsl::dsl::ModifierScope
    pub recorded_modifiers: Vec<RecordedModifier>,
    // The in-flight skill test now lives on its `Continuation::SkillTest(_)`
    // frame (#348); read it via [`Self::current_skill_test`]. The former
    // `in_flight_skill_test: Option<InFlightSkillTest>` field is removed —
    // the continuation stack is the single source of truth.
    /// Stack of currently-open windows. The top (`last()`) is the
    /// most recently-opened; closing pops the top. Carries pending
    /// reaction triggers and the Fast-action gate for each window.
    /// Replaced the earlier single-slot `in_flight_reaction_window:
    /// Option<ReactionWindow>` shape — multi-window nesting is now
    /// structural.
    ///
    /// Window kinds open at canonical timing points:
    /// - `AfterEnemyDefeated` — queued by `damage_enemy` when an
    ///   enemy reaches 0 health.
    /// - `PlayerWindow` — a printed player window at a Rules-Reference
    ///   timing step (e.g. `MythosAfterDraws`), opened by the phase
    ///   machine; gates Fast actions and runs a per-step continuation.
    ///
    /// Multi-window queueing (one effect that queues two windows in
    /// the same apply) is now structural — push twice, drive resumes
    /// in reverse open order.
    /// The single suspend/resume stack (umbrella §1 / Axis-B): the top
    /// frame is resumed by `resolve_input`, taking priority over the
    /// legacy `pending_*` modes. Open reaction/fast windows live here as
    /// `TimingPointWindow` / `FastWindow` frames (the former `open_windows` Vec,
    /// absorbed into the one stack). Required on the wire (#453). Inspect
    /// windows via [`Self::open_windows`] / [`Self::top_window`]. The stack
    /// checks its own invariants; see [`ContinuationStack`].
    pub continuations: ContinuationStack,
    /// Identifier of the scenario this state belongs to, if any.
    ///
    /// `None` for tests and fixtures that don't care about scenario
    /// resolution; in that case the engine's post-apply resolution
    /// hook short-circuits. `Some(id)` is the normal case: when the
    /// [`ScenarioEnd`](Continuation::ScenarioEnd) frame reaches its finalize
    /// step the engine looks up the module via
    /// [`scenario_registry::current`](crate::scenario_registry::current)
    /// and runs its `apply_resolution`.
    ///
    /// Serializable so action-log replay reproduces the lookup
    /// deterministically across host restarts.
    pub scenario_id: Option<ScenarioId>,
    // The Mythos step-1.4 encounter-draw loop now lives on its
    // `Continuation::EncounterDraw` frame (#348); read the prompted drawer via
    // [`Self::current_encounter_drawer`]. The former `mythos_draw_pending:
    // Option<InvestigatorId>` cursor is removed — the continuation stack is the
    // single source of truth (mirroring the `mulligan_pending` fold).
    /// Set by [`Effect::Cancel`](card_dsl::dsl::Effect::Cancel) while a `when`-cell
    /// reaction window resolves, to skip the prevented impact (Axis D #336).
    /// Read-and-cleared by whoever owns the condition's resolution: the timing
    /// coordinator at its resolve step for a coordinator-owned condition (clue
    /// discovery, #703), the emit site after the window closes for a
    /// caller-owned one (the enemy-attack loop). A bool suffices because
    /// Before-windows do not nest in scope — exactly one cancellable impact is
    /// ever in flight. TODO(#367): typed marker once Before-windows can nest.
    /// Required on the wire (#453).
    ///
    /// While set, a coordinator-owned condition's sequence is **abandoned**, not
    /// merely stripped of its resolve step: the `at` and `after` cells and the
    /// rest of the `when` cell are all suppressed (#714). One bool serves both
    /// suppressing arms — a cancel and a nature-changing replacement behave
    /// identically — and the non-suppressing third arm is #366. The citations
    /// live on `coordinator::prevented_in_the_when_cell`, which is where the
    /// signal is read.
    pub pending_cancellation: bool,
    // The former `pending_revelation_discard: Option<CardCode>` side-channel is
    // removed (#380): a drawn treachery's disposal now rides a
    // `Continuation::EncounterCard` frame whose framework teardown discards it
    // once the Revelation's whole sub-resolution completes — covering a
    // Revelation that suspends into a choice, not just a skill test.
    // The former `pending_played_event: Option<(InvestigatorId, CardCode)>`
    // side-channel is removed (#604/#565): a card that has commenced being played
    // (RR Appendix I step 3) but is not yet placed (step 4) now rides the frame
    // driving its play — `Continuation::PlayFromHand`, and the
    // `ActionResume::PlayCard` / `Continuation::SlotDiscard` frames that hand it
    // on across a suspension. A single global slot could not model nesting, and
    // plays nest: a Fast event played to cancel the attack of opportunity
    // provoked by a non-fast event overwrote the slot and erased the first card
    // from the game. See `docs/adr/0002-in-progress-play-lives-on-its-frame.md`.
    /// Active round-scoped skill substitutions (Mind over Matter 01036).
    /// While present, the owning investigator may make a `for_skills` test as
    /// a `use_skill` test instead (offered at test initiation). Cleared at the
    /// round boundary ("until the end of the round"). Required on the wire (#453).
    pub skill_substitutions: Vec<SkillSubstitution>,
    /// Shared encounter deck (top = front). Built at scenario setup
    /// from encounter-set codes; drawn from during Mythos. When the
    /// deck runs out, `draw_encounter_top` (in `engine::dispatch`)
    /// transparently reshuffles [`encounter_discard`](Self::encounter_discard)
    /// back in via the deterministic RNG path.
    ///
    /// Empty at the start of every scenario; populated by scenario
    /// setup (the first wiring lands in #126 alongside the synthetic
    /// fixture's encounter-set composition).
    pub encounter_deck: VecDeque<CardCode>,
    /// Encounter discard pile. Treacheries land here after Revelation
    /// resolves; defeated enemies (and other "discarded from play"
    /// encounter content) land here in later issues.
    ///
    /// Drained back into [`encounter_deck`](Self::encounter_deck) by
    /// `reshuffle_encounter_discard` (in `engine::dispatch`) when
    /// the deck runs empty.
    pub encounter_discard: Vec<CardCode>,
    /// The agenda deck (the doom-fueled lose track). `agenda_deck[agenda_index]`
    /// is the current agenda. Empty for tests/fixtures that don't model
    /// agendas — every agenda helper short-circuits on an empty deck.
    pub agenda_deck: Vec<Agenda>,
    /// Cursor into [`agenda_deck`](Self::agenda_deck): the current agenda.
    pub agenda_index: usize,
    /// Doom currently on the current agenda. Incremented +1 each Mythos
    /// step 1.2; reset to 0 when the agenda advances. (Doom on other
    /// cards in play is not summed yet — no corpus card carries doom.)
    pub agenda_doom: u8,
    /// The act deck (the investigator-driven win track). `act_deck[act_index]`
    /// is the current act. Empty for tests/fixtures that don't model acts.
    pub act_deck: Vec<Act>,
    /// Cursor into [`act_deck`](Self::act_deck): the current act.
    pub act_index: usize,
    /// Fire-once scenario-ending latch: **did the scenario end, and how?**
    /// `None` until the scenario ends; set by `end_scenario` at the
    /// act/agenda resolution point or the no-remaining-players elimination
    /// step, which pushes a [`ScenarioEnd`](Continuation::ScenarioEnd) frame
    /// with it.
    ///
    /// The two `None`s in play here are different questions, which is why this
    /// is not an `Option<ResolutionId>`: *this* `None` means the scenario has
    /// not ended, while [`ScenarioEnding::NoResolution`] means it ended without
    /// reaching a resolution point. Nothing here says whether the players
    /// *won* — that is a standalone-mode projection computed where the ending
    /// is displayed.
    ///
    /// [`ScenarioEnding::NoResolution`]: crate::scenario::ScenarioEnding::NoResolution
    ///
    /// While `Some`, the `drive` loop cancels every frame that is only an
    /// opportunity or a framework step
    /// ([`cancelled_by_scenario_end`](Continuation::cancelled_by_scenario_end))
    /// and lets mandatory resolution finish. *Has the ending finished?* is the
    /// `ScenarioEnd` frame's question, not this field's: the apply boundary pops
    /// that frame as it emits `Event::ScenarioResolved` and runs
    /// `apply_resolution`, exactly once (the idempotency guard formerly tracked
    /// as #131). See
    /// `docs/adr/0004-a-latched-resolution-cancels-opportunities-not-resolutions.md`.
    pub ending: Option<ScenarioEnding>,
    /// The victory display (Rules Reference p.21): an out-of-play zone of
    /// cards worth experience, scored at scenario end. Victory-point
    /// locations are placed here when the scenario resolves (in play +
    /// revealed + no clues); victory-point enemies enter as defeated
    /// (C3). Phase 9 sums these cards' corpus victory values for XP.
    pub victory_display: Vec<CardCode>,
    /// Cards removed from the game (#772), scenario-owned and sitting beside
    /// [`victory_display`](Self::victory_display) because removal is a property
    /// of the *card* rather than of any player's area.
    ///
    /// `glossary/Removed_from_Game.md`: *"A card that has been removed from the
    /// game is placed away from the game area and has no further interaction
    /// with the game in any manner for the duration of its removal."*
    ///
    /// **Not** [`Investigator::removed_from_game`](crate::state::Investigator::removed_from_game),
    /// which is a different thing wearing the same words: the pile elimination
    /// step 1 sweeps an *eliminated investigator's own deck's* cards into. A
    /// card a player controls but does not own has no business in it — which is
    /// exactly Lita Chantler 01117 after a Parley, whose ruling puts her here
    /// instead of in anyone's discard (<https://arkhamdb.com/card/01117>).
    ///
    /// `#[serde(default)]` for the same reason `interactive_acknowledge` carries
    /// one: seeds written before the field existed still deserialize.
    #[serde(default)]
    pub removed_from_game: Vec<CardCode>,
    /// When set, the engine suspends with an `AwaitingInput { InputKind::Confirm }`
    /// at skill-test resolution (after the result events are emitted, before the
    /// ST.7 consequence resolves) so an interactive host can show the player the
    /// result and wait for an acknowledgment (#478). A *cosmetic* pause — it
    /// makes no game decision — so it is gated: the server sets it for human play,
    /// while tests and non-interactive/headless consumers leave it `false` and
    /// resolve straight through. `#[serde(default)]` keeps already-persisted game
    /// seeds (written before this field existed) deserializable.
    #[serde(default)]
    pub interactive_acknowledge: bool,
}

/// One agenda card's mechanically-relevant state: the doom needed to
/// advance it. Card *effect* text is out of scope (per-scenario content),
/// and so is the printed `(→R#)` resolution point on its reverse — a
/// terminal agenda reaches its ending by *running*
/// [`Effect::ReachResolution`](card_dsl::dsl::Effect::ReachResolution) from
/// that reverse, and it is terminal because it is the last card in
/// [`GameState::agenda_deck`], not because it carries a flag. See
/// `docs/adr/0013-a-resolution-point-is-a-printed-effect.md`.
///
/// Deliberately NOT `#[non_exhaustive]`: scenario setup in the
/// `scenarios` crate constructs these with struct literals, which a
/// `#[non_exhaustive]` struct forbids cross-crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agenda {
    /// The encounter-card code this agenda is printed on (e.g.
    /// `01105`). Lets the trigger dispatcher resolve the agenda's
    /// `Trigger::OnEvent` abilities through the card registry — the
    /// agenda owns its Forced effects like any other card.
    pub code: CardCode,
    /// Total doom in play required to advance (Rules Reference p.24
    /// step 1.3). Flat value only for now; per-investigator scaling
    /// and `Objective –` overrides are deferred until a real
    /// scenario needs them.
    pub doom_threshold: u8,
}

/// One act card's mechanically-relevant state: the clues the group must
/// spend to advance it. Its `(→R#)` resolution point is printed on its
/// reverse and reached by effect, exactly as [`Agenda`]'s is. Not
/// `#[non_exhaustive]` for the same cross-crate-construction reason as
/// [`Agenda`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Act {
    /// The encounter-card code this act is printed on (e.g. `01108`).
    /// Lets the trigger dispatcher resolve the act's `Trigger::OnEvent`
    /// abilities through the card registry.
    pub code: CardCode,
    /// Clues the investigators must spend to advance (Rules Reference
    /// p.3). Flat value only for now.
    pub clue_threshold: u8,
}

/// An active "use X in place of Y" skill substitution (Mind over Matter
/// 01036). Round-scoped: cleared at the round boundary. While present, the
/// owning `investigator` may make a `for_skills` test as a `use_skill` test
/// instead — the choice is offered at test initiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSubstitution {
    /// Whose tests this substitution applies to.
    pub investigator: InvestigatorId,
    /// The skill used in place of `for_skills` (Mind over Matter: Intellect).
    pub use_skill: SkillKind,
    /// The skills that may be replaced (Mind over Matter: Combat, Agility).
    pub for_skills: Vec<SkillKind>,
}

crate::state::define_id! {
    /// Identity of one skill test, minted at
    /// `engine::dispatch::start_skill_test` and stamped onto the
    /// [`InFlightSkillTest`] frame.
    ///
    /// A test-scoped modifier is bought "for **this** skill test", so it has
    /// to be able to say which one: a [`RecordedModifier`] carries
    /// [`Lifetime::SkillTest`] with this id, and contributes only while the
    /// test in flight is the one it names. Hyperawareness 01034 (*"\[fast\]
    /// Spend 1 resource: You get +1 \[intellect\] for this skill test."*) is
    /// activatable at any player window and has no per-round limit
    /// (<https://arkhamdb.com/card/01034>: *"You can use \[fast\] fast
    /// actions as many times as you want, as long as you can pay the cost;
    /// there is no limit."*), so nothing else bounds the leak. (ADR 0005.)
    pub struct SkillTestId;
}

/// Which entity's quantity a modifier reaches, or a modified-value query
/// asks about.
///
/// Lives here rather than in the query module because a
/// [`RecordedModifier`] stores one: a row has to say *what* it modifies,
/// and a row is state (and therefore on the wire, #453). The query module
/// re-exports it as
/// [`modified_value::ModifierTarget`](crate::engine::modified_value::ModifierTarget).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModifierTarget {
    /// An investigator's skills and capacities.
    Investigator(InvestigatorId),
    /// A location's shroud.
    Location(LocationId),
    /// An enemy's fight, evade or health.
    Enemy(EnemyId),
    /// The in-flight skill test itself. Its **difficulty** is not a
    /// quantity of its own: reading it resolves the test's
    /// [`DifficultyBasis`] and asks that target instead, so a card that
    /// wants a test to be harder or easier modifies the location or enemy
    /// the test is against rather than naming the test.
    ///
    /// What *does* target the test is a
    /// [`Determination`] row (#685). An
    /// automatic success substitutes the test's total difficulty, and an
    /// automatic failure the tester's total skill value; neither belongs
    /// to the location or the enemy, and the precedence between them is a
    /// property of the test. So a determination row carries this target
    /// whichever of the two it is.
    Test,
}

/// What a skill test's difficulty **is**, rather than what it was when the
/// test started.
///
/// ADR 0005: *"An enemy's modified fight value **is** the difficulty of a
/// Fight action and its modified evade value **is** the difficulty of an
/// Evade action"*, and a location's modified shroud is the difficulty of an
/// investigation. `glossary/Skill_Tests.md`, "Variable Difficulty Skill
/// Tests" — the canonical statement of the rule, quoted here and linked
/// from everywhere else that depends on it:
///
/// > While performing a skill test whose difficulty is modified based on
/// > another aspect of the game, that difficulty changes whenever the
/// > corresponding aspect's status does.
/// >
/// > If an ability or game effect checks a variable difficulty, most
/// > commonly during a skill test's ST.6 to determine whether that test
/// > succeeded or failed, the value of that difficulty is set for the
/// > effect in question and is not re-evaluated if the variable difficulty
/// > continues to change.
///
/// So the in-flight test stores the *basis* — which quantity of which
/// entity its difficulty is read from — and never a number computed at
/// initiation. [`Fixed`](Self::Fixed) is the exception that proves it: a
/// treachery's printed difficulty is a base value on the card, not a
/// snapshot of anything on the board.
///
/// The second paragraph is what bounds the re-reading: the difficulty is
/// live *until* something checks it, and the ST.6 check is where it is set.
/// The engine matches that shape without a pinning step of its own — the
/// ST.6 read stores its verdict on
/// [`InFlightSkillTest::resolved`](InFlightSkillTest::resolved), and every
/// later step reads the verdict rather than re-comparing.
///
/// # When the basis names an entity that has left play
///
/// **A test whose difficulty target leaves play before ST.6 is abandoned**:
/// its frame is torn down, its committed cards are discarded, its test-scoped
/// modifier rows expire, [`SkillTestEnded`](crate::event::Event::SkillTestEnded)
/// fires, and no success or failure is ever declared. The gate is in
/// `skill_test::advance`'s loop preamble, beside the eliminated-tester one it
/// mirrors (#564). Reachable solo: Beat Cop 01018's *"\[fast\] Discard Beat
/// Cop: Deal 1 damage to an enemy at your location"* can defeat a 1-health
/// enemy at the ST.2 player window while the attack against it is in flight
/// (#682).
///
/// **This is an engine decision, not a citation.** The vendored sources do not
/// settle what becomes of a skill test whose target leaves play:
/// `glossary/Fight_Action.md` and `glossary/Evade_Action.md` describe the
/// attack against a present enemy, `glossary/Leaves_Play.md` says what leaving
/// play means but not what happens to a test already resolving against the
/// departed card, and the Official FAQ's nearest question is about the
/// *investigator* moving rather than the target vanishing
/// (`data/official-faq/Frequently_Asked_Questions.md`):
///
/// > Q: If I initiate a skill test at a given location, then trigger an effect
/// > that causes me to move before that test finishes resolving, what happens
/// > to that skill test?
/// >
/// > A: Once you initiate a skill test or ability, you’ll resolve that test
/// > or ability as completely as possible, regardless of your location (unless
/// > another effect cancels or interrupts it). For example, if you attacked an
/// > elusive enemy with One-Two Punch (\[nat\] 17, 32), you could attack that
/// > same enemy with the card’s second fight, even though it has moved to a
/// > connecting location.
///
/// The example is quoted because it is the closest the FAQ comes to this case
/// and it cuts the *other* way — an attack continuing against an enemy that
/// moved. It does not decide the question: One-Two Punch’s elusive enemy is
/// still **in play**, and a target that is in play is exactly the case the
/// engine already handles. The word that decided it is *possible*: resolving
/// a Fight or an Evade against an enemy that is not in play at all is not.
///
/// The rejected alternative was to pin the difficulty at its last read —
/// closer to what a table would improvise, but it keeps a *result*, and
/// therefore every `"If you succeed"` rider on every committed card, attached
/// to a test whose whole subject is gone. Doing nothing is not among the
/// options: with no presence check the modified-value query finds no entity,
/// answers a base of 0, and the attack automatically succeeds against a
/// difficulty of 0.
///
/// The gate covers all three board-reading bases, not only the two the defect
/// was reported against: an investigation whose location has been removed
/// ([`Shroud`](Self::Shroud)) reads a difficulty of 0 for the same reason and
/// wants the same answer. No corpus card removes a location from inside a
/// skill test today — 01108’s removal of the Study 01111 happens at act
/// advancement — so that arm ships on the argument rather than on a test.
///
/// The gate is bounded to tests that have not yet been compared
/// (`resolved.is_none()`). Past ST.6 the verdict is set and the target leaving
/// play is *ordinary* — a successful Fight's own follow-up damage is what
/// defeats the enemy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DifficultyBasis {
    /// A number printed on the initiating card — a Revelation test's
    /// difficulty ([`Effect::SkillTest`](card_dsl::dsl::Effect::SkillTest)).
    Fixed(i8),
    /// A location's modified shroud (an investigation).
    Shroud(LocationId),
    /// An enemy's modified fight value (a Fight action).
    Fight(EnemyId),
    /// An enemy's modified evade value (an Evade action).
    Evade(EnemyId),
}

/// When a [`RecordedModifier`] stops applying, with the boundary already
/// resolved to a concrete identity.
///
/// The card author's vocabulary is [`ModifierScope`] over in `card-dsl` —
/// "this skill test", "this turn". `Lifetime` is the engine's counterpart,
/// and the two are separate types because a scope becomes a lifetime only
/// by being *stamped*: `ThisSkillTest` has no meaning until it names the
/// test in flight. The evaluator's `Modify` arm performs the translation,
/// and it is the point where a scope with nothing to stamp is refused.
///
/// [`ModifierScope`]: card_dsl::dsl::ModifierScope
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Lifetime {
    /// Until the skill test with this id finishes tearing down. Inert
    /// while any other test is in flight, and outside a test entirely.
    ///
    /// The remaining lifetimes ADR 0005 names — until end of turn
    /// ([`ModifierScope::ThisTurn`], #572), until end of phase (Encyclopedia
    /// 01042) — arrive with the effects that record them; the evaluator
    /// still refuses those scopes, so there is no way to write one down.
    ///
    /// [`ModifierScope::ThisTurn`]: card_dsl::dsl::ModifierScope::ThisTurn
    SkillTest(SkillTestId),
}

impl Lifetime {
    /// Whether a row with this lifetime is active while the test identified
    /// by `in_flight` is running.
    #[must_use]
    pub fn applies_during_test(self, in_flight: SkillTestId) -> bool {
        match self {
            Self::SkillTest(id) => id == in_flight,
        }
    }

    /// Whether this lifetime ends with the teardown of the test identified
    /// by `ending`. Drives the expiry sweep at ST.8 and at the abandonment
    /// of a test whose tester was eliminated.
    #[must_use]
    pub fn ends_with_test(self, ending: SkillTestId) -> bool {
        match self {
            Self::SkillTest(id) => id == ending,
        }
    }
}

/// What a recorded row does to the quantity it names: add to it, or
/// replace it wholesale.
///
/// The two are different stages of ADR 0005's fold — the additive pass and
/// the stage-5 substitution — and they carry different payloads, so a row
/// is one or the other rather than a struct with fields that go unread.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RecordedModifierKind {
    /// A signed magnitude on one stat, folded into the additive pass.
    Delta {
        /// Which stat the modifier targets (the modified-value query maps
        /// `SkillKind` → `Stat` for matching).
        stat: Stat,
        /// The signed magnitude, as an **expression evaluated at read
        /// time** rather than a resolved integer — the same rule ADR 0005
        /// applies to the swept population. Esoteric Formula 02254 (*"You
        /// get +2 \[willpower\] for this attack for each clue on the
        /// attacked enemy"*) is the corpus consumer: freezing that to a
        /// number when the row is pushed reintroduces the stale-quantity
        /// bug one level down. Every row a `Modify` writes today carries a
        /// literal, since that is all the DSL's `Modify` can carry; the
        /// elder sign's row carries the investigator card's own expression.
        delta: IntExpr,
    },
    /// The test resolves a particular way whatever the numbers say — the
    /// fold's stage-5 whole-quantity substitution.
    ///
    /// Named against [`ModifierTarget::Test`], because the determination
    /// belongs to the test rather than to either quantity it substitutes,
    /// and read through the test-level query that resolves the precedence
    /// between the two (ADR 0007). Carries no stat: which quantity gets
    /// substituted follows from *which* determination it is, not from
    /// anything the row says.
    Determination(Determination),
}

/// One **recorded** modifier: a contribution whose lifetime is decoupled
/// from any card's zone, so the modified-value sweep over the board cannot
/// find it.
///
/// The decoupling is the printed rule, not an engine design choice.
/// `glossary/Lasting_Effects.md`: *"A lasting effect persists beyond the
/// resolution of the ability that created it, for the duration specified by
/// the effect. The effect continues to affect the game state for the
/// specified duration **regardless of whether the card that created the
/// lasting effect is or remains in play**."* The official FAQ says the same
/// on a worked case — Daring committed and then returned to hand mid-test:
/// *"The lasting effect … would continue for the duration of the skill
/// test, since it is a lasting effect, and lasting effects persist for the
/// specified duration regardless of whether the card that created the
/// lasting effect is or remains in play."*
/// (`data/official-faq/Frequently_Asked_Questions.md`.) A row that a
/// board sweep could find would be wrong for exactly that case, which is
/// why the two populations exist.
///
/// Pushed by the evaluator's `Modify` arm when an activated or triggered
/// ability resolves a `Modify` with a non-constant
/// [`ModifierScope`] — today only
/// [`ThisSkillTest`](card_dsl::dsl::ModifierScope::ThisSkillTest), which
/// stamps [`Lifetime::SkillTest`] with the id of the test in flight (and is
/// refused outright when there is none); by the evaluator's `AutoResolve`
/// arm, under the same "there must be a test to stamp" gate; and by the skill-test driver,
/// for the revealed chaos token's contribution and for the determination an
/// `[auto_fail]` token latches. Read at every modified-value query
/// alongside the swept population, and dropped by the boundary its
/// [`lifetime`](Self::lifetime) names.
///
/// [`ModifierScope`]: card_dsl::dsl::ModifierScope
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RecordedModifier {
    /// What the row modifies. An
    /// [`Investigator`](ModifierTarget::Investigator) for a `Modify` with
    /// audience [`Controller`](card_dsl::dsl::ModifierAudience::Controller);
    /// a [`Location`](ModifierTarget::Location) for the shroud reduction an
    /// [`Investigate`](card_dsl::dsl::ActionDesignator::Investigate) designator
    /// grants the investigation it performs (Flashlight 01087's *"Your location
    /// gets -2 shroud for this investigation."*).
    pub target: ModifierTarget,
    /// The "you" a
    /// [`Delta`](RecordedModifierKind::Delta)'s expression is evaluated
    /// against — the controller of the ability that wrote the row, which is
    /// also the investigator taking the test the row is scoped to. Distinct
    /// from [`target`](Self::target): Flashlight's row modifies a location
    /// but counts clues for *its controller* if it ever needs to.
    pub investigator: InvestigatorId,
    /// What the row does to the quantity it names.
    pub kind: RecordedModifierKind,
    /// When the row stops applying.
    pub lifetime: Lifetime,
    /// The in-play instance that produced the modifier, if any.
    /// `None` for modifiers from non-activated paths (e.g. an
    /// `OnPlay` ability that pushes a per-test buff). Limit-once-
    /// per-test logic in later cycles (Roland Banks, Hard Knocks
    /// upgrades) will key off this, as will Fire Axe 02032's *"(Limit three
    /// times per attack.)"*.
    pub source: Option<CardInstanceId>,
}

impl RecordedModifier {
    /// Construct a [`RecordedModifier`] modifying `investigator`'s own
    /// quantity — the shape every row a `Modify` writes has. Provided so
    /// callers outside the crate (integration tests, where
    /// `#[non_exhaustive]` blocks struct-literal construction) can build a
    /// row directly.
    #[must_use]
    pub fn new(
        investigator: InvestigatorId,
        stat: Stat,
        delta: IntExpr,
        lifetime: Lifetime,
        source: Option<CardInstanceId>,
    ) -> Self {
        Self::targeting(
            ModifierTarget::Investigator(investigator),
            investigator,
            stat,
            delta,
            lifetime,
            source,
        )
    }

    /// Construct the row that latches a test's
    /// [`Determination`].
    ///
    /// Targets [`ModifierTarget::Test`] — the determination is a property
    /// of the test, not of either quantity it substitutes — with
    /// `investigator` naming the tester. `lifetime` is the caller's to
    /// stamp, exactly as for a `Modify`'s row: there is no determination
    /// without a test to scope it to.
    #[must_use]
    pub fn determination(
        investigator: InvestigatorId,
        determination: Determination,
        lifetime: Lifetime,
        source: Option<CardInstanceId>,
    ) -> Self {
        Self {
            target: ModifierTarget::Test,
            investigator,
            kind: RecordedModifierKind::Determination(determination),
            lifetime,
            source,
        }
    }

    /// Construct a [`RecordedModifier`] over an arbitrary
    /// [`target`](Self::target), with `investigator` as the "you" its
    /// [`Delta`](RecordedModifierKind::Delta) expression reads. The shroud
    /// reduction an
    /// [`Investigate`](card_dsl::dsl::ActionDesignator::Investigate) designator
    /// grants (Flashlight 01087) is the one row today whose target is not its
    /// controller.
    #[must_use]
    pub fn targeting(
        target: ModifierTarget,
        investigator: InvestigatorId,
        stat: Stat,
        delta: IntExpr,
        lifetime: Lifetime,
        source: Option<CardInstanceId>,
    ) -> Self {
        Self {
            target,
            investigator,
            kind: RecordedModifierKind::Delta { stat, delta },
            lifetime,
            source,
        }
    }
}

impl GameState {
    /// The skill test currently in flight, if any; `None` outside a test. Reads
    /// the topmost `Continuation::SkillTest` frame — the continuation stack is
    /// the single source of truth for "a test is mid-resolution" (#348). Topmost
    /// (not the top) because a reaction window can sit above the test mid-
    /// resolution; "topmost `SkillTest` = the in-flight test".
    #[must_use]
    pub fn current_skill_test(&self) -> Option<&InFlightSkillTest> {
        self.continuations.topmost_of()
    }

    /// Mutable counterpart to [`Self::current_skill_test`]. Crate-private:
    /// the stack is mutated only by the engine.
    pub(crate) fn current_skill_test_mut(&mut self) -> Option<&mut InFlightSkillTest> {
        self.continuations.topmost_of_mut()
    }

    /// Remove and return the in-flight skill test (popping its frame off the
    /// continuation stack). Called at test teardown.
    pub(crate) fn take_skill_test(&mut self) -> Option<InFlightSkillTest> {
        self.continuations.remove_topmost()
    }

    /// Drop every [`RecordedModifier`] whose [`Lifetime`] ends with the
    /// teardown of the test identified by `ending`.
    ///
    /// Keyed on the test's identity rather than on the tester, so it catches
    /// every row bought for that test — including, once other investigators
    /// can commit to it, rows a teammate bought — and leaves rows belonging
    /// to any other test alone.
    pub fn expire_modifiers_for_test(&mut self, ending: SkillTestId) {
        self.recorded_modifiers
            .retain(|m| !m.lifetime.ends_with_test(ending));
    }

    /// The investigator currently prompted to mulligan, if a setup mulligan is
    /// in progress; `None` otherwise. Reads the top
    /// [`Continuation::Mulligan`] frame's `remaining[0]` — the continuation
    /// stack is the single source of truth for "a mulligan is pending" (#348,
    /// replacing the former `mulligan_pending` cursor). The frame is only ever
    /// the top during setup, so the top (not a topmost search) is correct.
    #[must_use]
    pub fn current_mulligan(&self) -> Option<InvestigatorId> {
        self.continuations
            .top()
            .and_then(MulliganFrame::downcast_ref)
            .and_then(|m| m.remaining.first().copied())
    }

    /// The investigator currently prompted to discard down to the hand-size
    /// limit, if an upkeep hand-size discard is in progress; `None` otherwise.
    /// Reads the top [`Continuation::HandSizeDiscard`] frame's `remaining[0]`
    /// — the frame is only the top while the discard is pending, so the top
    /// is correct (mirrors [`current_mulligan`](Self::current_mulligan)).
    #[must_use]
    pub fn current_hand_size_discard(&self) -> Option<InvestigatorId> {
        self.continuations
            .top()
            .and_then(HandSizeDiscard::downcast_ref)
            .and_then(|h| h.remaining.first().copied())
    }

    /// The investigator currently prompted to draw their Mythos step-1.4
    /// encounter card, if an encounter-draw loop is in progress; `None`
    /// otherwise. Reads the topmost [`Continuation::EncounterDraw`] frame's
    /// `remaining[0]` — the continuation stack is the single source of truth
    /// for "an encounter draw is pending" (#348, replacing the former
    /// `mythos_draw_pending` cursor). Topmost (not the top) because the
    /// drawer's [`PlayerDraw`](Continuation::PlayerDraw) chain frame — and a
    /// mid-chain [`SpawnEngage`](Continuation::SpawnEngage) above it while a
    /// spawn-engagement tie is resolved — sit above the loop frame.
    #[must_use]
    pub fn current_encounter_drawer(&self) -> Option<InvestigatorId> {
        self.continuations
            .topmost_of::<EncounterDrawFrame>()
            .and_then(|d| d.remaining.first().copied())
    }

    /// The **innermost** card currently mid-play, with its controller: one that
    /// has commenced being played (Rules Reference Appendix I step 3) and is not
    /// yet placed in play or in a discard pile (step 4). `None` when no play is
    /// in progress.
    ///
    /// Innermost because plays nest — a Fast event played to cancel the attack
    /// of opportunity a non-fast play provoked runs while the first card is
    /// still mid-play — and the topmost frame is the one whose play the engine
    /// is resolving right now. Replaces the former `pending_played_event` field
    /// as the read view of that state (#604); see
    /// [`Continuation::play_in_progress`].
    #[must_use]
    pub fn play_in_progress(&self) -> Option<(InvestigatorId, &CardCode)> {
        self.continuations
            .iter()
            .rev()
            .find_map(Continuation::play_in_progress)
    }

    /// Whether a skill test is currently in flight.
    #[must_use]
    pub fn has_skill_test_in_flight(&self) -> bool {
        self.continuations
            .topmost_of::<InFlightSkillTest>()
            .is_some()
    }

    /// Enemies engaged with `investigator`, in ascending [`EnemyId`] order
    /// (`BTreeMap` iteration), so callers that compare or index replay
    /// deterministically.
    ///
    /// The single reading of "engaged with you", shared by the kernel's
    /// [`Quantity::EngagedEnemies`](card_dsl::dsl::Quantity::EngagedEnemies) count
    /// and by card predicates in the `cards` crate (Machete 01020). Two
    /// hand-rolled copies of this filter drifting apart is what #592 was.
    ///
    /// `TODO(#579)`: **Massive** enemies are "considered" engaged with every
    /// investigator at their location, so they belong in this iterator — the
    /// keyword is unparsed corpus-wide today, so they are absent here and any
    /// caller inherits that gap.
    pub fn enemies_engaged_with(
        &self,
        investigator: InvestigatorId,
    ) -> impl Iterator<Item = (EnemyId, &Enemy)> {
        self.enemies
            .iter()
            .filter(move |(_, e)| e.engaged_with == Some(investigator))
            .map(|(id, e)| (*id, e))
    }

    /// Every open window/run frame on the stack, in stack order — legacy
    /// [`FastWindow`](Continuation::FastWindow) framework windows **and**
    /// [`TimingPointWindow`](Continuation::TimingPointWindow) event windows /
    /// forced runs (#433). A frame is a window/run iff it carries a candidate
    /// list ([`Continuation::pending_candidates`]).
    fn windows(&self) -> impl DoubleEndedIterator<Item = &Continuation> {
        self.continuations
            .iter()
            .filter(|c| c.pending_candidates().is_some())
    }

    /// The open windows as a `Vec` of references, in stack order. Read
    /// accessor for callers (and tests) that inspect the window stack the
    /// way they used to read the former `open_windows` field.
    #[must_use]
    pub fn open_windows(&self) -> Vec<&Continuation> {
        self.windows().collect()
    }

    /// The topmost open window regardless of pending triggers (the former
    /// `open_windows.last()`), e.g. for the Fast-play `permissive_window`
    /// timing gate — including a pure-Fast gate with empty `pending_triggers`.
    #[must_use]
    pub fn top_window(&self) -> Option<&Continuation> {
        self.windows().next_back()
    }

    /// Build a [`Location`] from its card `metadata`, minting a fresh id.
    /// Panics if `metadata` is not a `Location` card (a build-time
    /// invariant — scenarios hand their own location cards).
    fn location_from_metadata(&mut self, metadata: &CardMetadata) -> Location {
        let (shroud, printed_clues) = match &metadata.kind {
            CardKind::Location {
                shroud,
                printed_clues,
                ..
            } => (*shroud, *printed_clues),
            other => panic!(
                "add_location: card {} is not a Location ({other:?})",
                metadata.code
            ),
        };
        let id = self.location_ids.mint();
        Location {
            id,
            code: CardCode::new(metadata.code.clone()),
            name: metadata.name.clone(),
            shroud,
            clues: 0,
            revealed: false,
            printed_clues,
            connections: Vec::new(),
            attachments: Vec::new(),
            cards_at_location: Vec::new(),
        }
    }

    /// Add a location **into play** from its card metadata, returning the
    /// minted [`LocationId`]. The id is deterministic (construction order),
    /// so scenarios never hand-pick id literals.
    pub fn add_location(&mut self, metadata: &CardMetadata) -> LocationId {
        let loc = self.location_from_metadata(metadata);
        let id = loc.id;
        self.locations.insert(id, loc);
        id
    }

    /// Add a card to the **set-aside** (out-of-play) zone, recording its
    /// printed code only. Nothing is minted here — the cardtype is read
    /// back from the metadata when a card effect brings the card into play
    /// (see [`put_set_aside_card_into_play`]), which is where an enemy's
    /// per-investigator health and a location's [`LocationId`] can be
    /// resolved against the live board.
    ///
    /// [`put_set_aside_card_into_play`]: crate::engine::put_set_aside_card_into_play
    pub fn add_set_aside_card(&mut self, metadata: &CardMetadata) {
        self.set_aside_cards
            .push(CardCode::new(metadata.code.clone()));
    }

    /// Wire a **bidirectional** connection between two in-play locations
    /// (each gains the other in its `connections`).
    ///
    /// # Panics
    ///
    /// Panics if either `a` or `b` is not an in-play location — a build-time
    /// invariant (callers connect ids they just put into play).
    pub fn connect(&mut self, a: LocationId, b: LocationId) {
        self.locations
            .get_mut(&a)
            .unwrap_or_else(|| panic!("connect: location {a:?} not found"))
            .connections
            .push(b);
        self.locations
            .get_mut(&b)
            .unwrap_or_else(|| panic!("connect: location {b:?} not found"))
            .connections
            .push(a);
    }
}

#[cfg(test)]
mod tests;
