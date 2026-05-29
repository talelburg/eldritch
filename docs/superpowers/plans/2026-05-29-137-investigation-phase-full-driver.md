# Investigation Phase Full Driver (#137) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Investigation phase a full named-step driver matching Mythos/Upkeep/Enemy — `investigation_phase` (2.1) → `InvestigationBegins` window → `begin_investigator_turn` (2.2) → `InvestigatorTurnBegins` window → player actions (2.2.1) → `end_turn` (2.2.2) → `investigation_phase_end` (2.3) — with the phase beginning after the mulligan setup window closes.

**Architecture:** Event-sourced engine. The Investigation phase's *boundaries* become window-driven (via `open_fast_window` + `run_window_continuation`), while its *action-taking middle* stays player-driven (`Investigate`/`Move`/.../`EndTurn`). Round 1 enters via a mulligan-completion kickoff (setup has no action windows, per Rules Reference p.27); round ≥2 enters via `step_phase`. Once Investigation owns its `PhaseEnded`, all four phases own theirs and `step_phase`'s conditional `PhaseEnded` emit is deleted.

**Tech Stack:** Rust, `cargo test -p game-core`. Design spec: `docs/superpowers/specs/2026-05-29-137-investigation-phase-full-driver-design.md`.

**CI gauntlet (run before any commit you intend to push):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```

**Branch:** `engine/investigation-phase-driver` (already created; spec committed there).

---

## File structure

| File | Responsibility | Change |
|---|---|---|
| `crates/game-core/src/state/game_state.rs` | `WindowKind` enum + serde tests | Add 2 variants + 2 round-trip tests |
| `crates/game-core/src/engine/dispatch.rs` | phase drivers, windows, `end_turn`, `step_phase`, `apply_player_action` | Core of the change (new fns + rewires + tests) |
| `docs/phases/phase-4-scenario-plumbing.md` | phase tracking | Final commit only (Task 6) |

All new functions live in `dispatch.rs` alongside the existing phase drivers (`mythos_phase`, `upkeep_phase`, `enemy_phase`), following that file's established convention.

---

## Task 1: WindowKind variants + driver helpers + continuation arms (no behavior change)

Adds the two window variants and wires the `run_window_continuation` / `trigger_matches` matches so the crate compiles. Introduces `begin_investigator_turn` (referenced by the new continuation arm, so no dead-code warning). Nothing opens these windows yet, so existing behavior and tests are unchanged.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (enum + serde tests)
- Modify: `crates/game-core/src/engine/dispatch.rs` (`trigger_matches`, `run_window_continuation`, new `begin_investigator_turn`)

- [ ] **Step 1: Add the two `WindowKind` variants**

In `crates/game-core/src/state/game_state.rs`, immediately after the `AfterAllInvestigatorsAttacked,` variant (the last variant, ~line 544), add:

```rust
    /// The player window between Rules Reference p.24 step 2.1
    /// (Investigation phase begins) and step 2.2 (the first
    /// investigator's turn begins). Bare variant — no `EventPattern`
    /// matches it today; it exists so the printed timing point is
    /// addressable and so step 2.2's rotation runs in this window's
    /// continuation (preserving the printed 2.1 → window → 2.2 order).
    InvestigationBegins,
    /// The player window opened at the start of each investigator's
    /// turn (Rules Reference p.24 step 2.2, the "previous player window"
    /// that actions return to during step 2.2.1). Bare variant. One per
    /// investigator turn. Continuation is a no-op: the engine then waits
    /// for the active investigator's player-driven actions.
    InvestigatorTurnBegins,
