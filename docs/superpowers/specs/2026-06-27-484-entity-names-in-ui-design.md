# #484 — Display card/location names in the web UI

**Issue:** [#484](https://github.com/talelburg/eldritch/issues/484) — QoL:
display card/location names in the web UI instead of raw codes/ids. Labels:
`ui`, `p2-later`.

## Problem

The web client renders several entities as raw codes / ids: hand cards and
cards-in-play show their `CardCode` (e.g. `01030`), and an investigator's /
enemy's location shows `format!("loc {id}")`. Hard to read during play. The
locations panel, enemies panel, and investigator panel already render names
(those carry a `name` in `GameState`).

## Scope

UI display only. Engine prompt *strings* that embed codes (the commit-window
prompt, the #482 advance acknowledge, etc.) are out of scope — this is purely
about how the client renders entities on the board and its controls.

**Display format: name only** (e.g. "Magnifying Glass", "Study"), falling back
to the raw code / `loc {id}` only when the name is unavailable.

## The name source already exists client-side

`crates/web/src/main.rs` already installs the real card registry
(`game_core::card_registry::install(cards::REGISTRY)`), and `web` depends on
`cards` (which builds for wasm). So a code→name lookup needs no new data
plumbing. `CardMetadata.name: String` carries the printed name. Location names
are already in `GameState::locations`.

## Design

### 1. `crate::names` module (new)

A small, unconditional web module (both `board.rs` and the wasm-only `input.rs`
consume it), with two pure helpers:

```rust
/// Printed card name for `code`, or the raw code when unknown (unimplemented-
/// stub cards) or the registry is not installed (headless/native render tests).
pub fn card_name(code: &CardCode) -> String {
    game_core::card_registry::current()
        .and_then(|r| (r.metadata_for)(code))
        .map(|m| m.name.clone())
        .unwrap_or_else(|| code.to_string())
}

/// Display name for a location id, or "loc {id}" when it is not in state.
pub fn location_name(game: &GameState, id: LocationId) -> String {
    game.locations
        .get(&id)
        .map(|l| l.name.clone())
        .unwrap_or_else(|| format!("loc {}", id.0))
}
```

### 2. Swap the render sites

- `board.rs` — hand cards (`code.to_string()` → `card_name(code)`), cards in
  play (`c.code.to_string()` → `card_name(&c.code)`), investigator location
  (`format!("loc {id}")` → `location_name(game, id)` via the `Option`
  map), enemy location (same). Remove the stale "the client has no card-name
  source" doc comment.
- `input.rs` — the `PickMultiple` commit-hand buttons label each hand card via
  `card_name` instead of the raw code.
- Leave the locations/enemies/investigator panels (already names) untouched.

### 3. Testing

- **Native unit tests** (`names.rs`): `card_name` returns the printed name with
  the registry installed (a `#[ctor]` installs `cards::REGISTRY`) and falls back
  to the code for an unknown code; `location_name` returns the state name and
  falls back to `loc {id}` for an absent id.
- **wasm render test**: feed a state with a known card (`01030`) in hand (real
  registry installed) and assert the board renders "Magnifying Glass", not the
  code.

## Out of scope

- Engine prompt strings (unchanged).
- Showing the code alongside the name (name-only chosen).
- No engine/server changes.
