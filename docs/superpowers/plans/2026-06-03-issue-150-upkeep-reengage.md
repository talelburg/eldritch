# #150 — Re-engage Readied Enemies at Upkeep 4.3 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** At Upkeep step 4.3, an enemy that was exhausted (and unengaged) and is co-located with an active investigator engages that investigator as soon as it is readied (RR p.10).

**Architecture:** Extend `ready_exhausted_cards` in `crates/game-core/src/engine/dispatch.rs` with a second pass: after the (simultaneous) readying loop, call the existing `reengage_at_location` helper for each just-readied enemy that is still unengaged. The function stays a synchronous `fn` — `reengage_at_location` auto-picks the lead on a prey tie rather than suspending, so no `EngineOutcome` threading.

**Tech Stack:** Rust, `cargo test -p game-core`. Spec: `docs/superpowers/specs/2026-06-03-issue-150-upkeep-reengage-design.md`.

---

## File Structure

- **Modify:** `crates/game-core/src/engine/dispatch.rs`
  - `ready_exhausted_cards` (~line 4696): add the second-pass engagement loop + a sentence to its doc-comment.
  - `#[cfg(test)] mod tests` (same file, where `ready_exhausted_cards_*` tests already live ~line 7191): add three tests. `test_enemy`, `test_location`, `test_investigator`, `LocationId`, `EnemyId`, `InvestigatorId`, and the `assert_event!` / `assert_no_event!` macros are already in scope in this module.

No new files, no signature changes, no new imports.

---

## Task 1: Engage-on-ready core

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — `ready_exhausted_cards` and its test module.

- [ ] **Step 1: Write the failing test**

Add this test inside the `#[cfg(test)] mod tests` block in `dispatch.rs`, next to `ready_exhausted_cards_readies_investigator_cards_and_enemies`:

```rust
#[test]
fn ready_exhausted_cards_reengages_co_located_unengaged_enemy() {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(1);
    let loc = test_location(10, "Synth Loc");
    let mut enemy = test_enemy(1, "Test Enemy");
    enemy.exhausted = true; // exhausted + disengaged, e.g. survived a successful Evade
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = None;
    let mut state = TestGame::default()
        .with_investigator(test_investigator(1))
        .with_location(loc)
        .with_enemy(enemy)
        .with_turn_order([inv_id])
        .build();
    // Co-locate the investigator with the enemy.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .current_location = Some(LocationId(10));
    let mut events = Vec::new();

    ready_exhausted_cards(&mut state, &mut events);

    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert_eq!(
        state.enemies[&enemy_id].engaged_with,
        Some(inv_id),
        "readied enemy re-engages the co-located investigator (RR p.10)"
    );
    assert_event!(events, Event::EnemyReadied { enemy } if *enemy == enemy_id);
    assert_event!(events, Event::EnemyEngaged { investigator, .. } if *investigator == inv_id);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core ready_exhausted_cards_reengages_co_located_unengaged_enemy`
Expected: FAIL — `engaged_with` is still `None` (assert_eq! panics) and no `EnemyEngaged` event was pushed, because the current `ready_exhausted_cards` only flips `exhausted` without an engagement check.

- [ ] **Step 3: Write minimal implementation**

In `ready_exhausted_cards`, replace the enemy readying loop so it records the ids it readies, then add the second-pass engagement loop. The full updated enemy portion of the function (the investigator-card loop above it is unchanged):

```rust
    let enemy_ids: Vec<EnemyId> = state.enemies.keys().copied().collect();
    let mut newly_readied: Vec<EnemyId> = Vec::new();
    for eid in enemy_ids {
        let enemy = state.enemies.get_mut(&eid).expect("id from keys");
        if enemy.exhausted {
            enemy.exhausted = false;
            events.push(Event::EnemyReadied { enemy: eid });
            newly_readied.push(eid);
        }
    }
    // RR p.10: "if an exhausted enemy at the same location as an investigator
    // becomes ready, it engages as soon as it is readied." Runs after the
    // (simultaneous, RR p.25) readying pass. Only newly-readied enemies are
    // checked ("becomes ready"), and only those still unengaged —
    // reengage_at_location's precondition is engaged_with == None, so an enemy
    // that readied while still engaged keeps its existing engagement.
    // newly_readied is in ascending EnemyId order (BTreeMap key order).
    for eid in newly_readied {
        if state.enemies[&eid].engaged_with.is_none() {
            reengage_at_location(state, events, eid);
        }
    }
```

Also append one sentence to the `ready_exhausted_cards` doc-comment (after the existing description of the simultaneous ready), so the engage step is documented at the definition:

```rust
/// After readying, each enemy that became ready while unengaged and
/// co-located with an investigator engages it via [`reengage_at_location`]
/// (Rules Reference p.10: "if an exhausted enemy at the same location as an
/// investigator becomes ready, it engages as soon as it is readied").
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p game-core ready_exhausted_cards_reengages_co_located_unengaged_enemy`
Expected: PASS.

