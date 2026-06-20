# Unified Main Loop + Cascade-Fold (slice 1b) Implementation Plan

> **Status: ✅ shipped (PR #398).** Executed inline as 6 commits; behaviour-preserving, full gauntlet green, review APPROVE (`drive` proven non-spinning). Deviation from plan: the per-phase migration used driver *reuse* (`advance(Entry)` = pop placeholder + call the existing driver) rather than moving driver bodies, and Tasks 3+4 merged (the `step_phase` retirement touches all its call sites at once). Review surfaced a beneficial behaviour change — the unified guard now gates `Choice`/`SubstitutionPrompt` (a latent hole the old ladder missed) — and a `*Phase`-entry unification follow-up (`start_scenario`/`resume_mulligan`).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the forward-calling synchronous phase cascade (`step_phase` → driver → `*_phase_end` → `step_phase`) with a uniform `drive(cx)` loop that advances the top continuation frame, so phase transitions are loop-driven (pop anchor + push next anchor) rather than native-stack recursion — and the guard ladder + strand-guards collapse out of it. Behaviour-preserving.

**Architecture:** After slice 1a each phase is a `*Phase` anchor frame whose `resume`-keyed body runs on window close (`anchor_on_child_pop`). This slice makes those anchors **self-driving**: each `*Resume` gains an `Entry` variant; a new `advance(cx)` (the generalized `anchor_on_child_pop`) handles `Entry` (the phase opening — today's `mythos_phase`/… body), the boundary chunks (today's bodies), and **transitions** (pop self + advance `state.phase` + push next anchor `{Entry}` — replacing `*_phase_end`'s synchronous `step_phase`). A `drive(cx)` loop in the `apply` entry advances the top frame until it (a) hits a suspension awaiting input → `AwaitingInput`, (b) reaches the open turn (`InvestigationPhase{TurnBegins}`) → idle `Done`, or (c) reaches terminal → `Done`. `step_phase` and the four `*_phase_end` functions dissolve. The guard ladder collapses to "top frame is a non-anchor suspension ⇒ only `ResolveInput`"; the strand-guards become `debug_assert!` (a skill test sits *above* its phase anchor, so the loop never advances the anchor while one is in flight).

**Tech Stack:** Rust; `game-core` engine (event-sourced `apply` with transactional snapshot); serializable `Continuation` enum; `GameStateBuilder` + `with_phase_anchor` test harness; `cargo test`/clippy/fmt/doc/wasm gauntlet.

## Global Constraints

- **Behaviour-preserving:** the entire workspace suite + gauntlet must stay green at every task boundary. No event-stream changes.
- **Handler contract:** validate-first / mutate-second; `Rejected` ⇒ state + events unchanged (the `apply` transactional snapshot must keep covering the whole loop).
- **Serializable enum:** new `Entry` resume variants derive `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`.
- **Determinism:** the loop runs entirely within one `apply`; it must produce the identical event sequence the synchronous cascade did.
- **CI (strict):** `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`.
- **Commit style:** `engine: <description>`; body explains *why*; ends with `Refs #393.` (does not close #393).
- **Branch:** `engine/unified-main-loop`.

## File Structure

- `crates/game-core/src/state/game_state.rs` — add `Entry` to the four `*Resume` enums.
- `crates/game-core/src/engine/dispatch/phases.rs` — the bulk: generalize `anchor_on_child_pop` → `advance` with `Entry` + transition arms; fold the four phase drivers + four `*_phase_end` bodies into it; delete `step_phase`.
- `crates/game-core/src/engine/dispatch/mod.rs` — add the `drive(cx)` loop; run it from `apply_player_action`; collapse the guard ladder.
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — `run_window_continuation`'s `PlayerWindow(_)` arm calls `advance` (rename follow-through).
- Tests across `game-core`/`cards`/`scenarios` — fixtures that call `step_phase`/`*_phase_end` directly migrate to `advance`/`drive`; the `with_phase_anchor` helper absorbs mid-phase-state needs.

## Mechanism reference (read before Task 1)

**The loop.** `drive(cx)` after the action dispatch:
```text
loop {
    match cx.state.continuations.last() {
        None => return Done,                              // terminal / bootstrap
        Some(f) if f.awaits_input() => return AwaitingInput{…},  // suspension on top
        Some(f) if f.is_open_turn() => return Done,       // InvestigationPhase{TurnBegins} idle
        Some(_phase_anchor) => match advance(cx) {        // advance the phase anchor
            AwaitingInput{…} => return it,                // a child suspended mid-advance
            Done => continue,                             // progressed; loop
            Rejected{…} => return it,
        }
    }
}
```
- `awaits_input()` = "top frame is a suspension" = **not** a `*Phase` anchor. (Anchors are the only inert frames; everything else on top awaits input.)
- `is_open_turn()` = `InvestigationPhase{TurnBegins}` (the pre-slice-2 idle). Distinct from other anchor states because the engine waits for a *typed* player action here, not a `ResolveInput`.

**`advance(cx)`** = today's `anchor_on_child_pop` generalized. Match on the top anchor's `(phase, resume)`:
- `Entry` → run the phase opening (the current `mythos_phase`/`investigation_phase`/`enemy_phase`/`upkeep_phase` body: `PhaseStarted` + straight-line steps + push first child), setting `resume` to the first boundary.
- boundary (`AfterDraws`, `BeforeInvestigatorAttacked`, `AfterAllAttacked`, `Begins`, `TurnBegins`) → today's relocated body.
- transition (a phase's terminal chunk — today's `*_phase_end` tail) → emit `PhaseEnded`, pop self, set `state.phase = phase.next()`, push the next phase's anchor `{Entry}`, return `Done` (the loop advances it). The `PhaseEnded`-keyed forced emit + the Enemy/Upkeep round-end machinery stay exactly as today, just relocated.

**Entry points that push the *first* phase anchor** (today they call `step_phase`/a driver): `start_scenario`'s kickoff, and the bootstrap. After this slice, "enter phase X" = push `XPhase{Entry}` and let `drive` advance it.

**The open-turn idle.** `InvestigationPhase{TurnBegins}` advance is a no-op today (the player acts via typed actions). The loop must *not* call `advance` on it (would loop forever) — hence the explicit `is_open_turn()` break. Slice 2 replaces this with the real `InvestigatorTurn` frame that `awaits_input()`.

---

## Task 1: Add `Entry` resume variants + the `drive` loop scaffold (Mythos self-driving first)

**Files:** `game_state.rs` (`*Resume` enums), `phases.rs` (`advance`, Mythos fold, `mythos_phase_end` → transition), `mod.rs` (`drive` loop + call site).

**Interfaces:**
- Produces: `MythosResume::Entry` (+ `Entry` on the other three enums, unused until their tasks); `phases::advance(cx: &mut Cx) -> EngineOutcome` (renamed/generalized `anchor_on_child_pop`); `dispatch::drive(cx: &mut Cx, outcome: EngineOutcome) -> EngineOutcome`.
- Consumes: the slice-1a anchors + `anchor_on_child_pop` body.

- [ ] **Step 1: Write the failing test** — a Mythos phase entered via `MythosPhase{Entry}` + `drive` produces the same event stream as the old `mythos_phase`:

```rust
#[test]
fn mythos_drives_from_entry_via_the_loop() {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_investigator(1))
        .with_phase(Phase::Mythos)
        .with_phase_anchor(crate::state::Continuation::MythosPhase {
            resume: crate::state::MythosResume::Entry,
        })
        .build();
    state.turn_order = vec![InvestigatorId(1)];
    let mut events = Vec::new();
    // drive advances the Entry anchor: emits PhaseStarted(Mythos), pushes the
    // EncounterDraw loop, and suspends at the first drawer prompt.
    let outcome = drive(
        &mut Cx { state: &mut state, events: &mut events },
        EngineOutcome::Done,
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(events.iter().any(|e| matches!(e, Event::PhaseStarted { phase: Phase::Mythos })));
    assert!(state.continuations.iter().any(|c| matches!(c, crate::state::Continuation::EncounterDraw { .. })));
}
```

- [ ] **Step 2: Run it — verify it fails** (`MythosResume::Entry` and `drive` don't exist). `cargo test -p game-core --lib mythos_drives_from_entry 2>&1 | head`.

- [ ] **Step 3: Implement.**
  - Add `Entry` to all four `*Resume` enums (game_state.rs), deriving as the existing variants.
  - In phases.rs, rename `anchor_on_child_pop` → `advance` (update its `run_window_continuation` caller). Add the `MythosPhase { resume: Entry }` arm = the current `mythos_phase` body (emit `PhaseStarted(Mythos)`, round bump moved here, steps 1.1–1.3, push `EncounterDraw` or open the no-drawer `MythosAfterDraws` window), setting `resume = AfterDraws` where it pushes the loop. Change the `MythosPhase { resume: AfterDraws }` arm (today's `mythos_phase_end` body) to **transition via pop+push**: emit `PhaseEnded(Mythos)`, pop the anchor, `state.phase = Phase::Investigation`, push `InvestigationPhase { resume: Entry }`, return `Done`.
  - Add `advance(InvestigationPhase{Entry})` = the current `investigation_phase` body (so the loop can advance the pushed Investigation anchor). Leave Investigation's *other* arms + its `*_phase_end` synchronous for now (coexistence — Investigation→Enemy still uses `step_phase`).
  - Add `dispatch::drive(cx, outcome)`: if `outcome` is `AwaitingInput`/`Rejected`, return it; else run the loop above. Implement `Continuation::awaits_input()` (= not a `*Phase` anchor) and `is_open_turn()` (= `InvestigationPhase{TurnBegins}`) as methods on `Continuation`.
  - In `apply_player_action`, wrap the action `match`'s `outcome` in `drive(cx, outcome)` before returning (so any phase anchor left on top by a handler is advanced).
  - Keep `mythos_phase`/`step_phase` callable for the not-yet-migrated phases (Investigation/Enemy/Upkeep still cascade synchronously; only Mythos→Investigation is loop-driven this task).

- [ ] **Step 4: Run** `cargo test -p game-core --lib mythos` + `cargo test -p scenarios --test mythos_phase` + the_gathering. Expected PASS (behaviour-preserving). Fix the mid-phase fixtures the new `drive` path touches.

- [ ] **Step 5: Commit** `engine: drive loop + Mythos self-driving from Entry (1b). Refs #393.`

---

## Task 2: Investigation transition loop-driven

**Files:** `phases.rs`.

- [ ] **Step 1: Failing test** — `InvestigationPhase{Entry}` driven by `drive` rotates to the first turn and reaches the open-turn idle (`Done`, anchor `TurnBegins` on top), mirroring the old `investigation_phase` + cascade.
- [ ] **Step 2: Run — fails.**
- [ ] **Step 3:** Change `investigation_phase_end` (its body now an `advance` transition arm reached from the last turn's `EndTurn`) to pop the `InvestigationPhase` anchor, emit `PhaseEnded(Investigation)`, `state.phase = Enemy`, push `EnemyPhase{Entry}`, return `Done`. Add `advance(EnemyPhase{Entry})` = current `enemy_phase` body. Update `end_turn`'s terminal branch to route through `advance`/the transition rather than `investigation_phase_end → step_phase`. Verify the open-turn idle: when `begin_investigator_turn` leaves `InvestigationPhase{TurnBegins}` on top, `drive` breaks with `Done` (the `is_open_turn` arm) — the engine waits for the player's typed action.
- [ ] **Step 4: Run** `cargo test -p game-core --lib 'dispatch::phases'` + the_gathering + `end_turn` tests. PASS.
- [ ] **Step 5: Commit** `engine: Investigation transition loop-driven (1b). Refs #393.`

---

## Task 3: Enemy transition loop-driven

**Files:** `phases.rs`.

- [ ] **Step 1: Failing test** — `EnemyPhase{Entry}` via `drive` runs hunters + attack loop and transitions to Upkeep, mirroring the old cascade (use the hunter-tie fixture; assert the `UpkeepPhase{Entry}` push at the end).
- [ ] **Step 2: Run — fails.**
- [ ] **Step 3:** Change `enemy_phase_end`'s body (now an `advance` transition arm) to pop the `EnemyPhase` anchor, emit `PhaseEnded(Enemy)` + its forced emit (unchanged), `state.phase = Upkeep`, push `UpkeepPhase{Entry}`, return `Done`. Add `advance(UpkeepPhase{Entry})` = current `upkeep_phase` body. Confirm the soak/cancel-window resume path (`resume_enemy_attack` → `after_enemy_phase_attacks`) still drives the per-investigator loop correctly beneath the anchor.
- [ ] **Step 4: Run** the enemy-phase + combat + hunter suites + the_gathering. PASS.
- [ ] **Step 5: Commit** `engine: Enemy transition loop-driven (1b). Refs #393.`

---

## Task 4: Upkeep transition loop-driven + delete `step_phase`

**Files:** `phases.rs`.

- [ ] **Step 1: Failing test** — `UpkeepPhase{Entry}` via `drive` runs 4.2–4.6 + the round-end sequence and transitions to Mythos (`MythosPhase{Entry}` pushed; round bump fires on the Mythos `Entry` advance). Cover the act round-end window path (slice 0).
- [ ] **Step 2: Run — fails.**
- [ ] **Step 3:** Change `upkeep_round_end_teardown` (the single Upkeep→Mythos exit) to pop the `UpkeepPhase` anchor, `state.phase = Mythos`, push `MythosPhase{Entry}`, return `Done` (the loop advances it — running the round bump + `PhaseStarted(Mythos)`). With all four transitions loop-driven, **delete `step_phase`** (now unreferenced) and its `from == to` unreachable. Verify `start_scenario` pushes the first phase anchor `{Entry}` instead of calling a driver/`step_phase`.
- [ ] **Step 4: Run** the full workspace suite. PASS. (Expect the largest fixture tail here — tests that called `step_phase` directly must switch to pushing an `{Entry}` anchor + `drive`.)
- [ ] **Step 5: Commit** `engine: Upkeep transition loop-driven; delete step_phase (1b). Refs #393.`

---

## Task 5: Collapse the guard ladder

**Files:** `mod.rs`.

- [ ] **Step 1: Failing test** — assert a representative pending state (e.g. a `Choice` frame on top) rejects a non-`ResolveInput` action with the unified reason, and that an open-turn state (`InvestigationPhase{TurnBegins}` on top) *accepts* a typed `Move`/`EndTurn`.
- [ ] **Step 2: Run — fails** (the new unified reason string isn't produced yet, or behaviour differs).
- [ ] **Step 3:** Replace the 8 `if matches!(top, Pending::X) && !ResolveInput { reject }` blocks (mod.rs:61–189) with one:
  ```rust
  if let Some(top) = cx.state.continuations.last() {
      if top.awaits_input() && !matches!(action, PlayerAction::ResolveInput { .. }) {
          return EngineOutcome::Rejected {
              reason: "a prompt is outstanding; submit a PlayerAction::ResolveInput \
                       (see the AwaitingInput request for the expected InputResponse)".into(),
          };
      }
  }
  ```
  (`awaits_input()` is false for `*Phase` anchors, so the open turn still allows typed actions.) Keep `StartScenario`'s own gate if it has one.
- [ ] **Step 4: Run** the full workspace suite. PASS. (Reason-string assertions in existing rejection tests may need updating to the unified message — setup/expectation only, not behaviour.)
- [ ] **Step 5: Commit** `engine: collapse the guard ladder onto the top-frame rule (1b). Refs #393.`

---

## Task 6: Dissolve the strand-guards + gauntlet + PR

**Files:** `phases.rs`, spec/plan docs.

- [ ] **Step 1:** Downgrade the `unreachable!("…would strand the test in the wrong phase…")` skill-test-in-flight guards in `advance` to `debug_assert!(cx.state.current_skill_test().is_none(), …)` — they are now genuinely impossible (a skill test sits above its phase anchor, so `drive` never advances the anchor while one is in flight), so the comment changes from "corpus-absent" to "structurally impossible under the loop."
- [ ] **Step 2: Full gauntlet:**
  ```bash
  RUSTFLAGS="-D warnings" cargo test --all --all-features
  cargo clippy --all-targets --all-features -- -D warnings
  cargo fmt --check
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
  cargo build -p web --target wasm32-unknown-unknown
  ```
  Expected: all green.
- [ ] **Step 3:** Update spec §"Sequencing" 1b → `✅ shipped (PR #NN)` and the `What "done" looks like` note (`step_phase`/guard-ladder gone; transitions loop-driven). Add a plan status note. Commit, push, open the PR, watch CI. Request a review (this is a high-blast-radius change executed inline).

---

## Self-Review

**Spec coverage (1b merged):** Tasks 1–4 build the loop + fold all four transitions onto it (deleting `step_phase` + the `*_phase_end` functions); Task 5 collapses the guard ladder; Task 6 dissolves the strand-guards + ships. Matches the merged spec sequencing entry. ✓

**Placeholder scan:** the per-phase folds reference the *current* driver / `*_phase_end` bodies by name (copied, not paraphrased — the implementer reads each as they reach it). The `drive` loop, `awaits_input()`/`is_open_turn()`, and the guard-ladder replacement are shown in full. ⚠ Tasks 2–4's per-phase Step-1 tests are specified by pattern; expand them against the actual fixtures (mirroring Task 1's fully-shown test) at execution time.

**Type consistency:** `advance(cx) -> EngineOutcome` and `drive(cx, outcome) -> EngineOutcome` signatures are stable across tasks; `Entry` is added to all four enums in Task 1 (used in later tasks); `awaits_input()`/`is_open_turn()` are `Continuation` methods referenced identically in `drive` and the guard collapse. ✓

**Risk note:** highest-blast-radius slice in the arc. The coexistence states (Tasks 1–3, where some transitions are loop-driven and others synchronous) are the trickiest — verify the suite is green at *each* per-phase task, not just at the end. The transactional `apply` snapshot must still wrap the whole `drive` loop (a `Rejected` mid-loop restores cleanly) — confirm in Task 1.
