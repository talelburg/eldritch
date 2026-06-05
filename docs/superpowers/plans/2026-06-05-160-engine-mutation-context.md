# `Cx` Mutation-Context Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `(&mut GameState, &mut Vec<Event>)` parameter pair threaded through ~96 engine functions with a single `Cx<'a>` field bundle, with zero behavior change.

**Architecture:** A `pub(crate) struct Cx<'a> { state, events }` bare field bundle, threaded as `cx: &mut Cx` into free functions (not methods, no helpers). Migration proceeds module-by-module; at the migration frontier, calls between migrated and not-yet-migrated functions are bridged with two mechanical shims so every intermediate step compiles and the test suite stays green.

**Tech Stack:** Rust, `cargo` workspace (`game-core` kernel crate). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-06-05-160-engine-mutation-context-design.md`

---

## Background the implementer needs

### This is a pure refactor — no new tests, no behavior change

The existing test suite IS the correctness oracle. **Do not write new tests.** Each task's verification is "the suite still passes." A "Rejected leaves state untouched" structural test is explicitly *out of scope* (it belongs to issue #161).

### The transformation rule (uniform across every function)

A function that currently looks like:

```rust
pub(super) fn investigate(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != Phase::Investigation { /* ... */ }
    events.push(Event::Investigated { /* ... */ });
    // ...
}
```

becomes:

```rust
pub(super) fn investigate(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if cx.state.phase != Phase::Investigation { /* ... */ }
    cx.events.push(Event::Investigated { /* ... */ });
    // ...
}
```

i.e. drop the two params, add `cx: &mut Cx` as the **first** param, and rewrite the body: `state.` → `cx.state.`, `events.` → `cx.events.`, bare `state` → `cx.state`, bare `events` → `cx.events`.

Read-only functions that take `state: &GameState` (no `events`) are **left unchanged** — e.g. everything in `dispatch/cursor.rs`. A migrated caller calls them as `read_fn(cx.state, …)`.

### The two frontier shims (both are temporary; all are gone by the final task)

Because migration is incremental, a migrated function may call a not-yet-migrated one and vice-versa. Bridge each direction mechanically:

**Shim A — migrated function calls a raw (not-yet-migrated) function** → *unbundle*:

```rust
// callee still takes (state, events, …):
phases::start_scenario(cx.state, cx.events);
```

**Shim B — raw (not-yet-migrated) function calls a migrated function** → *re-bundle*:

```rust
// inside a fn that still has `state: &mut GameState, events: &mut Vec<Event>`:
let mut cx = Cx { state, events };
let outcome = cards::play_card(&mut cx, investigator, hand_index);
// `cx` drops here; `state`/`events` are usable again afterward
```

`Cx { state, events }` moves the reborrowed `&mut` references in; after `cx` drops at end of scope, `state`/`events` are free again (non-lexical lifetimes). If the function needs `state`/`events` again *after* the call in the same statement sequence, that's fine — the borrow ends when `cx` is last used.

### Migration order

Top-down from the entry point minimizes shim churn (you mostly only need Shim A). Module order below follows the rough call hierarchy. If the borrow checker forces a Shim B somewhere, use it — it's mechanical and removed when the callee's module migrates.

### The `eval_ctx` rename

The evaluator's semantic context is `EvalContext`, conventionally bound to a variable named `ctx`. Once `Cx`/`cx` exists, `cx` and `ctx` read ambiguously side-by-side. So **wherever an `EvalContext` value named `ctx` co-occurs with `cx`**, rename it `ctx` → `eval_ctx`. This affects the evaluator module (Task 13) and these 5 dispatch modules: `abilities`, `cards`, `reaction_windows`, `encounter`, `skill_test` (Tasks for those modules). `apply_effect` takes `ctx: EvalContext` **by value** (it is `Copy`); keep it by value, just rename to `eval_ctx`.

### Inner-loop verification commands

Fast loop after each task:

```sh
cargo build -p game-core --all-features
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
```

Full CI gauntlet (run on Task 1 to baseline, and on the final task):

```sh
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```

---

## Task 1: Define `Cx` and migrate the entry point

**Files:**
- Create: `crates/game-core/src/engine/cx.rs`
- Modify: `crates/game-core/src/engine/mod.rs` (add `mod cx;`, construct `cx`, migrate `fire_scenario_resolution`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`apply_player_action`, `apply_engine_record`, `resolve_input`)

- [ ] **Step 1: Create the `Cx` type**

`crates/game-core/src/engine/cx.rs`:

```rust
//! The engine's mutation context: the mutable working set threaded
//! through every dispatch handler and effect evaluation.
//!
//! `Cx` bundles the two `&mut` references that previously rode together
//! by hand through every signature — the [`GameState`] being mutated and
//! the [`Event`] buffer being emitted into. It is *not* a semantic
//! context: the "you"/"source" of card text lives in
//! [`EvalContext`](super::EvalContext), which travels alongside `Cx` as a
//! separate `eval_ctx` parameter in the evaluator.
//!
//! Bare field bundle by design — no helper methods. Read-only callees
//! keep taking `&GameState`; a holder of `cx` calls them as
//! `read_fn(cx.state, …)`, which borrows only `cx.state` and leaves
//! `cx.events` independently usable (disjoint-field borrowing).

