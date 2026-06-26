# Skill-Test Result Feedback (#478) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After a skill test resolves, pause and show the player the chaos token drawn, the final total vs difficulty, and pass/fail by N — dismissed with a Confirm before the consequence resolves.

**Architecture:** A new gated cursor step (`SkillTestStep::AcknowledgeOutcome`) makes the engine suspend with `AwaitingInput{Confirm}` at skill-test resolution when a `GameState.interactive_acknowledge` flag is set (the server sets it; tests and headless consumers leave it off, so nothing else churns). The engine stays presentation-free: the web client renders the result panel entirely from the structured events it already receives, retaining the resolution batch (`last_events`) and the test's difficulty (`last_skill_test_difficulty`) in the store.

**Tech Stack:** Rust, `game-core` engine (no_std-ish kernel), Leptos (web, wasm32 + native reducer), serde, sqlx (server), `wasm-bindgen-test`.

## Global Constraints

- **CI gauntlet (warnings-as-errors).** Before pushing, run all of: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Kernel purity.** `game-core` has no I/O and no presentation. The engine's Confirm prompt is a short generic string; all result formatting lives in the web client.
- **Validate-first / mutate-second.** Every engine handler checks all preconditions and returns `Rejected` (state/events unchanged) before mutating.
- **Wire compatibility.** New `GameState` field carries `#[serde(default)]` so already-persisted game seeds (which lack it) still deserialize.
- **No silent approximation.** The acknowledge is gated explicitly; the flag-off path must be behaviorally identical to today (no extra emitted events, same order).
- **Commit subjects:** `scope: description` (e.g. `engine: …`, `web: …`, `server: …`).

---

### Task 1: Engine — `interactive_acknowledge` flag on `GameState`

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (struct `GameState`, ~line 38; add field)
- Modify: `crates/game-core/src/state/builder.rs` (`build()`, ~line 353; set field)
- Test: `crates/game-core/src/state/builder.rs` (`#[cfg(test)]` module already present)

**Interfaces:**
- Produces: `GameState.interactive_acknowledge: bool` (public field, default `false`).

- [ ] **Step 1: Write the failing test**

In `crates/game-core/src/state/builder.rs`, inside the existing `#[cfg(test)] mod` (the one near the bottom that already has `fn build_starts_with_empty_set_aside_locations`), add:

```rust
    #[test]
    fn build_defaults_interactive_acknowledge_off() {
        let state = GameStateBuilder::new().build();
        assert!(
            !state.interactive_acknowledge,
            "interactive acknowledgment is opt-in; off by default so non-interactive \
             consumers and existing tests never pause"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core build_defaults_interactive_acknowledge_off`
Expected: FAIL — compile error, `no field interactive_acknowledge on type GameState`.

- [ ] **Step 3: Add the field to `GameState`**

In `crates/game-core/src/state/game_state.rs`, add this field to the `GameState` struct (place it just after the `pub resolution: Option<...>` / near the end of the struct, before the closing brace — any position is fine since the struct is `#[non_exhaustive]`):

```rust
    /// When set, the engine suspends with an `AwaitingInput { InputKind::Confirm }`
    /// at skill-test resolution (after the result events are emitted, before the
    /// ST.7 consequence resolves) so an interactive host can show the player the
    /// result and wait for an acknowledgment (#478). A *cosmetic* pause — it
    /// makes no game decision — so it is gated: the server sets it for human play,
    /// while tests and non-interactive/headless consumers leave it `false` and
    /// resolve straight through. `#[serde(default)]` keeps already-persisted game
    /// seeds (written before this field existed) deserializable.
    #[serde(default)]
    pub interactive_acknowledge: bool,
```

- [ ] **Step 4: Initialize it in `build()`**

In `crates/game-core/src/state/builder.rs`, in the `GameState { … }` literal inside `build()` (~line 353), add the field alongside the other initializers (e.g. right after `victory_display: Vec::new(),`):

```rust
            interactive_acknowledge: false,
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p game-core build_defaults_interactive_acknowledge_off`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/state/builder.rs
git commit -m "engine: add interactive_acknowledge flag to GameState (#478)"
```

---

### Task 2: Engine — `AcknowledgeOutcome` step, gated Confirm pause, and resume routing

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`enum SkillTestStep`, ~line 1165; add variant)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`determine_outcome_step` ~line 488; `advance` ~line 743; add `acknowledge_outcome` fn; tests at bottom)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resume_skill_test_commit`, ~line 479)
- Test: `crates/game-core/src/engine/dispatch/skill_test.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `GameState.interactive_acknowledge` (Task 1).
- Produces:
  - `SkillTestStep::AcknowledgeOutcome` (enum variant).
  - `skill_test::acknowledge_outcome(cx: &mut Cx) -> EngineOutcome` (`pub(super)`): validates the in-flight test's cursor is `AcknowledgeOutcome`, advances it to `FireOnCommit`, returns `EngineOutcome::Done`; rejects otherwise.
  - `resume_skill_test_commit` now also routes `InputResponse::Confirm` → `acknowledge_outcome`.

