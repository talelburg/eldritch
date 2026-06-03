# Act + Agenda Decks + Push-Model Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add act/agenda deck state + a doom counter to `GameState`, advance the agenda on doom threshold and the act on a clue spend, and replace the pull-based `detect_resolution` hook with a push-model resolution latch fired from the two rules-sanctioned trigger sites.

**Architecture:** Decks are `Vec<Agenda>` / `Vec<Act>` with a cursor index on `GameState`; the printed `(→R#)` resolution point is modeled as a `resolution: Option<Resolution>` field on each entry. Dispatch sites (`check_doom_threshold`, the `AdvanceAct` handler, `check_all_defeated`) set a one-shot `state.resolution` latch via `request_resolution`; the `mod.rs::apply` hook detects the `None`→`Some` transition and emits `Event::ScenarioResolved` + runs the module's `apply_resolution`. This keeps the registry confined to `mod.rs` and fires exactly once (closing #131).

**Tech Stack:** Rust, the `game-core` kernel crate + `scenarios` content crate. CI runs `fmt` / `clippy` / `test` / `doc` / `wasm-build`, all warnings-as-errors.

**Spec:** `docs/superpowers/specs/2026-05-31-act-agenda-resolution-design.md`

**Conventions for every task:**
- Validate-first / mutate-second in every dispatch handler.
- Run single tests with `cargo test -p <crate> <test_fn_name>`.
- Commit messages: `engine: <description>` (subject), body explains *why*, ends with `Closes #73.` only on the final commit (not intermediate ones — use plain bodies mid-stream).
- Do NOT update `docs/phases/phase-4-scenario-plumbing.md` until the final task.

---

### Task 1: Act/Agenda state types + `GameState` fields

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add `Agenda`, `Act` structs + 6 fields on `GameState`)
- Modify: `crates/game-core/src/test_support/builder.rs:250-276` (`TestGame::build`)

- [ ] **Step 1: Add the `Agenda` and `Act` types**

In `crates/game-core/src/state/game_state.rs`, near the other small state structs (after the `GameState` struct definition, before `InFlightSkillTest`), add:

```rust
/// One agenda card's mechanically-relevant state: the doom needed to
/// advance it, and the printed `(→R#)` resolution point on its reverse
/// (if any). Card *effect* text is out of scope (per-scenario content);
/// `resolution` is the structural pointer that ends the scenario when a
/// terminal agenda advances.
///
/// Deliberately NOT `#[non_exhaustive]`: scenario setup in the
/// `scenarios` crate constructs these with struct literals, which a
/// `#[non_exhaustive]` struct forbids cross-crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agenda {
    /// Total doom in play required to advance (Rules Reference p.24
    /// step 1.3). Flat value only for now; per-investigator (`󲆃`)
    /// scaling and `Objective –` overrides are deferred until a real
    /// scenario needs them.
    pub doom_threshold: u8,
    /// The printed resolution point on this agenda's reverse. `Some` on
    /// a terminal agenda (advancing it ends the scenario); `None` on an
    /// agenda that advances to the next card.
    pub resolution: Option<crate::scenario::Resolution>,
}

/// One act card's mechanically-relevant state: the clues the group must
/// spend to advance it, and its `(→R#)` resolution point (if any). Not
/// `#[non_exhaustive]` for the same cross-crate-construction reason as
/// [`Agenda`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Act {
    /// Clues the investigators must spend to advance (Rules Reference
    /// p.3). Flat value only for now.
    pub clue_threshold: u8,
    /// The printed resolution point on this act's reverse. `Some` on a
    /// terminal act; `None` otherwise.
    pub resolution: Option<crate::scenario::Resolution>,
}
```

- [ ] **Step 2: Add the six fields to `GameState`**

In the `GameState` struct, after `pub encounter_discard: Vec<CardCode>,` (line ~197), add:

```rust
    /// The agenda deck (the doom-fueled lose track). `agenda_deck[agenda_index]`
    /// is the current agenda. Empty for tests/fixtures that don't model
    /// agendas — every agenda helper short-circuits on an empty deck.
    pub agenda_deck: Vec<Agenda>,
    /// Cursor into [`agenda_deck`](Self::agenda_deck): the current agenda.
    pub agenda_index: usize,
    /// Doom currently on the current agenda. Incremented +1 each Mythos
    /// step 1.2; reset to 0 when the agenda advances. (Doom on other
    /// cards in play is not summed yet — no corpus card carries doom.)
    pub agenda_doom: u8,
    /// The act deck (the investigator-driven win track). `act_deck[act_index]`
    /// is the current act. Empty for tests/fixtures that don't model acts.
    pub act_deck: Vec<Act>,
    /// Cursor into [`act_deck`](Self::act_deck): the current act.
    pub act_index: usize,
    /// Fire-once scenario-resolution latch. `None` until a resolution
    /// fires; set by `request_resolution` at the act/agenda resolution
    /// point or the no-remaining-players elimination step. The
    /// `apply` hook detects the `None`→`Some` transition to emit
    /// [`Event::ScenarioResolved`] and run `apply_resolution` exactly
    /// once (the idempotency guard formerly tracked as #131).
    pub resolution: Option<crate::scenario::Resolution>,
```

- [ ] **Step 3: Initialize the new fields in `TestGame::build`**

In `crates/game-core/src/test_support/builder.rs`, inside the `GameState { … }` literal in `build` (after `encounter_discard: Vec::new(),`, line ~274), add:

```rust
            agenda_deck: Vec::new(),
            agenda_index: 0,
            agenda_doom: 0,
            act_deck: Vec::new(),
            act_index: 0,
            resolution: None,
