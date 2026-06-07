# WindowKind PlayerWindow Collapse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse the six payload-less marker `WindowKind` variants into `WindowKind::PlayerWindow(PhaseStep)`, keep the event-carrying `AfterEnemyDefeated` distinct, and delete the dead `BetweenPhases` variant.

**Architecture:** Behavior-preserving refactor of one enum in `game-core`. `WindowKind` shrinks to two variants; a new sibling enum `PhaseStep` enumerates the six timing points. Continuation dispatch and `trigger_matches` re-key on `PhaseStep` instead of variant name. Dead `BetweenPhases` test fixtures migrate to `PhaseStep::InvestigatorTurnBegins` (same payload-less, `Done`-continuation shape).

**Tech Stack:** Rust, `serde`. No new dependencies.

**Refactor note — no new tests, existing suite is the contract.** This change adds zero behavior. The verification at every step is "the existing suite stays green under CI-strict flags." The enum change is atomic within `game-core` (removing variants breaks every match + construction site at once), so Task 1 covers all of `game-core` and is verified by `cargo test -p game-core`; Task 2 covers the `cards` crate fixtures; Task 3 runs the full CI gauntlet.

Spec: `docs/superpowers/specs/2026-06-07-140-windowkind-playerwindow-collapse-design.md`.

---

### Task 1: Restructure `WindowKind` + migrate all of `game-core`

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (enum definition ~556–633, serde tests ~841–898)
- Modify: `crates/game-core/src/state/mod.rs` (re-export, line ~19–22)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`trigger_matches` ~142–188, `run_window_continuation` ~529–663, construction sites ~614/616, in-mod tests ~1100–1145)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (construction sites ~143/163/223/326/331/425; in-mod test assertions ~762/820/1044/1050/1117/1126/1459)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (construction site ~605)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (doc-link references only)
- Modify: `crates/game-core/src/event.rs` (import ~21; serde test ~489–494)
- Modify: `crates/game-core/src/test_support/builder.rs` (import ~32; `with_open_window` tests ~317–354)
- Modify: `crates/game-core/tests/reaction_windows.rs` (import ~27; empty-window-on-stack test ~885–944)

- [ ] **Step 1: Replace the enum definition.**

In `crates/game-core/src/state/game_state.rs`, replace the entire `WindowKind` enum (the `#[non_exhaustive] pub enum WindowKind { ... }` block, currently variants `AfterEnemyDefeated`, `BetweenPhases`, `MythosAfterDraws`, `UpkeepBegins`, `BeforeInvestigatorAttacked`, `AfterAllInvestigatorsAttacked`, `InvestigationBegins`, `InvestigatorTurnBegins`) with the two enums below. Move each marker's existing Rules-Reference doc-comment verbatim onto the matching `PhaseStep` variant. Delete `BetweenPhases`'s doc-comment entirely. Keep the `AfterEnemyDefeated` doc-comment and the enum-level doc-comment on `WindowKind`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WindowKind {
    /// Fires after an enemy was defeated. Pairs with
    /// [`EventPattern::EnemyDefeated`](crate::dsl::EventPattern::EnemyDefeated)
    /// with [`EventTiming::After`](crate::dsl::EventTiming::After).
    AfterEnemyDefeated {
        /// The defeated enemy. Carried so trigger effects keying on
        /// "the defeated enemy" can route against the right id even
        /// after `state.enemies` has dropped the entry.
        enemy: EnemyId,
        /// Who defeated it, if attributable. Mirrors the
        /// [`Event::EnemyDefeated`](crate::Event::EnemyDefeated)
        /// `by` field. `None` for non-investigator-attributed defeats.
        by: Option<InvestigatorId>,
    },
    /// A printed player window at a Rules-Reference timing step. Carries
    /// no event payload — these windows gate Fast actions (and run a
    /// per-step continuation when they close), they are not after-event
    /// reaction windows. The specific timing point is the [`PhaseStep`].
    PlayerWindow(PhaseStep),
}

