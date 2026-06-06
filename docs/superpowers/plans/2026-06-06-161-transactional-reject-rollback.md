# Transactional Reject Rollback (#161) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the engine's "`Rejected` ⟹ state unchanged" contract structural by snapshotting `GameState` before dispatch and restoring it on `Rejected` at the single `apply` boundary.

**Architecture:** In `apply_with_scenario_registry` (`crates/game-core/src/engine/mod.rs`), clone the state before running the dispatch handler and, if the outcome is `EngineOutcome::Rejected`, replace the (possibly partially-mutated) state with the pristine clone. The transaction boundary is the `apply` *call*, so a reject during `ResolveInput` rewinds to the `AwaitingInput` pause state, not the pre-action state. No handler or evaluator code changes — only doc-comments. The new guarantee is proven by an integration test that plays a probe card whose `OnPlay` effect mutates then rejects mid-resolution.

**Tech Stack:** Rust, `cargo test`. Engine crate `game-core`; integration tests live in `crates/cards/tests/` (own process, can install a custom `CardRegistry`).

---

## File Structure

- **Modify:** `crates/game-core/src/engine/mod.rs` — add snapshot/restore in `apply_with_scenario_registry`; update the `# Handler contract` doc-comment and the `events.clear()` comment. Add engine unit tests for guard-ladder and AwaitingInput-boundary rejects.
- **Create:** `crates/cards/tests/reject_rollback.rs` — integration test with a hand-rolled `CardRegistry` + probe card; the primary oracle for the new guarantee.
- **Modify (docs only):** `crates/game-core/src/engine/dispatch/cards.rs` — delete the `play_card` partial-state caveat. `crates/game-core/src/engine/evaluator.rs` — downgrade the `apply_seq` caveat.

No phase-doc update: #161 is unmilestoned (`p1-next`, not in any `docs/phases/phase-N` milestone).

---

## Task 1: Failing integration test — evaluator-driven mid-resolution reject leaves state untouched

**Files:**
- Create: `crates/cards/tests/reject_rollback.rs`

The probe card has `OnPlay = Seq[GainResources(2), Modify(Willpower, +1, ThisTurn)]`. `GainResources` mutates (resources +2); `Modify` with `ModifierScope::ThisTurn` is a TODO stub that returns `Rejected` (`evaluator.rs:258`). So `play_card` pushes `CardPlayed`, the `Seq` gains 2 resources, then rejects — leaving partial state until the boundary restores it.

- [ ] **Step 1: Write the failing test**

