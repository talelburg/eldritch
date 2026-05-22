# #74 Scenario module skeleton — Design

**Issue:** [#74](https://github.com/talelburg/eldritch/issues/74) — `[engine] scenario module skeleton: setup, detect_resolution, apply_resolution`.

**Phase:** 4 (scenario plumbing). Slot 2 in the Shape-B ordering, follows #103 (PR #129, merged 2026-05-21).

**Goal:** Define the scenario-module shape every scenario plugs into. Ship a synthetic test fixture, wire the engine to call `detect_resolution` after each `apply`, and emit `Event::ScenarioResolved` when a resolution fires.

## Background

The phase-4 design pass (2026-05-21) settled the high-level shape: `ScenarioModule` is a static struct of `fn` pointers mirroring `CardRegistry`; `ScenarioRegistry` looks up modules by `ScenarioId`. No `dyn`, no `Box`. Hosts install once at startup. `GameState` carries a serializable `ScenarioId` so action-log replay reproduces final state. The synthetic fixture lives under `crates/scenarios/src/test_fixtures/` gated `#[cfg(any(test, feature = "test_fixtures"))]` and exists only to demonstrate the shape — The Gathering is Phase-7 content.

This spec fills in the sub-decisions left open by the phase doc: exact `ScenarioId` shape, `Resolution` enum shape, what's on `ScenarioModule` v0, where `detect_resolution` fires from `apply()`, and how the synthetic stub's resolution condition is wired.

## Crate layering

Same direction as `card-dsl ← game-core ← cards`: **`game-core` owns the types and the registry slot; `scenarios` provides the values.**

- `game_core::scenario` — new module: `ScenarioId`, `Resolution`, `ScenarioModule`, `ScenarioRegistry`.
- `game_core::scenario_registry` — new module mirroring `card_registry`: `OnceLock<ScenarioRegistry>`, `install()`, `current()`.
- `crates/scenarios/src/test_fixtures/synthetic.rs` — new, gated `#[cfg(any(test, feature = "test_fixtures"))]`.
- `crates/scenarios/src/lib.rs` — exposes `pub const REGISTRY: ScenarioRegistry` (mirrors `cards::REGISTRY`).

`scenarios` already depends on `game-core` and `cards`. No new workspace deps.

## Data shape

```rust
// game_core::scenario

/// Stable, serializable identifier for a scenario module. Mirrors
/// CardCode: newtype around String, kept on GameState so action-log
/// replay can resolve the module via the registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScenarioId(String);

impl ScenarioId {
    pub fn new(s: impl Into<String>) -> Self { Self(s.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}

/// Outcome of a scenario. Phase-4 minimal shape; Phase-9 will refine
/// the payload (typed Fact log, branching campaign decisions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Resolution {
    /// Scenario was completed successfully.
    Won {
        /// Resolution branch identifier (e.g. "R1", "R2"). The
        /// per-scenario meaning lives in that scenario's module.
        id: String,
    },
    /// Scenario ended in defeat.
    Lost {
        /// Human-readable cause for diagnostics; not semantically
        /// load-bearing. Phase-9 may swap for a typed enum.
        reason: String,
    },
}

/// Static, host-installed bundle of fn pointers for one scenario.
/// Mirrors CardRegistry's shape: no dyn, no Box, Copy-able.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioModule {
    /// Build the scenario's initial GameState. Places locations,
    /// populates encounter / act / agenda decks, sets chaos bag
    /// modifiers. Stub returns minimal state for the skeleton.
    pub setup: fn() -> GameState,
    /// Pure check called by apply() after each Done outcome.
    pub detect_resolution: fn(&GameState) -> Option<Resolution>,
    /// Apply resolution effects (XP, trauma, scenario-end cleanup).
    /// Receives the events buffer so changes are observable.
    /// Stub is a no-op for the skeleton; Phase-9 fills in.
    pub apply_resolution: fn(&Resolution, &mut GameState, &mut Vec<Event>),
}

/// Host-installed lookup table. The `scenarios` crate exposes a
/// const REGISTRY wrapping its own by_id; hosts install once at
/// startup with `game_core::scenario_registry::install(scenarios::REGISTRY)`.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioRegistry {
    pub module_for: fn(&ScenarioId) -> Option<&'static ScenarioModule>,
}
```

`GameState` gains one field:

```rust
pub struct GameState {
    // … existing fields …
    /// Identifier of the scenario this state belongs to, if any.
    /// `None` for tests / fixtures that don't care about scenario
    /// resolution; the engine's post-apply detect_resolution call
    /// short-circuits on None or when no registry is installed.
    pub scenario_id: Option<ScenarioId>,
}
```

`Event` gains one variant:

```rust
pub enum Event {
    // … existing variants …
    /// A scenario resolved (won or lost). Emitted by `apply()` after
    /// dispatch when the installed scenario module's
    /// `detect_resolution` returns Some.
    ScenarioResolved {
        resolution: Resolution,
    },
}
```

## Engine integration

`apply()` gets a post-dispatch hook. Pseudo-Rust:

```rust
pub fn apply(state: GameState, action: Action) -> ApplyResult {
    let mut state = state;
    let mut events = Vec::new();
    let outcome = match action { /* … */ };

    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        events.clear();
    } else if matches!(outcome, EngineOutcome::Done) {
        if let (Some(id), Some(reg)) =
            (state.scenario_id.as_ref(), scenario_registry::current())
        {
            if let Some(module) = (reg.module_for)(id) {
                if let Some(res) = (module.detect_resolution)(&state) {
                    events.push(Event::ScenarioResolved {
                        resolution: res.clone(),
                    });
                    (module.apply_resolution)(&res, &mut state, &mut events);
                }
            }
        }
    }

    ApplyResult { state, events, outcome }
}
```

**Rationale for `Done`-only firing:** `AwaitingInput` means the engine paused mid-resolution (skill-test commit window, reaction-window prompt). State is potentially mid-update; a scenario module reading it could see, e.g., a damaged-but-not-yet-defeated investigator and false-fire `Lost`. On `Done` the action has fully settled. `Rejected` already short-circuits via the existing `events.clear()`. If a future scenario genuinely needs mid-`AwaitingInput` polling, we relax then.

**Idempotency:** `detect_resolution` fires every `Done` outcome. A scenario that resolves on round 3 will keep returning `Some(Won { … })` on every subsequent apply, re-emitting `ScenarioResolved` and re-calling `apply_resolution`. That's wrong for any non-trivial `apply_resolution` (e.g., XP would stack). **Out of scope for this PR** — fixed by either a `GameState.resolution: Option<Resolution>` field that `apply()` checks before calling `detect_resolution`, or by scenario modules guarding their own implementations. Settle this when Phase-9 lands the first real `apply_resolution`. Document the gap inline.

## Synthetic test fixture

`crates/scenarios/src/test_fixtures/synthetic.rs`:

```rust
//! Synthetic scenario fixture — the minimum a scenario needs to exist.
//! Used by Phase-4's engine integration tests; not part of any
//! shipped campaign.

pub const ID: &str = "synthetic";

pub fn setup() -> GameState {
    // Single dummy investigator, single location, scenario_id set,
    // turn_order populated, RNG seeded. Phase = Mythos, round = 0:
    // a fresh state ready for PlayerAction::StartScenario.
    TestGame::new()
        .with_investigator(test_investigator(1))
        .with_location(test_location(10, "Demo Location"))
        .with_turn_order([InvestigatorId(1)])
        .with_scenario_id(ScenarioId::new(ID))
        .build()
}

pub fn detect_resolution(state: &GameState) -> Option<Resolution> {
    // Fires once the engine has stepped past the initial Mythos
    // skip during StartScenario into Investigation with round = 1.
    if state.phase == Phase::Investigation && state.round >= 1 {
        Some(Resolution::Won { id: "demo".into() })
    } else {
        None
    }
}

pub fn apply_resolution(
    _resolution: &Resolution,
    _state: &mut GameState,
    _events: &mut Vec<Event>,
) {
    // Skeleton no-op. Phase-9 fills in XP / trauma application
    // when the campaign log lands.
}

pub const MODULE: ScenarioModule = ScenarioModule {
    setup,
    detect_resolution,
    apply_resolution,
};
```

`crates/scenarios/src/lib.rs` exposes:

```rust
#[cfg(any(test, feature = "test_fixtures"))]
pub mod test_fixtures;

#[cfg(any(test, feature = "test_fixtures"))]
fn by_id(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    match id.as_str() {
        test_fixtures::synthetic::ID => Some(&test_fixtures::synthetic::MODULE),
        _ => None,
    }
}

#[cfg(any(test, feature = "test_fixtures"))]
pub const REGISTRY: ScenarioRegistry = ScenarioRegistry { module_for: by_id };
```

When no fixtures and no real scenarios exist, the crate compiles with no `by_id` / `REGISTRY` — fine for now (no host installs it yet). The `cards` crate has the same shape: `REGISTRY` is gated on the cards corpus existing.

## TestGame builder

Add one method, mirroring the existing `with_*` setters:

```rust
impl TestGame {
    pub fn with_scenario_id(mut self, id: ScenarioId) -> Self {
        self.scenario_id = Some(id);
        self
    }
}
```

`TestGame::build()` includes `scenario_id: self.scenario_id` in the returned `GameState`. Default is `None`. Existing tests need no changes.

## Tests

Three layers:

### 1. `game_core::scenario_registry` unit tests

Mirror the `card_registry` tests (`crates/game-core/src/card_registry.rs` has the template). Use a hand-rolled `ScenarioModule` with mock fn pointers:

- `module_for` resolves a known id and returns `None` for unknown.
- `install()` is idempotent at the `OnceLock` level (second call returns `Err`).
- `current()` returns the installed value.

Tests construct local `ScenarioRegistry` instances and call `(reg.module_for)(&id)` directly — only one test touches the process-global `OnceLock`, matching `card_registry`'s pattern.

### 2. `game_core::engine` tests

In `crates/game-core/src/engine/mod.rs` `#[cfg(test)]` block:

- **No scenario_id → no resolution emitted.** `TestGame::new().build()` (scenario_id = None), drive `StartScenario`, assert no `Event::ScenarioResolved` in events.
- **scenario_id set but no module in registry → no resolution emitted.** `TestGame::new().with_scenario_id(ScenarioId::new("nonexistent")).build()` with a mock registry that returns `None` for that id, drive `StartScenario`, assert no `Event::ScenarioResolved`.
- **scenario_id set, module fires → resolution emitted.** Mock registry whose `module_for` returns a `ScenarioModule` with `detect_resolution = |_state| Some(Won { id: "test".into() })` and a no-op `apply_resolution`. Drive `StartScenario` from a TestGame with that scenario_id. Assert `Event::ScenarioResolved { resolution: Won { id: "test" } }` is in the events.
- **Rejected outcomes skip detect_resolution.** Same mock, but apply a `StartScenario` to a state with `round != 0` (already-started). Assert no `ScenarioResolved` event (events should be empty because the outcome is Rejected).

**Mocking the registry without touching the global `OnceLock`:** the post-dispatch hook is factored into a helper `fn fire_scenario_resolution(state: &mut GameState, events: &mut Vec<Event>, reg: Option<&ScenarioRegistry>)`. `apply()` passes `scenario_registry::current()`; engine unit tests pass a locally-constructed mock registry. The global `OnceLock` is exercised by exactly one test (the idempotent-install test in `scenario_registry`) and by the integration test below. Mirrors `card_registry`'s test layout.

### 3. Integration test in `crates/scenarios/tests/`

New file `crates/scenarios/tests/synthetic_resolution.rs`. Each `tests/*.rs` file is its own cargo binary, so it can `install(scenarios::REGISTRY)` without colliding with other test runs (per the pattern documented in CLAUDE.md and used by `cards/tests/play_card.rs`).

The test:
1. Install both registries: `cards::REGISTRY` (since `scenarios` depends on `cards`, doc comments may require it; safe to install regardless) and `scenarios::REGISTRY`.
2. Build state via `scenarios::test_fixtures::synthetic::setup()`.
3. Apply `PlayerAction::StartScenario`.
4. Assert `Event::ScenarioResolved { resolution: Won { id: "demo" } }` appears in the result's events.
5. Assert `result.state.phase == Phase::Investigation` and `result.state.round == 1` (sanity that the fixture's predicate fired for the right reason).

Cargo manifest changes for `crates/scenarios/Cargo.toml`:
- Add `[features] test_fixtures = []`. The fixture module and `REGISTRY` are gated on `#[cfg(any(test, feature = "test_fixtures"))]` (matches the phase doc verbatim). The integration test in `crates/scenarios/tests/` is compiled under `cfg(test)` for the crate, so the fixture is visible without enabling the feature; downstream crates (server / other crates' integration tests) opt in by enabling `scenarios/test_fixtures` as a dev-dependency feature.

## Sub-decisions captured

- **`ScenarioId` is a newtype `String`-backed** — mirrors `CardCode`. Serializable, replay-safe.
- **`Resolution::{Won { id }, Lost { reason }}`** with `#[non_exhaustive]`. String payloads are stand-ins; Phase-9 refines.
- **`ScenarioModule` v0 has three fields**: `setup`, `detect_resolution`, `apply_resolution`. No `special_rules` slot — YAGNI; add when a concrete scenario forces it.
- **`detect_resolution` fires on Done only** from inside `apply()`. `Rejected` and `AwaitingInput` skip.
- **Engine calls `apply_resolution` immediately** after emitting `ScenarioResolved`. Action-log replay reproduces XP/trauma deterministically.
- **`GameState.scenario_id: Option<ScenarioId>`** — `None` keeps every existing test green.
- **Idempotency gap deferred**: once a resolution fires, subsequent applies will keep firing it. Acceptable for the skeleton because `apply_resolution` is a no-op. Phase-9 fixes when real bodies arrive (likely via `GameState.resolution: Option<Resolution>` guard, or by giving scenario modules a one-shot semantics). Tracked inline; not a separate issue yet.
- **Post-dispatch hook is a parameterized helper** so engine unit tests can mock the registry without touching the process-global `OnceLock`.

## Out of scope

- The Gathering content (Phase 7).
- `CampaignState` / typed `Fact` log (Phase 9, per phase-doc migration of #75).
- Multi-resolution per scenario / branching (Phase 9).
- `apply_resolution` actually applying XP/trauma (Phase 9).
- The `OnceLock` idempotency gap (deferred — see above).
- Server / web integration of the registry (no host installs it yet).

## Acceptance (mirrors issue #74)

- [ ] Scenario module shape defined in `game_core::scenario` and consumed by `crates/scenarios`.
- [ ] One stub scenario module (`synthetic`) compiles under the test_fixtures gate.
- [ ] Engine calls `detect_resolution` post-action on `Done` outcomes; emits `Event::ScenarioResolved` and calls `apply_resolution` when a resolution fires.
- [ ] Tests: registry unit tests, engine integration tests (with parameterized mock registry), and one end-to-end integration test in `crates/scenarios/tests/`.
