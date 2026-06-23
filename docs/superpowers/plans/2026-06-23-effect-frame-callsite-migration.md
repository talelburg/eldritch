# Effect-frame call-site migration (Slice D, #423) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate every production effect-invocation site off the synchronous `apply_effect`/`drive_effect_to_base` bounded entry to pure top-frame dispatch, then delete the wrapper and rework its tests onto the real `drive`.

**Architecture:** Replace `let out = apply_effect(cx, &e, ctx); <post-logic>` with `push_effect(cx, &e, ctx)` + post-logic moved into an enclosing frame the global `drive` loop dispatches when the effect frame pops. Zero-post-logic sites push-and-return `Done`; sites with a live enclosing frame (`SkillTest`, reaction-window, forced-run, `EncounterCard`) reuse it; the two hand-play sites unify under a new `Continuation::PlayFromHand`; `EncounterCard` gains a `disposition` so treachery+enemy revelation share one frame.

**Tech Stack:** Rust, the `game-core` engine crate. No new dependencies.

## Global Constraints

- **Behaviour-preserving at the `apply` boundary.** A dispatch handler returns to `apply_player_action`, which runs `drive(cx, outcome)`; pushing the effect root and returning `Done` hands the same work to the same loop. The card tests (`crates/cards/src/impls/*`) and integration tests (`crates/cards/tests/*`) go through real `apply`/`drive` and MUST stay green **untouched** — they are the behaviour-preservation net.
- **Load-bearing cards** (verify each stays green at the relevant task): Dynamite Blast 01024 (suspending OnPlay + suspending Fast event), Crypt Chill 01167 (suspending on_fail), Frozen in Fear 01164 (forced/reaction + on_success effects that initiate a skill test), Research Librarian 01032 (after-enters-play reaction window), Grasping Hands / Crypt Chill in `revelation_treacheries`.
- **`apply_effect` and `drive_effect_to_base` are DELETED in the final task** — not demoted to test-only. `frame_of`, `step_effect_frame`, and the drive loop's `Continuation::Effect(_)` arm stay (production internals).
- Match CI's strict flags before declaring any task done: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, plus `cargo build -p web --target wasm32-unknown-unknown` and the wasm clippy.
- One PR; each task is its own commit and keeps the full strict gauntlet green and bisectable.

## File structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/game-core/src/engine/evaluator.rs` | `apply_effect`→`push_effect`; delete wrapper; rework effect tests | 1, 5 |
| `crates/game-core/src/engine/dispatch/abilities.rs` | sites 1a, 1b | 1 |
| `crates/game-core/src/engine/dispatch/forced_triggers.rs` | site 6 | 1 |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | site 5a (Task 1), site 5b (Task 4) | 1, 4 |
| `crates/game-core/src/state/game_state.rs` | `EncounterCard.disposition`; `Continuation::PlayFromHand` | 2, 4 |
| `crates/game-core/src/engine/dispatch/encounter.rs` | sites 3a, 3b; disposition-aware teardown | 2 |
| `crates/game-core/src/engine/dispatch/mod.rs` | drive-loop arms (`EncounterCard`, new `PlayFromHand`) | 2, 4 |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | sites 4a, 4b | 3 |
| `crates/game-core/src/engine/dispatch/cards.rs` | site 2 (`complete_play`/`resume_play_card`/`play_card`) | 4 |
| `crates/game-core/src/engine/dispatch/choice.rs` | test rework | 5 |

---

## Task 1: `push_effect` helper + the push-and-return sites (1a, 1b, 6, 5a)

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (add `push_effect` beside `apply_effect`)
- Modify: `crates/game-core/src/engine/dispatch/abilities.rs:124`, `:170`
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs:443`
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:758` (in-play branch of `fire_pending_trigger`)

**Interfaces:**
- Produces: `pub(crate) fn push_effect(cx: &mut Cx, effect: &Effect, eval_ctx: EvalContext)` — pushes `Continuation::Effect(frame_of(effect, eval_ctx))`, returns `()`. Used by every later task and the reworked tests.

