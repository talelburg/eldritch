# Skill-test ST.1/ST.2 Player Windows (#374) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open the two RR p.26 framework Fast/reaction player windows during every skill test — after ST.1 (before commit) and after ST.2 (before the chaos token) — as cursor-step insertions in the `advance` driver, auto-skipping when nothing is playable.

**Architecture:** Add two `SkillTestStep` cursor steps (`PreCommitWindow`, `PreTokenWindow`) that pre-advance and `return open_fast_window(...)`; one new `WindowKind::SkillTestPlayerWindow { before_token }` riding the existing `Continuation::Resolution` frame; a `run_window_continuation` arm that re-enters `advance` on close. No new `Continuation` variant.

**Tech Stack:** Rust (`game-core` kernel). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-06-22-skill-test-st1-st2-player-windows-design.md`

## Global Constraints

- **Behaviour-preserving except for the new windows.** The full existing suite must stay green; the only intended behaviour change is that every skill test now opens (and usually auto-skips) two windows, adding two `WindowOpened`/`WindowClosed` event pairs per test.
- **No new `Continuation` variant.** The windows ride `Continuation::Resolution` via `open_fast_window`.
- **The `WindowKind` + `before_token` discriminant is transitional** — absorbed by #431 (the `FastWindow` unification). Document it as such, don't entrench it.
- **Match CI's strict flags before every commit** (from `CLAUDE.md`):
  ```sh
  RUSTFLAGS="-D warnings" cargo test --all --all-features
  cargo clippy --all-targets --all-features -- -D warnings
  cargo fmt --check
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
  ```
- **Branch:** `engine/skilltest-player-windows` (already created; the spec is committed there).
- **Commit trailers** (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
  ```

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/game-core/src/state/game_state.rs` | `WindowKind` + `SkillTestStep` enums | Add `WindowKind::SkillTestPlayerWindow { before_token }`; add `SkillTestStep::PreCommitWindow` / `PreTokenWindow` |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | window pipeline | Add the `(kind, pattern)` no-match arm + the `run_window_continuation` re-entry arm |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | the `advance` driver + entry points | Add the two `advance` arms; init `start_skill_test` / `finish_skill_test` to the new steps; tests |

---

### Task 1: Add the `WindowKind` variant + its match arms

Add the window kind and satisfy the two exhaustive `match`es over `WindowKind`. Nothing opens this window yet, so behaviour is unchanged and the suite stays green.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`enum WindowKind`, ~line 1272)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (no-match arm ~417; `run_window_continuation` ~962)

**Interfaces:**
- Produces: `WindowKind::SkillTestPlayerWindow { before_token: bool }`. `run_window_continuation(cx, SkillTestPlayerWindow { .. })` calls `super::skill_test::advance(cx)`.

- [ ] **Step 1: Add the variant**

In `crates/game-core/src/state/game_state.rs`, add to `enum WindowKind` (after `AfterEnteredPlay { … }`, before the closing brace):

```rust
    /// A framework player window during a skill test (RR p.26): after ST.1
    /// (before commit) when `before_token` is `false`, after ST.2 (before the
    /// chaos token) when `true`. Carries no event payload — it gates Fast plays
    /// / reactions, it is not an after-event reaction window. Opened by the
    /// skill-test driver (`advance`) at the `PreCommitWindow` / `PreTokenWindow`
    /// cursor steps; its close re-enters `advance`.
    ///
    /// `before_token` feeds `WindowOpened`/`WindowClosed` observability only;
    /// both windows share one continuation (re-enter `advance`, which resumes at
    /// the pre-advanced cursor). Transitional: the EmitEvent-frame slice (#431)
    /// dissolves this into the generic `FastWindow`, where "which window" is read
    /// from the `SkillTest` frame's cursor beneath it, not stored here.
    SkillTestPlayerWindow {
        /// `false` = the ST.1→ST.2 window; `true` = the ST.2→ST.3 window.
        before_token: bool,
    },