- [ ] **Step 1: Write the failing tests**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, inside `#[cfg(test)] mod tests`, add these three tests (the module already has `use super::*;`, `use crate::event::Event;`, and `use crate::test_support::{test_investigator, GameStateBuilder};`):

```rust
    /// Flag on: a skill test pauses at the acknowledge step with a `Confirm`
    /// prompt — after the result events are emitted, before teardown — and a
    /// Confirm drives it to completion (#478).
    #[test]
    fn interactive_acknowledge_pauses_for_confirm_then_resolves() {
        use crate::state::ChaosToken;
        use crate::InputKind;

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        state.interactive_acknowledge = true;
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

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

        // Commit nothing -> resolution runs, then suspends at the acknowledge step.
        let out = finish_skill_test(&mut cx, &[]);
        let out = super::super::drive(&mut cx, out);
        let EngineOutcome::AwaitingInput { request, .. } = &out else {
            panic!("expected the acknowledge Confirm prompt, got {out:?}");
        };
        assert_eq!(request.kind, InputKind::Confirm, "acknowledge is a Confirm prompt");
        // The result is already logged when the player is asked to acknowledge.
        assert!(
            events.iter().any(|e| matches!(e, Event::ChaosTokenRevealed { .. })),
            "token revealed before the ack: {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(e, Event::SkillTestSucceeded { .. })),
            "outcome logged before the ack: {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(e, Event::SkillTestEnded { .. })),
            "teardown waits on the acknowledgment: {events:?}"
        );

        // Confirm -> drive into teardown.
        let out = acknowledge_outcome(&mut cx);
        let out = super::super::drive(&mut cx, out);
        assert_eq!(out, EngineOutcome::Done);
        assert!(
            events.iter().any(|e| matches!(e, Event::SkillTestEnded { .. })),
            "after Confirm the test resolved to the end: {events:?}"
        );
    }

    /// Flag off (default): no acknowledge pause — the test resolves straight
    /// through, exactly as before #478 (guards against test churn).
    #[test]
    fn no_acknowledge_pause_when_flag_off() {
        use crate::state::ChaosToken;

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
        let out = finish_skill_test(&mut cx, &[]);
        let out = super::super::drive(&mut cx, out);
        assert_eq!(out, EngineOutcome::Done, "no acknowledge pause when the flag is off");
        assert!(events.iter().any(|e| matches!(e, Event::SkillTestEnded { .. })));
    }

    /// `acknowledge_outcome` rejects (state untouched) when there is no in-flight
    /// test to acknowledge.
    #[test]
    fn acknowledge_outcome_rejects_without_in_flight_test() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = acknowledge_outcome(&mut cx);
        assert!(matches!(out, EngineOutcome::Rejected { .. }), "got {out:?}");
        assert!(events.is_empty(), "rejection emits no events");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p game-core interactive_acknowledge_pauses_for_confirm_then_resolves no_acknowledge_pause_when_flag_off acknowledge_outcome_rejects_without_in_flight_test`
Expected: FAIL — `AcknowledgeOutcome` variant and `acknowledge_outcome` fn don't exist (compile errors).

- [ ] **Step 3: Add the `AcknowledgeOutcome` cursor variant**

In `crates/game-core/src/state/game_state.rs`, in `pub enum SkillTestStep` (~line 1165), add this variant immediately after `DetermineOutcome` and before `FireOnCommit`:

```rust
    /// Cosmetic acknowledgment pause (#478). The result events
    /// (`ChaosTokenRevealed`, `SkillTestSucceeded`/`Failed`) are already emitted
    /// at [`DetermineOutcome`](Self::DetermineOutcome); when
    /// [`GameState::interactive_acknowledge`](crate::state::GameState::interactive_acknowledge)
    /// is set, `advance` suspends here with an `AwaitingInput { InputKind::Confirm }`
    /// so an interactive host can show the player the result before the ST.7
    /// consequence resolves. The cursor stays here across the suspension;
    /// `acknowledge_outcome` advances it to [`FireOnCommit`](Self::FireOnCommit)
    /// on the Confirm resume (mirroring the `AwaitingCommit` / `finish_skill_test`
    /// handshake). When the flag is off, `advance` advances straight to
    /// `FireOnCommit` without pausing.
    AcknowledgeOutcome,
```

- [ ] **Step 4: Retarget `determine_outcome_step` to the new step**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, in `determine_outcome_step` (~line 488), change the cursor pre-advance from `FireOnCommit` to `AcknowledgeOutcome`:

```rust
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = SkillTestStep::AcknowledgeOutcome;
```

