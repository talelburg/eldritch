# C1a — The Gathering `setup()` Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a real `the-gathering` `ScenarioModule` whose `setup()` builds the faithful Act-1 board (Study only), act/agenda decks, and the verified Standard chaos bag; add a `starting_location` placement channel; and deliver the Attic/Cellar forced-on-enter card abilities.

**Architecture:** `setup()` builds the *world* (no investigators); the `StartScenario` roster-seating step places investigators at `GameState.starting_location`. Scenario-structure card behavior comes from the card registry (`abilities_for`), so Attic/Cellar are hand-written `cards` impls. Faithful Study-only start — the four set-aside locations + the Door-on-the-Floor transition are C1b.

**Tech Stack:** Rust workspace (`card-dsl`, `game-core`, `cards`, `scenarios`). Tests via `cargo test`; event-assertion macros (`assert_event!`); the `TestGame` builder; `fire_forced_on_enter` test helper.

**Spec:** `docs/superpowers/specs/2026-06-11-phase-7-slice-1-c1a-gathering-setup-design.md`
**Depends on:** #248 (`Controller → You` rename) — already merged on `main`; this branch is rebased on it.

---

## File Structure

- `data/campaign-guides/night_of_the_zealot_campaign_guide.pdf` — **vendored** (done) + `SOURCE.md` (done). Provenance for the bag.
- `crates/game-core/src/state/game_state.rs` — **modify**: add `starting_location: Option<LocationId>` field.
- `crates/game-core/src/test_support/builder.rs` — **modify**: init `starting_location` in `build()`.
- `crates/game-core/src/engine/dispatch/phases.rs` — **modify**: `start_scenario` seats investigators at `starting_location`.
- `crates/cards/src/impls/attic.rs` — **create**: Attic (01113) forced 1 horror.
- `crates/cards/src/impls/cellar.rs` — **create**: Cellar (01114) forced 1 damage.
- `crates/cards/src/impls/mod.rs` — **modify**: register attic + cellar.
- `crates/scenarios/src/the_gathering.rs` — **create**: `setup()`, `standard_chaos_bag()`, `apply_resolution`, `MODULE`, `ID`, `STUDY_ID`.
- `crates/scenarios/src/lib.rs` — **modify**: un-gate `module_for`/`REGISTRY`, add the-gathering arm.
- `crates/scenarios/tests/the_gathering.rs` — **create**: integration test (placement, faithful bag, Won resolution, Attic/Cellar forced effects).

---

## Task 0: Vendor the campaign guide

**Files:**
- Create: `data/campaign-guides/night_of_the_zealot_campaign_guide.pdf` (already downloaded)
- Create: `data/campaign-guides/SOURCE.md` (already written)

- [ ] **Step 1: Verify the vendored files exist**

Run: `ls -la data/campaign-guides/ && file data/campaign-guides/night_of_the_zealot_campaign_guide.pdf`
Expected: the PDF (~2.4 MB, "PDF document … 8 page(s)") and `SOURCE.md` are present.

- [ ] **Step 2: Commit**

```bash
git add data/campaign-guides/
git commit -m "data: vendor Night of the Zealot campaign guide (chaos-bag source)"
```

---

