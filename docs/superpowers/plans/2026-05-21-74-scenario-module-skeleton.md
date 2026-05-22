# #74 Scenario Module Skeleton — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Define the scenario-module shape (`ScenarioId`, `Resolution`, `ScenarioModule`, `ScenarioRegistry`) in `game-core`, ship one synthetic test fixture in `scenarios`, and wire `apply()` to call `detect_resolution` and emit `Event::ScenarioResolved` after each `Done` outcome.

**Architecture:** Mirrors the existing `CardRegistry` pattern. `game-core` owns the types and a `OnceLock`-backed registry slot; the `scenarios` crate provides a `pub const REGISTRY: ScenarioRegistry` that hosts install at startup. `GameState` gains an `Option<ScenarioId>`; the post-dispatch hook short-circuits when either the id is `None` or no registry is installed. Engine unit tests use a parameterized helper so they can mock the registry without touching the process-global `OnceLock`; one integration test in `crates/scenarios/tests/` exercises the global wiring end-to-end.

**Tech Stack:** Rust, `cargo`, `gh` CLI. Workspace crates touched: `game-core`, `scenarios`.

**Spec:** `docs/superpowers/specs/2026-05-21-74-scenario-module-skeleton-design.md`.

---

## Branch and scope

- Branch name: `engine/scenario-module-skeleton` (per CLAUDE.md PR procedure: `<scope>/<short-slug>`).
- Closes issue #74.
- The phase-doc update (move `#74` from Open → Closed, flip the Arc row, drop the now-settled open question, add Decision for the parameterized-helper pattern) is the **final commit on the branch** before merge — per CLAUDE.md PR procedure step 7.

---

## File structure

**Modified:**
- `crates/game-core/src/state/game_state.rs` — add `GameState.scenario_id: Option<ScenarioId>`.
- `crates/game-core/src/state/mod.rs` — re-export `ScenarioId` and `Resolution`.
- `crates/game-core/src/event.rs` — add `Event::ScenarioResolved { resolution: Resolution }`.
- `crates/game-core/src/lib.rs` — `pub mod scenario; pub mod scenario_registry;` + re-exports.
- `crates/game-core/src/engine/mod.rs` — add `fire_scenario_resolution` helper, wire `apply()` to call it on `Done`; engine unit tests.
- `crates/game-core/src/test_support/builder.rs` — `TestGame.scenario_id`, `with_scenario_id`, threaded through `build()`.
- `crates/scenarios/Cargo.toml` — add `[features] test_fixtures = []`; add `[dev-dependencies]` block with `game-core = { path = "../game-core" }` (already a normal dep, but the integration test needs the feature on by default for the test binary — see Task 11 for the cleaner approach).
- `crates/scenarios/src/lib.rs` — declare `pub mod test_fixtures;` under cfg gate; expose `pub const REGISTRY: ScenarioRegistry`.
- `docs/phases/phase-4-scenario-plumbing.md` — move `#74` to Closed, flip Arc row, add Decision, drop settled Open question.

**New:**
- `crates/game-core/src/scenario.rs` — `ScenarioId`, `Resolution`, `ScenarioModule`, `ScenarioRegistry` types.
- `crates/game-core/src/scenario_registry.rs` — `OnceLock`-backed install/current pair; mirrors `card_registry.rs`.
- `crates/scenarios/src/test_fixtures/mod.rs` — module index for fixture submodules.
- `crates/scenarios/src/test_fixtures/synthetic.rs` — the one stub scenario.
- `crates/scenarios/tests/synthetic_resolution.rs` — end-to-end integration test.

**Not touched:**
- `crates/cards/` — no card changes.
- `crates/card-dsl/`, `crates/card-data-pipeline/` — no card-data shape changes.
- `crates/server/`, `crates/web/` — no host installs the scenario registry yet.

---

## Task 1: Branch setup

**Files:** none modified.

- [ ] **Step 1: Confirm `main` is clean and create the feature branch**

Run:
```bash
git status --short
git checkout -b engine/scenario-module-skeleton
```

Expected: `git status` shows only the untracked `docs/superpowers/` directory (the spec lives there and is intentionally untracked per user preference). The new branch is created from `main` at the merge commit of #129.

---

## Task 2: Add `game_core::scenario` module — `ScenarioId`, `Resolution`, `ScenarioModule`, `ScenarioRegistry` types

**Files:**
- Create: `crates/game-core/src/scenario.rs`
- Modify: `crates/game-core/src/lib.rs` (add `pub mod scenario;` and re-exports)

- [ ] **Step 1: Create `crates/game-core/src/scenario.rs` with the type definitions**

Create the file with:

```rust
//! Scenario-module data types: identifier, resolution outcome, and the
//! static `ScenarioModule` / `ScenarioRegistry` pair that bridges
//! engine ↔ scenarios crate.
//!
//! Mirrors [`card_registry`](crate::card_registry)'s shape: the
//! `scenarios` crate (which depends on `game-core`) provides a static
//! [`ScenarioRegistry`] of function pointers, and the host installs it
//! once at startup via
//! [`scenario_registry::install`](crate::scenario_registry::install).
//! The engine, after each `Done` apply outcome, looks up the active
//! scenario's module and asks it whether the new state has resolved.
//!
//! # Why function pointers, not `dyn Trait`?
//!
//! Same reasoning as `CardRegistry`: the surface is small and fixed.
//! Function pointers keep the registry [`Copy`], avoid vtable
//! overhead, and stay `serde`-free at the boundary. Tests construct
//! ad-hoc `ScenarioModule` values with mock function pointers.
//!
//! # Replay safety
//!
//! [`GameState::scenario_id`](crate::state::GameState::scenario_id) is
//! a serializable [`ScenarioId`]; function pointers are not
//! serializable. On reload, the host re-installs `REGISTRY` and the
//! engine looks the module up by id — the action log replays
//! deterministically.

use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::state::GameState;

/// Stable, serializable identifier for a scenario module.
///
/// Newtype around [`String`], mirroring
/// [`CardCode`](crate::state::CardCode). Kept on
/// [`GameState`](crate::state::GameState) so action-log replay can
/// resolve the active scenario module via the registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScenarioId(String);

impl ScenarioId {
    /// Construct a [`ScenarioId`] from any string-like value.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Outcome of a scenario.
///
/// Phase-4 minimal shape. Phase-9 will refine the payloads when the
/// typed campaign-log `Fact` enum and branching scenario sequencing
/// land; the `#[non_exhaustive]` annotation reserves that room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Resolution {
    /// Scenario completed successfully.
    Won {
        /// Per-scenario resolution branch identifier (e.g. `"R1"`,
        /// `"R2"`). The meaning is scenario-local — Phase 9's
        /// `next_scenario` orchestration interprets it.
        id: String,
    },
    /// Scenario ended in defeat.
    Lost {
        /// Human-readable cause for diagnostics. Not semantically
        /// load-bearing today; Phase 9 may swap for a typed enum.
        reason: String,
    },
}

