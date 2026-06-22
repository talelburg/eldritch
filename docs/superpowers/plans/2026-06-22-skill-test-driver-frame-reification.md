# Skill-test Driver Frame Reification — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the skill-test driver a frame the uniform `drive` loop owns end-to-end — commit emission, resolution, and teardown all flow through one `advance(cx)` driver — without changing any observable behaviour.

**Architecture:** Rename `FinishContinuation` → `SkillTestStep` and `drive_skill_test` → `advance`; add an explicit `Resolving` step so `finish_skill_test`'s body becomes a cursor arm; relocate the post-teardown tail (forced-run sibling / end-of-turn resume) into the `PostOnResolution` arm; add a `drive`-loop `SkillTest` arm that drives the commit→resolution transition; emit the commit prompt from `advance`'s `AwaitingCommit` arm. The five imperative re-entry sites are kept (renamed to `advance`), not deleted — eliminating them couples to encounter-card disposal (#380), out of scope.

**Tech Stack:** Rust (workspace: `game-core` kernel, `cards` content). No new dependencies. Event-sourced engine; continuation-frame control flow.

**Spec:** `docs/superpowers/specs/2026-06-22-skill-test-driver-frame-reification-design.md`

## Global Constraints

- **Behaviour-preserving.** The full existing engine + integration suite must stay green at every task boundary. This refactor changes *structure*, not rules. If an existing test changes behaviour, STOP and investigate — do not "fix" the test to match.
- **Zero new `Continuation` variants.** Only `SkillTestStep` gains the `Resolving` variant. The continuation-stack enum is untouched.
- **Match CI's strict flags before every commit** (from `CLAUDE.md`):
  ```sh
  RUSTFLAGS="-D warnings" cargo test --all --all-features
  cargo clippy --all-targets --all-features -- -D warnings
  cargo fmt --check
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
  ```
- **No silent approximation / validate-first-mutate-second.** Preserve the existing handler contract exactly; this refactor moves code, it does not relax any precondition.
- **Branch:** `engine/skilltest-frame-reification` (already created; the design spec is committed there).
- **Commit trailers** (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
  ```

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/game-core/src/state/game_state.rs` | `FinishContinuation` enum def + `InFlightSkillTest.continuation` field | Rename type → `SkillTestStep`; add `Resolving` variant |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | The driver (`start_skill_test`, `finish_skill_test`, `drive_skill_test`, `open_commit_window`, `resume_substitution_choice`) | Rename driver → `advance`; add `Resolving`/`AwaitingCommit` arms; relocate tail; fold `finish_skill_test` body; delete `open_commit_window` |
| `crates/game-core/src/engine/dispatch/mod.rs` | The `drive` loop + `resume_skill_test_commit` | Add `SkillTest` loop arm; shrink `resume_skill_test_commit` (drop tail, set `Resolving`, return `Done`) |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | Two re-entry sites (`close_reaction_window_at:933`, `resume_before_discover_window:1022`) | Rename call `drive_skill_test` → `advance` |
| `crates/game-core/src/engine/dispatch/choice.rs` | One re-entry site (`resume_effect_walk:114`) | Rename call → `advance` |
| `crates/game-core/src/engine/dispatch/combat.rs` | One re-entry site (`finish_attack_loop` Retaliate:1170) | Rename call → `advance` |
| `crates/game-core/src/engine/{evaluator.rs, event.rs, state/mod.rs, test_support/resolver.rs}` | Reference `FinishContinuation` (imports / doc-links) | Rename type → `SkillTestStep` |

**Characterization tests (existing — the backstop, not rewritten):**
- `crates/cards/tests/persistent_treachery.rs` — Frozen in Fear forced-run + end-of-turn resume (Tasks 3, 5).
- `crates/cards/tests/dr_milan.rs`, `crates/cards/tests/evidence.rs` — mid-test reaction windows (Tasks 3, 5).
- `crates/game-core/src/engine/dispatch/skill_test.rs::tests` (line 1092) — driver unit tests (all tasks).

---

### Task 1: Rename `FinishContinuation` → `SkillTestStep`

Pure mechanical rename of a distinct type name across the 7 files that reference it. No behaviour change.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (enum def + field doc), `crates/game-core/src/engine/dispatch/skill_test.rs`, `crates/game-core/src/engine/dispatch/reaction_windows.rs`, `crates/game-core/src/engine/evaluator.rs`, `crates/game-core/src/event.rs`, `crates/game-core/src/state/mod.rs`, `crates/game-core/src/test_support/resolver.rs`

**Interfaces:**
- Produces: the public enum is now `SkillTestStep` (variants unchanged: `AwaitingCommit`, `PostFollowUp { succeeded }`, `PostRetaliate { succeeded }`, `PostOnResolution { succeeded }`); `InFlightSkillTest.continuation: SkillTestStep`.

- [ ] **Step 1: Confirm the reference set**

Run: `git grep -l 'FinishContinuation' crates/`
Expected: exactly the 7 files listed above. If more appear, include them in the rename.

- [ ] **Step 2: Perform the rename**

Run:
```bash
git grep -l 'FinishContinuation' crates/ | xargs sed -i 's/FinishContinuation/SkillTestStep/g'
```

- [ ] **Step 3: Verify no occurrence remains**

Run: `git grep -n 'FinishContinuation' crates/`
Expected: no output (empty).

- [ ] **Step 4: Compile + full suite + lints**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS (rename only; behaviour identical).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: rename FinishContinuation -> SkillTestStep (skill-test reification)

Mechanical rename; the cursor now spans the whole test, not just the finish.
No behaviour change. Substrate for the driver reification.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 2: Rename `drive_skill_test` → `advance`

Mechanical rename of the driver function and its five call sites. No behaviour change.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (def at line ~395 + internal call at ~367), `crates/game-core/src/engine/dispatch/mod.rs` (call at ~350 is via `finish_skill_test`, not direct — check), `crates/game-core/src/engine/dispatch/reaction_windows.rs` (calls at ~933, ~1022), `crates/game-core/src/engine/dispatch/choice.rs` (call at ~114), `crates/game-core/src/engine/dispatch/combat.rs` (call at ~1170)

**Interfaces:**
- Produces: `pub(super) fn advance(cx: &mut Cx) -> EngineOutcome` (was `drive_skill_test`), called as `skill_test::advance` / `super::skill_test::advance`.

- [ ] **Step 1: Confirm the call set**

Run: `git grep -n 'drive_skill_test' crates/ | grep -v '//'`
Expected: the function def + the internal call (skill_test.rs) + the 3 external call sites (reaction_windows.rs ×2, choice.rs, combat.rs). (Doc-comment mentions are cosmetic; the rename will update them too.)

- [ ] **Step 2: Perform the rename (code + doc-comments)**

Run:
```bash
git grep -l 'drive_skill_test' crates/ | xargs sed -i 's/drive_skill_test/advance/g'
```

- [ ] **Step 3: Verify + check for accidental collisions**

Run: `git grep -n 'drive_skill_test' crates/`
Expected: no output.
Run: `git grep -n 'fn advance\b' crates/game-core/src/engine/dispatch/skill_test.rs`
Expected: exactly one `fn advance` (the renamed driver). If `advance` collides with another item in any touched file, the next step's compile will catch it.

- [ ] **Step 4: Compile + full suite + lints**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: rename drive_skill_test -> advance (skill-test reification)

Mechanical rename of the driver + its five call sites. No behaviour change.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 3: Relocate the teardown tail into `PostOnResolution`

Move the post-teardown tail (forced-run sibling re-drive + end-of-turn resume) from `resume_skill_test_commit` (mod.rs) into `advance`'s `PostOnResolution` arm (skill_test.rs), so it fires from the teardown step regardless of which resume re-entered the driver. Behaviour-preserving (the tail runs at the same moment — right after teardown).

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`PostOnResolution` arm, ~455–485)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resume_skill_test_commit`, ~346–391)
- Characterization: `crates/cards/tests/persistent_treachery.rs` (existing)

**Interfaces:**
- Consumes: `SkillTestStep` (Task 1), `advance` (Task 2).
- Produces: `resume_skill_test_commit` no longer inspects the frame beneath after `finish_skill_test`; the tail lives in `PostOnResolution`.

- [ ] **Step 1: Characterization checkpoint — confirm the existing tail tests pass NOW**

Run:
```bash
cargo test -p cards --test persistent_treachery
```
Expected: PASS. These pin the forced-run-sibling and end-of-turn-resume behaviour the tail provides. (They must stay PASS after the move.)

- [ ] **Step 2: Add the tail to `advance`'s `PostOnResolution` arm**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, the `PostOnResolution` arm currently ends with the teardown and `return EngineOutcome::Done;` (after the `take_skill_test` + `debug_assert!`). Replace that trailing `return EngineOutcome::Done;` with the relocated tail:

```rust
                let taken = cx.state.take_skill_test();
                debug_assert!(
                    taken.is_some(),
                    "skill-test teardown: no SkillTest frame on the continuation stack",
                );
                // Teardown tail (relocated from resume_skill_test_commit). The
                // test is fully torn down; resume whatever it was nested within.
                // A forced run beneath (2+ simultaneous EndOfTurn forced — two
                // Frozen in Fear copies, #213): fire its remaining siblings /
                // close it. An `InvestigatorTurn { ending }` beneath: a single
                // suspending EndOfTurn forced stranded `end_turn` before
                // rotation; resume it now (C4c, #235). A forced run owns its own
                // post-run continuation and never flags the turn frame, so it is
                // checked first.
                if matches!(
                    cx.state.continuations.last(),
                    Some(crate::state::Continuation::Resolution(f)) if f.is_forced()
                ) {
                    let idx = cx.state.continuations.len() - 1;
                    return super::reaction_windows::advance_resolution(cx, idx);
                }
                if let Some(crate::state::Continuation::InvestigatorTurn {
                    investigator,
                    ending: true,
                }) = cx.state.continuations.last()
                {
                    let active_id = *investigator;
                    return super::phases::resume_end_turn(cx, active_id);
                }
                return EngineOutcome::Done;
```

(If `super::reaction_windows::advance_resolution` or `super::phases::resume_end_turn` is not visible from `skill_test.rs`, widen its visibility to `pub(super)` — these are sibling modules under `dispatch`.)

- [ ] **Step 3: Remove the tail from `resume_skill_test_commit`**

In `crates/game-core/src/engine/dispatch/mod.rs`, replace the `PickMultiple` arm body so it returns `finish_skill_test`'s outcome directly (the post-`Done` tail block is gone):

```rust
fn resume_skill_test_commit(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    match response {
        InputResponse::PickMultiple { selected } => {
            let indices: Vec<u32> = selected.iter().map(|o| o.0).collect();
            skill_test::finish_skill_test(cx, &indices)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: skill-test commit window expects InputResponse::PickMultiple, \
                 got {other:?}",
            )
            .into(),
        },
    }
}
```

- [ ] **Step 4: Run the characterization tests — must still PASS**

Run:
```bash
cargo test -p cards --test persistent_treachery
```
Expected: PASS (behaviour unchanged; tail fires from the new location). If any test now fails or changes, STOP — the relocation altered a teardown path; investigate per the spec's "if any existing test changes, investigate" note before proceeding.

- [ ] **Step 5: Full suite + lints**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: relocate skill-test teardown tail into PostOnResolution

Move the forced-run-sibling re-drive (#213) and end-of-turn resume from
resume_skill_test_commit into advance's PostOnResolution arm, so the tail fires
from the teardown step regardless of which resume re-entered the driver.
Behaviour-preserving; persistent_treachery characterization tests stay green.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 4: Add the `Resolving` step + fold `finish_skill_test`'s body into `advance`

Introduce an explicit `SkillTestStep::Resolving` and move `finish_skill_test`'s post-validation body (sum → on_commit → token → follow-up → on_success/on_fail) into a new `Resolving` arm of `advance`. `finish_skill_test` shrinks to: validate indices, store them, set `step = Resolving`, call `advance`. Commit emission is still via `open_commit_window` (the `AwaitingCommit` arm stays `unreachable!` for now — Task 5 changes that). Behaviour-preserving.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add `Resolving` variant)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`finish_skill_test` body → `Resolving` arm; `advance` match gains `Resolving`)

**Interfaces:**
- Consumes: `SkillTestStep` (Task 1), `advance` (Task 2).
- Produces: `SkillTestStep::Resolving` (no payload); `advance` handles `AwaitingCommit` (unreachable), `Resolving` (the resolution body), `PostFollowUp`/`PostRetaliate`/`PostOnResolution` (unchanged). `finish_skill_test` = validate + store indices + set `Resolving` + `advance(cx)`.

- [ ] **Step 1: Add the `Resolving` variant**

In `crates/game-core/src/state/game_state.rs`, add to `enum SkillTestStep` (after `AwaitingCommit`):

```rust
    /// Commit submitted: the next driver iteration runs the resolution
    /// body (sum committed icons, fire `OnCommit`, resolve the chaos
    /// token, run the action follow-up + `on_success`/`on_fail`), then
    /// pre-advances to `PostFollowUp`.
    Resolving,
