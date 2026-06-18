# Axis D — Cancellation / Replacement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build general Before-timing dispatch through `emit_event` plus an `Effect::Cancel` signal the emit site honors, ship Dodge 01023 (cancel an enemy attack), and migrate Cover Up 01007 off its bespoke `clue_interrupt` seam onto the new mechanism.

**Architecture:** A reaction-only Before-timing window opens via `emit_event` at two chokepoints (the enemy-attack loop and `discover_clue`). A reaction fired in that window may set `GameState.pending_cancellation`; the emit site reads-and-clears it after the window closes and skips the prevented impact (damage, or clue discovery). Cover Up's "discard instead" becomes `Seq[discard-from-self, Cancel]`. Reuses the Axis-C candidate list (so Dodge plays from hand) and the existing `pending_enemy_attack` suspend/resume idiom.

**Tech Stack:** Rust, `card-dsl` / `game-core` / `cards` crates. Effect-DSL, event-sourced `apply` loop, `ResolutionFrame` continuation stack.

## Global Constraints

- **Spec:** `docs/superpowers/specs/2026-06-18-trigger-dispatch-axis-d-cancellation-design.md`. Read it.
- **Crate layering:** `card-dsl` (pure data) ← `game-core` (kernel, no I/O, wasm-safe) ← `cards` (content). Never make `game-core` depend on `cards`.
- **Handler contract:** validate-first / mutate-second. On any precondition failure return `EngineOutcome::Rejected { reason }` with state + events unchanged.
- **No silent approximation / no speculative primitives.** Every deferral has a filed issue + a code `TODO(#nnn)`: #293 (AoO-cancel), #366 (replacement beyond cancel), #367 (nested Before-windows / typed marker), #368 (discover eligibility + capped count).
- **Card text is law:** any card text quoted in code/comments is copied verbatim from `data/arkhamdb-snapshot/pack/core/core.json`.
- **CI gauntlet (strict, warnings-as-errors)** before pushing:
  ```sh
  RUSTFLAGS="-D warnings" cargo test --all --all-features
  cargo clippy --all-targets --all-features -- -D warnings
  cargo fmt --check
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
  cargo build -p web --target wasm32-unknown-unknown
  cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
  ```
- **Branch:** `engine/axis-d-cancellation` (already created; the spec commit lives there). One commit per task. Commit subjects `engine:`/`cards:` per scope. Closes #336 / #305.

---

### Task 1: DSL primitives — `Effect::Cancel` + `EventPattern::EnemyAttacks`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (the `Effect` enum near line 532; the `EventPattern` enum near line 216)

**Interfaces:**
- Produces: `card_dsl::dsl::Effect::Cancel` (unit variant), `card_dsl::dsl::EventPattern::EnemyAttacks` (unit variant).

- [ ] **Step 1: Write the failing test** — add to the `#[cfg(test)]` module at the bottom of `crates/card-dsl/src/dsl.rs`:

```rust
#[test]
fn cancel_effect_and_enemy_attacks_pattern_round_trip() {
    let e = Effect::Cancel;
    let json = serde_json::to_string(&e).unwrap();
    assert_eq!(Effect::Cancel, serde_json::from_str(&json).unwrap());

    let p = EventPattern::EnemyAttacks;
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(EventPattern::EnemyAttacks, serde_json::from_str(&json).unwrap());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p card-dsl cancel_effect_and_enemy_attacks_pattern_round_trip`
Expected: FAIL — `no variant named Cancel` / `no variant named EnemyAttacks`.

- [ ] **Step 3: Add the `EnemyAttacks` variant** to `EventPattern` (place it after `EnemyAttackDamagedSelf`, matching the bare-variant style of `EnemySpawned`):

```rust
    /// An enemy is making an attack against an investigator (RR p.25 step
    /// 3.3). Before-timing only: the cancel/replacement window where Dodge
    /// 01023 ("when an enemy attacks an investigator at your location:
    /// cancel that attack") fires. Bare — the "at your location" spatial
    /// scoping lives in the reaction-window scan (which has board state),
    /// mirroring the soaked-asset filter for `EnemyAttackDamagedSelf`.
    EnemyAttacks,
```

- [ ] **Step 4: Add the `Cancel` variant** to `Effect` (place it next to `DiscardSelf`):

```rust
    /// Cancel the current cancellable game impact (the subject of the
    /// Before-timing window this effect resolves inside). Sets the engine's
    /// `pending_cancellation` signal, which the emit site honors after the
    /// window closes — skipping the prevented impact (an enemy attack's
    /// damage/horror, or a clue discovery). RR p.6: the cancelled thing is
    /// "still regarded as initiated", only its effects are prevented.
    ///
    /// Cancel is the degenerate replacement ("replace with nothing"): a card
    /// that replaces with its own effect runs that effect then `Cancel`
    /// (Cover Up 01007 = `Seq[discard-from-self, Cancel]`).
    /// TODO(#366): a true replace-with-a-different-impact effect.
    Cancel,
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p card-dsl cancel_effect_and_enemy_attacks_pattern_round_trip`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/card-dsl/src/dsl.rs
git commit -m "card-dsl: add Effect::Cancel + EventPattern::EnemyAttacks (Axis D #336)"
```

---

### Task 2: Cancel signal — `GameState.pending_cancellation` + evaluator arm

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (GameState fields near line 235; the `Default`/constructor if fields are initialized there)
- Modify: `crates/game-core/src/engine/evaluator.rs` (the `apply_effect` match)

**Interfaces:**
- Consumes: `Effect::Cancel` (Task 1).
- Produces: `GameState.pending_cancellation: bool`; an `apply_effect` arm for `Effect::Cancel` that sets it.

- [ ] **Step 1: Write the failing test** — add to `crates/game-core/src/engine/evaluator.rs`'s `#[cfg(test)]` module:

