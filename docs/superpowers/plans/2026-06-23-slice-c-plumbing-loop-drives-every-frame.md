# Slice C-plumbing — loop drives every frame — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `drive` loop dispatch **every** continuation frame by uniform
top-frame dispatch — windows, skill tests, encounter-card disposal — deleting the
reach-down accessors, the five synchronous skill-test re-entry sites, and the
`resolve_input` encounter-disposal chokepoint.

**Architecture:** One atomic behaviour-preserving refactor built on a single
invariant: **the continuation stack is the resolution order; `continuations.last()`
is always what resolves or awaits next.** Every driver returns `Done` to the loop
instead of reaching down the stack to call the next driver; the loop dispatches
`last()`, advancing it when it is a phase-anchor / `ActionResolution` / `Effect` /
`SkillTest` / window-with-candidates, and idling (return `Done`) when it is
`InvestigatorTurn` / an empty-`FastWindow` gate / the empty stack. The advance-vs-idle
test is `Continuation::awaits_input()` / non-empty candidates, **except** a
`TimingPointWindow` is always dispatched (empty ⇒ close), while an empty `FastWindow`
gate idles (permissive, awaits `Skip`).

**Tech Stack:** Rust, `cargo test`/`clippy`/`fmt`/`doc`, the `game-core` engine crate.

## Global Constraints

- **Behaviour-preserving — event log byte-identical**, modulo exactly one rewritten
  test (`close_reaction_window_at_removes_reaction_window_not_empty_phase_gate_on_top`,
  which encodes a stack shape the invariant forbids). No game-rule change.
