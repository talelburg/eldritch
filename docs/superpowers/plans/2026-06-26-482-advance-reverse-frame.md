# Resumable Act/Agenda Advance with Gated Acknowledge (#482) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make an act/agenda advance a resumable `AdvanceReverse` continuation frame that absorbs a gated acknowledge `Confirm` and a *suspending* on-advance reverse (01105's interactive `ChooseOne`) uniformly, and defer the Mythos 1.4 encounter draws until it completes — fixing the doom-cascade panic.

**Architecture:** A new `Continuation::AdvanceReverse { deck, from, leaving_code, step }` frame driven by the `drive` loop through `AwaitAck → FireReverse → Finalize`: push the observable `…Advanced` event + (gated) a `Confirm`; fire the reverse via `emit_event` (queued, may suspend); then bump the index (correct RR order). `advance_agenda`/`advance_act` push this frame instead of doing synchronous emit+bookkeeping. `mythos_phase` defers the 1.4 draws to a new `MythosResume::Draws` anchor stage so they run after the frame pops.

**Tech Stack:** Rust, `game-core` engine kernel (no I/O, wasm32-compatible), the existing `Continuation` frame + `drive`/`resolve_input` dispatch idiom (mirrors `SkillTest`/`EncounterDraw`), `emit_event` forced-trigger machinery, #478's `interactive_acknowledge` flag.

## Global Constraints

- **CI gauntlet (warnings-as-errors).** Before pushing run all of: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Kernel purity.** `game-core` has no I/O and no presentation. The acknowledge `Confirm` prompt is a plain descriptive string (consistent with the open-turn / commit / #478 prompts); no client change.
- **Validate-first / mutate-second.** Resume handlers check preconditions and return `Rejected` (state/events unchanged) before mutating.
- **Gated acknowledge.** The advance `Confirm` is gated on the existing `GameState.interactive_acknowledge` (off by default → no pause, no churn; the server already sets it on for human play).
- **RR ordering.** The index bump (`agenda_index`/`act_index` += 1; `agenda_doom = 0`) happens at `Finalize`, **after** the reverse resolves ("flip the card, follow the reverse, then the next card becomes current").
- **Terminal advances unchanged.** A card carrying a resolution point goes through `request_resolution` (scenario-end latch), not `AdvanceReverse`.

---

### Task 1: `AdvanceReverse` frame — types, driver, resume

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`enum Continuation` ~line 395; add `AdvanceReverse` variant + `AdvanceDeck`/`AdvanceStep` enums; `enum MythosResume` ~line 913 add `Draws`)
- Create: `crates/game-core/src/engine/dispatch/advance_reverse.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (module decl; drive-loop arm; `resolve_input` arm)
- Test: `crates/game-core/src/engine/dispatch/advance_reverse.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `Continuation::AdvanceReverse { deck: AdvanceDeck, from: usize, leaving_code: CardCode, step: AdvanceStep }`.
  - `pub enum AdvanceDeck { Act, Agenda }`, `pub enum AdvanceStep { AwaitAck, FireReverse, Finalize }` (both `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]`).
  - `MythosResume::Draws` (used by Task 3).
  - `advance_reverse::drive(cx: &mut Cx) -> EngineOutcome`, `advance_reverse::resume(cx: &mut Cx, response: &InputResponse) -> EngineOutcome` (`pub(super)`).

- [ ] **Step 1: Add the enums + `Continuation` variant + `MythosResume::Draws`**

In `crates/game-core/src/state/game_state.rs`, add near the other small state enums (e.g. just before `pub enum Continuation`):

```rust
/// Which deck an [`AdvanceReverse`](Continuation::AdvanceReverse) frame advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvanceDeck {
    /// The act deck (`act_index` / clue thresholds).
    Act,
    /// The agenda deck (`agenda_index` / doom thresholds).
    Agenda,
}

/// Step cursor for the [`AdvanceReverse`](Continuation::AdvanceReverse) frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvanceStep {
    /// Push the observable `…Advanced` event; if interactive acknowledgment is
    /// on, suspend with a `Confirm` here (the cursor stays until resumed).
    AwaitAck,
    /// Fire the leaving card's Forced on-advance reverse via `emit_event`.
    FireReverse,
    /// The reverse has resolved: bump the deck cursor and pop the frame.
    Finalize,
}
```

Add this variant inside `pub enum Continuation { … }`:

```rust
    /// An act/agenda is advancing (#482). A small resumable sub-process that
    /// pushes the observable `…Advanced` event, optionally pauses for a gated
    /// acknowledge `Confirm`, fires the leaving card's Forced on-advance reverse
    /// (which may itself suspend — 01105's interactive `ChooseOne`), then bumps
    /// the deck cursor *after* the reverse resolves (RR order). Driven by the
    /// `drive` loop and resumed via `resolve_input` (mirrors the `SkillTest`
    /// frame). Replaces the former synchronous `advance_agenda`/`advance_act`
    /// emit-then-bump, whose post-forced bookkeeping stranded a suspending
    /// reverse.
    AdvanceReverse {
        /// Which deck is advancing.
        deck: AdvanceDeck,
        /// Cursor index of the leaving card (before the bump).
        from: usize,
        /// Printed code of the leaving card (its reverse fires).
        leaving_code: CardCode,
        /// Where in the sub-process we are.
        step: AdvanceStep,
    },
```

In `pub enum MythosResume { … }`, add after `Entry`:

```rust
    /// After step 1.2/1.3 (doom + agenda advance, incl. a suspending reverse)
    /// have resolved: run the step-1.4 encounter draws. `mythos_phase` parks the
    /// anchor here and the 1.4 draws run from `anchor_on_child_pop` once any
    /// `AdvanceReverse` frame above the anchor pops (#482).
    Draws,
```

- [ ] **Step 2: Write the failing driver test**

Create `crates/game-core/src/engine/dispatch/advance_reverse.rs` with the test module first (the impl is Step 4):

```rust
//! Resumable act/agenda advance (#482): the `AdvanceReverse` continuation frame
//! and its driver. See the `Continuation::AdvanceReverse` doc.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AdvanceDeck, AdvanceStep, Agenda, CardCode, Continuation};
    use crate::test_support::GameStateBuilder;

    fn state_advancing_agenda(interactive: bool) -> crate::state::GameState {
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![
            Agenda { code: CardCode("_a1".into()), doom_threshold: 1, resolution: None },
            Agenda { code: CardCode("_a2".into()), doom_threshold: 3, resolution: None },
        ];
        state.agenda_index = 0;
        state.interactive_acknowledge = interactive;
        // An AdvanceReverse frame as advance_agenda would push it (leaving = a1).
        state.continuations.push(Continuation::AdvanceReverse {
            deck: AdvanceDeck::Agenda,
            from: 0,
            leaving_code: CardCode("_a1".into()),
            step: AdvanceStep::AwaitAck,
        });
        state
    }

    /// Flag off: the frame drives straight through (no registry ⇒ no reverse) and
    /// the agenda cursor bumps at Finalize, the frame popping itself.
    #[test]
    fn advance_reverse_drives_through_when_not_interactive() {
        use crate::event::Event;
        let mut state = state_advancing_agenda(false);
        let mut events = Vec::new();
        let out = crate::engine::dispatch::drive(
            &mut Cx { state: &mut state, events: &mut events },
            EngineOutcome::Done,
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.agenda_index, 1, "cursor bumped at Finalize");
        assert!(
            !state.continuations.iter().any(|c| matches!(c, Continuation::AdvanceReverse { .. })),
            "frame popped"
        );
        assert!(events.iter().any(|e| matches!(e, Event::AgendaAdvanced { from: 0 })));
    }

    /// Flag on: the frame suspends at the acknowledge Confirm before firing the
    /// reverse — the cursor has NOT bumped yet.
    #[test]
    fn advance_reverse_pauses_for_acknowledge_when_interactive() {
        use crate::InputKind;
        let mut state = state_advancing_agenda(true);
        let mut events = Vec::new();
        let out = crate::engine::dispatch::drive(
            &mut Cx { state: &mut state, events: &mut events },
            EngineOutcome::Done,
        );
        let EngineOutcome::AwaitingInput { request, .. } = &out else {
            panic!("expected the acknowledge Confirm, got {out:?}");
        };
        assert_eq!(request.kind, InputKind::Confirm);
        assert_eq!(state.agenda_index, 0, "cursor must NOT bump before the reverse resolves");
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p game-core --lib advance_reverse`
Expected: FAIL — `advance_reverse` module/items not wired (compile error).

- [ ] **Step 4: Implement the driver + resume**

At the TOP of `crates/game-core/src/engine/dispatch/advance_reverse.rs` (above the test module) add:

