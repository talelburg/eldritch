# PR 2a (#348, part 1) — Fold `in_flight_skill_test` onto its `SkillTest` frame — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Carry the in-flight skill test's data on its `Continuation::SkillTest(InFlightSkillTest)` frame instead of the parallel `GameState::in_flight_skill_test: Option<…>` field, making the continuation stack the single source of truth for "a test is in flight."

**Architecture:** Same accessor-indirection strategy that worked for #345. Task 1 adds `current_skill_test()` / `current_skill_test_mut()` / `take_skill_test()` accessors on `GameState` (reading the *existing* `Option` field) and migrates every read/write site to them — pure refactor. Task 2 changes the `SkillTest` variant to carry the payload, pushes it early (at test start, replacing the `Option` set), drops the `Option` field, and points the accessors at the frame. Task 3 verifies the Mind-over-Matter substitution path (the one place test data exists before the commit window) still round-trips.

**Tech Stack:** Rust, the `game-core` crate (this PR is entirely within `game-core`; no `cards`/`scenarios`/wire changes).

This is **PR 2a of the #348 split** (2a = this; 2b = migrate `pending_*` modes onto frames + collapse both dispatch ladders; 2c = fold `Mulligan`/`DrawEncounterCard` into `InputResponse`). Spec: `docs/superpowers/specs/2026-06-19-continuation-stack-cleanup-design.md` §A. Series order (revised): #345 ✅ → **#348 (2a/2b/2c)** → #347 (tokens, clean on frames) → #380.

## Global Constraints

- **CI gauntlet, warnings-as-errors** (run all before pushing):
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **No behavior change.** This is a representation refactor; every existing skill-test, reaction-window-mid-test, and Mind-over-Matter test must pass unchanged.
- **`InFlightSkillTest` already derives** `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` (it was a serialized field); it can move onto the `Continuation::SkillTest` variant, which is part of the serialized `continuations` Vec, with no new derives.
- **No nesting today:** at most one `SkillTest` frame is ever on the stack. Accessors find the (unique) frame; if same-kind nesting ever lands, "innermost wins" = the topmost `SkillTest` frame (consistent with #345's binding rule).
- **Branch:** new branch off fresh `main` — `engine/fold-in-flight-skill-test`. Commit per task; push only when the full gauntlet is green.

---

### Task 1: Accessor indirection over the `Option` field

Add `GameState` accessors that locate the in-flight test, and migrate every site to them — while the data still lives in the `Option` field. Pure indirection; isolates the representation swap (Task 2) to the accessor bodies + the push/pop sites.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add accessor methods on `GameState`)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (the bulk of the read/write sites)
- Modify: `crates/game-core/src/engine/mod.rs`, `crates/game-core/src/engine/dispatch/mod.rs`, and any other module that reads `in_flight_skill_test` (enumerated in Step 3)

**Interfaces:**
- Produces (methods on `GameState`):
  - `fn current_skill_test(&self) -> Option<&InFlightSkillTest>`
  - `fn current_skill_test_mut(&mut self) -> Option<&mut InFlightSkillTest>`
  - `fn take_skill_test(&mut self) -> Option<InFlightSkillTest>` — clears the in-flight test (Task 1: `self.in_flight_skill_test.take()`; Task 2: pop the `SkillTest` frame and return its payload)
  - `fn has_skill_test_in_flight(&self) -> bool` — convenience for the `.is_some()` guards

- [ ] **Step 1: Add the accessors (reading the existing `Option` field)**

In `crates/game-core/src/state/game_state.rs`, add to `impl GameState` (near the other continuation helpers like `top_reaction_window`):

```rust
/// The skill test currently in flight, if any. Single source of truth for
/// "a test is mid-resolution"; `None` outside a test. (Task 2 moves the
/// payload onto the `Continuation::SkillTest` frame; this accessor hides that.)
#[must_use]
pub fn current_skill_test(&self) -> Option<&InFlightSkillTest> {
    self.in_flight_skill_test.as_ref()
}

/// Mutable counterpart to [`Self::current_skill_test`].
pub fn current_skill_test_mut(&mut self) -> Option<&mut InFlightSkillTest> {
    self.in_flight_skill_test.as_mut()
}

/// Remove and return the in-flight skill test (called at test teardown).
pub fn take_skill_test(&mut self) -> Option<InFlightSkillTest> {
    self.in_flight_skill_test.take()
}

/// Whether a skill test is currently in flight.
#[must_use]
pub fn has_skill_test_in_flight(&self) -> bool {
    self.in_flight_skill_test.is_some()
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p game-core`
Expected: PASS (new methods unused for now).

