# Effect Evaluator as Continuation Frames (#422) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the card-effect evaluator from single-pass suspend-and-replay to pure frame-driven control flow on the shared `continuations` stack, retiring `DecisionCursor`, and use the new substrate to make `Effect::Deal` soak distribution interactive (K5b-2 / #44).

**Architecture:** Each effect control-flow node (`Seq`, `ForEachPointFailed`, `If`, `ChooseOne`, `*::Chosen` grounding, `SearchDeck`, `Native`, leaves) becomes a `Continuation::Effect(EffectFrame)` on `GameState.continuations`. The existing global `drive` loop learns to step effect frames: complete a leaf → pop + advance parent via `on_child_pop`; a control node → push its next child; a choice → suspend. `apply_effect` shrinks to a thin **bounded entry** that pushes a root effect frame and runs `drive` until the stack returns to the depth it started at. No recursive Rust tree-walk and no replay remain, so a suspension parks the rest of the tree on the stack and resume walks forward — fixing the per-point `ForEachPointFailed` loss that blocked K5b-2.

**Tech Stack:** Rust (workspace crate `game-core`); `serde` (frames are serialized for replay); existing `Cx`/`EngineOutcome`/`Continuation` engine machinery; `cards` corpus for integration tests.

## Global Constraints

- **Action-log replay stays bit-for-bit deterministic.** The same `OptionId`s must be recorded as the same `ResolveInput` actions; retiring the internal `DecisionCursor` must not change which actions the log carries.
- **Validate-first / mutate-second** per frame step (handler contract, `engine/mod.rs::apply`).
- **Behaviour-preserving except K5b-2.** Every existing card/effect test must stay green; K5b-2 is purely additive (multi-point treachery harm newly distributes instead of being lost).
- **CI gauntlet, warnings-as-errors.** Before every push run all strict jobs from `CLAUDE.md` Commands: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, plus the wasm jobs.
- **Frames carry their own `EvalContext` snapshot** (reusing #345's serializable grouped bindings). `EvalContext` is `Copy`.
- **Two PRs.** Tasks 1–7 = PR 1 (substrate, branch `engine/effect-evaluator-frames`). Tasks 8–9 = PR 2 (K5b-2, branch `engine/k5b2-effect-soak-distribution`). The skill-test invocation cluster keeps the bounded `apply_effect` entry with a `TODO(#374)`; deep native composition + the three framework loop sites are out of scope (tracked follow-ups).

---

## File structure

- `crates/game-core/src/state/game_state.rs` — add `Continuation::Effect(EffectFrame)` and the `EffectFrame` enum (Seq/ForEachPointFailed/If/Leaf/Choosing — node-in-progress + its `EvalContext`). **Delete** `Continuation::Choice` + `ChoiceFrame` (replay-era prompt object; choices now suspend in place on `EffectFrame::Choosing`; see Task 3).
- `crates/game-core/src/engine/evaluator.rs` — the rewrite. `apply_effect` → bounded entry; delete `DecisionCursor`, `apply_effect_with_decisions`, `apply_effect_inner`'s recursion, and the `apply_seq`/`apply_native` guards. New: `step_effect_frame` (one step of the top effect frame) + per-node helpers that push child frames.
- `crates/game-core/src/engine/dispatch/mod.rs` — extend `drive` to step `Continuation::Effect` frames; route `Continuation::Effect` in `resolve_input` to the choice/native resume.
- `crates/game-core/src/engine/dispatch/choice.rs` — `resume_choice` → `resume_effect_choice`: top frame is the in-place `EffectFrame::Choosing`; set `chosen_option`, re-step the node, return to `drive` (no replay, no separate prompt object). `suspend_for_choice`/`suspend_for_native_choice` stop pushing a `Continuation` — they just build the `AwaitingInput` request.
- `crates/game-core/src/engine/dispatch/combat.rs` — Task 8: `Effect::Deal` leaf pushes `DamageAssignment` when contested; `resume_damage_assignment`'s `DamageSource::Effect` arm resumes the effect drive.
- `crates/cards/tests/non_attack_soak.rs` — Task 9: un-defer the interactive multi-point cases.
- New: `crates/game-core/src/engine/dispatch/effect_frames.rs` (optional) — if `evaluator.rs` grows unwieldy, house `step_effect_frame` + `on_child_pop` here. Decide in Task 2; default to keeping it in `evaluator.rs` unless it pushes the file past ~5000 lines.

---

## Task 1: Add the `EffectFrame` type and dormant `Continuation::Effect` variant

Introduces the new data types with no behavior change, so the big rewrite (Task 2) lands against a compiling baseline.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (Continuation enum ~line 602; add the variant + the `EffectFrame` enum near `ChoiceFrame` ~line 604)
- Test: `crates/game-core/src/state/game_state.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Produces: `Continuation::Effect(EffectFrame)`; `EffectFrame` enum with variants `Seq { effects: Vec<Effect>, next: usize, ctx: EvalContext }`, `ForEachPointFailed { remaining: u8, body: Box<Effect>, ctx: EvalContext }`, `If { then_: Box<Effect>, else_: Option<Box<Effect>>, took_then: bool, ctx: EvalContext }`, `Leaf { effect: Box<Effect>, ctx: EvalContext }`, `Choosing { effect: Box<Effect>, offered: Vec<OptionId>, ctx: EvalContext }`. **An effect node that needs a pick suspends *in place*** by becoming a `Choosing` frame — that frame *is* the prompt; it stays on the stack and is re-stepped on resume. So `Continuation::Effect` is itself an input-awaiting variant (exactly parallel to `DamageAssignment`), routed in `resolve_input`. The former top-level `Continuation::Choice`/`ChoiceFrame` are **deleted** in Task 3 (replay-era residue — the node's own frame already holds it; no separate prompt object, no node duplication). A suspending Native folds in: its `Leaf { effect: Native{tag} }` becomes `Choosing`.

- [ ] **Step 1: Write the failing test**

In the `game_state.rs` test module:

```rust
#[test]
fn effect_frame_variant_roundtrips_serde() {
    use crate::dsl::Effect;
    use crate::engine::EvalContext;
    use crate::state::{Continuation, EffectFrame, InvestigatorId};

    let frame = Continuation::Effect(EffectFrame::Seq {
        effects: vec![Effect::Seq(vec![])],
        next: 0,
        ctx: EvalContext::for_controller(InvestigatorId(1)),
    });
    let json = serde_json::to_string(&frame).expect("serialize");
    let back: Continuation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(frame, back);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core effect_frame_variant_roundtrips_serde`
Expected: FAIL — `EffectFrame` / `Continuation::Effect` not found.

- [ ] **Step 3: Add the variant and enum**

In `game_state.rs`, add to `enum Continuation` (before the closing `}` at ~line 602):

```rust
    /// A node of an in-progress card-effect walk (#422). The effect evaluator is
    /// frame-driven: each control-flow node parks here while its children
    /// resolve; the global `drive` loop steps the top frame. Replaces the former
    /// single-pass replay (`DecisionCursor`). Carries its own `EvalContext`
    /// snapshot (#345's grouped bindings) so resume re-binds without replay.
    Effect(EffectFrame),
```

Then define (near `ChoiceFrame`, ~line 604):

```rust
/// One node of a frame-driven card-effect walk (#422). See
/// [`Continuation::Effect`]. Stepped by the evaluator's `step_effect_frame`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectFrame {
    /// A `Seq([..])` in progress: run `effects[next]`, advance `next` on each
    /// child pop, complete when `next == effects.len()`.
    Seq {
        effects: Vec<card_dsl::dsl::Effect>,
        next: usize,
        ctx: crate::engine::EvalContext,
    },
    /// A `ForEachPointFailed(body)` in progress: run `body` `remaining` more
    /// times, decrementing on each child pop. Holds its own count on the stack
    /// so each iteration may suspend independently (fixes Grasping Hands).
    ForEachPointFailed {
        remaining: u8,
        body: Box<card_dsl::dsl::Effect>,
        ctx: crate::engine::EvalContext,
    },
    /// An `If` whose condition has been evaluated: the chosen branch frame is
    /// pushed; this frame completes immediately on that child's pop.
    If {
        then_: Box<card_dsl::dsl::Effect>,
        else_: Option<Box<card_dsl::dsl::Effect>>,
        took_then: bool,
        ctx: crate::engine::EvalContext,
    },
    /// A leaf effect (no child frames): apply synchronously when stepped, pop.
    /// Includes `Effect::Native { tag }` (the leaf step runs the native fn). If a
    /// leaf needs a controller pick (`ChooseOne`, a `*::Chosen` target, a native
    /// choice) it transitions to `Choosing` instead. `Effect::Deal` may instead
    /// push a `DamageAssignment` (K5b-2, Task 8).
    Leaf {
        effect: Box<card_dsl::dsl::Effect>,
        ctx: crate::engine::EvalContext,
    },
    /// An effect node suspended in place for a controller pick (#422). The frame
    /// *is* the prompt: it stays on top, awaits a `PickSingle`, and is re-stepped
    /// on resume with `ctx.chosen_option` set (the node then grounds/picks
    /// instead of suspending). `offered` validates the resume pick. Replaces the
    /// deleted top-level `Continuation::Choice`/`ChoiceFrame`.
    Choosing {
        effect: Box<card_dsl::dsl::Effect>,
        offered: Vec<crate::engine::OptionId>,
        ctx: crate::engine::EvalContext,
    },
}
```

(There is no separate choice `Continuation` variant — the node's own frame holds the suspension. Task 3 deletes `Continuation::Choice`/`ChoiceFrame`.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p game-core effect_frame_variant_roundtrips_serde`
Expected: PASS.