```rust
#[test]
fn cancel_effect_sets_pending_cancellation() {
    use crate::state::{Continuation, ResolutionFrame, ResolutionKind, ForcedContinuation};
    let mut state = crate::test_support::TestGame::new().build();
    // Effect::Cancel asserts a resolution frame is open; push a minimal one.
    state.continuations.push(Continuation::Resolution(ResolutionFrame {
        pending_triggers: Vec::new(),
        kind: ResolutionKind::Forced(ForcedContinuation::Terminal),
    }));
    assert!(!state.pending_cancellation);
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let out = apply_effect(&mut cx, &crate::dsl::Effect::Cancel, EvalContext::for_controller(crate::state::InvestigatorId(0)));
    assert!(matches!(out, EngineOutcome::Done));
    assert!(state.pending_cancellation);
}
```

(If `Cx` / `TestGame` construction differs in this module, mirror an existing evaluator test's setup — several already build a `Cx` inline.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core cancel_effect_sets_pending_cancellation`
Expected: FAIL — `no field pending_cancellation` / no `Effect::Cancel` arm.

- [ ] **Step 3: Add the state field** in `crates/game-core/src/state/game_state.rs` next to `clue_interrupt_pending`:

```rust
    /// Set by [`Effect::Cancel`](crate::dsl::Effect::Cancel) while a
    /// Before-timing reaction window resolves; read-and-cleared by the emit
    /// site (the enemy-attack loop, `discover_clue`) after the window closes,
    /// to skip the prevented impact (Axis D #336). A bool suffices because
    /// Before-windows do not nest in scope (exactly one cancellable impact is
    /// ever in flight). TODO(#367): typed marker once Before-windows can nest.
    #[serde(default)]
    pub pending_cancellation: bool,
```

Initialize it to `false` wherever the other `pending_*` fields are initialized (the `GameState` constructor / `Default` impl).

- [ ] **Step 4: Add the evaluator arm** in `apply_effect` (`crates/game-core/src/engine/evaluator.rs`):

```rust
        crate::dsl::Effect::Cancel => {
            debug_assert!(
                cx.state.top_reaction_window_index().is_some(),
                "Effect::Cancel evaluated with no open resolution window — a \
                 card cancelled outside a Before-timing window (TODO(#367) \
                 covers nesting; a malformed card otherwise)"
            );
            cx.state.pending_cancellation = true;
            EngineOutcome::Done
        }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p game-core cancel_effect_sets_pending_cancellation`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: pending_cancellation signal + Effect::Cancel evaluator arm (Axis D #336)"
```

---

### Task 3: Before-timing dispatch foundation (window kinds, timing events, `trigger_matches`)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (the `WindowKind` enum near line 901)
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` (`TimingEvent` + its three mapping methods)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`trigger_matches` near line 259)

**Interfaces:**
- Produces:
  - `WindowKind::BeforeEnemyAttack { enemy: EnemyId, investigator: InvestigatorId }`
  - `WindowKind::BeforeDiscoverClues { investigator: InvestigatorId, location: LocationId, count: u8 }`
  - `TimingEvent::EnemyAttacks { enemy: EnemyId, investigator: InvestigatorId }`
  - `TimingEvent::WouldDiscoverClues { investigator: InvestigatorId, location: LocationId, count: u8 }`
  - `trigger_matches` returns `true` for `(BeforeEnemyAttack, EnemyAttacks, Before)` and `(BeforeDiscoverClues, WouldDiscoverClues, Before)`.

- [ ] **Step 1: Write the failing test** — add to `crates/game-core/src/engine/dispatch/reaction_windows.rs`'s test module:

```rust
#[test]
fn trigger_matches_before_pairs() {
    use crate::state::{EnemyId, InvestigatorId, LocationId};
    let inv = InvestigatorId(0);
    assert!(trigger_matches(
        WindowKind::BeforeEnemyAttack { enemy: EnemyId(1), investigator: inv },
        &EventPattern::EnemyAttacks,
        EventTiming::Before,
        inv,
    ));
    assert!(trigger_matches(
        WindowKind::BeforeDiscoverClues { investigator: inv, location: LocationId(2), count: 1 },
        &EventPattern::WouldDiscoverClues,
        EventTiming::Before,
        inv,
    ));
    // Wrong timing / wrong pairing still false.
    assert!(!trigger_matches(
        WindowKind::BeforeEnemyAttack { enemy: EnemyId(1), investigator: inv },
        &EventPattern::EnemyAttacks,
        EventTiming::After,
        inv,
    ));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core trigger_matches_before_pairs`
Expected: FAIL — `no variant BeforeEnemyAttack`.

- [ ] **Step 3: Add the two `WindowKind` variants** in `crates/game-core/src/state/game_state.rs` (after `AfterSuccessfulInvestigate`):

```rust
    /// Before-timing window: an enemy is about to attack `investigator` (RR
    /// p.25 step 3.3). Opens *before* damage is dealt so a co-located cancel
    /// reaction (Dodge 01023) can cancel the attack. `enemy` is the attacker.
    /// (Axis D #336.)
    BeforeEnemyAttack {
        /// The attacking enemy.
        enemy: EnemyId,
        /// The investigator being attacked.
        investigator: InvestigatorId,
    },
    /// Before-timing window: `investigator` is about to discover `count`
    /// clues at `location`. Opens *before* the discovery so a replacement
    /// reaction (Cover Up 01007) can discard-instead and cancel the
    /// discovery. (Axis D #336; migrated from the C5a `clue_interrupt` seam.)
    BeforeDiscoverClues {
        /// The discovering investigator.
        investigator: InvestigatorId,
        /// The location the clues would come from.
        location: LocationId,
        /// The number of clues that would be discovered.
        count: u8,
    },
```

- [ ] **Step 4: Add the two `TimingEvent` variants + map them** in `crates/game-core/src/engine/dispatch/emit.rs`:

In the `TimingEvent` enum:
```rust
    /// An enemy is about to attack an investigator (reaction-only, Before).
    /// Opens the `BeforeEnemyAttack` cancel window (Dodge 01023). (Axis D.)
    EnemyAttacks {
        enemy: EnemyId,
        investigator: InvestigatorId,
    },
    /// An investigator is about to discover clues (reaction-only, Before).
    /// Opens the `BeforeDiscoverClues` replacement window (Cover Up 01007).
    /// (Axis D; migrated from the C5a `clue_interrupt` seam.)
    WouldDiscoverClues {
        investigator: InvestigatorId,
        location: LocationId,
        count: u8,
    },
```
Add `LocationId` to the `use crate::state::{…}` import if not present.

In `forced_point` — both are reaction-only, so add to the `None` cases:
```rust
            TimingEvent::EnemyAttacks { .. } | TimingEvent::WouldDiscoverClues { .. } => None,
```
(extend the existing `EnemyAttackDamagedSelf => None` arm.)

In `reaction_window`:
```rust
            TimingEvent::EnemyAttacks { enemy, investigator } => {
                Some(WindowKind::BeforeEnemyAttack { enemy: *enemy, investigator: *investigator })
            }
            TimingEvent::WouldDiscoverClues { investigator, location, count } => {
                Some(WindowKind::BeforeDiscoverClues {
                    investigator: *investigator, location: *location, count: *count,
                })
            }
```

In `forced_continuation` — both are reaction-only (no forced phase), so add them to the `None` arm alongside `EnemyAttackDamagedSelf`:
```rust
            | TimingEvent::EnemyAttacks { .. }
            | TimingEvent::WouldDiscoverClues { .. } => None,
```

- [ ] **Step 5: Restructure `trigger_matches`** in `crates/game-core/src/engine/dispatch/reaction_windows.rs` to admit the two Before pairs. Replace the `if timing != EventTiming::After { return false; }` guard + the trailing match with:

```rust
    match timing {
        EventTiming::Before => matches!(
            (kind, pattern),
            (WindowKind::BeforeEnemyAttack { .. }, EventPattern::EnemyAttacks)
                | (WindowKind::BeforeDiscoverClues { .. }, EventPattern::WouldDiscoverClues)
        ),
        EventTiming::After => match (kind, pattern) {
            // ... the entire existing (kind, pattern) match body, unchanged ...
        },
    }
```

(The existing `After` arms already enumerate every `(kind, pattern)`; wrap them under the `EventTiming::After` branch. Add the two new `WindowKind` variants to the catch-all `false` arm's kind list so the match stays exhaustive.)

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p game-core trigger_matches_before_pairs`
Expected: PASS.

- [ ] **Step 7: Run the full game-core suite** (no regressions from the `trigger_matches` restructure / exhaustiveness):

Run: `cargo test -p game-core`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/emit.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: Before-timing dispatch foundation — Before window kinds + timing events (Axis D #336)"
```

---

### Task 4: Enemy-attack loop consumer (the Dodge path, engine side)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`PendingEnemyAttack` near line 394; add `AttackLoopPhase`)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`drive_attack_loop`, `resume_enemy_attack`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`scan_pending_triggers` + `scan_hand_fast_events` co-location filter; `run_window_continuation` arm)

**Interfaces:**
- Consumes: `TimingEvent::EnemyAttacks`, `WindowKind::BeforeEnemyAttack` (Task 3); `pending_cancellation` (Task 2).
- Produces: `AttackLoopPhase { BeforeAttack, AfterSoak }`; `PendingEnemyAttack.phase: AttackLoopPhase`; a before-cancel window in `drive_attack_loop`.

- [ ] **Step 1: Write the failing test** — add to `crates/game-core/src/engine/dispatch/combat.rs`'s test module. This exercises the loop's cancel-resume directly (no registry needed: park a `BeforeAttack` pending and resume with the flag set):

```rust
#[test]
fn resume_before_attack_cancel_skips_damage_but_exhausts() {
    use crate::state::{AttackLoopPhase, EnemyAttackSource, InvestigatorId, PendingEnemyAttack, Status};
    let mut state = /* TestGame: one active investigator at a location, one engaged
        ready enemy dealing 1 damage; mirror resume_enemy_attack_drains_remaining_attackers test setup */;
    let inv = InvestigatorId(0);
    let enemy = /* the engaged enemy id */;
    let dmg_before = /* investigator accumulated_damage */;
    state.pending_cancellation = true; // a cancel reaction fired in the before-window
    state.pending_enemy_attack = Some(PendingEnemyAttack {
        investigator: inv,
        remaining_attackers: vec![enemy],
        source: EnemyAttackSource::EnemyPhase,
        phase: AttackLoopPhase::BeforeAttack,
    });
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let _ = super::resume_enemy_attack(&mut cx);
    // No damage dealt (cancelled), flag cleared, attacker exhausted.
    assert_eq!(/* investigator accumulated_damage */, dmg_before);
    assert!(!state.pending_cancellation);
    assert!(state.enemies.get(&enemy).unwrap().exhausted);
}
```

(Use the exact fixture shape from the existing `resume_enemy_attack_drains_remaining_attackers_and_advances_cursor` test in this file — it already builds an investigator + engaged enemies and parks a `PendingEnemyAttack`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core resume_before_attack_cancel_skips_damage_but_exhausts`
Expected: FAIL — `no field phase` / `no variant AttackLoopPhase`.

- [ ] **Step 3: Add `AttackLoopPhase` + the `phase` field** in `crates/game-core/src/state/game_state.rs`:

```rust
/// Which point in the per-attacker sequence a parked enemy-attack loop
/// suspended at (Axis D #336).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackLoopPhase {
    /// Suspended on the `BeforeEnemyAttack` cancel window, *before* the head
    /// attacker dealt damage. Resume reads `pending_cancellation`, then deals
    /// (or skips) and exhausts the head attacker.
    BeforeAttack,
    /// Suspended on the `AfterEnemyAttackDamagedAsset` soak window, *after*
    /// the head attacker dealt + exhausted. Resume drains the rest (the
    /// pre-Axis-D behavior).
    AfterSoak,
}
```

Add to `PendingEnemyAttack`:
```rust
    /// Where in the per-attacker sequence the loop suspended (Axis D #336).
    #[serde(default = "default_attack_loop_phase")]
    pub phase: AttackLoopPhase,
```
plus a serde default helper near the struct:
```rust
fn default_attack_loop_phase() -> AttackLoopPhase {
    AttackLoopPhase::AfterSoak // pre-Axis-D parked loops were always AfterSoak
}
```

- [ ] **Step 4: Restructure `drive_attack_loop`** in `crates/game-core/src/engine/dispatch/combat.rs`. Extract the deal-and-exhaust body into a helper and open the before-window at the top of each iteration:

```rust
/// Deal one attacker's damage (unless `cancelled`), queue its soak window,
/// and exhaust it. Leaves the open-window suspend check to the caller.
fn process_attacker_dealing(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    cancelled: bool,
) {
    if !cancelled {
        let damaged_survivors = enemy_attack(cx, enemy_id, investigator);
        for asset in damaged_survivors {
            let _ = super::emit::emit_event(
                cx,
                &super::emit::TimingEvent::EnemyAttackDamagedSelf { asset, enemy: enemy_id, controller: investigator },
            );
        }
    }
    // Exhaust the attacker — even on cancel (RR p.6 + p.25: the attack was
    // made; only its effect is cancelled). Enemy-phase only; AoO never
    // exhausts (RR p.7) and does not reach this loop yet (TODO(#293)).
    let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!("process_attacker_dealing: enemy {enemy_id:?} gone — state corruption")
    });
    enemy.exhausted = true;
    cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
}

