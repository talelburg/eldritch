# Upkeep Phase Content (#70) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Phase-4 Upkeep phase real — reset actions, ready every exhausted card (incl. enemies), each Active investigator draws 1 and gains 1 resource — driven by a phase-driver that mirrors the Mythos Fast-window machinery.

**Architecture:** A `upkeep_phase` driver (step 4.1 + open the post-4.1 player window) whose continuation `upkeep_resume` runs steps 4.2–4.5 then `upkeep_phase_end` (4.6 + Upkeep→Mythos), mirroring `mythos_phase` / `mythos_phase_end` inverted around the window position. Two per-round operations move to their rules-correct framework steps: the action refresh into Upkeep 4.2 (`reset_actions`, out of `rotate_to_active`), and the round-counter increment into Mythos 1.1 (`mythos_phase`, out of `step_phase`). Two refactors de-duplicate existing logic: `draw_one_with_deckout` (shared by the `Draw` action) and `grant_resources` (shared by the DSL `gain_resources`).

**Tech Stack:** Rust, `game-core` engine crate (no I/O, `wasm32`-compatible). Tests via the `TestGame` builder + `assert_event!` macros (`game-core` unit tests) and a separate cargo binary in `crates/scenarios/tests/` (integration).

**Reference spec:** `docs/superpowers/specs/2026-05-26-70-upkeep-phase-content-design.md`.

**Conventions for every commit in this plan:**
- Match CI strict flags before committing each task: `RUSTFLAGS="-D warnings" cargo test --all --all-features` (or at least `-p game-core` for engine-only tasks) and `cargo clippy --all-targets --all-features -- -D warnings`. The repo treats warnings as errors, so an unused helper fails the build — this plan orders tasks so every helper has a caller when it lands.
- Commit messages: `engine: <description>`, ending with the trailer:
  ```
  Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
  ```
- The spec + this plan get committed together as the first commit on the `engine/upkeep-phase` branch when execution starts (per the user's instruction), before Task 1.

---

## File Structure

- `crates/game-core/src/event.rs` — add `Event::CardReadied` (Task 1).
- `crates/game-core/src/state/game_state.rs` — add `WindowKind::UpkeepBegins` + serde test (Task 4).
- `crates/game-core/src/engine/evaluator.rs` — `gain_resources` delegates to `grant_resources` (Task 2).
- `crates/game-core/src/engine/dispatch.rs` — the bulk: `grant_resources` (T2), `draw_one_with_deckout` (T3), the Upkeep driver + `step_phase` / `end_turn` / `trigger_matches` / `run_window_continuation` changes (T5), `ready_exhausted_cards` (T6), `active_investigators_in_turn_order` + `upkeep_draw_and_resource` (T7), `reset_actions` + `rotate_to_active` / `start_scenario` relocation (T8), `mythos_phase` round-bump relocation (T9), plus all unit tests.
- `crates/scenarios/tests/upkeep_phase.rs` — new integration-test binary (Task 10).

---

## Task 1: `Event::CardReadied` variant

**Files:**
- Modify: `crates/game-core/src/event.rs`

`Event` is a public enum, so an unused variant produces no `dead_code` warning — it can land before its consumer (Task 6). It mirrors the existing `CardExhausted { investigator, instance_id, code }`.

- [ ] **Step 1: Add the variant** (place it adjacent to `CardExhausted`)

```rust
    /// An investigator's in-play card was readied (flipped from
    /// exhausted to ready) — e.g. during Upkeep step 4.3. Mirror of
    /// [`Event::CardExhausted`]. Enemies readying emit
    /// [`Event::EnemyReadied`] instead.
    CardReadied {
        /// The card's controller.
        investigator: InvestigatorId,
        /// The readied in-play instance.
        instance_id: CardInstanceId,
        /// The card code (for log readability; redundant with state).
        code: CardCode,
    },
```

- [ ] **Step 2: Build the workspace to surface any exhaustive `Event` match**

Run: `RUSTFLAGS="-D warnings" cargo build --all --all-features`
Expected: PASS. If a crate matches `Event` exhaustively (e.g. a server/web renderer) and `Event` is **not** `#[non_exhaustive]`, the compiler errors on the missing arm — add a `CardReadied` arm mirroring how that match handles `CardExhausted`. If `Event` is `#[non_exhaustive]`, downstream wildcard arms absorb it and nothing breaks.

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/event.rs
git commit -m "engine: add Event::CardReadied for in-play card readying

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 2: `grant_resources` helper + DSL `gain_resources` delegation

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (add `grant_resources`)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`gain_resources` delegates)
- Test: `crates/game-core/src/engine/dispatch.rs` (unit test) + existing `evaluator.rs` tests as the parity check

Pure refactor: extract the resource-grant core (saturating-add + `ResourcesGained` emit) so the DSL `gain_resources` and Upkeep 4.4 (Task 7) share it. The existing `gain_resources` tests in `evaluator.rs` (e.g. `gain_resources_increments_target_wallet_and_emits_event`, `gain_resources_zero_amount_is_a_silent_noop`) are the regression guard — they must stay green.

- [ ] **Step 1: Write the failing unit test** (add to the `dispatch.rs` test module that hosts other helper tests, e.g. near the upkeep tests area; import `TestGame`, `test_investigator`)

```rust
#[test]
fn grant_resources_adds_to_wallet_and_emits() {
    let id = InvestigatorId(1);
    let mut state = TestGame::default()
        .with_investigator(test_investigator(1))
        .build();
    let before = state.investigators[&id].resources;
    let mut events = Vec::new();

    grant_resources(&mut state, &mut events, id, 2);

    assert_eq!(state.investigators[&id].resources, before + 2);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::ResourcesGained { investigator, amount: 2 } if *investigator == id
    )));
}

#[test]
fn grant_resources_zero_is_silent_noop() {
    let id = InvestigatorId(1);
    let mut state = TestGame::default()
        .with_investigator(test_investigator(1))
        .build();
    let before = state.investigators[&id].resources;
    let mut events = Vec::new();

    grant_resources(&mut state, &mut events, id, 0);

    assert_eq!(state.investigators[&id].resources, before);
    assert!(events.is_empty());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core grant_resources`