- [ ] **Step 5: Ensure the new variant doesn't break exhaustive matches**

Run: `cargo build -p game-core 2>&1 | grep -A2 "non-exhaustive\|E0004"`
Expected: no `E0004` (missing-match-arm) errors. If any `match continuation` is exhaustive over `Continuation`, add a temporary arm for `Continuation::Effect(_)` that `unreachable!("effect frames are driven in Task 2")` — these are replaced in Task 2. List of likely sites: `resolve_input` (mod.rs:416), `drive` (mod.rs:173), `Continuation::is_phase_anchor`. Search: `grep -rn "Continuation::" crates/game-core/src --include=*.rs | grep "=>"`.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/state/game_state.rs
git commit -m "engine: add dormant EffectFrame type + Continuation::Effect variant (#422)"
```

---

## Task 2: Frame-driven evaluator core — step + on_child_pop + bounded entry

The atomic rewrite. All effect node types must convert together (a frame `Seq` whose child is a replay `ChooseOne` has no root to re-walk). The existing evaluator tests are the green gate for behaviour-preservation.

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`apply_effect` ~379; delete `DecisionCursor` ~318–360, `apply_effect_with_decisions` ~365, the recursion in `apply_effect_inner` ~388, `apply_seq` guard ~1436, `apply_native` guard ~1400; the `ground_chosen_targets`/`ChooseOne`/`SearchDeck` cursor params)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`drive` ~166)
- Test: existing `crates/game-core/src/engine/**` tests (gate) + one new test below.

**Interfaces:**
- Produces:
  - `pub(crate) fn apply_effect(cx: &mut Cx, effect: &Effect, eval_ctx: EvalContext) -> EngineOutcome` — unchanged signature; now the bounded entry.
  - `pub(super) fn step_effect_frame(cx: &mut Cx) -> EngineOutcome` — steps the top `Continuation::Effect` frame once. Returns `AwaitingInput` if it suspended (Choice/Native/Deal-contested), else `Done` (it pushed a child / completed + advanced parent / popped to base).
- Consumes: `Continuation::Effect(EffectFrame)` + the `EffectFrame` variants from Task 1; `eligible to extend` `EvalContext` accessors (`failed_by`, `chosen_*`, `set_chosen_option`) already present.

### Design notes for the implementer (read before coding)

The classic recursive walk (`apply_effect_inner`) is replaced by a **push-down state machine on the stack**. Translate each `apply_effect_inner` arm:

- **Leaf effects** (`GainResources`, `DiscoverClue`, `Deal`, `DealDamageToEnemy`, `Heal`, `Modify`, `AdvanceCurrentAct`, `DiscardSelf`, `Cancel`, `PutIntoThreatArea`, `Restrict` (reject), `Fight`, `BoostAttackDamage`, `DrawCards`, `Investigate`, `AttachSelfToLocation`, `SkillTest`): when a `Leaf` frame is stepped, call the existing handler fn (e.g. `gain_resources(cx, ctx, ..)`), pop the frame. If the handler itself returns `AwaitingInput` (e.g. `SkillTest` starts a test, `Deal` after Task 8), leave its pushed frame(s) and return `AwaitingInput`. **The leaf handlers are reused verbatim** — only their *invocation* moves into `step_effect_frame`'s `Leaf` arm. `ground_chosen_targets` still runs first (now without a cursor — see below).
- **`Seq(effects)`**: push `EffectFrame::Seq { effects, next: 0, ctx }`. When stepped with `next < len`, push a child frame for `frame_of(&effects[next], ctx)` and increment `next` *in place* (mutate the frame, then push child above it). When `next == len`, pop. (`on_child_pop` is implicit: stepping the Seq again after a child pops sees the advanced `next`.)
- **`ForEachPointFailed(body)`**: at frame creation `remaining = ctx.failed_by().unwrap_or(0)`. When stepped with `remaining > 0`, decrement in place and push a child frame `frame_of(body, ctx)`. When `remaining == 0`, pop. **This is the K5b-2 fix** — the count survives on the stack across a child suspension.
- **`If { condition, then, else_ }`**: evaluate `condition` synchronously at frame creation (conditions are pure reads — confirm via `apply_if`'s current body), push `EffectFrame::If` with the chosen branch already determined, and push that branch's child frame; the `If` frame pops when the child pops. (Simpler: skip an `If` frame entirely — evaluate the condition and directly push the chosen branch's frame. Prefer this unless a condition needs post-branch work; current `apply_if` does not.)
All choice kinds **suspend in place** by transitioning the node's frame to `EffectFrame::Choosing { effect, offered, ctx }` (stays on top, returns `AwaitingInput`). On resume `resume_effect_choice` (Task 3) sets `ctx.chosen_option = picked` and re-steps the node, which grounds/picks instead of suspending. There is **no separate choice frame and no node copy** — the suspended frame is the node's own frame. Each choice-consuming step must **read-and-consume `ctx.chosen_option`** when present instead of suspending (replacing today's `cursor.take()`); the node has ≤1 choice point, so one transient pick per re-step suffices and the parent `Seq`/loop frame below resumes automatically when the node pops.

- **`ChooseOne(branches)`**: apply `resolve_choice_count(branches.len())`. `Empty` → reject. If `ctx.chosen_option` is `Some(i)` (resume) or `Auto(i)` → push `frame_of(&branches[i], ctx)` and pop the ChooseOne frame. Else (`Suspend`, no pick) → transition to `Choosing { effect: ChooseOne(branches), offered: 0..len, ctx }` + `AwaitingInput`.
- **`*::Chosen` grounding** (`ground_chosen_targets` + `ground_investigator_choice`/`location`/`enemy`): enumerate candidates; if `ctx.chosen_option` is `Some(i)` → bind `candidates[i]` into `ctx`, clear `chosen_option`, proceed to the handler; else on 2+ candidates → transition to `Choosing { effect: effect.clone(), offered, ctx }` + `AwaitingInput`. The grounding dispatches on the effect's target type, so investigator/location/enemy need no separate handling.
- **`SearchDeck`**: currently cursor-driven. Inspect `apply_search_deck` and confirm its choice is a **single pick (≤1 choice point)**; if so it maps to the same `Choosing` shape. If it is a multi-select or has a second internal choice, it needs its own frame split — flag and handle explicitly, don't assume.
- **`Native { tag }`** (handled as `Leaf { effect: Native{tag} }`): the leaf step calls the native fn (reused). If it returns `AwaitingInput`, transition the frame to `Choosing { effect: Native(tag), offered, ctx }` (the native supplies `offered`). On resume the re-step re-invokes the native with `chosen_option` set. The re-invoke must be **idempotent up to its suspension** (choice-before-side-effect, standalone — its existing contract); document this at the leaf-Native arm.

`frame_of(effect, ctx) -> EffectFrame` is a small constructor: control nodes → their frame; everything else → `Leaf`.

`apply_effect` becomes:

```rust
pub(crate) fn apply_effect(cx: &mut Cx, effect: &Effect, eval_ctx: EvalContext) -> EngineOutcome {
    let base = cx.state.continuations.len();
    cx.state
        .continuations
        .push(Continuation::Effect(frame_of(effect, eval_ctx)));
    drive_effect_to_base(cx, base)
}
```

`drive_effect_to_base(cx, base)` loops `step_effect_frame` while the top frame is a `Continuation::Effect` at depth `> base`; returns `AwaitingInput` if a step suspends (frames left parked), `Done` when the stack returns to `base`, propagates `Rejected`. The global `drive` (mod.rs:166) gains an arm: `Some(Continuation::Effect(_)) => match step_effect_frame(cx) { Done => continue, other => return other }` so effect frames parked across an `apply()` boundary (after a window closes) get driven by the one global loop too.

- [ ] **Step 1: Write the failing test (the model's whole point — a choice after a Seq step)**

This case is currently *rejected* by the `apply_seq` guard. Put it in `evaluator.rs` tests (use the `TestGame` builder + a synthetic effect):

```rust
#[test]
fn choice_after_earlier_seq_step_no_longer_rejects() {
    use crate::dsl::Effect;
    // Seq[ GainResources(1), ChooseOne[ GainResources(1), GainResources(2) ] ]
    // The choice follows a mutating step — the old single-pass guard rejected
    // this; the frame model must suspend on the choice instead.
    let effect = Effect::Seq(vec![
        Effect::GainResources { target: investigator_self(), amount: 1 },
        Effect::ChooseOne(vec![
            Effect::GainResources { target: investigator_self(), amount: 1 },
            Effect::GainResources { target: investigator_self(), amount: 2 },
        ]),
    ]);
    let mut game = /* TestGame with one active investigator */;
    let out = apply_effect(&mut game.cx(), &effect, EvalContext::for_controller(game.active));
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "a choice after an earlier Seq step must suspend, not reject: {out:?}",
    );
}
```

(Adapt `investigator_self()` / `game.cx()` to the actual `TestGame` API in `evaluator.rs` tests; mirror an existing evaluator test's setup.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core choice_after_earlier_seq_step_no_longer_rejects`
Expected: FAIL — currently returns `Rejected` ("a choice after an earlier Seq step is not yet supported").