- [ ] **Step 1: Write the failing test for `push_effect`**

In `evaluator.rs`'s `#[cfg(test)] mod tests`, add a test that `push_effect` + the real `drive` runs an effect to completion identically to the old `apply_effect`. Use an existing simple effect from the test module (e.g. a `GainResources`) — match the pattern of a nearby effect test:

```rust
#[test]
fn push_effect_then_drive_runs_to_completion() {
    use crate::engine::dispatch::drive;
    use crate::state::Continuation;
    // (Mirror an existing test's state/ctx setup for a GainResources effect.)
    let (mut state, effect, ctx, inv) = gain_resources_fixture();
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    push_effect(&mut cx, &effect, ctx);
    assert!(matches!(cx.state.continuations.last(), Some(Continuation::Effect(_))));
    let out = drive(&mut cx, EngineOutcome::Done);
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(state.investigators[&inv].resources, 1); // effect ran
    assert!(state.continuations.is_empty(), "effect frame popped");
}
```

Replace `gain_resources_fixture()` with the inline setup copied from the nearest existing `apply_effect` GainResources test in this module (look at `evaluator.rs:2255+`). Do not factor a shared fixture unless one already exists.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core push_effect_then_drive_runs_to_completion`
Expected: FAIL — `push_effect` is not defined.

- [ ] **Step 3: Add `push_effect`**

In `evaluator.rs`, immediately after `apply_effect` (which ends at line 326), add:

```rust
/// Push an effect's root [`EffectFrame`](crate::state::EffectFrame) onto the
/// continuation stack for the global `drive` loop to own (top-frame dispatch,
/// #393/#423). The caller returns `EngineOutcome::Done`; `drive` then steps the
/// pushed frame via its `Continuation::Effect` arm. Replaces the synchronous
/// `apply_effect` bounded entry at every production site.
pub(crate) fn push_effect(cx: &mut Cx, effect: &Effect, eval_ctx: EvalContext) {
    cx.state
        .continuations
        .push(crate::state::Continuation::Effect(frame_of(effect, eval_ctx)));
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p game-core push_effect_then_drive_runs_to_completion`
Expected: PASS.

- [ ] **Step 5: Migrate site 1a (`activate_ability`)**

In `abilities.rs`, the Fast/exempt tail (line 123-124):

```rust
    let eval_ctx = EvalContext::for_controller_with_source(investigator, instance_id);
    apply_effect(cx, &effect, eval_ctx)
```
becomes
```rust
    let eval_ctx = EvalContext::for_controller_with_source(investigator, instance_id);
    super::super::evaluator::push_effect(cx, &effect, eval_ctx);
    EngineOutcome::Done
```
Update the `use super::super::evaluator::{apply_effect, EvalContext}` import (line 10) to `{push_effect, EvalContext}` only if `apply_effect` is now unused in this file (it is also at 1b — both migrate in this task, so switch the import to `push_effect` here).

- [ ] **Step 6: Migrate site 1b (`resume_activate_ability`)**

In `abilities.rs`, the tail (line 169-170):

```rust
    let eval_ctx = EvalContext::for_controller_with_source(investigator, instance_id);
    apply_effect(cx, effect, eval_ctx)
```
becomes
```rust
    let eval_ctx = EvalContext::for_controller_with_source(investigator, instance_id);
    push_effect(cx, effect, eval_ctx);
    EngineOutcome::Done
```

- [ ] **Step 7: Migrate site 6 (`resolve_one`)**

In `forced_triggers.rs`, the tail (line 442-443):

```rust
    let ctx =
        EvalContext::for_controller_with_optional_source(hit.controller, hit.source.instance());
    apply_effect(cx, &effect, ctx)
```
becomes
```rust
    let ctx =
        EvalContext::for_controller_with_optional_source(hit.controller, hit.source.instance());
    push_effect(cx, &effect, ctx);
    EngineOutcome::Done
```
Update the import (line 15) `{apply_effect, EvalContext}` → `{push_effect, EvalContext}`.

- [ ] **Step 8: Migrate site 5a (in-play branch of `fire_pending_trigger`)**

In `reaction_windows.rs`, replace the `let result = apply_effect(...); match result { ... }` block (lines 758-783) with a bump-before-push shape. The fired candidate is already removed above (lines 751-756); the window frame stays on top beneath the pushed effect.

```rust
    // Usage is consumed when the ability fires (the post-`Done` conditionality
    // was purely defensive against an `unreachable!` Rejected). Bump now, then
    // push the effect for the drive loop; the window frame beneath resumes its
    // candidate scan when the effect pops. In-scope suspending forced effects
    // (Frozen in Fear 01164) carry no usage limit, so an early bump is a no-op
    // for them. Slice D, #423.
    if usage_limit.is_some() {
        bump_usage_counter(cx.state, &trigger);
    }
    push_effect(cx, &ability.effect, eval_ctx);
    EngineOutcome::Done
```
Leave the `reaction_windows.rs` `apply_effect` import in place — `play_fast_event` (5b) still uses it until Task 4.

- [ ] **Step 9: Run the full strict gauntlet**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```
Expected: all green, with **no** edits to any `crates/cards/tests/*` or `crates/cards/src/impls/*` file. (Activated-ability cards — Machete/.45 Automatic Fight, Flashlight Investigate, First Aid Heal — and forced/reaction cards exercise 1a/1b/6/5a through real `apply`.)

- [ ] **Step 10: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/abilities.rs crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: push_effect helper + push-and-return effect sites (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Task 2: `EncounterCard` disposition (sites 3a, 3b)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (the `EncounterCard` variant + new `EncounterDisposition` enum)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (treachery arm, enemy arm, `teardown_encounter_card_if_top`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs:241` (the `EncounterCard` drive arm)

**Interfaces:**
- Consumes: `push_effect` (Task 1).
- Produces: `Continuation::EncounterCard { card: CardCode, disposition: EncounterDisposition }`; `enum EncounterDisposition { Discard, Spawn { investigator: InvestigatorId, metadata: &'static CardMetadata } }` (carry exactly what `spawn_enemy(cx, investigator, code, metadata)` needs beyond `card`). A disposition-aware disposal fn replacing the inline `teardown`/`spawn_enemy`.

- [ ] **Step 1: Write the failing test — enemy revelation spawns via the frame**

Enemy revelation (3b) is dormant for *Revelation effects* but the spawn path is live. Add an engine test (in `encounter.rs`'s test module) that resolving an enemy encounter card pushes an `EncounterCard { disposition: Spawn, .. }` and that driving it spawns the enemy. Model the state/registry setup on the existing enemy-spawn test in this module (search for `spawn_enemy` / `CardType::Enemy` tests). Assert: after `resolve_encounter_card` returns `Done`, driving disposes the frame and the enemy is in play.

```rust
#[test]
fn enemy_revelation_spawns_through_the_encounter_frame() {
    // (Setup mirrors the existing enemy-spawn encounter test: registry with an
    // enemy card that has NO Revelation ability, a location to spawn at.)
    let out = resolve_encounter_card(&mut cx, investigator, &code, metadata);
    let out = drive(&mut cx, out);
    assert_eq!(out, EngineOutcome::Done);
    assert!(enemy_is_in_play(&state, &code), "enemy spawned via the frame");
    assert!(!has_encounter_card_frame(&state), "the disposition frame was disposed");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core enemy_revelation_spawns_through_the_encounter_frame`
Expected: FAIL — `EncounterCard` has no `disposition` field / the enemy arm doesn't push the frame yet (compile error or assertion failure).

- [ ] **Step 3: Add the `disposition` field + `EncounterDisposition`**

In `game_state.rs`, change the `EncounterCard` variant to carry `disposition: EncounterDisposition` and add the enum (place it near the variant; derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` to match sibling types). `Spawn` carries `investigator` and `metadata` (`&'static CardMetadata`); confirm `CardMetadata: 'static` is already how it is threaded (the enemy arm holds `metadata` today). If `&'static` does not serialize cleanly, store the minimal owned spawn inputs instead — match how `spawn_enemy`'s args are sourced at `encounter.rs:223`.

- [ ] **Step 4: Make the disposal disposition-aware**

Rewrite `teardown_encounter_card_if_top` (`encounter.rs:853`) to dispatch on `disposition`: `Discard` → the current persistent/discard logic; `Spawn { investigator, metadata }` → pop the frame and call `spawn_enemy(cx, investigator, card, metadata)`. Rename to `dispose_encounter_card_if_top` for accuracy and update its two callers (the `mod.rs:241` drive arm, and — until Step 5 removes it — the treachery inline call).

- [ ] **Step 5: Migrate the treachery arm (3a)**

In `encounter.rs`, the treachery arm already pushes `EncounterCard` (line 172-174) — add `disposition: EncounterDisposition::Discard`. Replace the `for ability … { match apply_effect … }` loop + inline `teardown_encounter_card_if_top(cx)` tail (lines 175-193) with: combine the Revelation effects into a single `Effect::Seq` (preserving order), `push_effect` it once, and return `Done`. The `EncounterCard` frame (pushed above, beneath the effect) is disposed by the drive arm when the effect pops.

```rust
    cx.state.continuations.push(Continuation::EncounterCard {
        card: code.clone(),
        disposition: EncounterDisposition::Discard,
    });
    let revelation: Vec<_> = abilities
        .iter()
        .filter(|a| a.trigger == Trigger::Revelation)
        .map(|a| a.effect.clone())
        .collect();
    if !revelation.is_empty() {
        push_effect(cx, &crate::dsl::Effect::Seq(revelation), eval_ctx);
    }
    EngineOutcome::Done
```

- [ ] **Step 6: Migrate the enemy arm (3b)**

In `encounter.rs`, the enemy arm (lines 214-223): push `EncounterCard { card: code.clone(), disposition: EncounterDisposition::Spawn { investigator, metadata } }` BEFORE the Revelation effects, `push_effect` the `Seq`-combined Revelation effects (if any), and return `Done`. Remove the inline `spawn_enemy` call (the `Spawn` disposition runs it on disposal).

- [ ] **Step 7: Update the import + drive arm**

Switch `encounter.rs`'s `apply_effect` import to `push_effect`. Confirm the `mod.rs:241` arm now calls `dispose_encounter_card_if_top` (renamed in Step 4).

- [ ] **Step 8: Run the new test + the behaviour net**

Run:
```bash
cargo test -p game-core enemy_revelation_spawns_through_the_encounter_frame
cargo test -p cards --test revelation_treacheries
```
Expected: both PASS; `revelation_treacheries` (Crypt Chill / Grasping Hands — suspending treachery Revelations) green and untouched.

- [ ] **Step 9: Full strict gauntlet, then commit**

Run the gauntlet (Task 1 Step 9 commands). Then:
```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/encounter.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: EncounterCard disposition unifies treachery+enemy revelation (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Task 3: skill-test on_success / on_fail (sites 4a, 4b)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:335-365` (`run_resolution`)

**Interfaces:**
- Consumes: `push_effect`; the live `SkillTest` frame (pre-advanced to `SkillTestStep::PostFollowUp { succeeded }` at line 317-320); the `SkillTest` drive arm (`mod.rs:233`).
- Produces: no new types. `run_resolution` pushes on_success/on_fail as a child effect instead of running it synchronously.

**Invariants to preserve (verified by the behaviour net):**
- on_success runs only when the follow-up completed synchronously (the suspend path early-returns without running on_success).
- The cursor is pre-advanced to `PostFollowUp` *before* the follow-up, so a resume re-enters the driver at teardown, not re-running the follow-up or the on_fail/on_success effect.
- on_fail may suspend (Crypt Chill 01167) and must not re-run on resume.

- [ ] **Step 1: Confirm the behaviour net covers both branches (no new test yet)**

Run the existing suites that exercise these branches and note them green as the baseline:
```bash
cargo test -p cards --test revelation_treacheries   # Crypt Chill on_fail suspend
cargo test -p game-core --lib engine::dispatch::skill_test
```
Expected: PASS. These are the regression net; the migration must keep them green.

- [ ] **Step 2: Migrate on_success (4a)**

In `run_resolution`, replace the synchronous `apply_effect` + `debug_assert!(Done)` for on_success (lines 335-343) with a `push_effect`. Because the follow-up already ran to `Done` on this path (the suspend case returned above), pushing on_success as the next child is correct — when it pops, the `SkillTest` frame (cursor `PostFollowUp`) is re-exposed and the drive arm resumes at teardown:

```rust
        if let Some(effect) = &on_success {
            // Success-side card effect (Frozen in Fear 01164). Push for the
            // drive loop; the SkillTest frame (cursor PostFollowUp) resumes at
            // teardown when it pops. Slice D, #423.
            push_effect(cx, effect, card_ctx(investigator));
        }
```

- [ ] **Step 3: Migrate on_fail (4b)**

Replace the on_fail block (lines 351-365). The current code runs on_fail synchronously and early-returns on `AwaitingInput`. After migration, pushing on_fail as a child handles suspension automatically (the effect frame parks; the `SkillTest` resumes at teardown when it pops):

```rust
    } else if let Some(effect) = &on_fail {
        // Margin-keyed failure branch (Effect::SkillTest). `failed_by` is
        // threaded so Effect::ForEachPointFailed can scale. Push for the drive
        // loop; a suspending on_fail (Crypt Chill 01167) now parks as an effect
        // frame and the SkillTest resumes at teardown — it never re-runs. Slice
        // D, #423.
        let mut ctx = card_ctx(investigator);
        ctx.set_failed_by(failed_by);
        push_effect(cx, effect, ctx);
    }
    EngineOutcome::Done
```

- [ ] **Step 4: Verify ordering — write a contract test**

Add a `skill_test.rs` test that an on_success effect is pushed (not run inline) and resolves through the loop, and that teardown happens after. Model state on an existing `run_resolution` test (a Plain test with `on_success = Some(deal_horror(...))` and `Numeric(0)` passing token). Assert: immediately after `run_resolution`, the on_success effect frame is on top (not yet applied), then `drive` applies it and tears down the `SkillTest`.

```rust
#[test]
fn on_success_pushes_for_the_loop_then_resolves() {
    // (Setup: a passing Plain test with on_success = deal_horror(You,1).)
    let out = run_resolution(&mut cx, inv, &committed);
    assert_eq!(out, EngineOutcome::Done);
    assert!(matches!(cx.state.continuations.last(), Some(Continuation::Effect(_))),
        "on_success is pushed, not run inline");
    assert_eq!(state.investigators[&inv].horror, 0, "not applied until driven");
    let out = drive(&mut cx, EngineOutcome::Done);
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(state.investigators[&inv].horror, 1, "loop applied on_success");
    assert!(!has_skill_test_frame(&state), "torn down after");
}
```
Adjust `run_resolution`'s call signature/args to match its real signature (read lines 270-283 for the exact params). If `run_resolution` is not directly callable from the test module, drive the full test via `perform_skill_test` + commit + `drive` and assert the same end-state.

- [ ] **Step 5: Run the test + the behaviour net**

Run:
```bash
cargo test -p game-core on_success_pushes_for_the_loop_then_resolves
cargo test -p cards --test revelation_treacheries
cargo test -p game-core --lib engine::dispatch::skill_test
```
Expected: all PASS.

- [ ] **Step 6: Full strict gauntlet, then commit**

Run the gauntlet. Then:
```bash
git add crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: skill-test on_success/on_fail push effects for the loop (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Task 4: `PlayFromHand` frame (sites 2, 5b)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (new `Continuation::PlayFromHand` + `PlayFromHandStage`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (new drive-loop arm; route `resolve_input` defensively like `EncounterCard`)
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (`complete_play`, `resume_play_card`, `play_card` fast path)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:842` (`play_fast_event`)

**Interfaces:**
- Consumes: `push_effect`; `begin_event_play`, `flush_pending_played_event`, `new_in_play_instance`, `emit_event(EnteredPlay)`, `open_queued_reaction_window`, `resolve_play_target`, `PlayDestination` (all in `cards.rs`/`threat_area`/`emit`).
- Produces: `Continuation::PlayFromHand { investigator: InvestigatorId, code: CardCode, hand_index: u8, stage: PlayFromHandStage }`; `enum PlayFromHandStage { Dispose, AfterEnterWindow }` (the `AfterEnterWindow` stage resumes after an asset-entrance reaction window closes). The drive arm runs the stage-keyed disposal when the child effect frame pops.

**Disposal responsibility (ported from `complete_play` lines 631-680 + `play_fast_event` flush):**
- After the OnPlay/OnEvent effect completes: if `PlayDestination::InPlay` (asset) → remove from hand, `new_in_play_instance`, push to `cards_in_play`, `emit_event(EnteredPlay)`, and if a reaction window was queued on top, open it (re-park `PlayFromHand` at `AfterEnterWindow` so it resumes after). If event (`Discard`) → `flush_pending_played_event` (discards the stashed event exactly once).

**Invariant (spec open question #2):** the played event is discarded exactly once. Today the normal path relies on the apply-loop `flush_pending_played_event` on a `Done` apply; the Fast path flushes eagerly. After this task, `PlayFromHand`'s `Dispose` stage owns the flush for both. Read the apply-loop flush (search `flush_pending_played_event` callers) and ensure it becomes a harmless no-op once `PlayFromHand` has flushed (the fn is documented as a no-op when `pending_played_event` is already cleared), or remove the now-redundant apply-loop call if it would double-fire. Add a test asserting a single `CardDiscarded`.

- [ ] **Step 1: Write the failing contract test — normal event play pushes PlayFromHand then disposes**

Add a `cards.rs` (or a `crates/cards/tests/`) test: playing a non-fast event (e.g. Emergency Cache 01088 is an event with `GainResources` OnPlay) pushes a `PlayFromHand` frame, the effect runs through the loop, and the event is discarded exactly once. Use the existing play-card test harness in `crates/cards/tests/play_card.rs` as the model (it installs `cards::REGISTRY`).

```rust
#[test]
fn event_play_disposes_through_play_from_hand() {
    // Play Emergency Cache (01088): event, OnPlay GainResources(3).
    let result = drive(state, play_action, resolver);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_resources_gained(&result.state, 3);
    assert_event_count!(result.events, CardDiscarded { .. }, 1); // discarded exactly once
    assert!(!card_in_hand(&result.state, "01088"));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cards --test play_card event_play_disposes_through_play_from_hand`
Expected: FAIL — `PlayFromHand` does not exist; depending on harness, a compile error or a `CardDiscarded`-count mismatch.

- [ ] **Step 3: Add the `PlayFromHand` variant + stage**

In `game_state.rs`, add `Continuation::PlayFromHand { investigator, code, hand_index, stage }` and `enum PlayFromHandStage { Dispose, AfterEnterWindow }` (derives matching siblings). Document it as the unified hand-play resolution frame (run effect → type-disposition), AoO-agnostic, used by the normal `PlayCard` primary and `play_fast_event`. Add the `is_phase_anchor`/`awaits_input` classification as needed (it never awaits input — mirror `EncounterCard`).

- [ ] **Step 4: Add the drive-loop arm**

In `mod.rs`'s `drive` loop, add a `Some(Continuation::PlayFromHand { .. })` arm that calls a new `cards::dispose_play_from_hand(cx)`. In `resolve_input`, add a defensive-reject `PlayFromHand` arm (mirrors the `EncounterCard` arm at `mod.rs:482` — it never awaits input).

- [ ] **Step 5: Implement `dispose_play_from_hand` (the ported disposal)**

In `cards.rs`, add `pub(super) fn dispose_play_from_hand(cx: &mut Cx) -> EngineOutcome`. Pop/peek the `PlayFromHand` frame; on `stage: Dispose`, re-derive `destination` via `resolve_play_target(&code)` and run the disposal logic ported from `complete_play` lines 637-680 (asset enter-play with the `AfterEnterWindow` re-park if a reaction window opens; event → `flush_pending_played_event`). On `stage: AfterEnterWindow`, finish the post-window asset tail and pop. Keep the persistent/instance logic identical to `complete_play`.

- [ ] **Step 6: Rewire `complete_play` (site 2) to push `PlayFromHand`**

Replace `complete_play`'s body (the OnPlay loop + asset tail, lines 628-681) with: `Seq`-combine the OnPlay effects, push a `PlayFromHand { stage: Dispose, .. }` frame, `push_effect` the combined OnPlay `Seq` above it, return `Done`. `resume_play_card` (the post-AoO entry) and the `play_card` no-AoO fast path both already call `complete_play`, so both inherit the push. Confirm the event stash (`begin_event_play` / `pending_played_event`) still happens before `complete_play` for events in the normal path (read `play_card` lines 560-597 to confirm where the event is stashed; if it is stashed in `play_card`, no change needed).

- [ ] **Step 7: Migrate `play_fast_event` (site 5b)**

In `reaction_windows.rs`, replace the `match apply_effect(...) { ... flush ... }` block (lines 842-861) with: push `PlayFromHand { stage: Dispose, .. }` (above the window), `push_effect` the event effect, return `Done`. The `Dispose` stage flushes the event; `PlayFromHand` pops, exposing the window, which resumes its candidate scan. Switch the `reaction_windows.rs` `apply_effect` import to `push_effect` (5a already migrated; 5b is the last user here).

- [ ] **Step 8: Reconcile the event flush (invariant)**

Find every `flush_pending_played_event` caller. Ensure the event is discarded exactly once: `PlayFromHand`'s `Dispose` now flushes; verify the apply-loop flush is a no-op afterward (documented no-op when `pending_played_event` is cleared) or remove it if it would double-fire. Read `flush_pending_played_event`'s body to confirm the no-op guard.

- [ ] **Step 9: Run the contract test + the behaviour net**

Run:
```bash
cargo test -p cards --test play_card
cargo test -p cards   # all card + integration suites: Dynamite Blast (suspending OnPlay + Fast event), Research Librarian (after-enters-play window), assets enter play
```
Expected: PASS, with the new test green and all `crates/cards/*` untouched. Pay special attention to: a single `CardDiscarded` per event play; Dynamite Blast's location-choice suspension working through `PlayFromHand`; Research Librarian's after-enters-play reaction window driving via the `AfterEnterWindow` stage.

- [ ] **Step 10: Full strict gauntlet, then commit**

Run the gauntlet. Then:
```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/dispatch/cards.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: PlayFromHand frame unifies normal+Fast hand-plays (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Task 5: Delete `apply_effect` + `drive_effect_to_base`; rework the effect tests

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (delete both fns; rework ~30 test calls)
- Modify: `crates/game-core/src/engine/dispatch/choice.rs:150` (test rework)

**Interfaces:**
- Consumes: `push_effect`, `drive`.
- Produces: nothing new. A `#[cfg(test)]` helper `fn run(cx, effect, ctx) -> EngineOutcome { push_effect(cx, &effect, ctx); crate::engine::dispatch::drive(cx, EngineOutcome::Done) }` is acceptable in `evaluator.rs`'s test module — a thin alias over the real `drive`, carrying no resolution logic.

- [ ] **Step 1: Confirm no production caller remains**

Run: `grep -rn "apply_effect\|drive_effect_to_base" crates/game-core/src --include=*.rs | grep -v "fn apply_effect\|fn drive_effect_to_base"`
Expected: every hit is inside a `#[cfg(test)] mod tests` block (evaluator.rs ≈2154+, choice.rs:121/150). If any production hit remains, stop — an earlier task missed a site.

- [ ] **Step 2: Add the test-only `run` helper**

In `evaluator.rs`'s `#[cfg(test)] mod tests`, add the `run` alias (above). It pushes the root and drives via the real loop.

- [ ] **Step 3: Rework the evaluator test calls**

Replace each `apply_effect(cx, &effect, ctx)` / `apply_effect(\n …)` in the test module with `run(cx, effect, ctx)` (or `run(&mut cx, …)` matching the local binding). The semantics are identical: `Done` tests stay `Done`; `AwaitingInput` (controller-pick) tests stay `AwaitingInput` (the `Leaf` suspends in place under `drive` exactly as under `drive_effect_to_base`). Do them in batches, running `cargo test -p game-core --lib engine::evaluator` after each batch.

- [ ] **Step 4: Rework `choice.rs:150`**

Replace the `apply_effect` test call in `choice.rs`'s test module with the same `push_effect` + `drive` shape (inline, or a local `run` helper). Update its `use crate::engine::evaluator::{apply_effect, EvalContext}` import to `{push_effect, EvalContext}`.

- [ ] **Step 5: Delete the wrapper**

In `evaluator.rs`, delete `apply_effect` (lines 318-326) and `drive_effect_to_base` (lines 357-370). Keep `frame_of`, `step_effect_frame`, `suspend_leaf_in_place`. Fix any now-unused imports.

- [ ] **Step 6: Verify deletion + run evaluator/choice tests**

Run:
```bash
grep -rn "fn apply_effect\|fn drive_effect_to_base" crates/game-core/src && echo "STILL PRESENT" || echo "deleted"
cargo test -p game-core --lib engine::evaluator
cargo test -p game-core --lib engine::dispatch::choice
```
Expected: "deleted"; both test runs PASS.

- [ ] **Step 7: Full strict gauntlet incl. wasm**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green.

- [ ] **Step 8: Update the arc spec + commit**

In `docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md`, flip the #423 issue-map row (line ~240) from `open, keep as-is` to `✅ done — every effect site is top-frame dispatched; apply_effect/drive_effect_to_base deleted`. Then:
```bash
git add crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/choice.rs docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md
git commit -m "engine: delete apply_effect/drive_effect_to_base; tests on real drive (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Self-review notes

- **Spec coverage:** every spec site (1a/1b/6 → Task 1; 5a → Task 1; 3a/3b → Task 2; 4a/4b → Task 3; 2/5b → Task 4; delete + test rework → Task 5) maps to a task. The two structural changes (`PlayFromHand`, `EncounterCard.disposition`) are Tasks 4 and 2. The "delete, don't demote" + "test through real code" requirement is Task 5.
- **Open questions carried from the spec:** `PlayFromHandStage`'s exact variants (settled in Task 4 against `complete_play`'s live tail) and the event-flush reconciliation (Task 4 Step 8). Both are flagged with the invariant to hold (single `CardDiscarded`), not left vague.
- **Type consistency:** `push_effect(cx, &Effect, EvalContext)`, `Continuation::EncounterCard { card, disposition }`, `EncounterDisposition::{Discard, Spawn{investigator, metadata}}`, `Continuation::PlayFromHand { investigator, code, hand_index, stage }`, `PlayFromHandStage::{Dispose, AfterEnterWindow}` are used consistently across tasks.
- **Behaviour-preservation:** the card + integration suites stay untouched in Tasks 1–4; only the wrapper's own tests change (Task 5). Each task runs the strict gauntlet before its commit.