```

- [ ] **Step 2: Move the resolution body into `advance`'s `Resolving` arm**

In `crates/game-core/src/engine/dispatch/skill_test.rs`:

(a) `advance`'s match currently has `AwaitingCommit => unreachable!(...)`. Add the `Resolving` arm holding the body that today lives in `finish_skill_test` *after* index validation (the sum-skill through on_success/on_fail block, lines ~282–365), ending by falling through to continue the loop (which reads the pre-advanced `PostFollowUp`). Concretely the `Resolving` arm runs:

```rust
            SkillTestStep::Resolving => {
                // Sum committed icons, fire OnCommit buffs, resolve the chaos
                // token. (Body relocated verbatim from finish_skill_test.)
                let skill_value =
                    sum_skill_value(cx.state, investigator, /* skill */, /* kind */, &indices_u8);
                // ... persist committed_by_active (already done at commit), fire_on_commit,
                // resolve_chaos_token_and_emit -> (succeeded, failed_by) ...
                // Pre-advance to PostFollowUp BEFORE running the follow-up so a
                // suspending follow-up resumes at PostFollowUp (Cover Up 01007).
                cx.state
                    .current_skill_test_mut()
                    .expect("the SkillTest frame must persist across driver steps")
                    .continuation = SkillTestStep::PostFollowUp { succeeded };
                if succeeded {
                    let outcome = apply_skill_test_follow_up(cx, investigator, follow_up);
                    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                        return outcome;
                    }
                    debug_assert!(matches!(outcome, EngineOutcome::Done), /* msg */);
                    if let Some(effect) = &on_success {
                        let outcome = apply_effect(cx, effect, card_ctx(investigator));
                        debug_assert!(matches!(outcome, EngineOutcome::Done), /* msg */);
                    }
                } else if let Some(effect) = &on_fail {
                    let mut ctx = card_ctx(investigator);
                    ctx.set_failed_by(failed_by);
                    let outcome = apply_effect(cx, effect, ctx);
                    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                        return outcome;
                    }
                    debug_assert!(matches!(outcome, EngineOutcome::Done), /* msg */);
                }
                // fall through: loop reads PostFollowUp next
            }
