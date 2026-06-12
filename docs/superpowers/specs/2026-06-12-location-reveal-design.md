# Location reveal-on-entry + per-investigator clue placement

Design for [#257](https://github.com/talelburg/eldritch/issues/257): model the
Arkham location-reveal mechanic — locations enter play **unrevealed**, are
**revealed** the first time an *investigator* enters them, and at reveal place
clues equal to the printed clue value (per-investigator or fixed). Filed as a
deferral from C1b (#228): the `revealed` field on `Location` is currently
**dormant** (nothing reads it; `move_action` never reveals; clues are placed
flat at construction).

## Rules grounding

Verified against the Rules Reference (`data/rules-reference/…`).

**Reveal + clue placement** (p.14):
> Locations enter play in an "unrevealed" state… The first time a location is
> entered by an investigator, that location is revealed by turning it to its
> other side **and placing a number of clues on it equal to its clue value**
> (this may occur during setup). Most clue values are conveyed as a "per
> investigator" value. If an **enemy** moves to an unrevealed location, that
> location **remains unrevealed**.

**Per-investigator multiplier** (p.16, *Per Investigator*):
> That value is multiplied by **the number of investigators who started the
> scenario**. … If investigators have been eliminated from the scenario, they
> **still count** toward "per investigator" values.

So a 2-per-investigator location with 3 starting investigators places **6**
clues, even after one is eliminated. In this engine that count is
`state.investigators.len()`: elimination flips `Investigator.status`
(`Killed`/`Insane`/`Resigned`) and **never removes the entry** (the only
`investigators.remove` calls are test-only corruption fixtures), and seating is
the sole insertion point — so `len()` *is* the started-count, eliminated
included. **Load-bearing invariant:** per-investigator math relies on eliminated
investigators staying in the map; a future change that removes them must
substitute a stored started-count.

**`clues_fixed`** distinguishes the two: absent/false = per-investigator,
`true` = fixed. The Gathering's locations are all per-investigator; ~20 Dunwich
locations are fixed (already in the corpus). The pipeline currently drops the
flag.

**Move targets** (p.14): movement is to a *connecting* location. `move_action`
already enforces both connectivity and in-play-ness (see Out of scope).

## Design

### `ClueValue` — a shared enum (`card-dsl`)

```rust
pub enum ClueValue {
    /// `value × (number of investigators who started the scenario)`.
    PerInvestigator(u8),
    /// Exactly `value`, regardless of investigator count.
    Fixed(u8),
}
```

Used by **both** the corpus (`CardKind::Location`) and game-core's `Location`
(one representation, no flat↔enum mapping). Lives in `card-dsl` (below both).

### Pipeline + corpus

`CardKind::Location { shroud, clues: u8, victory }` →
`CardKind::Location { shroud, printed_clues: ClueValue, victory }`. The pipeline
(`card-data-pipeline`) reads `clues_fixed` from the snapshot JSON and emits
`ClueValue::Fixed(n)` when `clues_fixed == true`, else
`ClueValue::PerInvestigator(n)` (where `n` is `clues`, defaulting 0). Regenerate
`crates/cards/src/generated/cards.rs`. This remodels the `Location` variant (in
the spirit of #254's `CardMetadata` remodel); the only readers are
`GameState::add_location` and C2's future victory-point logic.

### `Location` (game-core state)

- Add `printed_clues: ClueValue` (the printed value + per-investigator/fixed).
- `clues: u8` becomes **current** clues on the location (0 while unrevealed;
  set at reveal).
- `revealed: bool` — locations built by `add_location` enter **`false`**.
- `add_location` / `add_set_aside_location` set `printed_clues` from the passed
  metadata's `CardKind::Location`, `clues = 0`, `revealed = false`.
- `Location::new(id, code, name, shroud, clues)` **keeps today's behavior**
  (`revealed = true`, `clues` as given) by defaulting `printed_clues =
  ClueValue::Fixed(clues)` — so the `test_location` fixture, the synthetic
  scenario, and the game-core unit tests that build revealed locations with
  explicit clues need no change.

### `reveal_location` — the shared reveal step

A helper in `game-core` (engine dispatch):

```
fn reveal_location(cx, location_id):
    let loc = locations.get_mut(location_id)   // no-op if absent / already revealed
    if loc.revealed: return
    loc.revealed = true
    loc.clues = match loc.printed_clues {
        PerInvestigator(n) => n * investigators.len() (saturating, u8),
        Fixed(n) => n,
    }
    emit Event::LocationRevealed { location, clues }
```

A new `Event::LocationRevealed { location, clues }` records the placement (for
observers/replay clarity). Idempotent: revealing an already-revealed location is
a no-op (no event, no extra clues).

### Reveal fires on *investigator* entry — three call sites

`reveal_location` is called wherever an investigator's `current_location` is set
to a possibly-unrevealed location:

1. **Seating** (`phases.rs` roster step) — after the investigators are inserted
   at `starting_location`, reveal it (so `investigators.len()` reflects the full
   seated count). The Study reveals here with `2 × len` clues.
2. **`move_action`** (`actions.rs`) — when the investigator's destination is set
   (after the existing AoO / move resolution), reveal the destination. Engaged
   enemies move with the investigator but do **not** trigger reveal — only the
   investigator's entry does.
3. **`RelocateAllInvestigators`** (`evaluator.rs`, Act-1's reverse) — after
   relocating everyone to the Hallway, reveal the Hallway.

**Enemy / Hunter movement (`hunters.rs`, spawn) is untouched** — enemies entering
an unrevealed location leave it unrevealed (per the rule).

### `investigate` gate

`investigate` gains an early rejection when the investigator's location is
unrevealed. In practice this is unreachable (being at a location means you
entered → revealed it), but it makes the "unrevealed isn't investigatable" rule
explicit and guards against a future path that parks an investigator on an
unrevealed location.

## Out of scope (already handled / deferred)

- **Move-to-connected-in-play:** already enforced by `move_action`
  (`state.locations.contains_key(destination)` — the in-play gate, since
  set-aside locations live in `set_aside_locations`, not `locations` — plus the
  `from_loc.connections.contains(destination)` check). #257 adds only a
  **regression test** asserting a move to a set-aside location is rejected,
  since set-aside is now a real concept.
- **Fixed-clue scenarios** (Dunwich) aren't *played* until Phase 10, but the
  corpus now represents them correctly via `ClueValue::Fixed`.
- **Per-investigator values elsewhere** (act/agenda thresholds, encounter
  effects) — out of scope; this issue only places location clues. The
  `investigators.len()` multiplier is reusable when those land.

## Testing

- **`card-dsl` unit:** `ClueValue` round-trips serde; `CardKind::Location`
  carries `printed_clues`.
- **Pipeline unit:** a `clues_fixed: true` location emits `Fixed(n)`; an absent
  flag emits `PerInvestigator(n)`.
- **Engine unit (`game_state.rs` / reveal helper):** `add_location` enters a
  location unrevealed with `clues == 0` and the right `printed_clues`;
  `reveal_location` places `n × len` (per-investigator, including a 2-investigator
  case) and `n` (fixed), emits `LocationRevealed`, and is idempotent.
- **Engine unit (`actions.rs`):** moving an investigator to an unrevealed in-play
  location reveals it + places clues; **moving to a set-aside location is
  rejected** (the regression test); `investigate` rejects on an unrevealed
  location.
- **Scenario (`the_gathering.rs` + `tests/`):** setup leaves the board
  unrevealed (clues 0); after seating, the Study is revealed with
  `2 × #investigators` clues; the act-1 board-rebuild test asserts the Hallway
  reveals (clues placed) when investigators are relocated, and the set-aside
  Attic/Cellar/Parlor enter unrevealed. Existing assertions that read the Study's
  clue count update from the flat `2` to the per-investigator placement.

## Open questions

None. The shared `ClueValue` enum (over a `clues_fixed: bool` on the corpus +
flat fields on `Location`) was chosen for a single clean representation across
corpus and state. The per-investigator multiplier is `state.investigators.len()`,
faithful because eliminated investigators remain in the map (documented as a
load-bearing invariant).
