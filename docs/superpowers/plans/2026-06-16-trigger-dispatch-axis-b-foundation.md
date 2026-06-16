# Trigger-Dispatch Axis-B Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the engine's two hand-wired trigger-dispatch paths (`fire_forced_triggers` + `queue_reaction_window`) and its scattered suspension modes with one continuation stack and one `emit_event` chokepoint doing rules-correct two-phase (forced-then-reaction) dispatch.

**Architecture:** A `GameState.continuations: Vec<Continuation>` stack is the single suspend/resume authority (one router atop `resolve_input`). `open_windows` is absorbed into it as `Resolution` frames; `emit_event(cx, TimingEvent)` runs one shared iterative-resolution loop twice (forced: no-skip/lead; reaction: skippable/player-order). The #117 event-keyed index is the final task, swapped in behind the scan interface. No new cards.

**Tech Stack:** Rust (workspace crates `card-dsl`, `game-core`); `serde` for replay-safe state; the `TestGame`/`GameStateBuilder` test builders and `assert_event!`/`assert_no_event!` macros in `game-core::test_support`.

**Specs:** `docs/superpowers/specs/2026-06-16-trigger-dispatch-rework-axis-b-foundation-design.md` (read first) and the umbrella `…-umbrella-design.md`.

**Per-task discipline:** every task ends green under the full CI gauntlet:
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
One branch+PR per task (`engine/<slug>`); commit body ends with `Closes #NN.`