```

- [ ] **Step 2: Add serde round-trip tests for both variants**

In `game_state.rs`, in the `open_window_tests` module, after `after_all_investigators_attacked_window_kind_serde_roundtrip` (~line 717), add:

```rust
    #[test]
    fn investigation_begins_window_kind_serde_roundtrip() {
        let kind = WindowKind::InvestigationBegins;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn investigator_turn_begins_window_kind_serde_roundtrip() {
        let kind = WindowKind::InvestigatorTurnBegins;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }
```

- [ ] **Step 3: Run the new serde tests — expect compile failure first**

Run: `cargo test -p game-core investigation_begins_window_kind_serde_roundtrip 2>&1 | head -30`
Expected: **compile error** — `trigger_matches` and `run_window_continuation` in `dispatch.rs` have non-exhaustive matches now that two variants were added. This is expected; Steps 4–6 fix it.

- [ ] **Step 4: Extend the `trigger_matches` no-pattern arm**

In `dispatch.rs`, in `trigger_matches` (~line 1776), add the two new variants to the timing-only window list:

```rust
        (
            WindowKind::BetweenPhases { .. }
            | WindowKind::AfterEnemyDefeated { .. }
            | WindowKind::MythosAfterDraws
            | WindowKind::UpkeepBegins
            | WindowKind::BeforeInvestigatorAttacked
            | WindowKind::AfterAllInvestigatorsAttacked
            | WindowKind::InvestigationBegins
            | WindowKind::InvestigatorTurnBegins,
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned,
        ) => false,
```

- [ ] **Step 5: Add `begin_investigator_turn`**

In `dispatch.rs`, directly after `investigation_phase` (ends ~line 855), add:

```rust
/// 2.2 Next investigator's turn begins. Rotates the active cursor to
/// `who` (the chosen/default investigator) and opens the post-2.2
/// player window. Called from the `InvestigationBegins` continuation
/// (first turn of the phase) and from `end_turn` (each subsequent turn
/// — the rules' "return to 2.2"). Step 2.2.1 (the active investigator's
/// actions) follows as player-driven inputs while `InvestigatorTurnBegins`
/// is the "previous player window."
///
/// `who` must be an `Active` investigator in `turn_order`; callers
/// resolve it via `first_active_investigator` / `next_active_investigator_after`.
fn begin_investigator_turn(state: &mut GameState, events: &mut Vec<Event>, who: InvestigatorId) {
    rotate_to_active(state, events, who);
    open_fast_window(state, events, WindowKind::InvestigatorTurnBegins);
}
```

- [ ] **Step 6: Add the two `run_window_continuation` arms**

In `dispatch.rs`, in `run_window_continuation`'s match (before the closing `}` of the match, after the `AfterAllInvestigatorsAttacked` arm ~line 4046), add:

```rust
        WindowKind::InvestigationBegins => {
            // Post-2.1 window closed; start the first turn (step 2.2).
            // No skill-test-in-flight guard: this runs at phase start
            // (no test can be in flight) and does not transition phase.
            match first_active_investigator(state) {
                Some(id) => begin_investigator_turn(state, events, id),
                None => {
                    // PARK: no active investigator can take a turn.
                    // TODO(#144): Rules Reference p.10 step 6 — with no
                    // remaining players the scenario ends.
                    // check_all_defeated already emits
                    // AllInvestigatorsDefeated, but the scenario-end
                    // consequence is unwired. Until it lands, park here
                    // (mirrors prior behavior). Auto-advancing would loop
                    // the round forever — every other phase auto-skips
                    // with no active investigators, so Investigation is
                    // the cascade's only natural pause point.
                }
            }
        }
        WindowKind::InvestigatorTurnBegins => {
            // 2.2.1 The active investigator now takes actions
            // (Investigate / Move / Fight / Evade / PlayCard / Draw /
            // ActivateAbility) as player-driven inputs, then ends the
            // turn via EndTurn (2.2.2). No continuation work — the engine
            // waits. The per-action "return to the previous player
            // window" re-open (Rules Reference p.24 2.2.1) is deferred
            // to #146.
        }
```

- [ ] **Step 7: Run the gauntlet — expect green, no behavior change**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core 2>&1 | tail -20`
Expected: PASS, including the two new serde tests. No existing test changes (the new windows are never opened yet). Then `cargo clippy --all-targets --all-features -- -D warnings` — expect no warnings (`begin_investigator_turn` is referenced by the `InvestigationBegins` arm).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch.rs
git commit -m "engine: add Investigation window variants + begin_investigator_turn