Expected: FAIL — `cannot find function 'grant_resources'`.

- [ ] **Step 3: Add `grant_resources` to `dispatch.rs`**

```rust
/// Grant `amount` resources to `investigator`: saturating-add to the
/// wallet and emit [`Event::ResourcesGained`]. The resource-grant core
/// shared by the DSL `gain_resources` (called after target resolution)
/// and Upkeep step 4.4. No-op (no event) when `amount == 0`, matching
/// the existing `gain_resources` zero-amount behavior.
///
/// Caller guarantees `investigator` exists in `state.investigators`.
pub(super) fn grant_resources(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) {
    if amount == 0 {
        return;
    }
    let inv = state
        .investigators
        .get_mut(&investigator)
        .expect("grant_resources: caller guarantees investigator exists");
    inv.resources = inv.resources.saturating_add(amount);
    events.push(Event::ResourcesGained {
        investigator,
        amount,
    });
}
```

- [ ] **Step 4: Delegate the DSL `gain_resources` mutation to it**

In `crates/game-core/src/engine/evaluator.rs`, `gain_resources` keeps its `amount == 0` early-return, target resolution, and the existence check (which returns `Rejected`). Replace the final mutation+emit block:

```rust
    investigator.resources = investigator.resources.saturating_add(amount);
    events.push(Event::ResourcesGained {
        investigator: target_id,
        amount,
    });
```

with a call to the shared helper (drop the now-unused mutable `investigator` binding from the existence check — change it to an immutable `state.investigators.contains_key(&target_id)` check, or `.get(&target_id)`, so the `Rejected`-on-missing path stays):

```rust
    if !state.investigators.contains_key(&target_id) {
        return EngineOutcome::Rejected {
            reason: format!("GainResources: investigator {target_id:?} is not in the state").into(),
        };
    }
    crate::engine::dispatch::grant_resources(state, events, target_id, amount);
    EngineOutcome::Done
```

(If `super::dispatch::grant_resources` resolves more cleanly than the fully-qualified path, use that. If `pub(super)` visibility doesn't reach the evaluator, widen to `pub(crate)`.)

- [ ] **Step 5: Run the new test + the existing DSL regression tests**

Run: `cargo test -p game-core grant_resources && cargo test -p game-core gain_resources`
Expected: PASS for all (new helper tests + existing `gain_resources_*` tests unchanged).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: extract grant_resources helper shared with DSL gain_resources

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 3: `draw_one_with_deckout` helper + `Draw` action delegation

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`
- Test: existing `Draw`-action tests are the parity guard; add one direct unit test

Pure refactor: lift the deck-out draw sequence out of the `Draw` action body (`dispatch.rs:2905–2926`) so the action and Upkeep 4.4 share it.

- [ ] **Step 1: Write the failing unit test**

```rust
#[test]
fn draw_one_with_deckout_empty_deck_reshuffles_and_takes_horror() {
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.deck.clear();
    inv.discard = vec![CardCode::from("01000"), CardCode::from("01001")];
    inv.horror = 0;
    let hand_before = inv.hand.len();
    let mut state = TestGame::default().with_investigator(inv).build();
    let mut events = Vec::new();

    draw_one_with_deckout(&mut state, &mut events, id);

    assert_eq!(state.investigators[&id].hand.len(), hand_before + 1, "drew 1");
    assert_eq!(state.investigators[&id].horror, 1, "deck-out costs 1 horror");
    assert!(events.iter().any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })));
}
```

(Adjust `CardCode::from(...)` to the actual `CardCode` constructor used elsewhere in `dispatch.rs` tests — e.g. `CardCode("01000".into())` or a helper. Match the existing test idiom.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core draw_one_with_deckout`
Expected: FAIL — `cannot find function 'draw_one_with_deckout'`.

- [ ] **Step 3: Add `draw_one_with_deckout` and refactor `draw`**

Add the helper:

```rust
/// Draw one card for `investigator`, applying the empty-deck rule:
/// reshuffle the discard into the deck if the deck is empty, draw,
/// and take 1 horror on any would-draw-from-empty. Extracted verbatim
/// from the `Draw` action body so the action and Upkeep step 4.4 share
/// one code path. The deck-out reading (horror on would-draw-from-empty;
/// no reshuffle of a zero-card discard per Rules Reference p.9) is
/// inherited unchanged.
///
/// Caller guarantees `investigator` exists in `state.investigators`.
fn draw_one_with_deckout(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    let inv = state
        .investigators
        .get(&investigator)
        .expect("draw_one_with_deckout: caller guarantees investigator exists");
    let deck_empty = inv.deck.is_empty();
    let discard_empty = inv.discard.is_empty();
    if deck_empty {
        if !discard_empty {
            reshuffle_discard_into_deck(state, events, investigator);
        }
        draw_cards(state, events, investigator, 1);
        take_horror(state, events, investigator, 1);
    } else {
        draw_cards(state, events, investigator, 1);
    }
}
```

In the `draw` action, replace the mutation block (the `let inv = ...; let deck_empty = ...;` through the `if deck_empty { ... } else { ... }`) with:

```rust
    // Mutate.
    spend_one_action(state, events, investigator);
    draw_one_with_deckout(state, events, investigator);
    EngineOutcome::Done
```

- [ ] **Step 4: Run the new test + existing `Draw` tests**

Run: `cargo test -p game-core draw_one_with_deckout && cargo test -p game-core draw`
Expected: PASS — new test + all existing `draw`-action tests (deck-out / reshuffle / both-empty) unchanged.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: extract draw_one_with_deckout helper shared with Draw action

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 4: `WindowKind::UpkeepBegins` variant

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (variant + serde test)
- Modify: `crates/game-core/src/engine/dispatch.rs` (`trigger_matches` fallthrough arm only — the `run_window_continuation` arm lands in Task 5 with the driver)

