# CardMetadata → struct + CardKind enum: design spec

**Date:** 2026-06-11
**Status:** Design approved; ready for plan.
**Issue:** #254 (`[engine]`)
**Followed by:** #252 (encounter-card ingestion, builds on this model).

## Goal

Replace the flat `CardMetadata` struct (a pile of `Option` fields, most `None`
for any given card) with a small **common identity core** plus a **`CardKind`
enum** holding the type-specific data. Pure structural remodel of the data the
pipeline already parses — no new fields, no encounter ingestion (that's #252).

## Why now

#252 is about to add ~8 type-specific encounter stats (shroud, clues, victory,
doom, enemy fight/evade/damage/horror). Added to the flat struct they'd be more
mostly-`None` `Option`s — exactly the smell an enum removes. Model by type first,
then #252 fills in encounter variants on a clean base. Same "land the structural
refactor separately" pattern as `Controller→You` (#248) and `GameStateBuilder`
(#251).

## The model

```rust
pub struct CardMetadata {
    pub code: String,            // identity: registry binary-search + sort key
    pub name: String,
    pub traits: Vec<String>,     // "Ghoul", "Item", … (empty when none)
    pub text: Option<String>,
    pub pack_code: String,
    pub kind: CardKind,
}

pub enum CardKind {
    Asset {
        class: Class, cost: Option<i8>, xp: Option<u8>, slots: Vec<Slot>,
        health: Option<u8>, sanity: Option<u8>, skill_icons: SkillIcons,
        is_fast: bool, deck_limit: u8,
    },
    Event { class: Class, cost: Option<i8>, xp: Option<u8>, skill_icons: SkillIcons, is_fast: bool, deck_limit: u8 },
    Skill { class: Class, xp: Option<u8>, skill_icons: SkillIcons, deck_limit: u8 },
    Investigator { class: Class, skills: Skills, health: u8, sanity: u8 },
    Enemy { health: Option<u8>, surge: bool, peril: bool, quantity: u8 },
    Treachery { surge: bool, peril: bool, quantity: u8 },
}
```

The current corpus is exactly these six types (Asset 105 / Event 59 / Skill 22 /
Investigator 10 / Treachery 17 / Enemy 3). `Location`/`Act`/`Agenda` variants and
the `Enemy` combat stats (`fight`/`evade`/`damage`/`horror`) land in #252.

### Field placement decisions (settled)

- **Common core = identity + universal cosmetics:** `code`, `name`, `traits`,
  `text`, `pack_code`. `code`/`name` stay flat because the registry does
  `binary_search_by(code)` and sorts by code — matching there is friction with no
  domain meaning. `traits` (Vec) / `text` (Option) model "absent" honestly with
  no sentinel.
- **Drop `position`, `flavor`, `illustrator`** — carried but unread (verified: no
  production consumers). Trivially re-addable from the snapshot. Pipeline stops
  parsing/emitting them.
- **`class` → the four player variants.** It's player-only; encounter cards have
  no class (`Class::Mythos` was a sentinel-for-absence). Kept (not dropped) so a
  future deck-validator has it.
- **`quantity` → `Enemy`/`Treachery`.** It's the encounter-deck build multiplicity
  (instantiate N copies); uninteresting on other types. (Locations/acts/agendas
  aren't shuffled.)
- **`surge`/`peril` → `Enemy`/`Treachery`.** Encounter-card keywords; meaningless
  on player cards (currently defaulted `false` for every card).
- **`health`/`sanity`:** `Asset` (`Option`, ally soak), `Investigator` (required
  `u8`), `Enemy` (`Option`, behavior-preserving — `encounter.rs` keeps its
  `health.unwrap_or(1)`).
- **`skill_icons` (player cards) vs `skills` (investigators) — distinct types.**
  Asset/Event/Skill carry `skill_icons: SkillIcons` (commit icons, incl. wild);
  `Investigator` carries `skills: Skills` (base willpower/intellect/combat/
  agility). These are genuinely different concepts and get different types. To
  make this possible across the layering, **move `Skills` + `SkillKind` (and the
  `Skills::value` method) down from `game-core` into `card-dsl`** — they're pure
  data, which is `card-dsl`'s charter, and `card-dsl` already owns the skill
  vocabulary (`Stat`, `SkillTestKind`). `game-core::state` re-exports them
  (`pub use card_dsl::…::{Skills, SkillKind}`) so every existing
  `game_core::state::{Skills, SkillKind}` reference keeps working unchanged. The
  pipeline maps the parsed `skill_*` (u8) into `Skills` (i8) for investigators
  and `SkillIcons` for player cards; the seating reader then reads
  `CardKind::Investigator.skills` directly (dropping today's
  `SkillIcons → Skills` `try_from` dance).

### `CardType` stays as a discriminant

`CardType` (the existing flat enum) is kept and exposed via a
`CardMetadata::card_type(&self) -> CardType` accessor derived by matching on
`kind`, so existing type-gate code (`card_type == CardType::Asset`) keeps working
without reaching into payloads. `kind` carries the data; `card_type()` is the
cheap tag.

## Components & blast radius

- **`crates/card-dsl`** — receive `Skills` + `SkillKind` (+ `Skills::value`)
  moved down from `game-core` (pure-data types; new module or into
  `card_data.rs`). Then define `CardKind`, reshape `CardMetadata`, add
  `card_type()` (and small accessors the readers want, e.g. `class()`), drop the
  three cosmetic fields, update the type's own unit tests/serde.
- **`crates/game-core/src/state`** — remove the `Skills`/`SkillKind` definitions
  from `investigator.rs`; re-export them from `card-dsl`
  (`pub use … {Skills, SkillKind}`) at the historical `game_core::state` path so
  all references compile unchanged.
- **`crates/card-data-pipeline/src/main.rs`** — `render()` emits
  `kind: CardKind::X { … }` per `type_code`; stop emitting position/flavor/
  illustrator; map skills into `Skills` vs `SkillIcons` by type. `RawCard` loses
  the three dropped fields. Regenerate `crates/cards/src/generated/cards.rs`
  (`cargo run -p card-data-pipeline`) — never hand-edit it.
- **The 3 stat-reader sites** (gain type safety via `match`/`if let` on `kind`):
  - `game-core/src/engine/dispatch/phases.rs` — seating reads
    `CardKind::Investigator { skills, health, sanity }` and uses `skills`
    directly (the `investigator_skills`/`try_from` mapping goes away).
  - `game-core/src/engine/dispatch/encounter.rs` — spawn reads
    `CardKind::Enemy { health, .. }` (keep `unwrap_or(1)`).
  - `game-core/src/engine/dispatch/cards.rs` — play reads `is_fast` from
    `CardKind::Asset|Event`.
- **~6 mock-literal sites** (`card_registry.rs`, `synth_cards.rs`, and the
  `game-core/tests/*.rs` that build mock `CardMetadata`) — migrate to the new
  shape.

## Data flow (unchanged)

Pipeline reads snapshot → emits `CardMetadata { …, kind: CardKind::… }` literals
→ `cards::all()` / `by_code` / registry serve them → engine readers match on
`kind`. No runtime behavior changes; only the data's shape and the three readers'
access pattern.

## Testing

- `card_data.rs` unit tests: `card_type()` returns the right tag per variant; a
  serde round-trip per representative variant; `class()` returns the player
  class / is absent for encounter kinds.
- `cards` crate: existing `by_code`/`is_playable`/corpus-sorted tests carry over
  (they touch common fields + `is_playable`); add a spot check that a known
  investigator (`01001`) is `CardKind::Investigator` with the right skills, and a
  known asset (`01030`) is `CardKind::Asset`.
- The 3 reader sites keep their existing engine tests (seating stats, spawn
  health, fast-play) — they must pass unchanged (behavior-preserving).
- Full strict gauntlet green; corpus regenerated (not hand-edited).

## Out of scope (→ #252)

- Ingesting `*_encounter.json`; the `Location`/`Act`/`Agenda` variants; the
  `Enemy` combat stats (`fight`/`evade`/`damage`/`horror`); `encounter_code` set
  membership; migrating `the_gathering::setup()` to read stats from the registry;
  the deckbuilding-pollution filter.

## Decisions captured for later PRs

- **Common core is identity-only** (`code`/`name`/`traits`/`text`/`pack_code`);
  everything type-varying lives in `CardKind`. Matching at consumers is the
  intended shape, not a cost to avoid.
- **`card_type()` is derived from `kind`** — don't store a separate discriminant
  field that can drift.
- **`Skills` + `SkillKind` live in `card-dsl`** (moved down from `game-core`,
  re-exported at `game_core::state` for compatibility). Investigators carry
  `skills: Skills`; player commit-cards carry `skill_icons: SkillIcons`. The two
  are no longer conflated.
