# C7b — Gathering Won/Lost resolutions test — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A headless integration test that drives solo Roland through The Gathering to a genuine engine-latched **Won** (`Resolution::Won { R1 }`) and **Lost** (`Resolution::Lost`).

**Architecture:** New integration-test binary `crates/scenarios/tests/the_gathering_resolutions.rs` (own process → installs the real `scenarios::REGISTRY` + `cards::REGISTRY`). A shared setup helper seats Roland on a controlled chaos bag; the Lost test seeds Roland 1-from-death + an engaged enemy and drives a real Enemy-phase attack; the Won test drives act 1 for real, seeds past act 2 (the documented fallback — the Hallway has 0 clues), then drives the defeating Fight. Both assert `Event::ScenarioResolved` + the latched `state.resolution`.

**Tech Stack:** Rust, `game_core::engine::apply`, the `scenarios`/`cards` crates, the in-repo test fixtures.

Spec: `docs/superpowers/specs/2026-06-16-phase-7-slice-1-c7b-gathering-resolutions-design.md`.

---

## Key facts (verified against corpus + engine)

- Roland 01001: health 9, sanity 5, intellect 3, combat 4.
- Study 01111: shroud 2, clues 2. Hallway 01112: shroud 1, **clues 0**.
- Act 1 (01108) clue threshold 2 (`AdvanceAct`); act 3 (01110) advances on Ghoul Priest (01116) defeat → terminal `Resolution::Won { R1 }`.
- Ghoul Priest 01116: health 5 (solo), fight 4, Hunter + Retaliate, damage 2. Retaliate fires only on a *failed* Fight, so a successful defeating Fight is clean.
- A single-token bag `ChaosBag::new([ChaosToken::Numeric(0)])` always draws `Numeric(0)` (modifier 0) — deterministic; Roland's intellect 3 ≥ shroud 2 and combat 4 ≥ fight 4 both succeed.
- `state.chaos_bag` is a public field, overridable after `setup()`.
- `StartScenario` → per-investigator `Mulligan` → turn begins (Investigation, active investigator, 3 actions).
- `state.resolution: Option<Resolution>` is the latch; `Event::ScenarioResolved` is emitted on the latch transition.

## File structure

- Create: `crates/scenarios/tests/the_gathering_resolutions.rs` — the whole deliverable (helper + two tests).

---

## Task 1: Harness + setup helper + smoke test

**Files:**
- Create: `crates/scenarios/tests/the_gathering_resolutions.rs`

- [ ] **Step 1: Write the file with the install helper, setup helper, and a smoke test**

```rust
//! C7b — the Slice-1 "done" gate: drive solo Roland through The Gathering
//! to a genuine engine-latched Won and Lost resolution, against the real
//! `scenarios` + `cards` registries.
//!
//! Hybrid fidelity (see the C7b design spec): drive the cheap, deterministic
//! real progression and seed only the expensive preconditions, so the
//! resolution itself is always engine-latched. Test-determinism stand-ins
//! (a controlled chaos bag, a minimal roster deck, seeded health/act state)
//! are called out at their use sites.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::state::{CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId};
use game_core::{Action, InputResponse, PlayerAction, RosterEntry};

const ROLAND: &str = "01001";
const INV: InvestigatorId = InvestigatorId(1);

fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(scenarios::REGISTRY);
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// The Gathering set up + solo Roland seated and past the mulligan, ready
/// to act in the Investigation phase. Determinism stand-in: the random
/// Standard bag (which contains AutoFail) is replaced with a single-token
/// `Numeric(0)` bag so skill tests resolve predictably.
fn seated_roland() -> GameState {
    install();
    let mut state = scenarios::the_gathering::setup();
    // Stand-in: deterministic chaos bag (production serves Standard).
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);

    // Stand-in: a minimal deck (the resolution paths don't read deck
    // contents). Eight copies of a real neutral event so the opening hand
    // of 5 draws cleanly.
    let roster = vec![RosterEntry {
        investigator: CardCode::new(ROLAND),
        deck: vec![CardCode::new("01088"); 8],
    }];
    let started = apply(
        state,
        Action::Player(PlayerAction::StartScenario { roster }),
    );
    assert!(
        matches!(started.outcome, EngineOutcome::AwaitingInput { .. }),
        "StartScenario should await the mulligan, got {:?}",
        started.outcome,
    );
    let after_mulligan = apply(
        started.state,
        Action::Player(PlayerAction::Mulligan {
            investigator: INV,
            indices_to_redraw: vec![],
        }),
    );
    assert_eq!(after_mulligan.outcome, EngineOutcome::Done);
    after_mulligan.state
}

#[test]
fn solo_roland_is_seated_in_the_study_ready_to_act() {
    let state = seated_roland();
    assert_eq!(state.round, 1);
    assert!(
        state.investigators.contains_key(&INV),
        "Roland seated as investigator 1"
    );
    assert!(
        state.resolution.is_none(),
        "no resolution latched at setup"
    );
}
```

