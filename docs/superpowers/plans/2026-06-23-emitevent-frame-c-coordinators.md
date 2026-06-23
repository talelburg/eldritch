# EmitEvent-frame C-coordinators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the RR `when → at → after` timing axis structural via `EmitEvent`/`TimingPoint` coordinator frames, complete the round-end remodel onto them, and delete the now-vestigial `ForcedContinuation` mechanism.

**Architecture:** `emit_event` stays the one trigger chokepoint. Single-bucket events fire inline exactly as today (Checkpoint-C: no frame). The only multi-bucket event, `RoundEnded`, pushes a `Continuation::EmitEvent { event, bucket }` coordinator that the C-plumbing `drive` loop walks `When → At → After`, each bucket a `Continuation::TimingPoint { event, bucket, sub }` running forced-then-reaction with per-cell re-scan. The round-end `when` act-advance and `at` doom become uniformly-scanned registry abilities; teardown moves to the Upkeep anchor's resume cursor. With round-end and EndOfTurn both resumed via their own frames, the forced-run `ForcedContinuation` parking mechanism is vestigial and is deleted.

**Tech Stack:** Rust, single workspace. Engine kernel in `crates/game-core`. Cards in `crates/cards`. No async, no I/O in `game-core`.

## Global Constraints

- **Validate-first / mutate-second** in every handler: check all preconditions, return `EngineOutcome::Rejected { reason }` with state+events unchanged on any failure, mutate only after.
- **CI gauntlet, warnings-as-errors**, all must pass before any push:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Never hand-edit** `crates/cards/src/generated/cards.rs` (generated).
- **Card/rules text** must be looked up (ArkhamDB / vendored rules), never paraphrased from memory, when quoted in comments/commits.
- **Behaviour-preserving** is the bar for Tasks 1–4 (event log byte-identical for in-scope play); only Task 5's per-cell re-scan adds new behaviour, exercised by a synthetic fixture.
- Branch: `engine/emitevent-coordinators` (already created). One commit per task minimum; commit messages `scope: description`, body explains *why*, ends with the `Co-Authored-By` / `Claude-Session` trailers.

**Spec:** `docs/superpowers/specs/2026-06-23-emitevent-frame-c-coordinators-design.md` — read it before starting.

**Ordering rationale:** Each task is independently green. Task 1 (EndOfTurn) is independent and removes one `ForcedContinuation` variant. Task 2 (scan extension) is a dormant, separately-tested addition that de-risks Task 3. Task 3 is the atomic coordinator + round-end switch (removes the second non-trivial variant). Task 4 deletes the now-vestigial enum. Task 5 proves the new re-scan behaviour.

---

## Task 1: Unify EndOfTurn rotation onto the `InvestigatorTurn { ending }` frame

Eliminate `ForcedContinuation::EndOfTurnAfterForced` by routing **both** end-of-turn suspend paths (single skill-test strand, 2+ forced run) through the existing `InvestigatorTurn { ending: true }` frame via a new `drive`-loop arm. `end_turn` is invoked only from `PlayerAction::EndTurn`, so the `EndOfTurn` forced emit always runs inline before any cede; `ending: bool` therefore unambiguously means "only rotation remains" — no sub-cursor needed.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (the `drive` loop, ~line 232–246: add the `InvestigatorTurn { ending: true }` arm before the idle catch-all)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs:231-270` (`end_turn`: always set `ending` on suspend, drop the `is_forced` branch) and `crates/game-core/src/engine/dispatch/emit.rs:202-249` (`TimingEvent::forced_continuation`: `EndOfTurn` arm → `Terminal`)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:543-561` (teardown: stop calling `resume_end_turn` directly; cede to the loop)
- Modify: `crates/game-core/src/state/game_state.rs` (`ForcedContinuation`: delete `EndOfTurnAfterForced`) and `crates/game-core/src/engine/dispatch/reaction_windows.rs:981-983` (delete its `resume_forced_continuation` arm)
- Test: `crates/game-core/src/engine/dispatch/phases.rs` `#[cfg(test)]` (end-of-turn tests already there: `end_turn_*`)

**Interfaces:**
- Consumes: `Continuation::InvestigatorTurn { investigator, ending }` (game_state.rs:559), `phases::resume_end_turn(cx, InvestigatorId) -> EngineOutcome` (pub(super), phases.rs:280), `Continuation::is_forced` (game_state.rs:803).
- Produces: a `drive`-loop arm dispatching `InvestigatorTurn { ending: true }`. After this task `ForcedContinuation` = `{ Terminal, UpkeepAfterRoundEnded }`.

- [ ] **Step 1: Write a failing test — 2+ EndOfTurn forced still rotates via the frame**

There is existing coverage in phases.rs tests. Add a regression test asserting rotation happens after a *suspending* EndOfTurn forced run resolves, with the unified path. If a 2+-Frozen-in-Fear fixture is impractical to build directly, assert the simpler invariant the change must preserve: a single suspending EndOfTurn forced (one Frozen in Fear) resolves and the turn rotates / phase ends. Place near the other `end_turn_*` tests in `crates/game-core/src/engine/dispatch/phases.rs`:

```rust
#[test]
fn end_turn_with_suspending_forced_rotates_via_investigator_turn_frame() {
    // Build: Investigation phase, active investigator with a threat-area card
    // whose EndOfTurn forced opens a skill test (Frozen in Fear 01164 shape),
    // a second active investigator to rotate to. Drive EndTurn with a resolver
    // that resolves the forced skill test, then assert the active investigator
    // advanced to the second player (rotation ran exactly once).
    // (Use the TestGame builder + drive(...) resolver harness.)
}
```

- [ ] **Step 2: Run it to confirm it fails (or passes pre-change as a guard)**

Run: `cargo test -p game-core end_turn_with_suspending_forced_rotates_via_investigator_turn_frame`
Expected: PASS today (it's a behaviour-preserving guard) — keep it as the regression anchor. If you wrote it to assert the *new* loop-arm path specifically (e.g. that `resume_end_turn` is not called from skill_test teardown), it will fail until Step 4.

- [ ] **Step 3: Add the `drive`-loop arm for `InvestigatorTurn { ending: true }`**

In `crates/game-core/src/engine/dispatch/mod.rs`, inside `drive`'s `match top`, add **before** the idle `_ => return EngineOutcome::Done` arm:

```rust
// The open turn is ending: a suspending EndOfTurn forced (skill test or 2+
// forced run) stranded `end_turn` before rotation, flagging this frame. Now
// re-exposed on top, drive the rotation tail. `ending: false` stays the idle
// open-turn sentinel (the `_` arm below).
Some(Continuation::InvestigatorTurn { investigator, ending: true }) => {
    match phases::resume_end_turn(cx, investigator) {
        EngineOutcome::Done => {} // rotated / phase ended; loop on
        other => return other,
    }
}
```

- [ ] **Step 4: Cede from skill-test teardown instead of calling `resume_end_turn`**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, replace the block at 543-561 (the `if let Some(... InvestigatorTurn { ending: true }) = ... { resume_end_turn }`) with a bare `return EngineOutcome::Done;` and update the comment: the `drive` loop now re-dispatches the re-exposed `InvestigatorTurn { ending: true }` (no reach-down into `resume_end_turn`). Keep the forced-run note (a 2+ forced run sits on top and is loop-dispatched first).

- [ ] **Step 5: In `end_turn`, always flag the frame on suspend; drop the `is_forced` branch**

In `crates/game-core/src/engine/dispatch/phases.rs`, the `AwaitingInput` arm of `end_turn` (237-268) currently flags `ending` only when no forced run is open. Replace its body so it **always** locates the `InvestigatorTurn { investigator: active_id }` frame and sets `ending = true`, then returns the `AwaitingInput`. Remove the `forced_run_open` / `is_forced` check entirely:

```rust
EngineOutcome::AwaitingInput { .. } => {
    // A suspending EndOfTurn forced (skill test, or a 2+ forced run) stranded
    // rotation. Flag the InvestigatorTurn frame (below the suspension); the
    // `drive` loop re-dispatches it as `ending: true` once the suspension
    // resolves, running `resume_end_turn`. Unified path — no `is_forced`
    // special-case (the 2+ forced run no longer carries EndOfTurnAfterForced).
    let ending = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find_map(|c| match c {
            crate::state::Continuation::InvestigatorTurn { investigator, ending }
                if *investigator == active_id => Some(ending),
            _ => None,
        })
        .unwrap_or_else(|| {
            unreachable!("end_turn stranded with no InvestigatorTurn({active_id:?}) on the stack")
        });
    *ending = true;
    end_of_turn
}
```

- [ ] **Step 6: Point `EndOfTurn`'s `forced_continuation` at `Terminal`**

In `crates/game-core/src/engine/dispatch/emit.rs`, in `TimingEvent::forced_continuation` (202-249), change the `EndOfTurn { .. }` arm from `Some(ForcedContinuation::EndOfTurnAfterForced { investigator: *investigator })` to `Some(ForcedContinuation::Terminal)` (the 2+ forced run now closes to `Done`; the loop re-dispatches `InvestigatorTurn { ending: true }`). Update the arm's doc-comment to say so.

- [ ] **Step 7: Delete `ForcedContinuation::EndOfTurnAfterForced`**

In `crates/game-core/src/state/game_state.rs`, delete the `EndOfTurnAfterForced { investigator: InvestigatorId }` variant from `ForcedContinuation`. In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, delete its `resume_forced_continuation` arm (981-983). Fix any now-unused `InvestigatorId` import if the compiler flags it.

- [ ] **Step 8: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features` then `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check`.
Expected: all green. The end-of-turn tests + `the_gathering*` integration tests exercise rotation; confirm no regression.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
engine: unify EndOfTurn rotation onto InvestigatorTurn{ending}, drop EndOfTurnAfterForced

end_turn is player-action-only, so the EndOfTurn forced emit always runs inline
before any cede; ending:bool unambiguously means "only rotation remains". Add an
InvestigatorTurn{ending:true} drive-loop arm and route both suspend paths (single
skill-test strand, 2+ forced run) through it. The skill-test teardown cedes to the
loop instead of calling resume_end_turn; the 2+ forced run closes via Terminal.
Deletes ForcedContinuation::EndOfTurnAfterForced. Behaviour-preserving.

Part of #435.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

## Task 2: Extend the reaction scan to the current act/agenda, filtered by bucket

Add the ability to surface act/agenda `Reaction`-timed abilities (01109's `When`-`RoundEnded` advance) as reaction-window candidates, mirroring how `collect_forced_hits` already scans `act_deck`/`agenda_deck`. Parameterize the reaction scan by `EventTiming` bucket. Dormant after this task (nothing routes a `RoundEnded` reaction window yet — Task 3 does), so behaviour-preserving and tested directly.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`scan_pending_triggers`, 119+: add a current-act/agenda scan + a `bucket: EventTiming` parameter; thread it through `queue_reaction_window`)
- Reference: `crates/game-core/src/engine/dispatch/forced_triggers.rs:236-249` (the `RoundEnded` forced arm — mirror its act/agenda lookup), `crates/game-core/src/engine/dispatch/forced_triggers.rs:384-415` (`push_matching` shape for building a `ResolutionCandidate`)
- Test: `crates/cards/tests/` — a new integration test (needs `cards::REGISTRY` for 01109's abilities). Pattern: `crates/cards/tests/act_advancement.rs`.

**Interfaces:**
- Consumes: `card_registry::current()`, `ResolutionCandidate`, `CandidateSource::{Board, InPlay}`, `GameState::{act_deck, act_index, agenda_deck, agenda_index, turn_order}`, `card_dsl::dsl::{EventTiming, TriggerKind, EventPattern}`, `the_barrier::abilities()` (the `When`-`RoundEnded` reaction at index 1).
- Produces: `scan_pending_triggers(state, event, bucket: EventTiming) -> Vec<ResolutionCandidate>` now includes current-act/agenda `Reaction` abilities whose timing == `bucket`. (Callers updated to pass the event's bucket; today every reaction event is `After` except the Before-windows which pass `When` — see Step 4.)

- [ ] **Step 1: Write a failing test — the scan surfaces 01109's When-RoundEnded reaction**

Add to a new `crates/cards/tests/round_end_reaction_scan.rs` (or extend `act_advancement.rs`). Install the registry, set up The Gathering at act 01109 with clues sufficient on the Hallway, and assert the reaction scan for `TimingEvent::RoundEnded` at bucket `When` yields one candidate sourced from the act (`CandidateSource::Board`, controller = lead). If `scan_pending_triggers` is `pub(super)` and unreachable from an integration test, drive it indirectly through Task 3's behaviour instead and make this a unit test in `reaction_windows.rs`'s `#[cfg(test)]` module. Concretely, as a unit test:

```rust
#[test]
fn reaction_scan_surfaces_act_when_round_ended_advance() {
    // TestGame at act 01109 (The Barrier), registry installed, lead in turn_order.
    // let hits = scan_pending_triggers(&state, &TimingEvent::RoundEnded, EventTiming::When);
    // assert_eq!(hits.len(), 1);
    // assert!(matches!(hits[0].source, CandidateSource::Board));
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p game-core reaction_scan_surfaces_act_when_round_ended_advance` (or `-p cards` if integration).
Expected: FAIL — the scan does not yet look at the act, and has no `bucket` parameter.

- [ ] **Step 3: Add the `bucket` parameter + act/agenda scan to `scan_pending_triggers`**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, give `scan_pending_triggers` a `bucket: EventTiming` parameter. Inside the per-card matching, filter reaction abilities to those whose `EventTiming == bucket` (today's reaction abilities are uniformly one bucket per event, so this is behaviour-preserving). After the `cards_in_play` scan, add a current-act + current-agenda scan mirroring `collect_forced_hits`'s `RoundEnded` arm, but for `TriggerKind::Reaction` abilities matching the event's pattern, controller = `turn_order.first()` (the lead), `CandidateSource::Board`:

```rust
// Current act + agenda reaction abilities (act 01109's When-RoundEnded
// group advance). Mirrors collect_forced_hits's act/agenda scan; the act
// is in act_deck, not cards_in_play. Controller = the lead.
if let Some(lead) = state.turn_order.first().copied() {
    for code in [
        state.act_deck.get(state.act_index).map(|a| &a.code),
        state.agenda_deck.get(state.agenda_index).map(|a| &a.code),
    ]
    .into_iter()
    .flatten()
    {
        // push a Board-sourced ResolutionCandidate for each Reaction ability
        // whose pattern matches `event` and whose timing == `bucket`.
    }
}
```

(Follow `push_matching`'s candidate-construction shape; factor a shared helper if it reads cleanly, but do not over-abstract — a local loop is fine.)

- [ ] **Step 4: Thread `bucket` through `queue_reaction_window` and its callers**

`queue_reaction_window(cx, event)` calls `scan_pending_triggers`. The reaction bucket for an event is fixed: the Before-windows (`EnemyAttacks`, `WouldDiscoverClues`) are `When`; the rest (`EnemyDefeated`, `SuccessfullyInvestigated`, `EnteredPlay`, `EnemyAttackDamagedSelf`) are `After`. Add a `TimingEvent::reaction_bucket() -> EventTiming` helper (or pass the bucket explicitly from the coordinator in Task 3 and default `queue_reaction_window` to the event's fixed bucket for the inline single-bucket path). Keep the single-bucket callers behaviour-identical.

- [ ] **Step 5: Run the test + gauntlet**

Run: the new test, then `RUSTFLAGS="-D warnings" cargo test --all --all-features`, clippy, fmt.
Expected: green; no behaviour change for existing reaction windows.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
engine: reaction scan reaches current act/agenda + takes a bucket filter

scan_pending_triggers now scans the current act/agenda (controller = lead,
CandidateSource::Board) and filters reaction abilities by EventTiming bucket,
mirroring collect_forced_hits. Surfaces act 01109's When-RoundEnded group-advance
reaction as a window candidate. Dormant until the round-end coordinator consumes
it (next task); behaviour-preserving for existing windows.

Part of #435.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

## Task 3: `EmitEvent`/`TimingPoint` coordinator frames + route round-end through them

The atomic switch: add the coordinator frames + their `drive`-loop arms, make `emit_event(RoundEnded)` push the coordinator, move round-end teardown to the Upkeep anchor's new `AfterRoundEnd` resume, and delete the `ActRoundEnd` hand-thread + `Act.round_end_advance` field + `ForcedContinuation::UpkeepAfterRoundEnded`. Behaviour-preserving for in-scope round-end (`when → at` order, same group spend, same doom).

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` — add `Continuation::EmitEvent { event, bucket }` + `Continuation::TimingPoint { event, bucket, sub }` + `enum TimingSub { Forced, Reaction }`; add `UpkeepResume::AfterRoundEnd`; delete `Continuation::ActRoundEnd` + `ActRoundEndPending` + `RoundEndAdvance` + `Act.round_end_advance`; delete `ForcedContinuation::UpkeepAfterRoundEnded`.
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` — add `EmitEvent` + `TimingPoint` arms to `drive`; delete the `ActRoundEnd` `ResolveInput`/`apply_player_action` routing arm.
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` — `emit_event`'s `RoundEnded` branch pushes the coordinator; move `RoundEnded`'s `forced_continuation` arm to the loud-guard group; add the `EmitEvent`/`TimingPoint` dispatch functions (or place them in a new `coordinator.rs` — see Step 1).
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` — rewrite `upkeep_phase_end`; add the `UpkeepResume::AfterRoundEnd` arm to `anchor_on_child_pop`; delete `round_end_advance_window`, `resume_act_round_end_advance`, `upkeep_round_end_at_and_after`, `fire_act_round_end_ability`, `round_end_advance_ability_index`; keep `upkeep_round_end_teardown` (re-pointed).
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` — delete `UpkeepAfterRoundEnded`'s `resume_forced_continuation` arm.
- Modify: scenario setup that sets `Act.round_end_advance` (`crates/scenarios/src/the_gathering.rs` — `setup()`); `crates/game-core/src/test_support/mod.rs` (the `round_end_advance` test-support reference flagged in the spec).
- Test: `crates/game-core/tests/act_round_end.rs`, `crates/cards/tests/act_advancement.rs`, `crates/scenarios/tests/the_gathering*.rs` (update the advance input shape from `ActRoundEnd` `Confirm`/`Skip` to the reaction-window `PickSingle`/`Skip`).

**Interfaces:**
- Consumes: `TimingEvent` (emit.rs:48), `card_dsl::EventTiming`, `collect_forced_hits(state, &ForcedTriggerPoint, EventTiming)` (forced_triggers.rs:149), `fire_forced_triggers(cx, &ForcedTriggerPoint, EventTiming)` (forced_triggers.rs:132), `open_forced_resolution(cx, &TimingEvent, Vec<ResolutionCandidate>, ForcedContinuation)` (reaction_windows.rs:83), `queue_reaction_window`/`open_queued_reaction_window` (reaction_windows.rs), `scan_pending_triggers(state, event, bucket)` (Task 2), `phases::upkeep_round_end_teardown`, `anchor_on_child_pop`.
- Produces: `Continuation::EmitEvent`/`TimingPoint` + their `drive` arms; `UpkeepResume::AfterRoundEnd`. After this task `ForcedContinuation` = `{ Terminal }` only.

- [ ] **Step 1: Add the frame types**

In `crates/game-core/src/state/game_state.rs`, add to `Continuation`:

```rust
/// Coordinator: iterate the RR timing buckets When → At → After for one game
/// event (EmitEvent-frame C-coordinators, #434). `bucket` is the cursor.
/// Pushed by `emit_event` for the only multi-bucket event (RoundEnded);
/// driven by the `drive` loop, suspending at the `when` reaction window.
EmitEvent {
    event: crate::engine::TimingEvent,
    bucket: crate::dsl::EventTiming,
},
/// Coordinator: one timing bucket, run forced then reaction (`sub` cursor).
/// What single-bucket `emit_event` does today, parameterized by bucket and
/// made frame-resumable. Child of `EmitEvent`.
TimingPoint {
    event: crate::engine::TimingEvent,
    bucket: crate::dsl::EventTiming,
    sub: TimingSub,
},
```

And the cursor enum (near the other `*Resume` enums):

```rust
/// `TimingPoint`'s forced-then-reaction sub-cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingSub { Forced, Reaction }
```

Add `AfterRoundEnd` to `UpkeepResume` (find its `enum UpkeepResume` definition near `MythosResume`).

- [ ] **Step 2: Write the failing behaviour test — round-end advance via the coordinator's reaction window**

In `crates/game-core/tests/act_round_end.rs` (or a new test), drive a round end at act 01109 with sufficient Hallway clues and a resolver that picks the offered advance candidate; assert the act advanced and the agenda doom (`at`) fired *after*. This is the existing round-end behaviour, now reached via the coordinator's `When` reaction `PickSingle` rather than the `ActRoundEnd` `Confirm`. Write it against the new input shape.

```rust
#[test]
fn round_end_advances_via_coordinator_when_reaction_then_at_doom() {
    // Gathering at act 01109, Hallway clues >= threshold, agenda with at-doom.
    // Drive to round end (EndTurn of last investigator). The When reaction
    // window offers the advance candidate; resolve PickSingle(that candidate).
    // Assert: ActAdvanced event precedes AgendaDoom change (when -> at order),
    // and the act index advanced.
}
```

- [ ] **Step 3: Run it to confirm it fails**

Run: `cargo test -p game-core round_end_advances_via_coordinator_when_reaction_then_at_doom`
Expected: FAIL — coordinator not wired; old path still uses `ActRoundEnd` `Confirm`.

- [ ] **Step 4: Implement `emit_event`'s coordinator branch + the dispatch functions**

In `crates/game-core/src/engine/dispatch/emit.rs`, change `emit_event` so the multi-bucket event short-circuits to the coordinator (keep the single-bucket path verbatim):

```rust
pub(crate) fn emit_event(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    // The only multi-bucket event: cede to the coordinator + the global loop.
    if matches!(event, TimingEvent::RoundEnded) {
        cx.state.continuations.push(crate::state::Continuation::EmitEvent {
            event: event.clone(),
            bucket: crate::dsl::EventTiming::When,
        });
        return EngineOutcome::Done; // caller must cede (set its resume cursor)
    }
    // --- single-bucket path, unchanged ---
    if event.opens_reaction_window() {
        super::reaction_windows::queue_reaction_window(cx, event);
    }
    let Some(point) = event.forced_point() else { return EngineOutcome::Done; };
    let candidates = collect_forced_hits(cx.state, &point, event.reaction_bucket_or_after());
    if candidates.len() >= 2 {
        let continuation = event.forced_continuation().unwrap_or_else(|| unreachable!(/* … */));
        super::reaction_windows::open_forced_resolution(cx, event, candidates, continuation)
    } else {
        fire_forced_triggers(cx, &point, event.forced_bucket())
    }
}
```

(Single-bucket events keep `EventTiming::After` as their forced bucket — `forced_bucket()` returns `After` for them. Keep the existing hardcoded `After` if you prefer; the bucket abstraction matters only for the coordinator.) Add the dispatch functions (in `emit.rs` or a new `crates/game-core/src/engine/dispatch/coordinator.rs` module — prefer a new module if `emit.rs` grows past ~400 lines):

```rust
/// Dispatch a `Continuation::EmitEvent` frame on top: walk When → At → After,
/// pushing a `TimingPoint` per populated bucket, re-scanning each cell fresh.
pub(super) fn dispatch_emit_event(cx: &mut Cx) -> EngineOutcome {
    use crate::dsl::EventTiming::*;
    let Some(Continuation::EmitEvent { event, bucket }) = cx.state.continuations.last().cloned()
    else { unreachable!("dispatch_emit_event: top is not EmitEvent") };
    // Re-scan eligibility for this bucket (board state may have changed).
    let point = event.forced_point();
    let has_forced = point.as_ref().map_or(false, |p| {
        !collect_forced_hits(cx.state, p, bucket).is_empty()
    });
    let has_reaction = !super::reaction_windows::scan_pending_triggers(cx.state, &event, bucket).is_empty();
    if has_forced || has_reaction {
        // pop EmitEvent, push it back with the same bucket beneath a fresh
        // TimingPoint, so on the child's pop we re-expose EmitEvent at `bucket`
        // and advance. (Or keep EmitEvent in place and push TimingPoint on top.)
        cx.state.continuations.push(Continuation::TimingPoint {
            event: event.clone(), bucket, sub: TimingSub::Forced,
        });
        return EngineOutcome::Done; // loop dispatches the TimingPoint
    }
    // Empty bucket: advance the cursor, or finish.
    match bucket {
        When => set_emit_bucket(cx, At),
        At => set_emit_bucket(cx, After),
        After => { cx.state.continuations.pop(); } // coordinator done
    }
    EngineOutcome::Done
}
```

The TimingPoint pop must advance `EmitEvent`'s bucket. Implement that by having `dispatch_emit_event` detect "I was just re-exposed after a child TimingPoint completed this bucket" — simplest: when the `TimingPoint` for `bucket` pops, advance `EmitEvent.bucket` in the same step (the `TimingPoint` dispatch advances its parent's cursor before popping itself), so the re-exposed `EmitEvent` is already at the next bucket and re-scans it. Pick one mechanism and keep it consistent; document it in the dispatch doc-comment. Add `set_emit_bucket(cx, EventTiming)` mutating the top `EmitEvent`'s `bucket` in place.

```rust
/// Dispatch a `Continuation::TimingPoint` on top: run `sub: Forced` then
/// `sub: Reaction` for one bucket, then advance the parent EmitEvent's cursor
/// and pop self.
pub(super) fn dispatch_timing_point(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::TimingPoint { event, bucket, sub }) = cx.state.continuations.last().cloned()
    else { unreachable!("dispatch_timing_point: top is not TimingPoint") };
    match sub {
        TimingSub::Forced => {
            // advance our own cursor to Reaction *first* (so re-dispatch after a
            // suspending forced run resumes at Reaction, not re-scanning forced).
            set_timing_sub(cx, TimingSub::Reaction);
            if let Some(point) = event.forced_point() {
                let candidates = collect_forced_hits(cx.state, &point, bucket);
                if candidates.len() >= 2 {
                    // No framework tail inside a coordinator — pass Terminal
                    // (no-op; the loop re-dispatches this TimingPoint at
                    // Reaction). Task 4 removes the continuation entirely.
                    return super::reaction_windows::open_forced_resolution(
                        cx, &event, candidates, crate::state::ForcedContinuation::Terminal,
                    );
                }
                return fire_forced_triggers(cx, &point, bucket);
            }
            EngineOutcome::Done
        }
        TimingSub::Reaction => {
            // Open the bucket's reaction window (round-end When act advance).
            let candidates = super::reaction_windows::scan_pending_triggers(cx.state, &event, bucket);
            if !candidates.is_empty() {
                // push a TimingPointWindow { mode: Reaction } and open it
                // (suspends). On its pop, re-expose this TimingPoint at Reaction;
                // we then fall through to "advance parent + pop self".
                return super::reaction_windows::open_reaction_window_with(cx, &event, candidates);
            }
            // Bucket done: advance parent EmitEvent's cursor, then pop self.
            advance_parent_emit_bucket(cx, bucket);
            cx.state.continuations.pop(); // pop this TimingPoint
            EngineOutcome::Done
        }
    }
}
```

Note the `Reaction` sub is re-entered after its window closes (the window popped, `TimingPoint` re-exposed at `Reaction`); guard against re-opening a now-resolved window by checking the scan is empty on re-entry (the act advance, once taken, no longer matches — the act advanced; if Skipped, the candidate is gone because the player declined — ensure the scan does not re-offer a declined window: a declined reaction window pops without re-pushing, and `scan_pending_triggers` re-run would re-offer it. To avoid a loop, the `Reaction` sub must distinguish "not yet opened" from "opened and closed". Track this with the existing window lifecycle: opening pushes the `TimingPointWindow`; its close pops back to `TimingPoint` at `Reaction`. Use a third `TimingSub::ReactionDone`, or detect the just-closed window via the resume path. **Decision: add `TimingSub::Done`** — set it when the window is opened, so on re-entry the `Reaction`→`Done` transition advances the parent and pops. Update the enum to `{ Forced, Reaction, Done }`.)

Reconcile the enum: `TimingSub { Forced, Reaction, Done }`. `Forced` → fire forced, set `Reaction`. `Reaction` → if candidates, open window + set `Done`; else advance parent + pop. `Done` → advance parent + pop (the window already resolved).

Add helpers `set_emit_bucket`, `set_timing_sub`, `advance_parent_emit_bucket` (the last mutates the `EmitEvent` frame *beneath* the just-popped `TimingPoint`). Add `open_reaction_window_with(cx, event, candidates)` in `reaction_windows.rs` (push `TimingPointWindow { mode: Reaction }` + `open_queued_reaction_window`) if no existing entry fits.

- [ ] **Step 5: Add the `drive`-loop arms**

In `crates/game-core/src/engine/dispatch/mod.rs`'s `drive`, add arms (before the idle catch-all):

```rust
Some(Continuation::EmitEvent { .. }) => match emit::dispatch_emit_event(cx) {
    EngineOutcome::Done => {}
    other => return other,
},
Some(Continuation::TimingPoint { .. }) => match emit::dispatch_timing_point(cx) {
    EngineOutcome::Done => {}
    other => return other,
},
```

- [ ] **Step 6: Rewrite `upkeep_phase_end` to cede to the coordinator**

In `crates/game-core/src/engine/dispatch/phases.rs`, replace `upkeep_phase_end`'s body (979-1025) so it: emits `PhaseEnded { Upkeep }` (single-bucket, inline, as today — keep the `debug_assert!(Done)`); sets the Upkeep anchor's resume cursor to `UpkeepResume::AfterRoundEnd`; calls `emit_event(cx, &TimingEvent::RoundEnded)` (which pushes the coordinator and returns `Done`); returns `Done`. Delete the `round_end_advance_window` / `ActRoundEnd` block and the `upkeep_round_end_at_and_after` call.

```rust
pub(crate) fn upkeep_phase_end(cx: &mut Cx) -> EngineOutcome {
    cx.events.push(Event::PhaseEnded { phase: Phase::Upkeep });
    let forced = super::emit::emit_event(cx, &super::emit::TimingEvent::PhaseEnded { phase: Phase::Upkeep });
    debug_assert!(matches!(forced, EngineOutcome::Done), "PhaseEnded(Upkeep) forced did not resolve: {forced:?}");
    // Set the anchor's resume to run teardown when the RoundEnded coordinator
    // pops, then cede: push the coordinator and return. The `when` act window
    // and `at` doom resolve under the global loop.
    set_upkeep_resume(cx, crate::state::UpkeepResume::AfterRoundEnd);
    super::emit::emit_event(cx, &super::emit::TimingEvent::RoundEnded) // pushes EmitEvent, returns Done
}
```

Add `set_upkeep_resume(cx, UpkeepResume)` mutating the top `UpkeepPhase` anchor's `resume` in place (the anchor is on top here — `debug_assert!` it). In `anchor_on_child_pop`, add the arm:

```rust
Some(Continuation::UpkeepPhase { resume: UpkeepResume::AfterRoundEnd }) => upkeep_round_end_teardown(cx),
```

- [ ] **Step 7: Delete the round-end hand-thread + `Act.round_end_advance`**

Delete from `phases.rs`: `round_end_advance_window`, `resume_act_round_end_advance`, `upkeep_round_end_at_and_after`, `fire_act_round_end_ability`, `round_end_advance_ability_index`. From `game_state.rs`: `Continuation::ActRoundEnd` + `ActRoundEndPending` + `RoundEndAdvance` + the `Act.round_end_advance` field (and its `None` initializer at ~2140). From `mod.rs`: the `ActRoundEnd` `ResolveInput`/`apply_player_action` routing arm. From `reaction_windows.rs`: the `UpkeepAfterRoundEnded` arm of `resume_forced_continuation`. From `game_state.rs`: `ForcedContinuation::UpkeepAfterRoundEnded` variant. In `the_barrier.rs`, update the doc-comment that says the contributor location "stays a kernel `Act.round_end_advance` data field" (now it's printed-in-card only). Update scenario setup (`the_gathering.rs::setup`) + `test_support/mod.rs` to stop setting `round_end_advance`.

- [ ] **Step 8: Move `RoundEnded`'s `forced_continuation` arm to the loud-guard group**

In `emit.rs`'s `TimingEvent::forced_continuation`, move `RoundEnded` from `Some(UpkeepAfterRoundEnded)` into the `None` loud-guard group (round-end no longer reaches `emit_event`'s forced path — it short-circuits to the coordinator). Update the doc-comment.

- [ ] **Step 9: Run the round-end test + full suite**

Run: `cargo test -p game-core round_end_advances_via_coordinator_when_reaction_then_at_doom`, then `RUSTFLAGS="-D warnings" cargo test --all --all-features`, clippy, fmt, doc.
Expected: green. Pay attention to `act_round_end.rs`, `act_advancement.rs`, `the_barrier.rs`, `the_gathering*.rs` — update any that drove the advance via `ActRoundEnd` `Confirm` to the new `PickSingle`/`Skip` reaction shape.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
engine: EmitEvent/TimingPoint coordinators + round-end remodel (#434)

Add the When->At->After coordinator frames (EmitEvent/TimingPoint) as drive-loop
arms; emit_event(RoundEnded) cedes to them. The round-end when-act-advance and
at-doom are now uniformly-scanned registry abilities; teardown runs on the Upkeep
anchor's AfterRoundEnd resume. Deletes the ActRoundEnd hand-thread, the
Act.round_end_advance field, and ForcedContinuation::UpkeepAfterRoundEnded.
Behaviour-preserving for in-scope round-end (when->at order, group spend, doom).

Part of #435.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

## Task 4: Delete `ForcedContinuation` entirely

Only `Terminal` survives (a no-op). Make forced runs carry no continuation: close → `Done` → the loop re-dispatches the exposed parent frame.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` — delete the `ForcedContinuation` enum; change `TimingMode::Forced(ForcedContinuation)` → `TimingMode::Forced`.
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` — `open_forced_resolution` drops its `continuation` arg; `close_reaction_window`'s forced arm returns `Done`; delete `resume_forced_continuation`.
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` — delete `TimingEvent::forced_continuation`; `emit_event`'s 2+ branch + `dispatch_timing_point`'s 2+ branch call `open_forced_resolution` without a continuation.
- Reference: every `TimingMode::Forced(...)` construction/match site (grep).

**Interfaces:**
- Consumes: nothing new.
- Produces: `TimingMode::Forced` (unit), `open_forced_resolution(cx, &TimingEvent, Vec<ResolutionCandidate>) -> EngineOutcome`.

- [ ] **Step 1: Confirm `Terminal` is the only remaining variant**

Run: `grep -rn "ForcedContinuation::" crates/game-core/src`
Expected: only `Terminal` constructions remain (in `emit_event`, `dispatch_timing_point`, and `forced_continuation`'s `EnteredLocation`/`RoundEnded`→Terminal arm). If any other variant appears, Tasks 1/3 are incomplete — stop and fix.

- [ ] **Step 2: Write/adjust a guard test — a 2+ forced run resumes the parent frame**

Add or confirm a test that a 2+ simultaneous forced run (e.g. EnteredLocation with two matching forced, or the round-end `at` doom + Dissonant Voices) resolves and control returns to the exposed parent frame, with the framework continuing correctly. Reuse existing forced-run coverage in `forced_triggers.rs`/`reaction_windows.rs` tests; assert the post-run framework still runs (e.g. round-end teardown reaches Mythos).

- [ ] **Step 3: Drop the continuation from `TimingMode::Forced`**

In `game_state.rs`, change `Forced(ForcedContinuation)` to `Forced`. Delete the `ForcedContinuation` enum. Fix every match/construction site the compiler flags (`open_forced_resolution`, `close_reaction_window`, any test).

- [ ] **Step 4: Simplify the forced-run close path**

In `reaction_windows.rs`: `open_forced_resolution(cx, event, candidates)` (no continuation arg) pushes `TimingMode::Forced`. In `close_reaction_window`, replace the `Continuation::TimingPointWindow { mode: Forced(continuation), .. }` arm's `resume_forced_continuation(cx, cont)` with `EngineOutcome::Done` (the loop re-dispatches `continuations.last()`). Delete `resume_forced_continuation`. Delete `TimingEvent::forced_continuation` in `emit.rs` and the `.unwrap_or_else(unreachable!)` in `emit_event`'s 2+ branch (now just `open_forced_resolution(cx, event, candidates)`); same in `dispatch_timing_point`.

- [ ] **Step 5: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, clippy, fmt, doc, plus the wasm build + wasm clippy.
Expected: all green. The `unreachable!` 2+ guards in single-bucket callers (`enemy_defeat`, `advance_agenda`, …) stay as `debug_assert!(forced == Done)` — they never open a run in scope; if one ever does, `open_forced_resolution` returns `AwaitingInput` and the caller's assert fires loudly (the documented invariant).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
engine: delete ForcedContinuation — forced runs carry no continuation

With round-end and EndOfTurn resumed via their own frames, the only surviving
variant (Terminal) was already a no-op. Drop TimingMode::Forced's payload, delete
the enum + resume_forced_continuation + TimingEvent::forced_continuation; a forced
run now closes to Done and the C-plumbing loop re-dispatches the exposed parent
frame. Invariant: any 2+-forced-capable emit site must resume via a frame, else
its debug_assert(Done) fires. Behaviour-preserving.

Part of #435.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

## Task 5: Per-cell re-scan §G synthetic regression test

Prove the coordinator re-scans eligibility entering each cell — a `when`-cell that mutates board state changes whether an `at`-cell forced fires. No production change (re-scan is structural from Task 3's `dispatch_emit_event`); this adds the synthetic act/agenda fixture test.

**Files:**
- Create/Modify: a test fixture + test in `crates/game-core/src/engine/` `#[cfg(test)]` or `crates/game-core/tests/`. If it needs registry abilities, use `crates/cards/tests/` with a test-only act/agenda whose abilities flip each other — or a `test_support` synthetic registry. Pattern: existing `act_round_end.rs` + the `TestGame` builder.

**Interfaces:**
- Consumes: the coordinator from Task 3; `TestGame` builder; `assert_event!`/`assert_no_event!` macros.

- [ ] **Step 1: Write the §G test**

A `when`-cell ability removes the precondition an `at`-cell forced ability needs, so the `at` forced must **not** fire after the `when` cell runs. Concretely: a synthetic act with a `When`-`RoundEnded` reaction whose effect clears some board state (e.g. removes a token/flag), and a synthetic agenda with an `At`-`RoundEnded` forced whose eligibility depends on that state. Drive round-end, take the `when` reaction, assert the `at` forced did not fire (its event is absent):

```rust
#[test]
fn round_end_at_forced_rescanned_after_when_cell_changes_eligibility() {
    // Synthetic act: When-RoundEnded reaction that clears a precondition.
    // Synthetic agenda: At-RoundEnded forced gated on that precondition.
    // Drive round-end + resolve the When reaction.
    // assert_no_event!(events, <the at-forced's effect event>);
    // (and the converse control: without taking the When reaction, the at fires.)
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p game-core round_end_at_forced_rescanned_after_when_cell_changes_eligibility` (or `-p cards`).
Expected: PASS (re-scan already structural). If it FAILS — the coordinator pre-computed eligibility instead of re-scanning per cell — fix `dispatch_emit_event` to re-scan at each bucket entry (it should already; this test guards it).

- [ ] **Step 3: Run the full gauntlet + commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
test: §G per-cell re-scan regression for the round-end coordinator

A when-cell that mutates board state changes whether an at-cell forced fires;
the coordinator re-scans eligibility entering each bucket rather than
pre-computing. Synthetic act/agenda fixture (no corpus card exercises
cross-bucket suppression in scope).

Closes #434. Part of #435.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

## Self-Review

**Spec coverage:**
- Coordinator frames + drive-loop arms → Task 3 (Steps 1, 4, 5). ✓
- `emit_event` chokepoint (inline single-bucket, coordinator for round-end) → Task 3 Step 4. ✓
- Round-end remodel (delete `ActRoundEnd`, Upkeep `AfterRoundEnd`, surface 01109 reaction) → Tasks 2 + 3. ✓
- EndOfTurn unification → Task 1. ✓
- Delete `ForcedContinuation` → Tasks 1 (EndOfTurn variant), 3 (Upkeep variant), 4 (the enum). ✓
- Per-cell re-scan + §G test → Task 3 (structural) + Task 5 (test). ✓
- Open question (potential-gate at 0 clues) → deferred per spec; native no-ops. Not a task. ✓

**Placeholder scan:** Step 4 of Task 3 contains genuine design detail (the `TimingSub::Done` reconciliation) rather than copy-paste code — this is the one step where the implementer must integrate against the live window lifecycle; it names the exact mechanism (`{ Forced, Reaction, Done }`) and the helpers to add, which is actionable, not a placeholder. All other code/deletions are concrete.

**Type consistency:** `TimingSub` resolved to `{ Forced, Reaction, Done }` (Task 3 Step 4) — used consistently in `dispatch_timing_point`. `open_forced_resolution` loses its `continuation` arg in Task 4 (Task 3 still passes `Terminal`; the call sites update in Task 4). `set_emit_bucket`/`set_timing_sub`/`advance_parent_emit_bucket`/`set_upkeep_resume`/`open_reaction_window_with` are the new helpers, defined where introduced.

**Risk note:** Task 3 is the large atomic switch and the highest-blast-radius. The `TimingSub::Done` re-entry guard (avoiding re-opening a declined reaction window) is the subtlest correctness point — verify with both the "take the advance" and "skip the advance" paths in the Task 3 Step 2 test (add a `_skip_` sibling asserting the act does not advance and the `at` doom still fires).