/// Static, host-installed bundle of function pointers for one
/// scenario module.
///
/// Mirrors [`CardRegistry`](crate::card_registry::CardRegistry)'s
/// shape: no `dyn`, no `Box`, [`Copy`]-able.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioModule {
    /// Build the scenario's initial [`GameState`]. Places locations,
    /// populates encounter / act / agenda decks (when those exist),
    /// sets chaos-bag modifiers, etc.
    ///
    /// For the Phase-4 skeleton, the synthetic fixture's `setup`
    /// returns a minimal state with one location and one investigator.
    pub setup: fn() -> GameState,
    /// Pure check called by [`apply`](crate::engine::apply) after each
    /// [`Done`](crate::EngineOutcome::Done) outcome. Returns
    /// `Some(resolution)` when the scenario has resolved.
    ///
    /// **Idempotency note:** the engine has no built-in latch that
    /// stops calling this once a resolution has fired. For the Phase-4
    /// skeleton this is acceptable because `apply_resolution` is a
    /// no-op. Phase 9 will add a guard (likely
    /// `GameState.resolution: Option<Resolution>`) when the first
    /// non-trivial `apply_resolution` lands.
    pub detect_resolution: fn(&GameState) -> Option<Resolution>,
    /// Apply the resolution's effects (XP, trauma, scenario-end
    /// cleanup). Receives the events buffer so changes are observable
    /// to clients.
    ///
    /// For the Phase-4 skeleton, the synthetic fixture's
    /// `apply_resolution` is a no-op. Phase 9 fills in real bodies
    /// once the campaign log lands.
    pub apply_resolution: fn(&Resolution, &mut GameState, &mut Vec<Event>),
}

/// Lookup table of [`ScenarioModule`]s, keyed by [`ScenarioId`].
///
/// The `scenarios` crate exposes a `pub const REGISTRY: ScenarioRegistry`
/// wrapping its own `by_id` lookup; hosts install it once at startup
/// via
/// [`scenario_registry::install`](crate::scenario_registry::install).
#[derive(Debug, Clone, Copy)]
pub struct ScenarioRegistry {
    /// Look up a scenario module by its id. Returns `None` for ids
    /// not known to this registry.
    pub module_for: fn(&ScenarioId) -> Option<&'static ScenarioModule>,
}
```

- [ ] **Step 2: Wire the module into `crates/game-core/src/lib.rs`**

Edit `crates/game-core/src/lib.rs`. Insert `pub mod scenario;` after `pub mod rng;` and before `pub mod state;` (alphabetical-ish, keeping the existing order otherwise). The full module-declaration block becomes:

```rust
pub mod action;
pub mod card_registry;
pub mod engine;
pub mod event;
pub mod rng;
pub mod scenario;
pub mod state;
```

And add a re-export near the existing `pub use card_registry::CardRegistry;` line:

```rust
pub use card_registry::CardRegistry;
pub use scenario::{Resolution, ScenarioId, ScenarioModule, ScenarioRegistry};
```

- [ ] **Step 3: Verify the crate compiles**

Run: `cargo build -p game-core`
Expected: clean build. The new module has no consumers yet but compiles standalone.

- [ ] **Step 4: Run the full clippy-strict check on game-core**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: PASS — the new types use the same derives and doc-comment conventions as `CardRegistry`, which already passes clippy.

---

## Task 3: Add `game_core::scenario_registry` module — `OnceLock`-backed install/current

**Files:**
- Create: `crates/game-core/src/scenario_registry.rs`
- Modify: `crates/game-core/src/lib.rs` (add `pub mod scenario_registry;`)

This task mirrors `crates/game-core/src/card_registry.rs` exactly — same shape, same documentation tone, same test set.

- [ ] **Step 1: Create `crates/game-core/src/scenario_registry.rs`**

Create the file with:

```rust
//! Global scenario-registry binding for engine ↔ scenarios crate
//! lookups.
//!
//! Mirrors [`card_registry`](crate::card_registry): the engine needs
//! to look up a [`ScenarioModule`] by [`ScenarioId`] when checking
//! whether the current state has resolved, but `scenarios` depends on
//! `game-core` and not the other way around. This module bridges the
//! gap with a `OnceLock`-backed global.
//!
//! Hosts (server, test setup) call [`install`] exactly once with the
//! `scenarios::REGISTRY` constant. Engine code calls [`current`] when
//! it needs a lookup and treats `None` as "no scenario behavior wired
//! up; skip the resolution check."
//!
//! # Why function pointers, not `dyn Trait`?
//!
//! Same reasoning as `card_registry`: the lookup interface is small
//! and fixed, the registry stays [`Copy`], and tests can construct
//! ad-hoc mock registries without touching the global.
//!
//! # Test isolation
//!
//! `OnceLock` is process-global, so tests that need a registry
//! installed run in their own integration-test binary (which is its
//! own process). Engine unit tests in `game-core` exercise the
//! resolution-hook logic by **bypassing the global**: they call the
//! engine's `fire_scenario_resolution` helper with a
//! locally-constructed mock [`ScenarioRegistry`]. The one test that
//! exercises the global itself is the idempotent-install test below,
//! which is robust to running alongside other global-touching tests.

use std::sync::OnceLock;

use crate::scenario::ScenarioRegistry;

static REGISTRY: OnceLock<ScenarioRegistry> = OnceLock::new();

/// Install the global scenario registry. Idempotent at the
/// `OnceLock` level: the first call wins; subsequent calls return
/// `Err` with the value the caller passed in.
///
/// Hosts call this once at startup. Tests that need real scenario
/// modules may call it from a `#[ctor]`-style helper or a
/// `LazyLock` initializer; double-install is harmless.
///
/// # Errors
///
/// Returns `Err(registry)` if a registry was already installed,
/// returning the value the caller passed in unchanged.
pub fn install(registry: ScenarioRegistry) -> Result<(), ScenarioRegistry> {
    REGISTRY.set(registry)
}

/// Get the installed registry, or `None` if no registry has been
/// installed yet. Engine code that needs a lookup should call this
/// and treat `None` as "no scenario behavior; skip" — the engine must
/// never panic on missing context.
#[must_use]
pub fn current() -> Option<&'static ScenarioRegistry> {
    REGISTRY.get()
}

#[cfg(test)]
mod tests {
    use super::{ScenarioRegistry, REGISTRY};
    use crate::event::Event;
    use crate::scenario::{Resolution, ScenarioId, ScenarioModule};
    use crate::state::GameState;
    use crate::test_support::TestGame;

    fn empty_state() -> GameState {
        TestGame::new().build()
    }

    fn never_resolves(_state: &GameState) -> Option<Resolution> {
        None
    }

    fn no_op_apply(
        _res: &Resolution,
        _state: &mut GameState,
        _events: &mut Vec<Event>,
    ) {
    }

    static FAKE_MODULE: ScenarioModule = ScenarioModule {
        setup: empty_state,
        detect_resolution: never_resolves,
        apply_resolution: no_op_apply,
    };

