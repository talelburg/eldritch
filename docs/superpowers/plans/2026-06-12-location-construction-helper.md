# Location construction helper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the hand-assigned `LocationId` literals + manual connection wiring in `the_gathering::setup()` with `GameState`-owned, deterministic location id allocation + metadata-driven construction methods, mirroring the existing `next_enemy_id` allocator.

**Architecture:** `GameState` gains a `next_location_id` counter and three registry-free methods: `add_location(&CardMetadata)` and `add_set_aside_location(&CardMetadata)` (mint a deterministic id, build a `Location` from the passed card metadata, insert into `locations` / `set_aside_locations`), and `connect(a, b)` (bidirectional, resolving ids across both zones). The scenario does its own `cards::by_code` corpus lookup and passes the `&CardMetadata` in — so game-core stays registry-free and the methods are directly unit-testable. `STUDY_ID` is removed in favour of `GameState.starting_location`.

**Tech Stack:** Rust workspace. `game-core` (state), `scenarios` (`the_gathering`). `CardMetadata`/`CardKind` live in `card-dsl`, re-exported at `game_core::card_data`. Codes are `String` in card-dsl; `CardCode` is game-core's newtype.

**Spec:** `docs/superpowers/specs/2026-06-12-location-construction-helper-design.md`

**CI gauntlet (run before every task-completing commit):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

---

### Task 1: `next_location_id` counter on `GameState`

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (struct `GameState`, after the `next_enemy_id` field ~line 108)
- Modify: `crates/game-core/src/state/builder.rs` (`build()`, alongside `next_enemy_id: 0`)
- Test: `crates/game-core/src/state/game_state.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test** (add to / create a test module in `game_state.rs`):

```rust
#[cfg(test)]
mod next_location_id_tests {
    use crate::test_support::GameStateBuilder;

    #[test]
    fn game_state_starts_next_location_id_at_zero() {
        let state = GameStateBuilder::new().build();
        assert_eq!(state.next_location_id, 0);
    }

    #[test]
    fn next_location_id_round_trips_through_serde() {
        let mut state = GameStateBuilder::new().build();
        state.next_location_id = 7;
        let json = serde_json::to_string(&state).expect("serialize");
        let back: crate::state::GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.next_location_id, 7);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core next_location_id`
Expected: FAIL — `no field next_location_id on type GameState`.

- [ ] **Step 3: Add the field** to `GameState`, immediately after `pub next_enemy_id: u32,` (mirror its doc):

```rust
    /// Monotonic counter for assigning [`LocationId`]s when scenarios
    /// build their board via [`add_location`](Self::add_location) /
    /// [`add_set_aside_location`](Self::add_set_aside_location). Starts
    /// at 0 and increments after each assignment; guarantees uniqueness
    /// within a scenario and deterministic ids across replays.
    pub next_location_id: u32,
```

Initialize it in `builder.rs`'s `build()` literal, next to `next_enemy_id: 0,`:

```rust
            next_location_id: 0,
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p game-core next_location_id`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/state/builder.rs
git commit -m "engine: add next_location_id counter to GameState (#260)"
```

---

### Task 2: `add_location` / `add_set_aside_location` (metadata-driven)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (an `impl GameState` block + imports)
- Test: `crates/game-core/src/state/game_state.rs` (`#[cfg(test)]`)

Registry-free: the caller passes the `&CardMetadata`. `CardMetadata`/`CardKind` come from `crate::card_data`; `CardKind::Location { shroud: u8, clues: u8, victory: Option<u8> }`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[cfg(test)]
mod add_location_tests {
    use crate::card_data::{CardKind, CardMetadata};
    use crate::test_support::GameStateBuilder;

    fn location_meta(code: &str, name: &str, shroud: u8, clues: u8) -> CardMetadata {
        CardMetadata {
            code: code.to_string(),
            name: name.to_string(),
            traits: vec![],
            text: None,
            pack_code: "core".to_string(),
            kind: CardKind::Location { shroud, clues, victory: None },
        }
    }

    #[test]
    fn add_location_mints_sequential_ids_and_extracts_metadata() {
        let mut state = GameStateBuilder::new().build();
        let a = state.add_location(&location_meta("01111", "Study", 2, 2));
        let b = state.add_location(&location_meta("01112", "Hallway", 1, 0));
        assert_ne!(a, b, "ids are distinct");
        let study = &state.locations[&a];
        assert_eq!(study.code.as_str(), "01111");
        assert_eq!(study.name, "Study");
        assert_eq!((study.shroud, study.clues), (2, 2));
        assert!(study.connections.is_empty());
        assert!(study.revealed);
        assert_eq!(state.next_location_id, 2, "counter advanced twice");
    }

