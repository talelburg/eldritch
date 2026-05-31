# Hunter Movement (#128) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Hunter keyword's Enemy-phase movement — each ready, unengaged hunter moves one location along a shortest path toward the nearest investigator and engages on arrival — with lead-investigator `AwaitingInput` choices, a shared prey-target resolver reused across move / engage-on-arrival / engage-on-spawn (clearing #127's deferred case), and uniform suspend at spawn (option A/(i)).

**Architecture:** A new `Prey` enum (`card-dsl`) and two runtime `Enemy` fields (`hunter: bool`, `prey: Prey`). A pure BFS layer (`bfs_distance`, `shortest_first_steps`) and a shared `resolve_prey` returning `One | Tie | None`. A new `GameState.hunter_move_pending: Option<HunterChoice>` cursor (two variants: `Move{PickLocation}` / `Engage{PickInvestigator}`) drives suspend/resume during Enemy step 3.2. `enemy_phase` runs hunters before the attack loop; `spawn_enemy` routes engagement through `resolve_prey` and suspends Mythos draws on a tie. Tests use a 2-investigator fixture.

**Tech Stack:** Rust (workspace crates `card-dsl`, `game-core`, `scenarios`). TDD with `cargo test`. CI gauntlet: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, `cargo build -p web --target wasm32-unknown-unknown`.

**Rules grounding (verbatim, from `data/rules-reference/ahc01_rules_reference_web.pdf`):**
- Hunter (p.12): "During the enemy phase (in framework step 3.2), each ready, unengaged enemy with the hunter keyword moves to a connecting location, along the shortest path towards the nearest investigator. Enemies at a location with one or more investigators do not move. … If there are multiple equidistant investigators who qualify as 'the nearest investigator,' the enemy moves towards the one of those who best meets its prey instructions. If none do, or if the enemy has no prey instructions, the lead investigator may choose an investigator for the enemy to move towards."
- Enemy Engagement (p.10): "Any time a ready unengaged enemy is at the same location as an investigator, it engages that investigator … If there are multiple investigators at the same location as a ready unengaged enemy, follow the enemy's prey instructions to determine which investigator is engaged."
- Prey (p.17): "If an enemy that is about to automatically engage an investigator at its location has multiple options of whom to engage, that enemy engages the investigator who best meets its 'prey' instructions (if multiple investigators are tied … the lead investigator may decide among them)."

**Spec:** `docs/superpowers/specs/2026-05-30-128-hunter-movement-design.md`

**Branch:** `engine/hunter-movement` (already created; spec already committed there).

---

## File Map

| File | Responsibility | Change |
|---|---|---|
| `crates/card-dsl/src/card_data.rs` | `Prey` enum (`Default`, `HighestStat(Stat)`) | Create enum |
| `crates/card-dsl/src/lib.rs` (or wherever `card_data` re-exports live) | Re-export `Prey` | Modify if needed |
| `crates/game-core/src/state/enemy.rs` | `Enemy.hunter: bool` + `Enemy.prey: Prey` fields | Modify struct |
| `crates/game-core/src/state/game_state.rs` | `hunter_move_pending` field + `HunterChoice` enum | Modify + add enum |
| `crates/game-core/src/test_support/builder.rs` | `test_enemy` literal gets new fields | Modify |
| `crates/game-core/src/engine/pathfinding.rs` | `bfs_distance`, `shortest_first_steps` (pure helpers) | Create |
| `crates/game-core/src/engine/mod.rs` | `mod pathfinding;` | Modify |
| `crates/game-core/src/engine/dispatch.rs` | `resolve_prey`, hunter driver, resume routing, `enemy_phase` restructure, `spawn_enemy` rewrite, Mythos-draw suspend | Modify |
| `crates/scenarios/src/test_fixtures/synth_cards.rs` | (no change — synth enemy stays default-prey) | — |
| `crates/scenarios/tests/hunter_movement.rs` | Integration: 2-investigator spawn-tie + replay | Create |

**Conventions to follow (from existing code):**
- Validate-first / mutate-second in every handler.
- `unreachable!()` for state-corruption invariants (active id missing from map, cursor `None` where it must be `Some`); `Rejected { reason }` for malformed-input / illegal-action paths.
- Cursor pattern mirrors `mythos_draw_pending` / `enemy_attack_pending`: `Option` field on `GameState`, advanced inside the continuation, `open_fast_window` is the `AwaitingInput` producer for windows. Hunter ties use a *direct* `AwaitingInput` return (not a window) — see Task 7.
- New `WindowKind`-free: hunter choices are NOT windows; they are a separate `Option<HunterChoice>` suspension routed in `resolve_input`.

---

## Task 1: `Prey` enum in `card-dsl`

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs` (add enum near `Spawn`, ~line 110)
- Test: same file, `#[cfg(test)]` module at the bottom

- [ ] **Step 1: Write the failing test**

Add to the test module at the bottom of `crates/card-dsl/src/card_data.rs`:

```rust
#[cfg(test)]
mod prey_tests {
    use super::*;

    #[test]
    fn prey_default_is_default() {
        assert_eq!(Prey::default(), Prey::Default);
    }

    #[test]
    fn prey_serde_roundtrip_highest_stat() {
        let original = Prey::HighestStat(Stat::Combat);
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Prey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p card-dsl prey_tests 2>&1 | head -20`
Expected: FAIL — `cannot find type Prey in this scope`.

- [ ] **Step 3: Add the enum**

Insert near the `Spawn` struct (~line 110, after the `Spawn` definition) in `crates/card-dsl/src/card_data.rs`. `Stat` is already in scope via the existing `use` of the dsl module — verify with `grep -n "use .*Stat\|Stat" crates/card-dsl/src/card_data.rs`; if not imported, add `use crate::dsl::Stat;` at the top.

```rust
/// An enemy's prey instruction (Rules Reference p.17): which
/// investigator it pursues / engages when it has a choice.
///
/// Phase-4 ships `Default` + `HighestStat`. `Default` covers "no prey
/// instruction" and "Prey – nearest" — among equidistant / co-located
/// investigators all are equal, so the lead investigator breaks the tie
/// (p.12 / p.17). `HighestStat(Stat::Combat)` is Ghoul Priest's
/// `Prey – Highest [combat]`. Other printed variants (`Lowest`,
/// `Bearer only`, `Most clues`, …) land with their first card consumer;
/// `#[non_exhaustive]` keeps that additive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Prey {
    /// No discriminating instruction — all candidates are equal; the
    /// lead investigator breaks ties.
    #[default]
    Default,
    /// Pursue / engage the investigator with the highest value of the
    /// given stat; ties fall to the lead investigator.
    HighestStat(Stat),
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p card-dsl prey_tests 2>&1 | tail -10`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/card-dsl/src/card_data.rs
git commit -m "card-dsl: add Prey enum (Default + HighestStat) for #128

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: `Enemy.hunter` + `Enemy.prey` fields

**Files:**
- Modify: `crates/game-core/src/state/enemy.rs` (struct + its doc; add `use`)
- Modify: `crates/game-core/src/test_support/fixtures.rs` (`test_enemy` literal, ~line 102)
- Modify: `crates/game-core/src/engine/dispatch.rs` (`spawn_enemy`'s `Enemy { … }` literal, ~line 519)
- Test: `crates/game-core/src/state/enemy.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

Add a test module to `crates/game-core/src/state/enemy.rs`:

```rust
#[cfg(test)]
mod hunter_prey_field_tests {
    use super::*;
    use crate::card_data::Prey;

    #[test]
    fn enemy_carries_hunter_and_prey() {
        let e = Enemy {
            id: EnemyId(1),
            name: "Ghoul Priest".into(),
            fight: 4,
            evade: 4,
            max_health: 5,
            damage: 0,
            attack_damage: 2,
            attack_horror: 2,
            current_location: None,
            exhausted: false,
            traits: vec!["Humanoid".into(), "Monster".into(), "Elite".into()],
            engaged_with: None,
            hunter: true,
            prey: Prey::Default,
        };
        assert!(e.hunter);
        assert_eq!(e.prey, Prey::Default);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core hunter_prey_field_tests 2>&1 | head -20`
Expected: FAIL — `struct Enemy has no field named hunter`.

- [ ] **Step 3: Add the fields**

In `crates/game-core/src/state/enemy.rs`, add to the top `use`:
```rust
use crate::card_data::Prey;
```
(Verify the crate path: `grep -n "card_data" crates/game-core/src/lib.rs` — `game_core::card_data` is the re-export. Inside the crate use `crate::card_data::Prey`.)

Add the two fields at the end of the `Enemy` struct (after `engaged_with`), and update the struct doc-comment's deferred-fields note (lines ~16-18 reference `hunter`/`prey` as deferred — change to present tense):

```rust
    /// Whether this enemy has the Hunter keyword (Rules Reference
    /// p.12): a ready, unengaged hunter moves toward the nearest
    /// investigator during Enemy-phase step 3.2.
    pub hunter: bool,
    /// Prey instruction (Rules Reference p.17): which investigator the
    /// enemy pursues / engages when it has a choice. `Prey::Default`
    /// for enemies with no printed prey line.
    pub prey: Prey,
```

Edit the struct doc lines that currently say `hunter`/`prey` are deferred (around lines 16-18: "- `aloof`, `prey`: …" / "- `hunter`, `prey`: hunter movement during the enemy phase (#71).") — replace with a single accurate line:
```rust
/// - `aloof`: spawn-time engagement rule (separate issue).
```
(Remove the `hunter`/`prey`-deferred bullets since those fields now exist.)

- [ ] **Step 4: Update the two `Enemy` literals**

In `crates/game-core/src/test_support/fixtures.rs`, `test_enemy` (~line 102) — add after `engaged_with: None,`:
```rust
        hunter: false,
        prey: crate::card_data::Prey::Default,
```

