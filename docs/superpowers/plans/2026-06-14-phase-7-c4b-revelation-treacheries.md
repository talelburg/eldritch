# C4b Revelation Treacheries — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement The Gathering's four one-shot Revelation treacheries (Grasping Hands 01162, Rotting Remains 01163, Crypt Chill 01167, Ancient Evils 01166), plus the shared engine machinery they need.

**Architecture:** Two PRs. **PR 1 (infra #286)** adds `Effect::SkillTest` + `Effect::ForEachPointFailed`, a failure-side follow-up on the skill-test driver, a `pending_revelation_discard` slot so a suspended-revelation treachery still reaches the discard pile, and a public doom helper. **PR 2 (C4b #234)** adds the four card impls (two pure-DSL, two card-local `Effect::Native`) on top.

**Tech Stack:** Rust, `card-dsl` (effect DSL), `game-core` (kernel/evaluator/dispatch), `cards` (content + registry). Strict CI gauntlet (see CLAUDE.md Commands).

**Spec:** `docs/superpowers/specs/2026-06-14-phase-7-c4b-revelation-treacheries-design.md`

**Card text (verified verbatim, `data/arkhamdb-snapshot/pack/core/core_encounter.json`):**
- 01162 Grasping Hands — *Revelation - Test [agility] (3). For each point you fail by, take 1 damage.*
- 01163 Rotting Remains — *Revelation - Test [willpower] (3). For each point you fail by, take 1 horror.*
- 01167 Crypt Chill — *Revelation - Test [willpower] (4). If you fail, choose and discard 1 asset you control (if you cannot, take 2 damage instead).*
- 01166 Ancient Evils — *Revelation - Place 1 doom on the current agenda. This effect can cause the current agenda to advance.*

**Key design facts confirmed by code-reading:**
- `Effect::SkillTest`'s `skill` field uses `card_data::SkillKind` (maps 1:1 to `start_skill_test`'s `skill: SkillKind` param — no `Stat`→`SkillKind` conversion). The spec said `Stat`; `SkillKind` is the simpler choice.
- The treachery-to-discard code is **not** available at `Effect::SkillTest` evaluation (only `resolve_encounter_card` knows it), so `on_fail` (on the in-flight record) and `pending_revelation_discard` (set by the encounter path) are **separate** mechanisms.
- `resolve_chaos_token_and_emit` (`skill_test.rs:435`) already computes the failure margin as `by = difficulty.saturating_sub(total)`.
- `place_doom_on_agenda` (`act_agenda.rs:12`) only adds doom; the advance check is the separate `check_doom_threshold` (`act_agenda.rs:31`). Ancient Evils needs both.
- `Zone::InPlay` (`card.rs:69`) is the `from` zone for discarding an asset out of play.
- `cards::by_code(code)` returns `Option<&'static CardMetadata>`; `matches!(m.kind, CardKind::Asset { .. })` identifies an asset.

---

# PR 1 — Engine machinery (#286)

Branch: `engine/skilltest-revelation`. Commit subjects: `engine: …`.

### Task 0: Branch + commit the spec

- [ ] **Step 1: Create the branch off main**

Run:
```bash
cd /home/talel/eldritch && git checkout main && git pull && git checkout -b engine/skilltest-revelation
```

- [ ] **Step 2: Commit the already-written spec + this plan**

```bash
git add docs/superpowers/specs/2026-06-14-phase-7-c4b-revelation-treacheries-design.md \
        docs/superpowers/plans/2026-06-14-phase-7-c4b-revelation-treacheries.md
git commit -m "$(cat <<'EOF'
docs: spec + plan for C4b revelation treacheries (#286, #234)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 1: State fields — `pending_revelation_discard` + `InFlightSkillTest.on_fail`

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add field to `GameState` and to `InFlightSkillTest`)

This is plumbing; the test is "existing suite still compiles + passes." The behavioral tests come in Tasks 3–4.

- [ ] **Step 1: Add `pending_revelation_discard` to `GameState`**

In `GameState` (near `in_flight_skill_test` / `hand_size_discard_pending`), add:
```rust
    /// A treachery whose Revelation suspended (e.g. initiated a skill
    /// test) and must be pushed to `encounter_discard` once the
    /// suspending sub-resolution completes. Set by
    /// `resolve_encounter_card` when its Revelation loop yields
    /// `AwaitingInput`; flushed by the skill-test driver's terminal
    /// teardown step. `None` for the common Investigate/Fight/Evade
    /// test (no pending revelation). TODO(#212): generalize beyond
    /// skill-test-suspended revelations once ChooseOne can suspend.
    pub pending_revelation_discard: Option<crate::state::CardCode>,
