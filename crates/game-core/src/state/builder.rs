//! Fluent [`GameStateBuilder`].
//!
//! The cross-crate constructor for [`GameState`] (which is
//! `#[non_exhaustive]`, so it can't be struct-literalled outside this
//! crate). Scenario `setup()` functions use it to build the initial
//! board, and it is also the single most-used piece of engine test
//! infrastructure — every card, scenario, and engine test depends on it,
//! so if the API is awkward, every caller pays. Keep it ergonomic.
//!
//! # Example
//!
//! ```
//! use game_core::engine::enumerate::TurnAction;
//! use game_core::state::{GameStateBuilder, InvestigatorId};
//! use game_core::test_support::{self, location};
//! # let registries = test_support::TEST_REGISTRIES;
//!
//! let state = GameStateBuilder::new()
//!     .with_investigator(test_support::test_investigator(1))
//!     .with_investigator(test_support::test_investigator(2))
//!     .with_seating([InvestigatorId(1), InvestigatorId(2)])
//!     .with_location(location(10))
//!     // Investigation phase, investigator 1 active, their turn menu up.
//!     .open_turn(InvestigatorId(1))
//!     .build();
//!
//! // Investigator 1 ends their turn, which opens investigator 2's.
//! let session = test_support::TestSession::new(state, registries)
//!     .take(&TurnAction::EndTurn);
//! session.assert_at_turn_menu(InvestigatorId(2));
//! ```

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::card_registry::CardRegistry;
use crate::engine::Registries;
use crate::rng::RngState;
use crate::scenario::ScenarioId;
use crate::state::staged::{Reprint, Staged};
use crate::state::{
    Act, Agenda, CardCode, CardInstanceId, ChaosBag, Continuation, ContinuationStack, Counter,
    EncounterDrawFrame, Enemy, EnemyId, EnemyPhaseFrame, EnemyResume, FastWindowFrame,
    FastWindowKind, GameState, HandSizeDiscard, InvestigationPhaseFrame, InvestigationResume,
    Investigator, InvestigatorId, InvestigatorTurnFrame, Location, LocationId, MulliganFrame,
    Phase, TokenModifiers, UpkeepPhaseFrame, UpkeepResume,
};

/// Fluent builder for a [`GameState`].
///
/// Construct with [`GameStateBuilder::new`], chain `.with_*` setters and
/// adders, then call [`build`](GameStateBuilder::build) to get a `GameState`
/// ready for [`apply`](crate::engine::apply).
///
/// To build a state with an investigator's turn open, use
/// [`open_turn`](GameStateBuilder::open_turn): it stages the Investigation
/// phase anchor and the turn frame together, so no caller can build a turn
/// without the anchor beneath it.
#[derive(Debug, Clone)]
#[must_use = "GameStateBuilder is a builder; call .build() to produce a GameState"]
pub struct GameStateBuilder {
    investigators: BTreeMap<InvestigatorId, Investigator>,
    /// Locations and enemies as staged: each resolves at [`build`](Self::build),
    /// reading a printed card's fields once every seat is known.
    locations: BTreeMap<LocationId, Staged<Location>>,
    enemies: BTreeMap<EnemyId, Staged<Enemy>>,
    /// The act and agenda decks as staged, in order: each card resolves at
    /// [`build`](Self::build), reading a printed card's threshold.
    act_deck: Vec<Staged<Act>>,
    agenda_deck: Vec<Staged<Agenda>>,
    act_index: usize,
    agenda_index: usize,
    agenda_doom: u8,
    encounter_deck: VecDeque<CardCode>,
    encounter_discard: Vec<CardCode>,
    chaos_bag: ChaosBag,
    token_modifiers: TokenModifiers,
    phase: Phase,
    round: u32,
    active_investigator: Option<InvestigatorId>,
    seating: Vec<InvestigatorId>,
    lead: Option<InvestigatorId>,
    rng: RngState,
    mulligan_remaining: Option<Vec<InvestigatorId>>,
    mythos_draw_remaining: Option<Vec<InvestigatorId>>,
    hand_size_discard_pending: Option<HandSizeDiscard>,
    open_windows: Vec<Continuation>,
    phase_anchor: Option<Continuation>,
    investigator_turn: Option<InvestigatorId>,
    engagements: Vec<(EnemyId, InvestigatorId)>,
    /// Frames staged on top of everything else, bottom first
    /// ([`with_frame`](Self::with_frame)).
    frames: Vec<Continuation>,
    scenario_id: Option<ScenarioId>,
    /// The registries a test named with
    /// [`with_registries`](Self::with_registries), which the test-only
    /// adders ([`with_seat`](Self::with_seat), [`session`](Self::session))
    /// resolve against. A production builder never sets it, and it is not
    /// part of the built state.
    registries: Option<Registries>,
}