In `crates/game-core/src/engine/dispatch.rs`, `spawn_enemy`'s `Enemy { … }` literal (~line 519, after `engaged_with,`) — add:
```rust
        hunter: false,
        prey: crate::card_data::Prey::Default,
```
(Spawned encounter enemies default to non-hunter / default-prey until the metadata side lands — per spec "No CardMetadata / pipeline changes.")

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p game-core hunter_prey_field_tests 2>&1 | tail -10`
Expected: PASS.
Run: `cargo build -p game-core 2>&1 | tail -5`
Expected: clean (any other `Enemy { … }` literal in tests will surface here — search `grep -rn "Enemy {" crates/game-core/src` and add the two fields to each; literals built via `test_enemy(..)` need no change).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/state/enemy.rs crates/game-core/src/test_support/fixtures.rs crates/game-core/src/engine/dispatch.rs
git commit -m "game-core: add Enemy.hunter + Enemy.prey runtime fields for #128

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: BFS pathfinding helpers

**Files:**
- Create: `crates/game-core/src/engine/pathfinding.rs`
- Modify: `crates/game-core/src/engine/mod.rs` (add `mod pathfinding;` — `pub(crate)` so dispatch can call it)
- Test: in `pathfinding.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

Create `crates/game-core/src/engine/pathfinding.rs` with ONLY the test module first:

```rust
//! Pure BFS helpers over the location-connection graph, used by Hunter
//! movement (#128, Rules Reference p.12 "shortest path towards the
//! nearest investigator").

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{LocationId, Phase};
    use crate::test_support::builder::{test_location, TestGame};

    /// Build a diamond: A(1) connects to B(2) and C(3); both connect to
    /// D(4). Bidirectional edges.
    fn diamond() -> crate::state::GameState {
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        let mut c = test_location(3, "C");
        let mut d = test_location(4, "D");
        a.connections = vec![LocationId(2), LocationId(3)];
        b.connections = vec![LocationId(1), LocationId(4)];
        c.connections = vec![LocationId(1), LocationId(4)];
        d.connections = vec![LocationId(2), LocationId(3)];
        TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_location(c)
            .with_location(d)
            .build()
    }

    #[test]
    fn distance_same_location_is_zero() {
        let s = diamond();
        assert_eq!(bfs_distance(&s, LocationId(1), LocationId(1)), Some(0));
    }

    #[test]
    fn distance_adjacent_is_one() {
        let s = diamond();
        assert_eq!(bfs_distance(&s, LocationId(1), LocationId(2)), Some(1));
    }

    #[test]
    fn distance_across_diamond_is_two() {
        let s = diamond();
        assert_eq!(bfs_distance(&s, LocationId(1), LocationId(4)), Some(2));
    }

    #[test]
    fn distance_unreachable_is_none() {
        let mut a = test_location(1, "A");
        let island = test_location(9, "Island");
        a.connections = vec![];
        let s = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(island)
            .build();
        assert_eq!(bfs_distance(&s, LocationId(1), LocationId(9)), None);
    }

    #[test]
    fn first_steps_single_when_one_shortest_path() {
        // Linear A-B-D (remove C). Only step toward D from A is B.
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        let mut d = test_location(4, "D");
        a.connections = vec![LocationId(2)];
        b.connections = vec![LocationId(1), LocationId(4)];
        d.connections = vec![LocationId(2)];
        let s = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_location(d)
            .build();
        assert_eq!(shortest_first_steps(&s, LocationId(1), LocationId(4)), vec![LocationId(2)]);
    }

    #[test]
    fn first_steps_both_when_two_equal_paths() {
        // Diamond: from A to D, both B and C are on a shortest path.
        let s = diamond();
        let mut steps = shortest_first_steps(&s, LocationId(1), LocationId(4));
        steps.sort();
        assert_eq!(steps, vec![LocationId(2), LocationId(3)]);
    }

    #[test]
    fn first_steps_empty_when_unreachable() {
        let mut a = test_location(1, "A");
        let island = test_location(9, "Island");
        a.connections = vec![];
        let s = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(island)
            .build();
        assert!(shortest_first_steps(&s, LocationId(1), LocationId(9)).is_empty());
    }

    #[test]
    fn first_steps_empty_when_already_at_target() {
        let s = diamond();
        assert!(shortest_first_steps(&s, LocationId(1), LocationId(1)).is_empty());
    }
}
```

- [ ] **Step 2: Wire the module and run to confirm it fails**

Add to `crates/game-core/src/engine/mod.rs` (near the other `mod` declarations — check `grep -n "^mod \|^pub(crate) mod \|^pub mod " crates/game-core/src/engine/mod.rs`):
```rust
pub(crate) mod pathfinding;
```

Run: `cargo test -p game-core pathfinding 2>&1 | head -20`
Expected: FAIL — `cannot find function bfs_distance`.

- [ ] **Step 3: Implement the helpers**

Add to the top of `crates/game-core/src/engine/pathfinding.rs` (above the test module):

```rust
use std::collections::{BTreeMap, VecDeque};

use crate::state::{GameState, LocationId};

/// Breadth-first distance (edge count) from `from` to `to` over the
/// location-connection graph. `Some(0)` when `from == to`; `None` when
/// `to` is unreachable. Connections are treated as given in
/// `Location.connections` (the engine maintains them bidirectionally,
/// but BFS does not assume that).
pub(crate) fn bfs_distance(state: &GameState, from: LocationId, to: LocationId) -> Option<u32> {
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
            if next == to {
                return Some(dist + 1);
            }
            if !seen.contains_key(&next) {
                seen.insert(next, dist + 1);
                queue.push_back(next);
            }
        }
    }
    None
}

/// Every neighbor of `from` that lies on *a* shortest path to `to`,
/// i.e. each connected location `n` with
/// `bfs_distance(n, to) == bfs_distance(from, to) - 1`. Empty when `to`
/// is unreachable or `from == to` (no step needed). Result order
/// follows `from`'s `connections` order; callers that need determinism
/// across that should sort.
pub(crate) fn shortest_first_steps(
    state: &GameState,
    from: LocationId,
    to: LocationId,
) -> Vec<LocationId> {
    let Some(total) = bfs_distance(state, from, to) else {
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
        .filter(|&n| bfs_distance(state, n, to) == Some(total - 1))
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core pathfinding 2>&1 | tail -12`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/pathfinding.rs crates/game-core/src/engine/mod.rs
git commit -m "game-core: BFS pathfinding helpers for hunter movement (#128)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: `resolve_prey` shared resolver

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (add `PreyResolution` enum + `resolve_prey` fn; place near `spawn_enemy`, ~line 450)
- Test: `dispatch.rs` `#[cfg(test)]` block

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `dispatch.rs` (find it: `grep -n "mod tests" crates/game-core/src/engine/dispatch.rs`):

```rust
#[test]
fn resolve_prey_default_single_candidate_is_one() {
    let state = TestGame::new().build();
    let r = resolve_prey(&state, &crate::card_data::Prey::Default, &[InvestigatorId(1)]);
    assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
}

#[test]
fn resolve_prey_default_multiple_is_tie() {
    let state = TestGame::new().build();
    let r = resolve_prey(
        &state,
        &crate::card_data::Prey::Default,
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
}

#[test]
fn resolve_prey_empty_is_none() {
    let state = TestGame::new().build();
    let r = resolve_prey(&state, &crate::card_data::Prey::Default, &[]);
    assert!(matches!(r, PreyResolution::None));
}

#[test]
fn resolve_prey_highest_stat_picks_max() {
    let mut hi = test_investigator(1);
    hi.skills.combat = 5;
    let mut lo = test_investigator(2);
    lo.skills.combat = 2;
    let state = TestGame::new()
        .with_investigator(hi)
        .with_investigator(lo)
        .build();
    let r = resolve_prey(
        &state,
        &crate::card_data::Prey::HighestStat(crate::dsl::Stat::Combat),
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
}

#[test]
fn resolve_prey_highest_stat_tie_is_tie() {
    let mut a = test_investigator(1);
    a.skills.combat = 4;
    let mut b = test_investigator(2);
    b.skills.combat = 4;
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .build();
    let r = resolve_prey(
        &state,
        &crate::card_data::Prey::HighestStat(crate::dsl::Stat::Combat),
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p game-core resolve_prey 2>&1 | head -20`
Expected: FAIL — `cannot find type PreyResolution` / `cannot find function resolve_prey`.

- [ ] **Step 3: Implement**

Add near `spawn_enemy` in `dispatch.rs`. Map only the four base skills from `Stat`; `Stat::MaxHealth`/`MaxSanity` aren't valid prey stats in scope — reject the unsupported variants loudly (state-corruption: a card-impl bug, since no in-scope prey uses them):

```rust
/// Result of narrowing a candidate investigator set by a prey
/// instruction (Rules Reference p.12 / p.17).
#[derive(Debug, Clone, PartialEq, Eq)]
enum PreyResolution {
    /// Exactly one investigator best meets the instruction.
    One(InvestigatorId),
    /// Two or more tie — the lead investigator decides (carries the
    /// tied set, in input order).
    Tie(Vec<InvestigatorId>),
    /// No candidates at all.
    None,
}

/// Narrow `candidates` by `prey`. `Default` treats all candidates as
/// equal; `HighestStat` keeps those with the maximum value of the
/// stat. Returns `One` (single best), `Tie` (2+ best — lead decides),
/// or `None` (empty candidate set). Caller supplies the candidate set
/// (equidistant-nearest investigators for movement; co-located
/// investigators for engagement).
fn resolve_prey(
    state: &GameState,
    prey: &crate::card_data::Prey,
    candidates: &[InvestigatorId],
) -> PreyResolution {
    use crate::card_data::Prey;
    if candidates.is_empty() {
        return PreyResolution::None;
    }
    let best: Vec<InvestigatorId> = match prey {
        Prey::Default => candidates.to_vec(),
        Prey::HighestStat(stat) => {
            let skill = stat_to_skill_kind(*stat);
            let max = candidates
                .iter()
                .filter_map(|id| state.investigators.get(id).map(|inv| inv.skills.value(skill)))
                .max();
            match max {
                Some(m) => candidates
                    .iter()
                    .copied()
                    .filter(|id| {
                        state
                            .investigators
                            .get(id)
                            .is_some_and(|inv| inv.skills.value(skill) == m)
                    })
                    .collect(),
                None => Vec::new(),
            }
        }
    };
    match best.as_slice() {
        [] => PreyResolution::None,
        [one] => PreyResolution::One(*one),
        _ => PreyResolution::Tie(best),
    }
}

/// Map a prey `Stat` to the `SkillKind` used for investigator lookup.
/// Only the four base skills are valid prey stats in Phase-4 scope; a
/// `MaxHealth`/`MaxSanity` prey would be a card-impl bug.
fn stat_to_skill_kind(stat: crate::dsl::Stat) -> SkillKind {
    use crate::dsl::Stat;
    match stat {
        Stat::Willpower => SkillKind::Willpower,
        Stat::Intellect => SkillKind::Intellect,
        Stat::Combat => SkillKind::Combat,
        Stat::Agility => SkillKind::Agility,
        Stat::MaxHealth | Stat::MaxSanity => unreachable!(
            "resolve_prey: prey stat {stat:?} is not a base skill; no in-scope \
             prey instruction uses MaxHealth/MaxSanity — card-impl bug"
        ),
    }
}
```