Adds InvestigationBegins / InvestigatorTurnBegins WindowKind variants
(bare, mirroring #71's shape), their serde round-trips, the
trigger_matches no-pattern arm, begin_investigator_turn, and the two
run_window_continuation arms. No behavior change yet — nothing opens
these windows until Task 2.

Refs #137.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Phase-begins-after-setup — rewire `investigation_phase`, `start_scenario`, mulligan kickoff

`investigation_phase` stops rotating inline and instead opens `InvestigationBegins` (rotation now happens in its continuation, preserving 2.1 → window → 2.2). `start_scenario` stops kicking off the phase during setup; the round-1 kickoff moves to the mulligan-completion site ("the game begins").

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — `investigation_phase` (~831), `start_scenario` (~748–751), `apply_player_action` (~169), and tests (~5051–5147)

- [ ] **Step 1: Update the mulligan-completion kickoff test (write the failing test)**

In `dispatch.rs`'s test module (where the other `apply_player_action` / mulligan tests live; search `mod tests` near `apply_player_action` or add to the dispatch test module), add:

```rust
    #[test]
    fn mulligan_completion_kicks_off_investigation_phase() {
        // After the last investigator mulligans, setup ends and the
        // Investigation phase begins (Rules Reference p.27: no action
        // windows during setup; the game begins after mulligans).
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.active_investigator = None;
        state.mulligan_window = true;
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .mulligan_used = false;

        let mut events = Vec::new();
        let outcome = apply_player_action(
            &mut state,
            &mut events,
            &PlayerAction::Mulligan {
                investigator: InvestigatorId(1),
                indices_to_redraw: vec![],
            },
        );

        assert!(matches!(outcome, EngineOutcome::Done));
        assert!(
            !state.mulligan_window,
            "mulligan window closes once every investigator has mulliganed"
        );
        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(1)),
            "Investigation phase kicks off and rotates to the lead after mulligan completes"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )),
            "PhaseStarted(Investigation) fires at mulligan completion, not during StartScenario"
        );
    }
```

- [ ] **Step 2: Run it — expect failure**

Run: `cargo test -p game-core mulligan_completion_kicks_off_investigation_phase -- --nocapture 2>&1 | tail -20`
Expected: FAIL — today the mulligan-completion block only flips `mulligan_window`; no `PhaseStarted(Investigation)` is emitted there and `active_investigator` stays `None`.

- [ ] **Step 3: Add the kickoff at the mulligan-completion site**

In `dispatch.rs`'s `apply_player_action`, inside the `if` block that flips the window (currently lines 165–170), after `state.mulligan_window = false;` add the kickoff:

```rust
    if matches!(outcome, EngineOutcome::Done)
        && matches!(action, PlayerAction::Mulligan { .. })
        && state.investigators.values().all(|inv| inv.mulligan_used)
    {
        state.mulligan_window = false;
        // Setup complete — "the game begins" (Rules Reference p.27).
        // Round 1 skips the Mythos phase (p.24), so the first phase to
        // begin is Investigation. Kick off its driver HERE, not in
        // start_scenario: setup has "no action windows" (p.27), so the
        // post-2.1 player window must not open until mulligans are done.
        investigation_phase(state, events);
    }
```

- [ ] **Step 4: Rewire `investigation_phase` to open the post-2.1 window**

In `dispatch.rs`, replace the body of `investigation_phase` (currently emits `PhaseStarted` then rotates inline, ~lines 831–855) with:

```rust
fn investigation_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 2.1 Investigation phase begins.
    events.push(Event::PhaseStarted {
        phase: Phase::Investigation,
    });
    // PLAYER WINDOW (post-2.1). Rotation to the first investigator
    // (step 2.2) runs in this window's continuation
    // (`run_window_continuation` → `InvestigationBegins`), so the printed
    // order 2.1 → window → 2.2 holds. Auto-skips inline when nothing is
    // Fast-eligible, so single-investigator entry still lands the lead
    // active within the same apply() call.
    open_fast_window(state, events, WindowKind::InvestigationBegins);
}
```

(Update the doc-comment above `investigation_phase` — the "Rotation policy (Phase 4)" paragraph — to say rotation now happens in the `InvestigationBegins` continuation via `begin_investigator_turn`, lead-first by default, with explicit player choice deferred to #146.)

- [ ] **Step 5: Stop `start_scenario` from kicking off the phase during setup**

In `dispatch.rs`'s `start_scenario` (~748–751), remove the `investigation_phase(state, events);` call and its preceding comment. The function now ends:

```rust
    // Round-1 action seed: round 1 skips Mythos, so there's no Upkeep 4.2
    // to grant the first round's actions. Every Active investigator → ACTIONS_PER_TURN.
    reset_actions(state, events);

    // NOTE: the Investigation phase is NOT begun here. Setup has no
    // action windows (Rules Reference p.27); the phase begins after the
    // mulligan window closes — see the kickoff in apply_player_action.
    EngineOutcome::Done
}
```

- [ ] **Step 6: Update the three `investigation_phase` unit tests**

The existing tests call `investigation_phase` directly. With the window in place, in a unit test (no card registry) `any_fast_play_eligible` returns `false`, so `InvestigationBegins` auto-skips inline → continuation rotates → `InvestigatorTurnBegins` auto-skips. Net: `active_investigator` still lands on the lead, plus window events. Update assertions:

Replace `investigation_phase_emits_phase_started_and_rotates_to_lead`'s body assertions to keep the existing `active_investigator == Some(InvestigatorId(1))` and `PhaseStarted` checks (they still hold), and add:

```rust
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::WindowOpened {
                    kind: WindowKind::InvestigationBegins
                }
            )),
            "investigation_phase opens the post-2.1 InvestigationBegins window"
        );
```

For `investigation_phase_with_empty_turn_order_is_noop_rotate`, rename to `investigation_phase_with_empty_turn_order_parks` and assert the park (no advance, no PhaseEnded):

```rust
    #[test]
    fn investigation_phase_with_empty_turn_order_parks() {
        // Degenerate (cannot occur in real gameplay): no investigators.
        // The InvestigationBegins continuation finds no active
        // investigator and PARKS — active stays None, no PhaseEnded, no
        // advance. Locks in the cascade-breaker behavior (see spec
        // "All-eliminated / no-active-investigator handling").
        let mut state = TestGame::default().with_phase(Phase::Mythos).build();
        state.turn_order.clear();
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut state, &mut events);

        assert_eq!(state.active_investigator, None, "no investigator to rotate to");
        assert_eq!(state.phase, Phase::Investigation, "phase must not advance");
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "parking must NOT end the phase (auto-advancing would loop the round)"
        );
    }
```

`investigation_phase_skips_defeated_lead_and_picks_first_active` keeps its assertion (`active_investigator == Some(InvestigatorId(2))`) — still holds via the continuation. No change needed beyond confirming it passes.

- [ ] **Step 7: Run the affected tests**

Run: `cargo test -p game-core investigation_phase 2>&1 | tail -20` and `cargo test -p game-core mulligan_completion 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 8: Run the full game-core suite — fix any start_scenario tests**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core 2>&1 | tail -30`
Expected: some existing `start_scenario` / round-cycle tests may fail because `StartScenario` no longer emits `PhaseStarted(Investigation)` and no longer sets `active_investigator`. For each failure, update the assertion to the new reality: `StartScenario` emits `ScenarioStarted` + deck/hand events and opens the mulligan window; `PhaseStarted(Investigation)` and the lead becoming active now happen at mulligan completion. Do not change production code to satisfy a stale assertion — verify each is a timing-shift, not a regression.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: Investigation phase begins after mulligan setup completes

investigation_phase now opens the post-2.1 InvestigationBegins window
(rotation moved into its continuation). start_scenario no longer begins
the phase during setup; the round-1 kickoff moves to the
mulligan-completion site, per Rules Reference p.27 (no action windows
during setup; the game begins after mulligans).

Refs #137.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Rewire `end_turn` + add `investigation_phase_end` + delete `step_phase`'s PhaseEnded emit

These are atomic: once `end_turn` ends the phase via `investigation_phase_end` (which emits `PhaseEnded(Investigation)`), `step_phase` must stop emitting it too, or a double `PhaseEnded` fires.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — `end_turn` (~789–815), new `investigation_phase_end`, `step_phase` (~941–968), and tests

- [ ] **Step 1: Write the failing test for `investigation_phase_end` ownership**

In the dispatch test module, add:

```rust
    #[test]
    fn end_turn_for_last_investigator_ends_phase_and_steps_to_enemy() {
        // Single investigator ends their turn: TurnEnded (2.2.2), then
        // PhaseEnded(Investigation) (2.3) from investigation_phase_end,
        // then the cascade enters the Enemy phase.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        let mut events = Vec::new();
        let outcome = end_turn(&mut state, &mut events);

        assert!(matches!(outcome, EngineOutcome::Done));
        assert!(
            events.iter().any(|e| matches!(e, Event::TurnEnded { investigator } if *investigator == InvestigatorId(1))),
            "step 2.2.2 emits TurnEnded"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "step 2.3 emits PhaseEnded(Investigation) via investigation_phase_end"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::PhaseEnded { phase: Phase::Investigation }))
                .count(),
            1,
            "exactly one PhaseEnded(Investigation) — step_phase must not also emit it"
        );
        assert_ne!(state.phase, Phase::Investigation, "phase advanced past Investigation");
    }

    #[test]
    fn end_turn_rotates_to_next_active_and_opens_turn_window() {
        // Two investigators: ending #1's turn returns to 2.2 for #2 and
        // opens the InvestigatorTurnBegins window for them.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];

        let mut events = Vec::new();
        let outcome = end_turn(&mut state, &mut events);

        assert!(matches!(outcome, EngineOutcome::Done));
        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(2)),
            "rotates to the next active investigator (return to 2.2)"
        );
        assert_eq!(state.phase, Phase::Investigation, "phase does not end mid-round");
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "phase must not end while an investigator is still to take a turn"
        );
    }
```

- [ ] **Step 2: Run them — expect failure**

Run: `cargo test -p game-core end_turn_for_last_investigator_ends_phase 2>&1 | tail -20`
Expected: FAIL — `investigation_phase_end` does not exist; today `end_turn`'s terminal branch calls `step_phase` directly and `PhaseEnded(Investigation)` comes from `step_phase`'s fallback.

- [ ] **Step 3: Add `investigation_phase_end`**

In `dispatch.rs`, after `begin_investigator_turn` (added in Task 1), add:

```rust
/// 2.3 Investigation phase ends. Owns the `PhaseEnded(Investigation)`
/// emit — lifted out of `step_phase`, mirroring `mythos_phase_end` /
/// `enemy_phase_end` / `upkeep_phase_end` — then transitions to the
/// Enemy phase. Called only from `end_turn`'s terminal branch (the last
/// investigator has taken a turn this round).
fn investigation_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    events.push(Event::PhaseEnded {
        phase: Phase::Investigation,
    });
    step_phase(state, events); // Investigation → Enemy; calls enemy_phase
}
```

- [ ] **Step 4: Rewire `end_turn`'s terminal/rotation branch**

In `dispatch.rs`'s `end_turn`, replace the rotation/step block (currently ~789–815, the `let next = ...` through the `if let Some(next_id) ... else ... step_phase(...)`) with:

```rust
    // 2.2.2 decision: "return to 2.2" for the next investigator, or
    // proceed to 2.3. next_active_investigator_after skips eliminated
    // investigators (Rules Reference p.10) — the same shared helper the
    // Enemy phase uses.
    match next_active_investigator_after(state, active_id) {
        Some(next_id) => begin_investigator_turn(state, events, next_id),
        None => {
            state.active_investigator = None;
            investigation_phase_end(state, events); // 2.3 → Enemy
        }
    }

    EngineOutcome::Done
}
```

Keep the existing validation prologue and the action-drain + `TurnEnded` emit (steps before this block) unchanged.

- [ ] **Step 5: Delete `step_phase`'s conditional `PhaseEnded` emit**

In `dispatch.rs`'s `step_phase` (~941), delete the block:

```rust
    // PhaseEnded suppressed when the from-phase's *_end helper owns it.
    if from != Phase::Mythos && from != Phase::Upkeep && from != Phase::Enemy {
        events.push(Event::PhaseEnded { phase: from });
    }
```

`step_phase` now emits no `PhaseEnded` — every phase's `*_end` helper (`mythos_phase_end`, `investigation_phase_end`, `enemy_phase_end`, `upkeep_phase_end`) owns its own. Rewrite the `step_phase` doc-comment "PhaseEnded suppression invariant" paragraph to state this simpler invariant.

- [ ] **Step 6: Run the new tests + the suite**

Run: `cargo test -p game-core end_turn 2>&1 | tail -20`
Expected: PASS.
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core 2>&1 | tail -30`
Expected: some round-cycle tests (e.g. `end_turn_cascades_through_upkeep_to_mythos_draw_pending`, `step_phase_from_enemy_does_not_emit_phase_ended_enemy`) may need assertion updates because (a) `end_turn` now opens `InvestigatorTurnBegins` on rotation and (b) `PhaseEnded(Investigation)` now originates from `investigation_phase_end`. Update each to the new event stream — confirm each is a sourcing/extra-window change, not a missing/duplicate transition.

- [ ] **Step 7: Add a step_phase no-emit regression test (if not already covered)**

In the dispatch test module, add:

```rust
    #[test]
    fn step_phase_emits_no_phase_ended() {
        // step_phase no longer emits PhaseEnded for any phase — each
        // phase's *_end helper owns it. Direct Investigation→Enemy step.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        let mut events = Vec::new();
        step_phase(&mut state, &mut events);

        assert!(
            !events.iter().any(|e| matches!(e, Event::PhaseEnded { .. })),
            "step_phase must emit no PhaseEnded — the *_end helpers own it"
        );
    }
```

Run: `cargo test -p game-core step_phase_emits_no_phase_ended 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: Investigation owns PhaseEnded via investigation_phase_end

end_turn rotates via begin_investigator_turn (return to 2.2) or ends the
phase via investigation_phase_end (2.3 → Enemy). With all four phases
owning their PhaseEnded, step_phase's conditional PhaseEnded emit is
deleted.

Refs #137.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Full-round cascade + replay verification

Confirms the end-to-end driver behaves and replays deterministically. Mostly assertion updates to existing round-cycle tests plus one focused new test.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (round-cycle tests) and/or `crates/game-core/src/engine/mod.rs` (replay tests)

- [ ] **Step 1: Add a window-ordering test for the normal phase entry**

In the dispatch test module:

```rust
    #[test]
    fn investigation_entry_emits_phase_started_then_windows_then_lead_active() {
        // Round ≥2 entry via step_phase (Mythos→Investigation) auto-skips
        // both windows (no registry → nothing Fast-eligible) and lands
        // the lead active, with no PhaseEnded yet.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.active_investigator = None;

        let mut events = Vec::new();
        step_phase(&mut state, &mut events); // Mythos→Investigation

        assert_eq!(state.phase, Phase::Investigation);
        assert_eq!(state.active_investigator, Some(InvestigatorId(1)));
        assert!(events.iter().any(|e| matches!(e, Event::PhaseStarted { phase: Phase::Investigation })));
        assert!(events.iter().any(|e| matches!(e, Event::WindowOpened { kind: WindowKind::InvestigationBegins })));
        assert!(events.iter().any(|e| matches!(e, Event::WindowOpened { kind: WindowKind::InvestigatorTurnBegins })));
        assert!(!events.iter().any(|e| matches!(e, Event::PhaseEnded { phase: Phase::Investigation })));
    }
```

Run: `cargo test -p game-core investigation_entry_emits_phase_started_then_windows 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 2: Verify/repair the existing replay test**

Locate the action-log replay test in `crates/game-core/src/engine/mod.rs` (search `replay`). Run:
`RUSTFLAGS="-D warnings" cargo test -p game-core replay 2>&1 | tail -20`
Expected: PASS. The driver changes are deterministic (no new RNG; windows auto-skip identically on replay). If a replay test hard-codes an event stream that shifted, update the expected stream to match — the produced stream must be reproducible, which is the property under test.

- [ ] **Step 3: Run the entire game-core suite once more**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core 2>&1 | tail -30`
Expected: PASS.

- [ ] **Step 4: Commit (only if this task changed files)**

```bash
git add crates/game-core/src/engine/dispatch.rs crates/game-core/src/engine/mod.rs
git commit -m "test: full-round cascade + window-ordering coverage for Investigation driver

Refs #137.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Full CI gauntlet across the workspace

**Files:** none (verification only)

- [ ] **Step 1: Run all five CI jobs locally**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```

Expected: all green. In particular, `cargo test --all` includes the `crates/cards/tests/` and `crates/scenarios/tests/` integration binaries — the synthetic-scenario round-cycle test may exercise the new driver. Update any integration assertion that shifted due to the mulligan-kickoff timing or the added window events; confirm each is a timing/sourcing change, not a regression.

- [ ] **Step 2: Fix `fmt` / `clippy` / `doc` issues if any, then re-run**

If `cargo fmt --check` fails, run `cargo fmt`. If `cargo doc` flags a broken intra-doc link to the new variants/fns, fix the link. Re-run the failing job until green. Commit any fixes:

```bash
git add -A
git commit -m "engine: fmt/clippy/doc fixes for Investigation driver

Refs #137.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Phase doc update (final commit before PR-ready)

Per `CLAUDE.md`: the phase doc is touched **once**, as the final commit, only when the PR is ready. Do this after the gauntlet is green and the PR number is known (or immediately before opening the PR).

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

- [ ] **Step 1: Move #137 to Closed and flip the Arc row**

- Move `#137` from the **Issues** (open) table to the **Closed** table with its PR number and a one-line note ("Investigation full driver: named-step `investigation_phase` / `begin_investigator_turn` / `investigation_phase_end` + `InvestigationBegins`/`InvestigatorTurnBegins` windows; phase-begins-after-mulligan kickoff; `step_phase` PhaseEnded emit deleted").
- Flip the Ordering (Shape B) table's slot-9 row to `✅ PR #N`.
- Bump the closed/open counts in the Status section.

- [ ] **Step 2: Add Decisions entries (only the load-bearing ones)**

Add entries for choices a future PR-author would otherwise re-litigate:
- **Investigation phase begins at mulligan completion, not in `start_scenario` (#137).** Round-1 `PhaseStarted(Investigation)` + lead-active now fire when the mulligan window closes (`apply_player_action`), not during `StartScenario`. Rules Reference p.27: no action windows during setup; the game begins after mulligans. Future setup-adjacent work inherits this kickoff point. (Note the sibling #147 will swap the kickoff trigger from `mulligan_window` to the `mulligan_pending` cursor.)
- **`step_phase` emits no `PhaseEnded` (#137).** All four phases' `*_end` helpers own their boundary emit; `step_phase`'s conditional emit was deleted. A new phase driver must add its own `*_end` helper.
- **No-active Investigation parks; all-eliminated scenario-end is #144 (#137).** The `InvestigationBegins` continuation parks when no investigator is active (cascade-breaker; auto-advancing loops the round). Rules Reference p.10 step 6 (scenario ends with no remaining players) is unwired beyond `check_all_defeated`'s event; it lands with the elimination flow (#144).

- [ ] **Step 3: Note the filed follow-ups + settle open questions**

- Add #146 (Phase 8 — Investigation turn-order choice + 2.2.1 between-action windows) and #147 (Phase 4 — mulligan player-order cursor) to the appropriate follow-up/unmilestoned lists.
- Remove any Open question #137 settled.

- [ ] **Step 4: Commit**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "docs: phase-4 doc update for Investigation full driver (#137)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-review checklist (completed by plan author)

- **Spec coverage:** named-step driver (Task 1+2+3), `investigation_phase_end` + `step_phase` deletion (Task 3), `InvestigationBegins`/`InvestigatorTurnBegins` windows (Task 1), phase-begins-after-mulligan (Task 2), no-active park + TODO(#144) (Task 1 arm), follow-ups #146/#147 (Task 6 doc). Deferred items (turn-order choice, between-action windows, mulligan cursor) correctly out of scope.
- **Type/name consistency:** `begin_investigator_turn(state, events, who: InvestigatorId)`, `investigation_phase_end(state, events)`, `WindowKind::InvestigationBegins` / `WindowKind::InvestigatorTurnBegins`, `Event::WindowOpened { kind }`, `Event::PhaseEnded { phase }`, `next_active_investigator_after(state, current)`, `first_active_investigator(state)` — all match the existing signatures verified in dispatch.rs/game_state.rs.
- **No placeholders:** every code step shows complete code; test-update steps name the specific assertion shift and the verify-don't-mask rule.
- **Atomicity flagged:** Task 3 bundles `end_turn` + `investigation_phase_end` + `step_phase` deletion because splitting them produces a transient double `PhaseEnded`.
