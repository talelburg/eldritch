# Location construction helper / id allocation

Design for [#260](https://github.com/talelburg/eldritch/issues/260): replace
the hand-assigned `LocationId` literals + manual connection wiring in scenario
`setup()` with a `GameState`-owned location-construction API, mirroring the
existing `next_enemy_id` / `next_card_instance_id` allocators.

## Problem

`crates/scenarios/src/the_gathering.rs::setup()` builds its board by
hand-picking `LocationId` literals and wiring connections by raw id:

```rust
pub const STUDY_ID: LocationId = LocationId(1);
const HALLWAY_ID: LocationId = LocationId(2);
const ATTIC_ID: LocationId = LocationId(3);
// … CELLAR_ID(4), PARLOR_ID(5)
let make = |id, code, name| { let (shroud, clues) = location_stats(code); Location::new(id, …) };
let mut hallway = make(HALLWAY_ID, "01112", "Hallway");
hallway.connections = vec![ATTIC_ID, CELLAR_ID, PARLOR_ID];
// … each spoke connects back to HALLWAY_ID; then state.set_aside_locations = vec![…]
```

This is verbatim and error-prone (ids must be unique and not collide), wires
connections by raw id, and is inconsistent with the rest of the engine —
enemies and card instances mint ids via auto-increment counters
(`GameState.next_enemy_id`, `next_card_instance_id`), but locations have no
equivalent; `GameStateBuilder::with_location` takes a caller-built `Location`
with a caller-chosen id. The pattern will be copy-pasted into every future
scenario.

## Design

### `GameState` gains location id allocation + construction methods

New field:

- `next_location_id: u32` — auto-increment counter (serde-roundtripped; builder
  initialises to 0), mirroring `next_enemy_id`. Ids are minted **sequentially
  and deterministically** in construction order, so callers never write
  `LocationId(N)` literals while replay/serialization determinism is preserved.

New methods:

- `add_location(&mut self, code: &str) -> LocationId` — mint the next id, read
  `name`/`shroud`/`clues` from the card metadata, insert into `locations`,
  return the id.
- `add_set_aside_location(&mut self, code: &str) -> LocationId` — identical, but
  insert into the `set_aside_locations` zone instead.
- `connect(&mut self, a: LocationId, b: LocationId)` — add a **bidirectional**
  connection (push `b` onto `a`'s `connections` and `a` onto `b`'s). It resolves
  each id across **both** `locations` and `set_aside_locations` (The Gathering's
  connections are wired among set-aside locations before they enter play).

The Gathering's `setup()` becomes:

```rust
let study   = state.add_location("01111");
let hallway = state.add_set_aside_location("01112");
let attic   = state.add_set_aside_location("01113");
let cellar  = state.add_set_aside_location("01114");
let parlor  = state.add_set_aside_location("01115");
state.connect(hallway, attic);
state.connect(hallway, cellar);
state.connect(hallway, parlor);
state.starting_location = Some(study);
```

`location_stats` is deleted (the metadata read moves into `add_location`).

### Corpus read goes through the card registry

`game-core` cannot call `cards::by_code` (that crate is a layer above it); it
reaches card metadata only through the installed **`card_registry`** — the same
path `PlayCard` and forced-trigger dispatch already use. So `add_location` /
`add_set_aside_location` read via `card_registry::current()` →
`(metadata_for)(code)` → `CardKind::Location { shroud, clues, .. }` + the
metadata's `name`.

A missing registry, a code absent from the corpus, or a non-`Location` card is
a **build-time invariant violation** (a scenario knows its own location codes),
so these panic with a clear message — matching what `location_stats` does today
(`.expect("location code in corpus")` + a kind mismatch `panic!`). The methods
return `LocationId` (not `Result`) to keep `setup()` ergonomic.

**Consequence:** `setup()`'s in-crate unit tests (`setup_reads_card_stats_from_corpus`,
`setup_places_study_in_play_and_four_set_aside`, etc.) currently rely on the
direct `cards::by_code` path and do **not** install the registry. They must now
install it (`let _ = game_core::card_registry::install(cards::REGISTRY);` — a
shared test helper or one line each). The integration tests in
`crates/scenarios/tests/the_gathering.rs` already install it. This is arguably
more honest: a scenario's `setup()` genuinely depends on card data being
available.