fn drive_attack_loop(
    cx: &mut Cx,
    investigator: InvestigatorId,
    mut attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    while let Some(&enemy_id) = attackers.first() {
        // Early-break on defeat (unchanged; see fn doc step 1).
        let active = cx.state.investigators.get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active { break; }

        // Before-attack cancel window (Axis D #336). Reaction-only Before
        // timing point; opens iff a co-located cancel reaction is available
        // (Dodge in hand, or an in-play reaction). Suspend BEFORE damage.
        let _ = super::emit::emit_event(
            cx,
            &super::emit::TimingEvent::EnemyAttacks { enemy: enemy_id, investigator },
        );
        if !cx.state.open_windows().is_empty() {
            cx.state.pending_enemy_attack = Some(PendingEnemyAttack {
                investigator,
                remaining_attackers: attackers, // head still at front
                source,
                phase: AttackLoopPhase::BeforeAttack,
            });
            return super::reaction_windows::open_queued_reaction_window(cx);
        }

        // No cancel reaction: this attacker is not cancelled.
        attackers.remove(0);
        process_attacker_dealing(cx, investigator, enemy_id, false);

        // Soak window suspend (unchanged invariant; see fn doc step 4).
        if !cx.state.open_windows().is_empty() {
            debug_assert_eq!(cx.state.open_windows().len(), 1, /* keep existing message */);
            cx.state.pending_enemy_attack = Some(PendingEnemyAttack {
                investigator,
                remaining_attackers: attackers,
                source,
                phase: AttackLoopPhase::AfterSoak,
            });
            return super::reaction_windows::open_queued_reaction_window(cx);
        }
    }
    EngineOutcome::Done
}
```

- [ ] **Step 5: Update `resume_enemy_attack`** to branch on `phase`:

```rust
pub(super) fn resume_enemy_attack(cx: &mut Cx) -> EngineOutcome {
    let pending = cx.state.pending_enemy_attack.take().unwrap_or_else(|| {
        unreachable!("resume_enemy_attack: no pending_enemy_attack parked — state corruption")
    });
    let PendingEnemyAttack { investigator, mut remaining_attackers, source, phase } = pending;

    if phase == AttackLoopPhase::BeforeAttack {
        // The before-cancel window for the head attacker closed.
        let cancelled = std::mem::take(&mut cx.state.pending_cancellation);
        let enemy_id = remaining_attackers.remove(0);
        process_attacker_dealing(cx, investigator, enemy_id, cancelled);
        // A soak window may have opened on the (non-cancelled) head: re-park.
        if !cx.state.open_windows().is_empty() {
            debug_assert_eq!(cx.state.open_windows().len(), 1, /* keep existing message */);
            cx.state.pending_enemy_attack = Some(PendingEnemyAttack {
                investigator, remaining_attackers, source, phase: AttackLoopPhase::AfterSoak,
            });
            return super::reaction_windows::open_queued_reaction_window(cx);
        }
    }

    let outcome = drive_attack_loop(cx, investigator, remaining_attackers, source);
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        return outcome;
    }
    debug_assert!(matches!(outcome, EngineOutcome::Done), "drive_attack_loop returned {outcome:?}");
    match source {
        EnemyAttackSource::EnemyPhase => super::reaction_windows::after_enemy_phase_attacks(cx, investigator),
        EnemyAttackSource::AttackOfOpportunity => EngineOutcome::Done,
    }
}
```

Add `AttackLoopPhase` to the `use crate::state::{…}` import in `combat.rs`.

- [ ] **Step 6: Add the co-location scan filter** in `crates/game-core/src/engine/dispatch/reaction_windows.rs`. Add the helper:

```rust
/// Whether investigators `a` and `b` share a (revealed) current location.
fn same_location(state: &GameState, a: InvestigatorId, b: InvestigatorId) -> bool {
    let la = state.investigators.get(&a).and_then(|i| i.current_location);
    la.is_some() && la == state.investigators.get(&b).and_then(|i| i.current_location)
}
```

In **both** `scan_pending_triggers` and `scan_hand_fast_events`, right after `let Some(inv) = state.investigators.get(&id) else { continue; };`, add:

```rust
        // "at your location" scoping for the before-attack cancel window
        // (Dodge 01023): a candidate's controller must be co-located with the
        // attacked investigator. Other window kinds pass all controllers.
        if let WindowKind::BeforeEnemyAttack { investigator, .. } = kind {
            if !same_location(state, id, investigator) {
                continue;
            }
        }
