# Barricade 01038 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Barricade 01038 — a played event that attaches to a location, blocks non-Elite enemy movement into it, and self-discards when an investigator leaves.

**Architecture:** Three precedented engine pieces plus the card: `Effect::AttachSelfToLocation` re-homes the played event (consuming `pending_played_event`, no duplicate); a constant `Restriction::EnemyMovementBlocked` read by hunter pathfinding (graph-level impassability for non-Elite enemies); and a `LeftLocation` forced trigger that fires `DiscardSelf`. All ride existing machinery (`attach_to_location`, the inspectable-`Restriction` pattern, `collect_forced_hits`, BFS pathfinding).

**Tech Stack:** Rust workspace — `card-dsl` (DSL types), `game-core` (kernel/evaluator/dispatch/pathfinding), `cards` (content). Tests: `cargo test`, per-card `#[cfg(test)]`, integration tests in `crates/cards/tests/`.

## Global Constraints

- CI runs `fmt`, `clippy`, `test`, `doc`, `wasm-build`, `wasm-test`, `wasm-clippy`, all warnings-as-errors. Match strict flags locally before pushing:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown` + `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- `card-dsl` sits below `game-core`; new `Effect`/`EventPattern`/`Restriction` variants derive the enum's existing set (`Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize`). Escape card text like `\[reaction\]`/`` `[reaction]` `` in doc comments (rustdoc parses `[x]` as an intra-doc link — the `doc` CI job fails otherwise).
- Handler contract: validate-first / mutate-second.
- Card text verbatim from `data/arkhamdb-snapshot/pack/core/core.json`; rules from `data/rules-reference/ahc01_rules_reference_web.pdf`.
- Deferred multiplayer ownership: [#371](https://github.com/talelburg/eldritch/issues/371) (`TODO(#371)` on the solo discard routing).
- Branch `engine/barricade` (already created). One PR. Commit subjects `scope: description`; bodies end with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

---

### Task 1: DSL — `AttachSelfToLocation`, `Restriction::EnemyMovementBlocked`, `EventPattern::LeftLocation`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (Effect enum ~line 735; Restriction enum; EventPattern enum ~line 372; builders)
- Test: `crates/card-dsl/src/dsl.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `Effect::AttachSelfToLocation`; `pub fn attach_self_to_location() -> Effect`; `Restriction::EnemyMovementBlocked`; `EventPattern::LeftLocation`.

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)]` module of `crates/card-dsl/src/dsl.rs`, add:

```rust
#[test]
fn barricade_dsl_variants_round_trip() {
    use crate::dsl::{attach_self_to_location, restrict, Restriction};
    let attach = attach_self_to_location();
    assert_eq!(attach, Effect::AttachSelfToLocation);
    let block = restrict(Restriction::EnemyMovementBlocked);
    for e in [attach, block] {
        let json = serde_json::to_string(&e).expect("ser");
        assert_eq!(e, serde_json::from_str::<Effect>(&json).expect("de"));
    }
    let pat = EventPattern::LeftLocation;
    let json = serde_json::to_string(&pat).expect("ser");
    assert_eq!(pat, serde_json::from_str::<EventPattern>(&json).expect("de"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p card-dsl barricade_dsl_variants_round_trip`
Expected: FAIL — variants/builder not found.

- [ ] **Step 3: Add the `Effect` variant + builder**

In `crates/card-dsl/src/dsl.rs`, after the `Effect::SearchDeck { … }` variant (before the `}` closing `pub enum Effect`), add:

```rust
    /// The currently-playing event attaches itself to its controller's
    /// current location (Barricade 01038): consume the
    /// [`pending_played_event`](crate::state) and re-home that same card into
    /// the location's attachment zone, instead of letting it discard. One card
    /// — hand → location attachment → (on a later effect) discard; no
    /// duplicate spawned by code (cf. [`PutIntoThreatArea`](Self::PutIntoThreatArea),
    /// which spawns by code only because an *encounter* card has no instance at
    /// Revelation time).
    AttachSelfToLocation,
```

After the `draw_cards` / `search_deck` builders (~line 1311), add:

```rust
/// Build an [`Effect::AttachSelfToLocation`].
#[must_use]
pub fn attach_self_to_location() -> Effect {
    Effect::AttachSelfToLocation
}
```

- [ ] **Step 4: Add the `Restriction` variant**

In the `pub enum Restriction { … }` block, after the `ExtraActionCost { … }` variant, add:

```rust
    /// Non-Elite enemies cannot move into the location this restriction's
    /// source is attached to (Barricade 01038). **Inspected, not executed** —
    /// hunter pathfinding (`engine::dispatch::hunters`) treats a location
    /// carrying this restriction as impassable for non-Elite enemies. The
    /// Elite exemption (RR: most movement-blockers exempt Elite) is applied at
    /// the read site, which has the moving enemy's traits.
    EnemyMovementBlocked,
```

- [ ] **Step 5: Add the `EventPattern` variant**

In `pub enum EventPattern { … }`, after `EnteredPlay` (added in the deck-search PR), add:

```rust
    /// An investigator left the location this ability's source is attached to
    /// (Barricade 01038's "Forced — When an investigator leaves attached
    /// location"). Bare and forced-only: the engine binds the leaving
    /// investigator (controller) and scans the *left* location's attachment
    /// zone. Matched only by the forced dispatch path
    /// (`ForcedTriggerPoint::LeftLocation`), never a reaction window.
    LeftLocation,
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p card-dsl barricade_dsl_variants_round_trip`
Expected: PASS. (`game-core` will not compile yet — its exhaustive matches don't cover the new variants; later tasks fix that. `cargo test -p card-dsl` is isolated.)

- [ ] **Step 7: Commit**

```bash
git add crates/card-dsl/src/dsl.rs
git commit -m "card-dsl: AttachSelfToLocation + EnemyMovementBlocked + LeftLocation

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Pathfinding — passability-predicate variants

**Files:**
- Modify: `crates/game-core/src/engine/pathfinding.rs`
- Test: `crates/game-core/src/engine/pathfinding.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces:
  - `pub(crate) fn bfs_distance_with(state: &GameState, from: LocationId, to: LocationId, is_passable: impl Fn(LocationId) -> bool) -> Option<u32>`
  - `pub fn shortest_first_steps_with(state: &GameState, from: LocationId, to: LocationId, is_passable: impl Fn(LocationId) -> bool) -> Vec<LocationId>`
  - existing `bfs_distance` / `shortest_first_steps` keep their signatures (delegate with `|_| true`).

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)]` module of `crates/game-core/src/engine/pathfinding.rs`, add (the `diamond()` helper already exists in that module):

```rust
#[test]
fn impassable_node_reroutes_distance_and_steps() {
    // Diamond A-B-D / A-C-D. Block B ⇒ the only route to D is via C.
    let s = diamond();
    let block_b = |loc: LocationId| loc != LocationId(2);
    assert_eq!(
        bfs_distance_with(&s, LocationId(1), LocationId(4), block_b),
        Some(2),
        "still distance 2 via C",
    );
    assert_eq!(
        shortest_first_steps_with(&s, LocationId(1), LocationId(4), block_b),
        vec![LocationId(3)],
        "only C is a legal first step",
    );
}