/// The Rules-Reference timing step a [`WindowKind::PlayerWindow`] sits
/// at. Each step uniquely determines its phase, so the phase is not
/// carried separately (the engine reads [`GameState::phase`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PhaseStep {
    /// <MOVE the MythosAfterDraws doc-comment body here>
    MythosAfterDraws,
    /// <MOVE the UpkeepBegins doc-comment body here>
    UpkeepBegins,
    /// <MOVE the BeforeInvestigatorAttacked doc-comment body here>
    BeforeInvestigatorAttacked,
    /// <MOVE the AfterAllInvestigatorsAttacked doc-comment body here>
    AfterAllInvestigatorsAttacked,
    /// <MOVE the InvestigationBegins doc-comment body here>
    InvestigationBegins,
    /// <MOVE the InvestigatorTurnBegins doc-comment body here>
    InvestigatorTurnBegins,
}
```

When moving doc-comments, update intra-doc links *within them* that point at sibling markers: `[`MythosAfterDraws`]: WindowKind::MythosAfterDraws` becomes `[`MythosAfterDraws`]: PhaseStep::MythosAfterDraws` (e.g. the link definitions on the `BeforeInvestigatorAttacked` / `AfterAllInvestigatorsAttacked` doc-comments).

- [ ] **Step 2: Re-export `PhaseStep`.**

In `crates/game-core/src/state/mod.rs`, add `PhaseStep` to the `pub use game_state::{ ... }` list (alphabetical neighborhood near `Phase`).

- [ ] **Step 3: Mechanical rename across all `game-core` markers.**

Apply this rule everywhere in `crates/game-core/src/` (code *and* doc-comments), for the six markers `MythosAfterDraws`, `UpkeepBegins`, `BeforeInvestigatorAttacked`, `AfterAllInvestigatorsAttacked`, `InvestigationBegins`, `InvestigatorTurnBegins`:

  - **Construction:** `WindowKind::<Marker>` → `WindowKind::PlayerWindow(PhaseStep::<Marker>)`.
    - Sites: `encounter.rs` (`MythosAfterDraws`); `phases.rs` (`InvestigationBegins`, `InvestigatorTurnBegins`, `MythosAfterDraws`, `BeforeInvestigatorAttacked`, `AfterAllInvestigatorsAttacked`, `UpkeepBegins`); `reaction_windows.rs` lines ~614/616 (`BeforeInvestigatorAttacked`, `AfterAllInvestigatorsAttacked`).
  - **Pattern match** `kind: WindowKind::<Marker>` (in `#[cfg(test)]` assertions in `phases.rs` and `reaction_windows.rs`) → `kind: WindowKind::PlayerWindow(PhaseStep::<Marker>)`.
  - **Doc-link** `[`...`](WindowKind::<Marker>)` and `[\`WindowKind::<Marker>\`]` → point at `PhaseStep::<Marker>`.

  Leave `WindowKind::AfterEnemyDefeated` and the new `WindowKind::PlayerWindow` untouched by this rule.

- [ ] **Step 4: Rewrite `trigger_matches`.**

In `reaction_windows.rs`, replace the six-name false-arm with a single `PlayerWindow` arm. The `match (kind, pattern)` becomes:

```rust
    match (kind, pattern) {
        (
            WindowKind::AfterEnemyDefeated { by, .. },
            EventPattern::EnemyDefeated { by_controller },
        ) => {
            if by_controller {
                by == Some(controller)
            } else {
                true
            }
        }
        // PlayerWindow steps open for timing reasons; no
        // Trigger::OnEvent pattern matches them — those windows gate
        // Fast actions, not after-event reactions. AfterEnemyDefeated
        // windows only match EnemyDefeated patterns (handled above);
        // encounter-reveal / spawn patterns return false.
        (
            WindowKind::PlayerWindow(_) | WindowKind::AfterEnemyDefeated { .. },
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned,
        ) => false,
    }
```

- [ ] **Step 5: Rewrite `run_window_continuation`.**

In `reaction_windows.rs`, restructure the top-level `match kind` so the six step arms nest under `PlayerWindow(step)`. Keep every arm body verbatim (including the `unreachable!` skill-test-in-flight guards and the cursor/attack logic). The shape:

```rust
pub(super) fn run_window_continuation(cx: &mut Cx, kind: WindowKind) -> EngineOutcome {
    match kind {
        WindowKind::PlayerWindow(step) => match step {
            PhaseStep::MythosAfterDraws => { /* existing MythosAfterDraws body */ }
            PhaseStep::UpkeepBegins => { /* existing UpkeepBegins body */ }
            PhaseStep::BeforeInvestigatorAttacked => { /* existing body */ }
            PhaseStep::AfterAllInvestigatorsAttacked => { /* existing body */ }
            PhaseStep::InvestigationBegins => { /* existing body */ }
            PhaseStep::InvestigatorTurnBegins => EngineOutcome::Done,
        },
        WindowKind::AfterEnemyDefeated { .. } => EngineOutcome::Done,
    }
}
```

