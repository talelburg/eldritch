# Delete apply_effect/drive_effect_to_base Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retire the last synchronous effect drivers — migrate `resume_effect_walk` off `drive_effect_to_base` to the global drive loop, then delete `apply_effect` and `drive_effect_to_base`, reworking their ~30 test callers onto the real `drive`.

**Architecture:** `resume_effect_walk` is the last production caller of `drive_effect_to_base`; it becomes "return `Done`, let the global `drive` loop step the parked effect frames" (the established Slice D pattern — `apply_player_action` runs `drive(cx, outcome)` on every `resolve_input` result). With no production caller left, both wrappers delete; their `#[cfg(test)]` callers move onto a thin test-only `run` helper (`push_effect` + `drive`).

**Tech Stack:** Rust, the `game-core` engine crate. No new dependencies.

## Global Constraints

- **Match CI's strict flags before declaring any task done** (copy verbatim):
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Behaviour-preserving at the `apply` boundary**: `crates/cards/*` and `crates/game-core/tests/*` go through real `apply`/`drive` and MUST stay green **untouched**. The in-crate `#[cfg(test)]` evaluator/choice tests are reworked (only their `apply_effect` call changes to `run`); their assertions stay identical (`Done` stays `Done`, `AwaitingInput` stays `AwaitingInput`).
- **`apply_effect` and `drive_effect_to_base` are DELETED, not demoted.** `frame_of`, `step_effect_frame`, `push_effect`, and the global `drive` loop's `Continuation::Effect(_)` arm stay (production internals).
- **Each task is its own commit and keeps the full strict gauntlet green and bisectable.** One PR (the final Slice D / #423 task on `engine/effect-callsite-migration`). After this, the branch is mergeable.
- **Commit trailers** (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB
  ```
- Original arc design: `docs/superpowers/specs/2026-06-23-effect-frame-callsite-migration-design.md` (this is its final task).

## File structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/game-core/src/engine/dispatch/choice.rs` | migrate `resume_effect_walk`; rework its 1 test call | 1 |
| `crates/game-core/src/engine/evaluator.rs` | delete `apply_effect` + `drive_effect_to_base`; add the test-only `run` helper; rework ~30 test calls | 1 |
| `docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md` | flip the #423 row to done | 2 |
| `docs/superpowers/plans/2026-06-24-slice-d-handoff.md` | mark Task 5 / #423 complete | 2 |

---

## Task 1: Migrate `resume_effect_walk` + delete the wrappers + rework tests

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/choice.rs`
- Modify: `crates/game-core/src/engine/evaluator.rs`

**Interfaces:**
- Consumes: `push_effect(cx, &Effect, EvalContext)`, `crate::engine::dispatch::drive(cx, EngineOutcome) -> EngineOutcome`, `step_effect_frame` (unchanged).
- Produces: nothing public. A `#[cfg(test)]` helper `fn run(cx: &mut Cx, effect: &Effect, ctx: EvalContext) -> EngineOutcome` in `evaluator.rs`'s test module (same signature as the deleted `apply_effect`, so the rework is a uniform call-rename).

- [ ] **Step 1: Establish the behaviour baseline (note green, do not edit)**

`resume_effect_walk`'s migration is behaviour-preserving; the suspending-choice suites are its regression net. Run them and note green:
```bash
cargo test -p cards --test revelation_treacheries   # Crypt Chill 01167 on_fail ChooseOne suspend/resume
cargo test -p game-core --lib engine::dispatch::choice
cargo test -p cards --test non_attack_soak          # resume_damage_assignment → resume_effect_walk
```
Expected: PASS. These must stay green after the migration (they exercise `resolve_input → resume_effect_choice/resume_damage_assignment → resume_effect_walk → drive`).

- [ ] **Step 2: Migrate `resume_effect_walk` to cede to the global loop**

In `crates/game-core/src/engine/dispatch/choice.rs`, replace `resume_effect_walk`'s body (the `base` computation + `drive_effect_to_base(cx, base)`) with a `Done` return, and delete the `use crate::engine::evaluator::drive_effect_to_base;` import (line 10):

```rust
/// Resume a parked effect walk after a player input by ceding to the global
/// `drive` loop (Slice D #423). The caller (`resume_effect_choice` /
/// `resume_damage_assignment`) has already recorded the input on the suspended
/// top `Effect` leaf; returning `Done` hands the parked frames to
/// `apply_player_action`'s `drive(cx, outcome)`, whose `Continuation::Effect`
/// arm steps them via the same `step_effect_frame` the old bounded
/// `drive_effect_to_base` used, then dispatches whatever frame the walk was
/// nested within (a `SkillTest` mid-resolution, a window with remaining
/// candidates). No bounded re-entry, no reach-down.
pub(crate) fn resume_effect_walk(_cx: &mut Cx) -> EngineOutcome {
    EngineOutcome::Done
}
```

The parameter is now unused, so prefix it `_cx` to satisfy the unused-variable lint while keeping the `&mut Cx` signature (callers are unchanged — `resume_effect_choice` line ~93 and the effect-path arm of `resume_damage_assignment` still call `resume_effect_walk(cx)`). Keeping the function preserves the named "resume the parked walk" seam and its doc; do not inline it into the callers.

- [ ] **Step 3: Verify the migration kept the net green**

Run the Step 1 suites again:
```bash
cargo test -p cards --test revelation_treacheries
cargo test -p game-core --lib engine::dispatch::choice
cargo test -p cards --test non_attack_soak
```
Expected: PASS unchanged. (`drive_effect_to_base` is now reachable only via the test-only `apply_effect`; do not add `#[allow(dead_code)]` — it is deleted in Step 6 of this same task, before the strict gauntlet runs.)

- [ ] **Step 4: Add the test-only `run` helper in `evaluator.rs`**

In `crates/game-core/src/engine/evaluator.rs`'s `#[cfg(test)] mod tests` (the module that currently imports `apply_effect` from `super`, ~line 2171), add a `run` helper with the **same signature** the deleted `apply_effect` had, so the call sites change by name only:

```rust
    /// Push an effect's root frame and drive it through the real global loop —
    /// the test-only successor to the deleted `apply_effect` bounded entry
    /// (Slice D #423). `Done` stays `Done`; a controller-pick `AwaitingInput`
    /// stays `AwaitingInput` (the leaf suspends in place under `drive` exactly
    /// as it did under the old bounded driver).
    fn run(cx: &mut Cx, effect: &Effect, ctx: EvalContext) -> EngineOutcome {
        push_effect(cx, effect, ctx);
        crate::engine::dispatch::drive(cx, EngineOutcome::Done)
    }
```

Update the test module's `use super::{ … }` import (~line 2171): drop `apply_effect`, add `push_effect`. (`EngineOutcome`, `EvalContext` are already imported there; `Effect` is in scope via the module's `use crate::dsl::…` / `super::*`.)

- [ ] **Step 5: Rework the evaluator test calls (`apply_effect` → `run`)**

Every call in `evaluator.rs`'s test module is `apply_effect(cx, &effect, ctx)`; `run` has the identical signature, so this is a uniform rename. Replace each `apply_effect(` call site with `run(` (the `&mut Cx { … }`, `&effect`, `ctx` arguments are unchanged). Example (representative — apply the same rename to every site):

```rust
        // before
        let outcome = apply_effect(
            &mut Cx { state: &mut state, events: &mut events },
            &gain_resources(InvestigatorTarget::You, 3),
            ctx(1),
        );
        // after
        let outcome = run(
            &mut Cx { state: &mut state, events: &mut events },
            &gain_resources(InvestigatorTarget::You, 3),
            ctx(1),
        );
```

Do them in batches; after each batch run `cargo test -p game-core --lib engine::evaluator` and confirm green. Do **not** change any assertion — `Done` tests stay `Done`, controller-pick tests stay `AwaitingInput`. When done, confirm none remain:
```bash
grep -n "apply_effect(" crates/game-core/src/engine/evaluator.rs | grep -v "fn run\|push_effect"
```
Expected: no call sites (only the doc-comment prose references that mention the historical name, which stay or are reworded in Step 6).

- [ ] **Step 6: Rework the `choice.rs` test call, then delete both wrappers**

In `crates/game-core/src/engine/dispatch/choice.rs`'s `#[cfg(test)] mod tests` (~line 150), the single `apply_effect(&mut Cx { … }, &effect, ctx)` call: replace it inline with `push_effect` + `drive` (one call site doesn't need a local helper), and update the test import `use crate::engine::evaluator::{apply_effect, EvalContext};` (line ~121) to `{push_effect, EvalContext}`:

```rust
        let out = {
            let mut cx = Cx { state: &mut state, events: &mut events };
            push_effect(&mut cx, &effect, ctx);
            crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done)
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "a 2-branch ChooseOne suspends for a pick",
        );
```

(`EngineOutcome` is in scope in that test module via its existing imports; add it to the `use` if not.)

Then in `evaluator.rs`, **delete** both `apply_effect` (the `#[allow(dead_code)]` + fn, ~lines 318–330) and `drive_effect_to_base` (~lines 375–388). Keep `push_effect`, `frame_of`, `step_effect_frame`. Fix any now-stale doc references: update `step_effect_frame`'s doc ("Used by `drive_effect_to_base` and the global `drive` loop" → "Used by the global `drive` loop") and reword the module-level / `apply_effect` prose mentions that survive as dangling (a quick `grep -n "apply_effect\|drive_effect_to_base" crates/game-core/src/engine/evaluator.rs` finds them; turn intra-doc links `[`apply_effect`](...)` into plain prose or delete the clause — a broken intra-doc link fails the `doc` job).

- [ ] **Step 7: Verify deletion + no remaining callers**

Run:
```bash
grep -rn "fn apply_effect\|fn drive_effect_to_base" crates/game-core/src && echo "STILL PRESENT" || echo "deleted"
grep -rn "apply_effect(\|drive_effect_to_base(" crates/game-core/src --include=*.rs | grep -v "push_effect"
```
Expected: "deleted"; the second grep prints nothing (no call sites anywhere).