## Task 1: `GameState.starting_location` field

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (struct + a serde test)
- Modify: `crates/game-core/src/test_support/builder.rs:266` (`build()`)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` block in `crates/game-core/src/state/game_state.rs`:

```rust
#[test]
fn game_state_starting_location_defaults_to_none_and_roundtrips() {
    use crate::test_support::TestGame;
    let mut state = TestGame::new().build();
    assert_eq!(state.starting_location, None, "default must be None");

    state.starting_location = Some(crate::state::LocationId(7));
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.starting_location, Some(crate::state::LocationId(7)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core game_state_starting_location_defaults_to_none_and_roundtrips`
Expected: FAIL — compile error, `GameState` has no field `starting_location`.

- [ ] **Step 3: Add the field**

In `crates/game-core/src/state/game_state.rs`, add to `pub struct GameState` (place it next to `locations`):

```rust
    /// Where roster-seated investigators are placed at scenario start.
    /// `setup()` sets it (e.g. The Gathering -> the Study); the
    /// `StartScenario` seating step reads it. `None` leaves seated
    /// investigators unplaced (`current_location: None`) — the legacy
    /// pre-seated test path, where `setup()` already placed them.
    pub starting_location: Option<LocationId>,
```

In `crates/game-core/src/test_support/builder.rs`, inside `build()`'s `GameState { … }` literal, add:

```rust
            starting_location: None,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p game-core game_state_starting_location_defaults_to_none_and_roundtrips`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/test_support/builder.rs
git commit -m "engine: add GameState.starting_location placement channel"
```

---

## Task 2: Roster seating places investigators at `starting_location`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs:107-130` (seating loop)

Placement behavior is verified end-to-end in **Task 7** (the scenarios integration test, where a real card registry + Roland exist). A game-core unit test can't seat a *real* investigator — seating resolves stats via `card_registry`, which game-core can't populate with the corpus. So this task ships the code change; Task 7's `roster_seating_places_at_study` assertion is its regression guard.

- [ ] **Step 1: Make the change**

In `crates/game-core/src/engine/dispatch/phases.rs`, just before the seating `for` loop (around line 107), capture the start location, and use it for each seated investigator:

```rust
    // Seated investigators start at the scenario's starting location
    // (set by setup()). None leaves them unplaced — the pre-seated path.
    let start = cx.state.starting_location;
    for (idx, (skills, health, sanity, name, deck)) in resolved.into_iter().enumerate() {
        let id = InvestigatorId(u32::try_from(idx).unwrap_or(0) + 1);
        cx.state.investigators.insert(
            id,
            Investigator {
                id,
                name,
                current_location: start,
```

(Only the `current_location` line changes — from `None` to `start`. Leave the rest of the `Investigator { … }` literal untouched.)

- [ ] **Step 2: Verify the workspace still compiles and existing tests pass**

Run: `cargo test -p game-core start_scenario`
Expected: PASS — existing `start_scenario_*` tests use the pre-seated/empty-roster paths (`starting_location` defaults `None`), so behavior is unchanged for them.

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/engine/dispatch/phases.rs
git commit -m "engine: seat roster investigators at GameState.starting_location"
```

---

## Task 3: Attic card impl (01113) — forced 1 horror

**Card text** (verified `data/arkhamdb-snapshot/pack/core/core_encounter.json`, 01113): *"**Forced** – After you enter the Attic: Take 1 horror."*

**Files:**
- Create: `crates/cards/src/impls/attic.rs`
- Modify: `crates/cards/src/impls/mod.rs`

- [ ] **Step 1: Write the impl with its failing test**

Create `crates/cards/src/impls/attic.rs`:

```rust
//! Attic (The Gathering location, 01113).
//!
//! ```text
//! Shroud: 1. Clues: 2. Victory 1.
//! Forced - After you enter the Attic: Take 1 horror.
//! ```
//!
//! Forced-on-enter via the `EnteredLocation` dispatch path
//! (`engine::dispatch::forced_triggers`); the controller binding is the
//! entering investigator ("you"). The Victory 1 and Clues 2 are location
//! *state* set by the scenario's `setup()`, not ability data — only the
//! Forced horror lives here.

use card_dsl::dsl::{
    deal_horror, on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
};

/// `ArkhamDB` code for the Attic.
pub const CODE: &str = "01113";

/// The Attic's Forced "after you enter: take 1 horror".
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::EnteredLocation,
        EventTiming::After,
        deal_horror(InvestigatorTarget::You, 1),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, InvestigatorTarget, Trigger};

    #[test]
    fn abilities_are_one_forced_enter_horror() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnteredLocation,
                timing: EventTiming::After,
            }
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::DealHorror {
                target: InvestigatorTarget::You,
                amount: 1,
            }
        ));
    }
}
```

- [ ] **Step 2: Register it**

In `crates/cards/src/impls/mod.rs`, add `pub mod attic;` (alphabetical, before `deduction`) and the match arm in `abilities_for`:

```rust
        attic::CODE => Some(attic::abilities()),
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p cards attic`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/cards/src/impls/attic.rs crates/cards/src/impls/mod.rs
git commit -m "card: Attic (01113) forced-on-enter 1 horror"
```

---

## Task 4: Cellar card impl (01114) — forced 1 damage

**Card text** (verified 01114): *"**Forced** – After you enter the Cellar: Take 1 damage."*

