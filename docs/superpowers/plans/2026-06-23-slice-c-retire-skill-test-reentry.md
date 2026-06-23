# Slice C tail: retire the skill-test commit-hop + substitution-resume re-entry (#431)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retire the two remaining synchronous skill-test re-entry sites (`finish_skill_test`'s commit hop and `resume_substitution_choice`) so skill-test resumption flows through the uniform `drive` loop's `SkillTest` arm — completing Slice C (#431).

**Architecture:** Both functions are entered only from `resolve_input` (`mod.rs:427`, `mod.rs:461`), and `apply_player_action` runs `drive(cx, outcome)` immediately after `resolve_input` returns (`mod.rs:147`). Today each ends with a synchronous `advance(cx)` that drives the parked `SkillTest` frame to its next suspension/teardown. We replace that tail with `EngineOutcome::Done`, leaving the `SkillTest` frame on top with its cursor pre-advanced; the loop's `SkillTest` arm (`mod.rs:233 → skill_test::advance(cx)`) then drives it. Behavior-preserving at the `apply` boundary — only the *direct-call unit tests* that bypass `drive` observe the new park-don't-drive contract and must `drive` explicitly.

**Tech Stack:** Rust, the `game-core` engine crate. No new dependencies.

## Global Constraints

- Behavior-preserving at the `apply` / `resolve_input` boundary: the integration suite (`crates/cards/tests/{commit_cap,deduction,mind_over_matter,retaliate_windows}.rs`) and `crates/cards/tests/revelation_treacheries.rs` MUST stay green untouched — they are the behavior-preservation net.
- Match CI's strict flags before declaring done: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- The `drive` loop's `SkillTest` arm already exists (`crates/game-core/src/engine/dispatch/mod.rs:233`). Do **not** add new loop arms — this slice only retires the imperative re-entry into the existing arm.
- Do **not** touch `run_fast_continuation`'s `FastWindowKind::SkillTest => skill_test::advance(cx)` (`reaction_windows.rs:1072`) — it is the window's *own* inline continuation (incl. the open-time auto-skip), deliberately imperative, and is **not** a driver-to-driver reach-down (documented at `reaction_windows.rs:1063-1069`).
- The test module refers to the loop entry point as `super::super::drive` (parent `dispatch` module; mirrors the existing `super::super::reaction_windows::…` / `super::super::evaluator::…` references). `SkillTestStep` is matched with `matches!`, not `==`.

---

## File structure

| File | Responsibility | Change |
|---|---|---|
| `crates/game-core/src/engine/dispatch/skill_test.rs` | the skill-test driver + its `#[cfg(test)]` module | Modify `finish_skill_test` (Task 1) and `resume_substitution_choice` (Task 2) tails + their doc-comments; add two contract tests; add `drive` calls to six existing direct-call tests |
| `docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md` | the arc decomposition spec (issue-map + narrative) | Mark #431's Slice-C re-entry retirement done (Task 3) |

No production files other than `skill_test.rs` change. No new files.

---

### Task 1: Retire the commit hop (`finish_skill_test`)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:239-248` (the tail of `finish_skill_test`)
- Test (new + modified): `crates/game-core/src/engine/dispatch/skill_test.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `start_skill_test`, `finish_skill_test(cx, indices) -> EngineOutcome`, `super::super::drive(cx, outcome) -> EngineOutcome`, `Cx`, `EngineOutcome`, `Continuation::SkillTest`, `SkillTestStep::PreTokenWindow`, `Event::SkillTestEnded`.
- Produces: `finish_skill_test` now returns `EngineOutcome::Done` with the `SkillTest` frame parked on top at cursor `SkillTestStep::PreTokenWindow` (instead of driving to teardown). No signature change.

- [ ] **Step 1: Write the failing contract test**

Add this test inside the `#[cfg(test)] mod tests` block in `skill_test.rs` (e.g. immediately after `commit_emits_then_resolves_through_advance`, which ends near line 1516):