- **This is ONE atomic PR.** The conversion is holistic (see spec "Why the plumbing is
  one atomic slice"): you cannot half-flip the drivers — a driver still reaching down
  past a frame the loop now owns is the exact contradiction that failed the first
  attempt. The suite is green only at the **end**, not between sub-edits.
- **CI gauntlet, warnings-as-errors.** Before push, match every flag from `CLAUDE.md`
  Commands (test/clippy/fmt/doc + wasm build/clippy). Plain `cargo test` is insufficient.
- **No silent reach-down left behind:** after the change, `grep -rn
  'top_reaction_window_index\|win_idx > st\|top_reaction_window\b'` over
  `crates/game-core/src/engine/` must return nothing (the `permissive_window`
  `top_window` read is allowed and stays).
- **Commit subject:** `engine: <description>`; body explains *why*, ends with
  `Part of #431.`

---

## Scope note

Covers **only** the C-plumbing slice of the arc (spec:
`docs/superpowers/specs/2026-06-23-emitevent-frame-slice-c-loop-driving-design.md`).
C-coordinators and D (#423) follow.

**Stays imperative (out of scope, by design):**
- The combat re-entry `run_reaction_continuation` → `resume_enemy_attack`
  (`AttackLoop` is not yet a loop arm — #411 Shape A).
- `open_fast_window`'s open-time **auto-skip** decision (it stays; only the `Phase`
  continuation it runs is affected — see Step 5's caveat).
- The `top_window` Fast-play `permissive_window` timing gate (a legitimate read, not a
  driver reach-down).

## File structure

| File | Change |
|---|---|
| `crates/game-core/src/engine/dispatch/mod.rs` | `drive`: add `TimingPointWindow` / `FastWindow` / `SkillTest` / `EncounterCard` arms. `resolve_input`: handlers step-and-return; delete the tail `EncounterCard` chokepoint. |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | `fire_pending_trigger` / `play_fast_event` return `Done`; `close_reaction_window_at` delete the skill-test seam; `run_fast_continuation` both paths return `Done`; `resume_before_discover_window` return `Done`. |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | `advance`: delete the `rposition(SkillTest)` + `win_idx > st` self-location; `fire_retaliate_if_any`/`drive_retaliate` Retaliate tail returns `Done`. |
| `crates/game-core/src/engine/dispatch/choice.rs` | `resume_effect_walk`: return `Done` instead of re-entering `advance`/`advance_resolution`. |
| `crates/game-core/src/engine/dispatch/combat.rs` | `drive_retaliate` Retaliate-source resume (`:1170`) returns `Done`. |
| `crates/game-core/src/engine/dispatch/encounter.rs` | `resolve_encounter_card` synchronous disposal collapses to "push frame, return to loop"; expose the disposal body for the loop's `EncounterCard` arm. |
| `crates/game-core/src/state/game_state.rs` | Delete `top_reaction_window_index`; reduce/retire `top_reaction_window` / `top_reaction_window_mut` (keep `top_window`). |
| `crates/game-core/tests/reaction_windows.rs` | Rewrite the synthetic gate-above-reaction test. |

---

### Task 1: Convert the loop to drive every frame (atomic)

**Files:** all of the above.

**Interfaces:**
- Consumes: `Continuation::awaits_input()` (`game_state.rs:756`),
  `pending_candidates()` (`:784`), `advance_resolution(cx, idx)`
  (`reaction_windows.rs:727`), `skill_test::advance(cx)`,
  `teardown_encounter_card_if_top`'s disposal body (`encounter.rs:853`).
- Produces: `drive` dispatches `last()` for all frame kinds; no `top_reaction_window*`
  reach-down remains; the five re-entry sites and the chokepoint are gone.

- [ ] **Step 1: Characterization baseline.**

Run the suites that pin the behaviour this refactor must preserve, and confirm green:

Run: `cargo test -p game-core --test reaction_windows --test forced_triggers && cargo test -p cards --test revelation_treacheries`
Expected: PASS. The load-bearing characterizations: `multiple_pending_triggers_resolve_one_at_a_time` (window re-prompt/close), the Frozen-in-Fear forced-run-beneath-skill-test path in `forced_triggers`, and Crypt Chill / Grasping Hands (encounter disposal seam). These are the golden masters — do **not** add new ones; they already cover the paths.

- [ ] **Step 2: Add the `drive`-loop arms.**

In `crates/game-core/src/engine/dispatch/mod.rs`, replace `drive`'s `_ => return
EngineOutcome::Done` catch-all with the four arms + the idle fall-through. A
`TimingPointWindow` is always dispatched (empty ⇒ close); a `FastWindow` only when it
has candidates (empty ⇒ idle, the permissive gate awaiting `Skip`):

```rust
            // A window with candidates on top: advance one resume step — re-prompt
            // the next candidate, or (empty) close + run its continuation. A
            // `TimingPointWindow` is always dispatched (its candidates are exhausted
            // only by firing, so empty ⇒ close); an empty `FastWindow` is a permissive
            // Fast-gate awaiting `Skip` and falls through to idle below.
            Some(Continuation::TimingPointWindow { .. }) => {
                let idx = cx.state.continuations.len() - 1;
                match reaction_windows::advance_resolution(cx, idx) {
                    EngineOutcome::Done => {} // closed; loop on to the exposed frame
                    other => return other, // re-prompt, or a suspended continuation
                }
            }
            Some(Continuation::FastWindow { .. })
                if cx
                    .state
                    .continuations
                    .last()
                    .is_some_and(crate::state::Continuation::awaits_input) =>
            {
                let idx = cx.state.continuations.len() - 1;
                match reaction_windows::advance_resolution(cx, idx) {
                    EngineOutcome::Done => {}
                    other => return other,
                }
            }
            // A skill test re-exposed on top (a mid-test window/effect closed): step
            // its driver. No `rposition`/`win_idx > st` — it is top, by the invariant.
            Some(Continuation::SkillTest(_)) => match skill_test::advance(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
            // An encounter-treachery frame re-exposed after its Revelation's
            // sub-resolution completed: dispose of the card + pop (replaces the
            // `resolve_input` chokepoint).
            Some(Continuation::EncounterCard { .. }) => {
                encounter::teardown_encounter_card_if_top(cx);
            }
            // Idle: the open turn, an empty `FastWindow` permissive gate, the empty
            // stack, or a suspension already surfaced as AwaitingInput.
            _ => return EngineOutcome::Done,
```

(`teardown_encounter_card_if_top` already returns `Done` and pops; calling it from the
loop is the chokepoint relocated. Confirm `encounter` + `skill_test` are in scope in
`mod.rs` — both already are.)

- [ ] **Step 3: `resolve_input` — delete the chokepoint; handlers already return through `drive`.**

`apply_player_action` already calls `drive(cx, outcome)` after `resolve_input`
(`mod.rs:146`), so once the handlers step-and-return (Steps 4-7) the loop takes over.
Delete the tail chokepoint in `resolve_input` (`mod.rs:484-486`):

```rust
    // (delete) if matches!(outcome, EngineOutcome::Done) {
    //     return encounter::teardown_encounter_card_if_top(cx);
    // }
    outcome
```

The `EncounterCard` arm in `drive` (Step 2) now disposes the card when it is re-exposed.

- [ ] **Step 4: `fire_pending_trigger` / `play_fast_event` step-and-return.**

In `reaction_windows.rs`, change `fire_pending_trigger`'s `EngineOutcome::Done` arm
(after the `bump_usage_counter` call) to return `EngineOutcome::Done` instead of
`advance_resolution(cx, window_idx)`. Same for `play_fast_event`'s `Done` arm (after
`flush_pending_played_event`). `play_fast_event`'s `window_idx` param becomes unused —
drop it and update the one call site in `fire_pending_trigger`. (Identical to the
reverted C-i edits; re-apply them.)

- [ ] **Step 5: `close_reaction_window_at` — delete the skill-test seam; `run_fast_continuation` returns `Done`.**

In `close_reaction_window_at` (`reaction_windows.rs:823`), **delete** the trailing
skill-test re-entry block (`reaction_windows.rs:872-876`):

```rust
    // (delete) if let Some(in_flight) = cx.state.current_skill_test() {
    //     if !matches!(in_flight.continuation, SkillTestStep::AwaitingCommit) {
    //         return super::skill_test::advance(cx);
    //     }
    // }
    EngineOutcome::Done
```

After running the window's non-driver continuation, return `Done` — the loop
dispatches the now-top `SkillTest` (Step 2's arm).

In `run_fast_continuation` (`reaction_windows.rs:922`), both arms return `Done` so the
loop dispatches the exposed frame:

```rust
    match kind {
        // The `*Phase` anchor beneath is a drive-loop frame; the skill-test driver is
        // too (Step 2). Return Done; the loop dispatches whichever is now top.
        FastWindowKind::Phase(_) | FastWindowKind::SkillTest { .. } => EngineOutcome::Done,
    }
```

**Auto-skip caveat (must verify, not defer):** `open_fast_window`'s auto-skip path
(`reaction_windows.rs:1058-1063`) pops the window and calls `run_fast_continuation`
inline. With the `Phase` path now returning `Done`, the phase anchor must be
re-dispatched by the loop — confirm `anchor_on_child_pop`'s resume cursor is advanced
**before** the gate opens (trace the `open_fast_window` callers in `phases.rs`:
`InvestigationBegins` :345, `InvestigatorTurnBegins` :382, the generic step :600,
`UpkeepBegins` :957, `MythosAfterDraws` :486). If any opens the gate *before*
advancing its cursor, have that site's auto-skip return `Done` to the loop instead of
running the continuation inline. The `the_gathering*` playthrough + phase unit tests
are the gate.

- [ ] **Step 6: Flip the remaining re-entry sites.**

- `resume_before_discover_window` (`reaction_windows.rs:947`): return `Done` (the loop
  dispatches the in-flight `SkillTest`) instead of `super::skill_test::advance(cx)`.
- `resume_effect_walk` (`choice.rs:114`): on the in-flight-skill-test / top-window
  branch, return `Done` instead of calling `skill_test::advance` /
  `advance_resolution` — the loop dispatches the exposed `SkillTest` / window.
- `drive_retaliate`'s Retaliate-source resume (`combat.rs:1170`,
  `EnemyAttackSource::Retaliate => super::skill_test::advance(cx)`): return `Done`.
- The commit hop: `resume_skill_test_commit` → `finish_skill_test`'s teardown tail
  that re-drives a forced-run sibling via `advance_resolution` now relies on the loop
  — confirm the forced run is re-exposed as top and dispatched by Step 2's
  `TimingPointWindow` arm rather than re-entered. Pin the exact set during execution
  (`mod.rs:354/430`).

- [ ] **Step 7: Delete the reach-down accessors + `advance` self-location.**

- In `skill_test::advance` (`skill_test.rs:445-454`): delete the `rposition(SkillTest)`
  + `if let Some(win_idx) = top_reaction_window_index() { if win_idx > st { ...
  open_queued_reaction_window } }` block. (A queued window above the test is now opened
  by the loop dispatching it, not by `advance` reaching up.) Verify the
  treachery-Revelation `EncounterCard` disposal still runs (the disposal moved to the
  loop arm in Step 2).
- In `game_state.rs`: delete `top_reaction_window_index` (`:1640`); delete/retire
  `top_reaction_window` (`:1502`) and `top_reaction_window_mut` (`:1512`) once Steps
  4-6 stop using them. **Keep** `top_window` (`:1618`) and `windows()`/`open_windows()`
  (the `permissive_window` gate + test reads).

- [ ] **Step 8: Rewrite the one expected test delta.**

`close_reaction_window_at_removes_reaction_window_not_empty_phase_gate_on_top`
(`reaction_windows.rs:941`) hand-`push`es an empty phase gate **above** a pending
reaction window — a stack the invariant forbids (a pending mandatory window gates the
framework from advancing, so a gate never opens above it). Replace it with a test that
asserts the invariant rather than the old reach-down close: drive to the reaction
window, assert it is `last()` and `awaits_input()`, resolve it, and assert it closes
and control proceeds — i.e. a pending window is never stranded beneath a permissive
gate. If, after writing it, the test asserts nothing the existing
`multiple_pending_triggers` / `skip_*` tests don't already cover, delete it instead
(it was defending a now-impossible shape).

- [ ] **Step 9: Verify — full suite green.**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS, including `reaction_windows`, `forced_triggers`,
`cards/revelation_treacheries`, and `scenarios/the_gathering*`. Investigate any
failure as a real regression (the conversion is behaviour-preserving) — do not paper
over it by re-introducing a reach-down.

- [ ] **Step 10: No-reach-down + gauntlet.**

```bash
grep -rn 'top_reaction_window_index\|win_idx > st\|top_reaction_window\b' crates/game-core/src/engine/   # expect: no matches
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: the grep is empty; all jobs clean. Fix any stale doc-comment referencing the
deleted accessors / seams (the `doc` job + the `close_reaction_window_at` /
`advance` / `top_reaction_window_index` doc blocks).

- [ ] **Step 11: Commit.**

```bash
git add -A
git commit -m "engine: drive every frame by top-frame dispatch (Slice C-plumbing)

The drive loop gains arms for TimingPointWindow / FastWindow / SkillTest /
EncounterCard, dispatched off continuations.last(); every driver returns Done
to the loop instead of reaching down the stack. Deletes the reach-down
accessors (top_reaction_window_index, advance's win_idx>st), the five
synchronous skill-test re-entry sites, and the resolve_input encounter-disposal
chokepoint. Behaviour-preserving (one synthetic gate-above-reaction test
rewritten — it encoded a stack shape the resolution-order invariant forbids).
Unblocks #423.

Part of #431.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## Self-review

**Spec coverage.** Loop arms for all four frame kinds — Step 2. `resolve_input`
through `drive` + chokepoint deletion — Step 3. Step-and-return drivers — Steps 4-6.
Reach-down accessor + `advance` self-location deletion — Step 7. Invariant + the
synthetic-test rewrite — Steps 1/8. The `open_fast_window` auto-skip caveat — Step 5.
Out-of-scope (combat re-entry, `top_window`) — scope note.

**Placeholder scan.** The loop-arm code (Step 2) and the deletions (Steps 3/5/7) are
literal. The driver flips (Steps 4/6) reference exact functions + line numbers and the
precise change ("return `Done` instead of calling X"); the commit-hop exact set is
explicitly pinned-during-execution because `finish_skill_test`'s teardown tail must be
traced against live code — this is the one genuinely-needs-the-code spot, flagged
rather than fabricated.

**Type consistency.** `advance_resolution(cx, idx)` matches its signature
(`reaction_windows.rs:727`); `awaits_input` / `pending_candidates` are the exact
`Continuation` methods; arms match the exact variant names
(`TimingPointWindow`/`FastWindow`/`SkillTest`/`EncounterCard`).

## Risks

| Risk | Mitigation |
|---|---|
| Half-conversion: a driver still reaches down | Step 10's grep gates it; `forced_triggers` (Frozen-in-Fear) is the reentrancy backstop |
| `TimingPointWindow` empty-close vs `FastWindow` empty-idle handled wrong | Step 2 dispatches `TimingPointWindow` unconditionally (empty ⇒ close) but `FastWindow` only when `awaits_input()`; `skip_*` + `the_gathering*` tests guard both |
| `open_fast_window` auto-skip stalls a phase transition | Step 5 caveat: verify/repair the anchor cursor before the gate opens; phase suites + playthrough gate it |
| Encounter card stranded after Revelation suspends | `EncounterCard` loop arm = chokepoint body relocated 1:1; `revelation_treacheries` backstop |
