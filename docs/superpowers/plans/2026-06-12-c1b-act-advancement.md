# C1b — Act advancement (reverse effects + objective types) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make The Gathering's act spine real — advancing Act 1 (01108) rebuilds the board (set-aside locations enter play, investigators move to the Hallway, the Study is removed), and Act 3 (01110) advances — forced — when the Ghoul Priest (01116) is defeated.

**Architecture:** Both behaviors ride the existing single-trigger forced path (`fire_forced_triggers` / `ForcedTriggerPoint`, `crates/game-core/src/engine/dispatch/forced_triggers.rs`). Reverse effects and objectives live as `Trigger::OnEvent` abilities *on the act cards* (Option C), looked up by code through `cards::REGISTRY` — exactly like the existing Attic/Cellar forced location abilities. Four new DSL `Effect`s + two new `ForcedTriggerPoint` variants; no new window/suspend machinery. Act 2's round-end objective is **out of scope** (moved to C3c/#232).

**Tech Stack:** Rust workspace. Crates: `card-dsl` (DSL types/builders), `game-core` (engine/state/evaluator/dispatch), `cards` (per-card `abilities()` impls + registry), `scenarios` (`the_gathering.rs`). Tests: per-crate `#[cfg(test)]` + integration tests in `crates/cards/tests/` and `crates/scenarios/tests/` (each its own process, installs `cards::REGISTRY`).

**Spec:** `docs/superpowers/specs/2026-06-12-phase-7-slice-1-c1b-act-advancement-design.md`

**CI gauntlet (run before every commit that finishes a task):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

---

## Pillar 1 — Act-1 reverse board-build (Tasks 1–7)

### Task 1: `set_aside_locations` zone on `GameState`

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (struct `GameState`, ~line 40 after `locations`)
- Modify: `crates/game-core/src/state/builder.rs` (`build()`, ~line 255)
- Test: `crates/game-core/src/state/builder.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test** in `builder.rs`'s test module:

```rust
#[test]
fn build_starts_with_empty_set_aside_locations() {
    let state = GameStateBuilder::new().build();
    assert!(state.set_aside_locations.is_empty());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core build_starts_with_empty_set_aside_locations`
Expected: FAIL — `no field set_aside_locations on type GameState`.

- [ ] **Step 3: Add the field** to `GameState` (after the `locations` field):

```rust
    /// Locations set aside, out of play (Rules Reference p.3, "set
    /// aside"). Brought into play by card effects — The Gathering's
    /// Act-1 reverse drains these via
    /// [`Effect::PutSetAsideLocationsIntoPlay`](crate::dsl::Effect::PutSetAsideLocationsIntoPlay).
    pub set_aside_locations: Vec<Location>,
```

And initialize it in `builder.rs`'s `build()` (in the `GameState { … }` literal):

```rust
            set_aside_locations: Vec::new(),
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p game-core build_starts_with_empty_set_aside_locations`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/state/builder.rs
git commit -m "engine: add set_aside_locations zone to GameState (#228)"
```

---

### Task 2: DSL — `EventPattern::ActAdvanced` + three world-build `Effect`s

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (enum `EventPattern` ~line 190; enum `Effect` ~line 409; builders ~line 720)
- Test: `crates/card-dsl/src/dsl.rs` (`#[cfg(test)]`)

These are bare/`String`-typed data types (card-dsl has no `game-core` `CardCode`; it uses `String` codes, mirroring `SpawnLocation::Specific(String)`).

- [ ] **Step 1: Write the failing test:**

```rust
#[test]
fn world_build_effect_builders_have_expected_shape() {
    assert!(matches!(
        put_set_aside_locations_into_play(),
        Effect::PutSetAsideLocationsIntoPlay
    ));
    assert!(matches!(
        relocate_all_investigators("01112"),
        Effect::RelocateAllInvestigators { to } if to == "01112"
    ));
    assert!(matches!(
        remove_location_from_game("01111"),
        Effect::RemoveLocationFromGame { location } if location == "01111"
    ));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p card-dsl world_build_effect_builders_have_expected_shape`
Expected: FAIL — builders/variants not found.

- [ ] **Step 3: Add the variants and builders.**

Add to `enum EventPattern` (a bare variant, like `EnemySpawned`):

```rust
    /// The act this ability is printed on advanced (its reverse side
    /// resolves). Fired forced via
    /// `ForcedTriggerPoint::ActAdvanced`; binds controller = the lead
    /// investigator (board-wide reverse effects ignore it).
    ActAdvanced,
```

Add to `enum Effect`:

```rust
    /// Put every location in `set_aside_locations` into play (Rules
    /// Reference p.3 "set aside" → in play). Board-wide; ignores the
    /// controller.
    PutSetAsideLocationsIntoPlay,
    /// Move every investigator to the in-play location with this
    /// printed `code`. Rejects if no such location is in play.
    RelocateAllInvestigators { to: String },
    /// Remove the in-play location with this printed `code` from the
    /// game. Rejects if no such location is in play.
    RemoveLocationFromGame { location: String },
```

Add builders near the other `pub fn` builders (~line 720), each `#[must_use]`:

```rust
/// Build an [`Effect::PutSetAsideLocationsIntoPlay`].
#[must_use]
pub fn put_set_aside_locations_into_play() -> Effect {
    Effect::PutSetAsideLocationsIntoPlay
}

/// Build an [`Effect::RelocateAllInvestigators`] targeting `to` (a
/// printed location code).
#[must_use]
pub fn relocate_all_investigators(to: impl Into<String>) -> Effect {
    Effect::RelocateAllInvestigators { to: to.into() }
}

/// Build an [`Effect::RemoveLocationFromGame`] targeting `location` (a
/// printed location code).
#[must_use]
pub fn remove_location_from_game(location: impl Into<String>) -> Effect {
    Effect::RemoveLocationFromGame {
        location: location.into(),
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p card-dsl world_build_effect_builders_have_expected_shape`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/card-dsl/src/dsl.rs
git commit -m "card-dsl: ActAdvanced pattern + world-build effects (#228)"
```

---

### Task 3: Evaluator arms for the three world-build effects

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (the `apply_effect` match ~line 130; add a `location_id_by_code` helper)
- Test: `crates/game-core/src/engine/evaluator.rs` (`#[cfg(test)]`)

No registry needed — these are pure state mutations through `apply_effect`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn put_set_aside_drains_into_locations() {
    use crate::state::{CardCode, Location, LocationId};
    let mut state = crate::test_support::GameStateBuilder::new().build();
    state.set_aside_locations = vec![Location::new(
        LocationId(2),
        CardCode("01112".into()),
        "Hallway",
        1,
        0,
    )];
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let out = apply_effect(
        &mut cx,
        &Effect::PutSetAsideLocationsIntoPlay,
        EvalContext::for_controller(crate::state::InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    assert!(state.set_aside_locations.is_empty());
    assert!(state.locations.contains_key(&LocationId(2)));
}

#[test]
fn relocate_all_moves_everyone_to_named_code() {
    use crate::state::{CardCode, InvestigatorId, Location, LocationId};
    use crate::test_support::{test_investigator, GameStateBuilder};
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_location(Location::new(LocationId(1), CardCode("01111".into()), "Study", 2, 2))
        .with_location(Location::new(LocationId(2), CardCode("01112".into()), "Hallway", 1, 0))
        .with_investigator(inv)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let out = apply_effect(
        &mut cx,
        &Effect::RelocateAllInvestigators { to: "01112".into() },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].current_location, Some(LocationId(2)));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::InvestigatorMoved { investigator, from, to }
            if *investigator == InvestigatorId(1) && *from == LocationId(1) && *to == LocationId(2)
    )));
}

