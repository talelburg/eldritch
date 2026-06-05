# Transactional `Rejected ⟹ state unchanged` (#161)

**Date:** 2026-06-06
**Issue:** #161 — Refactor `apply()` to a structural validate-first / apply-second two-phase shape
**Status:** Design approved; ready for implementation plan

## Problem

The engine's handler contract — "on `EngineOutcome::Rejected`, the returned
state and event list are unchanged from the input" — is enforced **by
convention, not structurally** (CLAUDE.md, "Handler contract"). Today it rests on:

- A belt-and-suspenders `events.clear()` on `Rejected` in
  `apply_with_scenario_registry` (`engine/mod.rs:105-109`).
- A standing `TODO(#17+)` at `engine/mod.rs:58-60` and the doc-comment at
  `mod.rs:54-57` admitting the guarantee holds for events but **not** state.

The convention already holds for the common rejection paths:

- The guard ladder in `apply_player_action` (mulligan / reaction-window /
  skill-test / hunter / spawn-engage pending) rejects **before any mutation**.
- The `check_*` validators already implement the "validated plan" shape:
  `check_play_card → PlayCheckResult`, `check_activate_ability →
  ActivateCheckResult`. They do a read-only pass over `&GameState` and return a
  plan struct that the mutation step consumes.

So the issue's option 1 (validated plan) and option 3 (typestate split) are
**largely already done** for the validators. Extending them everywhere would be
busywork that does **not** close the actual gap.

### The actual gap: the DSL evaluator path

The gap is narrow and lives in one place — the fallible-and-mutating DSL
evaluator. Two documented instances:

- `play_card` (`dispatch/cards.rs:494-504`): after the clean `check_play_card`
  passes, the *mutate* phase pushes `Event::CardPlayed`, then runs each `OnPlay`
  effect through `apply_effect`; `if !matches!(outcome, Done) { return outcome; }`.
  An effect that rejects mid-loop leaves `CardPlayed` on the buffer, any earlier
  effect's mutations committed, and the card still in hand. (Caveat documented at
  `cards.rs:457-466`.)
- `apply_seq` (`engine/evaluator.rs:393-408`): "Stop at the first non-Done
  outcome. A Rejected mid-Seq leaves earlier effects committed — not great as a
  rollback story." A `Seq[GainResources(2), Modify{ThisTurn}]` mutates
  (resources +2) then rejects (the `Modify{ThisTurn}` TODO stub at
  `evaluator.rs:258`).

The evaluator **interleaves validation and mutation** and is itself fallible —
you cannot pre-validate a DSL effect tree without running it (`If` / `ForEach` /
`ChooseOne` mutation shape depends on runtime state). Therefore options 1 and 3
cannot fix the evaluator case; they only relocate where the fallible-mutating
evaluator gets called. The same shape will recur in `activate_ability` and every
future triggered-effect path.

### Invariant precision

The contract is **`Rejected ⟹ state unchanged`**, *not* "non-`Done` ⟹
unchanged." `AwaitingInput` legitimately returns partial state — a
`PerformSkillTest` that suspends at the commit window has already emitted
`SkillTestStarted` and populated `in_flight_skill_test` (`mod.rs:62-69`).
Whatever we do must restore on `Rejected` **only**.

## Decision

**Transactional snapshot/rollback at the `apply` boundary.** `GameState` derives
`Clone` (`state/game_state.rs:33`). In `apply_with_scenario_registry`
(`mod.rs:88-127`), snapshot before dispatch and restore on `Rejected`:

```rust
let mut state = state;
let mut events = Vec::new();
let pristine = state.clone();                 // NEW: transactional snapshot
let resolution_already_fired = state.resolution.is_some();
let outcome = { /* build Cx, dispatch */ };
if matches!(outcome, EngineOutcome::Rejected { .. }) {
    state = pristine;                          // NEW: structural restore
    events.clear();                            // kept; now part of the transaction
}
// resolution hook unchanged (fires only on non-Rejected)
```

This is the entire mechanism. Because the snapshot is taken before *any* handler
runs and restored whenever the outcome is `Rejected`, no handler — including
evaluator-driven ones — can leak partial state on rejection. `AwaitingInput` is
untouched (we restore only on `Rejected`), preserving its legitimately-partial
state.

### Why this shape over the alternatives

- **vs. "snapshot only fallible regions":** re-introduces per-handler discipline
  — the thing #161 wants to eliminate. Every evaluator-driven handler would have
  to remember to wrap itself.
- **vs. "rewrite evaluator as two-phase plan":** large and speculative; the DSL's
  conditional nodes mean the plan pass must itself evaluate conditions. Not
  justified by the current gap.

The cost is one `GameState` clone per `apply` call. In a turn-based engine this
is negligible; `apply` is not a hot loop. We accept it deliberately rather than
add dirty-tracking machinery on speculation (see Out of Scope).

## Transaction boundary: the `apply` call, not the logical action

The snapshot is taken at the start of **whichever `apply` call is running**, so
the rollback target is that call's input state — not the start of a multi-call
logical action that paused at `AwaitingInput`.

```
apply(s0, action_A)        → AwaitingInput,  state = s1   (s1 real, retained, persisted)
apply(s1, ResolveInput{r}) → Rejected,       state = s1   (snapshot was s1, restored to s1)
```

