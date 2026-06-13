# `Effect::Native` Bridge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a generic `Effect::Native { tag }` variant + a `CardRegistry` bridge so single-use card logic lives card-locally in Rust instead of accreting one-off variants in the shared `Effect` enum; prove it by migrating `act_01108` and removing C1b's three single-use variants.

**Architecture:** `card-dsl` gains one serializable `Native { tag: String }` variant. `game-core`'s evaluator resolves the tag through a new `CardRegistry.native_effect_for` fn pointer to a `cards`-provided `NativeEffectFn = fn(&mut Cx, &EvalContext) -> EngineOutcome`. The bridge lives in the registry because `Effect` (in `card-dsl`, below `game-core`) can't name `GameState`/`Cx`, and `Effect` must stay serde-serializable.

**Tech Stack:** Rust workspace (`card-dsl`, `game-core`, `cards`, `scenarios`). CI gauntlet per `CLAUDE.md`.

**Spec:** `docs/superpowers/specs/2026-06-13-native-effect-bridge-design.md` (issue #276).

**Branch:** `engine/native-effect` (already checked out).

**Note on the tag type:** the spec sketched `tag: &'static str`, but `Effect: Deserialize` (a tested round-trip contract) can't produce a `'static` borrow, so the field is `String` and the builder takes `impl Into<String>` — matching existing variants like `RemoveLocationFromGame { location: String }`.

---

### Task 1: `Effect::Native` variant + dispatch mechanism

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (variant + builder + serde test)
- Modify: `crates/game-core/src/engine/cx.rs` (make `Cx` public)
- Modify: `crates/game-core/src/engine/mod.rs` (`pub use cx::Cx`)
- Modify: `crates/game-core/src/card_registry.rs` (`NativeEffectFn` type + field + fake_registry)
- Modify: `crates/game-core/src/lib.rs` (re-exports)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`Native` arm)
- Modify (add `native_effect_for: |_| None` to every `CardRegistry { … }` literal):
  - `crates/cards/src/lib.rs:91`
  - `crates/scenarios/src/test_fixtures/synth_cards.rs:203`
  - `crates/cards/tests/reject_rollback.rs:68`
  - `crates/game-core/tests/on_skill_test_resolution.rs:75`
  - `crates/game-core/tests/activate_ability.rs:93`
  - `crates/game-core/tests/forced_triggers.rs:93`
  - `crates/game-core/tests/reaction_windows.rs:97`
  - `crates/game-core/src/engine/evaluator.rs:1513` (`fake_registry`)
  - `crates/game-core/src/engine/dispatch/hunters.rs:664` (`fake_registry`)
- Test: `crates/game-core/tests/native_effect.rs` (new integration test)

- [ ] **Step 1: Write the failing integration test**

Create `crates/game-core/tests/native_effect.rs`:

```rust
//! `Effect::Native` dispatch: a card's `native(tag)` effect resolves
//! through `CardRegistry.native_effect_for` to a host-provided Rust fn.
//! Exercised via the forced-trigger path (the real apply route) since
//! `apply_effect` is `pub(crate)`.

use std::sync::OnceLock;

use card_dsl::dsl::{native, on_event, Ability, EventPattern, EventTiming};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::state::{Agenda, CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{fire_forced_on_phase_end, test_investigator, GameStateBuilder};
use game_core::{Cx, EngineOutcome, EvalContext};

const AGENDA: &str = "TEST-AGENDA";

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == AGENDA {
        // Forced at end of enemy phase -> a native effect tagged "test:set-doom".
        Some(vec![on_event(
            EventPattern::PhaseEnded { phase: card_dsl::dsl::Phase::Enemy },
            EventTiming::After,
            native("test:set-doom"),
        )])
    } else {
        None
    }
}

fn set_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    cx.state.agenda_doom = 7;
    EngineOutcome::Done
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        "test:set-doom" => Some(set_doom),
        _ => None,
    }
}

fn install() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: mock_native_for,
        });
    });
}

