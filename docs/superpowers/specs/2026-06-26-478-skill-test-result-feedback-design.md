# #478 — Surface skill-test results (Confirm-to-dismiss)

**Issue:** [#478](https://github.com/talelburg/eldritch/issues/478) — QoL: surface skill-test
results (drawn chaos token + total vs difficulty + pass/fail). Labels: `ui`,
`p2-later`. A concrete instance of #466 (surface auto-resolved framework
effects).

## Problem

A skill test resolves with no visible feedback: the player commits cards (or
nothing) and the board's counters just change. There is no way to see **which
chaos token was drawn**, the **final total vs difficulty**, or **pass/fail by
N**. This makes tests confusing and makes bugs like #476 hard to even notice.

## Goal

After a skill test resolves, pause and show the player a result panel — chaos
token drawn, final total vs difficulty, succeeded/failed by N — and wait for a
**Confirm** to dismiss it, so the player registers the result **before the
consequence resolves** (clue discovery, damage, on-fail effects).

## Scope decisions (from brainstorming)

- **Events-only math.** Show *token drawn → final total vs difficulty →
  pass/fail by N*. The full additive breakdown (base skill + committed icons +
  modifiers + token) is **out of scope** — no event carries that split, and
  adding it is a separate engine concern. The issue explicitly frames the
  events-only render as the "first cut".
