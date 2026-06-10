# Phase 7 Slice 1 B2 — Roster/seating + `StartScenario` investigator selection

**Issue:** [#221](https://github.com/talelburg/eldritch/issues/221). Follow-up cleanup: [#224](https://github.com/talelburg/eldritch/issues/224).
**Parent spec:** [`2026-06-10-phase-7-slice-1-gathering-design.md`](2026-06-10-phase-7-slice-1-gathering-design.md) (§"Investigator seating", build-order step 6).

## Goal

Let a host pick which investigators play a scenario. The scenario module builds the **world** (locations, decks, bag) with no investigators; a **roster** carried on `StartScenario` seats the chosen investigators — resolving each one's stats from card data and taking their deck as a player-supplied input — before the existing shuffle/deal step runs.

## Why this shape

`ScenarioModule.setup: fn() -> GameState` takes no arguments, so there is no channel for "which investigator." Rather than overload `setup` with a roster parameter (couples deck-loading to every scenario), the selection rides on the `StartScenario` action and seating happens in `start_scenario`, which already owns the per-investigator shuffle/deal loop.

Two facts drive the design:

1. **Investigator stats are static card data that already exist in the corpus.** The pipeline emits an investigator's base skills into `CardMetadata.skill_icons` and max health/sanity into `CardMetadata.health` / `.sanity`. E.g. Roland (`01001`): `skill_icons { willpower: 3, intellect: 3, combat: 4, agility: 2, wild: 0 }`, `health: Some(9)`, `sanity: Some(5)`. So seating needs **no new registry** — it reads stats through the existing [`CardRegistry`]. (A second bridge for data that is already card data would be redundant; key→data lookup via `metadata_for` is the idiomatic, established pattern.)
2. **A deck is *not* intrinsic to an investigator** — the player builds it (and Phase 9 will import decklists from an external source). So the deck must be a separate, free input, not something resolved from the investigator code.

## Architecture

### Component 1 — investigator-skills helper (game-core seating)

For player cards, `skill_icons` means "icons committed to a test"; for **investigator** cards the same fields are *base skills*. A type-guarded conversion makes that read explicit and prevents misuse on non-investigator cards.

**Layering:** `Skills` is a `game-core` type (`game_core::state`) and `CardMetadata` lives in `card-dsl`, which must **not** depend on `game-core` (the dependency runs the other way). So this is **not** a method on `CardMetadata` (that would be a circular dep) — it is a free helper in the `game-core` seating module, reading `card-dsl` types it already has in scope:

```rust
// in game-core seating code
fn investigator_skills(meta: &CardMetadata) -> Option<Skills> {
    if meta.card_type != CardType::Investigator { return None; }
    Some(Skills {
        willpower: i8::try_from(meta.skill_icons.willpower).ok()?,
        intellect: i8::try_from(meta.skill_icons.intellect).ok()?,
        combat:    i8::try_from(meta.skill_icons.combat).ok()?,
        agility:   i8::try_from(meta.skill_icons.agility).ok()?,
    })
}
```

`Skills` fields are `i8`; `SkillIcons` fields are `u8`. Conversion is via `i8::try_from` (base skills are single-digit, so this never fails in practice — `None`/reject on the impossible overflow rather than panic). `CardType` is re-exported through `game_core::card_data`, so the `card_type` guard is in scope without a new dependency.

### Component 2 — protocol: `StartScenario { roster }`

```rust
pub enum PlayerAction {
    StartScenario { roster: Vec<RosterEntry> },
    // …
}

/// One seat in the scenario: which investigator, and the deck the player
/// chose for them. The deck is a free input (Phase 9 will populate it
/// from an external decklist import); seating takes it verbatim.
pub struct RosterEntry {
    pub investigator: CardCode,
    pub deck: Vec<CardCode>,
}
```

Was a unit variant. Every existing `StartScenario` construction site gains `roster: vec![]`. `RosterEntry` is `Serialize`/`Deserialize` (it crosses the wire and lands in the action log). The decklist is logged (deck composition is load-bearing for deterministic replay and is player-authored); stats are **not** logged — they're resolved from the code, so a client cannot inject inflated stats. That authority split is deliberate.

### Component 3 — seating in `start_scenario` (validate-first)

Before the existing shuffle/deal loop:

1. If `roster` is non-empty, for each entry resolve stats: `card_registry::current()` → `metadata_for(&entry.investigator)` → require `investigator_skills(meta)` to be `Some` (i.e. `card_type == Investigator`) and `health`/`sanity` both present. **Any failure → `Rejected`, state and events unchanged** (no registry installed, unknown code, non-investigator card, or missing health/sanity).
2. Seat an `Investigator` per entry: ids assigned sequentially in roster order, `turn_order` in that order, `current_location: None`, `skills`/`max_health`/`max_sanity` from card data, `deck` = `entry.deck`, `resources: 5` (Rules Reference setup), other counters zeroed; `actions_remaining` left to the existing `reset_actions(cx)` call.
3. **Invariant:** after seating, if `state.investigators` is empty → `Rejected { "a scenario requires at least one investigator" }`. In production `setup()` seats nobody, so this makes the roster mandatory: empty roster ⇒ reject.
4. Then the existing shuffle/deal + mulligan-cursor + `reset_actions` logic runs unchanged.

### Data flow

```
host picks investigators + decks
  → ClientMessage::Submit { StartScenario { roster } }
    → start_scenario: resolve stats (CardRegistry) + seat (deck from payload)
      → invariant: ≥1 investigator
        → existing shuffle/deal/mulligan
```

## Error handling

All seating failures are `EngineOutcome::Rejected` with unchanged state/events (validate-first, per the kernel handler contract): missing registry, unknown code, non-investigator code, missing health/sanity, and the zero-investigator invariant. No partial seating — validate the whole roster before mutating.

## Scope & deferrals

- **Map placement** is *not* done here: seated investigators get `current_location: None`. Placing them at the scenario's starting location (the Study, for The Gathering) is scenario content — **Group C**.
- **Roland's signature/weakness cards and starter decklist contents** are **Group C**. B2's test supplies a deck inline.
- **Server install of the real registries** so a browser game can seat is **B3 (#222) / Group D**; B2's engine path returns `Rejected` cleanly when no registry is installed.

## Temporary scaffolding → follow-up #224

The zero-investigator invariant (not "non-empty roster required") is chosen so the **pre-seated unit-test path survives**: `TestGame::new().with_investigator(…)` + `roster: vec![]` still works because the count is ≥1. ~15 engine tests and ~10 `crates/cards/tests/` / `crates/scenarios/tests/` files seat **synthetic** investigators (`test_investigator(N)`) whose codes have no real `CardMetadata` and so cannot resolve stats from the registry — forcing them onto the roster now would mean a fake-metadata install per test for no domain gain.

This tolerance is temporary scaffolding tied to the synthetic cards. It carries a `TODO(#224)`. When real cards become the test default (synthetic-card removal), **#224** migrates every `StartScenario` test to roster seating and tightens `start_scenario` to require a non-empty roster (dropping the tolerance), leaving a single seating path.

## Testing

- **Integration** (`crates/cards/tests/`, installs `cards::REGISTRY`): a roster of `[{ investigator: "01001", deck: <inline codes> }]` seats Roland with W3/I3/C4/A2, max_health 9, max_sanity 5, and the payload deck; the deck is shuffled and the opening hand dealt by the existing flow.
- **Engine unit** (`game-core`): empty roster + pre-seated investigator passes (passthrough); empty roster + zero investigators rejects; unknown code rejects; a non-investigator code (e.g. an asset like `01030`) rejects; all reject paths leave state/events unchanged.

## Decisions made

- **No new registry** — investigator stats resolve from `CardMetadata` via the existing `CardRegistry`; the deck is a separate payload input. (Eliminates the `InvestigatorRegistry` bridge considered earlier.)
- **`StartScenario` carries the roster** (not `CreateGameRequest`); deck is player-supplied for Phase-9-import forward-compatibility.
- **Invariant is ≥1-investigator-after-seating**, not non-empty-roster, to preserve the pre-seated test affordance until #224.