- [ ] **Step 3: Implement the frame machine**

Write `frame_of`, `step_effect_frame`, `drive_effect_to_base`; rewrite `apply_effect`; delete `DecisionCursor`, `apply_effect_with_decisions`, the recursive `apply_effect_inner`, and the `apply_seq`/`apply_native` guards; drop the `cursor` params from `ground_*` / `apply_choose_one` / `apply_search_deck`. Add the `drive` arm in mod.rs. Translate each node per the design notes above.

- [ ] **Step 4: Run the new test + the full evaluator suite (behaviour gate)**

Run: `cargo test -p game-core choice_after_earlier_seq_step_no_longer_rejects`
Expected: PASS.
Run: `cargo test -p game-core engine::evaluator engine::dispatch`
Expected: all PASS (behaviour-preserving). Investigate any failure with systematic-debugging — a regression here means a node translation diverged.

- [ ] **Step 5: Update `resume_choice` (see Task 3) is required for resume-path tests** — if resume tests fail at this step because `resume_choice` still calls the deleted `apply_effect_with_decisions`, do Task 3 now (the two are coupled; Tasks 2+3 may land in one commit).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: frame-driven effect evaluator core; retire DecisionCursor + #346/#334 guards (#422)"
```

---

## Task 3: Delete `Continuation::Choice`; resume the in-place `Choosing` frame

Effect choices suspend in place on `EffectFrame::Choosing` (Task 1/2). So the top-level `Continuation::Choice` + `ChoiceFrame` (replay-era prompt object) are deleted, and `resolve_input` resumes the `Choosing` frame directly.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (delete `Continuation::Choice` ~429 + `ChoiceFrame` ~615; remove from the two classifier matches ~731/756)
- Modify: `crates/game-core/src/engine/dispatch/choice.rs` (`resume_choice` ~101 → `resume_effect_choice`; `suspend_for_choice` ~43 / `suspend_for_native_choice` ~80 → return the `AwaitingInput` request, no push)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` ~421 — replace the `Continuation::Choice` arm with a `Continuation::Effect(EffectFrame::Choosing{..})` arm)
- Modify: `crates/game-core/src/engine/evaluator.rs` (the suspend sites in `apply_choose_one` + the three `ground_*` fns + the native leaf — transition to `Choosing`; read-and-consume `ctx.chosen_option`)