```
Initialize it to `None` everywhere `GameState` is constructed (grep for the struct literal / `Default`; add `pending_revelation_discard: None`).

- [ ] **Step 2: Add `on_fail` to `InFlightSkillTest`**

In `InFlightSkillTest` (game_state.rs:360), after `follow_up`, add:
```rust
    /// Effect to run **on failure** after the chaos token resolves,
    /// with the failure margin available via `EvalContext::failed_by`.
    /// Carried by treachery-Revelation tests (`Effect::SkillTest`);
    /// `None` for action tests, which have only the success-side
    /// `follow_up`. Orthogonal to `follow_up` — separate axes.
    pub on_fail: Option<card_dsl::dsl::Effect>,
```
(`card_dsl` is already a dep of `game-core`; check the existing `use` for the right path — `crate::dsl::Effect` re-export also works.)

- [ ] **Step 3: Thread `on_fail` through `start_skill_test`**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, add a param to `start_skill_test` (after `follow_up`):
```rust
    follow_up: SkillTestFollowUp,
    on_fail: Option<card_dsl::dsl::Effect>,
) -> EngineOutcome {
```
Set it on the record (in the `InFlightSkillTest { … }` literal at ~line 76):
```rust
        follow_up,
        on_fail,
        continuation: FinishContinuation::AwaitingCommit,
```

- [ ] **Step 4: Update the four existing callers to pass `None`**

`actions.rs:125` (investigate), `actions.rs:420` (fight), `actions.rs:452` (evade), `skill_test.rs:718` (perform_skill_test). Add `None,` as the new trailing arg before the closing `)`.

- [ ] **Step 5: Compile + run the skill-test suite**

Run:
```bash
cargo test -p game-core skill_test 2>&1 | tail -20
```
Expected: PASS (pure plumbing; behavior unchanged).

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: state slots for revelation-test follow-up + pending discard

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: DSL variants — `Effect::SkillTest` + `Effect::ForEachPointFailed`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (enum variants + builders + serde test)

- [ ] **Step 1: Write the failing serde round-trip test**

In the `#[cfg(test)]` module of `dsl.rs`, add:
```rust
#[test]
fn skill_test_and_for_each_point_failed_round_trip() {
    use crate::card_data::SkillKind;
    let effect = Effect::SkillTest {
        skill: SkillKind::Agility,
        difficulty: 3,
        on_fail: Box::new(Effect::ForEachPointFailed(Box::new(Effect::DealDamage {
            target: InvestigatorTarget::You,
            amount: 1,
        }))),
    };
    let json = serde_json::to_string(&effect).expect("serialize");
    let back: Effect = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(effect, back);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p card-dsl skill_test_and_for_each_point_failed_round_trip 2>&1 | tail -5`