```

- [ ] **Step 4: Verify the workspace compiles and existing tests pass**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS (new fields are inert; nothing reads them yet). If `cargo doc` complains about the `Resolution` intra-doc path, the fully-qualified `crate::scenario::Resolution` in the field types resolves it.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/test_support/builder.rs
git commit -m "engine: add Act/Agenda state + resolution latch fields to GameState"
```

---

### Task 2: `AgendaAdvanced` / `ActAdvanced` events

**Files:**
- Modify: `crates/game-core/src/event.rs` (add two variants to `enum Event`)

- [ ] **Step 1: Add the two event variants**

In `crates/game-core/src/event.rs`, inside `pub enum Event` (just before the closing `ScenarioResolved { … }` variant near line 443), add:

```rust
    /// The agenda deck advanced: the agenda at `from` met its doom
    /// threshold and the next agenda became current. Doom was reset to
    /// 0. Not emitted when a *terminal* agenda is reached — that fires
    /// [`ScenarioResolved`] instead.
    ///
    /// [`ScenarioResolved`]: Self::ScenarioResolved
    AgendaAdvanced {
        /// The `agenda_index` of the agenda that advanced (before the
        /// cursor moved).
        from: usize,
    },
    /// The act deck advanced: the investigators spent the act at `from`'s
    /// clue threshold and the next act became current. Not emitted when
    /// a *terminal* act is reached — that fires [`ScenarioResolved`].
    ///
    /// [`ScenarioResolved`]: Self::ScenarioResolved
    ActAdvanced {
        /// The `act_index` of the act that advanced (before the cursor
        /// moved).
        from: usize,
    },
```

- [ ] **Step 2: Verify compile**

Run: `RUSTFLAGS="-D warnings" cargo build -p game-core`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/event.rs
git commit -m "engine: add AgendaAdvanced / ActAdvanced events"
```

---

### Task 3: Doom accumulation, agenda advance, and `request_resolution`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs:1056-1068` (fill `place_doom_on_agenda` + `check_doom_threshold` stubs; add `advance_agenda` + `request_resolution`)
- Test: `crates/game-core/src/engine/dispatch.rs` (`#[cfg(test)]` module at the bottom of the file)

- [ ] **Step 1: Write the failing tests**

Find the `#[cfg(test)] mod tests { … }` block at the end of `crates/game-core/src/engine/dispatch.rs`. Add these tests inside it (they call the private helpers directly — the test module has access):

```rust
#[test]
fn place_doom_increments_agenda_doom() {
    use crate::state::Agenda;
    let mut state = TestGame::new().build();
    state.agenda_deck = vec![Agenda { doom_threshold: 2, resolution: None }];
    let mut events = Vec::new();
    place_doom_on_agenda(&mut state, &mut events);
    assert_eq!(state.agenda_doom, 1);
    place_doom_on_agenda(&mut state, &mut events);
    assert_eq!(state.agenda_doom, 2);
}

#[test]
fn doom_threshold_advances_non_terminal_agenda() {
    use crate::state::Agenda;
    use crate::scenario::Resolution;
    let mut state = TestGame::new().build();
    state.agenda_deck = vec![
        Agenda { doom_threshold: 2, resolution: None },
        Agenda { doom_threshold: 2, resolution: Some(Resolution::Lost { reason: "agenda".into() }) },
    ];
    state.agenda_doom = 2;
    let mut events = Vec::new();
    check_doom_threshold(&mut state, &mut events);
    assert_eq!(state.agenda_index, 1);
    assert_eq!(state.agenda_doom, 0, "doom resets on advance");
    assert!(state.resolution.is_none(), "non-terminal advance does not resolve");
    assert_event!(events, Event::AgendaAdvanced { from } if *from == 0);
}

#[test]
fn doom_threshold_on_terminal_agenda_sets_resolution_latch() {
    use crate::state::Agenda;
    use crate::scenario::Resolution;
    let mut state = TestGame::new().build();
    state.agenda_deck = vec![
        Agenda { doom_threshold: 2, resolution: Some(Resolution::Lost { reason: "doom".into() }) },
    ];
    state.agenda_doom = 2;
    let mut events = Vec::new();
    check_doom_threshold(&mut state, &mut events);
    assert_eq!(state.agenda_index, 0, "cursor does not move on a terminal agenda");
    assert!(matches!(state.resolution, Some(Resolution::Lost { .. })));
    assert_no_event!(events, Event::AgendaAdvanced { .. });
}

#[test]
fn doom_threshold_not_met_does_nothing() {
    use crate::state::Agenda;
    let mut state = TestGame::new().build();
    state.agenda_deck = vec![Agenda { doom_threshold: 3, resolution: None }];
    state.agenda_doom = 2;
    let mut events = Vec::new();
    check_doom_threshold(&mut state, &mut events);
    assert_eq!(state.agenda_index, 0);
    assert_eq!(state.agenda_doom, 2);
    assert!(events.is_empty());
}

#[test]
fn request_resolution_is_first_writer_wins() {
    use crate::scenario::Resolution;
    let mut state = TestGame::new().build();
    request_resolution(&mut state, Resolution::Lost { reason: "first".into() });
    request_resolution(&mut state, Resolution::Won { id: "second".into() });
    assert!(matches!(state.resolution, Some(Resolution::Lost { ref reason }) if reason == "first"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core doom_threshold_advances_non_terminal_agenda`
