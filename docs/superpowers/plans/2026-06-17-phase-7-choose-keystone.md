# Unified `Choose` Keystone (#349) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the bare `*::ChosenByController` target stubs with the V7 unified `Choose` surface (`Choose<S>` + a shared `LocationSet` spatial vocabulary + an entity `EntityScope`), and make the resolver honor an `At(your-location)` constraint — closing #349 and unblocking the choice-cluster cards.

**Architecture:** A type-parameterized `Choose<S>` wraps a scope; `LocationSet{Here,Anywhere}` is the chooser-relative spatial vocabulary used directly by location-picks and via `EntityScope::At(LocationSet)` by entity-position-filters. Variety stays statically typed at the effect target (investigator/location), so illegal pairs are unrepresentable. The evaluator's existing `ground_chosen_targets` + `resolve_choice_count`/`Choice`-frame machinery (Axis A) is reused unchanged; only the matched variant and the candidate enumeration change.

**Tech Stack:** Rust workspace; `card-dsl` crate (pure DSL types), `game-core` crate (evaluator). serde-derive on all DSL types.

## Global Constraints

- Match CI's strict flags before every commit: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- `card-dsl` is **below** `game-core`: no `game-core` types may appear in `card-dsl`.
- All DSL types derive `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize` (the existing `InvestigatorTarget`/`LocationTarget` are `Copy`; the new types must stay `Copy` so the target enums remain `Copy`).
- Validate-first / mutate-second: grounding a choice with 0 legal options returns `Rejected` with state unchanged.
- Scope boundary for this PR (#349): ships the **investigator + location** varieties + `LocationSet{Here, Anywhere}`. The **enemy** variety + `chosen_enemy` binding ship in PR-2 (#301); `LocationSet::YourOrConnecting` ships in PR-8 (#306); `chooser` is deferred. Do not add them here.

**Branch:** `engine/choose-keystone` (one branch per issue, #349). The two already-written design docs (`docs/superpowers/specs/2026-06-17-phase-7-choice-cluster-completion-decomposition-design.md` and `2026-06-17-phase-7-choice-keystone-design.md`) are committed as the branch's first commit before Task 1.

---

### Task 1: V7 surface + behavior-preserving migration

Introduce the new DSL types and replace every `ChosenByController` with `Chosen(Choose{…Anywhere})`. This is a pure refactor: `Anywhere` reproduces today's all-candidates behavior, so every existing test passes unchanged. The new testable deliverable is the types' serde round-trip.

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (add types + constructors; change the two target enums + doc comments)
- Modify: `crates/game-core/src/engine/evaluator.rs` (match `Chosen(_)` in `ground_chosen_targets`, `resolve_investigator_target`, `resolve_location_target`; migrate inline-test call sites + module doc comments)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:806` (doc-comment mention of `LocationTarget::ChosenByController`)
- Test: `crates/card-dsl/src/dsl.rs` (`#[cfg(test)]`, new round-trip test)

**Interfaces:**
- Produces: `Choose<S> { pub scope: S }`; `enum LocationSet { Here, Anywhere }`; `enum EntityScope { At(LocationSet) }`; `InvestigatorTarget::Chosen(Choose<EntityScope>)` + `InvestigatorTarget::chosen_anywhere() -> InvestigatorTarget`; `LocationTarget::Chosen(Choose<LocationSet>)` + `LocationTarget::chosen_anywhere() -> LocationTarget`.

- [ ] **Step 1: Write the failing serde round-trip test** (append to `crates/card-dsl/src/dsl.rs` test module)

```rust
#[test]
fn choose_surface_serde_round_trips() {
    let inv = InvestigatorTarget::chosen_anywhere();
    let loc = LocationTarget::chosen_anywhere();
    let here = InvestigatorTarget::Chosen(Choose {
        scope: EntityScope::At(LocationSet::Here),
    });
    for t in [inv, here] {
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<InvestigatorTarget>(&json).unwrap(), t);
    }
    let json = serde_json::to_string(&loc).unwrap();
    assert_eq!(serde_json::from_str::<LocationTarget>(&json).unwrap(), loc);
}
```

- [ ] **Step 2: Run it to verify it fails (does not compile — types absent)**

Run: `cargo test -p card-dsl choose_surface_serde_round_trips`
Expected: FAIL — `cannot find type Choose` / `no associated function chosen_anywhere`.

- [ ] **Step 3: Add the three types + constructors** (in `crates/card-dsl/src/dsl.rs`, near `InvestigatorTarget`)

```rust
/// A controller-facing choice of a board entity or location. Generic over its
/// `scope` (the candidate filter). `chooser` is deferred — every choice is the
/// controller's today; agenda 01105's "lead" choice already works via the
/// forced-dispatch `controller = lead` binding. The wrapper reserves its home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Choose<S> {
    /// The candidate filter (an [`EntityScope`] or [`LocationSet`]).
    pub scope: S,
}

/// The chooser-relative set of locations a choice is measured against —
/// shared by location-picks (which locations may I pick?) and entity-position
/// filters (where must the entity be?), so "your location" is defined once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LocationSet {
    /// The chooser's own location ("your location"). Empty when the chooser is
    /// between locations.
    Here,
    /// Any location in play (the old bare `ChosenByController` for locations).
    Anywhere,
    // `YourOrConnecting` is added by PR-8 (#306) with the adjacency model.
}

/// An entity-choice filter. Locational today; non-spatial arms (`Engaged`,
/// `WithTrait`, …) accrete here when a card needs them — additively, touching
/// neither [`LocationSet`] nor location-picks. (The `UsagePeriod::Round`-only
/// minimal-enum-with-a-growth-path idiom.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityScope {
    /// An entity whose location is in the given [`LocationSet`].
    At(LocationSet),
}

impl InvestigatorTarget {
    /// "Choose an investigator" with no location constraint (any investigator
    /// in play). The successor to the bare `ChosenByController`.
    #[must_use]
    pub fn chosen_anywhere() -> Self {
        InvestigatorTarget::Chosen(Choose {
            scope: EntityScope::At(LocationSet::Anywhere),
        })
    }
}

impl LocationTarget {
    /// "Choose a location" with no constraint (any location in play).
    #[must_use]
    pub fn chosen_anywhere() -> Self {
        LocationTarget::Chosen(Choose {
            scope: LocationSet::Anywhere,
        })
    }
}
```

- [ ] **Step 4: Replace the `ChosenByController` variants**

In `crates/card-dsl/src/dsl.rs`, change `InvestigatorTarget`'s `ChosenByController` variant to:

```rust
    /// The chooser picks one investigator from the [`Choose`]'s scope. Bound by
    /// the evaluator's `ground_chosen_targets` before the effect's handler runs.
    Chosen(Choose<EntityScope>),
```

and `LocationTarget`'s `ChosenByController` variant to:

```rust
    /// The chooser picks one location from the [`Choose`]'s scope. Bound by
    /// `ground_chosen_targets` before the handler runs.
    Chosen(Choose<LocationSet>),
```

Update any doc comments in this file that referenced `ChosenByController` (search the file) to name `Chosen` instead, so `cargo doc -D warnings` has no broken intra-doc links.

- [ ] **Step 5: Migrate `game-core` references**

In `crates/game-core/src/engine/evaluator.rs`:

- `resolve_investigator_target`: change the match arm
  `InvestigatorTarget::ChosenByController => ctx.chosen_investigator.ok_or(…)`
  to `InvestigatorTarget::Chosen(_) => ctx.chosen_investigator.ok_or(…)` (update the message string to say `Chosen`).
- `resolve_location_target`: change `LocationTarget::ChosenByController => ctx.chosen_location.ok_or(…)` to `LocationTarget::Chosen(_) => ctx.chosen_location.ok_or(…)`.
- `ground_chosen_targets`: change
  `matches!(inv_target, Some(InvestigatorTarget::ChosenByController))` to
  `matches!(inv_target, Some(InvestigatorTarget::Chosen(_)))`, and the
  `Effect::DiscoverClue { from: LocationTarget::ChosenByController, .. }` match to
  `Effect::DiscoverClue { from: LocationTarget::Chosen(_), .. }`.
- Update the module-level doc comments (lines ~40–45, ~107–112, ~176) and the `ground_*` doc comments that name `ChosenByController` to name `Chosen`.
- In the `#[cfg(test)]` module, replace each `InvestigatorTarget::ChosenByController` with `InvestigatorTarget::chosen_anywhere()` and each `LocationTarget::ChosenByController` with `LocationTarget::chosen_anywhere()`.

In `crates/game-core/src/engine/dispatch/skill_test.rs:806`: update the doc-comment mention of `LocationTarget::ChosenByController` to `LocationTarget::Chosen`.

- [ ] **Step 6: Run the full strict gauntlet — everything green (behavior preserved)**

Run:
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: PASS. The migrated choice tests (`chosen_investigator_*`, `chosen_location_*`, `choose_one_then_chosen_target_*`, `two_choices_resume_*`) pass unchanged — `Anywhere` enumerates exactly what the bare stub did.

- [ ] **Step 7: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: unified Choose surface (V7) — migrate ChosenByController to Chosen(…Anywhere)"
```

---

### Task 2: scope-aware enumeration (`At(your location)`)

Make the resolver honor the scope: `EntityScope::At(LocationSet::Here)` offers only co-located investigators; `LocationSet::Here` offers only the chooser's location. `Anywhere` is unchanged.

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (add the `chosen_at_your_location()` constructor)
- Modify: `crates/game-core/src/engine/evaluator.rs` (thread the scope; add candidate helpers; the two `ground_*_choice` fns take a scope)
- Test: `crates/game-core/src/engine/evaluator.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `InvestigatorTarget::Chosen(Choose<EntityScope>)`, `EntityScope::At`, `LocationSet` (Task 1).
- Produces: `InvestigatorTarget::chosen_at_your_location() -> InvestigatorTarget`; scope-aware `ground_investigator_choice` / `ground_location_choice`.

- [ ] **Step 1: Write the failing test** (append to the evaluator `#[cfg(test)]` module)

```rust
#[test]
fn chosen_at_your_location_auto_binds_the_sole_co_located_investigator() {
    // Investigator 1 (controller) and 2 are in play; only 1 is at the
    // controller's location. `At(Here)` must offer only investigator 1 and
    // auto-bind it (1 candidate ⇒ no suspend) — `Anywhere` would see 2 and
    // suspend.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_investigator(test_investigator(2))
        .with_location(test_location(1, "A"))
        .with_location(test_location(2, "B"))
        .build();
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(LocationId(1));
    state.investigators.get_mut(&InvestigatorId(2)).unwrap().current_location = Some(LocationId(2));
    let before1 = state.investigators[&InvestigatorId(1)].resources;
    let mut events = Vec::new();
    let outcome = apply_effect(
        &mut Cx { state: &mut state, events: &mut events },
        &gain_resources(InvestigatorTarget::chosen_at_your_location(), 2),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].resources, before1 + 2);
    assert!(state.continuations.is_empty(), "single co-located candidate auto-binds");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core chosen_at_your_location_auto_binds_the_sole_co_located_investigator`
Expected: FAIL — `no associated function chosen_at_your_location` (and, once added, it would suspend rather than auto-bind because enumeration still returns all).

- [ ] **Step 3: Add the constructor** (`crates/card-dsl/src/dsl.rs`, in `impl InvestigatorTarget`)

```rust
    /// "Choose an investigator at your location."
    #[must_use]
    pub fn chosen_at_your_location() -> Self {
        InvestigatorTarget::Chosen(Choose {
            scope: EntityScope::At(LocationSet::Here),
        })
    }
```

- [ ] **Step 4: Thread the scope + implement constrained enumeration** (`crates/game-core/src/engine/evaluator.rs`)

In `ground_chosen_targets`, capture and forward the scope:

```rust
    if let Some(InvestigatorTarget::Chosen(choose)) = inv_target {
        if eval_ctx.chosen_investigator.is_none() {
            return ground_investigator_choice(cx, eval_ctx, cursor, choose.scope);
        }
    }

    if let Effect::DiscoverClue { from: LocationTarget::Chosen(choose), .. } = effect {
        if eval_ctx.chosen_location.is_none() {
            return ground_location_choice(cx, eval_ctx, cursor, choose.scope);
        }
    }
```

Change `ground_investigator_choice` to take `scope: EntityScope` and compute candidates via a helper (the `resolve_choice_count`/bind/suspend body is otherwise unchanged):

```rust
fn ground_investigator_choice(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    cursor: &mut DecisionCursor<'_>,
    scope: card_dsl::dsl::EntityScope,
) -> Result<EvalContext, EngineOutcome> {
    use crate::engine::dispatch::choice::{resolve_choice_count, suspend_for_choice, ChoiceResolution};
    let candidates = investigator_candidates(cx.state, eval_ctx.controller, scope);
    let bind = |id| { let mut ctx = eval_ctx; ctx.chosen_investigator = Some(id); Ok(ctx) };
    match resolve_choice_count(candidates.len()) {
        ChoiceResolution::Empty => Err(EngineOutcome::Rejected {
            reason: "Chosen investigator: no candidate in scope".into(),
        }),
        ChoiceResolution::Auto(i) => bind(candidates[i]),
        ChoiceResolution::Suspend => {
            if let Some(crate::engine::OptionId(i)) = cursor.take() {
                bind(candidates[i as usize])
            } else {
                let labels = candidates.iter().map(|id| format!("{id:?}")).collect();
                Err(suspend_for_choice(cx, "Choose an investigator", labels,
                    cursor.recorded_so_far(), cursor.root(), eval_ctx))
            }
        }
    }
}

/// Investigators matching an [`EntityScope`], in `BTreeMap` (id) order so the
/// `OptionId` index replays deterministically.
fn investigator_candidates(
    state: &GameState,
    controller: crate::state::InvestigatorId,
    scope: card_dsl::dsl::EntityScope,
) -> Vec<crate::state::InvestigatorId> {
    use card_dsl::dsl::{EntityScope, LocationSet};
    let EntityScope::At(set) = scope;
    match set {
        LocationSet::Anywhere => state.investigators.keys().copied().collect(),
        LocationSet::Here => match state
            .investigators
            .get(&controller)
            .and_then(|i| i.current_location)
        {
            Some(here) => state
                .investigators
                .iter()
                .filter(|(_, inv)| inv.current_location == Some(here))
                .map(|(id, _)| *id)
                .collect(),
            None => Vec::new(), // controller is between locations ⇒ no "your location"
        },
    }
}
```

Apply the symmetric change to `ground_location_choice` (takes `scope: LocationSet`, uses a `location_candidates` helper):

```rust
/// Locations matching a [`LocationSet`], in `BTreeMap` (id) order.
fn location_candidates(
    state: &GameState,
    controller: crate::state::InvestigatorId,
    set: card_dsl::dsl::LocationSet,
) -> Vec<crate::state::LocationId> {
    use card_dsl::dsl::LocationSet;
    match set {
        LocationSet::Anywhere => state.locations.keys().copied().collect(),
        LocationSet::Here => state
            .investigators
            .get(&controller)
            .and_then(|i| i.current_location)
            .into_iter()
            .collect(), // the singleton your-location, or empty
    }
}
```

- [ ] **Step 5: Run the new test — passes**

Run: `cargo test -p game-core chosen_at_your_location_auto_binds_the_sole_co_located_investigator`
Expected: PASS.

- [ ] **Step 6: Add the suspend + reject coverage** (append to the test module)

```rust
#[test]
fn chosen_at_your_location_suspends_when_two_are_co_located() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_investigator(test_investigator(2))
        .with_location(test_location(1, "A"))
        .build();
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(LocationId(1));
    state.investigators.get_mut(&InvestigatorId(2)).unwrap().current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = apply_effect(
        &mut Cx { state: &mut state, events: &mut events },
        &gain_resources(InvestigatorTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    match state.continuations.last() {
        Some(crate::state::Continuation::Choice(frame)) => {
            assert_eq!(frame.offered.len(), 2, "two co-located investigators offered");
        }
        other => panic!("expected a Choice frame, got {other:?}"),
    }
}

#[test]
fn chosen_at_your_location_rejects_when_controller_between_locations() {
    // test_investigator defaults to current_location = None.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    let mut events = Vec::new();
    let outcome = apply_effect(
        &mut Cx { state: &mut state, events: &mut events },
        &gain_resources(InvestigatorTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(state.continuations.is_empty());
}
```

Run: `cargo test -p game-core chosen_at_your_location`
Expected: PASS (all three).

- [ ] **Step 7: Full strict gauntlet, then commit**

Run the four strict commands from Global Constraints; expect PASS. Then:

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: At(your-location) constrained choice enumeration (#349)"
```

---

### Task 3: phase-7 doc update + milestone (final commit, after CI is green)

Per the repo PR procedure, the phase-doc update is the **final** commit, made only once CI is green on the opened PR so it reflects the shipping state.

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Pull #349 onto the milestone**

Run: `gh issue edit 349 --milestone "phase-7-the-gathering"`

- [ ] **Step 2: Update the phase-7 doc**

In `docs/phases/phase-7-the-gathering.md`, under the trigger-dispatch-rework "Axis A" / future-slices area, add the keystone as shipped and record one Decisions entry (apply the "would a future PR-author choose differently without this?" test):

> **Axis-E choice surface unifies on the spatial vocabulary, not the monolithic `{variety,constraint,chooser}` (#349, PR #NN).** `Choose<S>` wraps a scope; `LocationSet{Here,Anywhere}` (chooser-relative) is reused directly for location-picks and via `EntityScope::At(LocationSet)` for entity-position-filters, so "your location" is defined once and illegal pairs (a location "at your location") are unrepresentable — no runtime guard. Variety stays typed at the effect target. `chooser` is deferred (latent in solo; 01105's lead choice rides the forced `controller = lead` binding). Enemy variety → #301; `YourOrConnecting` + adjacency → #306. Reuses Axis A's `Choice` frame / resolve convention unchanged.

Move #349 to the Closed table / flip its Arc row per `docs/phases/README.md` ("Maintaining these docs").

- [ ] **Step 3: Commit**

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — Axis-A choice surface keystone (#349)"
```

---

## Self-Review

**Spec coverage** (against the keystone design doc): §1 surface → Task 1 Step 3-4; migration → Task 1 Step 4-5; §2 resolver constrained enumeration → Task 2 Step 4; §3 scope boundary (enemy/YourOrConnecting/chooser excluded) → Global Constraints; §4 Axis-A reuse-unchanged → Task 1 keeps `resolve_choice_count`/frame untouched; §6 testing (migration-preserving + constrained pick + reject) → Task 1 Step 6, Task 2 Steps 1/6; §7 sequencing → the three tasks; phase-doc → Task 3.

**Placeholder scan:** none — every code step shows full code; the only `#NN` is the PR number, filled at doc time.

**Type consistency:** `Choose<S>{scope}`, `LocationSet{Here,Anywhere}`, `EntityScope::At(LocationSet)`, `chosen_anywhere()`/`chosen_at_your_location()`, `ground_investigator_choice(.., scope: EntityScope)`, `ground_location_choice(.., scope: LocationSet)`, `investigator_candidates(state, controller, EntityScope)`, `location_candidates(state, controller, LocationSet)` — names consistent across tasks. `current_location: Option<LocationId>` matches `state/investigator.rs`.