Verify `SkillKind` is imported in `dispatch.rs` (`grep -n "SkillKind" crates/game-core/src/engine/dispatch.rs` — it's used by `sum_skill_value`, so it's already in scope).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core resolve_prey 2>&1 | tail -12`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "game-core: resolve_prey shared prey-target resolver (#128)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: `HunterChoice` + `hunter_move_pending` state

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add field after `enemy_attack_pending` ~line 146; add `HunterChoice` enum near `WindowKind`; ensure `GameState::new`/default constructor initializes the field)
- Test: `game_state.rs` `#[cfg(test)]` or a dispatch serde roundtrip

- [ ] **Step 1: Write the failing test**

Add to `crates/game-core/src/state/game_state.rs` test module (find/确认: `grep -n "mod tests\|fn .*roundtrip" crates/game-core/src/state/game_state.rs`; if none, create a `#[cfg(test)] mod hunter_pending_tests`):

```rust
#[cfg(test)]
mod hunter_pending_tests {
    use super::*;

    #[test]
    fn hunter_choice_move_serde_roundtrip() {
        let original = HunterChoice::Move {
            enemy: EnemyId(3),
            candidates: vec![LocationId(2), LocationId(3)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: HunterChoice = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn hunter_choice_engage_serde_roundtrip() {
        let original = HunterChoice::Engage {
            enemy: EnemyId(5),
            candidates: vec![InvestigatorId(1), InvestigatorId(2)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: HunterChoice = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p game-core hunter_pending_tests 2>&1 | head -20`
Expected: FAIL — `cannot find type HunterChoice`.

- [ ] **Step 3: Add the enum and field**

Add the enum near `WindowKind` in `game_state.rs` (after the `WindowKind` enum, ~line 517). Confirm `EnemyId`, `InvestigatorId`, `LocationId` are imported at the top of the file (they are — `WindowKind`/`OpenWindow` use them).

```rust
/// A suspended Hunter-movement choice awaiting the lead investigator's
/// input during Enemy-phase step 3.2 (#128). `Some` only while
/// suspended on a tie; cleared once resolved. The `EnemyId` inside is
/// the movement cursor — on resume the engine finishes this enemy then
/// scans `state.enemies` for the next eligible hunter with a strictly
/// greater id.
///
/// Two shapes because the two choice points need different input:
/// movement is a `PickLocation` over a prey-filtered destination set
/// (the chosen prey doesn't persist, so picking a location is
/// outcome-equivalent to picking an investigator-then-path); engagement
/// on arrival is a `PickInvestigator` over the co-located set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HunterChoice {
    /// Lead investigator picks the hunter's destination among tied
    /// prey-legal shortest-path next steps (Rules Reference p.12).
    Move {
        /// The hunter being moved.
        enemy: EnemyId,
        /// Legal destinations to choose among (the validated option set).
        candidates: Vec<LocationId>,
    },
    /// Lead investigator picks whom the hunter engages among co-located
    /// tied prey candidates (Rules Reference p.10 / p.17).
    Engage {
        /// The hunter that arrived.
        enemy: EnemyId,
        /// Co-located investigators to choose among.
        candidates: Vec<InvestigatorId>,
    },
}
```

Add the field to `GameState` after `enemy_attack_pending` (~line 146):
```rust
    /// Suspended Hunter-movement choice (#128), `Some` only while the
    /// Enemy phase is paused on a lead-investigator tie. See
    /// [`HunterChoice`].
    pub hunter_move_pending: Option<HunterChoice>,
```

- [ ] **Step 4: Initialize the field in the constructor**

Find where `GameState` is constructed with field defaults: `grep -n "enemy_attack_pending: None\|mythos_draw_pending: None" crates/game-core/src/state/game_state.rs crates/game-core/src/test_support/builder.rs`. Add `hunter_move_pending: None,` alongside each `enemy_attack_pending: None,`. (Likely two sites: `GameState`'s `Default`/`new` and the `TestGame` builder's initial state.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p game-core hunter_pending_tests 2>&1 | tail -10`
Expected: PASS (2 tests).
Run: `cargo build -p game-core 2>&1 | tail -5`
Expected: clean (a missing-field error here means another `GameState { … }` literal needs `hunter_move_pending: None,`).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/test_support/builder.rs
git commit -m "game-core: HunterChoice enum + hunter_move_pending cursor (#128)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Hunter movement core — single hunter, no tie (move + auto-engage)

This task builds the per-hunter processing and the driver loop for the **non-interactive** cases (one investigator ⇒ no ties). Ties (Task 7) and spawn (Task 8) build on it.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — replace the `hunter_movement_step` stub (~line 2950) with the real driver; add helpers `eligible_hunters`, `process_one_hunter`, `move_hunter_to`, `engage_on_arrival`.
- Test: `dispatch.rs` `#[cfg(test)]`

**Design of the driver (no ties yet):**
- `drive_hunter_moves(state, events) -> EngineOutcome`: loop — find the next eligible hunter (lowest `EnemyId` whose id is `>` the last processed, ready + unengaged + `hunter`), process it; when none remain, return `EngineOutcome::Done`. (Tie suspension added in Task 7.)
- A hunter is **eligible** iff `!exhausted && engaged_with.is_none() && hunter && current_location.is_some()`.
- `process_one_hunter`: if any investigator is at the hunter's location → no move (p.12 "Enemies at a location with one or more investigators do not move") → then still attempt engage-on-arrival at the current location (it's already there). Else compute `destinations` (Task 6 helper); `One(loc)` → move there; `None` → skip; (`Tie` handled in Task 7 — for now `unreachable!` with a note it lands in Task 7, OR compute but assert single — see Step 3). After moving (or staying), run engage-on-arrival.

- [ ] **Step 1: Write the failing tests**

Add to `dispatch.rs` tests. Use a small linear map and `with_enemy`/`with_investigator`. Note `enemy_phase` integration is Task 9 — here we call `drive_hunter_moves` directly.

```rust
#[test]
fn hunter_moves_one_step_toward_sole_investigator_and_engages_on_arrival() {
    // Map: A(1)-B(2)-C(3). Investigator at C; hunter at A. Hunter moves
    // A->B (one step). No investigator at B, so no engage yet.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    let mut c = test_location(3, "C");
    a.connections = vec![LocationId(2)];
    b.connections = vec![LocationId(1), LocationId(3)];
    c.connections = vec![LocationId(2)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(3));
    let mut ghoul = test_enemy(1, "Swarm");
    ghoul.hunter = true;
    ghoul.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b).with_location(c)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(ghoul)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut state, &mut events);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(2)));
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_event!(events, Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == LocationId(2));
}

#[test]
fn hunter_engages_when_it_moves_into_investigators_location() {
    // Map A(1)-B(2). Investigator at B; hunter at A. Hunter moves A->B
    // and engages on arrival.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    a.connections = vec![LocationId(2)];
    b.connections = vec![LocationId(1)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(2));
    let mut h = test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut state, &mut events);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(2)));
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(InvestigatorId(1)));
    assert_event!(events, Event::EnemyEngaged { enemy, investigator } if *enemy == EnemyId(1) && *investigator == InvestigatorId(1));
}

#[test]
fn hunter_with_no_path_does_not_move() {
    // Hunter at isolated island; investigator elsewhere. No path.
    let mut a = test_location(1, "A");
    let island = test_location(9, "Island");
    a.connections = vec![];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(1));
    let mut h = test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(LocationId(9));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(island)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut state, &mut events);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(9)));
    assert_no_event!(events, Event::EnemyMoved { .. });
}

#[test]
fn exhausted_hunter_is_skipped() {
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    a.connections = vec![LocationId(2)];
    b.connections = vec![LocationId(1)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(2));
    let mut h = test_enemy(1, "Hunter");
    h.hunter = true;
    h.exhausted = true;
    h.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut state, &mut events);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(1)));
    assert_no_event!(events, Event::EnemyMoved { .. });
}

#[test]
fn non_hunter_enemy_does_not_move() {
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    a.connections = vec![LocationId(2)];
    b.connections = vec![LocationId(1)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(2));
    let mut e = test_enemy(1, "Slug");
    e.hunter = false;
    e.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(e)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut state, &mut events);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(1)));
    assert_no_event!(events, Event::EnemyMoved { .. });
}
```

This task also needs a new `Event::EnemyMoved`. Check it doesn't exist: `grep -n "EnemyMoved" crates/game-core/src/event.rs`. If absent, add it (Step 3a).

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p game-core hunter_moves_one_step 2>&1 | head -20`
Expected: FAIL — `cannot find function drive_hunter_moves` (and `Event::EnemyMoved`).

- [ ] **Step 3a: Add `Event::EnemyMoved`**

In `crates/game-core/src/event.rs`, near the other `Enemy*` variants (~line 200):
```rust
    /// A hunter enemy moved one location during Enemy-phase step 3.2
    /// (Rules Reference p.12). Engagement on arrival, if any, emits a
    /// paired [`EnemyEngaged`](Self::EnemyEngaged) immediately after.
    EnemyMoved {
        /// The enemy that moved.
        enemy: EnemyId,
        /// Destination location.
        to: LocationId,
    },
```
(`EnemyId` / `LocationId` are already imported in `event.rs` — see its top `use`.)

- [ ] **Step 3b: Implement the driver and helpers**

Replace the `hunter_movement_step` stub body (~line 2950) and add helpers. Keep `hunter_movement_step` as a thin shim that `enemy_phase` already calls — but it must now return the outcome. For this task, `hunter_movement_step` is superseded by `drive_hunter_moves`; rename at the call site in Task 9. For now add `drive_hunter_moves` as a new fn and leave `hunter_movement_step` unused (Task 9 removes it). To avoid a dead-code warning under `-D warnings`, mark the old stub `#[allow(dead_code)]` temporarily OR (cleaner) just leave the call in `enemy_phase` pointing at the stub until Task 9. **Chosen:** leave the stub as-is and add `drive_hunter_moves` new; Task 9 swaps the call and deletes the stub in one commit. Mark `drive_hunter_moves` `#[allow(dead_code)]` only if CI fails this task in isolation (subagent-driven runs the full suite per task, so the test references keep it live — no allow needed).

