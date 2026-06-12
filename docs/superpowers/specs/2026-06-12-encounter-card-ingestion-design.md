# Encounter-card ingestion: design spec

**Date:** 2026-06-12
**Status:** Design approved; ready for plan.
**Issue:** #252 (`[infra]`)
**Builds on:** #254 (the `CardKind` model).

## Goal

Ingest the in-scope encounter cards (locations, acts, agendas, enemies,
treacheries, story assets) into the generated corpus with their printed stats,
by adding the `Location`/`Act`/`Agenda` `CardKind` variants and the `Enemy`
combat stats. Then de-hardcode `the_gathering::setup()` by reading those stats
from the corpus. This unblocks reading scenario-structure / enemy / treachery
stats from the registry instead of retyping them per scenario (C3 enemies #231,
C4 treacheries, future encounter-deck construction).

## Scope confirmed from the snapshot

8 in-scope encounter files (`core_encounter.json` + the 7 `dwl/*_encounter.json`;
the `core_2026*` reprints are out of scope per `data/arkhamdb-snapshot/SOURCE.md`).
284 cards: `location 110, treachery 53, enemy 44, agenda 34, act 33, asset 16,
scenario 11`. Factions are all `mythos`/`neutral` (both already mapped). Only
`scenario` lacks a `CardKind` variant.

## Components

### 1. New `CardKind` variants + extended `Enemy` (`crates/card-dsl/src/card_data.rs`)

```rust
// extend the existing Enemy variant:
Enemy {
    fight: u8, evade: u8, damage: u8, horror: u8,   // NEW combat stats
    health: Option<u8>, victory: Option<u8>,        // victory is NEW
    spawn: Option<Spawn>, surge: bool, peril: bool, quantity: u8,
},
// new variants:
Location { shroud: u8, clues: u8, victory: Option<u8> },
Act      { clue_threshold: Option<u8>, victory: Option<u8> },
Agenda   { doom_threshold: u8 },
```

- `Act.clue_threshold` is `Option` because some acts (e.g. The Gathering's
  `01110`) advance on a non-clue objective and carry `clues: null`.
