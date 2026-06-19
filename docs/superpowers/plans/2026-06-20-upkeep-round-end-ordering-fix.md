# Upkeep round-end `when→at` ordering fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Upkeep step-4.6 round-end fire act 01109's "**when** the round ends" clue-spend window *before* agenda 01107's "**at** the end of the round" doom, per the Rules Reference "At" entry (`when → at → after`).

**Architecture:** Today `upkeep_phase_end` fires the `RoundEnded` Forced abilities (the agenda's `at` doom) and *then* opens the act's `when` window — inverted. The fix rethreads the two independently-suspendable round-end steps: open the `when` window first; run the `at` `RoundEnded` Forced abilities + teardown (lasting-effect expiry + Upkeep→Mythos) on the window's *resume* (or inline when no window opens). This is the standalone pre-req (spec §G / spec step 0) for the #393 unified-control-flow arc; it must land before the Upkeep-anchor slice so that slice stays behaviour-preserving.

**Tech Stack:** Rust; `game-core` engine (event-sourced `apply`); `cards` integration tests with the real card registry; `cargo test` / clippy / fmt / doc gauntlet.

## Global Constraints

- **Handler contract:** validate-first / mutate-second — on any `Rejected`, state and events unchanged (dispatch/mod.rs apply loop clears events on `Rejected`).
- **Card/rules text:** never paraphrase from memory; the load-bearing rule here is the RR **"At"** entry — *"abilities [using] 'at' … such as 'at the end of the round' … trigger in between any 'when…' abilities and any 'after…' abilities with the same triggering condition."* Quote verbatim in doc-comments where it shapes behaviour.
- **CI (all must pass with strict flags):** `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- **Commit subject style:** `engine: <description>`; body explains *why* and ends with `Closes #NN.`
- **Branch:** `engine/upkeep-round-end-ordering` (one branch for this issue).
- **Generated files:** never hand-edit `crates/cards/src/generated/cards.rs`.

---

## Prep (not code)

- [ ] **File the bug issue.** Title: `[engine] Upkeep round-end ordering: act 01109 "when" window must precede agenda 01107 "at" doom`. Labels: `engine`, `p1-next`. Body: summarize the RR `when→at` rule, the two cards (01109 act / 01107 agenda), the inverted ordering in `upkeep_phase_end`, and that it's the spec §G pre-req for #393. Note its number as `#NN` for the commit's `Closes #NN.`

---

## Task 1: Rethread Upkeep round-end so the act `when` window precedes the agenda `at` doom

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (`upkeep_phase_end` ~646–681; replace `upkeep_after_round_ended` ~690–719; `resume_act_round_end_advance` tails ~750–770)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:1175` (`UpkeepAfterRoundEnded` continuation target)
- Modify: `crates/game-core/src/engine/mod.rs` (re-export the two fns the test shims call)
- Modify: `crates/game-core/src/test_support/mod.rs` (add two driver shims)
- Test: `crates/cards/tests/agenda_01107.rs` (add the ordering regression test)

**Interfaces:**
- Produces (engine internals, `pub(crate)` re-exported from `crate::engine`):
  - `upkeep_phase_end(cx: &mut Cx) -> EngineOutcome`
  - `resume_act_round_end_advance(cx: &mut Cx, response: &InputResponse) -> EngineOutcome`
  - `upkeep_round_end_at_and_after(cx: &mut Cx) -> EngineOutcome` (`pub(super)`)
  - `upkeep_round_end_teardown(cx: &mut Cx) -> EngineOutcome` (`pub(super)`)
- Produces (`test_support`, `pub`):
  - `run_upkeep_round_end(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome`
  - `resume_round_end_window(state: &mut GameState, events: &mut Vec<Event>, response: &InputResponse) -> EngineOutcome`
- Consumes (existing): `round_end_advance_window`, `super::act_agenda::{investigators_at, clues_held, spend_clues_from, advance_act}`, `super::emit::{emit_event, TimingEvent}`, `crate::state::Continuation::ActRoundEnd`.

- [ ] **Step 1: Bump visibility of the two driver fns and re-export them**

In `crates/game-core/src/engine/dispatch/phases.rs`, change the signature line of `upkeep_phase_end` from:

```rust
fn upkeep_phase_end(cx: &mut Cx) -> EngineOutcome {
```
to:
```rust
pub(crate) fn upkeep_phase_end(cx: &mut Cx) -> EngineOutcome {
```

Change `resume_act_round_end_advance` from `pub(super) fn` to `pub(crate) fn`:

```rust
pub(crate) fn resume_act_round_end_advance(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
```

In `crates/game-core/src/engine/mod.rs`, after the existing `pub(crate) use dispatch::...` lines (near line 44), add:

```rust
pub(crate) use dispatch::phases::{resume_act_round_end_advance, upkeep_phase_end};
```

- [ ] **Step 2: Add the two `test_support` driver shims**

In `crates/game-core/src/test_support/mod.rs`, after `fire_forced_on_round_end` (ends ~line 70), add:

```rust
/// Test helper: run the Upkeep step-4.6 round-end sequence
/// (`upkeep_phase_end`), returning the `EngineOutcome`. Suspends on act
/// 01109's "when the round ends" clue-spend window when affordable; resume it
/// with [`resume_round_end_window`].
pub fn run_upkeep_round_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::upkeep_phase_end(&mut cx)
}

/// Test helper: resume a parked act round-end clue-spend window
/// (`resume_act_round_end_advance`) with `response`.
pub fn resume_round_end_window(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    response: &crate::action::InputResponse,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::resume_act_round_end_advance(&mut cx, response)
}
```

- [ ] **Step 3: Write the failing regression test**

In `crates/cards/tests/agenda_01107.rs`, extend the imports. Change the `game_core::state` import line to include `Act` and `RoundEndAdvance`, and the `game_core::test_support` import to include the two new shims, and add `InputResponse` + `Continuation`:

```rust
use game_core::state::{
    Act, Agenda, CardCode, Continuation, Enemy, EnemyId, GameState, InvestigatorId, Location,
    LocationId, Phase, RoundEndAdvance,
};
use game_core::action::InputResponse;
use game_core::test_support::{
    fire_forced_on_phase_end, fire_forced_on_round_end, resume_round_end_window,
    run_upkeep_round_end, test_enemy, test_investigator, GameStateBuilder,
};
```

Then append this test:

```rust
#[test]
fn round_end_act_when_window_opens_before_agenda_at_doom() {
    install();
    // Act 01109 ("The Barrier") carries the "when the round ends" clue-spend
    // window; agenda 01107 carries the "at the end of the round" doom. Per the
    // RR "At" entry, `when` resolves before `at`, so the act window must open
    // BEFORE any doom is placed.
    let mut state = board_with_agenda();
    state.phase = Phase::Upkeep;

    // Affordable act window: investigator in the Hallway (01112) with >= 3 clues.
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 3,
        resolution: None,
        round_end_advance: Some(RoundEndAdvance {
            contributor_location: CardCode::new("01112"),
        }),
    }];
    state.act_index = 0;
    {
        let inv = state.investigators.get_mut(&InvestigatorId(1)).unwrap();
        inv.current_location = Some(LocationId(2)); // Hallway
        inv.clues = 3;
    }
    // Two Ghouls in Hallway/Parlor -> agenda 01107 would place 2 doom.
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2)));
    state.enemies.insert(EnemyId(2), ghoul(2, LocationId(5)));

    let mut events = Vec::new();
    let out = run_upkeep_round_end(&mut state, &mut events);

    // The act's `when the round ends` window opens first...
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
    assert!(matches!(
        state.continuations.last(),
        Some(Continuation::ActRoundEnd(_))
    ));
    // ...and the agenda's `at the end of the round` doom is NOT placed yet.
    assert_eq!(
        state.agenda_doom, 0,
        "`when` resolves before `at`: doom must wait for the act window"
    );

    // Declining the `when` window then runs the `at` doom.
    let _ = resume_round_end_window(&mut state, &mut events, &InputResponse::Skip);
    assert!(
        state.agenda_doom >= 2,
        "the `at` doom lands after the act window resolves"
    );
}
```

- [ ] **Step 4: Run the regression test against current code — verify it FAILS**

Run:
```bash
cargo test -p cards --test agenda_01107 round_end_act_when_window_opens_before_agenda_at_doom
```
Expected: **FAIL** at the `agenda_doom == 0` assertion (`left: 2, right: 0`) — today's code fires `RoundEnded` (placing 2 doom) before opening the act window.

- [ ] **Step 5: Implement the rethread in `phases.rs`**

Replace the body of `upkeep_phase_end` (the `match super::emit::emit_event(... RoundEnded)` block) so it opens the `when` window first. The full function becomes:

```rust
pub(crate) fn upkeep_phase_end(cx: &mut Cx) -> EngineOutcome {
    // 4.6 Upkeep phase ends. Round ends.
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Upkeep,
    });
    // "End of the upkeep phase" Forced (none in the Core+Dunwich corpus).
    let forced = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::PhaseEnded {
            phase: Phase::Upkeep,
        },
    );
    debug_assert!(
        matches!(forced, EngineOutcome::Done),
        "upkeep_phase_end PhaseEnded(Upkeep) forced did not resolve to Done: {forced:?}"
    );
    // RR "At" entry: `at the end of the round` abilities "trigger in between any
    // 'when...' abilities and any 'after...' abilities with the same triggering
    // condition." So act 01109's "when the round ends" clue-spend window opens
    // BEFORE agenda 01107's "at the end of the round" doom. Open the `when`
    // window first; the `at` RoundEnded Forced abilities + teardown run on its
    // resume (resume_act_round_end_advance) or inline below when no window opens.
    if let Some(pending) = round_end_advance_window(cx.state) {
        let prompt = format!(
            "End of round: investigators at the contributor location may, as a group, \
             spend {} clues to advance the current act. Submit ResolveInput with \
             InputResponse::Confirm to spend and advance, or Skip to decline.",
            pending.threshold,
        );
        cx.state
            .continuations
            .push(crate::state::Continuation::ActRoundEnd(pending));
        return EngineOutcome::AwaitingInput {
            request: InputRequest::prompt(prompt),
            resume_token: ResumeToken(0),
        };
    }
    upkeep_round_end_at_and_after(cx)
}
```

Replace the entire `upkeep_after_round_ended` function (the one beginning `pub(super) fn upkeep_after_round_ended`, ~690–719) with these two functions:

```rust
/// The round-end `at the end of the round` bucket + teardown, run after the
/// `when the round ends` act window (if any) has resolved. Fires the
/// `RoundEnded` Forced abilities — agenda 01107's doom, Dissonant Voices
/// 01165's discard (the RR `at` bucket) — then tears down. If 2+ Forced fire
/// and the run suspends for the lead's ordering (#213), the
/// `UpkeepAfterRoundEnded` continuation resumes the teardown via
/// [`upkeep_round_end_teardown`].
pub(super) fn upkeep_round_end_at_and_after(cx: &mut Cx) -> EngineOutcome {
    match super::emit::emit_event(cx, &super::emit::TimingEvent::RoundEnded) {
        EngineOutcome::Done => upkeep_round_end_teardown(cx),
        suspended @ EngineOutcome::AwaitingInput { .. } => suspended,
        rejected @ EngineOutcome::Rejected { .. } => rejected,
    }
}