- [ ] **Step 3: Migrate every read/write site to the accessors**

Enumerate the sites first so none is missed:

```bash
grep -rn "in_flight_skill_test" crates/game-core/src --include=*.rs | grep -v "fn current_skill_test\|fn take_skill_test\|fn has_skill_test_in_flight\|pub in_flight_skill_test"
```

Apply these mechanical transforms (one worked example each; apply to every matching site the grep lists, **including `#[cfg(test)]` code**):

- `cx.state.in_flight_skill_test.is_some()` → `cx.state.has_skill_test_in_flight()`
  - worked: `skill_test.rs:70` `if cx.state.in_flight_skill_test.is_some() {` → `if cx.state.has_skill_test_in_flight() {`
- `state.in_flight_skill_test.is_none()` → `!state.has_skill_test_in_flight()`
  - worked: `skill_test.rs:1254` `assert!(state.in_flight_skill_test.is_none());` → `assert!(!state.has_skill_test_in_flight());`
- `…in_flight_skill_test.as_ref()` → `…current_skill_test()`
  - worked: `skill_test.rs:241` `let Some(in_flight) = cx.state.in_flight_skill_test.as_ref() else {` → `let Some(in_flight) = cx.state.current_skill_test() else {`
  - the `.as_ref().unwrap()` / `.as_ref().expect(...)` / `.as_ref().map(...)` forms become `current_skill_test().unwrap()` / `.expect(...)` / `.map(...)` (e.g. `skill_test.rs:868`, `:1379`, `:1421`, `:1457`)
- `cx.state.in_flight_skill_test.as_mut()` → `cx.state.current_skill_test_mut()`
- `cx.state.in_flight_skill_test = None;` → `let _ = cx.state.take_skill_test();` **only at the genuine teardown site** (`skill_test.rs:466`). See the note below — this is the one site whose meaning changes in Task 2.
- `cx.state.in_flight_skill_test = Some(InFlightSkillTest { … })` (the push sites at `skill_test.rs:85` and the test-only `skill_test.rs:1108`): **leave as direct field writes for now** — Task 2 converts them to frame pushes. Mark each with a `// Task 2: becomes a SkillTest-frame push` comment so they are easy to find.

For chained field reads like `cx.state.in_flight_skill_test.as_ref().expect(...).investigator`, rewrite to `cx.state.current_skill_test().expect(...).investigator`.

- [ ] **Step 4: Full test suite — confirm pure-indirection (no behavior change)**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS.

- [ ] **Step 5: Clippy + fmt**

Run: `cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: PASS. (Clippy may suggest collapsing `current_skill_test().map(...)` forms — apply its suggestions.)

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "engine: route in_flight_skill_test reads through GameState accessors

Add current_skill_test / _mut / take_skill_test / has_skill_test_in_flight and
migrate every read/write site to them. Pure indirection over the existing
Option field — isolates the move onto the SkillTest frame (#348).

Refs #348.
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Carry the payload on the `SkillTest` frame; drop the `Option` field

Move the data onto `Continuation::SkillTest(InFlightSkillTest)`, push it early (at test start, replacing the `Option` set), repoint the accessors at the frame, and remove the field.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`Continuation::SkillTest` variant; accessor bodies; `as_resolution` match arms; remove the field)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (push site, `open_commit_window`, teardown, the `rposition` frame searches)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` `SkillTest` match arm — now binds the payload)

**Interfaces:**
- Consumes: the Task-1 accessor names (signatures unchanged; only bodies change).
- Produces: `Continuation::SkillTest(InFlightSkillTest)` (was a unit variant). `current_skill_test()` returns the topmost `SkillTest` frame's payload.

- [ ] **Step 1: Change the variant to carry the payload**

In `crates/game-core/src/state/game_state.rs`, the `Continuation` enum: change

```rust
    /// A skill test is mid-resolution. A resume-handle only — the test's
    /// data lives in the singleton [`GameState::in_flight_skill_test`] …
    SkillTest,
```

to

```rust
    /// A skill test is mid-resolution. Carries the in-flight test's data
    /// directly (the former `GameState::in_flight_skill_test` singleton, folded
    /// onto the frame — #348). Pushed at test start; popped when the test fully
    /// resolves. At most one is ever on the stack (no nesting today).
    SkillTest(InFlightSkillTest),
```