#[test]
fn remove_location_drops_it_from_play() {
    use crate::state::{CardCode, Location, LocationId};
    use crate::test_support::GameStateBuilder;
    let mut state = GameStateBuilder::new()
        .with_location(Location::new(LocationId(1), CardCode("01111".into()), "Study", 2, 2))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let out = apply_effect(
        &mut cx,
        &Effect::RemoveLocationFromGame { location: "01111".into() },
        EvalContext::for_controller(crate::state::InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    assert!(state.locations.is_empty());
}

#[test]
fn relocate_to_missing_code_rejects() {
    use crate::state::InvestigatorId;
    use crate::test_support::GameStateBuilder;
    let mut state = GameStateBuilder::new().build();
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let out = apply_effect(
        &mut cx,
        &Effect::RelocateAllInvestigators { to: "09999".into() },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert!(matches!(out, EngineOutcome::Rejected { .. }));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core -- put_set_aside relocate_all remove_location relocate_to_missing`
Expected: FAIL — non-exhaustive match / arms unimplemented.

- [ ] **Step 3: Implement the arms.** Add a private helper at the bottom of `evaluator.rs`:

```rust
/// Find the in-play location whose printed code equals `code`.
fn location_id_by_code(state: &GameState, code: &str) -> Option<crate::state::LocationId> {
    state
        .locations
        .iter()
        .find(|(_, loc)| loc.code.as_str() == code)
        .map(|(id, _)| *id)
}
```

Add the match arms in `apply_effect` (alongside the existing `Effect::` arms):

```rust
        Effect::PutSetAsideLocationsIntoPlay => {
            let drained = std::mem::take(&mut cx.state.set_aside_locations);
            for loc in drained {
                cx.state.locations.insert(loc.id, loc);
            }
            EngineOutcome::Done
        }
        Effect::RelocateAllInvestigators { to } => {
            let Some(dest) = location_id_by_code(cx.state, to) else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "RelocateAllInvestigators: no in-play location with code {to}"
                    )
                    .into(),
                };
            };
            let ids: Vec<_> = cx.state.investigators.keys().copied().collect();
            for id in ids {
                let inv = cx
                    .state
                    .investigators
                    .get_mut(&id)
                    .expect("id sourced from keys()");
                let from = inv.current_location;
                inv.current_location = Some(dest);
                if let Some(from_id) = from {
                    if from_id != dest {
                        cx.events.push(Event::InvestigatorMoved {
                            investigator: id,
                            from: from_id,
                            to: dest,
                        });
                    }
                }
            }
            EngineOutcome::Done
        }
        Effect::RemoveLocationFromGame { location } => {
            let Some(target) = location_id_by_code(cx.state, location) else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "RemoveLocationFromGame: no in-play location with code {location}"
                    )
                    .into(),
                };
            };
            cx.state.locations.remove(&target);
            EngineOutcome::Done
        }
```

Ensure `Event` and `GameState` are in scope (the module already imports `crate::event::Event` and `crate::state::GameState`).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p game-core -- put_set_aside relocate_all remove_location relocate_to_missing`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs
git commit -m "engine: evaluator arms for world-build effects (#228)"
```

---

### Task 4: `ForcedTriggerPoint::ActAdvanced` + fire from `advance_act`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`ForcedTriggerPoint` enum ~line 26; `collect_forced_hits` ~line 67)
- Modify: `crates/game-core/src/engine/dispatch/act_agenda.rs` (`advance_act` ~line 157)
- Modify: `crates/game-core/src/test_support/mod.rs` (add `fire_forced_on_act_advance` helper)
- Test: `crates/game-core/src/engine/dispatch/act_agenda.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test** in `act_agenda.rs`'s `advance_act_tests` — without a registry, advancing must still bump the cursor (the forced fire is a no-op):

```rust
#[test]
fn advance_act_without_registry_still_advances() {
    use crate::scenario::Resolution;
    use crate::state::{Act, CardCode, InvestigatorId, Phase};
    use crate::test_support::{test_investigator, GameStateBuilder};
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 2;
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![
        Act { code: CardCode("01108".into()), clue_threshold: 2, resolution: None },
        Act { code: CardCode("01109".into()), clue_threshold: 3,
              resolution: Some(Resolution::Won { id: "R1".into() }) },
    ];
    let result = apply(state, Action::Player(PlayerAction::AdvanceAct { investigator: inv }));
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.act_index, 1, "cursor advances even with no forced ability");
}
```

- [ ] **Step 2: Run to verify it passes already** (regression guard — `advance_act` already advances; this pins behavior before the wiring):

Run: `cargo test -p game-core advance_act_without_registry_still_advances`
Expected: PASS (existing behavior).

- [ ] **Step 3: Add the `ForcedTriggerPoint` variant** in `forced_triggers.rs`:

```rust
    /// An act advanced (its reverse side resolves). Scans the
    /// *leaving* act's card for `EventPattern::ActAdvanced` forced
    /// abilities; binds controller = the lead investigator.
    ActAdvanced {
        /// Printed code of the act that advanced.
        code: CardCode,
    },
```

Add a match arm in `collect_forced_hits` (mirrors the `PhaseEnded` act-scan, but uses the carried `code` directly):