```

- [ ] **Step 7: Add the `run_window_continuation` arm** in `reaction_windows.rs`. Merge `BeforeEnemyAttack` with the existing soak arm:

```rust
        WindowKind::AfterEnemyAttackDamagedAsset { .. } | WindowKind::BeforeEnemyAttack { .. } => {
            super::combat::resume_enemy_attack(cx)
        }
```

(Add `BeforeDiscoverClues` to the match too, as a temporary `=> EngineOutcome::Done` placeholder; Task 6 replaces it. This keeps the match exhaustive and compiling.)

- [ ] **Step 8: Run the test + the combat suite**

Run: `cargo test -p game-core resume_before_attack_cancel_skips_damage_but_exhausts`
Then: `cargo test -p game-core combat`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/combat.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: before-attack cancel window in the enemy-attack loop (Axis D #336)"
```

---

### Task 5: Dodge 01023 card + integration test

**Files:**
- Create: `crates/cards/src/impls/dodge.rs`
- Modify: `crates/cards/src/impls/mod.rs` (add `pub mod dodge;` + an `abilities_for` arm)
- Create: `crates/cards/tests/dodge.rs`

**Interfaces:**
- Consumes: `Effect::Cancel`, `EventPattern::EnemyAttacks` (Task 1); the engine before-attack window (Task 4).
- Produces: `cards::dodge::{CODE, abilities}`; `cards::abilities_for("01023")`.