Expected: FAIL (compile error — `Effect::SkillTest` / `Effect::ForEachPointFailed` don't exist).

- [ ] **Step 3: Add the two variants**

In `enum Effect` (after `Native { tag }`, before the closing brace):
```rust
    /// Initiate a skill test as part of a card effect (treachery
    /// Revelation, agenda forced effect, …). Maps `skill` to the
    /// engine's `SkillKind` and runs the test against `difficulty`.
    /// Always suspends at the commit window. `on_fail` runs after the
    /// test resolves **on failure**, with the failure margin available
    /// via `EvalContext::failed_by` (success is a no-op for the cards
    /// in scope). See issue #286.
    SkillTest {
        skill: crate::card_data::SkillKind,
        difficulty: u8,
        on_fail: Box<Effect>,
    },
    /// Run `body` once per point the just-resolved skill test was
    /// failed by ("for each point you fail by, …"). Reads the margin
    /// from `EvalContext::failed_by`; a `0` margin (or no test in
    /// context) runs `body` zero times. Only meaningful inside an
    /// `Effect::SkillTest`'s `on_fail`.
    ForEachPointFailed(Box<Effect>),
```

- [ ] **Step 4: Add builders**

Near the other builders (after `native`):
```rust
/// Construct an [`Effect::SkillTest`].
#[must_use]
pub fn skill_test(
    skill: crate::card_data::SkillKind,
    difficulty: u8,
    on_fail: Effect,
) -> Effect {
    Effect::SkillTest {
        skill,
        difficulty,
        on_fail: Box::new(on_fail),
    }
}

/// Construct an [`Effect::ForEachPointFailed`].
#[must_use]
pub fn for_each_point_failed(body: Effect) -> Effect {
    Effect::ForEachPointFailed(Box::new(body))
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p card-dsl skill_test_and_for_each_point_failed_round_trip 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
card-dsl: Effect::SkillTest + Effect::ForEachPointFailed variants

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Evaluator — `EvalContext::failed_by`, `ForEachPointFailed`, `SkillTest` arms

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (EvalContext field + two match arms)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (bump `start_skill_test` visibility)

- [ ] **Step 1: Write the failing test for `ForEachPointFailed`**

In evaluator.rs `#[cfg(test)]`, add a test that runs `ForEachPointFailed(DealDamage{You,1})` with `failed_by = Some(2)` and asserts 2 damage. Use the existing test scaffolding (`ctx(id)` helper at evaluator.rs:770 and the TestGame builder pattern used by nearby damage tests near line 1859):
```rust
#[test]
fn for_each_point_failed_scales_damage_by_margin() {
    use crate::test_support::TestGame;
    let mut state = TestGame::new()
        .with_investigator(/* id */ 1, /* … per existing damage tests */)
        .build();
    // Construct Cx + a ctx with failed_by = Some(2).
    let effect = Effect::ForEachPointFailed(Box::new(Effect::DealDamage {
        target: InvestigatorTarget::You,
        amount: 1,
    }));
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let mut ctx = EvalContext::for_controller(InvestigatorId(1));
    ctx.failed_by = Some(2);
    let outcome = apply_effect(&mut cx, &effect, ctx);
    assert!(matches!(outcome, EngineOutcome::Done));
    // 2 damage dealt → 2 HealthChanged / damage events (match the exact
    // event the existing DealDamage test at ~1859 asserts).
    assert_eq!(
        events.iter().filter(|e| matches!(e, Event::DamageDealt { .. })).count(),
        2
    );
}
```
(Adapt `TestGame`/`Cx`/event names to the exact forms the neighboring tests use — read evaluator.rs:1840-1870 first for the canonical `DealDamage` test and copy its setup verbatim.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core for_each_point_failed_scales_damage_by_margin 2>&1 | tail -8`
Expected: FAIL (compile error — `EvalContext` has no `failed_by`; no `ForEachPointFailed` arm).

- [ ] **Step 3: Add `failed_by` to `EvalContext`**

In `pub struct EvalContext` (evaluator.rs:75), add:
```rust
    /// The just-resolved skill test's failure margin, set only while
    /// running an `Effect::SkillTest`'s `on_fail`. Read by
    /// `Effect::ForEachPointFailed`. `None` outside that window.
    pub failed_by: Option<u8>,
```
Set `failed_by: None` in both constructors (`for_controller` line 96, `for_controller_with_source` line 111) and in the test helper `ctx(id)` (evaluator.rs:770).

- [ ] **Step 4: Add the `ForEachPointFailed` and `SkillTest` match arms**

In `apply_effect` (evaluator.rs:132), add arms:
```rust
        Effect::ForEachPointFailed(body) => {
            let n = eval_ctx.failed_by.unwrap_or(0);
            for _ in 0..n {
                match apply_effect(cx, body, eval_ctx) {
                    EngineOutcome::Done => {}
                    other => return other,
                }
            }
            EngineOutcome::Done
        }
        Effect::SkillTest {
            skill,
            difficulty,
            on_fail,
        } => crate::engine::dispatch::skill_test::start_skill_test(
            cx,
            eval_ctx.controller,
            *skill,
            crate::dsl::SkillTestKind::Plain,
            i8::try_from(*difficulty).unwrap_or(i8::MAX),
            crate::state::SkillTestFollowUp::None,
            Some((**on_fail).clone()),
        ),
```
`EvalContext` must derive `Copy` for the `for _ in 0..n { … eval_ctx … }` reuse — confirm it does (it's a 2-field POD); if not, pass `eval_ctx` by value via clone inside the loop.

- [ ] **Step 5: Bump `start_skill_test` visibility**

In skill_test.rs, change `pub(super) fn start_skill_test` to `pub(in crate::engine) fn start_skill_test` so the sibling `evaluator` module can call it.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p game-core for_each_point_failed_scales_damage_by_margin 2>&1 | tail -8`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: evaluator arms for SkillTest + ForEachPointFailed

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Failure-side follow-up + revelation-discard flush

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`resolve_chaos_token_and_emit` return; `finish_skill_test` failure branch; `drive_skill_test` teardown flush)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (set `pending_revelation_discard` on suspend)

- [ ] **Step 1: Make `resolve_chaos_token_and_emit` return the failure margin**

Change its signature to `-> (bool, u8)` and return `(succeeded, by_or_zero)`:
```rust
    let by = if succeeded { 0 } else { difficulty.saturating_sub(total) };
    // … existing event pushes use `by` on the failure branch …
    // (note: `by` is i8; cast to u8 for the return)
    (succeeded, u8::try_from(by).unwrap_or(0))
```
Keep the existing `SkillTestSucceeded`/`SkillTestFailed` emits unchanged (they already compute `by` on the failure branch — hoist that `let by` above the `if`).

- [ ] **Step 2: Run `on_fail` in the failure branch of `finish_skill_test`**

In `finish_skill_test` (skill_test.rs:176-180), snapshot `on_fail` alongside `follow_up` (line 154):
```rust
    let on_fail = in_flight.on_fail.clone();
```
Then replace the success-only block:
```rust
    let (succeeded, failed_by) =
        resolve_chaos_token_and_emit(cx, investigator, skill, difficulty, skill_value);

    if succeeded {
        apply_skill_test_follow_up(cx, investigator, follow_up);
    } else if let Some(effect) = &on_fail {
        let mut ctx = EvalContext::for_controller(investigator);
        ctx.failed_by = Some(failed_by);
        // on_fail effects in scope (DealDamage/DealHorror/Native) run to
        // completion; a future suspending on_fail is #212 reentrancy work.
        let outcome = apply_effect(cx, effect, ctx);
        debug_assert!(
            matches!(outcome, EngineOutcome::Done),
            "revelation on_fail must resolve to Done in C4b scope: {outcome:?}"
        );
    }
```
(Add `use crate::engine::evaluator::{apply_effect, EvalContext};` if not already imported.)

- [ ] **Step 3: Flush `pending_revelation_discard` at teardown**

In `drive_skill_test`'s terminal `FinishContinuation::PostOnResolution` arm (skill_test.rs:263-275), before `cx.state.in_flight_skill_test = None;`:
```rust
                if let Some(code) = cx.state.pending_revelation_discard.take() {
                    cx.state.encounter_discard.push(code);
                }
```
(Matches the eventless push at encounter.rs:142.)

- [ ] **Step 4: Set `pending_revelation_discard` when a Revelation suspends**

In `resolve_encounter_card`'s treachery arm (encounter.rs:133-141), change the early-return so a *suspension* records the discard:
```rust
            for ability in abilities.iter().filter(|a| a.trigger == Trigger::Revelation) {
                let outcome = apply_effect(cx, &ability.effect, eval_ctx);
                match outcome {
                    EngineOutcome::Done => {}
                    EngineOutcome::AwaitingInput { .. } => {
                        // Revelation suspended (e.g. initiated a skill test).
                        // The card discards once the suspended resolution
                        // completes — record it for the resume path to flush.
                        cx.state.pending_revelation_discard = Some(code.clone());
                        return outcome;
                    }
                    EngineOutcome::Rejected { .. } => return outcome,
                }
            }
            cx.state.encounter_discard.push(code);
            EngineOutcome::Done
```

- [ ] **Step 5: Compile + run skill-test + encounter suites**

Run:
```bash
cargo test -p game-core 2>&1 | tail -15
```
Expected: PASS (the `(bool, u8)` return change touches only the one caller).

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: failure-side on_fail follow-up + revelation-discard flush

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Public `place_doom_on_current_agenda`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/act_agenda.rs` (new pub wrapper)
- Modify: `crates/game-core/src/lib.rs` (re-export, matching the #276 pattern that re-exported `location_id_by_code`)

- [ ] **Step 1: Add the wrapper**

In act_agenda.rs:
```rust
/// Place 1 doom on the current agenda and run the doom-threshold check
/// (which may advance or resolve the agenda). The card-facing combo of
/// `place_doom_on_agenda` + `check_doom_threshold`, exposed for
/// card-local native effects (Ancient Evils 01166). No-op on an empty
/// agenda deck (both helpers guard).
pub fn place_doom_on_current_agenda(cx: &mut Cx) {
    place_doom_on_agenda(cx);
    check_doom_threshold(cx);
}
```

- [ ] **Step 2: Re-export from `game_core`**

In `crates/game-core/src/lib.rs`, alongside the existing `pub use …::{location_id_by_code, …}`, add `place_doom_on_current_agenda` (grep `location_id_by_code` in lib.rs to find the exact re-export line and extend it).

- [ ] **Step 3: Write a test**

In act_agenda.rs `#[cfg(test)]` (near `place_doom_increments_agenda_doom` at line 275): place doom up to threshold and assert the agenda advances.
```rust
#[test]
fn place_doom_on_current_agenda_advances_at_threshold() {
    // Build a state with a 2-agenda deck, doom_threshold = 1.
    // … (copy the setup from place_doom_increments_agenda_doom) …
    place_doom_on_current_agenda(&mut cx);
    assert_eq!(state.agenda_index, 1, "agenda advanced at threshold");
}
```

- [ ] **Step 4: Run + verify**

Run: `cargo test -p game-core place_doom_on_current_agenda_advances_at_threshold 2>&1 | tail -6`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: pub place_doom_on_current_agenda for card-local doom effects

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Engine integration test — suspend → resume → on_fail → discard

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` `#[cfg(test)]` (or `encounter.rs` tests)

- [ ] **Step 1: Write the test (synthetic, no real corpus)**

Drive a treachery-shaped `Effect::SkillTest` directly via the dispatch path and assert: it returns `AwaitingInput`; `pending_revelation_discard` is set mid-flight; after committing zero cards against a rigged (AutoFail or low) bag it deals the margin in damage; and the source code lands in `encounter_discard` with the in-flight test cleared. Model it on the existing `start_skill_test` tests (which rig `chaos_bag` + `rng`). Key assertions:
```rust
    // After start (revelation suspends):
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(state.pending_revelation_discard.as_deref(), Some("Ttest"));
    // After ResolveInput::CommitCards with [] against a failing bag:
    assert!(state.encounter_discard.iter().any(|c| c.0 == "Ttest"));
    assert!(state.in_flight_skill_test.is_none());
    // Damage equal to the failure margin was dealt.
```

- [ ] **Step 2: Add the regression assertion for a plain Investigate**

A second test (or extend an existing Investigate test): after a normal Investigate skill test resolves, assert `state.pending_revelation_discard.is_none()` (the slot stays untouched for non-revelation tests).

- [ ] **Step 3: Run + verify**

Run: `cargo test -p game-core 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 4: Verify the `Active`-status risk**

Confirm `start_skill_test`'s `status == Active` gate (skill_test.rs:42) treats a Mythos-phase, in-play investigator as `Active` (i.e. `Active` = in play, not turn ownership). Read `enum Status` and its setters. If a Mythos-phase investigator is **not** `Active`, add a follow-up note to issue #286 and relax the gate for `SkillTestFollowUp::None` + `on_fail.is_some()` tests (or accept that the real Mythos draw in C6d will surface it). Record the finding in the PR description either way.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: integration test for suspended-revelation skill test + discard

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: PR 1 gauntlet + open PR

- [ ] **Step 1: Run the full strict gauntlet** (all jobs from CLAUDE.md Commands):
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | tail -15
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -8
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features 2>&1 | tail -5
cargo build -p web --target wasm32-unknown-unknown 2>&1 | tail -5
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings 2>&1 | tail -5
```
Expected: all green. Fix anything before pushing.

- [ ] **Step 2: Push + open the PR**
```bash
git push -u origin engine/skilltest-revelation
gh pr create --fill --base main
```
PR body: summarize the four mechanism pieces, note the `Active`-status finding from Task 6 Step 4, and `Closes #286.`

- [ ] **Step 3: Watch CI**: `gh pr checks <PR#> --watch` (background). Fix failures with follow-up commits. **Do not merge** (await user approval). Phase-doc update is deferred to after PR 2 (or done per the README spec when this PR is ready to merge).

---

# PR 2 — C4b cards (#234)

Branch `card/revelation-treacheries` off `main` **after PR 1 merges** (these impls call `skill_test`/`for_each_point_failed`/`native`/`place_doom_on_current_agenda` from PR 1). Commit subjects: `card: …`.

> Each card is a module `crates/cards/src/impls/treachery_<code>.rs` exposing `pub const CODE: &str` + `pub fn abilities() -> Vec<Ability>`, wired into `impls/mod.rs` (`pub mod …;` + an `abilities_for` arm; native cards also extend the `native_effect_for` chain). The corpus metadata for all four already exists (`crates/cards/src/generated/cards.rs`); only abilities/native need wiring. **Do not edit `generated/cards.rs`.**

### Task 8: Grasping Hands 01162 (pure DSL)

**Files:**
- Create: `crates/cards/src/impls/treachery_01162.rs`
- Modify: `crates/cards/src/impls/mod.rs`

- [ ] **Step 1: Write the card impl with its failing test**

```rust
//! Grasping Hands (The Gathering treachery, 01162).
//!
//! ```text
//! Revelation - Test [agility] (3). For each point you fail by, take 1 damage.
//! ```
//!
//! Pure DSL: `Trigger::Revelation` → `Effect::SkillTest` whose `on_fail`
//! deals 1 damage per point failed (#286 machinery).

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    deal_damage, for_each_point_failed, revelation, skill_test, Ability, InvestigatorTarget,
};