```rust
        ForcedTriggerPoint::ActAdvanced { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            push_matching(reg, &code, lead, &mut hits, |p| {
                matches!(p, EventPattern::ActAdvanced)
            });
        }
```

(Note: this match arm consumes `code` by value; `point` is taken by value in `collect_forced_hits`. Since `CardCode` is not `Copy`, bind with `ForcedTriggerPoint::ActAdvanced { code }` and pass `&code`.)

- [ ] **Step 4: Fire it from `advance_act`** in `act_agenda.rs`. Replace the body of `advance_act`:

```rust
pub(crate) fn advance_act(cx: &mut Cx) {
    let from = cx.state.act_index;
    let leaving_code = cx.state.act_deck[from].code.clone();
    cx.events.push(crate::event::Event::ActAdvanced { from });
    // Resolve the leaving act's Forced on-advance reverse effect (the
    // board world-build) before the next act becomes current — Rules
    // Reference p.3: flip the card, follow the reverse, then the next
    // card becomes current. `()` return can't propagate a 2+-trigger
    // reject; `debug_assert!` guards it (mirror of `upkeep_phase_end`).
    let forced = super::forced_triggers::fire_forced_triggers(
        cx,
        super::forced_triggers::ForcedTriggerPoint::ActAdvanced { code: leaving_code },
    );
    debug_assert!(
        matches!(forced, EngineOutcome::Done),
        "advance_act on-advance forced did not resolve to Done: {forced:?} (2+ needs #213)"
    );
    cx.state.act_index += 1;
    if cx.state.act_index >= cx.state.act_deck.len() {
        unreachable!(
            "advance_act: act {from} advanced past the end of the deck without a resolution \
             firing — a terminal act must carry a resolution point; this is malformed \
             scenario data"
        );
    }
}
```

Note the visibility bump: `fn advance_act` → `pub(crate) fn advance_act` (Task 9's `AdvanceCurrentAct` evaluator arm reuses it). Also bump `request_resolution` to `pub(crate)` while here (same reason).

- [ ] **Step 5: Add the `fire_forced_on_act_advance` test helper** in `test_support/mod.rs` (mirror `fire_forced_on_phase_end`):

```rust
/// Test helper: fire forced triggers for an act advancing, returning
/// the `EngineOutcome`. See `fire_forced_on_enter`.
pub fn fire_forced_on_act_advance(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        crate::engine::ForcedTriggerPoint::ActAdvanced { code },
    )
}
```

- [ ] **Step 6: Run the regression test + fmt/clippy**

Run: `cargo test -p game-core advance_act_without_registry_still_advances && cargo clippy -p game-core --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/act_agenda.rs crates/game-core/src/test_support/mod.rs
git commit -m "engine: fire on-advance forced trigger from advance_act (#228)"
```

---

### Task 5: `01108` abilities impl + registry registration

**Files:**
- Create: `crates/cards/src/impls/act_01108.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`pub mod` + `abilities_for` arm + doc list)
- Test: in `act_01108.rs` (`#[cfg(test)]`)

Mirror `attic.rs`. 01108 back (verbatim, `core_encounter.json`): *"Put into play the set-aside Hallway, Cellar, Attic, and Parlor. Discard each enemy in the Study. Place each investigator in the Hallway. Remove the Study from the game."*

- [ ] **Step 1: Write the failing test** (in the new file's test module):

```rust
#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger};

    #[test]
    fn abilities_are_one_forced_on_advance_world_build() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent { pattern: EventPattern::ActAdvanced, timing: EventTiming::After }
        );
        let Effect::Seq(steps) = &abilities[0].effect else {
            panic!("expected a Seq, got {:?}", abilities[0].effect);
        };
        assert!(matches!(steps[0], Effect::PutSetAsideLocationsIntoPlay));
        assert!(matches!(&steps[1], Effect::RelocateAllInvestigators { to } if to == "01112"));
        assert!(matches!(&steps[2], Effect::RemoveLocationFromGame { location } if location == "01111"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cards abilities_are_one_forced_on_advance_world_build`
Expected: FAIL — module not declared.

- [ ] **Step 3: Write the impl** (`act_01108.rs` body, above the test module):

```rust
//! Trapped (The Gathering Act 1, 01108).
//!
//! ```text
//! Act 1 — Trapped. Clues: 2.
//! (reverse) Put into play the set-aside Hallway, Cellar, Attic, and
//! Parlor. Discard each enemy in the Study. Place each investigator in
//! the Hallway. Remove the Study from the game.
//! ```
//!
//! The reverse side is a Forced on-advance ability (Option C): it fires
//! via `ForcedTriggerPoint::ActAdvanced` when the act advances, before
//! the next act becomes current. "Discard each enemy in the Study" is a
//! faithful **no-op** — nothing can spawn into the isolated Act-1 Study
//! in Slice-1 scope (location reveal-on-entry is TODO(#257); no encounter
//! path targets the Study). The set-aside locations + their connections
//! are built by the scenario's `setup()`; this ability just moves them
//! into play.

use card_dsl::dsl::{
    on_event, put_set_aside_locations_into_play, relocate_all_investigators,
    remove_location_from_game, Ability, Effect, EventPattern, EventTiming,
};

/// `ArkhamDB` code for Act 1, "Trapped".
pub const CODE: &str = "01108";

/// 01108's Forced on-advance reverse: build the Act-1 board.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::ActAdvanced,
        EventTiming::After,
        Effect::Seq(vec![
            put_set_aside_locations_into_play(),
            relocate_all_investigators("01112"), // the Hallway
            remove_location_from_game("01111"),  // the Study
        ]),
    )]
}
```

- [ ] **Step 4: Register it** in `impls/mod.rs` — add `pub mod act_01108;`, add the match arm `act_01108::CODE => Some(act_01108::abilities()),`, and a bullet in the `# Implemented so far` doc list.

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p cards abilities_are_one_forced_on_advance_world_build`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/cards/src/impls/act_01108.rs crates/cards/src/impls/mod.rs
git commit -m "cards: 01108 Trapped on-advance reverse ability (#228)"
```

---

### Task 6: `the_gathering.rs` setup — five-location board + set-aside split

**Files:**
- Modify: `crates/scenarios/src/the_gathering.rs` (`setup()` ~line 89; module doc; setup tests)
- Test: `crates/scenarios/src/the_gathering.rs` (`#[cfg(test)]`)

The board graph (scenario knowledge — no connection data in the corpus): the **Hallway (01112)** is the hub, connected to **Attic (01113)**, **Cellar (01114)**, **Parlor (01115)**. The **Study (01111)** is isolated. The Study starts in play; the other four are set aside.

