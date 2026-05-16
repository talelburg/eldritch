# Phase 1 — Engine bones

## Status

✅ Closed.

## Goal

Engine can apply actions and emit events; test harness ready.

## Issues (all closed)

- `#14` — core state types + `Action` / `Event` enums.
- `#15` — `apply()` loop + `EngineOutcome`.
- `#16` — deterministic RNG + RNG actions.
- `#17` — phase machine + action points.
- `#18` — fluent `TestGame` builder.
- `#20` — event-assertion macros.

## Decisions made

- **`apply(state, action) -> ApplyResult`** is the only entry point for state mutation. The action log is a flat `Vec<Action>`; replaying from initial state reproduces current state bit-for-bit.
- **Validate-first / mutate-second** as the per-handler convention: every dispatch handler checks every precondition before any mutation. On rejection, neither state nor events should be touched. The apply loop has a belt-and-suspenders `events.clear()` on `Rejected`. (Convention, not yet structurally enforced; tracked in the `apply()` doc as a TODO.)
- **Deterministic RNG** via `RngState { seed, draws }` + ChaCha8. Reconstructed on demand from the state; deck shuffles and chaos-token draws are reproducible from the action log.
- **Fluent `TestGame` builder** as the single source of test-state-construction defaults. Adders are `with_*`; build is terminal. Fixtures `test_investigator(id)`, `test_location(id, name)`, `test_enemy(id, name)` provide reasonable defaults.
- **Event-assertion macros** are order-insensitive by default: `assert_event!`, `assert_no_event!`, `assert_event_count!`. Plain `assert_eq!` on the events slice when exact contiguous order matters. (`assert_event_sequence!` for subsequence-in-order checks landed later, in PR `#95`.)

## Dependencies

Phase 0 (foundations).

## What "done" looked like

Engine compiles to native + wasm32; `apply` round-trips deterministically; `TestGame` builder + macros + fixtures available; first test cases pass.