- [ ] **Step 1: Verify the card text** (don't trust the plan):

Run: `python3 -c "import json; [print(c['name'],'|',repr(c['text'])) for c in json.load(open('data/arkhamdb-snapshot/pack/core/core.json')) if c.get('code')=='01023']"`
Expected: `Dodge | 'Fast. Play when an enemy attacks an investigator at your location.\nCancel that attack.'`

- [ ] **Step 2: Write the card impl** `crates/cards/src/impls/dodge.rs`:

```rust
//! Dodge (Neutral Tactic event, 01023).
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! ```text
//! Fast. Play when an enemy attacks an investigator at your location.
//! Cancel that attack.
//! ```
//!
//! A reaction event played from hand in the `BeforeEnemyAttack` window
//! (Axis C + Axis D). The play-timing predicate is the `OnEvent` pattern
//! (RR p.11). "Cancel that attack" is `Effect::Cancel` — the emit site (the
//! enemy-attack loop) skips the attack's damage/horror but still exhausts
//! the attacker (RR p.6 + p.25). The Fast/cost metadata comes from the corpus.

use card_dsl::dsl::{reaction_on_event, Ability, Effect, EventPattern, EventTiming};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01023";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::EnemyAttacks,
        EventTiming::Before,
        Effect::Cancel,
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger, TriggerKind};

    #[test]
    fn one_before_enemy_attack_reaction_that_cancels() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyAttacks,
                timing: EventTiming::Before,
                kind: TriggerKind::Reaction,
            },
        );
        assert!(matches!(abilities[0].effect, Effect::Cancel));
        assert!(abilities[0].usage_limit.is_none());
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
```

- [ ] **Step 3: Register it** in `crates/cards/src/impls/mod.rs`: add `pub mod dodge;` (alphabetical-ish with the others) and, in `abilities_for`, `dodge::CODE => Some(dodge::abilities()),`.

- [ ] **Step 4: Run the card unit tests**

Run: `cargo test -p cards dodge`
Expected: PASS.

- [ ] **Step 5: Write the integration test** `crates/cards/tests/dodge.rs` (real registry; mirror `crates/cards/tests/guard_dog_soak.rs` setup for an enemy-phase attack, and `crates/cards/tests/evidence.rs` for installing the registry + seeding a hand):

```rust
//! Dodge 01023 cancels an enemy-phase attack (Axis D #336 / #305).
use game_core::{/* apply, PlayerAction, InputResponse, Event, … as guard_dog_soak.rs uses */};

