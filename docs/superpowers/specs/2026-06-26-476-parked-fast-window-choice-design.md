# #476 — Surface parked Fast windows as a skippable choice

**Issue:** [#476](https://github.com/talelburg/eldritch/issues/476) — Stranded
state after a Mythos skill-test treachery (Rotting Remains): board shows
Investigation round 2 with no input controls and no rejection. Labels: `bug`,
`engine`, `p1-next` (playability blocker).

## Root cause (confirmed by reproduction)

A framework **Fast player window** (`Continuation::FastWindow`, e.g.
`Phase(InvestigatorTurnBegins)`) opens at the start of the Investigation turn.
When the player holds a **fast-play-eligible card**, `open_fast_window`
(`reaction_windows.rs`) finds an eligible play and **parks** the window —
returning `EngineOutcome::Done` with the `FastWindow` still on the continuation
stack, *not* `AwaitingInput`. This is the engine's intentional "Done-idle + open
window = awaiting a fast-play-or-pass" protocol (the test resolver honors it; the
`apply` drive loop idles such a window).

The **web client renders only when the outcome is `AwaitingInput`** (`input.rs`);
nothing inspects `state.open_windows()`. So a parked Fast window yields **no
controls and no rejection** — the exact symptom. It is **independent of #478**
(the acknowledge step) and of the skill-test/symbol-token effects (those resolve
correctly).

Reproduced in a pure-engine test (`crates/scenarios/tests/issue_476_repro.rs`):
real registries, Roland (01001), Rotting Remains (01163) on the encounter deck, a
forced Cultist bag, **and Magnifying Glass (01030, a Fast asset) in hand**. After
the Mythos draw + failed willpower test resolve, the state is
`phase=Investigation, round=2, outcome=Done, continuations=[InvestigationPhase,
FastWindow{InvestigatorTurnBegins}]`, `open_windows=1`. With an *empty* hand the
window auto-skips and the open turn appears normally — which is why earlier
synthetic repros (empty deck) never stranded.

Beyond the strand, this exposes a broader gap: the web client currently has **no
way to play fast cards in any framework window** — when such a window auto-skips
the player never notices; when it parks the player strands.

## Goal

When a framework Fast window has eligible fast plays, surface them to the player
as a **skippable choice** (play a fast card / activate a 0-cost ability, or pass)
instead of parking silently as `Done`. This fixes the strand and delivers
fast-play-in-windows using the client's existing `PickSingle` + `Skip` rendering.

## Decision (from brainstorming)

Fix **engine-side**: a parked Fast window emits
`AwaitingInput { PickSingle(<fast candidates>), skippable }` rather than `Done`.
The candidate set includes **both** fast card plays and 0-cost activated
abilities. After a fast play, the window **re-opens** (the player may play more)
until they Skip. The web client needs **no change** — its existing skippable
`PickSingle` rendering handles it.

## Design

### 1. Enumerate fast candidates

Refactor the existing `any_fast_play_eligible(state) -> bool`
(`dispatch/reaction_windows.rs`) into:

```
fn enumerate_fast_plays(state: &GameState) -> Vec<TurnAction>
```

It walks exactly what `any_fast_play_eligible` walks today, collecting the
eligible plays as `TurnAction`s instead of short-circuiting to `true`:

- **Fast cards in hand** — for each `inv` and `hand_index`,
  `check_play_card(state, inv, hand_index)` is `Ok` *and* `result.is_fast` →
  `TurnAction::PlayCard { investigator, hand_index }`.
- **0-action activated abilities** — for each card in play with a
  `Trigger::Activated { action_cost: 0 }` ability where
  `check_activate_ability(state, inv, instance_id, ability_index)` is `Ok` →
  `TurnAction::ActivateAbility { investigator, instance_id, ability_index }`.

`permits_fast` gating is **already applied** by `check_play_card` (it reads
`state.top_window().permits_fast(investigator)`), so this is enumerated *while
the FastWindow is on the stack* and respects each window's actor scope
(turn-begin permits the active investigator; mythos-after-draws permits anyone).

`any_fast_play_eligible(state)` becomes `!enumerate_fast_plays(state).is_empty()`
(retain the name where call sites want the bool, or inline).

### 2. Emit a skippable choice instead of `Done`

