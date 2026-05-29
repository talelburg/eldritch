# Enemy Phase: Engagement Attacks (#71) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Phase-4 Enemy phase real — each Active investigator's engaged ready enemies attack and exhaust, framed by per-investigator + final Fast-windows — driven by a phase-driver mirroring the Mythos cursor machinery.

**Architecture:** An `enemy_phase` driver (step 3.1 + 3.2 hunter-movement stub + open the first per-investigator window) whose `run_window_continuation` arms execute the per-investigator attack loop (`resolve_attacks_for_investigator`), advance the `enemy_attack_pending` cursor, and ultimately hand to `enemy_phase_end` (step 3.4 + Enemy→Upkeep transition). The per-investigator-cursor pattern is extracted from Mythos via two shared helpers (`first_active_investigator`, `next_active_investigator_after`) so Mythos's seed and `advance_mythos_draw_pending` collapse to one-liners. `step_phase`'s `_` fallback becomes `unreachable!` once all four phases are driver-dispatched.

**Tech Stack:** Rust, `game-core` engine crate (no I/O, `wasm32`-compatible). Tests via the `TestGame` builder + `assert_event!` macros in `crates/game-core/src/engine/dispatch.rs`'s `#[cfg(test)]` blocks.

**Reference spec:** `docs/superpowers/specs/2026-05-28-71-enemy-phase-engagement-attacks-design.md`.

**Conventions for every commit in this plan:**
- Match CI strict flags before committing each task. The minimum gauntlet for engine-only tasks is:
  ```
  RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
  cargo clippy -p game-core --all-targets --all-features -- -D warnings
  cargo fmt --check
  ```
  Before opening the PR (after the final task), run the full five-job gauntlet from CLAUDE.md.
- Commit messages: `engine: <description>`, ending with the trailer:
  ```
  Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
  ```