The old combined `AfterEnemyDefeated | BetweenPhases | InvestigatorTurnBegins => Done` arm is gone: `InvestigatorTurnBegins` is now a `PlayerWindow` step returning `Done`, `AfterEnemyDefeated` keeps its own `Done` arm, and `BetweenPhases` no longer exists. Update the function's doc-comment to drop the `BetweenPhases` mention and refer to `PhaseStep::…` for the step names.

- [ ] **Step 6: Migrate the `BetweenPhases` serde test in `game_state.rs`.**

In the `open_window_tests` mod, delete the five separate per-marker round-trip tests (`mythos_after_draws_window_kind_serde_roundtrip`, `upkeep_begins_…`, `before_investigator_attacked_…`, `after_all_investigators_attacked_…`, `investigation_begins_…`, `investigator_turn_begins_…`) and the `between_phases_window_kind_serde_roundtrip` test, replacing them with one representative `PlayerWindow` round-trip. Keep `open_window_serde_roundtrip` (the `AfterEnemyDefeated` one) as-is.

```rust
    #[test]
    fn player_window_kind_serde_roundtrip() {
        let kind = WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws);
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }
```

- [ ] **Step 7: Migrate the `event.rs` serde test.**

In `crates/game-core/src/event.rs`, the `#[cfg(test)]` test (~489–494) that builds a `WindowKind::BetweenPhases { from, to }`: change it to `WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws)`. Update the test's `use crate::state::{...}` line to import `PhaseStep` instead of `Phase` if `Phase` is now unused there.

- [ ] **Step 8: Migrate the `builder.rs` dead fixtures.**

In `crates/game-core/src/test_support/builder.rs`, the `with_open_window_tests` mod uses `WindowKind::BetweenPhases { from, to }` as a generic Fast-window stand-in. Replace both uses with `WindowKind::PlayerWindow(PhaseStep::InvestigatorTurnBegins)`. The `with_open_window_stacks_in_order` test's `matches!(... WindowKind::BetweenPhases { to: Phase::Enemy, .. })` assertion must change to distinguish the two pushed windows another way — push two *different* steps (e.g. `MythosAfterDraws` then `InvestigatorTurnBegins`) and assert the top is `WindowKind::PlayerWindow(PhaseStep::InvestigatorTurnBegins)`. Update the `use` import to add `PhaseStep` and drop `Phase` if it becomes unused in that test mod.

```rust
    #[test]
    fn with_open_window_stacks_in_order() {
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_open_window(
                WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws),
                FastActorScope::Any,
            )
            .with_open_window(
                WindowKind::PlayerWindow(PhaseStep::InvestigatorTurnBegins),
                FastActorScope::ActiveInvestigator(InvestigatorId(1)),
            )
            .build();
        assert_eq!(state.open_windows.len(), 2);
        assert!(matches!(
            state.open_windows[1].kind,
            WindowKind::PlayerWindow(PhaseStep::InvestigatorTurnBegins)
        ));
    }
```

- [ ] **Step 9: Migrate `tests/reaction_windows.rs` dead fixture.**

In `crates/game-core/tests/reaction_windows.rs`, the empty-window-on-stack test (~885–944) injects an empty `WindowKind::BetweenPhases { from, to }` on top of a reaction window to verify `close_reaction_window_at` pops the right index. Replace both the constructed window (~909) and the assertion `matches!` (~940) with `WindowKind::PlayerWindow(PhaseStep::InvestigatorTurnBegins)`. Update the comments that name "BetweenPhases" to name the new window (e.g. "an empty player-window gate"). Add `PhaseStep` to the `use game_core::state::{...}` import; drop `Phase` if it becomes unused.

- [ ] **Step 10: Build and test game-core under CI-strict flags.**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
```
Expected: both PASS. The `doc` run is the safety net for any missed `WindowKind::<Marker>` intra-doc link — a stale link fails it. If anything fails, fix the named site and re-run.

- [ ] **Step 11: Commit.**

```bash
git add crates/game-core
git commit -m "$(printf 'engine: collapse marker WindowKind variants into PlayerWindow\n\nReplace the six payload-less marker variants with\nWindowKind::PlayerWindow(PhaseStep) and remove the dead BetweenPhases\nvariant. Continuation dispatch and trigger_matches re-key on PhaseStep.\n\nRefs #140.\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