**Interfaces:**
- Produces: `resume_effect_choice(cx, response)` — top frame is `Continuation::Effect(EffectFrame::Choosing { effect, offered, ctx })`; validate `PickSingle` ∈ `offered`; set `ctx.chosen_option = Some(OptionId(picked))`; replace the `Choosing` frame with `frame_of(&effect, ctx)` (the re-step); run the effect drive to base; keep the existing post-`Done` re-entry into `drive_skill_test` / `advance_resolution` (choice.rs:131–144) — that seam is the bounded-entry boundary for the not-yet-migrated callers.
- `resolve_input` routes `Some(Continuation::Effect(EffectFrame::Choosing{..})) => resume_effect_choice`. A non-`Choosing` `Continuation::Effect` on top during `resolve_input` is spurious (drive would have stepped it) → reject defensively (mirror the `AttackLoop`/anchor arms).
- Requires: `EvalContext::chosen_option` is read-and-consumed by `apply_choose_one`, `ground_chosen_targets`, and the native leaf (replacing `cursor.take()`); a step that fails to consume it would re-suspend forever (loud, detectable).

- [ ] **Step 1: Run the resume-path tests as the gate** (these exist and must stay green)

Run: `cargo test -p game-core engine::dispatch::choice`
Run: `cargo test -p cards --test play_card`  (Research Librarian 01032 SearchDeck choice; Crypt Chill 01167 on_fail)
Expected after implementation: PASS.