```

- [ ] **Step 2: Add the no-match arm in the `(kind, pattern)` matcher**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, the `match (kind, pattern)` block (~line 417) has a tuple arm listing every window kind that yields *no* reaction match. Add `SkillTestPlayerWindow` to that window-kind list so it returns no candidates (no `Trigger::OnEvent` pattern binds to it — it gates Fast plays only). Find the arm whose window-kind side begins `WindowKind::PlayerWindow(_) | WindowKind::AfterEnemyDefeated { .. } | …` and add one line to that `|` chain:

```rust
            WindowKind::PlayerWindow(_)
            | WindowKind::AfterEnemyDefeated { .. }
            | WindowKind::SkillTestPlayerWindow { .. }
            | WindowKind::AfterEnemyAttackDamagedAsset { .. }
            | WindowKind::AfterSuccessfulInvestigate { .. }
            | WindowKind::AfterEnteredPlay { .. }
            | WindowKind::BeforeEnemyAttack { .. }
            | WindowKind::BeforeDiscoverClues { .. },
```

(If the compiler reports the match is still non-exhaustive, add `WindowKind::SkillTestPlayerWindow { .. }` wherever it points — the no-match arm is the correct home: this window has no reaction pattern.)

- [ ] **Step 3: Add the `run_window_continuation` re-entry arm**

In the same file, `run_window_continuation` (~line 962), add an arm (after the `PlayerWindow(_)` arm):

```rust
        // A skill-test player window (#374) closed: re-enter the skill-test
        // driver. The cursor was pre-advanced before the window opened, so
        // `advance` resumes at the next step (AwaitingCommit after window 1,
        // Resolving after window 2). Reached on both the auto-skip inline path
        // and the wait-then-close path.
        WindowKind::SkillTestPlayerWindow { .. } => super::skill_test::advance(cx),
```

- [ ] **Step 4: Compile + full suite (behaviour unchanged — nothing opens this window yet)**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: add WindowKind::SkillTestPlayerWindow + its match arms (#374)

The skill-test player-window kind + the no-reaction-match arm and the
run_window_continuation re-entry into advance. Nothing opens it yet; behaviour
unchanged. Transitional kind (absorbed by #431).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 2: Open the windows — cursor steps + `advance` arms + entry inits

Wire the windows: two new `SkillTestStep` steps, the two `advance` arms, and the entry-point cursor inits. This is the behaviour change (windows now open). Includes the auto-skip flow test.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`enum SkillTestStep` + its doc-block)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`advance` match; `start_skill_test` init; `finish_skill_test` init; tests)

**Interfaces:**
- Consumes: `WindowKind::SkillTestPlayerWindow { before_token }` + the `run_window_continuation` arm (Task 1); `super::reaction_windows::open_fast_window` (already `pub(super)`).
- Produces: `SkillTestStep::PreCommitWindow` (initial state) and `PreTokenWindow` (post-commit state). `advance` opens window 1 at `PreCommitWindow` (pre-advancing to `AwaitingCommit`) and window 2 at `PreTokenWindow` (pre-advancing to `Resolving`).

- [ ] **Step 1: Add the cursor variants**

In `crates/game-core/src/state/game_state.rs`, add to `enum SkillTestStep` — `PreCommitWindow` as the **first** variant (the new initial state), and `PreTokenWindow` right after `AwaitingCommit`:

```rust
    /// The RR p.26 player window after ST.1 (skill determined) and before ST.2
    /// (commit). The initial state at skill-test start. `advance` opens the
    /// window here, pre-advancing to `AwaitingCommit`. (#374.)
    PreCommitWindow,
    /// Initial state: waiting on the commit-window
    /// [`ResolveInput`](crate::action::PlayerAction::ResolveInput).
    AwaitingCommit,
    /// The RR p.26 player window after ST.2 (commit) and before ST.3 (reveal
    /// chaos token). Set by `finish_skill_test` once the commit is stored;
    /// `advance` opens the window here, pre-advancing to `Resolving`. (#374.)
    PreTokenWindow,
    /// Commit submitted: the next driver iteration runs the resolution body …
    Resolving,