### `STUDY_ID` is removed

The `pub const STUDY_ID: LocationId = LocationId(1)` is dropped. With minted
ids, keeping it would just re-encode "the Study is added first" — the implicit
coupling this change removes. Its role (naming the starting location) is already
served by `GameState.starting_location`, which `setup()` sets from the minted
Study id. The ~8 references migrate:

- `the_gathering.rs` `setup()` + its unit tests: read the Study via
  `state.starting_location` (e.g. `s.locations[&s.starting_location.unwrap()]`).
  The `s.starting_location == Some(STUDY_ID)` assertion becomes "the starting
  location's code is `01111`" (asserting identity, not a magic id).
- `crates/scenarios/tests/the_gathering.rs`: the seating assertion and the
  board-rebuild test's `current_location = Some(STUDY_ID)` switch to
  `state.starting_location`.

## Scope

- **In:** the new `GameState` field + three methods; migrating
  `the_gathering::setup()` and its tests (incl. the registry install + `STUDY_ID`
  removal).
- **Out:** the synthetic fixture (`test_fixtures/synthetic.rs`) and the
  `test_support::test_location(id, name)` fixture keep explicit ids — unit-test
  fixtures pin ids deliberately, which is legitimate, not the pain point. No new
  scenarios exist to migrate (the rest of Group C is Gathering *content*, not new
  boards).

### Where the registry-reading methods are tested

`add_location` / `add_set_aside_location` read `card_registry::current()`. The
game-core lib test binary can't supply real location metadata (it can't depend
on `cards`, and its existing `card_registry` test already pins a `TEST1`-only
global fake — `install` is first-wins, so a competing global install is
unreliable). So these two methods are **not** unit-tested inside game-core;
they're covered where the real corpus is reachable — the `scenarios` crate,
which installs `cards::REGISTRY` and exercises them against the **actual**
Study/Hallway/Attic/Cellar/Parlor metadata. This mirrors how the engine's
`encounter.rs` spawn handlers (which also read `current()`) are integration-
tested rather than unit-tested with fakes. `connect` and the `next_location_id`
counter are registry-free, so they keep direct game-core unit tests.

- **Engine unit (`game_state.rs` / `builder.rs`):** `next_location_id` starts at
  0 and round-trips through serde; `connect` makes both locations reference each
  other and works across the in-play / set-aside split (build the two locations
  via direct insertion of `Location::new` fixtures — no registry needed).
- **Scenario unit (`the_gathering.rs` `#[cfg(test)]`):** install `cards::REGISTRY`,
  then the setup tests (updated for the `STUDY_ID` → `starting_location`
  migration) pin the board built by the real `add_location` calls: Study in play
  with code `01111` + shroud/clues `(2,2)`; four set-aside locations in order
  with distinct ids; Hallway ↔ Attic/Cellar/Parlor; Study isolated; the four
  minted ids are distinct from the Study's. This is the coverage for
  `add_location` / `add_set_aside_location` against actual locations.
- **Integration (`tests/the_gathering.rs`):** unchanged behavior — the seating
  and act-1 board-rebuild tests still pass via `starting_location`.

## Open questions

None. The registry-based `GameState` method (over a `scenarios`-layer helper
using `cards::by_code`) was chosen for the "on game state" intent + the engine's
sanctioned card-data path. The one concern it raised — unit-testing a method
that reads the process-global registry inside game-core — is resolved by testing
`add_location` / `add_set_aside_location` at the `scenarios` layer against the
**actual** location corpus (see Testing), rather than with game-core fakes; the
methods need no in-game-core unit test, exactly like the `encounter.rs` spawn
handlers that also read `current()`.