- [ ] **Step 2: Delete `Continuation::Choice` + `ChoiceFrame`** in game_state.rs; remove the variant from the two classifier matches (~731/756). The only producers were `suspend_for_choice`/`suspend_for_native_choice` (grep-confirmed evaluator-internal), now reshaped in Step 3.

- [ ] **Step 3: Reshape the suspend helpers + evaluator suspend sites.** `suspend_for_choice`/`suspend_for_native_choice` now just build the `AwaitingInput { request, .. }` (no `Continuation` push). The evaluator suspend sites (`apply_choose_one`, `ground_investigator_choice`/`location`/`enemy`, the native leaf) transition the current frame to `EffectFrame::Choosing { effect, offered, ctx }` and return that `AwaitingInput`; and they **read-and-consume `ctx.chosen_option`** (replacing `cursor.take()`) so a resume binds/picks instead of re-suspending.

- [ ] **Step 4: Write `resume_effect_choice`** (per the Interfaces block) and wire the `resolve_input` arm.

- [ ] **Step 5: Rewrite the replay-asserting tests.** The ~6 evaluator tests asserting `frame.decisions` (evaluator.rs:3049, 3342, 3382, 3499, 3611, 3751) encode the old replay recording; rewrite each to assert the new shape (the top `EffectFrame::Choosing`'s `offered`, or simply that the right branch ran after the pick). Update `choice_frame_snapshots_active_skill_test_binding` (choice.rs:168) to assert the snapshotted `ctx.failed_by()` on the `Choosing` frame.

- [ ] **Step 6: Run the gate**

Run: `cargo test -p game-core engine::dispatch::choice` and `cargo test -p cards --test play_card`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/choice.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/evaluator.rs crates/game-core/src/state/game_state.rs
git commit -m "engine: suspend effect choices in place; delete Continuation::Choice (#422)"
```

---

## Task 4: Verify the full suite green + run the CI gauntlet (substrate checkpoint)

No new code unless a gauntlet job fails. This is the behaviour-preservation gate before migrating call sites.

- [ ] **Step 1: Full workspace test**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS. Any failure = a node/resume translation regression; debug before continuing.

- [ ] **Step 2: Clippy + fmt + doc + wasm**

Run each: `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
Expected: all clean. Fix intra-doc links broken by the deleted `DecisionCursor` / `apply_effect_with_decisions` (grep `DecisionCursor` / `apply_effect_with_decisions` in doc comments).

- [ ] **Step 3: Commit any gauntlet fixes**

```bash
git add -A && git commit -m "engine: fix gauntlet (docs/clippy) after evaluator frame rewrite (#422)"
```

---

## Task 5: Migrate the "free" invocation sites to push-root + on_child_pop

The sites that already have an "effect Done → re-enter driver" seam. Each moves off the bounded `apply_effect` to a pushed root effect frame whose completion the enclosing engine frame handles via the global `drive`.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`fire_pending_trigger` ~683, `play_fast_event` ~767)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`resolve_one` ~417)
- Modify: `crates/game-core/src/engine/dispatch/abilities.rs` (`resume_activate_ability` ~170)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (treachery revelation ~179)
- Test: existing reaction-window / forced-trigger / encounter tests as the gate.