```

(Leave the existing `AwaitingCommit` / `Resolving` doc bodies intact; only insert the two new variants around them.)

- [ ] **Step 2: Update the `SkillTestStep` doc-block "Variants" list**

In the same doc-block (the `/// Variants:` prose above the enum), add bullets so the list is complete:

```rust
/// - [`PreCommitWindow`](Self::PreCommitWindow) — initial state; `advance` opens
///   the ST.1→ST.2 player window, then pre-advances to `AwaitingCommit`.
/// - [`PreTokenWindow`](Self::PreTokenWindow) — set after the commit; `advance`
///   opens the ST.2→ST.3 player window, then pre-advances to `Resolving`.
```

- [ ] **Step 3: Add the two `advance` arms**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, in `advance`'s `match continuation` block, add the `PreCommitWindow` arm (before `AwaitingCommit`) and the `PreTokenWindow` arm (before `Resolving`):

```rust
            SkillTestStep::PreCommitWindow => {
                // RR p.26 player window after ST.1. Pre-advance to AwaitingCommit
                // BEFORE opening (the suspend/resume invariant), then return the
                // window's outcome directly: open_fast_window either auto-skips
                // (running run_window_continuation -> advance inline, which
                // resumes at AwaitingCommit) or parks the window on top and
                // returns Done (a pure-Fast window emits no AwaitingInput, so the
                // engine idles with it on top). Either way we must NOT fall
                // through — a parked window is invisible to the loop's
                // top_reaction_window_index check.
                cx.state
                    .current_skill_test_mut()
                    .expect("advance(PreCommitWindow): the SkillTest frame must exist")
                    .continuation = SkillTestStep::AwaitingCommit;
                return super::reaction_windows::open_fast_window(
                    cx,
                    crate::state::WindowKind::SkillTestPlayerWindow { before_token: false },
                );
            }
            // … existing AwaitingCommit arm …
            SkillTestStep::PreTokenWindow => {
                // RR p.26 player window after ST.2. Pre-advance to Resolving,
                // then return open_fast_window's outcome (see PreCommitWindow).
                cx.state
                    .current_skill_test_mut()
                    .expect("advance(PreTokenWindow): the SkillTest frame must exist")
                    .continuation = SkillTestStep::Resolving;
                return super::reaction_windows::open_fast_window(
                    cx,
                    crate::state::WindowKind::SkillTestPlayerWindow { before_token: true },
                );
            }
            // … existing Resolving arm …
```

- [ ] **Step 4: Init the entry points to the new steps**

In `crates/game-core/src/engine/dispatch/skill_test.rs`:

(a) `start_skill_test` — change the initial cursor (currently `continuation: SkillTestStep::AwaitingCommit,` at ~line 104) to:
```rust
            continuation: SkillTestStep::PreCommitWindow,
```

(b) `finish_skill_test` — change the post-commit cursor (currently `t.continuation = SkillTestStep::Resolving;` at ~line 245) to:
```rust
    t.continuation = SkillTestStep::PreTokenWindow;
```

> Note: the in-module test fixture that builds a `SkillTest` directly at `SkillTestStep::AwaitingCommit` (~line 1178) is intentionally left as-is — it bypasses `PreCommitWindow` to test commit behaviour directly.

