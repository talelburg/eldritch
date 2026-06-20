# Phase Anchor Frames (slice 1a) Implementation Plan

> **Status: ✅ shipped (PR #397).** All six tasks executed inline; behaviour-preserving (review-confirmed faithful), full gauntlet green. Note: Task 3 (Investigation) expanded well beyond plan — introducing the anchor made "anchor on the stack throughout a phase" a real invariant, so ~20 tests that construct mid-phase states directly needed the new `GameStateBuilder::with_phase_anchor` helper.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each game phase a real continuation-stack frame (a `*Phase` *anchor*) that owns "what runs after a framework window closes," relocating `run_window_continuation`'s six `PlayerWindow(PhaseStep)` arms onto the anchors — behaviour-preserving.

**Architecture:** Today the phase cascade runs synchronously inside handlers, and `run_window_continuation` (reaction_windows.rs) is a `match WindowKind` whose six `PlayerWindow(PhaseStep)` arms encode the phase-transition continuations. This slice introduces four `Continuation` anchor variants (`MythosPhase`/`InvestigationPhase`/`EnemyPhase`/`UpkeepPhase`), each carrying a per-phase `resume` enum naming its child-pop boundaries. At phase entry the driver pushes the anchor; before opening a framework window it sets the anchor's `resume` to the matching boundary; when that window closes, the close path routes to the anchor's `on_child_pop`, which reads `resume` and runs the relocated logic. The anchor is a real stack frame from day one (not a dispatch table). The synchronous cascade itself is untouched here (that is slice 1b); the guard ladder and action `match` are untouched (slice 1c / slice 2). Card-reaction arms of `run_window_continuation` stay put.

**Tech Stack:** Rust; `game-core` engine (event-sourced `apply`, serializable `Continuation` enum); `TestGame`/`GameStateBuilder` unit harness + `assert_event!` macros; `cargo test`/clippy/fmt/doc gauntlet.

## Global Constraints

- **Behaviour-preserving:** the entire existing engine + integration suite must stay green at every task boundary. This slice changes *structure*, not rules. No event stream changes.
- **Handler contract:** validate-first / mutate-second; on `Rejected`, state + events unchanged.
- **Serializable enum, not trait objects:** `Continuation` derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`; every new variant + `resume` enum must derive the same. No `Box<dyn …>`.
- **Closed, kernel-owned frame set:** anchors live in `game-core`; `cards`/`scenarios` never define them.
- **CI (strict):** `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- **Commit style:** `engine: <description>`; body explains *why*; ends with `Refs #393.` (this slice does not close #393).
- **Branch:** `engine/phase-anchors`.

---

## File Structure

- `crates/game-core/src/state/game_state.rs` — add the four anchor variants + four `*Resume` enums to `Continuation`; extend `as_resolution`/`as_resolution_mut` with the new arms (return `None`). Add an `on_child_pop_resume()` accessor if convenient. (~80 lines added.)
- `crates/game-core/src/engine/dispatch/phases.rs` — push the anchor at each phase entry; set `resume` before each framework-window open; add the per-phase `on_child_pop` bodies (the relocated arm logic, called from the close router). This is the bulk.
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — `run_window_continuation`'s six `PlayerWindow(PhaseStep)` arms route to the top anchor's `on_child_pop` instead of running inline; the card-reaction arms are unchanged.
- `crates/game-core/src/engine/dispatch/mod.rs` — `resolve_input`'s top-frame `match` gets four new arms (anchors never `await` input → `Rejected` "no prompt outstanding", mirroring the `EncounterCard` arm).

## Mechanism reference (read before Task 1)

The six arms and destinations (current bodies in reaction_windows.rs:966–1101):

| `PhaseStep` (window that closed) | Relocated body | Anchor / `resume` |
|---|---|---|
| `MythosAfterDraws` | skill-test-in-flight `unreachable!` guard, then `phases::mythos_phase_end(cx)` | `MythosPhase` / `MythosResume::AfterDraws` |
| `UpkeepBegins` | guard, then `phases::upkeep_resume(cx)` | `UpkeepPhase` / `UpkeepResume::Begins` |
| `BeforeInvestigatorAttacked` | guard, read `enemy_attack_pending` (`unreachable!` if `None`), `combat::resolve_attacks_for_investigator`, propagate `AwaitingInput` without advancing, else cursor-advance via `after_enemy_phase_attacks(cx, investigator)` | `EnemyPhase` / `EnemyResume::BeforeInvestigatorAttacked` |
| `AfterAllInvestigatorsAttacked` | guard, then `phases::enemy_phase_end(cx)` | `EnemyPhase` / `EnemyResume::AfterAllAttacked` |
| `InvestigationBegins` | `if let Some(id) = cursor::first_active_investigator { begin_investigator_turn(cx, id) }` else `Done` | `InvestigationPhase` / `InvestigationResume::Begins` |
| `InvestigatorTurnBegins` | `Done` | `InvestigationPhase` / `InvestigationResume::TurnBegins` |

The card-reaction arms (`AfterEnemyDefeated`/`AfterSuccessfulInvestigate`/`AfterEnteredPlay`/`AfterEnemyAttackDamagedAsset`/`BeforeEnemyAttack`/`BeforeDiscoverClues`) are **not** phase-structure — leave them exactly as-is.

The phase-entry / window-open sites that must push the anchor + set `resume` (phases.rs):
- `mythos_phase` (~354): pushes `MythosPhase` at entry; the `MythosAfterDraws` window is opened after step 1.4 — set `resume = AfterDraws` there.
- `investigation_phase` (289): push `InvestigationPhase`; `InvestigationBegins` window (300) → `resume = Begins`. `begin_investigator_turn` (321): `InvestigatorTurnBegins` window (323) → `resume = TurnBegins`.
- `enemy_phase` (531): push `EnemyPhase`; `enemy_attack_kickoff` (499) opens `BeforeInvestigatorAttacked`/`AfterAllInvestigatorsAttacked` → set the matching `resume` at each open site.
- `upkeep_phase` (617): push `UpkeepPhase`; `UpkeepBegins` window → `resume = Begins`.

**The anchor pops** when its phase transitions away. Today each phase's `*_end` helper (`mythos_phase_end`, `enemy_phase_end`, `upkeep_phase_end`, `investigation_phase_end`) emits `PhaseEnded` and calls `step_phase`/the next driver. Add a single `cx.state.continuations.pop()` of the anchor at the top of each `*_end` helper (the anchor is the top frame once its last child window has closed). Verify the anchor is the top frame with a `debug_assert!`.

---

## Task 1: Add the four anchor variants + `*Resume` enums

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (the `Continuation` enum ~417–499; `as_resolution` ~528–545; `as_resolution_mut` ~548–560)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` match ~414–438)

**Interfaces:**
- Produces:
  - `Continuation::MythosPhase { resume: MythosResume }`
  - `Continuation::InvestigationPhase { resume: InvestigationResume }`
  - `Continuation::EnemyPhase { resume: EnemyResume }`
  - `Continuation::UpkeepPhase { resume: UpkeepResume }`
  - `enum MythosResume { AfterDraws }`
  - `enum InvestigationResume { Begins, TurnBegins }`
  - `enum EnemyResume { BeforeInvestigatorAttacked, AfterAllAttacked }`
  - `enum UpkeepResume { Begins }`

- [ ] **Step 1: Write the failing test**

In `game_state.rs`'s `#[cfg(test)] mod continuation_stack_tests` (near line 1668), add:

```rust
#[test]
fn phase_anchor_variants_round_trip_and_are_not_resolution_windows() {
    use super::{Continuation, EnemyResume, InvestigationResume, MythosResume, UpkeepResume};
    let anchors = [
        Continuation::MythosPhase { resume: MythosResume::AfterDraws },
        Continuation::InvestigationPhase { resume: InvestigationResume::TurnBegins },
        Continuation::EnemyPhase { resume: EnemyResume::BeforeInvestigatorAttacked },
        Continuation::UpkeepPhase { resume: UpkeepResume::Begins },
    ];
    for a in anchors {
        // Anchors are framework frames, never reaction windows.
        assert!(a.as_resolution().is_none());
        // Serializable like every other frame.
        let json = serde_json::to_string(&a).unwrap();
        let back: Continuation = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }
}
```

- [ ] **Step 2: Run it — verify it fails to compile (variants don't exist)**

Run: `cargo test -p game-core --lib phase_anchor_variants_round_trip 2>&1 | head`
Expected: compile error, `no variant named MythosPhase`.

- [ ] **Step 3: Add the variants and enums**

In `game_state.rs`, add to the `Continuation` enum (after the `EncounterCard` variant, before the closing `}` at ~499):

```rust
    /// The Mythos phase anchor (slice 1a). Pushed at Mythos entry; sits beneath
    /// the phase's framework windows. On a child window's close the framework
    /// routes to its `on_child_pop` (keyed by `resume`). Never awaits input.
    MythosPhase { resume: MythosResume },
    /// The Investigation phase anchor (slice 1a). See [`Continuation::MythosPhase`].
    InvestigationPhase { resume: InvestigationResume },
    /// The Enemy phase anchor (slice 1a). See [`Continuation::MythosPhase`].
    EnemyPhase { resume: EnemyResume },
    /// The Upkeep phase anchor (slice 1a). See [`Continuation::MythosPhase`].
    UpkeepPhase { resume: UpkeepResume },
```

Add the four `resume` enums after the `Continuation` enum's `impl` block (near the `ChoiceFrame` definitions, ~500):

```rust
/// The Mythos-phase child-pop boundary an anchor resumes at (slice 1a). Names
/// the framework windows whose close re-enters the Mythos driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MythosResume {
    /// Post-step-1.4 (encounter draws done) window closed; run `mythos_phase_end`.
    AfterDraws,
}

/// The Investigation-phase child-pop boundary (slice 1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvestigationResume {
    /// Post-2.1 window closed; begin the first investigator's turn.
    Begins,
    /// Post-2.2 turn-begins window closed; the investigator now acts (no
    /// continuation work — slice 2 makes this an `InvestigatorTurn` frame).
    TurnBegins,
}

/// The Enemy-phase child-pop boundary (slice 1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnemyResume {
    /// Before-investigator-attacked window closed; resolve this investigator's
    /// attacks (step 3.3).
    BeforeInvestigatorAttacked,
    /// After-all-investigators-attacked window closed; run `enemy_phase_end`.
    AfterAllAttacked,
}

/// The Upkeep-phase child-pop boundary (slice 1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpkeepResume {
    /// Post-4.1 window closed; run `upkeep_resume` (steps 4.2–4.6).
    Begins,
}
```

Extend `as_resolution` and `as_resolution_mut` (the long match arms that return `None`): add `Continuation::MythosPhase { .. } | Continuation::InvestigationPhase { .. } | Continuation::EnemyPhase { .. } | Continuation::UpkeepPhase { .. }` to the existing `None`-returning arm list in both.

In `dispatch/mod.rs` `resolve_input` (the `match cx.state.continuations.last()`), add an arm before `None`:

```rust
        // Phase anchors (slice 1a) never await input — they only sit beneath
        // framework windows. If one is somehow top, no prompt is outstanding.
        Some(
            Continuation::MythosPhase { .. }
            | Continuation::InvestigationPhase { .. }
            | Continuation::EnemyPhase { .. }
            | Continuation::UpkeepPhase { .. },
        ) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (a phase anchor is top)".into(),
        },
```

- [ ] **Step 4: Run — verify the test passes and the workspace compiles**

Run: `cargo test -p game-core --lib phase_anchor_variants_round_trip 2>&1 | tail`
Expected: PASS. Then `cargo build -p game-core 2>&1 | tail` — expect clean (all exhaustive `Continuation` matches updated; if the compiler flags another non-exhaustive match, add the four anchors to its `None`/unreachable arm).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: add *Phase anchor Continuation variants + resume enums (1a)

Inert frames so far — no driver pushes them yet. Refs #393."
```

---

## Task 2: Relocate the Mythos arm + push/pop the `MythosPhase` anchor

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (`mythos_phase` ~354; `mythos_phase_end` ~ its definition; the `MythosAfterDraws` window-open site)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`MythosAfterDraws` arm ~969)
- Test: `crates/game-core/src/engine/dispatch/phases.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `phases::anchor_on_child_pop(cx: &mut Cx) -> EngineOutcome` — reads the top `*Phase` anchor's `resume` and runs the relocated body. Added incrementally (Mythos arm first; later tasks extend its match).
- Consumes: `Continuation::MythosPhase`, `MythosResume` (Task 1).

- [ ] **Step 1: Write the failing test**

Add to `phases.rs` tests:

```rust
#[test]
fn mythos_anchor_on_stack_during_phase_and_popped_at_end() {
    // A Mythos phase pushes its anchor; after the post-1.4 window closes and
    // mythos_phase_end runs, the anchor is gone and we've moved to Investigation.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Upkeep) // step_phase Upkeep->Mythos enters mythos_phase
        .build();
    state.round = 1;
    let mut events = Vec::new();
    // Drive Upkeep->Mythos so mythos_phase runs and pushes the anchor.
    let _ = step_phase(&mut Cx { state: &mut state, events: &mut events });
    assert!(
        state.continuations.iter().any(|c| matches!(c, Continuation::MythosPhase { .. })),
        "MythosPhase anchor pushed during the Mythos phase",
    );
}
```

(Adjust the build/`step_phase` drive to match the existing `round_increments_on_mythos_entry_via_driver` test's setup if the encounter-draw prompt requires an encounter deck; reuse that test's fixture pattern.)

- [ ] **Step 2: Run — verify it fails**

Run: `cargo test -p game-core --lib mythos_anchor_on_stack 2>&1 | tail`
Expected: FAIL (no anchor on the stack — nothing pushes it yet).

- [ ] **Step 3: Push the anchor, set `resume`, relocate the arm**

In `phases.rs` `mythos_phase`, immediately after `cx.events.push(Event::PhaseStarted { phase: Phase::Mythos });`, push the anchor:

```rust
    cx.state
        .continuations
        .push(crate::state::Continuation::MythosPhase {
            resume: crate::state::MythosResume::AfterDraws,
        });
```

(`AfterDraws` is the only Mythos boundary, so it can be set at entry.)

In `phases.rs`, add the relocated body as the first arm of a new `anchor_on_child_pop`:

```rust
/// Run the top `*Phase` anchor's continuation after one of its framework
/// windows closed (slice 1a). The window has already been popped by the
/// close path; the anchor is now the top frame. Reads the anchor's `resume`
/// to pick the relocated body. Suspension-agnostic: a body that itself
/// suspends returns `AwaitingInput` unchanged.
pub(super) fn anchor_on_child_pop(cx: &mut Cx) -> EngineOutcome {
    use crate::state::{Continuation, MythosResume};
    match cx.state.continuations.last() {
        Some(Continuation::MythosPhase { resume: MythosResume::AfterDraws }) => {
            if let Some(in_flight) = cx.state.current_skill_test() {
                unreachable!(
                    "MythosAfterDraws closed while a skill test is in flight \
                     (continuation={:?}); Phase 4 has no Mythos-phase skill-test sources",
                    in_flight.continuation,
                );
            }
            mythos_phase_end(cx)
        }
        other => unreachable!("anchor_on_child_pop: top frame is not a known phase anchor: {other:?}"),
    }
}
```

In `mythos_phase_end`, pop the anchor as the first statement (it is the top frame once `MythosAfterDraws` closed):

```rust
    debug_assert!(
        matches!(cx.state.continuations.last(), Some(crate::state::Continuation::MythosPhase { .. })),
        "mythos_phase_end: expected MythosPhase anchor on top",
    );
    cx.state.continuations.pop();
```

In `reaction_windows.rs`, replace the `MythosAfterDraws` arm body (the guard + `mythos_phase_end(cx)`) with:

```rust
            PhaseStep::MythosAfterDraws => super::phases::anchor_on_child_pop(cx),
```

- [ ] **Step 4: Run — verify the new test + the full Mythos suite pass**

Run:
```bash
cargo test -p game-core --lib 'dispatch::phases'
cargo test -p game-core mythos
cargo test -p scenarios --test mythos_phase
```
Expected: all PASS (behaviour-preserving — the anchor pop + relocation produce the identical cascade and event stream).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/phases.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: Mythos phase anchor + relocate MythosAfterDraws (1a)

Refs #393."
```

---

## Task 3: Relocate the Investigation arms + anchor (`InvestigationBegins`, `InvestigatorTurnBegins`)

**Files:** `phases.rs` (`investigation_phase` ~289, `begin_investigator_turn` ~321, `investigation_phase_end` ~339), `reaction_windows.rs` (the two arms ~1073, ~1100).

**Interfaces:** Consumes `Continuation::InvestigationPhase`, `InvestigationResume`. Extends `anchor_on_child_pop` with the Investigation arms.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn investigation_anchor_on_stack_through_turn() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([id])
        .with_active_investigator(id)
        .with_phase(Phase::Mythos)
        .build();
    let mut events = Vec::new();
    // Mythos->Investigation via step_phase enters investigation_phase.
    // (Use the existing investigation_phase test fixture if a cleaner entry exists.)
    investigation_phase(&mut Cx { state: &mut state, events: &mut events });
    assert!(
        state.continuations.iter().any(|c| matches!(c, Continuation::InvestigationPhase { .. })),
        "InvestigationPhase anchor pushed",
    );
}
```

- [ ] **Step 2: Run — verify it fails**

Run: `cargo test -p game-core --lib investigation_anchor_on_stack 2>&1 | tail`
Expected: FAIL.

- [ ] **Step 3: Push anchor + set resume + relocate both arms**

In `investigation_phase`, after `PhaseStarted(Investigation)` push:
```rust
    cx.state.continuations.push(crate::state::Continuation::InvestigationPhase {
        resume: crate::state::InvestigationResume::Begins,
    });
```
In `begin_investigator_turn`, before opening the `InvestigatorTurnBegins` window, set the anchor's resume:
```rust
    if let Some(crate::state::Continuation::InvestigationPhase { resume }) =
        cx.state.continuations.iter_mut().rev().find(|c| matches!(c, crate::state::Continuation::InvestigationPhase { .. }))
    {
        *resume = crate::state::InvestigationResume::TurnBegins;
    }
```
Add the Investigation arms to `anchor_on_child_pop`'s match:
```rust
        Some(Continuation::InvestigationPhase { resume: InvestigationResume::Begins }) => {
            if let Some(id) = super::cursor::first_active_investigator(cx.state) {
                begin_investigator_turn(cx, id);
            }
            EngineOutcome::Done
        }
        Some(Continuation::InvestigationPhase { resume: InvestigationResume::TurnBegins }) => {
            // 2.2.1 — the investigator acts as player-driven input; no
            // continuation work (slice 2 makes this an InvestigatorTurn frame).
            EngineOutcome::Done
        }
```
In `investigation_phase_end`, pop the anchor (debug_assert it is top) before `PhaseEnded(Investigation)` + `step_phase`.
In `reaction_windows.rs`, replace both arms' bodies with `=> super::phases::anchor_on_child_pop(cx),`.

- [ ] **Step 4: Run**
```bash
cargo test -p game-core --lib 'dispatch::phases'
cargo test -p scenarios --test the_gathering
```
Expected: PASS.

- [ ] **Step 5: Commit** `engine: Investigation phase anchor + relocate its two arms (1a). Refs #393.`

---

## Task 4: Relocate the Enemy arms + anchor (`BeforeInvestigatorAttacked`, `AfterAllInvestigatorsAttacked`)

**Files:** `phases.rs` (`enemy_phase` ~531, `enemy_attack_kickoff` ~499, `enemy_phase_end`), `reaction_windows.rs` (arms ~1006, ~1059).

**Interfaces:** Consumes `Continuation::EnemyPhase`, `EnemyResume`. Extends `anchor_on_child_pop`. **Note** the `BeforeInvestigatorAttacked` arm's mid-loop `AwaitingInput` propagation (soak window) must be preserved exactly — copy the body verbatim from reaction_windows.rs:1006–1057 into the anchor arm, replacing only the surrounding `match`.

- [ ] **Step 1: Write the failing test** — anchor present during the enemy phase (mirror Task 3's shape, entering via `enemy_phase`).
- [ ] **Step 2: Run — fails.**
- [ ] **Step 3:** Push `EnemyPhase` anchor in `enemy_phase` after `PhaseStarted(Enemy)`. In `enemy_attack_kickoff`, set `resume = BeforeInvestigatorAttacked` before the `BeforeInvestigatorAttacked` open and `resume = AfterAllAttacked` before the `AfterAllInvestigatorsAttacked` open (and at the advance site in the relocated body). Copy the two arm bodies (verbatim, incl. the skill-test guards, the `enemy_attack_pending` read, and the no-advance `AwaitingInput` propagation) into `anchor_on_child_pop`. Pop the anchor in `enemy_phase_end`. Route both reaction_windows arms to `anchor_on_child_pop`.
- [ ] **Step 4: Run** `cargo test -p game-core 'dispatch'` + `cargo test -p scenarios --test the_gathering` + the enemy-phase engagement tests. Expected: PASS.
- [ ] **Step 5: Commit** `engine: Enemy phase anchor + relocate its two arms (1a). Refs #393.`

---

## Task 5: Relocate the Upkeep arm + anchor (`UpkeepBegins`)

**Files:** `phases.rs` (`upkeep_phase` ~617, `upkeep_resume` ~633, `upkeep_phase_end`), `reaction_windows.rs` (arm ~991).

**Interfaces:** Consumes `Continuation::UpkeepPhase`, `UpkeepResume`. Extends `anchor_on_child_pop`.

- [ ] **Step 1: Write the failing test** — `UpkeepPhase` anchor present during Upkeep (mirror Task 2).
- [ ] **Step 2: Run — fails.**
- [ ] **Step 3:** Push `UpkeepPhase { resume: Begins }` in `upkeep_phase` after `PhaseStarted(Upkeep)`. Add the `UpkeepResume::Begins => { guard; upkeep_resume(cx) }` arm to `anchor_on_child_pop`. Pop the anchor at the top of `upkeep_phase_end` (it was made `pub(crate)` in slice 0 — the pop is the first statement, `debug_assert!` it is top). Route the reaction_windows `UpkeepBegins` arm to `anchor_on_child_pop`.
  - **Caution:** verify the anchor is the top frame at `upkeep_phase_end` *and* on the `resume_act_round_end_advance` → `upkeep_round_end_at_and_after` path (slice 0). The anchor is pushed at Upkeep entry and the `ActRoundEnd` window is pushed *above* it later, so by the time `upkeep_phase_end`/teardown runs, the anchor is top once the `ActRoundEnd` frame has popped. If the pop site is ambiguous, pop in `upkeep_round_end_teardown` (the single Upkeep→Mythos exit) instead of `upkeep_phase_end`, and `debug_assert!` there.
- [ ] **Step 4: Run** `cargo test -p game-core upkeep` + `cargo test -p cards --test theyre_getting_out` + `cargo test -p scenarios --test upkeep_phase --test upkeep_hand_size`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: Upkeep phase anchor + relocate UpkeepBegins (1a). Refs #393.`

---

## Task 6: Tidy `run_window_continuation` + the slice gauntlet

**Files:** `reaction_windows.rs` (the now-uniform `PlayerWindow(PhaseStep)` arm), spec.

- [ ] **Step 1:** With all six arms routing to `anchor_on_child_pop`, collapse the `WindowKind::PlayerWindow(step) => match step { … }` into `WindowKind::PlayerWindow(_) => super::phases::anchor_on_child_pop(cx)` — the `PhaseStep` is no longer the continuation key (the anchor's `resume` is). Keep the `WindowKind::PlayerWindow` variant itself (it still names the window; slice 1b removes the step). Update the doc-comment to say the phase-structure continuation now lives on the `*Phase` anchors.
- [ ] **Step 2: Run the full gauntlet**
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```
Expected: all green.
- [ ] **Step 3:** Update spec §"Sequencing" 1a line to `✅ shipped (PR #NN)` and the `What "done" looks like` note that `run_window_continuation`'s phase-structure arms are now anchor-owned (the `WindowKind` table for *card reactions* remains until later slices). Commit, push, open the PR (`engine/phase-anchors`), watch CI.

---

## Self-Review

**Spec coverage (1a):** Task 1 adds the four anchor variants + resume enums; Tasks 2–5 relocate all six `PhaseStep` arms onto the anchors and push/pop the anchors at phase entry/exit; Task 6 collapses the dispatcher and runs the gauntlet. Guard ladder, action `match`, and card-reaction arms are explicitly untouched (deferred to 1b/1c/slice 2), matching the spec's 1a scope. ✓

**Placeholder scan:** the relocation bodies are specified by exact source location + the instruction to copy verbatim (Task 4's `BeforeInvestigatorAttacked` body is the one large block — copied, not paraphrased). Tasks 3–5 abbreviate the TDD step prose (mirroring Task 2's fully-shown pattern) but give exact code for every *new* construct; the test-fixture entry is flagged to reuse the existing per-phase test setup rather than invent one. No "TODO"/"handle edge cases". ⚠ If executing strictly TDD, expand Tasks 3–5 Step-1 tests against the actual fixtures as you reach them.

**Type consistency:** `anchor_on_child_pop(cx)` signature is stable across Tasks 2–6; the four `*Resume` enums and their variants are referenced identically everywhere; `Continuation::*Phase { resume }` field name `resume` is consistent. ✓

**Behaviour-preserving check:** every relocated body is the current arm body unchanged (including the `unreachable!` skill-test guards and the soak-window `AwaitingInput` propagation); the only new runtime effect is the anchor push at entry + pop at exit, which is invisible to the event stream and to `resolve_input` (anchors reject as "no prompt"). The existing suite is the regression oracle at every task boundary. ✓
