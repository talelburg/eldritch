# Phase-4 Closing Demo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the Phase-4 milestone with two end-to-end integration walks (Won + Lost) that cycle all four phases and verify replay determinism via a serialize round-trip.

**Architecture:** First derive `PartialEq`/`Eq` on the `game-core` state tree (unblocks `assert_eq!` on whole states). Then add `crates/scenarios/tests/closing_demo.rs` with two `#[test]` walks over the synthetic fixture, each driving real actions through Mythos→Investigation→Enemy→Upkeep and asserting a resolution, each verified deterministic by serializing mid-scenario, deserializing, and continuing. Finally convert two existing JSON/field-wise state comparisons to direct `assert_eq!`.

**Tech Stack:** Rust, `cargo test`, `serde_json` (already a dev-dep of `scenarios`), the engine's `apply()` loop and `assert_event!` macro.

**Spec:** `docs/superpowers/specs/2026-06-04-phase4-closing-demo-design.md` · **Issue:** #157

---

## File Structure

- **Modify (Task 1):** `crates/game-core/src/state/game_state.rs` (3 derive lines: `GameState` + the two sub-structs at `:240` and `:257`), `crates/game-core/src/state/enemy.rs:27`, `crates/game-core/src/state/investigator.rs:32`, `crates/game-core/src/state/location.rs:16`, `crates/game-core/src/state/chaos_bag.rs:39`. Each gains `PartialEq` (and `Eq` where it derives cleanly).
- **Create (Tasks 2–3):** `crates/scenarios/tests/closing_demo.rs` — its own cargo test binary; installs `scenarios::REGISTRY` + `synth_cards::TEST_REGISTRY`; holds the two walks + shared `drive` / `replay_with_roundtrip` helpers.
- **Modify (Task 4):** `crates/scenarios/tests/hunter_movement.rs` (serde-string compare → `assert_eq!`), `crates/scenarios/tests/upkeep_phase.rs` (field-wise compare → `assert_eq!`).
- **Modify (Task 5):** `docs/phases/phase-4-scenario-plumbing.md`.

### Load-bearing facts (verified against the code)