- [ ] **Step 5: Build + run the existing skill-test suite; fix exact-event-slice assertions**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | tee /tmp/374-test.log; grep -E "FAILED|panicked" /tmp/374-test.log || echo "ALL PASS"
```
Most skill-test tests use the order-insensitive `assert_event!` macros and are unaffected. Any failure will be a test asserting an **exact** event slice (`assert_eq!` on `result.events`) for a skill test — it now has two extra `WindowOpened`/`WindowClosed { SkillTestPlayerWindow { before_token } }` pairs (window 1 before `SkillTestStarted`'s commit step, window 2 after commit before the token). For each such failure, insert the two pairs at the correct positions in the expected slice. Re-run until green.

- [ ] **Step 6: Add the auto-skip flow test**

In `crates/game-core/src/engine/dispatch/skill_test.rs::tests`, add a test that drives a full no-eligibility skill test and asserts both windows open+close (auto-skipped) around commit. Adapt state setup from the sibling `commit_emits_then_resolves_through_advance` test:

```rust
/// Both ST.1/ST.2 player windows open and auto-skip (no registry / nothing
/// Fast-eligible), bracketing the commit, and the test still resolves. (#374.)
#[test]
fn skill_test_opens_and_auto_skips_both_player_windows() {
    use crate::state::{ChaosToken, WindowKind};

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

    // start -> PreCommitWindow auto-skips window 1 -> parks at AwaitingCommit.
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
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "commit prompt");
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::WindowOpened { kind: WindowKind::SkillTestPlayerWindow { before_token: false } }
        )) && events.iter().any(|e| matches!(
            e,
            Event::WindowClosed { kind: WindowKind::SkillTestPlayerWindow { before_token: false } }
        )),
        "window 1 (before commit) opened and auto-skipped: {events:?}",
    );

    // commit nothing -> PreTokenWindow auto-skips window 2 -> resolves to end.
    let out = finish_skill_test(&mut cx, &[]);
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::WindowOpened { kind: WindowKind::SkillTestPlayerWindow { before_token: true } }
        )) && events.iter().any(|e| matches!(
            e,
            Event::WindowClosed { kind: WindowKind::SkillTestPlayerWindow { before_token: true } }
        )),
        "window 2 (before token) opened and auto-skipped: {events:?}",
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "the test resolved to the end: {events:?}",
    );
}
```

Run it:
```bash
cargo test -p game-core --lib engine::dispatch::skill_test::tests::skill_test_opens_and_auto_skips_both_player_windows
```
Expected: PASS.

- [ ] **Step 7: Full gauntlet (host + wasm)**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | grep -E "FAILED|panicked" || echo "TESTS PASS"
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: open ST.1/ST.2 skill-test player windows (#374)

Two SkillTestStep cursor steps (PreCommitWindow/PreTokenWindow) open the RR p.26
framework player windows via open_fast_window, pre-advancing the cursor and
returning the window's outcome (auto-skip drives onward; a parked pure-Fast
window idles with the frame on top). Auto-skips when nothing is playable.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 3: Resume-arm test

Prove that closing a skill-test player window re-enters `advance` at the pre-advanced cursor (the `run_window_continuation` arm), independent of the auto-skip path.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs::tests` (or `reaction_windows.rs::tests`)

**Interfaces:**
- Consumes: `run_window_continuation` (Task 1), the cursor steps (Task 2).

- [ ] **Step 1: Add the resume-arm test**

In `crates/game-core/src/engine/dispatch/skill_test.rs::tests`, add a test that constructs a "window 1 about to close" state — a `SkillTest` whose cursor is already `AwaitingCommit` (pre-advanced) — and calls `run_window_continuation` for the skill-test window, asserting it re-enters `advance` and emits the commit prompt:

```rust
/// Closing a skill-test player window re-enters `advance` at the pre-advanced
/// cursor (the run_window_continuation arm), not just via the auto-skip path.
/// (#374.)
#[test]
fn closing_a_skill_test_player_window_re_enters_advance() {
    use crate::state::{ChaosToken, Continuation, InFlightSkillTest, WindowKind};

    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    // A SkillTest pre-advanced to AwaitingCommit, as if window 1 just opened.
    state.continuations.push(Continuation::SkillTest(InFlightSkillTest {
        investigator: inv,
        skill: SkillKind::Willpower,
        kind: SkillTestKind::Plain,
        difficulty: 2,
        committed_by_active: Vec::new(),
        tested_location: None,
        follow_up: SkillTestFollowUp::None,
        on_fail: None,
        on_success: None,
        source: None,
        continuation: SkillTestStep::AwaitingCommit,
        test_modifier: 0,
        bonus_attack_damage: 0,
    }));
    let mut events = Vec::new();
    let out = super::super::reaction_windows::run_window_continuation(
        &mut Cx { state: &mut state, events: &mut events },
        WindowKind::SkillTestPlayerWindow { before_token: false },
    );
    let EngineOutcome::AwaitingInput { request, .. } = &out else {
        panic!("expected the commit prompt after the window closed, got {out:?}");
    };
    assert!(
        request.prompt.contains("Commit cards"),
        "re-entered advance at AwaitingCommit: {request:?}",
    );
}
```