/// `ArkhamDB` code for Grasping Hands.
pub const CODE: &str = "01162";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(skill_test(
        SkillKind::Agility,
        3,
        for_each_point_failed(deal_damage(InvestigatorTarget::You, 1)),
    ))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::Effect;

    #[test]
    fn revelation_tests_agility_3_then_damage_per_point() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        let Effect::SkillTest { skill, difficulty, on_fail } = &abilities[0].effect else {
            panic!("expected SkillTest, got {:?}", abilities[0].effect);
        };
        assert_eq!(*skill, SkillKind::Agility);
        assert_eq!(*difficulty, 3);
        assert!(matches!(
            **on_fail,
            Effect::ForEachPointFailed(ref b)
                if matches!(**b, Effect::DealDamage { amount: 1, .. })
        ));
    }
}
```

- [ ] **Step 2: Wire into the registry**

In `crates/cards/src/impls/mod.rs`: add `pub mod treachery_01162;`, and in `abilities_for` add `treachery_01162::CODE => Some(treachery_01162::abilities()),`.

- [ ] **Step 3: Run the card test**

Run: `cargo test -p cards treachery_01162 2>&1 | tail -6`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
card: Grasping Hands (01162) revelation — agility test, damage per point

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: Rotting Remains 01163 (pure DSL)

**Files:**
- Create: `crates/cards/src/impls/treachery_01163.rs`
- Modify: `crates/cards/src/impls/mod.rs`

- [ ] **Step 1: Write the impl + test**

Identical shape to Task 8 but `SkillKind::Willpower`, difficulty `3`, and `deal_horror` instead of `deal_damage`:
```rust
//! Rotting Remains (The Gathering treachery, 01163).
//!
//! ```text
//! Revelation - Test [willpower] (3). For each point you fail by, take 1 horror.
//! ```
use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    deal_horror, for_each_point_failed, revelation, skill_test, Ability, InvestigatorTarget,
};