- [ ] **Step 2: Run the smoke test, expect PASS**

Run: `cargo test -p scenarios --test the_gathering_resolutions solo_roland_is_seated -- --nocapture`
Expected: PASS. If the mulligan/seat shape differs (e.g. `RosterEntry` import path, an extra `AwaitingInput` step), fix the helper until the smoke test passes — this validates the harness before the resolution tests build on it.

- [ ] **Step 3: Commit**

```bash
git add crates/scenarios/tests/the_gathering_resolutions.rs
git commit -m "test: C7b harness — seat solo Roland in The Gathering (smoke)"
```

---

## Task 2: Lost resolution (all investigators defeated)

**Files:**
- Modify: `crates/scenarios/tests/the_gathering_resolutions.rs`

- [ ] **Step 1: Append the Lost test**

Append imports as needed at the call site (use fully-qualified `game_core::state::...` for `Enemy`/`EnemyId`/`Phase` to avoid a churny `use` edit). Add:

```rust
/// Lost via the real all-investigators-defeated latch: Roland is seeded
/// one hit from death with an engaged Ghoul Minion, then a real Enemy-phase
/// attack defeats him and `check_all_defeated` latches `Resolution::Lost`.
#[test]
fn enemy_attack_defeats_roland_and_latches_lost() {
    use game_core::state::{Enemy, EnemyId, Phase, Prey};

    let mut state = seated_roland();

    // Seed: Roland one hit from death (health 9 → damage 8).
    {
        let roland = state.investigators.get_mut(&INV).expect("Roland seated");
        roland.damage = roland.max_health - 1;
    }
    let loc = state.investigators[&INV]
        .current_location
        .expect("Roland is at a location");

    // Seed: a Ghoul Minion engaged with Roland at his location (attack
    // damage 1 ≥ his 1 remaining health → lethal).
    let enemy_id = EnemyId(900);
    state.enemies.insert(
        enemy_id,
        Enemy {
            id: enemy_id,
            name: "Ghoul Minion".into(),
            code: CardCode::new("01160"),
            fight: 2,
            evade: 2,
            max_health: 2,
            damage: 0,
            attack_damage: 1,
            attack_horror: 0,
            current_location: Some(loc),
            exhausted: false,
            traits: vec!["Monster".into(), "Ghoul".into()],
            engaged_with: Some(INV),
            hunter: false,
            prey: Prey::Default,
            retaliate: false,
            victory: None,
        },
    );

    // Drive: end Roland's turn → tick into the Enemy phase → the engaged
    // enemy attacks → Roland defeated → all-defeated → Resolution::Lost.
    let result = apply(state, Action::Player(PlayerAction::EndTurn));

    assert_event!(result.events, Event::AllInvestigatorsDefeated);
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert!(
        matches!(result.state.resolution, Some(Resolution::Lost { .. })),
        "expected a Lost resolution, got {:?}",
        result.state.resolution,
    );
    let _ = Phase::Enemy; // documents the phase the attack resolves in
}
```