(Also update the trailing doc line of `determine_outcome_step` that reads "Pre-advances the cursor to [`FireOnCommit`]…" to say "…to [`AcknowledgeOutcome`]…".)

- [ ] **Step 5: Add the `advance` arm for `AcknowledgeOutcome`**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, in `advance`'s `match continuation { … }` (~line 742, between the `DetermineOutcome` arm and the `FireOnCommit` arm), insert:

```rust
            SkillTestStep::AcknowledgeOutcome => {
                // RR-neutral cosmetic pause (#478). The result events were emitted
                // at DetermineOutcome; if interactive acknowledgment is enabled,
                // suspend with a Confirm so the player registers the result before
                // the ST.7 consequence resolves. The cursor stays at
                // AcknowledgeOutcome across the suspension — `acknowledge_outcome`
                // advances it on the Confirm resume (the AwaitingCommit /
                // finish_skill_test handshake). When off, advance straight to
                // FireOnCommit so non-interactive drives are unchanged.
                if cx.state.interactive_acknowledge {
                    return EngineOutcome::AwaitingInput {
                        request: InputRequest::confirm("Acknowledge the skill-test result."),
                        resume_token: ResumeToken(0),
                    };
                }
                cx.state
                    .current_skill_test_mut()
                    .expect("the SkillTest frame must persist across driver steps")
                    .continuation = SkillTestStep::FireOnCommit;
            }
```

- [ ] **Step 6: Add the `acknowledge_outcome` resume fn**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, add this function next to `finish_skill_test` (e.g. right after it, ~line 258):

```rust
/// Resume the cosmetic acknowledgment pause (#478): the player has Confirmed the
/// skill-test result. Validate-first — the in-flight test's cursor must be at
/// [`SkillTestStep::AcknowledgeOutcome`] — then advance it to
/// [`SkillTestStep::FireOnCommit`] and return [`EngineOutcome::Done`] so the
/// caller's `drive` loop runs the ST.7 consequences. Mirrors
/// [`finish_skill_test`]'s park-and-return-`Done` shape; on a bad cursor or no
/// in-flight test it rejects with state and events untouched.
pub(super) fn acknowledge_outcome(cx: &mut Cx) -> EngineOutcome {
    let Some(t) = cx.state.current_skill_test() else {
        return EngineOutcome::Rejected {
            reason: "skill-test acknowledge: no in-flight skill test to resume".into(),
        };
    };
    if !matches!(t.continuation, SkillTestStep::AcknowledgeOutcome) {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill-test acknowledge: not at the acknowledge step (continuation {:?})",
                t.continuation,
            )
            .into(),
        };
    }
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above")
        .continuation = SkillTestStep::FireOnCommit;
    EngineOutcome::Done
}
```

- [ ] **Step 7: Route `Confirm` to it in `resume_skill_test_commit`**

In `crates/game-core/src/engine/dispatch/mod.rs`, replace the body of `resume_skill_test_commit` (~line 482) with the cursor-agnostic response dispatch (add the `Confirm` arm; widen the reject message):

```rust
fn resume_skill_test_commit(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    match response {
        InputResponse::PickMultiple { selected } => {
            let indices: Vec<u32> = selected.iter().map(|o| o.0).collect();
            // The teardown tail (forced-run-sibling re-drive / end-of-turn
            // resume) now lives in `advance`'s `PostOnResolution` arm, so it
            // fires from teardown regardless of which resume re-entered the
            // driver.
            skill_test::finish_skill_test(cx, &indices)
        }
        // The cosmetic acknowledgment pause (#478) is the SkillTest frame's other
        // suspension point; a Confirm advances past it into the ST.7 consequences.
        InputResponse::Confirm => skill_test::acknowledge_outcome(cx),
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: the skill-test window expects InputResponse::PickMultiple \
                 (commit) or InputResponse::Confirm (acknowledge), got {other:?}",
            )
            .into(),
        },
    }
}
```

Also update the function's doc comment first line to reflect that it now resumes either the commit window or the acknowledgment pause.

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p game-core interactive_acknowledge_pauses_for_confirm_then_resolves no_acknowledge_pause_when_flag_off acknowledge_outcome_rejects_without_in_flight_test`
Expected: PASS (all three).

- [ ] **Step 9: Run the full engine suite to confirm no churn**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS — the flag defaults off, so existing skill-test tests are unaffected.

- [ ] **Step 10: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: gated Confirm-to-dismiss acknowledge step for skill tests (#478)"
```

---

### Task 3: Test-support — auto-confirm acknowledge in the no-commits driver + apply-level integration test