- The spec + this plan get committed together as the first commit on the `engine/enemy-phase-attacks` branch when execution starts (per the user's instruction), before Task 1.

---

## File Structure

- `crates/game-core/src/state/game_state.rs` — add two `WindowKind` variants (Task 3); add `enemy_attack_pending` field on `GameState` (Task 2); two new `WindowKind` serde-roundtrip tests (Task 3); one new `enemy_attack_pending` serde-roundtrip test (Task 2).
- `crates/game-core/src/engine/dispatch.rs` — the bulk: shared cursor helpers + Mythos refactor (T1), `resolve_attacks_for_investigator` + `hunter_movement_step` + their unit tests (T4), `enemy_phase` + `enemy_phase_end` + wired `run_window_continuation` arms + `step_phase` wiring + driver-cascade unit tests (T5), pause/resume tests (T6).
- `crates/game-core/src/engine/dispatch.rs` lines 1734–1773 (`trigger_matches`) — add the two new variants to the no-pattern arm (Task 3).
- `docs/phases/phase-4-scenario-plumbing.md` — phase-doc update (Task 7), last commit before merge.

---

## Task 1: Shared cursor helpers + Mythos refactor

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (add `first_active_investigator`, `next_active_investigator_after`; replace inline code at `mythos_phase` lines 886–891 and `advance_mythos_draw_pending` lines 5006–5030).

**Why first:** The helpers have an immediate caller (Mythos's existing code), so they don't trip the `-D warnings` dead-code lint. The refactor is pure (no behavior change), and the existing Mythos cursor tests at lines 5258, 5293, 5322 act as the regression guard.

- [ ] **Step 1: Add the two helper functions**

Add adjacent to `active_investigators_in_turn_order` (line 3649) so the per-investigator-cursor helpers cluster together:

```rust
/// First investigator in [`turn_order`] whose status is
/// [`Status::Active`]. Eliminated investigators
/// ([`Status::Killed`] / [`Status::Insane`] / [`Status::Resigned`])
/// are skipped per Rules Reference p.10 (Elimination).
///
/// Used by per-investigator phase loops to seed their cursor:
/// Mythos 1.4 draws ([`mythos_phase`] seeds `mythos_draw_pending`),
/// Enemy 3.3 attacks ([`enemy_phase`] seeds `enemy_attack_pending`).
///
/// [`turn_order`]: GameState::turn_order
fn first_active_investigator(state: &GameState) -> Option<InvestigatorId> {
    state.turn_order.iter().copied().find(|id| {
        state
            .investigators
            .get(id)
            .is_some_and(|inv| inv.status == Status::Active)
    })
}

/// First investigator in [`turn_order`] whose status is
/// [`Status::Active`], positioned strictly after `current`. Returns
/// `None` when no Active investigator follows `current` in
/// `turn_order`, or when `current` is not in `turn_order` at all.
///
/// Eliminated investigators are skipped per Rules Reference p.10
/// (same predicate as [`first_active_investigator`]).
///
/// Used by per-investigator phase loops to advance their cursor:
/// `advance_mythos_draw_pending` after a draw chain completes, and
/// `run_window_continuation`'s `BeforeInvestigatorAttacked` arm after
/// one investigator's engaged-enemy attacks resolve.
///
/// Notable: `current` may itself be non-Active (e.g. defeated mid-loop
/// in Enemy phase) — using `turn_order` as the index basis (rather
/// than the filtered-Active list) makes this case the same single-pass
/// lookup.
///
/// [`turn_order`]: GameState::turn_order
fn next_active_investigator_after(
    state: &GameState,
    current: InvestigatorId,
) -> Option<InvestigatorId> {
    state
        .turn_order
        .iter()
        .position(|id| *id == current)
        .and_then(|idx| {
            state.turn_order.iter().skip(idx + 1).copied().find(|id| {
                state
                    .investigators
                    .get(id)
                    .is_some_and(|inv| inv.status == Status::Active)
            })
        })
}
```

- [ ] **Step 2: Refactor `mythos_phase` seed** (line 886–891)

Old code:
```rust
    state.mythos_draw_pending = state.turn_order.iter().copied().find(|id| {
        state
            .investigators
            .get(id)
            .is_some_and(|inv| inv.status == Status::Active)
    });
```

New code (same place):
```rust
    state.mythos_draw_pending = first_active_investigator(state);
```

- [ ] **Step 3: Refactor `advance_mythos_draw_pending` body** (line 5006–5030)

Old code:
```rust
fn advance_mythos_draw_pending(state: &mut GameState, events: &mut Vec<Event>) {
    let current = state
        .mythos_draw_pending
        .expect("advance_mythos_draw_pending called only after a successful chain");
    // Per Rules Reference p.10 (Elimination), eliminated investigators
    // do not draw encounter cards. Skip any non-Active entries after
    // the current position rather than blindly taking turn_order[idx+1].
    let next = state
        .turn_order
        .iter()
        .position(|id| *id == current)
        .and_then(|idx| {
            state.turn_order.iter().skip(idx + 1).copied().find(|id| {
                state
                    .investigators
                    .get(id)
                    .is_some_and(|inv| inv.status == Status::Active)
            })
        });

    state.mythos_draw_pending = next;
    if next.is_none() {
        open_fast_window(state, events, WindowKind::MythosAfterDraws);
    }
}
```

New code:
```rust
fn advance_mythos_draw_pending(state: &mut GameState, events: &mut Vec<Event>) {
    let current = state
        .mythos_draw_pending
        .expect("advance_mythos_draw_pending called only after a successful chain");
    // Eliminated-skip semantics live in `next_active_investigator_after`.
    let next = next_active_investigator_after(state, current);
    state.mythos_draw_pending = next;
    if next.is_none() {
        open_fast_window(state, events, WindowKind::MythosAfterDraws);
    }
}
```

- [ ] **Step 4: Add direct helper unit tests**

Add to the same `#[cfg(test)]` module that hosts the Mythos cursor tests (around line 5258). Place them at the bottom of that module.

```rust
    #[test]
    fn first_active_investigator_finds_first_active_skipping_eliminated() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .build();
        state.turn_order = vec![
            InvestigatorId(1),
            InvestigatorId(2),
            InvestigatorId(3),
        ];
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().status = Status::Killed;
        state.investigators.get_mut(&InvestigatorId(2)).unwrap().status = Status::Insane;

        assert_eq!(
            first_active_investigator(&state),
            Some(InvestigatorId(3)),
            "first Active in turn_order after skipping eliminated"
        );
    }

    #[test]
    fn first_active_investigator_returns_none_when_all_eliminated() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().status = Status::Killed;

        assert_eq!(first_active_investigator(&state), None);
    }

    #[test]
    fn first_active_investigator_returns_none_when_turn_order_empty() {
        let state = TestGame::default().build();
        assert_eq!(first_active_investigator(&state), None);
    }

    #[test]
    fn next_active_investigator_after_skips_eliminated_middle() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .with_investigator(test_investigator(4))
            .build();
        state.turn_order = vec![
            InvestigatorId(1),
            InvestigatorId(2),
            InvestigatorId(3),
            InvestigatorId(4),
        ];
        state.investigators.get_mut(&InvestigatorId(2)).unwrap().status = Status::Killed;

        assert_eq!(
            next_active_investigator_after(&state, InvestigatorId(1)),
            Some(InvestigatorId(3)),
            "advance from 1 skips Killed 2, lands on 3"
        );
        assert_eq!(
            next_active_investigator_after(&state, InvestigatorId(3)),
            Some(InvestigatorId(4)),
            "advance from 3 lands on 4"
        );
        assert_eq!(
            next_active_investigator_after(&state, InvestigatorId(4)),
            None,
            "advance past the last entry returns None"
        );
    }

    #[test]
    fn next_active_investigator_after_returns_none_when_current_not_in_turn_order() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        assert_eq!(
            next_active_investigator_after(&state, InvestigatorId(99)),
            None
        );
    }

    #[test]
    fn next_active_investigator_after_works_when_current_is_non_active() {
        // Defeated-mid-loop semantics: `current` may be Killed by the
        // time we advance from them. The cursor still finds the right
        // successor.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().status = Status::Killed;

        assert_eq!(
            next_active_investigator_after(&state, InvestigatorId(1)),
            Some(InvestigatorId(2)),
            "current=1 is non-Active but turn_order still anchors the index"
        );
    }
```

- [ ] **Step 5: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features`
Expected: PASS. All existing Mythos cursor tests stay green (regression guard); new helper tests pass.

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: extract first_active_investigator / next_active_investigator_after

Pure refactor — extracts the shared eliminated-investigator-skip cursor
lookups from mythos_phase (seed) and advance_mythos_draw_pending
(advance). The two new helpers will be reused by #71's enemy_phase
seed and per-investigator-window continuation. No behavior change;
existing Mythos cursor tests stay green.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `GameState::enemy_attack_pending` field

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs`

The cursor field for Enemy phase 3.3, mirror of `mythos_draw_pending`. Lands ahead of its consumer so subsequent tasks can read/write it.

- [ ] **Step 1: Add the field on `GameState`**

Find the `pub mythos_draw_pending: Option<InvestigatorId>` field and add `enemy_attack_pending` directly below it (with the doc-comment block):

```rust
    /// The next investigator due to resolve engaged-enemy attacks
    /// during Enemy phase step 3.3. Mirror of [`mythos_draw_pending`]:
    ///
    /// - Set to the first [`Status::Active`] investigator in
    ///   [`turn_order`] when [`enemy_phase`] runs step 3.3's loop
    ///   kickoff.
    /// - Advanced by [`run_window_continuation`] after each
    ///   per-investigator attack resolution closes, to the next Active
    ///   investigator in [`turn_order`] (or `None` when the loop is
    ///   done).
    /// - Stays `None` during all phases other than Enemy.
    ///
    /// Eliminated investigators ([`Status::Killed`] / [`Status::Insane`]
    /// / [`Status::Resigned`]) are skipped during advance, mirroring
    /// the `mythos_draw_pending` semantics established in #69.
    ///
    /// [`mythos_draw_pending`]: GameState::mythos_draw_pending
    /// [`turn_order`]: GameState::turn_order
    /// [`enemy_phase`]: crate::engine::dispatch::enemy_phase
    /// [`run_window_continuation`]: crate::engine::dispatch::run_window_continuation
    pub enemy_attack_pending: Option<InvestigatorId>,
```

- [ ] **Step 2: Default-initialize the field**

Find the `Default` impl (or the constructor that builds the initial `GameState`) — `mythos_draw_pending: None,` already lives there. Add `enemy_attack_pending: None,` adjacent.

(If `GameState` uses `#[derive(Default)]` and `Option<InvestigatorId>` derives `Default = None` automatically, no change is needed here. Verify by grepping for `mythos_draw_pending:` to see whether it appears in any explicit constructor — if so, mirror that.)

- [ ] **Step 3: Add a serde-roundtrip test**

Add to the same module pattern as `next_enemy_id_round_trips_through_serde` (line 711 in `game_state.rs`). Place it in or adjacent to that module:

```rust
    #[test]
    fn enemy_attack_pending_round_trips_through_serde() {
        let mut state = TestGame::new().build();
        state.enemy_attack_pending = Some(InvestigatorId(7));
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.enemy_attack_pending, Some(InvestigatorId(7)));
    }

    #[test]
    fn enemy_attack_pending_defaults_to_none() {
        let state = TestGame::new().build();
        assert_eq!(state.enemy_attack_pending, None);
    }
```

(If `InvestigatorId` isn't in scope in the module, add `use crate::state::InvestigatorId;` to the test module's `use` block.)

- [ ] **Step 4: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features`
Expected: PASS. Existing tests untouched; new tests pass.

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs
git commit -m "$(cat <<'EOF'
engine: add GameState::enemy_attack_pending cursor field

Mirror of mythos_draw_pending: tracks the next Active investigator due
to resolve engaged-enemy attacks during Enemy phase step 3.3. Used by
#71's enemy_phase driver to drive the per-investigator window loop;
None elsewhere. Serde roundtrip + default-None tests included.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `WindowKind` variants + `trigger_matches` arm + serde tests

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (variants + serde tests).
- Modify: `crates/game-core/src/engine/dispatch.rs:1734` (`trigger_matches` no-pattern arm).
- Modify: `crates/game-core/src/engine/dispatch.rs:3712` (`run_window_continuation` placeholder arms; real bodies land in Task 5).

`WindowKind` is `#[non_exhaustive]`, so downstream consumers (in `cards`, `scenarios`, `server`, `web`) won't break from new variants. Inside `game-core` itself, the two `match` sites that exhaustively destructure `WindowKind` (`trigger_matches` and `run_window_continuation`) must gain arms or the workspace fails to compile.

- [ ] **Step 1: Add the two variants on `WindowKind`**

Find the `WindowKind` enum (around line 458 in `game_state.rs`). Add the two new variants at the bottom of the enum (preserving existing variant ordering):

```rust
    /// The player window opened before an investigator's engaged
    /// enemies resolve their attacks (Rules Reference p.25 step 3.3,
    /// the "previous player window" investigators "return to" between
    /// resolutions). The investigator to be attacked next is carried
    /// on [`GameState::enemy_attack_pending`], not in the variant —
    /// mirror of [`MythosAfterDraws`] + [`GameState::mythos_draw_pending`].
    ///
    /// Continuation (in [`crate::engine::dispatch::run_window_continuation`]):
    /// read the cursor, resolve the pending investigator's engaged
    /// ready enemies in [`EnemyId`] order, exhaust each, advance the
    /// cursor to the next Active investigator in [`turn_order`] (or
    /// `None`), open the next window
    /// (`BeforeInvestigatorAttacked` if Some,
    /// `AfterAllInvestigatorsAttacked` if None).
    ///
    /// One window per Active investigator in `turn_order`.
    ///
    /// [`turn_order`]: GameState::turn_order
    BeforeInvestigatorAttacked,
    /// The player window after all investigators have resolved their
    /// engaged enemies' attacks (Rules Reference p.25 step 3.3, the
    /// "next player window" entered after the final investigator).
    /// Continuation runs
    /// [`crate::engine::dispatch::enemy_phase_end`] (step 3.4 +
    /// transition). Mirror of [`MythosAfterDraws`]'s end-of-step shape.
    AfterAllInvestigatorsAttacked,
```

- [ ] **Step 2: Add serde-roundtrip tests for both variants**

Add to the `mod open_window_tests` block in `game_state.rs` (alongside `between_phases_window_kind_serde_roundtrip` etc.):

```rust
    #[test]
    fn before_investigator_attacked_window_kind_serde_roundtrip() {
        let kind = WindowKind::BeforeInvestigatorAttacked;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn after_all_investigators_attacked_window_kind_serde_roundtrip() {
        let kind = WindowKind::AfterAllInvestigatorsAttacked;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }
```

- [ ] **Step 3: Extend `trigger_matches`' no-pattern arm**

Open `crates/game-core/src/engine/dispatch.rs` and find `fn trigger_matches` at line 1734. The match's tail arm currently lists `MythosAfterDraws | UpkeepBegins` among the kinds that don't pair with any `EventPattern`. Add the two new variants to that same arm:

Old:
```rust
        (
            WindowKind::BetweenPhases { .. }
            | WindowKind::AfterEnemyDefeated { .. }
            | WindowKind::MythosAfterDraws
            | WindowKind::UpkeepBegins,
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned,
        ) => false,
```

New:
```rust
        (
            WindowKind::BetweenPhases { .. }
            | WindowKind::AfterEnemyDefeated { .. }
            | WindowKind::MythosAfterDraws
            | WindowKind::UpkeepBegins
            | WindowKind::BeforeInvestigatorAttacked
            | WindowKind::AfterAllInvestigatorsAttacked,
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned,
        ) => false,
```

Rationale: like `MythosAfterDraws` / `UpkeepBegins`, the new windows are timing-marker windows for printed Fast-play points — no `EventPattern` currently matches them. (If `trigger_matches`' surrounding doc-comment at line 1718–1772 explicitly enumerates which kinds gate Fast actions, append `BeforeInvestigatorAttacked` and `AfterAllInvestigatorsAttacked` to the list in the same comment update.)

- [ ] **Step 4: Add placeholder arms to `run_window_continuation`**

Find `fn run_window_continuation` at line 3712 in `dispatch.rs`. The current tail arm catches `AfterEnemyDefeated` and `BetweenPhases` as no-op kinds. Real bodies for the two new arms land in Task 5; for now we add explicit no-op arms so the match stays exhaustive and the build is clean:

Old:
```rust
        WindowKind::AfterEnemyDefeated { .. } | WindowKind::BetweenPhases { .. } => {}
    }
}
```

New:
```rust
        WindowKind::AfterEnemyDefeated { .. } | WindowKind::BetweenPhases { .. } => {}
        // Real bodies land in Task 5; until then these arms are
        // unreachable in practice (nothing in the engine opens these
        // window kinds yet).
        WindowKind::BeforeInvestigatorAttacked
        | WindowKind::AfterAllInvestigatorsAttacked => {
            unreachable!(
                "run_window_continuation: enemy-phase window kinds are \
                 not yet opened by any engine path (T5 of #71 wires \
                 enemy_phase + real continuation bodies). If you hit \
                 this, a task ordering invariant was broken."
            )
        }
    }
}
```

- [ ] **Step 5: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features`
Expected: PASS. Workspace compiles (all `match WindowKind` sites are now exhaustive); existing tests untouched; new serde tests pass.

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add WindowKind variants for Enemy phase 3.3 windows

Adds WindowKind::BeforeInvestigatorAttacked (per-investigator window
opened before each Active investigator's engaged enemies attack) and
WindowKind::AfterAllInvestigatorsAttacked (final window before 3.4).
Bare variants; cursor lives on GameState::enemy_attack_pending.

trigger_matches treats both as timing-marker kinds (no EventPattern
matches), mirror of MythosAfterDraws / UpkeepBegins. Serde roundtrip
tests for both variants.

run_window_continuation gains unreachable!() placeholder arms; real
bodies land alongside the enemy_phase driver in the next commit.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `resolve_attacks_for_investigator` + `hunter_movement_step` + unit tests

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

The per-investigator attack loop (the work step 3.3 actually does) and the 3.2 hunter-movement TODO stub for #128. Lands with direct unit tests as the callers — no `enemy_phase` driver yet, so the helpers are exercised by tests until Task 5 wires them in.

- [ ] **Step 1: Write the failing unit tests**

Add a new `#[cfg(test)] mod enemy_phase_tests` block at the bottom of `dispatch.rs` (after the upkeep test module). Add these four tests:

```rust
#[cfg(test)]
mod enemy_phase_tests {
    use super::*;
    use crate::state::{CardCode, EnemyId, InvestigatorId, Phase, Status};
    use crate::test_support::{test_enemy, test_investigator, TestGame};
    use crate::Event;

    #[test]
    fn resolve_attacks_for_investigator_fires_engaged_ready_enemy_and_exhausts() {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        enemy.attack_horror = 0;
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        resolve_attacks_for_investigator(&mut state, &mut events, inv_id);

        // Damage placed.
        assert!(events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        )), "expected DamageTaken { amount: 1 }; events = {events:?}");

        // Enemy exhausted in state and event.
        assert!(state.enemies[&enemy_id].exhausted, "enemy must be exhausted");
        assert!(events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )), "expected EnemyExhausted; events = {events:?}");

        // Ordering: DamageTaken precedes EnemyExhausted (post-attack exhaust).
        let damage_pos = events.iter().position(|e| matches!(e, Event::DamageTaken { .. })).unwrap();
        let exhaust_pos = events.iter().position(|e| matches!(e, Event::EnemyExhausted { .. })).unwrap();
        assert!(damage_pos < exhaust_pos, "DamageTaken must precede EnemyExhausted; events = {events:?}");
    }

    #[test]
    fn resolve_attacks_for_investigator_excludes_exhausted_and_unengaged_enemies() {
        let inv_id = InvestigatorId(1);

        // Engaged but exhausted — must NOT attack.
        let mut e1 = test_enemy(1, "Exhausted Engaged");
        e1.engaged_with = Some(inv_id);
        e1.exhausted = true;
        e1.attack_damage = 5;

        // Ready but unengaged — must NOT attack.
        let mut e2 = test_enemy(2, "Ready Unengaged");
        e2.engaged_with = None;
        e2.attack_damage = 5;

        // Ready engaged — the only one that attacks.
        let mut e3 = test_enemy(3, "Ready Engaged");
        e3.engaged_with = Some(inv_id);
        e3.attack_damage = 1;

        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(e1)
            .with_enemy(e2)
            .with_enemy(e3)
            .build();
        let mut events = Vec::new();

        resolve_attacks_for_investigator(&mut state, &mut events, inv_id);

        // Exactly one DamageTaken (from e3, amount 1).
        let damages: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::DamageTaken { .. }))
            .collect();
        assert_eq!(damages.len(), 1, "exactly one attacker should fire; events = {events:?}");
        assert!(matches!(damages[0], Event::DamageTaken { amount: 1, .. }));

        // Only e3 exhausted; e1 already was; e2 must remain ready.
        assert!(state.enemies[&EnemyId(1)].exhausted, "e1 was already exhausted; still is");
        assert!(!state.enemies[&EnemyId(2)].exhausted, "e2 must NOT exhaust (didn't attack)");
        assert!(state.enemies[&EnemyId(3)].exhausted, "e3 attacked and exhausted");

        // Exactly one EnemyExhausted event (e3). e1's prior-state exhausted doesn't re-emit.
        let exhausted_events: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::EnemyExhausted { .. }))
            .collect();
        assert_eq!(exhausted_events.len(), 1);
        assert!(matches!(
            exhausted_events[0],
            Event::EnemyExhausted { enemy: EnemyId(3) }
        ));
    }

    #[test]
    fn resolve_attacks_for_investigator_iterates_attackers_in_enemy_id_order() {
        let inv_id = InvestigatorId(1);

        let mut e_lower = test_enemy(2, "Lower id"); // EnemyId(2)
        e_lower.engaged_with = Some(inv_id);
        e_lower.attack_damage = 1;

        let mut e_higher = test_enemy(10, "Higher id"); // EnemyId(10)
        e_higher.engaged_with = Some(inv_id);
        e_higher.attack_damage = 2;

        let mut state = TestGame::default()
            .with_investigator({
                let mut inv = test_investigator(1);
                inv.max_health = 100; // survive both attacks
                inv
            })
            .with_enemy(e_higher) // insert in NON-id order to confirm BTreeMap ordering wins
            .with_enemy(e_lower)
            .build();
        let mut events = Vec::new();

        resolve_attacks_for_investigator(&mut state, &mut events, inv_id);

        // The two DamageTaken events must appear in EnemyId(2) → EnemyId(10) order
        // (verifiable via their amounts: 1 then 2).
        let damages: Vec<u8> = events
            .iter()
            .filter_map(|e| match e {
                Event::DamageTaken { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect();
        assert_eq!(damages, vec![1, 2], "EnemyId order: 2 (dmg 1) before 10 (dmg 2)");
    }

    #[test]
    fn resolve_attacks_for_investigator_early_breaks_when_target_defeated_mid_loop() {
        let inv_id = InvestigatorId(1);

        // EnemyId(1) deals the killing blow on its attack.
        let mut e1 = test_enemy(1, "Killer");
        e1.engaged_with = Some(inv_id);
        e1.attack_damage = 1;

        // EnemyId(2) must NOT attack (active check fails at loop top).
        let mut e2 = test_enemy(2, "Bystander");
        e2.engaged_with = Some(inv_id);
        e2.attack_damage = 5;

        let mut state = TestGame::default()
            .with_investigator({
                let mut inv = test_investigator(1);
                inv.max_health = 1; // e1's attack defeats
                inv
            })
            .with_enemy(e1)
            .with_enemy(e2)
            .build();
        let mut events = Vec::new();

        resolve_attacks_for_investigator(&mut state, &mut events, inv_id);

        // e1 attacked + exhausted.
        assert!(state.enemies[&EnemyId(1)].exhausted, "e1 attacked, must exhaust");
        // e2 did NOT attack and did NOT exhaust.
        assert!(!state.enemies[&EnemyId(2)].exhausted, "e2 must not exhaust (early-break)");

        let damages: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::DamageTaken { .. }))
            .collect();
        assert_eq!(damages.len(), 1, "only e1's attack lands; events = {events:?}");

        let exhausted_events: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::EnemyExhausted { .. }))
            .collect();
        assert_eq!(exhausted_events.len(), 1);
        assert!(matches!(
            exhausted_events[0],
            Event::EnemyExhausted { enemy: EnemyId(1) }
        ));

        // Investigator was defeated.
        assert_eq!(state.investigators[&inv_id].status, Status::Killed);
    }
}
```

- [ ] **Step 2: Run to verify tests fail with "not in scope"**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features enemy_phase_tests`
Expected: FAIL (compile error: `resolve_attacks_for_investigator` not found in scope).

- [ ] **Step 3: Implement `hunter_movement_step` and `resolve_attacks_for_investigator`**

Add to `dispatch.rs`, adjacent to the other Enemy-phase machinery sites (just after `fire_attacks_of_opportunity` at line 2822 is a natural home — both helpers iterate engaged ready enemies):

```rust
/// 3.2 Hunter enemies move. Rules Reference p.25: "Resolve the hunter
/// keyword for each ready, unengaged enemy that has the hunter
/// keyword."
fn hunter_movement_step(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#128): iterate ready unengaged enemies with the Hunter
    //             keyword; BFS over the location-connection graph;
    //             move + engage-on-arrival. Ambiguous shortest paths
    //             prompt the active investigator via AwaitingInput +
    //             InputResponse::PickLocation. Currently no Hunter
    //             keyword exists on CardMetadata; #128 lands it
    //             alongside this body.
}

/// Resolve all of one investigator's engaged ready enemies' attacks
/// (Rules Reference p.25 step 3.3 inner body). Snapshot the attacker
/// list in [`EnemyId`] order (BTreeMap iteration is sorted), then
/// for each attacker:
///
/// 1. Early-break if `investigator` is no longer [`Status::Active`]
///    (defeated by an earlier attack in the same loop). Remaining
///    attackers do not attack and do not exhaust, per Rules
///    Reference p.10 Elimination step 3 ("All enemies engaged with
///    that player are placed at the location ... unengaged but
///    otherwise maintaining their current game state") and p.25
///    ("Each ready, engaged enemy makes an attack" — a disengaged
///    enemy is not "engaged").
///
///    Today [`apply_investigator_defeat`] only flips `Status`; the
///    full disengage + re-engage flow lands in #144 (Phase-4
///    milestone, blocked on #128 which lands the prey logic needed
///    for multi-investigator re-engagement). The early-break here
///    is the rules-correct minimal interpretation: no incorrect
///    events fire, no behavior anomaly visible at the rules level.
///    After #144 lands, the `enemy.engaged_with` field is properly
///    cleared on defeat too; this early-break stays as the simpler
///    form (one redundant check, harmless).
///
/// 2. Call [`enemy_attack`] (places damage + horror simultaneously
///    per p.7, fires [`apply_investigator_defeat`] if either
///    crosses).
///
/// 3. Set `enemy.exhausted = true`, emit
///    [`Event::EnemyExhausted { enemy }`]. Per Rules Reference p.25,
///    exhaustion happens "Upon completion of dealing the attack (and
///    all abilities triggered by the attack)" — no carve-out for
///    "the attack defeated the target," so an attack that lands and
///    defeats its target still exhausts the attacker.
///
/// **Atomicity invariant:** the snapshot + loop run as a block
/// within [`run_window_continuation`]'s `BeforeInvestigatorAttacked`
/// arm — no Fast plays or reactions interpose mid-loop. The first
/// PR that adds a reaction `EventPattern` matching events emitted
/// inside this loop ([`DamageTaken`] / [`HorrorTaken`] /
/// [`EnemyExhausted`] / [`EnemyDefeated`]-from-attack) must persist
/// the remaining-attackers list on `GameState` (analogous to
/// [`GameState::enemy_attack_pending`]) so resume-after-pause
/// re-enters the right iteration point.
///
/// **Attack order:** deterministic by [`EnemyId`]. Rules Reference
/// p.25 prescribes "the order of the attacked investigator's
/// choosing" when an investigator is engaged with multiple enemies;
/// #143 (unmilestoned) covers both this site and
/// [`fire_attacks_of_opportunity`] (which has the same TODO).
///
/// [`Event::EnemyExhausted`]: crate::Event::EnemyExhausted
/// [`DamageTaken`]: crate::Event::DamageTaken
/// [`HorrorTaken`]: crate::Event::HorrorTaken
/// [`EnemyExhausted`]: crate::Event::EnemyExhausted
/// [`EnemyDefeated`]: crate::Event::EnemyDefeated
fn resolve_attacks_for_investigator(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    // Snapshot ready engaged attackers in deterministic EnemyId order.
    // BTreeMap iteration is already key-sorted.
    let attackers: Vec<EnemyId> = state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator) && !e.exhausted)
        .map(|(id, _)| *id)
        .collect();

    for enemy_id in attackers {
        // Early-break on defeat. See fn doc.
        let active = state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active {
            break;
        }

        // Damage + horror placement (simultaneous per p.7) + defeat.
        enemy_attack(state, events, enemy_id, investigator);

        // Exhaust the attacker post-resolution.
        let enemy = state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "resolve_attacks_for_investigator: snapshotted enemy \
                 {enemy_id:?} is gone from state.enemies; this is a \
                 state-corruption invariant violation"
            )
        });
        enemy.exhausted = true;
        events.push(Event::EnemyExhausted { enemy: enemy_id });
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features enemy_phase_tests`
Expected: PASS for all four new tests.

- [ ] **Step 5: Address the `hunter_movement_step` dead-code warning**

`hunter_movement_step` has no caller in this task (it lands in Task 5). Verify whether `RUSTFLAGS="-D warnings" cargo build -p game-core --all-features` warns about it. If yes, either:

(a) Move `hunter_movement_step` into Task 5 (where `enemy_phase` calls it), OR
(b) Add `#[allow(dead_code)]` on the stub with a comment pointing at Task 5.

