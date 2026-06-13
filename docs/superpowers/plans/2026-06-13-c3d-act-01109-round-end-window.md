# C3d — Act-2 (01109) Round-End Clue-Spend Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement act 01109's round-end Objective — at the end of the round, Hallway investigators may, as a group, spend the act's `clue_threshold` clues to advance — as a suspendable kernel window.

**Architecture:** A generic `Act.round_end_advance: Option<RoundEndAdvance>` (threshold from the corpus, objective shape content-set in `the_gathering.rs`). `upkeep_phase_end` is threaded to `EngineOutcome` and, after the round-end forced dispatch, opens a `Confirm/Skip` window when the current act has the objective and the contributor-location investigators can afford it. Suspension mirrors the hand-size-discard pattern (`act_round_end_pending` field + action-gate guard + `resolve_input` routing + a resume fn). `AdvanceAct` is re-gated for round-end-advance acts.

**Tech Stack:** Rust workspace (`game-core`, `scenarios`, `cards`). CI gauntlet per `CLAUDE.md`.

**Spec:** `docs/superpowers/specs/2026-06-13-phase-7-c3d-act-01109-round-end-window-design.md` (issue #275).

**Branch:** `engine/act-01109-round-end` (already checked out; carries the spec commit).

**Verified facts:** `Act` lives in `crates/game-core/src/state/game_state.rs` (`{ code, clue_threshold, resolution }`); the only `GameState { … }` construction is the builder's `build()` (so the new pending field needs init only there). `ResumeToken(0)` is the established placeholder (routing is by pending-field). `step_phase`, `upkeep_phase_end` return-threading targets, and `advance_act` (`pub(crate)`) are in `phases.rs`/`act_agenda.rs`. By the time act 2 is current, the Hallway (01112) is in play and investigators have been relocated there (act-1 board build).

---

### Task 1: `Act.round_end_advance` + content wiring + `AdvanceAct` re-gating

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`RoundEndAdvance` struct + `Act` field)
- Modify: every `state::Act { … }` literal in the workspace (add `round_end_advance: None`) — enumerated below
- Modify: `crates/scenarios/src/the_gathering.rs` (set `Some(..)` for 01109)
- Modify: `crates/game-core/src/engine/dispatch/act_agenda.rs` (`advance_act_action` re-gating + test)

- [ ] **Step 1: Add `RoundEndAdvance` + the `Act` field**