pub const CODE: &str = "01163";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(skill_test(
        SkillKind::Willpower,
        3,
        for_each_point_failed(deal_horror(InvestigatorTarget::You, 1)),
    ))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::Effect;

    #[test]
    fn revelation_tests_willpower_3_then_horror_per_point() {
        let abilities = abilities();
        let Effect::SkillTest { skill, difficulty, on_fail } = &abilities[0].effect else {
            panic!("expected SkillTest");
        };
        assert_eq!(*skill, SkillKind::Willpower);
        assert_eq!(*difficulty, 3);
        assert!(matches!(
            **on_fail,
            Effect::ForEachPointFailed(ref b)
                if matches!(**b, Effect::DealHorror { amount: 1, .. })
        ));
    }
}
```

- [ ] **Step 2: Wire into `mod.rs`** (`pub mod treachery_01163;` + `abilities_for` arm).

- [ ] **Step 3: Run** `cargo test -p cards treachery_01163 2>&1 | tail -6` → PASS.

- [ ] **Step 4: Commit**
```bash
git add -A && git commit -m "$(cat <<'EOF'
card: Rotting Remains (01163) revelation — willpower test, horror per point

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 10: Ancient Evils 01166 (native doom)

**Files:**
- Create: `crates/cards/src/impls/treachery_01166.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`abilities_for` arm + `native_effect_for` chain)

- [ ] **Step 1: Write the impl + test**

```rust
//! Ancient Evils (The Gathering treachery, 01166).
//!
//! ```text
//! Revelation - Place 1 doom on the current agenda. This effect can cause
//!   the current agenda to advance.
//! ```
//!
//! Card-local native (#276): a single consumer of "place doom on the
//! current agenda", so it doesn't earn a shared `Effect` variant. Calls
//! the engine's `place_doom_on_current_agenda` (place + threshold check).