```rust
/// Whether an enemy is an eligible hunter for step-3.2 movement:
/// ready, unengaged, has the keyword, and is on the map.
fn is_eligible_hunter(enemy: &Enemy) -> bool {
    enemy.hunter
        && !enemy.exhausted
        && enemy.engaged_with.is_none()
        && enemy.current_location.is_some()
}

/// Investigators (Active, on the map) at `loc`, in `turn_order` order
/// so prey ties carry a deterministic, lead-first candidate list.
fn active_investigators_at(state: &GameState, loc: LocationId) -> Vec<InvestigatorId> {
    state
        .turn_order
        .iter()
        .copied()
        .filter(|id| {
            state.investigators.get(id).is_some_and(|inv| {
                inv.status == Status::Active && inv.current_location == Some(loc)
            })
        })
        .collect()
}

/// Compute the prey-legal destination set for a hunter at `from`:
/// the union of shortest-path first-steps toward each
/// equidistant-nearest, prey-filtered investigator. Empty when no
/// investigator is reachable. Returns the (possibly multi-element)
/// destination set; deterministic order (sorted LocationId).
fn hunter_destinations(state: &GameState, from: LocationId, prey: &crate::card_data::Prey) -> Vec<LocationId> {
    use crate::engine::pathfinding::{bfs_distance, shortest_first_steps};
    // Active investigators on the map, with their distance from the hunter.
    let mut nearest: Vec<(InvestigatorId, u32)> = Vec::new();
    let mut min_dist: Option<u32> = None;
    for id in &state.turn_order {
        let Some(inv) = state.investigators.get(id) else { continue };
        if inv.status != Status::Active { continue; }
        let Some(loc) = inv.current_location else { continue };
        let Some(d) = bfs_distance(state, from, loc) else { continue };
        min_dist = Some(min_dist.map_or(d, |m| m.min(d)));
        nearest.push((*id, d));
    }
    let Some(min) = min_dist else { return Vec::new() };
    let nearest_ids: Vec<InvestigatorId> =
        nearest.iter().filter(|(_, d)| *d == min).map(|(id, _)| *id).collect();
    // Prey-filter the equidistant-nearest set. Default keeps all; the
    // lead breaks the tie among whatever survives.
    let chosen: Vec<InvestigatorId> = match resolve_prey(state, prey, &nearest_ids) {
        PreyResolution::One(id) => vec![id],
        PreyResolution::Tie(v) => v,
        PreyResolution::None => return Vec::new(),
    };
    // Union of shortest-path first-steps toward each chosen investigator.
    let mut dests: Vec<LocationId> = Vec::new();
    for id in chosen {
        let Some(loc) = state.investigators.get(&id).and_then(|i| i.current_location) else { continue };
        for step in shortest_first_steps(state, from, loc) {
            if !dests.contains(&step) {
                dests.push(step);
            }
        }
    }
    dests.sort();
    dests
}

/// Move `enemy` to `to`, emitting `EnemyMoved`. Mutate-only; caller
/// validated `to`.
fn move_hunter_to(state: &mut GameState, events: &mut Vec<Event>, enemy_id: EnemyId, to: LocationId) {
    let enemy = state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!("move_hunter_to: enemy {enemy_id:?} vanished mid-movement; state corruption")
    });
    enemy.current_location = Some(to);
    events.push(Event::EnemyMoved { enemy: enemy_id, to });
}

/// Engage-on-arrival for a hunter now at its (possibly unchanged)
/// location. Returns `Some(HunterChoice::Engage{..})` if the co-located
/// set ties under prey (caller suspends), else engages the resolved
/// investigator (or no-one) and returns `None`.
fn engage_on_arrival(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
) -> Option<HunterChoice> {
    let loc = state.enemies[&enemy_id].current_location.unwrap_or_else(|| {
        unreachable!("engage_on_arrival: enemy {enemy_id:?} has no location; state corruption")
    });
    let prey = state.enemies[&enemy_id].prey.clone();
    let candidates = active_investigators_at(state, loc);
    match resolve_prey(state, &prey, &candidates) {
        PreyResolution::None => None,
        PreyResolution::One(target) => {
            engage_enemy_with(state, events, enemy_id, target);
            None
        }
        PreyResolution::Tie(v) => Some(HunterChoice::Engage { enemy: enemy_id, candidates: v }),
    }
}

/// Set engagement + emit `EnemyEngaged`. Shared by movement and spawn.
fn engage_enemy_with(state: &mut GameState, events: &mut Vec<Event>, enemy_id: EnemyId, target: InvestigatorId) {
    let enemy = state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!("engage_enemy_with: enemy {enemy_id:?} vanished; state corruption")
    });
    enemy.engaged_with = Some(target);
    events.push(Event::EnemyEngaged { enemy: enemy_id, investigator: target });
}

/// Process a single hunter (movement + engage-on-arrival). Returns
/// `Some(HunterChoice)` if a tie suspends (Task 7 wires the Move-tie
/// branch; engage-tie returned here), else `None` (fully resolved).
fn process_one_hunter(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
) -> Option<HunterChoice> {
    let from = state.enemies[&enemy_id].current_location.unwrap_or_else(|| {
        unreachable!("process_one_hunter: enemy {enemy_id:?} has no location; state corruption")
    });
    // p.12: enemies already at a location with an investigator do not move.
    let here = active_investigators_at(state, from);
    if here.is_empty() {
        let prey = state.enemies[&enemy_id].prey.clone();
        let dests = hunter_destinations(state, from, &prey);
        match dests.as_slice() {
            [] => return None, // unreachable target — no move, nobody to engage here
            [one] => move_hunter_to(state, events, enemy_id, *one),
            _ => return Some(HunterChoice::Move { enemy: enemy_id, candidates: dests }),
        }
    }
    // Either we just moved, or we were already co-located: engage.
    engage_on_arrival(state, events, enemy_id)
}

/// Find the next eligible hunter with id strictly greater than `after`
/// (or the first eligible if `after` is `None`). Deterministic ascending
/// `EnemyId` order via the BTreeMap's sorted iteration.
fn next_eligible_hunter(state: &GameState, after: Option<EnemyId>) -> Option<EnemyId> {
    state
        .enemies
        .iter()
        .filter(|(id, e)| after.map_or(true, |a| **id > a) && is_eligible_hunter(e))
        .map(|(id, _)| *id)
        .next()
}

/// Drive Enemy-phase step 3.2: process eligible hunters in ascending
/// `EnemyId` order until none remain (returns `Done`) or one suspends
/// on a lead-investigator tie (returns `AwaitingInput`, Task 7).
fn drive_hunter_moves(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    let mut cursor: Option<EnemyId> = None;
    while let Some(id) = next_eligible_hunter(state, cursor) {
        if let Some(choice) = process_one_hunter(state, events, id) {
            // Tie — suspend (Task 7 stores + returns AwaitingInput).
            return suspend_hunter_choice(state, choice);
        }
        cursor = Some(id);
    }
    EngineOutcome::Done
}
```

For THIS task, `suspend_hunter_choice` is not yet needed by the passing tests (no ties), but `drive_hunter_moves` references it. Add a minimal version now (Task 7 fleshes the prompt text + tests):

```rust
/// Store the pending hunter choice and return `AwaitingInput` for the
/// lead investigator. (#128)
fn suspend_hunter_choice(state: &mut GameState, choice: HunterChoice) -> EngineOutcome {
    let prompt = match &choice {
        HunterChoice::Move { enemy, candidates } => format!(
            "Hunter {enemy:?} movement: lead investigator picks a destination among {candidates:?} \
             (submit InputResponse::PickLocation)"
        ),
        HunterChoice::Engage { enemy, candidates } => format!(
            "Hunter {enemy:?} engagement: lead investigator picks whom to engage among {candidates:?} \
             (submit InputResponse::PickInvestigator)"
        ),
    };
    state.hunter_move_pending = Some(choice);
    EngineOutcome::AwaitingInput {
        request: InputRequest { prompt },
        resume_token: ResumeToken(0),
    }
}
```

Verify imports in `dispatch.rs`: `InputRequest`, `ResumeToken` (used by `open_queued_reaction_window` already — in scope), `Enemy`, `Status`, `EnemyId`, `InvestigatorId`, `LocationId` (all in scope). `Event::EnemyMoved` / `EnemyEngaged` exist.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core hunter_ 2>&1 | tail -15`
Expected: PASS (the 5 movement tests). The tie tests come in Task 7.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs crates/game-core/src/event.rs
git commit -m "game-core: hunter movement core — move + engage-on-arrival, no ties (#128)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Lead-investigator tie resolution (suspend + resume)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — add `resume_hunter_choice`; route it from `resolve_input`; add the pending-input guard to `apply_player_action`.
- Test: `dispatch.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn hunter_move_tie_suspends_then_resumes_on_pick_location() {
    // Diamond A(1)-{B(2),C(3)}-D(4). Investigator at D; hunter at A,
    // default prey. Two equal first-steps (B, C) -> AwaitingInput.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    let mut c = test_location(3, "C");
    let mut d = test_location(4, "D");
    a.connections = vec![LocationId(2), LocationId(3)];
    b.connections = vec![LocationId(1), LocationId(4)];
    c.connections = vec![LocationId(1), LocationId(4)];
    d.connections = vec![LocationId(2), LocationId(3)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(4));
    let mut h = test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b).with_location(c).with_location(d)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut state, &mut events);
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(state.hunter_move_pending.is_some());
    // Resume by picking C.
    let mut ev2 = Vec::new();
    let resumed = resolve_input(&mut state, &mut ev2, &InputResponse::PickLocation(LocationId(3)));
    assert_eq!(resumed, EngineOutcome::Done);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(3)));
    assert!(state.hunter_move_pending.is_none());
    assert_event!(ev2, Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == LocationId(3));
}