**Interfaces:**
- Consumes: the global `drive`'s new `Continuation::Effect` stepping (Task 2).

**Implementation note:** For each site, the current shape is `match apply_effect(..) { Done => <post-effect>, suspend => return suspend, Rejected => unreachable }`. The `<post-effect>` (bump usage counter, `advance_resolution`, `flush_pending_played_event`, `spawn`/teardown) must run when the effect frame completes. Because the enclosing frame (`Resolution`, `EncounterCard`, `ActionResolution`) is *beneath* the pushed effect root, the cleanest migration keeps the synchronous shape **but via the bounded entry** for now where the post-effect logic is trivial — OR moves `<post-effect>` into the enclosing frame's `on_child_pop`. Default: migrate the ones whose enclosing frame already has an `on_child_pop` hook (`EncounterCard` teardown at `resolve_input` mod.rs:486; `ActionResolution` via `resume_action_resolution`); leave reaction-window/forced on the bounded entry if moving their post-effect into `advance_resolution` is non-trivial, and note it. **Right-size to what's clean; do not force a fragile conversion.** Document each decision inline with `// #422:` comments.

- [ ] **Step 1: Run the gate suite for these sites**

Run: `cargo test -p game-core engine::dispatch::reaction_windows engine::dispatch::forced_triggers engine::dispatch::encounter engine::dispatch::abilities`
Run: `cargo test -p cards`
Expected (after migration): PASS, unchanged behaviour.

- [ ] **Step 2: Migrate `resume_activate_ability` and the `EncounterCard` teardown path** (cleanest — enclosing frame + on_child_pop already exist). Keep behaviour identical.

- [ ] **Step 3: Evaluate reaction-window/forced sites; migrate or annotate**

For `fire_pending_trigger` / `play_fast_event` / `resolve_one`: migrate to push-root if `advance_resolution` / `close_reaction_window_at` can be reached via the enclosing `Resolution` frame's drive; else keep the bounded entry with `// #422: bounded-entry residual — post-effect (advance_resolution) not yet a frame on_child_pop`.

- [ ] **Step 4: Run the gate**

Run: `cargo test -p game-core engine::dispatch && cargo test -p cards`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/
git commit -m "engine: migrate free effect-invocation sites onto the global effect drive (#422)"
```

---

## Task 6: Migrate the "bounded" invocation sites (play_card, enemy reveal, fast ability)

Sites needing a small enclosing frame for their post-effect bookkeeping.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (`complete_play` OnPlay loop ~632)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (enemy revelation ~218)
- Modify: `crates/game-core/src/engine/dispatch/abilities.rs` (`activate_ability` fast/Fight ~124)
- Test: existing play / spawn / activated-ability tests.

**Implementation note:** `play_card` OnPlay's post-effect (remove-from-hand → instantiate in-play → `EnteredPlay` event → reaction-window check) is real bookkeeping. For non-fast plays it already runs under `ActionResolution` (via `resume_play_card`); fold the post-effect into a new `ActionResume`-like step or run it after the bounded entry returns `Done`. Right-size: if a clean `on_child_pop` is awkward, keep `complete_play` on the bounded entry — its post-effect is self-contained and the substrate goal (global drive handles effect frames) is already met. **Prefer the bounded entry here over a fragile new frame**; the value of this task is confirming these sites work unchanged on the new substrate, not forcing frame conversion. Annotate residuals with `// #422:`.

- [ ] **Step 1: Gate suite**

Run: `cargo test -p game-core engine::dispatch::cards engine::dispatch::abilities && cargo test -p cards --test play_card`
Expected (after): PASS.

- [ ] **Step 2: Confirm/convert each site**