Update the two `Continuation::as_resolution` / `as_resolution_mut` match arms (`game_state.rs:516,524`) from `Continuation::SkillTest | Continuation::Choice(_) => None` to `Continuation::SkillTest(_) | Continuation::Choice(_) => None`.

- [ ] **Step 2: Repoint the accessors at the frame; remove the field**

Remove the `pub in_flight_skill_test: Option<InFlightSkillTest>` field from the `GameState` struct (and its doc-comment). Rewrite the Task-1 accessor bodies:

```rust
#[must_use]
pub fn current_skill_test(&self) -> Option<&InFlightSkillTest> {
    self.continuations.iter().rev().find_map(|c| match c {
        Continuation::SkillTest(t) => Some(t),
        _ => None,
    })
}

pub fn current_skill_test_mut(&mut self) -> Option<&mut InFlightSkillTest> {
    self.continuations.iter_mut().rev().find_map(|c| match c {
        Continuation::SkillTest(t) => Some(t),
        _ => None,
    })
}

pub fn take_skill_test(&mut self) -> Option<InFlightSkillTest> {
    let pos = self
        .continuations
        .iter()
        .rposition(|c| matches!(c, Continuation::SkillTest(_)))?;
    match self.continuations.remove(pos) {
        Continuation::SkillTest(t) => Some(t),
        _ => unreachable!("rposition matched SkillTest"),
    }
}

#[must_use]
pub fn has_skill_test_in_flight(&self) -> bool {
    self.continuations
        .iter()
        .any(|c| matches!(c, Continuation::SkillTest(_)))
}
```