**Files:**
- Create: `crates/cards/src/impls/cellar.rs`
- Modify: `crates/cards/src/impls/mod.rs`

- [ ] **Step 1: Write the impl with its failing test**

Create `crates/cards/src/impls/cellar.rs`:

```rust
//! Cellar (The Gathering location, 01114).
//!
//! ```text
//! Shroud: 4. Clues: 2. Victory 1.
//! Forced - After you enter the Cellar: Take 1 damage.
//! ```
//!
//! Forced-on-enter via the `EnteredLocation` dispatch path; the
//! controller binding is the entering investigator ("you"). Shroud /
//! Clues / Victory are location state set by `setup()`.

use card_dsl::dsl::{
    deal_damage, on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
};

/// `ArkhamDB` code for the Cellar.
pub const CODE: &str = "01114";

/// The Cellar's Forced "after you enter: take 1 damage".
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::EnteredLocation,
        EventTiming::After,
        deal_damage(InvestigatorTarget::You, 1),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, InvestigatorTarget, Trigger};

    #[test]
    fn abilities_are_one_forced_enter_damage() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnteredLocation,
                timing: EventTiming::After,
            }
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::DealDamage {
                target: InvestigatorTarget::You,
                amount: 1,
            }
        ));
    }
}
```

- [ ] **Step 2: Register it**

In `crates/cards/src/impls/mod.rs`, add `pub mod cellar;` (after `attic`) and the arm:

```rust
        cellar::CODE => Some(cellar::abilities()),
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p cards cellar`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/cards/src/impls/cellar.rs crates/cards/src/impls/mod.rs
git commit -m "card: Cellar (01114) forced-on-enter 1 damage"
```

---

## Task 5: `the_gathering` scenario module

**Card data** (verified snapshot): Study 01111 (shroud 2, clues 2); acts 01108/01109/01110 (clues 2/3/null); agendas 01105/01106/01107 (doom 3/7/10). Bag verified against the campaign guide (Task 0 SOURCE.md).

**Files:**
- Create: `crates/scenarios/src/the_gathering.rs`
- Modify: `crates/scenarios/src/lib.rs` (add `pub mod the_gathering;`)

- [ ] **Step 1: Write the module with unit tests**

Create `crates/scenarios/src/the_gathering.rs`:

