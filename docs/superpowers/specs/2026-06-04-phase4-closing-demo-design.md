# Phase-4 closing demo — design

**Issue:** [#157](https://github.com/talelburg/eldritch/issues/157)
**Date:** 2026-06-04
**Status:** approved (pending spec review)

## Goal

Close the Phase-4 milestone (`docs/phases/phase-4-scenario-plumbing.md`, slot 13) with the
two pieces the phase doc names as remaining: a **fuller setup → resolution walk** and a
**replay-determinism assertion**. Concretely: two end-to-end integration tests over the
synthetic fixture that each drive a continuous multi-round, four-phase cycle with varied real
actions, ending in a resolution, each verified deterministic via a serialize round-trip.

## Background (load-bearing facts)

- Determinism is sourced from a seeded `RngState` carried **in** `GameState` (`state.rng`),
  not a replayed `EngineRecord` log — there is no `action_log` field on the state. Re-driving
  the same player-action script from a fresh `setup()` (identical seed) reproduces state
  bit-for-bit; cascaded chaos draws / shuffles advance the in-state RNG identically.
- `RngState` already derives `PartialEq, Eq` and is serialized (no `serde(skip)`), so it is
  **not** a blocker for a state-wide `PartialEq`.
- Only 7 `game-core` types lack `PartialEq`: `GameState`, two window/continuation sub-structs
  in `state/game_state.rs`, `Enemy`, `Investigator`, `Location`, `ChaosBag`. Their field types
  are domain primitives that already derive `PartialEq`.
- `test_location` defaults to shroud 2, 0 clues. `TestGame::new()` seeds an **empty** chaos bag
  (`ChaosBag::new([])`). So a real Investigate needs the test to locally seed clues on the
  location and a chaos bag — exactly how existing integration tests mutate state after
  `setup()` (e.g. `synthetic_resolution.rs` sets `clues = 4`). **No shared-fixture change.**
- The synthetic fixture (`crates/scenarios/src/test_fixtures/synthetic.rs`): 1 investigator,
  1 location (`SYNTH_LOC_CODE`), encounter deck seeded with one `SYNTH_TREACHERY_CODE`,
  act deck `[thr 2 → none, thr 2 → Won{"demo"}]`, agenda deck `[thr 2 → none, thr 2 → Lost]`.
  `SYNTH_ENEMY_CODE` is a spawn-bearing enemy (`SpawnLocation::Specific(SYNTH_LOC_CODE)`),
  pushed onto the encounter deck by tests that want a spawn.
- Round 1 skips Mythos (Rules Reference p.24); the game begins after the mulligan window.

## Design

### Step 1 — derive `PartialEq` (standalone first commit)

Add `PartialEq` (and `Eq` where every field supports it) to the 7 types above. Build + full
test gauntlet green before any demo code. Verify on first compile that the two window/
continuation sub-structs at `state/game_state.rs:240` and `:257` derive cleanly (they are
already `Serialize`, so their fields are data, not fn-pointers — `PartialEq` should follow).

This unblocks `assert_eq!(s1, s2)` on whole states and is the precondition for steps 4 and 5.

### Step 2 — Won walk (`crates/scenarios/tests/closing_demo.rs`)

New integration test binary; installs `scenarios::REGISTRY` + `synth_cards::TEST_REGISTRY`
via a `std::sync::Once` (mirrors `synthetic_resolution.rs`).

Seed locally after `setup()` (no fixture change): clues on the location + a deterministic
favorable chaos bag so a real Investigate succeeds predictably.

Walk:
- **Round 1** (Mythos skipped): `StartScenario` → `Mulligan` (no redraw) → `Investigate`
  (real Phase-3 skill test, discovers clue(s)) → `EndTurn` → Enemy (empty) → Upkeep
  (ready / draw / gain, round → 2).
- **Round 2**: Mythos auto-draws the synthetic treachery + adds doom (below threshold);
  `Investigate` to top up clues; `AdvanceAct` ×2 spends clues across both acts → terminal
  act resolution point → `Event::ScenarioResolved { Won { id: "demo" } }`.

Asserts: the four-phase cycle order is observed across the walk (e.g. `PhaseStarted` /
`PhaseEnded` sequence), key framework events present, and `state.resolution == Won`.

### Step 3 — Lost walk (same file)

Seed `SYNTH_ENEMY_CODE` on top of the encounter deck after `setup()`.

Walk: `StartScenario` → `Mulligan` → then an EndTurn cascade with break-on-resolution
(mirrors the cadence-tolerant loop in `synthetic_resolution.rs::synthetic_scenario_resolves_lost_via_doom`,
drawing only when `mythos_draw_pending.is_some()`):
- Round-2 Mythos **draws → spawns → engages** the enemy at the location.
- Round-2 Enemy phase: the engaged enemy **attacks** the investigator; Upkeep readies it.
- Doom accrues each Mythos → agenda 0 advances at threshold → terminal agenda latches
  `Event::ScenarioResolved { Lost }`.

Asserts: `EnemySpawned` + paired `EnemyEngaged`, an attack event, `AgendaAdvanced { from: 0 }`,
and `state.resolution == Lost`.

### Step 4 — determinism via serialize round-trip (per walk)

Local helper in `closing_demo.rs`:

1. Drive the full action script from a fresh `setup()` (+ local seeding) → **state A**.
2. Drive the same script to a midpoint, `serde_json` serialize → deserialize → continue the
   remaining actions → **state B**.
3. `assert_eq!(A, B)` (clean now that `GameState: PartialEq`).

This proves replay determinism **and** serde round-trip fidelity — the property Phase 5
(server + persistence) depends on.

### Step 5 — convert existing JSON / field-wise comparisons (the requested detour)

Now that the state derives `PartialEq`, replace the two workaround comparisons with direct
`assert_eq!` on whole states, and update/remove the now-stale "isn't PartialEq" comments:
- `crates/scenarios/tests/hunter_movement.rs:117-124` — `serde_json::to_string` compare →
  `assert_eq!(s1, s2)`.
- `crates/scenarios/tests/upkeep_phase.rs:158+` — field-wise compare → `assert_eq!(final_state, replayed_state)`.

(The other `serde_json::to_string` sites in `game-core` are single-value serialization
round-trip unit tests — unrelated, left untouched.)

## Scope boundaries

- **No change** to the shared `synthetic::setup()` fixture; tests seed their own clues /
  chaos bag / encounter deck locally.
- **Keep** the existing `synthetic_resolution.rs` latch tests as-is — they pin the resolution
  latch precisely; the demo adds the fuller integration walks alongside. Intentional, small
  overlap; not deleting code outside this request.
- Fight / evade actions are not required (the 1-location fixture has no movement; the Lost
  walk exercises combat via the enemy's attack). Investigate is the representative Phase-3
  action in the Won walk.

## Verification

Full local CI gauntlet, all `-D warnings`:
```
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```

Final commit updates `docs/phases/phase-4-scenario-plumbing.md`: move `#157` to the Closed
table, flip the slot-13 Arc row to ✅, drop the stale "blocked on `GameState` not deriving
`PartialEq`" note (now false), and flip the milestone Status to ✅ closed.

## Commit plan

1. `engine: derive PartialEq on the game-core state tree` (step 1).
2. `test: phase-4 closing demo — Won + Lost full-cycle walks + replay round-trip` (steps 2–4).
3. `test: compare states via PartialEq, not JSON/field-wise` (step 5).
4. `docs: close phase-4 scenario plumbing` (phase-doc update, final).