fn state_with_agenda(code: &str) -> GameState {
    // `turn_order` must be non-empty: `PhaseEnded` forced dispatch binds
    // the controller to `turn_order.first()` and returns no hits otherwise.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(code),
        doom_threshold: 10,
        resolution: None,
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn native_effect_runs_via_registry() {
    install();
    let mut state = state_with_agenda(AGENDA);
    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 7, "native effect mutated state");
}
```

- [ ] **Step 2: Run it to verify it fails (does not compile)**

Run: `cargo test -p game-core --test native_effect`
Expected: FAIL — `native` not found in `card_dsl::dsl`, `NativeEffectFn`/`Cx` not found in `game_core`, `CardRegistry` has no field `native_effect_for`.

- [ ] **Step 3: Add the `Native` variant + builder to `card-dsl`**

In `crates/card-dsl/src/dsl.rs`, add to the `Effect` enum (after `AdvanceCurrentAct`):

```rust
    /// A card-local Rust effect, resolved by tag through the host's
    /// `CardRegistry.native_effect_for`. The generic escape hatch for
    /// single-use card logic that doesn't earn a shared `Effect` variant
    /// (see issue #276). The `cards` crate maps the tag to a Rust fn; the
    /// evaluator rejects loudly on an unknown tag or absent registry.
    Native { tag: String },
```

Add the builder near `advance_current_act` (after it):

```rust
/// Build an [`Effect::Native`] referencing a host-registered Rust effect
/// by `tag` (convention: `"<cardcode>:<name>"`).
#[must_use]
pub fn native(tag: impl Into<String>) -> Effect {
    Effect::Native { tag: tag.into() }
}
```

- [ ] **Step 4: Add a serde round-trip test for `Native` in `card-dsl`**

In `crates/card-dsl/src/dsl.rs` tests module, add:

```rust
    #[test]
    fn native_effect_round_trips_through_serde_json() {
        let effect = native("01108:board-build");
        let json = serde_json::to_string(&effect).expect("serialize");
        let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(effect, recovered);
    }
```

- [ ] **Step 5: Make `Cx` public**

In `crates/game-core/src/engine/cx.rs`, change `pub(crate) struct Cx<'a>` to `pub struct Cx<'a>`. Add a doc line noting it is the public effect-resolution context passed to `NativeEffectFn`.

In `crates/game-core/src/engine/mod.rs:14`, change `pub(crate) use cx::Cx;` to `pub use cx::Cx;`.

- [ ] **Step 6: Define `NativeEffectFn` and add the registry field**

In `crates/game-core/src/card_registry.rs`, add the type alias (near the top, after imports — it needs `Cx`, `EvalContext`, `EngineOutcome`):

```rust
use crate::engine::{Cx, EngineOutcome, EvalContext};

/// A card-local Rust effect: mutates state and emits events through the
/// effect-resolution context, returning the resolution outcome. Provided
/// by the `cards` crate and dispatched from [`Effect::Native`] via
/// [`CardRegistry::native_effect_for`].
pub type NativeEffectFn = fn(&mut Cx, &EvalContext) -> EngineOutcome;
```

Add the field to `CardRegistry`:

```rust
    /// Look up a card-local Rust effect by its [`Effect::Native`] tag.
    /// Returns `None` for unregistered tags.
    pub native_effect_for: fn(&str) -> Option<NativeEffectFn>,
```

Update the in-module `fake_registry` (`card_registry.rs:142`) to add `native_effect_for: |_| None,`.

- [ ] **Step 7: Re-export the public effect API from `game-core`**

In `crates/game-core/src/lib.rs:45`, extend the re-export:

```rust
pub use engine::{apply, ApplyResult, Cx, EngineOutcome, EvalContext, InputRequest, ResumeToken};
```

(`EvalContext` is already re-exported by `engine/mod.rs`; this surfaces it + `Cx` at the crate root.) `NativeEffectFn` is already `pub` in `card_registry`, reachable as `game_core::card_registry::NativeEffectFn`.

- [ ] **Step 8: Add the `Native` arm to `apply_effect`**

In `crates/game-core/src/engine/evaluator.rs`, in the `apply_effect` match, add:

```rust
        Effect::Native { tag } => {
            let Some(reg) = crate::card_registry::current() else {
                return EngineOutcome::Rejected {
                    reason: format!("Native effect {tag:?}: no card registry installed").into(),
                };
            };
            let Some(f) = (reg.native_effect_for)(tag) else {
                return EngineOutcome::Rejected {
                    reason: format!("Native effect {tag:?}: no handler registered").into(),
                };
            };
            f(cx, &eval_ctx)
        }