#[test]
fn hunter_move_tie_rejects_invalid_pick() {
    // Same diamond setup; resume with a location not in candidates.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    let mut c = test_location(3, "C");
    let mut d = test_location(4, "D");
    a.connections = vec![LocationId(2), LocationId(3)];
    b.connections = vec![LocationId(1), LocationId(4)];
    c.connections = vec![LocationId(1), LocationId(4)];
    d.connections = vec![LocationId(2), LocationId(3)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(4));
    let mut h = test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b).with_location(c).with_location(d)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut state, &mut events);
    let mut ev2 = Vec::new();
    // LocationId(4) is the destination, not a first-step candidate.
    let r = resolve_input(&mut state, &mut ev2, &InputResponse::PickLocation(LocationId(4)));
    assert!(matches!(r, EngineOutcome::Rejected { .. }));
    assert!(state.hunter_move_pending.is_some(), "pending stays open on invalid pick");
}

#[test]
fn hunter_engage_tie_suspends_then_resumes_on_pick_investigator() {
    // Two investigators at B; hunter moves A->B; default prey -> tie ->
    // PickInvestigator.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    a.connections = vec![LocationId(2)];
    b.connections = vec![LocationId(1)];
    let mut i1 = test_investigator(1);
    i1.current_location = Some(LocationId(2));
    let mut i2 = test_investigator(2);
    i2.current_location = Some(LocationId(2));
    let mut h = test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b)
        .with_investigator(i1).with_investigator(i2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut state, &mut events);
    // Moved to B already, suspended on engagement tie.
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(2)));
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    let mut ev2 = Vec::new();
    let resumed = resolve_input(&mut state, &mut ev2, &InputResponse::PickInvestigator(InvestigatorId(2)));
    assert_eq!(resumed, EngineOutcome::Done);
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(InvestigatorId(2)));
    assert!(state.hunter_move_pending.is_none());
}

#[test]
fn highest_combat_prey_breaks_move_tie_without_prompt() {
    // Diamond; two equidistant investigators (at B and C ends) but one
    // has higher combat -> resolve_prey picks them, no prompt.
    // Layout: hunter at A(1); inv1 at B(2) combat 5; inv2 at C(3) combat 2.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    let mut c = test_location(3, "C");
    a.connections = vec![LocationId(2), LocationId(3)];
    b.connections = vec![LocationId(1)];
    c.connections = vec![LocationId(1)];
    let mut i1 = test_investigator(1);
    i1.current_location = Some(LocationId(2));
    i1.skills.combat = 5;
    let mut i2 = test_investigator(2);
    i2.current_location = Some(LocationId(3));
    i2.skills.combat = 2;
    let mut h = test_enemy(1, "Ghoul Priest");
    h.hunter = true;
    h.prey = crate::card_data::Prey::HighestStat(crate::dsl::Stat::Combat);
    h.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b).with_location(c)
        .with_investigator(i1).with_investigator(i2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut state, &mut events);
    assert_eq!(outcome, EngineOutcome::Done);
    // Moves toward inv1 (B) and engages immediately (arrives at B).
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(2)));
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(InvestigatorId(1)));
}

#[test]
fn multi_hunter_one_suspends_then_next_processed_on_resume() {
    // Hunter 1 ties (diamond toward D); hunter 2 has a clean single
    // step. After resolving hunter 1, hunter 2 is processed.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    let mut c = test_location(3, "C");
    let mut d = test_location(4, "D");
    a.connections = vec![LocationId(2), LocationId(3)];
    b.connections = vec![LocationId(1), LocationId(4)];
    c.connections = vec![LocationId(1), LocationId(4)];
    d.connections = vec![LocationId(2), LocationId(3)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(4));
    let mut h1 = test_enemy(1, "Tie Hunter");
    h1.hunter = true;
    h1.current_location = Some(LocationId(1)); // ties B/C toward D
    let mut h2 = test_enemy(2, "Clean Hunter");
    h2.hunter = true;
    h2.current_location = Some(LocationId(2)); // single step B->D
    let mut state = TestGame::new()
        .with_phase(Phase::Enemy)
        .with_location(a).with_location(b).with_location(c).with_location(d)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h1).with_enemy(h2)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut state, &mut events);
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    // Resolve hunter 1's tie -> hunter 2 then moves B->D and engages.
    let mut ev2 = Vec::new();
    let resumed = resolve_input(&mut state, &mut ev2, &InputResponse::PickLocation(LocationId(2)));
    assert_eq!(resumed, EngineOutcome::Done);
    assert_eq!(state.enemies[&EnemyId(2)].current_location, Some(LocationId(4)));
    assert_eq!(state.enemies[&EnemyId(2)].engaged_with, Some(InvestigatorId(1)));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p game-core hunter_move_tie 2>&1 | head -25`
Expected: FAIL — `resolve_input` doesn't route hunter choices (panics or rejects "no AwaitingInput outstanding").

- [ ] **Step 3: Implement `resume_hunter_choice` + routing + guard**

Add `resume_hunter_choice` in `dispatch.rs`:

```rust
/// Resume a suspended Hunter-movement choice with the lead
/// investigator's response, then continue driving remaining hunters.
/// Validates the response against the stored candidate set; on an
/// invalid pick, rejects and leaves `hunter_move_pending` untouched so
/// the client can retry. (#128)
fn resume_hunter_choice(
    state: &mut GameState,
    events: &mut Vec<Event>,
    response: &InputResponse,
) -> EngineOutcome {
    let pending = state.hunter_move_pending.clone().unwrap_or_else(|| {
        unreachable!("resume_hunter_choice: called with no pending hunter choice")
    });
    let current_enemy = match (&pending, response) {
        (HunterChoice::Move { enemy, candidates }, InputResponse::PickLocation(loc)) => {
            if !candidates.contains(loc) {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: hunter move destination {loc:?} not among candidates {candidates:?}"
                    )
                    .into(),
                };
            }
            state.hunter_move_pending = None;
            move_hunter_to(state, events, *enemy, *loc);
            // After the move, attempt engage-on-arrival; that itself may
            // suspend on an engagement tie.
            if let Some(choice) = engage_on_arrival(state, events, *enemy) {
                return suspend_hunter_choice(state, choice);
            }
            *enemy
        }
        (HunterChoice::Engage { enemy, candidates }, InputResponse::PickInvestigator(who)) => {
            if !candidates.contains(who) {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: hunter engage target {who:?} not among candidates {candidates:?}"
                    )
                    .into(),
                };
            }
            state.hunter_move_pending = None;
            engage_enemy_with(state, events, *enemy, *who);
            *enemy
        }
        (HunterChoice::Move { .. }, other) => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "ResolveInput: hunter movement expects InputResponse::PickLocation, got {other:?}"
                )
                .into(),
            };
        }
        (HunterChoice::Engage { .. }, other) => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "ResolveInput: hunter engagement expects InputResponse::PickInvestigator, got {other:?}"
                )
                .into(),
            };
        }
    };
    // Continue with the next eligible hunter after the one we finished.
    let mut cursor = Some(current_enemy);
    while let Some(id) = next_eligible_hunter(state, cursor) {
        if let Some(choice) = process_one_hunter(state, events, id) {
            return suspend_hunter_choice(state, choice);
        }
        cursor = Some(id);
    }
    EngineOutcome::Done
}
```

Route it in `resolve_input` — add as the FIRST branch (before the reaction-window check), since a pending hunter choice is its own suspension mode:

```rust
    if state.hunter_move_pending.is_some() {
        return resume_hunter_choice(state, events, response);
    }
```

Add the pending-input guard in `apply_player_action` (mirror the `in_flight_skill_test` guard at ~line 103) so non-`ResolveInput` actions reject while a hunter choice is outstanding. Place it next to the existing guards:

```rust
    if state.hunter_move_pending.is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a hunter-movement choice is pending; submit a PlayerAction::ResolveInput \
                     with InputResponse::PickLocation (movement) or \
                     InputResponse::PickInvestigator (engagement) before any other action"
                .into(),
        };
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core hunter 2>&1 | tail -20`
Expected: PASS (all hunter tests so far: Task 6's 5 + Task 7's 5).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "game-core: lead-investigator hunter tie resolution (suspend/resume) (#128)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: Spawn engagement via `resolve_prey` + Mythos-draw suspend (option A)

Replace `spawn_enemy`'s #127 multi-investigator reject with the shared resolver, suspending the Mythos encounter-draw loop on a tie.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — `spawn_enemy` (~line 496-549), the Mythos draw loop in `draw_encounter_card` (~line 5456-5510), and `resume_hunter_choice` (the spawn-tie path re-enters the draw loop).
- Test: `dispatch.rs` `#[cfg(test)]` for the deterministic spawn path; the interactive 2-investigator spawn-tie + Mythos resume is covered in the Task 10 integration test (needs a registry).

**Design:** Spawn-tie uses the SAME `hunter_move_pending` suspension? No — spawn is not a hunter move. Add a distinct, minimal pending marker for spawn so resume routing is unambiguous. Reuse `HunterChoice::Engage` is wrong semantically (it would re-enter the hunter loop). Instead add a third pending state for spawn. **Decision (keeps Task-5 enum focused):** add `GameState.spawn_engage_pending: Option<SpawnEngagePending>` where `SpawnEngagePending { enemy: EnemyId, investigator_to_draw: InvestigatorId, candidates: Vec<InvestigatorId> }`. `investigator_to_draw` is the drawing investigator whose Mythos draw chain must resume after engagement is chosen.

- [ ] **Step 1: Add the spawn-pending state (TDD: serde test first)**

In `game_state.rs`, add near `HunterChoice`:
```rust
/// A suspended engagement-on-spawn choice (#128, option A): a
/// multi-investigator spawn tie awaiting the lead investigator's
/// `PickInvestigator`. `investigator_to_draw` is the drawing
/// investigator whose Mythos encounter-draw chain resumes once the
/// engagement is chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SpawnEngagePending {
    /// The spawned enemy awaiting an engagement target.
    pub enemy: EnemyId,
    /// The investigator who drew the enemy (Mythos draw resumes for them).
    pub investigator_to_draw: InvestigatorId,
    /// Co-located investigators to choose among.
    pub candidates: Vec<InvestigatorId>,
}
```
Add field to `GameState` after `hunter_move_pending`:
```rust
    /// Suspended engagement-on-spawn choice (#128). See [`SpawnEngagePending`].
    pub spawn_engage_pending: Option<SpawnEngagePending>,
```
Initialize `spawn_engage_pending: None,` everywhere `hunter_move_pending: None,` was added (Task 5 sites).

