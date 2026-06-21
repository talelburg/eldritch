# Phase 7 — reify the effect evaluator as continuation frames (#422) — design

Tracking: **[#422](https://github.com/talelburg/eldritch/issues/422)**. Extends
the unified control-flow model (`2026-06-20-unified-control-flow-model-design.md`,
**#393**) — the "every step is a frame" transformation — from phases/turns/the
attack loop down into the **DSL effect walk**. Completes the **keystone** arc
(`2026-06-20-phase-7-keystone-mid-action-park-design.md`, ordering step 4): its
last sub-slice **K5b-2** (interactive soak distribution for `Effect::Deal` harm,
part of **#44**) is blocked on the evaluator's current model and this spec is the
unblock. **Supersedes [#346](https://github.com/talelburg/eldritch/issues/346)**
(two-pass evaluator for a choice after a `Seq` mutation) — effect-frames are a
more complete fix for the same root cause.

## Why this pass exists

The evaluator (`crates/game-core/src/engine/evaluator.rs`) resolves an effect
tree by a **recursive Rust walk** (`apply_effect_inner`) carrying a
`DecisionCursor` that **re-runs the whole tree from the top on each resume**,
replaying recorded picks to reach the next ungrounded choice. This single-pass
**suspend-and-replay** model supports **at most one suspension per walk, before
any irreversible side effect** — because re-running would double-apply anything
that already ran. The constraint is enforced by two loud rejects:

- `apply_seq` (~evaluator.rs:1424): *"a choice after an earlier Seq step is not
  yet supported (Axis-A single-pass replay; the two-pass split is deferred,
  #346)"* — rejects rather than double-mutate.
- `apply_native` (~evaluator.rs:1380): a `debug_assert` tripwire for a native
  that suspends after the cursor already consumed picks (native↔DSL-choice
  interleaving, #334).

This is exactly what blocks **K5b-2**. Grasping Hands 01162 is
`ForEachPointFailed(deal_damage(1))` — one damage *per point failed*. When the
first per-point deal suspends on the soak-distribution prompt,
`resume_damage_assignment`'s `DamageSource::Effect` arm places the point and
returns `Done` but **never re-enters the walk**, so the loop's remaining
iterations are silently lost — a correctness regression. (Rotting Remains 01163
happens to work only because it fails by exactly 1: a single iteration.)

The fix is structural: **a recursive call stack can't be suspended and stored**,
so the "rest of the in-progress tree" must live as continuation frames instead.
Defunctionalize the evaluator's control flow onto the existing
`GameState.continuations` stack — the same transformation #393 applied to
phases/turns/the attack loop. With no replay, each effect runs **exactly once**;
a suspension parks the remaining tree on the stack and resume walks forward.
"deal 1 → suspend → deal 1 → suspend" then works naturally, no double-apply.

Groundwork is already laid: `EvalContext` was made serializable with grouped
per-frame bindings (**#345**) precisely so it can ride continuation frames.

## Scope

**In scope (this spec):**

- The **evaluator core** rewritten to pure frame-driven control flow:
  `Seq`, `ForEach`, `ForEachPointFailed`, `ChooseOne`, `*::Chosen` target
  grounding, leaf effects, and `Native` leaves — all reified as frames on the
  shared `continuations` stack. `DecisionCursor` and both loud-reject guards
  (#346/#334) deleted.
- The **global drive loop** learns to dispatch effect frames (top-frame handler +
  `on_child_pop`), so a single drive carries effect resolution.
- `apply_effect` reduced to a **thin bounded entry** into that one drive (push
  root frame, drive until base depth, return `EngineOutcome`) — *not* a second
  loop.
- Migrate every effect-invocation site **except the skill-test cluster** off the
  bounded entry to direct push-root + `on_child_pop`.
- **K5b-2**: `Effect::Deal` routes through the interactive `DamageAssignment`
  distribution; un-defer the `non_attack_soak.rs` interactive cases.

**Out of scope (tracked follow-ups):**

- **The three framework loop sites** — Dynamite Blast 01024's per-investigator
  loop (`dynamite_blast.rs`), `apply_symbol_outcome` (`skill_test.rs`), and the
  draw-from-empty horror penalty (`draw_one_with_deckout`, `phases.rs`). They
  **soak** (soak-first, K5a) but not interactively; each needs a resumable cursor
  the substrate provides. New issues, one per site, adopting the substrate.
- **The skill-test invocation cluster** (`on_success`/`on_fail`/`OnCommit`/
  `OnSkillTestResolution`/investigate-follow-up) keeps the bounded `apply_effect`
  entry, with a `TODO(#374)`. Its post-effect logic *is* the `FinishContinuation`
  sequence (`PostFollowUp → PostRetaliate → PostOnResolution`); fully reifying it
  is **item 5** (#374/#64, "move the skill-test path from Shape A toward
  end-state B") — the next roadmap item. Deleting the bounded entry is gated on
  item 5, not on this slice.
- **Deep native↔frame composition** (a native that composes mid-tree, e.g. the
  Dynamite Blast loop). Folds in with the loop-site follow-ups.

## Design

### 1. Pure frame-driven internals; one global drive

No recursive Rust tree-walk remains. Each control-flow node is a frame; the
global drive pops the top frame, runs **one step** (complete → pop + signal
parent via `on_child_pop`; or push a child; or suspend), and loops. Engine frames
(`SkillTest`, `AttackLoop`, `ActionResolution`, `DamageAssignment`, phase
anchors) and effect frames interleave on one stack under uniform top-frame
dispatch — the #393 end-state, extended to the evaluator.

```
drive (global):
  loop:
    match continuations.last():
      EffectFrame(Seq{i})        -> child Done? advance i; exhausted? pop + parent.on_child_pop
      EffectFrame(ForEach{n})    -> child Done? push body, n-=1; n==0? pop + parent.on_child_pop
      EffectFrame(Choice{..})    -> suspend (AwaitingInput) until ResolveInput
      EffectFrame(leaf)          -> apply; pop; parent.on_child_pop   (Deal may push DamageAssignment)
      DamageAssignment{..}       -> (existing) on pop: place; parent.on_child_pop
      <engine frames>            -> (existing)
```

### 2. Frame representation — one wrapper variant

Add `Continuation::Effect(EffectFrame)`, an inner enum (one wrapper variant
rather than N top-level `Continuation` variants):

- `Seq { effects: Vec<Effect>, next: usize, ctx: EvalContext }` — `on_child_pop`
  advances `next`; done when exhausted.
- `ForEach { remaining: Vec<Item>, body: Effect, ctx }` and
  `ForEachPointFailed { remaining: u8, body: Effect, ctx }` — looping frames hold
  their own cursor on the stack. **This is what fixes Grasping Hands**: each
  per-point `Deal` suspends independently and the count survives on the stack —
  no point lost.
- `Choice { offered: Vec<OptionId>, branches: …, ctx }` — suspends; on resume,
  pushes the **chosen branch's** frame. **No replay of the root.** Replaces the
  current replay-based `ChoiceFrame`.
- `TargetChoice { … }` — the `ground_chosen_targets` suspension for `*::Chosen`
  becomes a frame that, on resume, binds the pick into `ctx` then pushes the
  effect node.
- `Native { tag, chosen_option, ctx }` — see §4.

Each frame carries its own `EvalContext` snapshot — reusing the grouped
per-frame bindings #345 built for this. Leaf effects (`GainResources`,
`DiscoverClue`, …) run synchronously inside their step. `DamageAssignment` stays
the existing engine-level variant; `Effect::Deal` leaves push it (§3).

`DecisionCursor`, `apply_effect_with_decisions`, and the `apply_seq`/`apply_native`
loud-reject guards are deleted. `apply_effect_inner`'s recursive dispatch is
replaced by the per-frame `on_child_pop` handlers.

### 3. K5b-2 — `Effect::Deal` interactive distribution

The `Effect::Deal` leaf step: build soakers (`build_soakers`); **if a soaker can
take a contested point**, push `DamageAssignment { source: Effect, … }` and
suspend — gated exactly as K5b-1 (prompt only when a soaker can take the point);
otherwise place synchronously (no prompt). `resume_damage_assignment`'s
`DamageSource::Effect` arm changes from *"place + return `Done`"* to *"place +
return to the drive"*, so the parent `ForEachPointFailed`/`Seq` frame advances via
`on_child_pop`. Then un-defer `non_attack_soak.rs` for Grasping Hands (2 damage,
multi-point) and Rotting Remains.

### 4. Natives stay leaves (documented; deep composition deferred)

In this spec a native remains a **synchronous leaf** that may suspend **once,
choice-before-side-effect, standalone** — its existing contract (Crypt Chill
01167 is standalone; 01105's native is deterministic). The `Native` frame carries
the recorded option and re-invokes the native fn on resume; this is safe
precisely because the in-scope suspending natives choose *before* mutating, so
re-invocation re-reaches the same suspension idempotently rather than
double-applying a side effect. Deeper native↔frame composition (a native that
mutates then suspends, or composes mid-tree) is **not supported** and is deferred
to the loop-site follow-ups. This contract is documented at the `Native` frame
and the re-invoke site so the constraint is visible, not implicit.

### 5. Invocation boundary — migrate all but skill-test

`apply_effect(effect, ctx) -> EngineOutcome` becomes a thin **bounded entry**:
record `continuations.len()` as a base depth, push the root effect frame, run the
**global** drive until the stack returns to base (→ `Done`) or a frame suspends
above it (→ `AwaitingInput`, frames parked). It reuses the one drive — no
duplicate loop.

Call sites split three ways (from the call-site audit):

- **Free to migrate** (already have an "effect `Done` → re-enter driver" seam =
  an `on_child_pop` in disguise): reaction-window effects + fast-event play
  (`Resolution` + `advance_resolution`), forced triggers (`close_reaction_window_at`),
  resumed activated ability (`ActionResolution`), treachery-revelation teardown
  (`EncounterCard`). Convert to push-root + `on_child_pop` under the global drive.
- **Bounded but real** (need a small enclosing frame to host post-effect
  bookkeeping): `play_card` OnPlay (move-to-play + `EnteredPlay` + window check),
  enemy revelation (`spawn_enemy` after), fast activated ability. Migrate as the
  slice's final step.
- **Keep the bounded entry** (entangled with item 5): the skill-test cluster, with
  `TODO(#374)`. Deletion gated on item 5.

End-state of this slice: the global drive handles effect frames everywhere
separable; the only synchronous-entry residual is the skill-test island, which
item 5 (the **next** roadmap item) absorbs as part of work it already owns.

## Invariants

- **Action-log replay stays bit-for-bit deterministic.** The same `OptionId`s are
  recorded as the same `ResolveInput` actions; retiring the *internal*
  `DecisionCursor` does not touch the *action-log* replay path.
- **Validate-first / mutate-second** holds per frame step.
- **Every existing card/effect test stays green** throughout — the rewrite is
  behaviour-preserving except K5b-2, which is additive (multi-point treachery harm
  newly distributes instead of being lost).

## Plan decomposition (two-PR pair)

The evaluator core is **not stageable** — a frame-based `Seq` whose child is a
still-replay-based `ChooseOne` has no single root to re-walk; the node types must
convert together. So the substrate lands as one PR; K5b-2 rides on top.

- **PR 1 — substrate.** `Continuation::Effect(EffectFrame)` + per-node
  `on_child_pop`; global drive dispatches effect frames; `apply_effect` → bounded
  entry; delete `DecisionCursor` + replay + both guards; migrate the free +
  bounded sites. Behaviour-preserving; all existing tests green.
- **PR 2 — K5b-2 + tests.** `Effect::Deal` interactive distribution; flip the
  `resume_damage_assignment` `Effect` arm; un-defer `non_attack_soak.rs`
  (Grasping Hands multi-point, Rotting Remains). Closes the K5b-2 part of #44.

Each PR is independently green and TDD'd task-by-task.

## Follow-ups this spec creates

- One issue per **loop site** (Dynamite Blast, `apply_symbol_outcome`,
  draw-deckout) to adopt the substrate for interactive distribution.
- The **skill-test invocation reification + bounded-entry deletion** is folded
  into **item 5** (#374/#64), not a new issue.
- **#346 closed → #422** (superseded). **#363** (general fan-out) note updated to
  point at #422 as the substrate (already done in the phase doc).

## Open questions

- **`ForEach` item materialization timing.** `ForEach` currently enumerates its
  items eagerly; under a frame the `remaining: Vec<Item>` is snapshotted at frame
  creation. Confirm no in-scope effect depends on re-enumerating after a mid-loop
  mutation (none known; Grasping Hands' `ForEachPointFailed` carries a fixed
  count, not a re-queried set).
- **Bounded-entry base-depth vs. nested `apply_effect`.** Confirm no in-scope site
  calls `apply_effect` *while an effect frame from an outer `apply_effect` is
  parked above its base* (would mis-bound the inner drive). The audit shows the
  skill-test cluster is the only re-entrant caller and it runs effects
  sequentially, not nested — verify during PR 1.