```rust
/// The commit hop parks the resolution for the loop rather than driving it
/// itself: `finish_skill_test` returns `Done` with the `SkillTest` frame on
/// top at `PreTokenWindow` and emits no `SkillTestEnded`; the `drive` loop's
/// `SkillTest` arm then resolves it to teardown. (Slice C, #431 — commit-hop
/// re-entry retired.)
#[test]
fn finish_skill_test_parks_the_resolution_for_the_loop() {
    use crate::state::{ChaosToken, Continuation, SkillTestStep};

    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    // Park at AwaitingCommit (the commit prompt).
    let out = start_skill_test(
        &mut cx,
        inv,
        SkillKind::Willpower,
        SkillTestKind::Plain,
        2,
        SkillTestFollowUp::None,
        None,
        None,
        None,
        0,
    );
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));

    // Commit nothing: the hop PARKS — it must not itself resolve the test.
    let out = finish_skill_test(&mut cx, &[]);
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::SkillTest(t)) if matches!(t.continuation, SkillTestStep::PreTokenWindow)
        ),
        "the commit hop parks the SkillTest at PreTokenWindow for the loop to drive",
    );
    assert!(
        !events.iter().any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "the hop itself does not resolve the test to teardown: {events:?}",
    );

    // The loop's SkillTest arm drives the parked frame the rest of the way.
    let out = super::super::drive(&mut cx, out);
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        events.iter().any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "the loop resolved the test to teardown: {events:?}",
    );
    assert!(
        !cx.state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
        "the SkillTest frame was torn down by the loop",
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p game-core finish_skill_test_parks_the_resolution_for_the_loop`
Expected: FAIL — today `finish_skill_test` calls `advance(cx)`, which drives to teardown, so immediately after the call the `SkillTest` frame is already gone and `SkillTestEnded` is already emitted. The `matches!(… Some(Continuation::SkillTest(t)) …)` assertion fails (frame absent), and/or the "no `SkillTestEnded` yet" assertion fails.

- [ ] **Step 3: Make the minimal production change**

In `finish_skill_test`, replace the synchronous drive tail (currently `skill_test.rs:239-248`):

```rust
    // Persist the committed indices and advance to `Resolving`; the driver
    // runs the resolution body from there (its loop snapshot reads
    // `committed_by_active`).
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above");
    t.committed_by_active = indices_u8;
    t.continuation = SkillTestStep::PreTokenWindow;
    advance(cx)
}
```

with:

```rust
    // Persist the committed indices and pre-advance the cursor to
    // `PreTokenWindow`, then park: return `Done` so the `drive` loop's
    // `SkillTest` arm (dispatch/mod.rs) runs the resolution body from there.
    // The frame stays on top and `resolve_input`'s caller drives it
    // (apply_player_action runs `drive` after this returns). Slice C, #431 —
    // the commit-hop `advance` reach-down is retired.
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above");
    t.committed_by_active = indices_u8;
    t.continuation = SkillTestStep::PreTokenWindow;
    EngineOutcome::Done
}
```

- [ ] **Step 4: Run the new test to verify it passes**

Run: `cargo test -p game-core finish_skill_test_parks_the_resolution_for_the_loop`
Expected: PASS.

- [ ] **Step 5: Update the four existing direct-call unit tests to drive explicitly**

These call `finish_skill_test` directly and assert on the *driven* end-state (events / horror / teardown), so they must now `drive` after the hop. In each, insert a `drive` line immediately after the `let out = finish_skill_test(&mut cx, &[]);` line:

```rust
    let out = finish_skill_test(&mut cx, &[]);
    let out = super::super::drive(&mut cx, out);
```

Apply to all four call sites (current line numbers; the `cx`/`out` bindings are identical in each):
- `plain_skill_test_disposes_of_no_encounter_card` — `skill_test.rs:1305`
- `skill_test_runs_on_success_effect_on_a_passing_draw` — `skill_test.rs:1342`
- `skill_test_opens_and_auto_skips_both_player_windows` — `skill_test.rs:1387`
- `commit_emits_then_resolves_through_advance` — `skill_test.rs:1498`

Leave every following assertion in those tests unchanged — driving restores the exact prior end-state. Do **not** touch `closing_a_skill_test_player_window_re_enters_advance` (it exercises `run_fast_continuation`, unchanged) or any `start_skill_test`-only test.

- [ ] **Step 6: Run the whole skill_test module to verify all pass**