```rust
use crate::action::InputResponse;
use crate::event::Event;
use crate::state::{AdvanceDeck, AdvanceStep, Continuation};

use super::super::outcome::{EngineOutcome, InputRequest, ResumeToken};
use super::Cx;

/// Read the top `AdvanceReverse` frame's fields. The frame is the top
/// continuation whenever the driver / resume runs (the `drive` loop / `resolve_input`
/// route here only with it on top).
fn top(cx: &Cx) -> (AdvanceDeck, usize, crate::state::CardCode, AdvanceStep) {
    match cx.state.continuations.last() {
        Some(Continuation::AdvanceReverse { deck, from, leaving_code, step }) => {
            (*deck, *from, leaving_code.clone(), *step)
        }
        other => unreachable!("advance_reverse: AdvanceReverse frame must be on top, got {other:?}"),
    }
}

/// Set the top `AdvanceReverse` frame's step cursor.
fn set_step(cx: &mut Cx, next: AdvanceStep) {
    match cx.state.continuations.last_mut() {
        Some(Continuation::AdvanceReverse { step, .. }) => *step = next,
        other => unreachable!("advance_reverse: AdvanceReverse frame must be on top, got {other:?}"),
    }
}

fn advanced_event(deck: AdvanceDeck, from: usize) -> Event {
    match deck {
        AdvanceDeck::Act => Event::ActAdvanced { from },
        AdvanceDeck::Agenda => Event::AgendaAdvanced { from },
    }
}

fn reverse_timing(deck: AdvanceDeck, code: crate::state::CardCode) -> super::emit::TimingEvent {
    match deck {
        AdvanceDeck::Act => super::emit::TimingEvent::ActAdvanced { code },
        AdvanceDeck::Agenda => super::emit::TimingEvent::AgendaAdvanced { code },
    }
}

/// Human label for the acknowledge prompt (1-based, e.g. "Agenda 1 advanced").
fn ack_prompt(deck: AdvanceDeck, from: usize) -> String {
    let what = match deck {
        AdvanceDeck::Act => "Act",
        AdvanceDeck::Agenda => "Agenda",
    };
    format!("{what} {} advanced — acknowledge.", from + 1)
}

/// Drive the top `AdvanceReverse` frame one step (#482). `AwaitAck` pushes the
/// observable `…Advanced` event and, when `interactive_acknowledge` is set,
/// suspends with a `Confirm` (the cursor stays at `AwaitAck` until `resume`).
/// `FireReverse` fires the leaving card's Forced reverse via `emit_event`
/// (queued; may suspend). `Finalize` bumps the deck cursor and pops the frame.
pub(super) fn drive(cx: &mut Cx) -> EngineOutcome {
    let (deck, from, leaving_code, step) = top(cx);
    match step {
        AdvanceStep::AwaitAck => {
            cx.events.push(advanced_event(deck, from));
            if cx.state.interactive_acknowledge {
                // Suspend for the acknowledge; cursor stays at AwaitAck. `resume`
                // advances to FireReverse on Confirm.
                return EngineOutcome::AwaitingInput {
                    request: InputRequest::confirm(ack_prompt(deck, from)),
                    resume_token: ResumeToken(0),
                };
            }
            set_step(cx, AdvanceStep::FireReverse);
            EngineOutcome::Done
        }
        AdvanceStep::FireReverse => {
            // Pre-advance BEFORE emitting so a suspending reverse resumes at
            // Finalize once its frames pop.
            set_step(cx, AdvanceStep::Finalize);
            super::emit::emit_event(cx, &reverse_timing(deck, leaving_code))
        }
        AdvanceStep::Finalize => {
            finalize(cx, deck, from);
            EngineOutcome::Done
        }
    }
}

/// Bump the deck cursor (RR order: after the reverse resolved) and pop the frame.
fn finalize(cx: &mut Cx, deck: AdvanceDeck, from: usize) {
    match deck {
        AdvanceDeck::Agenda => {
            cx.state.agenda_doom = 0;
            cx.state.agenda_index += 1;
            assert!(
                cx.state.agenda_index < cx.state.agenda_deck.len(),
                "advance_reverse: agenda {from} advanced past the end without a resolution \
                 (terminal agendas carry a resolution point); malformed scenario data",
            );
        }
        AdvanceDeck::Act => {
            cx.state.act_index += 1;
            assert!(
                cx.state.act_index < cx.state.act_deck.len(),
                "advance_reverse: act {from} advanced past the end without a resolution \
                 (terminal acts carry a resolution point); malformed scenario data",
            );
        }
    }
    let popped = cx.state.continuations.pop();
    debug_assert!(
        matches!(popped, Some(Continuation::AdvanceReverse { .. })),
        "advance_reverse: Finalize must pop the AdvanceReverse frame, popped {popped:?}",
    );
}

/// Resume the acknowledge pause (#482): a `Confirm` at `AwaitAck` advances the
/// cursor to `FireReverse`; the `drive` loop then fires the reverse. Validate-
/// first: a non-`Confirm`, or a frame past `AwaitAck`, rejects untouched.
pub(super) fn resume(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let (_, _, _, step) = top(cx);
    if !matches!(step, AdvanceStep::AwaitAck) {
        return EngineOutcome::Rejected {
            reason: format!("advance acknowledge: not at the acknowledge step (step {step:?})").into(),
        };
    }
    if !matches!(response, InputResponse::Confirm) {
        return EngineOutcome::Rejected {
            reason: format!("advance acknowledge: expected InputResponse::Confirm, got {response:?}").into(),
        };
    }
    set_step(cx, AdvanceStep::FireReverse);
    EngineOutcome::Done
}
```

