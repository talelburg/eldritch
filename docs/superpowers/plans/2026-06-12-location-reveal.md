# Location reveal-on-entry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Model the Arkham location-reveal mechanic — locations enter play unrevealed, reveal on the first *investigator* entry, and at reveal place clues equal to the printed value (`PerInvestigator(n) → n × investigators.len()`, or `Fixed(n)`).

**Architecture:** A shared `ClueValue` enum (`card-dsl`) reshapes `CardKind::Location` (`clues: u8` → `printed_clues: ClueValue`) and game-core's `Location`. The pipeline ingests `clues_fixed`. A `reveal_location(cx, id)` helper fires from the three investigator-entry sites (seating, `move_action`, `RelocateAllInvestigators`); enemy movement is untouched. Built green-at-each-step: the reveal machinery + call sites land while locations still enter *revealed* (so the wired sites are no-ops), then a final task flips `add_location` to enter unrevealed, activating them.

**Tech Stack:** Rust workspace. `card-dsl` (`ClueValue`, `CardKind`), `card-data-pipeline` (ingest + regenerate `cards/src/generated/cards.rs`), `game-core` (`Location`, `Event`, dispatch), `scenarios` (`the_gathering`). Per-investigator multiplier = `state.investigators.len()` (faithful: eliminated investigators stay in the map).

**Spec:** `docs/superpowers/specs/2026-06-12-location-reveal-design.md`

**CI gauntlet (before each task-completing commit):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

---

### Task 1: `ClueValue` enum (`card-dsl`)

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs` (add the enum near `CardKind`)
- Test: `crates/card-dsl/src/card_data.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test:**

