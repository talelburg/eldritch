# Skill-test outcome timing point Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the investigate-only `SuccessfullyInvestigated` timing point with a general `SkillTestResolved { kind, outcome }` point fired at ST.6 for every skill test, and re-order chaos-symbol side-effects to their correct RR steps (immediate→ST.4, on_fail→ST.7) applied via suspendable `Effect::Deal`.

**Architecture:** Three commits on branch `engine/effect-callsite-migration`, continuing Slice D / #423. Task 1 subsumes the timing-point *types* behaviour-preservingly. Task 2 generalizes the *emission* (fire for every test/outcome). Task 3 reworks the skill-test driver frame so symbol effects resolve at ST.4/ST.7 through pushed `Effect::Deal` (interactive soak → may suspend), splitting the `Resolving` step and carrying the determination across the yield.

**Tech Stack:** Rust, the `game-core` engine crate + `card-dsl` + `cards` content. No new dependencies.

## Global Constraints

- **Match CI's strict flags before declaring any task done** (copy verbatim):
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Validate-first / mutate-second** in every dispatch handler (engine convention).
- **Each task is its own commit and keeps the full strict gauntlet green and bisectable.** One PR.
- **Verify any card/rules text against ArkhamDB or the vendored PDF, never memory.** Design doc cites: Dr. Milan 01033 "After you successfully investigate: Gain 1 resource"; Obscuring Fog 01168 "Forced – After attached location is successfully investigated: Discard Obscuring Fog"; RR ST.4 (apply chaos symbol effects) precedes ST.5/ST.6; result-conditional symbol effects resolve at ST.7.
- **Commit trailers** (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB
  ```
- Spec: `docs/superpowers/specs/2026-06-24-skill-test-outcome-timing-point-design.md`.

## File structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/card-dsl/src/dsl.rs` | `EventPattern::SkillTestResolved` (replaces `SuccessfullyInvestigated` + `AfterLocationInvestigated`) | 1 |
| `crates/game-core/src/engine/dispatch/emit.rs` | `TimingEvent::SkillTestResolved` + `forced_point`/`reaction_bucket`/`opens_reaction_window` | 1 |
| `crates/game-core/src/engine/dispatch/forced_triggers.rs` | `ForcedTriggerPoint::SkillTestResolved` + `collect_forced_hits` arm (controlled instances + `tested_location` attachments) | 1 |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | `trigger_matches` + `run_reaction_continuation` arms | 1 |
| `crates/cards/src/impls/dr_milan_christopher.rs` | reaction pattern → `SkillTestResolved` | 1 |
| `crates/cards/src/impls/obscuring_fog.rs` | forced pattern → `SkillTestResolved` | 1 |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | emit-step generalization (T2); frame rework + symbol `Effect::Deal` (T3) | 1,2,3 |
| `crates/game-core/src/state/game_state.rs` | `SkillTestStep` variants; `InFlightSkillTest.resolved` + `ResolvedTest` | 2,3 |
| `crates/game-core/tests/skill_test_outcome_timing.rs` | new integration tests (fixture registry) | 2,3 |

---

## Task 1: Subsume the timing-point types (behaviour-preserving)

Rename/reroute `SuccessfullyInvestigated` + `AfterLocationInvestigated` → one `SkillTestResolved { kind, outcome }` triple across the DSL, the dispatch machinery, and the two consumer cards. The emission stays gated on Investigate + success, so **behaviour is unchanged** — Dr. Milan and Obscuring Fog keep working with the same observable effect; their tests change only the pattern they assert.

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (EventPattern)
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` (TimingEvent + 3 methods)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (ForcedTriggerPoint + collect_forced_hits)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (trigger_matches + run_reaction_continuation)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`emit_success_reactions_step`)
- Modify: `crates/cards/src/impls/dr_milan_christopher.rs`, `crates/cards/src/impls/obscuring_fog.rs`

**Interfaces:**
- Produces: `EventPattern::SkillTestResolved { outcome: TestOutcome, kind: Option<SkillTestKind> }`; `TimingEvent::SkillTestResolved { investigator: InvestigatorId, kind: SkillTestKind, outcome: TestOutcome }`; `ForcedTriggerPoint::SkillTestResolved { investigator: InvestigatorId, kind: SkillTestKind, outcome: TestOutcome }`.

- [ ] **Step 1: Inventory the sites to change**

Run: `git grep -n "SuccessfullyInvestigated\|AfterLocationInvestigated" -- crates/`
Expected: hits in `dsl.rs`, `emit.rs`, `forced_triggers.rs`, `reaction_windows.rs`, `skill_test.rs`, `state/game_state.rs` (doc comment), `dr_milan_christopher.rs`, `obscuring_fog.rs`. Every hit must be addressed (code) or updated (doc comment) by the end of this task.

- [ ] **Step 2: Replace the two `EventPattern` variants**

In `crates/card-dsl/src/dsl.rs`, delete the `SuccessfullyInvestigated` and `AfterLocationInvestigated` variants of `EventPattern` (and their doc comments) and add, in their place:

```rust
    /// A skill test resolved with the given `outcome`. The card-facing
    /// narrowing of the engine's ST.6→ST.7 timing point — the general form of
    /// which "after you successfully investigate" (Dr. Milan 01033, Obscuring
    /// Fog 01168) is the `{ Success, Investigate }` case. `kind: None` matches
    /// any test type; `Some(k)` narrows to that type. Forced (Obscuring Fog)
    /// vs reaction (Dr. Milan) is the `OnEvent { kind }` distinction, not a
    /// pattern distinction — both share this pattern. (Slice D, #423; collapses
    /// the #212/#213 forced/reaction pattern split for this timing point.)
    SkillTestResolved {
        /// Whether the listener fires on a passed or failed test.
        outcome: TestOutcome,
        /// Narrow to a test type, or `None` for any.
        kind: Option<SkillTestKind>,
    },
```

`TestOutcome` and `SkillTestKind` are already defined in this module (both derive `Hash, Serialize, Deserialize` — matching `EventPattern`'s derives).

- [ ] **Step 3: Build the DSL + fix DSL-internal references**

Run: `cargo build -p card-dsl`
Expected: FAIL only inside `card-dsl` if any builder/match references the removed variants (none expected). If `trigger_matches` or an evaluator pattern-match in `card-dsl` referenced them, update to `SkillTestResolved`. Re-run until `card-dsl` builds.

- [ ] **Step 4: Replace `TimingEvent::SuccessfullyInvestigated`**

In `crates/game-core/src/engine/dispatch/emit.rs`, delete the `SuccessfullyInvestigated { investigator, location }` variant and its doc comment; add:

```rust
    /// A skill test resolved (RR ST.6). **Dual** (forced + reaction). The
    /// general timing point of which "after you successfully investigate" is
    /// the `{ Investigate, Success }` narrowing. Carries no location: the
    /// forced scan derives the investigated location from the still-live
    /// in-flight `SkillTest` frame (`current_skill_test().tested_location`).
    SkillTestResolved {
        investigator: InvestigatorId,
        kind: crate::dsl::SkillTestKind,
        outcome: crate::dsl::TestOutcome,
    },
```

- [ ] **Step 5: Update the three `TimingEvent` methods**

In the same file, in `forced_point()` replace the `SuccessfullyInvestigated` arm with:

```rust
            TimingEvent::SkillTestResolved {
                investigator,
                kind,
                outcome,
            } => Some(ForcedTriggerPoint::SkillTestResolved {
                investigator: *investigator,
                kind: *kind,
                outcome: *outcome,
            }),
```

In `reaction_bucket()`: `SkillTestResolved` is `After` — it already falls into the catch-all `_ => EventTiming::After` arm, so no change unless `SuccessfullyInvestigated` was named explicitly there (it is not). Verify by reading the function.

In `opens_reaction_window()` replace `SuccessfullyInvestigated { .. }` with `SkillTestResolved { .. }` in the `matches!` list.

- [ ] **Step 6: Replace `ForcedTriggerPoint::AfterLocationInvestigated`**

In `crates/game-core/src/engine/dispatch/forced_triggers.rs`, delete the `AfterLocationInvestigated { investigator, location }` variant + doc; add:

```rust
    /// A skill test resolved (RR ST.6). Forced side of
    /// [`TimingEvent::SkillTestResolved`]. The collector scans the resolving
    /// investigator's controlled instances **and** the investigated location's
    /// attachment zone (Obscuring Fog 01168 attaches to the location) — the
    /// latter derived from the in-flight `SkillTest` frame's `tested_location`,
    /// so this point carries no location of its own.
    SkillTestResolved {
        investigator: InvestigatorId,
        kind: crate::dsl::SkillTestKind,
        outcome: crate::dsl::TestOutcome,
    },
```

- [ ] **Step 7: Rewrite the `collect_forced_hits` arm**

In `collect_forced_hits`, replace the `AfterLocationInvestigated { investigator, location }` arm with:

```rust
        ForcedTriggerPoint::SkillTestResolved {
            investigator,
            kind,
            outcome,
        } => {
            let Some(inv) = state.investigators.get(investigator) else {
                return hits;
            };
            // Match the card-facing narrowing: same outcome, and either an
            // unnarrowed (`None`) or kind-matching listener.
            let want = |p: &EventPattern| {
                let EventPattern::SkillTestResolved {
                    outcome: o,
                    kind: k,
                } = p
                else {
                    return false;
                };
                *o == *outcome && (k.is_none() || *k == Some(*kind))
            };
            // Scan the investigator's controlled instances (in-play + threat
            // area). Bind source = the firing instance so DiscardSelf finds
            // itself.
            for card in inv.controlled_card_instances() {
                push_matching(
                    reg,
                    &card.code,
                    *investigator,
                    Some(card.instance_id),
                    &mut hits,
                    bucket,
                    want,
                );
            }
            // Scan the investigated location's attachment zone (Obscuring Fog
            // 01168). Derive the location from the still-live in-flight test
            // frame — teardown is at PostOnResolution, well after this fires.
            if let Some(loc_id) = state.current_skill_test().and_then(|t| t.tested_location) {
                if let Some(loc) = state.locations.get(&loc_id) {
                    for att in &loc.attachments {
                        push_matching(
                            reg,
                            &att.code,
                            *investigator,
                            Some(att.instance_id),
                            &mut hits,
                            bucket,
                            want,
                        );
                    }
                }
            }
        }
```

(Confirm `EventPattern` is in scope in this file — it is, used by the other arms. `current_skill_test()` is a `GameState` method already used elsewhere.)

- [ ] **Step 8: Update the reaction-scan arms**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, replace the `trigger_matches` arm

```rust
        (
            TimingEvent::SuccessfullyInvestigated { investigator, .. },
            EventPattern::SuccessfullyInvestigated,
        ) => *investigator == controller,
```
with
```rust
        // "after you succeed/fail a skill test" — scoped to the controller's
        // own test ("after **you** …"), narrowed by outcome and (optionally)
        // test kind. Dr. Milan 01033 is { Success, Some(Investigate) }.
        (
            TimingEvent::SkillTestResolved {
                investigator,
                kind,
                outcome,
            },
            EventPattern::SkillTestResolved {
                outcome: p_out,
                kind: p_kind,
            },
        ) => {
            *investigator == controller
                && outcome == p_out
                && (p_kind.is_none() || *p_kind == Some(*kind))
        }
```

And in `run_reaction_continuation` replace `| TimingEvent::SuccessfullyInvestigated { .. }` with `| TimingEvent::SkillTestResolved { .. }`.

- [ ] **Step 9: Update the emit site (behaviour-preserving)**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, in `emit_success_reactions_step`, the body still fires only on Investigate + success, but now emits the general event (no `location` — the forced scan derives it). Replace the `if succeeded && matches!(follow_up, …Investigate)` block's `emit_event(... SuccessfullyInvestigated { investigator, location })` with:

```rust
    if succeeded && matches!(follow_up, SkillTestFollowUp::Investigate) {
        let kind = cx
            .state
            .current_skill_test()
            .expect("emit_success_reactions_step: the SkillTest frame must persist")
            .kind;
        return super::emit::emit_event(
            cx,
            &super::emit::TimingEvent::SkillTestResolved {
                investigator,
                kind,
                outcome: crate::dsl::TestOutcome::Success,
            },
        );
    }
```

The `location` lookup that fed the old event is now dead — delete it. Add `TestOutcome` to the `crate::dsl::{…}` import in this file if not already present.

- [ ] **Step 10: Reroute Dr. Milan (reaction)**

In `crates/cards/src/impls/dr_milan_christopher.rs`, change the `abilities()` reaction to the new pattern, and add the needed imports (`SkillTestKind`, `TestOutcome`):

```rust
        reaction_on_event(
            EventPattern::SkillTestResolved {
                outcome: TestOutcome::Success,
                kind: Some(SkillTestKind::Investigate),
            },
            EventTiming::After,
            gain_resources(InvestigatorTarget::You, 1),
        ),
```

Update the `tests` module's assertion to the same pattern. Update the module-level doc comment if it names the old pattern.

- [ ] **Step 11: Reroute Obscuring Fog (forced)**

In `crates/cards/src/impls/obscuring_fog.rs`, change the forced ability to:

```rust
        forced_on_event(
            EventPattern::SkillTestResolved {
                outcome: TestOutcome::Success,
                kind: Some(SkillTestKind::Investigate),
            },
            EventTiming::After,
            discard_self(),
        ),
```

Add `SkillTestKind, TestOutcome` to the imports, and update the `tests` module assertion (`abilities[2].trigger`) to match.

- [ ] **Step 12: Update the `SkillTestStep` doc comment**

In `crates/game-core/src/state/game_state.rs`, the `SkillTestStep::EmitSuccessReactions` doc references "SuccessfullyInvestigated". Update the prose to say it fires the `SkillTestResolved` timing point (still gated to Investigate + success at this task; generalized in Task 2).

- [ ] **Step 13: Run the strict gauntlet**

Run the six Global-Constraint commands.
Expected: all green, with **no assertion changes** in `crates/cards/tests/*` and only the pattern-string changes in the two card unit tests. The Dr. Milan reaction and Obscuring Fog forced-discard behaviours are unchanged.

- [ ] **Step 14: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch/emit.rs crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/reaction_windows.rs crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/src/state/game_state.rs crates/cards/src/impls/dr_milan_christopher.rs crates/cards/src/impls/obscuring_fog.rs
git commit -m "engine: subsume SuccessfullyInvestigated into SkillTestResolved timing point (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Task 2: Generalize the emission to every test/outcome

The timing point now fires for **every** resolved skill test (all kinds, success *and* failure), not just Investigate-success. Empty windows are free (`queue_reaction_window` early-returns on no candidates; `collect_forced_hits` returns empty), so plain tests with no listener are unchanged.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`emit_success_reactions_step` → `emit_outcome_reactions_step`)
- Modify: `crates/game-core/src/state/game_state.rs` (`SkillTestStep::EmitSuccessReactions` → `EmitOutcomeReactions`)
- Create: `crates/game-core/tests/skill_test_outcome_timing.rs`

**Interfaces:**
- Consumes: `TimingEvent::SkillTestResolved` (Task 1).

- [ ] **Step 1: Write the failing integration test (fires for a non-Investigate test)**

Create `crates/game-core/tests/skill_test_outcome_timing.rs`. Model the fixture-registry + skill-test setup on `crates/game-core/tests/on_skill_test_resolution.rs` (read it first for the exact `GameStateBuilder`, chaos-bag, and `card_registry::install` calls). The fixture card is an in-play asset whose `abilities()` is a single `reaction_on_event(EventPattern::SkillTestResolved { outcome: TestOutcome::Success, kind: None }, After, gain_resources(You, 1))`. Drive a passing **Plain** `PerformSkillTest` (kind = Plain, not Investigate) for an investigator who controls that asset; commit no cards; force a passing token via a single-`Numeric(+N)` chaos bag.

```rust
// Fixture: an in-play asset reacting to ANY successful skill test.
const REACTOR: &str = "test-skilltest-reactor";

fn fixture_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    (code.as_str() == REACTOR).then(|| {
        vec![reaction_on_event(
            EventPattern::SkillTestResolved {
                outcome: TestOutcome::Success,
                kind: None,
            },
            EventTiming::After,
            gain_resources(InvestigatorTarget::You, 1),
        )]
    })
}

#[test]
fn general_timing_point_fires_for_non_investigate_test() {
    // (Install a CardRegistry whose abilities_for = fixture_abilities and
    // metadata_for returns minimal asset metadata for REACTOR. Build a state
    // with one investigator controlling a REACTOR instance in play, a chaos
    // bag of a single Numeric(+5) token, and run a passing Plain PerformSkillTest
    // at difficulty 0. Then resolve the reaction window by firing the reaction.)
    // Assert the reaction resolved: the investigator gained 1 resource.
    assert_eq!(resources_after, resources_before + 1);
}
```

Fill in the registry install, builder, and resolution-input plumbing exactly as `on_skill_test_resolution.rs` does (it already commits cards and resolves windows through real `apply`). The assertion is that the reaction's `gain_resources` ran — proving the timing point fired for a Plain test.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core --test skill_test_outcome_timing general_timing_point_fires_for_non_investigate_test`
Expected: FAIL — the reaction never fires because the emit is still gated on Investigate + success (no resource gained).

- [ ] **Step 3: Generalize the emit step**

In `skill_test.rs`, rename `emit_success_reactions_step` → `emit_outcome_reactions_step` and drop the Investigate/success gate so it fires for every test:

```rust
/// RR ST.6→ST.7 boundary — fire the general "after you succeed/fail a skill
/// test" timing point ([`TimingEvent::SkillTestResolved`]) on the outcome
/// established at ST.6, before any ST.7 consequence. Fires for **every** test
/// and both outcomes; the forced/reaction scans (and an empty candidate set)
/// decide whether any window actually opens. Pre-advances the cursor to
/// `FireOnCommit` first, so a suspending window resumes past this step.
fn emit_outcome_reactions_step(
    cx: &mut Cx,
    investigator: InvestigatorId,
    succeeded: bool,
    failed_by: u8,
) -> EngineOutcome {
    let kind = cx
        .state
        .current_skill_test()
        .expect("emit_outcome_reactions_step: the SkillTest frame must persist")
        .kind;
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = SkillTestStep::FireOnCommit {
        succeeded,
        failed_by,
    };
    let outcome = if succeeded {
        crate::dsl::TestOutcome::Success
    } else {
        crate::dsl::TestOutcome::Failure
    };
    super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::SkillTestResolved {
            investigator,
            kind,
            outcome,
        },
    )
}
```

Rename the `SkillTestStep::EmitSuccessReactions` variant → `EmitOutcomeReactions` in `state/game_state.rs` (keep its `{ succeeded, failed_by }` payload and update its doc to "fires for every test/outcome"), and update the `advance` match arm name + the call to `emit_outcome_reactions_step`. The arm body is unchanged (it already returns the outcome on `AwaitingInput`).

- [ ] **Step 4: Run the new test + the behaviour net**

Run:
```bash
cargo test -p game-core --test skill_test_outcome_timing
cargo test -p cards   # Dr. Milan + Obscuring Fog still green (Investigate narrowing)
cargo test -p game-core --lib engine::dispatch::skill_test
```
Expected: all PASS — the Plain-test reaction now fires; Investigate-narrowed consumers unaffected.

- [ ] **Step 5: Add the "no spurious window" test**

In the same test file, add a test that a passing Plain `PerformSkillTest` with **no** listener in play resolves straight to `Done` (no `AwaitingInput`) — proving the generalized emit opens no window when nothing matches.

```rust
#[test]
fn general_timing_point_opens_no_window_without_a_listener() {
    // Same setup minus the REACTOR asset (registry returns None for abilities).
    // Run the passing Plain PerformSkillTest to completion.
    assert!(matches!(final_outcome, EngineOutcome::Done));
}
```

Run: `cargo test -p game-core --test skill_test_outcome_timing` — Expected: PASS.

- [ ] **Step 6: Full strict gauntlet, then commit**

Run the six Global-Constraint commands.
```bash
git add crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/src/state/game_state.rs crates/game-core/tests/skill_test_outcome_timing.rs
git commit -m "engine: fire SkillTestResolved for every test/outcome (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Task 3: Symbol effects at ST.4/ST.7 via suspendable `Effect::Deal`

Re-order chaos-symbol side-effects to their RR steps and apply them through pushed `Effect::Deal` (interactive `soak_and_distribute`, may suspend): `immediate` at ST.4 (before the determination), `on_fail` at ST.7 (after the timing point). Split `Resolving` so the determination is computed once and carried across the ST.4 yield (no re-draw), and fold the ST.6 logged event + the timing-point emit into one `DetermineOutcome` step.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`InFlightSkillTest.resolved` + `ResolvedTest`; `SkillTestStep`: drop `EmitOutcomeReactions`, add `DetermineOutcome` + `ApplySymbolOnFail`)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (Resolving / DetermineOutcome / ApplySymbolOnFail; replace `apply_symbol_outcome`)
- Modify: `crates/game-core/tests/skill_test_outcome_timing.rs` (symbol-ordering + soak-suspend tests)
- Modify (tests only): any `InFlightSkillTest { … }` literal (e.g. `skill_test.rs` `fight_follow_up_adds_bonus_attack_damage`) gains `resolved: None`.

**Interfaces:**
- Consumes: `push_effect(cx, &Effect, EvalContext)`; `Effect::Deal { kind: HarmKind, target: InvestigatorTarget, amount: u8 }`; `TimingEvent::SkillTestResolved` (Task 1); `crate::scenario::{resolve_symbol_token, SymbolOutcome, TokenEffect}`.
- Produces: `InFlightSkillTest.resolved: Option<ResolvedTest>`; `SkillTestStep::{DetermineOutcome, ApplySymbolOnFail { succeeded }}`.

- [ ] **Step 1: Add `ResolvedTest` and the frame field**

In `crates/game-core/src/state/game_state.rs`, add the struct (near `InFlightSkillTest`):

```rust
/// The chaos-token determination computed at the `Resolving` step (RR
/// ST.5/ST.6), carried forward so (a) the logged `SkillTestSucceeded`/
/// `SkillTestFailed` and the `SkillTestResolved` timing point are emitted at
/// `DetermineOutcome` — *after* the ST.4 `immediate` symbol effects, which may
/// suspend on a soak window — and (b) the ST.7 `symbol_on_fail` effect is
/// available at `ApplySymbolOnFail`. The chaos token is drawn once at
/// `Resolving`; nothing re-draws it on resume. `None` until the token is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTest {
    /// Whether the test passed (ST.6).
    pub succeeded: bool,
    /// Failure margin (`difficulty - total`, clamped ≥ 0); `0` on success.
    pub failed_by: u8,
    /// Success margin (`total - difficulty`, ≥ 0 on success); for the logged
    /// `SkillTestSucceeded { margin }`.
    pub margin: i8,
    /// Why the test failed (meaningful only when `!succeeded`).
    pub fail_reason: crate::event::FailureReason,
    /// The chaos symbol's result-conditional `on_fail` effect (Cultist's
    /// horror), built as an `Effect` and pushed at `ApplySymbolOnFail` (ST.7).
    /// `None` when the test passed or the symbol has no `on_fail`.
    pub symbol_on_fail: Option<card_dsl::dsl::Effect>,
}
```

Add the field to `InFlightSkillTest` (after `bonus_attack_damage`):

```rust
    /// Post-draw determination + ST.7 symbol on_fail, set at `Resolving` and
    /// read at `DetermineOutcome` / `ApplySymbolOnFail`. `None` pre-draw.
    pub resolved: Option<ResolvedTest>,
```

(`FailureReason` already derives `Copy, Serialize, Deserialize`; `card_dsl::dsl::Effect` is already serializable and used by the sibling `on_fail`/`on_success` fields.)

- [ ] **Step 2: Initialize the new field everywhere `InFlightSkillTest` is constructed**

In `skill_test.rs` `start_skill_test`, add `resolved: None,` to the `InFlightSkillTest { … }` literal. Then:

Run: `cargo build -p game-core --tests`
Expected: FAIL with "missing field `resolved`" at each test literal (e.g. `fight_follow_up_adds_bonus_attack_damage`). Add `resolved: None,` to each until it builds. Do not change `SkillTestStep` yet.

- [ ] **Step 3: Replace the `SkillTestStep` step variants**

In `state/game_state.rs`, delete `EmitOutcomeReactions { succeeded, failed_by }` and add:

```rust
    /// RR ST.6→ST.7 — emit the logged `SkillTestSucceeded`/`SkillTestFailed`
    /// (now, *after* the ST.4 immediate symbol effects) and the general
    /// `SkillTestResolved` timing point, reading the determination off
    /// [`InFlightSkillTest::resolved`]. Pre-advances to `FireOnCommit`. The
    /// determination was computed at `Resolving`; this step never re-draws.
    /// (Slice D #423.)
    DetermineOutcome,
    /// RR ST.7 — push the chaos symbol's result-conditional `on_fail` effect
    /// (Cultist 01104's horror) as an `Effect::Deal`, when the test failed.
    /// Sits among the ST.7 result effects (after the card `on_fail` of
    /// `ApplyResultEffect`); RR lets the test-performer order multiple results,
    /// the engine sequences deterministically. Pushes via `Effect::Deal` so a
    /// sanity-soak (Holy Rosary 01028) suspends cleanly. Pre-advances to
    /// `FireOnResolution`. (Slice D #423.)
    ApplySymbolOnFail {
        /// Threaded outcome; the symbol `on_fail` pushes only on failure.
        succeeded: bool,
    },
```

This will break `skill_test.rs`'s `advance` (the deleted variant + the missing arms). Fixed in the next steps.

- [ ] **Step 4: Restructure the `Resolving` arm + `run_resolution`**

In `skill_test.rs`, rewrite `run_resolution` so it draws once, computes the determination (pure), stores `resolved` on the frame, pushes the `immediate` symbol effects, and advances to `DetermineOutcome`. Replace `resolve_chaos_token_and_emit` with a `resolve_chaos_token` that **does not** emit the determination events and **does not** apply symbol effects — it returns the raw resolution so the caller computes the outcome:

```rust
/// RR ST.3–ST.6 computation. Draws the chaos token (records RNG), resolves any
/// scenario symbol outcome, emits `ChaosTokenRevealed`, computes the
/// determination (ST.5 total, ST.6 success/failure) and stashes it on the
/// in-flight frame as [`ResolvedTest`] (so the logged events fire later at
/// `DetermineOutcome`, after the ST.4 immediate effects). Then pushes the
/// symbol's `immediate` effects (ST.4) as one `Effect::Deal` `Seq` and
/// pre-advances to `DetermineOutcome`. Pushes nothing else; returns `Done`
/// (the pushed immediate effect, if any, becomes the top frame → `advance`
/// yields and re-dispatches at `DetermineOutcome`).
fn run_resolution(cx: &mut Cx, investigator: InvestigatorId, indices_u8: &[u8]) {
    let (skill, kind, difficulty) = {
        let t = cx
            .state
            .current_skill_test()
            .expect("run_resolution: the SkillTest frame must exist");
        (t.skill, t.kind, t.difficulty)
    };

    // ST.3 draw + resolve symbol; ST.5 inputs.
    let token_idx = cx.state.rng.next_index(cx.state.chaos_bag.tokens.len());
    let token = cx.state.chaos_bag.tokens[token_idx];
    let symbol_outcome = match token {
        ChaosToken::Skull | ChaosToken::Cultist | ChaosToken::Tablet | ChaosToken::ElderThing => {
            crate::scenario::resolve_symbol_token(cx.state, token, investigator)
        }
        _ => None,
    };
    let resolution = match &symbol_outcome {
        Some(o) => TokenResolution::Modifier(o.modifier),
        None => resolve_token(token, &cx.state.token_modifiers),
    };
    cx.events
        .push(Event::ChaosTokenRevealed { token, resolution });

    // ST.4 immediate symbol effects (before the total), pushed as Effect::Deal.
    if let Some(o) = &symbol_outcome {
        push_symbol_effects(cx, investigator, &o.immediate);
    }

    // ST.5 modified skill value; ST.6 success/failure (pure — not emitted yet).
    let skill_value = sum_skill_value(cx.state, investigator, skill, kind, indices_u8);
    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => (skill_value.saturating_add(n).max(0), FailureReason::Total),
        TokenResolution::ElderSign => (skill_value.max(0), FailureReason::Total),
        TokenResolution::AutoFail => (0, FailureReason::AutoFail),
    };
    let auto_fail = matches!(resolution, TokenResolution::AutoFail);
    let margin = total.saturating_sub(difficulty);
    let succeeded = margin >= 0 && !auto_fail;
    let failed_by = if succeeded { 0 } else { difficulty.saturating_sub(total) };

    // Build the ST.7 symbol on_fail effect (Cultist horror) for later; None on success.
    let symbol_on_fail = if succeeded {
        None
    } else {
        symbol_outcome
            .as_ref()
            .and_then(|o| symbol_effects_to_effect(&o.on_fail))
    };

    // Stash the determination + pre-advance. The token is now consumed; nothing re-draws.
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("run_resolution: the SkillTest frame must exist");
    t.resolved = Some(crate::state::ResolvedTest {
        succeeded,
        failed_by: u8::try_from(failed_by).unwrap_or(0),
        margin,
        fail_reason,
        symbol_on_fail,
    });
    t.continuation = SkillTestStep::DetermineOutcome;
}
```

Notes for the implementer: the old `resolve_chaos_token_and_emit` also handled `apply_symbol_outcome(... succeeded)` and emitted the determination — both are removed here (immediate → pushed above; logged events → `DetermineOutcome`; on_fail → `ApplySymbolOnFail`). Keep the `ChaosToken`, `TokenResolution`, `resolve_token`, `FailureReason` imports. Delete `resolve_chaos_token_and_emit` and `apply_symbol_outcome`.

The `Resolving` arm becomes:

```rust
            SkillTestStep::Resolving => {
                // ST.3–ST.6: draw once, compute, push ST.4 immediate symbol
                // effects, advance to DetermineOutcome. If an immediate effect
                // was pushed it is the new top frame → the loop yields (it may
                // suspend on a soak window) and re-dispatches at DetermineOutcome.
                run_resolution(cx, investigator, &indices_u8);
            }
```

- [ ] **Step 5: Add the symbol-effect push helpers**

In `skill_test.rs`, replace `apply_symbol_outcome` with:

```rust
/// Convert symbol [`TokenEffect`]s into a single `Effect` (a `Seq` of
/// `Effect::Deal` targeting the tester), or `None` if empty. `Effect::Deal`
/// routes through the interactive `soak_and_distribute` path, so a soak asset
/// makes these suspend (RR-correct: the player assigns damage to soak assets).
fn symbol_effects_to_effect(effects: &[crate::scenario::TokenEffect]) -> Option<card_dsl::dsl::Effect> {
    use crate::dsl::{Effect, HarmKind, InvestigatorTarget};
    use crate::scenario::TokenEffect;
    let deals: Vec<Effect> = effects
        .iter()
        .map(|e| match e {
            TokenEffect::Damage(n) => Effect::Deal {
                kind: HarmKind::Damage,
                target: InvestigatorTarget::You,
                amount: *n,
            },
            TokenEffect::Horror(n) => Effect::Deal {
                kind: HarmKind::Horror,
                target: InvestigatorTarget::You,
                amount: *n,
            },
        })
        .collect();
    match deals.len() {
        0 => None,
        1 => deals.into_iter().next(),
        _ => Some(Effect::Seq(deals)),
    }
}

/// Push the symbol effects (built by [`symbol_effects_to_effect`]) for the
/// drive loop, controller-scoped to the tester (symbol effects have no source).
fn push_symbol_effects(
    cx: &mut Cx,
    investigator: InvestigatorId,
    effects: &[crate::scenario::TokenEffect],
) {
    if let Some(effect) = symbol_effects_to_effect(effects) {
        push_effect(cx, &effect, EvalContext::for_controller(investigator));
    }
}
```

- [ ] **Step 6: Add the `DetermineOutcome` arm**

In `advance`, add:

```rust
            SkillTestStep::DetermineOutcome => {
                // ST.6 logged event (now after the ST.4 immediate effects) +
                // the general SkillTestResolved timing point, folded. Read the
                // determination off the frame; pre-advance to FireOnCommit
                // before emitting (suspend/resume invariant).
                let outcome = determine_outcome_step(cx, investigator);
                if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                    return outcome;
                }
                // Else emit_event may have pushed a reaction window — the
                // top-of-loop check yields; or nothing matched and we fall into
                // FireOnCommit.
            }
```

And the helper:

```rust
/// RR ST.6→ST.7. Emit the logged `SkillTestSucceeded`/`SkillTestFailed` from
/// the stashed [`ResolvedTest`] (the ST.4 immediate effects have already run),
/// then the general `SkillTestResolved` timing point. Pre-advances to
/// `FireOnCommit` before the emit. Returns the emit outcome.
fn determine_outcome_step(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let (skill, kind, resolved) = {
        let t = cx
            .state
            .current_skill_test()
            .expect("determine_outcome_step: the SkillTest frame must persist");
        (
            t.skill,
            t.kind,
            t.resolved
                .clone()
                .expect("determine_outcome_step: Resolving must have stashed the determination"),
        )
    };
    if resolved.succeeded {
        cx.events.push(Event::SkillTestSucceeded {
            investigator,
            skill,
            margin: resolved.margin,
        });
    } else {
        cx.events.push(Event::SkillTestFailed {
            investigator,
            skill,
            reason: resolved.fail_reason,
            by: resolved.failed_by,
        });
    }
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = SkillTestStep::FireOnCommit {
        succeeded: resolved.succeeded,
        failed_by: resolved.failed_by,
    };
    let outcome = if resolved.succeeded {
        crate::dsl::TestOutcome::Success
    } else {
        crate::dsl::TestOutcome::Failure
    };
    super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::SkillTestResolved {
            investigator,
            kind,
            outcome,
        },
    )
}
```

Delete the old `emit_outcome_reactions_step` (its work is now split across `run_resolution` + `determine_outcome_step`).

- [ ] **Step 7: Insert `ApplySymbolOnFail` into the ST.7 sequence**

The ST.7 cursor chain currently runs `…ApplyResultEffect → FireOnResolution…`. Make `apply_result_effect_step` pre-advance to `ApplySymbolOnFail { succeeded }` instead of `FireOnResolution`, and add the new arm that pushes the symbol on_fail then advances to `FireOnResolution`.

In `apply_result_effect_step`, change the pre-advance target:

```rust
        .continuation = SkillTestStep::ApplySymbolOnFail { succeeded };
```

Add the `advance` arm:

```rust
            SkillTestStep::ApplySymbolOnFail { succeeded } => {
                // RR ST.7 — push the chaos symbol's on_fail effect (Cultist
                // horror) when the test failed. Pre-advance to FireOnResolution
                // first; the pushed Deal may suspend on a sanity-soak.
                cx.state
                    .current_skill_test_mut()
                    .expect("the SkillTest frame must persist across driver steps")
                    .continuation = SkillTestStep::FireOnResolution {
                    succeeded,
                    next: 0,
                };
                if !succeeded {
                    let on_fail = cx
                        .state
                        .current_skill_test()
                        .and_then(|t| t.resolved.as_ref())
                        .and_then(|r| r.symbol_on_fail.clone());
                    if let Some(effect) = on_fail {
                        push_effect(cx, &effect, EvalContext::for_controller(investigator));
                    }
                }
            }
```

(The `FireOnResolution { succeeded, next: 0 }` payload matches the existing variant — confirm by reading its definition before writing.)

- [ ] **Step 8: Fix the `apply_symbol_outcome` unit test**

The `apply_symbol_outcome_runs_immediate_always_and_on_fail_only_on_failure` test in `skill_test.rs` calls the deleted fn. Replace it with a unit test of `symbol_effects_to_effect`:

```rust
#[test]
fn symbol_effects_to_effect_builds_deal_seq() {
    use crate::dsl::{Effect, HarmKind, InvestigatorTarget};
    use crate::scenario::TokenEffect;
    assert_eq!(symbol_effects_to_effect(&[]), None);
    assert_eq!(
        symbol_effects_to_effect(&[TokenEffect::Horror(1)]),
        Some(Effect::Deal {
            kind: HarmKind::Horror,
            target: InvestigatorTarget::You,
            amount: 1,
        })
    );
    assert!(matches!(
        symbol_effects_to_effect(&[TokenEffect::Damage(1), TokenEffect::Horror(2)]),
        Some(Effect::Seq(v)) if v.len() == 2
    ));
}
```

- [ ] **Step 9: Run the lib tests + behaviour net**

Run:
```bash
cargo test -p game-core --lib engine::dispatch::skill_test
cargo test -p cards --test revelation_treacheries   # Crypt Chill / Grasping Hands on_fail suspend
cargo test -p cards   # Dr. Milan / Obscuring Fog
```
Expected: PASS. If a scenario test in `crates/scenarios` asserts the *old* symbol-effect event order (Tablet damage after `SkillTestFailed`, or Cultist horror before the timing point), update its expected order to: `ChaosTokenRevealed` → [immediate `DamageTaken`] → `SkillTestSucceeded`/`SkillTestFailed` → … → [ST.7 `on_fail` `HorrorTaken`].

- [ ] **Step 10: Add the ST.4 ordering test**

In `crates/game-core/tests/skill_test_outcome_timing.rs`, add a test driving a Tablet draw with a Ghoul in play (use The Gathering scenario module, or a fixture symbol hook) and assert `DamageTaken` precedes `SkillTestSucceeded`/`SkillTestFailed` in the event slice (event subsequence).

```rust
#[test]
fn immediate_symbol_damage_precedes_the_determination() {
    // Tablet (with a Ghoul in play) -> immediate Damage(1). Drive a test and
    // assert the DamageTaken event index < the SkillTest{Succeeded|Failed} index.
}
```

- [ ] **Step 11: Add the soak-suspend (no re-draw) test**

Add a test: a Tablet draw (Ghoul in play) while the tester controls a health-bearing soak asset opens an `AwaitingInput` (soak distribution/window); resuming it completes the test and the event log contains exactly **one** `ChaosTokenRevealed` (proving no re-draw across the suspend).

```rust
#[test]
fn symbol_damage_suspends_on_soak_without_redrawing() {
    // Drive to the soak AwaitingInput, resolve it, then assert:
    //   - the test reaches Done, and
    //   - count of ChaosTokenRevealed events == 1.
}
```

- [ ] **Step 12: Add the ST.7 on_fail ordering test**

Add a test: a Cultist draw on a **failed** test emits its `HorrorTaken` *after* `SkillTestFailed` and after the `SkillTestResolved` window; on a **passed** test, no symbol `HorrorTaken`. (Assert via event subsequence / absence.)

Run: `cargo test -p game-core --test skill_test_outcome_timing` — Expected: PASS.

- [ ] **Step 13: Full strict gauntlet**

Run all six Global-Constraint commands. Expected: all green.

- [ ] **Step 14: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/tests/skill_test_outcome_timing.rs
# plus any crates/scenarios test files whose expected event order was updated
git commit -m "engine: chaos-symbol effects at ST.4/ST.7 via suspendable Effect::Deal (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Self-Review notes

- **Spec coverage:** subsume types → Task 1; general pattern + `Option<SkillTestKind>` narrowing → Task 1/2; fire for every test → Task 2; ST.4 immediate + ST.7 on_fail via `Effect::Deal` (suspendable) → Task 3; drawn-once / determination carried across yield → Task 3 (`ResolvedTest`); folded `DetermineOutcome` emit → Task 3; location derived from `tested_location` → Task 1 (`collect_forced_hits`); reaction reroute → Task 1. New tests cover: fires-for-non-investigate, no-spurious-window, ST.4 ordering, soak-suspend-no-redraw, ST.7 on_fail ordering.
- **Type consistency:** `EventPattern::SkillTestResolved { outcome: TestOutcome, kind: Option<SkillTestKind> }`; `TimingEvent`/`ForcedTriggerPoint::SkillTestResolved { investigator, kind: SkillTestKind, outcome: TestOutcome }`; `ResolvedTest { succeeded: bool, failed_by: u8, margin: i8, fail_reason: FailureReason, symbol_on_fail: Option<Effect> }`; `SkillTestStep::{DetermineOutcome, ApplySymbolOnFail { succeeded: bool }}`; `Effect::Deal { kind: HarmKind, target: InvestigatorTarget, amount: u8 }`. The matcher idiom `*o == *outcome && (k.is_none() || *k == Some(*kind))` is used identically in the forced and reaction scans.
- **Behaviour preservation:** Task 1 changes no card-test assertions except the pattern strings; Task 2 only widens emission (empty windows are free); Task 3's intended behaviour changes (Tablet→ST.4, Cultist→ST.7, interactive soak) are covered by new + updated tests. The `crates/cards/*` suites are the regression net throughout.
- **Open implementation choices pinned:** determination carried on the frame (`InFlightSkillTest.resolved`), not the Copy cursor, because the symbol `on_fail` `Effect` is non-Copy; `ApplySymbolOnFail` placed after `ApplyResultEffect` (order-among-ST.7-results not load-bearing in scope).