```

Update the `evaluator.rs` tests' `fake_registry` (`evaluator.rs:1513`) and `hunters.rs` `fake_registry` (`hunters.rs:664`) to add `native_effect_for: |_| None,`.

- [ ] **Step 9: Add `native_effect_for: |_| None` to remaining `CardRegistry` literals**

Add `native_effect_for: |_| None,` to each construction site listed under **Files** that hasn't been updated yet:
`cards/src/lib.rs:91`, `scenarios/src/test_fixtures/synth_cards.rs:203`, `cards/tests/reject_rollback.rs:68`, `game-core/tests/on_skill_test_resolution.rs:75`, `game-core/tests/activate_ability.rs:93`, `game-core/tests/forced_triggers.rs:93`, `game-core/tests/reaction_windows.rs:97`.

- [ ] **Step 10: Run the new test + the workspace build**

Run: `cargo test -p game-core --test native_effect`
Expected: PASS (`native_effect_runs_via_registry`).

Run: `cargo test -p card-dsl native_effect_round_trips`
Expected: PASS.

Run: `cargo build --all`
Expected: clean (all registry literals updated).

- [ ] **Step 11: Add the unknown-tag reject test**

Append to `crates/game-core/tests/native_effect.rs` a second mock path. Add a constant and ability for a missing tag, then a test. Simplest: extend `mock_abilities_for` to also answer a second agenda code whose native tag is unregistered.

Add near the top:

```rust
const AGENDA_BAD: &str = "TEST-AGENDA-BAD";
```

In `mock_abilities_for`, add a branch before the `else`:

```rust
    } else if code.as_str() == AGENDA_BAD {
        Some(vec![on_event(
            EventPattern::PhaseEnded { phase: card_dsl::dsl::Phase::Enemy },
            EventTiming::After,
            native("test:missing"),
        )])
```

Add the test:

```rust
#[test]
fn native_effect_rejects_unknown_tag() {
    install();
    let mut state = state_with_agenda(AGENDA_BAD);
    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }), "unknown tag rejects");
    assert_eq!(state.agenda_doom, 0, "no mutation on reject");
}
```

Run: `cargo test -p game-core --test native_effect`
Expected: PASS (both tests).

> **No dedicated "no-registry" test.** The spec lists it, but it's unreachable via the normal path: finding a `native` ability requires `abilities_for` (the same registry), so the `Native` arm can only run once a registry is installed. The `card_registry::current()`-is-`None` guard mirrors the existing `resolve_one` guard in `forced_triggers.rs` and stays as defensive belt-and-suspenders.

- [ ] **Step 12: Commit**

```bash
git add crates/card-dsl crates/game-core crates/cards crates/scenarios
git commit -m "engine: Effect::Native + CardRegistry native-effect bridge

Generic escape hatch for card-local Rust effects, resolved by tag
through the registry. Effect stays serde-serializable; the fn pointer
lives in the registry (game-core) since card-dsl can't name GameState.

Refs #276.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Migrate `act_01108` to a card-local native fn

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (make `location_id_by_code` `pub`)
- Modify: `crates/game-core/src/engine/dispatch/reveal.rs` (make `reveal_location` `pub`)
- Modify: `crates/game-core/src/lib.rs` (re-export the two helpers)
- Modify: `crates/cards/src/impls/act_01108.rs` (card-local `board_build` fn + native tag + test)
- Modify: `crates/cards/src/impls/mod.rs` (`native_effect_for` resolver)
- Modify: `crates/cards/src/lib.rs` (wire resolver into `REGISTRY`)
- Regression: `crates/scenarios/tests/the_gathering.rs::advancing_act_1_rebuilds_the_board` stays green.

- [ ] **Step 1: Make the two helpers public and re-export them**

In `crates/game-core/src/engine/evaluator.rs`, change `fn location_id_by_code(` to `pub fn location_id_by_code(`. Add a doc line: "Resolve an in-play location's `LocationId` by its printed card code; `None` if no in-play location carries that code."

In `crates/game-core/src/engine/dispatch/reveal.rs:15`, change `pub(crate) fn reveal_location(` to `pub fn reveal_location(`.

In `crates/game-core/src/lib.rs`, after the existing re-exports, add:

```rust
pub use engine::dispatch::reveal::reveal_location;
pub use engine::evaluator::location_id_by_code;
```