```rust
//! The Gathering (Night of the Zealot, scenario 1) — Slice 1 C1a skeleton.
//!
//! Builds the faithful **Act-1 board**: only the Study is in play (the
//! Hallway/Attic/Cellar/Parlor are set aside and enter via the Act-1
//! "Door on the Floor" transition — C1b). `setup()` builds the world;
//! the `StartScenario` roster step seats investigators at
//! [`STUDY_ID`] via `GameState.starting_location`.
//!
//! Faithful where it can be (agenda doom 3/7/10; the verified Standard
//! chaos bag; Study shroud/clues); structural stand-in where the rest of
//! Group C owns fidelity (act 01110's clue threshold is a placeholder —
//! its real "Ghoul Priest defeated" objective is C1b; symbol-token
//! effects on reference card 01104 are C2). C1a does not claim faithful
//! win/lose semantics — only structural reachability, proven by
//! `tests/the_gathering.rs`.

use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId, ScenarioModule};
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, LocationId, TokenModifiers,
};
use game_core::test_support::{test_location, TestGame};

/// String id used to look this module up in [`crate::REGISTRY`].
pub const ID: &str = "the-gathering";

/// `ArkhamDB` reference-card code (chaos-symbol effects; evaluated in C2).
pub const REFERENCE_CARD: &str = "01104";

/// The Study's [`LocationId`] — the scenario's starting location.
pub const STUDY_ID: LocationId = LocationId(1);

/// The verified Standard-difficulty Night of the Zealot chaos bag (16
/// tokens). Source: `data/campaign-guides/SOURCE.md` (campaign guide
/// p.1, "Assemble the campaign chaos bag", Standard).
fn standard_chaos_bag() -> ChaosBag {
    use ChaosToken::{AutoFail, Cultist, ElderSign, Numeric, Skull, Tablet};
    ChaosBag::new([
        Numeric(1),
        Numeric(0),
        Numeric(0),
        Numeric(-1),
        Numeric(-1),
        Numeric(-1),
        Numeric(-2),
        Numeric(-2),
        Numeric(-3),
        Numeric(-4),
        Skull,
        Skull,
        Cultist,
        Tablet,
        AutoFail,
        ElderSign,
    ])
}

/// Build the initial [`GameState`]: the Study in play (isolated), the
/// act/agenda decks, the Standard chaos bag, and `starting_location`.
/// No investigators — the `StartScenario` roster step seats them.
pub fn setup() -> GameState {
    // The Study (01111): shroud 2, clues 2, revealed, no connections
    // (Act 1 is "trapped in the Study"). `test_location` is the only
    // cross-crate GameState/Location constructor (both are
    // `#[non_exhaustive]`); we override its fields, exactly as the
    // synthetic fixture does.
    let mut study = test_location(STUDY_ID.0, "Study");
    study.code = CardCode("01111".into());
    study.shroud = 2;
    study.clues = 2;
    study.revealed = true;
    study.connections = Vec::new();

    let mut state = TestGame::new()
        .with_location(study)
        .with_chaos_bag(standard_chaos_bag())
        .with_scenario_id(ScenarioId::new(ID))
        .build();

    state.starting_location = Some(STUDY_ID);

    // The Gathering's symbol effects are printed on reference card 01104
    // (board-dependent; evaluated in C2). Until then these flat NotZ
    // fallbacks stand in; they are off the C1a structural test path.
    state.token_modifiers = TokenModifiers {
        skull: -1,
        cultist: -2,
        tablet: -3,
        elder_thing: -4,
    };

    // Act deck 01108 -> 01109 -> 01110. Clue thresholds 2/3 are the real
    // printed values; 01110's is a placeholder (its real "Ghoul Priest
    // defeated" objective is C1b). The terminal act carries the Won latch.
    state.act_deck = vec![
        Act {
            code: CardCode("01108".into()),
            clue_threshold: 2,
            resolution: None,
        },
        Act {
            code: CardCode("01109".into()),
            clue_threshold: 3,
            resolution: None,
        },
        Act {
            code: CardCode("01110".into()),
            clue_threshold: 2, // placeholder; real objective is C1b
            resolution: Some(Resolution::Won { id: "R1".into() }),
        },
    ];

    // Agenda deck 01105 -> 01106 -> 01107. Doom thresholds 3/7/10 are the
    // real printed values. The terminal agenda carries the Lost latch.
    state.agenda_deck = vec![
        Agenda {
            code: CardCode("01105".into()),
            doom_threshold: 3,
            resolution: None,
        },
        Agenda {
            code: CardCode("01106".into()),
            doom_threshold: 7,
            resolution: None,
        },
        Agenda {
            code: CardCode("01107".into()),
            doom_threshold: 10,
            resolution: Some(Resolution::Lost {
                reason: "The ghouls break free".into(),
            }),
        },
    ];

    state
}

/// No-op for C1a (matches the synthetic fixture). XP / trauma / campaign
/// log application is Phase 9.
pub fn apply_resolution(
    _resolution: &Resolution,
    _state: &mut GameState,
    _events: &mut Vec<Event>,
) {
}