impl GameStateBuilder {
    /// Start a new builder with empty investigators / locations / chaos
    /// bag, `Phase::Mythos`, round 0, no active investigator, RNG
    /// seeded at zero. Most tests override the seed explicitly via
    /// [`with_rng_seed`](Self::with_rng_seed).
    pub fn new() -> Self {
        Self {
            investigators: BTreeMap::new(),
            locations: BTreeMap::new(),
            enemies: BTreeMap::new(),
            act_deck: Vec::new(),
            agenda_deck: Vec::new(),
            act_index: 0,
            agenda_index: 0,
            agenda_doom: 0,
            encounter_deck: VecDeque::new(),
            encounter_discard: Vec::new(),
            chaos_bag: ChaosBag::new([]),
            token_modifiers: TokenModifiers::default(),
            phase: Phase::Mythos,
            round: 0,
            active_investigator: None,
            seating: Vec::new(),
            lead: None,
            rng: RngState::new(0),
            mulligan_remaining: None,
            mythos_draw_remaining: None,
            hand_size_discard_pending: None,
            open_windows: Vec::new(),
            phase_anchor: None,
            investigator_turn: None,
            engagements: Vec::new(),
            frames: Vec::new(),
            scenario_id: None,
            registries: None,
        }
    }

    /// Add an investigator to the state. Replaces any existing entry
    /// with the same id.
    pub fn with_investigator(mut self, investigator: Investigator) -> Self {
        self.investigators.insert(investigator.id, investigator);
        self
    }

    /// Add an investigator placed at `location`. Sets the investigator's
    /// `current_location` to `Some(location)` then inserts; equivalent to
    /// the pre-existing two-step `let mut inv = …; inv.current_location =
    /// Some(loc); .with_investigator(inv)` shape. Replaces any existing
    /// investigator entry with the same id, like [`with_investigator`].
    /// The named location itself must still be added separately via
    /// [`with_location`] (this helper does not insert one — a test stages
    /// the location with its own shroud / connections / clues).
    ///
    /// # Example
    ///
    /// ```
    /// use game_core::state::{GameStateBuilder, InvestigatorId, LocationId};
    /// use game_core::test_support::{self, location};
    ///
    /// let state = GameStateBuilder::new()
    ///     .with_investigator_at(test_support::test_investigator(1), LocationId(10))
    ///     .with_location(location(10))
    ///     .build();
    /// assert_eq!(
    ///     state.investigators[&InvestigatorId(1)].current_location,
    ///     Some(LocationId(10)),
    /// );
    /// ```
    ///
    /// [`with_investigator`]: Self::with_investigator
    /// [`with_location`]: Self::with_location
    pub fn with_investigator_at(
        mut self,
        mut investigator: Investigator,
        location: LocationId,
    ) -> Self {
        investigator.current_location = Some(location);
        self.investigators.insert(investigator.id, investigator);
        self
    }

    /// Stage `location`, replacing any staged with the same id. The test-only
    /// [`with_location`](Self::with_location) is the caller.
    pub(crate) fn stage_location(&mut self, location: Staged<Location>) {
        self.locations.insert(location.piece.id, location);
    }

    /// Stage `enemy`, replacing any staged with the same id (and its
    /// engagement). The test-only [`with_enemy`](Self::with_enemy) is the
    /// caller.
    pub(crate) fn stage_enemy(&mut self, enemy: Staged<Enemy>) {
        let id = enemy.piece.id;
        self.engagements.retain(|&(engaged, _)| engaged != id);
        self.enemies.insert(id, enemy);
    }

    /// Engage the staged enemy `enemy` with `investigator` at
    /// [`build`](Self::build), which places it at that investigator's
    /// location — so the investigator may be added before or after.
    pub(crate) fn stage_engagement(&mut self, enemy: EnemyId, investigator: InvestigatorId) {
        self.engagements.retain(|&(engaged, _)| engaged != enemy);
        self.engagements.push((enemy, investigator));
    }