**Files:**
- Modify: `crates/game-core/src/test_support/resolver.rs` (`drive_to_terminal_no_commits`, ~line 344; imports ~line 50)
- Test: `crates/game-core/src/test_support/resolver.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `GameState.interactive_acknowledge` (Task 1); the dispatch `Confirm` routing (Task 2).
- Produces: `drive_to_terminal_no_commits` answers a `Confirm`-kind prompt with `InputResponse::Confirm`.

- [ ] **Step 1: Write the failing test**

In `crates/game-core/src/test_support/resolver.rs`, inside `#[cfg(test)] mod tests` (which has `use super::*;`), add:

```rust
    /// Flag on: a no-commits drive auto-answers the acknowledge `Confirm` and
    /// resolves the skill test to teardown (exercises both the helper and the
    /// dispatch-level Confirm routing end-to-end through `apply`).
    #[test]
    fn flag_on_no_commits_drive_auto_confirms_acknowledge() {
        use crate::event::Event;
        use crate::state::{ChaosToken, InvestigatorId, SkillKind};
        use crate::test_support::{test_investigator, GameStateBuilder};

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        state.interactive_acknowledge = true;

        let result = perform_skill_test_no_commits(state, inv, SkillKind::Willpower, 2);

        assert!(
            matches!(result.outcome, EngineOutcome::Done),
            "drive auto-confirmed the acknowledge and reached a terminal outcome: {:?}",
            result.outcome
        );
        assert!(
            result.events.iter().any(|e| matches!(e, Event::SkillTestEnded { .. })),
            "the test resolved to the end: {:?}",
            result.events
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core flag_on_no_commits_drive_auto_confirms_acknowledge`
Expected: FAIL — the helper answers the `Confirm` prompt with an empty `PickMultiple`, which the dispatch rejects (commit window already closed), so the drive never reaches `Done`/`SkillTestEnded` (panics or asserts in the helper / fails the assertions).

- [ ] **Step 3: Add `InputKind` to the imports**

In `crates/game-core/src/test_support/resolver.rs`, extend the engine import (~line 50) to include `InputKind` (it is re-exported from `crate::engine` alongside `InputRequest`):

```rust
use crate::engine::{apply, ApplyResult, EngineOutcome, InputKind, InputRequest};
```

- [ ] **Step 4: Teach the no-commits driver to answer `Confirm`**

In `crates/game-core/src/test_support/resolver.rs`, in `drive_to_terminal_no_commits`, replace the `let next = …` selection (~line 344) with a version that branches on the prompt kind:

```rust
        // The only `AwaitingInput`s in a no-commits drive are the commit window
        // (PickMultiple) and the #478 acknowledge pause (Confirm); a `Done`-idle
        // with an open window is a parked Fast player window to decline. Anything
        // else is terminal.
        let next = if let EngineOutcome::AwaitingInput { request, .. } = &outcome {
            match request.kind {
                InputKind::Confirm => InputResponse::Confirm,
                _ => InputResponse::PickMultiple {
                    selected: Vec::new(),
                },
            }
        } else if matches!(outcome, EngineOutcome::Done) && !state.open_windows().is_empty() {
            InputResponse::Skip
        } else {
            return ApplyResult {
                state,
                events,
                outcome,
            };
        };
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p game-core flag_on_no_commits_drive_auto_confirms_acknowledge`
Expected: PASS.

- [ ] **Step 6: Run the test-support suite**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core resolver`
Expected: PASS (existing resolver tests unaffected — they don't set the flag).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/test_support/resolver.rs
git commit -m "test: auto-confirm the skill-test acknowledge in the no-commits driver (#478)"
```

---

### Task 4: Web store — retain the resolution batch and the test difficulty

**Files:**
- Modify: `crates/web/src/store.rs` (`ClientState` ~line 26; `reduce` ~line 36)
- Test: `crates/web/src/store.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `ClientState.last_events: Vec<game_core::Event>` — the most recent `Applied` batch's events (`Hello` clears).
  - `ClientState.last_skill_test_difficulty: Option<i8>` — captured from the most recent `SkillTestStarted` event (`Hello` clears).

- [ ] **Step 1: Write the failing tests**

In `crates/web/src/store.rs`, inside `#[cfg(test)] mod tests` (which has `use super::*;`), add:

```rust
    #[test]
    fn applied_retains_events_and_captures_difficulty() {
        use game_core::state::{InvestigatorId, SkillKind};
        use game_core::Event;

        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: vec![Event::SkillTestStarted {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Willpower,
                    difficulty: 3,
                }],
                outcome: EngineOutcome::Done,
            },
        );
        assert_eq!(s.last_skill_test_difficulty, Some(3));
        assert_eq!(s.last_events.len(), 1);
    }

    #[test]
    fn hello_clears_retained_events_and_difficulty() {
        let mut s = ClientState {
            last_events: vec![],
            last_skill_test_difficulty: Some(3),
            ..Default::default()
        };
        // seed a non-empty last_events too
        s.last_events.push(game_core::Event::ScenarioStarted);
        reduce(
            &mut s,
            ServerMessage::Hello {
                state: Box::new(sample_state()),
                outcome: EngineOutcome::Done,
            },
        );
        assert!(s.last_events.is_empty(), "Hello clears the retained event batch");
        assert_eq!(s.last_skill_test_difficulty, None, "Hello clears the retained difficulty");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p web applied_retains_events_and_captures_difficulty hello_clears_retained_events_and_difficulty`