/// The [`ScenarioModule`] value for The Gathering.
pub const MODULE: ScenarioModule = ScenarioModule {
    reference_card: REFERENCE_CARD,
    setup,
    apply_resolution,
};

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::ChaosToken;

    #[test]
    fn setup_places_only_the_isolated_study() {
        let s = setup();
        assert_eq!(s.locations.len(), 1, "Act-1 board is the Study only");
        let study = s.locations.get(&STUDY_ID).expect("Study present");
        assert_eq!(study.code, CardCode("01111".into()));
        assert_eq!(study.shroud, 2);
        assert_eq!(study.clues, 2);
        assert!(study.revealed);
        assert!(study.connections.is_empty(), "Study is isolated in Act 1");
        assert_eq!(s.starting_location, Some(STUDY_ID));
        assert_eq!(s.scenario_id, Some(ScenarioId::new(ID)));
        assert!(s.investigators.is_empty(), "setup() seats no one");
    }

    #[test]
    fn setup_seeds_act_and_agenda_decks_with_terminal_latches() {
        let s = setup();
        let act_codes: Vec<_> = s.act_deck.iter().map(|a| a.code.as_str()).collect();
        assert_eq!(act_codes, ["01108", "01109", "01110"]);
        assert_eq!(s.act_deck[0].clue_threshold, 2);
        assert_eq!(s.act_deck[1].clue_threshold, 3);
        assert!(matches!(
            s.act_deck[2].resolution,
            Some(Resolution::Won { .. })
        ));

        let agenda_codes: Vec<_> = s.agenda_deck.iter().map(|a| a.code.as_str()).collect();
        assert_eq!(agenda_codes, ["01105", "01106", "01107"]);
        assert_eq!(
            s.agenda_deck.iter().map(|a| a.doom_threshold).collect::<Vec<_>>(),
            [3, 7, 10]
        );
        assert!(matches!(
            s.agenda_deck[2].resolution,
            Some(Resolution::Lost { .. })
        ));
    }

    #[test]
    fn setup_seeds_verified_standard_chaos_bag() {
        let s = setup();
        let mut tokens = s.chaos_bag.tokens.clone();
        let mut expected = vec![
            ChaosToken::Numeric(1),
            ChaosToken::Numeric(0),
            ChaosToken::Numeric(0),
            ChaosToken::Numeric(-1),
            ChaosToken::Numeric(-1),
            ChaosToken::Numeric(-1),
            ChaosToken::Numeric(-2),
            ChaosToken::Numeric(-2),
            ChaosToken::Numeric(-3),
            ChaosToken::Numeric(-4),
            ChaosToken::Skull,
            ChaosToken::Skull,
            ChaosToken::Cultist,
            ChaosToken::Tablet,
            ChaosToken::AutoFail,
            ChaosToken::ElderSign,
        ];
        // Order is not significant in the bag; compare as multisets.
        tokens.sort_by_key(|t| format!("{t:?}"));
        expected.sort_by_key(|t| format!("{t:?}"));
        assert_eq!(tokens, expected, "Standard NotZ bag is 16 tokens");
    }
}
```

- [ ] **Step 2: Declare the module**

In `crates/scenarios/src/lib.rs`, add near the top (unconditional, *not* behind `test_fixtures`):

```rust
pub mod the_gathering;
```

- [ ] **Step 3: Run the unit tests**

Run: `cargo test -p scenarios the_gathering::tests`
Expected: PASS (all three).

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/src/the_gathering.rs crates/scenarios/src/lib.rs
git commit -m "scenario: the-gathering setup() skeleton (Study board, decks, Standard bag)"
```

---

## Task 6: Registry wiring — un-gate `module_for` / `REGISTRY`

**Files:**
- Modify: `crates/scenarios/src/lib.rs`

The current `module_for`/`REGISTRY` are gated behind `test_fixtures`. Make them unconditional with a the-gathering arm; keep the synthetic arm gated.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `crates/scenarios/src/lib.rs`:

```rust
    #[test]
    fn module_for_resolves_the_gathering() {
        let id = ScenarioId::new(the_gathering::ID);
        let module = module_for(&id).expect("the-gathering module present");
        assert_eq!(module.reference_card, "01104");
    }
```

(The existing tests `use game_core::scenario::ScenarioId;` — keep that import.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p scenarios module_for_resolves_the_gathering`
Expected: FAIL — `module_for` doesn't yet match `the-gathering` (and may be `test_fixtures`-gated).

- [ ] **Step 3: Rewrite the registry block unconditionally**

Replace the gated `module_for` + `REGISTRY` in `crates/scenarios/src/lib.rs` with:

```rust
use game_core::scenario::{ScenarioId, ScenarioModule, ScenarioRegistry};

/// Look up a scenario module by id. Returns `None` for unknown ids.
#[must_use]
pub fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    match id.as_str() {
        the_gathering::ID => Some(&the_gathering::MODULE),
        #[cfg(any(test, feature = "test_fixtures"))]
        test_fixtures::synthetic::ID => Some(&test_fixtures::synthetic::MODULE),
        _ => None,
    }
}

/// Ready-made [`ScenarioRegistry`] backed by this crate's modules. The
/// host installs it once at startup with
/// [`game_core::scenario_registry::install`].
pub const REGISTRY: ScenarioRegistry = ScenarioRegistry { module_for };
```

Remove the now-duplicate `#[cfg(...)] use game_core::scenario::{...}` import line above (the new `use` covers it). The `test_fixtures` module declaration stays gated as-is.