    fn fake_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        if id.as_str() == "fake" {
            Some(&FAKE_MODULE)
        } else {
            None
        }
    }

    fn fake_registry() -> ScenarioRegistry {
        ScenarioRegistry {
            module_for: fake_module_for,
        }
    }

    #[test]
    fn module_for_returns_known_id() {
        let reg = fake_registry();
        let id = ScenarioId::new("fake");
        assert!((reg.module_for)(&id).is_some());
    }

    #[test]
    fn module_for_returns_none_for_unknown_id() {
        let reg = fake_registry();
        let id = ScenarioId::new("nonexistent");
        assert!((reg.module_for)(&id).is_none());
    }

    /// Process-global install — must run alongside other
    /// global-touching tests; we observe both outcomes to make the
    /// test robust to scheduling.
    #[test]
    fn install_is_idempotent_and_current_reflects_installed_value() {
        let first_attempt = super::install(fake_registry());
        let installed = super::current().expect("registry should be present after install");
        let id = ScenarioId::new("fake");
        let _ = (installed.module_for)(&id);
        if first_attempt.is_ok() {
            assert!(super::install(fake_registry()).is_err());
        }
        assert!(REGISTRY.get().is_some());
    }
}
```

- [ ] **Step 2: Add `pub mod scenario_registry;` to `lib.rs`**

In `crates/game-core/src/lib.rs`, the module list becomes:

```rust
pub mod action;
pub mod card_registry;
pub mod engine;
pub mod event;
pub mod rng;
pub mod scenario;
pub mod scenario_registry;
pub mod state;
```

No `pub use` for the registry itself (matches how `card_registry` is only re-exported indirectly via `pub use card_registry::CardRegistry;`).

- [ ] **Step 3: Run the new tests**

Run: `cargo test -p game-core --lib scenario_registry::tests`
Expected: 3 tests pass.

- [ ] **Step 4: Commit Tasks 2–3**

```bash
git add crates/game-core/src/scenario.rs crates/game-core/src/scenario_registry.rs crates/game-core/src/lib.rs
git commit -m "$(cat <<'EOF'
engine: scenario-module types and registry slot

Mirrors the CardRegistry pattern: ScenarioId / Resolution /
ScenarioModule / ScenarioRegistry in game_core::scenario, with a
OnceLock-backed install/current in game_core::scenario_registry.
No engine wiring yet; consumers land in subsequent commits.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add `Event::ScenarioResolved` variant

**Files:**
- Modify: `crates/game-core/src/event.rs` (add variant + import for `Resolution`)

- [ ] **Step 1: Add the import at the top of `event.rs`**

Edit `crates/game-core/src/event.rs`. The existing import block is:

```rust
use serde::{Deserialize, Serialize};

use crate::state::{
    CardCode, CardInstanceId, ChaosToken, DefeatCause, EnemyId, InvestigatorId, LocationId, Phase,
    SkillKind, TokenResolution, WindowKind, Zone,
};
```

Add `use crate::scenario::Resolution;` after the existing `use crate::state::{…};` block:

```rust
use serde::{Deserialize, Serialize};

use crate::scenario::Resolution;
use crate::state::{
    CardCode, CardInstanceId, ChaosToken, DefeatCause, EnemyId, InvestigatorId, LocationId, Phase,
    SkillKind, TokenResolution, WindowKind, Zone,
};
```

- [ ] **Step 2: Add the variant at the end of the `Event` enum**

In `crates/game-core/src/event.rs`, immediately before the closing `}` of `pub enum Event`, insert (note the trailing comma after `WindowClosed { kind: WindowKind }`):

```rust
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
    /// `apply_resolution`.
    ScenarioResolved {
        /// The resolution returned by the scenario module.
        resolution: Resolution,
    },
```

- [ ] **Step 3: Verify the crate still compiles**

Run: `cargo build -p game-core`
Expected: clean build. `Event` already carries `#[non_exhaustive]`, so adding a variant is internally non-breaking; all existing matches use `match … { … }` patterns that don't enumerate.

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/event.rs
git commit -m "$(cat <<'EOF'
engine: add Event::ScenarioResolved variant

Carries a Resolution payload. Emitted by apply() after each Done
outcome whose scenario module's detect_resolution returns Some;
wiring lands in the next commit.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Add `GameState.scenario_id` field

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add field on `GameState`)
- Modify: `crates/game-core/src/state/mod.rs` (no new re-export needed; `ScenarioId` lives outside `state`)

- [ ] **Step 1: Add the `scenario_id` field on `GameState`**

In `crates/game-core/src/state/game_state.rs`, the `GameState` struct currently ends with `open_windows: Vec<OpenWindow>`. Append `scenario_id` as the last field. The full struct now ends:

```rust
    pub open_windows: Vec<OpenWindow>,
    /// Identifier of the scenario this state belongs to, if any.
    ///
    /// `None` for tests and fixtures that don't care about scenario
    /// resolution; in that case the engine's post-apply resolution
    /// hook short-circuits. `Some(id)` is the normal case: the
    /// engine looks up the module via
    /// [`scenario_registry::current`](crate::scenario_registry::current)
    /// and asks it whether the new state has resolved.
    ///
    /// Serializable so action-log replay reproduces the lookup
    /// deterministically across host restarts.
    pub scenario_id: Option<crate::scenario::ScenarioId>,
}
```