Expected: FAIL — `no field last_events` / `last_skill_test_difficulty` on `ClientState`.

- [ ] **Step 3: Add the fields to `ClientState`**

In `crates/web/src/store.rs`, add to the `ClientState` struct (~line 26):

```rust
    /// The most recent `Applied` batch's events, retained for views that render
    /// from event history (the #478 skill-test result panel). Cleared by `Hello`.
    pub last_events: Vec<game_core::Event>,
    /// Difficulty of the most recently *started* skill test, captured from
    /// `Event::SkillTestStarted` (which arrives in an earlier batch than the
    /// resolution). The result panel pairs it with the resolution batch's
    /// `SkillTestSucceeded`/`Failed` margin to show total-vs-difficulty.
    /// Cleared by `Hello`.
    pub last_skill_test_difficulty: Option<i8>,
```

- [ ] **Step 4: Populate them in `reduce`**

In `crates/web/src/store.rs`, update the two relevant arms of `reduce`. Replace the `Hello` arm:

```rust
        ServerMessage::Hello { state: s, outcome } => {
            state.game = Some(*s);
            state.outcome = Some(outcome);
            state.last_rejection = None;
            state.last_events = Vec::new();
            state.last_skill_test_difficulty = None;
        }
```

Replace the `Applied` arm (bind `events` instead of ignoring it):

```rust
        ServerMessage::Applied {
            state: s,
            events,
            outcome,
        } => {
            state.game = Some(*s);
            state.outcome = Some(outcome);
            if let Some(difficulty) = events.iter().find_map(|e| match e {
                game_core::Event::SkillTestStarted { difficulty, .. } => Some(*difficulty),
                _ => None,
            }) {
                state.last_skill_test_difficulty = Some(difficulty);
            }
            state.last_events = events;
        }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p web applied_retains_events_and_captures_difficulty hello_clears_retained_events_and_difficulty`
Expected: PASS.

- [ ] **Step 6: Confirm the existing reducer tests still pass**

Run: `cargo test -p web store`
Expected: PASS (the existing `applied_updates_game_and_outcome` etc. are unaffected).

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/store.rs
git commit -m "web: retain last event batch + skill-test difficulty in the store (#478)"
```

---

### Task 5: Web — `SkillTestResultView` panel

**Files:**
- Create: `crates/web/src/skill_test_result.rs`
- Modify: `crates/web/src/lib.rs` (add `pub mod skill_test_result;`)
- Modify: `crates/web/src/app.rs` (mount the view in the wasm-only block)
- Test (native): `crates/web/src/skill_test_result.rs` (`#[cfg(test)] mod tests` — pure `summarize`)
- Test (wasm): `crates/web/tests/skill_test_result.rs`

**Interfaces:**
- Consumes: `ClientState.last_events`, `ClientState.last_skill_test_difficulty` (Task 4).
- Produces:
  - `pub fn summarize(events: &[game_core::Event], difficulty: Option<i8>) -> Option<SkillTestSummary>` — pure; `None` unless the batch has a resolution event and a known difficulty.
  - `pub struct SkillTestSummary { pub token: String, pub total: i8, pub difficulty: i8, pub outcome: String }`.
  - `#[component] pub fn SkillTestResultView() -> impl IntoView`.

- [ ] **Step 1: Write the failing pure-logic tests**

Create `crates/web/src/skill_test_result.rs` with ONLY the test module first (so the test names exist to fail):