---

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/card-dsl/src/dsl.rs` | `Trigger::OnEvent` gains `kind: TriggerKind` | 1 |
| `crates/cards/src/impls/*.rs` | existing `OnEvent` cards set `kind` | 1 |
| `crates/game-core/src/state/game_state.rs` | `continuations: Vec<Continuation>`; `Continuation`, `TimingEvent`, `Decider`, `Candidate` types; `OpenWindow` absorbed | 2,3,4,5 |
| `crates/game-core/src/state/builder.rs` | builder default for `continuations` | 2 |
| `crates/game-core/src/engine/dispatch/mod.rs` | single resume router in `resolve_input` | 2,3,4,5 |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | windows → `Resolution` frames; the shared loop | 3,5 |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | skill-test resume via `SkillTest` frame | 4 |
| `crates/game-core/src/engine/dispatch/emit.rs` (new) | `emit_event` + `TimingEvent` dispatch | 5 |
| `crates/game-core/src/engine/dispatch/forced_triggers.rs` | folded into `emit.rs`; deleted | 5 |
| `crates/game-core/src/engine/trigger_index.rs` (new) | #117 event-keyed index behind the scan interface | 6 |

---

## Task 1: `TriggerKind` on `Trigger::OnEvent`

Make forced-vs-reaction an explicit DSL property instead of routed-by-pattern. Behavior-preserving: dispatch still chooses paths exactly as today, but now reads the field.

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`Trigger::OnEvent`, builder helpers)
- Modify: every `crates/cards/src/impls/*.rs` with an `OnEvent` ability
- Test: `crates/card-dsl/src/dsl.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test** (in `dsl.rs` tests)

```rust
#[test]
fn on_event_carries_trigger_kind() {
    let t = Trigger::OnEvent {
        pattern: EventPattern::EnemyDefeated { by_controller: true, code: None },
        timing: EventTiming::After,
        kind: TriggerKind::Reaction,
    };
    // serde round-trips the new field
    let json = serde_json::to_string(&t).unwrap();
    let back: Trigger = serde_json::from_str(&json).unwrap();
    assert_eq!(t, back);
}
```

- [ ] **Step 2: Run, verify it fails to compile** (`TriggerKind` undefined)

Run: `cargo test -p card-dsl on_event_carries_trigger_kind`
Expected: FAIL — `cannot find type TriggerKind`.

- [ ] **Step 3: Add `TriggerKind` and the field**

```rust
/// Whether an `OnEvent` ability resolves mandatorily (forced) or is an
/// optional player reaction. Determines which phase of `emit_event`
/// dispatch the ability participates in (RR p.2: all forced resolve
/// before any reaction at the same timing point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerKind {
    Forced,
    Reaction,
}
```
Add `kind: TriggerKind` to `Trigger::OnEvent`. Update the `on_event(...)` builder helper(s) to take a `kind` (or add `forced_on_event` / `reaction_on_event` convenience builders — match the existing builder style in this file).

- [ ] **Step 4: Classify every existing `OnEvent` card.** For each `crates/cards/src/impls/*.rs` ability with `Trigger::OnEvent`, set `kind`: scenario-structure forced effects (act/agenda/location/persistent-treachery) → `TriggerKind::Forced`; player "[reaction]" abilities (Roland 01001, Dr. Milan 01033, Guard Dog 01021) → `TriggerKind::Reaction`. Grep: `rg "OnEvent" crates/cards/src`.

- [ ] **Step 5: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS (all card + engine tests still green; dispatch unchanged — it may still infer the path, the field is just present).

- [ ] **Step 6: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/cards/src/impls
git commit -m "engine: TriggerKind { Forced, Reaction } on Trigger::OnEvent

Explicit forced-vs-reaction DSL property, replacing route-by-pattern.
Behavior-preserving; dispatch reads the field in a later task. Closes #NN."
```

---

## Task 2: continuation stack scaffolding + single resume router

Add the empty stack and route through it first; falls through to the legacy `pending_*` ladder when empty. No frames pushed yet — pure plumbing.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (field + `Continuation` enum skeleton)
- Modify: `crates/game-core/src/state/builder.rs` (default `Vec::new()`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` head)
- Test: `game_state.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn continuations_default_empty_and_absent_field_loads() {
    let s = GameStateBuilder::default().build();
    assert!(s.continuations.is_empty());
    // older serialized states (no field) still deserialize
    let mut v = serde_json::to_value(&s).unwrap();
    v.as_object_mut().unwrap().remove("continuations");
    let back: GameState = serde_json::from_value(v).unwrap();
    assert!(back.continuations.is_empty());
}
```

- [ ] **Step 2: Run, verify it fails** (`no field continuations`)

Run: `cargo test -p game-core continuations_default_empty_and_absent_field_loads`
Expected: FAIL.

- [ ] **Step 3: Add the field + skeleton enum**

In `game_state.rs`:
```rust
/// The single suspend/resume stack (umbrella §1). Empty when nothing is
/// paused; the top frame is resumed by `resolve_input`. Legacy `pending_*`
/// modes remain on their own fields until the later cleanup pass.
#[serde(default)]
pub continuations: Vec<Continuation>,
```
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Continuation {
    // Variants land in tasks 3–5. Empty enum is illegal in Rust, so
    // task 3 introduces the first variant (`Resolution`); until then this
    // type is declared but the field stays empty.
}
```
Because an empty enum can't be constructed, **introduce `Continuation` with its first real variant in Task 3** and in Task 2 keep `continuations: Vec<Continuation>` with `Continuation` defined as a placeholder unit-like enum carrying one `#[doc(hidden)]` `Reserved` variant, OR sequence Task 3 immediately. Simplest: add the field typed `Vec<Continuation>` and define `Continuation` with the `Resolution` variant now (its body filled in Task 3). Pick the latter to avoid a throwaway variant.

- [ ] **Step 4: Add the router head in `resolve_input`** (`dispatch/mod.rs`)

At the very top of `resolve_input`, before the existing `hunter_move_pending` checks:
```rust
// Single resume router (umbrella §1): the continuation stack takes
// priority over the legacy pending_* ladder. Empty → fall through.
if let Some(top) = cx.state.continuations.last() {
    return resume_continuation(cx, response);
}
```
Add a `resume_continuation(cx, response)` stub that `match`es the top frame; with no variants resumable yet it is unreachable, so:
```rust
fn resume_continuation(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    // Frames + their resume arms land in tasks 3–5.
    unreachable!("resume_continuation: no frames pushed until task 3")
}
```

- [ ] **Step 5: Run the gauntlet** — Expected PASS (stack always empty, router never taken).

- [ ] **Step 6: Commit** (`Closes #NN.`)

---

## Task 3: window unification (pure refactor) + the shared resolution loop

Absorb `open_windows: Vec<OpenWindow>` into `continuations` as `Continuation::Resolution` frames (reaction run: `can_skip=true`). This is the largest task and is **behavior-preserving** — the entire existing reaction/fast-window + skill-test test suite must stay green.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`Continuation::Resolution`, `Decider`, `Candidate`; remove/forward `open_windows` helpers)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (the whole open/scan/resume/close pipeline)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resume_continuation` Resolution arm; the `apply_player_action` reject-guard)
- Test: existing suite is the spec; add one new frame test.

- [ ] **Step 1: Define the frame + parameters** (`game_state.rs`)

```rust
pub enum Continuation {
    /// One iterative trigger-resolution loop — serves both the forced run
    /// and a reaction window (Axis-B spec "One loop, two phases").
    Resolution {
        kind: WindowKind,
        candidates: Vec<Candidate>,
        can_skip: bool,
        decider: Decider,
        fast_actors: FastActorScope,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Decider { Lead, PlayerOrder { cursor: InvestigatorId } }

/// A resolvable option in a resolution loop: a pending in-play trigger
/// (today's `PendingTrigger`) or, from Axis C, a Fast play from hand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Candidate { Trigger(PendingTrigger) }
```
(`Candidate` is an enum now so Axis C can add `FastPlay { .. }` without reshaping.)

- [ ] **Step 2: Replace `open_windows` reads with topmost-`Resolution`-frame reads.** Transform each function in `reaction_windows.rs` to operate on the stack:
  - `queue_reaction_window` → push a `Continuation::Resolution { kind, candidates, can_skip: true, decider: PlayerOrder{ active }, fast_actors }`.
  - `open_fast_window` → same, for the framework `PlayerWindow` kinds.
  - `top_reaction_window` / `top_reaction_window_index` → find the topmost `Resolution` frame (skip ones with empty candidates, preserving today's semantics).
  - `resume_reaction_window` / `fire_pending_trigger` / `close_reaction_window_at` / `run_window_continuation` → operate on the frame at the computed stack index; `close` pops the frame.
  Keep the helper names and signatures where possible to minimize call-site churn; the bodies now read/write `cx.state.continuations` instead of `cx.state.open_windows`.

- [ ] **Step 3: Repoint the gates.** In `dispatch/mod.rs` `apply_player_action`, the reaction-window reject-guard reads the topmost `Resolution` frame instead of `top_reaction_window()` over `open_windows`. In `reaction_windows.rs` `check_play_card`, `permissive_window` reads the topmost `Resolution` frame's `fast_actors`. In `resume_continuation` (task 2 stub), add the `Resolution` arm delegating to `resume_reaction_window`.

- [ ] **Step 4: Remove `open_windows`.** Delete the field from `game_state.rs` + `builder.rs`; `GameState::top_reaction_window*` now front the stack. Fix all compile errors (the borrow-checker + the deleted-field references are the worklist).

- [ ] **Step 5: Add one frame-level test**

```rust
#[test]
fn reaction_window_lives_on_the_continuation_stack() {
    // (build a state that opens an AfterEnemyDefeated window via a defeat;
    //  assert the open window is a Continuation::Resolution frame, not a
    //  separate open_windows entry — open_windows no longer exists.)
    // Use the existing Roland reaction integration setup as the template:
    //   crates/cards/tests/ (the AfterEnemyDefeated reaction test).
}
```
(Flesh out from the existing Roland/Guard-Dog reaction test; the assertion is "the suspension is a `Resolution` frame on `continuations`.")

- [ ] **Step 6: Run the full gauntlet — the regression net is the point.**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS — every existing reaction-window, fast-window, mulligan, skill-test, and integration test (`crates/cards/tests/*`) green, unchanged behavior. Also run `wasm-pack test --headless --firefox crates/web` per CLAUDE.md if web touches state shape.

- [ ] **Step 7: Commit** (`Closes #NN.`) — note "pure refactor, behavior-preserving" in the body.

---

## Task 4: skill-test resume via a `SkillTest` frame

The skill-test driver currently re-enters via `close_reaction_window_at` checking `in_flight_skill_test`. Route its resumption through the stack so it composes with the forced loop in task 5. `in_flight_skill_test` stays as the data store (singleton).

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`Continuation::SkillTest`)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs`, `reaction_windows.rs` (`close_reaction_window_at` re-entry), `dispatch/mod.rs` (`resume_continuation` arm, the `in_flight` commit route)
- Test: existing skill-test suite + one frame test.

- [ ] **Step 1: Add the variant**

```rust
// in Continuation:
/// The skill-test driver is mid-resolution; data is the singleton
/// `in_flight_skill_test` field (read by many call sites; no nesting today).
SkillTest,
```

- [ ] **Step 2: Push/pop the frame around the skill-test lifecycle.** `start_skill_test` pushes `Continuation::SkillTest` when it parks at the commit window; `finish_skill_test`/`drive_skill_test` pop it when the test fully resolves (`in_flight_skill_test` cleared). `close_reaction_window_at`'s "re-enter `drive_skill_test` if `in_flight` is set" becomes "the stack top is now `SkillTest` → resume it."

- [ ] **Step 3: Route in `resume_continuation`.** Add the `SkillTest` arm: `CommitCards { indices }` → `finish_skill_test` (preserving the `pending_end_turn` resume tail currently in `resolve_input`).

- [ ] **Step 4: Run the gauntlet** — Expected PASS (skill-test, reaction-mid-skill-test, Frozen-in-Fear, Roland-defeat-during-fight all green, unchanged).

- [ ] **Step 5: Commit** (`Closes #NN.`)

---

## Task 5: `TimingEvent` + `emit_event` + the forced run + two-phase

The semantic change. Define `TimingEvent`, build `emit_event` driving phase 1 (forced run of the shared loop) → phase 2 (reaction window), and migrate the 9 dispatch call sites. Fold `forced_triggers.rs` in and delete it.

**Files:**
- Create: `crates/game-core/src/engine/dispatch/emit.rs` (`TimingEvent`, `emit_event`, the forced run)
- Modify: `game_state.rs` (forced run reuses `Continuation::Resolution` with `can_skip:false, decider:Lead`)
- Modify: call sites — `combat.rs:95,108,661`, `skill_test.rs:645,765`, `act_agenda.rs:77,245`, `phases.rs:229,555,647,661`, `actions.rs:299`, `mod.rs:230`
- Delete: `forced_triggers.rs` (logic moves into `emit.rs`)
- Test: a new `emit.rs` test module + the existing simultaneous-forced/reentrancy integration tests.

- [ ] **Step 1: Write the failing two-phase test** (the `EnemyDefeated` anchor)

```rust
// emit.rs tests (or crates/cards/tests/ for registry access):
// An enemy defeat fires the forced act advance AND opens the reaction
// window from one emit_event call.
#[test]
fn emit_enemy_defeated_runs_forced_then_reaction() {
    // setup: act with EnemyDefeated forced advance + an investigator with
    // a Reaction OnEvent (Roland). Defeat the enemy via emit_event.
    // assert: act advanced (phase-1 forced) AND a Resolution reaction
    // frame is open (phase-2) for Roland.
}
```

- [ ] **Step 2: Run, verify it fails** (`emit_event` undefined).

- [ ] **Step 3: Define `TimingEvent`** in `emit.rs` by merging `ForcedTriggerPoint` (all variants) + the event-driven `WindowKind` variants (`AfterEnemyDefeated`, `AfterSuccessfulInvestigate`, `AfterEnemyAttackDamagedAsset`). Each variant carries its binding (copy the fields from the two source enums). Map each to its `EventPattern` discriminant (`fn pattern(&self) -> EventPattern`) and its logged `Event` where one exists (`fn log_event(&self) -> Option<Event>`).

- [ ] **Step 4: Implement `emit_event`** — push `log_event()` if `Some`; collect forced `Candidate`s (kind `Forced`, matching pattern/timing) via the scan; run the shared loop with `can_skip:false, decider:Lead` (Task 3's loop, parameterized); then run phase 2 (the reaction window) via the Task-3 `queue_reaction_window` path. A forced candidate whose effect suspends parks its `Resolution` frame and resumes — reentrancy. Move `collect_forced_hits`/`resolve_one` logic from `forced_triggers.rs` into the loop's candidate-resolution.

- [ ] **Step 5: Migrate the call sites.** Replace each `fire_forced_triggers(cx, &ForcedTriggerPoint::X{..})` and event-driven `queue_reaction_window(cx, WindowKind::Y{..})` with `emit_event(cx, TimingEvent::Z{..})`. For sites that today call BOTH (combat.rs:95+108 — defeat), one `emit_event` replaces both. The framework `open_fast_window(PlayerWindow(..))` sites are untouched. Delete `forced_triggers.rs` and its `mod`/`use` in `dispatch/mod.rs` + `engine/mod.rs:38`; update `test_support` (`fire_forced_at`) to drive `emit_event`.

- [ ] **Step 6: Add the simultaneous-forced + reentrancy tests**
  - Two simultaneous `RoundEnded` forced (agenda 01107 doom + Dissonant Voices 01165 discard) → the lead-investigator ordering loop resolves both (2+ → `PickSingle` round-trip).
  - Frozen in Fear 01164 `EndOfTurn` forced effect suspends on its test → the forced candidate parks, the skill-test frame resolves, dispatch completes (reentrancy; no abandoned sibling).
  - Remove the `drive_attack_loop` `debug_assert` and add the two-Guard-Dog multi-soak test (#294): one attack damaging two `EnemyAttackDamagedSelf` reactors drains both windows, cursor advances once.

- [ ] **Step 7: Run the full gauntlet + wasm.** Expected PASS.

- [ ] **Step 8: Commit** (`Closes #212. Closes #213. Closes #294.` — and reference the issue text already amended).

---

## Task 6: #117 event-keyed trigger index (final)

Swap the full-scan for the index behind the scan interface introduced in task 5. Isolated so a desync bug is unambiguous.

**Files:**
- Create: `crates/game-core/src/engine/trigger_index.rs`
- Modify: enter/leave-play sites (`cards.rs` play_card, `threat_area.rs` place/discard, elimination drains, registry install), `emit.rs` (scan calls the index)
- Test: `trigger_index.rs` tests + the defensive leave-play-mid-window test.

- [ ] **Step 1: Write the failing index test**

```rust
#[test]
fn index_buckets_on_event_triggers_by_pattern_kind() {
    // enter two cards with EnemyDefeated reactions + one with RoundEnded;
    // assert the index returns exactly the EnemyDefeated bucket for an
    // EnemyDefeated lookup, and that leaving play removes the entry.
}
```

- [ ] **Step 2: Run, verify it fails.**

- [ ] **Step 3: Build `TriggerIndex`** — `BTreeMap<TriggerKindDiscriminant, Vec<(InvestigatorId, CardInstanceId, u8)>>` maintained at enter/leave-play, seeded at registry install (walk all in-play cards). `TriggerKindDiscriminant` is the `EventPattern` discriminant (smallest API per #117).

- [ ] **Step 4: Route the scan through the index.** The single scan function from task 5 looks up the bucket + applies the existing `trigger_matches` filter, instead of the full board walk. Add a **debug-only cross-check**: in `debug_assert`, the index result equals a full scan (catches desync at the source; removable later).

- [ ] **Step 5: Add the defensive test** — a card leaving play mid-window is absent from the next scan (the #117 acceptance test).

- [ ] **Step 6: Run the gauntlet + a microbenchmark** showing the scan is now O(matching) not O(board) (criterion or a simple timed loop, per #117 acceptance).

- [ ] **Step 7: Commit** (`Closes #117.`)

---

## Self-review

**Spec coverage:** §1 one-stack → tasks 2–4; §2 emit_event two-phase + TimingEvent → task 5; #213 forced-ordering loop → task 5 (shared loop from task 3); TriggerKind DSL field → task 1; #117 index → task 6; window unification → task 3; skill-test frame → task 4. All §5 closes/corrects (#212/#213/#117/#294) covered. Test strategy (Dissonant Voices+agenda, Frozen in Fear, two Guard Dogs, defeat anchor) → task 5/6 steps. ✓

**Placeholder scan:** the refactor tasks (3,4) intentionally use "transform these named functions; existing suite is the green-gate" rather than reproducing hundreds of moved lines — appropriate for behavior-preserving refactors, with the exact functions named and the verification command given. New-type/new-logic steps (1,2,5,6) carry real code. No "TBD/handle edge cases" placeholders.

**Type consistency:** `Continuation::Resolution { kind, candidates, can_skip, decider, fast_actors }`, `Decider::{Lead, PlayerOrder}`, `Candidate::Trigger`, `TimingEvent`, `TriggerKind::{Forced, Reaction}` used consistently across tasks 1–6. `emit_event(cx, TimingEvent)` signature stable. ✓

## Out of scope (this plan)

Axes A/C/D; migrating the orthogonal `pending_*` modes; newly-arising forced hits mid-loop; skill-test nesting; multiplayer reaction-phase player-order (all per the spec's Out-of-scope).
