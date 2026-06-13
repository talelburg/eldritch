# Phase 7 Slice 1 — C3b: The Gathering enemies (design)

**Issue:** [#231](https://github.com/talelburg/eldritch/issues/231) · **Date:** 2026-06-13
**Depends on:** C3a (#230 — `Prey`/`Retaliate` machinery), C1a (#227 — locations for spawn).

## Goal

Make the six Gathering encounter enemies spawn with their correct printed
stats and keywords, by reading them from the corpus instead of the
hardcoded placeholders in `spawn_enemy`. The combat stats already live in
the corpus (`CardKind::Enemy`); this PR adds the missing keyword/scaling
data (Hunter, Retaliate, Prey, per-investigator health, spawn-location),
parses it in the pipeline, and wires `spawn_enemy` to use all of it.

## The six enemies (stats verified against `data/arkhamdb-snapshot/pack/core/core_encounter.json`)

| Enemy | code | fight | evade | dmg | horror | health | victory | qty | keywords |
|---|---|---|---|---|---|---|---|---|---|
| Ghoul Priest | 01116 | 4 | 4 | 2 | 2 | 5 *(per-investigator)* | 2 | 1 | Prey-Highest [combat], Hunter, Retaliate, Elite |
| Flesh-Eater | 01118 | 4 | 1 | 1 | 2 | 4 | 1 | 1 | Spawn-Attic |
| Icy Ghoul | 01119 | 3 | 4 | 2 | 1 | 4 | 1 | 1 | Spawn-Cellar |
| Ghoul Minion | 01160 | 2 | 2 | 1 | 1 | 2 | — | 3 | (none) |
| Ravenous Ghoul | 01161 | 3 | 3 | 1 | 1 | 3 | — | 1 | Prey-Lowest remaining health |
| Swarm of Rats | 01159 | 1 | 3 | 1 | 0 | 1 | — | 3 | Hunter |

Attic = `01113`, Cellar = `01114` (from `crates/cards/src/impls/{attic,cellar}.rs`).

## Current state

- **Combat stats already in the corpus** (`CardKind::Enemy { fight, evade,
  damage, horror, health, victory, spawn, surge, peril, quantity }`), but
  `spawn_enemy` (`crates/game-core/src/engine/dispatch/encounter.rs:309`)
  **ignores them** and hardcodes `fight: 1, evade: 1, attack_damage: 0,
  attack_horror: 0`, `max_health = health.unwrap_or(1)`, `prey:
  Prey::Default`, `hunter: false`, `retaliate: false`.
- **C3a's keyword machinery exists** (`Prey::Ranked`, `resolve_prey`,
  hunter movement, retaliate in skill-test) but is only exercised by tests
  that mutate the `Enemy` struct directly — there is no production path
  that sets `hunter`/`retaliate`/`prey` on a spawned enemy.
- **Keywords + spawn-location are not in the corpus** — they live only in
  card `text`; the pipeline emits `spawn: None, surge: false, peril:
  false` as documented "not-yet-parsed defaults" (`main.rs:390`).

## Design

### 1. `card-dsl` — extend `CardKind::Enemy`

Three genuinely new fields (everything else already exists):

- `hunter: bool`
- `retaliate: bool`
- `prey: Prey` (the enum already lives in `card-dsl`)

And change `health: Option<u8>` → `health: Option<HealthValue>`, mirroring
the location `ClueValue` pattern:

```rust
/// An enemy's printed health. Mirrors `ClueValue`: `PerInvestigator(n)`
/// scales by the number of investigators in the game; `Fixed(n)` is a flat
/// value. Distinguishes ArkhamDB's `health_per_investigator` (absent/false
/// → fixed; `true` → per-investigator). Note the polarity is the opposite
/// of `ClueValue` (clues default to per-investigator).
pub enum HealthValue {
    Fixed(u8),
    PerInvestigator(u8),
}
```

`health` stays `Option<_>` to preserve the existing "enemy may have no
health" semantics (unlike location clues, which are always present).

Update the hand-written construction sites: the `01112` literal at
`card_data.rs:683`, synth fixtures, and mocks.

### 2. `card-data-pipeline` — parse keywords + spawn + per-investigator health

Parse from enemy `text`:

- `Hunter.` → `hunter: true`; `Retaliate.` → `retaliate: true` (substring).
- `Prey - Highest [combat].` → `Prey::Ranked { direction: Highest,
  measure: Skill(Combat) }`; `Prey - Lowest remaining health.` →
  `{ Lowest, RemainingHealth }`. (The four skill brackets map to
  `SkillKind`; `remaining health` → `RemainingHealth`.)
- `Spawn - <LocationName>.` → resolve name → code over the snapshot's
  Location cards → `SpawnLocation::Specific(code)` (Attic→01113,
  Cellar→01114).

Read JSON `health_per_investigator: Option<bool>` (normalized to `bool`,
default false), and emit health via a `health_value_lit(health,
health_per_investigator)` helper mirroring the existing
`clue_value_lit(clues, clues_fixed)`:
`PerInvestigator(n)` when `health_per_investigator`, else `Fixed(n)`.

Extend the Enemy arm of `render_kind` to emit the new fields, then
regenerate the corpus (`cargo run -p card-data-pipeline`).

**Totality over the whole pack.** The corpus ingests the *entire* core
encounter set, which includes out-of-scope keyword forms the model can't
yet represent — Masked Hunter's `Prey - Most clues` (no
`PreyMeasure::Clues`) and `Spawn - Engaged with Prey` (no such
`SpawnLocation`). Those fall back to the existing documented defaults
(`Prey::Default`, `spawn: None`) **plus an `eprintln` warning** naming each
enemy + unparsed line, so it's a loud stub rather than a silent
approximation — matching how `surge`/`peril` already default. The six
in-scope enemies use only modeled forms.