use card_dsl::dsl::{native, revelation, Ability};
use game_core::card_registry::NativeEffectFn;
use game_core::{place_doom_on_current_agenda, Cx, EngineOutcome, EvalContext};

pub const CODE: &str = "01166";
const PLACE_DOOM: &str = "01166:place-doom";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(native(PLACE_DOOM))]
}

pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        PLACE_DOOM => Some(place_doom as NativeEffectFn),
        _ => None,
    }
}

fn place_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    place_doom_on_current_agenda(cx);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn revelation_is_native_place_doom() {
        let abilities = abilities();
        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == PLACE_DOOM));
        assert!(native_effect_for(PLACE_DOOM).is_some());
        assert!(native_effect_for("nope").is_none());
    }
}
```
(Confirm `place_doom_on_current_agenda` is re-exported at `game_core::place_doom_on_current_agenda` per PR 1 Task 5 Step 2; adjust the path if it's under a submodule.)

- [ ] **Step 2: Wire into `mod.rs`**: `pub mod treachery_01166;`, an `abilities_for` arm, and add `.or_else(|| treachery_01166::native_effect_for(tag))` to the `native_effect_for` chain.

- [ ] **Step 3: Run** `cargo test -p cards treachery_01166 2>&1 | tail -6` → PASS.

- [ ] **Step 4: Commit**
```bash
git add -A && git commit -m "$(cat <<'EOF'
card: Ancient Evils (01166) revelation — place doom on current agenda

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: Crypt Chill 01167 (test + native fail branch)