- **Confirm-to-dismiss, engine-driven.** The engine pauses at skill-test
  resolution with `AwaitingInput { InputKind::Confirm }`; the consequence
  resolves only after the player acknowledges. (Per the issue's optional-but-
  preferred Confirm step, reusing `InputKind::Confirm` from #205.)
- **Gated behind an interactive flag.** The acknowledge is a *purely cosmetic*
  pause (no game decision, unlike the rules-meaningful commit window). Making it
  unconditional would bake a UX-only pause into mandatory kernel semantics,
  force every non-interactive consumer (tests, future headless/AI) to drive
  through a meaningless pause, and churn ~20+ skill-test tests. Instead it is
  gated on a `GameState` flag (default off); the server turns it on for human
  play. This keeps the kernel honest and the test suite quiet.
- **Engine stays presentation-free.** The result *panel* is rendered entirely
  client-side from the structured events the client already receives. The
  engine's Confirm prompt carries only a short generic string; no display text
  or token-value state is added to the kernel for the panel's sake.

## Engine design (`game-core`)

The skill-test driver in `crates/game-core/src/engine/dispatch/skill_test.rs`
walks a fixed `SkillTestStep` cursor (`PreCommitWindow → AwaitingCommit →
PreTokenWindow → Resolving → DetermineOutcome → FireOnCommit → ApplyFollowUp →
… → PostOnResolution`) with established pause-and-resume idioms (the commit
window, the ST.1/ST.2 player windows). The acknowledge is one more step in this
machine.

### 1. Interactive-acknowledge flag

Add a boolean to `GameState` (e.g. `interactive_acknowledge: bool`), default
`false` (so `Default`/existing constructors and every test are unchanged).
Serde-serialized like the rest of `GameState`. The server sets it `true` when
it creates a game (human play); the web client never sets engine state directly.

### 2. New `SkillTestStep::AcknowledgeOutcome`

Insert a cursor step between `DetermineOutcome` and `FireOnCommit`:

- `DetermineOutcome` already emits `SkillTestSucceeded` / `SkillTestFailed` and
  fires the `SkillTestResolved` timing point. Change only its pre-advance
  target from `FireOnCommit` to `AcknowledgeOutcome`.
- The `advance` arm for `AcknowledgeOutcome`:
  - If `interactive_acknowledge` is **off**: pre-advance to `FireOnCommit` and
    `continue` (no pause — behaviour identical to today).
  - If **on**: pre-advance the cursor to `FireOnCommit`, then return
    `AwaitingInput { request: InputRequest::confirm(<short prompt>),
    resume_token }`. The prompt string is a minimal generic label (e.g.
    "Acknowledge skill-test result"); the rich panel is the client's job.

Because `DetermineOutcome`'s success/failure events and `Resolving`'s
`ChaosTokenRevealed` are emitted in the same drive as the pause, they land in
the **same `Applied` batch** as the `AwaitingInput`. The in-flight `SkillTest`
frame (carrying `difficulty`) is still on the continuation stack — teardown
(`PostOnResolution`) is later — so the client can read `difficulty` from state
if needed.

### 3. Resume routing

`resume_skill_test_commit` in `dispatch/mod.rs` is the `SkillTest`-frame resume
hook. Today it only accepts `PickMultiple` (the commit). Generalise it to
dispatch on the frame's cursor:

- cursor `AwaitingCommit` + `PickMultiple` → `finish_skill_test` (today).
- cursor `AcknowledgeOutcome` + `Confirm` → re-drive (`advance`) from the
  already-pre-advanced `FireOnCommit` cursor.
- otherwise → `Rejected` with a clear reason (mirroring the existing
  wrong-response arm).

### 4. Test-support helper

`drive_to_terminal_no_commits` (in `test_support/resolver.rs`) auto-answers any
`AwaitingInput` with an empty `PickMultiple`. Teach it to answer a `Confirm`-
kind request with `InputResponse::Confirm`. This only matters for tests that
opt into the flag; with the flag default-off, the existing suite is untouched.

## Client design (`web`)

Keep the engine presentation-free — render from the structured events the
client already receives but currently discards.

### 1. Retain events in the store

`crates/web/src/store.rs`: add `last_events: Vec<game_core::Event>` to
`ClientState`; populate it from the `Applied` arm (currently dropped). `Hello`
clears it; `Rejected` leaves it untouched (mirrors the existing field rules).

### 2. Result panel component

New `crates/web/src/skill_test_result.rs` (a small component, following the
`board.rs` / `input.rs` style):

- Reads `last_events` from the store. When the batch contains the skill-test
  resolution events (`ChaosTokenRevealed` + `SkillTestSucceeded`/`Failed`),
  render a panel:
  - **chaos token drawn** — from `ChaosTokenRevealed.token` /
    `.resolution`.
  - **final total vs difficulty** — `difficulty` from the in-flight test in
    `game` state (or the retained `SkillTestStarted`); `total = difficulty +
    margin` (success) or `difficulty - by` (failure); `AutoFail` shows total 0.
  - **outcome** — "Succeeded by N" / "Failed by N" (+ `AutoFail` note when the
    failure reason is `AutoFail`).
- The existing `AwaitingInputView` (`input.rs`) already renders the **Confirm**
  button for `InputKind::Confirm`; the panel sits alongside it. No change to the
  Confirm dispatch.

### 3. Server flips the flag

The server sets `interactive_acknowledge = true` when constructing a new game,
so human play gets the acknowledge step.

## Testing

- **Engine unit tests** (`skill_test.rs` `#[cfg(test)]`):
  - flag **on**: a skill test pauses at `AcknowledgeOutcome` with an
    `AwaitingInput { kind: Confirm }`; the success/failure + `ChaosTokenRevealed`
    events are present in the batch; a `Confirm` resume drives into the
    consequence (e.g. clue discovery) and to `SkillTestEnded`.
  - flag **off**: no pause — resolves straight through (guards against churn /
    proves the gate).
  - a non-`Confirm` response at `AcknowledgeOutcome` rejects cleanly.
- **Test-support**: `drive_to_terminal_no_commits` answers `Confirm` (covered
  by an on-flag no-commits drive).
- **Client**:
  - `store.rs` reducer test: `Applied` retains `last_events`; `Hello` clears
    them.
  - a render/unit test for the panel's total/outcome computation across
    success, failure, and `AutoFail`.

## Out of scope

- The additive math breakdown (base + icons + modifiers + token) — needs new
  engine data; possible follow-up.
- A general event-log panel (this is the focused skill-test slice of #466).
- Multiplayer input routing (solo-scope assumption: the test performer is the
  acting investigator, consistent with the rest of the client).