- `synthetic::setup()` builds: 1 investigator `InvestigatorId(1)` (skills all 3, `actions_remaining: 3`), 1 location `LocationId(10)` (shroud 2, 0 clues, `code = SYNTH_LOC_CODE`), encounter deck `[SYNTH_TREACHERY_CODE]`, act deck `[thr 2 → None, thr 2 → Won{"demo"}]`, agenda deck `[thr 2 → None, thr 2 → Lost{"agenda"}]`, phase Mythos, round 0.
- `StartScenario` + `Mulligan{inv, []}` ⇒ round 1, Investigation, `inv` active. Round 1 skips Mythos.
- `Investigate{investigator}` is one action: intellect test vs shroud. With `ChaosBag::new([ChaosToken::Numeric(0)])` and intellect 3 ≥ shroud 2, it succeeds and discovers 1 clue (mirrors `crates/cards/tests/deduction.rs`).
- `AdvanceAct{investigator}` requires `Phase::Investigation` + active investigator; costs **no action point**; rejects unless the group holds ≥ `clue_threshold` clues; spends that many; on the terminal act calls `request_resolution(Won)`.
- `EndTurn` cascades Investigation→Enemy→Upkeep→Mythos; it **pauses** at round-2 Mythos step 1.4 by setting `mythos_draw_pending` (returns `Done`, not `AwaitingInput`). The player then issues `DrawEncounterCard` (a peer action) to resolve the draw and finish Mythos → Investigation. Doom is added at Mythos 1.2 (before the 1.4 pause).
- The synthetic spawned enemy has `attack_damage: 0, attack_horror: 0` (hardcoded by `spawn_enemy`, #127) and `take_damage` no-ops at 0 ⇒ **no `DamageTaken` event**. The observable proof of an attack is `EnemyExhausted` (the attacker exhausts after attacking).
- These walks never produce `AwaitingInput`: single investigator (no engagement/hunter ties), no-redraw mulligan, auto-resolving Investigate, non-hunter synthetic enemy. So each walk is a flat `Vec<Action>`.
- Event field names: `PhaseStarted{phase}`, `PhaseEnded{phase}`, `EnemySpawned{enemy,code,location,engaged_with}`, `EnemyEngaged{enemy,investigator}`, `EnemyExhausted{enemy}`, `AgendaAdvanced{from:usize}`, `ActAdvanced{from:usize}`, `ScenarioResolved{resolution}`.

---

## Task 1: Derive `PartialEq`/`Eq` on the state tree

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (derive lines at `:33`, `:240`, `:257`)
- Modify: `crates/game-core/src/state/enemy.rs:27`
- Modify: `crates/game-core/src/state/investigator.rs:32`
- Modify: `crates/game-core/src/state/location.rs:16`
- Modify: `crates/game-core/src/state/chaos_bag.rs:39`

There is no behavioral unit test for a derive; the verification is that the whole workspace still compiles and the existing suite stays green (it is the regression guard), plus a one-line compile-time proof that `GameState: PartialEq`.

- [ ] **Step 1: Add the derive to each of the 7 types**

For each derive line listed above, add `PartialEq, Eq` after `Clone`. Example (`game_state.rs:33`):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
```

Apply the identical edit to the derive lines at `game_state.rs:240` and `game_state.rs:257`, and to `enemy.rs:27`, `investigator.rs:32`, `location.rs:16`, `chaos_bag.rs:39`.

If `Eq` fails to derive for a specific type (e.g. a field that is `PartialEq` but not `Eq`), drop `Eq` for that one type and keep `PartialEq` only — `PartialEq` is all the demo needs. (`RngState` already derives `PartialEq, Eq`, so the RNG is not a blocker.)

- [ ] **Step 2: Add a compile-time assertion that `GameState: PartialEq`**

In `crates/game-core/src/state/game_state.rs`, inside the existing `#[cfg(test)] mod tests { ... }`, add:

```rust
#[test]
fn game_state_is_partial_eq() {
    fn assert_partial_eq<T: PartialEq>() {}
    assert_partial_eq::<GameState>();
}
```

- [ ] **Step 3: Build the workspace**

Run: `RUSTFLAGS="-D warnings" cargo build --all --all-features`
Expected: clean build. If a `derive(Eq)` errors on a type, drop `Eq` for that type per Step 1 and rebuild.

- [ ] **Step 4: Run the full test suite**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS (all existing tests green + the new `game_state_is_partial_eq`).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/
git commit -m "$(cat <<'EOF'
engine: derive PartialEq/Eq on the game-core state tree

Enables whole-state assert_eq! comparisons for the Phase-4 closing
demo's replay-determinism check, replacing the serde-string / field-wise
workarounds. RngState already derives PartialEq+Eq, so the RNG is not a
blocker; only 7 state types needed the derive.

Refs #157.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `closing_demo.rs` scaffold + Won walk

**Files:**
- Create: `crates/scenarios/tests/closing_demo.rs`

- [ ] **Step 1: Write the scaffold + Won walk test**

Create `crates/scenarios/tests/closing_demo.rs` with the full contents below.

```rust
//! Phase-4 closing demo: two end-to-end walks over the synthetic
//! fixture, each cycling Mythos -> Investigation -> Enemy -> Upkeep with
//! real actions and ending in a resolution, each verified deterministic
//! by a serialize round-trip mid-scenario.
//!
//! Lives in `crates/scenarios/tests/` (its own process) so it can
//! `install` the process-global registries without colliding with
//! `game-core`'s unit tests, and so it can reach the real
//! `scenarios::REGISTRY` + synthetic card corpus.

use std::sync::Once;

use game_core::engine::apply;
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, LocationId, Phase, TokenModifiers,
};
use game_core::{assert_event, Action, PlayerAction};
use scenarios::test_fixtures::synth_cards::{TEST_REGISTRY, SYNTH_ENEMY_CODE};
use scenarios::REGISTRY;

static INSTALL: Once = Once::new();

fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

/// Apply `actions` in order from `initial`, concatenating all emitted
/// events. Every action applies cleanly — these walks never produce
/// `AwaitingInput` (single investigator, no-redraw mulligan,
/// auto-resolving Investigate, non-hunter enemy).
fn drive(mut state: GameState, actions: &[Action]) -> (GameState, Vec<Event>) {
    let mut events = Vec::new();
    for a in actions {
        let r = apply(state, a.clone());
        events.extend(r.events);
        state = r.state;
    }
    (state, events)
}

/// Replay-determinism with a serialize round-trip: drive `log` from a
/// fresh `make_initial()` to the midpoint, serialize -> deserialize,
/// then continue. Returns the round-tripped final state. Proves both
/// replay determinism (seeded `state.rng` reproduces draws) and serde
/// round-trip fidelity (the property Phase 5's persistence depends on).
fn replay_with_roundtrip(make_initial: impl Fn() -> GameState, log: &[Action]) -> GameState {
    let split = log.len() / 2;
    let mut state = make_initial();
    for a in &log[..split] {
        state = apply(state, a.clone()).state;
    }
    let json = serde_json::to_string(&state).expect("serialize mid-scenario state");
    let mut state: GameState =
        serde_json::from_str(&json).expect("deserialize mid-scenario state");
    for a in &log[split..] {
        state = apply(state, a.clone()).state;
    }
    state
}

#[test]
fn won_walk_full_cycle_replays_identically() {
    install_registry();
    let inv = InvestigatorId(1);

    // setup() + deterministic local seeding: 4 clues to discover and a
    // +0 chaos bag so Investigate succeeds against shroud 2 (intellect 3).
    let make_initial = || {
        let mut s = scenarios::test_fixtures::synthetic::setup();
        s.locations.get_mut(&LocationId(10)).unwrap().clues = 4;
        s.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
        s.token_modifiers = TokenModifiers::default();
        s
    };

    // Round 1 (Mythos skipped): Investigate x3 -> EndTurn cascades and
    // pauses at round-2 Mythos 1.4 -> DrawEncounterCard finishes Mythos ->
    // round 2 Investigate (4th clue) -> AdvanceAct x2 (act 0 -> 1 -> Won).
    let log = vec![
        Action::Player(PlayerAction::StartScenario),
        Action::Player(PlayerAction::Mulligan { investigator: inv, indices_to_redraw: vec![] }),
        Action::Player(PlayerAction::Investigate { investigator: inv }),
        Action::Player(PlayerAction::Investigate { investigator: inv }),
        Action::Player(PlayerAction::Investigate { investigator: inv }),
        Action::Player(PlayerAction::EndTurn),
        Action::Player(PlayerAction::DrawEncounterCard),
        Action::Player(PlayerAction::Investigate { investigator: inv }),
        Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
    ];

    let (final_state, events) = drive(make_initial(), &log);

    // Cycled all four phases across the two rounds.
    assert_event!(events, Event::PhaseEnded { phase } if *phase == Phase::Investigation);
    assert_event!(events, Event::PhaseStarted { phase } if *phase == Phase::Upkeep);
    assert_event!(events, Event::PhaseStarted { phase } if *phase == Phase::Mythos);
    // Investigation discovered clues; the act advanced; the scenario was won.
    assert_event!(events, Event::ActAdvanced { from } if *from == 0);
    assert_event!(
        events,
        Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "demo"
    );
    assert!(matches!(final_state.resolution, Some(Resolution::Won { .. })));

    let replayed = replay_with_roundtrip(make_initial, &log);
    assert_eq!(
        final_state, replayed,
        "Won walk must replay identically across a serialize round-trip",
    );
}
```

- [ ] **Step 2: Run the Won walk**

Run: `cargo test -p scenarios --test closing_demo won_walk_full_cycle_replays_identically -- --nocapture`
Expected: PASS. If a clue-count or pause assumption is off, debug against the load-bearing facts above (do not weaken assertions to make it pass).

- [ ] **Step 3: Commit**

```bash
git add crates/scenarios/tests/closing_demo.rs
git commit -m "$(cat <<'EOF'
test: phase-4 closing demo — Won full-cycle walk + replay round-trip

Drives the synthetic fixture setup -> round-1 Investigate x3 -> phase
cascade -> round-2 Investigate -> AdvanceAct x2 -> Resolution::Won, then
asserts the action log replays identically across a mid-scenario
serialize/deserialize round-trip.

Refs #157.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Lost walk

**Files:**
- Modify: `crates/scenarios/tests/closing_demo.rs`

- [ ] **Step 1: Append the Lost walk test**

Add to the end of `crates/scenarios/tests/closing_demo.rs`:

```rust
#[test]
fn lost_walk_spawn_attack_doom_replays_identically() {
    install_registry();
    let inv = InvestigatorId(1);

    // setup() + seed the spawn-bearing synthetic enemy on top of the
    // encounter deck so a Mythos draw spawns + engages it.
    let make_initial = || {
        let mut s = scenarios::test_fixtures::synthetic::setup();
        scenarios::test_fixtures::synthetic::with_encounter_deck(
            &mut s,
            vec![
                CardCode(SYNTH_ENEMY_CODE.into()),
                CardCode(scenarios::test_fixtures::synth_cards::SYNTH_TREACHERY_CODE.into()),
            ],
        );
        s
    };

    // Setup + close mulligan, then drive an EndTurn cascade, drawing only
    // when a Mythos draw is pending and breaking on resolution. Record the
    // realized action log so the round-trip replays exactly what ran.
    let mut log = vec![
        Action::Player(PlayerAction::StartScenario),
        Action::Player(PlayerAction::Mulligan { investigator: inv, indices_to_redraw: vec![] }),
    ];
    let (mut state, mut events) = drive(make_initial(), &log);

    for _ in 0..12 {
        let act = Action::Player(PlayerAction::EndTurn);
        log.push(act.clone());
        let r = apply(state, act);
        events.extend(r.events);
        state = r.state;
        if state.resolution.is_some() {
            break;
        }
        if state.mythos_draw_pending.is_some() {
            let act = Action::Player(PlayerAction::DrawEncounterCard);
            log.push(act.clone());
            let r = apply(state, act);
            events.extend(r.events);
            state = r.state;
            if state.resolution.is_some() {
                break;
            }
        }
    }

    // Enemy spawned, engaged, and attacked (proven by EnemyExhausted —
    // the synthetic enemy deals 0 damage, so no DamageTaken fires).
    assert_event!(events, Event::EnemySpawned { code, .. } if code.0 == SYNTH_ENEMY_CODE);
    assert_event!(events, Event::EnemyEngaged { investigator, .. } if *investigator == inv);
    assert_event!(events, Event::EnemyExhausted { .. });
    // Doom advanced the agenda and then latched the loss.
    assert_event!(events, Event::AgendaAdvanced { from } if *from == 0);
    assert_event!(events, Event::ScenarioResolved { resolution: Resolution::Lost { .. } });
    assert!(matches!(state.resolution, Some(Resolution::Lost { .. })));

    let replayed = replay_with_roundtrip(make_initial, &log);
    assert_eq!(
        state, replayed,
        "Lost walk must replay identically across a serialize round-trip",
    );
}
```

- [ ] **Step 2: Run the Lost walk**

Run: `cargo test -p scenarios --test closing_demo lost_walk_spawn_attack_doom_replays_identically -- --nocapture`
Expected: PASS. If the enemy never spawns/attacks, confirm the encounter-deck seeding and that round-2 Mythos draws the enemy; if resolution never latches within 12 rounds, raise the loop bound (doom +1/Mythos, two thresholds of 2 ⇒ Lost by ~round 5).

- [ ] **Step 3: Run both walks together**

Run: `cargo test -p scenarios --test closing_demo`
Expected: 2 passed.

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/tests/closing_demo.rs
git commit -m "$(cat <<'EOF'
test: phase-4 closing demo — Lost walk (spawn + attack + doom)

EndTurn cascade where a Mythos draw spawns + engages the synthetic
enemy, it attacks (proven via EnemyExhausted), and doom advances the
agenda to Resolution::Lost; verified via the same serialize round-trip.

Refs #157.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Convert existing JSON / field-wise comparisons to `assert_eq!`

**Files:**
- Modify: `crates/scenarios/tests/hunter_movement.rs:115-125`
- Modify: `crates/scenarios/tests/upkeep_phase.rs:157-180`

- [ ] **Step 1: Convert `hunter_movement.rs`**

Replace the comment + `serde_json::to_string` comparison (around `:115-125`) with a direct state comparison:

```rust
    // Replay determinism is a whole-state property: replaying an
    // identical action log reproduces state bit-for-bit.
    assert_eq!(
        s1, s2,
        "replaying the same action log must reproduce identical state",
    );
```

If the `serde_json` import becomes unused in that file, remove it (clean up only the orphan this change creates).

- [ ] **Step 2: Convert `upkeep_phase.rs`**

Replace the `// GameState does not derive PartialEq; compare field-wise.` comment and the block of per-field `assert_eq!` calls (from `:158` through the end of that field-wise group) with a single whole-state comparison:

```rust
    // Replaying the same action log reproduces state bit-for-bit.
    assert_eq!(final_state, replayed_state, "replay must reproduce identical state");
```

- [ ] **Step 3: Run the two affected tests**

Run: `cargo test -p scenarios --test hunter_movement && cargo test -p scenarios --test upkeep_phase`
Expected: PASS for both.

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/tests/hunter_movement.rs crates/scenarios/tests/upkeep_phase.rs
git commit -m "$(cat <<'EOF'
test: compare replayed states via PartialEq, not JSON/field-wise

Now that the state tree derives PartialEq, the serde-string compare in
hunter_movement and the field-wise compare in upkeep_phase become direct
assert_eq! on whole states — stronger and clearer than either workaround.

Refs #157.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Phase-doc update + full CI gauntlet

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

- [ ] **Step 1: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```
Expected: all five green. Fix any fmt/clippy/doc issues before proceeding.

- [ ] **Step 2: Update the phase doc**

In `docs/phases/phase-4-scenario-plumbing.md`:
- Flip the **Status** line to closed (✅), dated 2026-06-04, noting the closing demo shipped.
- Add an `#157` row to the **Closed** table (PR #, one-line note: two full-cycle walks + serialize round-trip; derived state `PartialEq`).
- Flip the **slot-13** Ordering row to `✅ PR #<n>`.
- Remove the now-false "blocked on `GameState` not deriving `PartialEq`" / "needs serde-based comparison" wording from the Status blurb and the "What done looks like" replay bullet (the state now derives `PartialEq`).
- Per `docs/phases/README.md`, add a **Decisions made** entry only if it passes the "would a future PR-author choose differently without this" test — likely a one-liner that the state tree now derives `PartialEq`/`Eq` (so future tests use `assert_eq!`, not JSON).

- [ ] **Step 3: Commit**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: close phase-4 scenario plumbing

Closing demo (#157) shipped: two full-cycle Won/Lost walks + serialize
round-trip determinism. Milestone deliverables complete.

Closes #157.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 4: Push + open PR**

```bash
git push -u origin test/phase4-closing-demo
gh pr create --fill
```
Then watch CI: `gh pr checks <PR#> --watch`. Fix failures with follow-up commits to the same branch. Merge only after explicit user approval.

---

## Self-Review

**Spec coverage:**
- Derive `PartialEq` (spec step 1) → Task 1. ✓
- Won walk (spec step 2) → Task 2. ✓
- Lost walk (spec step 3) → Task 3. ✓
- Serialize round-trip determinism (spec step 4) → `replay_with_roundtrip`, used in Tasks 2 & 3. ✓
- Convert existing comparisons (spec step 5) → Task 4. ✓
- No fixture change; keep `synthetic_resolution.rs` (spec scope boundaries) → honored (only local seeding; that file untouched). ✓
- Verification + phase-doc close → Task 5. ✓

**Placeholder scan:** No "TBD"/"add appropriate…"; every code step shows complete code; commands have expected output. ✓

**Type/identifier consistency:** `drive` / `replay_with_roundtrip` / `make_initial` / `install_registry` used consistently across Tasks 2–3; event field names (`phase`, `from`, `code`, `investigator`, `enemy`, `resolution`) and ids (`InvestigatorId(1)`, `LocationId(10)`) match the verified code; `SYNTH_ENEMY_CODE`/`SYNTH_TREACHERY_CODE`/`with_encounter_deck` match `synth_cards`/`synthetic`. ✓