- [ ] **Step 2: Verify game-core still compiles (TestGame::build will need updating; that's Task 6)**

Run: `cargo build -p game-core 2>&1 | head -30`
Expected: compile error pointing at `TestGame::build`'s struct-literal `GameState { … }` missing the new field. This is what Task 6 fixes; do not fix here.

---

## Task 6: Wire `scenario_id` through `TestGame` builder

**Files:**
- Modify: `crates/game-core/src/test_support/builder.rs`

- [ ] **Step 1: Add the field to the `TestGame` struct**

In `crates/game-core/src/test_support/builder.rs`, locate the `TestGame` struct (around lines 41-54). The current field list ends with `open_windows: Vec<OpenWindow>`. Add `scenario_id: Option<ScenarioId>` as the last field. The full struct becomes:

```rust
pub struct TestGame {
    investigators: BTreeMap<InvestigatorId, Investigator>,
    locations: BTreeMap<crate::state::LocationId, Location>,
    enemies: BTreeMap<EnemyId, Enemy>,
    chaos_bag: ChaosBag,
    token_modifiers: TokenModifiers,
    phase: Phase,
    round: u32,
    active_investigator: Option<InvestigatorId>,
    turn_order: Vec<InvestigatorId>,
    rng: RngState,
    mulligan_window: bool,
    open_windows: Vec<OpenWindow>,
    scenario_id: Option<crate::scenario::ScenarioId>,
}
```

Add the import at the top of the file (alongside the existing `use crate::state::{…};` block):

```rust
use crate::scenario::ScenarioId;
```

(Or fold it into the existing `use crate::state::{…};` re-export path — but `ScenarioId` lives outside `state`, so it gets its own `use` line.)

- [ ] **Step 2: Default the field in `TestGame::new`**

In the `TestGame::new()` body (around lines 61-76), append `scenario_id: None,` to the struct literal. It becomes:

```rust
    pub fn new() -> Self {
        Self {
            investigators: BTreeMap::new(),
            locations: BTreeMap::new(),
            enemies: BTreeMap::new(),
            chaos_bag: ChaosBag::new([]),
            token_modifiers: TokenModifiers::default(),
            phase: Phase::Mythos,
            round: 0,
            active_investigator: None,
            turn_order: Vec::new(),
            rng: RngState::new(0),
            mulligan_window: false,
            open_windows: Vec::new(),
            scenario_id: None,
        }
    }
```

- [ ] **Step 3: Add the `with_scenario_id` setter**

Insert a new setter after `with_open_window` (around line 212). Match the existing setter style:

```rust
    /// Set the scenario id this state belongs to. `None` (the
    /// default from [`TestGame::new`]) means the engine's post-apply
    /// resolution hook will short-circuit; passing a `ScenarioId`
    /// means a `ScenarioRegistry` capable of resolving it must be
    /// installed (or the resolution lookup will silently no-op when
    /// `module_for` returns `None`).
    pub fn with_scenario_id(mut self, id: ScenarioId) -> Self {
        self.scenario_id = Some(id);
        self
    }
```

- [ ] **Step 4: Thread `scenario_id` through `TestGame::build`**

In `TestGame::build()` (around lines 235-253), the returned `GameState` struct literal currently ends:

```rust
            mulligan_window: self.mulligan_window,
            next_card_instance_id: 0,
            pending_skill_modifiers: Vec::new(),
            in_flight_skill_test: None,
            open_windows: self.open_windows,
        }
    }
```

Add `scenario_id: self.scenario_id` to the literal:

```rust
            mulligan_window: self.mulligan_window,
            next_card_instance_id: 0,
            pending_skill_modifiers: Vec::new(),
            in_flight_skill_test: None,
            open_windows: self.open_windows,
            scenario_id: self.scenario_id,
        }
    }
```

- [ ] **Step 5: Verify game-core compiles cleanly**

Run: `cargo build -p game-core`
Expected: clean build. Field-add only; no existing test should break.

- [ ] **Step 6: Run the full game-core test suite**

Run: `cargo test -p game-core --lib`
Expected: every existing test still passes (the new field defaults to `None`, which is a no-op for any test that doesn't opt in).

- [ ] **Step 7: Commit Tasks 5–6**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/test_support/builder.rs
git commit -m "$(cat <<'EOF'
engine: GameState.scenario_id + TestGame.with_scenario_id

Option<ScenarioId> on GameState; None is the default and a no-op
for the engine's post-apply resolution hook. TestGame gets the
matching .with_scenario_id setter; the field threads through .build.
Existing tests are unaffected (default = None).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Engine post-dispatch hook — `fire_scenario_resolution` helper + `apply()` wiring + unit tests

**Files:**
- Modify: `crates/game-core/src/engine/mod.rs`

This is the load-bearing change. The helper is intentionally parameterized so unit tests can mock the registry without touching the process-global `OnceLock` — the test pattern called out in the spec.

- [ ] **Step 1: Write failing tests for the post-dispatch hook**

In `crates/game-core/src/engine/mod.rs`, locate the existing `#[cfg(test)] mod tests { … }` block (starts around line 87). Append the following inside that module, after the existing tests:

```rust
    use crate::scenario::{Resolution, ScenarioId, ScenarioModule, ScenarioRegistry};

    /// Mock scenario module that always returns `Won { id: "test" }`.
    fn always_wins(_state: &crate::state::GameState) -> Option<Resolution> {
        Some(Resolution::Won { id: "test".into() })
    }

    /// Mock scenario module that never resolves.
    fn never_resolves(_state: &crate::state::GameState) -> Option<Resolution> {
        None
    }

    /// Empty setup; tests build state via TestGame.
    fn unused_setup() -> crate::state::GameState {
        TestGame::new().build()
    }

    /// No-op apply_resolution; tests assert on the emitted event only.
    fn no_op_apply(
        _res: &Resolution,
        _state: &mut crate::state::GameState,
        _events: &mut Vec<Event>,
    ) {
    }

    static ALWAYS_WINS_MODULE: ScenarioModule = ScenarioModule {
        setup: unused_setup,
        detect_resolution: always_wins,
        apply_resolution: no_op_apply,
    };

    static NEVER_RESOLVES_MODULE: ScenarioModule = ScenarioModule {
        setup: unused_setup,
        detect_resolution: never_resolves,
        apply_resolution: no_op_apply,
    };

    fn always_wins_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        if id.as_str() == "wins" {
            Some(&ALWAYS_WINS_MODULE)
        } else {
            None
        }
    }

    fn never_resolves_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        if id.as_str() == "neutral" {
            Some(&NEVER_RESOLVES_MODULE)
        } else {
            None
        }
    }

    #[test]
    fn scenario_resolution_fires_when_module_returns_some() {
        let id = InvestigatorId(1);
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_scenario_id(ScenarioId::new("wins"))
            .build();
        let reg = ScenarioRegistry { module_for: always_wins_module_for };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::StartScenario),
            Some(&reg),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "test"
        );
    }

    #[test]
    fn scenario_resolution_is_skipped_when_module_returns_none() {
        let id = InvestigatorId(1);
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_scenario_id(ScenarioId::new("neutral"))
            .build();
        let reg = ScenarioRegistry { module_for: never_resolves_module_for };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::StartScenario),
            Some(&reg),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_no_event!(result.events, Event::ScenarioResolved { .. });
    }

    #[test]
    fn scenario_resolution_is_skipped_when_scenario_id_is_none() {
        let id = InvestigatorId(1);
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .build();
        let reg = ScenarioRegistry { module_for: always_wins_module_for };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::StartScenario),
            Some(&reg),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_no_event!(result.events, Event::ScenarioResolved { .. });
    }

    #[test]
    fn scenario_resolution_is_skipped_when_no_registry_installed() {
        let id = InvestigatorId(1);
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_scenario_id(ScenarioId::new("wins"))
            .build();
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::StartScenario),
            None,
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_no_event!(result.events, Event::ScenarioResolved { .. });
    }

    #[test]
    fn scenario_resolution_is_skipped_on_rejected_outcome() {
        let state = TestGame::new()
            .with_round(7) // already in progress -> StartScenario rejects
            .with_scenario_id(ScenarioId::new("wins"))
            .build();
        let reg = ScenarioRegistry { module_for: always_wins_module_for };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::StartScenario),
            Some(&reg),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }
```

- [ ] **Step 2: Confirm the new tests fail to compile**

Run: `cargo test -p game-core --lib engine::tests::scenario_resolution_ 2>&1 | head -20`
Expected: compile error — `apply_with_scenario_registry` is not defined.

- [ ] **Step 3: Add the helper and wire `apply()` to call it**

In `crates/game-core/src/engine/mod.rs`, add the import for `ScenarioRegistry` near the existing imports:

```rust
use crate::action::Action;
use crate::event::Event;
use crate::scenario::ScenarioRegistry;
use crate::state::GameState;
```

Then replace the existing `apply()` function. The current body is:

```rust
pub fn apply(state: GameState, action: Action) -> ApplyResult {
    let mut state = state;
    let mut events = Vec::new();
    let outcome = match action {
        Action::Player(p) => dispatch::apply_player_action(&mut state, &mut events, &p),
        Action::Engine(e) => dispatch::apply_engine_record(&mut state, &mut events, &e),
    };
    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        events.clear();
    }
    ApplyResult {
        state,
        events,
        outcome,
    }
}
```

Replace with:

```rust
pub fn apply(state: GameState, action: Action) -> ApplyResult {
    apply_with_scenario_registry(state, action, crate::scenario_registry::current())
}

/// Apply a single action with an explicit [`ScenarioRegistry`].
///
/// `apply` is the production entry point and reads the registry from
/// the global [`scenario_registry::current`](crate::scenario_registry::current).
/// This variant exists so engine unit tests can drive the post-apply
/// resolution hook against a locally-constructed mock registry
/// without touching the process-global `OnceLock`.
///
/// The same `Done`-only firing rule applies regardless of how the
/// registry is supplied: a `Rejected` outcome clears events and
/// skips the hook; an `AwaitingInput` outcome means the engine is
/// paused mid-resolution and the scenario module would see a
/// potentially inconsistent state, so the hook is skipped there too.
pub fn apply_with_scenario_registry(
    state: GameState,
    action: Action,
    registry: Option<&ScenarioRegistry>,
) -> ApplyResult {
    let mut state = state;
    let mut events = Vec::new();
    let outcome = match action {
        Action::Player(p) => dispatch::apply_player_action(&mut state, &mut events, &p),
        Action::Engine(e) => dispatch::apply_engine_record(&mut state, &mut events, &e),
    };
    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        // Belt-and-suspenders: handlers are expected to validate before
        // mutating, so events should already be empty here. Clear
        // anyway in case a handler accidentally pushed before bailing.
        events.clear();
    } else if matches!(outcome, EngineOutcome::Done) {
        fire_scenario_resolution(&mut state, &mut events, registry);
    }
    ApplyResult {
        state,
        events,
        outcome,
    }
}

/// Post-dispatch hook: if the state belongs to a scenario whose
/// module is resolvable in `registry`, ask it whether the new state
/// has resolved. On `Some(res)`, emit
/// [`Event::ScenarioResolved`] and call the module's
/// `apply_resolution`.
///
/// Idempotency note: there is no engine-side latch on whether a
/// resolution has already fired. A scenario whose `detect_resolution`
/// keeps returning `Some` will keep emitting `ScenarioResolved` on
/// every subsequent apply. Phase 9 adds the guard alongside the
/// first non-trivial `apply_resolution` body.
fn fire_scenario_resolution(
    state: &mut GameState,
    events: &mut Vec<Event>,
    registry: Option<&ScenarioRegistry>,
) {
    let Some(id) = state.scenario_id.as_ref() else { return };
    let Some(reg) = registry else { return };
    let Some(module) = (reg.module_for)(id) else { return };
    let Some(resolution) = (module.detect_resolution)(state) else { return };
    events.push(Event::ScenarioResolved {
        resolution: resolution.clone(),
    });
    (module.apply_resolution)(&resolution, state, events);
}
```

- [ ] **Step 4: Update the public re-export**

In `crates/game-core/src/lib.rs`, the existing engine re-export is:

```rust
pub use engine::{apply, ApplyResult, EngineOutcome, InputRequest, ResumeToken};
```

Expand it to also re-export the parameterized helper (useful for downstream test code that wants the same mocking pattern):

```rust
pub use engine::{
    apply, apply_with_scenario_registry, ApplyResult, EngineOutcome, InputRequest, ResumeToken,
};
```

- [ ] **Step 5: Run the new tests**

Run: `cargo test -p game-core --lib engine::tests::scenario_resolution_`
Expected: all 5 tests pass.

- [ ] **Step 6: Run the full game-core test suite (regression check)**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: every existing test still passes; new tests pass.

- [ ] **Step 7: Run clippy-strict on game-core**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/mod.rs crates/game-core/src/lib.rs
git commit -m "$(cat <<'EOF'
engine: post-apply scenario resolution hook

Adds fire_scenario_resolution helper plus an apply_with_scenario_registry
variant that takes the registry as a parameter. apply() reads from
the process-global; tests use the parameterized variant with a mock
registry so OnceLock contention is irrelevant. Hook fires only on
Done outcomes; emits Event::ScenarioResolved and calls the module's
apply_resolution when detect_resolution returns Some.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: `scenarios` crate — feature flag + synthetic test fixture

**Files:**
- Modify: `crates/scenarios/Cargo.toml` (add `[features]` block)
- Create: `crates/scenarios/src/test_fixtures/mod.rs`
- Create: `crates/scenarios/src/test_fixtures/synthetic.rs`

- [ ] **Step 1: Add the `test_fixtures` feature**

Edit `crates/scenarios/Cargo.toml`. After the `[dependencies]` block, add:

```toml
[features]
# Compile the `test_fixtures` module (synthetic / minimal scenarios)
# into the crate. Used by `crates/scenarios/tests/` integration tests
# (always enabled there via `cfg(test)`) and may be enabled by
# downstream crates (server / web integration tests) that want the
# fixture without writing their own.
test_fixtures = []
```

The final manifest:

```toml
[package]
name = "scenarios"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
description = "Eldritch scenario modules and campaign orchestrators."

[lints]
workspace = true

[dependencies]
game-core = { path = "../game-core" }
cards = { path = "../cards" }

[features]
test_fixtures = []
```

- [ ] **Step 2: Create the `test_fixtures` module index**

Create `crates/scenarios/src/test_fixtures/mod.rs`:

```rust
//! Synthetic / minimal scenario fixtures.
//!
//! These exist only to exercise the engine's scenario-module wiring;
//! they are *not* part of any shipped campaign. Gated behind
//! `cfg(any(test, feature = "test_fixtures"))` at the crate root so
//! they never ship in a release build.

pub mod synthetic;
```

- [ ] **Step 3: Create the synthetic scenario module**

Create `crates/scenarios/src/test_fixtures/synthetic.rs`:

```rust
//! The minimum a scenario needs to exist.
//!
//! Teaching example — a Phase-7 implementer reading this should see
//! the shape of a scenario module without having to grok any real
//! scenario's content. One investigator, one location, a
//! one-line resolution predicate.

use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId, ScenarioModule};
use game_core::state::{GameState, InvestigatorId, Phase};
use game_core::test_support::{test_investigator, test_location, TestGame};

/// String id used to look this module up in
/// [`crate::REGISTRY`](crate::REGISTRY).
pub const ID: &str = "synthetic";

/// Build the initial [`GameState`] for this fixture: one
/// investigator, one location, scenario_id set, turn_order
/// populated. Phase = Mythos, round = 0 — ready for
/// [`PlayerAction::StartScenario`](game_core::PlayerAction::StartScenario).
pub fn setup() -> GameState {
    TestGame::new()
        .with_investigator(test_investigator(1))
        .with_location(test_location(10, "Demo Location"))
        .with_turn_order([InvestigatorId(1)])
        .with_scenario_id(ScenarioId::new(ID))
        .build()
}

/// Resolves with [`Resolution::Won`] once the engine has stepped
/// past `StartScenario`'s automatic Mythos skip into
/// [`Phase::Investigation`] with `round >= 1`.
///
/// One-liner deliberately: the integration test asserts this fires
/// after a single `StartScenario` apply.
#[must_use]
pub fn detect_resolution(state: &GameState) -> Option<Resolution> {
    if state.phase == Phase::Investigation && state.round >= 1 {
        Some(Resolution::Won { id: "demo".into() })
    } else {
        None
    }
}

/// No-op. Phase 9 fills in real bodies once campaign-log XP / trauma
/// application lands.
pub fn apply_resolution(
    _resolution: &Resolution,
    _state: &mut GameState,
    _events: &mut Vec<Event>,
) {
}

/// The [`ScenarioModule`] value for the synthetic fixture. Bundles
/// the three `fn` pointers above; referenced from
/// [`crate::module_for`](crate::module_for).
pub const MODULE: ScenarioModule = ScenarioModule {
    setup,
    detect_resolution,
    apply_resolution,
};
```

- [ ] **Step 4: Verify the fixture compiles under the feature flag**

Run: `cargo build -p scenarios --features test_fixtures`
Expected: clean build. (The module isn't yet referenced from `lib.rs`; that's Task 9.)

---

## Task 9: `scenarios::REGISTRY` const + module wiring in `lib.rs`

**Files:**
- Modify: `crates/scenarios/src/lib.rs`

- [ ] **Step 1: Rewrite `lib.rs` to expose `test_fixtures` and `REGISTRY`**

Replace the entire content of `crates/scenarios/src/lib.rs` with:

```rust
//! Scenarios and campaigns for Eldritch.
//!
//! Each scenario is a Rust module exposing `setup`,
//! `detect_resolution`, and `apply_resolution`. Campaigns
//! orchestrate scenarios with branching rules and a typed campaign
//! log.
//!
//! # Engine integration
//!
//! The engine (in `game-core`) can't depend on this crate (cycle).
//! Engine code that needs a scenario lookup goes through
//! [`game_core::scenario_registry`]. This crate exposes [`REGISTRY`]
//! as a ready-made [`game_core::ScenarioRegistry`] value that the
//! host installs via
//! [`game_core::scenario_registry::install`](game_core::scenario_registry::install)
//! before running actions that touch scenario data.
//!
//! Phase-4 ships one module: the
//! [`synthetic`](test_fixtures::synthetic) fixture used by the
//! engine's resolution-hook integration test. Real scenarios (The
//! Gathering, Dunwich, …) land in subsequent phases.

#[cfg(any(test, feature = "test_fixtures"))]
pub mod test_fixtures;

#[cfg(any(test, feature = "test_fixtures"))]
use game_core::scenario::{ScenarioId, ScenarioModule, ScenarioRegistry};

/// Look up a scenario module by id. Returns `None` for ids not
/// known to this crate.
///
/// Gated behind `test_fixtures` for now — once a real scenario
/// (Phase 7 Gathering) lands, this becomes the unconditional
/// implementation.
#[cfg(any(test, feature = "test_fixtures"))]
#[must_use]
pub fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    match id.as_str() {
        test_fixtures::synthetic::ID => Some(&test_fixtures::synthetic::MODULE),
        _ => None,
    }
}

/// Ready-made [`ScenarioRegistry`] backed by this crate's scenario
/// modules. The host installs it once at startup with
/// [`game_core::scenario_registry::install`](game_core::scenario_registry::install).
#[cfg(any(test, feature = "test_fixtures"))]
pub const REGISTRY: ScenarioRegistry = ScenarioRegistry {
    module_for,
};

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::scenario::ScenarioId;

    #[test]
    fn module_for_resolves_synthetic() {
        let id = ScenarioId::new(test_fixtures::synthetic::ID);
        assert!(module_for(&id).is_some());
    }

    #[test]
    fn module_for_returns_none_for_unknown() {
        let id = ScenarioId::new("not-a-real-scenario");
        assert!(module_for(&id).is_none());
    }

    #[test]
    fn registry_dispatches_to_module_for() {
        let id = ScenarioId::new(test_fixtures::synthetic::ID);
        assert!((REGISTRY.module_for)(&id).is_some());
    }
}
```

- [ ] **Step 2: Run scenarios tests**

Run: `cargo test -p scenarios`
Expected: 3 tests pass (the `#[cfg(test)]` automatic-enable of `test_fixtures` is via cfg(test)). If they don't compile because the cfg gating misses the `cfg(test)` form, the `#[cfg(any(test, feature = "test_fixtures"))]` annotation on `test_fixtures` and friends should already cover it — verify the module is visible inside `tests`.

- [ ] **Step 3: Run scenarios tests with the explicit feature, too**

Run: `cargo test -p scenarios --features test_fixtures`
Expected: same 3 tests pass.

- [ ] **Step 4: Commit Tasks 8–9**

```bash
git add crates/scenarios/Cargo.toml crates/scenarios/src/lib.rs crates/scenarios/src/test_fixtures/
git commit -m "$(cat <<'EOF'
scenarios: synthetic test fixture + REGISTRY const

One stub scenario module — single location, single investigator,
resolves Won when the engine has stepped to Investigation with
round >= 1. Gated behind cfg(any(test, feature = "test_fixtures"))
so it never ships in a release build. lib.rs exposes a const
REGISTRY mirroring cards::REGISTRY.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Integration test — `scenarios::tests::synthetic_resolution`

**Files:**
- Create: `crates/scenarios/tests/synthetic_resolution.rs`

This file is its own cargo-test binary (separate process), so the `scenario_registry::install` call inside doesn't collide with anything else.

- [ ] **Step 1: Create the integration test**

Create `crates/scenarios/tests/synthetic_resolution.rs`:

```rust
//! End-to-end test of the scenario-module wiring with the real
//! `scenarios::REGISTRY` installed.
//!
//! Drives `PlayerAction::StartScenario` against the synthetic
//! fixture and asserts `Event::ScenarioResolved` fires. Lives in
//! `crates/scenarios/tests/` rather than `game-core/src/engine/`
//! because:
//!
//! - The engine crate can't depend on `scenarios` (cycle direction
//!   is `game-core ← scenarios`).
//! - `scenario_registry::install` is process-global; an integration
//!   test binary gets its own process, so this install doesn't
//!   collide with `game-core`'s unit tests (which exercise the
//!   parameterized `apply_with_scenario_registry` helper instead).

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::state::Phase;
use game_core::{assert_event, Action, PlayerAction};
use scenarios::REGISTRY;

static INSTALL: Once = Once::new();

fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
    });
}