```

> The relocated body needs `skill`, `kind`, `follow_up`, `on_fail`, `on_success`, `source` from the in-flight record and the committed `indices_u8`. The `advance` loop already snapshots `(continuation, investigator, indices_u8)` per iteration; extend that snapshot to also read `skill`, `kind`, `follow_up`, `on_fail.clone()`, `on_success.clone()`, `source` so the `Resolving` arm has them. Move the `card_ctx` closure and the `sum_skill_value` / `fire_on_commit` / `resolve_chaos_token_and_emit` calls verbatim from `finish_skill_test`.

(b) Shrink `finish_skill_test` to validation + store + set `Resolving` + `advance`:

```rust
pub(super) fn finish_skill_test(cx: &mut Cx, indices: &[u32]) -> EngineOutcome {
    let Some(in_flight) = cx.state.current_skill_test() else {
        return EngineOutcome::Rejected {
            reason: "skill-test commit: no in-flight skill test to resume".into(),
        };
    };
    if !matches!(in_flight.continuation, SkillTestStep::AwaitingCommit) {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill-test commit: commit window already closed (continuation {:?}); \
                 the engine is mid-resolution, not at the commit step",
                in_flight.continuation,
            )
            .into(),
        };
    }
    let investigator = in_flight.investigator;
    let indices_u8 = match validate_commit_indices(cx.state, investigator, indices) {
        Ok(v) => v,
        Err(rejected) => return rejected,
    };
    // Persist the committed indices and advance to Resolving; the driver runs
    // the resolution body from there.
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above");
    t.committed_by_active = indices_u8;
    t.continuation = SkillTestStep::Resolving;
    advance(cx)
}
```

(c) The `Resolving` arm reads `committed_by_active` (now stored by `finish_skill_test`) for `indices_u8`; drop the old in-body `committed_by_active.clone_from(...)` write since it is set in `finish_skill_test`.

- [ ] **Step 3: Compile + full suite**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
```
Expected: PASS. The resolution sequence is identical — `finish_skill_test` now reaches the same body via the `Resolving` arm instead of inline.