A reject during `ResolveInput` processing rewinds to `s1` — the pause point —
**not** to `s0`. This is correct: `s1` was the product of an apply that returned
`AwaitingInput`, whose contract states that partial state is legitimate and
committed. Only the rejected call's own mutations vanish.

Two flavors of reject during `ResolveInput`, both handled identically:

1. **Malformed response** (e.g. `PickIndex` where `CommitCards` is expected,
   out-of-bounds index) — already rejects in `resolve_input`'s validation prefix
   before mutating; state is `s1` with or without this change. The snapshot is a
   belt for these.
2. **Valid response, downstream effect rejects mid-resolution** (e.g. commit
   cards → test resolves → an after-success trigger's effect rejects) — *this* is
   what the snapshot newly protects: without it the partial resolution leaks into
   `s1`; with it we rewind cleanly to `s1`.

Why restoring to `s1` (not `s0`) is right:

- All suspension latches (`in_flight_skill_test`, `open_windows`,
  `hunter_move_pending`, `spawn_engage_pending`, `mulligan_pending`,
  `mythos_draw_pending`) are plain `GameState` fields, captured by the clone, so
  the restore puts the engine back at exactly the same outstanding prompt.
- The host still holds the original `request` + `resume_token` from the first
  call; since `s1` is byte-identical, that token is still valid — the host shows
  the reject reason and re-prompts. Retry composes for free.
- The server's action log stores *actions*, not states; a rejected `ResolveInput`
  is not appended, so replay deterministically re-reaches `s1` and waits for a
  valid response.

**Honest caveat:** if the reject is *deterministic* for a given valid response (a
genuine engine bug or an illegal game situation), the player can re-submit the
same choice and get the same reject — stuck at the prompt. This is the safe
failure: a no-corruption stall that surfaces the bug, far better than committing
half a skill-test resolution.

## What this lets us delete / correct

| Location | Action |
|---|---|
| `cards.rs:457-466` (`play_card` caveat) | **Delete.** The mid-resolution partial-state hole is closed structurally. |
| `evaluator.rs:394-399` (`apply_seq` caveat) | **Downgrade.** Mid-Seq reject still leaves *intra-apply* partial mutation, but it's rolled back at the boundary; reword to point at `mod.rs`. |
| `mod.rs:58-60` (`TODO(#17+)`) | **Remove.** Resolved by this change. |
| `mod.rs:54-57` (doc: "not for state") | **Rewrite** to state the guarantee is now structural. |
| `mod.rs:106-109` (`events.clear()` comment) | **Reword** from "belt-and-suspenders for a convention" to "part of the transactional restore." The call stays; `events` starts empty each apply, so clearing == restoring it. |

No code in `cards.rs` / `evaluator.rs` changes — only the doc-comments. The
mechanism is confined to `mod.rs`.

## Testing

1. **New integration test** `crates/cards/tests/reject_rollback.rs` (own process,
   per CLAUDE.md test layering). Install a **hand-rolled** `CardRegistry` (test
   fn pointers, not `cards::REGISTRY`) whose probe card has metadata
   `CardType::Asset`/`Event` and an `OnPlay` ability `=
   Seq[GainResources(2), <a rejecting effect>]`. The rejecting sibling is an
   effect that rejects regardless of context — candidate: `Modify{ThisTurn}`
   (the TODO stub at `evaluator.rs:258`); the exact effect is pinned during TDD.
   Play the card; assert:
   - outcome is `Rejected`,
   - the acting investigator's `resources`, `hand`, and `cards_in_play` equal
     their pre-play values,
   - the returned `events` are empty.

   Without the restore this fails (resources +2, `CardPlayed` emitted, card
   consumed); with it, it passes. **This is the test that proves the new
   guarantee.**

2. **Engine regression tests** (`engine/mod.rs` `#[cfg(test)]`): two or three
   "guard-ladder reject returns byte-identical state" cases (assert
   `apply(s, illegal_action).state == s`), locking the invariant for the
   pre-mutation paths too.

3. **AwaitingInput-boundary test** (`engine/mod.rs` or the integration test):
   drive `apply` to an `AwaitingInput` state `s1`, submit a malformed
   `ResolveInput`, assert outcome `Rejected` and `state == s1` (suspension latch
   still set). Exercises the transaction-boundary semantics above.

4. **Full existing suite** is the no-regression oracle — no external behavior
   change for already-correct handlers.

## Out of scope (YAGNI)

- **Clone-on-write / dirty-tracking optimization.** One `GameState` clone per
  `apply` is negligible for a turn-based engine; no machinery on speculation. If
  profiling ever flags it, revisit then.
- **Rewriting the evaluator into a two-phase plan-builder.**
- **Removing the `check_*` validated-plan validators.** They are good as-is; this
  change is complementary, not a replacement.

## Acceptance

- [ ] `apply_with_scenario_registry` snapshots state and restores it on
      `Rejected`; `events.clear()` retained as part of the transaction.
- [ ] `reject_rollback.rs` integration test proves an evaluator-driven
      mid-resolution reject leaves state and events untouched.
- [ ] AwaitingInput-boundary reject rewinds to the pause state, not the
      pre-action state.
- [ ] `play_card` caveat, `apply_seq` caveat, `TODO(#17+)`, and the
      contract doc-comments are updated to reflect the structural guarantee.
- [ ] Full CI gauntlet (fmt, clippy, test, doc, wasm-build) green with strict
      flags.