#[test]
fn impassable_destination_is_unreachable() {
    // Linear A-B. Block B (the destination) ⇒ unreachable.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    a.connections = vec![LocationId(2)];
    b.connections = vec![LocationId(1)];
    let s = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .build();
    assert_eq!(
        bfs_distance_with(&s, LocationId(1), LocationId(2), |loc| loc != LocationId(2)),
        None,
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core impassable_`
Expected: FAIL — `bfs_distance_with` / `shortest_first_steps_with` not found.

- [ ] **Step 3: Add the predicate variants and delegate the old fns**

In `crates/game-core/src/engine/pathfinding.rs`, replace the bodies of `bfs_distance` and `shortest_first_steps` so they delegate, and add the `_with` variants:

```rust
/// Breadth-first distance over the connection graph, skipping any location
/// for which `is_passable` returns `false` (an impassable node is never
/// entered — including the destination, which is then unreachable). The
/// `from` node is always the start regardless of `is_passable`.
pub(crate) fn bfs_distance_with(
    state: &GameState,
    from: LocationId,
    to: LocationId,
    is_passable: impl Fn(LocationId) -> bool,
) -> Option<u32> {
    if from == to {
        return Some(0);
    }
    let mut seen: BTreeMap<LocationId, u32> = BTreeMap::new();
    seen.insert(from, 0);
    let mut queue: VecDeque<LocationId> = VecDeque::new();
    queue.push_back(from);
    while let Some(cur) = queue.pop_front() {
        let dist = seen[&cur];
        let Some(loc) = state.locations.get(&cur) else {
            continue;
        };
        for &next in &loc.connections {
            if !is_passable(next) {
                continue;
            }
            if next == to {
                return Some(dist + 1);
            }
            if let std::collections::btree_map::Entry::Vacant(e) = seen.entry(next) {
                e.insert(dist + 1);
                queue.push_back(next);
            }
        }
    }
    None
}

/// Edge-count distance over the connection graph (every node passable).
pub(crate) fn bfs_distance(state: &GameState, from: LocationId, to: LocationId) -> Option<u32> {
    bfs_distance_with(state, from, to, |_| true)
}

/// Every neighbor of `from` on a shortest path to `to`, skipping impassable
/// nodes (so a barricaded location is neither a step nor a path waypoint).
pub fn shortest_first_steps_with(
    state: &GameState,
    from: LocationId,
    to: LocationId,
    is_passable: impl Fn(LocationId) -> bool,
) -> Vec<LocationId> {
    let Some(total) = bfs_distance_with(state, from, to, &is_passable) else {
        return Vec::new();
    };
    if total == 0 {
        return Vec::new();
    }
    let Some(loc) = state.locations.get(&from) else {
        return Vec::new();
    };
    loc.connections
        .iter()
        .copied()
        .filter(|&n| is_passable(n) && bfs_distance_with(state, n, to, &is_passable) == Some(total - 1))
        .collect()
}

/// Neighbors of `from` on a shortest path to `to` (every node passable).
pub fn shortest_first_steps(state: &GameState, from: LocationId, to: LocationId) -> Vec<LocationId> {
    shortest_first_steps_with(state, from, to, |_| true)
}
```

(`&is_passable` is passed to `bfs_distance_with` because `&F` implements `Fn` when `F: Fn`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core impassable_ && cargo test -p game-core --lib pathfinding`
Expected: PASS (new tests + the existing `diamond`/linear tests unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/pathfinding.rs
git commit -m "engine: bfs_distance_with / shortest_first_steps_with passability predicate

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Evaluator — `apply_attach_self_to_location`

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (dispatch arm ~line 378; new handler)
- Test: covered by Task 7's integration test (needs a played event + registry); a unit assertion is added here for the no-pending reject path.

**Interfaces:**
- Consumes: `Effect::AttachSelfToLocation`; `GameState.pending_played_event: Option<(InvestigatorId, CardCode)>`; `crate::engine::dispatch::threat_area::attach_to_location(cx, LocationId, CardCode) -> Option<CardInstanceId>`.
- Produces: the `Effect::AttachSelfToLocation` evaluator behavior.

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)]` module of `crates/game-core/src/engine/evaluator.rs`, add:

```rust
#[test]
fn attach_self_to_location_rejects_with_no_pending_event() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    let mut events = Vec::new();
    let outcome = apply_effect(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &Effect::AttachSelfToLocation,
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core attach_self_to_location_rejects`
Expected: FAIL — `Effect::AttachSelfToLocation` not handled (non-exhaustive match).

- [ ] **Step 3: Add the dispatch arm + handler**

In `apply_effect_inner`'s match (after the `Effect::SearchDeck { … }` arm added by the deck-search PR), add:

```rust
        Effect::AttachSelfToLocation => apply_attach_self_to_location(cx),
```

Add the handler near `apply_search_deck`:

```rust
/// Resolve [`Effect::AttachSelfToLocation`]: the currently-playing event
/// (held in `pending_played_event`) attaches itself to its controller's
/// current location, and is **consumed** from the pending slot so the apply
/// loop's `flush_pending_played_event` does not also discard it — one card,
/// no duplicate. Rejects if no event is mid-play or the controller is between
/// locations.
fn apply_attach_self_to_location(cx: &mut Cx) -> EngineOutcome {
    let Some((investigator, code)) = cx.state.pending_played_event.clone() else {
        return EngineOutcome::Rejected {
            reason: "AttachSelfToLocation: no event is mid-play".into(),
        };
    };
    let Some(location) = cx
        .state
        .investigators
        .get(&investigator)
        .and_then(|i| i.current_location)
    else {
        return EngineOutcome::Rejected {
            reason: "AttachSelfToLocation: controller has no current location".into(),
        };
    };
    crate::engine::dispatch::threat_area::attach_to_location(cx, location, code);
    // Consume the pending event so it is re-homed, not discarded.
    cx.state.pending_played_event = None;
    EngineOutcome::Done
}
```

- [ ] **Step 4: Run test + build**

Run: `cargo test -p game-core attach_self_to_location_rejects && cargo build -p game-core`
Expected: PASS / clean build.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs
git commit -m "engine: Effect::AttachSelfToLocation evaluator handler

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `DiscardSelf` — route a player-card attachment to the owner's discard

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`discard_self`, the `att_owner` branch)
- Test: `crates/game-core/src/engine/evaluator.rs` (`#[cfg(test)]`) for the encounter-card path stays; player-card routing is covered by Task 7's integration test (needs registry metadata).

**Interfaces:**
- Consumes: `CardMetadata::card_type() -> CardType`; `crate::card_registry::current()`.
- Produces: `DiscardSelf` routing a player-card-type (`Asset | Event | Skill`) location attachment to `investigators[controller].discard`, an encounter/unknown one to `encounter_discard` (unchanged).

- [ ] **Step 1: Note the existing test stays green**

The existing `discard_self_*` tests use encounter codes (01165/01168, `CardType::Treachery`) with no registry installed, so they must keep routing to `encounter_discard`. No new game-core test here (player-card routing needs real metadata — Task 7's integration test covers it); this task must not regress the encounter path.

- [ ] **Step 2: Edit the `att_owner` branch**

In `discard_self` (`crates/game-core/src/engine/evaluator.rs`), replace the location-attachment branch's discard so it routes by card class:

```rust
    if let Some((loc_id, pos)) = att_owner {
        let card = cx
            .state
            .locations
            .get_mut(&loc_id)
            .expect("found above")
            .attachments
            .remove(pos);
        // A player-card-type attachment (Barricade 01038 — `Event`) goes to
        // its owner's player discard; an encounter attachment (Obscuring Fog
        // 01168 — `Treachery`) to the encounter discard. Without a registry the
        // type is unknown, so default to the encounter discard (preserves the
        // pre-Barricade behavior).
        let is_player_card = crate::card_registry::current()
            .and_then(|reg| (reg.metadata_for)(&card.code))
            .is_some_and(|m| {
                matches!(
                    m.card_type(),
                    crate::card_data::CardType::Asset
                        | crate::card_data::CardType::Event
                        | crate::card_data::CardType::Skill
                )
            });
        if is_player_card {
            // Solo: the firing controller is the owner. TODO(#371): track the
            // attachment's owner for multiplayer (owner may differ from the
            // leaving investigator).
            if let Some(inv) = cx.state.investigators.get_mut(&eval_ctx.controller) {
                inv.discard.push(card.code.clone());
            }
        } else {
            cx.state.encounter_discard.push(card.code.clone());
        }
        cx.events.push(Event::CardDiscarded {
            investigator: eval_ctx.controller,
            code: card.code,
            from: Zone::LocationAttachment,
        });
        return EngineOutcome::Done;
    }
```

- [ ] **Step 3: Run the existing discard_self tests**

Run: `cargo test -p game-core discard_self`
Expected: PASS (encounter-path tests unchanged — no registry installed in them ⇒ `is_player_card == false` ⇒ encounter_discard).

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs
git commit -m "engine: DiscardSelf routes a player-card attachment to owner discard (#371)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Hunters — non-Elite movement block

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/hunters.rs` (`hunter_destinations` signature + body; `process_one_hunter` call; new helpers; imports)
- Test: `crates/game-core/src/engine/dispatch/hunters.rs` (`#[cfg(test)]`) — uses real registry codes, so gate with `card_registry::current()`-installed setup or place in Task 7's integration test. Engine-only behavior (predicate plumbing) is tested in Task 2; this task's card-data behavior is tested in Task 7.

**Interfaces:**
- Consumes: `bfs_distance_with` / `shortest_first_steps_with` (Task 2); `Restriction::EnemyMovementBlocked` (Task 1); `Enemy.code`; `CardMetadata.traits`.
- Produces: `hunter_destinations(state, from, prey, enemy_is_elite: bool)`; non-Elite hunters never path into a `EnemyMovementBlocked` location.

- [ ] **Step 1: Update imports + add helpers**

In `crates/game-core/src/engine/dispatch/hunters.rs`, change the pathfinding import to the `_with` variants and add the DSL imports:

```rust
use crate::dsl::{Effect, Restriction, Stat, Trigger};
use crate::engine::pathfinding::{bfs_distance_with, shortest_first_steps_with};
```

(`Stat` was already imported; merge into the one `use crate::dsl::{…}` line. Remove the old `bfs_distance, shortest_first_steps` import — if any other fn in the file still uses the unfiltered ones, keep them imported too; grep first.)

Add helpers near `is_eligible_hunter`:

```rust
/// Whether `enemy` is Elite — read from its printed traits
/// (`CardMetadata.traits` contains `"Elite"`). `false` with no registry or no
/// metadata (treated as non-Elite, hence subject to movement blocks).
fn enemy_is_elite(enemy: &Enemy) -> bool {
    card_registry::current()
        .and_then(|reg| (reg.metadata_for)(&enemy.code))
        .is_some_and(|m| m.traits.iter().any(|t| t == "Elite"))
}

/// Whether `loc` carries a constant `EnemyMovementBlocked` restriction (a
/// Barricade 01038 attachment) — read the way `play_is_prohibited` reads
/// constant restrictions. `false` with no registry.
fn location_blocks_enemy_movement(state: &GameState, loc: LocationId) -> bool {
    let Some(reg) = card_registry::current() else {
        return false;
    };
    let Some(location) = state.locations.get(&loc) else {
        return false;
    };
    location.attachments.iter().any(|att| {
        (reg.abilities_for)(&att.code)
            .into_iter()
            .flatten()
            .any(|a| {
                a.trigger == Trigger::Constant
                    && matches!(&a.effect, Effect::Restrict(Restriction::EnemyMovementBlocked))
            })
    })
}
```

- [ ] **Step 2: Thread the predicate through `hunter_destinations`**

Change the signature and the two pathfinding calls inside `hunter_destinations`:

```rust
fn hunter_destinations(
    state: &GameState,
    from: LocationId,
    prey: Prey,
    enemy_is_elite: bool,
) -> Vec<LocationId> {
    // A barricaded location is impassable to a non-Elite enemy — graph-level,
    // so it shifts which investigator is nearest, not just the final step.
    let is_passable = |loc: LocationId| enemy_is_elite || !location_blocks_enemy_movement(state, loc);
    let mut reachable: Vec<(InvestigatorId, u32)> = Vec::new();
    let mut min_dist: Option<u32> = None;
    for id in &state.turn_order {
        let Some(inv) = state.investigators.get(id) else {
            continue;
        };
        if inv.status != crate::state::Status::Active {
            continue;
        }
        let Some(loc) = inv.current_location else {
            continue;
        };
        let Some(d) = bfs_distance_with(state, from, loc, is_passable) else {
            continue;
        };
        min_dist = Some(min_dist.map_or(d, |m| m.min(d)));
        reachable.push((*id, d));
    }
    let Some(min) = min_dist else {
        return Vec::new();
    };
    let nearest_ids: Vec<InvestigatorId> = reachable
        .iter()
        .filter(|(_, d)| *d == min)
        .map(|(id, _)| *id)
        .collect();
    let chosen: Vec<InvestigatorId> = match resolve_prey(state, prey, &nearest_ids) {
        PreyResolution::One(id) => vec![id],
        PreyResolution::Tie(v) => v,
        PreyResolution::None => return Vec::new(),
    };
    let mut dests: Vec<LocationId> = Vec::new();
    for id in chosen {
        let Some(loc) = state
            .investigators
            .get(&id)
            .and_then(|i| i.current_location)
        else {
            continue;
        };
        for step in shortest_first_steps_with(state, from, loc, is_passable) {
            if !dests.contains(&step) {
                dests.push(step);
            }
        }
    }
    dests.sort();
    dests
}
```

- [ ] **Step 3: Pass elite-ness at the `process_one_hunter` call site**

In `process_one_hunter`, replace the `hunter_destinations` call:

```rust
        let prey = cx.state.enemies[&enemy_id].prey;
        let enemy_is_elite = enemy_is_elite(&cx.state.enemies[&enemy_id]);
        let dests = hunter_destinations(cx.state, from, prey, enemy_is_elite);
```

- [ ] **Step 4: Build + run the existing hunter tests**

Run: `cargo build -p game-core && cargo test -p game-core --lib hunters`
Expected: clean build; existing hunter tests still pass (no registry installed ⇒ `is_passable` is always-true ⇒ unchanged behavior). The non-Elite-block / Elite-passes / nearest-prey-shift behavior is asserted end-to-end in Task 7.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/hunters.rs
git commit -m "engine: non-Elite hunters cannot path into a barricaded location

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: `LeftLocation` forced trigger

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`ForcedTriggerPoint` enum + `collect_forced_hits`)
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` (`TimingEvent` + its 3 methods)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` (`move_action` tail)
- Test: Task 7's integration test (leave-discards) — a move + a registry are required.

**Interfaces:**
- Produces: `ForcedTriggerPoint::LeftLocation { investigator, location }`; `TimingEvent::LeftLocation { investigator, location }`; emit from `move_action`.

- [ ] **Step 1: Add the `ForcedTriggerPoint` variant + collect arm**

In `crates/game-core/src/engine/dispatch/forced_triggers.rs`, add to `ForcedTriggerPoint` (after `AfterLocationInvestigated`):

```rust
    /// An investigator left a location. Scans that location's attachment zone
    /// for `EventPattern::LeftLocation` forced abilities (Barricade 01038's
    /// self-discard); binds controller = the leaving investigator, source =
    /// the firing attachment instance. Mirrors the attachment scan in
    /// `AfterLocationInvestigated`.
    LeftLocation {
        /// The investigator who left.
        investigator: InvestigatorId,
        /// The location they left.
        location: LocationId,
    },
```

In `collect_forced_hits`, add the arm (mirroring the `AfterLocationInvestigated` attachment scan but attachments-only):

```rust
        ForcedTriggerPoint::LeftLocation {
            investigator,
            location,
        } => {
            if let Some(loc) = state.locations.get(location) {
                for att in &loc.attachments {
                    push_matching(
                        reg,
                        &att.code,
                        *investigator,
                        Some(att.instance_id),
                        &mut hits,
                        |p| matches!(p, EventPattern::LeftLocation),
                    );
                }
            }
        }
```

- [ ] **Step 2: Add the `TimingEvent` variant + wire its 3 methods**

In `crates/game-core/src/engine/dispatch/emit.rs`, add to `TimingEvent` (after `EnteredPlay`):

```rust
    /// An investigator left a location (forced only — Barricade 01038's
    /// self-discard). Scans the left location's attachment zone.
    LeftLocation {
        investigator: InvestigatorId,
        location: LocationId,
    },
```

In `forced_point`, add:

```rust
            TimingEvent::LeftLocation {
                investigator,
                location,
            } => Some(ForcedTriggerPoint::LeftLocation {
                investigator: *investigator,
                location: *location,
            }),
```

`reaction_window`: `LeftLocation` opens none — it falls into the existing `_ => None` arm, no edit needed.

In `forced_continuation`, add `LeftLocation` to the non-terminal-unwired `None` group (no in-scope site produces 2+ forced here, so the loud guard is correct):

```rust
            TimingEvent::PhaseEnded { .. }
            | TimingEvent::ActAdvanced { .. }
            | TimingEvent::AgendaAdvanced { .. }
            | TimingEvent::EnemyDefeated { .. }
            | TimingEvent::GameEnd
            | TimingEvent::EnemyAttackDamagedSelf { .. }
            | TimingEvent::SuccessfullyInvestigated { .. }
            | TimingEvent::LeftLocation { .. }
            | TimingEvent::EnemyAttacks { .. }
            | TimingEvent::EnteredPlay { .. }
            | TimingEvent::WouldDiscoverClues { .. } => None,
```

- [ ] **Step 3: Emit from `move_action`**

In `crates/game-core/src/engine/dispatch/actions.rs`, the move's tail currently ends with the `EnteredLocation` emit. Emit `LeftLocation` for the *from* location first (you leave, then arrive), chaining the outcome:

```rust
    // The leaving investigator left `from`: fire any "when an investigator
    // leaves attached location" forced abilities (Barricade 01038 discards
    // itself). In scope this is a single deterministic self-discard, so it
    // resolves synchronously; a 2+-forced suspend at this point is out of
    // Slice-1 scope (emit_event's loud guard, like the other non-terminal
    // forced sites).
    let left = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::LeftLocation {
            investigator,
            location: from,
        },
    );
    if !matches!(left, EngineOutcome::Done) {
        return left;
    }
    super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::EnteredLocation {
            investigator,
            location: destination,
        },
    )
```

(Replaces the single trailing `EnteredLocation` emit — keep the existing doc comment above it.)

- [ ] **Step 4: Build + run move/forced tests**

Run: `cargo build -p game-core && cargo test -p game-core --lib actions && cargo test -p game-core --lib forced`
Expected: clean build; existing move tests pass (no attachments ⇒ `LeftLocation` finds no hits ⇒ `Done` ⇒ proceeds to `EnteredLocation` as before).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/emit.rs crates/game-core/src/engine/dispatch/actions.rs
git commit -m "engine: LeftLocation forced trigger (scans the left location's attachments)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Barricade 01038 card + register + tests

**Files:**
- Create: `crates/cards/src/impls/barricade.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`pub mod` + `abilities_for` arm)
- Test: `crates/cards/src/impls/barricade.rs` (`#[cfg(test)]`) + `crates/cards/tests/barricade.rs`

**Interfaces:**
- Consumes: `attach_self_to_location`, `restrict`, `Restriction::EnemyMovementBlocked`, `discard_self`, `on_play`, `constant`, `forced_on_event`, `EventPattern::LeftLocation`, `EventTiming` (Task 1).
- Produces: `barricade::CODE = "01038"`, `barricade::abilities()`.

- [ ] **Step 1: Create the card module + unit test**

Create `crates/cards/src/impls/barricade.rs`:

```rust
//! Barricade (Seeker event, 01038).
//!
//! ```text
//! Insight. Tactic.
//! Attach to your location.
//! Non-Elite enemies cannot move into attached location.
//! Forced - When an investigator leaves attached location: Discard Barricade.
//! ```
//!
//! Three abilities on one card: `OnPlay` attaches the played event to the
//! controller's location ([`Effect::AttachSelfToLocation`] — one card, no
//! duplicate); a `Constant` [`Restriction::EnemyMovementBlocked`] (inspected by
//! hunter pathfinding — non-Elite enemies cannot path into the attached
//! location); and a `Forced` self-discard when an investigator leaves the
//! attached location ([`EventPattern::LeftLocation`] → [`Effect::DiscardSelf`],
//! routed to the owner's player discard).

use card_dsl::dsl::{
    attach_self_to_location, constant, discard_self, forced_on_event, on_play, restrict, Ability,
    EventPattern, EventTiming, Restriction,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01038";

/// Attach-on-play, the constant non-Elite movement block, and the
/// leave-location forced self-discard.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        on_play(attach_self_to_location()),
        constant(restrict(Restriction::EnemyMovementBlocked)),
        forced_on_event(
            EventPattern::LeftLocation,
            EventTiming::After,
            discard_self(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, Restriction, Trigger, TriggerKind};

    #[test]
    fn abilities_are_attach_block_and_leave_discard() {
        let a = super::abilities();
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].trigger, Trigger::OnPlay);
        assert_eq!(a[0].effect, Effect::AttachSelfToLocation);
        assert_eq!(a[1].trigger, Trigger::Constant);
        assert_eq!(
            a[1].effect,
            Effect::Restrict(Restriction::EnemyMovementBlocked)
        );
        assert!(matches!(
            a[2].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::LeftLocation,
                kind: TriggerKind::Forced,
                ..
            }
        ));
        assert_eq!(a[2].effect, Effect::DiscardSelf);
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/cards/src/impls/mod.rs`, add `pub mod barricade;` in name order (after `automatic_45` / before `beat_cop`), and the `abilities_for` arm in the same position:

```rust
        barricade::CODE => Some(barricade::abilities()),
```

- [ ] **Step 3: Run the unit test**

Run: `cargo test -p cards barricade`
Expected: PASS.

- [ ] **Step 4: Write the integration tests**

First verify the codes used resolve in the corpus with the expected traits:

Run: `cargo run -p card-data-pipeline >/dev/null 2>&1; grep -c '01116\|01160' crates/cards/src/generated/cards.rs`
(If 01116 / 01160 aren't in the generated corpus, pick another in-corpus Elite enemy for the Elite test and an in-corpus non-Elite enemy for the block test — verify traits against `data/arkhamdb-snapshot/pack/`. 01116 Ghoul Priest = `Humanoid. Monster. Ghoul. Elite.`; 01160 Ghoul Minion = `Humanoid. Monster. Ghoul.`)

Create `crates/cards/tests/barricade.rs`:

```rust
//! #323 integration: Barricade 01038's attach / non-Elite movement block /
//! leave-location self-discard, end-to-end against the real `cards::REGISTRY`.
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, Enemy, EnemyId, InvestigatorId, LocationId, Phase};
use game_core::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};
use game_core::{apply, Action, PlayerAction};