- [ ] **Step 4: Targeted skill-test + characterization tests**

Run:
```bash
cargo test -p game-core skill_test
cargo test -p cards --test persistent_treachery --test dr_milan --test evidence
```
Expected: PASS.

- [ ] **Step 5: Lints + docs**

Run:
```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: add SkillTestStep::Resolving; fold finish_skill_test body into advance

The commit-stage resolution body (sum icons, OnCommit, chaos token, follow-up,
on_success/on_fail) becomes advance's Resolving arm; finish_skill_test shrinks to
validate + store + set Resolving + advance. Behaviour-preserving.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 5: Commit-via-`awaiting()` + `drive` `SkillTest` arm + funnel entries

Make `advance`'s `AwaitingCommit` arm emit the commit prompt (folding `open_commit_window`); funnel `start_skill_test` and `resume_substitution_choice` through `advance`; add the `drive`-loop `SkillTest` arm; make `resume_skill_test_commit` set `Resolving` and return `Done` so the loop drives the commit→resolution transition. Behaviour-preserving — the commit `AwaitingInput` still propagates synchronously out of the entry path (halting an enclosing forced run).

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`AwaitingCommit` arm; `start_skill_test` tail; `resume_substitution_choice` tail; delete `open_commit_window`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`drive` loop `SkillTest` arm; `resume_skill_test_commit` → set `Resolving` + return `Done`)
- Characterization: `crates/cards/tests/persistent_treachery.rs`, `crates/cards/tests/dr_milan.rs`, `crates/cards/tests/evidence.rs` (existing); plus one new unit test for the loop-driven commit→resolution path.

**Interfaces:**
- Consumes: `SkillTestStep::Resolving` + `advance` (Task 4).
- Produces: `advance`'s `AwaitingCommit` arm returns the commit `PickMultiple` `AwaitingInput`; `open_commit_window` deleted; `drive` has a `SkillTest` arm; `resume_skill_test_commit` = validate + store + set `Resolving` + return `Done`.

- [ ] **Step 1: Characterization checkpoint — confirm relevant tests pass NOW**

Run:
```bash
cargo test -p cards --test persistent_treachery --test dr_milan --test evidence
cargo test -p game-core skill_test
```
Expected: PASS. These pin commit emission, mid-test windows, forced-run resume, and substitution flow.

- [ ] **Step 2: Make `AwaitingCommit` emit the commit prompt**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, replace `advance`'s `AwaitingCommit => unreachable!(...)` arm with the body of today's `open_commit_window`:

```rust
            SkillTestStep::AwaitingCommit => {
                // The frame's awaiting(): the test parks here for the player's
                // commit. Resolution (reaction/fast) frames push *above* this
                // frame when a window opens mid-test; it is popped at teardown.
                let (investigator, skill, difficulty) = {
                    let t = cx
                        .state
                        .current_skill_test()
                        .expect("advance(AwaitingCommit): in-flight test must exist");
                    (t.investigator, t.skill, t.difficulty)
                };
                return EngineOutcome::AwaitingInput {
                    request: InputRequest::prompt(format!(
                        "Commit cards from hand for {investigator:?}'s {skill:?} skill test \
                         (difficulty {difficulty}); submit InputResponse::PickMultiple with the \
                         hand indices as option ids. Empty selection commits no cards.",
                    )),
                    resume_token: ResumeToken(0),
                };
            }