In `crates/game-core/src/state/game_state.rs`, add the struct near `Act` (mirror `Act`'s derives — check the `#[derive(...)]` on `Act` and copy it):

```rust
/// A round-end "may spend clues to advance" objective (Rules Reference:
/// act objectives). 01109 "The Barrier": investigators in the Hallway may,
/// as a group, spend the act's `clue_threshold` clues to advance when the
/// round ends. Generic mechanics — only the contributor location is
/// card-specific, so it is set by content (`the_gathering.rs`), not parsed
/// from the corpus (no structured ArkhamDB field exists for it; single
/// consumer). See issue #275.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundEndAdvance {
    /// Only investigators at this in-play location (by printed code) may
    /// contribute clues — 01109: the Hallway `01112`.
    pub contributor_location: CardCode,
}
```

Add the field to `Act` (after `resolution`):

```rust
    /// When `Some`, this act offers a round-end clue-spend objective
    /// instead of an Investigation-phase `AdvanceAct` (see [`RoundEndAdvance`]).
    /// `None` for acts that advance by the normal action or a forced trigger.
    pub round_end_advance: Option<RoundEndAdvance>,
```

- [ ] **Step 2: Add `round_end_advance: None` to every `state::Act` literal**

Run to enumerate the literals (the multi-line `state::Act { code: …, clue_threshold: …, resolution: … }` form — **not** `CardKind::Act { … }` in `cards.rs`, and **not** `Agenda { … }`):

```bash
grep -rn "Act {" crates/ --include=*.rs | grep -vE "CardKind::Act|struct Act|RoundEndAdvance|AdvanceAct|-> |: GameState|&mut|impl"
```

Add `round_end_advance: None,` after the `resolution: …,` line in each. Known sites: `the_gathering.rs` (×3 — 01109 gets `Some` in Step 3, 01108/01110 get `None`), `scenarios/src/test_fixtures/synthetic.rs` (×2), `act_agenda.rs` tests (several), `evaluator.rs` tests (×3), `engine/mod.rs` test, `state/game_state.rs` test, `forced_triggers.rs` test, `the_gathering_symbols.rs` (×2), `act_advancement.rs`, `web/tests/board.rs`. After this step the workspace must compile:

Run: `cargo build --all`
Expected: clean (every `Act` literal has the new field).

- [ ] **Step 3: Set the objective for 01109 in `the_gathering.rs`**

In `crates/scenarios/src/the_gathering.rs`, import `RoundEndAdvance` (alongside the existing `Act` import) and set the field on the 01109 act-deck entry (leave 01108/01110 as `round_end_advance: None`):

```rust
        Act {
            code: CardCode("01109".into()),
            clue_threshold: act_clue_threshold("01109"),
            resolution: None,
            round_end_advance: Some(RoundEndAdvance {
                contributor_location: CardCode("01112".into()), // the Hallway
            }),
        },
```

- [ ] **Step 4: Write the failing `AdvanceAct` re-gating test**

In `crates/game-core/src/engine/dispatch/act_agenda.rs` tests, add:

```rust
    #[test]
    fn advance_act_rejected_for_round_end_advance_act() {
        use crate::state::{CardCode, RoundEndAdvance};
        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv])
            .with_phase(Phase::Investigation)
            .build();
        // An investigator holding plenty of clues — the reject must be about
        // the round-end objective, not affordability.
        state.investigators.get_mut(&inv).unwrap().clues = 9;
        state.act_deck = vec![Act {
            code: CardCode("01109".into()),
            clue_threshold: 3,
            resolution: None,
            round_end_advance: Some(RoundEndAdvance {
                contributor_location: CardCode("01112".into()),
            }),
        }];
        state.act_index = 0;
        let mut events = Vec::new();
        let out = advance_act_action(
            &mut Cx { state: &mut state, events: &mut events },
            inv,
        );
        assert!(matches!(out, EngineOutcome::Rejected { .. }));
        assert_eq!(state.act_index, 0, "act did not advance");
        assert_eq!(state.investigators[&inv].clues, 9, "no clues spent");
    }
```

Run: `cargo test -p game-core --lib advance_act_rejected_for_round_end_advance_act`
Expected: FAIL — the action currently advances (no re-gate yet).

- [ ] **Step 5: Add the re-gating to `advance_act_action`**

In `advance_act_action` (`act_agenda.rs`), after the `act_deck.is_empty()` check and before reading the threshold, add:

```rust
    if cx.state.act_deck[cx.state.act_index]
        .round_end_advance
        .is_some()
    {
        return EngineOutcome::Rejected {
            reason: "this act advances only at the end of the round (its round-end \
                     objective), not via the AdvanceAct action"
                .into(),
        };
    }
```

Run: `cargo test -p game-core --lib advance_act_rejected_for_round_end_advance_act`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core crates/scenarios
git commit -m "engine: Act.round_end_advance + AdvanceAct re-gating

Models act objectives that advance at round end (01109: Hallway
investigators spend clues). The threshold stays corpus-sourced; the
objective shape is content-set in the_gathering.rs. AdvanceAct now
rejects for round-end-advance acts.

Refs #275.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Suspendable round-end window (state + `upkeep_phase_end` + resume + routing)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`ActRoundEndPending` + `GameState.act_round_end_pending`)
- Modify: `crates/game-core/src/state/builder.rs` (init the field in `build()`)
- Modify: `crates/game-core/src/engine/dispatch/act_agenda.rs` (Hallway contributor/spend helpers)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (`upkeep_phase_end` → `EngineOutcome` + window + `resume_act_round_end_advance`; propagate in `upkeep_resume`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (action-gate guard + `resolve_input` routing)

- [ ] **Step 1: Add the pending state + builder init**

In `game_state.rs`, add (mirror `HandSizeDiscard`'s derives):

```rust
/// A parked act round-end clue-spend window (see [`RoundEndAdvance`]). The
/// decision context is snapshotted at park time; resolved via
/// `resume_act_round_end_advance`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActRoundEndPending {
    /// In-play location whose investigators may contribute clues.
    pub contributor_location: LocationId,
    /// Clues to spend to advance (the act's `clue_threshold`).
    pub threshold: u8,
}
```

Add the field to `GameState` (next to `hand_size_discard_pending`):

```rust
    /// A parked act round-end clue-spend window (#275). `Some` only while
    /// awaiting the group's Confirm/Skip at the end of the round.
    pub act_round_end_pending: Option<ActRoundEndPending>,
```

In `builder.rs`'s `build()` `GameState { … }` literal, add `act_round_end_pending: None,`.

Run: `cargo build -p game-core`
Expected: clean.

- [ ] **Step 2: Add Hallway contributor/spend helpers**

In `act_agenda.rs`, add (`pub(crate)` so `phases.rs` can call them):

```rust
/// Investigators currently at `location`, in `turn_order` (deterministic).
pub(crate) fn investigators_at(state: &GameState, location: LocationId) -> Vec<InvestigatorId> {
    state
        .turn_order
        .iter()
        .copied()
        .filter(|id| {
            state
                .investigators
                .get(id)
                .and_then(|i| i.current_location)
                == Some(location)
        })
        .collect()
}

/// Total clues held by `ids`.
pub(crate) fn clues_held(state: &GameState, ids: &[InvestigatorId]) -> u32 {
    ids.iter()
        .filter_map(|id| state.investigators.get(id))
        .map(|i| u32::from(i.clues))
        .sum()
}

/// Spend `amount` clues from `ids` in order. Caller must have validated the
/// group holds at least `amount` (`clues_held`). Mirrors `spend_clues`.
pub(crate) fn spend_clues_from(state: &mut GameState, ids: &[InvestigatorId], amount: u8) {
    let mut remaining = amount;
    for id in ids {
        if remaining == 0 {
            break;
        }
        if let Some(inv) = state.investigators.get_mut(id) {
            let take = inv.clues.min(remaining);
            inv.clues -= take;
            remaining -= take;
        }
    }
    debug_assert_eq!(remaining, 0, "spend_clues_from called without enough clues");
}
```

- [ ] **Step 3: Write the failing window tests (`phases.rs`)**

In `phases.rs` tests, add these (they call the private `upkeep_phase_end` / `resume_act_round_end_advance` directly — same module). A shared fixture builds act 2 with the objective + a Hallway location + an investigator at it:

```rust
    fn round_end_window_state(clues: u8) -> (crate::state::GameState, InvestigatorId) {
        use crate::state::{CardCode, Location, LocationId, RoundEndAdvance};
        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv])
            .with_phase(Phase::Upkeep)
            .with_location(Location::new(
                LocationId(2),
                CardCode("01112".into()),
                "Hallway",
                1,
                0,
            ))
            .build();
        let i = state.investigators.get_mut(&inv).unwrap();
        i.current_location = Some(LocationId(2));
        i.clues = clues;
        state.act_deck = vec![
            Act {
                code: CardCode("01109".into()),
                clue_threshold: 3,
                resolution: None,
                round_end_advance: Some(RoundEndAdvance {
                    contributor_location: CardCode("01112".into()),
                }),
            },
            Act {
                code: CardCode("01110".into()),
                clue_threshold: 0,
                resolution: Some(crate::scenario::Resolution::Won { id: "R1".into() }),
                round_end_advance: None,
            },
        ];
        state.act_index = 0;
        (state, inv)
    }

    #[test]
    fn upkeep_phase_end_opens_window_when_affordable() {
        let (mut state, _) = round_end_window_state(3);
        let mut events = Vec::new();
        let out = upkeep_phase_end(&mut Cx { state: &mut state, events: &mut events });
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        assert!(state.act_round_end_pending.is_some());
        assert_eq!(state.phase, Phase::Upkeep, "parked: did not transition");
        assert_no_event!(events, Event::PhaseStarted { phase: Phase::Mythos });
    }

    #[test]
    fn upkeep_phase_end_skips_window_when_unaffordable() {
        let (mut state, _) = round_end_window_state(2); // < threshold 3
        let mut events = Vec::new();
        let out = upkeep_phase_end(&mut Cx { state: &mut state, events: &mut events });
        assert_eq!(out, EngineOutcome::Done);
        assert!(state.act_round_end_pending.is_none());
        assert_eq!(state.phase, Phase::Mythos, "no window → straight to Mythos");
    }

    #[test]
    fn resume_confirm_spends_and_advances() {
        use crate::action::InputResponse;
        let (mut state, inv) = round_end_window_state(3);
        let mut events = Vec::new();
        let _ = upkeep_phase_end(&mut Cx { state: &mut state, events: &mut events });
        let out = resume_act_round_end_advance(
            &mut Cx { state: &mut state, events: &mut events },
            &InputResponse::Confirm,
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.act_index, 1, "advanced act 2 -> act 3");
        assert_eq!(state.investigators[&inv].clues, 0, "spent 3 clues");
        assert!(state.act_round_end_pending.is_none());
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn resume_skip_advances_nothing_and_continues() {
        use crate::action::InputResponse;
        let (mut state, inv) = round_end_window_state(3);
        let mut events = Vec::new();
        let _ = upkeep_phase_end(&mut Cx { state: &mut state, events: &mut events });
        let out = resume_act_round_end_advance(
            &mut Cx { state: &mut state, events: &mut events },
            &InputResponse::Skip,
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.act_index, 0, "no advance on Skip");
        assert_eq!(state.investigators[&inv].clues, 3, "no clues spent");
        assert!(state.act_round_end_pending.is_none());
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn resume_rejects_wrong_response_kind() {
        use crate::action::InputResponse;
        let (mut state, _) = round_end_window_state(3);
        let mut events = Vec::new();
        let _ = upkeep_phase_end(&mut Cx { state: &mut state, events: &mut events });
        let out = resume_act_round_end_advance(
            &mut Cx { state: &mut state, events: &mut events },
            &InputResponse::DiscardCards { indices: vec![] },
        );
        assert!(matches!(out, EngineOutcome::Rejected { .. }));
        assert!(state.act_round_end_pending.is_some(), "still pending");
        assert_eq!(state.phase, Phase::Upkeep);
    }

    #[test]
    fn affordability_counts_only_contributor_location() {
        use crate::state::{CardCode, Location, LocationId};
        let (mut state, _) = round_end_window_state(0); // Hallway inv holds 0
        // A second investigator elsewhere holds plenty — must NOT count.
        let other = InvestigatorId(2);
        let mut o = test_investigator(2);
        o.current_location = Some(LocationId(9));
        o.clues = 9;
        state.investigators.insert(other, o);
        state.turn_order.push(other);
        state.locations.insert(
            LocationId(9),
            Location::new(LocationId(9), CardCode("99999".into()), "Far", 1, 0),
        );
        let mut events = Vec::new();
        let out = upkeep_phase_end(&mut Cx { state: &mut state, events: &mut events });
        assert_eq!(out, EngineOutcome::Done, "unaffordable by Hallway alone");
        assert!(state.act_round_end_pending.is_none());
    }
```

Run: `cargo test -p game-core --lib upkeep_phase_end_opens_window_when_affordable`
Expected: FAIL — `upkeep_phase_end` returns `()` / no window logic / `resume_act_round_end_advance` undefined.

- [ ] **Step 4: Thread `upkeep_phase_end` → `EngineOutcome` + open the window**

In `phases.rs`, change `fn upkeep_phase_end(cx: &mut Cx)` to `-> EngineOutcome`. After the existing `RoundEnded` forced dispatch + its `debug_assert!`, replace the tail (`let outcome = step_phase(cx); debug_assert_eq!(...);`) with:

```rust
    // Act objective: a round-end "may spend clues to advance" window
    // (01109). Opens only when the current act carries it AND the
    // contributor-location investigators can afford the threshold — the
    // "may … spend the requisite number" is moot otherwise. Suspends; the
    // Upkeep→Mythos transition is deferred to resume_act_round_end_advance.
    if let Some(pending) = round_end_advance_window(cx.state) {
        let prompt = format!(
            "End of round: investigators at the contributor location may, as a group, \
             spend {} clues to advance the current act. Submit ResolveInput with \
             InputResponse::Confirm to spend and advance, or Skip to decline.",
            pending.threshold,
        );
        cx.state.act_round_end_pending = Some(pending);
        return EngineOutcome::AwaitingInput {
            request: InputRequest { prompt },
            resume_token: ResumeToken(0),
        };
    }
    step_phase(cx) // Upkeep → Mythos
}

/// The round-end advance window to open, if the current act offers one and
/// the contributor-location investigators can afford its `clue_threshold`.
fn round_end_advance_window(state: &GameState) -> Option<ActRoundEndPending> {
    let act = state.act_deck.get(state.act_index)?;
    let adv = act.round_end_advance.as_ref()?;
    let loc = crate::engine::location_id_by_code(state, adv.contributor_location.as_str())?;
    let contributors = super::act_agenda::investigators_at(state, loc);
    if super::act_agenda::clues_held(state, &contributors) < u32::from(act.clue_threshold) {
        return None;
    }
    Some(ActRoundEndPending {
        contributor_location: loc,
        threshold: act.clue_threshold,
    })
}

/// Resume a parked act round-end clue-spend window. Confirm spends the
/// threshold from the contributor-location investigators and advances the
/// act; Skip declines; either way the round closes (Upkeep→Mythos). A wrong
/// response kind rejects with state untouched.
pub(super) fn resume_act_round_end_advance(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let pending = cx
        .state
        .act_round_end_pending
        .clone()
        .unwrap_or_else(|| unreachable!("resume_act_round_end_advance: no pending window"));
    match response {
        InputResponse::Confirm => {
            let contributors = super::act_agenda::investigators_at(cx.state, pending.contributor_location);
            if super::act_agenda::clues_held(cx.state, &contributors) < u32::from(pending.threshold) {
                return EngineOutcome::Rejected {
                    reason: "act round-end advance: contributors no longer hold enough clues".into(),
                };
            }
            super::act_agenda::spend_clues_from(cx.state, &contributors, pending.threshold);
            cx.state.act_round_end_pending = None;
            // act 2 (01109) is non-terminal (resolution None) — advance the cursor.
            super::act_agenda::advance_act(cx);
            step_phase(cx)
        }
        InputResponse::Skip => {
            cx.state.act_round_end_pending = None;
            step_phase(cx)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: act round-end advance expects Confirm or Skip, got {other:?}"
            )
            .into(),
        },
    }
}
```

Add any missing imports to `phases.rs` (`ActRoundEndPending`, `InputRequest`, `ResumeToken`, `InputResponse`, `GameState` — most are already in scope; add what the compiler flags).

- [ ] **Step 5: Propagate the outcome from both callers**

In `upkeep_resume`, change the tail `upkeep_phase_end(cx); EngineOutcome::Done` to `upkeep_phase_end(cx)`.

In `resume_hand_size_discard`, the queue-drained branch currently runs `upkeep_phase_end(cx); EngineOutcome::Done` — change it to `upkeep_phase_end(cx)`.

- [ ] **Step 6: Run the window tests**

Run: `cargo test -p game-core --lib round_end`
Expected: the six new tests PASS.

Run: `cargo test -p game-core --lib upkeep`
Expected: existing upkeep tests still PASS (note: `upkeep_resume_parks_at_hand_size_discard` and friends still hold — `upkeep_phase_end`'s new return is `Done` on the no-window path).

> **Round-end ordering** (agenda doom *then* act window) is **structural**: `upkeep_phase_end` runs the `PhaseEnded(Upkeep)` and `RoundEnded` forced dispatches *before* `round_end_advance_window`, in source order. It's covered by the union of C3c's `round_ended.rs` (the `RoundEnded` forced fires) and these window tests (the window opens after the forced dispatch returns) — no separate combined test, which would need a mock-registry agenda + fragile phase-cycle driving for marginal value (mirrors the #276 "no-registry" rationale).

- [ ] **Step 7: Add the action-gate guard + `resolve_input` routing**

In `crates/game-core/src/engine/dispatch/mod.rs`, after the `hand_size_discard_pending` action-gate guard, add:

```rust
    // A pending act round-end advance (#275) blocks every action but
    // `ResolveInput`. Upkeep-phase only; never coexists with the others.
    if cx.state.act_round_end_pending.is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "an act round-end advance choice is pending; submit a PlayerAction::ResolveInput \
                     with InputResponse::Confirm or Skip before any other action"
                .into(),
        };
    }
```

In `resolve_input`, add `cx.state.act_round_end_pending.is_some()` to the mutual-exclusion `debug_assert!` array, and add the routing branch after the hand-size one:

```rust
    if cx.state.act_round_end_pending.is_some() {
        return phases::resume_act_round_end_advance(cx, response);
    }
```

- [ ] **Step 8: Build + commit**

Run: `cargo build --all && cargo test -p game-core --lib round_end`
Expected: clean + PASS.

```bash
git add crates/game-core
git commit -m "engine: suspendable act round-end clue-spend window

upkeep_phase_end threads to EngineOutcome and opens a Confirm/Skip window
when the current act has a round-end objective affordable by its
contributor-location investigators. Resume spends + advances (Confirm) or
continues (Skip); routed via act_round_end_pending like hand-size discard.

Refs #275.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Guard + routing through public `apply` (integration test)

The Task-2 unit tests cover the window logic directly (`upkeep_phase_end`/`resume_act_round_end_advance`). The integration-specific gap is the **action-gate guard** and **`resolve_input` routing** through the public `apply` entry point. This test presets `act_round_end_pending` (so it doesn't depend on fragile phase-cycle driving) and exercises those two paths. No registry/scenario needed — the window is pure kernel logic.

**Files:**
- Create: `crates/game-core/tests/act_round_end.rs`

- [ ] **Step 1: Write the integration test**

```rust
//! Act round-end window through the public `apply` entry: the action-gate
//! guard blocks non-`ResolveInput` actions while a window is pending, and
//! `ResolveInput` routes to the resume (Confirm spends + advances).

use game_core::action::{InputResponse, PlayerAction};
use game_core::state::{
    Act, ActRoundEndPending, CardCode, GameState, InvestigatorId, Location, LocationId, Phase,
    RoundEndAdvance,
};
use game_core::test_support::{test_investigator, GameStateBuilder};
use game_core::{apply, Action, EngineOutcome};

/// Act 2 current, a Hallway investigator with `clues`, and the round-end
/// window already parked (so we test the guard + routing, not the phase
/// cycle that opens it — that's covered by the `phases.rs` unit tests).
fn parked_window_state(clues: u8) -> GameState {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([inv])
        .with_phase(Phase::Upkeep)
        .with_location(Location::new(
            LocationId(2),
            CardCode("01112".into()),
            "Hallway",
            1,
            0,
        ))
        .build();
    let i = state.investigators.get_mut(&inv).unwrap();
    i.current_location = Some(LocationId(2));
    i.clues = clues;
    state.act_deck = vec![
        Act {
            code: CardCode("01109".into()),
            clue_threshold: 3,
            resolution: None,
            round_end_advance: Some(RoundEndAdvance {
                contributor_location: CardCode("01112".into()),
            }),
        },
        Act {
            code: CardCode("01110".into()),
            clue_threshold: 0,
            resolution: Some(game_core::scenario::Resolution::Won { id: "R1".into() }),
            round_end_advance: None,
        },
    ];
    state.act_index = 0;
    state.act_round_end_pending = Some(ActRoundEndPending {
        contributor_location: LocationId(2),
        threshold: 3,
    });
    state
}

#[test]
fn pending_window_blocks_non_resolve_actions() {
    let state = parked_window_state(3);
    let r = apply(state, Action::Player(PlayerAction::EndTurn));
    assert!(
        matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "the guard blocks non-ResolveInput actions while a window is pending"
    );
    assert!(r.state.act_round_end_pending.is_some(), "still pending");
}

#[test]
fn resolve_confirm_routes_to_resume_and_advances() {
    let state = parked_window_state(3);
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_eq!(r.state.act_index, 1, "advanced act 2 -> act 3");
    assert_eq!(r.state.investigators[&InvestigatorId(1)].clues, 0, "spent 3");
    assert!(r.state.act_round_end_pending.is_none());
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p game-core --test act_round_end`
Expected: PASS (both). If a `use` is unused under `-D warnings`, trim it.

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/tests/act_round_end.rs
git commit -m "test: act round-end window guard + routing via apply

Refs #275.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Gauntlet + push + PR + phase doc + demo

**Files:** none (verification + delivery) except the phase doc.

- [ ] **Step 1: Full strict gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green. Fix warnings/lints before proceeding. (Watch for: `web` and other crates that pattern-match `PlayerAction` exhaustively are unaffected — no new action; the `Act` literals in `web/tests` were updated in Task 1.)

- [ ] **Step 2: Push**

```bash
git push -u origin engine/act-01109-round-end
```

- [ ] **Step 3: Open the PR**

```bash
gh pr create --base main \
  --title "engine: C3d — act 01109 round-end clue-spend window"
```
Body: the suspendable round-end window, the `Act.round_end_advance` model (threshold corpus-sourced, objective content-set), `AdvanceAct` re-gating, round-end ordering. Add `Closes #275.`

- [ ] **Step 4: Watch CI**

Run: `gh pr checks <PR#> --watch` (background). Fix failures with follow-up commits.

- [ ] **Step 5: Demo the window firing**

After CI is green, run a focused trace to show the behavior (the user asked to see it in action). Either: (a) `cargo test -p game-core --lib resume_confirm_spends_and_advances -- --nocapture` with a temporary `dbg!` of `state.act_index`/clues across the window, or (b) the `/verify` flow / `crates/cards/tests/act_01109_round_end.rs` run, narrating the AwaitingInput → Confirm → act-advanced transition. Present the observed transition to the user.

- [ ] **Step 6: Update the phase doc (only once CI is green)**

In `docs/phases/phase-7-the-gathering.md`, flip the C3d row to `✅ PR #N`. Add a **Decisions made** entry only if load-bearing for a future PR (e.g. `upkeep_phase_end` is now suspendable and returns `EngineOutcome` — relevant to any future round-end window; the `Act.round_end_advance` content-set-vs-corpus rationale). Do not merge — stop for explicit user approval.