Test:
```rust
#[test]
fn spawn_engage_pending_serde_roundtrip() {
    let original = SpawnEngagePending {
        enemy: EnemyId(2),
        investigator_to_draw: InvestigatorId(1),
        candidates: vec![InvestigatorId(1), InvestigatorId(2)],
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: SpawnEngagePending = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}
```
Run: `cargo test -p game-core spawn_engage_pending_serde 2>&1 | tail -8` → after adding, PASS.

- [ ] **Step 2: Write the failing spawn-resolver tests (deterministic cases)**

These don't need a registry — call `spawn_enemy` directly with a hand-built `CardMetadata`. Add to `dispatch.rs` tests:

```rust
#[test]
fn spawn_engages_sole_colocated_investigator() {
    // (regression: #127 single-investigator path still works)
    let mut loc = test_location(1, "Hall");
    loc.code = CardCode("_loc".into());
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    let meta = enemy_metadata_no_spawn("_e"); // helper below
    let mut events = Vec::new();
    let outcome = spawn_enemy(&mut state, &mut events, InvestigatorId(1), CardCode("_e".into()), &meta);
    assert_eq!(outcome, EngineOutcome::Done);
    let spawned = state.enemies.values().next().expect("one enemy");
    assert_eq!(spawned.engaged_with, Some(InvestigatorId(1)));
}

#[test]
fn spawn_tie_suspends_for_lead_pick() {
    let mut loc = test_location(1, "Hall");
    loc.code = CardCode("_loc".into());
    let mut i1 = test_investigator(1);
    i1.current_location = Some(LocationId(1));
    let mut i2 = test_investigator(2);
    i2.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_investigator(i1).with_investigator(i2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();
    state.mythos_draw_pending = Some(InvestigatorId(1));
    let meta = enemy_metadata_no_spawn("_e");
    let mut events = Vec::new();
    let outcome = spawn_enemy(&mut state, &mut events, InvestigatorId(1), CardCode("_e".into()), &meta);
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(state.spawn_engage_pending.is_some());
    // Enemy exists, not yet engaged.
    let spawned = state.enemies.values().next().expect("one enemy");
    assert_eq!(spawned.engaged_with, None);
}
```

Add a test helper near the test module (build a minimal enemy `CardMetadata`, `spawn: None`):
```rust
#[cfg(test)]
fn enemy_metadata_no_spawn(code: &str) -> crate::card_data::CardMetadata {
    use crate::card_data::{CardMetadata, CardType, Class, SkillIcons};
    CardMetadata {
        code: code.into(),
        name: "Synth".into(),
        class: Class::Mythos,
        card_type: CardType::Enemy,
        cost: None, xp: None, text: None, flavor: None, illustrator: None,
        traits: Vec::new(), slots: Vec::new(),
        skill_icons: SkillIcons { willpower: 0, intellect: 0, combat: 0, agility: 0, wild: 0 },
        health: Some(1), sanity: None, deck_limit: 1, quantity: 1,
        pack_code: "_synth".into(), position: 1,
        is_fast: false, spawn: None, surge: false, peril: false,
    }
}
```
(Cross-check the `CardMetadata` field list against `crates/card-dsl/src/card_data.rs` — Task author must mirror it exactly; it's not `#[non_exhaustive]`.)

Run: `cargo test -p game-core spawn_ 2>&1 | head -20` → FAIL (spawn still rejects multi-investigator; `spawn_engage_pending` path absent).

- [ ] **Step 3: Rewrite `spawn_enemy`'s engagement step**

Replace the `engaged_with` match (the `_ => Rejected{… requires Prey …}` arm, ~line 503-513) so it (a) mints + places the enemy first (unengaged), then (b) resolves engagement via `resolve_prey`, suspending on a tie. Restructure: mint the enemy with `engaged_with: None` and `hunter: false, prey: Prey::Default`, emit `EnemySpawned { engaged_with: None }`... but #127 currently emits `EnemySpawned` with the denormalized `engaged_with` and a paired `EnemyEngaged`. To preserve that for the deterministic case while supporting suspension:

- Compute `candidates = active_investigators_at(state, location_id)` (reuse Task 6 helper; note it uses `turn_order` + Active filter — for spawn we want investigators physically at the location, same predicate).
- Mint + insert the enemy with `engaged_with: None`.
- `match resolve_prey(state, &Prey::Default, &candidates)`:
  - `None` → emit `EnemySpawned { engaged_with: None }`; `Done`.
  - `One(target)` → emit `EnemySpawned { engaged_with: Some(target) }` then `EnemyEngaged`; set `enemy.engaged_with`; `Done`. (Matches #127's event shape.)
  - `Tie(v)` → emit `EnemySpawned { engaged_with: None }`; set `state.spawn_engage_pending = Some(SpawnEngagePending { enemy: enemy_id, investigator_to_draw: investigator, candidates: v })`; return `AwaitingInput` (prompt: "engagement-on-spawn tie; lead picks via PickInvestigator").

Note: spawned enemies use `Prey::Default` (the metadata side is out of scope), so any 2+ co-located investigators tie — exactly option A's interactive spawn.

The enemy literal already has `hunter: false, prey: crate::card_data::Prey::Default` from Task 2.

- [ ] **Step 4: Route spawn resume + re-enter the Mythos draw loop**

In `resolve_input`, add a branch BEFORE the hunter branch (or after — they're mutually exclusive; assert so):
```rust
    if state.spawn_engage_pending.is_some() {
        return resume_spawn_engage(state, events, response);
    }
```
Add `resume_spawn_engage`:
```rust
/// Resume a suspended engagement-on-spawn choice (#128, option A),
/// then continue the drawing investigator's Mythos encounter-draw chain
/// (surge loop + cursor advance) exactly as `draw_encounter_card` would
/// have.
fn resume_spawn_engage(
    state: &mut GameState,
    events: &mut Vec<Event>,
    response: &InputResponse,
) -> EngineOutcome {
    let pending = state.spawn_engage_pending.clone().unwrap_or_else(|| {
        unreachable!("resume_spawn_engage: called with no pending spawn engagement")
    });
    let InputResponse::PickInvestigator(who) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: spawn engagement expects InputResponse::PickInvestigator, got {response:?}"
            )
            .into(),
        };
    };
    if !pending.candidates.contains(who) {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: spawn engage target {who:?} not among candidates {:?}",
                pending.candidates
            )
            .into(),
        };
    }
    state.spawn_engage_pending = None;
    engage_enemy_with(state, events, pending.enemy, *who);
    // Continue the drawing investigator's draw chain: the spawned card
    // may have surge; then advance the Mythos cursor. This mirrors the
    // tail of draw_encounter_card after encounter_card_revealed returns.
    continue_mythos_draw_chain(state, events, pending.investigator_to_draw)
}
```

Extract the surge-loop tail of `draw_encounter_card` into `continue_mythos_draw_chain` so both the initial draw and the post-suspend resume share it. Refactor `draw_encounter_card`'s loop body: the loop draws+resolves one card via `encounter_card_revealed`; on `AwaitingInput` (spawn tie) it must RETURN that outcome (the chain is parked). So:

```rust
/// Run the surge-draw loop for `investigator` starting fresh, then
/// advance the Mythos cursor. Returns `AwaitingInput` if a spawn tie
/// suspends mid-chain (state carries `spawn_engage_pending`); the
/// resume path calls `continue_mythos_draw_chain` to finish.
fn continue_mythos_draw_chain(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    // Check surge of the just-resolved card (the one whose engagement we
    // just finished, or the previous loop iteration), then keep drawing.
    loop {
        let surged = last_drawn_was_surge(state, events, investigator);
        if !surged {
            break;
        }
        let outcome = encounter_card_revealed(state, events, investigator);
        match outcome {
            EngineOutcome::Rejected { .. } => return outcome,
            EngineOutcome::AwaitingInput { .. } => return outcome, // spawn tie mid-surge
            EngineOutcome::Done => {}
        }
    }
    advance_mythos_draw_pending(state, events);
    EngineOutcome::Done
}
```

And rewrite the main loop in `draw_encounter_card` to delegate: draw the first card, then if it didn't suspend, call `continue_mythos_draw_chain`:
```rust
    // First draw of this investigator's encounter card.
    let first = encounter_card_revealed(state, events, investigator);
    match first {
        EngineOutcome::Rejected { .. } => return first,
        EngineOutcome::AwaitingInput { .. } => return first, // spawn tie on first card
        EngineOutcome::Done => {}
    }
    continue_mythos_draw_chain(state, events, investigator)
```
Keep `MAX_SURGE_CHAIN` protection: move the chain counter into `continue_mythos_draw_chain` (a local `chain` incremented each surge iteration; `unreachable!` past `MAX_SURGE_CHAIN`). Confirm `last_drawn_was_surge` signature: `grep -n "fn last_drawn_was_surge" -A6 crates/game-core/src/engine/dispatch.rs` and adapt.

> **Implementer note (verify before coding):** read the *current* `draw_encounter_card` body (`sed -n '5438,5512p' crates/game-core/src/engine/dispatch.rs`) and `last_drawn_was_surge`, then refactor preserving the existing surge semantics + the `MAX_SURGE_CHAIN` cap exactly. The above is the target shape; match real signatures.

- [ ] **Step 5: Update the now-obsolete `encounter_spawn.rs` reject test**

The existing integration test `revealing_synth_enemy_with_two_investigators_at_loc_rejects_pointing_at_128` (in `crates/scenarios/tests/encounter_spawn.rs`, ~line 113) asserts the OLD behavior — a multi-investigator spawn *rejects* with a `#128`-pointing reason. After this task it *suspends* instead. **Convert that test** to assert the new behavior: the apply returns `AwaitingInput`, `state.spawn_engage_pending` is `Some`, and the enemy is placed but `engaged_with: None`. Rename it to `revealing_synth_enemy_with_two_investigators_at_loc_suspends_for_lead_pick`. Replace the `match result.outcome { Rejected … }` block with:
```rust
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "multi-investigator spawn now suspends for the lead's PickInvestigator, got {:?}",
        result.outcome,
    );
    assert!(result.state.spawn_engage_pending.is_some());
    let enemy = result.state.enemies.values().next().expect("enemy placed");
    assert_eq!(enemy.engaged_with, None, "engagement deferred until the lead picks");
```
(Full interactive resume through this path is the new dedicated test in Task 10; this conversion just stops the old assertion from failing.)

- [ ] **Step 6: Run tests**

Run: `cargo test -p game-core spawn_ 2>&1 | tail -15`
Expected: PASS (`spawn_engages_sole_colocated_investigator`, `spawn_tie_suspends_for_lead_pick`, serde).
Run: `cargo test -p game-core 2>&1 | tail -15` — full crate, ensure no Mythos-draw regressions (the existing `*mythos*` / surge tests must still pass).
Run: `cargo test -p scenarios --test encounter_spawn 2>&1 | tail -10` — the converted test passes.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs crates/game-core/src/state/game_state.rs crates/game-core/src/test_support/builder.rs crates/scenarios/tests/encounter_spawn.rs
git commit -m "game-core: spawn engagement via resolve_prey + Mythos-draw suspend (#127/#128)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 9: Wire `enemy_phase` to the hunter driver (cascade integration)

Make Enemy phase 3.2 run the real driver and propagate `AwaitingInput` out through the `EndTurn` cascade.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — `enemy_phase` (~line 2970), delete the `hunter_movement_step` stub, thread the outcome through `step_phase` / `investigation_phase_end` / `end_turn`.
- Test: `dispatch.rs` `#[cfg(test)]`

**Problem:** today `step_phase`, `enemy_phase`, `investigation_phase_end`, `enemy_phase_end`, `end_turn` return `()` for the cascade (they push events and call each other). A hunter tie mid-`enemy_phase` must surface as `AwaitingInput` from the originating `apply` call (an `EndTurn`, or the mulligan-completion kickoff that ends the round into Enemy — though round 1 has no Enemy until a turn ends).

**Approach (minimal, matches existing flow):** `enemy_phase` already runs `hunter_movement_step` then seeds the attack loop. Change `enemy_phase` to:
1. emit `PhaseStarted(Enemy)`,
2. `let outcome = drive_hunter_moves(state, events);`
3. if `AwaitingInput` → return it (park; the attack-loop kickoff happens on resume),
4. else → `enemy_attack_kickoff(state, events)` (extracted from the current tail) and return `Done`.

`step_phase` must return `EngineOutcome` so the Enemy arm can propagate. This is a signature change rippling to all `step_phase` callers (`mythos_phase_end`, `investigation_phase_end`, `upkeep_phase_end`, `enemy_phase_end`, `start_scenario`). Since only the Enemy transition can currently produce `AwaitingInput` from `step_phase`, the other arms return `Done`. Then `investigation_phase_end` returns that outcome; `end_turn`'s terminal branch returns it; `end_turn` already returns `EngineOutcome`.

When the hunter tie resolves (Task 7 `resume_hunter_choice` finishes all hunters), it must run `enemy_attack_kickoff` before returning `Done`. Update `resume_hunter_choice`: after the `while` loop completes with no suspension, call `enemy_attack_kickoff(state, events)` — but ONLY when the pending choice was a hunter move/engage during the Enemy phase (it always is). Add that call.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn enemy_phase_runs_hunters_then_attack_loop_when_no_tie() {
    // One hunter, one investigator, single-step path; no tie. Hunter
    // moves+engages, then attack loop kicks off (BeforeInvestigatorAttacked).
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    a.connections = vec![LocationId(2)];
    b.connections = vec![LocationId(1)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(2));
    let mut h = test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Investigation) // step_phase will move to Enemy
        .with_location(a).with_location(b)
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    // Drive the Investigation->Enemy transition via end_turn.
    let mut events = Vec::new();
    let outcome = end_turn(&mut state, &mut events);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.phase, Phase::Enemy);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(LocationId(2)));
    assert_event!(events, Event::EnemyEngaged { enemy, .. } if *enemy == EnemyId(1));
    assert_event!(events, Event::WindowOpened { kind } if *kind == WindowKind::BeforeInvestigatorAttacked);
}

