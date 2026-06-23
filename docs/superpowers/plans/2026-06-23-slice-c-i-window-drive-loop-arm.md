# Slice C-i — Window `drive`-loop arm — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `drive` loop re-dispatch an open `TimingPointWindow` / `FastWindow`
frame after a candidate resolves (re-prompt the next candidate, or close + run the
window's continuation), instead of the window-resume handlers running that
advance/close cascade synchronously in place.

**Architecture:** Behaviour-preserving structural refactor. Today
`fire_pending_trigger` / `play_fast_event` call `advance_resolution` on their
synchronous-completion tail, draining the window before control returns to `drive`.
This task flips those tails to return `Done`, leaving the window frame on top, and
adds a `drive`-loop arm that calls `advance_resolution` on the top frame. Net
behaviour is identical (`advance_resolution` is invoked exactly once, just from the
loop); the point is that the window frame becomes a loop-dispatched frame — the
prerequisite that lets Slice C-ii intercept the window-close → skill-test seam at the
loop boundary.

**Tech Stack:** Rust, `cargo test`/`clippy`/`fmt`/`doc`, the `game-core` engine crate.

## Global Constraints

- **Behaviour-preserving — event log byte-identical.** No game-rule or event-output
  change. The existing engine + integration suites are the characterization backstop.
- **CI gauntlet, warnings-as-errors.** Before any push, match all flags from
  `CLAUDE.md` Commands: `RUSTFLAGS="-D warnings" cargo test --all --all-features`,
  `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`,
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, plus
  the wasm build/clippy. Plain `cargo test` is not sufficient.
- **Validate-first / mutate-second** handler contract is unchanged here — this task
  touches only the post-resolution control-flow tail, not validation.
- **Commit scope/subject convention:** `engine: <description>`; commit body explains
  the *why* and ends with `Closes #431` is **NOT** used (the issue closes at the end
  of Slice C, not C-i) — reference it with `Part of #431.` instead.

---

## Scope note (read before starting)

This plan covers **only Slice C-i** of the four-PR Slice C arc (spec:
`docs/superpowers/specs/2026-06-23-emitevent-frame-slice-c-loop-driving-design.md`).
C-ii / C-iii / D each gate on the prior landing and get their own plan.

**Deliberately NOT in C-i** (corrected during planning — see the spec's C-i section):

- **The skill-test seam stays imperative.** `close_reaction_window_at`'s
  `current_skill_test()` → `skill_test::advance` re-entry (reaction_windows.rs:872-876)
  is untouched. C-ii retires it.
- **`run_fast_continuation`'s Phase path stays inline.** It still calls
  `anchor_on_child_pop` synchronously (reaction_windows.rs:924). Deferring it to the
  loop is **not** safe in isolation: `open_fast_window`'s auto-skip path
  (reaction_windows.rs:1058-1063) pops the window and runs `run_fast_continuation`
  inline, relying on the Phase continuation to advance the phase-anchor cursor in the
  same call. Returning `Done` there would stall the auto-skip transition unless the
  anchor's `resume` cursor is already advanced — which needs `anchor_on_child_pop`
  cursor handling that rides C-ii. So this moves to C-ii.
- **The `EncounterCard` disposal chokepoint** (`resolve_input` tail, mod.rs:484) is
  untouched — C-ii.

## File structure

| File | Change |
|---|---|
| `crates/game-core/src/engine/dispatch/mod.rs` | Add the `TimingPointWindow` / `FastWindow` arm to `drive` (after the `Effect` arm, before `_ => return Done`). |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | Flip `fire_pending_trigger`'s and `play_fast_event`'s synchronous-`Done` tails to return `Done`; drop `play_fast_event`'s now-unused `window_idx` param + update its one call site. |
| `crates/game-core/tests/reaction_windows.rs` (+ `forced_triggers.rs`) | Characterization baseline (existing tests); add one golden-master `assert_event_sequence!` if a two-candidate path is uncovered. |

---

### Task 1: Drive the window frame's re-prompt/close from the loop

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`drive`, around line 199-211)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:631-637` (`fire_pending_trigger` tail), `:653-657` (`play_fast_event` signature), `:705-718` (`play_fast_event` tail), `:543` (the call site)
- Test: `crates/game-core/tests/reaction_windows.rs`, `crates/game-core/tests/forced_triggers.rs`

**Interfaces:**
- Consumes: `advance_resolution(cx: &mut Cx, window_idx: usize) -> EngineOutcome`
  (reaction_windows.rs:727) — re-prompts if candidates remain, else closes via
  `close_reaction_window_at`. Already `pub(super)`. Unchanged by this task.
- Consumes: `Continuation::TimingPointWindow` / `Continuation::FastWindow` (the two
  window frame variants). The top frame, when one of these, is the window to advance
  (the loop always dispatches the top frame; no `top_reaction_window_index` skipping
  needed because there is nothing above it).
- Produces: no new public surface; `play_fast_event` loses its `window_idx`
  parameter.

---

- [ ] **Step 1: Establish the characterization baseline (golden master).**

Identify the existing tests that exercise (a) a multi-candidate reaction window
(two reactions, or one reaction + an Axis-C hand Fast event like Evidence! 01022 on
after-defeat), (b) a 2+ forced run (the `#213` lead-ordering path), and (c) a
framework Fast window resolution. These live in
`crates/game-core/tests/reaction_windows.rs`, `crates/game-core/tests/forced_triggers.rs`,
and the phase tests. Run them and confirm green — this is the behaviour the refactor
must preserve:

Run: `cargo test -p game-core --test reaction_windows && cargo test -p game-core --test forced_triggers`
Expected: PASS (baseline).

If no existing test asserts the *full event sequence* for a two-candidate window
(fire one, get re-prompted, fire the second, window closes), add one golden-master
test now so the relocation is locked. Use the `TestGame` builder + `assert_event_sequence!`
(order-sensitive subsequence), modeled on the existing reaction-window tests in that
file. It must PASS before the change (it characterizes current behaviour):

```rust
// Golden master: a window with two candidates re-prompts after the first
// fires, then closes after the second — the event order must survive the
// move of `advance_resolution` into the drive loop.
#[test]
fn two_candidate_reaction_window_resolves_in_order_then_closes() {
    // ... build a state whose emit opens a reaction window with two
    // candidates (mirror the existing multi-candidate setup in this file),
    // submit PickSingle for each, and assert the event subsequence across
    // both fires is unchanged. (Reuse the nearest existing fixture; do not
    // invent new card content.)
}
```

Run: `cargo test -p game-core --test reaction_windows two_candidate_reaction_window_resolves_in_order_then_closes`
Expected: PASS (characterizes current behaviour before the refactor).

- [ ] **Step 2: Add the `drive`-loop window arm.**

In `crates/game-core/src/engine/dispatch/mod.rs`, inside `drive`'s `loop`, add an arm
**after** the `Continuation::Effect(_)` arm and **before** the `_ => return
EngineOutcome::Done` catch-all:

```rust
            // A window frame parked on top after a candidate's effect completed
            // (Slice C-i): re-dispatch it — re-prompt the next candidate, or close
            // it and run its continuation. `advance_resolution` operates on the top
            // window (nothing sits above it in the loop), unifying the former
            // `fire_pending_trigger` / `play_fast_event` synchronous tail with the
            // loop. The window-close → skill-test seam inside `close_reaction_window_at`
            // stays imperative until Slice C-ii.
            Some(Continuation::TimingPointWindow { .. } | Continuation::FastWindow { .. }) => {
                let idx = cx.state.continuations.len() - 1;
                match reaction_windows::advance_resolution(cx, idx) {
                    EngineOutcome::Done => {
                        // Window closed + its continuation ran to Done; loop on to
                        // whatever frame the close exposed.
                    }
                    other => return other, // re-prompt (candidates remain) or a suspended continuation
                }
            }
```

Confirm `reaction_windows` is in scope in `mod.rs` (it is — `resume_window` already
calls `reaction_windows::resume_reaction_window`).

- [ ] **Step 3: Flip `fire_pending_trigger`'s synchronous-`Done` tail.**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, change the
`EngineOutcome::Done` arm of `fire_pending_trigger` (currently lines 631-636):

```rust
        EngineOutcome::Done => {
            if usage_limit.is_some() {
                bump_usage_counter(cx.state, &trigger);
            }
            advance_resolution(cx, window_idx)
        }
```

to:

```rust
        EngineOutcome::Done => {
            if usage_limit.is_some() {
                bump_usage_counter(cx.state, &trigger);
            }
            // The window frame stays on top with its remaining candidates; the
            // `drive` loop's window arm re-dispatches it (re-prompt or close).
            // Slice C-i.
            EngineOutcome::Done
        }
```

(The suspending `AwaitingInput` arm at line 630 is unchanged: a reaction effect that
starts a skill test still suspends with the `SkillTest` frame above this window — the
`#213` reentrancy path, untouched in C-i.)

- [ ] **Step 4: Flip `play_fast_event`'s synchronous-`Done` tail and drop the dead param.**

In the same file, change `play_fast_event`'s `EngineOutcome::Done` arm (lines 705-717):

```rust
        EngineOutcome::Done => {
            super::cards::flush_pending_played_event(cx);
            advance_resolution(cx, window_idx)
        }
```

to:

```rust
        EngineOutcome::Done => {
            super::cards::flush_pending_played_event(cx);
            // Window stays on top; the drive loop re-dispatches it. Slice C-i.
            EngineOutcome::Done
        }
```

`window_idx` is now unused in `play_fast_event` (it was only passed to
`advance_resolution`), which fails `-D warnings`. Remove the parameter from the
signature (lines 653-657):

```rust
fn play_fast_event(cx: &mut Cx, candidate: &ResolutionCandidate) -> EngineOutcome {
```

and update the single call site in `fire_pending_trigger` (line 543):

```rust
        return play_fast_event(cx, &trigger);
```

(The candidate is already removed from the run at lines 539-542 before this call —
unchanged. `fire_pending_trigger` still uses `window_idx` for that removal and the
in-play removal at line 610, so its own param stays.)

- [ ] **Step 5: Run the characterization suites — confirm byte-identical behaviour.**

Run: `cargo test -p game-core --test reaction_windows && cargo test -p game-core --test forced_triggers`
Expected: PASS — including the Step 1 golden master, unchanged.

Then the full engine + integration suites (the broad behaviour-preservation net):

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS. In particular `crates/scenarios/tests/the_gathering*.rs` (full
playthrough), `crates/cards/tests/` (Evidence! / reaction cards), and
`crates/cards/tests/revelation_treacheries.rs` stay green — the window-close seam
into skill tests still works (imperatively, per scope).

- [ ] **Step 6: Match the rest of the CI gauntlet.**

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all clean. (Clippy is the gate that catches the dropped-param dead code if
Step 4 missed a call site.)

- [ ] **Step 7: Commit.**

```bash
git add crates/game-core/src/engine/dispatch/mod.rs \
        crates/game-core/src/engine/dispatch/reaction_windows.rs \
        crates/game-core/tests/reaction_windows.rs
git commit -m "engine: drive window re-prompt/close from the loop (Slice C-i)

fire_pending_trigger / play_fast_event return Done on synchronous
completion instead of calling advance_resolution in place; the drive
loop gains a TimingPointWindow/FastWindow arm that re-dispatches the
top window frame. Behaviour-preserving (advance_resolution runs once,
relocated into the loop) — the prerequisite for Slice C-ii to intercept
the window-close → skill-test seam at the loop boundary.

Part of #431.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## Self-review

**Spec coverage.** C-i's spec deliverables: (1) `drive` arms for `TimingPointWindow`
+ `FastWindow` — Step 2; (2) window resumes step-and-return — Steps 3-4; (3) retire
`advance_resolution`'s loop role into the arm — Steps 2-4 (it is now called from the
loop, not the handler tail); (4) keep the skill-test seam — explicitly preserved
(scope note + Step 3 note). The spec also listed "retire `run_fast_continuation`'s
Phase path" under C-i; planning found that unsafe in isolation (the `open_fast_window`
auto-skip interaction) and reassigned it to C-ii — the spec's C-i section is updated
to match.

**Placeholder scan.** The only non-literal content is the Step 1 golden-master test
body, which is intentionally a *guided* addition (reuse the nearest existing fixture;
do not invent card content) because the exact fixture depends on the live test
harness — the surrounding steps and all production-code changes are complete and
literal.

**Type consistency.** `advance_resolution(cx, window_idx)` signature matches its
definition (reaction_windows.rs:727). `play_fast_event` is reduced to
`(cx, candidate)` consistently at both the definition and the single call site
(line 543). The arm matches on `Continuation::TimingPointWindow` / `FastWindow`, the
exact variant names in `game_state.rs:422/437`.

## Risks

| Risk | Mitigation |
|---|---|
| Loop spins if `advance_resolution` returns `Done` without changing the top | It only returns `Done` via `close_reaction_window_at`, which `remove`s the window frame — the top always changes. The phase-anchor no-progress guard is not needed here; if defensiveness is wanted, assert the stack length shrank. |
| A `fire_pending_trigger` caller not followed by `drive` would strand the window | The only caller is `resume_reaction_window` ← `resume_window` ← `resolve_input` ← `apply_player_action`, which always ends in `drive(outcome)` (mod.rs:146). Confirm with a grep for `fire_pending_trigger` before Step 3. |
| Forced-run reentrancy (Frozen in Fear) double-advances | Unchanged in C-i: the suspending arm still returns `AwaitingInput`; the skill-test commit-resume still drains the forced run via its own `advance_resolution` call (the kept seam). The loop arm only fires on the synchronous-`Done` tail. `forced_triggers` is the backstop. |
