# #205 — Structured input rendering via an `InputKind` discriminator

**Date:** 2026-06-26
**Issue:** #205 (structured input rendering, client half)
**Phase:** 7 — The Gathering. This is the **sole remaining gate item**: the
in-browser end-to-end playthrough stalls at the first Mythos encounter draw.

## Problem

`InputRequest` (`crates/game-core/src/engine/outcome.rs`) carries only
`{ prompt: String, options: Vec<ChoiceOption> }` — there is **no discriminator
for which `InputResponse` variant the prompt expects**. So the web client
(`AwaitingInputView`, `crates/web/src/input.rs`) chooses the control purely by
whether `options` is empty:

- `options` non-empty → `PickSingle` option-list — works.
- `options` empty → the legacy `PickMultiple` hand-card "Commit" UI — the only
  fallback.

But empty-`options` prompts are **not** all `PickMultiple`. Three different
responses hide behind empty options:

| Prompt | Expected `InputResponse` | Source |
|---|---|---|
| Mythos encounter draw | `Confirm` | `encounter.rs::resume_encounter_draw` |
| Skill-test commit / setup mulligan / hand-size discard | `PickMultiple` | `cards.rs`, `skill_test.rs`, `phases.rs` |

The client renders `PickMultiple` for all of them, so the Mythos draw (which
expects `Confirm`) rejects:

> `ResolveInput: Mythos encounter draw expects InputResponse::Confirm, got PickMultiple`

A **second, latent** instance of the same class of bug: the reaction / fast-play
**resolution windows** (`reaction_windows.rs`) are `PickSingle` prompts that can
*also* be declined with `Skip`, but the browser renders no Skip/Pass control —
so an open window can't be passed. Not yet hit in the current playthrough (it
stalls at the Mythos draw first), but the same root cause.

A prompt-text heuristic (sniffing `"Confirm"` out of `prompt`) is explicitly
**not** the fix — that is the heuristic this issue exists to remove.

## Solution

Add an expected-response discriminator to `InputRequest` so the client renders
the right control with no heuristics.

### Type changes (`crates/game-core/src/engine/outcome.rs`)

A new enum, and two new **required** fields on the one struct:

```rust
/// Which `InputResponse` variant the client must echo back for a prompt.
/// Names mirror the `InputResponse` variants 1:1 so the kind *is* the
/// expected response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InputKind {
    PickSingle,   // -> InputResponse::PickSingle(id)        — single-option list
    PickMultiple, // -> InputResponse::PickMultiple { selected } — subset select
    Confirm,      // -> InputResponse::Confirm               — single button
}

pub struct InputRequest {
    pub prompt: String,
    pub options: Vec<ChoiceOption>,
    pub kind: InputKind,   // NEW
    pub skippable: bool,   // NEW
}
```

`skippable` is **orthogonal** to `kind`: it is a flag, not a fourth kind. When
true the client also renders a Skip/Pass button → `InputResponse::Skip`. This
matches the issue's note that Skip is an affordance available *alongside* a
window, not a standalone prompt.

### Constructors

Replace today's `prompt()` / `choice()` with typed constructors:

- `InputRequest::pick_single(text, options)` → `kind: PickSingle`, `skippable: false`
- `InputRequest::pick_multiple(text)` → `kind: PickMultiple`, empty `options`
- `InputRequest::confirm(text)` → `kind: Confirm`, empty `options`
- chainable `.skippable(self) -> Self` → sets `skippable = true` (for reaction /
  fast windows)

`pick_multiple` carries this doc caveat (a plain note, **no** `TODO`/issue — it
is a "could happen", not a "will happen"):

> `options` is left empty: every current consumer (skill-test commit, setup
> mulligan, hand-size discard) picks a subset of the *prompted investigator's
> hand*, and the client derives candidates from the hand, treating each
> `OptionId(i)` as hand index `i`. This hand-index convention only holds while
> `PickMultiple` decisions are hand-scoped; a future subset-pick over non-hand
> candidates (e.g. revealed cards, enemies) would need to carry them in
> `options` and render from there, like `pick_single`.

### Engine call-site migration

Mechanical swap to typed constructors; no behavior change beyond the new
metadata:

| Site | Today | Becomes |
|---|---|---|
| `encounter.rs:589` Mythos draw | `prompt` | `confirm(…)` ← **the gate fix** |
| `skill_test.rs:594` commit window | `prompt` | `pick_multiple(…)` |
| `cards.rs:293` setup mulligan | `prompt` | `pick_multiple(…)` |
| `phases.rs:1118` hand-size discard | `prompt` | `pick_multiple(…)` |
| `mod.rs:113` turn menu; `hunters.rs:421`; `choice.rs:49` DSL `ChooseOne`; `combat.rs:897/1139`; `skill_test.rs:128` substitution; `encounter.rs:458` | `choice` | `pick_single(…)` |
| `reaction_windows.rs:587/887` resolution windows | `choice` | `pick_single(…)` + `.skippable()` **iff `!forced`** |

The reaction-window sites already compute a `skip_hint` from
`window.is_forced()`; reuse that **exact** condition to gate `.skippable()` so
the button and the prompt text cannot drift.

Test-support sites get the same swap: `test_support/fixtures.rs:131/147` and the
`req()` helper in `test_support/resolver.rs`. The `ScriptedResolver` itself is
FIFO-scripted (tests call `.confirm()` / `.skip()` / `.commit_cards()`
explicitly), so it does **not** infer `kind` — no resolver logic changes.

### Web client rendering (`crates/web/src/input.rs`)

`AwaitingInputView` stops branching on `request.options.is_empty()` and switches
on `request.kind`:

- **`PickSingle`** → existing option-list (one button per `ChoiceOption` →
  `ResolveInput(PickSingle(id))`). Unchanged.
- **`PickMultiple`** → existing hand-card multi-select + "Commit" →
  `ResolveInput(PickMultiple { selected })`. Unchanged behavior; now reached by
  `kind`, not by emptiness.
- **`Confirm`** → **new**: a single "Confirm" button → `ResolveInput(Confirm)`.
  Unblocks the Mythos draw.

Then, independent of kind: **if `request.skippable`, also render a "Skip"
button** → `ResolveInput(Skip)`. Fixes the reaction / fast-window decline path.

The module doc comment (currently describing the two-branch options-empty
heuristic) is rewritten to describe the three-way `kind` switch + the skippable
affordance.

## Backward compatibility

`seed_outcome` (server migration `0002`) is a **TEXT column holding a serialized
`EngineOutcome`** — for a mulligan-pending seed that is
`AwaitingInput { request: InputRequest { … } }`. The two new **required** fields
change that JSON's shape, so existing rows fail to deserialize on `load`.

Decision: **required fields, no `serde(default)`, no new SQL migration.**

- Consistent with #453's established stance (fields required-on-the-wire; loud
  errors beat silent degradation). A `serde(default)` on `kind` would
  reintroduce exactly the silent-inference pattern #453 removed.
- The **column** schema is unchanged (it is opaque JSON), so no `.sql` migration
  is warranted; only stale row *content* is affected.
- Pre-release, no real data: recreate the local SQLite file. There are two
  existing migrations (`0001_init`, `0002_seed_outcome`), applied at startup via
  `sqlx::migrate!`; we add none.

## Testing

- **`outcome.rs` serde round-trips** updated for the new fields; one unit test
  per constructor asserting `kind` + `skippable`.
- **Engine:** the existing Mythos-draw test (`encounter.rs:1904/1964`) already
  exercises the `Confirm` round-trip and stays green; extend it to assert the
  emitted request carries `kind: InputKind::Confirm`. Add an assertion that a
  non-forced reaction window emits `skippable: true` and a forced one
  `skippable: false`.
- **wasm client:** add/extend a `crates/web` wasm test asserting
  `AwaitingInputView` renders a Confirm button for `kind: Confirm`, and a Skip
  button when `skippable` — the regression guard for the gate.
- **CI gauntlet** (all seven jobs, strict flags) before push, including
  `wasm-test` and `wasm-clippy`, since this touches `crates/web`.

## Out of scope

- **Structured options for `PickMultiple`** (carrying hand candidates in
  `options` rather than client-derived hand indices) — deferred; see the
  `pick_multiple` caveat above.
- **Richer per-option metadata** beyond `label` (the broader `#205` enrichment
  the open-turn menu wants) — separate concern; this PR is the discriminator
  only.

## Done criteria

- The in-browser solo Gathering playthrough progresses past the first Mythos
  encounter draw (Confirm renders and resolves).
- Reaction / fast-play windows render a working Skip control.
- All seven CI jobs green; phase-7 doc's "End-to-end browser playthrough" bullet
  updated (stall resolved) as the final commit.
