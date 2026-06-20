# AttackLoop Frame Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the `GameState::enemy_attack_pending` and `GameState::pending_enemy_attack` framework cursors by moving the parked enemy-attack loop onto a `Continuation::AttackLoop` frame and the per-investigator cursor onto an `attacking` field of the `EnemyPhase` anchor — a behaviour-preserving lift (step 3 of #393).

**Architecture:** Three independent, behaviour-preserving refactors, each leaving the tree green: (1) rename the misnamed `AttackLoopPhase` enum to `AttackLoopStage`; (2) lift the parked suspension `pending_enemy_attack` onto a new `Continuation::AttackLoop` stack frame; (3) lift the `enemy_attack_pending` cursor onto the `EnemyPhase` anchor as an `attacking: Option<InvestigatorId>` field. The existing combat / enemy-phase test suite is the regression net — it must stay green untouched except for the three lib tests that construct the lifted state directly and two serde round-trip tests.

**Tech Stack:** Rust, `cargo test`/`clippy`/`fmt`, `serde`/`serde_json`.

## Global Constraints

- Crate layering: `game-core` is the kernel; no I/O, no async, compiles to `wasm32`. This work is entirely within `crates/game-core`.
- CI is warnings-as-errors across `fmt`, `clippy`, `test`, `doc`. Match the strict flags locally before pushing (see `CLAUDE.md` Commands):
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
- Doc-comments: never document an absent derive; intra-doc links must resolve (the `doc` job fails on broken links).
- Behaviour-preserving: no change to attack-resolution behaviour. Every existing test that is not explicitly rewritten below must pass unchanged.
- Commit-message footer (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
  ```

---

## Task 1: Rename `AttackLoopPhase` → `AttackLoopStage`

Pure mechanical rename — the enum, its `phase` field on `PendingEnemyAttack`, and every reference. "Phase" is a load-bearing game concept (Mythos/Investigation/Enemy/Upkeep); `AttackLoopPhase` collides with it. `Stage` is unused (`PhaseStep` already owns "step").

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (enum def ~345-365, `PendingEnemyAttack.phase` field ~388-400)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`park_on_soak_window` ~739, `drive_attack_loop` ~807, `resume_enemy_attack` ~852-882, three lib tests ~1135/1168/1211/1235/1266/1290, plus doc-comments referencing `AttackLoopPhase`)

**Interfaces:**
- Produces: `AttackLoopStage { BeforeAttack, AfterSoak }` (was `AttackLoopPhase`); `PendingEnemyAttack.stage` (was `.phase`). Task 2 consumes these names.

- [ ] **Step 1: Rename the enum and its doc-comment**

In `crates/game-core/src/state/game_state.rs`, rename the type and update the doc-comment's `[\`AttackLoopPhase::…\`]` intra-doc links:

```rust
/// Which point in the per-attacker sequence a parked enemy-attack loop
/// suspended at (Axis D #336).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackLoopStage {
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

- [ ] **Step 2: Rename the `phase` field on `PendingEnemyAttack`**

Same file, the `PendingEnemyAttack` struct. Update both the field and the two `[\`AttackLoopPhase::…\`]` references in its doc-comments to `[\`AttackLoopStage::…\`]`:

```rust
    /// The current attacker is still at the head for
    /// [`AttackLoopStage::BeforeAttack`] (it has not dealt yet); already
    /// removed for [`AttackLoopStage::AfterSoak`].
    pub remaining_attackers: Vec<EnemyId>,
    /// Which loop to re-enter.
    pub source: EnemyAttackSource,
    /// Where in the per-attacker sequence the loop suspended (Axis D #336).
    pub stage: AttackLoopStage,
```

- [ ] **Step 3: Update all references in `combat.rs`**

In `crates/game-core/src/engine/dispatch/combat.rs`: replace `AttackLoopPhase` → `AttackLoopStage` and the `phase:` struct field → `stage:` everywhere. Specific sites:
- `park_on_soak_window`: `phase: AttackLoopPhase::AfterSoak` → `stage: AttackLoopStage::AfterSoak`
- `drive_attack_loop`: `phase: AttackLoopPhase::BeforeAttack` → `stage: AttackLoopStage::BeforeAttack`
- `resume_enemy_attack`: the destructure `phase,` → `stage,` and the doc-comment's `[\`AttackLoopPhase\`]` / `[\`AttackLoopPhase::BeforeAttack\`]` / `[\`AttackLoopPhase::AfterSoak\`]` links → `AttackLoopStage` equivalents; the comparison `if phase == AttackLoopPhase::BeforeAttack` → `if stage == AttackLoopStage::BeforeAttack`
- the three lib tests (`resume_enemy_attack_drains_…`, `resume_before_attack_cancel_…`, `resume_before_attack_without_cancel_…`): the `use crate::state::{ AttackLoopPhase, … }` imports → `AttackLoopStage`, and each `phase: AttackLoopPhase::…` → `stage: AttackLoopStage::…`

Run a sanity grep — there should be zero matches left:

```bash
grep -rn "AttackLoopPhase\|phase: AttackLoop\|\.phase\b" crates/game-core/src/engine/dispatch/combat.rs
```

- [ ] **Step 4: Run the rename regression**

The whole point is no behaviour change, so the existing tests are the test. Run:

```bash
grep -rn "AttackLoopPhase" crates/game-core/src   # expect: no output
RUSTFLAGS="-D warnings" cargo test -p game-core
```
Expected: no `AttackLoopPhase` matches; all `game-core` tests pass.

- [ ] **Step 5: Lint + commit**

```bash
cargo fmt
cargo clippy -p game-core --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: rename AttackLoopPhase to AttackLoopStage (#411)

\"Phase\" is a load-bearing game concept (Mythos/Investigation/Enemy/Upkeep);
the attack-loop sub-cursor enum collided with it. Pure rename, no behaviour
change.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## Task 2: Lift `pending_enemy_attack` → `Continuation::AttackLoop`

Replace the `GameState::pending_enemy_attack` side-field with a stack frame. Push it where the code today sets the field to `Some(...)` (immediately beneath the reaction window the next call opens above it); pop it where `resume_enemy_attack` today calls `.take()`.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (remove `PendingEnemyAttack` struct + `pending_enemy_attack` field; add `Continuation::AttackLoop` variant; wire `awaits_input` / `as_resolution` / `as_resolution_mut`; repoint the `pending_enemy_attack` serde test)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`park_on_soak_window`, `drive_attack_loop`, `resume_enemy_attack`, the three lib tests)
- Modify: `crates/game-core/src/state/builder.rs` (drop the `pending_enemy_attack: None` initialiser)

**Interfaces:**
- Consumes: `AttackLoopStage`, `EnemyAttackSource` (Task 1).
- Produces: `Continuation::AttackLoop { investigator: InvestigatorId, remaining_attackers: Vec<EnemyId>, source: EnemyAttackSource, stage: AttackLoopStage }`. A helper to pop it: `GameState`-level the resume reads `continuations.pop()`. Task 3 leaves this untouched.

- [ ] **Step 1: Add the `AttackLoop` variant to `Continuation`**

In `crates/game-core/src/state/game_state.rs`, add the variant to `enum Continuation` (place it after `InvestigatorTurn`, before the closing brace). Move the doc-prose from the deleted `PendingEnemyAttack` struct onto it:

```rust
    /// A parked enemy-attack loop, suspended because an attack opened a reaction
    /// window — either the soak window (`AfterEnemyAttackDamagedAsset`, after
    /// damage; C5b #237) or the before-attack cancel window (`BeforeEnemyAttack`,
    /// before damage; Axis D #336), distinguished by [`Self::stage`]. Pushed
    /// *beneath* that reaction window by the attack-loop driver; resumed by
    /// [`resume_enemy_attack`](crate::engine) (which pops it) once the window
    /// closes. An internal sequencing frame — never awaits player input itself
    /// (the window above it does). (#411, step 3 of #393.)
    AttackLoop {
        /// The investigator whose engaged enemies are attacking.
        investigator: InvestigatorId,
        /// Attackers not yet resolved, in resolution order. The current attacker
        /// is still at the head for [`AttackLoopStage::BeforeAttack`] (it has not
        /// dealt yet); already removed for [`AttackLoopStage::AfterSoak`].
        remaining_attackers: Vec<EnemyId>,
        /// Which loop to re-enter.
        source: EnemyAttackSource,
        /// Where in the per-attacker sequence the loop suspended (Axis D #336).
        stage: AttackLoopStage,
    },
```

- [ ] **Step 2: Wire `AttackLoop` into the three `Continuation` methods**

Same file, `impl Continuation`:

`awaits_input` — add an explicit arm returning `false` (internal sequencing frame), placed before the `other =>` catch-all so the catch-all does not classify it as a prompt:

```rust
            // The parked attack loop is internal sequencing: the reaction window
            // pushed above it is the player-facing prompt, not this frame. It is
            // only ever momentarily on top inside `resume_enemy_attack` (between
            // the window pop and its own pop), never at a suspension boundary.
            Continuation::AttackLoop { .. } => false,
```

`as_resolution` and `as_resolution_mut` — add `Continuation::AttackLoop { .. }` to each `None`-returning match list (next to `InvestigatorTurn { .. }`).

- [ ] **Step 3: Delete the `PendingEnemyAttack` struct and the state field**

Same file: delete the entire `pub struct PendingEnemyAttack { … }` (its doc-prose moved to the variant in Step 1) and delete the `pub pending_enemy_attack: Option<PendingEnemyAttack>,` field (with its `#[serde(default)]` and doc-comment) from `GameState`.

- [ ] **Step 4: Repoint the `pending_enemy_attack` serde test**

Same file, in `mod enemy_attack_pending_tests`, replace the `pending_enemy_attack`-specific coverage. (The `enemy_attack_pending` tests in this module are handled in Task 3; here only add the new `AttackLoop` round-trip — there is no existing `pending_enemy_attack` serde test to delete, so this is purely additive.) Add:

```rust
    #[test]
    fn attack_loop_frame_round_trips_through_serde() {
        use crate::state::{
            AttackLoopStage, Continuation, EnemyAttackSource, EnemyId,
        };
        let mut state = GameStateBuilder::new().build();
        state.continuations.push(Continuation::AttackLoop {
            investigator: InvestigatorId(7),
            remaining_attackers: vec![EnemyId(2), EnemyId(3)],
            source: EnemyAttackSource::EnemyPhase,
            stage: AttackLoopStage::AfterSoak,
        });
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.continuations, state.continuations);
    }
```

- [ ] **Step 5: Drop the builder initialiser**

In `crates/game-core/src/state/builder.rs`, delete the line `pending_enemy_attack: None,` (keep `enemy_attack_pending: None,` for now — Task 3 removes it).

- [ ] **Step 6: Push the frame at the two park sites in `combat.rs`**

In `crates/game-core/src/engine/dispatch/combat.rs`:

`park_on_soak_window` — replace the `cx.state.pending_enemy_attack = Some(PendingEnemyAttack { … });` assignment with a push (the `open_queued_reaction_window` call right after stays):

```rust
    cx.state.continuations.push(crate::state::Continuation::AttackLoop {
        investigator,
        remaining_attackers,
        source,
        stage: AttackLoopStage::AfterSoak,
    });
    super::reaction_windows::open_queued_reaction_window(cx)
```

`drive_attack_loop` — replace the before-cancel `cx.state.pending_enemy_attack = Some(PendingEnemyAttack { … });` assignment with a push:

```rust
        if !cx.state.open_windows().is_empty() {
            cx.state.continuations.push(crate::state::Continuation::AttackLoop {
                investigator,
                remaining_attackers: attackers,
                source,
                stage: AttackLoopStage::BeforeAttack,
            });
            return super::reaction_windows::open_queued_reaction_window(cx);
        }
```

Update the `use` line at the top of `combat.rs` (it imports `PendingEnemyAttack`) — drop `PendingEnemyAttack`, keep `InvestigatorId, Status` etc. (`AttackLoopStage`/`EnemyAttackSource` are referenced via `crate::state::` or already imported in scope as before).

- [ ] **Step 7: Pop the frame in `resume_enemy_attack`**

Replace the `let PendingEnemyAttack { … } = cx.state.pending_enemy_attack.take().unwrap_or_else(…)` destructure with a pop of the top frame, asserting it is an `AttackLoop`:

```rust
pub(super) fn resume_enemy_attack(cx: &mut Cx) -> EngineOutcome {
    let Some(crate::state::Continuation::AttackLoop {
        investigator,
        mut remaining_attackers,
        source,
        stage,
    }) = cx.state.continuations.pop()
    else {
        unreachable!(
            "resume_enemy_attack: top frame is not an AttackLoop; the \
             soak / before-attack continuations only fire after the attack \
             loop pushed one — state-corruption invariant violation"
        )
    };
```

Then update the doc-comment of `resume_enemy_attack`: it currently says "Takes the parked [`PendingEnemyAttack`] …" — change to "Pops the parked [`Continuation::AttackLoop`] frame …", and the `[\`GameState::pending_enemy_attack\`]` reference in `process_attacker_dealing`'s doc (step 4, ~643) → "the attack-loop driver pushes a [`Continuation::AttackLoop`] frame".

- [ ] **Step 8: Rewrite the three lib tests to push the frame instead of setting the field**

Each test currently does `state.pending_enemy_attack = Some(PendingEnemyAttack { … });`. The frame must sit on top of the EnemyPhase anchor (the real stack shape after the window closed and was popped). Replace each with a `continuations.push`. For `resume_enemy_attack_drains_remaining_attackers_and_advances_cursor`:

```rust
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, InvestigatorId};
        // … builder unchanged through .build() …
        // The enemy phase set the cursor to this investigator before opening
        // the BeforeInvestigatorAttacked window; resume must advance it.
        state.enemy_attack_pending = Some(inv_id);
        state.continuations.push(Continuation::AttackLoop {
            investigator: inv_id,
            remaining_attackers: vec![second, third],
            source: EnemyAttackSource::EnemyPhase,
            stage: AttackLoopStage::AfterSoak,
        });
```

and the post-condition `assert!(state.pending_enemy_attack.is_none(), …)` becomes:

```rust
        assert!(
            !state.continuations.iter().any(|c| matches!(c, Continuation::AttackLoop { .. })),
            "resume consumed the parked attack loop frame"
        );
```

Apply the analogous change to `resume_before_attack_cancel_skips_damage_but_exhausts` (`stage: AttackLoopStage::BeforeAttack`, `remaining_attackers: vec![attacker]`) and `resume_before_attack_without_cancel_deals_damage` (same). These two do not assert on `pending_enemy_attack`, so only the push and the `use` import change. Leave the `state.enemy_attack_pending = Some(inv_id);` lines for Task 3.

- [ ] **Step 9: Run the lift regression**

```bash
grep -rn "pending_enemy_attack\|PendingEnemyAttack" crates/game-core/src   # expect: no output
RUSTFLAGS="-D warnings" cargo test -p game-core
```
Expected: no matches; all `game-core` tests pass (the cross-crate enemy-attack integration tests in `crates/cards/tests/` are exercised by the full run in Task 3's gauntlet, but run them now too if convenient: `RUSTFLAGS="-D warnings" cargo test --all`).

- [ ] **Step 10: Lint + commit**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
git add crates/game-core/src/state/game_state.rs crates/game-core/src/state/builder.rs crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: lift pending_enemy_attack onto a Continuation::AttackLoop frame (#411)

The parked enemy-attack loop now lives on the continuation stack beneath the
reaction window it suspended on, instead of in a side-field. Behaviour-
preserving: pushed where the field was set to Some, popped where it was taken.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## Task 3: Lift `enemy_attack_pending` → the `EnemyPhase` anchor's `attacking` field

Move the per-investigator cursor onto the anchor. A nullable field (not an `EnemyResume::AttacksFor(_)` payload) because the anchor exists before an investigator is selected (`enemy_phase` pushes it ahead of hunter movement).

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add `attacking` field to `Continuation::EnemyPhase`; remove `enemy_attack_pending` field; repoint its serde + default tests)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (`enemy_phase` push, `set_enemy_anchor` mutator, `enemy_attack_kickoff`, the `BeforeInvestigatorAttacked` anchor arm, the transition push in `advance_phase_entry`'s caller, the `Entry` push sites, tests)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`after_enemy_phase_attacks`)
- Modify: `crates/game-core/src/state/builder.rs` (drop `enemy_attack_pending: None`)
- Modify: `crates/game-core/src/engine/dispatch/cursor.rs` (doc-comment reference)

**Interfaces:**
- Consumes: `Continuation::EnemyPhase`, `EnemyResume` (existing).
- Produces: `Continuation::EnemyPhase { resume: EnemyResume, attacking: Option<InvestigatorId> }`; `phases::set_enemy_anchor(cx: &mut Cx, resume: EnemyResume, attacking: Option<InvestigatorId>)`.

- [ ] **Step 1: Add the `attacking` field to `Continuation::EnemyPhase`**

In `crates/game-core/src/state/game_state.rs`:

```rust
    /// The Enemy phase anchor (slice 1a, #393). See [`Continuation::MythosPhase`].
    EnemyPhase {
        /// Which child-pop boundary the anchor resumes at.
        resume: EnemyResume,
        /// The investigator whose engaged enemies are currently attacking
        /// (Enemy step 3.3), or `None` before kickoff / after the last
        /// investigator. The per-investigator cursor, lifted off the former
        /// `GameState::enemy_attack_pending` (#411, step 3 of #393).
        attacking: Option<InvestigatorId>,
    },
```

The `impl Continuation` matches that destructure `EnemyPhase { .. }` (`is_phase_anchor`, `as_resolution`, `as_resolution_mut`) already use `..`, so they need no change. Verify with a build after this step.

- [ ] **Step 2: Remove the `enemy_attack_pending` field**

Same file: delete `pub enemy_attack_pending: Option<InvestigatorId>,` and its long doc-comment from `GameState`.

- [ ] **Step 3: Repoint the `enemy_attack_pending` tests**

Same file, `mod enemy_attack_pending_tests`. The default test asserted the field is `None`; the round-trip set/read it. Replace both with anchor-`attacking` equivalents:

```rust
    #[test]
    fn enemy_phase_anchor_attacking_round_trips_through_serde() {
        use crate::state::{Continuation, EnemyResume};
        let mut state = GameStateBuilder::new().build();
        state.continuations.push(Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(InvestigatorId(7)),
        });
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.continuations, state.continuations);
    }
```

(Delete `game_state_default_has_no_enemy_attack_pending` and `enemy_attack_pending_round_trips_through_serde`; the `attack_loop_frame_round_trips_through_serde` from Task 2 stays.)

- [ ] **Step 4: Drop the builder initialiser**

In `crates/game-core/src/state/builder.rs`, delete `enemy_attack_pending: None,`.

- [ ] **Step 5: Replace `set_enemy_anchor_resume` with `set_enemy_anchor`**

In `crates/game-core/src/engine/dispatch/phases.rs`, change the helper to set both fields:

```rust
/// Set the Enemy phase anchor's `resume` and `attacking` cursor (slice 1a /
/// #411) before opening one of its attack windows, so the window's close routes
/// to the matching `anchor_on_child_pop` body for the right investigator. The
/// anchor is the bottom-most Enemy frame; a no-op if it is absent (only in tests
/// that drive the attack loop in isolation).
pub(super) fn set_enemy_anchor(
    cx: &mut Cx,
    resume: crate::state::EnemyResume,
    attacking: Option<InvestigatorId>,
) {
    if let Some(c) = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find(|c| matches!(c, crate::state::Continuation::EnemyPhase { .. }))
    {
        *c = crate::state::Continuation::EnemyPhase { resume, attacking };
    }
}
```

- [ ] **Step 6: Update `enemy_attack_kickoff`**

Same file. Replace the `enemy_attack_pending` reads/writes with a local + `set_enemy_anchor`:

```rust
pub(super) fn enemy_attack_kickoff(cx: &mut Cx) -> EngineOutcome {
    let first = super::cursor::first_active_investigator(cx.state);

    if let Some(inv) = first {
        set_enemy_anchor(
            cx,
            crate::state::EnemyResume::BeforeInvestigatorAttacked,
            Some(inv),
        );
        super::reaction_windows::open_fast_window(
            cx,
            WindowKind::PlayerWindow(PhaseStep::BeforeInvestigatorAttacked),
        )
    } else {
        set_enemy_anchor(cx, crate::state::EnemyResume::AfterAllAttacked, None);
        super::reaction_windows::open_fast_window(
            cx,
            WindowKind::PlayerWindow(PhaseStep::AfterAllInvestigatorsAttacked),
        )
    }
}
```

- [ ] **Step 7: Update the `enemy_phase` anchor push**

Same file, in `enemy_phase` (~627): the push of the running anchor gains `attacking: None` (kickoff sets it after hunter movement):

```rust
    cx.state
        .continuations
        .push(crate::state::Continuation::EnemyPhase {
            resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
            attacking: None,
        });
```

- [ ] **Step 8: Update the two `Entry` push sites**

Same file, the transition push (~422-425) and the start-of-phase push (~522-523) construct `EnemyPhase { resume: EnemyResume::Entry }`. Add `attacking: None` to both.

- [ ] **Step 9: Update the `BeforeInvestigatorAttacked` anchor arm**

Same file, in `anchor_on_child_pop` (~811). The match arm reads the investigator from the bound `attacking` field instead of the state cursor:

```rust
        Some(Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking,
        }) => {
            debug_assert!(
                cx.state.current_skill_test().is_none(),
                "BeforeInvestigatorAttacked advanced with a skill test in flight",
            );
            // Cursor expect-Some: BeforeInvestigatorAttacked is only ever opened
            // after the anchor's `attacking` cursor is set to Some(_). A None
            // here is a state-corruption invariant violation.
            let investigator = attacking.unwrap_or_else(|| {
                unreachable!(
                    "BeforeInvestigatorAttacked closed with anchor.attacking \
                     == None; state-corruption invariant violation"
                )
            });
            let outcome = super::combat::resolve_attacks_for_investigator(cx, investigator);
            if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                return outcome;
            }
            debug_assert!(
                matches!(outcome, EngineOutcome::Done),
                "resolve_attacks_for_investigator returned unexpected {outcome:?}",
            );
            super::reaction_windows::after_enemy_phase_attacks(cx, investigator)
        }
```

The sibling `AfterAllAttacked` arm matches `EnemyPhase { resume: EnemyResume::AfterAllAttacked }` — add `attacking: _` (or `..`) so it still compiles.

- [ ] **Step 10: Update `after_enemy_phase_attacks`**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs` (~1070). Replace the cursor read/write with a local + `set_enemy_anchor`:

```rust
pub(super) fn after_enemy_phase_attacks(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let next = super::cursor::next_active_investigator_after(cx.state, investigator);

    if let Some(inv) = next {
        super::phases::set_enemy_anchor(
            cx,
            crate::state::EnemyResume::BeforeInvestigatorAttacked,
            Some(inv),
        );
        open_fast_window(
            cx,
            WindowKind::PlayerWindow(PhaseStep::BeforeInvestigatorAttacked),
        )
    } else {
        super::phases::set_enemy_anchor(cx, crate::state::EnemyResume::AfterAllAttacked, None);
        open_fast_window(
            cx,
            WindowKind::PlayerWindow(PhaseStep::AfterAllInvestigatorsAttacked),
        )
    }
}
```

Update its doc-comment: the `[\`GameState::enemy_attack_pending\`]` references → "the `EnemyPhase` anchor's `attacking` cursor".

- [ ] **Step 11: Update the `cursor.rs` doc-comment**

In `crates/game-core/src/engine/dispatch/cursor.rs` (~48): the comment `Enemy 3.3 attacks ([\`enemy_phase\`] seeds \`enemy_attack_pending\`).` → `Enemy 3.3 attacks ([\`enemy_phase\`] seeds the EnemyPhase anchor's \`attacking\` cursor).`

- [ ] **Step 12: Fix the tests that touch `enemy_attack_pending`**

The three combat lib tests (Task 2 left their `state.enemy_attack_pending = Some(inv_id);` lines and their `.with_phase_anchor(EnemyPhase { resume: BeforeInvestigatorAttacked })`):
- delete the `state.enemy_attack_pending = Some(inv_id);` line in each;
- change each `.with_phase_anchor(crate::state::Continuation::EnemyPhase { resume: crate::state::EnemyResume::BeforeInvestigatorAttacked })` to add `attacking: Some(inv_id)`;
- in `resume_enemy_attack_drains_…`, the post-condition `assert!(state.enemy_attack_pending.is_none(), "cursor advanced past the sole investigator to None")` becomes an assertion that no `EnemyPhase` anchor is still pointing at an investigator:

```rust
        assert!(
            !state.continuations.iter().any(|c| matches!(
                c,
                Continuation::EnemyPhase { attacking: Some(_), .. }
            )),
            "cursor advanced past the sole investigator (no anchor still attacking)"
        );
```

In `crates/game-core/src/engine/dispatch/phases.rs` tests: grep for `enemy_attack_pending` and `EnemyPhase {` constructions and fix each:
- `enemy_phase_attack_lands_in_full_cascade` (~3463) asserts `state.enemy_attack_pending == None, "cursor cleared at end"` → after the cascade the EnemyPhase anchor is popped, so assert no `EnemyPhase` anchor remains: `assert!(!result.state.continuations.iter().any(|c| matches!(c, crate::state::Continuation::EnemyPhase { .. })), "enemy anchor popped at phase end");`
- `enemy_phase_resumes_via_skip_input` (~3711) `.with_phase_anchor(EnemyPhase { resume: BeforeInvestigatorAttacked })` and a `state.enemy_attack_pending = Some(inv_id)` (~3719) → add `attacking: Some(inv_id)` to the anchor and delete the cursor line; its post-assert (~3759) `result.state.enemy_attack_pending == None` → the no-`EnemyPhase`-anchor assertion above (or `attacking: Some(_)` check, per what the test pins).

Run the grep to be exhaustive:

```bash
grep -rn "enemy_attack_pending" crates/game-core/src   # expect: no output after all edits
```

- [ ] **Step 13: Run the full regression**

```bash
grep -rn "enemy_attack_pending\|set_enemy_anchor_resume" crates/game-core/src   # expect: no output
RUSTFLAGS="-D warnings" cargo test --all --all-features
```
Expected: no matches; the whole workspace test suite passes (including `crates/cards/tests/` enemy-attack integration tests).

- [ ] **Step 14: Full local CI gauntlet**

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all clean. (`wasm-pack test` is browser-gated; the engine change is platform-agnostic, so the wasm build + clippy cover the wasm jobs.)

- [ ] **Step 15: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/state/builder.rs crates/game-core/src/engine/dispatch/phases.rs crates/game-core/src/engine/dispatch/reaction_windows.rs crates/game-core/src/engine/dispatch/cursor.rs
git commit -m "engine: lift enemy_attack_pending onto the EnemyPhase anchor's attacking field (#411)

The per-investigator enemy-attack cursor now lives on the EnemyPhase anchor
(serializing with the frame) instead of a standalone GameState field. A
nullable field, not an EnemyResume payload, because the anchor exists before
an investigator is selected (pushed ahead of hunter movement). Closes the
last two framework cursors the #393 model targets.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## Task 4: PR + phase-doc update

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md` (final commit, after CI green)

- [ ] **Step 1: Push the branch and open the PR**

```bash
git push -u origin engine/attack-loop-frame
gh pr create --fill   # use the repo template; Closes #411; design-decisions paragraph
```
The PR body's design paragraph: the Shape-A cursor lift (parked-only `AttackLoop` frame + anchor `attacking` field), and the explicit deferral of the frame's full per-investigator span to the keystone slice (step 4).

- [ ] **Step 2: Watch CI**

```bash
gh pr checks <PR#> --watch
```
Fix any failures with follow-up commits to the same branch (do not amend/force-push).

- [ ] **Step 3: Phase-doc update (final commit, only once CI is green)**

In `docs/phases/phase-7-the-gathering.md`, per `docs/phases/README.md` "Maintaining these docs":
- move #411 to the Closed table (bump counts);
- in the Ordering section, mark step 3 (`AttackLoop` frame cursor lift) shipped with the PR #;
- add a **Decisions made** entry **only if** it passes the future-PR-author test — here it does: record that step 3 lifted *only* the parked suspension (Shape A); the `AttackLoop` frame's full per-investigator span and the attacker-snapshot-timing decision are **deferred to the keystone (step 4)**, which must extend the frame when it parks the triggering action. This is the deferral the user asked to be captured.

Commit:

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — close #411, note AttackLoop full-span deferral to keystone

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
git push
```

- [ ] **Step 4: Merge only after explicit user approval**

```bash
gh pr merge <PR#> --squash --delete-branch
git checkout main && git pull
```
Confirm #411 auto-closed.

---

## Self-review notes

- **Spec coverage:** §1 rename → Task 1. §2 AttackLoop frame → Task 2. §3 cursor→anchor field → Task 3. Testing section → the regression runs in each task + the gauntlet in Task 3 Step 14. Phase-doc deferral → Task 4 Step 3.
- **Type consistency:** `AttackLoopStage`/`stage` (Task 1) used in Task 2's variant; `Continuation::AttackLoop { investigator, remaining_attackers, source, stage }` consistent across push (Task 2 Step 6), pop (Step 7), tests (Step 8); `set_enemy_anchor(cx, resume, attacking)` consistent across kickoff (Task 3 Step 6) and after_enemy_phase_attacks (Step 10); `EnemyPhase { resume, attacking }` consistent across all push/match sites.
- **Behaviour-preserving:** no resolution logic changes; only where parked state lives. Existing tests are the net; only state-construction tests and serde tests change.