```

- [ ] **Step 3: Funnel entries through `advance`; delete `open_commit_window`**

(a) `start_skill_test`: replace the trailing `open_commit_window(cx)` (line ~145) with `advance(cx)`.

(b) `resume_substitution_choice`: replace its trailing `open_commit_window(cx)` (line ~219) with `advance(cx)`.

(c) Delete the `open_commit_window` function (lines ~157–182).

- [ ] **Step 4: Add the `drive`-loop `SkillTest` arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, in the `drive` loop, add an arm alongside `ActionResolution` / `Effect` (before the `_ => return EngineOutcome::Done` catch-all):

```rust
            Some(Continuation::SkillTest(_)) => {
                // The commit->resolution transition (and any loop-reached
                // SkillTest): advance the driver. It either tears the test down
                // (frame gone; loop on) or suspends (AwaitingInput short-circuits
                // the loop). The five imperative re-entry sites still call
                // advance directly; this arm does not replace them.
                match skill_test::advance(cx) {
                    EngineOutcome::Done => {}
                    other => return other,
                }
            }
```

- [ ] **Step 5: Make `resume_skill_test_commit` set `Resolving` + return `Done`**

In `crates/game-core/src/engine/dispatch/mod.rs`, change the `PickMultiple` arm to validate + store + set `Resolving` + return `Done`, letting `apply_player_action`'s `drive` run `advance`. Move the validation helper call so it rejects cleanly on bad input (state unchanged):

```rust
        InputResponse::PickMultiple { selected } => {
            let indices: Vec<u32> = selected.iter().map(|o| o.0).collect();
            // Validate + store the commit and advance to Resolving; the drive
            // loop's SkillTest arm then runs advance(Resolving). Returns Rejected
            // (state unchanged) on no-in-flight / wrong-step / bad indices.
            skill_test::commit_indices(cx, &indices)
        }