- `Agenda` has no `victory` field (agendas don't carry victory points in scope).
- `card_type()` gains arms for `Location`/`Act`/`Agenda`; `class()` returns
  `None` for them (they're encounter cards). Enemy/Treachery already return
  `None`. The encounter-set `asset`-type cards use the existing `Asset` variant
  (faction `mythos`/`neutral` → `Class::Mythos`/`Neutral`).

### 2. Pipeline ingestion (`crates/card-data-pipeline/src/main.rs`)

- **`PACK_FILES`** gains the 8 encounter files. Update the module-doc note that
  currently says encounter companions are skipped.
- **Skip `scenario`-type cards.** `process_raw` (or a dedicated filter) drops
  any card whose `type_code` has no `CardKind` variant — currently just
  `scenario`. (The Gathering reference card `01104` is a `scenario` card; its
  symbol-token effects come from an `abilities()` impl in C2, not metadata, so
  it doesn't need a corpus entry.) Skip silently, like the skeleton-entry skip.
- **`RawCard`** gains `shroud`, `clues`, `victory`, `doom`, `enemy_fight`,
  `enemy_evade`, `enemy_damage`, `enemy_horror` (all `Option<u8>`).
  **`NormalizedCard`** carries them as `Option<u8>` (one `clues` field — the
  same JSON `clues` is a location's starting clues *and* an act's advance
  threshold; the consumer interprets per kind). Enemy combat stats default 0 at
  render via `unwrap_or(0)`.
- **`render_kind`** gains `Location`/`Act`/`Agenda` arms and extends `Enemy`
  (uses the existing `opt_u8` helper for `Option<u8>` fields):

```rust
"Location" => format!("CardKind::Location {{ shroud: {}, clues: {}, victory: {} }}",
                      c.shroud.unwrap_or(0), c.clues.unwrap_or(0), opt_u8(c.victory)),
"Act"      => format!("CardKind::Act {{ clue_threshold: {}, victory: {} }}",
                      opt_u8(c.clues), opt_u8(c.victory)),
"Agenda"   => format!("CardKind::Agenda {{ doom_threshold: {} }}", c.doom.unwrap_or(0)),
"Enemy"    => format!("CardKind::Enemy {{ fight: {}, evade: {}, damage: {}, horror: {}, \
                       health: {}, victory: {}, spawn: None, surge: false, peril: false, quantity: {} }}",
                      c.enemy_fight.unwrap_or(0), c.enemy_evade.unwrap_or(0),
                      c.enemy_damage.unwrap_or(0), c.enemy_horror.unwrap_or(0),
                      opt_u8(c.health), opt_u8(c.victory), c.quantity),
```

  The generated-file `use` line gains nothing new (`CardKind` already imported).
  Duplicate-code dedup is unchanged: if a code appears in both a player and an
  encounter file the pipeline errors loudly (handle at plan time only if it
  fires; player/encounter code ranges are expected disjoint).

### 3. `the_gathering::setup()` migration (`crates/scenarios/src/the_gathering.rs`)

Read printed stats from the **compile-time corpus** via `cards::by_code(code)`
(scenarios already depends on `cards`) — no runtime registry, no
`setup() -> GameState` "registry missing" problem. A small helper extracts the
stat from the looked-up `CardKind`, `.expect()`-ing presence (a build-time
invariant the corpus guarantees; a test catches regressions):

- Study (`01111`): `shroud`/`clues` from `CardKind::Location`.
- Acts `01108`/`01109`: `clue_threshold` from `CardKind::Act` (2 / 3).
- Agendas `01105`/`01106`/`01107`: `doom_threshold` from `CardKind::Agenda`
  (3 / 7 / 10).

**`01110` keeps its placeholder `clue_threshold`** — its metadata `clue_threshold`
is `None` (real "Ghoul Priest defeated" objective is C1b). So #252 de-hardcodes
everything except the one value C1b owns. The terminal Won/Lost resolutions and
the deck ordering stay scenario-level data in `setup()`.

## Out of scope (deferred, noted)

- **`encounter_code`** (set membership: "torch", …) — needed to build the
  encounter *deck from sets*, a C3/C4 feature. Not parsed here.
- **A deck-legality filter** — no deckbuilder consumes `cards::all()` today, and
  `is_playable` (has an `abilities()` impl) gates the current deck-import path.
  Latent edge: encounter cards with abilities (Attic/Cellar) pass `is_playable`;
  a real "player-card kinds only" filter lands when a deckbuilder does.
- Faithful act-objective types / victory-point scoring (C1b / C2).

## Testing

- **Pipeline unit tests:** a location/act/agenda/enemy `NormalizedCard` renders
  the matching `CardKind` arm (e.g. `render_kind` for an enemy contains
  `CardKind::Enemy { fight: …`); a `scenario`-type card is skipped by
  `process_raw`.
- **Corpus regen** (`cargo run -p card-data-pipeline`); the `cards` crate
  compiles against the new variants. Spot-check tests: `by_code("01111")` is a
  `CardKind::Location { shroud: 2, clues: 2, .. }`; `by_code("01116")` (Ghoul
  Priest) is a `CardKind::Enemy { fight: 4, .. }`.
- **`the_gathering`:** a test asserts `setup()` reads match the snapshot — Study
  shroud 2 / clues 2, agenda doom thresholds 3 / 7 / 10, act 01108/01109
  thresholds 2 / 3. The existing C1a integration tests still pass.
- Full strict gauntlet; corpus regenerated (never hand-edited).

## Decisions captured for later PRs

- **The corpus now carries scenario-structure + enemy stats** — C3 (#231) /
  C4 read enemy/treachery stats via `metadata_for`/`by_code` instead of
  hand-typing; new scenarios read location/act/agenda stats the same way.
- **`scenario`-type cards are not ingested** — they have no `CardKind` variant;
  their effects (e.g. `01104` symbol tokens) live in `abilities()` impls.
- **Scenario `setup()` reads card stats via `cards::by_code`** (compile-time),
  not the runtime registry — the pattern future scenarios follow.