#[test]
fn dodge_cancels_enemy_phase_attack_no_damage_attacker_exhausts() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Seat solo investigator at a location, engaged with a ready enemy that
    // deals damage; put Dodge ("01023") in hand. Drive into the enemy phase
    // so the before-attack window opens (offers Dodge as PickSingle(0)).
    // ... build state per guard_dog_soak.rs ...

    // Resolve the before-attack window by playing Dodge.
    let out = apply(state, PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(0)),
    });
    // No damage/horror dealt; attacker exhausted; Dodge in discard.
    assert_no_event!(out.events, Event::DamageDealt { .. });
    assert_event!(out.events, Event::EnemyExhausted { .. });
    assert!(/* "01023" is in the investigator's discard */);
}

#[test]
fn declining_the_window_lets_the_attack_land() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Same setup; submit InputResponse::Skip instead → damage lands, attacker exhausts.
    // ... assert DamageDealt present ...
}
```

(Fill the state-building with the concrete `guard_dog_soak.rs` pattern — same crate, same helpers. Use the exact `Event` variant names that crate already asserts on.)

- [ ] **Step 6: Run the integration test**

Run: `cargo test -p cards --test dodge`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/cards/src/impls/dodge.rs crates/cards/src/impls/mod.rs crates/cards/tests/dodge.rs
git commit -m "cards: Dodge 01023 — cancel an enemy attack (Axis D, closes #305)"
```

---

### Task 6: Cover Up migration onto the Before-discover window

**Files:**
- Modify: `crates/cards/src/impls/treachery_01007.rs` (Cover Up reaction effect → `Seq[native, Cancel]`)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`discover_clue` interrupt block → `emit_event`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`scan_pending_triggers` discover eligibility; `fire_pending_trigger` count threading; `run_window_continuation` `BeforeDiscoverClues` arm; widen `open_queued_reaction_window` visibility)
- Modify: `crates/scenarios/tests/cover_up_interrupt.rs` (new input protocol)

**Interfaces:**
- Consumes: `WindowKind::BeforeDiscoverClues`, `TimingEvent::WouldDiscoverClues` (Task 3); `pending_cancellation` (Task 2).
- Produces: `discover_clue` opens the Before-discover window; Cover Up cancels the discovery.

- [ ] **Step 1: Change Cover Up's reaction effect** in `crates/cards/src/impls/treachery_01007.rs` — wrap the native in `Seq[…, Cancel]` so firing it both discards and cancels the discovery:

```rust
        reaction_on_event(
            EventPattern::WouldDiscoverClues,
            EventTiming::Before,
            card_dsl::dsl::Effect::Seq(vec![native(DISCARD_TAG), card_dsl::dsl::Effect::Cancel]),
        ),
```

Update the in-file test `revelation_places_with_three_clues_plus_interrupt_and_gameend` to assert `abilities[1].effect` is a `Seq` whose last element is `Effect::Cancel`.

