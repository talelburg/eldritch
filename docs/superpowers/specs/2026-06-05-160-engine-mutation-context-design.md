# Consolidate `(&mut GameState, &mut Vec<Event>)` threading into a `Cx` context (#160)

**Date:** 2026-06-05
**Issue:** #160
**Type:** Pure refactor — behavior-preserving signature change. No logic changes.

## Problem

Nearly every function in the engine threads the same two mutable references by
hand: `state: &mut GameState` and `events: &mut Vec<Event>`. In the dispatch
layer alone there are ~91 signature sites taking the pair; the evaluator adds 5
more. It is the single most-repeated pattern in the engine — it bloats every
signature and makes call sites noisy (`foo(state, events, a, b)`).

This is debt worth clearing **before Phase 5** puts a server on top of the
engine: refactoring the engine's core mutation-threading pattern is cheapest
while the engine is the only consumer of these signatures.

## Why this is a clean move

- **Zero behavior change.** Only signatures and call sites change; bodies are a
  mechanical `state.` → `cx.state.` / `events.push` → `cx.events.push` rewrite.
  The full existing test suite plus the CI gauntlet is the correctness oracle.
- **No new tests.** A "Rejected leaves state untouched" structural test belongs
  to #161 (the two-phase `apply()` refactor), not here.
- **Disjoint-field borrowing is preserved** (see below), so the migration does
  not introduce borrow-checker friction.

## Decisions settled in brainstorming

1. **Bare field bundle, free functions** — not methods on the context, not an
   engine struct. Methods read cleaner at call sites but are a much larger diff
   and invite partial-borrow pain. Free functions taking `cx: &mut Cx` is the
   smallest, most mechanical migration.
2. **No helpers in v1** — no `cx.emit(e)`, no `cx.investigator_mut(id)`.
   - Helpers are a *separable* concern: they can be added in a later 10-line
     follow-up with zero churn to the ~95 migrated sites, because they don't
     change `Cx`'s shape. "No helpers now" is cheaply reversible; "helpers now"
     bakes a guess into the big diff.
   - `*_mut`-style accessors are *actively harmful*: a method like
     `cx.investigator_mut(id)` borrows all of `*cx`, so you couldn't touch
     `cx.events` while holding the result. Raw field access
     (`cx.state.investigators.get_mut(&id)`) borrows only `cx.state`, leaving
     `cx.events` independently borrowable. Accessor methods would *remove* an
     ergonomic property we want to keep.
   - `cx.emit(e)` is the strongest helper candidate (59 `events.push` sites, no
     borrow downside) but is pure sugar today. It gains a real job in #161 (an
     emission chokepoint for the apply pass), so it's deferred to land there
     with a purpose rather than guessed at now.
3. **Include the evaluator** (5 sites) so the whole engine mutation path speaks
   one language and there's no seam where dispatch speaks `Cx` but its callee
   doesn't.
4. **Naming: `Cx` / `cx`.** The evaluator already has a *semantic* context,
   `EvalContext` (`controller`, `source` — the "you"/"source" of card text;
   `Copy`, no mutable refs). `Cx` is a different animal: the *mutation-plumbing*
   bundle. They will co-occur in evaluator signatures, so the existing
   `EvalContext` parameter is renamed `ctx` → `eval_ctx` throughout the
   evaluator module to keep the two visibly distinct:
   `apply_effect(cx: &mut Cx, effect, eval_ctx: &EvalContext)`.

## Design

### The type

A new `pub(crate)` type in a dedicated `crates/game-core/src/engine/cx.rs`:

```rust
pub(crate) struct Cx<'a> {
    pub state:  &'a mut GameState,
    pub events: &'a mut Vec<Event>,
}
```

Pure field bundle. No `Clone`/`Copy` (it holds `&mut`), no helper methods, no
constructor ceremony — built with a struct literal at the one place it
originates. Fields are `pub(crate)` so handlers write `cx.state…` /
`cx.events.push(…)` directly.

### What changes

- **Construction point** — `apply_with_scenario_registry` (`engine/mod.rs`)
  builds `let mut cx = Cx { state: &mut state, events: &mut events }` and passes
  `&mut cx` into `apply_player_action` / `apply_engine_record` /
  `fire_scenario_resolution`. After the dispatch call `cx`'s borrows end (NLL),
  so the owned `state` / `events` move into `ApplyResult` exactly as today. The
  belt-and-suspenders backstop becomes `cx.events.clear()`.
- **Dispatch handlers (~91 sites)** — `fn investigate(cx: &mut Cx, inv: …)`
  instead of `(state, events, inv, …)`. Bodies rewrite `state.` → `cx.state.`,
  `events.push` → `cx.events.push`.
- **Evaluator (5 sites)** — same migration, plus the `ctx: &EvalContext` →
  `eval_ctx: &EvalContext` rename throughout the evaluator module.
- **Read-only functions stay on `&GameState`.** A handler holding `cx` calls
  them as `read_fn(cx.state, …)` — disjoint-field borrowing keeps `cx.events`
  independently usable. This is the ergonomic property the "no accessor helpers"
  decision exists to preserve.

### Explicit non-goals (deferred to #161)

- **No validate/apply split.** `Cx` is a single mutating context. #161 owns
  introducing a read-only validate pass and may add a read-only view then. The
  existing `check_play_card` / `PlayCheckResult` plan-then-apply seam is left
  exactly as-is.
- **No `emit` or accessor helpers** (see Decision 2).

## Migration shape (keeps it compiling + reviewable)

The signatures are interdependent, but a migrated function can **unbundle at the
frontier** — call a not-yet-migrated callee as `f(cx.state, cx.events, …)`. So
the migration proceeds incrementally with the suite green between each step:

1. **Define `Cx`; wire the entry point.** `apply_with_scenario_registry`
   constructs `cx` and immediately unbundles into today's handlers
   (`apply_player_action(&mut cx.state, &mut cx.events, …)` initially, or migrate
   the two entry dispatchers in this step). Compiles, tests pass.
2. **Migrate one dispatch submodule at a time** — `actions`, `cards`, `combat`,
   `encounter`, `hunters`, `phases`, `reaction_windows`, `skill_test`,
   `act_agenda`, `abilities`, `elimination`, `cursor` — unbundling at calls into
   not-yet-migrated modules. Run the suite each step.
3. **Migrate the evaluator** (+ `eval_ctx` rename) **and `fire_scenario_resolution`**;
   remove the last unbundling sites. Full CI gauntlet.

## What "done" looks like

- `Cx<'a>` exists in `engine/cx.rs`; every dispatch handler and evaluator
  function that previously took `(&mut GameState, &mut Vec<Event>)` now takes
  `cx: &mut Cx`.
- No remaining unbundling sites (`f(cx.state, cx.events, …)`) except where the
  callee is a genuinely read-only `&GameState` function.
- The `EvalContext` parameter is named `eval_ctx` wherever it co-occurs with
  `cx`.
- The full CI gauntlet (`fmt`, `clippy -D warnings`, `test`, `doc`,
  `wasm-build`) is green with no behavior change.

## Dependencies

- #159 (dispatch split) — landed (`c961264`). Soft prerequisite: the migration
  happens against navigable per-domain files rather than one 9.5k-line file.
- Coordinates with #161 (two-phase `apply()`): `Cx` is the single mutating
  context #161 builds on when it introduces the read-only validate pass.
