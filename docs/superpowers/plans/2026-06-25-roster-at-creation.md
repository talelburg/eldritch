# Roster at game-creation Implementation Plan (#459 + #224)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move investigator seating out of the `StartScenario` player action and into game creation, so the persisted seed is already seated and the browser picker creates a playable Roland game in The Gathering.

**Architecture:** A new engine function `seat_and_open(setup_state, &roster) -> ApplyResult` runs the existing seating logic via the shared `apply_via` scaffolding (no new transactional code). `GameSession::create` calls it and persists the seated result as the seed; the action log becomes `ResolveInput`-only. `PlayerAction::StartScenario` is deleted, every test site migrates to `seat_and_open`, and seating tightens to require a non-empty roster (#224). A minimal Leptos picker collects the roster and drives creation.

**Tech Stack:** Rust (workspace crates `card-dsl`, `game-core`, `cards`, `scenarios`, `protocol`, `server`, `web`), `axum` + `sqlx` (server), Leptos + `gloo-net` + `wasm-bindgen-test` (web).

## Global Constraints

- Match CI's strict flags before pushing: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, `cargo build -p web --target wasm32-unknown-unknown`, `wasm-pack test --headless --firefox crates/web`, `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- Handler contract: validate-first / mutate-second. `seat_and_open` rejects with state unchanged on any precondition failure (`apply_via` enforces this structurally via snapshot/restore).
- Crate layering: `game-core` never depends on `cards`/`scenarios`. In-crate game-core tests resolve investigators via the synthetic `TEST_INV` registry (`test_support::install_test_registry`), never `cards::REGISTRY`.
- Verify every card code against `crates/cards/src/impls/` before using it. The default deck uses only implemented codes.
- Commit subjects: `scope: description` (scope = `engine` / `protocol` / `server` / `web` / `test` / `docs`). End commit bodies with the `Co-Authored-By` / `Claude-Session` trailers.
- One branch: `engine/roster-at-creation` (already created). Phase-doc edits land last (Task 9), only after CI is green.

---

## File Structure

- `crates/game-core/src/engine/mod.rs` — add `seat_and_open` (public engine fn) + crate-root re-export.
- `crates/game-core/src/engine/dispatch/mod.rs` — add internal `seat_and_open` (start_scenario + drive); later delete the `StartScenario` dispatch arm.
- `crates/game-core/src/engine/dispatch/phases.rs` — tighten `start_scenario` validation (#224).
- `crates/game-core/src/action.rs` — delete `PlayerAction::StartScenario`; keep `RosterEntry`.
- `crates/protocol/src/lib.rs` — add `roster` to `CreateGameRequest`.
- `crates/server/src/session.rs` — `create` takes a roster, seats into the seed; add `SessionError::Seating`.
- `crates/server/src/lifecycle.rs` — thread `request.roster`; map `Seating` → 422.
- `crates/web/src/picker.rs` — **new**: picker component + default Roland deck const.
- `crates/web/src/transport.rs` — gate creation on the picker; `CreateGameRequest` carries the roster.
- `crates/web/src/store.rs` — add `ConnStatus::AwaitingRoster`.
- `crates/web/src/app.rs`, `crates/web/src/lib.rs` — mount `PickerView`, drop `ActionControls`.
- `crates/web/src/controls.rs` + `crates/web/tests/controls.rs` — **deleted** (StartScenario button gone).
- `crates/web/tests/picker.rs` — **new**: picker headless test.
- Test sites (migrated to `seat_and_open`): `crates/game-core/src/engine/mod.rs`, `.../dispatch/{mod,phases}.rs`, `crates/game-core/tests/{act_round_end,reaction_windows}.rs`, `crates/scenarios/src/test_fixtures/synthetic.rs`, `crates/scenarios/tests/{closing_demo,mythos_phase,revelation_choice,synthetic_resolution,the_gathering,the_gathering_resolutions,upkeep_hand_size,upkeep_phase}.rs`, `crates/cards/tests/roster_seating.rs`, `crates/server/tests/{closing_demo,game_session,resume,ws}.rs`.

---

## Task 1: `seat_and_open` engine function (coexists with `StartScenario`)

Add the new entry point wrapping the **existing** `start_scenario` body. `StartScenario` stays for now so the build stays green; later tasks migrate callers and delete it.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (add internal fn)
- Modify: `crates/game-core/src/engine/mod.rs` (add public `seat_and_open` + re-export note)
- Modify: `crates/game-core/src/lib.rs:45-53` (re-export)
- Test: `crates/game-core/src/engine/mod.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `game_core::seat_and_open(setup_state: GameState, roster: &[RosterEntry]) -> ApplyResult`. Same `ApplyResult { state, events, outcome }` contract as `apply`. `AwaitingInput` (mulligan) for a valid roster; `Rejected` for an invalid one (state unchanged).
- Consumes: existing `dispatch::phases::start_scenario`, `dispatch::drive`, `engine::apply_via`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/game-core/src/engine/mod.rs` (it already imports `GameStateBuilder`, `test_investigator`, `Action`, `PlayerAction`):

```rust
#[test]
fn seat_and_open_opens_mulligan_for_a_synthetic_roster() {
    use crate::action::RosterEntry;
    use crate::state::CardCode;
    crate::test_support::install_test_registry();

    let setup = GameStateBuilder::new().build(); // round 0, no investigators
    let roster = vec![RosterEntry {
        investigator: CardCode::new(crate::test_support::TEST_INV),
        deck: vec![],
    }];

    let result = seat_and_open(setup, &roster);

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        result.outcome
    );
    assert_eq!(result.state.round, 1);
    assert!(result.state.investigators.contains_key(&crate::state::InvestigatorId(1)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core seat_and_open_opens_mulligan_for_a_synthetic_roster`
Expected: FAIL — `cannot find function seat_and_open`.

- [ ] **Step 3: Add the internal dispatch fn**

In `crates/game-core/src/engine/dispatch/mod.rs`, after `apply_player_action`, add (mirrors that fn's `start_scenario` + `drive` tail, without the pending-prompt gate — creation never has an outstanding prompt):

```rust
/// Seat a roster and drive to the first `AwaitingInput` (the setup mulligan),
/// without going through a logged `PlayerAction`. The engine entry point
/// [`crate::seat_and_open`] wraps this in the shared `apply_via` scaffolding.
/// Used at game creation (server `GameSession::create`); the action log that
/// follows is `ResolveInput`-only.
pub(crate) fn seat_and_open(cx: &mut Cx, roster: &[crate::action::RosterEntry]) -> EngineOutcome {
    let outcome = phases::start_scenario(cx, roster);
    drive(cx, outcome)
}
```

- [ ] **Step 4: Add the public engine fn**

In `crates/game-core/src/engine/mod.rs`, after `apply_with_scenario_registry`, add:

```rust
/// Create a freshly seated game: run scenario setup's roster seating over
/// `setup_state` and drive to the first `AwaitingInput` (the setup mulligan).
///
/// This is the non-logged seating path (#459). The returned
/// [`ApplyResult::state`] is already seated, shuffled, and mulligan-pending —
/// hosts persist it as the seed, so the action log is `ResolveInput`-only and
/// replay never re-runs setup RNG. Validation mirrors a player action: an
/// empty roster, an unknown/non-investigator code, or an already-started
/// state rejects with state unchanged.
pub fn seat_and_open(setup_state: GameState, roster: &[crate::action::RosterEntry]) -> ApplyResult {
    apply_via(setup_state, crate::scenario_registry::current(), |cx| {
        dispatch::seat_and_open(cx, roster)
    })
}
```

- [ ] **Step 5: Re-export at the crate root**

In `crates/game-core/src/lib.rs`, add `seat_and_open` to the `pub use engine::{ … }` block (alphabetical, near `round_end_advance`):

```rust
    resolve_encounter_card, reveal_location, round_end_advance, seat_and_open,
    shortest_first_steps, shortest_first_steps_with, spawn_set_aside_enemy,
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p game-core seat_and_open_opens_mulligan_for_a_synthetic_roster`
Expected: PASS.

- [ ] **Step 7: Add a rejection test**

```rust
#[test]
fn seat_and_open_rejects_an_unknown_investigator_code() {
    use crate::action::RosterEntry;
    use crate::state::CardCode;
    crate::test_support::install_test_registry();

    let setup = GameStateBuilder::new().build();
    let roster = vec![RosterEntry { investigator: CardCode::new("99999"), deck: vec![] }];

    let result = seat_and_open(setup, &roster);

    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0, "rejected seating leaves state unchanged");
}
```

Run: `cargo test -p game-core seat_and_open_rejects_an_unknown_investigator_code`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/mod.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/lib.rs
git commit -m "engine: seat_and_open — non-logged seating entry point (#459)"
```

---

## Task 2: `CreateGameRequest` carries the roster

**Files:**
- Modify: `crates/protocol/src/lib.rs:93-98` (`CreateGameRequest`)
- Test: `crates/protocol/src/lib.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `protocol::CreateGameRequest { scenario_id: String, roster: Vec<game_core::action::RosterEntry> }`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/protocol/src/lib.rs`:

```rust
#[test]
fn create_game_request_round_trips_with_a_roster() {
    use game_core::action::RosterEntry;
    use game_core::state::CardCode;
    let req = CreateGameRequest {
        scenario_id: "the-gathering".into(),
        roster: vec![RosterEntry { investigator: CardCode::new("01001"), deck: vec![] }],
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let back: CreateGameRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.roster.len(), 1);
    assert_eq!(back.scenario_id, "the-gathering");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p protocol create_game_request_round_trips_with_a_roster`
Expected: FAIL — missing field `roster`.

- [ ] **Step 3: Add the field**

In `crates/protocol/src/lib.rs`, extend `CreateGameRequest` and its imports:

```rust
use game_core::action::RosterEntry;
```

```rust
/// Body of `POST /games`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameRequest {
    /// The scenario module to set up.
    pub scenario_id: String,
    /// The investigators to seat at creation, each paired with the deck the
    /// player chose. Seated into the persisted seed (#459); a rejected
    /// seating fails creation with no game row.
    pub roster: Vec<RosterEntry>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p protocol create_game_request_round_trips_with_a_roster`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol/src/lib.rs
git commit -m "protocol: CreateGameRequest carries the roster (#459)"
```

---

## Task 3: `GameSession::create` seats into the seed + server tests migrate

**Files:**
- Modify: `crates/server/src/session.rs:18-84` (`SessionError`, `create`)
- Modify: `crates/server/src/lifecycle.rs:17-32` (thread roster, map 422)
- Test/Modify: `crates/server/tests/{game_session,resume,ws,closing_demo}.rs`

**Interfaces:**
- Consumes: `game_core::seat_and_open` (Task 1), `protocol::CreateGameRequest.roster` (Task 2).
- Produces: `GameSession::create(db, game_id, scenario_id, roster: Vec<RosterEntry>) -> Result<Self, SessionError>`; `SessionError::Seating(String)`.

- [ ] **Step 1: Write the failing test**

Add to `crates/server/tests/game_session.rs` (it already installs registries via `common`; confirm `cards::REGISTRY` is installed in `common` — if not, install it in the test). Use a real Roland roster:

```rust
#[tokio::test]
async fn create_seats_the_roster_into_the_seed() {
    let pool = common::test_pool().await; // existing helper
    let roster = vec![game_core::action::RosterEntry {
        investigator: game_core::state::CardCode::new("01001"),
        deck: vec![],
    }];
    let session = server::GameSession::create(
        pool.clone(), "seated", game_core::scenario::ScenarioId::new(common::TEST_SCENARIO_ID), roster,
    )
    .await
    .expect("create");

    // Seed is already seated + mulligan-pending.
    assert!(session.state.investigators.contains_key(&game_core::state::InvestigatorId(1)));
    assert!(matches!(session.outcome, game_core::EngineOutcome::AwaitingInput { .. }));

    // load replays a ResolveInput-only log and reproduces it bit-for-bit.
    let loaded = server::GameSession::load(pool, &"seated".into()).await.expect("load").expect("exists");
    assert_eq!(loaded.state, session.state);
}
```

> Adjust `common::test_pool` / `TEST_SCENARIO_ID` to the actual helper names in `crates/server/tests/common/mod.rs`. `TEST_SCENARIO_ID` must be `"the-gathering"` (the real module) so a Roland code resolves — if `common` uses a synthetic scenario, point this test at the gathering module and ensure `cards::REGISTRY` + `scenarios::REGISTRY` are installed.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p server create_seats_the_roster_into_the_seed`
Expected: FAIL — `create` takes 3 args, not 4.

- [ ] **Step 3: Add `SessionError::Seating`**

In `crates/server/src/session.rs`, extend the enum:

```rust
    /// Seating the roster at creation was rejected by the engine (empty
    /// roster, unknown/non-investigator code, or an already-started seed).
    #[error("seating rejected: {0}")]
    Seating(String),
```

- [ ] **Step 4: Seat in `create`**

Replace the body of `create` from `let state = (module.setup)();` onward:

```rust
        let setup = (module.setup)();
        let result = game_core::seat_and_open(setup, &roster);
        let outcome = match result.outcome {
            game_core::EngineOutcome::Rejected { reason } => {
                return Err(SessionError::Seating(reason.to_string()));
            }
            other => other,
        };
        let state = result.state;
        let seed_state = serde_json::to_string(&state)?;
        let game_id = game_id.into();
        store::insert_game(&db, &game_id, &scenario_id, &seed_state, &unix_millis_string()).await?;

        Ok(Self { game_id, state, outcome, seq: 0, db })
```

Update the signature to add `roster: Vec<game_core::action::RosterEntry>` and import `RosterEntry` / `EngineOutcome` as needed. Update the doc-comment: the seed is now *seated* (no longer "the scenario's `setup()` output").

- [ ] **Step 5: Thread the roster through the HTTP handler**

In `crates/server/src/lifecycle.rs`, pass `request.roster` and map the new error:

```rust
    match GameSession::create(state.db.clone(), random_game_id(), scenario_id, request.roster).await {
        Ok(session) => Ok((StatusCode::CREATED, Json(CreateGameResponse { game_id: session.game_id }))),
        Err(SessionError::UnknownScenario(_)) => Err(StatusCode::BAD_REQUEST),
        Err(SessionError::Seating(_)) => Err(StatusCode::UNPROCESSABLE_ENTITY),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
```

- [ ] **Step 6: Migrate the other server tests**

In `crates/server/tests/{game_session,resume,ws,closing_demo}.rs`, every `GameSession::create(pool, id, ScenarioId::new(...))` call gains a 4th arg. For tests that drove setup via a later `StartScenario` submit over WS / `session.apply`, pass the roster to `create` and **delete** the `StartScenario` submit (seating now happens at create). Use a Roland roster against the gathering module, or a `TEST_INV` roster if the test installs only the synthetic registry:

```rust
let roster = vec![game_core::action::RosterEntry {
    investigator: game_core::state::CardCode::new("01001"), deck: vec![],
}];
let session = GameSession::create(pool.clone(), "g2", ScenarioId::new(TEST_SCENARIO_ID), roster).await?;
// (removed: session.apply(PlayerAction::StartScenario { .. }))
```

The `game_session.rs` "unknown scenario" test (`no-such-scenario`) passes `vec![]` for the roster — it must still 400 on the scenario lookup *before* seating (the scenario lookup precedes `seat_and_open` in `create`, so an unknown scenario never reaches the empty-roster check).

- [ ] **Step 7: Run the server suite**

Run: `cargo test -p server`
Expected: PASS (all migrated tests).

- [ ] **Step 8: Commit**

```bash
git add crates/server
git commit -m "server: seat the roster into the seed at creation; 422 on bad roster (#459)"
```

---

## Task 4: Migrate game-core in-crate test sites to `seat_and_open`

18 sites across `engine/mod.rs` (13), `engine/dispatch/phases.rs` (4), `engine/dispatch/mod.rs` (1). `StartScenario` still exists; these stop using it. Each site resolves investigators via the synthetic `TEST_INV` registry (crate layering forbids `cards`).

**Files:**
- Modify: `crates/game-core/src/engine/mod.rs`, `crates/game-core/src/engine/dispatch/phases.rs`, `crates/game-core/src/engine/dispatch/mod.rs`

**Interfaces:**
- Consumes: `seat_and_open` (Task 1), `test_support::{install_test_registry, TEST_INV}`.

**Transformation recipe (apply to every in-crate site):**

Before — pre-seeded synthetic investigator + empty-roster `StartScenario`:

```rust
let id = InvestigatorId(1);
let state = GameStateBuilder::new()
    .with_investigator(test_investigator(1))
    .with_turn_order([id])
    .build();
let start_result = apply(state, Action::Player(PlayerAction::StartScenario { roster: vec![] }));
```

After — seat a `TEST_INV` roster (drop the pre-seeding + `with_turn_order`; seating builds turn order):

```rust
crate::test_support::install_test_registry();
let state = GameStateBuilder::new().build();
let roster = vec![crate::action::RosterEntry {
    investigator: crate::state::CardCode::new(crate::test_support::TEST_INV),
    deck: vec![],
}];
let start_result = seat_and_open(state, &roster);
let id = InvestigatorId(1);
```

For **N investigators**, seat `vec![RosterEntry { investigator: CardCode::new(TEST_INV), deck: vec![] }; N]`; ids mint `1..=N` in roster order.

**Assertion fixups to expect:** the seated investigator is named `"Test Investigator"` (no `N` suffix) and starts at `starting_location` (often `None`). Update any `assert_eq!(inv.name, "Test Investigator 1")` to `"Test Investigator"`, and any location assertion to the seated start. Skills (3/3/3/3), `actions_remaining` (reset to 3), `resources` (5), `clues` (0) are unchanged.

- [ ] **Step 1: Migrate `engine/dispatch/phases.rs` seating tests (4 sites)**

These directly test `start_scenario`. Convert each `apply(state, Action::Player(StartScenario { roster }))` to `seat_and_open(state, &roster)`. Note `start_scenario_on_already_started_state_is_rejected` (round 7) keeps its assertion — `seat_and_open` rejects an already-started state identically. **Leave `start_scenario_empty_roster_passes_through_with_preseated_investigator` as-is for now** (Task 6 inverts it together with the validation tightening).

- [ ] **Step 2: Run the phases tests**

Run: `cargo test -p game-core engine::dispatch::phases`
Expected: PASS (except the not-yet-inverted empty-roster test, which still relies on tolerance — it stays green because tolerance is still in place until Task 6).

- [ ] **Step 3: Migrate `engine/mod.rs` (13 sites) + `engine/dispatch/mod.rs` (1 site)**

Apply the recipe to each. Run the full game-core lib test suite after.

- [ ] **Step 4: Run game-core lib tests**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine
git commit -m "test: migrate game-core in-crate StartScenario sites to seat_and_open (#224)"
```

---

## Task 5: Migrate integration + scenario test sites to `seat_and_open`

Files in `crates/game-core/tests/`, `crates/scenarios/`, `crates/cards/tests/`. These install real registries and may seat Roland (`01001`) or `TEST_INV` as the test prefers.

**Files:**
- Modify: `crates/game-core/tests/{act_round_end,reaction_windows}.rs`
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs`
- Modify: `crates/scenarios/tests/{closing_demo,mythos_phase,revelation_choice,synthetic_resolution,the_gathering,the_gathering_resolutions,upkeep_hand_size,upkeep_phase}.rs`
- Modify: `crates/cards/tests/roster_seating.rs`

**Two worked examples:**

*Direct apply site* (e.g. `synthetic_resolution.rs`): replace
`apply_checked(state, &Action::Player(PlayerAction::StartScenario { roster: vec![] }))`
with `seat_and_open(state, &roster)` where `roster` seats the investigator(s) the test needs (`TEST_INV` for synthetic states — install the synthetic registry; `01001` for corpus states).

*Fold-style site* (e.g. `the_gathering.rs`, `closing_demo.rs`): the test builds `[StartScenario, ResolveInput(mulligan), …]` and folds with `apply_checked`. Seat **first**, then fold only the `ResolveInput`s:

```rust
fn setup_and_seat() -> GameState {
    install_registries();
    let roster = vec![RosterEntry { investigator: CardCode("01001".into()), deck: vec![] }];
    let mut state = seat_and_open(the_gathering::setup(), &roster).state;
    for a in [
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    ] {
        state = apply_checked(state, &a);
    }
    state
}
```

`roster_seating.rs` already builds real rosters via `StartScenario`; convert its `apply(state, Action::Player(StartScenario { roster }))` calls to `seat_and_open(state, &roster)` and keep its assertions.

- [ ] **Step 1: Migrate `crates/cards/tests/roster_seating.rs`**

Convert the `StartScenario` applies to `seat_and_open`. Run: `cargo test -p cards --test roster_seating` → PASS.

- [ ] **Step 2: Migrate `crates/scenarios/` sites (fixtures + 8 test files)**

Apply the recipe/examples per file. Run: `RUSTFLAGS="-D warnings" cargo test -p scenarios` → PASS.

- [ ] **Step 3: Migrate `crates/game-core/tests/{act_round_end,reaction_windows}.rs`**

These run in separate processes and install their own mock registries — compose `metadata_for_test_inv` so `TEST_INV` resolves (see `test_support` docs), then seat a `TEST_INV` roster. Run: `cargo test -p game-core --test act_round_end --test reaction_windows` → PASS.

- [ ] **Step 4: Full workspace test**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS (`StartScenario` still defined; nothing references it except its own arm + the not-yet-inverted empty-roster test).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/tests crates/scenarios crates/cards/tests
git commit -m "test: migrate integration/scenario StartScenario sites to seat_and_open (#224)"
```

---

## Task 6: Tighten seating to require a non-empty roster (#224)

Now that no caller relies on the empty-roster-on-pre-seeded tolerance, make `start_scenario` reject an empty roster outright — a single seating path.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs:69-79` (validation) + the inverted test

- [ ] **Step 1: Invert the empty-roster test**

In `crates/game-core/src/engine/dispatch/phases.rs`, rename and rewrite `start_scenario_empty_roster_passes_through_with_preseated_investigator` to assert rejection:

```rust
#[test]
fn seat_and_open_rejects_an_empty_roster() {
    install_test_registry();
    let state = GameStateBuilder::new().build();
    let result = seat_and_open(state, &[]);
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "an empty roster must reject, got {:?}",
        result.outcome
    );
}
```

(Import `seat_and_open`, `install_test_registry` as needed in the test module.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core seat_and_open_rejects_an_empty_roster`
Expected: FAIL — empty roster currently passes (tolerance still in place), so the outcome is `AwaitingInput`/`Done`, not `Rejected`.

- [ ] **Step 3: Tighten the guard**

Replace the guard at `phases.rs:75` and its comment:

```rust
    // A scenario requires at least one investigator. Seating is the sole
    // seater (#224): the roster is mandatory, an empty roster rejects.
    if resolved.is_empty() {
        return EngineOutcome::Rejected {
            reason: "a scenario requires a non-empty roster".into(),
        };
    }
```

Also remove the now-stale pre-seeded-path comments at `phases.rs:81-91` (the "ASSUMES an empty investigator set" / "pre-seated test path" notes) since seating is always from an empty set.

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p game-core seat_and_open_rejects_an_empty_roster`
Expected: PASS. Then `cargo test -p game-core --lib` → PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/phases.rs
git commit -m "engine: require a non-empty roster — single seating path (#224)"
```

---

## Task 7: Delete `PlayerAction::StartScenario`

Nothing references the variant now except its own dispatch arm and doc-comments.

**Files:**
- Modify: `crates/game-core/src/action.rs:42-64` (delete variant, update enum doc)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs:156-159` (delete arm), `:119` (doc)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (rename `start_scenario` doc to note it's reached via `seat_and_open`, not a player action)
- Modify: `crates/protocol/src/lib.rs` (doc references, if any)

- [ ] **Step 1: Delete the variant**

In `crates/game-core/src/action.rs`, remove the `StartScenario { roster }` variant. Update the `PlayerAction` doc to: a single `#[non_exhaustive]` `ResolveInput` variant — the action log is input-only; seating is a non-logged engine entry point (`seat_and_open`). Keep `RosterEntry`.

- [ ] **Step 2: Delete the dispatch arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, the `match action` in `apply_player_action` becomes a single arm:

```rust
    let outcome = match action {
        PlayerAction::ResolveInput { response } => resolve_input(cx, response),
    };
```

Update the surrounding doc-comment (drop the `StartScenario` mention; the wire surface is now just `ResolveInput`).

- [ ] **Step 3: Build + run**

Run: `RUSTFLAGS="-D warnings" cargo build --all && cargo test -p game-core --lib`
Expected: PASS. A non-exhaustive single-variant enum still compiles; the `match` is exhaustive without a wildcard.

- [ ] **Step 4: Doc check**

Run: `RUSTDOCFLAGS="-D warnings" cargo doc -p game-core -p protocol --no-deps --all-features`
Expected: PASS — fix any intra-doc link that pointed at `PlayerAction::StartScenario`.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/action.rs crates/game-core/src/engine/dispatch crates/protocol
git commit -m "engine: delete PlayerAction::StartScenario — action log is ResolveInput-only (#459)"
```

---

## Task 8: Browser picker + gated creation

Replace the auto-create + `StartScenario` button with a picker that collects the roster and drives creation.

**Files:**
- Create: `crates/web/src/picker.rs` (component + `ROLAND_DEFAULT_DECK`)
- Modify: `crates/web/src/store.rs:9-16` (`ConnStatus::AwaitingRoster`)
- Modify: `crates/web/src/transport.rs` (gate creation; `CreateGameRequest` from the picker)
- Modify: `crates/web/src/app.rs`, `crates/web/src/lib.rs` (mount `PickerView`, drop `ActionControls`)
- Delete: `crates/web/src/controls.rs`, `crates/web/tests/controls.rs`
- Create: `crates/web/tests/picker.rs`

**Interfaces:**
- Consumes: `protocol::CreateGameRequest { scenario_id, roster }`, `game_core::action::RosterEntry`.
- Produces: `web::picker::{PickerView, ROLAND_DEFAULT_DECK}`; `web::store::ConnStatus::AwaitingRoster`; `CreateTx = futures::channel::mpsc::UnboundedSender<protocol::CreateGameRequest>` (context-provided).

- [ ] **Step 1: Add the `AwaitingRoster` status**

In `crates/web/src/store.rs`, add to `ConnStatus`:

```rust
    /// No saved game and no roster chosen yet — render the picker.
    AwaitingRoster,
```

- [ ] **Step 2: Write the picker module + default deck**

Create `crates/web/src/picker.rs`. Codes verified against `crates/cards/src/impls/`:

```rust
//! Pre-game investigator/scenario picker (wasm-only). Collects a roster and
//! submits a `CreateGameRequest` on the `CreateTx` channel; the transport
//! creates the game. Replaces the former `StartScenario` button (#459).

use futures::channel::mpsc;
use game_core::action::RosterEntry;
use game_core::state::CardCode;
use leptos::prelude::*;
use protocol::CreateGameRequest;

use crate::store::{use_store, ConnStatus};

/// Channel the picker uses to hand a chosen `CreateGameRequest` to the
/// transport's creation loop. Provided into context by `transport::start`.
pub type CreateTx = mpsc::UnboundedSender<CreateGameRequest>;

/// Placeholder default deck for Roland (01001) until Phase 9 decklist import.
/// Implemented Guardian/Seeker/neutral cards only, so the opening hand is
/// playable. NOT a legal 30+1 deck — a scaffold for UI testing.
pub const ROLAND_DEFAULT_DECK: &[&str] = &[
    "01006", // .38 Special (signature)
    "01020", // Machete
    "01018", // Beat Cop
    "01021", // Guard Dog
    "01019", // First Aid
    "01024", // Dynamite Blast
    "01022", // Evidence!
    "01023", // Dodge
    "01025", // Vicious Blow
    "01030", // Magnifying Glass
    "01039", // Deduction
    "01037", // Working a Hunch
    "01089", // Guts
    "01090", // Perception
    "01091", // Overpower
    "01092", // Manual Dexterity
    "01093", // Unexpected Courage
    "01007", // Cover Up (signature weakness)
];

/// Build the default Roland roster: investigator 01001 + the placeholder deck.
pub fn roland_roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: ROLAND_DEFAULT_DECK.iter().map(|c| CardCode::new(*c)).collect(),
    }]
}

/// Pre-game picker. Renders only while `status == AwaitingRoster`. Submits a
/// `CreateGameRequest` (The Gathering + Roland) on click.
#[component]
pub fn PickerView() -> impl IntoView {
    let store = use_store();
    let create_tx = use_context::<CreateTx>();

    view! {
        {move || {
            if store.get().status != ConnStatus::AwaitingRoster {
                return ().into_any();
            }
            let tx = create_tx.clone();
            view! {
                <section class="picker">
                    <h2>"New Game"</h2>
                    <label>"Scenario: " <select><option>"The Gathering"</option></select></label>
                    <fieldset>
                        <legend>"Investigator"</legend>
                        <label><input type="radio" name="inv" checked=true/> "Roland Banks (01001)"</label>
                    </fieldset>
                    <button
                        class="create-game"
                        on:click=move |_| {
                            if let Some(tx) = tx.clone() {
                                let _ = tx.unbounded_send(CreateGameRequest {
                                    scenario_id: "the-gathering".to_string(),
                                    roster: roland_roster(),
                                });
                            }
                        }
                    >
                        "Create game"
                    </button>
                </section>
            }
            .into_any()
        }}
    }
}
```

- [ ] **Step 3: Register the module**

In `crates/web/src/lib.rs`: add `pub mod picker;`, remove `pub mod controls;`.

- [ ] **Step 4: Gate creation in the transport**

In `crates/web/src/transport.rs`:
- Provide a `CreateTx` alongside `OutboundTx` in `start`; keep a `CreateRx` for `run`.
- `create_game(store, request: CreateGameRequest)` takes the request (drop the internal hardcoded `CreateGameRequest`/`SCENARIO_ID`).
- `bootstrap`: if a saved id exists, return it; else set `status = AwaitingRoster`, `await` the next `CreateGameRequest` from `CreateRx`, then `create_game(store, request)`.
- The `StaleId` arm: `clear_saved_id()`, set `AwaitingRoster`, `await` the next request, `create_game`.

Sketch (thread `create_rx` into `run`):

```rust
pub fn start(store: StoreSignal) {
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let (create_tx, create_rx) = mpsc::unbounded::<CreateGameRequest>();
    provide_context(tx);
    provide_context::<crate::picker::CreateTx>(create_tx);
    spawn_local(run(store, rx, create_rx));
}

async fn await_roster(store: &StoreSignal, create_rx: &mut mpsc::UnboundedReceiver<CreateGameRequest>) -> Option<CreateGameRequest> {
    store.update(|s| s.status = ConnStatus::AwaitingRoster);
    create_rx.next().await
}
```

`bootstrap`/`StaleId` call `await_roster` then `create_game(store, req)`.

- [ ] **Step 5: Mount the picker, drop ActionControls**

In `crates/web/src/app.rs`, replace `ActionControls` with `PickerView`:

```rust
{ view! { <crate::picker::PickerView/><crate::input::AwaitingInputView/> }.into_any() }
```

Delete `crates/web/src/controls.rs` and `crates/web/tests/controls.rs`.

- [ ] **Step 6: Write the picker headless test**

Create `crates/web/tests/picker.rs` (model the mount harness on the former `controls.rs` test):

```rust
#![cfg(target_arch = "wasm32")]
use futures::channel::mpsc;
use futures::StreamExt as _;
use leptos::prelude::*;
use protocol::CreateGameRequest;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::picker::{CreateTx, PickerView};
use web::store::{ClientState, ConnStatus};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn create_button_sends_a_roster() {
    let store = RwSignal::new(ClientState { status: ConnStatus::AwaitingRoster, ..Default::default() });
    let (tx, mut rx) = mpsc::unbounded::<CreateGameRequest>();
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<CreateTx>(tx.clone());
        view! { <PickerView/> }
    });
    // Click the create button.
    let doc = web_sys::window().unwrap().document().unwrap();
    let btn = doc.query_selector(".create-game").unwrap().unwrap()
        .dyn_into::<web_sys::HtmlElement>().unwrap();
    btn.click();

    let req = rx.next().await.expect("a CreateGameRequest was sent");
    assert_eq!(req.scenario_id, "the-gathering");
    assert_eq!(req.roster.len(), 1);
    assert_eq!(req.roster[0].investigator.as_str(), "01001");
    assert!(!req.roster[0].deck.is_empty(), "Roland is seated with the default deck");
}
```

- [ ] **Step 7: wasm build + test + clippy**

Run:
```bash
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/web
git commit -m "web: investigator/scenario picker drives game creation (#459)"
```

---

## Task 9: Full gauntlet + phase-doc updates

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green. Fix any failures before proceeding.

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin engine/roster-at-creation
gh pr create --fill
```
PR body: design-decisions paragraph (seating moves to creation; seed bakes in seating + setup shuffle; `PlayerAction` collapses to `ResolveInput`; #224 folded in — single seating path, TEST_INV-seated synthetic tests). `Closes #459.` and `Closes #224.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`
Fix failures with follow-up commits to the same branch.

- [ ] **Step 4: Update the phase doc (only once CI is green)**

In `docs/phases/phase-7-the-gathering.md`, per `docs/phases/README.md` "Maintaining these docs":
- Mark the **Investigator/scenario picker** capstone bullet ✅ shipped (PR #N), and **#459** ✅ shipped.
- Add a brief note that **#224** closed alongside (single seating path; `seat_and_open` replaces the `StartScenario` action; the action log is `ResolveInput`-only; the seed bakes in seating + setup shuffle).
- Add a **Decisions made**-style entry only if load-bearing for a future PR: e.g. "Seating is a non-logged engine entry point (`seat_and_open`), not an `apply` handler; hosts persist the seated result as the seed."
- Drop any settled open question.

- [ ] **Step 5: Commit the phase-doc update (final commit)**

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — roster-at-creation picker + #224 shipped"
git push
```

- [ ] **Step 6: Merge only after explicit user approval**

`gh pr merge <PR#> --squash --delete-branch`, confirm #459 and #224 auto-closed, `git checkout main && git pull`.

---

## Self-Review

**Spec coverage:**
- §1 seating-as-function → Task 1, 7. §1 validation tightening → Task 6. §2 `CreateGameRequest`/seed/422 → Tasks 2, 3. §3 picker/transport/default deck/StaleId → Task 8. §4 test migration → Tasks 4, 5 (+ server in 3). §5 server path → Task 3. Testing strategy → tests embedded per task. All covered.

**Placeholder scan:** Migration tasks 4/5 use a concrete transformation recipe + worked examples + an exhaustive file list (the edits are uniform; enumerating 18 near-identical diffs verbatim would be noise, not signal). The one soft spot — exact `common` helper names in `crates/server/tests/common/mod.rs` — is flagged inline for the implementer to confirm.

**Type consistency:** `seat_and_open(GameState, &[RosterEntry]) -> ApplyResult` is consistent across Tasks 1/3/4/5/6. `CreateGameRequest { scenario_id, roster }` consistent across Tasks 2/3/8. `SessionError::Seating(String)` consistent across Task 3. `ConnStatus::AwaitingRoster` / `CreateTx` consistent across Task 8.