### 3. `game-core` — `spawn_enemy` reads the corpus

In `crates/game-core/src/engine/dispatch/encounter.rs`:

- Read `fight`/`evade`/`damage`/`horror` from `CardKind::Enemy` (replace
  the hardcoded `1`/`1`/`0`/`0`).
- Resolve `max_health` from `HealthValue`: `Fixed(n) => n`,
  `PerInvestigator(n) => n * count`, where `count =
  state.investigators.len()` — the **same source** the per-investigator
  clue path uses (`reveal.rs:20`), carrying the same future caveat about
  switching to a stored started-count.
- Set `hunter`/`retaliate`/`prey` from metadata.
- Use the enemy's actual `prey` (not the hardcoded `Prey::Default`) for
  the spawn-engagement narrowing (`resolve_prey` call).

### 4. Tests (TDD)

- **Pipeline unit tests** — each parse function on representative text,
  including the unparsed-form fallback (assert default + warning path).
  Mirror the existing `clue_value_lit` tests for `health_value_lit`.
- **`game-core` unit tests** — extend `spawn_enemy_tests` with mock
  metadata carrying keywords + `HealthValue::PerInvestigator` → assert the
  minted `Enemy` fields (incl. health scaling by investigator count).
- **Integration test** (`crates/cards/tests/enemies.rs`) — install
  `cards::REGISTRY`, spawn each of the six real enemies through the engine,
  and assert stats / keywords / spawn-location against the corpus. Under
  this approach there is no per-enemy `impls/` module, so the issue's
  "card test per enemy" acceptance is realized here.

## Out of scope / noted

- `surge`/`peril` stay unparsed (`false`) — tracked by #138; none of the
  six in-scope enemies carry them.
- Agenda-driven Ghoul movement and the Act-3 advance-on-Priest-defeat are
  **C3c (#232)** / already-shipped **C1b (#259)**, not this PR.
- Out-of-scope enemies' un-modeled Prey/Spawn forms (Masked Hunter, etc.)
  default + warn; modeling them is future work as those cards land.

## Success criteria

- `RUSTFLAGS="-D warnings" cargo test --all --all-features` green, plus the
  full CI gauntlet (fmt, clippy, doc, wasm-build, wasm-test, wasm-clippy).
- Regenerated corpus carries the parsed keyword/health/spawn fields for the
  six enemies (and sane defaults + warnings for un-modeled forms).
- The integration test demonstrates each enemy spawning with its verified
  stats, keywords, and (for Flesh-Eater/Icy Ghoul) spawn-location;
  Ghoul Priest's health scales per investigator.