- [ ] **Step 2: Widen `open_queued_reaction_window` visibility** in `reaction_windows.rs`: change `pub(super) fn open_queued_reaction_window` to `pub(crate) fn open_queued_reaction_window` (the evaluator's `discover_clue` calls it).

- [ ] **Step 3: Replace `discover_clue`'s interrupt block** in `crates/game-core/src/engine/evaluator.rs`. Delete the entire `if let Some(reg) = crate::card_registry::current() { … clue_interrupt_pending = … return AwaitingInput … }` block (lines ~876–955) and the `perform_discovery` tail, replacing with:

```rust
    // Before-timing clue-discovery window (Cover Up 01007; Axis D #336,
    // migrated from the C5a clue_interrupt seam). Reaction-only Before timing
    // point: emit_event queues the window iff an eligible WouldDiscoverClues
    // reaction is controlled at the discovery location; if it opened, suspend.
    let _ = crate::engine::dispatch::emit::emit_event(
        cx,
        &crate::engine::dispatch::emit::TimingEvent::WouldDiscoverClues {
            investigator: eval_ctx.controller,
            location: location_id,
            count,
        },
    );
    if !cx.state.open_windows().is_empty() {
        return crate::engine::dispatch::reaction_windows::open_queued_reaction_window(cx);
    }
    perform_discovery(cx, location_id, count, eval_ctx.controller);
    EngineOutcome::Done
```

- [ ] **Step 4: Add the discover eligibility filter** in `scan_pending_triggers` (`reaction_windows.rs`), right after the `AfterEnemyAttackDamagedAsset` self-binding filter (per-card, inside the `for card in …` loop):

```rust
            // "When YOU would discover … at YOUR location" (Cover Up 01007):
            // controller is the discoverer, at the discovery location, and the
            // card has clues to discard. The `card.clues > 0` gate is a
            // single-consumer stand-in for RR p.2 "potential to change the
            // game state" — TODO(#368) lift to a per-ability predicate.
            if let WindowKind::BeforeDiscoverClues { investigator, location, .. } = kind {
                if id != investigator { continue; }
                let at_loc = state.investigators.get(&id)
                    .and_then(|i| i.current_location) == Some(location);
                if !at_loc || card.clues == 0 { continue; }
            }
```

- [ ] **Step 5: Thread the count** in `fire_pending_trigger` (`reaction_windows.rs`), right after the `attacking_enemy` threading block:

```rust
    // For BeforeDiscoverClues, bind the would-be discovery count so the
    // replacement effect (Cover Up "discard that many") discards the right
    // number. TODO(#368): `count` is the requested, not capped, count.
    if let Some(WindowKind::BeforeDiscoverClues { count, .. }) =
        cx.state.continuations[window_idx].as_resolution().and_then(ResolutionFrame::kind)
    {
        eval_ctx.clue_discovery_count = Some(count);
    }
```

- [ ] **Step 6: Replace the `BeforeDiscoverClues` placeholder arm** in `run_window_continuation` (added in Task 4 step 7) with the real continuation:

```rust
        WindowKind::BeforeDiscoverClues { investigator, location, count } => {
            // The before-discover window closed. If a reaction cancelled the
            // discovery (Cover Up played its replacement), skip it; otherwise
            // perform the deferred discovery. Then, if a skill test is in
            // flight (the dominant path: Investigate's follow-up), resume its
            // driver — its continuation was pre-advanced to PostFollowUp.
            let cancelled = std::mem::take(&mut cx.state.pending_cancellation);
            if !cancelled {
                crate::engine::evaluator::perform_discovery(cx, location, count, investigator);
            }
            if cx.state.in_flight_skill_test.is_some() {
                super::skill_test::drive_skill_test(cx)
            } else {
                EngineOutcome::Done
            }
        }
```

- [ ] **Step 7: Update the scenario test protocol** in `crates/scenarios/tests/cover_up_interrupt.rs`: the old seam used `InputResponse::Confirm` (replace) / `Skip` (discover). The new window uses `InputResponse::PickSingle(OptionId(0))` (play Cover Up = replace) / `Skip` (discover). Replace `Confirm` submissions with `PickSingle(OptionId(0))`; the assertions (clue discarded from Cover Up vs. discovered) stay the same.

- [ ] **Step 8: Run the Cover Up suites**

Run: `cargo test -p cards --test cover_up`
Then: `cargo test -p scenarios --test cover_up_interrupt`
Expected: PASS (replace path discards from Cover Up + no clue discovered; Skip path discovers).

- [ ] **Step 9: Commit**

```bash
git add crates/cards/src/impls/treachery_01007.rs crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/reaction_windows.rs crates/scenarios/tests/cover_up_interrupt.rs
git commit -m "engine: migrate Cover Up clue interrupt onto the Before-discover window (Axis D #336)"
```

---

### Task 7: Delete the dead `clue_interrupt` seam + wire deferral TODOs

**Files:**
- Delete: `crates/game-core/src/engine/dispatch/clue_interrupt.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (remove `mod clue_interrupt;`, the pre-action guard ~line 103, the resume routing ~line 491)
- Modify: `crates/game-core/src/state/game_state.rs` (remove `ClueInterruptPending` struct + `clue_interrupt_pending` field + its tests)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`EvalContext.clue_discovery_count` doc / any stale `clue_interrupt` refs)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`TODO(#293)` on `fire_attacks_of_opportunity`)

**Interfaces:**
- Consumes: nothing new — pure removal + doc/TODO repointing.

- [ ] **Step 1: Delete the resume module + its routing.** Remove `crates/game-core/src/engine/dispatch/clue_interrupt.rs`; in `dispatch/mod.rs` remove `mod clue_interrupt;`, the `if cx.state.clue_interrupt_pending.is_some() && !matches!(action, ResolveInput…)` pre-action guard, and the `if cx.state.clue_interrupt_pending.is_some() { return clue_interrupt::resume_clue_interrupt(…); }` resume block.

- [ ] **Step 2: Remove the state type.** In `game_state.rs` delete the `clue_interrupt_pending` field, the `ClueInterruptPending` struct, and the `clue_interrupt_pending_tests` module. Remove its initialization from the constructor/`Default`.

- [ ] **Step 3: Repoint stale TODOs.** Update the surviving `TODO(#212)` clue refs to `TODO(#368)` (the eligibility/count concerns now live in `scan_pending_triggers` / `fire_pending_trigger`, added in Task 6 — confirm those carry `TODO(#368)`). Leave the unrelated `pending_revelation_discard` `TODO(#212)` at `game_state.rs` untouched (out of scope — note this in the commit body).

- [ ] **Step 4: Add the AoO deferral TODO** on `fire_attacks_of_opportunity` in `combat.rs` (it already documents the soak gap; extend it):

```rust
        // ... existing soak-window-gap comment ...
        // The before-attack cancel window (Dodge 01023) is likewise not
        // opened here for the same reason — TODO(#293) routes AoO through
        // drive_attack_loop so both windows fire (and must NOT exhaust the
        // attacker: RR p.7).
```

- [ ] **Step 5: Compile + run the game-core + scenarios suites** (catch every reference to the deleted symbols):

Run: `cargo test -p game-core`
Then: `cargo test -p scenarios`
Expected: PASS. (Fix any lingering `clue_interrupt_pending` references the compiler flags — they should all be gone.)

- [ ] **Step 6: Confirm the deferral TODOs are all present:**

Run: `grep -rn "TODO(#293)\|TODO(#366)\|TODO(#367)\|TODO(#368)" crates/`
Expected: #366 on `Effect::Cancel` (card-dsl), #367 on `pending_cancellation` + the `Effect::Cancel` evaluator arm, #368 in the discover scan/threading, #293 on `fire_attacks_of_opportunity`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "engine: delete the bespoke clue_interrupt seam, repoint TODOs (Axis D #336)"
```

---

### Task 8: Full gauntlet + phase-doc update (final commit) + PR

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Run the full CI gauntlet** (every job, strict flags from Global Constraints). Fix anything red before proceeding.

Run (each in turn):
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green.

- [ ] **Step 2: Push + open the PR** (per the autonomous PR flow): push `engine/axis-d-cancellation`, `gh pr create` with the template; body explains the Before-timing+cancel design, the Cover Up migration, and lists the deferrals (#293/#366/#367/#368). `Closes #336.` and `Closes #305.`

- [ ] **Step 3: Watch CI** via `gh pr checks <PR#> --watch`; fix failures with follow-up commits to the same branch.

- [ ] **Step 4: Update the phase doc** (`docs/phases/phase-7-the-gathering.md`) as the **final** commit, once CI is green — per `docs/phases/README.md`:
  - Flip the Axis-D row (Future slices §, "**D** cancellation/replacement (#336)") to `✅ PR #N`.
  - Note Dodge 01023 (#305) shipped and the Cover Up `clue_interrupt` seam was deleted/migrated.
  - Add a **Decisions made** entry only if load-bearing for future PRs (the §10 entry from the spec: Before-timing = reaction-only `emit_event` window + `Effect::Cancel`/`pending_cancellation` honored at the emit site; cancel = degenerate replacement; bool suffices (no nesting, #367); cancelled enemy-phase attack still exhausts (RR p.6+p.25) but AoO never does (#293)).
  - Remove no open question (none was open on Axis D).

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — Axis D cancellation shipped (Dodge 01023, Cover Up migrated)"
git push
```

- [ ] **Step 5: Merge only after explicit user approval**, via `gh pr merge <PR#> --squash --delete-branch`; confirm #336 + #305 auto-closed and `git pull` on `main`.

---

## Self-Review

**Spec coverage:**
- §3 DSL surface → Task 1 (Cancel, EnemyAttacks), Task 3 (WindowKinds, TimingEvents, trigger_matches). ✓
- §3 scan spatial/eligibility filters → Task 4 (BeforeEnemyAttack co-location), Task 6 (BeforeDiscoverClues eligibility). ✓
- §4 cancel signal (bool, debug_assert, take-at-emit-site) → Task 2 (field + arm), Task 4 + Task 6 (consume). ✓
- §5 enemy-attack loop (AttackLoopPhase, before-window, exhaust-always) → Task 4. ✓
- §6 Cover Up migration (emit, threading, continuation, seam deletion) → Task 6 + Task 7. ✓
- §7 Dodge card + tests → Task 5. ✓
- §8 deferrals (#293/#366/#367/#368 each with a TODO) → Task 1 (#366), Task 2 (#367), Task 6 (#368), Task 7 (#293 + #368 repoint). ✓

**Placeholder scan:** The integration-test bodies (Task 5 step 5, Task 4 step 1) intentionally point at concrete sibling fixtures (`guard_dog_soak.rs`, `evidence.rs`, the existing `resume_enemy_attack_…` test) for the state-building boilerplate rather than reproducing ~40 lines of unverified fixture code; the assertions and the Axis-D-specific calls are spelled out. Every other code step is complete.

**Type consistency:** `AttackLoopPhase { BeforeAttack, AfterSoak }`, `PendingEnemyAttack.phase`, `pending_cancellation: bool`, `WindowKind::BeforeEnemyAttack { enemy, investigator }` / `BeforeDiscoverClues { investigator, location, count }`, `TimingEvent::EnemyAttacks { enemy, investigator }` / `WouldDiscoverClues { investigator, location, count }` are used identically across Tasks 3–7. `open_queued_reaction_window` widened to `pub(crate)` in Task 6 before the evaluator calls it. `perform_discovery` (pub(crate)) reused by the continuation arm.
