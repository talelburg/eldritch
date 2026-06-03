# #147 — Mulligan in player order: cursor model

**Date:** 2026-06-03
**Issue:** [#147](https://github.com/talelburg/eldritch/issues/147) (engine, p2-later) — a `#137` follow-up.
**Milestone:** phase-4-scenario-plumbing.

## Goal

Replace the order-insensitive mulligan model with a `turn_order`-driven
cursor that enforces "each player, in player order, may mulligan once",
mirroring the existing `mythos_draw_pending` cursor exactly.

Rules Reference:

- p.16: *"Players take or forgo the opportunity to mulligan in player order."*
- Setup step 8, p.27: *"Draw opening hands. Each player draws 5 cards. Each
  player, in player order, may mulligan once at this time."*

The current model — `GameState.mulligan_window: bool` + per-investigator
`Investigator.mulligan_used: bool` + an "all investigators `mulligan_used`"
completion scan in `apply_player_action` — accepts `Mulligan` from any
investigator in any order until everyone has gone. That diverges from "in
player order."

The cursor/Mythos analogy is exact:

| Mythos 1.4 | Mulligan (setup) |
|---|---|
| `mythos_draw_pending: Option<InvestigatorId>` | `mulligan_pending: Option<InvestigatorId>` |
| `DrawEncounterCard` valid only when `pending == Some(actor)` | `Mulligan { investigator }` valid only when `pending == Some(investigator)` |
| advance cursor in player order after each draw | advance cursor in player order after each mulligan |
| `None` → open `MythosAfterDraws` | `None` → setup ends → Investigation 2.1 begins |

## State changes

- **`GameState`** (`state/game_state.rs`): remove `mulligan_window: bool`;
  add `mulligan_pending: Option<InvestigatorId>`, placed next to
  `mythos_draw_pending` with parallel documentation.
- **`Investigator`** (`state/investigator.rs`): remove `mulligan_used: bool`.
  The cursor is now the single source of truth — a second mulligan is
  rejected because the cursor has already moved past you. Drops from the
  struct definition, its serde round-trip test, and `test_support/fixtures.rs`.
- **Test builder** (`test_support/builder.rs`): replace the `mulligan_window`
  field + `with_mulligan_window_open()` with a `mulligan_pending:
  Option<InvestigatorId>` field + `with_mulligan_pending(id)`.

## Engine changes (`engine/dispatch.rs`)

1. **`start_scenario`** — replace `state.mulligan_window = true;` with
   `state.mulligan_pending = first_active_investigator(state);` (the same
   `turn_order`-based seed Mythos/Enemy use). `turn_order` is populated by
   the host/builder before `StartScenario`; if it is empty / all-eliminated
   the cursor seeds `None` — the same degenerate no-op as today (documented,
   not specially handled).

2. **`mulligan()` handler** — the single check
   `mulligan_pending == Some(investigator)` replaces all three old
   validations: window-open, "already used", and "it's your turn". Because
   the cursor only ever holds an `Active` `turn_order` id (seeded/advanced
   via `first_active_investigator` / `next_active_investigator_after`, which
   skip non-Active investigators), a mismatch covers *setup-over* (`None`),
   *too-early / wrong-player*, and *already-went* uniformly. Index
   bounds/uniqueness validation is unchanged. On success, advance:
   `state.mulligan_pending = next_active_investigator_after(state, investigator)`.

   The investigator's existence in `state.investigators` is guaranteed by
   the cursor invariant (turn_order ids are in the map); implementation
   mirrors `draw_encounter_card`'s trust of its cursor.

3. **`apply_player_action` outer gate** — the setup gate keys off
   `state.mulligan_pending.is_some()` instead of `state.mulligan_window`.

4. **`apply_player_action` completion block** — fire `investigation_phase`
   when `state.mulligan_pending.is_none()` (cursor reached the end) instead
   of the `all(|inv| inv.mulligan_used)` scan; drop the
   `state.mulligan_window = false` line. This is the ~5-line kickoff swap
   `#137` flagged ("swap the kickoff trigger from the `mulligan_window` flip
   to the `mulligan_pending` cursor reaching `None`").

## Doc updates

`action.rs` `Mulligan` doc comment + `game_state.rs` / `investigator.rs`
field docs rewritten from the window/flag model to the cursor model.

## Test changes (mechanical but broad)

- **Inverted semantics**: `multi_investigator_mulligan_order_does_not_matter`
  becomes `multi_investigator_mulligan_out_of_order_is_rejected` —
  `Mulligan{inv2}` while the cursor is on `inv1` must reject; then inv1
  mulligans → cursor advances to inv2 → inv2 mulligans → cursor `None` →
  Investigation begins.
- **Updated**: every `mulligan_window` assertion / `with_mulligan_window_open()`
  call across `engine/mod.rs`, `dispatch.rs`
  (`mulligan_completion_kicks_off_investigation_phase`), the three
  `crates/scenarios/tests/` files (`synthetic_resolution`, `upkeep_phase`,
  `mythos_phase`), and `game-core/tests/reaction_windows.rs` — set
  `turn_order` and `with_mulligan_pending(lead)`; assert the cursor
  advances / reaches `None`.
- `mulligan_by_defeated_investigator_is_rejected`: now rejects via cursor
  mismatch (the cursor never points at a defeated investigator) — kept,
  rejection reason updated.

## Explicit non-goals (YAGNI)

- **No `AwaitingInput` path** — mulligan stays a direct `PlayerAction`, not a
  sub-choice; the existing "mulligan never returns AwaitingInput" assumption
  in `apply_player_action` holds.
- **No special immediate-kickoff for an empty-`turn_order` setup** —
  degenerate, matches today's behavior.
- **No interactive choice** — player order is fixed by `turn_order`, so there
  is nothing to prompt. This is exactly why `#147` is single-player-complete,
  unlike the Phase-8 multiplayer prey-tie choice (`#151`).

## Success criteria (verifiable)

- Full CI gauntlet green: `fmt`, `clippy --all-targets --all-features -D
  warnings`, `test` under `RUSTFLAGS="-D warnings"`, `doc` under
  `RUSTDOCFLAGS="-D warnings"`, and the `wasm32` build.
- New out-of-order rejection test plus updated order-enforcement tests pass.
- No `mulligan_window` or `mulligan_used` references remain anywhere in the
  workspace.