#[test]
fn enemy_phase_suspends_on_hunter_tie_then_resumes_into_attack_loop() {
    // Diamond; tie. end_turn returns AwaitingInput; resume completes
    // movement AND kicks off the attack loop.
    let mut a = test_location(1, "A");
    let mut b = test_location(2, "B");
    let mut c = test_location(3, "C");
    let mut d = test_location(4, "D");
    a.connections = vec![LocationId(2), LocationId(3)];
    b.connections = vec![LocationId(1), LocationId(4)];
    c.connections = vec![LocationId(1), LocationId(4)];
    d.connections = vec![LocationId(2), LocationId(3)];
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(4));
    let mut h = test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(LocationId(1));
    let mut state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_location(a).with_location(b).with_location(c).with_location(d)
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    let outcome = end_turn(&mut state, &mut events);
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(state.phase, Phase::Enemy);
    let mut ev2 = Vec::new();
    let resumed = resolve_input(&mut state, &mut ev2, &InputResponse::PickLocation(LocationId(2)));
    assert_eq!(resumed, EngineOutcome::Done);
    assert_event!(ev2, Event::WindowOpened { kind } if *kind == WindowKind::BeforeInvestigatorAttacked);
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p game-core enemy_phase_runs_hunters enemy_phase_suspends 2>&1 | head -20`
Expected: FAIL (compile error: `step_phase`/`end_turn` cascade doesn't propagate; attack loop runs before hunters / unconditionally).

- [ ] **Step 3: Refactor signatures + `enemy_phase`**

1. Extract the attack-loop kickoff from the current `enemy_phase` tail (the `state.enemy_attack_pending = first_active_investigator(...)` + window-open block, ~line 2985-2994) into:
```rust
/// Seed the per-investigator attack cursor and open the first attack
/// window (or the final window if no Active investigator). Called after
/// hunter movement (step 3.2) completes.
fn enemy_attack_kickoff(state: &mut GameState, events: &mut Vec<Event>) {
    state.enemy_attack_pending = first_active_investigator(state);
    if state.enemy_attack_pending.is_some() {
        open_fast_window(state, events, WindowKind::BeforeInvestigatorAttacked);
    } else {
        open_fast_window(state, events, WindowKind::AfterAllInvestigatorsAttacked);
    }
}
```

2. Rewrite `enemy_phase` to return `EngineOutcome` and delete `hunter_movement_step`:
```rust
fn enemy_phase(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    events.push(Event::PhaseStarted { phase: Phase::Enemy });
    // 3.2 Hunter enemies move (#128). May suspend on a lead-investigator tie.
    match drive_hunter_moves(state, events) {
        EngineOutcome::AwaitingInput { request, resume_token } => {
            return EngineOutcome::AwaitingInput { request, resume_token };
        }
        EngineOutcome::Rejected { reason } => {
            unreachable!("enemy_phase: hunter movement rejected unexpectedly: {reason}")
        }
        EngineOutcome::Done => {}
    }
    // 3.3 Begin the per-investigator attack loop.
    enemy_attack_kickoff(state, events);
    EngineOutcome::Done
}
```

3. Change `step_phase` to return `EngineOutcome`. Each non-Enemy arm returns `Done` (their drivers return `()` today — wrap: call them then return `Done`); the Enemy arm returns `enemy_phase(...)`'s outcome:
```rust
fn step_phase(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    let from = state.phase;
    let to = from.next();
    state.phase = to;
    match to {
        Phase::Mythos if from != Phase::Mythos => { mythos_phase(state, events); EngineOutcome::Done }
        Phase::Investigation if from != Phase::Investigation => { investigation_phase(state, events); EngineOutcome::Done }
        Phase::Enemy if from != Phase::Enemy => enemy_phase(state, events),
        Phase::Upkeep if from != Phase::Upkeep => { upkeep_phase(state, events); EngineOutcome::Done }
        _ => unreachable!(
            "step_phase: from == to (from={from:?}, to={to:?}); Phase::next never \
             returns the same phase, so this branch is structurally unreachable."
        ),
    }
}
```

4. `investigation_phase_end` returns the `step_phase` outcome:
```rust
fn investigation_phase_end(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    events.push(Event::PhaseEnded { phase: Phase::Investigation });
    step_phase(state, events) // Investigation → Enemy; may AwaitingInput on a hunter tie
}
```

5. `end_turn`'s terminal branch returns that outcome instead of `Done`:
```rust
    if let Some(next_id) = next_active_investigator_after(state, active_id) {
        begin_investigator_turn(state, events, next_id);
        EngineOutcome::Done
    } else {
        state.active_investigator = None;
        investigation_phase_end(state, events) // 2.3 → Enemy (may AwaitingInput)
    }
```
(Delete the trailing `EngineOutcome::Done` that previously closed `end_turn`.)

6. Other `step_phase` callers (`mythos_phase_end`, `enemy_phase_end`, `upkeep_phase_end`, `start_scenario`) now get an `EngineOutcome` back. They run during cascades that themselves return `()` or `Done`. For each, since none of THEIR transitions produce `AwaitingInput` (only Investigation→Enemy does, owned by `investigation_phase_end`), discard with a debug assertion:
```rust
    let outcome = step_phase(state, events);
    debug_assert_eq!(outcome, EngineOutcome::Done, "unexpected suspension in non-Enemy transition");
```
Apply at each of those call sites. (`enemy_phase_end` steps Enemy→Upkeep, which is `Phase::Upkeep` arm → `Done`, safe.)

7. In `resume_hunter_choice` (Task 7), after the `while` loop completes without suspension, call the kickoff:
```rust
    // All hunters processed — begin the attack loop (step 3.3).
    enemy_attack_kickoff(state, events);
    EngineOutcome::Done
```
(Replace the bare `EngineOutcome::Done` at the end of `resume_hunter_choice`.)

> **Implementer note:** `grep -n "step_phase(state, events)" crates/game-core/src/engine/dispatch.rs` to find every caller; update each. The `enemy_phase` call inside `step_phase` is the only one that may now return non-`Done`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p game-core enemy_phase 2>&1 | tail -15`
Expected: PASS (both new tests).
Run: `cargo test -p game-core 2>&1 | tail -20`
Expected: full crate green — especially existing Enemy-phase tests (`end_turn_for_last_investigator_ends_phase_and_steps_to_enemy`, the attack-loop tests, `end_turn_cascades_through_upkeep_to_mythos_draw_pending`). Fix any that asserted on the old `enemy_phase` `()` shape.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "game-core: wire enemy_phase to hunter driver, propagate AwaitingInput through cascade (#128)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 10: Integration test — 2-investigator spawn tie + replay equality

**Files:**
- Create: `crates/scenarios/tests/hunter_movement.rs`