#[test]
fn synthetic_scenario_resolves_after_start_scenario() {
    install_registry();
    let state = scenarios::test_fixtures::synthetic::setup();
    let result = apply(state, Action::Player(PlayerAction::StartScenario));

    // StartScenario steps Mythos -> Investigation and bumps round to 1;
    // the synthetic fixture's detect_resolution fires on that condition.
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.phase, Phase::Investigation);
    assert_eq!(result.state.round, 1);
    assert_event!(
        result.events,
        Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "demo"
    );
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p scenarios --test synthetic_resolution`
Expected: 1 test passes. (Default cargo-test feature gating includes `[features] test_fixtures = []` automatically? No — features default off. The `#[cfg(any(test, feature = "test_fixtures"))]` gate uses **either** `cfg(test)` OR the feature; for `tests/*.rs` binaries, cargo compiles the dependency target with `cfg(test)` for the crate-under-test, so the gate is active. If this turns out to be wrong, fall back to adding `default = ["test_fixtures"]` in the `[features]` block.)

- [ ] **Step 3: If Step 2 fails because the fixture module isn't visible from `tests/`, enable the feature by default**

If `cargo test -p scenarios --test synthetic_resolution` fails with `unresolved import scenarios::test_fixtures` or `scenarios::REGISTRY`, edit `crates/scenarios/Cargo.toml`'s `[features]` block to set a default:

```toml
[features]
default = ["test_fixtures"]
test_fixtures = []
```

Then re-run `cargo test -p scenarios --test synthetic_resolution`. Expected: PASS.

(The `cfg(test)` form of the gate is only active when *this* crate is being compiled for its own tests. When `tests/*.rs` binaries are built, the `scenarios` crate itself is compiled as a *normal dependency* of the test binary, **not** with `cfg(test)`. So we do need either `default = ["test_fixtures"]` or `required-features = ["test_fixtures"]` on the `[[test]]` entry. The simpler path is `default = ["test_fixtures"]` since this crate currently has no production consumer that would mind.)

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/tests/synthetic_resolution.rs crates/scenarios/Cargo.toml
git commit -m "$(cat <<'EOF'
scenarios: end-to-end resolution-hook integration test

Installs scenarios::REGISTRY and drives PlayerAction::StartScenario
against the synthetic fixture, asserting Event::ScenarioResolved
fires with the expected payload. Also confirms StartScenario lands
the state at Phase::Investigation / round=1 — the predicate that
triggered the resolution.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Run the full CI-equivalent gauntlet locally

**Files:** none modified (verification only).

CLAUDE.md PR procedure step 1: match CI's strict flags exactly. Plain `cargo test` won't catch broken intra-doc links or strict clippy lints.