- [ ] **Step 1: Update the failing setup tests.** Replace `setup_places_only_the_isolated_study` and extend the pinning so they assert the new split:

```rust
#[test]
fn setup_places_study_in_play_and_four_set_aside() {
    let s = setup();
    // In play: only the Study (Act-1 board).
    assert_eq!(s.locations.len(), 1);
    let study = s.locations.get(&STUDY_ID).expect("Study present");
    assert_eq!(study.code, CardCode("01111".into()));
    assert!(study.connections.is_empty(), "Study is isolated");
    // Set aside: Hallway, Attic, Cellar, Parlor, each pre-connected.
    let codes: Vec<_> = s.set_aside_locations.iter().map(|l| l.code.as_str().to_owned()).collect();
    assert_eq!(codes, ["01112", "01113", "01114", "01115"]);
    let hallway = s.set_aside_locations.iter().find(|l| l.code.as_str() == "01112").unwrap();
    let mut hall_conns: Vec<_> = hallway.connections.clone();
    hall_conns.sort();
    let mut others: Vec<_> = s.set_aside_locations.iter()
        .filter(|l| l.code.as_str() != "01112").map(|l| l.id).collect();
    others.sort();
    assert_eq!(hall_conns, others, "Hallway connects to Attic/Cellar/Parlor");
    for l in s.set_aside_locations.iter().filter(|l| l.code.as_str() != "01112") {
        assert_eq!(l.connections, vec![hallway.id], "spokes connect back to the Hallway");
    }
    assert_eq!(s.starting_location, Some(STUDY_ID));
    assert!(s.investigators.is_empty(), "setup() seats no one");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p scenarios setup_places_study_in_play_and_four_set_aside`
Expected: FAIL — `set_aside_locations` is empty.

- [ ] **Step 3: Build the five locations** in `setup()`. Assign stable `LocationId`s, wire connections, split into in-play / set-aside. Insert after the existing Study construction (which can be reused) and before the `GameStateBuilder`:

```rust
    // LocationIds for the Gathering board. Study is STUDY_ID (1); the
    // four set-aside locations get 2..=5. Connections are wired here
    // (scenario map knowledge — the corpus carries none) so they enter
    // play already connected when Act 1 advances.
    const HALLWAY_ID: LocationId = LocationId(2);
    const ATTIC_ID: LocationId = LocationId(3);
    const CELLAR_ID: LocationId = LocationId(4);
    const PARLOR_ID: LocationId = LocationId(5);

    let mut make = |id: LocationId, code: &str, name: &str| {
        let (shroud, clues) = location_stats(code);
        Location::new(id, CardCode(code.into()), name, shroud, clues)
    };
    let mut hallway = make(HALLWAY_ID, "01112", "Hallway");
    hallway.connections = vec![ATTIC_ID, CELLAR_ID, PARLOR_ID];
    let mut attic = make(ATTIC_ID, "01113", "Attic");
    attic.connections = vec![HALLWAY_ID];
    let mut cellar = make(CELLAR_ID, "01114", "Cellar");
    cellar.connections = vec![HALLWAY_ID];
    let mut parlor = make(PARLOR_ID, "01115", "Parlor");
    parlor.connections = vec![HALLWAY_ID];
```

After `state` is built and `starting_location` is set, attach the set-aside zone (matching how `act_deck` is assigned directly):

```rust
    state.set_aside_locations = vec![hallway, attic, cellar, parlor];
```

(Keep the Study built via `Location::new` and passed to `.with_location(study)` as today.)

- [ ] **Step 4: Update the module doc** — replace the "the Hallway/Attic/Cellar/Parlor are set aside and enter via the Act-1 'Door on the Floor' transition — C1b" sentence with: *"the Hallway/Attic/Cellar/Parlor are set aside (`set_aside_locations`) and enter play via Act 1's (01108) Forced on-advance reverse, which also moves investigators to the Hallway and removes the Study."*

- [ ] **Step 5: Run to verify it passes** (and the other setup tests still pass)