```rust
//! Skill-test result panel (#478): renders the just-resolved test — chaos token
//! drawn, final total vs difficulty, pass/fail by N — from the events the store
//! retained ([`crate::store::ClientState::last_events`] +
//! [`last_skill_test_difficulty`](crate::store::ClientState::last_skill_test_difficulty)).
//! Pairs with the Confirm button rendered by [`crate::input::AwaitingInputView`].

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::{ChaosToken, InvestigatorId, SkillKind, TokenResolution};
    use game_core::{Event, FailureReason};

    fn reveal(modifier: i8) -> Event {
        Event::ChaosTokenRevealed {
            token: ChaosToken::Numeric(modifier),
            resolution: TokenResolution::Modifier(modifier),
        }
    }

    #[test]
    fn summarizes_a_success() {
        let events = vec![
            reveal(1),
            Event::SkillTestSucceeded {
                investigator: InvestigatorId(1),
                skill: SkillKind::Willpower,
                margin: 2,
            },
        ];
        let s = summarize(&events, Some(3)).expect("a success summary");
        assert_eq!(s.difficulty, 3);
        assert_eq!(s.total, 5, "total = difficulty + margin");
        assert!(s.outcome.contains("Succeeded by 2"), "{}", s.outcome);
    }

    #[test]
    fn summarizes_a_failure() {
        let events = vec![
            reveal(-1),
            Event::SkillTestFailed {
                investigator: InvestigatorId(1),
                skill: SkillKind::Combat,
                reason: FailureReason::Total,
                by: 2,
            },
        ];
        let s = summarize(&events, Some(4)).expect("a failure summary");
        assert_eq!(s.total, 2, "total = difficulty - by");
        assert!(s.outcome.contains("Failed by 2"), "{}", s.outcome);
    }

    #[test]
    fn summarizes_an_autofail() {
        let events = vec![
            Event::ChaosTokenRevealed {
                token: ChaosToken::AutoFail,
                resolution: TokenResolution::AutoFail,
            },
            Event::SkillTestFailed {
                investigator: InvestigatorId(1),
                skill: SkillKind::Agility,
                reason: FailureReason::AutoFail,
                by: 3,
            },
        ];
        let s = summarize(&events, Some(3)).expect("an autofail summary");
        assert_eq!(s.total, 0, "auto-fail clamps total to 0");
        assert!(s.outcome.contains("auto-fail"), "notes the auto-fail: {}", s.outcome);
    }

    #[test]
    fn no_summary_without_resolution_events() {
        assert!(summarize(&[], Some(3)).is_none());
    }

    #[test]
    fn no_summary_without_known_difficulty() {
        let events = vec![Event::SkillTestSucceeded {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            margin: 0,
        }];
        assert!(summarize(&events, None).is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p web summarizes_a_success summarizes_a_failure summarizes_an_autofail no_summary_without_resolution_events no_summary_without_known_difficulty`
Expected: FAIL — `summarize` / `SkillTestSummary` not defined.

- [ ] **Step 3: Implement `SkillTestSummary` + `summarize`**

At the TOP of `crates/web/src/skill_test_result.rs` (above the test module), add:

```rust
use game_core::state::{ChaosToken, TokenResolution};
use game_core::{Event, FailureReason};
use leptos::prelude::*;

use crate::store::use_store;

/// The data the result panel renders: a display string for the drawn token, the
/// final total vs difficulty, and a player-facing outcome line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillTestSummary {
    pub token: String,
    pub total: i8,
    pub difficulty: i8,
    pub outcome: String,
}

/// Build a [`SkillTestSummary`] from a resolution event batch and the test's
/// difficulty, or `None` if the batch carries no skill-test result or the
/// difficulty is unknown. Pure — no DOM, unit-tested on native.
///
/// `total` is reconstructed from the logged margin: `difficulty + margin` on a
/// success, `difficulty - by` on a failure (an `AutoFail` reports `by =
/// difficulty`, so the total clamps to 0).
#[must_use]
pub fn summarize(events: &[Event], difficulty: Option<i8>) -> Option<SkillTestSummary> {
    let difficulty = difficulty?;
    let token = events.iter().find_map(|e| match e {
        Event::ChaosTokenRevealed { token, resolution } => Some(token_display(*token, *resolution)),
        _ => None,
    });
    for e in events {
        match e {
            Event::SkillTestSucceeded { margin, .. } => {
                return Some(SkillTestSummary {
                    token: token.unwrap_or_else(|| "—".to_string()),
                    total: difficulty.saturating_add(*margin),
                    difficulty,
                    outcome: format!("Succeeded by {margin}"),
                });
            }
            Event::SkillTestFailed { reason, by, .. } => {
                let note = if matches!(reason, FailureReason::AutoFail) {
                    " (auto-fail)"
                } else {
                    ""
                };
                return Some(SkillTestSummary {
                    token: token.unwrap_or_else(|| "—".to_string()),
                    total: difficulty.saturating_sub(*by),
                    difficulty,
                    outcome: format!("Failed by {by}{note}"),
                });
            }
            _ => {}
        }
    }
    None
}