```

Where `commit_indices` is `finish_skill_test` renamed/repurposed to *not* call `advance` itself — it validates, stores, sets `Resolving`, returns `Done`:

```rust
// in skill_test.rs
pub(super) fn commit_indices(cx: &mut Cx, indices: &[u32]) -> EngineOutcome {
    let Some(in_flight) = cx.state.current_skill_test() else {
        return EngineOutcome::Rejected {
            reason: "skill-test commit: no in-flight skill test to resume".into(),
        };
    };
    if !matches!(in_flight.continuation, SkillTestStep::AwaitingCommit) {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill-test commit: commit window already closed (continuation {:?}); \
                 the engine is mid-resolution, not at the commit step",
                in_flight.continuation,
            )
            .into(),
        };
    }
    let investigator = in_flight.investigator;
    let indices_u8 = match validate_commit_indices(cx.state, investigator, indices) {
        Ok(v) => v,
        Err(rejected) => return rejected,
    };
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above");
    t.committed_by_active = indices_u8;
    t.continuation = SkillTestStep::Resolving;
    EngineOutcome::Done
}
```

> Note: `commit_indices` returns `Done`, so the `Resolving` work happens in the loop (Step 4 arm). Any other internal caller of the old `finish_skill_test` (e.g. the `skill_test.rs::tests` at lines 1238/1275 call `finish_skill_test(&mut cx, &[])` directly) should switch to: `commit_indices(&mut cx, &[]); drive(&mut cx, EngineOutcome::Done)` — or keep a thin `finish_skill_test` that calls `commit_indices` then `advance`, whichever keeps those unit tests behaviour-identical. Prefer the thin wrapper to minimize test churn:
> ```rust
> #[cfg(test)]
> pub(super) fn finish_skill_test(cx: &mut Cx, indices: &[u32]) -> EngineOutcome {
>     match commit_indices(cx, indices) {
>         EngineOutcome::Done => advance(cx),
>         other => other,
>     }
> }
> ```

- [ ] **Step 6: Add a unit test for the loop-driven commit→resolution path**

In `crates/game-core/src/engine/dispatch/skill_test.rs::tests` (line ~1092), add a test that a basic skill test commits and resolves through `apply_player_action` (the public entry that runs `drive`), asserting `SkillTestStarted` then `SkillTestEnded` are emitted and the `SkillTest` frame is gone. Adapt the nearest existing test in that module for state construction (same `TestGame`/`Cx` setup):

```rust
#[test]
fn commit_resolves_through_the_drive_loop() {
    // Build a state with an in-flight basic skill test parked at AwaitingCommit
    // (reuse the setup of the existing tests in this module), then submit an
    // empty commit via the public apply entry so the drive loop runs advance.
    // Assert the test resolved: SkillTestEnded emitted, no SkillTest frame left.
    // (Fill state setup from the sibling test at line ~1238.)
}
```

Run it and confirm it passes:
```bash
cargo test -p game-core commit_resolves_through_the_drive_loop
```
Expected: PASS.

- [ ] **Step 7: Characterization tests — must still PASS**

Run:
```bash
cargo test -p cards --test persistent_treachery --test dr_milan --test evidence
cargo test -p game-core skill_test
```
Expected: PASS. If any fails, STOP and investigate (commit emission / forced-run halting / mid-test resume is the likely culprit).

- [ ] **Step 8: Full gauntlet (all seven CI jobs locally)**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all PASS. (If `wasm-pack` is available, also run `wasm-pack test --headless --firefox crates/web`.)

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: emit commit from advance + add drive SkillTest arm (reification done)

advance's AwaitingCommit arm now emits the commit prompt (open_commit_window
deleted); start_skill_test / resume_substitution_choice funnel through advance;
the drive loop gains a SkillTest arm that runs the commit->resolution transition;
commit resume sets Resolving + returns Done. The commit AwaitingInput still
propagates synchronously to halt an enclosing forced run. Behaviour-preserving.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 6: Phase-doc update (final commit, only once the PR is ready)

Per `CLAUDE.md` and `docs/phases/README.md`, update the phase doc **only** when the PR is ready to merge (CI green), as the final commit.

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Update ordering step 5**

In `docs/phases/phase-7-the-gathering.md`, ordering step 5 (the "Skill-test windows" entry): note that the **skill-test driver frame reification substrate shipped** (this PR #) — `drive_skill_test` → loop-driven `advance`; commit emitted via `AwaitingCommit`; teardown tail relocated to `PostOnResolution`; the five imperative re-entry sites kept (renamed), eliminating them deferred to the EmitEvent-frame slice (couples to #380). #374/#64 now land as cursor-step window insertions on this substrate. Add a **Decisions made** entry only if a future PR-author would choose differently without it — e.g. "skill-test windows insert at `advance` cursor steps, not new `Continuation` variants."

- [ ] **Step 2: Commit (after CI is green on the opened PR)**

```bash
git add docs/phases/phase-7-the-gathering.md && git commit -m "$(cat <<'EOF'
docs: phase-7 — skill-test driver reification substrate shipped

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