- [ ] **Step 8: Full strict gauntlet, then commit**

Run all six Global-Constraint commands. Expected: all green (no dead-code warnings — both wrappers are gone). Then:
```bash
git add crates/game-core/src/engine/dispatch/choice.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: delete apply_effect/drive_effect_to_base; tests on real drive (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Task 2: Mark the arc + handoff complete

**Files:**
- Modify: `docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md`
- Modify: `docs/superpowers/plans/2026-06-24-slice-d-handoff.md`

- [ ] **Step 1: Flip the #423 arc-spec row to done**

In `docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md`, find the `D. #423` row (~line 97, "effect call-site … every apply_effect site → push root Effect frame …"). Update it to record completion — every effect site is now top-frame dispatched and `apply_effect`/`drive_effect_to_base` are deleted. Match the doc's existing row/marker style (read the surrounding rows first); keep it one line, factual.

- [ ] **Step 2: Mark Task 5 / #423 complete in the Slice D handoff**

In `docs/superpowers/plans/2026-06-24-slice-d-handoff.md`, update the **Remaining** section: mark Task 5 ✅ DONE (commit from Task 1), and note that with Task 5 landed, **#423 is complete and the branch is mergeable** (no Slice D tasks remain). Remove the "do NOT merge" caveat tied to Task 5.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md docs/superpowers/plans/2026-06-24-slice-d-handoff.md
git commit -m "docs: #423 effect call-site migration complete (Slice D)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Self-Review notes

- **Spec coverage:** migrate `resume_effect_walk` (Task 1 Step 2); delete `apply_effect` + `drive_effect_to_base` (Step 6); rework the ~30 evaluator test calls + 1 choice call onto the real `drive` (Steps 4–6); arc-spec/handoff completion (Task 2). Matches the original arc design's final-task definition.
- **Type consistency:** `run(cx: &mut Cx, effect: &Effect, ctx: EvalContext) -> EngineOutcome` is exactly the deleted `apply_effect` signature, so Step 5 is a pure call-rename and every existing assertion holds. `resume_effect_walk(cx) -> EngineOutcome` keeps its signature (callers unchanged).
- **Behaviour preservation:** the integration suites (`crates/cards/*`, `crates/game-core/tests/*`) are untouched and green throughout (Steps 1, 3); only in-crate `#[cfg(test)]` evaluator/choice calls change, by name only. `resume_effect_walk`'s migration reuses the same `step_effect_frame` via the global loop, so the suspend/resume behaviour is identical.
- **Dead-code sequencing:** `drive_effect_to_base` goes briefly caller-less-in-production after Step 2 but is deleted in Step 6 of the same task before the strict `-D warnings` gauntlet runs (Step 8) — no `#[allow(dead_code)]` interim needed; mid-task focused `cargo test` runs without `-D warnings`.