    /// Stage the act deck, replacing any staged before. The test-only
    /// [`with_act_deck`](Self::with_act_deck) is the caller.
    pub(crate) fn stage_act_deck(&mut self, deck: Vec<Staged<Act>>) {
        self.act_deck = deck;
    }

    /// Stage the agenda deck, replacing any staged before. The test-only
    /// [`with_agenda_deck`](Self::with_agenda_deck) is the caller.
    pub(crate) fn stage_agenda_deck(&mut self, deck: Vec<Staged<Agenda>>) {
        self.agenda_deck = deck;
    }

    /// Make the act at `index` in the act deck the current act,
    /// [`GameState::act_index`]. Defaults to 0, the deck's first act.
    pub fn with_current_act(mut self, index: usize) -> Self {
        self.act_index = index;
        self
    }

    /// Make the agenda at `index` in the agenda deck the current agenda,
    /// [`GameState::agenda_index`]. Defaults to 0, the deck's first agenda.
    pub fn with_current_agenda(mut self, index: usize) -> Self {
        self.agenda_index = index;
        self
    }

    /// Put `doom` doom on the current agenda, [`GameState::agenda_doom`].
    /// Defaults to 0.
    pub fn with_agenda_doom(mut self, doom: u8) -> Self {
        self.agenda_doom = doom;
        self
    }

    /// Set the encounter deck, [`GameState::encounter_deck`], top card first.
    /// Replaces any set before; defaults to empty.
    pub fn with_encounter_deck(
        mut self,
        cards: impl IntoIterator<Item = impl Into<CardCode>>,
    ) -> Self {
        self.encounter_deck = cards.into_iter().map(Into::into).collect();
        self
    }

    /// Set the encounter discard pile, [`GameState::encounter_discard`],
    /// bottom card first (the order the engine pushes discards in). Replaces
    /// any set before; defaults to empty.
    pub fn with_encounter_discard(
        mut self,
        cards: impl IntoIterator<Item = impl Into<CardCode>>,
    ) -> Self {
        self.encounter_discard = cards.into_iter().map(Into::into).collect();
        self
    }

    /// Set the chaos bag. Replaces any prior bag.
    pub fn with_chaos_bag(mut self, chaos_bag: ChaosBag) -> Self {
        self.chaos_bag = chaos_bag;
        self
    }

    /// Set the per-scenario symbol-token modifiers. Defaults to all
    /// zeros if not called.
    pub fn with_token_modifiers(mut self, modifiers: TokenModifiers) -> Self {
        self.token_modifiers = modifiers;
        self
    }

    /// Set the current phase.
    pub fn with_phase(mut self, phase: Phase) -> Self {
        self.phase = phase;
        self
    }

    /// Set the round counter.
    pub fn with_round(mut self, round: u32) -> Self {
        self.round = round;
        self
    }

    /// Mark an investigator as the active one. The id must refer to an
    /// investigator already added via [`with_investigator`]; this is
    /// not enforced at build time but later [`apply`](crate::engine::apply)
    /// calls will surface state-corruption invariant violations
    /// loudly.
    ///
    /// [`with_investigator`]: Self::with_investigator
    pub fn with_active_investigator(mut self, id: InvestigatorId) -> Self {
        self.active_investigator = Some(id);
        self
    }

    /// Set the seating, [`GameState::seating`]. Unless
    /// [`with_lead`](Self::with_lead) names another, its first entry is the
    /// lead, so player order is the seating as given. Left unset,
    /// [`open_turn`](Self::open_turn) seats its investigator alone.
    pub fn with_seating(mut self, order: impl IntoIterator<Item = InvestigatorId>) -> Self {
        self.seating = order.into_iter().collect();
        self
    }

    /// Set the lead investigator, [`GameState::lead`], so player order starts
    /// at `id` rather than at the seating's first entry.
    pub fn with_lead(mut self, id: InvestigatorId) -> Self {
        self.lead = Some(id);
        self
    }

    /// Seed the deterministic RNG. Resets `draws` to 0.
    pub fn with_rng_seed(mut self, seed: u64) -> Self {
        self.rng = RngState::new(seed);
        self
    }