- [ ] **Step 4: Run the scenarios test suite**

Run: `cargo test -p scenarios`
Expected: PASS — `module_for_resolves_the_gathering`, the existing `module_for_resolves_synthetic` / `registry_dispatches_to_module_for`, and the setup unit tests all green.

- [ ] **Step 5: Commit**

```bash
git add crates/scenarios/src/lib.rs
git commit -m "scenario: register the-gathering; make module_for/REGISTRY unconditional"
```

---

## Task 7: Integration test — placement, bag, Won resolution, forced effects

**Files:**
- Create: `crates/scenarios/tests/the_gathering.rs`

Own process — installs `cards::REGISTRY` (real corpus: Roland + Attic/Cellar abilities) + `scenarios::REGISTRY`. Mirrors `crates/scenarios/tests/closing_demo.rs`.

- [ ] **Step 1: Write the integration test**

Create `crates/scenarios/tests/the_gathering.rs`:

```rust
//! C1a end-to-end: the-gathering setup() seats a roster at the Study and
//! reaches a resolution; the Attic/Cellar forced-on-enter abilities fire
//! through the real card registry. Own process so it can install the
//! process-global registries against the real `cards` corpus.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::scenario::Resolution;
use game_core::state::{InvestigatorId, LocationId};
use game_core::test_support::fire_forced_on_enter;
use game_core::action::RosterEntry;
use game_core::state::CardCode;
use game_core::{Action, PlayerAction};
use scenarios::{the_gathering, REGISTRY};

static INSTALL: Once = Once::new();

fn install_registries() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Apply one action, asserting it is not `Rejected`.
fn apply_checked(
    state: game_core::state::GameState,
    action: &Action,
) -> game_core::state::GameState {
    let r = apply(state, action.clone());
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "action {action:?} was rejected: {:?}",
        r.outcome,
    );
    r.state
}

/// Seat solo Roland (01001, empty deck) and close the mulligan window.
fn setup_and_seat() -> game_core::state::GameState {
    let inv = InvestigatorId(1);
    let mut state = the_gathering::setup();
    for a in [
        Action::Player(PlayerAction::StartScenario {
            roster: vec![RosterEntry {
                investigator: CardCode("01001".into()),
                deck: vec![],
            }],
        }),
        Action::Player(PlayerAction::Mulligan {
            investigator: inv,
            indices_to_redraw: vec![],
        }),
    ] {
        state = apply_checked(state, &a);
    }
    state
}

#[test]
fn roster_seating_places_investigator_at_study() {
    install_registries();
    let state = setup_and_seat();
    let roland = state
        .investigators
        .get(&InvestigatorId(1))
        .expect("Roland seated");
    assert_eq!(
        roland.current_location,
        Some(the_gathering::STUDY_ID),
        "seating must place investigators at setup()'s starting_location",
    );
}

#[test]
fn drives_to_a_won_resolution() {
    install_registries();
    let inv = InvestigatorId(1);
    let mut state = setup_and_seat();

    // Hand the investigator enough clues to clear all three act
    // thresholds (2 + 3 + 2 = 7); AdvanceAct spends from group clues
    // (Rules Reference clue-spend), no chaos draw involved. This proves
    // the resolution latch fires for the real act deck, deterministically.
    state.investigators.get_mut(&inv).unwrap().clues = 7;

    for _ in 0..3 {
        state = apply_checked(state, &Action::Player(PlayerAction::AdvanceAct { investigator: inv }));
    }

    assert!(
        matches!(state.resolution, Some(Resolution::Won { .. })),
        "advancing through the terminal act latches Won, got {:?}",
        state.resolution,
    );
}

#[test]
fn attic_forced_enter_deals_one_horror() {
    install_registries();
    // A bare board with the Attic (01113) present; fire the forced
    // EnteredLocation trigger directly via the test helper (live entry
    // isn't reachable until C1b's Door-on-the-Floor transition).
    use game_core::test_support::{test_investigator, test_location, TestGame};
    let mut attic = test_location(20, "Attic");
    attic.code = CardCode("01113".into());
    let mut state = TestGame::new()
        .with_investigator_at(test_investigator(1), LocationId(20))
        .with_location(attic)
        .build();
    let mut events = Vec::new();

    let outcome = fire_forced_on_enter(&mut state, &mut events, InvestigatorId(1), LocationId(20));
    assert!(matches!(outcome, EngineOutcome::Done));
    assert_eq!(
        state.investigators.get(&InvestigatorId(1)).unwrap().horror,
        1,
        "entering the Attic deals 1 horror to the entering investigator",
    );
}

#[test]
fn cellar_forced_enter_deals_one_damage() {
    install_registries();
    use game_core::test_support::{test_investigator, test_location, TestGame};
    let mut cellar = test_location(21, "Cellar");
    cellar.code = CardCode("01114".into());
    let mut state = TestGame::new()
        .with_investigator_at(test_investigator(1), LocationId(21))
        .with_location(cellar)
        .build();
    let mut events = Vec::new();

    let outcome = fire_forced_on_enter(&mut state, &mut events, InvestigatorId(1), LocationId(21));
    assert!(matches!(outcome, EngineOutcome::Done));
    assert_eq!(
        state.investigators.get(&InvestigatorId(1)).unwrap().damage,
        1,
        "entering the Cellar deals 1 damage to the entering investigator",
    );
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p scenarios --test the_gathering`
Expected: PASS (all four).