```rust
//! Proves the engine's transactional guarantee: an action that mutates
//! state and then rejects mid-resolution leaves state AND events
//! byte-identical to the pre-action state.
//!
//! Own integration-test binary so it can install a *hand-rolled*
//! `CardRegistry` (a probe card whose OnPlay effect mutates then
//! rejects) without colliding with `game-core`'s registry-free unit
//! tests or the real-corpus `play_card.rs` binary.

use std::sync::OnceLock;

use game_core::card_data::{CardMetadata, CardType, Class, SkillIcons};
use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{gain_resources, modify, on_play, seq, Ability};
use game_core::dsl::{InvestigatorTarget, ModifierScope, Stat};
use game_core::engine::{apply, EngineOutcome};
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{test_investigator, test_location, TestGame};
use game_core::{Action, PlayerAction};

/// Code for the synthetic probe card. Not in the real corpus; only the
/// hand-rolled registry below resolves it.
const PROBE: &str = "ROLLBACK1";

/// OnPlay that gains 2 resources (mutates) then runs a ThisTurn Modify,
/// which is an evaluator TODO stub that returns `Rejected` — producing a
/// mid-resolution reject after a committed mutation.
fn probe_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() != PROBE {
        return None;
    }
    Some(vec![on_play(seq([
        gain_resources(InvestigatorTarget::Active, 2),
        modify(Stat::Willpower, 1, ModifierScope::ThisTurn),
    ]))])
}

fn probe_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| CardMetadata {
        code: PROBE.to_string(),
        name: "Rollback Probe".to_string(),
        class: Class::Neutral,
        card_type: CardType::Asset,
        cost: Some(0),
        xp: Some(0),
        text: None,
        flavor: None,
        illustrator: None,
        traits: vec![],
        slots: vec![],
        skill_icons: SkillIcons::default(),
        health: None,
        sanity: None,
        deck_limit: 2,
        quantity: 1,
        pack_code: "test".to_string(),
        position: 1,
        is_fast: false,
        spawn: None,
        surge: false,
        peril: false,
    })
}

fn probe_metadata(code: &CardCode) -> Option<&'static CardMetadata> {
    if code.as_str() == PROBE {
        Some(probe_metadata_static())
    } else {
        None
    }
}

/// Install the hand-rolled probe registry once for this binary.
fn install_probe_registry() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: probe_metadata,
        abilities_for: probe_abilities,
    });
}

#[test]
fn mid_resolution_reject_leaves_state_and_events_untouched() {
    install_probe_registry();

    let id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.hand = vec![CardCode::new(PROBE)];

    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_location(test_location(101, "Study"))
        .build();

    // Capture the pre-action state to compare against byte-for-byte.
    let before = state.clone();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: id,
            hand_index: 0,
        }),
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "ThisTurn Modify stub should reject, got {:?}",
        result.outcome,
    );
    assert_eq!(
        result.state, before,
        "rejected play must leave state byte-identical (resources, hand, cards_in_play)",
    );
    assert!(
        result.events.is_empty(),
        "rejected play must emit no events, got {:?}",
        result.events,
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cards --test reject_rollback mid_resolution_reject_leaves_state_and_events_untouched`
Expected: FAIL on the `result.state == before` assertion (the acting investigator's `resources` is `before + 2` because `GainResources` committed before the `Modify` reject, and `state` is not yet restored). The `outcome` and `events` assertions already pass (`play_card` returns before the destination move; the existing `events.clear()` empties the buffer).

- [ ] **Step 3: Commit the failing test**

```bash
git add crates/cards/tests/reject_rollback.rs
git commit -m "test: failing reject-rollback probe for #161

Plays a probe card whose OnPlay Seq gains resources then hits the
ThisTurn Modify stub (rejects). Asserts state is byte-identical and
events empty after the rejected play — fails today because state is
not restored on Rejected.

Refs #161."
```

---

## Task 2: Implement snapshot/restore at the apply boundary

**Files:**
- Modify: `crates/game-core/src/engine/mod.rs:88-127` (`apply_with_scenario_registry`)

- [ ] **Step 1: Add the snapshot before dispatch and restore on Rejected**

In `apply_with_scenario_registry`, the current body is:

```rust
    let mut state = state;
    let mut events = Vec::new();
    let resolution_already_fired = state.resolution.is_some();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = match action {
            Action::Player(p) => dispatch::apply_player_action(&mut cx, &p),
            Action::Engine(e) => dispatch::apply_engine_record(&mut cx, &e),
        };
        if matches!(outcome, EngineOutcome::Rejected { .. }) {
            // Belt-and-suspenders: handlers are expected to validate before
            // mutating, so events should already be empty here. Clear
            // anyway in case a handler accidentally pushed before bailing.
            cx.events.clear();
        } else if !resolution_already_fired {
```

Change it to add the snapshot and restore. The `cx` borrows `state`, so the `state = pristine` restore must happen *after* the `cx` block ends (the borrow is released at the block's close). Keep `events.clear()` inside the block (it goes through `cx.events`), and move the state restore to just after:

```rust
    let mut state = state;
    let mut events = Vec::new();
    // Transactional snapshot: a Rejected outcome must leave the returned
    // state byte-identical to the input (the engine's "Rejected => state
    // unchanged" contract). Taken before any handler runs and restored
    // below if the outcome is Rejected, so no handler — including the
    // fallible-and-mutating DSL evaluator — can leak partial state on
    // rejection. AwaitingInput is untouched: it legitimately returns the
    // work done up to the pause point, so we restore on Rejected only.
    let pristine = state.clone();
    let resolution_already_fired = state.resolution.is_some();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = match action {
            Action::Player(p) => dispatch::apply_player_action(&mut cx, &p),
            Action::Engine(e) => dispatch::apply_engine_record(&mut cx, &e),
        };
        if matches!(outcome, EngineOutcome::Rejected { .. }) {
            // Transactional restore (event half): the events buffer is
            // per-apply and starts empty, so clearing it == restoring it.
            // State half is restored after this block (the `cx` borrow on
            // `state` releases at the block close).
            cx.events.clear();
        } else if !resolution_already_fired {
```

Then, immediately after the `outcome` block closes (after the line `// `cx` drops here, releasing borrows on `state` and `events`.`) and before constructing `ApplyResult`, add:

```rust
    // State half of the transactional restore: now that `cx`'s borrow on
    // `state` is released, swap the (possibly partially-mutated) state
    // back to the pristine snapshot on rejection.
    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        state = pristine;
    }
    ApplyResult {
        state,
        events,
        outcome,
    }
```

- [ ] **Step 2: Run the Task 1 test to verify it now passes**

Run: `cargo test -p cards --test reject_rollback mid_resolution_reject_leaves_state_and_events_untouched`
Expected: PASS — `result.state == before` now holds because the rejected play restores the pristine snapshot.

- [ ] **Step 3: Run the full game-core + cards suites to verify no regression**

Run: `cargo test -p game-core -p cards`
Expected: PASS (all existing tests; no external behavior change for correct handlers).

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/mod.rs
git commit -m "engine: transactional reject rollback at the apply boundary (#161)

Snapshot GameState before dispatch; on Rejected, restore it. Makes
'Rejected => state unchanged' structural for every handler, including
the fallible-and-mutating DSL evaluator path. AwaitingInput is
untouched. events.clear() retained as the event half of the restore.

Refs #161."
```

---

## Task 3: Engine unit tests — guard-ladder and AwaitingInput-boundary rejects

**Files:**
- Modify: `crates/game-core/src/engine/mod.rs` (inside the existing `#[cfg(test)] mod tests`)

These lock the invariant for the pre-mutation paths and the transaction-boundary semantics (a `ResolveInput` reject rewinds to the pause state, not the pre-action state). They need no card registry.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `mod.rs`:

```rust
#[test]
fn rejected_action_returns_byte_identical_state() {
    // A reject with nothing outstanding (ResolveInput on a fresh state)
    // must leave state untouched.
    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let before = state.clone();

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state, before, "rejected action must not mutate state");
    assert!(result.events.is_empty());
}

#[test]
fn rejected_resolve_input_rewinds_to_pause_state_not_pre_action() {
    // Drive a skill test to its commit-window AwaitingInput, then submit
    // a malformed response. The reject must rewind to the *pause* state
    // (in_flight_skill_test still set), not to before the skill test.
    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();

    let paused = apply(
        state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            difficulty: 2,
        }),
    );
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "skill test should pause at the commit window, got {:?}",
        paused.outcome,
    );
    assert!(paused.state.in_flight_skill_test.is_some());
    let s1 = paused.state.clone();

    // Malformed response: commit window expects CommitCards; send Skip.
    let result = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state, s1,
        "rejected ResolveInput rewinds to the pause state, not pre-action",
    );
    assert!(result.state.in_flight_skill_test.is_some(), "suspension stays open");
    assert!(result.events.is_empty());
}
```

Note: `SkillKind`, `InputResponse`, `Phase`, `InvestigatorId` are already imported in the `tests` module (see `mod.rs:158-168`). If `PerformSkillTest`'s commit window expects a different malformed response to reject (verify against `resolve_input` in `dispatch/mod.rs:345-354` — any non-`CommitCards` rejects), `Skip` is correct.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p game-core rejected_action_returns_byte_identical_state rejected_resolve_input_rewinds_to_pause_state_not_pre_action`
Expected: PASS (Task 2 already implemented the mechanism; these are regression locks). If `rejected_resolve_input_rewinds...` does not reach `AwaitingInput`, inspect the actual `PerformSkillTest` outcome and adjust the difficulty/skill to a paused configuration per existing skill-test tests in `mod.rs`.

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/engine/mod.rs
git commit -m "test: lock reject-rollback invariant for guard-ladder + AwaitingInput boundary (#161)

Refs #161."
```

---

## Task 4: Update the now-stale doc-comments

**Files:**
- Modify: `crates/game-core/src/engine/mod.rs:52-60` (`# Handler contract` doc)
- Modify: `crates/game-core/src/engine/dispatch/cards.rs:457-466` (`play_card` caveat)
- Modify: `crates/game-core/src/engine/evaluator.rs:394-399` (`apply_seq` caveat)

- [ ] **Step 1: Rewrite the `apply` `# Handler contract` doc**

In `mod.rs`, replace the current `# Handler contract` paragraph (lines 52-60):

```rust
/// # Handler contract
///
/// On [`EngineOutcome::Rejected`], the returned state and event list
/// must be unchanged from the input. `apply` enforces this for the
/// event list (it clears events post-dispatch on rejection) but **not**
/// for state — handlers are expected to validate before mutating.
/// TODO(#17+): once non-trivial handlers exist, refactor to a strict
/// validate-first / apply-second two-phase shape so this is structural
/// rather than a per-handler convention.
```

with:

```rust
/// # Handler contract
///
/// On [`EngineOutcome::Rejected`], the returned state and event list
/// are unchanged from the input. `apply` enforces this **structurally**:
/// it snapshots the state before dispatch and restores the snapshot on
/// rejection, and clears the (per-apply) event buffer. So no handler —
/// including the fallible-and-mutating DSL evaluator — can leak partial
/// state on rejection; handlers need not be defensively validate-first
/// for *correctness* of this invariant (they still should be, for clear
/// rejection messages and to avoid wasted work).
///
/// The transaction boundary is the `apply` *call*, not a multi-call
/// logical action: a reject during a
/// [`ResolveInput`](crate::action::PlayerAction::ResolveInput) rewinds to
/// the [`AwaitingInput`](EngineOutcome::AwaitingInput) pause state (the
/// input to that `apply`), not to before the original action — the pause
/// state was the product of an apply that returned `AwaitingInput`, whose
/// partial state is legitimate and retained.
```

- [ ] **Step 2: Update the `events.clear()` inline comment**

In `mod.rs`, replace the comment inside the `Rejected` branch (lines 106-109):

```rust
            // Belt-and-suspenders: handlers are expected to validate before
            // mutating, so events should already be empty here. Clear
            // anyway in case a handler accidentally pushed before bailing.
            cx.events.clear();
```

(This was already replaced in Task 2 Step 1 with the "Transactional restore (event half)" comment. If Task 2 was applied, this step is a no-op — verify the comment reads "Transactional restore (event half)" and move on.)

- [ ] **Step 3: Delete the `play_card` caveat**

In `dispatch/cards.rs`, delete the entire `# State-mutation contract caveat` doc section (lines 457-466):

```rust
/// # State-mutation contract caveat
///
/// For the Phase-3-scoped Core cards the on-play effects in scope
/// (`DiscoverClue`, `GainResources`) can't reject after the standard
/// validation prefix passes. If a future on-play effect can reject
/// mid-resolution, the partial mutation between [`Event::CardPlayed`]
/// and the destination move violates the engine's "no state change on
/// rejection" contract. The apply loop's belt-and-suspenders
/// `events.clear()` still clears the event stream on a rejected
/// outcome; the state-rollback hardening is out of scope here.
```

Replace it with a one-line pointer:

```rust
/// # State-mutation contract
///
/// A mid-resolution reject here (an `OnPlay` effect returning non-`Done`
/// after [`Event::CardPlayed`] and earlier effects have committed) is
/// rolled back at the `apply` boundary — see [`apply`](crate::engine::apply)'s
/// "Handler contract". No per-handler rollback is needed.
```

- [ ] **Step 4: Downgrade the `apply_seq` caveat**

In `evaluator.rs`, replace the first paragraph of the `apply_seq` comment (lines 394-399):

```rust
    // Stop at the first non-Done outcome. A Rejected mid-Seq leaves
    // earlier effects committed — not great as a rollback story, but
    // matches the existing handler contract (the validate-first
    // refactor that fixes this for whole handlers is TODO'd in
    // engine/mod.rs::apply). Most card sequences are short enough
    // that the lack of mid-sequence rollback is fine for now.
```

with:

```rust
    // Stop at the first non-Done outcome. A Rejected mid-Seq leaves
    // earlier effects committed *within this apply*, but the `apply`
    // boundary rolls the whole call back to its pre-dispatch snapshot on
    // Rejected (see engine/mod.rs::apply "Handler contract"), so the
    // partial mutation never escapes. The AwaitingInput-resume note below
    // still stands.
```

(Leave the `**AwaitingInput resume:**` paragraph that follows unchanged.)

- [ ] **Step 5: Verify docs build and tests still pass**

Run: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features && cargo test -p game-core -p cards`
Expected: PASS — no broken intra-doc links (the new `[\`apply\`]` link resolves), all tests green.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/mod.rs crates/game-core/src/engine/dispatch/cards.rs crates/game-core/src/engine/evaluator.rs
git commit -m "docs: reflect structural reject-rollback in engine contracts (#161)

Rewrite apply()'s Handler contract, delete the play_card partial-state
caveat, downgrade the apply_seq caveat, remove TODO(#17+).

Refs #161."
```

---

## Task 5: Full CI gauntlet

**Files:** none (verification only)

- [ ] **Step 1: Run all five CI jobs locally with strict flags**

```bash
RUSTFLAGS="-D warnings"    cargo test --all --all-features
                           cargo clippy --all-targets --all-features -- -D warnings
                           cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
                           cargo build -p web --target wasm32-unknown-unknown
```

Expected: all green. Fix any clippy/fmt/doc issues before proceeding (e.g. `cargo fmt` if `--check` fails, then re-run).

- [ ] **Step 2: Push the branch and open the PR**

```bash
git push -u origin engine/reject-rollback
gh pr create --fill
```

Use the repo PR template; include a short design-decisions paragraph (snapshot/restore at the apply boundary; transaction boundary = the apply call) and the `Closes #161.` line.

---

## Self-Review

**Spec coverage:**
- Snapshot/restore mechanism at `apply` boundary → Task 2. ✅
- `Rejected`-only restore, `AwaitingInput` untouched → Task 2 (restore guarded by `matches!(Rejected)`), Task 3 boundary test. ✅
- Transaction boundary = apply call (s0/s1 semantics) → Task 3 `rejected_resolve_input_rewinds...`, Task 4 doc. ✅
- Integration test proving evaluator-driven mid-resolution reject is rolled back → Task 1/2. ✅
- Guard-ladder byte-identical regression → Task 3 `rejected_action_returns_byte_identical_state`. ✅
- Delete `play_card` caveat, downgrade `apply_seq` caveat, remove `TODO(#17+)`, rewrite contract + `events.clear()` doc → Task 4. ✅
- `events.clear()` retained → Task 2. ✅
- Out of scope (no clone-on-write, no evaluator rewrite, keep `check_*` validators) → respected; no task touches them. ✅
- Full CI gauntlet → Task 5. ✅

**Placeholder scan:** No TBD/TODO-as-instruction; all code blocks are complete and copy-runnable.

**Type consistency:** `CardRegistry { metadata_for, abilities_for }` field names match `card_registry.rs`. Builders `on_play`/`seq`/`gain_resources`/`modify` and enums `InvestigatorTarget::Active`, `Stat::Willpower`, `ModifierScope::ThisTurn` match `card-dsl/src/dsl.rs`. `CardMetadata` literal fields match `card_data.rs` (not `#[non_exhaustive]`). `EngineOutcome::Rejected`, `ApplyResult { state, events, outcome }` match `outcome.rs`/`mod.rs`.