    /// Set the full RNG state (seed + draws). Useful for tests that
    /// want to start mid-stream.
    pub fn with_rng(mut self, rng: RngState) -> Self {
        self.rng = rng;
        self
    }

    /// Stage a pending setup mulligan over the given player-order queue
    /// (front = currently prompted). By default no mulligan is staged so
    /// tests don't accidentally exercise Mulligan paths; opt in when a test
    /// wants to resume the mulligan loop directly via `ResolveInput` without
    /// going through scenario setup (via `seat_and_open`). The queue must list
    /// the investigators in player order (the seating set via
    /// [`with_seating`](Self::with_seating), from the lead) so the loop advances
    /// correctly. Stages a [`Continuation::Mulligan`] frame at
    /// [`build`](Self::build).
    pub fn with_mulligan_remaining(
        mut self,
        remaining: impl IntoIterator<Item = InvestigatorId>,
    ) -> Self {
        self.mulligan_remaining = Some(remaining.into_iter().collect());
        self
    }

    /// Stage a pending Mythos step-1.4 encounter draw over the given
    /// player-order queue (front = currently prompted). By default none is
    /// staged; opt in when a test wants to resume the encounter-draw loop
    /// directly via `ResolveInput(Confirm)` without driving the full Mythos
    /// cascade. The queue must list the investigators in player order (the
    /// seating set via [`with_seating`](Self::with_seating), from the lead) so
    /// the loop advances correctly. Stages a [`Continuation::EncounterDraw`] frame at
    /// [`build`](Self::build).
    pub fn with_mythos_draw_remaining(
        mut self,
        remaining: impl IntoIterator<Item = InvestigatorId>,
    ) -> Self {
        self.mythos_draw_remaining = Some(remaining.into_iter().collect());
        self
    }

    /// Seed a pending upkeep hand-size discard for the given player-order
    /// queue (front = currently prompted).
    pub fn with_hand_size_discard_pending(
        mut self,
        remaining: impl IntoIterator<Item = InvestigatorId>,
    ) -> Self {
        self.hand_size_discard_pending = Some(HandSizeDiscard {
            remaining: remaining.into_iter().collect(),
        });
        self
    }

    /// Push a framework [`FastWindow`](Continuation::FastWindow) onto the
    /// build's window stack for tests that need a specific window-state shape.
    ///
    /// The pushed window has no pending candidates (test paths that
    /// also need a reaction queue should manipulate `state` after
    /// `build()` rather than complicate this builder).
    pub fn with_open_window(mut self, kind: FastWindowKind) -> Self {
        // Framework player windows are `FastWindow`. The builder
        // only constructs framework windows; event windows / the forced run
        // (`TimingPointWindow`) are produced by the engine, not seeded here.
        self.open_windows.push(
            FastWindowFrame {
                candidates: Vec::new(),
                kind,
            }
            .into(),
        );
        self
    }

    /// Stage a `*Phase` anchor frame at the **bottom** of the
    /// continuation stack — the realistic invariant for a state constructed
    /// mid-phase (the real driver pushes the anchor at phase entry, beneath any
    /// framework windows). Tests that drive `end_turn` / open phase windows
    /// without going through the phase driver use this to satisfy the anchor
    /// invariant.
    ///
    /// # Panics
    ///
    /// Panics if `anchor` is not a `*Phase` anchor (`MythosPhase` /
    /// `InvestigationPhase` / `EnemyPhase` / `UpkeepPhase`).
    pub fn with_phase_anchor(mut self, anchor: impl Into<Continuation>) -> Self {
        let frame = anchor.into();
        assert!(
            frame.is_phase_anchor(),
            "with_phase_anchor expects a *Phase anchor variant, got {frame:?}",
        );
        self.phase_anchor = Some(frame);
        self
    }

    /// Open `investigator`'s turn: the state the engine rests in when that
    /// investigator's turn menu is up.
    ///
    /// Sets the Investigation phase, makes `investigator` active, stages the
    /// Investigation phase anchor at turn-begins and the open turn above it.
    /// A seating already set is kept; with none set, it defaults to
    /// `[investigator]`.
    pub fn open_turn(mut self, investigator: InvestigatorId) -> Self {
        if self.seating.is_empty() {
            self.seating = vec![investigator];
        }
        self.phase = Phase::Investigation;
        self.active_investigator = Some(investigator);
        self.phase_anchor = Some(
            InvestigationPhaseFrame {
                resume: InvestigationResume::TurnBegins,
            }
            .into(),
        );
        self.with_investigator_turn(investigator)
    }