Recommended: (a). Adjust this task to add `resolve_attacks_for_investigator` only; move the `hunter_movement_step` snippet (3 lines of body + 9 lines of doc) into Task 5's Step 1.

If (a), remove the `hunter_movement_step` block from this task's Step 3 and adjust the file footprint accordingly; if (b), add:

```rust
#[allow(dead_code)] // TODO(#71 T5): consumed by enemy_phase in the next commit
fn hunter_movement_step(...) { ... }
```

(Decision belongs to the engineer at execution time based on what the local `cargo build` shows.)

- [ ] **Step 6: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features`
Expected: PASS.

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add resolve_attacks_for_investigator (Enemy phase 3.3 loop)

The per-investigator attack resolution loop: snapshot engaged ready
enemies in EnemyId order, fire enemy_attack + exhaust each, with an
early-break on the investigator becoming non-Active mid-loop (defeat
during their own attack resolution). Tests cover basic fire+exhaust,
exhausted/unengaged exclusion, EnemyId iteration order, and the
early-break-on-defeat path.

hunter_movement_step lands as a named #128 TODO stub (no body) for
3.2 — same shape as place_doom_on_agenda / check_doom_threshold /
check_hand_size.

The enemy_phase driver wires both into the phase shape in the next
commit.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `enemy_phase` + `enemy_phase_end` + wired continuation arms + `step_phase` wiring + driver tests

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

The big one. Lands `enemy_phase` (3.1 + 3.2 stub + cursor seed + first window), `enemy_phase_end` (3.4 + transition), fills in the `run_window_continuation` placeholder arms with real bodies (calling `resolve_attacks_for_investigator` + advancing the cursor + opening the next window), and wires `step_phase` (PhaseEnded suppression, dispatch arm, `unreachable!` fallback). Tests assert the full driver cascade.

- [ ] **Step 1: Write the failing driver-shape tests**

Add to the `mod enemy_phase_tests` block in `dispatch.rs` (alongside the existing Task-4 tests):

```rust
    #[test]
    fn enemy_phase_emits_phase_started_and_cascades_to_mythos_in_no_eligibility_case() {
        // 1 Active investigator, no engaged enemies. Auto-skip
        // cascades through both windows + enemy_phase_end +
        // Upkeep → Mythos.
        let inv_id = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![inv_id];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // Positional ordering of the major events.
        let pos = |pred: &dyn Fn(&Event) -> bool| events.iter().position(pred);
        let started = pos(&|e| matches!(e, Event::PhaseStarted { phase: Phase::Enemy })).expect("PhaseStarted(Enemy)");
        let w1_open = pos(&|e| matches!(e, Event::WindowOpened { kind: WindowKind::BeforeInvestigatorAttacked })).expect("WindowOpened(Before)");
        let w1_close = pos(&|e| matches!(e, Event::WindowClosed { kind: WindowKind::BeforeInvestigatorAttacked })).expect("WindowClosed(Before)");
        let w2_open = pos(&|e| matches!(e, Event::WindowOpened { kind: WindowKind::AfterAllInvestigatorsAttacked })).expect("WindowOpened(After)");
        let w2_close = pos(&|e| matches!(e, Event::WindowClosed { kind: WindowKind::AfterAllInvestigatorsAttacked })).expect("WindowClosed(After)");
        let ended = pos(&|e| matches!(e, Event::PhaseEnded { phase: Phase::Enemy })).expect("PhaseEnded(Enemy)");
        let upkeep_started = pos(&|e| matches!(e, Event::PhaseStarted { phase: Phase::Upkeep })).expect("PhaseStarted(Upkeep)");

        assert!(
            started < w1_open && w1_open < w1_close && w1_close < w2_open && w2_open < w2_close && w2_close < ended && ended < upkeep_started,
            "ordered: 3.1 → BeforeInv window → AfterAll window → 3.4 → Upkeep 4.1; events = {events:?}"
        );
        assert_eq!(state.phase, Phase::Mythos, "cascade lands in Mythos");
        assert_eq!(state.enemy_attack_pending, None, "cursor cleared at end");
    }

    #[test]
    fn enemy_phase_with_two_investigators_iterates_in_turn_order() {
        let id1 = InvestigatorId(1);
        let id2 = InvestigatorId(2);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1, id2];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // Two BeforeInvestigatorAttacked windows + one AfterAll.
        let before_opens: Vec<usize> = events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| matches!(e, Event::WindowOpened { kind: WindowKind::BeforeInvestigatorAttacked }).then_some(i))
            .collect();
        let after_opens: Vec<usize> = events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| matches!(e, Event::WindowOpened { kind: WindowKind::AfterAllInvestigatorsAttacked }).then_some(i))
            .collect();
        assert_eq!(before_opens.len(), 2, "one window per Active investigator");
        assert_eq!(after_opens.len(), 1);
        assert!(before_opens[0] < before_opens[1] && before_opens[1] < after_opens[0]);
    }

    #[test]
    fn enemy_phase_skips_eliminated_investigator_in_advance() {
        let id1 = InvestigatorId(1);
        let id2 = InvestigatorId(2);
        let id3 = InvestigatorId(3);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1, id2, id3];
        state.active_investigator = None;
        state.investigators.get_mut(&id2).unwrap().status = Status::Insane;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // Only 2 BeforeInvestigatorAttacked windows (id1 + id3).
        let before_count = events
            .iter()
            .filter(|e| matches!(e, Event::WindowOpened { kind: WindowKind::BeforeInvestigatorAttacked }))
            .count();
        assert_eq!(before_count, 2, "Insane id2 must be skipped");
    }

    #[test]
    fn enemy_phase_with_all_eliminated_opens_after_all_directly() {
        let id1 = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1];
        state.active_investigator = None;
        state.investigators.get_mut(&id1).unwrap().status = Status::Killed;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // No BeforeInvestigatorAttacked windows — straight to AfterAll.
        assert!(
            events.iter().all(|e| !matches!(e, Event::WindowOpened { kind: WindowKind::BeforeInvestigatorAttacked })),
            "no per-investigator window when all are eliminated; events = {events:?}"
        );
        assert!(events.iter().any(|e| matches!(e, Event::WindowOpened { kind: WindowKind::AfterAllInvestigatorsAttacked })));
        assert_eq!(state.phase, Phase::Mythos, "cascade still lands in Mythos");
    }

    #[test]
    fn enemy_phase_attack_lands_in_full_cascade() {
        // 1 investigator engaged with 1 ready enemy. Full Investigation→Enemy→Upkeep→Mythos
        // cascade; attack lands inside the BeforeInvestigatorAttacked continuation.
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![inv_id];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // The attack landed.
        assert!(events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )));
        assert!(state.enemies[&enemy_id].exhausted);

        // Cascade landed in Mythos.
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn step_phase_from_enemy_does_not_emit_phase_ended_enemy() {
        // Direct unit-level check: step_phase's PhaseEnded fallback must
        // suppress for Phase::Enemy (enemy_phase_end owns that emit).
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Enemy)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.active_investigator = None;
        // Use a state where Upkeep's cascade can complete (Active investigator exists).
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Enemy → Upkeep

        // step_phase itself MUST NOT emit PhaseEnded(Enemy); only
        // enemy_phase_end is allowed to (which doesn't run here — we
        // started in Enemy and stepped out, simulating the "phase
        // transition without driver-owned end emit" path).
        let phase_ended_enemy_count = events
            .iter()
            .filter(|e| matches!(e, Event::PhaseEnded { phase: Phase::Enemy }))
            .count();
        assert_eq!(
            phase_ended_enemy_count, 0,
            "step_phase must NOT emit PhaseEnded(Enemy); only enemy_phase_end may. events = {events:?}"
        );
    }