(`rev().find_map` returns the *topmost* `SkillTest` frame — the innermost test under any reaction-window frames above it, matching the old singleton's "the current test" semantics.)

- [ ] **Step 3: Push the frame early at test start (replacing the `Option` set)**

In `skill_test.rs`, replace the `cx.state.in_flight_skill_test = Some(InFlightSkillTest { … });` block at line ~85 with a frame push of the same payload:

```rust
    cx.state
        .continuations
        .push(crate::state::Continuation::SkillTest(InFlightSkillTest {
            investigator,
            skill,
            kind,
            difficulty,
            committed_by_active: Vec::new(),
            tested_location,
            follow_up,
            on_fail,
            on_success,
            source,
            continuation: FinishContinuation::AwaitingCommit,
            test_modifier,
            bonus_attack_damage: 0,
        }));
```

This is safe (all validation precedes line 85; the only post-85 outcomes are `AwaitingInput`). The Mind-over-Matter substitution path (line ~112) now runs with the `SkillTest` frame already on the stack and `pending_substitution_prompt` set above it — the `resolve_input` cascade routes substitution first (unchanged), and `resume_substitution_choice` rewrites the skill via `current_skill_test_mut()` (Task 1 already migrated it).

- [ ] **Step 4: Stop pushing in `open_commit_window`**

In `open_commit_window` (`skill_test.rs:152`), the frame is now already on the stack from Step 3. Remove the push:

```rust
    // before
    cx.state
        .continuations
        .push(crate::state::Continuation::SkillTest);
    // after — (delete; the SkillTest frame was pushed at test start)
```

Keep the rest (it reads `current_skill_test()` for the prompt and returns the commit `AwaitingInput`).

- [ ] **Step 5: Fix the teardown to a single pop**

At the teardown (`skill_test.rs:466` region), the old code did `in_flight_skill_test = None;` *then* searched + removed the `SkillTest` frame via `rposition` (lines ~474-479). Both are now one operation. Replace the pair with:

```rust
    let _ = cx.state.take_skill_test();
```

Delete the now-redundant `rposition`/`remove` block that followed. (The other `rposition` for `SkillTest` at `skill_test.rs:398-400`, used to *locate* the frame mid-drive, becomes a `current_skill_test()` read or stays a `matches!(_, SkillTest(_))` position search — keep whichever the surrounding code needs; if it only checked presence, use `has_skill_test_in_flight()`.)

- [ ] **Step 6: Update the `resolve_input` `SkillTest` arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, the match `Some(crate::state::Continuation::SkillTest)` (line ~500) becomes `Some(crate::state::Continuation::SkillTest(_))`.

- [ ] **Step 7: Fix the test-only push site**

`skill_test.rs:1108` (`state.in_flight_skill_test = Some(InFlightSkillTest { … })` in a `#[cfg(test)]`): replace with a `continuations.push(Continuation::SkillTest(InFlightSkillTest { … }))`. Likewise update the engine-unit-test fixture at `engine/mod.rs:4429` (`vec![crate::state::Continuation::SkillTest]` → `vec![Continuation::SkillTest(<payload>)]`) — construct a minimal `InFlightSkillTest` there; copy the field set from the production push in Step 3.

- [ ] **Step 8: Compile + fix fallout**

Run: `cargo build -p game-core --all-targets`
Expected: errors only at remaining unit-variant `Continuation::SkillTest` matches/constructions (no payload). Fix each: `SkillTest` → `SkillTest(_)` in patterns, `SkillTest(payload)` in constructions. Re-run until clean.

- [ ] **Step 9: Full suite**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS — every skill-test path (Investigate/Fight/Evade/`PerformSkillTest`), reaction-window-mid-test, and Mind-over-Matter substitution test unchanged.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "engine: carry in-flight skill test on its SkillTest frame

Change Continuation::SkillTest to carry InFlightSkillTest; push it at test
start (replacing the in_flight_skill_test Option set), point the accessors at
the topmost SkillTest frame, single-pop at teardown, and remove the Option
field. The continuation stack is now the only record of an in-flight test.

Refs #348.
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Regression test — the substitution path round-trips through the frame

The Mind-over-Matter prompt is the one place test data exists before the commit window; pin that it now lives on the frame and survives the substitution resume.

**Files:**
- Test: `crates/game-core/src/engine/dispatch/skill_test.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `GameState::current_skill_test()`, `Continuation::SkillTest`.

- [ ] **Step 1: Add the test**

Locate the existing Mind-over-Matter substitution test in `skill_test.rs` (grep `substitution` in the test module). Add an assertion-style test alongside it that, after `start_skill_test` returns the substitution `AwaitingInput`, asserts the `SkillTest` frame already holds the payload:

```rust
#[test]
fn substitution_prompt_keeps_the_test_on_its_frame() {
    // Build a state where the active investigator has a Mind-over-Matter
    // substitution covering the about-to-be-tested skill, then start the test.
    // (Reuse the existing substitution-test setup helper in this module.)
    let mut cx = /* the same Cx the neighboring substitution test builds */;
    let outcome = start_skill_test(/* …Combat/Agility test args… */);
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "substitution prompt should suspend",
    );
    assert!(
        cx.state.current_skill_test().is_some(),
        "the in-flight test must live on a SkillTest frame during the \
         substitution prompt, not in a removed Option field",
    );
    assert!(
        cx.state
            .continuations
            .iter()
            .any(|c| matches!(c, crate::state::Continuation::SkillTest(_))),
        "a SkillTest frame is on the stack before the commit window",
    );
}
```

Fill the `cx` / `start_skill_test` call from the neighboring substitution test's setup verbatim (the implementer reads that test for the exact builder calls — do not invent fixture APIs).

- [ ] **Step 2: Run it**

Run: `cargo test -p game-core substitution_prompt_keeps_the_test_on_its_frame`
Expected: PASS.

- [ ] **Step 3: Full CI gauntlet**

Run all six commands from Global Constraints.
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "engine: test the in-flight test lives on its frame during substitution

Refs #348.
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage (spec §A — \"fold in_flight_skill_test onto its frame\"):**
- `SkillTest(InFlightSkillTest)` payload + field removal → Task 2 Steps 1–2. ✓
- `current_skill_test()` accessor finding the frame wherever it is (not `.last()`) → Task 2 Step 2 (`rev().find_map`). ✓
- Innermost-wins if nesting ever appears → `rev()` returns topmost. ✓
- No behavior change → Task 1 pure indirection; Task 2 preserves payload + ordering; Task 3 pins the one tricky path. ✓

**Placeholder scan:** Task 3's `cx`/`start_skill_test` call is deliberately delegated to "copy the neighboring substitution test's setup" rather than inventing fixture APIs I haven't read — the implementer has the exact source adjacent. All other steps show complete code. The mechanical 57-site migration uses one worked example per access-pattern + the exact enumerating grep (DRY, as in #345 Task 1).

**Type consistency:** accessor names (`current_skill_test`/`_mut`/`take_skill_test`/`has_skill_test_in_flight`) are identical across Task 1 (Option bodies) and Task 2 (frame bodies); `Continuation::SkillTest(InFlightSkillTest)` matches its construction (Task 2 Steps 3, 7) and patterns (Steps 1, 6, 8).

**Out of scope (later sub-PRs):** `pending_*` mode migration + ladder collapse (2b), `Mulligan`/`DrawEncounterCard` → `InputResponse` (2c), tokens (#347), revelation disposal (#380).