Run: `cargo test -p scenarios the_gathering`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/scenarios/src/the_gathering.rs
git commit -m "scenario: build the five-location Gathering board + set-aside split (#228)"
```

---

### Task 7: Integration test — Act-1 advance rebuilds the board

**Files:**
- Modify: `crates/scenarios/tests/the_gathering.rs` (real `cards::REGISTRY` installed)

Drive the real `AdvanceAct` action so `advance_act` fires 01108's on-advance reverse through the registry.

- [ ] **Step 1: Write the failing test.** Add to `crates/scenarios/tests/the_gathering.rs` (follow the file's existing setup + registry-install pattern; seat one investigator at the Study with 2 clues, set Investigation phase, then advance Act 1):

```rust
#[test]
fn advancing_act_1_rebuilds_the_board() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let mut state = scenarios::the_gathering::setup();

    // Seat one investigator at the Study with the 2 clues Act 1 needs.
    let inv = game_core::state::InvestigatorId(1);
    let mut investigator = game_core::test_support::test_investigator(1);
    investigator.current_location = Some(scenarios::the_gathering::STUDY_ID);
    investigator.clues = 2;
    state.investigators.insert(inv, investigator);
    state.turn_order = vec![inv];
    state.active_investigator = Some(inv);
    state.phase = game_core::state::Phase::Investigation;

    let result = game_core::engine::apply(
        state,
        game_core::action::Action::Player(
            game_core::action::PlayerAction::AdvanceAct { investigator: inv },
        ),
    );
    assert_eq!(result.outcome, game_core::engine::EngineOutcome::Done);

    // Board rebuilt: four locations in play, Study gone, set-aside empty.
    let codes: std::collections::BTreeSet<_> = result.state.locations.values()
        .map(|l| l.code.as_str().to_owned()).collect();
    assert_eq!(codes, ["01112", "01113", "01114", "01115"].into_iter().map(String::from).collect());
    assert!(result.state.set_aside_locations.is_empty());
    // Investigator relocated to the Hallway (01112).
    let hallway_id = result.state.locations.values().find(|l| l.code.as_str() == "01112").unwrap().id;
    assert_eq!(result.state.investigators[&inv].current_location, Some(hallway_id));
    // Act cursor moved to Act 2.
    assert_eq!(result.state.act_index, 1);
}
```

- [ ] **Step 2: Run to verify it fails** (then passes once the chain is wired):

Run: `cargo test -p scenarios --test the_gathering advancing_act_1_rebuilds_the_board`
Expected: PASS (all upstream tasks done). If FAIL, inspect which link (registry install, ability lookup, evaluator arm).

- [ ] **Step 3: Commit**

```bash
git add crates/scenarios/tests/the_gathering.rs
git commit -m "test: act-1 advance rebuilds the Gathering board end-to-end (#228)"
```

---

## Pillar 3 — Act-3 forced advance-on-defeat (Tasks 8–13)

### Task 8: `EventPattern::EnemyDefeated` enemy-code narrow (`Copy` → `Clone`)

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`EventPattern::EnemyDefeated` ~line 195; remove `Copy` from `EventPattern` and `Trigger` derives)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`push_matching` ~line 128–152)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (~line 101, 154)
- Modify: `crates/game-core/src/engine/dispatch/abilities.rs` (~line 191)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (~line 576)
- Modify: `crates/cards/src/impls/roland_banks.rs` (the `EnemyDefeated { by_controller: true }` constructor)
- Test: `crates/card-dsl/src/dsl.rs`

- [ ] **Step 1: Write the failing test** in `card-dsl`:

```rust
#[test]
fn enemy_defeated_carries_optional_code_narrow() {
    let any = EventPattern::EnemyDefeated { by_controller: false, code: None };
    let narrowed = EventPattern::EnemyDefeated { by_controller: false, code: Some("01116".into()) };
    assert_ne!(any, narrowed);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p card-dsl enemy_defeated_carries_optional_code_narrow`
Expected: FAIL — `EnemyDefeated` has no field `code`.

- [ ] **Step 3: Add the field + drop `Copy`.** Change the `EnemyDefeated` variant:

```rust
    EnemyDefeated {
        /// If `true`, only fires when the controller of this ability is
        /// credited with the defeat. If `false`, any defeat matches.
        by_controller: bool,
        /// Narrow the match to a specific defeated enemy printed code
        /// (e.g. the Ghoul Priest's `"01116"` for Act 3's objective).
        /// `None` matches any enemy's defeat (e.g. Roland's reaction).
        code: Option<String>,
    },
```

Remove `Copy` from the derives on **`enum EventPattern`** (line ~189) and **`enum Trigger`** (line ~68) — keep `Clone`. (`String` is not `Copy`, so the enums can no longer be `Copy`. `Ability` was already non-`Copy`.)

- [ ] **Step 4: Fix the by-value match sites** to take `&ability.trigger` references:

In `forced_triggers.rs` `push_matching` — change the signature and loop:

```rust
fn push_matching(
    reg: &card_registry::CardRegistry,
    code: &CardCode,
    controller: InvestigatorId,
    out: &mut Vec<ForcedHit>,
    want: impl Fn(&EventPattern) -> bool,
) {
    let Some(abilities) = (reg.abilities_for)(code) else {
        return;
    };
    for (idx, ability) in abilities.iter().enumerate() {
        if let Trigger::OnEvent { pattern, timing } = &ability.trigger {
            if *timing == EventTiming::After && want(pattern) {
                out.push(ForcedHit { code: code.clone(), ability_index: idx, controller });
            }
        }
    }
}
```

Update the two `collect_forced_hits` closures to take `&p` references (they become `|p| matches!(p, EventPattern::EnteredLocation)` where `p: &EventPattern` — `matches!` on a reference works as-is; for the `ActAdvanced` arm from Task 4, same).

In `reaction_windows.rs:101`, `abilities.rs:191`, `skill_test.rs:576` — change `= ability.trigger` to `= &ability.trigger` and adjust the bound sub-fields to deref where they were copied (e.g. `action_cost` becomes `&u8` → use `*action_cost`; `outcome` similarly). Compile-driven: fix each `cannot move out of` error by referencing/deref.

In `reaction_windows.rs:154`, update the `EnemyDefeated { by_controller }` match to `EnemyDefeated { by_controller, code: _ }` (or `..`) so it still compiles; the reaction-window matcher ignores the act-only `code` narrow.

- [ ] **Step 5: Fix the Roland constructor** in `roland_banks.rs`:

```rust
        EventPattern::EnemyDefeated { by_controller: true, code: None },
```

- [ ] **Step 6: Run the full suite** (compile-driven fixups until green):

Run: `RUSTFLAGS="-D warnings" cargo test --all && cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch/ crates/cards/src/impls/roland_banks.rs
git commit -m "card-dsl: EnemyDefeated enemy-code narrow; EventPattern Copy->Clone (#228)"
```

---

### Task 9: `Effect::AdvanceCurrentAct` + builder + evaluator arm

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`Effect` enum + builder)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`apply_effect` arm)
- Test: `crates/game-core/src/engine/evaluator.rs`

- [ ] **Step 1: Write the failing tests** in `evaluator.rs` (no registry needed — `AdvanceCurrentAct` reuses `advance_act`/`request_resolution`):

```rust
#[test]
fn advance_current_act_non_terminal_bumps_cursor() {
    use crate::scenario::Resolution;
    use crate::state::{Act, CardCode, InvestigatorId};
    use crate::test_support::GameStateBuilder;
    let mut state = GameStateBuilder::new().with_turn_order([InvestigatorId(1)]).build();
    state.act_deck = vec![
        Act { code: CardCode("a1".into()), clue_threshold: 0, resolution: None },
        Act { code: CardCode("a2".into()), clue_threshold: 0,
              resolution: Some(Resolution::Won { id: "R1".into() }) },
    ];
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let out = apply_effect(&mut cx, &Effect::AdvanceCurrentAct,
        EvalContext::for_controller(InvestigatorId(1)));
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(state.act_index, 1);
    assert!(state.resolution.is_none());
}

#[test]
fn advance_current_act_terminal_latches_resolution() {
    use crate::scenario::Resolution;
    use crate::state::{Act, CardCode, InvestigatorId};
    use crate::test_support::GameStateBuilder;
    let mut state = GameStateBuilder::new().with_turn_order([InvestigatorId(1)]).build();
    state.act_deck = vec![Act { code: CardCode("a1".into()), clue_threshold: 0,
        resolution: Some(Resolution::Won { id: "R1".into() }) }];
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let out = apply_effect(&mut cx, &Effect::AdvanceCurrentAct,
        EvalContext::for_controller(InvestigatorId(1)));
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(state.act_index, 0, "terminal act does not move the cursor");
    assert!(matches!(state.resolution, Some(Resolution::Won { .. })));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core -- advance_current_act_non_terminal advance_current_act_terminal`
Expected: FAIL — `AdvanceCurrentAct` not found.

- [ ] **Step 3: Add the DSL variant + builder** in `dsl.rs`:

```rust
    /// Advance the current act one step. If the act is terminal (carries
    /// a resolution) the scenario resolves; otherwise the cursor moves
    /// and the act's on-advance reverse fires. Used by act objectives
    /// like 01110 ("If the Ghoul Priest is Defeated, advance.").
    AdvanceCurrentAct,
```

```rust
/// Build an [`Effect::AdvanceCurrentAct`].
#[must_use]
pub fn advance_current_act() -> Effect {
    Effect::AdvanceCurrentAct
}
```

- [ ] **Step 4: Add the evaluator arm** in `apply_effect` (reusing the now-`pub(crate)` `advance_act` / `request_resolution` from Task 4):

```rust
        Effect::AdvanceCurrentAct => {
            use crate::engine::dispatch::act_agenda::{advance_act, request_resolution};
            if cx.state.act_deck.is_empty() {
                return EngineOutcome::Rejected {
                    reason: "AdvanceCurrentAct: no act deck is modeled".into(),
                };
            }
            match cx.state.act_deck[cx.state.act_index].resolution.clone() {
                Some(resolution) => request_resolution(cx.state, resolution),
                None => advance_act(cx),
            }
            EngineOutcome::Done
        }
```

(If `crate::engine::dispatch::act_agenda` is not importable at that path, add a `pub(crate) use` re-export in `engine/dispatch/mod.rs` or call via the existing `super::dispatch` path — match the crate's module conventions.)

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p game-core -- advance_current_act_non_terminal advance_current_act_terminal`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/
git commit -m "engine: Effect::AdvanceCurrentAct (#228)"
```

---

### Task 10: `Enemy.code` + `ForcedTriggerPoint::EnemyDefeated` + fire from the defeat path

The `Enemy` struct does **not** carry its printed code today (only `id`, `name`, stats, location, traits). The act-3 objective routes on the *defeated enemy's code*, and `Event::EnemyDefeated` carries only the `EnemyId` — so the enemy must carry its code, captured before removal. Part A adds it; Part B wires the dispatch.

**Files:**
- Modify: `crates/game-core/src/state/enemy.rs` (struct `Enemy` + the `#[cfg(test)]` if present)
- Modify: `crates/game-core/src/test_support/fixtures.rs` (`test_enemy` sets a default code)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (`spawn_enemy` sets `code` from `metadata.code` ~line 308)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (variant + `collect_forced_hits` arm)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`damage_enemy` defeat branch ~line 28)
- Modify: `crates/game-core/src/test_support/mod.rs` (`fire_forced_on_enemy_defeat` helper)
- Test: `crates/game-core/src/state/enemy.rs`, `crates/game-core/src/engine/dispatch/combat.rs`

**Part A — add `code` to `Enemy`:**

- [ ] **Step A1: Write the failing test** in `enemy.rs`'s test module:

```rust
#[test]
fn test_enemy_fixture_carries_a_code() {
    let e = crate::test_support::test_enemy(7, "Ghoul");
    assert!(!e.code.as_str().is_empty(), "every enemy carries its printed code");
}
```

- [ ] **Step A2: Run to verify it fails**

Run: `cargo test -p game-core test_enemy_fixture_carries_a_code`
Expected: FAIL — `no field code on Enemy`.

- [ ] **Step A3: Add the field + set it at the two construction sites.** In `enemy.rs`, add to `struct Enemy` (after `name`):

```rust
    /// Printed `ArkhamDB` code (e.g. `"01116"` for the Ghoul Priest).
    /// Carried so framework effects keyed on a specific enemy — Act 3's
    /// "If the Ghoul Priest is Defeated, advance." — can match after the
    /// enemy leaves `state.enemies`.
    pub code: CardCode,
```

(Add `use crate::state::CardCode;` / `super::CardCode` as the module needs.)

In `test_support/fixtures.rs` `test_enemy`, set a synthetic default so existing callers don't change:

```rust
        code: CardCode::new(format!("_test_enemy_{id}")),
```

In `encounter.rs` `spawn_enemy`'s `Enemy { … }` literal, set the real code from metadata:

```rust
        code: CardCode::new(metadata.code.clone()),
```

- [ ] **Step A4: Run to verify it passes** (and nothing else broke)

Run: `cargo test -p game-core test_enemy_fixture_carries_a_code && RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step A5: Commit**

```bash
git add crates/game-core/src/state/enemy.rs crates/game-core/src/test_support/fixtures.rs crates/game-core/src/engine/dispatch/encounter.rs
git commit -m "engine: Enemy carries its printed code (#228)"
```

**Part B — EnemyDefeated forced dispatch:**

- [ ] **Step 1: Write the failing regression test** in `combat.rs` — defeating an enemy with no registry still works (forced fire is a no-op):

```rust
#[test]
fn defeating_enemy_without_registry_still_removes_it() {
    use crate::state::{EnemyId, InvestigatorId};
    use crate::test_support::{test_enemy, GameStateBuilder};
    let eid = EnemyId(1);
    let mut enemy = test_enemy(1, "Ghoul"); // max_health small; see fixtures
    enemy.max_health = 1;
    let mut state = GameStateBuilder::new().build();
    state.enemies.insert(eid, enemy);
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    super::combat::damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));
    assert!(!state.enemies.contains_key(&eid), "defeated enemy removed");
}
```

(Adjust `test_enemy` usage to the fixture's actual signature — see `crates/game-core/src/test_support/fixtures.rs`.)

- [ ] **Step 2: Run to verify it passes** (regression — current behavior):

Run: `cargo test -p game-core defeating_enemy_without_registry_still_removes_it`
Expected: PASS.

- [ ] **Step 3: Add the `ForcedTriggerPoint` variant** in `forced_triggers.rs`:

```rust
    /// An enemy was defeated. Scans the *current act* for
    /// `EventPattern::EnemyDefeated` forced abilities whose `code`
    /// narrow matches (or is `None`); binds controller = the lead
    /// investigator. The act-3 objective (01110) advances on the Ghoul
    /// Priest's defeat through this point.
    EnemyDefeated {
        /// Printed code of the defeated enemy (for `code`-narrow matching).
        code: CardCode,
    },