Expected: FAIL — the stub `check_doom_threshold` does nothing / `request_resolution` and `Agenda` import unresolved.

- [ ] **Step 3: Implement `request_resolution`, fill the stubs, add `advance_agenda`**

In `crates/game-core/src/engine/dispatch.rs`, replace the two stub functions at lines ~1056-1068:

```rust
fn place_doom_on_agenda(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#73): ...
}

fn check_doom_threshold(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#73): ...
}
```

with:

```rust
/// Mythos step 1.2 (Rules Reference p.24): "Take 1 doom from the token
/// pool, and place it on the current agenda card." No-op when no agenda
/// deck is modeled (tests/fixtures without an agenda).
fn place_doom_on_agenda(state: &mut GameState, _events: &mut Vec<Event>) {
    if state.agenda_deck.is_empty() {
        return;
    }
    state.agenda_doom = state.agenda_doom.saturating_add(1);
}

/// Mythos step 1.3 (Rules Reference p.24): compare doom in play with the
/// current agenda's threshold; if met, the agenda advances. We model
/// doom only on the agenda (no corpus card carries doom yet — summing
/// "doom on each other card in play" would add zero).
///
/// TODO(#73 follow-up): sum doom on other cards in play once a
/// doom-bearing card exists.
///
/// If the current agenda is terminal (carries a `resolution`), advancing
/// it ends the scenario: set the resolution latch instead of moving the
/// cursor. Otherwise emit [`Event::AgendaAdvanced`], reset doom, and make
/// the next agenda current.
fn check_doom_threshold(state: &mut GameState, events: &mut Vec<Event>) {
    if state.agenda_deck.is_empty() {
        return;
    }
    let agenda = &state.agenda_deck[state.agenda_index];
    if state.agenda_doom < agenda.doom_threshold {
        return;
    }
    match agenda.resolution.clone() {
        Some(resolution) => request_resolution(state, resolution),
        None => advance_agenda(state, events),
    }
}

/// Advance the agenda deck one step: emit [`Event::AgendaAdvanced`],
/// reset doom (Rules Reference p.24: "remove all doom from play"), and
/// move the cursor to the next agenda.
///
/// Only ever called for a *non-terminal* agenda (one whose `resolution`
/// is `None`). A non-terminal agenda must have a successor; reaching the
/// end of the deck without a resolution firing is malformed scenario
/// data (the final agenda must carry a `(→R#)` resolution point), so the
/// missing-successor case is `unreachable!()` — mirrors the surge-chain
/// malformation guards from #69.
fn advance_agenda(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.agenda_index;
    events.push(Event::AgendaAdvanced { from });
    state.agenda_doom = 0;
    state.agenda_index += 1;
    if state.agenda_index >= state.agenda_deck.len() {
        unreachable!(
            "advance_agenda: agenda {from} advanced past the end of the deck without a \
             resolution firing — a terminal agenda must carry a resolution point; this is \
             malformed scenario data"
        );
    }
}

/// Set the scenario-resolution latch. First-writer-wins: a resolution
/// already latched this scenario is authoritative and a later request is
/// ignored. The `apply` hook (in `engine::mod`) observes the `None`→`Some`
/// transition to emit [`Event::ScenarioResolved`] and run the scenario
/// module's `apply_resolution` exactly once.
fn request_resolution(state: &mut GameState, resolution: crate::scenario::Resolution) {
    if state.resolution.is_none() {
        state.resolution = Some(resolution);
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p game-core doom_threshold && cargo test -p game-core place_doom_increments && cargo test -p game-core request_resolution_is_first_writer`
Expected: PASS (all five new tests).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: doom accumulation + threshold-driven agenda advance (Mythos 1.2/1.3)"
```

---

### Task 4: `PlayerAction::AdvanceAct` (prototype) + clue spend + `advance_act`

**Files:**
- Modify: `crates/game-core/src/action.rs` (add `AdvanceAct` variant to `PlayerAction`)
- Modify: `crates/game-core/src/engine/dispatch.rs:139-183` (dispatch arm) + add `advance_act_action` / `advance_act` / `spend_clues` helpers
- Test: `crates/game-core/src/engine/dispatch.rs` (`#[cfg(test)]` module)

- [ ] **Step 1: Add the `AdvanceAct` variant**

In `crates/game-core/src/action.rs`, inside `pub enum PlayerAction`, after the `EndTurn` variant (or any other simple variant), add:

```rust
    /// Spend clues to advance the current act (Rules Reference p.3:
    /// "spend the requisite number of clues … normally a Fast player
    /// ability").
    ///
    /// **Prototype.** Built minimally for the Phase-4 synthetic demo: a
    /// flat `clue_threshold`, a single-step spend with a deterministic
    /// allocation (the acting investigator's clues first, then the rest
    /// in `turn_order`), no `Objective –` handling, and no per-action
    /// Fast-window gating. Real consumers (Phase 7, The Gathering) drive
    /// its final form — see the design spec.
    AdvanceAct {
        /// The investigator initiating the spend (the "acting" player;
        /// their clues are spent first when the group holds a surplus).
        investigator: InvestigatorId,
    },
```

- [ ] **Step 2: Write the failing tests**

In the `#[cfg(test)]` module at the bottom of `crates/game-core/src/engine/dispatch.rs`, add:

```rust
#[test]
fn advance_act_rejects_when_clues_insufficient() {
    use crate::state::Act;
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 1;
    let mut state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![Act { clue_threshold: 2, resolution: None }];

    let result = apply(state, Action::Player(PlayerAction::AdvanceAct { investigator: inv }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.act_index, 0);
    assert_eq!(result.state.investigators[&inv].clues, 1, "no clues spent on reject");
}

#[test]
fn advance_act_spends_clues_and_advances_non_terminal() {
    use crate::state::Act;
    use crate::scenario::Resolution;
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 3;
    let mut state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![
        Act { clue_threshold: 2, resolution: None },
        Act { clue_threshold: 2, resolution: Some(Resolution::Won { id: "demo".into() }) },
    ];

    let result = apply(state, Action::Player(PlayerAction::AdvanceAct { investigator: inv }));
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.act_index, 1);
    assert_eq!(result.state.investigators[&inv].clues, 1, "spent exactly 2 of 3");
    assert!(result.state.resolution.is_none());
    assert_event!(result.events, Event::ActAdvanced { from } if *from == 0);
}

#[test]
fn advance_act_on_terminal_act_sets_resolution_latch() {
    use crate::state::Act;
    use crate::scenario::Resolution;
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 2;
    let mut state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![Act { clue_threshold: 2, resolution: Some(Resolution::Won { id: "demo".into() }) }];

    let result = apply(state, Action::Player(PlayerAction::AdvanceAct { investigator: inv }));
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.act_index, 0, "cursor does not move on a terminal act");
    assert!(matches!(result.state.resolution, Some(Resolution::Won { .. })));
    assert_no_event!(result.events, Event::ActAdvanced { .. });
    assert_eq!(result.state.investigators[&inv].clues, 0);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p game-core advance_act`
Expected: FAIL — `PlayerAction::AdvanceAct` has no dispatch arm (non-exhaustive match) / helpers undefined.

- [ ] **Step 4: Add the dispatch arm**

In `crates/game-core/src/engine/dispatch.rs`, in the `match action` block of `apply_player_action` (after the `ResolveInput` arm at line ~182, or alongside the others), add:

```rust
        PlayerAction::AdvanceAct { investigator } => {
            advance_act_action(state, events, *investigator)
        }
```

- [ ] **Step 5: Implement the handler + `spend_clues` + `advance_act`**

Add to `crates/game-core/src/engine/dispatch.rs` (near the other act/agenda helpers from Task 3):

```rust
/// Handler for [`PlayerAction::AdvanceAct`] — a prototype clue-spend to
/// advance the current act (see the action's doc comment and the design
/// spec). Validate-first: reject outside the Investigation phase, when no
/// act deck is modeled, or when the group holds fewer clues than the
/// current act's `clue_threshold`. On success spend exactly the threshold
/// (acting investigator first, then the rest in `turn_order`) and either
/// set the resolution latch (terminal act) or emit [`Event::ActAdvanced`]
/// and advance the cursor.
fn advance_act_action(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "AdvanceAct is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        };
    }
    if state.act_deck.is_empty() {
        return EngineOutcome::Rejected {
            reason: "AdvanceAct: no act deck is modeled for this scenario".into(),
        };
    }
    let threshold = state.act_deck[state.act_index].clue_threshold;
    let total_clues: u32 = state.investigators.values().map(|i| u32::from(i.clues)).sum();
    if total_clues < u32::from(threshold) {
        return EngineOutcome::Rejected {
            reason: format!(
                "AdvanceAct: act requires {threshold} clues, group holds {total_clues}"
            )
            .into(),
        };
    }

    // All validations passed — mutate.
    spend_clues(state, investigator, threshold);
    match state.act_deck[state.act_index].resolution.clone() {
        Some(resolution) => request_resolution(state, resolution),
        None => advance_act(state, events),
    }
    EngineOutcome::Done
}

/// Spend `amount` clues from the group, deterministically: the acting
/// investigator's clues first, then the remaining investigators in
/// `turn_order`. Callers must have already validated the group holds at
/// least `amount` clues, so the spend always completes.
///
/// TODO(#73 follow-up — Phase 8): let players choose who contributes
/// when the group holds a surplus (an `AwaitingInput` allocation prompt).
/// The fixed order here is outcome-equivalent single-player.
fn spend_clues(state: &mut GameState, acting: InvestigatorId, amount: u8) {
    let mut remaining = amount;
    // Acting investigator first, then turn_order (skipping the acting
    // one so it isn't drained twice).
    let order = std::iter::once(acting).chain(state.turn_order.iter().copied().filter(|id| *id != acting));
    let ids: Vec<InvestigatorId> = order.collect();
    for id in ids {
        if remaining == 0 {
            break;
        }
        if let Some(inv) = state.investigators.get_mut(&id) {
            let take = inv.clues.min(remaining);
            inv.clues -= take;
            remaining -= take;
        }
    }
    debug_assert_eq!(remaining, 0, "spend_clues called without enough clues in the group");
}

/// Advance the act deck one step: emit [`Event::ActAdvanced`] and move the
/// cursor. Only called for a non-terminal act; the missing-successor case
/// is `unreachable!()` (a terminal act must carry a resolution point —
/// malformed scenario data otherwise). Mirrors [`advance_agenda`].
fn advance_act(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.act_index;
    events.push(Event::ActAdvanced { from });
    state.act_index += 1;
    if state.act_index >= state.act_deck.len() {
        unreachable!(
            "advance_act: act {from} advanced past the end of the deck without a resolution \
             firing — a terminal act must carry a resolution point; this is malformed \
             scenario data"
        );
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p game-core advance_act`
Expected: PASS (all three). The `apply` calls use no installed global registry, so `state.resolution` is set by dispatch but no `ScenarioResolved` event fires — the latch assertions cover the dispatch behavior.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/action.rs crates/game-core/src/engine/dispatch.rs
git commit -m "engine: PlayerAction::AdvanceAct prototype — spend clues to advance the act"
```

---

### Task 5: Replace pull-model `detect_resolution` with the push-model latch hook

**Files:**
- Modify: `crates/game-core/src/scenario.rs:84-111` (remove `detect_resolution` from `ScenarioModule`)
- Modify: `crates/game-core/src/engine/mod.rs:86-142` (rework `apply_with_scenario_registry` + `fire_scenario_resolution`)
- Modify: `crates/game-core/src/engine/mod.rs:3745-3899` (rewrite the resolution-hook unit tests for the push model)
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs` (drop `detect_resolution`; seed 2 agendas + 2 acts)
- Modify: `crates/scenarios/tests/synthetic_resolution.rs` (push-model rewrite)

This is the atomic "flip" — removing a struct field forces all referencing sites to change together.

- [ ] **Step 1: Remove `detect_resolution` from `ScenarioModule`**

In `crates/game-core/src/scenario.rs`, delete the entire `detect_resolution` field (the doc comment + `pub detect_resolution: fn(&GameState) -> Option<Resolution>,`, lines ~92-102). Update the struct doc comment's mention of the post-apply hook to describe the latch model. The struct becomes:

```rust
#[derive(Debug, Clone, Copy)]
pub struct ScenarioModule {
    /// Build the scenario's initial [`GameState`]. Places locations,
    /// populates encounter / act / agenda decks, sets chaos-bag
    /// modifiers, etc.
    pub setup: fn() -> GameState,
    /// Apply the resolution's effects (XP, trauma, scenario-end cleanup).
    /// Called by [`apply`](crate::engine::apply) exactly once, when the
    /// engine observes `GameState.resolution` transition from `None` to
    /// `Some` during an apply. Receives the events buffer so changes are
    /// observable to clients.
    ///
    /// For the Phase-4 synthetic fixture this is a no-op. Phase 9 fills in
    /// real bodies once the campaign log lands.
    pub apply_resolution: fn(&Resolution, &mut GameState, &mut Vec<Event>),
}
```

- [ ] **Step 2: Rework the `apply` hook**

In `crates/game-core/src/engine/mod.rs`, in `apply_with_scenario_registry` (starts line ~86), capture the latch before dispatch and fire after. Replace the body's dispatch+hook region. The current shape is:

```rust
    let outcome = match action { … };
    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        events.clear();
    } else if matches!(outcome, EngineOutcome::Done) {
        fire_scenario_resolution(&mut state, &mut events, registry);
    }
    ApplyResult { state, events, outcome }
```

Change to:

```rust
    let resolution_already_fired = state.resolution.is_some();
    let outcome = match action { … };   // leave the match arms unchanged
    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        events.clear();
    } else if !resolution_already_fired {
        // A dispatch site may have set the resolution latch this apply
        // (act/agenda resolution point, or no-remaining-players
        // elimination). Fire the module's hook exactly once, on the
        // None->Some transition. Runs on Done AND AwaitingInput (a
        // resolution can latch during an apply that pauses, e.g. doom
        // crosses the threshold in Mythos 1.3 before the 1.4 draw
        // pause).
        fire_scenario_resolution(&mut state, &mut events, registry);
    }
    ApplyResult { state, events, outcome }
```

Then rewrite `fire_scenario_resolution` (lines ~112-142):

```rust
/// Post-dispatch hook: if a dispatch site latched a resolution this apply
/// (`state.resolution` went `None`→`Some`), emit [`Event::ScenarioResolved`]
/// and run the active scenario module's `apply_resolution`. Caller guards
/// the `None`→`Some` transition (it checks `state.resolution.is_some()`
/// *before* dispatch), so this fires exactly once per scenario.
///
/// Short-circuits when no resolution latched, when the state has no
/// `scenario_id`, or when no module is registered for it (the event still
/// can't be applied without a module, but the latch stays set).
fn fire_scenario_resolution(
    state: &mut GameState,
    events: &mut Vec<Event>,
    registry: Option<&ScenarioRegistry>,
) {
    let Some(resolution) = state.resolution.clone() else {
        return;
    };
    events.push(Event::ScenarioResolved {
        resolution: resolution.clone(),
    });
    let Some(id) = state.scenario_id.as_ref() else {
        return;
    };
    let Some(reg) = registry else { return };
    let Some(module) = (reg.module_for)(id) else {
        return;
    };
    (module.apply_resolution)(&resolution, state, events);
}
```

Note: `ScenarioResolved` now fires whenever the latch is set, even without a registry/module — the resolution is a property of engine state, not the registry. Only `apply_resolution` needs the module.

- [ ] **Step 3: Rewrite the resolution-hook unit tests**

In `crates/game-core/src/engine/mod.rs` (the `#[cfg(test)]` region, lines ~3745-3920), the old tests construct mock modules with `detect_resolution` and drive `StartScenario`. Replace them with push-model tests that latch a resolution via a real push site (`AdvanceAct` on a terminal act). Replace the mock-module helpers and tests:

```rust
    use crate::scenario::{Resolution, ScenarioId, ScenarioModule, ScenarioRegistry};
    use crate::state::Act;

    /// `apply_resolution` that records it ran by stamping the acting
    /// investigator's resources to a sentinel value, so tests can assert
    /// the module hook (not just the event) fired.
    fn stamp_apply(_res: &Resolution, state: &mut crate::state::GameState, _events: &mut Vec<Event>) {
        if let Some(inv) = state.investigators.values_mut().next() {
            inv.resources = 99;
        }
    }

    fn unused_setup() -> crate::state::GameState {
        TestGame::new().build()
    }

    static STAMP_MODULE: ScenarioModule = ScenarioModule {
        setup: unused_setup,
        apply_resolution: stamp_apply,
    };

    fn stamp_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        if id.as_str() == "stamp" { Some(&STAMP_MODULE) } else { None }
    }

    /// Build an Investigation-phase state whose current (only) act is
    /// terminal and whose investigator holds exactly enough clues to
    /// advance it — so a single `AdvanceAct` latches `Won`.
    fn terminal_act_state(scenario_id: Option<&str>) -> crate::state::GameState {
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 1;
        let mut builder = TestGame::new()
            .with_phase(crate::state::Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv]);
        if let Some(id) = scenario_id {
            builder = builder.with_scenario_id(ScenarioId::new(id));
        }
        let mut state = builder.build();
        state.act_deck = vec![Act {
            clue_threshold: 1,
            resolution: Some(Resolution::Won { id: "test".into() }),
        }];
        state
    }

    #[test]
    fn resolution_fires_and_applies_when_latch_set_with_module() {
        let state = terminal_act_state(Some("stamp"));
        let reg = ScenarioRegistry { module_for: stamp_module_for };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: InvestigatorId(1) }),
            Some(&reg),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "test"
        );
        assert_eq!(
            result.state.investigators[&InvestigatorId(1)].resources, 99,
            "apply_resolution ran"
        );
    }

    #[test]
    fn resolution_event_fires_without_a_registered_module() {
        // No registry: the event still fires (resolution is engine state),
        // but apply_resolution can't run.
        let state = terminal_act_state(Some("unknown"));
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: InvestigatorId(1) }),
            None,
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::ScenarioResolved { .. });
    }

    #[test]
    fn resolution_does_not_refire_on_a_later_apply() {
        let state = terminal_act_state(Some("stamp"));
        let reg = ScenarioRegistry { module_for: stamp_module_for };
        // First apply latches + fires.
        let first = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: InvestigatorId(1) }),
            Some(&reg),
        );
        assert_event!(first.events, Event::ScenarioResolved { .. });
        // A second apply (EndTurn) must NOT re-emit — latch already set.
        let second = super::apply_with_scenario_registry(
            first.state,
            Action::Player(PlayerAction::EndTurn),
            Some(&reg),
        );
        assert_no_event!(second.events, Event::ScenarioResolved { .. });
    }

    #[test]
    fn resolution_skipped_on_rejected_outcome() {
        // Rejected AdvanceAct (insufficient clues) latches nothing.
        let inv = InvestigatorId(1);
        let mut state = terminal_act_state(Some("stamp"));
        state.investigators.get_mut(&inv).unwrap().clues = 0;
        let reg = ScenarioRegistry { module_for: stamp_module_for };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
            Some(&reg),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_no_event!(result.events, Event::ScenarioResolved { .. });
    }
```

Delete the old `always_wins` / `never_resolves` / `no_op_apply` / `ALWAYS_WINS_MODULE` / `NEVER_RESOLVES_MODULE` / `*_module_for` items and the five old `scenario_resolution_*` tests they backed.

- [ ] **Step 4: Update the synthetic fixture**

In `crates/scenarios/src/test_fixtures/synthetic.rs`: delete the `detect_resolution` fn (lines ~65-78) and drop it from the `MODULE` literal. Seed the act/agenda decks in `setup()`. After `state.encounter_deck.push_back(...)` and before `state`, add:

```rust
    use game_core::state::{Act, Agenda};
    state.agenda_deck = vec![
        Agenda { doom_threshold: 2, resolution: None },
        Agenda { doom_threshold: 2, resolution: Some(Resolution::Lost { reason: "agenda".into() }) },
    ];
    state.act_deck = vec![
        Act { clue_threshold: 2, resolution: None },
        Act { clue_threshold: 2, resolution: Some(Resolution::Won { id: "demo".into() }) },
    ];
```

Update the `MODULE` const to drop `detect_resolution`:

```rust
pub const MODULE: ScenarioModule = ScenarioModule {
    setup,
    apply_resolution,
};
```

Update the module-level/`setup` doc comments that mention `detect_resolution` to describe the seeded decks instead.

- [ ] **Step 5: Rewrite the synthetic integration test**

Replace `crates/scenarios/tests/synthetic_resolution.rs`'s single test. The old test asserted resolution fires after `StartScenario` (the old pull predicate). Under the push model, `StartScenario` alone resolves nothing. Replace with a Won-via-act test:

```rust
#[test]
fn synthetic_scenario_resolves_won_via_act_advance() {
    install_registry();
    let inv = InvestigatorId(1);
    let mut state = scenarios::test_fixtures::synthetic::setup();

    // StartScenario + close the mulligan window -> Investigation, round 1.
    let (mut state, _) = drive(
        state,
        vec![
            Action::Player(PlayerAction::StartScenario),
            Action::Player(PlayerAction::Mulligan { investigator: inv, indices_to_redraw: vec![] }),
        ],
    );
    assert_eq!(state.phase, Phase::Investigation);

    // Seed enough clues to advance both acts (2 + 2), then spend twice.
    state.investigators.get_mut(&inv).unwrap().clues = 4;
    let (state, events) = drive(
        state,
        vec![
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }), // act 0 -> 1
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }), // act 1 -> Won
        ],
    );

    assert_event!(
        events,
        Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "demo"
    );
    assert!(state.resolution.is_some());
}
```

Add a local `drive` helper (copy from `mythos_phase.rs:47-59`) and the needed imports (`InvestigatorId`, `assert_event`). Keep the `install_registry` / `INSTALL` scaffolding.

- [ ] **Step 6: Run the full test suite**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS. Watch for: any other references to `detect_resolution` (grep `rg detect_resolution crates/` — should return nothing after this task).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/scenario.rs crates/game-core/src/engine/mod.rs crates/scenarios/src/test_fixtures/synthetic.rs crates/scenarios/tests/synthetic_resolution.rs
git commit -m "engine: replace pull-model detect_resolution with push-model resolution latch"
```

---

### Task 6: Wire elimination step 6 (no remaining players) to the resolution latch

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs:3051-3062` (`check_all_defeated`)
- Modify: `crates/game-core/src/engine/dispatch.rs` (remove the `TODO(#144)`/`TODO(#73)` at the #137 no-active-investigator park site)
- Test: `crates/game-core/src/engine/dispatch.rs` (`#[cfg(test)]` module)

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)]` module of `crates/game-core/src/engine/dispatch.rs`:

```rust
#[test]
fn last_investigator_defeated_latches_lost_resolution() {
    // Single investigator; defeat them and assert the no-remaining-players
    // resolution latch is set (Rules Reference p.10 step 6).
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.max_sanity = 1;
    let mut state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    let mut events = Vec::new();

    // Apply lethal horror through the standard defeat path.
    take_horror(&mut state, &mut events, inv, 1);

    assert_event!(events, Event::AllInvestigatorsDefeated);
    assert!(
        matches!(state.resolution, Some(crate::scenario::Resolution::Lost { .. })),
        "no-remaining-players must latch Lost"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core last_investigator_defeated_latches_lost_resolution`
Expected: FAIL — `state.resolution` is `None` (nothing latches it yet).

- [ ] **Step 3: Latch the resolution in `check_all_defeated`**

In `crates/game-core/src/engine/dispatch.rs`, update `check_all_defeated` to latch the "no resolution was reached" Lost when it emits `AllInvestigatorsDefeated`:

```rust
fn check_all_defeated(state: &mut GameState, events: &mut Vec<Event>) {
    let any_active = state
        .investigators
        .values()
        .any(|inv| inv.status == Status::Active);
    if !any_active && !state.investigators.is_empty() {
        events.push(Event::AllInvestigatorsDefeated);
        // Rules Reference p.10 step 6: "If there are no remaining players,
        // the scenario ends. Refer to the 'no resolution was reached'
        // entry for that scenario." Latch the loss (first-writer-wins, so
        // an already-fired act/agenda resolution stays authoritative).
        request_resolution(
            state,
            crate::scenario::Resolution::Lost {
                reason: "no resolution was reached".into(),
            },
        );
    }
}
```

Note `check_all_defeated`'s signature already takes `&mut GameState` — confirm; if it currently takes `&GameState`, widen it to `&mut GameState` and update its single caller (`apply_investigator_defeat`).

- [ ] **Step 4: Remove the now-resolved park TODO**

Grep for the #137 no-active-investigator park comment: `rg "TODO\(#144\)|park" crates/game-core/src/engine/dispatch.rs`. At the `InvestigationBegins`-continuation park site (where it does nothing when no `Status::Active` investigator exists), the `TODO(#144)`/`TODO(#73)` noting "scenario-end lands with #73" is now satisfied — elimination latches `Lost`, the `apply` hook fires `ScenarioResolved`. Update that comment to state the resolution now fires via `check_all_defeated` + the latch hook (don't delete the park branch itself — it's still the cascade-breaker; just drop the stale TODO).

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p game-core last_investigator_defeated_latches_lost_resolution`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: latch Lost resolution when the last investigator is eliminated (RR p.10 step 6)"
```

---

### Task 7: Integration test — doom-to-Lost playthrough

**Files:**
- Modify: `crates/scenarios/tests/synthetic_resolution.rs` (add the doom playthrough)

- [ ] **Step 1: Write the doom-to-Lost test**

The synthetic fixture seeds agendas `[threshold 2 (non-terminal), threshold 2 (terminal Lost)]`. Round 1 skips Mythos; each subsequent Mythos adds 1 doom. So Mythos at round 3 advances agenda 0→1; Mythos at round 5 hits the terminal agenda and latches `Lost`. Each round is one `EndTurn` (cascades to the Mythos draw pause) + one `DrawEncounterCard` (completes Mythos → Investigation).

Add to `crates/scenarios/tests/synthetic_resolution.rs`:

```rust
#[test]
fn synthetic_scenario_resolves_lost_via_doom() {
    install_registry();
    let inv = InvestigatorId(1);
    let mut base = scenarios::test_fixtures::synthetic::setup();
    base.encounter_discard.clear();

    // Setup + close mulligan -> Investigation, round 1.
    let (mut state, _) = drive(
        base,
        vec![
            Action::Player(PlayerAction::StartScenario),
            Action::Player(PlayerAction::Mulligan { investigator: inv, indices_to_redraw: vec![] }),
        ],
    );

    // Each round: EndTurn (-> Mythos draw pause) + DrawEncounterCard
    // (-> Investigation). Collect events across all rounds; stop after
    // the round whose Mythos crosses the terminal agenda's threshold.
    let mut all_events = Vec::new();
    for _ in 0..4 {
        let r1 = apply(state, Action::Player(PlayerAction::EndTurn));
        all_events.extend(r1.events);
        state = r1.state;
        // EndTurn pauses at Mythos 1.4 awaiting the draw (AwaitingInput);
        // the doom check (1.3) already ran this apply.
        let r2 = apply(state, Action::Player(PlayerAction::DrawEncounterCard));
        all_events.extend(r2.events);
        state = r2.state;
    }

    // Agenda 0 advanced once (round 3 Mythos), then the terminal agenda
    // latched Lost (round 5 Mythos).
    assert_event!(all_events, Event::AgendaAdvanced { from } if *from == 0);
    assert_event!(
        all_events,
        Event::ScenarioResolved { resolution: Resolution::Lost { .. } }
    );
    assert!(matches!(state.resolution, Some(Resolution::Lost { .. })));
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p scenarios --test synthetic_resolution synthetic_scenario_resolves_lost_via_doom`
Expected: PASS.

If the round count is off (the resolution fires a round earlier/later than the loop covers), adjust the `0..4` bound to match the observed doom cadence — print `state.round` / `state.agenda_doom` / `state.agenda_index` after each iteration to confirm: agenda advances when doom first reaches 2 (round-3 Mythos), terminal latch when doom next reaches 2 (round-5 Mythos). The loop must run enough rounds to reach the terminal latch.

- [ ] **Step 3: Commit**

```bash
git add crates/scenarios/tests/synthetic_resolution.rs
git commit -m "engine: integration test — synthetic scenario resolves Lost via doom"
```

---

### Task 8: Follow-up issue, full CI gauntlet, and phase-doc update

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

- [ ] **Step 1: File the clue-allocation follow-up issue**

```bash
gh issue create \
  --title "[engine] Player-chosen clue allocation when advancing the act on a surplus" \
  --label engine --label p2-later \
  --body "$(cat <<'EOF'
`PlayerAction::AdvanceAct` (the #73 prototype) spends clues in a deterministic
order — the acting investigator first, then the rest in `turn_order` — when the
group holds more clues than the act's threshold. Per Rules Reference p.3 ("Any
or all investigators may contribute any number of clues"), players should choose
who contributes how many.

Add an `AwaitingInput` allocation prompt when the group holds a surplus.
Multiplayer-only (the fixed order is outcome-equivalent single-player), so this
sits with the other Phase-8 interactive-choice deferrals (cf. #151).

Follow-up from #73.
EOF
)"
```

Capture the new issue number from the output for the phase-doc entry.

- [ ] **Step 2: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```

Expected: all five PASS. Fix any clippy/doc findings before proceeding (e.g., intra-doc links to `Resolution` / `Agenda` / `Act`, unused imports).

- [ ] **Step 3: Update the phase doc**

In `docs/phases/phase-4-scenario-plumbing.md`:
- Move `#73` from the open Issues table to the **Closed** table with its PR number and a one-line summary (push-model resolution replacing #74's pull `detect_resolution`; act/agenda decks + doom; `AdvanceAct` prototype; elimination step 6 wired; #131 latch folded in).
- Flip the Ordering row for slot #12 (`#73`) to `✅ PR #N`.
- Update the Status line and open-issue count (the clue-allocation follow-up from Step 1 joins the open/follow-up list; `#73` leaves it).
- Add a **Decisions made** entry only for what's load-bearing for future PRs: the push-model resolution shape (latch + two trigger sites, `detect_resolution` removed, #131 closed) and that `AdvanceAct` is a prototype awaiting Phase-7 consumers. Remove the now-settled note about `#73` wiring `Resolution::Lost` making the #137 park unreachable (it's done).
- Note the closing-demo slot (#13) still owns replay-determinism testing.

Do this as the final commit so it reflects the shipping state.

- [ ] **Step 4: Commit**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "engine: act/agenda decks + push-model resolution (#73)

Replaces #74's pull-based detect_resolution with a push-model resolution
latch fired at the two rules-sanctioned trigger sites (act/agenda
resolution point; no-remaining-players elimination). Adds act/agenda deck
state + a doom counter, doom-threshold agenda advance, and a prototype
AdvanceAct clue-spend. Folds in the #131 fire-once latch.

Closes #73."
```

---

## Self-Review notes (for the executor)

- **Spec coverage:** state (T1) · events (T2) · doom + agenda advance (T3) · act advance via `AdvanceAct` (T4) · push-model resolution + `detect_resolution` removal + synthetic rework (T5) · elimination step 6 (T6) · doom-to-Lost integration (T7) · Won-via-act integration (T5 step 5) · follow-up issue + phase doc (T8). Replay determinism intentionally out (slot-13 scope).
- **Type consistency:** `Agenda { doom_threshold, resolution }`, `Act { clue_threshold, resolution }`, `Event::AgendaAdvanced { from }` / `ActAdvanced { from }`, `request_resolution(&mut GameState, Resolution)`, `advance_agenda`/`advance_act`/`spend_clues`/`advance_act_action` are referenced consistently across tasks.
- **`AwaitingInput` firing:** T5 step 2's hook runs on any non-`Rejected` outcome so the doom-crosses-in-Mythos case (T7), which ends `AwaitingInput` at the 1.4 draw pause, still fires `ScenarioResolved`.
- **Registry boundary:** dispatch sets the latch (no registry); `mod.rs` emits the event + runs `apply_resolution` (registry-local). Unit tests in `game-core` assert on the latch; the event/apply path is covered in `mod.rs` tests (mock registry) + `scenarios` integration tests (real registry).