- [ ] **Step 1: Run the test job**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: every test in every crate passes; no warnings.

- [ ] **Step 2: Run the clippy job**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS.

- [ ] **Step 3: Run the fmt job**

Run: `cargo fmt --check`
Expected: clean.

- [ ] **Step 4: Run the doc job**

Run: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
Expected: clean. Watch in particular for intra-doc-link errors — the new types reference `card_registry`, `scenario_registry`, `ScenarioRegistry`, `Event::ScenarioResolved`, etc. across module boundaries. Any `[BrokenLink]` line is a failure even though `cargo test` would pass.

- [ ] **Step 5: Run the wasm-build job**

Run: `cargo build -p web --target wasm32-unknown-unknown`
Expected: clean. The web crate doesn't yet use scenarios, but a transitive `serde` derive that fails in `wasm32` is the kind of regression this catches.

- [ ] **Step 6: If anything fails, fix and re-run before continuing**

Fix-then-re-run is the pattern. Do not move to the phase-doc update or PR opening with red CI locally.

---

## Task 12: Update phase-4 doc — `#74` to Closed, Arc row flip, Decision, drop settled question

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

Per CLAUDE.md PR procedure step 7: this is the **final commit on the branch**, made only after CI is green locally and review-driven fixes (if any) have already landed.

- [ ] **Step 1: Update the Status section**

In `docs/phases/phase-4-scenario-plumbing.md`, the current Status line is:

```
🟡 In progress. Design pass complete 2026-05-21. First PR (`#103` unified window stack) merged 2026-05-21 as PR #129. Remaining: `#74`, `#72`, `#126`, `#127`, `#69`, `#70`, `#71`, `#128`, `#73`.
```

Change to (PR number will be known by the time this step runs — substitute `<this-PR>`):

```
🟡 In progress. Design pass complete 2026-05-21. First two PRs merged: `#103` unified window stack as PR #129 and `#74` ScenarioModule skeleton as PR #<this-PR>. Remaining: `#72`, `#126`, `#127`, `#69`, `#70`, `#71`, `#128`, `#73`.
```

- [ ] **Step 2: Move `#74` from the Issues table's open rows to a Closed section**

The current Issues table has `#74` as the second row, in the open section. Move it to a new (or existing) Closed table at the bottom of the Issues section. If no Closed table exists yet, add one with the heading `### Closed` and a single row:

```markdown
### Closed

| # | Title | PR | Notes |
|---|---|---|---|
| `#103` | unified window stack (player + reaction) | #129 | Foundational refactor of #52 machinery; ships unified `open_windows` stack. |
| `#74` | scenario module skeleton | #<this-PR> | ScenarioId / Resolution / ScenarioModule / ScenarioRegistry in game-core; synthetic fixture in scenarios; engine post-apply hook with parameterized helper for test mocking. |
```

(`#103` moves there at the same time if it wasn't already migrated in its own PR — verify against the current state of the doc; if `#103` is already in Closed, just add the `#74` row.)

- [ ] **Step 3: Flip the Arc / Ordering row**

The Ordering table currently has row 2 as `#74 ScenarioModule + registry + synthetic fixture stub` with no PR marker. Change to:

```
| 2 | `#74` `ScenarioModule` + registry + synthetic fixture stub | ✅ PR #<this-PR>. Defines the shape every later issue conforms to. Synthetic fixture: 1 location, 1 investigator, one-line resolution predicate. Engine learns to call `detect_resolution` post-`apply`. |
```

- [ ] **Step 4: Add a Decision entry for the parameterized-helper pattern**

Add to the **Decisions made** section (which currently spans 2026-05-21 design-pass entries plus the #103 PR-#129 decisions). Append:

```markdown
- **Engine resolution hook is a parameterized helper (`#74`, PR #<this-PR>).** `apply()` is a one-liner over `apply_with_scenario_registry(state, action, scenario_registry::current())`. Engine unit tests pass a locally-constructed mock `ScenarioRegistry` to the parameterized variant; the process-global `OnceLock` is only touched by one test (the idempotent-install test in `scenario_registry`) and by the `scenarios::tests::synthetic_resolution` integration test. Pattern is the recommended shape for future engine ↔ registry interactions where unit tests would otherwise contend on `OnceLock`.
- **`Resolution` is `Won { id: String } / Lost { reason: String }` (`#74`, PR #<this-PR>).** String payloads stand in for Phase-9's typed campaign-log `Fact` enum. Both variants kept `#[non_exhaustive]` so Phase-9 can extend without breaking Phase-4 consumers.
- **`apply_resolution` is called by the engine right after `ScenarioResolved` (`#74`, PR #<this-PR>).** Same `apply()` call, same events buffer. Action-log replay reproduces XP / trauma changes deterministically. Idempotency latch is deferred — see open question below.
```

- [ ] **Step 5: Remove the settled "`detect_resolution` polling frequency" open question**

The Open questions section currently has:

```
- **`detect_resolution` polling frequency.** Currently "after each `apply`." Correct but potentially expensive at scale. Defer perf concern until a real scenario is observable; flag inline like `#52`'s trigger-indexing deferral.
```

Remove this bullet — the answer is settled by PR #<this-PR>: fires on `Done` outcomes only. The perf concern is captured in code comments, not in the doc.

- [ ] **Step 6: Add the new idempotency open question**

Add to the Open questions section:

```markdown
- **Resolution-fired idempotency latch.** `apply()` re-calls `detect_resolution` on every `Done` outcome with no engine-side guard. Acceptable for Phase 4 (synthetic fixture's `apply_resolution` is a no-op) but the first real `apply_resolution` (Phase 9 XP / trauma) will stack effects unless we add a latch. Likely shape: `GameState.resolution: Option<Resolution>` checked at the top of `fire_scenario_resolution`. Defer until the first real scenario module forces it.
```

- [ ] **Step 7: Verify the doc still renders sensibly**

Run: `mdcat docs/phases/phase-4-scenario-plumbing.md 2>/dev/null | head -50` *(if mdcat is unavailable, just `head -50` the file)*
Expected: section headings still present, Status line reflects two-PR progress, Closed table includes `#74`, Decisions has the three new entries, Open questions has the idempotency latch.

- [ ] **Step 8: Commit the phase-doc update**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: phase-4 — close #74, record ScenarioModule decisions

Moves #74 to Closed (PR #<this-PR>), flips the Arc row, adds three
Decisions (parameterized helper, Resolution shape, auto apply_resolution),
drops the settled detect_resolution-polling-frequency question, and
files the new idempotency-latch open question for Phase 9.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: Open PR, watch CI, spawn review-agent in parallel

**Files:** none modified (process step).

Per CLAUDE.md PR procedure steps 3–5: this is the standard PR-open protocol.

- [ ] **Step 1: Push the branch**

Run: `git push -u origin engine/scenario-module-skeleton`

- [ ] **Step 2: Open the PR with the standard template**

Run:

```bash
gh pr create --title "engine: scenario module skeleton (#74)" --body "$(cat <<'EOF'
## Summary

- Adds `ScenarioId`, `Resolution`, `ScenarioModule`, `ScenarioRegistry` to `game-core::scenario` plus the `OnceLock`-backed install/current pair in `scenario_registry`. Mirrors `CardRegistry`'s shape exactly.
- Adds `Event::ScenarioResolved { resolution: Resolution }` and `GameState.scenario_id: Option<ScenarioId>`.
- Wires `apply()` to call `detect_resolution` after each `Done` outcome via a `fire_scenario_resolution` helper; engine unit tests use a parameterized `apply_with_scenario_registry` to mock the registry without touching the process-global `OnceLock`.
- Ships one synthetic test fixture in `crates/scenarios/src/test_fixtures/synthetic.rs` (gated `cfg(any(test, feature = "test_fixtures"))`) and exposes `pub const REGISTRY: ScenarioRegistry` from `scenarios::lib`.
- Integration test in `crates/scenarios/tests/synthetic_resolution.rs` exercises the full path (registry install → `StartScenario` apply → `Event::ScenarioResolved` assertion).

Phase doc updated to move `#74` to Closed, flip the Arc row, capture decisions, and replace the now-settled `detect_resolution` open question with a new one tracking the post-Phase-9 idempotency latch.

## Design decisions worth flagging

- **Parameterized `apply_with_scenario_registry` helper.** Engine unit tests for the resolution hook contended unavoidably with `OnceLock` if the only path was through the global. Splitting `apply()` into a one-liner over an inner helper that takes `Option<&ScenarioRegistry>` makes the hook logic testable in isolation; the global is exercised by the integration test and the idempotent-install test only.
- **Engine auto-calls `apply_resolution`** right after emitting `ScenarioResolved`, all inside the same `apply()` invocation. Replay reproduces XP / trauma deterministically without needing a separate `ApplyResolution` action variant.
- **No idempotency latch yet.** A scenario whose `detect_resolution` keeps returning `Some` would re-fire `ScenarioResolved` forever — acceptable for the skeleton because `apply_resolution` is a no-op. Captured as a new Phase-4 open question to be resolved when the first real `apply_resolution` lands (likely Phase 9).
- **`test_fixtures` defaults on** if the integration test compilation otherwise can't see the fixture from `tests/*.rs` (cargo's cfg(test) doesn't propagate to dev-dependency test binaries). Documented inline in the task that flipped this.

Closes #74.

## Test plan

- [ ] `RUSTFLAGS="-D warnings" cargo test --all --all-features` passes locally.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo fmt --check` clean.
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` clean.
- [ ] `cargo build -p web --target wasm32-unknown-unknown` clean.
- [ ] All 5 CI jobs green on the PR.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Capture the PR number from the URL printed by `gh pr create` — call it `$PR`.

- [ ] **Step 3: Watch CI in the background**

Run: `gh pr checks $PR --watch` (use the Bash `run_in_background: true` option).

- [ ] **Step 4: Spawn the review-agent in parallel**

Per CLAUDE.md PR procedure step 4 and the user's `feedback_pr_review_process` memory: launch the `review-agent` subagent concurrently with the CI watch. Brief the agent with:

- The PR number and branch name.
- The design-decision paragraph from the PR body (parameterized helper, auto-apply pattern, no idempotency latch yet, test_fixtures default-on).
- What's in scope: the new files + the engine wiring.
- What's intentionally out of scope: real scenarios (Phase 7), campaign log (Phase 9), idempotency latch, server / web host installs.

- [ ] **Step 5: Surface CI + review findings to the user**

Per CLAUDE.md PR procedure step 5 and the user's `feedback_present_review_agent_notes` memory: present the review-agent's findings verbatim (severity-bucketed) **even if CI fails**, then ask the user how to proceed.

---

## Task 14: Address CI / review feedback (if any), then merge

**Files:** as needed.

Per CLAUDE.md PR procedure steps 6 and 8.

- [ ] **Step 1: Fold review-driven fixes into follow-up commits on the same branch**

Do not amend / force-push unless the user requests it. Each fix is a new commit. If the fix lands a structural change that invalidates a Decision entry already in the phase doc, **re-do the phase-doc update at the very end** (after the user approves the merge) rather than mid-stream.

- [ ] **Step 2: Re-run CI locally for any non-trivial fix**

Same five commands as Task 11.

- [ ] **Step 3: Once CI is green and review concerns are resolved, ask the user for the merge call**

Do not merge without explicit user approval (per CLAUDE.md step 8).

- [ ] **Step 4: After user approval, squash-merge**

Run: `gh pr merge $PR --squash --delete-branch`

- [ ] **Step 5: Verify the issue auto-closed and sync `main`**

Run:
```bash
gh issue view 74 --json state
git checkout main
git pull
```

Expected: issue state is `CLOSED`; local `main` is up to date with the new merge commit.

---

## Self-review

I read the plan back against the spec.

**Spec coverage:**

- ✅ Crate layering — Tasks 2–3 (game-core types + registry) + 8–9 (scenarios crate values).
- ✅ Data shape (ScenarioId, Resolution, ScenarioModule, ScenarioRegistry) — Task 2.
- ✅ Event::ScenarioResolved — Task 4.
- ✅ GameState.scenario_id — Task 5.
- ✅ Engine post-dispatch hook (Done-only firing, parameterized helper) — Task 7.
- ✅ Synthetic test fixture — Task 8.
- ✅ scenarios::REGISTRY + module_for — Task 9.
- ✅ TestGame.with_scenario_id — Task 6.
- ✅ Engine unit tests (5 cases mocking the registry) — Task 7.
- ✅ scenario_registry unit tests (3 cases) — Task 3.
- ✅ Integration test in scenarios/tests/ — Task 10.
- ✅ CI gauntlet — Task 11.
- ✅ Phase-doc update — Task 12.
- ✅ PR + review + merge protocol — Tasks 13–14.

**Idempotency latch deferral** is captured in three places (ScenarioModule docs, Event::ScenarioResolved docs, and fire_scenario_resolution doc comment) plus as a new Open question in the phase doc. Single source of truth would be cleaner, but three references is what the spec asked for — each lives where a reader of *that* code would care.

**Type / signature consistency:** spot-checked. `apply_with_scenario_registry` signature matches between Task 7's implementation and the engine-tests' callsites. `ScenarioModule` field names (`setup`, `detect_resolution`, `apply_resolution`) are consistent across game-core, scenarios, and tests. `ScenarioRegistry.module_for` signature is consistent. `Resolution::Won { id }` / `Resolution::Lost { reason }` field names match across the type definition, the engine tests, the synthetic fixture, and the integration test.

**Cargo feature default-on fallback (Task 10 step 3):** The cfg-gating issue with `tests/*.rs` binaries not seeing `cfg(test)` for the dependency is a real Cargo behavior. The plan handles it as a step-3 fallback rather than committing up-front because it's worth confirming the cleaner gate (no default-on feature) is actually broken before locking it in. If Step 2 passes, Step 3 is skipped.

No placeholders, no "TBD"s, no unbound types or functions. Plan is ready to execute.