```

Add the `collect_forced_hits` arm (scan the current act only — no agenda consumes this yet):

```rust
        ForcedTriggerPoint::EnemyDefeated { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            if let Some(act) = state.act_deck.get(state.act_index) {
                let defeated = code.as_str().to_owned();
                push_matching(reg, &act.code, lead, &mut hits, move |p| {
                    matches!(
                        p,
                        EventPattern::EnemyDefeated { code: narrow, .. }
                            if narrow.as_deref().is_none_or(|c| c == defeated)
                    )
                });
            }
        }
```

(If the toolchain predates `Option::is_none_or`, use `narrow.as_deref().map_or(true, |c| c == defeated)`.)

- [ ] **Step 4: Fire it from `damage_enemy`** — rewrite the defeat branch (`if new_damage >= enemy.max_health`) to capture the enemy's `code` *before* removal (the `Enemy` is dropped from `state.enemies` in this branch), then fire after queueing the reaction window:

```rust
    if new_damage >= enemy.max_health {
        let defeated_code = enemy.code.clone(); // capture before the enemy is removed
        cx.events.push(Event::EnemyDefeated { enemy: enemy_id, by });
        cx.state.enemies.remove(&enemy_id);
        super::reaction_windows::queue_reaction_window(
            cx,
            WindowKind::AfterEnemyDefeated { enemy: enemy_id, by },
        );
        // Forced act objectives keyed to this defeat (Act 3's "If the
        // Ghoul Priest is Defeated, advance."). `()` return can't
        // propagate a 2+-trigger reject; debug_assert guards it (mirror of
        // upkeep_phase_end / advance_act). Ordering vs. the
        // AfterEnemyDefeated reaction window is fixed-deterministic for
        // now; #212/#213 revisit.
        let forced = super::forced_triggers::fire_forced_triggers(
            cx,
            super::forced_triggers::ForcedTriggerPoint::EnemyDefeated { code: defeated_code },
        );
        debug_assert!(
            matches!(forced, crate::engine::EngineOutcome::Done),
            "EnemyDefeated forced did not resolve to Done: {forced:?} (2+ needs #213)"
        );
    }