Add `use game_core::assert_event;` to the imports at the top (replace the existing `use game_core::{...}` line to include it):

```rust
use game_core::{assert_event, Action, InputResponse, PlayerAction, RosterEntry};
```

- [ ] **Step 2: Run, expect PASS (or adjust the drive)**

Run: `cargo test -p scenarios --test the_gathering_resolutions enemy_attack_defeats_roland -- --nocapture`
Expected: PASS. If a single `EndTurn` does not reach the enemy attack (e.g. the enemy phase needs the turn-advance to cascade, or the attack opens a window), inspect `result.events`/`result.outcome` and add the minimal follow-up applies (e.g. resolve an `AwaitingInput`) until Roland is defeated and `Resolution::Lost` latches. Keep the seeds; only adjust the driving.

- [ ] **Step 3: Commit**

```bash
git add crates/scenarios/tests/the_gathering_resolutions.rs
git commit -m "test: C7b — Lost via all-investigators-defeated (real enemy attack)"
```

---

## Task 3: Won resolution (Ghoul Priest defeated)

**Files:**
- Modify: `crates/scenarios/tests/the_gathering_resolutions.rs`

- [ ] **Step 1: Add a small drive helper for the investigate→commit round-trip**

Append:

```rust
/// Drive one Investigate action through its commit window (committing
/// nothing), asserting it resolves to `Done` (no reaction in play → no
/// after-investigate window). Returns the post-commit state.
fn investigate_once(state: GameState) -> GameState {
    let paused = apply(
        state,
        Action::Player(PlayerAction::Investigate { investigator: INV }),
    );
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "Investigate should pause at the commit window, got {:?}",
        paused.outcome,
    );
    let resolved = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices: vec![] },
        }),
    );
    assert_eq!(resolved.outcome, EngineOutcome::Done);
    resolved
}
```

- [ ] **Step 2: Append the Won test**

```rust
/// Won via the real defeat→advance→win latch. Drive act 1 for real
/// (investigate the Study twice → AdvanceAct), then take the documented
/// act-2 fallback (the Hallway has 0 clues, so its round-end clue-spend has
/// no local source): seed the act deck to the terminal act and place the
/// Ghoul Priest one hit from death, then drive the defeating Fight. The win
/// itself — `act_01110`'s forced advance on the Priest's defeat — is real.
#[test]
fn defeating_the_ghoul_priest_latches_won() {
    use game_core::state::{Enemy, EnemyId, Prey};

    // --- Act 1, driven for real: 2 clues from the Study, then AdvanceAct.
    let mut state = seated_roland();
    state = investigate_once(state); // Study clues 2 → 1
    state = investigate_once(state); // Study clues 1 → 0; Roland holds 2
    assert_eq!(
        state.investigators[&INV].clues, 2,
        "two successful investigates of the Study"
    );
    let advanced = apply(
        state,
        Action::Player(PlayerAction::AdvanceAct { investigator: INV }),
    );
    assert_eq!(advanced.outcome, EngineOutcome::Done);
    let mut state = advanced.state;
    let loc = state.investigators[&INV]
        .current_location
        .expect("relocated by the act-1 reverse"); // the Hallway

    // --- Act-2 fallback (seeded): make the terminal act (01110) current and
    // place the Ghoul Priest one hit from death, engaged with Roland. The
    // act-2 round-end clue-spend + spawn is unit-tested in C3d / act_01109.
    state.act_index = state.act_deck.len() - 1; // terminal act 01110
    let priest = EnemyId(901);
    state.enemies.insert(
        priest,
        Enemy {
            id: priest,
            name: "Ghoul Priest".into(),
            code: CardCode::new("01116"),
            fight: 4,
            evade: 4,
            max_health: 5,
            damage: 4, // one hit from death
            attack_damage: 2,
            attack_horror: 2,
            current_location: Some(loc),
            exhausted: false,
            traits: vec!["Humanoid".into(), "Monster".into(), "Ghoul".into(), "Elite".into()],
            engaged_with: Some(INV),
            hunter: true,
            prey: Prey::Default,
            retaliate: true,
            victory: Some(2),
        },
    );

    // --- Drive the defeating Fight: combat 4 + Numeric(0) ≥ fight 4 →
    // success → deal 1 → damage 5 ≥ health 5 → defeated → act 3 advances →
    // Resolution::Won { R1 }.
    let paused = apply(
        state,
        Action::Player(PlayerAction::Fight {
            investigator: INV,
            enemy: priest,
        }),
    );
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "Fight should pause at the commit window, got {:?}",
        paused.outcome,
    );
    let result = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices: vec![] },
        }),
    );

    assert_event!(result.events, Event::EnemyDefeated { .. });
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert!(
        matches!(result.state.resolution, Some(Resolution::Won { .. })),
        "expected a Won resolution, got {:?}",
        result.state.resolution,
    );
}
```

