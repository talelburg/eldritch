# Investigator Elimination Flow (#144) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `apply_investigator_defeat` to execute the full Rules Reference p.10 Elimination flow (remove cards, place clues, disengage + prey-re-engage enemies, with documented no-ops for steps 4/5/6), keeping the function synchronous.

**Architecture:** All five steps run inside `apply_investigator_defeat` (the single defeat chokepoint), between the existing `InvestigatorDefeated` emit and `check_all_defeated`. A new reusable helper `reengage_at_location` resolves co-located re-engagement via the existing `resolve_prey` (#128), auto-picking the lead on a tie (deferred-UX, no suspension). One new `Investigator` field backs the removed-from-game pile.

**Tech Stack:** Rust, `game-core` crate. Engine unit tests in `crates/game-core/src/engine/dispatch.rs` under `#[cfg(test)]` (which can call the private `apply_investigator_defeat` directly), using `TestGame` + `test_investigator` / `test_location` / `test_enemy` fixtures and the `assert_event!` / `assert_no_event!` macros.

**CI gauntlet (run before every commit that ends a task):**
```sh
RUSTFLAGS="-D warnings" cargo test -p game-core
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

---

## File Structure

- `crates/game-core/src/state/investigator.rs` — add `removed_from_game` field + update the in-file doctest/unit constructor.
- `crates/game-core/src/test_support/fixtures.rs` — add the field to `test_investigator`.
- `crates/game-core/src/engine/dispatch.rs` — extend `apply_investigator_defeat`; add `reengage_at_location`; update the early-break doc-comment + `TODO(#144)` markers; all new tests.

No `event.rs` change: reuses `LocationCluesChanged`, `EnemyDisengaged`, `EnemyEngaged`, `AllInvestigatorsDefeated`.

---

## Task 1: Add the `removed_from_game` pile to `Investigator`

**Files:**
- Modify: `crates/game-core/src/state/investigator.rs` (struct + in-file test constructor)
- Modify: `crates/game-core/src/test_support/fixtures.rs:34-58` (`test_investigator`)

- [ ] **Step 1: Add the field to the struct**

In `crates/game-core/src/state/investigator.rs`, add after the `mulligan_used` field (the last field, currently at line 85), before the closing `}` of `struct Investigator`:

```rust
    /// Cards removed from the game (Rules Reference p.10 Elimination
    /// step 1). When this investigator is eliminated, every card they
    /// control or own in an out-of-play area — `hand`, `deck`,
    /// `discard`, `cards_in_play` — is drained into this pile and
    /// removed from the game. Stays empty for Active investigators.
    /// `#[serde(default)]` so states serialized before this field
    /// existed still deserialize.
    #[serde(default)]
    pub removed_from_game: Vec<CardCode>,
```

(`CardCode` is already imported at the top of the file via `use super::card::{CardCode, CardInPlay};`.)

- [ ] **Step 2: Update the `test_investigator` fixture**

In `crates/game-core/src/test_support/fixtures.rs`, in `test_investigator` (line 34), add after `mulligan_used: false,` (line 57), before the closing `}`:

```rust
        removed_from_game: Vec::new(),
```

- [ ] **Step 3: Find any other `Investigator { .. }` literal and fix it**

Run: `grep -rn "Investigator {" crates/ --include=*.rs`
Expected: matches in `test_support/fixtures.rs` (just edited) and `state/investigator.rs` (the doctest/unit test, if any constructs the full struct). If `state/investigator.rs` has a literal constructor in a `#[cfg(test)]` block, add `removed_from_game: Vec::new(),` to it. (At time of writing there is no full-struct literal in `investigator.rs`'s tests, only field reads — confirm with the grep and only edit literals.)

- [ ] **Step 4: Add a serde-default unit test**

In `crates/game-core/src/state/investigator.rs`, inside a `#[cfg(test)] mod` block (create one at the end of the file if none exists — match the existing `location.rs` test-module style):

```rust
#[cfg(test)]
mod removed_from_game_tests {
    use super::*;

    #[test]
    fn new_investigator_has_empty_removed_pile() {
        let inv = Investigator {
            id: InvestigatorId(1),
            name: "Test".into(),
            current_location: None,
            skills: Skills { willpower: 3, intellect: 3, combat: 3, agility: 3 },
            max_health: 8,
            damage: 0,
            max_sanity: 8,
            horror: 0,
            clues: 0,
            resources: 0,
            actions_remaining: 3,
            status: Status::Active,
            deck: Vec::new(),
            hand: Vec::new(),
            discard: Vec::new(),
            cards_in_play: Vec::new(),
            mulligan_used: false,
            removed_from_game: Vec::new(),
        };
        assert!(inv.removed_from_game.is_empty());
    }

    #[test]
    fn deserializes_when_field_absent() {
        // A JSON object missing `removed_from_game` must still parse
        // (serde default), proving forward-compat for pre-field states.
        let json = r#"{
            "id": 1, "name": "Test", "current_location": null,
            "skills": {"willpower":3,"intellect":3,"combat":3,"agility":3},
            "max_health": 8, "damage": 0, "max_sanity": 8, "horror": 0,
            "clues": 0, "resources": 0, "actions_remaining": 3,
            "status": "Active", "deck": [], "hand": [], "discard": [],
            "cards_in_play": [], "mulligan_used": false
        }"#;
        let inv: Investigator = serde_json::from_str(json).expect("deserialize");
        assert!(inv.removed_from_game.is_empty());
    }
}
```

Note: `InvestigatorId` is defined in this file; `Skills`, `Status` too. `serde_json` is already a dev/normal dependency of `game-core` (used by `location.rs` tests). If the `InvestigatorId(1)` / field set drifts, mirror whatever `test_investigator` constructs.

- [ ] **Step 5: Run the tests to verify they pass and the crate compiles**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core removed_from_game`
Expected: PASS (2 tests). Then `cargo build -p game-core` clean.

- [ ] **Step 6: Run the full gauntlet and commit**

Run the CI gauntlet (top of plan). Then:

```bash
git add crates/game-core/src/state/investigator.rs crates/game-core/src/test_support/fixtures.rs
git commit -m "engine: add removed_from_game pile to Investigator (#144)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Step 1 — drain controlled/owned cards into the removed-from-game pile

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — `apply_investigator_defeat` (line ~2856) + new test

**Context:** `apply_investigator_defeat` currently (lines 2856–2884): looks up the investigator, returns early if not `Active`, flips `status`, pushes `InvestigatorDefeated`, calls `check_all_defeated`. We insert the elimination steps **after** the `InvestigatorDefeated` push and **before** `check_all_defeated`. This task adds only step 1.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `dispatch.rs` (the same module that holds `resolve_attacks_for_investigator_*` tests — those call the private fn directly, so this one can too):

```rust
#[test]
fn elimination_step1_removes_controlled_and_owned_cards() {
    use crate::state::CardInPlay;
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.max_health = 1;
    inv.hand = vec![CardCode("h1".into()), CardCode("h2".into())];
    inv.deck = vec![CardCode("d1".into())];
    inv.discard = vec![CardCode("x1".into())];
    inv.cards_in_play = vec![CardInPlay::enter_play(CardCode("p1".into()), CardInstanceId(1))];

    let mut state = TestGame::default().with_investigator(inv).build();
    let mut events = Vec::new();

    apply_investigator_defeat(&mut state, &mut events, id, DefeatCause::Damage);

    let after = &state.investigators[&id];
    assert!(after.hand.is_empty(), "hand drained");
    assert!(after.deck.is_empty(), "deck drained");
    assert!(after.discard.is_empty(), "discard drained");
    assert!(after.cards_in_play.is_empty(), "cards_in_play drained");
    // All five codes landed in the removed pile (order: in-play, hand, deck, discard).
    let removed: Vec<&str> = after.removed_from_game.iter().map(|c| c.as_str()).collect();
    assert_eq!(removed.len(), 5, "all controlled/owned cards removed");
    assert!(removed.contains(&"p1"));
    assert!(removed.contains(&"h1"));
    assert!(removed.contains(&"d1"));
    assert!(removed.contains(&"x1"));
}
```

Check the imports at the top of the test module include `CardInstanceId` and `CardCode` (add to the existing `use crate::state::{...}` line if missing). `CardInPlay::enter_play` is the constructor referenced in `card.rs`; confirm its exact signature with `grep -n "pub fn enter_play" crates/game-core/src/state/card.rs` and match it (it takes the code and an instance id).

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core elimination_step1 -- --nocapture`
Expected: FAIL — `cards_in_play`/`hand`/etc. are NOT drained (the assertion `hand drained` fails), because `apply_investigator_defeat` doesn't touch them yet.

- [ ] **Step 3: Implement step 1**

In `apply_investigator_defeat`, replace the body from the status flip through `check_all_defeated`. The new shape (keep the early-`return` guard and the `events.push(InvestigatorDefeated)` exactly as they are; insert step 1 between the push and `check_all_defeated`):

```rust
    inv.status = match cause {
        DefeatCause::Damage => Status::Killed,
        DefeatCause::Horror => Status::Insane,
        DefeatCause::Resigned => Status::Resigned,
    };
    events.push(Event::InvestigatorDefeated {
        investigator,
        cause,
    });

    // Rules Reference p.10 Elimination steps 1–5 run here, between the
    // defeat event and the all-defeated check (step 6 signal). See the
    // design doc 2026-05-31-144 for the full breakdown.
    run_elimination_steps(state, events, investigator);

    check_all_defeated(state, events);
```

Then add the new private fn near `apply_investigator_defeat` (step 1 only for now; later tasks fill in 2 & 3):

```rust
/// Execute Rules Reference p.10 Elimination steps 1–5 for an
/// investigator whose `status` has just been flipped to a defeated
/// variant. Synchronous: the step-3 re-engagement tie auto-picks the
/// lead rather than suspending (see `reengage_at_location`).
fn run_elimination_steps(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    // Step 1: remove every card this investigator controls in play and
    // owns in out-of-play areas (hand/deck/discard) from the game.
    let inv = state.investigators.get_mut(&investigator).unwrap_or_else(|| {
        unreachable!(
            "run_elimination_steps: investigator {investigator:?} not in map; state corruption"
        )
    });
    // Build the pile in an owned local so each `extend` borrows only one
    // field of `inv` at a time (extending `inv.removed_from_game` directly
    // from `inv.hand.drain(..)` would borrow two fields of `inv` at once
    // through the argument — rejected by the borrow checker).
    let mut removed = std::mem::take(&mut inv.removed_from_game);
    removed.extend(inv.cards_in_play.drain(..).map(|c| c.code));
    removed.extend(inv.hand.drain(..));
    removed.extend(inv.deck.drain(..));
    removed.extend(inv.discard.drain(..));
    inv.removed_from_game = removed;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core elimination_step1`
Expected: PASS.

- [ ] **Step 5: Run the gauntlet and commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: elimination step 1 — remove controlled/owned cards (#144)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Step 2 — place clues at the investigator's location, return resources

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — `run_elimination_steps` + new test

**Context:** Read the eliminated investigator's location once at the top of `run_elimination_steps` (it's needed by both step 2 and step 3). `Location.clues` and `Event::LocationCluesChanged { location, new_count }` already exist.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn elimination_step2_places_clues_at_location_and_zeroes_resources() {
    let id = InvestigatorId(1);
    let loc_id = LocationId(1);
    let mut inv = test_investigator(1);
    inv.max_health = 1;
    inv.current_location = Some(loc_id);
    inv.clues = 2;
    inv.resources = 4;

    let mut loc = test_location(1, "Study");
    loc.clues = 1;

    let mut state = TestGame::default()
        .with_investigator(inv)
        .with_location(loc)
        .build();
    let mut events = Vec::new();

    apply_investigator_defeat(&mut state, &mut events, id, DefeatCause::Damage);

    assert_eq!(state.locations[&loc_id].clues, 3, "2 investigator clues added to location's 1");
    assert_eq!(state.investigators[&id].clues, 0, "investigator clues cleared");
    assert_eq!(state.investigators[&id].resources, 0, "resources returned to pool");
    assert_event!(events, Event::LocationCluesChanged { location: loc_id, new_count: 3 });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core elimination_step2`
Expected: FAIL — location still has 1 clue; investigator still has 2 clues / 4 resources.

- [ ] **Step 3: Implement step 2**

In `run_elimination_steps`, capture the location at the top (before step 1's `get_mut` borrow, or re-read after — capture it first to avoid borrow overlap), then add step 2 after step 1:

```rust
fn run_elimination_steps(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    // The location the investigator was at "when eliminated" — read once;
    // steps 2 and 3 both place tokens/enemies here.
    let last_location = state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location);

    // Step 1: remove controlled/owned cards (owned-local pattern, as in
    // Task 2 — avoids borrowing two `inv` fields at once).
    let inv = state.investigators.get_mut(&investigator).unwrap_or_else(|| {
        unreachable!(
            "run_elimination_steps: investigator {investigator:?} not in map; state corruption"
        )
    });
    let mut removed = std::mem::take(&mut inv.removed_from_game);
    removed.extend(inv.cards_in_play.drain(..).map(|c| c.code));
    removed.extend(inv.hand.drain(..));
    removed.extend(inv.deck.drain(..));
    removed.extend(inv.discard.drain(..));
    inv.removed_from_game = removed;

    // Step 2: place possessed clues at the location; return resources to
    // the (unmodeled, infinite) token pool by zeroing them.
    let clues = inv.clues;
    inv.clues = 0;
    inv.resources = 0;
    if clues > 0 {
        if let Some(loc_id) = last_location {
            if let Some(loc) = state.locations.get_mut(&loc_id) {
                loc.clues = loc.clues.saturating_add(clues);
                let new_count = loc.clues;
                events.push(Event::LocationCluesChanged {
                    location: loc_id,
                    new_count,
                });
            }
        }
    }
}
```

(The `inv` binding from step 1 is still in scope and mutable; reuse it for the `clues`/`resources` reads/writes before the `state.locations.get_mut` re-borrow — note `inv` borrows `state.investigators` while `state.locations` is a different field, but the borrow checker sees `state` as a whole. To satisfy it: read `clues` and zero `inv.clues`/`inv.resources` while holding the `inv` borrow, then **drop `inv`** by ending its use before `state.locations.get_mut`. The code above does exactly this — the last use of `inv` is `inv.resources = 0;`, after which `inv` is dead and `state.locations.get_mut` borrows freely.)

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core elimination_step2`
Expected: PASS. If the borrow checker complains, hoist the `clues`/`resources` mutations into a small block `{ let inv = ...; clues = inv.clues; inv.clues = 0; inv.resources = 0; }` so the borrow ends explicitly.

- [ ] **Step 5: Run the gauntlet and commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: elimination step 2 — clues to location, resources to pool (#144)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Step 3 — the `reengage_at_location` helper

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — add `reengage_at_location` + its unit tests

**Context:** Build the reusable helper first, in isolation, with its own tests. It mirrors `engage_on_arrival` (line ~3243) but **does not suspend** — on a prey `Tie` it auto-engages the lead (`tied[0]`, which is `turn_order`-first because `active_investigators_at` returns turn-order-ordered candidates). It is a no-op for exhausted enemies and for enemies with no location. `resolve_prey`, `active_investigators_at`, and `engage_enemy_with` already exist.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn reengage_at_location_engages_sole_co_located_survivor() {
    let surv = InvestigatorId(2);
    let loc = LocationId(1);
    let survivor = {
        let mut i = test_investigator(2);
        i.current_location = Some(loc);
        i
    };
    let enemy = {
        let mut e = test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = None;
        e
    };
    let mut state = TestGame::default()
        .with_investigator(survivor)
        .with_location(test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([surv])
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut state, &mut events, EnemyId(1));

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(surv));
    assert_event!(events, Event::EnemyEngaged { enemy: EnemyId(1), investigator: surv });
}

#[test]
fn reengage_at_location_no_co_located_investigator_leaves_unengaged() {
    let loc = LocationId(1);
    let enemy = {
        let mut e = test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = None;
        e
    };
    let mut state = TestGame::default()
        .with_location(test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([])
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut state, &mut events, EnemyId(1));

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn reengage_at_location_tie_auto_picks_lead_first_in_turn_order() {
    // Two co-located survivors, Prey::Default → tie → engage turn_order-first (lead).
    let lead = InvestigatorId(2);
    let other = InvestigatorId(3);
    let loc = LocationId(1);
    let mk = |raw: u32| {
        let mut i = test_investigator(raw);
        i.current_location = Some(loc);
        i
    };
    let enemy = {
        let mut e = test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = None;
        e.prey = crate::card_data::Prey::Default;
        e
    };
    let mut state = TestGame::default()
        .with_investigator(mk(2))
        .with_investigator(mk(3))
        .with_location(test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([lead, other]) // lead first
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut state, &mut events, EnemyId(1));

    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(lead),
        "tie engages the lead (turn_order-first)"
    );
}

#[test]
fn reengage_at_location_exhausted_enemy_does_not_engage() {
    let surv = InvestigatorId(2);
    let loc = LocationId(1);
    let survivor = {
        let mut i = test_investigator(2);
        i.current_location = Some(loc);
        i
    };
    let enemy = {
        let mut e = test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = None;
        e.exhausted = true; // exhausted unengaged enemy does not engage (RR p.10)
        e
    };
    let mut state = TestGame::default()
        .with_investigator(survivor)
        .with_location(test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([surv])
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut state, &mut events, EnemyId(1));

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core reengage_at_location`
Expected: FAIL to compile — `reengage_at_location` doesn't exist yet.

- [ ] **Step 3: Implement the helper**

Add near `engage_on_arrival` in `dispatch.rs`:

```rust
/// Engage a now-unengaged enemy with a co-located investigator per the
/// general engagement rule (Rules Reference p.10): "Any time a ready
/// unengaged enemy is at the same location as an investigator, it
/// engages that investigator … follow the enemy's prey instructions."
///
/// No-op when the enemy is exhausted (an exhausted unengaged enemy does
/// not engage until readied) or has no location. On a prey `Tie` this
/// engages the lead (`tied[0]`, which is `turn_order`-first because
/// `active_investigators_at` is turn-order-ordered) rather than
/// suspending for the lead's `PickInvestigator` — keeping every defeat
/// caller synchronous. TODO(#<phase-8-issue>): make the multiplayer tie
/// an interactive lead choice when multiplayer lands.
///
/// Shared primitive: the elimination flow's step-3 re-engagement is the
/// first consumer; the Upkeep-4.3 "engage on ready" gap (separate issue)
/// will reuse it.
fn reengage_at_location(state: &mut GameState, events: &mut Vec<Event>, enemy_id: EnemyId) {
    let enemy = &state.enemies[&enemy_id];
    if enemy.exhausted {
        return;
    }
    let Some(loc) = enemy.current_location else {
        return;
    };
    let prey = enemy.prey;
    let candidates = active_investigators_at(state, loc);
    match resolve_prey(state, prey, &candidates) {
        PreyResolution::None => {}
        PreyResolution::One(target) => engage_enemy_with(state, events, enemy_id, target),
        PreyResolution::Tie(tied) => engage_enemy_with(state, events, enemy_id, tied[0]),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core reengage_at_location`
Expected: PASS (4 tests).

- [ ] **Step 5: Run the gauntlet and commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: reengage_at_location helper (prey, auto-lead-on-tie) (#144)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Step 3 — wire disengage + re-engage into `run_elimination_steps`, clear location

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — `run_elimination_steps` + tests

**Context:** Two-pass step 3 (disengage all "simultaneously," then re-engage), then clear the eliminated investigator's `current_location`. Steps 4 & 5 are documented no-ops.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn elimination_step3_disengages_then_reengages_ready_enemy_onto_survivor() {
    let dead = InvestigatorId(1);
    let surv = InvestigatorId(2);
    let loc = LocationId(1);

    let mut dying = test_investigator(1);
    dying.max_health = 1;
    dying.current_location = Some(loc);

    let mut survivor = test_investigator(2);
    survivor.current_location = Some(loc);

    let enemy = {
        let mut e = test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = Some(dead); // engaged with the about-to-die investigator
        e
    };

    let mut state = TestGame::default()
        .with_investigator(dying)
        .with_investigator(survivor)
        .with_location(test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_defeat(&mut state, &mut events, dead, DefeatCause::Damage);

    assert_event!(events, Event::EnemyDisengaged { enemy: EnemyId(1), investigator: dead });
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(surv),
        "ready enemy re-engages the co-located survivor"
    );
    assert_event!(events, Event::EnemyEngaged { enemy: EnemyId(1), investigator: surv });
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(loc));
    assert_eq!(state.investigators[&dead].current_location, None, "eliminated ⇒ between locations");
}

#[test]
fn elimination_step3_solo_defeat_leaves_enemy_unengaged() {
    let dead = InvestigatorId(1);
    let loc = LocationId(1);

    let mut dying = test_investigator(1);
    dying.max_health = 1;
    dying.current_location = Some(loc);

    let enemy = {
        let mut e = test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = Some(dead);
        e
    };

    let mut state = TestGame::default()
        .with_investigator(dying)
        .with_location(test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([dead])
        .build();
    let mut events = Vec::new();

    apply_investigator_defeat(&mut state, &mut events, dead, DefeatCause::Damage);

    assert_event!(events, Event::EnemyDisengaged { enemy: EnemyId(1), investigator: dead });
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        None,
        "no surviving co-located investigator ⇒ stays unengaged"
    );
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn elimination_step3_exhausted_engaged_enemy_disengages_but_does_not_reengage() {
    let dead = InvestigatorId(1);
    let surv = InvestigatorId(2);
    let loc = LocationId(1);

    let mut dying = test_investigator(1);
    dying.max_health = 1;
    dying.current_location = Some(loc);

    let mut survivor = test_investigator(2);
    survivor.current_location = Some(loc);

    let enemy = {
        let mut e = test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = Some(dead);
        e.exhausted = true; // does not re-engage even with a co-located survivor
        e
    };

    let mut state = TestGame::default()
        .with_investigator(dying)
        .with_investigator(survivor)
        .with_location(test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_defeat(&mut state, &mut events, dead, DefeatCause::Damage);

    assert_event!(events, Event::EnemyDisengaged { enemy: EnemyId(1), investigator: dead });
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core elimination_step3`
Expected: FAIL — `engaged_with` is still `Some(dead)` (no disengage), no `EnemyDisengaged` event, `current_location` not cleared.

- [ ] **Step 3: Implement step 3 + location clear + steps 4/5 doc no-ops**

Append to `run_elimination_steps`, after step 2's clue/resource block:

```rust
    // Step 3: disengage every enemy engaged with the eliminated
    // investigator, placing them at the investigator's last location
    // "unengaged but otherwise maintaining their current game state"
    // (RR p.10). Disengage all first (simultaneous), then let the ready
    // ones re-engage a surviving co-located investigator per prey.
    let affected: Vec<EnemyId> = state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator))
        .map(|(id, _)| *id)
        .collect();
    for &eid in &affected {
        let enemy = state.enemies.get_mut(&eid).unwrap_or_else(|| {
            unreachable!("run_elimination_steps: enemy {eid:?} vanished; state corruption")
        });
        enemy.engaged_with = None;
        enemy.current_location = last_location;
        events.push(Event::EnemyDisengaged {
            enemy: eid,
            investigator,
        });
    }
    for &eid in &affected {
        reengage_at_location(state, events, eid);
    }

    // Step 4: place other (non-enemy) threat-area cards in the
    // appropriate discard pile. No-op: treachery/asset-in-threat-area
    // state is not modeled yet (enemies are the only threat-area
    // occupants). TODO: wire when threat-area cards land (Phase 7+).

    // Step 5: lead-investigator transfer. No-op by construction: there
    // is no stored lead; `first_active_investigator` recomputes the lead
    // as the first Active investigator in `turn_order`, so a defeated
    // lead is automatically replaced. UX for "remaining players choose"
    // is deferred (Phase 8) alongside the re-engagement-tie pick.

    // Step 6 (no remaining players → scenario ends) is signaled by
    // `check_all_defeated` (caller) emitting AllInvestigatorsDefeated;
    // the Resolution::Lost consequence is wired by #73.

    // The investigator has left play — clear their location last, after
    // steps 2 & 3 consumed `last_location`.
    if let Some(inv) = state.investigators.get_mut(&investigator) {
        inv.current_location = None;
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core elimination_step3`
Expected: PASS (3 tests).

- [ ] **Step 5: Run the full elimination suite + the #71 early-break regression**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core elimination_ && cargo test -p game-core resolve_attacks_for_investigator_early_breaks`
Expected: all PASS (the early-break test still passes — now backed by real disengage).

- [ ] **Step 6: Run the gauntlet and commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: elimination step 3 — disengage + prey re-engage; clear location (#144)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Defeat-by-horror coverage + early-break doc-comment + TODO cleanup

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — one test + two comment edits

- [ ] **Step 1: Write a defeat-by-horror test (proves the flow is cause-agnostic)**

```rust
#[test]
fn elimination_runs_on_horror_defeat_too() {
    let dead = InvestigatorId(1);
    let surv = InvestigatorId(2);
    let loc = LocationId(1);

    let mut dying = test_investigator(1);
    dying.max_sanity = 1;
    dying.current_location = Some(loc);
    dying.clues = 1;

    let mut survivor = test_investigator(2);
    survivor.current_location = Some(loc);

    let enemy = {
        let mut e = test_enemy(1, "Whippoorwill");
        e.current_location = Some(loc);
        e.engaged_with = Some(dead);
        e
    };

    let mut state = TestGame::default()
        .with_investigator(dying)
        .with_investigator(survivor)
        .with_location(test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_defeat(&mut state, &mut events, dead, DefeatCause::Horror);

    assert_eq!(state.investigators[&dead].status, Status::Insane);
    assert_eq!(state.locations[&loc].clues, 1, "clue placed at location");
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(surv), "re-engaged survivor");
    assert_eq!(state.investigators[&dead].current_location, None);
}
```

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core elimination_runs_on_horror_defeat_too`
Expected: PASS immediately (the flow is cause-agnostic — this is a characterization test, no new code).

- [ ] **Step 2: Update the #71 early-break doc-comment**

In `resolve_attacks_for_investigator`'s doc-comment (around dispatch.rs:3046–3054), replace the paragraph that says the disengage flow "lands in #144" / "Today `apply_investigator_defeat` only flips `Status`" with the post-#144 reality:

```rust
///    `apply_investigator_defeat` (#144) now clears `engaged_with` on
///    every enemy engaged with a defeated investigator (Rules Reference
///    p.10 Elimination step 3), so a disengaged enemy genuinely is no
///    longer "engaged" by the time the next loop iteration would run.
///    The early-break here is therefore redundant with that flow — it
///    is kept as the simpler, local form (one extra status check,
///    harmless) so the loop body stays self-evidently correct without
///    cross-referencing the elimination flow.
```

(Match surrounding `///` comment style; keep the rest of the doc-comment — the p.7/p.25 exhaust reasoning — intact.)

- [ ] **Step 3: Repoint the line-4629 `TODO(#144)`**

In `run_window_continuation`'s `InvestigationBegins` arm (around dispatch.rs:4629), the `TODO(#144)` for the no-active-investigator park branch belongs to #73 now (Resolution::Lost). Update it:

```rust
            // None branch: no active investigator can take a turn. Per
            // Rules Reference p.10 step 6 the scenario ends; #144 fires
            // AllInvestigatorsDefeated via check_all_defeated, but the
            // Resolution::Lost consequence (and removing this park) is
            // #73's resolution-layer work. TODO(#73): end the scenario
            // here instead of parking.
```

- [ ] **Step 4: Confirm no stale `TODO(#144)` remains except intentional ones**

Run: `grep -n "TODO(#144)" crates/game-core/src/engine/dispatch.rs`
Expected: no matches (line 3047's was rewritten in Step 2; line 4629's was repointed to #73 in Step 3). If any remain, they were missed — resolve them.

- [ ] **Step 5: Run the gauntlet and commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: horror-defeat coverage + update #71 early-break/park TODOs (#144)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: File follow-up issues

**Files:** none (GitHub only)

- [ ] **Step 1: File the Phase-8 interactive-tie issue**

```bash
gh issue create \
  --title "[engine] Interactive lead choice for re-engagement-tie (multiplayer)" \
  --label engine,p2-later \
  --body "Follow-up to #144. \`reengage_at_location\` auto-engages the lead (\`tied[0]\`) on a prey tie instead of suspending for the lead's \`PickInvestigator\`. When multiplayer (Phase 8) lands, make the multi-investigator tie an interactive choice, consistent with #128's hunter/spawn tie suspension and #137's deferred ChooseFirstActor. Single-player is unaffected (tie is unreachable with one investigator)."
```

- [ ] **Step 2: File the Upkeep-4.3 engage-on-ready issue**

```bash
gh issue create \
  --title "[engine] Upkeep 4.3: enemy engages on ready (Rules Reference p.10)" \
  --label engine,p2-later \
  --body "\`ready_exhausted_cards\` (Upkeep 4.3) readies enemies without running the engagement check. Per Rules Reference p.10, 'if an exhausted enemy at the same location as an investigator becomes ready, it engages as soon as it is readied.' Reachable today: a successful Evade exhausts + disengages an enemy, leaving it ready-able + unengaged + co-located; surviving to Upkeep, it should re-engage but doesn't. Fix: call \`reengage_at_location\` (added in #144) for each newly-readied enemy in \`ready_exhausted_cards\`."
```

- [ ] **Step 3: Note the new issue numbers and patch the TODO in `reengage_at_location`**

After `gh issue create` prints the URLs, replace `TODO(#<phase-8-issue>)` in `reengage_at_location`'s doc-comment with the real issue number from Step 1.

```bash
# Example, using the number gh printed:
# sed is fine here, but prefer the Edit tool to swap `#<phase-8-issue>` → `#NNN`.
grep -n "phase-8-issue" crates/game-core/src/engine/dispatch.rs
```

Edit that line to the real number, then:

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core reengage_at_location
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: point re-engagement-tie TODO at the filed follow-up (#144)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: Final verification + phase-doc update + PR

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md` (only now, as the final commit)

- [ ] **Step 1: Run the FULL CI gauntlet (all five jobs)**

```sh
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```
Expected: all clean. Fix anything before proceeding.

- [ ] **Step 2: Update the phase doc** (per `docs/phases/README.md` "Maintaining these docs")

In `docs/phases/phase-4-scenario-plumbing.md`:
- Move `#144` from the open Issues table to the **Closed** table with its PR number and a one-line note (steps 1–3 implemented; 4/5 documented no-ops; step-6 Resolution left to #73; `reengage_at_location` shared helper; auto-lead-on-tie with Phase-8 follow-up).
- Flip the Ordering row (slot 11) to `✅ PR #NN`.
- Update the Status line and open-issue counts (open: 3 → 2; remaining `#73`, `#147`).
- Add a **Decisions made** entry ONLY if load-bearing for a future PR — candidate: "`apply_investigator_defeat` stays synchronous; re-engagement tie auto-picks lead (interactive pick = Phase-8 follow-up); step-6 Resolution is #73's." Apply the test: would a future PR-author choose differently without it? The auto-lead-on-tie + step-6-split is worth one entry; skip the rest.

- [ ] **Step 3: Commit the phase-doc update and push the branch**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "docs: phase-4 — close #144 (elimination flow)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push -u origin engine/elimination-flow
```

- [ ] **Step 4: Open the PR**

```bash
gh pr create --fill --base main
```
Use the repo template; include a short design-decisions paragraph (synchronous defeat, auto-lead-on-tie, steps 4/5 no-ops, step-6 split to #73) and the verbatim p.10 Elimination clause that shapes step 3. End the body with `Closes #144.` and the Claude Code attribution.

- [ ] **Step 5: Watch CI**

```bash
gh pr checks <PR#> --watch
```
Fix failures with follow-up commits to the same branch (don't amend/force-push). Merge only after explicit user approval.

---

## Notes for the implementer

- `apply_investigator_defeat`, `run_elimination_steps`, and `reengage_at_location` are all **private** to `dispatch.rs`; the `#[cfg(test)] mod tests` in the same file calls them directly (see the existing `resolve_attacks_for_investigator_*` tests for the idiom).
- The test module's `use` block may need `CardCode`, `CardInstanceId`, `CardInPlay`, `LocationId`, `EnemyId` added — check the existing imports and extend rather than duplicate.
- Do NOT thread `EngineOutcome` anywhere — the whole point of the auto-lead-on-tie decision is that the defeat path stays synchronous.
- Event ordering within a single defeat: `InvestigatorDefeated` → (`LocationCluesChanged`) → `EnemyDisengaged`* → `EnemyEngaged`* → (`AllInvestigatorsDefeated`). Tests assert presence via `assert_event!`, not contiguous order, so this ordering is not brittle — but keep it causal.
```