/// Teardown after the round-end `at` Forced abilities resolve: expire active
/// "until the end of the round" lasting effects (Mind over Matter 01036's
/// substitution — RR p.24, "after the round-end forced abilities have
/// resolved"), then transition Upkeep -> Mythos.
pub(super) fn upkeep_round_end_teardown(cx: &mut Cx) -> EngineOutcome {
    cx.state.skill_substitutions.clear();
    // Upkeep -> Mythos; calls mythos_phase. Only Investigation->Enemy suspends
    // (hunter movement), so this never suspends on its own.
    step_phase(cx)
}
```

In `resume_act_round_end_advance`, change **both** tail calls from `step_phase(cx)` to `upkeep_round_end_at_and_after(cx)` so the `at` doom runs *after* this `when` window. The `Confirm` arm's last line:

```rust
        super::act_agenda::spend_clues_from(cx.state, &contributors, pending.threshold);
        cx.state.continuations.pop();
        // The round-end-advance act (01109) is non-terminal (resolution
        // None) — advance the cursor to the next act.
        super::act_agenda::advance_act(cx);
        // Now run the `at the end of the round` doom + teardown, AFTER this
        // `when the round ends` window (RR `when` -> `at`).
        upkeep_round_end_at_and_after(cx)
```

The `Skip` arm:

```rust
    InputResponse::Skip => {
        cx.state.continuations.pop();
        upkeep_round_end_at_and_after(cx)
    }