**Files:**
- Create: `crates/cards/src/impls/treachery_01167.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`abilities_for` arm + `native_effect_for` chain)

- [ ] **Step 1: Write the impl + test**

```rust
//! Crypt Chill (The Gathering treachery, 01167).
//!
//! ```text
//! Revelation - Test [willpower] (4). If you fail, choose and discard 1
//!   asset you control (if you cannot, take 2 damage instead).
//! ```
//!
//! The willpower(4) test is shared DSL; the failure branch is card-local
//! native (#276) — a single consumer of "discard an asset you control".
//! TODO(#212): "choose" is an interactive decision; until mid-revelation
//! `ChooseOne` can suspend, this discards the first asset in play order
//! (a deterministic legal outcome, mirroring the 01105 reverse). The "2
//! damage" branch is the printed fallback for controlling **no** asset,
//! not a pass/fail alternative.

use card_dsl::card_data::{CardKind, SkillKind};
use card_dsl::dsl::{native, revelation, skill_test, Ability};
use game_core::card_registry::NativeEffectFn;
use game_core::state::Zone;
use game_core::{deal_damage_to, Cx, EngineOutcome, EvalContext, Event};

pub const CODE: &str = "01167";
const CRYPT_CHILL_FAIL: &str = "01167:crypt-chill-fail";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(skill_test(
        SkillKind::Willpower,
        4,
        native(CRYPT_CHILL_FAIL),
    ))]
}

pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        CRYPT_CHILL_FAIL => Some(crypt_chill_fail as NativeEffectFn),
        _ => None,
    }
}

