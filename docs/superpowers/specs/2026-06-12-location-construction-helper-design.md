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

New methods (all **registry-free** — they take the card metadata as a parameter;
see below):

- `add_location(&mut self, metadata: &CardMetadata) -> LocationId` — mint the
  next id; build a `Location` from `metadata` (`code`, `name`, and
  `shroud`/`clues` out of `CardKind::Location`); insert into `locations`; return
  the id.
- `add_set_aside_location(&mut self, metadata: &CardMetadata) -> LocationId` —
  identical, but insert into the `set_aside_locations` zone instead.
- `connect(&mut self, a: LocationId, b: LocationId)` — add a **bidirectional**
  connection (push `b` onto `a`'s `connections` and `a` onto `b`'s). It resolves
  each id across **both** `locations` and `set_aside_locations` (The Gathering's
  connections are wired among set-aside locations before they enter play), and
  `expect`s each id to resolve (a build-time invariant — `connect` is called with
  freshly-minted ids).

A non-`Location` `metadata.kind` is a build-time invariant violation (a scenario
hands its own location cards), so the extraction `panic!`s with a clear message —
matching what `location_stats` does today on a kind mismatch. The methods return
`LocationId` (not `Result`) to keep `setup()` ergonomic.

### The corpus read stays in the scenario layer (metadata passed in)

`game-core` cannot call `cards::by_code` (that crate is a layer above it), and we
deliberately **don't** make these methods read the process-global
`card_registry` either — doing so makes them un-unit-testable inside game-core
(its lib test binary already pins a `TEST1`-only global fake; `install` is
first-wins). Instead the **scenario** does the corpus lookup (it already depends
on `cards`) and passes the `&CardMetadata` in. game-core stays registry-free for
construction and the methods are trivially unit-testable with a constructed
`CardMetadata` (which is deliberately not `#[non_exhaustive]`, expressly for
mocks).

The Gathering's `setup()` becomes (a small local closure keeps the corpus lookup
terse):

```rust
let meta = |code| cards::by_code(code).expect("Gathering location in corpus");
let study   = state.add_location(meta("01111"));
let hallway  = state.add_set_aside_location(meta("01112"));
let attic   = state.add_set_aside_location(meta("01113"));
let cellar  = state.add_set_aside_location(meta("01114"));
let parlor  = state.add_set_aside_location(meta("01115"));
state.connect(hallway, attic);
state.connect(hallway, cellar);
state.connect(hallway, parlor);
state.starting_location = Some(study);
```

`location_stats` and the `make` closure are deleted (their work moves into
`add_location`). `setup()`'s unit tests keep using the direct `cards::by_code`
path — **no registry install needed** (only the `STUDY_ID` migration below).

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

Because the methods are registry-free (metadata passed in), everything is
directly testable in its own layer with no fakes-fighting-the-global.

- **Engine unit (`game_state.rs` / `builder.rs`):** `next_location_id` starts at
  0 and round-trips through serde. `add_location` mints a sequential id, inserts
  into `locations`, and extracts `code`/`name`/`shroud`/`clues` — tested with a
  constructed `CardMetadata { kind: CardKind::Location { shroud, clues, victory } }`
  (no registry; `CardMetadata` is non-`#[non_exhaustive]` for exactly this). A
  second metadata with a non-`Location` kind asserts the panic. `add_set_aside_location`
  inserts into the set-aside zone. `connect` makes both locations reference each
  other and works across the in-play / set-aside split (insert two `Location::new`
  fixtures, call `connect`, assert both `connections`).
- **Scenario unit (`the_gathering.rs` `#[cfg(test)]`):** the existing setup tests,
  updated only for the `STUDY_ID` → `starting_location` migration (no registry
  install needed — `cards::by_code` is direct), still pin the board built by the
  real metadata: Study in play with code `01111` + shroud/clues `(2,2)`; four
  set-aside locations in order with distinct ids; Hallway ↔ Attic/Cellar/Parlor;
  Study isolated.
- **Integration (`tests/the_gathering.rs`):** unchanged behavior — the seating
  and act-1 board-rebuild tests still pass via `starting_location`.

## Open questions

None. The design fork over how the corpus stats reach `add_location` is settled:
the **scenario passes `&CardMetadata` in** (it already has `cards::by_code`),
keeping the id-allocation + construction on `GameState` while leaving game-core
registry-free and directly unit-testable. This is simpler than reading the
process-global `card_registry` from a `GameState` method (which the game-core
lib test binary can't reliably supply, given its first-wins `TEST1` global fake)
and avoids putting the corpus read in a separate `scenarios`-layer helper.