### Task 2: Migrate the `cards` crate Fast-play fixtures

**Files:**
- Modify: `crates/cards/tests/fast_play.rs` (import ~38; six `WindowKind::BetweenPhases` uses ~66/108/151/224/309/375)

- [ ] **Step 1: Replace the six `BetweenPhases` uses.**

In `crates/cards/tests/fast_play.rs`, each `WindowKind::BetweenPhases { from, to }` was a generic "Fast-allowed window whose continuation is a no-op". Replace each with `WindowKind::PlayerWindow(PhaseStep::InvestigatorTurnBegins)` (same payload-less, `Done`-continuation shape). Add `PhaseStep` to the `use ... { WindowKind }` import; drop now-unused `Phase` import only if nothing else in the file uses it (check — `Phase` may still be used for `.with_phase(...)`).

- [ ] **Step 2: Test the cards crate under CI-strict flags.**

```bash
RUSTFLAGS="-D warnings" cargo test -p cards --all-features
```
Expected: PASS (all Fast-play tests green with the new window kind).

- [ ] **Step 3: Commit.**

```bash
git add crates/cards
git commit -m "$(printf 'test: migrate cards Fast-play fixtures off dead BetweenPhases\n\nUse WindowKind::PlayerWindow(PhaseStep::InvestigatorTurnBegins) as the\ngeneric Fast-allowed window stand-in, matching the removed\nBetweenPhases shape (payload-less, no-op continuation).\n\nRefs #140.\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

### Task 3: Full CI gauntlet + scenarios cross-check

**Files:** none (verification only; fix-forward if a site was missed)

- [ ] **Step 1: Confirm no stray references remain.**

```bash
grep -rn "BetweenPhases" crates/ ; echo "exit: $?"
grep -rn "WindowKind::MythosAfterDraws\|WindowKind::UpkeepBegins\|WindowKind::BeforeInvestigatorAttacked\|WindowKind::AfterAllInvestigatorsAttacked\|WindowKind::InvestigationBegins\|WindowKind::InvestigatorTurnBegins" crates/
```
Expected: the first grep prints nothing (exit 1 from grep = no matches); the second prints nothing. Any hit is a missed site — fix it (note `crates/scenarios/tests/mythos_phase.rs` references `WindowKind::MythosAfterDraws` and `WindowKind::InvestigationBegins` and must be migrated the same way, with `PhaseStep` added to its import).

- [ ] **Step 2: Run the full CI gauntlet.**

```bash
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```
Expected: all five PASS.

- [ ] **Step 3: Commit any fixes from Step 1/2.**

```bash
git add -A
git commit -m "$(printf 'test: migrate scenarios fixtures off marker WindowKind variants\n\nRefs #140.\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```
(Skip if Steps 1–2 needed no edits.)

---

## Self-Review

**Spec coverage:**
- Remove `BetweenPhases` + migrate test sites → Task 1 (builder, tests/reaction_windows, event serde), Task 2 (cards fast_play), Task 3 (scenarios). ✅
- `PlayerWindow(PhaseStep)`, no redundant `phase` → Task 1 Step 1. ✅
- `trigger_matches` collapse → Task 1 Step 4. ✅
- `run_window_continuation` restructure, bodies verbatim → Task 1 Step 5. ✅
- Serde test consolidation → Task 1 Step 6. ✅
- Doc-link migration (doc -D warnings safety net) → Task 1 Step 3 + Step 10. ✅
- Serde/replay safety → no code action needed (no `Action` carries `WindowKind`); covered by gauntlet. ✅

**Placeholder scan:** The `<MOVE the … doc-comment body here>` markers in Step 1 are explicit instructions to relocate existing verbatim doc text (the source text is in `game_state.rs` today), not unwritten content. All code blocks are complete.

**Type consistency:** `WindowKind::PlayerWindow(PhaseStep)` and the six `PhaseStep::*` names are used identically across Tasks 1–3. `InvestigatorTurnBegins` is the uniform dead-fixture replacement in builder.rs, tests/reaction_windows.rs, and fast_play.rs.