const BARRICADE: &str = "01038";
const GHOUL_PRIEST: &str = "01116"; // Elite + Hunter
const GHOUL_MINION: &str = "01160"; // non-Elite
const INV: InvestigatorId = InvestigatorId(1);
const A: LocationId = LocationId(1);
const B: LocationId = LocationId(2);

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Hunter enemy `id` (code `code`) at location `at`, ready and unengaged.
fn hunter(id: u32, code: &str, at: LocationId) -> Enemy {
    let mut e = test_enemy(id, "Hunter");
    e.code = CardCode::new(code);
    e.hunter = true;
    e.current_location = Some(at);
    e.engaged_with = None;
    e.exhausted = false;
    e
}

#[test]
fn playing_barricade_attaches_one_card_and_does_not_discard_the_event() {
    install();
    let mut inv = test_investigator(1);
    inv.current_location = Some(A);
    inv.hand = vec![CardCode::new(BARRICADE)];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(test_location(1, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .build();

    let r = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    // Exactly one Barricade: attached to the location, none in hand/discard.
    let loc = &r.state.locations[&A];
    assert_eq!(
        loc.attachments
            .iter()
            .filter(|c| c.code == CardCode::new(BARRICADE))
            .count(),
        1,
        "attached once",
    );
    assert!(r.state.investigators[&INV].hand.is_empty(), "left hand");
    assert!(
        r.state.investigators[&INV].discard.is_empty(),
        "not discarded (re-homed, not duplicated)",
    );
    game_core::assert_event!(r.events, Event::CardAttachedToLocation { .. });
}

/// Linear map A—B with a Barricade attached at B; a hunter at A whose prey is
/// at B. `enemy_code` decides whether it's blocked (non-Elite) or not (Elite).
fn map_with_barricade_at_b(enemy_code: &str) -> game_core::GameState {
    install();
    let mut inv = test_investigator(1);
    inv.current_location = Some(B);
    let mut a = test_location(1, "A");
    a.connections = vec![B];
    let mut b = test_location(2, "B");
    b.connections = vec![A];
    // Attach Barricade at B directly (skip the play step — tested above).
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_investigator(inv)
        .with_location(a)
        .with_location(b)
        .with_enemy(hunter(100, enemy_code, A))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .build();
    let inst = game_core::state::CardInstanceId(900);
    state.locations.get_mut(&B).unwrap().attachments.push(
        game_core::state::CardInPlay::enter_play(CardCode::new(BARRICADE), inst),
    );
    state
}

#[test]
fn non_elite_hunter_cannot_enter_the_barricaded_location() {
    let state = map_with_barricade_at_b(GHOUL_MINION);
    let r = game_core::test_support::drive_enemy_phase_hunters(state);
    assert_eq!(
        r.state.enemies[&EnemyId(100)].current_location,
        Some(A),
        "non-Elite hunter stayed (only path is into the barricaded location)",
    );
}

#[test]
fn elite_hunter_enters_the_barricaded_location() {
    let state = map_with_barricade_at_b(GHOUL_PRIEST);
    let r = game_core::test_support::drive_enemy_phase_hunters(state);
    assert_eq!(
        r.state.enemies[&EnemyId(100)].current_location,
        Some(B),
        "Elite hunter ignores the barricade",
    );
}

#[test]
fn leaving_the_barricaded_location_discards_barricade() {
    install();
    let mut inv = test_investigator(1);
    inv.current_location = Some(A);
    let mut a = test_location(1, "A");
    a.connections = vec![B];
    let mut b = test_location(2, "B");
    b.connections = vec![A];
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(a)
        .with_location(b)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .build();
    let inst = game_core::state::CardInstanceId(900);
    state.locations.get_mut(&A).unwrap().attachments.push(
        game_core::state::CardInPlay::enter_play(CardCode::new(BARRICADE), inst),
    );

    let r = apply(
        state,
        Action::Player(PlayerAction::Move {
            investigator: INV,
            destination: B,
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert!(
        r.state.locations[&A].attachments.is_empty(),
        "Barricade discarded on leave",
    );
    assert!(
        r.state.investigators[&INV]
            .discard
            .contains(&CardCode::new(BARRICADE)),
        "to the owner's player discard",
    );
}
```

> **Setup adapters (resolve at implementation time):** mirror an existing
> enemy-phase test for driving hunters — if there's no
> `test_support::drive_enemy_phase_hunters` helper, drive the hunter step the
> way `crates/cards/tests/agenda_01107.rs` / the existing hunter-movement
> integration test does (e.g. an `apply` that advances into the Enemy phase,
> or the public `drive_hunter_moves` entry). Likewise confirm `PlayerAction::Move`'s
> exact field name (`destination`) and `test_enemy`/`Enemy` field names against
> a current enemy test. Keep the assertions as written.

- [ ] **Step 5: Run the integration tests**

Run: `cargo test -p cards --test barricade`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/cards/src/impls/barricade.rs crates/cards/src/impls/mod.rs crates/cards/tests/barricade.rs
git commit -m "cards: Barricade 01038 (attach + non-Elite movement block + leave-discard)

Closes #323.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Full gauntlet + PR + phase-doc

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. (`cargo fmt` first if `--check` complains.)

- [ ] **Step 2: Push + open the PR (before the phase-doc commit)**

```bash
git push -u origin engine/barricade
gh pr create --title "engine: Barricade 01038 (attach-to-location + non-Elite movement block + leave-discard)" --fill
```

PR body: describe the four pieces; cite RR p.12 (hunter shortest-path) for graph-level impassability; note "Closes #323" and the deferred "#371". Watch CI: `gh pr checks <PR#> --watch`.

- [ ] **Step 3: Update the phase doc once CI is green**

In `docs/phases/phase-7-the-gathering.md`: flip Barricade (#323) to `✅ PR #<n>` in the C6b row and the Axis-E note (leaving only Mind over Matter #322 open there). Add **one** Decisions entry:

> **Barricade 01038 — `AttachSelfToLocation` + `EnemyMovementBlocked` + `LeftLocation` (#323, PR #<n>).** A played event re-homes itself to its location's attachment zone by consuming `pending_played_event` (one card, no duplicate — *not* the `PutIntoThreatArea`-by-code pattern, which only spawns because encounter cards have no instance at Revelation). A constant `Restriction::EnemyMovementBlocked` makes the location **graph-level impassable** to non-Elite enemies in hunter pathfinding (`bfs_distance_with`/`shortest_first_steps_with` — so it shifts nearest-prey, not just the final step); non-Elite = the `Elite` metadata trait (Ghoul Priest 01116 passes). A new `LeftLocation` forced trigger scans the left location's attachments → `DiscardSelf`, routed to the owner's player discard (solo; multiplayer ownership is #371). **A future location-attachment / movement-blocker reuses these.** Non-hunter enemy movement (agenda 01107's Ghoul move) does not yet consult the block — deferred edge.

Remove #323 from the open-Axis-E list (`#322` remains).

- [ ] **Step 4: Commit + push the doc**

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — close #323 (Barricade)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push
```

- [ ] **Step 5: Confirm CI green; await user approval to merge** (`gh pr merge <PR#> --squash --delete-branch`). Do not merge without explicit approval.

---

## Self-Review

**Spec coverage:** Component 1 (`AttachSelfToLocation`, single-copy) → Tasks 1+3 ✓; Component 2 (`EnemyMovementBlocked` + graph-level hunter impassability + Elite-trait exemption) → Tasks 1+2+5 ✓; Component 3 (`LeftLocation` forced + `DiscardSelf` to owner discard) → Tasks 1+4+6 ✓; Component 4 (card) → Task 7 ✓; testing → Tasks 2 (pathfinding), 7 (attach/block/elite/leave) ✓; #371 referenced in Tasks 4/8 ✓. The spec's "non-hunter movement consults the same predicate when it lands" is honored by the reusable `_with` variants; the agenda-01107 interaction is explicitly noted deferred in the phase-doc entry (Task 8).

**Placeholder scan:** The Task-7 integration test flags two implementation-time adapters (the hunter-phase driver entry and `PlayerAction::Move`/`Enemy` field names) — these are mechanical "match the nearest existing test" notes, not behavioral gaps; the assertions are fully specified. The corpus-code verification step (01116/01160) is an explicit check with a documented fallback. No `TODO`/`TBD` in implementation code beyond the intended `TODO(#371)`.

**Type consistency:** `Effect::AttachSelfToLocation` (unit variant) / `attach_self_to_location()`; `Restriction::EnemyMovementBlocked`; `EventPattern::LeftLocation`; `ForcedTriggerPoint::LeftLocation { investigator, location }` / `TimingEvent::LeftLocation { investigator, location }`; `bfs_distance_with` / `shortest_first_steps_with(state, from, to, is_passable)`; `hunter_destinations(state, from, prey, enemy_is_elite)` — all consistent across tasks. `CardMetadata::card_type() -> CardType`, the player-type (`Asset | Event | Skill`) discriminator used once (Task 4).