The variant is payload-less (mirror `MythosAfterDraws`). It is referenced by two exhaustive `WindowKind` matches: `trigger_matches` (this task) and `run_window_continuation` (Task 5, where `upkeep_resume` exists to wire it to).

- [ ] **Step 1: Write the failing serde test** (mirror `between_phases_window_kind_serde_roundtrip` in `game_state.rs`'s `#[cfg(test)] mod open_window_tests`)

```rust
#[test]
fn upkeep_begins_window_kind_serde_roundtrip() {
    let kind = WindowKind::UpkeepBegins;
    let json = serde_json::to_string(&kind).expect("serialize");
    let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, kind);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core upkeep_begins_window_kind_serde_roundtrip`
Expected: FAIL — `no variant named 'UpkeepBegins'`.

- [ ] **Step 3: Add the variant**

In the `WindowKind` enum (after `MythosAfterDraws`):

```rust
    /// The player window between Rules Reference p.25 step 4.1 (upkeep
    /// phase begins) and step 4.2 (reset actions). Carries no payload —
    /// no `EventPattern` matches against it specifically today; the
    /// variant exists so the rule's printed timing point is addressable
    /// when a future card binds to it. Mirror of `MythosAfterDraws`.
    UpkeepBegins,
```

- [ ] **Step 4: Add `UpkeepBegins` to the `trigger_matches` fallthrough arm**

In `dispatch.rs`, the `trigger_matches` match (`dispatch.rs:1747–1754`) currently lists the Fast-only window kinds. Add `UpkeepBegins` so the match stays exhaustive:

```rust
        (
            WindowKind::BetweenPhases { .. }
            | WindowKind::AfterEnemyDefeated { .. }
            | WindowKind::MythosAfterDraws
            | WindowKind::UpkeepBegins,
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned,
        ) => false,
```

- [ ] **Step 5: Build to surface any other exhaustive `WindowKind` match**

Run: `RUSTFLAGS="-D warnings" cargo build -p game-core`
Expected: an error on `run_window_continuation`'s non-exhaustive match (`UpkeepBegins` not covered). **Leave that one for Task 5** — to keep this task self-contained, temporarily add `WindowKind::UpkeepBegins => {}` to `run_window_continuation`'s match (it'll be replaced in Task 5). Re-run; expect PASS. If any *other* `WindowKind` match errors, add a `UpkeepBegins` arm mirroring `MythosAfterDraws`'s handling.

- [ ] **Step 6: Run the serde test**

Run: `cargo test -p game-core upkeep_begins_window_kind_serde_roundtrip`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch.rs
git commit -m "engine: add WindowKind::UpkeepBegins variant

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 5: Upkeep driver skeleton + step_phase / end_turn / continuation wiring

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`
- Test: `crates/game-core/src/engine/dispatch.rs` (new `upkeep_phase_tests` module)

Lands the phase-driver plumbing with an empty-content `upkeep_resume` (just the 4.5 stub + 4.6 transition). Steps 4.2/4.3/4.4 are wired into `upkeep_resume` by Tasks 6–8. At this task, `rotate_to_active` still refreshes actions and the round bump still lives in `step_phase` — both relocated later — so the build stays coherent.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod upkeep_phase_tests {
    use super::*;
    use crate::event::Event;
    use crate::state::{InvestigatorId, Phase, Status};
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn upkeep_phase_emits_phase_started_and_auto_skips_to_mythos() {
        // No Fast-eligible cards / no reactions installed → the post-4.1
        // window auto-skips inline, the continuation runs, and the
        // cascade lands in Mythos.
        let id = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Enemy)
            .build();
        state.turn_order = vec![id];
        state.active_investigator = None;

        let mut events = Vec::new();
        step_phase(&mut state, &mut events); // Enemy → Upkeep, cascades to Mythos

        // Event order: PhaseStarted(Upkeep) → WindowOpened(UpkeepBegins) →
        // WindowClosed(UpkeepBegins) → PhaseEnded(Upkeep) → PhaseStarted(Mythos).
        let pos = |pred: &dyn Fn(&Event) -> bool| events.iter().position(|e| pred(e));
        let started = pos(&|e| matches!(e, Event::PhaseStarted { phase: Phase::Upkeep })).expect("PhaseStarted(Upkeep)");
        let w_open = pos(&|e| matches!(e, Event::WindowOpened { kind: WindowKind::UpkeepBegins })).expect("WindowOpened");
        let w_close = pos(&|e| matches!(e, Event::WindowClosed { kind: WindowKind::UpkeepBegins })).expect("WindowClosed");
        let ended = pos(&|e| matches!(e, Event::PhaseEnded { phase: Phase::Upkeep })).expect("PhaseEnded(Upkeep)");
        let mythos = pos(&|e| matches!(e, Event::PhaseStarted { phase: Phase::Mythos })).expect("PhaseStarted(Mythos)");
        assert!(started < w_open && w_open < w_close && w_close < ended && ended < mythos,
            "upkeep sub-step events must be ordered 4.1 → window → 4.6 → Mythos 1.1");
        assert_eq!(state.phase, Phase::Mythos, "cascade lands in Mythos");
        assert!(state.open_windows.is_empty(), "UpkeepBegins must not persist on the stack");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core upkeep_phase_emits_phase_started_and_auto_skips_to_mythos`
Expected: FAIL — `cannot find function 'upkeep_phase'` (and `step_phase` doesn't yet dispatch to it).

- [ ] **Step 3: Add the driver functions**

```rust
/// Entered by `step_phase` on the Enemy→Upkeep transition. Owns the
/// `PhaseStarted(Upkeep)` emit (step 4.1) and opens the post-4.1 player
/// window. Steps 4.2 onward run as the window's continuation
/// (`upkeep_resume`). Mirror of `mythos_phase`, inverted: Mythos's
/// window sits at the END, so its driver runs content then opens;
/// Upkeep's sits at the START, so the driver opens immediately and the
/// content is the continuation.
fn upkeep_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 4.1 Upkeep phase begins.
    events.push(Event::PhaseStarted { phase: Phase::Upkeep });
    // PLAYER WINDOW (post-4.1). Auto-skips inline (running upkeep_resume
    // via run_window_continuation) when nothing is Fast-eligible.
    open_fast_window(state, events, WindowKind::UpkeepBegins);
}

/// The post-4.1 window continuation. Steps 4.2–4.5 land here as named
/// call sites; 4.2/4.3/4.4 are wired in by later tasks. Then hands to
/// `upkeep_phase_end` for 4.6 + transition.
fn upkeep_resume(state: &mut GameState, events: &mut Vec<Event>) {
    // 4.2 reset_actions      — wired in Task 8 (action-refresh relocation)
    // 4.3 ready_exhausted_cards — wired in Task 6
    // 4.4 upkeep_draw_and_resource — wired in Task 7
    check_hand_size(state, events);  // 4.5 (TODO #111)
    upkeep_phase_end(state, events); // 4.6 + transition
}

/// Owns step 4.6's `PhaseEnded(Upkeep)` emit, then transitions to
/// Mythos. Exact analog of `mythos_phase_end`. `step_phase` suppresses
/// its `PhaseEnded(Upkeep)` fallback when `from == Upkeep`.
fn upkeep_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 4.6 Upkeep phase ends. Round ends.
    events.push(Event::PhaseEnded { phase: Phase::Upkeep });
    step_phase(state, events); // Upkeep → Mythos; calls mythos_phase
}

/// 4.5 Each investigator checks hand size.
fn check_hand_size(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#111): in player order, each investigator with more than 8
    //   cards in hand discards down to 8 (Rules Reference p.25 step 4.5).
    //   Needs an AwaitingInput producer for the discard choice. The call
    //   site exists so the rule step is grep-able and #111 plugs in here
    //   without changing the driver shape.
}
```

- [ ] **Step 4: Wire `run_window_continuation`** — replace the temporary `WindowKind::UpkeepBegins => {}` arm added in Task 4 with:

```rust
        WindowKind::UpkeepBegins => {
            // Phase-transitioning continuation (4.2–4.6 then Upkeep→Mythos):
            // cannot run while a skill test is in flight. Phase 4 has no
            // Upkeep-phase skill-test source, so structurally unreachable.
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "UpkeepBegins window closed while a skill test is in flight \
                     (continuation={:?}). Phase 4 has no Upkeep-phase skill-test \
                     sources; a future PR adding one needs the window-close + \
                     phase-transition ordering redesigned before this fires.",
                    in_flight.continuation,
                );
            }
            upkeep_resume(state, events);
        }
```

- [ ] **Step 5: Wire `step_phase`** — extend the `PhaseEnded` suppression and add the dispatch arm:

```rust
    // PhaseEnded suppressed when the from-phase's *_end helper owns it.
    if from != Phase::Mythos && from != Phase::Upkeep {
        events.push(Event::PhaseEnded { phase: from });
    }
```
```rust
    match to {
        Phase::Mythos if from != Phase::Mythos => mythos_phase(state, events),
        Phase::Investigation if from != Phase::Investigation => investigation_phase(state, events),
        Phase::Upkeep if from != Phase::Upkeep => upkeep_phase(state, events),
        _ => events.push(Event::PhaseStarted { phase: to }),
    }
```

- [ ] **Step 6: Update `end_turn`** — drop the third explicit `step_phase` call. Replace the no-next-investigator branch's three `step_phase` calls + `mythos_draw_pending` early-return with:

```rust
        state.active_investigator = None;
        step_phase(state, events); // Investigation → Enemy (empty until #71)
        step_phase(state, events); // Enemy → Upkeep: upkeep_phase opens the
                                   // post-4.1 window. Auto-skip cascades
                                   // 4.2–4.6 → Upkeep→Mythos → mythos_phase
                                   // (seeds mythos_draw_pending). If a Fast
                                   // play is eligible the window stays open
                                   // and the cascade pauses in Upkeep. Either
                                   // way the wait is signalled on state.
```

(Leave the `if let Some(next_id) = next { rotate_to_active(...) }` mid-round branch unchanged. The function's trailing `EngineOutcome::Done` covers both pause points.)

- [ ] **Step 7: Update existing phase-transition tests for the new window events**

Run: `cargo test -p game-core` and audit failures. Existing tests that assert exact event sequences across the Investigation→…→Mythos cascade (notably in the Mythos draw tests and `end_turn` tests) will see the new `PhaseStarted(Upkeep)` / `WindowOpened(UpkeepBegins)` / `WindowClosed(UpkeepBegins)` / `PhaseEnded(Upkeep)` events and the relocated transition. Update any order-sensitive (`assert_eq!` on the events slice) assertions; order-insensitive `assert_event!` assertions are unaffected. Re-run until green.

- [ ] **Step 8: Run the new test + full game-core suite**

Run: `cargo test -p game-core upkeep_phase_emits_phase_started_and_auto_skips_to_mythos && RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: Upkeep phase driver skeleton + step_phase/end_turn wiring

upkeep_phase opens the post-4.1 window mirroring the Mythos Fast window;
upkeep_phase_end owns PhaseEnded(Upkeep) and the Upkeep->Mythos step.
Content steps 4.2-4.4 land in follow-up tasks.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 6: Step 4.3 — `ready_exhausted_cards`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`
- Test: `crates/game-core/src/engine/dispatch.rs` (`upkeep_phase_tests`)

Readies every exhausted card in play — investigator in-play cards (`CardReadied`, Task 1) and enemies (`EnemyReadied`). Wired into `upkeep_resume` before `check_hand_size`.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn ready_exhausted_cards_readies_investigator_cards_and_enemies() {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut inv = test_investigator(1);
        // Place one exhausted in-play card. Use the in-play card
        // constructor the codebase exposes (mirror existing dispatch tests
        // that push to cards_in_play); set exhausted = true.
        inv.cards_in_play = vec![exhausted_in_play_card(CardCode::from("01000"), CardInstanceId(1))];
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true;
        let mut state = TestGame::default()
            .with_investigator(inv)
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut state, &mut events);

        assert!(!state.investigators[&inv_id].cards_in_play[0].exhausted, "card readied");
        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert!(events.iter().any(|e| matches!(
            e, Event::CardReadied { investigator, instance_id, .. }
            if *investigator == inv_id && *instance_id == CardInstanceId(1))));
        assert!(events.iter().any(|e| matches!(
            e, Event::EnemyReadied { enemy } if *enemy == enemy_id)));
    }

    #[test]
    fn ready_exhausted_cards_leaves_ready_cards_untouched() {
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = false; // already ready
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut state, &mut events);

        assert!(events.is_empty(), "no readying events for already-ready cards");
    }
```

(Replace `exhausted_in_play_card(...)` with however `dispatch.rs` tests construct a `CardInPlay` — search existing tests for `cards_in_play = vec![` and copy the idiom, setting `exhausted: true`. Confirm `EnemyId`, `CardInstanceId`, `test_enemy`, `with_enemy` imports.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core ready_exhausted_cards`
Expected: FAIL — `cannot find function 'ready_exhausted_cards'`.

- [ ] **Step 3: Implement**

```rust
/// 4.3 Ready exhausted cards. Rules Reference p.25: "Simultaneously
/// ready each exhausted card." "Each exhausted card" is every exhausted
/// card in play regardless of controller — investigator in-play cards
/// AND enemies. Simultaneous, so iteration order is immaterial; we
/// iterate deterministically (investigator id then in-play order; then
/// enemy id) for reproducible event streams. Already-ready cards emit
/// nothing.
fn ready_exhausted_cards(state: &mut GameState, events: &mut Vec<Event>) {
    let inv_ids: Vec<InvestigatorId> = state.investigators.keys().copied().collect();
    for id in inv_ids {
        let inv = state.investigators.get_mut(&id).expect("id from keys");
        for card in &mut inv.cards_in_play {
            if card.exhausted {
                card.exhausted = false;
                events.push(Event::CardReadied {
                    investigator: id,
                    instance_id: card.instance_id,
                    code: card.code.clone(),
                });
            }
        }
    }
    let enemy_ids: Vec<EnemyId> = state.enemies.keys().copied().collect();
    for eid in enemy_ids {
        let enemy = state.enemies.get_mut(&eid).expect("id from keys");
        if enemy.exhausted {
            enemy.exhausted = false;
            events.push(Event::EnemyReadied { enemy: eid });
        }
    }
}
```

- [ ] **Step 4: Wire into `upkeep_resume`** — add before `check_hand_size`:

```rust
    ready_exhausted_cards(state, events); // 4.3
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p game-core ready_exhausted_cards`
Expected: PASS (both).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: Upkeep 4.3 ready_exhausted_cards (cards + enemies)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 7: Step 4.4 — `upkeep_draw_and_resource` + `active_investigators_in_turn_order`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`
- Test: `crates/game-core/src/engine/dispatch.rs` (`upkeep_phase_tests`)

Each Active investigator draws 1 (via `draw_one_with_deckout`, Task 3) then — after all draws — gains 1 resource (via `grant_resources`, Task 2). Introduces the shared `active_investigators_in_turn_order` filter. Wired into `upkeep_resume` after `ready_exhausted_cards`, before `check_hand_size`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn upkeep_draw_and_resource_draws_and_grants_per_active_investigator() {
        let (a, b, c) = (InvestigatorId(1), InvestigatorId(2), InvestigatorId(3));
        let mut inv_a = test_investigator(1);
        inv_a.deck = vec![CardCode::from("01000")];
        let mut inv_b = test_investigator(2);
        inv_b.deck = vec![CardCode::from("01001")];
        let mut inv_c = test_investigator(3);
        inv_c.status = Status::Resigned; // eliminated → skipped
        inv_c.deck = vec![CardCode::from("01002")];
        let res_a = inv_a.resources;
        let res_c = inv_c.resources;
        let mut state = TestGame::default()
            .with_investigator(inv_a).with_investigator(inv_b).with_investigator(inv_c)
            .build();
        state.turn_order = vec![a, b, c];
        let mut events = Vec::new();

        upkeep_draw_and_resource(&mut state, &mut events);

        assert_eq!(state.investigators[&a].resources, res_a + 1);
        assert_eq!(state.investigators[&b].resources, test_investigator(2).resources + 1);
        assert_eq!(state.investigators[&c].resources, res_c, "eliminated investigator skipped");
        assert_eq!(state.investigators[&a].hand.len(), test_investigator(1).hand.len() + 1);
        assert_eq!(state.investigators[&c].deck.len(), 1, "eliminated investigator did not draw");
    }

    #[test]
    fn upkeep_draw_and_resource_two_pass_ordering() {
        // All CardsDrawn events precede all ResourcesGained events.
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut inv_a = test_investigator(1);
        inv_a.deck = vec![CardCode::from("01000")];
        let mut inv_b = test_investigator(2);
        inv_b.deck = vec![CardCode::from("01001")];
        let mut state = TestGame::default()
            .with_investigator(inv_a).with_investigator(inv_b)
            .build();
        state.turn_order = vec![a, b];
        let mut events = Vec::new();

        upkeep_draw_and_resource(&mut state, &mut events);

        let last_draw = events.iter().rposition(|e| matches!(e, Event::CardsDrawn { .. })).expect("draws");
        let first_gain = events.iter().position(|e| matches!(e, Event::ResourcesGained { .. })).expect("gains");
        assert!(last_draw < first_gain, "all draws must precede all resource gains");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core upkeep_draw_and_resource`
Expected: FAIL — `cannot find function 'upkeep_draw_and_resource'` / `active_investigators_in_turn_order`.

- [ ] **Step 3: Implement both functions**

```rust
/// `turn_order` entries whose status is `Active`, in turn order. Shared
/// by per-investigator Upkeep steps (4.2 reset, 4.4 draw + resource).
/// Eliminated investigators (Killed / Insane / Resigned) are excluded
/// per Rules Reference p.10.
fn active_investigators_in_turn_order(state: &GameState) -> Vec<InvestigatorId> {
    state
        .turn_order
        .iter()
        .copied()
        .filter(|id| {
            state
                .investigators
                .get(id)
                .is_some_and(|inv| inv.status == Status::Active)
        })
        .collect()
}

/// 4.4 Each investigator draws 1 card and gains 1 resource. Rules
/// Reference p.25: "In player order, each investigator draws 1 card.
/// Once those cards have been drawn, each investigator gains 1
/// resource." Two passes to honor that ordering: all draws first, then
/// all resource gains.
fn upkeep_draw_and_resource(state: &mut GameState, events: &mut Vec<Event>) {
    let ids = active_investigators_in_turn_order(state);
    for &id in &ids {
        draw_one_with_deckout(state, events, id);
    }
    for &id in &ids {
        grant_resources(state, events, id, 1);
    }
}
```

- [ ] **Step 4: Wire into `upkeep_resume`** — add after `ready_exhausted_cards`, before `check_hand_size`:

```rust
    upkeep_draw_and_resource(state, events); // 4.4
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p game-core upkeep_draw_and_resource`
Expected: PASS (both).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: Upkeep 4.4 draw + gain resource (two-pass, Active only)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 8: Step 4.2 — `reset_actions` + action-refresh relocation

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (`reset_actions`, `rotate_to_active`, `start_scenario`, `upkeep_resume`, affected tests)

Moves the `actions_remaining` refresh out of `rotate_to_active` (set-active-only now) into Upkeep 4.2 (`reset_actions`), with `start_scenario` seeding round 1. This is atomic — wiring `reset_actions` into `upkeep_resume` (round 2+) and `start_scenario` (round 1) lands together with removing `rotate_to_active`'s refresh, so actions are reset exactly once per round at every point.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn reset_actions_sets_active_to_per_turn_and_skips_eliminated() {
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut inv_a = test_investigator(1);
        inv_a.actions_remaining = 0;
        let mut inv_b = test_investigator(2);
        inv_b.actions_remaining = 0;
        inv_b.status = Status::Killed;
        let mut state = TestGame::default()
            .with_investigator(inv_a).with_investigator(inv_b)
            .build();
        state.turn_order = vec![a, b];
        let mut events = Vec::new();

        reset_actions(&mut state, &mut events);

        assert_eq!(state.investigators[&a].actions_remaining, ACTIONS_PER_TURN);
        assert_eq!(state.investigators[&b].actions_remaining, 0, "eliminated skipped");
        assert!(events.iter().any(|e| matches!(
            e, Event::ActionsRemainingChanged { investigator, new_count }
            if *investigator == a && *new_count == ACTIONS_PER_TURN)));
        assert!(!events.iter().any(|e| matches!(
            e, Event::ActionsRemainingChanged { investigator, .. } if *investigator == b)));
    }

    #[test]
    fn rotate_to_active_does_not_refresh_actions() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.actions_remaining = 1;
        let mut state = TestGame::default().with_investigator(inv).build();
        let mut events = Vec::new();

        rotate_to_active(&mut state, &mut events, id);

        assert_eq!(state.active_investigator, Some(id));
        assert_eq!(state.investigators[&id].actions_remaining, 1, "rotate must not refresh actions");
        assert!(events.is_empty(), "rotate no longer emits ActionsRemainingChanged");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core reset_actions_sets_active_to_per_turn_and_skips_eliminated rotate_to_active_does_not_refresh_actions`
Expected: FAIL — `reset_actions` missing; `rotate_to_active` still refreshes + emits.

- [ ] **Step 3: Add `reset_actions`**

```rust
/// 4.2 Reset actions. Rules Reference p.25: "Flip each investigator's
/// mini card back to its colored side. This indicates that the
/// investigator's actions have been reset for his or her next turn."
///
/// The canonical action-refresh site. Sets `actions_remaining` to
/// `ACTIONS_PER_TURN` for each Active investigator and emits
/// `ActionsRemainingChanged` when the value changes. `rotate_to_active`
/// no longer refreshes (step 2.2 is just "the turn begins");
/// `start_scenario` seeds round 1. Eliminated investigators are skipped
/// (Rules Reference p.10).
fn reset_actions(state: &mut GameState, events: &mut Vec<Event>) {
    for id in active_investigators_in_turn_order(state) {
        let inv = state
            .investigators
            .get_mut(&id)
            .expect("id from active_investigators_in_turn_order");
        if inv.actions_remaining != ACTIONS_PER_TURN {
            inv.actions_remaining = ACTIONS_PER_TURN;
            events.push(Event::ActionsRemainingChanged {
                investigator: id,
                new_count: ACTIONS_PER_TURN,
            });
        }
    }
}
```

- [ ] **Step 4: Simplify `rotate_to_active`** — remove the action refresh + `ActionsRemainingChanged` emit, keep set-active:

```rust
/// Set `active_investigator` to `id`. Does NOT refresh actions —
/// actions are reset at Upkeep step 4.2 (`reset_actions`) for the whole
/// next round, and seeded for round 1 by `start_scenario`. By the time
/// an investigator becomes active, `actions_remaining` already holds
/// this round's allotment.
///
/// `id` must refer to an investigator in `state.investigators` (a
/// whole-program invariant for ids drawn from `turn_order`).
fn rotate_to_active(state: &mut GameState, _events: &mut Vec<Event>, id: InvestigatorId) {
    debug_assert!(
        state.investigators.contains_key(&id),
        "rotate_to_active: investigator {id:?} not in investigators (state corruption)"
    );
    state.active_investigator = Some(id);
}
```

(Keep the `_events` parameter so call sites are unchanged; or drop it and update the three call sites — implementer's choice, but `_events` is the lower-churn option.)

- [ ] **Step 5: Seed round-1 actions in `start_scenario`** — add immediately before the `investigation_phase(state, events);` call:

```rust
    // Round-1 action seed: round 1 skips Mythos, so there's no Upkeep 4.2
    // to grant the first round's actions. Every Active investigator → ACTIONS_PER_TURN.
    reset_actions(state, events);
```

- [ ] **Step 6: Wire `reset_actions` into `upkeep_resume`** — add as the FIRST step (before `ready_exhausted_cards`):

```rust
    reset_actions(state, events); // 4.2
```

- [ ] **Step 7: Update tests that asserted rotate-time `ActionsRemainingChanged`**

Update `investigation_phase_emits_phase_started_and_rotates_to_lead` (`dispatch.rs:4561`) — `rotate_to_active` no longer emits `ActionsRemainingChanged`, so replace the rotate-ordering assertion:

```rust
        assert_eq!(state.active_investigator, Some(InvestigatorId(1)),
            "investigation_phase must rotate to the lead (first in turn_order)");
        assert!(events.iter().any(|e| matches!(e,
            Event::PhaseStarted { phase: Phase::Investigation })),
            "PhaseStarted(Investigation) must be emitted");
        assert!(!events.iter().any(|e| matches!(e, Event::ActionsRemainingChanged { .. })),
            "rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2)");
```

Then run the full suite and audit any remaining failures: `cargo test -p game-core` — search test modules for `ActionsRemainingChanged` and update any assertion expecting a rotate (turn-start) to emit it. `start_scenario_advances_to_investigation_with_round_one` (in `engine/mod.rs`) should still pass (the lead's `ActionsRemainingChanged` + `actions_remaining == 3` now come from the round-1 `reset_actions` seed) — confirm it does.

- [ ] **Step 8: Run the targeted + full suite**

Run: `cargo test -p game-core reset_actions rotate_to_active && RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: relocate action refresh from rotate_to_active to Upkeep 4.2

reset_actions is the canonical refresh; rotate_to_active is set-active-only;
start_scenario seeds round 1. ActionsRemainingChanged now fires once per
round at Upkeep, not at each turn-start.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 9: Round-counter relocation (step_phase → mythos_phase 1.1)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (`step_phase`, `mythos_phase`)
- Test: `crates/game-core/src/engine/dispatch.rs` (`upkeep_phase_tests`)

Pure refactor: move `state.round` increment out of `step_phase`'s Mythos-entry bump into `mythos_phase`'s step 1.1. `mythos_phase` is the sole entry into `Phase::Mythos`, so no read site sees a different value.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn round_increments_on_mythos_entry_via_driver() {
        let id = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Upkeep)
            .build();
        state.turn_order = vec![id];
        state.active_investigator = None;
        state.round = 4;

        let mut events = Vec::new();
        step_phase(&mut state, &mut events); // Upkeep → Mythos (via upkeep? no — direct)

        assert_eq!(state.round, 5, "round bumps on Mythos entry");
        assert_eq!(state.phase, Phase::Mythos);
    }
```

(Note: `step_phase` from `Phase::Upkeep` enters `upkeep_phase` first — to test the *Mythos* bump directly, instead drive from a state already transitioning into Mythos. Simpler: build with `.with_phase(Phase::Upkeep)` and call `upkeep_phase_end` directly, OR assert via the auto-skip cascade. If the direct `step_phase(Upkeep→…)` path enters `upkeep_phase`, the cascade still lands in Mythos with `round == 5` — assert that end state. Adjust the test to whichever entry is cleanest; the invariant under test is `round == 5` and `phase == Mythos` after the cascade.)

- [ ] **Step 2: Run to verify it fails or passes-for-wrong-reason**

Run: `cargo test -p game-core round_increments_on_mythos_entry_via_driver`
Expected: PASS currently (bump still in `step_phase`) — this is a characterization test guarding the refactor. Proceed; the refactor must keep it green.

- [ ] **Step 3: Remove the bump from `step_phase`** — replace:

```rust
    state.phase = to;
    if to == Phase::Mythos {
        state.round = state.round.saturating_add(1);
    }
```
with:
```rust
    state.phase = to;
    // The round-counter bump moves into mythos_phase (step 1.1). step_phase
    // no longer touches state.round.
```

- [ ] **Step 4: Add the bump to `mythos_phase` step 1.1** — as the first action, before the `PhaseStarted(Mythos)` emit:

```rust
    // 1.1 Round begins. Mythos phase begins.
    //     Rules Reference p.24: "As this is the first framework event of
    //     the round, it [1.1] also formalizes the beginning of a new game
    //     round." The round-counter increment lives HERE (not in
    //     step_phase) so the rule's round-begin point has explicit driver
    //     ownership, mirroring PhaseStarted(Mythos). Round 1 is bypassed:
    //     start_scenario sets round = 1 directly (Mythos skipped). This is
    //     also the future home for a RoundStarted event when a consumer lands.
    state.round = state.round.saturating_add(1);
    events.push(Event::PhaseStarted { phase: Phase::Mythos });
```

Also update the existing 1.1 comment block in `mythos_phase` that claims "step_phase has already … bumped the round counter" to reflect the new ownership.

- [ ] **Step 5: Run the test + full suite**

Run: `cargo test -p game-core round_increments_on_mythos_entry_via_driver && RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS. Existing round-increment assertions stay green (observable behavior unchanged).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: relocate round-counter bump into mythos_phase step 1.1

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 10: Integration tests + full gauntlet

**Files:**
- Create: `crates/scenarios/tests/upkeep_phase.rs`
- Test: that file (separate cargo binary; installs the registries)

End-to-end proof on the synthetic scenario, plus an `end_turn`-cascade unit test and replay determinism. Ready-flips and deck-out paths are already covered by the engine unit tests (Tasks 6–7); the integration tests focus on the full-round cascade with the real registries.

- [ ] **Step 1: Add the `end_turn` cascade unit test** (in `dispatch.rs::upkeep_phase_tests`)

```rust
    #[test]
    fn end_turn_cascades_through_upkeep_to_mythos_draw_pending() {
        // Single investigator, non-empty deck, an exhausted in-play card.
        // After EndTurn: card readied, hand +1, resources +1, landed in
        // Mythos with draw pending and round bumped.
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.actions_remaining = 0;
        inv.deck = vec![CardCode::from("01000"), CardCode::from("01001")];
        inv.cards_in_play = vec![exhausted_in_play_card(CardCode::from("01002"), CardInstanceId(1))];
        let res_before = inv.resources;
        let hand_before = inv.hand.len();
        let mut state = TestGame::default()
            .with_investigator(inv)
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id];
        state.active_investigator = Some(id);
        state.round = 1;

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.phase, Phase::Mythos);
        assert_eq!(result.state.round, 2, "round bumped on Mythos entry");
        assert_eq!(result.state.mythos_draw_pending, Some(id));
        assert_eq!(result.state.active_investigator, None);
        assert!(!result.state.investigators[&id].cards_in_play[0].exhausted, "readied");
        assert_eq!(result.state.investigators[&id].resources, res_before + 1, "gained 1");
        assert_eq!(result.state.investigators[&id].hand.len(), hand_before + 1, "drew 1");
    }
```

(Imports: `apply`, `Action`, `PlayerAction`, `EngineOutcome` — mirror the imports in `engine/mod.rs` tests. This test needs no registry because the synthetic codes never get looked up before the Mythos draw pauses.)

- [ ] **Step 2: Run it**

Run: `cargo test -p game-core end_turn_cascades_through_upkeep_to_mythos_draw_pending`
Expected: PASS.

- [ ] **Step 3: Write the integration tests** in `crates/scenarios/tests/upkeep_phase.rs`

Model on `crates/scenarios/tests/mythos_phase.rs` (registry install, synthetic scenario setup). Cover:

```rust
// Drive a synthetic 1-investigator scenario: StartScenario, take/end the
// first turn, and assert the Upkeep cascade ran (drew 1, gained 1 resource)
// and landed at Mythos round 2 with a draw pending. Ensure the fixture's
// investigator deck has >5 cards so a card remains to draw at upkeep
// (top up the fixture deck in-test if needed).
#[test]
fn upkeep_full_round_draws_and_grants_then_pauses_at_mythos() { /* ... */ }

// Drive the same scenario through to the round boundary, snapshot the
// action log, replay it from the initial state, and assert bit-for-bit
// identical resulting state.
#[test]
fn upkeep_round_replay_is_deterministic() { /* ... */ }
```

Fill in the bodies using the `mythos_phase.rs` patterns: `install(cards::REGISTRY)` + `install(scenarios::REGISTRY)`, build the synthetic scenario state, drive actions via `apply`, accumulate the action log, and replay by folding `apply` over the recorded actions from the initial state. Assert `resources`, `hand.len()`, `phase == Mythos`, `mythos_draw_pending.is_some()`, and (for replay) `state == replayed_state` (or field-wise equality if `GameState` isn't `PartialEq` — compare `phase`, `round`, per-investigator `resources` / `hand` / `deck`).

- [ ] **Step 4: Run the integration tests**

Run: `cargo test -p scenarios --test upkeep_phase`
Expected: PASS.

- [ ] **Step 5: Run the full CI-equivalent gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```
Expected: all PASS. Fix any `fmt` / `clippy` / intra-doc-link issues surfaced.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs crates/scenarios/tests/upkeep_phase.rs
git commit -m "test: Upkeep phase integration + end_turn cascade + replay determinism

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Final: phase-doc update (last commit before merge)

Per CLAUDE.md, update `docs/phases/phase-4-scenario-plumbing.md` **only** once the PR is ready to merge (PR number known, review fixes folded in):
- Move `#70` from the Issues table to the Closed table; bump closed/open counts.
- Flip Ordering / Arc row #7 (`#70`) to `✅ PR #N`.
- Correct the stale `#70` note that says it "folds in `GameState.round`" — the counter pre-exists; #70 *relocates* its increment into `mythos_phase` 1.1.
- Add the Decisions-made entries listed in the spec's "Phase-doc entries" section (Upkeep driver shape; action-refresh relocation; round-counter relocation; "ready each exhausted card" incl. enemies; 4.4 two-pass; shared helpers).

---

## Self-Review

**Spec coverage:** Every spec section maps to a task — `CardReadied` (T1), `grant_resources` (T2), `draw_one_with_deckout` (T3), `UpkeepBegins` (T4), driver + `step_phase`/`end_turn`/`run_window_continuation` (T5), `ready_exhausted_cards` (T6), `upkeep_draw_and_resource` + `active_investigators_in_turn_order` (T7), `reset_actions` + relocation (T8), round-counter relocation (T9), integration + cascade + replay (T10), `check_hand_size` stub (T5). 4.5/#111 stays a stub; out-of-scope items (BetweenPhases removal → #140, RoundStarted/RoundEnded) are not tasked, per spec.

**Type/name consistency:** `grant_resources` / `draw_one_with_deckout` / `reset_actions` / `ready_exhausted_cards` / `upkeep_draw_and_resource` / `active_investigators_in_turn_order` / `upkeep_phase` / `upkeep_resume` / `upkeep_phase_end` / `check_hand_size` used identically across tasks. `Event::CardReadied { investigator, instance_id, code }` and `WindowKind::UpkeepBegins` consistent. `ACTIONS_PER_TURN` is the existing constant.

**Build-green ordering:** Every helper lands with a caller (no `-D warnings` dead-code break): T2/T3 wire into existing callers; T6/T7/T8 wire into `upkeep_resume` (which exists from T5) and `start_scenario`; `CardReadied` (T1) is a pub-enum variant (no dead-code warning) consumed in T6. The action-refresh swap (T8) and round-bump move (T9) are each atomic.

**Placeholders:** The only intentionally-deferred bodies are `check_hand_size` (TODO #111, with a tracked issue) and the integration-test bodies in T10 (described against an existing template file, `mythos_phase.rs`); construction idioms (`exhausted_in_play_card`, `CardCode::from`) are flagged to match existing test code.