- [ ] **Step 5: Run the pre-existing ready tests to confirm no regression**

Run: `cargo test -p game-core ready_exhausted_cards`
Expected: PASS — `ready_exhausted_cards_readies_investigator_cards_and_enemies` and `ready_exhausted_cards_leaves_ready_cards_untouched` still pass (the latter has no enemies co-located, so the second pass is a no-op and emits nothing).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: re-engage readied enemies at Upkeep 4.3 (#150)"
```

---

## Task 2: Negative-path regression guards

These two tests pass with the Task-1 implementation already in place; they pin the two no-engagement branches so a future change can't silently start engaging in them.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` — test module.

- [ ] **Step 1: Write the "no co-located investigator" test**

```rust
#[test]
fn ready_exhausted_cards_no_engage_when_no_co_located_investigator() {
    let enemy_id = EnemyId(1);
    let inv_id = InvestigatorId(1);
    let loc = test_location(10, "Synth Loc");
    let mut enemy = test_enemy(1, "Test Enemy");
    enemy.exhausted = true;
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = None;
    let mut state = TestGame::default()
        .with_investigator(test_investigator(1)) // current_location stays None
        .with_location(loc)
        .with_enemy(enemy)
        .with_turn_order([inv_id])
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut state, &mut events);

    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert_eq!(
        state.enemies[&enemy_id].engaged_with, None,
        "no investigator at the enemy's location → no engagement"
    );
    assert_no_event!(events, Event::EnemyEngaged { .. });
}
```

- [ ] **Step 2: Write the "already engaged" test**

```rust
#[test]
fn ready_exhausted_cards_keeps_existing_engagement_no_duplicate() {
    let enemy_id = EnemyId(1);
    let inv_id = InvestigatorId(1);
    let mut enemy = test_enemy(1, "Test Enemy");
    enemy.exhausted = true; // exhausted but still engaged (e.g. attacked last Enemy phase)
    enemy.engaged_with = Some(inv_id);
    let mut state = TestGame::default()
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();

    ready_exhausted_cards(&mut state, &mut events);

    assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
    assert_eq!(
        state.enemies[&enemy_id].engaged_with,
        Some(inv_id),
        "an already-engaged enemy keeps its engagement"
    );
    assert_no_event!(events, Event::EnemyEngaged { .. });
}
```

- [ ] **Step 3: Run both new tests to verify they pass**

Run: `cargo test -p game-core ready_exhausted_cards`
Expected: PASS — all five `ready_exhausted_cards_*` tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "engine: regression guards for Upkeep 4.3 no-engage branches (#150)"
```

---

## Task 3: Full CI gauntlet

**Files:** none (verification only).

- [ ] **Step 1: Run the five-job gauntlet locally (CLAUDE.md strict flags)**

Run, in order, and confirm each is clean:

```bash
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```

Expected: all pass with no warnings. The intra-doc link `[`reengage_at_location`]` added to the doc-comment must resolve under `cargo doc` (it points to a sibling `fn` in the same module — already referenced this way elsewhere in the file).

- [ ] **Step 2: Confirm no stray changes**

Run: `git status --short`
Expected: clean working tree (the phase-4 doc `#153`-move edit is handled separately at PR-ready time, not in these commits).

---

## Post-plan process (handled by the executor, per CLAUDE.md PR procedure — not plan tasks)

- Phase-doc update: at PR-ready time, in `docs/phases/phase-4-scenario-plumbing.md`, move `#150` from the open Issues table to the Closed table (PR #), flip its Ordering row to `✅ PR #N`, and fold in the already-staged `#153`→Phase-8 working-tree edit as the same final doc commit.
- Open the PR with the repo template; body explains the second-pass / unengaged-guard design and cites RR p.10 + p.25 verbatim; ends with `Closes #150.`
- Watch CI; merge only after explicit user approval.

---

## Self-Review

- **Spec coverage:** Logic change (second pass + three guards) → Task 1 (engage + newly-readied + unengaged guards; the "investigator cards never engage" guard is structural — only the enemy loop feeds `newly_readied`). Three spec tests → Task 1 test (engages) + Task 2 tests (no co-located, already engaged). Aloof out-of-scope → untouched (no code references it). Rules clauses → quoted in the doc-comment and code comment.
- **Placeholder scan:** none — every step has concrete code or a concrete command.
- **Type consistency:** `Event::EnemyReadied { enemy }`, `Event::EnemyEngaged { enemy, investigator }`, `reengage_at_location(&mut GameState, &mut Vec<Event>, EnemyId)`, `Enemy.engaged_with: Option<InvestigatorId>`, `Enemy.current_location: Option<LocationId>` all verified against the current source. Builder methods `with_location` / `with_enemy` / `with_turn_order` / `with_investigator` and fixtures `test_enemy` / `test_location` / `test_investigator` verified present.
