# C3d — Act-2 (01109) round-end clue-spend window (design)

**Issue:** [#275](https://github.com/talelburg/eldritch/issues/275) (Phase 7, Slice 1, Group C).
**Depends on:** C3c ([#232](https://github.com/talelburg/eldritch/issues/232), PR #278) — the `RoundEnded` framework timing point fires in `upkeep_phase_end`.

## Verified card text (snapshot)

**01109 The Barrier** (act 2, `clues: 3`):

> **Objective** - When the round ends, investigators in the hallway may, as a group, spend the requisite number of clues to advance.

## Problem

C1b left act 2 on the interim action-driven `AdvanceAct` (spend `clue_threshold` clues during the Investigation phase, like act 1). The faithful objective is a **round-end** decision restricted to **Hallway** investigators. This needs the engine to *pause at round end* for an optional group choice — a suspendable player window — which is why it's split from C3c (the agenda's round-end doom is fire-and-forget and needs no window).

## Why a kernel `Act` field, not a card native effect

The objective is **generic framework mechanics** ("at round end, investigators at location L may spend the act's `clue_threshold` to advance"), not bespoke card logic: the advance is already `advance_act`, the spend is the existing `spend_clues` pattern, and the window lifecycle + the Upkeep→Mythos continuation are inherently kernel concerns. Unlike agenda 01107 (genuinely card-specific Ghoul pathfinding → `Effect::Native`), there is no card-specific Rust to localize — only two *parameters*: the contributor location and the threshold.

Those parameters live on `Act`, next to the `clue_threshold` and `resolution` it already carries. **The threshold comes from the corpus** (`CardKind::Act { clue_threshold }`, read from ArkhamDB's structured `clues: 3` field). **The objective shape (round-end timing + Hallway contributor) is hand-set by content** in `the_gathering.rs` — consistent with how the sibling acts' objectives are hand-authored (01108's board-build and 01110's `EnemyDefeated` are ability impls, not corpus-parsed). ArkhamDB has no structured field for "round-end advance"; it's free text, and a single consumer — so no pipeline parsing (revisit if a second round-end-advance act lands).

A card-local suspending native effect was considered and rejected for this slice: it would still leave the framework continuation (`step_phase`) in the kernel, and making the *forced-trigger path* propagate/resume `AwaitingInput` is the suspendable-dispatch north-star (#212/#213), not slice-1 scope.

## Design

### 1. Model the objective on the kernel `Act`

```rust
/// A round-end "may spend clues to advance" objective (e.g. 01109).
pub struct RoundEndAdvance {
    /// Only investigators at this in-play location may contribute clues
    /// (01109: the Hallway 01112).
    pub contributor_location: CardCode,
}
// on Act:
pub round_end_advance: Option<RoundEndAdvance>,
```

`the_gathering.rs` sets `Some(RoundEndAdvance { contributor_location: CardCode::new("01112") })` on the 01109 act-deck entry; 01108/01110 stay `None`. Cost = the act's existing `clue_threshold` (3).

### 2. Thread `upkeep_phase_end` → `EngineOutcome`

Today it fires the two forced dispatches then calls `step_phase` (Upkeep→Mythos) and returns `()`. New shape:

1. Fire `PhaseEnded { Upkeep }` forced, then `RoundEnded` forced (the agenda doom) — unchanged.
2. If the current act has `round_end_advance: Some(adv)` **and** the investigators at `adv.contributor_location` collectively hold ≥ `clue_threshold` clues: park `act_round_end_pending` and return `AwaitingInput` (Confirm/Skip). `step_phase` is deferred to the resume.
3. Otherwise: `step_phase` + `Done`, as today.

Both callers propagate instead of discarding: `upkeep_resume` returns `upkeep_phase_end(cx)` (not `; Done`); `resume_hand_size_discard`'s queue-drained branch likewise returns it.

### 3. Suspension state + resume (mirrors hand-size discard)

```rust
// on GameState:
pub act_round_end_pending: Option<ActRoundEndPending>,
// snapshot of the resolved decision context at park time:
pub struct ActRoundEndPending {
    pub contributor_location: LocationId,
    pub threshold: u8,
}
```

- **Action-gate guard** (`dispatch/mod.rs`): while `act_round_end_pending.is_some()`, only `ResolveInput` is valid (mirror of the `hand_size_discard_pending` guard).
- **`resolve_input` routing**: a branch routing to `resume_act_round_end_advance` when `act_round_end_pending.is_some()` (it arises only in Upkeep, never mid-skill-test, so it's mutually exclusive with the others — add to the existing `debug_assert!` set).
- **`resume_act_round_end_advance(cx, response)`** (validate-first):
  - `InputResponse::Confirm` → re-validate the contributor-location investigators still hold ≥ `threshold` (they do — nothing mutates between park and resume); spend `threshold` clues from them (deterministic order, the existing `spend_clues` discipline restricted to that location); `advance_act`. On a wrong response kind → `Rejected`, state untouched.
  - `InputResponse::Skip` → no advance.
  - Either way: clear `act_round_end_pending`, then `step_phase` (Upkeep→Mythos), return `Done`.

### 4. `AdvanceAct` re-gating

`advance_act_action` rejects when the current act has `round_end_advance: Some(..)`: "act 01109 advances only at round end." (Act 1 still advances via `AdvanceAct`; act 3 via its forced `EnemyDefeated`.)

### 5. Round-end ordering (deterministic, documented)

`PhaseEnded(Upkeep)` forced → `RoundEnded` forced (agenda doom) → act-2 window → `step_phase`. The doom placement and the act advance are independent, so order is immaterial to outcome.

## Components / boundaries

- `state` (`Act`, `GameState`, new `RoundEndAdvance`/`ActRoundEndPending`) — data only.
- `the_gathering.rs` — sets `round_end_advance` for 01109 (the only content change).
- `phases.rs` — `upkeep_phase_end` window-open + threading; `resume_act_round_end_advance`.
- `act_agenda.rs` — `AdvanceAct` re-gating; a Hallway-restricted contributor/spend helper (a thin variant of the existing `clue_contributors`/`spend_clues`).
- `dispatch/mod.rs` — action-gate guard + `resolve_input` routing.

## Testing

Engine unit tests (`phases.rs`):
- Window opens at round end when the current act has `round_end_advance` and Hallway investigators hold ≥ threshold; parks `act_round_end_pending`, phase stays Upkeep, no `PhaseEnded(Mythos)` yet.
- Does **not** open when unaffordable (Hallway clues < threshold) or when the act has no `round_end_advance` → goes straight to Mythos.
- Affordability counts **only** contributor-location investigators (clues elsewhere don't count).
- `resume` Confirm: spends exactly `threshold` from Hallway investigators, advances act-2 → act-3, clears pending, lands in Mythos.
- `resume` Skip: no spend, no advance, clears pending, lands in Mythos.
- `resume` wrong response kind → rejected, state untouched, still pending.
- Round-end ordering: agenda doom is placed **and** the window opens in the same round-end (when both an agenda 01107 and act 01109 are current).

`AdvanceAct` re-gating (`act_agenda.rs`): rejected when the current act has `round_end_advance: Some(..)`.

Integration (`crates/cards/tests/`): on a built board with act 2 current and a Hallway investigator holding 3 clues, a round-end cycle opens the window; Confirm advances to act 3.

## Out of scope

- Multi-investigator **clue allocation** choice (who contributes the surplus) — stays the existing deterministic spend (TODO #153).
- Suspendable *forced/native* dispatch (#212/#213) — this is a bespoke kernel window, not the general mechanism.