/// A short display string for the drawn token and how it resolved (e.g.
/// `"+1"`, `"Skull (-2)"`, `"AutoFail (auto-fail)"`).
fn token_display(token: ChaosToken, resolution: TokenResolution) -> String {
    let suffix = match resolution {
        TokenResolution::Modifier(n) => format!("{n:+}"),
        TokenResolution::AutoFail => "auto-fail".to_string(),
        TokenResolution::ElderSign => "elder sign".to_string(),
        // `TokenResolution` is #[non_exhaustive]; a future kind gets a placeholder.
        _ => "?".to_string(),
    };
    match token {
        // A numeric token reads cleanly as just its signed value.
        ChaosToken::Numeric(n) => format!("{n:+}"),
        // `ChaosToken` is #[non_exhaustive]; render the symbol via Debug + suffix.
        other => format!("{other:?} ({suffix})"),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p web summarizes_a_success summarizes_a_failure summarizes_an_autofail no_summary_without_resolution_events no_summary_without_known_difficulty`
Expected: PASS (all five).

- [ ] **Step 5: Add the component**

In `crates/web/src/skill_test_result.rs`, add the component below `token_display` (above the test module):

```rust
/// Result panel for the just-resolved skill test. Renders nothing unless the
/// store's retained batch carries a skill-test result and a known difficulty
/// (i.e. exactly while the #478 acknowledge pause is live). Reads the store
/// reactively.
#[component]
pub fn SkillTestResultView() -> impl IntoView {
    let store = use_store();
    view! {
        {move || {
            let st = store.get();
            let Some(s) = summarize(&st.last_events, st.last_skill_test_difficulty) else {
                return ().into_any();
            };
            view! {
                <section class="skill-test-result">
                    <p class="str-token">"Chaos token: " {s.token}</p>
                    <p class="str-total">
                        "Total " {s.total} " vs difficulty " {s.difficulty}
                    </p>
                    <p class="str-outcome">{s.outcome}</p>
                </section>
            }
            .into_any()
        }}
    }
}
```

- [ ] **Step 6: Register the module and mount the view**

In `crates/web/src/lib.rs`, add alongside the other `pub mod` lines:

```rust
pub mod skill_test_result;
```

In `crates/web/src/app.rs`, mount the view in the existing wasm-only block — change the line:

```rust
                { view! { <crate::picker::PickerView/><crate::input::AwaitingInputView/> }.into_any() }
```

to:

```rust
                { view! {
                    <crate::picker::PickerView/>
                    <crate::skill_test_result::SkillTestResultView/>
                    <crate::input::AwaitingInputView/>
                }.into_any() }
```

- [ ] **Step 7: Write the failing wasm render test**

Create `crates/web/tests/skill_test_result.rs`:

```rust
//! Headless test for `SkillTestResultView` (#478): feed a `SkillTestStarted`
//! batch (captures difficulty) then a resolution batch (chaos token + outcome)
//! through the store, and assert the panel renders the token, total-vs-difficulty,
//! and outcome lines. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use game_core::state::{ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenResolution};
use game_core::test_support::fixtures::test_investigator;
use game_core::{EngineOutcome, Event};
use leptos::prelude::*;
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::skill_test_result::SkillTestResultView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

fn base_game() -> game_core::state::GameState {
    GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build()
}

fn last_section() -> Option<web_sys::Element> {
    let secs = leptos::prelude::document()
        .query_selector_all(".skill-test-result")
        .expect("query");
    let n = secs.length();
    if n == 0 {
        return None;
    }
    Some(
        secs.item(n - 1)
            .expect("present")
            .dyn_into::<web_sys::Element>()
            .expect("Element"),
    )
}

#[wasm_bindgen_test]
async fn renders_token_total_and_outcome_after_resolution() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <SkillTestResultView/> }
    });

    // Batch 1: the test started at difficulty 3 (captures difficulty).
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Applied {
                state: Box::new(base_game()),
                events: vec![Event::SkillTestStarted {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Willpower,
                    difficulty: 3,
                }],
                outcome: EngineOutcome::Done,
            },
        );
    });
    leptos::task::tick().await;

    // Batch 2: resolution — +1 token, succeeded by 2 (total 5).
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Applied {
                state: Box::new(base_game()),
                events: vec![
                    Event::ChaosTokenRevealed {
                        token: ChaosToken::Numeric(1),
                        resolution: TokenResolution::Modifier(1),
                    },
                    Event::SkillTestSucceeded {
                        investigator: InvestigatorId(1),
                        skill: SkillKind::Willpower,
                        margin: 2,
                    },
                ],
                outcome: EngineOutcome::Done,
            },
        );
    });
    leptos::task::tick().await;

    let section = last_section().expect("the result panel renders after resolution");
    let text = section.text_content().unwrap_or_default();
    assert!(text.contains("Chaos token"), "shows the token line: {text}");
    assert!(text.contains("Total 5"), "shows total: {text}");
    assert!(text.contains("difficulty 3"), "shows difficulty: {text}");
    assert!(text.contains("Succeeded by 2"), "shows outcome: {text}");
}