- [ ] **Step 3: Commit**

```bash
git add crates/scenarios/tests/the_gathering.rs
git commit -m "test: the-gathering C1a integration (placement, bag, Won, forced effects)"
```

---

## Task 8: Full gauntlet + phase-doc update

- [ ] **Step 1: Run the full CI gauntlet locally** (per CLAUDE.md)

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. Fix any clippy/doc issues inline.

- [ ] **Step 2: Push, open PR, watch CI**

Push `scenario/gathering-setup`; open the PR with the template (`Closes #227`); watch `gh pr checks <PR#> --watch`.

- [ ] **Step 3: Update the phase doc** (only once CI is green, as the final commit)

Per CLAUDE.md / `docs/phases/README.md`: in `docs/phases/phase-7-the-gathering.md`, move #227 to the Closed table (bump counts), flip its Arc/Ordering row to `✅ PR #N`, drop any Open question it settled, and add a **Decisions made** entry only if load-bearing for a future PR (candidate: "faithful Study-only Act-1 board; set-aside locations + Door-on-the-Floor are C1b" and "`starting_location` is the generic seating-placement channel"). Then commit and push that doc-only commit.

---

## Self-Review

**Spec coverage:**
- setup() builds Study + decks + bag → Tasks 5, 0. ✓
- Attic/Cellar forced effects → Tasks 3, 4 (+ Task 7 firing assertions). ✓
- `starting_location` placement channel → Tasks 1, 2 (+ Task 7 placement assertion). ✓
- Integration test seats roster + reaches a resolution → Task 7. ✓
- Faithful Study-only start; set-aside + transition deferred to C1b → Task 5 (doc + isolated Study). ✓
- Registry un-gating → Task 6. ✓
- Bag provenance → Task 0. ✓

**Type consistency:** `STUDY_ID: LocationId(1)` used in Task 5 and Task 7; `the_gathering::{ID, STUDY_ID, MODULE, setup}` names consistent; `RosterEntry { investigator: CardCode, deck: Vec<CardCode> }` matches `action.rs`; `Effect::DealHorror/DealDamage { target, amount }`, `InvestigatorTarget::You`, `EventPattern::EnteredLocation`, `EventTiming::After` match `card-dsl`; `fire_forced_on_enter(state, events, investigator, location)` matches `test_support`.

**Placeholder scan:** No "TBD"/"implement later". Act 01110's `clue_threshold: 2` is an explicit, commented structural placeholder (its real objective is C1b) — a design decision, not a plan gap.

**Import paths verified:** `RosterEntry` is at `game_core::action::RosterEntry` (not re-exported at the crate root). Builder methods `with_investigator_at` / `with_location` / `with_chaos_bag` / `with_scenario_id` all exist in `test_support/builder.rs` (used by the synthetic fixture). `cards` is a dependency of `scenarios`, so `cards::REGISTRY` is reachable from `scenarios/tests/`.