use crate::event::Event;
use crate::state::GameState;

/// Mutable engine working set: the state being mutated plus the event
/// buffer being emitted into. Threaded as `cx: &mut Cx` through dispatch
/// handlers and the effect evaluator.
pub(crate) struct Cx<'a> {
    /// The game state being mutated.
    pub state: &'a mut GameState,
    /// The events emitted by the current `apply` call.
    pub events: &'a mut Vec<Event>,
}
```

- [ ] **Step 2: Declare the module and re-export `Cx` within the engine**

In `crates/game-core/src/engine/mod.rs`, add the module declaration near the other `mod` lines and make `Cx` reachable from sibling modules:

```rust
mod cx;
pub(crate) use cx::Cx;
```

- [ ] **Step 3: Migrate `fire_scenario_resolution` and the construction site**

In `crates/game-core/src/engine/mod.rs`, change `fire_scenario_resolution`'s signature from `(state: &mut GameState, events: &mut Vec<Event>, registry: …)` to `(cx: &mut Cx, registry: …)` and rewrite its body (`state.` → `cx.state.`, `events.push` → `cx.events.push`).

Then in `apply_with_scenario_registry`, build the `cx` and route through it:

```rust
let mut state = state;
let mut events = Vec::new();
let resolution_already_fired = state.resolution.is_some();
let mut cx = Cx { state: &mut state, events: &mut events };
let outcome = match action {
    Action::Player(p) => dispatch::apply_player_action(&mut cx, &p),
    Action::Engine(e) => dispatch::apply_engine_record(&mut cx, &e),
};
if matches!(outcome, EngineOutcome::Rejected { .. }) {
    cx.events.clear();
} else if !resolution_already_fired {
    fire_scenario_resolution(&mut cx, registry);
}
drop(cx);
ApplyResult { state, events, /* … unchanged … */ }
```

(The explicit `drop(cx)` makes the borrow release before `state`/`events` move into `ApplyResult`; NLL would also handle it, but the explicit drop is clearer.)

- [ ] **Step 4: Migrate the three dispatchers in `dispatch/mod.rs`**

Change `apply_player_action`, `apply_engine_record`, and `resolve_input` to take `cx: &mut Cx` instead of `(state, events)`. Rewrite their bodies with the transformation rule. Every call into a `dispatch::*` submodule stays raw for now — use **Shim A** at each:

```rust
PlayerAction::StartScenario => phases::start_scenario(cx.state, cx.events),
PlayerAction::Investigate { investigator } => {
    actions::investigate(cx.state, cx.events, *investigator)
}
// … etc for every arm; and the guard ladder's `state.` → `cx.state.`
```

Likewise `phases::investigation_phase(cx.state, cx.events)` in the post-mulligan block, and the `hunters::`/`reaction_windows::`/`skill_test::` calls in `resolve_input`.

- [ ] **Step 5: Build and test**

Run:
```sh
cargo build -p game-core --all-features
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
```
Expected: builds clean, all tests PASS. (`Cx` may trigger a `dead_code` warning on the `state`/`events` fields if something is off — they're read via `cx.state`/`cx.events`, so a clean build means the wiring is right.)

- [ ] **Step 6: Run the full CI gauntlet to baseline**

Run the five-job gauntlet from the Background section. Expected: all green. This baselines the refactor.

- [ ] **Step 7: Commit**

```sh
git add crates/game-core/src/engine/cx.rs crates/game-core/src/engine/mod.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: introduce Cx bundle, migrate apply entry point (#160)"
```

---

## Tasks 2–12: Migrate one dispatch submodule each

Each task migrates every `(state, events)`-taking function in one module to `cx: &mut Cx`, fixes call sites at the module's boundaries, and (for the 5 flagged modules) renames `EvalContext` locals `ctx` → `eval_ctx`. The procedure is identical per module; only the file and function count differ.

**Per-module procedure (apply to each module below):**

- [ ] **Step 1: Migrate every function in the module** that takes `state: &mut GameState, events: &mut Vec<Event>`. Apply the transformation rule (drop the pair, add `cx: &mut Cx` first, rewrite body). Functions taking only `state: &GameState` (read-only) are left unchanged.

- [ ] **Step 2: Fix intra-module call sites** — calls between functions now both taking `cx` pass `cx` directly: `helper(cx, …)`.

- [ ] **Step 3: Fix outbound calls into not-yet-migrated modules** with **Shim A** (unbundle): `other_mod::raw_fn(cx.state, cx.events, …)`.

- [ ] **Step 4: Fix inbound calls from already-migrated modules** — callers that previously used Shim A (`this_mod::fn(cx.state, cx.events, …)`) now pass `cx` directly: `this_mod::fn(cx, …)`. Search the whole `engine/` tree for callers of each migrated function and update them. If a *not-yet-migrated* module calls into this one, bridge with **Shim B** (re-bundle).

- [ ] **Step 5 (flagged modules only): rename `EvalContext` locals** `ctx` → `eval_ctx` so they don't read ambiguously next to `cx`. Flagged: `abilities`, `cards`, `reaction_windows`, `encounter`, `skill_test`. The `apply_effect` call becomes `apply_effect(cx.state, cx.events, &effect, eval_ctx)` while `apply_effect` itself is still raw (it migrates in Task 13) — Shim A applies to it until then.

- [ ] **Step 6: Build + test**
```sh
cargo build -p game-core --all-features
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
```
Expected: clean build, all tests PASS.

- [ ] **Step 7: Commit** with message `engine: thread Cx through dispatch::<module> (#160)`.

**Module order and counts** (do these as separate tasks/commits, top-down):

- [ ] **Task 2 — `dispatch/phases.rs`** (19 functions; the phase-driver hub, called by `mod.rs`)
- [ ] **Task 3 — `dispatch/actions.rs`** (5 functions: `investigate`, `move_action`, `fight`, `evade`, +1)
- [ ] **Task 4 — `dispatch/cards.rs`** (9 functions; **flagged** — `eval_ctx` rename; note `cards::grant_resources` is reached by the evaluator via its full path, keep it `pub(super)`)
- [ ] **Task 5 — `dispatch/skill_test.rs`** (9 functions; **flagged**)
- [ ] **Task 6 — `dispatch/combat.rs`** (6 functions)
- [ ] **Task 7 — `dispatch/encounter.rs`** (11 functions; **flagged**)
- [ ] **Task 8 — `dispatch/hunters.rs`** (9 functions)
- [ ] **Task 9 — `dispatch/reaction_windows.rs`** (7 functions; **flagged**)
- [ ] **Task 10 — `dispatch/act_agenda.rs`** (7 functions)
- [ ] **Task 11 — `dispatch/abilities.rs`** (2 functions; **flagged**)
- [ ] **Task 12 — `dispatch/elimination.rs`** (4 functions)

(`dispatch/cursor.rs` has zero `&mut GameState` functions — nothing to migrate.)

---

## Task 13: Migrate the evaluator and remove the last shims

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`apply_effect` + 5 internal effect helpers + the `#[cfg(test)]` callers)
- Modify: any dispatch module still using Shim A to call `apply_effect`

- [ ] **Step 1: Migrate `apply_effect` and its effect helpers**

Change `apply_effect`'s signature from:

```rust
pub fn apply_effect(
    state: &mut GameState,
    events: &mut Vec<Event>,
    effect: &Effect,
    ctx: EvalContext,
) -> EngineOutcome
```

to:

```rust
pub fn apply_effect(
    cx: &mut Cx,
    effect: &Effect,
    eval_ctx: EvalContext,
) -> EngineOutcome
```

Do the same for the internal effect-application helpers that thread `(state, events, …, ctx)` (`apply_if`, `apply_seq`, and the per-effect appliers around lines 237/290/324, plus `gain_resources`/`discover_clue`): add `cx: &mut Cx` first, drop the pair, rename `ctx` → `eval_ctx`, rewrite bodies. Intra-evaluator calls pass `cx` and `eval_ctx` through.

- [ ] **Step 2: Update the dispatch call sites to pass `cx`**

The 7 `apply_effect(cx.state, cx.events, &effect, eval_ctx)` Shim-A sites (in `abilities.rs`, `encounter.rs` ×2, `cards.rs`, `skill_test.rs` ×2, `reaction_windows.rs`) become `apply_effect(cx, &effect, eval_ctx)`.

- [ ] **Step 3: Update the evaluator's own `#[cfg(test)]` tests**

The evaluator's in-file tests construct a state + events and call `apply_effect`. Update them to build a `Cx` and pass it:

```rust
let mut events = Vec::new();
let mut cx = Cx { state: &mut state, events: &mut events };
let outcome = apply_effect(&mut cx, &effect, eval_ctx(1));
```

(The test helper `fn ctx(id) -> EvalContext` may be renamed `eval_ctx` for consistency, but it's local to the test module and optional.)

- [ ] **Step 4: Sweep for any remaining shims**

Search the engine tree for leftover frontier bridges — there should be none in production code:
```sh
grep -rn "cx.state, cx.events" crates/game-core/src/engine/
grep -rn "Cx { state, events }" crates/game-core/src/engine/
```
Expected: only `read_fn(cx.state, …)` style single-field uses remain (calls into genuinely read-only `&GameState` functions). No `(cx.state, cx.events)` pair passed to a migratable function; no Shim-B re-bundle left.

- [ ] **Step 5: Build + test**
```sh
cargo build -p game-core --all-features
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
```
Expected: clean, all PASS.

- [ ] **Step 6: Full CI gauntlet**

Run all five jobs. Pay attention to:
- `doc` — the `pub use evaluator::{apply_effect, EvalContext}` re-export still resolves; the intra-doc link `[apply_effect]` in `state/game_state.rs:732` must still link. `RUSTDOCFLAGS="-D warnings" cargo doc` catches breakage.
- `clippy` — watch for `needless_pass_by_ref_mut` on any function that ends up only reading `cx` (unlikely, but if flagged, that function should have stayed on `&GameState` — convert it).
- `wasm-build` — `game-core` is `wasm32`-clean; confirm the bundle didn't pull anything non-portable (it won't — pure refactor).

Expected: all green.

- [ ] **Step 7: Commit**
```sh
git add crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/
git commit -m "engine: thread Cx through evaluator; remove frontier shims (#160)"
```

---

## Task 14: Final verification and phase doc

**Files:**
- Modify: `docs/phases/` — only if an open question or arc row references this issue (check `docs/phases/README.md` "Cross-cutting / unmilestoned work" — #160 is unmilestoned, so likely no phase-doc row to flip; if none, skip per the "lean toward skipping" rule).

- [ ] **Step 1: Confirm the invariant holds across the suite**

Run the full gauntlet once more from a clean tree:
```sh
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```
Expected: all green. This is the zero-behavior-change proof.

- [ ] **Step 2: Sanity-check the diff is purely mechanical**

```sh
git diff main --stat
```
Expected: changes confined to `crates/game-core/src/engine/` (plus the new `cx.rs` and the two spec/plan docs). No card, scenario, server, or web source touched.

- [ ] **Step 3: Push and open the PR**

Per the repo PR procedure: push `engine/mutation-context-cx`, open the PR with `gh pr create` (template; design-decisions paragraph referencing the spec), `Closes #160.`, then `gh pr checks --watch`.

---

## Self-review notes (for the executor)

- **Spec coverage:** Cx type (Task 1) ✔; dispatch migration (Tasks 2–12) ✔; evaluator + `eval_ctx` rename (Task 13) ✔; read-only fns stay `&GameState` (transformation rule) ✔; no helpers / no validate-split (by omission — nothing in the plan adds them) ✔; zero-behavior-change oracle = existing suite (every task's verify) ✔.
- **No new tests** is intentional, per spec — do not add a "reject leaves state untouched" test here (that's #161).
- **Counts** (19/5/9/9/6/11/9/7/7/2/4 = 88 dispatch functions + ~6 evaluator + 1 `fire_scenario_resolution` + 3 dispatchers ≈ 96 sites) are from `grep -c "state: &mut GameState"` per file at plan time; if a count is off by one in the actual file, migrate whatever's there — the rule is uniform.