#[wasm_bindgen_test]
async fn renders_nothing_before_any_resolution() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <SkillTestResultView/> }
    });
    leptos::task::tick().await;
    // No batch fed: no panel. (Other tests on the page may have rendered one;
    // assert on a *fresh* store by checking this mount produced no new section
    // beyond what its own empty state yields — the empty store yields None.)
    let st = store.get_untracked();
    assert!(
        web::skill_test_result::summarize(&st.last_events, st.last_skill_test_difficulty).is_none(),
        "an empty store yields no summary"
    );
}
```

- [ ] **Step 8: Run the wasm test to verify it fails, then passes**

Run: `wasm-pack test --headless --firefox crates/web -- --test skill_test_result`
Expected: with Steps 5–6 already applied, PASS. (If you reach this step before Steps 5–6, it FAILs to compile — `SkillTestResultView` not found — which is the expected red.)

- [ ] **Step 9: Run native + wasm web suites**

Run: `cargo test -p web` then `wasm-pack test --headless --firefox crates/web`
Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add crates/web/src/skill_test_result.rs crates/web/src/lib.rs crates/web/src/app.rs crates/web/tests/skill_test_result.rs
git commit -m "web: skill-test result panel rendered from retained events (#478)"
```

---

### Task 6: Server — enable interactive acknowledgment for human play

**Files:**
- Modify: `crates/server/src/session.rs` (`GameSession::create`, ~line 75)
- Test: `crates/server/tests/game_session.rs`

**Interfaces:**
- Consumes: `GameState.interactive_acknowledge` (Task 1).
- Produces: every server-created game has `state.interactive_acknowledge == true`.

- [ ] **Step 1: Write the failing test**

In `crates/server/tests/game_session.rs`, add (it already has `install_registry`, `memory_pool`, `roster`, `TEST_SCENARIO_ID`):

```rust
#[tokio::test]
async fn create_enables_interactive_acknowledge() {
    install_registry();
    let pool = memory_pool().await;
    let session = GameSession::create(
        pool,
        "ack",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
    )
    .await
    .expect("create");
    assert!(
        session.state.interactive_acknowledge,
        "human-play sessions pause to acknowledge skill-test results (#478)"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p server create_enables_interactive_acknowledge`
Expected: FAIL — `interactive_acknowledge` is `false` (the engine default).

- [ ] **Step 3: Flip the flag at game creation**

In `crates/server/src/session.rs`, in `GameSession::create`, change the setup line (~line 75) from:

```rust
        let setup = (module.setup)();
        let result = game_core::seat_and_open(setup, &roster);
```

to:

```rust
        let mut setup = (module.setup)();
        // Human play surfaces skill-test results with a Confirm-to-dismiss step
        // (#478); the engine gates that pause on this flag (default off for tests
        // and non-interactive consumers). The flag persists through seating.
        setup.interactive_acknowledge = true;
        let result = game_core::seat_and_open(setup, &roster);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p server create_enables_interactive_acknowledge`
Expected: PASS.

- [ ] **Step 5: Run the server session suite**

Run: `RUSTFLAGS="-D warnings" cargo test -p server`
Expected: PASS (existing session tests use the same `create` path; seating still completes — the flag does not affect mulligan/seating).

- [ ] **Step 6: Commit**

```bash
git add crates/server/src/session.rs crates/server/tests/game_session.rs
git commit -m "server: enable interactive skill-test acknowledgment for new games (#478)"
```

---

### Final: full CI gauntlet + phase doc

- [ ] **Run the complete gauntlet** (all seven jobs, from Global Constraints). Fix any `fmt`/`clippy`/`doc` findings.
- [ ] **Update the phase doc** per `docs/phases/README.md` ("Maintaining these docs") — issue #478 is `ui` / `p2-later`; locate its phase doc (likely the phase-6/iteration doc that tracks the web client), move #478 to the Closed table, flip its Arc/Ordering row to `✅ PR #N`, and add a **Decisions made** entry ONLY if load-bearing for a future PR (candidate: "skill-test acknowledgment is a *gated* engine pause via `GameState.interactive_acknowledge`, not always-on — non-interactive consumers stay churn-free"). Do this as the FINAL commit, only once the PR is open and CI is green.

## Notes for the implementer

- **Why gated, not always-on:** the acknowledge makes no game decision, so baking it unconditionally into the kernel would force every non-interactive consumer (tests, future headless/AI) through a meaningless pause and churn ~20+ skill-test tests. The flag keeps the kernel honest and the suite quiet. See the design doc: `docs/superpowers/specs/2026-06-26-478-skill-test-result-feedback-design.md`.
- **Timing nuance:** the acknowledge sits after `DetermineOutcome` (which emits the result events *and* fires the `SkillTestResolved` timing point) and before `FireOnCommit`. A reaction to the result (rare; e.g. Dr. Milan) therefore resolves before the acknowledge. This is rules-neutral (the acknowledge is cosmetic) and the lowest-risk insertion; refine only if it proves confusing.
- **No new engine display state:** the engine's Confirm prompt is a generic string; the panel's content comes entirely from the events the client already receives plus the retained difficulty.