- [ ] **Step 5: Wire the module + the `drive` loop arm + the `resolve_input` arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, add the module declaration alongside the others (e.g. after `pub(super) mod act_agenda;`'s sibling `mod abilities;` block — any position):

```rust
pub(super) mod advance_reverse;
```

In the `drive` loop's `match top`, add this arm immediately before the `SkillTest` arm (`Some(Continuation::SkillTest(_)) => …`):

```rust
            // An act/agenda advance sub-process on top (#482): drive its step
            // machine (acknowledge → reverse → finalize). A reverse it fires
            // lands above this frame and the loop drives it first; the frame is
            // re-exposed at Finalize when the reverse pops.
            Some(Continuation::AdvanceReverse { .. }) => match advance_reverse::drive(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
```

In `resolve_input`'s top-frame match, add this arm (e.g. next to the `SkillTest` arm `Some(Continuation::SkillTest(_)) => resume_skill_test_commit(cx, response)`):

```rust
        Some(Continuation::AdvanceReverse { .. }) => advance_reverse::resume(cx, response),
```

- [ ] **Step 6: Run the driver tests to verify they pass**

Run: `cargo test -p game-core --lib advance_reverse`
Expected: PASS (both).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/advance_reverse.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: AdvanceReverse frame — resumable act/agenda advance with gated ack (#482)"
```

---

### Task 2: Push the frame from `advance_agenda` / `advance_act`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/act_agenda.rs` (`advance_agenda` ~line 96; `advance_act` ~line 315; `doom_agenda_tests`)

**Interfaces:**
- Consumes: `Continuation::AdvanceReverse`, `AdvanceDeck`, `AdvanceStep` (Task 1); the `drive` loop arm (Task 1).
- Produces: `advance_agenda`/`advance_act` push an `AdvanceReverse` frame (no synchronous emit/bump).

- [ ] **Step 1: Update the blast-radius unit tests to drive the deferred frame**

In `crates/game-core/src/engine/dispatch/act_agenda.rs`, two existing tests assert `agenda_index == 1` *synchronously* after `check_doom_threshold` / `place_doom_on_current_agenda`. The advance is now deferred to the `AdvanceReverse` frame, so they must drive it (no registry ⇒ the reverse fires nothing ⇒ the frame drives straight through). Update `place_doom_on_current_agenda_advances_at_threshold` and `doom_threshold_advances_non_terminal_agenda` to drive after the call. Replace the body of `place_doom_on_current_agenda_advances_at_threshold` so it drives:

```rust
    #[test]
    fn place_doom_on_current_agenda_advances_at_threshold() {
        use crate::state::{Agenda, CardCode};
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![
            Agenda { code: CardCode("_agenda_1".into()), doom_threshold: 1, resolution: None },
            Agenda { code: CardCode("_agenda_2".into()), doom_threshold: 3, resolution: None },
        ];
        let mut events = Vec::new();
        place_doom_on_current_agenda(&mut Cx { state: &mut state, events: &mut events });
        // The advance is deferred to an AdvanceReverse frame (#482); drive it.
        crate::engine::dispatch::drive(
            &mut Cx { state: &mut state, events: &mut events },
            EngineOutcome::Done,
        );
        assert_eq!(state.agenda_index, 1, "agenda advanced at threshold");
        assert_eq!(state.agenda_doom, 0, "doom reset on advance");
    }
```

And in `doom_threshold_advances_non_terminal_agenda`, add the drive after `check_doom_threshold(...)` and before the `agenda_index` assert:

```rust
        check_doom_threshold(&mut Cx { state: &mut state, events: &mut events });
        crate::engine::dispatch::drive(
            &mut Cx { state: &mut state, events: &mut events },
            EngineOutcome::Done,
        );
        assert_eq!(state.agenda_index, 1);
```

(Add `use crate::engine::EngineOutcome;` to the test module's imports if not present; `EngineOutcome` is re-exported there. The `Event::AgendaAdvanced` assert in that test still holds — the event is pushed at `AwaitAck`.)

- [ ] **Step 2: Run those tests to verify they now FAIL against the old `advance_agenda`**

Run: `cargo test -p game-core --lib doom_agenda_tests`
Expected: FAIL — the old `advance_agenda` still bumps synchronously AND the new drive bumps again (so `agenda_index == 2`), or the frame isn't pushed. This red confirms the wiring is needed.

- [ ] **Step 3: Rewrite `advance_agenda` and `advance_act` to push the frame**

In `crates/game-core/src/engine/dispatch/act_agenda.rs`, replace the body of `advance_agenda` (~line 96):

```rust
pub(super) fn advance_agenda(cx: &mut Cx) {
    let from = cx.state.agenda_index;
    let leaving_code = cx.state.agenda_deck[from].code.clone();
    // Defer to the resumable AdvanceReverse sub-process (#482): it pushes the
    // observable event, optionally pauses for the gated acknowledge, fires the
    // leaving agenda's Forced reverse (which may suspend — 01105's ChooseOne),
    // then bumps the cursor at Finalize. The drive loop owns it from here.
    cx.state.continuations.push(crate::state::Continuation::AdvanceReverse {
        deck: crate::state::AdvanceDeck::Agenda,
        from,
        leaving_code,
        step: crate::state::AdvanceStep::AwaitAck,
    });
}
```

Replace the body of `advance_act` (~line 315) the same way:

```rust
pub(crate) fn advance_act(cx: &mut Cx) {
    let from = cx.state.act_index;
    let leaving_code = cx.state.act_deck[from].code.clone();
    // Mirror of advance_agenda (#482): defer to the AdvanceReverse sub-process.
    cx.state.continuations.push(crate::state::Continuation::AdvanceReverse {
        deck: crate::state::AdvanceDeck::Act,
        from,
        leaving_code,
        step: crate::state::AdvanceStep::AwaitAck,
    });
}
```

Remove any now-unused imports the old bodies needed (e.g. `Event` if only used there — let the compiler/clippy flag it). The past-the-end `unreachable!` and the `debug_assert!(… did not resolve to Done …)` are gone (the terminal assert moved to `advance_reverse::finalize`).

- [ ] **Step 4: Run the doom-agenda tests to verify they pass**

Run: `cargo test -p game-core --lib doom_agenda_tests`
Expected: PASS.

- [ ] **Step 5: Run the full game-core suite; note remaining blast radius**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: most pass; any failure is a test that drove an advance and asserted synchronous completion or exact frame/outcome — fix each by driving through the `AdvanceReverse` frame (the `drive(cx, Done)` idiom) or asserting the deferred frame. Record fixes in the commit message.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/act_agenda.rs
git commit -m "engine: advance_agenda/advance_act push the AdvanceReverse frame (#482)"
```

---

### Task 3: Mythos cascade defers the 1.4 draws

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (`mythos_phase` ~line 411; `anchor_on_child_pop` MythosPhase arms ~line 898)
- Test: `crates/scenarios/tests/issue_482_advance.rs` (create)

**Interfaces:**
- Consumes: `MythosResume::Draws` (Task 1); the `AdvanceReverse` frame (Tasks 1–2).
- Produces: `mythos_phase` parks the anchor at `Draws` and returns without pushing `EncounterDraw`; `anchor_on_child_pop`'s `MythosPhase{Draws}` arm runs the 1.4 draws.

- [ ] **Step 1: Write the failing regression test**

Create `crates/scenarios/tests/issue_482_advance.rs`:

```rust
//! #482 regression: advancing The Gathering's agenda 01105 via the real Mythos
//! doom-to-threshold cascade. Its Forced reverse is the lead's interactive
//! ChooseOne, which suspends. The cascade must let it resolve before the 1.4
//! draws — no stranded Effect frame / anchor_on_child_pop panic.

use game_core::action::RosterEntry;
use game_core::engine::{seat_and_open, EngineOutcome};
use game_core::state::{CardCode, GameState, InputKind};
use game_core::test_support::take_turn_action;
use game_core::{apply, Action, InputResponse, PlayerAction, TurnAction};
use scenarios::{the_gathering, REGISTRY};

#[ctor::ctor]
fn install_registries() {
    let _ = game_core::scenario_registry::install(REGISTRY);
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Seat Roland, set agenda doom to threshold-1, give the lead a card (so the
/// random-discard branch is legal and the ChooseOne genuinely suspends), and
/// `interactive_acknowledge` per the arg. Returns the state right after EndTurn.
fn drive_to_mythos_advance(interactive: bool) -> game_core::engine::ApplyResult {
    let roster = vec![RosterEntry { investigator: CardCode("01001".into()), deck: vec![] }];
    let mut state: GameState = seat_and_open(the_gathering::setup(), &roster).state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput { response: InputResponse::PickMultiple { selected: vec![] } }),
    )
    .state;
    let threshold = state.agenda_deck[state.agenda_index].doom_threshold;
    state.agenda_doom = threshold - 1;
    state.encounter_discard.clear();
    state.interactive_acknowledge = interactive;
    state
        .investigators
        .values_mut()
        .next()
        .unwrap()
        .hand
        .push(CardCode("01088".into()));
    take_turn_action(state, &TurnAction::EndTurn)
}

#[test]
fn mythos_agenda_advance_choose_one_resolves_without_panic() {
    // Flag off: no acknowledge — the live prompt is the lead's ChooseOne.
    let r = drive_to_mythos_advance(false);
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        panic!("expected the agenda ChooseOne prompt, got {:?}", r.outcome);
    };
    assert_eq!(request.kind, InputKind::PickSingle, "the lead's choose-one: {request:?}");
    assert!(request.options.len() >= 2, "two branches: {request:?}");
    // The agenda advanced; the choice is live BEFORE the encounter draws.
    assert_eq!(r.state.agenda_index, 1);
    assert_eq!(r.state.current_encounter_drawer(), None, "draws wait for the advance choice");
}

#[test]
fn mythos_agenda_advance_acknowledge_precedes_the_choice() {
    // Flag on (server path): the acknowledge Confirm precedes the ChooseOne.
    let r = drive_to_mythos_advance(true);
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        panic!("expected the advance acknowledge Confirm, got {:?}", r.outcome);
    };
    assert_eq!(request.kind, InputKind::Confirm, "{request:?}");
    // Acknowledge → the ChooseOne becomes the live prompt.
    let r2 = apply(
        r.state,
        Action::Player(PlayerAction::ResolveInput { response: InputResponse::Confirm }),
    );
    let EngineOutcome::AwaitingInput { request, .. } = &r2.outcome else {
        panic!("expected the ChooseOne after acknowledge, got {:?}", r2.outcome);
    };
    assert_eq!(request.kind, InputKind::PickSingle);
}
```

- [ ] **Step 2: Run to verify it fails (panics on the strand)**

Run: `cargo test -p scenarios --test issue_482_advance`
Expected: FAIL — `mythos_agenda_advance_choose_one_resolves_without_panic` panics in `anchor_on_child_pop` (the strand), or the outcome is the encounter-draw `Confirm` rather than the `ChooseOne` (draws ran before the choice).

- [ ] **Step 3: Park the Mythos anchor at `Draws`; stop pushing `EncounterDraw` inline**

In `crates/game-core/src/engine/dispatch/phases.rs`, in `mythos_phase` (~line 411): change the anchor push from `AfterDraws` to `Draws`, and **delete** the step-1.4 block (the `let remaining = …` through `prompt_encounter_draw(cx)` return, ~lines 446–475), replacing the function tail so it returns `Done` after `check_doom_threshold`:

```rust
    // Push the Mythos phase anchor parked at `Draws` (#482): steps 1.2/1.3 below
    // may push an `AdvanceReverse` frame whose reverse suspends; the 1.4 draws run
    // from this anchor's `Draws` resume once that frame pops, never before.
    cx.state
        .continuations
        .push(crate::state::Continuation::MythosPhase {
            resume: crate::state::MythosResume::Draws,
        });

    // 1.2 Place 1 doom on the current agenda.
    super::act_agenda::place_doom_on_agenda(cx);

    // 1.3 Check doom threshold (may push an AdvanceReverse frame above the anchor).
    super::act_agenda::check_doom_threshold(cx);

    // 1.4 runs from the anchor's `Draws` resume (anchor_on_child_pop), after any
    // advance sub-process resolves. Cede to the loop.
    EngineOutcome::Done
}
```

- [ ] **Step 4: Add the `MythosPhase{Draws}` arm to `anchor_on_child_pop`; relocate the 1.4 draws into it**

In `crates/game-core/src/engine/dispatch/phases.rs`, in `anchor_on_child_pop`'s match, add a `Draws` arm before the `AfterDraws` arm (~line 898). It re-parks the anchor at `AfterDraws` and runs the relocated 1.4-draw logic:

```rust
        Some(Continuation::MythosPhase {
            resume: MythosResume::Draws,
        }) => {
            // 1.4 encounter draws (#482): re-park the anchor at AfterDraws, then
            // run the draws. Reached once any AdvanceReverse frame (the agenda's
            // on-advance reverse) above the anchor has popped.
            cx.state.continuations.pop();
            cx.state.continuations.push(Continuation::MythosPhase {
                resume: MythosResume::AfterDraws,
            });
            let remaining = super::cursor::active_investigators_in_turn_order(cx.state);
            if remaining.is_empty() {
                // No Active drawers: open + auto-skip the post-1.4 window inline
                // (its continuation runs mythos_phase_end → Investigation).
                let outcome = super::reaction_windows::open_fast_window(
                    cx,
                    FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
                );
                debug_assert_eq!(
                    outcome,
                    EngineOutcome::Done,
                    "open_fast_window(MythosAfterDraws) unexpectedly suspended",
                );
                return EngineOutcome::Done;
            }
            cx.state
                .continuations
                .push(crate::state::Continuation::EncounterDraw { remaining });
            super::encounter::prompt_encounter_draw(cx)
        }
        Some(Continuation::MythosPhase {
            resume: MythosResume::AfterDraws,
        }) => {
```

(The existing `AfterDraws` arm body — the `debug_assert` + `mythos_phase_end(cx); EngineOutcome::Done` — stays unchanged directly below.)

Verify `FastWindowKind`, `PhaseStep`, and `super::cursor` are already in scope in this function/module (they are — the deleted `mythos_phase` block used them; keep their `use`s).

- [ ] **Step 5: Run the regression + Mythos suite**

Run: `cargo test -p scenarios --test issue_482_advance` then `cargo test -p scenarios --test mythos_phase`
Expected: PASS — the agenda choice resolves before the draws; no panic. Fix any Mythos-suite assertion that depended on `mythos_phase` pushing `EncounterDraw` inline (the draws now come one drive-step later, via the anchor).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/phases.rs crates/scenarios/tests/issue_482_advance.rs
git rm --ignore-unmatch crates/scenarios/tests/agenda_advance_repro.rs
git commit -m "engine: Mythos cascade defers the 1.4 draws past a suspending agenda reverse (#482)"
```

---

### Task 4: Act-path proof — a suspending act reverse resolves cleanly

**Files:**
- Test: `crates/cards/tests/advance_act_interactive_reverse.rs` (create) — uses a synthetic registry with an interactive act reverse.

**Interfaces:**
- Consumes: the full advance mechanism (Tasks 1–3).

- [ ] **Step 1: Write the proof test**

The simplest way to exercise a *suspending* act reverse without a real such card: install a small mock card registry whose `abilities_for` returns a `Forced` `OnEvent(ActAdvanced)` `ChooseOne` for a chosen act code, then advance the act via `advance_act_action` and drive. Create `crates/cards/tests/advance_act_interactive_reverse.rs`:

```rust
//! #482 act-path proof: an *interactive* act on-advance reverse (a synthetic
//! Forced ChooseOne) resolves cleanly through the AdvanceReverse frame — it does
//! not strand, mirroring the agenda path. No such card exists in the corpus, so
//! we install a mock registry that gives act code "_iact" a ChooseOne reverse.

use card_dsl::dsl::{choose_one, deal_horror, forced_on_event, native, Ability, EventPattern,
    EventTiming, InvestigatorTarget};
use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::engine::EngineOutcome;
use game_core::state::{Act, CardCode, InvestigatorId};
use game_core::test_support::{take_turn_action, test_investigator, GameStateBuilder};
use game_core::{InputKind, TurnAction};

const IACT: &str = "_iact";

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    (code.as_str() == IACT).then(|| {
        vec![forced_on_event(
            EventPattern::ActAdvanced,
            EventTiming::After,
            // Two always-legal branches ⇒ the choice suspends.
            choose_one(vec![
                deal_horror(InvestigatorTarget::You, 1u8),
                deal_horror(InvestigatorTarget::You, 2u8),
            ]),
        )]
    })
}

fn metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

#[ctor::ctor]
fn install() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for,
        abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
    });
}

#[test]
fn interactive_act_reverse_resolves_cleanly() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_phase(game_core::state::Phase::Investigation)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .with_investigator(test_investigator(1))
        .build();
    // Two acts: the leaving one (_iact, threshold 0 so AdvanceAct is affordable)
    // carries the interactive reverse; a successor so the advance is non-terminal.
    state.act_deck = vec![
        Act { code: CardCode(IACT.into()), clue_threshold: 0, resolution: None },
        Act { code: CardCode("_iact_2".into()), clue_threshold: 3, resolution: None },
    ];
    state.act_index = 0;

    let r = take_turn_action(state, &TurnAction::AdvanceAct { investigator: inv });

    // The act's interactive reverse is the live prompt (it did not strand); the
    // act cursor has NOT bumped yet (Finalize runs after the choice).
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        panic!("expected the act reverse ChooseOne, got {:?}", r.outcome);
    };
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert_eq!(r.state.act_index, 0, "cursor bumps only after the reverse resolves");
}
```

NOTE for the implementer: confirm `with_turn_order`, `Act` fields (`code`/`clue_threshold`/`resolution`), and the `AdvanceAct` action's affordability gate (a `clue_threshold: 0` act should be advanceable with 0 clues; if `check_advance_act` requires clues regardless, give the investigator `clues` and a real location). Adjust the fixture minimally if the affordability gate rejects — the assertion (live `PickSingle`, `act_index == 0`) is the contract.

- [ ] **Step 2: Run to verify it passes**

Run: `cargo test -p cards --test advance_act_interactive_reverse`
Expected: PASS (the act reverse suspends and is presented; no strand).

- [ ] **Step 3: Commit**

```bash
git add crates/cards/tests/advance_act_interactive_reverse.rs
git commit -m "test: act-path proof — interactive act reverse resolves via AdvanceReverse (#482)"
```

---

### Final: full CI gauntlet

- [ ] **Run the complete gauntlet** (all seven jobs, from Global Constraints). The `test` job is where remaining blast radius surfaces — any test that advanced an act/agenda and asserted synchronous completion, exact stack shape, or that `mythos_phase` pushed `EncounterDraw` inline. Fix each by driving through the `AdvanceReverse` frame (`drive(cx, Done)` / one more `apply`) or updating the assertion to the deferred shape. Never suppress the frame. Pay special attention to `crates/cards/tests/agenda_reverses.rs` (fires the reverse directly via `fire_forced_on_agenda_advance` — confirm that helper still bypasses the frame, or update it), `the_gathering.rs`, and `act_advancement.rs`.
- [ ] **No phase-doc update.** #482 is an unmilestoned `bug`/`engine`/`p1-next` issue not tracked in any `docs/phases/*` doc (consistent with its sibling bug fixes #476/#478).

## Notes for the implementer

- **The frame is the single source of truth for an advance.** `advance_agenda`/`advance_act` only push it; all of "emit event → optional acknowledge → fire reverse → bump cursor" lives in `advance_reverse::drive`. This is what makes the acknowledge apply uniformly to acts and agendas and fixes the suspending-reverse strand.
- **Why `Finalize` bumps the cursor (not `advance_*`):** RR order — the reverse resolves while the leaving card is still current; the next card becomes current after. This also retires the old synchronous-bump quirk and the misleading `debug_assert(… did not resolve to Done …)`.
- **`fire_forced_on_agenda_advance` test helper** (`test_support`) fires the reverse directly (not via the frame); it stays valid for unit-testing a reverse in isolation. The frame path is exercised by Tasks 3–4.
- Design doc: `docs/superpowers/specs/2026-06-26-482-advance-reverse-frame-design.md`.
```