Run: `cargo build -p game-core`
Expected: clean (these are `pub` items re-exported from within the crate; intermediate module privacy doesn't block in-crate re-export).

- [ ] **Step 2: Update the `act_01108` structural test to expect the native effect**

In `crates/cards/src/impls/act_01108.rs`, replace the existing `abilities_are_one_forced_on_advance_world_build` test body's effect assertions with a single native-tag assertion:

```rust
    #[test]
    fn abilities_are_one_forced_on_advance_native_board_build() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After
            }
        );
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == "01108:board-build"),
            "board build is a card-local native effect, got {:?}",
            abilities[0].effect
        );
    }
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p cards --lib act_01108`
Expected: FAIL — `abilities()` still returns the `Seq` of the three old variants, not `Native`.

- [ ] **Step 4: Rewrite `act_01108::abilities()` + add the card-local `board_build` fn**

Replace the `use` line and `abilities()` in `crates/cards/src/impls/act_01108.rs`:

```rust
use card_dsl::dsl::{native, on_event, Ability, EventPattern, EventTiming};
use game_core::{location_id_by_code, reveal_location, Cx, EngineOutcome, EvalContext, Event};

/// `ArkhamDB` code for Act 1, "Trapped".
pub const CODE: &str = "01108";

/// Native-effect tag for this act's reverse board build.
const BOARD_BUILD: &str = "01108:board-build";

/// 01108's Forced on-advance reverse: build the Act-1 board.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::ActAdvanced,
        EventTiming::After,
        native(BOARD_BUILD),
    )]
}

/// Resolve [`BOARD_BUILD`] if `tag` matches. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<game_core::card_registry::NativeEffectFn> {
    (tag == BOARD_BUILD).then_some(board_build as game_core::card_registry::NativeEffectFn)
}

/// Put the set-aside Hallway/Cellar/Attic/Parlor into play, relocate
/// every investigator to the Hallway (01112), and remove the Study
/// (01111). Ports the three former `Effect` arms verbatim, now
/// card-local. Rejects (leaving state partially built — matching the
/// former `Seq` short-circuit) if 01112 or 01111 are not in play.
fn board_build(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    // Put set-aside locations into play.
    let drained = std::mem::take(&mut cx.state.set_aside_locations);
    for loc in drained {
        cx.state.locations.insert(loc.id, loc);
    }
    // Relocate all investigators to the Hallway (01112).
    let Some(dest) = location_id_by_code(cx.state, "01112") else {
        return EngineOutcome::Rejected {
            reason: "01108 board-build: no in-play Hallway (01112)".into(),
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
    reveal_location(cx, dest);
    // Remove the Study (01111) from the game.
    let Some(study) = location_id_by_code(cx.state, "01111") else {
        return EngineOutcome::Rejected {
            reason: "01108 board-build: no in-play Study (01111)".into(),
        };
    };
    cx.state.locations.remove(&study);
    EngineOutcome::Done
}
```

Update the test module's `use` (top of `mod tests`) to import `Effect` for the `matches!`:

```rust
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger};
```

- [ ] **Step 5: Wire the resolver into the crate registry**

In `crates/cards/src/impls/mod.rs`, add a crate-level resolver that delegates to each card's `native_effect_for`. After the `abilities_for` match function add:

```rust
/// Resolve an [`Effect::Native`] tag to the card-local Rust fn that
/// implements it. Mirrors `abilities_for`'s per-card delegation.
pub fn native_effect_for(tag: &str) -> Option<game_core::card_registry::NativeEffectFn> {
    act_01108::native_effect_for(tag)
}
```

In `crates/cards/src/lib.rs`, add an adapter + wire it into `REGISTRY`:

```rust
/// Adapter from a native-effect tag to its handler.
fn registry_native_effect_for(tag: &str) -> Option<game_core::card_registry::NativeEffectFn> {
    impls::native_effect_for(tag)
}
```

In the `REGISTRY` literal (`cards/src/lib.rs:91`), replace the placeholder `native_effect_for: |_| None,` from Task 1 with:

```rust
    native_effect_for: registry_native_effect_for,
```

- [ ] **Step 6: Run the card test + the board-build regression**

Run: `cargo test -p cards --lib act_01108`
Expected: PASS (`abilities_are_one_forced_on_advance_native_board_build`).

Run: `cargo test -p scenarios --test the_gathering advancing_act_1_rebuilds_the_board`
Expected: PASS — board built identically through the native path (four locations in play, Study gone, set-aside empty, investigator relocated to + reveal of 01112).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core crates/cards
git commit -m "card: migrate act_01108 board build to a card-local native effect

Replaces the three single-use Effect variants with one card-local Rust
fn dispatched via Effect::Native. Promotes location_id_by_code and
reveal_location to pub.

Refs #276.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Remove the three single-use `Effect` variants

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (remove 3 variants + 3 builders + their builder tests)
- Modify: `crates/game-core/src/engine/evaluator.rs` (remove 3 match arms + their unit tests)
- Modify: `crates/game-core/src/state/game_state.rs:44` (fix doc comment)
- Modify: `crates/game-core/src/engine/dispatch/reveal.rs:5` (fix doc comment)

- [ ] **Step 1: Confirm nothing else references the three variants/builders**

Run:
```bash
grep -rn "PutSetAsideLocationsIntoPlay\|RelocateAllInvestigators\|RemoveLocationFromGame\|put_set_aside_locations_into_play\|relocate_all_investigators\|remove_location_from_game" crates/ --include=*.rs
```
Expected: only the declarations to be removed (enum variants in `dsl.rs`, builders in `dsl.rs`, match arms + unit tests in `evaluator.rs`) and the two doc-comment mentions (`game_state.rs:44`, `reveal.rs:5`). No live call sites in `cards`/`scenarios` (act_01108 migrated in Task 2).

- [ ] **Step 2: Remove the variants + builders + builder tests from `card-dsl`**

In `crates/card-dsl/src/dsl.rs`:
- Delete the `PutSetAsideLocationsIntoPlay`, `RelocateAllInvestigators { to: String }`, and `RemoveLocationFromGame { location: String }` variants from the `Effect` enum (keep `AdvanceCurrentAct`).
- Delete the `put_set_aside_locations_into_play`, `relocate_all_investigators`, and `remove_location_from_game` builder fns.
- Delete any builder unit tests that construct those three (search the tests module for the builder names; remove only those test fns).

- [ ] **Step 3: Remove the three match arms + unit tests from `game-core`**

In `crates/game-core/src/engine/evaluator.rs`:
- Delete the `Effect::PutSetAsideLocationsIntoPlay => { … }`, `Effect::RelocateAllInvestigators { to } => { … }`, and `Effect::RemoveLocationFromGame { location } => { … }` arms from `apply_effect`.
- Delete the unit tests that exercise them (around `evaluator.rs:1909/1947/1980/2001` — the tests asserting set-aside drain, relocate-all, and remove-location). Remove the whole `#[test] fn` for each.
- If `location_id_by_code` becomes unused inside `evaluator.rs` after removing `RelocateAllInvestigators`/`RemoveLocationFromGame`, it is still `pub` (re-exported for `cards`), so it will not trip dead-code lints; leave it.

- [ ] **Step 4: Fix the two doc comments**

In `crates/game-core/src/state/game_state.rs:44`, reword the comment so it no longer names the removed `PutSetAsideLocationsIntoPlay` variant — e.g. "Act-1's reverse drains these into play (the `01108:board-build` native effect)."

In `crates/game-core/src/engine/dispatch/reveal.rs:5`, reword the comment listing callers so it no longer names `RelocateAllInvestigators` — e.g. "(seating, `move_action`, and act-1's board-build native effect) call this."

- [ ] **Step 5: Build + test the affected crates**

Run: `cargo test -p card-dsl`
Expected: PASS (no dangling references to removed builders).

Run: `cargo test -p game-core`
Expected: PASS (no dangling match arms; exhaustiveness holds).

Run: `cargo build --all`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/card-dsl crates/game-core
git commit -m "engine: remove C1b's three single-use Effect variants

PutSetAsideLocationsIntoPlay / RelocateAllInvestigators /
RemoveLocationFromGame now live as act_01108's card-local native effect.
Keeps AdvanceCurrentAct (a reusable framework op).

Refs #276.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Full strict gauntlet + push + PR

**Files:** none (verification + delivery).

- [ ] **Step 1: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green. Fix any warning/lint before proceeding.

- [ ] **Step 2: Push the branch**

```bash
git push -u origin engine/native-effect
```

- [ ] **Step 3: Open the PR**

```bash
gh pr create --fill --base main \
  --title "engine: Effect::Native — card-local Rust effects via registry bridge"
```
Body: summarize the mechanism + the act_01108 migration + variant removal; include the design-decision paragraph (why the registry bridge, why tag-not-fn-pointer, why `Cx` public). Add `Closes #276.`

- [ ] **Step 4: Watch CI**

Run: `gh pr checks <PR#> --watch` (background).
Expected: all seven jobs green. Fix failures with follow-up commits to the same branch.

- [ ] **Step 5: Update the phase doc (only once CI is green)**

Per `CLAUDE.md` step 6, as the final commit: in `docs/phases/phase-7-the-gathering.md`, record #276 in the Closed table and add a **Decisions made** entry for the `Effect::Native` mechanism (load-bearing for every future bespoke card). Do not merge — stop for explicit user approval.
```
