# Place defeated victory-point enemies into the victory display (design)

**Issue:** [#273](https://github.com/talelburg/eldritch/issues/273) · **Date:** 2026-06-13

## Goal

When an enemy with a printed Victory value is defeated, place it into the
victory display (Rules Reference p.21). C2 (PR #263) implemented the
**location** side of the victory display at scenario resolution; C3b
(PR #272) put each enemy's printed `victory` into the corpus. This issue
adds the **enemy** side: capture victory at defeat time.

## Why not the location approach

The location path scans `state.locations` at scenario resolution and reads
`victory` from the registry, because locations stay in play and their
state doesn't carry `victory`. A defeated enemy is **removed from
`state.enemies` immediately** (`combat.rs:33`), so it cannot be scanned at
resolution — its victory must be captured at the moment of defeat. And the
`Enemy` struct already mirrors every printed stat copied from
`CardKind::Enemy` at spawn (fight/evade/health/attack/keywords), so
`victory` belongs there too. This keeps the defeat path free of the global
`card_registry` and therefore unit-testable inside `game-core` (the
location *positive* test had to move to an integration test precisely
because of that coupling).

## Design

Approach: **carry `victory` on the `Enemy` struct, set at spawn; read it in
the defeat handler.**

### 1. `Enemy` gains a `victory` field

`crates/game-core/src/state/enemy.rs` — add `pub victory: Option<u8>`
(printed Victory value; `None` when the enemy awards no victory points).

### 2. `spawn_enemy` sets it from the corpus

`crates/game-core/src/engine/dispatch/encounter.rs` — `spawn_enemy` already
destructures `CardKind::Enemy { .. }`; bind `victory` and set it on the
minted `Enemy`, alongside the existing stat reads.

### 3. `damage_enemy` places it on defeat

`crates/game-core/src/engine/dispatch/combat.rs` — in the defeat branch
(`new_damage >= max_health`), after the `EnemyDefeated` event and the
`state.enemies.remove`, and before the `AfterEnemyDefeated` reaction window:
if the defeated enemy's `victory` is `Some(v)` with `v > 0`, push its code
to `state.victory_display` and emit the existing
`Event::EnteredVictoryDisplay { code, victory: v }`. Capture the value
before the enemy is removed (the code is already captured as
`defeated_code`).

This mirrors the location placement (`engine/mod.rs:210-214`) exactly:
same zone, same event.

### 4. Update `Enemy` construction sites

The `test_enemy` fixture (`test_support/fixtures.rs`) defaults `victory:
None`; the one other test literal (`state/enemy.rs`) gets the same. Tests
wanting a victory enemy set `e.victory = Some(v)` after building, mirroring
how `hunter`/`retaliate` are set in existing tests.

## Testing

game-core unit tests in `combat.rs` (no registry needed):

- Defeat an enemy with `victory: Some(2)` → its code is in
  `state.victory_display` and an `EnteredVictoryDisplay { victory: 2 }`
  event is emitted.
- Defeat an enemy with `victory: None` → not placed, no
  `EnteredVictoryDisplay` event.

A `spawn_enemy` unit test asserts the spawned `Enemy.victory` reflects the
metadata's `CardKind::Enemy { victory }`.

## Out of scope

- **XP summing** of the victory display (Phase 9).
- **Non-damage defeat paths** — none exist; `damage_enemy` is the sole
  enemy defeat/removal path today. A future non-damage "defeat" effect
  would route through the same placement.

## Success criteria

- Full CI gauntlet green.
- Defeating a victory-bearing enemy places it in the victory display with
  the correct value and event; non-victory enemies do not.