- [ ] **Step 3: Run, expect PASS (adjust driving only)**

Run: `cargo test -p scenarios --test the_gathering_resolutions defeating_the_ghoul_priest -- --nocapture`
Expected: PASS. Likely adjustments, all in the *driving* (never the assertions):
- **Actions budget:** 2 investigates + AdvanceAct = 3 actions (Roland's full turn). If the third action is rejected for "no actions," the act-1 drive must span a turn boundary — inspect and insert an `EndTurn` + re-enter, or reduce to 1 investigate by seeding the second clue.
- **`Enemy`/`Prey` field names** may differ from the literals above — match the real `game_core::state::Enemy` struct (check `test_enemy` in `game_core::test_support::fixtures`).
- **`act_index` / `act_deck` field names** — confirm against `the_gathering::setup()`'s construction (Task references `state.act_index`, `state.act_deck`).
- **Fight on an Elite/Retaliate Hunter:** a *successful* Fight skips Retaliate; if the Fight rejects pre-commit (e.g. an engagement precondition), satisfy it via the seed (the Priest is already `engaged_with: Some(INV)`).

If the act-1 drive proves brittle, the spec permits seeding it too (set `state.investigators[&INV].clues` and skip straight to `AdvanceAct`, or seed `act_index` from `seated_roland()`); prefer driven, fall back to seeded, and leave a comment noting which.

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/tests/the_gathering_resolutions.rs
git commit -m "test: C7b — Won via Ghoul-Priest defeat (real defeat→advance latch)"
```

---

## Task 4: Full gauntlet + phase doc

- [ ] **Step 1: Run the full strict gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
wasm-pack test --headless --firefox crates/web
```

Expected: all green. Fix any fmt/clippy/doc issues in the test file.

- [ ] **Step 2: Open the PR (closes #245), watch CI, then update the phase doc as the final commit**

Per the repo PR procedure: branch `test/gathering-resolutions`, PR body notes the three stand-ins (controlled bag, minimal deck, seeded combat/health + the act-2 seeded fallback) and that both resolutions are engine-latched. After CI is green, flip the C7b row in `docs/phases/phase-7-the-gathering.md` to `✅ PR #NN`, mark **Slice 1 complete**, and add a short decision entry recording the hybrid approach + stand-ins. Then request merge approval.

---

## Self-review

- **Spec coverage:** Won (Task 3), Lost (Task 2), real engine-latched resolutions (assertions in both), controlled-bag + minimal-deck + seeded stand-ins (Task 1/2/3 comments), act-2 fallback (Task 3, justified by Hallway 0 clues), gauntlet + doc (Task 4). All spec sections covered.
- **Placeholders:** none — every step has concrete code or commands.
- **Type consistency:** `seated_roland()`/`investigate_once()` signatures consistent; `INV`/`ROLAND` constants reused; `Resolution::{Won,Lost}` + `Event::{ScenarioResolved,EnemyDefeated,AllInvestigatorsDefeated}` match the engine. The `Enemy`/act-field literals are flagged as "match the real struct" in the run steps because they're the one place runtime shapes must be confirmed.