    /// End the Enemy phase: the moment after the last investigator's attacks,
    /// when step 3.4 runs.
    ///
    /// Sets the Enemy phase and stages its anchor at
    /// [`AfterAllAttacked`](EnemyResume::AfterAllAttacked), so the engine's next
    /// drive runs the phase end — Rules Reference 3.4, *"This step formalizes the
    /// end of the enemy phase."* — resolves the `PhaseEnded { Enemy }` forced
    /// abilities, and transitions to Upkeep.
    pub fn ending_enemy_phase(mut self) -> Self {
        self.phase = Phase::Enemy;
        self.phase_anchor = Some(
            EnemyPhaseFrame {
                resume: EnemyResume::AfterAllAttacked,
            }
            .into(),
        );
        self
    }

    /// End the Upkeep phase, and with it the round: the moment after step 4.4's
    /// draw and resource.
    ///
    /// Sets the Upkeep phase and stages its anchor at
    /// [`AfterDraw`](UpkeepResume::AfterDraw), so the engine's next drive runs
    /// step 4.5 (*"each investigator with more than 8 cards in hand chooses and
    /// discards cards from his or her hand until he or she has 8 cards remaining
    /// in hand"* — nothing, unless a hand is over the cap) and then step 4.6:
    /// *"This step formalizes the end of the upkeep phase. As the upkeep phase is
    /// the final phase in the round, this step also formalizes the end of the
    /// round. Any active "until the end of the round" lasting effects expire at
    /// this time."* The `PhaseEnded { Upkeep }` forced abilities resolve, then
    /// the round end's, then play proceeds to the next round's Mythos phase.
    pub fn ending_upkeep_phase(mut self) -> Self {
        self.phase = Phase::Upkeep;
        self.phase_anchor = Some(
            UpkeepPhaseFrame {
                resume: UpkeepResume::AfterDraw,
            }
            .into(),
        );
        self
    }

    /// Stage an [`InvestigatorTurn`](Continuation::InvestigatorTurn) frame
    /// on top of the staged `*Phase` anchor — the realistic
    /// invariant for a state constructed mid-turn (the real driver pushes it once
    /// the `InvestigatorTurnBegins` window closes).
    ///
    /// Private: a turn frame without its phase anchor is a state the engine
    /// never rests in, so [`open_turn`](Self::open_turn) is the only public way
    /// to open a turn — it stages the anchor and the turn together.
    fn with_investigator_turn(mut self, investigator: InvestigatorId) -> Self {
        self.investigator_turn = Some(investigator);
        self
    }

    /// Stage `frame` on top of everything else the builder stages, above any
    /// frame staged before it — the state the engine rests in with that frame
    /// pending. Settled into a session, a prompt frame staged this way shows its
    /// prompt, which is how a test reaches a prompt without driving into it.
    ///
    /// The frame is pushed with the stack's usual checks, so a second skill
    /// test or a phase anchor above a queued ability panics at
    /// [`build`](Self::build) in debug builds.
    pub fn with_frame(mut self, frame: impl Into<Continuation>) -> Self {
        self.frames.push(frame.into());
        self
    }

    /// Set the scenario id this state belongs to. `None` (the
    /// default from [`GameStateBuilder::new`]) means the engine's post-apply
    /// resolution hook will short-circuit. Passing a `ScenarioId`
    /// means the `ScenarioRegistry` each apply is handed must resolve it
    /// *if you want resolutions to fire* — the resolution lookup silently
    /// no-ops when `module_for` returns `None`.
    pub fn with_scenario_id(mut self, id: ScenarioId) -> Self {
        self.scenario_id = Some(id);
        self
    }