In `open_fast_window`, the parked branch (today: `EngineOutcome::Done` with the
window left on the stack) becomes: build a `PickSingle` from
`enumerate_fast_plays` (each `OptionId(i)` indexes the list; label via
`TurnAction::label(state)`), mark it `.skippable()`, and return
`AwaitingInput { request, resume_token: ResumeToken(0) }`. Mirrors the open-turn
`turn_menu` builder, but skippable and filtered to fast plays.

The auto-skip branch (no eligible plays) is **unchanged**: pop the window and run
the continuation inline (still resolves to `Done`/next prompt). So windows with
nothing to do behave exactly as before — no churn there.

A `FastWindow` re-exposed on top by the `apply` drive loop must likewise emit
this prompt rather than idling. The cleanest single source of truth is a helper
`emit_fast_window(cx) -> EngineOutcome` that both `open_fast_window` (parked
branch) and the resume path call; the drive-loop `FastWindow` idle arm routes a
candidate-bearing window through it.

### 3. Resume routing

The top-frame `FastWindow` is already routed to `resume_window`
(`dispatch/mod.rs`) on `ResolveInput`. Extend it:

- `Skip` → existing behavior: `close_reaction_window` → run the window's
  continuation → proceed (e.g. to the open-turn menu).
- `PickSingle(OptionId(i))` → re-enumerate `enumerate_fast_plays`, dispatch the
  i-th `TurnAction` via the existing `dispatch_turn_action`, then **re-open**:
  re-enumerate; if any plays remain, `emit_fast_window` again (the player may
  play another); else close the window and run its continuation.
  - Bounds-check `i` against the re-enumerated list; out-of-range → `Rejected`
    (mirrors the open-turn `OptionId` bounds arm).
- Any other response → `Rejected` with a clear reason.

Re-enumeration at resume (no stored option list) matches the open-turn pattern
and stays correct if the playable set changes after a play.

### 4. Client

No change. The engine now emits `AwaitingInput { PickSingle, skippable }`, which
`AwaitingInputView`'s existing PickSingle arm + Skip control render directly:
option buttons for each fast play, a Skip button to pass.

### 5. Blast radius

Every framework Fast window (`InvestigationBegins`, `InvestigatorTurnBegins`,
`MythosAfterDraws`, `UpkeepBegins`) now prompts when a fast play is available,
instead of idling to `Done`. Affected:

- **`drive_to_terminal_no_commits`** (`test_support/resolver.rs`) currently
  treats "`Done` + open window" as "send `Skip`". It must instead answer a
  **skippable** `AwaitingInput` (the fast-window prompt) with `Skip`. Rule: in
  the no-commits driver, a `request.skippable` prompt → `Skip`.
- A handful of integration tests that drive a sequence where a fast window parks
  will shift from asserting `Done` to driving one more `Skip` (or asserting the
  new prompt). Most test hands hold no fast card, so their windows still
  auto-skip (`Done`) — the churn is expected to be small. Quantified during
  planning by running the full suite.

### 6. Testing

- **Regression** (`crates/scenarios/tests/issue_476_repro.rs`, promoted from the
  debugging scratch file): real registries + Roland + Rotting Remains + Cultist
  bag + Magnifying Glass in hand. After commit-nothing, the outcome is
  `AwaitingInput { PickSingle, skippable }` (not `Done`+open-window); the option
  list includes the Magnifying Glass play; `Skip` reaches the open turn; picking
  the option plays the asset (and the window re-opens / then Skip proceeds).
- **Engine units** (`reaction_windows.rs` `#[cfg(test)]`): `enumerate_fast_plays`
  returns the expected `TurnAction`s for a fast card + a 0-cost ability and
  `[]` when none; a parked window emits the skippable PickSingle; the empty case
  still returns `Done` via inline auto-skip; the re-open loop (play one →
  prompt again → Skip → close); out-of-range OptionId rejects.
- **Harness**: `drive_to_terminal_no_commits` Skips the fast-window prompt
  (covered by an existing or new no-commits drive that parks a window).

## Out of scope

- Reaction (timing-point) windows already emit `AwaitingInput{skippable}` — only
  framework `FastWindow`s change here.
- Multiplayer fast-window arbitration (who acts first across investigators) —
  solo scope: `enumerate_fast_plays` iterates all investigators but solo has one,
  and `permits_fast` already gates per window.
- Any change to which windows open or to fast-play *rules* — this only changes
  how an already-open, already-parking window is *surfaced*.