```

In `crates/game-core/src/engine/dispatch/reaction_windows.rs:1175`, repoint the continuation (the `at`-doom run now happens *after* the window, so its suspend-resume tail is teardown-only — it must not re-open the window):

```rust
        ForcedContinuation::UpkeepAfterRoundEnded => super::phases::upkeep_round_end_teardown(cx),
```

- [ ] **Step 6: Run the regression test — verify it PASSES**

Run:
```bash
cargo test -p cards --test agenda_01107 round_end_act_when_window_opens_before_agenda_at_doom
```
Expected: **PASS** — `agenda_doom == 0` at the suspend; `>= 2` after the `Skip` resume.

- [ ] **Step 7: Run the existing round-end / upkeep suites — verify still green**

Run:
```bash
cargo test -p game-core upkeep
cargo test -p game-core round_end
cargo test -p cards --test agenda_01107
```
Expected: **all PASS**, including `upkeep_phase_end_opens_window_when_affordable`, `upkeep_phase_end_skips_window_when_unaffordable`, `resume_confirm_spends_and_advances`, `resume_skip_advances_nothing_and_continues`, and `round_end_clears_round_scoped_skill_substitutions` (teardown still clears `skill_substitutions`, now after the `at` doom). If `round_end_clears_round_scoped_skill_substitutions` fails, confirm its setup reaches `upkeep_round_end_teardown` (no affordable act window) and adjust only the test's drive if needed — do **not** weaken the clear.

- [ ] **Step 8: Run the full CI gauntlet locally**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all green. (No wasm-only code touched, so the wasm jobs are unaffected, but CI will run them.)

- [ ] **Step 9: Correct spec §G and commit**

In `docs/superpowers/specs/2026-06-20-unified-control-flow-model-design.md` §G, replace "the cheap reorder" / "It's a pre-req bug" framing with the accurate shape: *a small rethread of two independently-suspendable round-end steps (act `when` window → `at` doom → teardown), since both the window and the multi-Forced doom run (#213) can suspend.* Keep the rest of §G.

Commit:
```bash
git add crates/game-core/src/engine/dispatch/phases.rs \
        crates/game-core/src/engine/dispatch/reaction_windows.rs \
        crates/game-core/src/engine/mod.rs \
        crates/game-core/src/test_support/mod.rs \
        crates/cards/tests/agenda_01107.rs \
        docs/superpowers/specs/2026-06-20-unified-control-flow-model-design.md