Verify `complete_play`, enemy revelation, and fast `activate_ability` resolve correctly through the new `apply_effect` bounded entry. Convert to push-root + on_child_pop only where clean; else annotate the residual.

- [ ] **Step 3: Run the gate** — same commands as Step 1. Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/
git commit -m "engine: confirm play/spawn/fast-ability effect sites on the frame substrate (#422)"
```

---

## Task 7: PR 1 — gauntlet, push, PR, phase doc

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md` (fold the already-pending #422 edit; mark K5b-2 unblocked-in-progress; add a Decisions-made entry only if load-bearing per `docs/phases/README.md`)

- [ ] **Step 1: Full CI gauntlet** (all jobs from Global Constraints). Expected: clean.
- [ ] **Step 2: Push branch + open PR** with `gh pr create` using the repo template; design-decisions paragraph references the spec (`docs/superpowers/specs/2026-06-21-phase-7-422-effect-evaluator-frames-design.md`). Body: "Substrate only; K5b-2 in follow-up. Skill-test cluster + 3 loop sites + deep native composition are tracked follow-ups." `Closes` nothing yet (#422 closes with PR 2).
- [ ] **Step 3: Watch CI** `gh pr checks <PR#> --watch`; fix failures with follow-up commits.
- [ ] **Step 4: Phase-doc commit** as the final commit once CI is green (per `feedback_phase_doc_updates`): flip K5b-2 to in-progress, fold the pending edit. Push.
- [ ] **Step 5: Stop for user review before merge.** Do not merge without explicit approval.

---

## Task 8: K5b-2 — `Effect::Deal` interactive distribution (PR 2)

Branch from merged `main` after PR 1 lands: `git checkout main && git pull && git checkout -b engine/k5b2-effect-soak-distribution`.

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`deal_effect` ~1264 — the `Leaf` Deal step)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`resume_damage_assignment` `DamageSource::Effect` arm ~936)
- Test: `crates/game-core/src/engine/dispatch/combat.rs` `#[cfg(test)]` + `crates/cards/tests/non_attack_soak.rs` (Task 9)

**Interfaces:**
- Consumes: `build_soakers`, `eligible_targets`, `assign_attack`/`credit_point`, `place_assignment`, `prompt_current_point`, `Continuation::DamageAssignment { source: DamageSource::Effect }` (all in combat.rs).

**Implementation note:** Today `deal_effect` (→ `take_damage`/`take_horror` → `soak_and_place`) places synchronously. Change the `Effect::Deal` leaf so that, when a soaker can take a contested point, it pushes `Continuation::DamageAssignment { investigator, remaining_damage, remaining_horror, assignment, source: DamageSource::Effect }` and returns `AwaitingInput` (via `prompt_current_point`) — **gated exactly like K5b-1**: only prompt when `eligible_targets` for some point includes a soaker (otherwise place synchronously, no prompt — preserving the current uncontested path). The gating predicate already exists in the attack path (K5b-1); reuse it. Then flip `resume_damage_assignment`'s `DamageSource::Effect` arm from `place + return Done` to `place + return to the effect drive` so the parent `ForEachPointFailed`/`Seq` frame advances:

```rust
DamageSource::Effect => {
    let _ = place_assignment(cx, investigator, &assignment);
    // Resume the parked effect walk: the parent ForEachPointFailed/Seq frame is
    // now top — let the global drive step it forward (no point lost). (#422 K5b-2)
    super::drive(cx, EngineOutcome::Done)
}
```

- [ ] **Step 1: Write the failing unit test (multi-point effect distribution across soaker + self)**

In combat.rs tests, set up an investigator controlling a 1-health soaker, deal 2 damage via `take_damage`/the Deal leaf with a contested point, script two `PickSingle`s (soaker then self), assert: first point → `AwaitingInput`; after both picks → both points placed (1 on soaker, 1 on self), no points lost.

```rust
#[test]
fn effect_deal_distributes_each_point_without_loss() {
    // soaker health 1; deal 2 damage; pick soaker then self.
    // Assert: prompt opens; after 2 picks, soaker has 1, investigator has 1.
    // (Mirror the K5b-1 attack-path distribution test in this module.)
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core effect_deal_distributes_each_point_without_loss`
Expected: FAIL — current `Effect` arm returns `Done` and loses the second point / never prompts.

- [ ] **Step 3: Implement the gated Deal-leaf suspend + the resume-arm flip.**

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p game-core effect_deal_distributes_each_point_without_loss`
Expected: PASS.

- [ ] **Step 5: Run the combat + evaluator suites (no regression to the attack path or uncontested soak)**

Run: `cargo test -p game-core engine::dispatch::combat engine::evaluator`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: interactive per-point soak distribution for Effect::Deal (K5b-2 of #44)"
```

---

## Task 9: Un-defer the `non_attack_soak.rs` interactive cases + PR 2