fn crypt_chill_fail(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let controller = ctx.controller;
    let Some(inv) = cx.state.investigators.get_mut(&controller) else {
        return EngineOutcome::Rejected {
            reason: "01167 crypt-chill-fail: controller not in state".into(),
        };
    };
    // Deterministic stand-in for "choose" (TODO #212): first asset in
    // play order. `crate::by_code` reads the corpus kind.
    let asset_pos = inv
        .cards_in_play
        .iter()
        .position(|c| matches!(crate::by_code(&c.code.0).map(|m| &m.kind), Some(CardKind::Asset { .. })));
    match asset_pos {
        Some(pos) => {
            let card = inv.cards_in_play.remove(pos);
            inv.discard.push(card.code.clone());
            cx.events.push(Event::CardDiscarded {
                investigator: controller,
                code: card.code,
                from: Zone::InPlay,
            });
            EngineOutcome::Done
        }
        None => {
            // Cannot discard an asset → take 2 damage instead.
            // Use the engine's damage path (re-exported helper).
            deal_damage_to(cx, controller, 2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::Effect;

    #[test]
    fn revelation_tests_willpower_4_then_native_fail() {
        let abilities = abilities();
        let Effect::SkillTest { skill, difficulty, on_fail } = &abilities[0].effect else {
            panic!("expected SkillTest");
        };
        assert_eq!(*skill, SkillKind::Willpower);
        assert_eq!(*difficulty, 4);
        assert!(matches!(**on_fail, Effect::Native { ref tag } if tag == CRYPT_CHILL_FAIL));
        assert!(native_effect_for(CRYPT_CHILL_FAIL).is_some());
    }
}
```

> **Implementer note:** `deal_damage_to` is a placeholder name for "deal N damage to an investigator from the kernel." Find the actual helper the evaluator's `Effect::DealDamage` arm uses (`deal_damage_effect` → an internal fn in evaluator.rs); if no public kernel helper exists, either (a) build a tiny `EvalContext` + call `apply_effect(cx, &deal_damage(You,2), ctx)` from this native fn (cleanest — reuses the DSL), or (b) add a `pub` re-export in PR 1. Prefer (a): `apply_effect` is reachable via `game_core::...`? It's `pub(crate)`. So instead emit damage by constructing the effect and routing through the registry is awkward. **Decision: in PR 1 Task 5, also re-export a `pub fn deal_damage_to(cx, inv, amount)` thin wrapper** over the existing damage path, and use it here. Update PR 1 Task 5 to add it if this note's option (a) isn't viable.

- [ ] **Step 2: Wire into `mod.rs`** (`pub mod treachery_01167;` + `abilities_for` arm + `native_effect_for` chain entry).

- [ ] **Step 3: Run** `cargo test -p cards treachery_01167 2>&1 | tail -6` → PASS.

- [ ] **Step 4: Commit**
```bash
git add -A && git commit -m "$(cat <<'EOF'
card: Crypt Chill (01167) revelation — willpower test, discard asset/2 damage

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 12: Integration test — full draw→reveal→commit→resolve→discard

**Files:**
- Create: `crates/cards/tests/revelation_treacheries.rs` (its own process; can `install(cards::REGISTRY)`)

- [ ] **Step 1: Write the integration test**

Install the real registry, build a minimal scenario state with one investigator + a non-empty chaos bag, and for at least one test treachery (e.g. 01162) and Ancient Evils (01166):
- Drive `resolve_encounter_card`/the encounter draw for 01162 → assert `AwaitingInput`, `pending_revelation_discard == Some("01162")`.
- Submit `ResolveInput::CommitCards([])` against a failing bag → assert the investigator took the margin in damage, `encounter_discard` contains `01162`, `in_flight_skill_test` is `None`.
- For 01166: resolve it → assert agenda doom incremented (and advanced when rigged at threshold), and `01166` is in `encounter_discard`.
Pattern-match the harness on `crates/cards/tests/agenda_reverses.rs` (seeded deck) and `crates/cards/tests/play_card.rs` (registry install).

- [ ] **Step 2: Run** `cargo test -p cards --test revelation_treacheries 2>&1 | tail -12` → PASS.

- [ ] **Step 3: Commit**
```bash
git add -A && git commit -m "$(cat <<'EOF'
test: integration — revelation treacheries draw→resolve→discard

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 13: PR 2 gauntlet, PR, phase-doc

- [ ] **Step 1: Full strict gauntlet** (same commands as PR 1 Task 7 Step 1). All green.

- [ ] **Step 2: Push + open PR**
```bash
git push -u origin card/revelation-treacheries
gh pr create --fill --base main
```
Body: list the four cards with verbatim text, note the deferred Crypt Chill choice (TODO #212), `Closes #234.`

- [ ] **Step 3: Watch CI** (`gh pr checks <PR#> --watch`, background); fix failures with follow-up commits.

- [ ] **Step 4: Phase-doc update (final commit, once PR is ready to merge)** — per `docs/phases/README.md`. In `docs/phases/phase-7-the-gathering.md`: flip the C4b row (line 71) to `✅ PR #N`; update the Status paragraph's "Next" pointer (line 20) to `C4c → C5 → C7`; add the #286 infra PR to the C-breakdown table (a new row near #276, line 67); add a **Decisions made** entry only if load-bearing for a future PR (candidate: "`Effect::SkillTest` + `ForEachPointFailed` + `pending_revelation_discard` are the test-initiating-revelation rails; C4c extends `pending_revelation_discard` for threat-area placement"). Apply the "would a future PR-author choose differently without this?" test before adding.

- [ ] **Step 5: Stop. Await user approval before merging either PR.**

---

## Self-review notes (author)

- **Spec coverage:** every spec section maps to a task — `Effect::SkillTest`/`ForEachPointFailed` (T2–3), failure follow-up (T4), `pending_revelation_discard` (T1/T4), `place_doom_on_current_agenda` (T5), four cards (T8–11), test matrix (T3,T6,T8–12), `Active`-status risk (T6 Step 4).
- **Known soft spots flagged inline for the implementer:** exact `DealDamage` test event name (T3 Step 1 — read neighboring test first); the kernel damage helper for Crypt Chill (T11 note — prefer re-exporting `deal_damage_to` in T5); exact `GameState`/`InFlightSkillTest` construction sites for the new fields (T1 — grep). These are "match the existing code" lookups, not design gaps.
- **Type consistency:** `skill_test(SkillKind, u8, Effect)`, `for_each_point_failed(Effect)`, `Effect::SkillTest { skill: SkillKind, difficulty: u8, on_fail: Box<Effect> }`, `EvalContext::failed_by: Option<u8>`, `InFlightSkillTest::on_fail: Option<Effect>`, `GameState::pending_revelation_discard: Option<CardCode>` — names used consistently across tasks.