    /// Every card-instance id on a card staged so far — in a play area, a
    /// threat area, at or attached to a location, or attached to an enemy —
    /// **except investigator cards'**. A stock test investigator's card takes
    /// `u32::MAX - id`, out of the range the counter mints from, and seeding
    /// past it would saturate the counter.
    pub(crate) fn card_instance_ids_in_use(&self) -> impl Iterator<Item = CardInstanceId> + '_ {
        let investigators = self
            .investigators
            .values()
            .flat_map(|inv| inv.cards_in_play.iter().chain(&inv.threat_area));
        let locations = self
            .locations
            .values()
            .map(|loc| &loc.piece)
            .flat_map(|loc| loc.attachments.iter().chain(&loc.cards_at_location));
        let enemies = self
            .enemies
            .values()
            .flat_map(|enemy| &enemy.piece.attachments);
        investigators
            .chain(locations)
            .chain(enemies)
            .map(|card| card.instance_id)
    }

    /// The id the card-instance counter mints next: one past every id
    /// [in use](Self::card_instance_ids_in_use), so the engine's first mint
    /// can never take an id a fixture already placed.
    pub(crate) fn next_card_instance_id(&self) -> u32 {
        self.card_instance_ids_in_use()
            .map(|id| id.0.saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// The registries named by [`with_registries`](Self::with_registries),
    /// if any.
    pub(crate) fn registries(&self) -> Option<Registries> {
        self.registries
    }

    /// Record the registries a test names; the test-only
    /// [`with_registries`](Self::with_registries) is the one caller.
    pub(crate) fn set_registries(&mut self, registries: Registries) {
        self.registries = Some(registries);
    }

    /// Materialize the configured [`GameState`].
    ///
    /// The card-instance counter starts one past every card-instance id
    /// already on the board (investigator cards aside), and the enemy and
    /// location id counters one past the highest staged enemy and location,
    /// so nothing the engine mints collides with what the caller placed.
    ///
    /// A staged enemy or location that names a printed card
    /// ([`as_card`](crate::test_support::StagedEnemy::as_card)) reads it here,
    /// from the registries named by [`with_registries`](Self::with_registries),
    /// with a per-investigator value scaled to every investigator on the
    /// board — whichever order they and the card were staged in. So does each
    /// printed card in a staged act or agenda deck
    /// ([`with_act_deck`](Self::with_act_deck)), for its threshold.
    ///
    /// # Panics
    ///
    /// Panics if two staged cards share a card-instance id (investigator
    /// cards aside), if an enemy was engaged with an investigator that was
    /// never added, or, naming the code, if a staged printed card can't be
    /// read: no registries were named, the registry doesn't know it, or it is
    /// the wrong card type.
    pub fn build(self) -> GameState {
        let mut seen = BTreeSet::new();
        for id in self.card_instance_ids_in_use() {
            assert!(
                seen.insert(id),
                "GameStateBuilder::build: two staged cards share the instance id {id:?}",
            );
        }
        let card_instance_ids = Counter::at(self.next_card_instance_id());
        let investigator_count = u8::try_from(self.investigators.len()).unwrap_or(u8::MAX);
        let cards = self.registries.map(|registries| registries.cards);
        let locations: BTreeMap<LocationId, Location> = self
            .locations
            .into_iter()
            .map(|(id, staged)| (id, staged.resolve(cards, investigator_count)))
            .collect();
        let mut enemies: BTreeMap<EnemyId, Enemy> = self
            .enemies
            .into_iter()
            .map(|(id, staged)| (id, staged.resolve(cards, investigator_count)))
            .collect();
        let act_deck = resolve_deck(self.act_deck, cards, investigator_count);
        let agenda_deck = resolve_deck(self.agenda_deck, cards, investigator_count);
        let location_ids = Counter::at(next_id(locations.keys().map(|id| id.0)));
        let enemy_ids = Counter::at(next_id(enemies.keys().map(|id| id.0)));
        for (enemy, investigator) in self.engagements {
            let location = self
                .investigators
                .get(&investigator)
                .unwrap_or_else(|| {
                    panic!("with_enemy_engaged: {investigator:?} was never added to the builder")
                })
                .current_location;
            if let Some(enemy) = enemies.get_mut(&enemy) {
                enemy.engaged_with = Some(investigator);
                enemy.current_location = location;
            }
        }
        // Builder-staged windows become `FastWindow` frames on the one
        // continuation stack; a staged hand-size discard becomes a
        // `HandSizeDiscard` frame on top of them.
        // A staged `*Phase` anchor sits at the bottom of the
        // stack — beneath any windows, which open *above* it during the phase.
        let mut continuations = ContinuationStack::new();
        if let Some(anchor) = self.phase_anchor {
            continuations.push(anchor);
        }
        // A staged open turn sits directly above the anchor;
        // any window opened during the turn is a sub-resolution above it.
        if let Some(investigator) = self.investigator_turn {
            continuations.push(InvestigatorTurnFrame {
                investigator,
                ending: false,
            });
        }
        for window in self.open_windows {
            continuations.push(window);
        }
        if let Some(hsd) = self.hand_size_discard_pending {
            continuations.push(Continuation::HandSizeDiscard(hsd));
        }
        // A staged setup mulligan becomes a `Mulligan` frame. Setup and
        // upkeep are disjoint phases, so this never coexists with a staged
        // hand-size discard; push order is immaterial.
        if let Some(remaining) = self.mulligan_remaining {
            continuations.push(MulliganFrame { remaining });
        }
        // A staged Mythos encounter draw becomes an `EncounterDraw` frame.
        // Disjoint from setup/upkeep, so push order is immaterial.
        if let Some(remaining) = self.mythos_draw_remaining {
            continuations.push(EncounterDrawFrame { remaining });
        }
        // Frames staged with `with_frame` go on top of everything else, in the
        // order they were staged.
        for frame in self.frames {
            continuations.push(frame);
        }
        GameState {
            investigators: self.investigators,
            locations,
            set_aside_cards: Vec::new(),
            starting_location: None,
            enemies,
            chaos_bag: self.chaos_bag,
            token_modifiers: self.token_modifiers,
            phase: self.phase,
            round: self.round,
            active_investigator: self.active_investigator,
            lead: self.lead.or_else(|| self.seating.first().copied()),
            seating: self.seating,
            rng: self.rng,
            card_instance_ids,
            enemy_ids,
            location_ids,
            skill_test_ids: Counter::new(),
            recorded_modifiers: Vec::new(),
            continuations,
            scenario_id: self.scenario_id,
            lasting_effects: Vec::new(),
            encounter_deck: self.encounter_deck,
            encounter_discard: self.encounter_discard,
            agenda_deck,
            agenda_index: self.agenda_index,
            agenda_doom: self.agenda_doom,
            act_deck,
            act_index: self.act_index,
            ending: None,
            victory_display: Vec::new(),
            removed_from_game: Vec::new(),
            interactive_acknowledge: false,
        }
    }
}

/// One past the highest of `ids`, or 0 for none: where a counter starts so
/// its first mint takes none of them.
fn next_id(ids: impl Iterator<Item = u32>) -> u32 {
    ids.map(|id| id.saturating_add(1)).max().unwrap_or(0)
}

/// Each card of a staged deck, [resolved](Staged::resolve), in order.
fn resolve_deck<T: Reprint>(
    deck: Vec<Staged<T>>,
    cards: Option<&CardRegistry>,
    investigator_count: u8,
) -> Vec<T> {
    deck.into_iter()
        .map(|staged| staged.resolve(cards, investigator_count))
        .collect()
}

impl Default for GameStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod set_aside_cards_tests {
    use super::*;

    #[test]
    fn build_starts_with_empty_set_aside_cards() {
        let state = GameStateBuilder::new().build();
        assert!(state.set_aside_cards.is_empty());
    }

    #[test]
    fn build_defaults_interactive_acknowledge_off() {
        let state = GameStateBuilder::new().build();
        assert!(
            !state.interactive_acknowledge,
            "interactive acknowledgment is opt-in; off by default so non-interactive \
             consumers and existing tests never pause"
        );
    }
}

#[cfg(test)]
mod with_open_window_tests {
    use super::*;
    use crate::state::PhaseStep;
    use crate::test_support::{seat, TEST_REGISTRIES};

    #[test]
    fn with_open_window_pushes_onto_the_stack() {
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(seat(1))
            .with_open_window(FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins))
            .build();
        assert_eq!(state.open_windows().len(), 1);
        assert!(matches!(
            state.open_windows()[0],
            Continuation::FastWindow(FastWindowFrame { .. })
        ));
        assert!(state.open_windows()[0]
            .pending_candidates()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn with_open_window_stacks_in_order() {
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(seat(1))
            .with_open_window(FastWindowKind::Phase(PhaseStep::MythosAfterDraws))
            .with_open_window(FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins))
            .build();
        assert_eq!(state.open_windows().len(), 2);
        assert!(matches!(
            state.open_windows()[1],
            Continuation::FastWindow(FastWindowFrame {
                kind: FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
                ..
            })
        ));
    }
}