    #[test]
    fn add_set_aside_location_goes_to_the_set_aside_zone() {
        let mut state = GameStateBuilder::new().build();
        let id = state.add_set_aside_location(&location_meta("01113", "Attic", 1, 2));
        assert!(!state.locations.contains_key(&id), "not in play");
        assert_eq!(state.set_aside_locations.len(), 1);
        assert_eq!(state.set_aside_locations[0].id, id);
        assert_eq!(state.set_aside_locations[0].code.as_str(), "01113");
    }

    #[test]
    #[should_panic(expected = "not a Location")]
    fn add_location_panics_on_non_location_metadata() {
        let mut state = GameStateBuilder::new().build();
        let meta = CardMetadata {
            code: "01108".to_string(),
            name: "Trapped".to_string(),
            traits: vec![],
            text: None,
            pack_code: "core".to_string(),
            kind: CardKind::Act { clue_threshold: Some(2), victory: None },
        };
        state.add_location(&meta);
    }
}
```

Note: confirm `CardKind::Act`'s exact fields against `crates/card-dsl/src/card_data.rs` and adjust the panic-test literal if they differ (the point is "any non-`Location` kind").

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core add_location`
Expected: FAIL — `add_location` not found.

- [ ] **Step 3: Implement.** Add (or extend) an `impl GameState` block in `game_state.rs`. Ensure the imports `use crate::card_data::{CardKind, CardMetadata};` are present at the top of the file (add if missing). `CardCode`, `Location`, `LocationId` are already in scope in this module.

```rust
impl GameState {
    /// Mint a fresh, deterministic [`LocationId`] (sequential from
    /// `next_location_id`).
    fn mint_location_id(&mut self) -> LocationId {
        let id = LocationId(self.next_location_id);
        self.next_location_id = self.next_location_id.saturating_add(1);
        id
    }

    /// Build a [`Location`] from its card `metadata`, minting a fresh id.
    /// Panics if `metadata` is not a `Location` card (a build-time
    /// invariant — scenarios hand their own location cards).
    fn location_from_metadata(&mut self, metadata: &CardMetadata) -> Location {
        let (shroud, clues) = match &metadata.kind {
            CardKind::Location { shroud, clues, .. } => (*shroud, *clues),
            other => panic!(
                "add_location: card {} is not a Location ({other:?})",
                metadata.code
            ),
        };
        let id = self.mint_location_id();
        Location::new(
            id,
            CardCode::new(metadata.code.clone()),
            metadata.name.clone(),
            shroud,
            clues,
        )
    }

    /// Add a location **into play** from its card metadata, returning the
    /// minted [`LocationId`]. The id is deterministic (construction order),
    /// so scenarios never hand-pick id literals.
    pub fn add_location(&mut self, metadata: &CardMetadata) -> LocationId {
        let loc = self.location_from_metadata(metadata);
        let id = loc.id;
        self.locations.insert(id, loc);
        id
    }

    /// Add a location to the **set-aside** (out-of-play) zone from its card
    /// metadata, returning the minted [`LocationId`]. Card effects (e.g.
    /// The Gathering's Act-1 reverse) later move it into play.
    pub fn add_set_aside_location(&mut self, metadata: &CardMetadata) -> LocationId {
        let loc = self.location_from_metadata(metadata);
        let id = loc.id;
        self.set_aside_locations.push(loc);
        id
    }
}
```

If an `impl GameState` block already exists in this file, add the methods there rather than opening a second block (keep it tidy; match the file's structure).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p game-core add_location`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs
git commit -m "engine: GameState::add_location/add_set_aside_location from metadata (#260)"
```

---

### Task 3: `connect` — bidirectional, cross-zone

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (the `impl GameState` block)
- Test: `crates/game-core/src/state/game_state.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing tests:**

```rust
#[cfg(test)]
mod connect_tests {
    use crate::state::{CardCode, Location, LocationId};
    use crate::test_support::GameStateBuilder;

    #[test]
    fn connect_wires_both_directions() {
        let mut state = GameStateBuilder::new()
            .with_location(Location::new(LocationId(1), CardCode("a".into()), "A", 1, 0))
            .with_location(Location::new(LocationId(2), CardCode("b".into()), "B", 1, 0))
            .build();
        state.connect(LocationId(1), LocationId(2));
        assert_eq!(state.locations[&LocationId(1)].connections, vec![LocationId(2)]);
        assert_eq!(state.locations[&LocationId(2)].connections, vec![LocationId(1)]);
    }

    #[test]
    fn connect_resolves_set_aside_locations() {
        // Both endpoints live in the set-aside zone (The Gathering wires
        // its board there before Act 1 brings it into play).
        let mut state = GameStateBuilder::new().build();
        state.set_aside_locations.push(Location::new(LocationId(2), CardCode("hub".into()), "Hub", 1, 0));
        state.set_aside_locations.push(Location::new(LocationId(3), CardCode("spoke".into()), "Spoke", 1, 0));
        state.connect(LocationId(2), LocationId(3));
        let hub = state.set_aside_locations.iter().find(|l| l.id == LocationId(2)).unwrap();
        let spoke = state.set_aside_locations.iter().find(|l| l.id == LocationId(3)).unwrap();
        assert_eq!(hub.connections, vec![LocationId(3)]);
        assert_eq!(spoke.connections, vec![LocationId(2)]);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core connect_`
Expected: FAIL — `connect` not found.

- [ ] **Step 3: Implement** — add to the `impl GameState` block:

```rust
    /// Find a location by id across both the in-play and set-aside zones.
    fn location_mut(&mut self, id: LocationId) -> Option<&mut Location> {
        if let Some(loc) = self.locations.get_mut(&id) {
            return Some(loc);
        }
        self.set_aside_locations.iter_mut().find(|l| l.id == id)
    }

    /// Wire a **bidirectional** connection between two locations (each gains
    /// the other in its `connections`). Resolves both ids across the in-play
    /// and set-aside zones. `expect`s each to exist — a build-time invariant
    /// (callers connect freshly-minted ids).
    pub fn connect(&mut self, a: LocationId, b: LocationId) {
        self.location_mut(a)
            .unwrap_or_else(|| panic!("connect: location {a:?} not found"))
            .connections
            .push(b);
        self.location_mut(b)
            .unwrap_or_else(|| panic!("connect: location {b:?} not found"))
            .connections
            .push(a);
    }
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p game-core connect_`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs
git commit -m "engine: GameState::connect (bidirectional, cross-zone) (#260)"
```

---

### Task 4: Migrate `the_gathering::setup()` + remove `STUDY_ID`

**Files:**
- Modify: `crates/scenarios/src/the_gathering.rs` (`setup()`, `location_stats` deletion, the `HALLWAY_ID..PARLOR_ID` + `STUDY_ID` const deletions, module doc, the `#[cfg(test)]` setup tests)
- Modify: `crates/scenarios/tests/the_gathering.rs` (integration tests referencing `STUDY_ID`)

This is an atomic refactor: removing the `pub const STUDY_ID` breaks every reference, so `setup()`, its unit tests, and the integration tests must all change in one commit. The existing tests are the safety net — they must stay green (run them before and after). **There is no new behavior** — the board is byte-for-byte the same except location ids are now minted (deterministically, starting at 0) instead of hand-assigned `1..=5`.

- [ ] **Step 1: Confirm the green baseline**

Run: `cargo test -p scenarios`
Expected: PASS (record this — it's the before state).

- [ ] **Step 2: Rewrite `setup()`'s board construction.** Delete: the `location_stats` helper (lines ~26–32), the `STUDY_ID` const (line ~59), the `HALLWAY_ID..PARLOR_ID` consts (lines ~86–93), the Study `Location::new` construction (lines ~100–109), and the `make`-closure block (lines ~111–128). Drop the `.with_location(study)` call from the `GameStateBuilder` chain (the chain keeps `.with_chaos_bag(...)`, `.with_scenario_id(...)`, `.with_token_modifiers(...)`, `.build()`). Also delete the old `state.starting_location = Some(STUDY_ID);` and `state.set_aside_locations = vec![…];` lines.

Then, **after** `let mut state = …build();`, build the board with the new methods:

```rust
    // The Gathering board. Ids are minted by `add_location` /
    // `add_set_aside_location` (deterministic, construction order), so no
    // hand-assigned LocationId literals. The scenario looks up each
    // location's metadata in the corpus and hands it to the engine; stats
    // (shroud/clues) come from the metadata. The Study starts in play
    // (isolated — Act 1 is "trapped in the Study"); the Hallway hub +
    // Attic/Cellar/Parlor spokes are set aside until Act 1's (01108)
    // Forced on-advance reverse brings them into play.
    let meta = |code: &str| cards::by_code(code).expect("Gathering location in corpus");
    let study = state.add_location(meta("01111"));
    let hallway = state.add_set_aside_location(meta("01112"));
    let attic = state.add_set_aside_location(meta("01113"));
    let cellar = state.add_set_aside_location(meta("01114"));
    let parlor = state.add_set_aside_location(meta("01115"));
    state.connect(hallway, attic);
    state.connect(hallway, cellar);
    state.connect(hallway, parlor);
    state.starting_location = Some(study);
```

Update the `STUDY_ID` doc reference in the module-level doc-comment (line ~8) — replace "seats investigators at [`STUDY_ID`]" with "seats investigators at the starting location (the Study, `01111`)".

- [ ] **Step 3: Update `setup()`'s unit tests** (`#[cfg(test)] mod tests`). They reference `STUDY_ID`; switch to `starting_location`. Concretely:
  - `setup_reads_card_stats_from_corpus` (~line 228): `let study = s.locations.get(&STUDY_ID).unwrap();` → `let study = &s.locations[&s.starting_location.unwrap()];`
  - `setup_places_study_in_play_and_four_set_aside` (~line 247): `let study = s.locations.get(&STUDY_ID).expect("Study present");` → `let study = &s.locations[&s.starting_location.unwrap()];`. The `assert_eq!(s.starting_location, Some(STUDY_ID));` (~line 286) → assert the starting location's code instead: `assert_eq!(s.locations[&s.starting_location.unwrap()].code.as_str(), "01111");`
  - Any other `STUDY_ID` use in this module: replace via `s.starting_location`.

Keep all the other assertions (set-aside codes in order, Hallway connections, spokes → Hallway, Study isolated) — they still hold.

- [ ] **Step 4: Update the integration tests** in `crates/scenarios/tests/the_gathering.rs`. Two references (~line 72 seating assertion, ~line 155 board-rebuild seating):
  - `Some(the_gathering::STUDY_ID)` → `the_gathering::setup().starting_location` is awkward; instead read it from the state under test. For the seating assertion (`roster_seating_places_investigator_at_study`): assert `roland.current_location == state.starting_location` (the state already in scope) and that it's `Some`. For the board-rebuild test (`advancing_act_1_rebuilds_the_board`, ~line 155): `investigator.current_location = Some(the_gathering::STUDY_ID);` → `investigator.current_location = state.starting_location;` (the `state` from `the_gathering::setup()` is in scope there).

- [ ] **Step 5: Run the scenario suite + confirm the board is unchanged**

Run: `cargo test -p scenarios`
Expected: PASS — every setup + integration test green (same board, minted ids). If a test fails because it asserted a specific `LocationId` value, fix it to use `starting_location` / look-up-by-code (do not re-introduce id literals).

- [ ] **Step 6: Full strict gauntlet, then commit**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
All green, then:
```bash
git add crates/scenarios/src/the_gathering.rs crates/scenarios/tests/the_gathering.rs
git commit -m "scenario: build the Gathering board via add_location; drop STUDY_ID (#260)"
```

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

- [ ] No phase-doc update needed (#260 is cross-cutting engine ergonomics, not a phase-N deliverable). The PR closes #260; the `TODO(#260)` comment added in PR #259 is removed by Task 4's `setup()` rewrite.

---

## Self-Review notes (author)

- **Spec coverage:** `next_location_id` → Task 1; `add_location`/`add_set_aside_location` (metadata-driven, registry-free) → Task 2; `connect` → Task 3; `setup()` migration + `STUDY_ID` removal + test updates → Task 4. Scope-out (synthetic fixture, `test_location`) untouched — confirmed no task changes them.
- **Type consistency:** methods take `&CardMetadata` (from `crate::card_data`); `CardKind::Location { shroud: u8, clues: u8, victory: Option<u8> }`; `Location::new(id, CardCode, name: impl Into<String>, shroud, clues)`; ids minted from `next_location_id` (start 0). `connect` is bidirectional and resolves both zones via `location_mut`.
- **Atomicity:** Task 4 is deliberately one commit — removing `pub const STUDY_ID` forces simultaneous updates across `setup()`, its unit tests, and the integration tests; the existing tests guard that the board is unchanged (only ids are now minted).
- **Determinism:** minted ids are sequential from 0 — deterministic across replays (the property the event-sourced engine needs), just not written as literals.