This file is its own cargo binary, so it installs `TEST_REGISTRY` (synthetic cards) without colliding. It exercises (a) the #127-clearing multi-investigator spawn tie through the real Mythos draw + registry path, and (b) replay equality across a `PickLocation` round-trip.

- [ ] **Step 1: Write the test file**

`synthetic::setup()` yields one investigator (`InvestigatorId(1)`) at `LocationId(10)` (code `SYNTH_LOC_CODE`) — confirmed from `encounter_spawn.rs`. The spawn-tie test adapts that file's two-investigator setup but drives through the real Mythos draw via `PlayerAction::DrawEncounterCard` (phase = Mythos, `mythos_draw_pending` seeded). The replay test builds its own diamond via `TestGame` (no registry needed for movement) and checks action-log determinism.

```rust
//! #128 integration: spawn-engagement tie resolved through the real
//! registry + Mythos draw path (option A), plus hunter-movement replay
//! equality across a PickLocation round-trip.

use std::sync::Once;

use game_core::action::{EngineRecord, InputResponse, PlayerAction};
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{EnemyId, InvestigatorId, LocationId, Phase};
use game_core::test_support::{test_enemy, test_investigator, test_location, TestGame};
use game_core::Action;
use scenarios::test_fixtures::synth_cards::{SYNTH_ENEMY_CODE, TEST_REGISTRY};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();
fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

#[test]
fn multi_investigator_spawn_engagement_resolves_via_lead_pick() {
    install_test_registry();
    let mut state = synthetic::setup();
    // Second investigator co-located at the synth spawn location (10).
    let mut inv2 = test_investigator(2);
    inv2.current_location = Some(LocationId(10));
    state.investigators.insert(InvestigatorId(2), inv2);
    state.turn_order.push(InvestigatorId(2));
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    // Drive through the real Mythos draw path.
    state.phase = Phase::Mythos;
    state.mythos_draw_pending = Some(InvestigatorId(1));
    state.encounter_deck.clear();
    state
        .encounter_deck
        .push_back(game_core::state::CardCode(SYNTH_ENEMY_CODE.into()));

    // 1) Drawing the enemy suspends for the lead's PickInvestigator.
    let r1 = apply(state, Action::Player(PlayerAction::DrawEncounterCard));
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "multi-investigator spawn suspends, got {:?}",
        r1.outcome,
    );
    assert!(r1.state.spawn_engage_pending.is_some());
    let spawned = r1.state.enemies.values().next().expect("enemy placed");
    assert_eq!(spawned.engaged_with, None, "engagement deferred until pick");

    // 2) Lead picks investigator 2; engagement resolves, draw chain ends.
    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickInvestigator(InvestigatorId(2)),
        }),
    );
    assert_eq!(r2.outcome, EngineOutcome::Done);
    assert!(r2.state.spawn_engage_pending.is_none());
    let enemy = r2.state.enemies.values().next().expect("enemy in play");
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(2)));
}

#[test]
fn hunter_movement_pick_location_replays_identically() {
    // Diamond A(1)-{B(2),C(3)}-D(4); investigator at D, hunter at A.
    // No registry needed — movement reads only runtime Enemy fields.
    fn diamond_state() -> game_core::state::GameState {
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        let mut c = test_location(3, "C");
        let mut d = test_location(4, "D");
        a.connections = vec![LocationId(2), LocationId(3)];
        b.connections = vec![LocationId(1), LocationId(4)];
        c.connections = vec![LocationId(1), LocationId(4)];
        d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut h = test_enemy(1, "Hunter");
        h.hunter = true;
        h.current_location = Some(LocationId(1));
        TestGame::new()
            .with_phase(Phase::Investigation)
            .with_location(a).with_location(b).with_location(c).with_location(d)
            .with_investigator(inv)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(h)
            .build()
    }

    let actions = [
        Action::Player(PlayerAction::EndTurn),
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickLocation(LocationId(3)),
        }),
    ];

    // Run once.
    let mut s1 = diamond_state();
    for a in &actions {
        s1 = apply(s1, a.clone()).state;
    }
    // Replay onto a fresh identical initial state.
    let mut s2 = diamond_state();
    for a in &actions {
        s2 = apply(s2, a.clone()).state;
    }
    assert_eq!(
        serde_json::to_string(&s1).unwrap(),
        serde_json::to_string(&s2).unwrap(),
        "replaying the same action log must reproduce identical state",
    );
    // Sanity: the hunter ended at the chosen location.
    assert_eq!(s1.enemies[&EnemyId(1)].current_location, Some(LocationId(3)));
}
```

> No `todo!()` — both tests are complete. If `synthetic::setup()`'s investigator/location ids differ from `(InvestigatorId(1), LocationId(10))`, adjust to match (cross-check the head of `encounter_spawn.rs`, which uses the same fixture). The replay test is registry-free, so it needs no `install_test_registry()`.

- [ ] **Step 2: Run to confirm it builds + behaves**

Run: `cargo test -p scenarios --test hunter_movement 2>&1 | tail -15`
Expected: both PASS. If the first fails on the draw not suspending, re-check Task 8's `continue_mythos_draw_chain` wiring.

- [ ] **Step 5: Commit**

```bash
git add crates/scenarios/tests/hunter_movement.rs
git commit -m "scenarios: hunter movement + spawn-tie integration tests (#128)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 11: Full CI gauntlet + doc polish

**Files:**
- Modify: any file flagged by clippy/fmt/doc.

- [ ] **Step 1: fmt**

Run: `cargo fmt --all`
Then: `cargo fmt --check 2>&1 | tail -5`
Expected: no diff.

- [ ] **Step 2: clippy (warnings-as-errors)**

Run: `cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -25`
Expected: clean. Common fixes: `needless_return`, `manual_map`, `if let ... else` style on the outcome matches; `map_or(true, …)` → clippy may suggest `is_none_or` — apply whatever it asks.

- [ ] **Step 3: tests with strict flags**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | tail -25`
Expected: all green.

- [ ] **Step 4: docs (broken-intra-doc-link gate)**

Run: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features 2>&1 | tail -20`
Expected: clean. Fix any `[`Foo`]` links that don't resolve (e.g. `[`HunterChoice`]`, `[`Prey`]`, `[`resolve_prey`]`, `[`Event::EnemyMoved`]`).

- [ ] **Step 5: wasm build**

Run: `cargo build -p web --target wasm32-unknown-unknown 2>&1 | tail -10`
Expected: clean.

- [ ] **Step 6: Commit any fixes**

```bash
git add -A
git commit -m "game-core: CI gauntlet fixes for #128 (fmt/clippy/doc)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 12: Phase-doc update (final commit, only when PR is ready)

Per CLAUDE.md, the phase-doc edit is the **last** commit before merge — done when the PR # is known and review fixes are folded in. **Do not do this task until the PR is open and approved-to-merge.**

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

- [ ] **Step 1: Move #128 to Closed + flip Ordering row**

In `docs/phases/phase-4-scenario-plumbing.md`:
- Move the `#128` row from the open Issues table to the **Closed** table with its PR number.
- Flip the Ordering (Shape B) row 10 (`#128 Hunter movement`) to `✅ PR #NN`.
- Update the Status line + open-count (`open: 4` → `open: 3`).
- Remove the settled **Open question** "Hunter target-selection details (#128)".
- Note in `#144`'s row that its #128 blocker is cleared (it can now use the prey resolver / `resolve_prey`).
- Update `#127`'s Closed note or add a Decision: the multi-investigator engagement-on-spawn reject is now resolved via `resolve_prey` + `spawn_engage_pending` (option A — uniform suspend through the Mythos draw loop).

- [ ] **Step 2: Add Decisions-made entries (only the load-bearing ones)**

Add entries a future PR-author would otherwise re-derive:
- **Prey is a runtime `Enemy` field, not metadata (`#128`).** `hunter: bool` + `prey: Prey` live on the runtime `Enemy`; `CardMetadata`/pipeline unchanged. First real spawning hunter extends metadata + threads the fields through `spawn_enemy`.
- **Movement is one `PickLocation`, engagement one `PickInvestigator` (`#128`).** Chosen prey doesn't persist, so movement collapses to a destination pick over the prey-filtered shortest-step union; engage-on-arrival stays an investigator pick. `resolve_prey` is shared across move / engage / spawn.
- **Spawn ties suspend the Mythos draw loop (option A, `#128`).** `spawn_engage_pending` + `continue_mythos_draw_chain` thread `AwaitingInput` through the surge loop; spawn and movement ties behave identically.
- **`step_phase` returns `EngineOutcome` (`#128`).** Only the Investigation→Enemy transition can suspend (hunter tie); other transitions `debug_assert!` `Done`.

- [ ] **Step 3: Commit**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "docs: phase-4 — close #128 hunter movement (PR #NN)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review Notes (author)

- **Spec coverage:** Prey enum (T1) ✓; hunter/prey fields (T2) ✓; BFS (T3) ✓; resolve_prey (T4) ✓; HunterChoice/cursor (T5) ✓; movement+engage no-tie (T6) ✓; tie suspend/resume PickLocation+PickInvestigator (T7) ✓; spawn option A + Mythos suspend (T8) ✓; enemy_phase cascade integration (T9) ✓; 2-investigator fixture + replay (T10) ✓; out-of-scope items (Lowest/Bearer, metadata, Aloof, multi-step, blocked-move) left unbuilt per spec ✓.
- **One deviation from the spec to flag at review:** spec described a single `Option<HunterChoice>` cursor; the plan adds a *separate* `Option<SpawnEngagePending>` for the spawn path (T8) because spawn resume must re-enter the Mythos draw chain, not the hunter loop — conflating them into `HunterChoice` would mis-route resume. Same lead-decides UX, cleaner routing. Worth a sentence in the PR description.
- **Type consistency:** `resolve_prey` / `PreyResolution::{One,Tie,None}` used identically in T4/T6/T8; `drive_hunter_moves` / `process_one_hunter` / `engage_on_arrival` / `engage_enemy_with` / `move_hunter_to` / `next_eligible_hunter` / `suspend_hunter_choice` / `resume_hunter_choice` names consistent T6→T7→T9; `enemy_attack_kickoff` introduced T9 and called from both `enemy_phase` and `resume_hunter_choice`.
- **Verify-before-coding callouts** are embedded where the live code must be read (draw_encounter_card refactor T8, step_phase callers T9, CardMetadata field list T8, encounter_spawn.rs pattern T10).