**Files:**
- Modify: `crates/cards/tests/non_attack_soak.rs` (Grasping Hands 01162 = 2 damage multi-point; Rotting Remains 01163)
- Modify: `docs/phases/phase-7-the-gathering.md` (mark K5b-2 shipped; move #44 closing notes per the keystone arc)

**Interfaces:**
- Consumes: the K5b-2 path from Task 8; the existing `ScriptedResolver`/`reveal_top`/`board_with_soaker` harness in the test file.

**Implementation note:** The existing `grasping_hands_damage_soaks_onto_guard_dog` test asserts both points soak onto a 3-health Guard Dog (uncontested — Guard Dog can take both, no prompt). Add a **contested** case: a soaker that can take *some but not all* points (e.g. a 1-health/1-sanity soaker vs Grasping Hands' 2 damage), scripting the player's per-point `PickSingle` distribution, asserting the prompt opens and points land where chosen with none lost. Verify against the cards: **before writing, WebFetch `https://arkhamdb.com/card/01162` and `https://arkhamdb.com/card/01163`** (text + FAQ) to confirm Grasping Hands deals damage = amount failed by and Rotting Remains deals horror = amount failed by (per CLAUDE.md: never cite card text from memory).

- [ ] **Step 1: Verify the card texts**

WebFetch `https://arkhamdb.com/card/01162` and `https://arkhamdb.com/card/01163` (text + FAQ). Confirm the harm shape before asserting amounts.

- [ ] **Step 2: Write the contested-distribution test(s)**

Add a `grasping_hands_damage_distributes_across_contested_soaker` (and a horror analog for Rotting Remains if a contested setup is constructible) using a soaker that can't absorb everything, scripting the distribution.

- [ ] **Step 3: Run to verify they pass**

Run: `cargo test -p cards --test non_attack_soak`
Expected: PASS.

- [ ] **Step 4: Full gauntlet**

Run all jobs from Global Constraints. Expected: clean.

- [ ] **Step 5: Commit, push, PR**

```bash
git add crates/cards/tests/non_attack_soak.rs
git commit -m "test: contested Effect::Deal soak distribution for Grasping Hands / Rotting Remains (K5b-2 of #44)"
```

PR body: design-decisions paragraph; `Closes #422.` (and notes the K5b-2 part of #44 is done — the keystone arc's #44 closes when its remaining loop-site follow-ups land, per the spec; confirm whether #44 should close here or stay open for the loop sites — check the phase doc's #44 status).

- [ ] **Step 6: Watch CI; fix failures with follow-up commits.**

- [ ] **Step 7: Phase-doc commit** (final, after CI green): mark K5b-2 shipped (PR #), flip the Arc row, drop the settled "K5b-2 blocked" note, add a Decisions-made entry only if load-bearing. File the three loop-site follow-up issues + confirm #346 is closed→#422.

- [ ] **Step 8: Stop for user review before merge.**

---

## Self-review

**Spec coverage:**
- Evaluator core → frames (Seq/ForEachPointFailed/If/ChooseOne/Chosen-grounding/SearchDeck/Native/leaves): Task 2. ✓
- Retire `DecisionCursor` + #346/#334 guards: Task 2. ✓
- Global drive handles effect frames: Task 2 (drive arm); choices suspend in place on `EffectFrame::Choosing` (Task 3) routed by a `Continuation::Effect` `resolve_input` arm — `Continuation::Choice`/`ChoiceFrame` deleted; `Continuation::Effect` joins `DamageAssignment` as a work-and-prompt variant. ✓
- `apply_effect` = bounded entry: Task 2. ✓
- Migrate all-but-skill-test sites: Tasks 5–6 (right-sized; residuals annotated). ✓
- Skill-test cluster keeps bounded entry + TODO(#374): Global Constraints + Task 5/6 notes. ✓
- K5b-2 `Effect::Deal` distribution + un-defer tests: Tasks 8–9. ✓
- Two-PR pair: Tasks 1–7 / 8–9. ✓
- Loop-site + deep-native follow-ups tracked: Task 9 Step 7. ✓
- Bit-for-bit replay + behaviour-preservation invariants: Global Constraints + Task 4 gate. ✓
- Open questions (ForEach materialization — note: `Effect::ForEach` is a *stub* today, only `ForEachPointFailed` is live, so the "materialization timing" question is moot in scope; nested `apply_effect` re-entry — base-depth bound in Task 2 + skill-test is the only re-entrant caller): addressed. ✓

**Placeholder scan:** No "TBD"/"implement later". The `// #422:` annotations and right-size guidance are deliberate (this is a refactor where some call-site conversions are judgment calls); test bodies for Tasks 8–9 are sketched with explicit setup/assert intent and pointers to the K5b-1 mirror because the exact builder API must match the existing combat.rs test module — the implementer copies that module's harness rather than inventing one.

**Type consistency:** `EffectFrame` variants and `Continuation::Effect` are defined once (Task 1) and consumed consistently (Tasks 2/3/8). `step_effect_frame` / `frame_of` / `drive_effect_to_base` names are used consistently. `DamageSource::Effect` matches the existing combat.rs variant.