```

- [ ] **Step 2: Run to verify the new tests fail**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features enemy_phase_tests`
Expected: FAIL (compile errors: `enemy_phase` not in scope, `step_phase` doesn't yet dispatch to Enemy driver).

- [ ] **Step 3: Add `hunter_movement_step` if Task 4 deferred it**

If Task 4 deferred `hunter_movement_step` to here (recommended path): add it now, adjacent to `resolve_attacks_for_investigator`:

```rust
/// 3.2 Hunter enemies move. Rules Reference p.25: "Resolve the hunter
/// keyword for each ready, unengaged enemy that has the hunter
/// keyword."
fn hunter_movement_step(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#128): iterate ready unengaged enemies with the Hunter
    //             keyword; BFS over the location-connection graph;
    //             move + engage-on-arrival. Ambiguous shortest paths
    //             prompt the active investigator via AwaitingInput +
    //             InputResponse::PickLocation. Currently no Hunter
    //             keyword exists on CardMetadata; #128 lands it
    //             alongside this body.
}
```

- [ ] **Step 4: Add `enemy_phase` and `enemy_phase_end`**

Add immediately after `hunter_movement_step` / `resolve_attacks_for_investigator`:

```rust
/// Entered by [`step_phase`] on the Investigation→Enemy transition.
/// Owns the `PhaseStarted(Enemy)` emit (Rules Reference p.25 step 3.1)
/// and kicks off the per-investigator attack loop (step 3.3) by
/// seeding [`GameState::enemy_attack_pending`] and opening the first
/// [`WindowKind::BeforeInvestigatorAttacked`] window. The loop body
/// runs in [`run_window_continuation`]'s arms; this driver returns
/// after the kickoff.
///
/// Hunter movement (step 3.2) is a named TODO stub
/// ([`hunter_movement_step`]) deferred to #128.
fn enemy_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 3.1 Enemy phase begins.
    events.push(Event::PhaseStarted {
        phase: Phase::Enemy,
    });

    // 3.2 Hunter enemies move. TODO(#128).
    hunter_movement_step(state, events);

    // 3.3 Kick off the per-investigator attack loop. Seed the cursor
    //     to the first Active investigator in turn_order. Eliminated
    //     investigators (Killed / Insane / Resigned) are skipped per
    //     Rules Reference p.10 (Elimination); first_active_investigator
    //     is the shared helper used by Mythos 1.4 (#69) for the same
    //     semantics.
    state.enemy_attack_pending = first_active_investigator(state);

    if state.enemy_attack_pending.is_some() {
        open_fast_window(state, events, WindowKind::BeforeInvestigatorAttacked);
    } else {
        // No Active investigators (turn_order empty or all eliminated).
        // Skip straight to the final window — mirror of mythos_phase's
        // no-drawer path.
        open_fast_window(state, events, WindowKind::AfterAllInvestigatorsAttacked);
    }
}

/// Called from [`run_window_continuation`]'s
/// [`WindowKind::AfterAllInvestigatorsAttacked`] arm. Emits step
/// 3.4's `PhaseEnded(Enemy)` marker, then transitions to Upkeep.
/// Exact analog of [`mythos_phase_end`] / [`upkeep_phase_end`].
fn enemy_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 3.4 Enemy phase ends.
    events.push(Event::PhaseEnded {
        phase: Phase::Enemy,
    });
    step_phase(state, events); // Enemy → Upkeep; calls upkeep_phase
}
```

- [ ] **Step 5: Replace the `run_window_continuation` placeholder arms with real bodies**

Find the placeholder arms in `run_window_continuation` (added in Task 3) and replace them:

Old (Task 3 placeholders):
```rust
        WindowKind::BeforeInvestigatorAttacked
        | WindowKind::AfterAllInvestigatorsAttacked => {
            unreachable!(
                "run_window_continuation: enemy-phase window kinds are \
                 not yet opened by any engine path (T5 of #71 wires \
                 enemy_phase + real continuation bodies). If you hit \
                 this, a task ordering invariant was broken."
            )
        }
```

New (real bodies):
```rust
        WindowKind::BeforeInvestigatorAttacked => {
            // Phase-transitioning continuation (advances to the next
            // window and ultimately to Upkeep) — cannot run while a
            // skill test is in flight (would strand it). Phase 4 has
            // no Enemy-phase skill-test source, so this branch is
            // structurally unreachable today. A future PR adding one
            // (e.g. a treachery-style "make an Agility test or take
            // damage" attack ability) must redesign the window-close
            // + phase-transition ordering before this assertion fires.
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "BeforeInvestigatorAttacked window closed while a \
                     skill test is in flight (continuation={:?}). Phase \
                     transition would strand the skill test in the \
                     wrong phase. Phase 4 has no Enemy-phase skill test \
                     sources; if a future PR adds one, the window-close \
                     + phase-transition ordering needs redesign before \
                     this assertion can be relaxed.",
                    in_flight.continuation,
                );
            }

            // Cursor expect-Some: BeforeInvestigatorAttacked is only
            // ever opened after enemy_attack_pending is set to Some(_)
            // in enemy_phase or in the advance below. A None cursor
            // here is a state-corruption invariant violation, not a
            // normal rejection path.
            let investigator = state.enemy_attack_pending.unwrap_or_else(|| {
                unreachable!(
                    "BeforeInvestigatorAttacked closed with \
                     enemy_attack_pending == None; this is a \
                     state-corruption invariant violation"
                )
            });

            resolve_attacks_for_investigator(state, events, investigator);

            // Advance the cursor: next Active investigator AFTER
            // `investigator` in turn_order. The shared helper uses
            // turn_order (not the filtered-Active list) as the index
            // basis, so `investigator` itself can have been defeated
            // mid-loop and we still find the right successor.
            state.enemy_attack_pending =
                next_active_investigator_after(state, investigator);

            if state.enemy_attack_pending.is_some() {
                open_fast_window(state, events, WindowKind::BeforeInvestigatorAttacked);
            } else {
                open_fast_window(state, events, WindowKind::AfterAllInvestigatorsAttacked);
            }
        }
        WindowKind::AfterAllInvestigatorsAttacked => {
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "AfterAllInvestigatorsAttacked window closed while a \
                     skill test is in flight (continuation={:?}). Phase \
                     4 has no Enemy-phase skill-test sources; a future \
                     PR adding one needs the window-close + \
                     phase-transition ordering redesigned before this \
                     fires.",
                    in_flight.continuation,
                );
            }
            enemy_phase_end(state, events);
        }
```

- [ ] **Step 6: Wire `step_phase`**

Find `step_phase` at line 937 in `dispatch.rs`. Three changes:

(a) Extend the `PhaseEnded` suppression set to cover Enemy. Old:
```rust
    if from != Phase::Mythos && from != Phase::Upkeep {
        events.push(Event::PhaseEnded { phase: from });
    }
```

New:
```rust
    if from != Phase::Mythos && from != Phase::Upkeep && from != Phase::Enemy {
        events.push(Event::PhaseEnded { phase: from });
    }
```

(b) Add the dispatch arm for `Phase::Enemy`. Old:
```rust
    match to {
        Phase::Mythos if from != Phase::Mythos => mythos_phase(state, events),
        Phase::Investigation if from != Phase::Investigation => investigation_phase(state, events),
        Phase::Upkeep if from != Phase::Upkeep => upkeep_phase(state, events),
        _ => events.push(Event::PhaseStarted { phase: to }),
    }
```

New:
```rust
    match to {
        Phase::Mythos if from != Phase::Mythos => mythos_phase(state, events),
        Phase::Investigation if from != Phase::Investigation => investigation_phase(state, events),
        Phase::Enemy if from != Phase::Enemy => enemy_phase(state, events),
        Phase::Upkeep if from != Phase::Upkeep => upkeep_phase(state, events),
        _ => unreachable!(
            "step_phase: from == to (from={from:?}, to={to:?}); Phase::next \
             never returns the same phase, so this branch is structurally \
             unreachable. If it ever fires, something has corrupted \
             state.phase between the read and the dispatch."
        ),
    }
```

(c) Update the `step_phase` doc-comment block at lines 920–936. The "PhaseEnded(Mythos) suppression invariant" paragraph should grow to mention Enemy alongside Mythos and Upkeep. (Be surgical: the existing prose explains the Mythos-specific reasoning; one sentence appended to that paragraph is sufficient, e.g. *"#71 extends the same suppression to `Phase::Enemy` — `enemy_phase_end` owns the step 3.4 `PhaseEnded(Enemy)` emit."*)

- [ ] **Step 7: Update the `end_turn` comment**

`end_turn` at line 804 says `step_phase(state, events); // Investigation → Enemy (empty until #71)`. Remove the `(empty until #71)` qualifier — Enemy is no longer empty:

Old:
```rust
        step_phase(state, events); // Investigation → Enemy (empty until #71)
```

New:
```rust
        step_phase(state, events); // Investigation → Enemy
```

- [ ] **Step 8: Run the driver-shape tests**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features enemy_phase_tests`
Expected: PASS for all six new driver-cascade tests (including the four from Task 4 and the new six in this task).

- [ ] **Step 9: Run the full game-core test suite for regression**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features`
Expected: PASS. Mythos/Upkeep tests stay green; the new Enemy tests pass.

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: enemy_phase driver + per-investigator attack loop wiring (#71)

Phase-4 Enemy-phase driver. enemy_phase (3.1 PhaseStarted + 3.2 hunter
stub for #128 + 3.3 cursor seed + first BeforeInvestigatorAttacked
window). enemy_phase_end (3.4 PhaseEnded + Enemy→Upkeep). The
BeforeInvestigatorAttacked window's continuation calls
resolve_attacks_for_investigator for the cursor's pending investigator,
advances the cursor via next_active_investigator_after, and opens the
next window (BeforeInvestigatorAttacked or AfterAllInvestigatorsAttacked).
The AfterAll window's continuation calls enemy_phase_end.

step_phase: PhaseEnded suppression now covers Enemy alongside Mythos
and Upkeep; new Phase::Enemy dispatch arm; the _ fallback becomes
unreachable!() since all four Phase::next outputs are matched and from
== to is impossible. end_turn's stale "(empty until #71)" comment
goes too.

Driver-cascade tests cover: 1-investigator full cascade through
Upkeep to Mythos; 2-investigator iteration in turn order; eliminated
middle investigator skip; all-eliminated direct AfterAll path;
attack-lands-in-cascade end-to-end; PhaseEnded(Enemy) suppression in
step_phase itself.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Pause/resume tests (Fast play eligibility at the per-investigator window)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (add to the `enemy_phase_tests` module).

The auto-skip cascade tests in Task 5 only cover the no-Fast-eligible path. Tests in this task confirm the wait shape when a Fast play IS eligible at `BeforeInvestigatorAttacked`, and the resume-via-Skip path.

- [ ] **Step 1: Survey how Mythos's `MythosAfterDraws` pause/resume is tested**

Read `crates/game-core/src/engine/dispatch.rs` around lines 5148–5230 (Mythos's pause/resume tests). Identify which Fast-eligibility shortcut they use — likely a `state.investigators[id].hand`-populated Fast event card, OR direct manipulation of `state.open_windows`. Re-use the same pattern.

(Hand-populating a Fast event requires the card registry to be installed. If Mythos's tests use a lighter-weight shortcut — e.g., they directly push a synthetic `OpenWindow` onto `state.open_windows` to simulate the "stays-on-stack" state, then submit `Skip` to close it — this task mirrors that shortcut. The pause/resume *contract* is what we're testing, not the Fast-eligibility scan itself.)

If Mythos doesn't have a corresponding test (the auto-skip path may be the only one tested in `dispatch.rs`'s Mythos tests), check the integration tests at `crates/scenarios/tests/upkeep_phase.rs` for the analogous shape. If neither exists in a usable form, defer Test 11 (the Fast-eligibility pause test) and ship Test 10 (Skip-resume) only as a leaner shape that exercises the continuation arm via direct window manipulation.

- [ ] **Step 2: Write the Skip-resume test**

```rust
    #[test]
    fn enemy_phase_resumes_via_skip_input() {
        // Construct the state mid-pause: BeforeInvestigatorAttacked
        // window is on the stack, cursor points at inv1. Submitting
        // PlayerAction::ResolveInput(InputResponse::Skip) closes the
        // window via close_reaction_window_at, runs the continuation,
        // attacks resolve, cursor advances to None, AfterAll window
        // opens + auto-skips, enemy_phase_end fires, cascade lands.
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .with_phase(Phase::Enemy)
            .build();
        state.turn_order = vec![inv_id];
        state.active_investigator = None;
        state.enemy_attack_pending = Some(inv_id);
        state.open_windows.push(OpenWindow {
            kind: WindowKind::BeforeInvestigatorAttacked,
            pending_triggers: Vec::new(),
            fast_actors: FastActorScope::Any,
        });
        // Inject a fictitious "Fast-eligible" marker so the window
        // actually stays open. If the test fixture above wouldn't
        // produce a real eligibility hit, use the alternative
        // construction surveyed in Step 1.

        let outcome = apply(
            state.clone(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
        );

        match outcome.outcome {
            EngineOutcome::Done => {
                assert_eq!(outcome.state.phase, Phase::Mythos, "cascade lands in Mythos");
                assert!(
                    outcome.events.iter().any(|e| matches!(e, Event::DamageTaken { amount: 1, .. })),
                    "attack should have landed during the resumed continuation"
                );
                assert!(outcome.state.enemies[&enemy_id].exhausted);
            }
            other => panic!("expected Done after Skip; got {other:?}; events = {:?}", outcome.events),
        }
    }
```

(The exact field paths above — `outcome.state`, `outcome.outcome`, `outcome.events` — must match `ApplyResult`'s shape. Adjust if the fields are named differently. The `OpenWindow` literal needs `use crate::state::{FastActorScope, OpenWindow};` in the test module's imports.)

- [ ] **Step 3: Run the new test**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features enemy_phase_tests::enemy_phase_resumes_via_skip_input`
Expected: PASS.

If the test as written reveals a contract gap (e.g., the auto-skip path doesn't reactivate because the window was synthetically pushed without going through `open_fast_window`), adjust the test to construct the state via a real `step_phase` call that pauses naturally — then `Skip` is the resume. The Mythos test pattern surveyed in Step 1 should resolve any structural questions.

- [ ] **Step 4 (optional): Add the Fast-eligibility pause test**

If a tractable Fast-eligibility setup exists (card registry installed via a test fixture, etc.), add:

```rust
    #[test]
    fn enemy_phase_pauses_when_fast_play_eligible() {
        // ... construct state with a Fast event in inv1's hand + resources to play it ...
        let outcome = apply(state, Action::Player(PlayerAction::EndTurn));
        match outcome.outcome {
            EngineOutcome::AwaitingInput { .. } => {
                assert_eq!(outcome.state.phase, Phase::Enemy);
                assert_eq!(outcome.state.enemy_attack_pending, Some(inv_id));
                assert!(matches!(
                    outcome.state.open_windows.last(),
                    Some(OpenWindow { kind: WindowKind::BeforeInvestigatorAttacked, .. })
                ));
            }
            other => panic!("expected AwaitingInput at Fast-eligible window; got {other:?}"),
        }
    }
```

If the setup is too involved relative to the test's value, defer it — the Skip-resume test plus the auto-skip cascade tests cover the load-bearing contract. Document the deferral inline (e.g., `// TODO: pause-on-Fast-eligibility test — needs a card registry install fixture; deferred`).

- [ ] **Step 5: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features`
Expected: PASS.

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: enemy phase pause/resume test via Skip input

Confirms the BeforeInvestigatorAttacked window's Skip-resume path
runs the continuation correctly: closes the window, fires the
pending investigator's engaged-enemy attacks, advances the cursor,
opens the AfterAllInvestigatorsAttacked window, cascades to
Upkeep → Mythos. [Optionally:] pause-on-Fast-eligibility test
covering the awaiting-input shape with enemy_attack_pending
preserved across apply calls.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Phase-doc update (last commit before merge)

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

Per CLAUDE.md's PR procedure section: the phase doc gets touched **exactly once per PR, as the final commit before merge**, so it reflects the actually-shipping state. Run this task only after the PR has been opened, CI is green, and any review-driven fixes have folded in. **Do not include phase-doc edits in earlier commits.**

- [ ] **Step 1: Verify CI green on the PR**

Run: `gh pr checks <PR#>` and confirm all five jobs pass.

- [ ] **Step 2: Move `#71` from the Open table to the Closed table**

In `docs/phases/phase-4-scenario-plumbing.md`:
- Remove the `#71` row from the "Issues (11 — …)" table (the open issues table at the top).
- Add a new row to the "Closed" table immediately below it, in the format used by `#69` / `#70`:

```markdown
| `#71` | Enemy phase: engagement attacks | #<PR#> | <one- or two-paragraph summary of the load-bearing decisions and shape that ships> |
```

Suggested summary content (refine based on what actually shipped):
> `enemy_phase` + `enemy_phase_end` mirror the Mythos cursor-driven shape: per-investigator `BeforeInvestigatorAttacked` window + final `AfterAllInvestigatorsAttacked` window, with `enemy_attack_pending: Option<InvestigatorId>` as the cross-`apply` cursor (mirror of `mythos_draw_pending`). `resolve_attacks_for_investigator` snapshots engaged ready enemies in `EnemyId` order, fires `enemy_attack` + exhausts each, with an early-break on `Status != Active` (rules-correct minimal interpretation until #144 lands; #144 is in Phase 4, blocked on #128 so multi-investigator re-engagement can use prey logic directly). Two shared cursor helpers (`first_active_investigator`, `next_active_investigator_after`) extracted from Mythos collapse its seed and advance call sites to one-liners. `step_phase`'s `_` fallback becomes `unreachable!()` now that all four phases are driver-dispatched. 3.2 hunter movement is a named `hunter_movement_step` stub for #128.

- [ ] **Step 3: Update the Status section counts**

Find the "Status" section at the top of the doc. Update:
- The list of merged PRs to include `#71 as PR #<PR#>`.
- The "Remaining" list to drop `#71`.
- Any open/closed count references.

- [ ] **Step 4: Update the Ordering / Arc table**

In the "Ordering (Shape B)" table, flip row 8 (`#71 Enemy phase: engagement attacks`) from a planned step to:

```markdown
| 8 | `#71` Enemy phase: engagement attacks | ✅ PR #<PR#>. <one-sentence shape summary, e.g. "Per-investigator + final windows; deterministic EnemyId attack order with #143 filed for both this site and AoO; early-break on defeat with #144 scheduled after #128."> |
```

- [ ] **Step 5: Add a `Decisions made` entry (only the load-bearing ones)**

Per CLAUDE.md guidance ("Decisions entries are not a changelog; they're a context-saver for the next person"), include only entries future PR-authors will need to make their next decision. Recommended set:

```markdown
- **Per-investigator + final windows, bare `WindowKind` variants, cursor on `enemy_attack_pending` (`#71`, PR #<PR#>).** Mirror of `mythos_draw_pending` rather than a payload-carrying `WindowKind`. Reasoning: the consolidation-into-single-variant follow-up's option space is preserved, and the per-investigator window subsumes the rules' "return to the previous player window" reading without inventing a separate inter-step window. Future per-investigator phase loops follow this shape.
- **Shared cursor helpers extracted from Mythos (`#71`, PR #<PR#>).** `first_active_investigator` and `next_active_investigator_after` replace duplicated inline lookups at Mythos's seed and advance sites; future per-investigator loops use the same helpers. Surfacing the eliminated-skip predicate (Rules p.10) in one canonical site means the rule's call-site enumeration won't drift.
- **Early-break on `Status != Active` inside `resolve_attacks_for_investigator` (`#71`, PR #<PR#>).** Rules-correct minimal interpretation until the elimination-flow follow-up lands. The early-break stays as the simpler form even after that PR clears `enemy.engaged_with` on defeat — it just becomes one redundant check that doesn't hurt.
- **`step_phase`'s `_` arm is `unreachable!`, not a defensive emit (`#71`, PR #<PR#>).** Now that all four phases are driver-dispatched, `Phase::next()`'s "never returns its input" property makes the arm structurally unreachable. If it ever fires, it's a state-corruption invariant violation, not a normal fallback.
```

Skip routine entries: TODO-stub additions (`hunter_movement_step`) mirror an existing precedent and don't constrain later work; `Event::EnemyAttacked` deferral is concrete-consumer-first and doesn't have a future-shape implication.

- [ ] **Step 6: Add the two new follow-up issues to the doc**

Both issues were filed before this PR opened (see the spec's "Follow-ups filed alongside this spec" section). Add them:

- **In the Phase-4 Open table:**
  - `#144` — Formalize investigator elimination flow (Rules Reference p.10 Elimination steps 1–5). Blocked on #128.

- **In "Still unmilestoned (concrete-consumer-first)":**
  - `#143` — Player picks engaged-enemy attack order (Enemy phase 3.3 + attacks of opportunity). Concrete consumer = multi-engagement + multi-investigator scenario (Phase 7+).

Use the format already in the doc for unmilestoned items and Phase-4 open issues.

- [ ] **Step 7: Remove any Open question the PR settled**

Scan the "Open questions" section. The window-stack and skill-test-in-flight invariants are tightened by this PR but were already settled in spirit — no removals required unless an explicit question was filed about Enemy-phase shape (none currently). If one is found, remove it.

- [ ] **Step 8: Run formatters and review**

Run: `cargo fmt --check`
Expected: clean (no Rust source changed in this task).

Open `docs/phases/phase-4-scenario-plumbing.md` and sanity-check that the updates read coherently — Closed-table entry, Status section counts, Ordering row flip, Decisions additions, follow-up insertions. Markdown lint is not enforced but consistency with surrounding entries matters.

- [ ] **Step 9: Commit**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: update Phase-4 phase doc for #71 (Enemy phase: engagement attacks)

Move #71 from Open to Closed; flip Ordering row 8 to ✅ PR; add
Decisions entries for the four load-bearing choices (per-investigator
window shape + bare WindowKind variants + cursor field, shared cursor
helpers extracted from Mythos, early-break on defeat, step_phase _
arm becomes unreachable!); register the two follow-up issues filed
alongside this PR (#144 elimination flow in Phase 4 / blocked on
#128; #143 attack-order player-pick unmilestoned).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 10: Push and merge**

```bash
git push origin engine/enemy-phase-attacks
gh pr merge <PR#> --squash --delete-branch
```

Confirm the issue auto-closed via the `Closes #71` line in the PR body. `git pull` on `main` to sync.

---

## Final gauntlet (before opening the PR)

After Task 6 and before `gh pr create`, run the full five-job CI-equivalent gauntlet from CLAUDE.md:

```sh
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```

All five must pass before pushing. The `doc` job in particular catches broken intra-doc links to the new helpers / variants / fields — surface and fix any link errors locally rather than discovering them in CI.

---

## Self-review summary

**Spec coverage check:**
- Section "Scope" bullets — each maps to a task:
  - `BeforeInvestigatorAttacked` / `AfterAllInvestigatorsAttacked` → T3.
  - `enemy_attack_pending` field → T2.
  - `enemy_phase` / `hunter_movement_step` / `resolve_attacks_for_investigator` / `enemy_phase_end` → T4 + T5.
  - Shared cursor helpers + Mythos refactor → T1.
  - `run_window_continuation` arms → T3 placeholder, T5 real bodies.
  - `step_phase` edits (PhaseEnded suppression, dispatch arm, `unreachable!`) → T5.
  - Engine unit tests (14 total) → T4 (resolve_attacks_for_investigator 4) + T5 (driver shape 6) + T6 (pause/resume 1–2) + T1 (shared helper 5).
- Section "Out of scope" items remain out of scope (no tasks for them).
- Decisions table contents → T7 step 5.
- Follow-up issues → T7 step 6.

**Placeholder scan:** No `TBD` / `TODO: implement later` markers in the plan body. The `TODO(#128)` / `TODO(#143)` / `TODO(#144)` markers inside the code blocks are intentional `TODO` comments in the source that point at the corresponding GitHub issues.

**Type consistency:** Helper names, variant names, field names match across tasks: `first_active_investigator`, `next_active_investigator_after`, `BeforeInvestigatorAttacked`, `AfterAllInvestigatorsAttacked`, `enemy_attack_pending`, `enemy_phase`, `enemy_phase_end`, `resolve_attacks_for_investigator`, `hunter_movement_step`.

**Ordering safety:** Each task ships callable code (no dead_code warnings under `-D warnings`); each test compiles against state and APIs that exist at that task's point in the timeline.