git commit -m "$(cat <<'EOF'
engine: round-end fires act `when` window before agenda `at` doom

Per the RR "At" entry (`when -> at -> after`), act 01109's "when the round
ends" clue-spend window must resolve before agenda 01107's "at the end of
the round" doom. upkeep_phase_end had it inverted (fired RoundEnded Forced,
then opened the act window). Rethread: open the `when` window first; run the
`at` RoundEnded Forced + teardown (until-end-of-round expiry, Upkeep->Mythos)
on the window's resume, or inline when no window opens. Both the window and
the multi-Forced run can suspend, so the teardown has a dedicated
continuation (UpkeepAfterRoundEnded -> upkeep_round_end_teardown).

Pre-req for the #393 unified-control-flow arc (spec step 0 / §G).

Closes #NN.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**Spec coverage (§G):** the plan implements the §G fix (act `when` before agenda `at`), adds a regression test on the agenda-3 + act-2 + ghouls state, files the bug issue, and corrects §G's "cheap reorder" wording (Step 9). ✓

**Placeholder scan:** no TBD/TODO/"handle edge cases" — every step has exact code, paths, and commands. The only `#NN` is the to-be-filed issue number, resolved in Prep and used in the commit. ✓

**Type consistency:** `run_upkeep_round_end` / `resume_round_end_window` signatures match between the `test_support` definitions (Step 2), the `Interfaces` block, and the test call sites (Step 3). `upkeep_round_end_at_and_after` / `upkeep_round_end_teardown` are defined once (Step 5) and referenced consistently in `upkeep_phase_end`, `resume_act_round_end_advance`, and the `reaction_windows.rs` continuation. The re-export (Step 1) names match the shim calls (`crate::engine::upkeep_phase_end`, `crate::engine::resume_act_round_end_advance`). ✓