```

- [ ] **Step 5: Add the test helper** in `test_support/mod.rs`:

```rust
/// Test helper: fire forced triggers for an enemy defeat, returning the
/// `EngineOutcome`. See `fire_forced_on_enter`.
pub fn fire_forced_on_enemy_defeat(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        crate::engine::ForcedTriggerPoint::EnemyDefeated { code },
    )
}
```

- [ ] **Step 6: Run the regression + clippy**

Run: `cargo test -p game-core defeating_enemy_without_registry_still_removes_it && cargo clippy -p game-core --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/combat.rs crates/game-core/src/test_support/mod.rs
git commit -m "engine: fire EnemyDefeated forced trigger from the defeat path (#228)"
```

---

### Task 11: `01110` abilities impl + registry registration

**Files:**
- Create: `crates/cards/src/impls/act_01110.rs`
- Modify: `crates/cards/src/impls/mod.rs`
- Test: in `act_01110.rs`

01110 front (verbatim): *"**Objective** – If the Ghoul Priest is Defeated, advance."* — forced (no "may"); the Ghoul Priest is **01116**.

- [ ] **Step 1: Write the failing test:**

```rust
#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger};

    #[test]
    fn abilities_advance_on_ghoul_priest_defeat() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyDefeated { by_controller: false, code: Some("01116".into()) },
                timing: EventTiming::After,
            }
        );
        assert!(matches!(abilities[0].effect, Effect::AdvanceCurrentAct));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cards abilities_advance_on_ghoul_priest_defeat`
Expected: FAIL — module not declared.

- [ ] **Step 3: Write the impl:**

```rust
//! What Have You Done? (The Gathering Act 3, 01110).
//!
//! ```text
//! Act 3 — What Have You Done?
//! Objective – If the Ghoul Priest is Defeated, advance.
//! ```
//!
//! Forced (no "may" — Rules Reference p.3; the bare "advance" with no
//! clue threshold cannot be the optional clue-spend ability): the act
//! advances the instant the Ghoul Priest (01116) is defeated, firing
//! its terminal Won/R1 resolution. Wired via `ForcedTriggerPoint::
//! EnemyDefeated` from the defeat path; narrowed to 01116 so other
//! ghouls' defeats don't advance it.
//!
//! Act-3's *reverse* (the R1/R2 resolution choice) is deferred to
//! Phase 9 (campaign log gives the branch meaning); the scenario keeps
//! a single Won/R1 latch. The Ghoul Priest enemy + its spawn land in
//! C3 (#231); this objective is unit-tested here and proven end-to-end
//! in C7b (#245).

use card_dsl::dsl::{advance_current_act, on_event, Ability, EventPattern, EventTiming};

/// `ArkhamDB` code for Act 3, "What Have You Done?".
pub const CODE: &str = "01110";