```rust
#[test]
fn clue_value_round_trips_through_serde() {
    for cv in [ClueValue::PerInvestigator(2), ClueValue::Fixed(3)] {
        let json = serde_json::to_string(&cv).expect("serialize");
        let back: ClueValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cv, back);
    }
}
```
(Place it in `card_data.rs`'s existing `#[cfg(test)]` module, importing via `super::*` as the file's other tests do.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p card-dsl clue_value_round_trips`
Expected: FAIL — `ClueValue` not found.

- [ ] **Step 3: Add the enum** (near `CardKind`; match `CardKind`'s derives plus `Copy`, since both variants are `u8`):

```rust
/// A location's printed clue value. `PerInvestigator(n)` places
/// `n × (number of investigators who started the scenario)` on reveal;
/// `Fixed(n)` places exactly `n`. Distinguishes ArkhamDB's `clues_fixed`
/// (absent/false → per-investigator; `true` → fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClueValue {
    /// `value × #investigators` at reveal time.
    PerInvestigator(u8),
    /// Exactly `value`, regardless of investigator count.
    Fixed(u8),
}
```
(`Serialize`/`Deserialize` are already imported in this file — confirm.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p card-dsl clue_value_round_trips`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/card-dsl/src/card_data.rs
git commit -m "card-dsl: add ClueValue enum (per-investigator / fixed) (#257)"
```

---

### Task 2: Reshape `CardKind::Location` + ingest `clues_fixed` + regenerate corpus

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs` (the `CardKind::Location` variant)
- Modify: `crates/card-data-pipeline/src/main.rs` (`RawCard`, the `Location` emit, the generated-file `use` header, a `clue_value_lit` helper)
- Regenerate: `crates/cards/src/generated/cards.rs` (via the pipeline)
- Modify: `crates/game-core/src/state/game_state.rs` (`location_from_metadata` — read `printed_clues`'s base value; behavior unchanged)
- Modify: the `add_location_tests` helper in `game_state.rs` (the `CardKind::Location` literal)

This is an atomic type reshape: `CardKind::Location`'s `clues: u8` becomes `printed_clues: ClueValue`. **Behavior is unchanged** — `add_location` still builds revealed locations with the flat clue count (the reveal mechanic lands in Tasks 3–4).

- [ ] **Step 1: Add a pipeline unit test** (in `card-data-pipeline/src/main.rs`'s `#[cfg(test)]`, mirroring the existing pipeline tests' style):

```rust
#[test]
fn location_clue_value_reflects_clues_fixed() {
    assert_eq!(clue_value_lit(2, false), "ClueValue::PerInvestigator(2)");
    assert_eq!(clue_value_lit(2, true), "ClueValue::Fixed(2)");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p card-data-pipeline location_clue_value`
Expected: FAIL — `clue_value_lit` not found.

- [ ] **Step 3: Reshape the type.** In `card_data.rs`, change the `Location` variant:

```rust
    /// Location — a place investigators move between and investigate.
    Location {
        /// Shroud (investigate difficulty).
        shroud: u8,
        /// Printed clue value (per-investigator or fixed).
        printed_clues: ClueValue,
        /// Victory points when in the victory display.
        victory: Option<u8>,
    },
```

- [ ] **Step 4: Update the pipeline.** In `card-data-pipeline/src/main.rs`:
  - Add `clues_fixed: Option<bool>,` to the `RawCard` struct (next to `clues: Option<u8>`).
  - Add the helper:
    ```rust
    /// Emit the `ClueValue` literal for a location's clues.
    fn clue_value_lit(clues: u8, fixed: bool) -> String {
        if fixed {
            format!("ClueValue::Fixed({clues})")
        } else {
            format!("ClueValue::PerInvestigator({clues})")
        }
    }
    ```
  - Change the `"Location"` emit arm:
    ```rust
        "Location" => format!(
            "CardKind::Location {{ shroud: {}, printed_clues: {}, victory: {} }}",
            c.shroud.unwrap_or(0),
            clue_value_lit(c.clues.unwrap_or(0), c.clues_fixed.unwrap_or(false)),
            opt_u8(c.victory),
        ),
    ```
  - Add `ClueValue` to the generated-file `use` header (the string at the `out.push_str` / `use card_dsl::card_data::{…}` site, ~main.rs:344): `use card_dsl::card_data::{CardKind, CardMetadata, ClueValue, Class, SkillIcons, Skills, Slot};`.
  - The `CardMetadata` struct (the normalized intermediate, ~main.rs:203) carries `clues: Option<u8>` — keep it; also add `clues_fixed: bool` there and set it from `raw.clues_fixed.unwrap_or(false)` (~main.rs:250) so the emit can read it. (Check the actual intermediate struct's fields and thread `clues_fixed` through to the emit; the emit reads `c.clues` / `c.clues_fixed`.)

- [ ] **Step 5: Run the pipeline test + regenerate the corpus**

Run: `cargo test -p card-data-pipeline location_clue_value` → PASS.
Run: `cargo run -p card-data-pipeline`
Then verify the regenerated `crates/cards/src/generated/cards.rs` now emits `CardKind::Location { shroud: …, printed_clues: ClueValue::PerInvestigator(…)/Fixed(…), victory: … }` and imports `ClueValue`. Spot-check: `01111` (Study) → `PerInvestigator(2)`; a Dunwich location like `02242` (Dunwich Village) → `Fixed(…)`.

- [ ] **Step 6: Fix the consumers** (compile-driven under the type change):
  - `game_state.rs` `location_from_metadata` — the match now binds `printed_clues`. Keep behavior by extracting the base value:
    ```rust
        let (shroud, clues) = match &metadata.kind {
            CardKind::Location { shroud, printed_clues, .. } => {
                let base = match printed_clues {
                    ClueValue::PerInvestigator(n) | ClueValue::Fixed(n) => *n,
                };
                (*shroud, base)
            }
            other => panic!("add_location: card {} is not a Location ({other:?})", metadata.code),
        };
    ```
    Add `ClueValue` to the `use crate::card_data::{…}` line in `game_state.rs`.
  - The `add_location_tests::location_meta` helper — change its `CardKind::Location { shroud, clues, victory: None }` literal to `CardKind::Location { shroud, printed_clues: ClueValue::PerInvestigator(clues), victory: None }` (import `ClueValue` in that test module). Its existing assertions (`study.clues == 2` etc.) still hold.
  - Any other hand-written `CardKind::Location` literal the compiler flags (grep `CardKind::Location` across `crates/` — the generated file is already handled): update to `printed_clues`.

- [ ] **Step 7: Full strict gauntlet, then commit**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
All green, then:
```bash
git add crates/card-dsl/src/card_data.rs crates/card-data-pipeline/src/main.rs crates/cards/src/generated/cards.rs crates/game-core/src/state/game_state.rs
git commit -m "card-data: CardKind::Location carries ClueValue; ingest clues_fixed (#257)"
```

---

### Task 3: `Event::LocationRevealed` + `Location.printed_clues` + `reveal_location` helper (wired as no-ops)

**Files:**
- Modify: `crates/game-core/src/event.rs` (add `LocationRevealed`)
- Modify: `crates/game-core/src/state/location.rs` (add `printed_clues` field; `Location::new` defaults it)
- Modify: `crates/game-core/src/test_support/fixtures.rs` (`test_location` literal)
- Modify: `crates/game-core/src/state/game_state.rs` (`location_from_metadata` stores the real `printed_clues`; still revealed)
- Create: `crates/game-core/src/engine/dispatch/reveal.rs` (the helper) + declare it in `dispatch/mod.rs`
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (seating), `actions.rs` (`move_action`), `crates/game-core/src/engine/evaluator.rs` (`RelocateAllInvestigators`) — call the helper
- Test: `reveal.rs` (`#[cfg(test)]`)

After this task locations still enter **revealed** (`add_location` unchanged in that respect), so every wired `reveal_location` call is a no-op — green, no behavior change. The helper's clue-placement is unit-tested directly with a manually-unrevealed location.

- [ ] **Step 1: Add the event.** In `event.rs`'s `Event` enum (`LocationId` is already imported):

```rust
    /// A location was revealed (turned face-up) on first investigator
    /// entry; `clues` were placed on it.
    LocationRevealed {
        /// The revealed location.
        location: LocationId,
        /// Clues placed at reveal.
        clues: u8,
    },
```

- [ ] **Step 2: Add the `printed_clues` field to `Location`.** In `location.rs`, add to the struct (after `clues`):

```rust
    /// Printed clue value, read at reveal time to place `clues`.
    pub printed_clues: ClueValue,
```
Import `ClueValue` (`use card_dsl::card_data::ClueValue;` — confirm the crate path used in `location.rs`; it's re-exported at `game_core::card_data`). In `Location::new`, set `printed_clues: ClueValue::Fixed(clues)` (preserves today's behavior — a `new`-built location is revealed with its clues as a fixed count). Update the `test_location` fixture literal in `fixtures.rs` to include `printed_clues: ClueValue::Fixed(clues)` (it sets `clues` directly — mirror it). Fix any other `Location { … }` literal the compiler flags (e.g. `Location::new`'s own tests).

- [ ] **Step 3: Store the real `printed_clues` in `add_location`.** In `game_state.rs` `location_from_metadata`, replace the body to build the `Location` carrying the metadata's `printed_clues` (still `revealed: true`, `clues = base` — behavior unchanged):

```rust
    fn location_from_metadata(&mut self, metadata: &CardMetadata) -> Location {
        let (shroud, printed_clues) = match &metadata.kind {
            CardKind::Location { shroud, printed_clues, .. } => (*shroud, *printed_clues),
            other => panic!("add_location: card {} is not a Location ({other:?})", metadata.code),
        };
        let base = match printed_clues {
            ClueValue::PerInvestigator(n) | ClueValue::Fixed(n) => n,
        };
        let id = self.mint_location_id();
        Location {
            id,
            code: CardCode::new(metadata.code.clone()),
            name: metadata.name.clone(),
            shroud,
            clues: base,
            revealed: true,
            printed_clues,
            connections: Vec::new(),
        }
    }
```
(`Location` is `#[non_exhaustive]` but built within game-core, so the struct literal compiles. Keep `add_location`/`add_set_aside_location` themselves unchanged.)

- [ ] **Step 4: Write the helper test** in the new `reveal.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::reveal_location;
    use crate::card_data::ClueValue;
    use crate::engine::Cx;
    use crate::event::Event;
    use crate::state::{CardCode, Location, LocationId};
    use crate::test_support::{test_investigator, GameStateBuilder};

    fn unrevealed(id: u32, code: &str, printed: ClueValue) -> Location {
        let mut loc = Location::new(LocationId(id), CardCode(code.into()), "L", 1, 0);
        loc.revealed = false;
        loc.printed_clues = printed;
        loc.clues = 0;
        loc
    }

    #[test]
    fn reveal_places_per_investigator_clues_times_count() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(unrevealed(5, "x", ClueValue::PerInvestigator(2)))
            .build();
        let mut events = Vec::new();
        reveal_location(&mut Cx { state: &mut state, events: &mut events }, LocationId(5));
        let loc = &state.locations[&LocationId(5)];
        assert!(loc.revealed);
        assert_eq!(loc.clues, 4, "2 per-investigator × 2 investigators");
        assert!(events.iter().any(|e| matches!(e, Event::LocationRevealed { location, clues } if *location == LocationId(5) && *clues == 4)));
    }

    #[test]
    fn reveal_places_fixed_clues_regardless_of_count() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(unrevealed(5, "x", ClueValue::Fixed(3)))
            .build();
        let mut events = Vec::new();
        reveal_location(&mut Cx { state: &mut state, events: &mut events }, LocationId(5));
        assert_eq!(state.locations[&LocationId(5)].clues, 3);
    }

    #[test]
    fn reveal_is_idempotent_on_already_revealed() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(Location::new(LocationId(5), CardCode("x".into()), "L", 1, 9))
            .build();
        let mut events = Vec::new();
        reveal_location(&mut Cx { state: &mut state, events: &mut events }, LocationId(5));
        assert_eq!(state.locations[&LocationId(5)].clues, 9, "unchanged");
        assert!(!events.iter().any(|e| matches!(e, Event::LocationRevealed { .. })));
    }
}
```

- [ ] **Step 5: Run to verify it fails**

Run: `cargo test -p game-core reveal_` (the module isn't declared yet → fails to compile / not found).

- [ ] **Step 6: Implement the helper.** Create `crates/game-core/src/engine/dispatch/reveal.rs`:

```rust
//! Location reveal-on-entry (Rules Reference p.14): the first time an
//! investigator enters a location it is revealed and clues are placed
//! (`PerInvestigator(n) → n × #investigators`, or `Fixed(n)`). Enemy
//! movement does not reveal — only the investigator-entry call sites
//! (seating, `move_action`, `RelocateAllInvestigators`) call this.

use crate::card_data::ClueValue;
use crate::event::Event;
use crate::state::LocationId;

use super::Cx;

/// Reveal `location_id` if it is unrevealed, placing its printed clues.
/// No-op if the location is absent or already revealed.
pub(crate) fn reveal_location(cx: &mut Cx, location_id: LocationId) {
    // "Number of investigators who started the scenario" — `len()` is
    // faithful because eliminated investigators stay in the map (status
    // flipped, never removed). If that invariant changes, per-investigator
    // math here must switch to a stored started-count.
    let count = u8::try_from(cx.state.investigators.len()).unwrap_or(u8::MAX);
    let Some(loc) = cx.state.locations.get_mut(&location_id) else {
        return;
    };
    if loc.revealed {
        return;
    }
    loc.revealed = true;
    let clues = match loc.printed_clues {
        ClueValue::PerInvestigator(n) => n.saturating_mul(count),
        ClueValue::Fixed(n) => n,
    };
    loc.clues = clues;
    cx.events.push(Event::LocationRevealed {
        location: location_id,
        clues,
    });
}
```
Declare the module in `crates/game-core/src/engine/dispatch/mod.rs`: `pub(crate) mod reveal;` (it must be reachable from `evaluator.rs`, like `act_agenda` is).

- [ ] **Step 7: Wire the three call sites** (no-ops while locations enter revealed):
  - **Seating** (`phases.rs`, the roster step): after the `for` loop that inserts investigators (so `investigators.len()` is final), if `start` is `Some(loc)`, call `super::reveal::reveal_location(cx, loc)`. (`start` is the `starting_location` captured earlier.)
  - **`move_action`** (`actions.rs`): right after the `cx.events.push(Event::InvestigatorMoved { … })` and **before** the forced-trigger fire, add `super::reveal::reveal_location(cx, destination);`.
  - **`RelocateAllInvestigators`** (`evaluator.rs`): after the relocation `for` loop, add `crate::engine::dispatch::reveal::reveal_location(cx, dest);` (use the path that compiles from `evaluator.rs`, mirroring how it reaches `act_agenda`).

- [ ] **Step 8: Run to verify the helper tests pass + nothing regressed**

Run: `cargo test -p game-core reveal_` → PASS (3 tests).
Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features` → all green (no behavior change — the wired calls are no-ops on revealed locations).

- [ ] **Step 9: Full gauntlet + commit**

```sh
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check && RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Then:
```bash
git add crates/game-core/src/event.rs crates/game-core/src/state/location.rs crates/game-core/src/test_support/fixtures.rs crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/reveal.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/dispatch/phases.rs crates/game-core/src/engine/dispatch/actions.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: reveal_location helper + LocationRevealed; wire entry sites (no-op) (#257)"
```

---

### Task 4: Enter unrevealed (activate reveal) + investigate gate + move regression + scenario tests

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`location_from_metadata` — enter unrevealed)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (`investigate` revealed gate)
- Modify: `crates/game-core/src/state/game_state.rs` `add_location_tests` (unrevealed expectations)
- Modify: `crates/scenarios/src/the_gathering.rs` (`#[cfg(test)]` setup tests) + `crates/scenarios/tests/the_gathering.rs` (integration)
- Test: `actions.rs` (`#[cfg(test)]` — move-to-set-aside regression + investigate-unrevealed)

This flips `add_location` to enter locations **unrevealed** with `clues = 0`. The reveal sites wired in Task 3 now activate: seating reveals the Study, `move_action`/`RelocateAllInvestigators` reveal on entry.

- [ ] **Step 1: Write the failing tests.**
  - In `actions.rs`'s test module, a move-to-set-aside regression and an investigate-unrevealed rejection (adapt the harness to the file's existing move/investigate test scaffolding — find a helper like `move_scenario()`):
    ```rust
    #[test]
    fn move_to_a_set_aside_location_is_rejected() {
        // A location that exists only in the set-aside zone is out of play;
        // moving to it must be rejected (not in state.locations).
        use crate::state::{CardCode, Location, LocationId};
        let (inv_id, a, _b, mut state) = move_scenario();
        // Add a set-aside location and (illegally) connect `a` to it.
        state.set_aside_locations.push(Location::new(LocationId(99), CardCode("setaside".into()), "Aside", 1, 0));
        state.locations.get_mut(&a).unwrap().connections.push(LocationId(99));
        let r = apply(state, Action::Player(PlayerAction::Move { investigator: inv_id, destination: LocationId(99) }));
        assert!(matches!(r.outcome, EngineOutcome::Rejected { .. }), "set-aside is out of play");
    }
    ```
    (Use the actual return shape of the file's `move_scenario()` — it yields `(inv_id, a, b, state)` per the existing tests; adjust names.)
  - In the scenario setup tests, the post-setup expectation changes (Study now unrevealed, 0 clues) and a post-seating expectation is added. See Steps 3–4.

- [ ] **Step 2: Flip `add_location` to enter unrevealed.** In `location_from_metadata` (`game_state.rs`), change the built `Location` to `revealed: false` and `clues: 0` (drop the `base` local — `printed_clues` carries the value for reveal):

```rust
    fn location_from_metadata(&mut self, metadata: &CardMetadata) -> Location {
        let (shroud, printed_clues) = match &metadata.kind {
            CardKind::Location { shroud, printed_clues, .. } => (*shroud, *printed_clues),
            other => panic!("add_location: card {} is not a Location ({other:?})", metadata.code),
        };
        let id = self.mint_location_id();
        Location {
            id,
            code: CardCode::new(metadata.code.clone()),
            name: metadata.name.clone(),
            shroud,
            clues: 0,
            revealed: false,
            printed_clues,
            connections: Vec::new(),
        }
    }
```

- [ ] **Step 3: Add the investigate gate.** In `investigate` (`actions.rs`), after the `location` is resolved (the `let location = cx.state.locations.get(&location_id)…` block) and before computing `difficulty`, add:

```rust
    if !location.revealed {
        return EngineOutcome::Rejected {
            reason: format!("Investigate: location {location_id:?} is not revealed").into(),
        };
    }
```

- [ ] **Step 4: Update `add_location_tests`** (`game_state.rs`): the existing `add_location_mints_sequential_ids_and_extracts_metadata` asserts `(study.shroud, study.clues) == (2, 2)`; change to assert the location enters unrevealed with `clues == 0` and the right `printed_clues`:
```rust
        assert_eq!(study.shroud, 2);
        assert_eq!(study.clues, 0, "enters unrevealed with no clues");
        assert!(!study.revealed);
        assert_eq!(study.printed_clues, ClueValue::PerInvestigator(2));
```

- [ ] **Step 5: Update the Gathering scenario tests.**
  - `the_gathering.rs` `#[cfg(test)] mod tests`: `setup_reads_card_stats_from_corpus` asserts `(study.shroud, study.clues) == (2, 2)` — change to `study.shroud == 2`, `study.clues == 0`, `!study.revealed`, `study.printed_clues == ClueValue::PerInvestigator(2)` (import `ClueValue`). Any set-aside-location clue assertions similarly become `clues == 0` / `!revealed`.
  - `tests/the_gathering.rs` integration: in `roster_seating_places_investigator_at_study`, after seating, additionally assert the Study is now revealed with `2 × #seated` clues:
    ```rust
    let study = &state.locations[&state.starting_location.unwrap()];
    assert!(study.revealed, "seating reveals the starting location");
    assert_eq!(study.clues, 2, "1 investigator × 2 per-investigator");
    ```
    In `advancing_act_1_rebuilds_the_board`, after the act-1 advance, assert the Hallway (where investigators were relocated) is revealed:
    ```rust
    let hallway = result.state.locations.values().find(|l| l.code.as_str() == "01112").unwrap();
    assert!(hallway.revealed, "relocate-to-Hallway reveals it");
    ```
    (The Attic/Cellar/Parlor remain unrevealed — they're in play but unentered.)

- [ ] **Step 6: Run the suites.** Fix any other test that asserted location clues from the old flat placement (e.g. closing-demo / synthetic — those use `test_location`, which stays revealed with explicit clues, so they should be unaffected; if any scenario test investigated a location whose clues now require reveal, ensure the investigator entered it first).

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: all green.

- [ ] **Step 7: Full gauntlet + commit**

```sh
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check && RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Then:
```bash
git add -A
git commit -m "engine: locations enter unrevealed; reveal on entry places clues (#257)"
```
(Review `git status` before `git add -A` — only the intended files.)

---

## Final: full CI gauntlet

- [ ] Run the complete gauntlet (incl. wasm) on the final state:
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green.

- [ ] Phase-doc: #257 is cross-cutting (engine mechanic), not a phase-N deliverable — no `docs/phases` update. The PR closes #257.

---

## Self-Review notes (author)

- **Spec coverage:** `ClueValue` → Task 1; corpus reshape + `clues_fixed` ingest → Task 2; `Location.printed_clues` + `reveal_location` + `LocationRevealed` + entry-site wiring → Task 3; enter-unrevealed activation + investigate gate + move-to-set-aside regression + scenario tests → Task 4. Per-investigator = `investigators.len()` (documented invariant in `reveal.rs`). Enemy/Hunter movement untouched (no site added there). Move-to-connected-in-play already enforced — Task 4 adds only the regression test.
- **Green-at-each-step:** Tasks 1–2 are behavior-preserving (corpus type reshape only). Task 3 wires the reveal sites while locations still enter revealed → all calls are no-ops → no behavior change. Task 4 flips `add_location` to unrevealed, atomically activating the sites + updating the scenario expectations. The `reveal_location` helper is unit-tested in Task 3 via a manually-unrevealed location, independent of the flip.
- **Type consistency:** `ClueValue { PerInvestigator(u8) | Fixed(u8) }` (Copy) used by `CardKind::Location.printed_clues` and `Location.printed_clues`; `Location::new` defaults `Fixed(clues)` (keeps `test_location`/synthetic/unit fixtures revealed-with-clues, unchanged); `add_location` builds via struct literal carrying the metadata's `printed_clues`. `Event::LocationRevealed { location: LocationId, clues: u8 }`. `reveal_location(cx, LocationId)` is `pub(crate)` in `dispatch::reveal`, called from `phases`/`actions`/`evaluator`.
- **Scope-out confirmed untouched:** the `test_location(id,name)` fixture and the synthetic scenario keep explicit ids + revealed locations (they use `Location::new`/`test_location`, not `add_location`).