> If `run_window_continuation` or `InFlightSkillTest`'s fields aren't reachable at this path, mirror the import style of the sibling tests in the module (they already construct `Cx` and reference `crate::state::…`). Adjust the field list if `InFlightSkillTest` has changed.

- [ ] **Step 2: Run it**

```bash
cargo test -p game-core --lib engine::dispatch::skill_test::tests::closing_a_skill_test_player_window_re_enters_advance
```
Expected: PASS.

- [ ] **Step 3: Full host gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | grep -E "FAILED|panicked" || echo "TESTS PASS"
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: test skill-test player-window close re-enters advance (#374)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 4: Phase-doc update (final commit, after CI green)

Per `CLAUDE.md` / `docs/phases/README.md`, update the phase doc only when the PR is ready to merge.

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Update ordering step 5 + section D**

In `docs/phases/phase-7-the-gathering.md`: in **section D** (Skill-test player windows) and **ordering step 5**, note that #374 shipped (this PR #) — the two RR p.26 framework player windows now open in `advance` (`PreCommitWindow`/`PreTokenWindow`), via `WindowKind::SkillTestPlayerWindow`, auto-skipping when empty; #64 and Fire Axe (02032, first real consumer) remain. Add the **#431 cross-reference** noted in the substrate PR: the `WindowKind` discriminant dissolves into the generic `FastWindow` there. Add a **Decisions made** entry only if load-bearing — e.g. "skill-test fast windows ride a transitional `WindowKind` (→ #431), not `PlayerWindow(PhaseStep)` (whose close routes to a phase anchor, not `advance`)."

- [ ] **Step 2: Commit (after CI is green on the opened PR)**

```bash
git add docs/phases/phase-7-the-gathering.md && git commit -m "$(cat <<'EOF'
docs: phase-7 — ST.1/ST.2 skill-test player windows shipped (#374)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

## Self-Review

**1. Spec coverage:**
- Cursor flow `PreCommitWindow → AwaitingCommit → PreTokenWindow → Resolving` → Task 2 (variants + arms + inits). ✓
- `WindowKind::SkillTestPlayerWindow { before_token }` riding `Continuation::Resolution` → Task 1. ✓
- `run_window_continuation` re-enters `advance` → Task 1 Step 3. ✓
- Arm `return`s `open_fast_window` (not fall-through) — the §1 correctness point → Task 2 Step 3 (code + comment). ✓
- Auto-skip behaviour + test → Task 2 Step 6. ✓
- Resume-arm test → Task 3. ✓
- Substitution interaction (window 1 after ST.1 substitution) — covered by construction (init to `PreCommitWindow`; `resume_substitution_choice` already calls `advance`, which now hits `PreCommitWindow` first). No code change needed; the existing substitution tests in the suite are the regression (Task 2 Step 5/7). ✓
- Forced-run-below guard unchanged → not touched. ✓
- Event-stream change + exact-slice assertion fixes → Task 2 Step 5. ✓
- Transitional `WindowKind` note (#431) → Task 1 Step 1 doc + Task 4. ✓
- Fire Axe documented as follow-up → Task 4 (phase doc); spec already records it. ✓
- Light test scope (no synthetic registry) → Tasks 2–3 only. ✓

**2. Placeholder scan:** No "TBD"/"TODO"/"handle edge cases". Task 2 Step 5 describes a *conditional* fix (exact-slice assertions) with the exact transformation (insert the two pairs) rather than vague "fix tests" — acceptable because the set of failing tests is discovered by running, and the fix is mechanical and fully specified. Test code blocks are complete.

**3. Type consistency:** `SkillTestPlayerWindow { before_token: bool }` used identically in Tasks 1–3. `PreCommitWindow` / `PreTokenWindow` consistent. `open_fast_window(cx, WindowKind)` signature matches the existing `pub(super)` fn. `run_window_continuation(cx, kind)` matches. `InFlightSkillTest` field list in Task 3 mirrors `start_skill_test`'s constructor (spec/substrate). ✓