/// 01110's Forced objective: advance when the Ghoul Priest is defeated.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::EnemyDefeated { by_controller: false, code: Some("01116".to_owned()) },
        EventTiming::After,
        advance_current_act(),
    )]
}
```

- [ ] **Step 4: Register it** in `impls/mod.rs` (`pub mod act_01110;`, match arm, doc bullet).

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p cards abilities_advance_on_ghoul_priest_defeat`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/cards/src/impls/act_01110.rs crates/cards/src/impls/mod.rs
git commit -m "cards: 01110 What Have You Done? advance-on-defeat objective (#228)"
```

---

### Task 12: `the_gathering.rs` — real 01110 objective (drop the placeholder)

**Files:**
- Modify: `crates/scenarios/src/the_gathering.rs` (`setup()` act_deck ~line 137; `act_clue_threshold` helper; module doc; setup tests)

- [ ] **Step 1: Update the failing setup test** — 01110 no longer has a placeholder threshold; pin its terminal Won + threshold 0:

```rust
#[test]
fn act_three_advances_on_objective_not_clues() {
    let s = setup();
    assert_eq!(s.act_deck[2].code.as_str(), "01110");
    assert_eq!(s.act_deck[2].clue_threshold, 0, "01110 advances on Ghoul-Priest-defeat, not clues");
    assert!(matches!(s.act_deck[2].resolution, Some(Resolution::Won { .. })));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p scenarios act_three_advances_on_objective_not_clues`
Expected: FAIL — placeholder threshold is 2.

- [ ] **Step 3: Drop the placeholder.** In `setup()`'s `act_deck`, change the 01110 entry to `clue_threshold: 0` (objective-driven, no clue cost):

```rust
        Act {
            code: CardCode("01110".into()),
            clue_threshold: 0, // advances via 01110's EnemyDefeated objective (cards), not clues
            resolution: Some(Resolution::Won { id: "R1".into() }),
        },
```

Remove the now-unused `placeholder` path: simplify `act_clue_threshold` to drop its `placeholder` parameter (every act now reads a real value — 01108→2, 01109→3, 01110 has `clues: null` so map `None`→`0`). Update its two other call sites (01108, 01109) accordingly:

```rust
/// Read an act's printed clue threshold from the corpus. Acts that
/// advance on a non-clue objective (01110) carry `null` clues → 0.
fn act_clue_threshold(code: &str) -> u8 {
    match cards::by_code(code).expect("act code in corpus").kind {
        CardKind::Act { clue_threshold, .. } => clue_threshold.unwrap_or(0),
        ref k => panic!("{code} is not an Act ({k:?})"),
    }
}
```

Update the module doc: remove the "act 01110's clue threshold is a placeholder" sentence; replace with "act 01110 advances via its Forced EnemyDefeated objective (01116; in `cards`). Its R1/R2 resolution choice is Phase-9 — `TODO`."

Add a `// TODO(#phase-9): 01110's reverse is the lead investigator's R1/R2 choice…` comment at the 01110 act entry, and a `// TODO(#231): the Ghoul Priest (01116) spawns at Act-2 advance` near the act_deck.

- [ ] **Step 4: Run the scenario tests** (the existing `setup_reads_card_stats_from_corpus` etc. — update any that asserted the old placeholder/`act_clue_threshold` signature):

Run: `cargo test -p scenarios the_gathering`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/scenarios/src/the_gathering.rs
git commit -m "scenario: real 01110 objective; drop placeholder threshold (#228)"
```

---

### Task 13: Integration test — defeating the Ghoul Priest advances Act 3 to Won

**Files:**
- Create: `crates/cards/tests/act_advancement.rs` (own process; installs `cards::REGISTRY`)

Uses the `fire_forced_on_enemy_defeat` helper + the real 01110 ability + a real corpus 01116 (it carries metadata since #252), proving dispatch → ability → `AdvanceCurrentAct` → Won.

- [ ] **Step 1: Write the test:**

```rust
//! Act-3 objective: defeating the Ghoul Priest (01116) advances Act 3
//! (01110) to its terminal Won resolution. The Ghoul Priest enemy +
//! spawn land in C3 (#231); here we drive the forced dispatch directly
//! with the real registry. End-to-end defeat→Won via a real Fight is
//! C7b (#245).

use game_core::scenario::Resolution;
use game_core::state::{Act, CardCode, InvestigatorId};

#[test]
fn defeating_ghoul_priest_advances_act_3_to_won() {
    let _ = game_core::card_registry::install(cards::REGISTRY);

    let inv = InvestigatorId(1);
    let mut state = game_core::test_support::GameStateBuilder::new()
        .with_turn_order([inv])
        .build();
    // Act 3 is current and terminal-Won (mirrors the_gathering setup()).
    state.act_deck = vec![Act {
        code: CardCode("01110".into()),
        clue_threshold: 0,
        resolution: Some(Resolution::Won { id: "R1".into() }),
    }];

    let mut events = Vec::new();
    let out = game_core::test_support::fire_forced_on_enemy_defeat(
        &mut state,
        &mut events,
        CardCode("01116".into()), // the Ghoul Priest
    );
    assert_eq!(out, game_core::engine::EngineOutcome::Done);
    assert!(matches!(state.resolution, Some(Resolution::Won { .. })));
}

#[test]
fn defeating_other_enemy_does_not_advance_act_3() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let inv = InvestigatorId(1);
    let mut state = game_core::test_support::GameStateBuilder::new()
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![Act {
        code: CardCode("01110".into()),
        clue_threshold: 0,
        resolution: Some(Resolution::Won { id: "R1".into() }),
    }];
    let mut events = Vec::new();
    let out = game_core::test_support::fire_forced_on_enemy_defeat(
        &mut state,
        &mut events,
        CardCode("01103".into()), // some other enemy, not the Ghoul Priest
    );
    assert_eq!(out, game_core::engine::EngineOutcome::Done);
    assert!(state.resolution.is_none(), "only the Ghoul Priest's defeat advances Act 3");
}
```

- [ ] **Step 2: Run to verify they pass**

Run: `cargo test -p cards --test act_advancement`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add crates/cards/tests/act_advancement.rs
git commit -m "test: defeating the Ghoul Priest advances Act 3 to Won (#228)"
```

---

## Final: full CI gauntlet + phase doc

- [ ] **Run the complete gauntlet** (all jobs, strict flags):

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```
Expected: all green.

- [ ] **Phase doc** (`docs/phases/phase-7-the-gathering.md`) is updated **as the final commit, only once the PR is green** (per the repo's PR procedure — not now): move C1b (#228) to the Closed table / flip its Arc row to `✅ PR #N`; add a **Decisions made** entry for the Pillar-2→C3c re-scope (#232) and the act-objective forced-vs-optional split; note new follow-ups #257, #258. Do **not** make this edit in an earlier task.

---

## Self-Review notes (author)

- **Spec coverage:** Pillar 1 → Tasks 1–7; Pillar 3 → Tasks 8–13. Act-2 (Pillar 2) is explicitly out of scope (C3c/#232) — no task, by design. Deferral TODOs (#231, #257, #258, Phase 9) land in Tasks 5/6/11/12 doc-comments.
- **Type consistency:** card-dsl codes are `String` (not `CardCode`); `Event::InvestigatorMoved.from` is a non-optional `LocationId` (Task 3 emits only when `from` is `Some`); `EventPattern`/`Trigger` lose `Copy` in Task 8 (match sites switch to `&ability.trigger`). `advance_act`/`request_resolution` bumped to `pub(crate)` in Task 4 for reuse in Task 9. **`Enemy` gains a `code: CardCode` field** in Task 10 Part A (it had none) — set in `test_enemy` (synthetic) and `spawn_enemy` (`metadata.code`); the defeat branch captures it before removal.
- **Ordering:** Task 8 (drop `Copy`) precedes Task 11 (01110 uses the `code` field). Task 4's visibility bump precedes Task 9's reuse. Task 6 (set-aside build) precedes Task 7 (integration). All forced-dispatch behavior is exercised through `test_support` helpers + real-registry integration tests, mirroring the existing `fire_forced_on_enter` convention.