## Self-Review

**1. Spec coverage:**
- Rename `FinishContinuation` → `SkillTestStep` → Task 1. ✓
- `drive_skill_test` → `advance` → Task 2. ✓
- `Resolving` step + fold `finish_skill_test` body → Task 4. ✓
- Commit via `AwaitingCommit` / `awaiting()`; `open_commit_window` deleted; entries funnel through `advance` → Task 5. ✓
- `drive` `SkillTest` arm; commit resume sets `Resolving` + returns `Done` → Task 5. ✓
- Teardown tail relocated to `PostOnResolution` → Task 3. ✓
- Five re-entry sites kept, renamed (not deleted) → Tasks 2 (rename) + spec "deliberately kept". ✓
- Forced-run-below guard preserved verbatim → untouched (the `advance` window-check block is copied from `drive_skill_test`; Task 2 renames the fn but not the guard). ✓
- Entry funnels through `advance` so commit `AwaitingInput` propagates (halts forced run) → Task 5 Step 3/5. ✓
- Behaviour-preserving; full suite green at each boundary → every task's verification steps. ✓
- #423 out of scope (`apply_effect` sub-calls unchanged) → no task touches them. ✓

**2. Placeholder scan:** Task 4 Step 2's `Resolving` arm uses `/* skill */`, `/* kind */`, `/* msg */` ellipses inside a "relocate verbatim" block — these are explicit pointers to copy the existing `finish_skill_test` body (lines ~282–365) verbatim, with the named snapshot extension called out above the block. Task 5 Step 6's test body is a skeleton pointing at the sibling test (line ~1238) to copy state setup from — acceptable because the exact `Cx`/`TestGame` construction must match an existing in-module test rather than be invented. All other steps carry complete code.

**3. Type consistency:** `SkillTestStep` (not `FinishContinuation`) used in Tasks 4–5. `advance` (not `drive_skill_test`) used in Tasks 3–5. `commit_indices` (returns `Done`) vs the `#[cfg(test)]` `finish_skill_test` wrapper (calls `advance`) are distinguished in Task 5 Step 5. `Resolving` carries no payload (consistent across Task 4 def and Task 5 use). ✓