Run: `cargo test -p game-core --lib engine::dispatch::skill_test`
Expected: PASS (the new test + the four edited tests + all others green).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: retire the skill-test commit-hop re-entry (Slice C, #431)

finish_skill_test parked at PreTokenWindow and called advance(cx) directly;
return Done instead so the drive loop's SkillTest arm resolves it. Behaviour-
preserving at the apply boundary (resolve_input is always followed by drive);
the four direct-call unit tests now drive() explicitly to reach the same
end-state.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

### Task 2: Retire the substitution resume (`resume_substitution_choice`)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:194-198` (the tail of `resume_substitution_choice`)
- Test (new + modified): `crates/game-core/src/engine/dispatch/skill_test.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `substitution_state(inv) -> GameState` (existing test helper), `resume_substitution_choice(cx, response) -> EngineOutcome`, `super::super::drive`, `InputResponse::PickSingle`, `OptionId`, `Continuation::{SkillTest, SubstitutionPrompt}`.
- Produces: `resume_substitution_choice` now returns `EngineOutcome::Done` with the `SubstitutionPrompt` popped and the `SkillTest` frame on top at its pre-commit cursor (instead of driving to the commit window). No signature change.

- [ ] **Step 1: Write the failing contract test**

Add this test inside `#[cfg(test)] mod tests` (e.g. immediately after `substitution_choice_no_keeps_the_printed_skill`, near line 1669):

```rust
/// The substitution resume parks the test for the loop rather than driving to
/// the commit window itself: choosing the substitution pops the
/// `SubstitutionPrompt`, rewrites the skill, and returns `Done` with the
/// `SkillTest` on top; the `drive` loop then opens the commit window. (Slice C,
/// #431 — substitution-resume re-entry retired.)
#[test]
fn resume_substitution_choice_parks_for_the_loop() {
    use crate::state::Continuation;

    let inv = InvestigatorId(1);
    let mut state = substitution_state(inv);
    let mut events = Vec::new();
    let out = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        start_skill_test(
            &mut cx,
            inv,
            SkillKind::Combat,
            SkillTestKind::Fight,
            3,
            SkillTestFollowUp::None,
            None,
            None,
            None,
            2,
        )
    };
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "substitution prompt");

    // Choose the substitution: the resume PARKS — it does not itself open the
    // commit window.
    let out = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(0)))
    };
    assert_eq!(out, EngineOutcome::Done, "the substitution resume parks for the loop");
    assert!(
        !matches!(
            state.continuations.last(),
            Some(Continuation::SubstitutionPrompt { .. })
        ),
        "the SubstitutionPrompt was consumed",
    );
    assert!(
        matches!(state.continuations.last(), Some(Continuation::SkillTest(_))),
        "the SkillTest frame is parked on top for the loop to drive",
    );
    assert_eq!(
        state.current_skill_test().unwrap().skill,
        SkillKind::Intellect,
        "the substitution rewrote the skill before parking",
    );

    // The loop drives the parked test to its commit window.
    let out = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        super::super::drive(&mut cx, out)
    };
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "commit window");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p game-core resume_substitution_choice_parks_for_the_loop`
Expected: FAIL — today `resume_substitution_choice` ends with `advance(cx)`, which drives straight to the commit window, so it returns `AwaitingInput`; `assert_eq!(out, EngineOutcome::Done)` fails.

- [ ] **Step 3: Make the minimal production change**

In `resume_substitution_choice`, replace the synchronous drive tail (currently `skill_test.rs:194-198`):

```rust
    // Drive the test from `PreCommitWindow`: `advance` opens the ST.1 player
    // window (#374) first, then — on auto-skip — parks at `AwaitingCommit` and
    // emits the commit prompt (reading the now-possibly-rewritten skill). With a
    // Fast play available it parks at the window instead.
    advance(cx)
}
```

with:

```rust
    // Park: return `Done` so the `drive` loop's `SkillTest` arm drives the test
    // from its pre-commit cursor — opening the ST.1 player window (#374), then
    // (on auto-skip) the commit prompt reading the now-possibly-rewritten skill,
    // or parking at the window if a Fast play is available. The frame is on top
    // and `resolve_input`'s caller drives it. Slice C, #431 — the
    // substitution-resume `advance` reach-down is retired.
    EngineOutcome::Done
}
```

- [ ] **Step 4: Run the new test to verify it passes**

Run: `cargo test -p game-core resume_substitution_choice_parks_for_the_loop`
Expected: PASS.

- [ ] **Step 5: Update the two existing direct-call unit tests to drive explicitly**

`combat_test_with_substitution_prompts_then_becomes_intellect_on_yes` (`skill_test.rs:1567`) and `substitution_choice_no_keeps_the_printed_skill` (`skill_test.rs:1658`) call `resume_substitution_choice` in a `cx` block and assert the result is the `AwaitingInput` commit window. Each must now `drive` the parked frame to reach that window. In both, the block currently reads:

```rust
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(0)))
        };
```

(the second test uses `OptionId(1)`). Append a driving block immediately after each, before the existing `assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "commit window")`:

```rust
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            super::super::drive(&mut cx, out)
        };
```

Leave every following assertion unchanged — the state assertions (`t.skill == Intellect`, `test_modifier == 0`, `SubstitutionPrompt` gone; or `skill == Agility`) are set before the parked cursor and survive the drive, and `out` is once again the commit-window `AwaitingInput`.

- [ ] **Step 6: Run the whole skill_test module to verify all pass**

Run: `cargo test -p game-core --lib engine::dispatch::skill_test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: retire the skill-test substitution-resume re-entry (Slice C, #431)

resume_substitution_choice rewrote the skill then called advance(cx); return
Done instead so the drive loop's SkillTest arm opens the commit window.
Behaviour-preserving at the apply boundary; the two direct-call unit tests now
drive() explicitly.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

### Task 3: Verify the full gauntlet + record Slice C complete in the arc spec

**Files:**
- Modify: `docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md:239` (issue-map row for #431)

**Interfaces:**
- Consumes: nothing (verification + docs).
- Produces: the arc spec reflects that Slice C's re-entry retirement is done.

- [ ] **Step 1: Run the behavior-preservation net (integration tests) — no edits expected**

Run:
```bash
cargo test -p cards --test commit_cap
cargo test -p cards --test deduction
cargo test -p cards --test mind_over_matter
cargo test -p cards --test retaliate_windows
cargo test -p cards --test revelation_treacheries
```
Expected: PASS, with **no** source edits to any `crates/cards/tests/*` file — these go through real `apply`/`drive` and prove the change is behavior-preserving at the boundary.

- [ ] **Step 2: Run the full CI gauntlet locally (strict flags)**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all green. If `fmt --check` flags anything, run `cargo fmt` and fold it into the relevant task's commit (do not create a stray formatting commit).

- [ ] **Step 3: Update the arc spec issue-map row**

In `docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md`, change the #431 issue-map row (line ~239) from:

```markdown
| C | [#431](https://github.com/talelburg/eldritch/issues/431) | open, keep as-is |
```

to:

```markdown
| C | [#431](https://github.com/talelburg/eldritch/issues/431) | ✅ done — re-entry retirement complete (commit hop + substitution resume); A-iv window arms + encounter disposal landed in C-plumbing |
```

- [ ] **Step 4: Commit the doc update**

```bash
git add docs/superpowers/specs/2026-06-22-emitevent-frame-arc-decomposition-design.md
git commit -m "docs: mark Slice C re-entry retirement complete (#431)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

- [ ] **Step 5: Open the PR**

Branch `engine/retire-skill-test-reentry`, push, and `gh pr create` with the repo template. Body should note: completes Slice C (#431); the only remaining genuine re-entry sites (commit hop + substitution resume) now park for the loop; behavior-preserving (integration net untouched); the intentionally-imperative `run_fast_continuation` path is left as-is. Include `Closes #431.`

---

## Self-review notes

- **Spec coverage:** #431's acceptance has three boxes — encounter-card disposal loop-driven (already done by C-plumbing), the five re-entry sites retired (four done by C-plumbing; commit hop = Task 1, substitution resume = Task 2 — the scope chosen with the user, broader than the spec's literal five), and `revelation_treacheries` + suite green (Task 3). All covered.
- **Out of scope (intentional):** `run_fast_continuation`'s `FastWindowKind::SkillTest` inline `advance` (documented as deliberately imperative); the fresh-action bounded entries `start_skill_test`/`perform_skill_test` (entered from `apply`, not resumptions — they legitimately drive); Slice D (#423, the `apply_effect` bounded-entry migration).
- **Type consistency:** `finish_skill_test(cx, &[u32])`, `resume_substitution_choice(cx, &InputResponse)`, `super::super::drive(cx, EngineOutcome)` all match existing signatures; `SkillTestStep` matched via `matches!` (no `PartialEq` assumed); `Event::SkillTestEnded`, `Continuation::{SkillTest, SubstitutionPrompt}` are the names used by neighboring tests.
