# PR 2b (#348, part 2) — Migrate the Tier-A suspension modes onto Continuation frames — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task (fresh subagent per task + review between tasks). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the five cleanly-`ResolveInput`-resumed suspension modes off their `Option<…>` fields on `GameState` and onto typed `Continuation` frames, collapsing their `resolve_input` cascade arms and `apply_player_action` guard-ladder entries into uniform top-frame dispatch.

**Architecture:** Task 1 introduces a frame-aware dispatch skeleton (`resolve_input` dispatches on `continuations.last()`) and a unified guard, both covering the *existing* frame variants (`Resolution`/`Choice`/`SkillTest`) with no behavior change. Each of Tasks 2–6 then migrates one mode end-to-end (new variant, suspend→push, resume→pop-from-frame, dispatch arm, and removal of the field + old cascade arm + old guard entry), leaving the build green. Task 7 removes the emptied cascade scaffolding.

**Tech Stack:** Rust, the `game-core` crate. No `cards`/`scenarios`/wire changes (these five modes are engine-internal; their `InputResponse` variants already exist).

This is **PR 2b of the #348 split.** Scope decision (recorded): **Tier A only** — `hunter_move_pending`, `spawn_engage_pending`, `hand_size_discard_pending`, `act_round_end_pending`, `pending_substitution_prompt`. **Tier B** (`pending_enemy_attack`, `pending_end_turn` — framework-resumed via continuation-close paths) is deferred to the **keystone** (Phase-7 step 3), which rebuilds that machinery. **Tier C** (`mulligan_pending`, `mythos_draw_pending`) is **2c** (the `Mulligan`/`DrawEncounterCard`→`InputResponse` fold); `enemy_attack_pending` stays a cursor per the spec. Spec §A: `docs/superpowers/specs/2026-06-19-continuation-stack-cleanup-design.md`. Series order: #345 ✅ → #348 (2a ✅ · **2b** · 2c) → #347 → #380.

## Global Constraints

- **CI gauntlet, warnings-as-errors** (all before pushing):
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **No behavior change.** Pure suspension-storage refactor. Every Hunter-movement, spawn-engagement, hand-size-discard, act-round-end, and Mind-over-Matter test must pass unchanged.
- **Frames push ABOVE what they suspend within.** Each mode is pushed when its suspension begins and is the top frame while active, so `continuations.last()`-based dispatch routes it correctly. `pending_substitution_prompt` in particular pushes **above** the `SkillTest` frame (which 2a guarantees exists from test start) — top-frame dispatch then routes the substitution prompt before the commit window, replacing the old "route substitution first" special-case.
- **Tier B / Tier C untouched.** `pending_enemy_attack`, `pending_end_turn`, `mulligan_pending`, `mythos_draw_pending`, `enemy_attack_pending` stay as `Option` fields with their existing guards/handling. The unified guard (Task 1) covers only frame-based input-awaiting suspensions; the mulligan/mythos cursor guards remain separate.
- **Branch:** `engine/migrate-tier-a-suspensions` off fresh `main`. Commit per task; push only when the full gauntlet is green.

## Per-mode reference table

| Mode (field) | Payload type | New variant | Suspend site | Resume fn | Resume `InputResponse` |
|---|---|---|---|---|---|
| `hunter_move_pending` | `HunterChoice` | `HunterMove(HunterChoice)` | `hunters.rs:397` | `hunters::resume_hunter_choice` | `PickLocation` / `PickInvestigator` |
| `spawn_engage_pending` | `SpawnEngagePending` | `SpawnEngage(SpawnEngagePending)` | `encounter.rs:463` | `hunters::resume_spawn_engage` | `PickInvestigator` |
| `hand_size_discard_pending` | `HandSizeDiscard` | `HandSizeDiscard(HandSizeDiscard)` | `phases.rs:850` (+ builder staging `builder.rs:222`) | `phases::resume_hand_size_discard` | `DiscardCards` |
| `act_round_end_pending` | `ActRoundEndPending` | `ActRoundEnd(ActRoundEndPending)` | `phases.rs:706` | `phases::resume_act_round_end_advance` | `Confirm` / `Skip` |
| `pending_substitution_prompt` | `InvestigatorId` | `SubstitutionPrompt { investigator: InvestigatorId }` | `skill_test.rs` (the `pending_substitution_prompt = Some(...)` line) | `skill_test::resume_substitution_choice` | `PickSingle` |

---

### Task 1: Frame-aware dispatch skeleton + unified guard (no behavior change)

Make `resolve_input` dispatch on `continuations.last()` and add a unified "input-awaiting frame" guard, both covering only the existing variants (`Resolution`/`Choice`/`SkillTest`) for now. This is the seam Tasks 2–6 extend.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add `Continuation::awaits_resolve_input`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` head; `apply_player_action` guard)

**Interfaces:**
- Produces: `Continuation::awaits_resolve_input(&self) -> bool` — true for frames whose resume is a player `ResolveInput`. (Window frames return true; they additionally admit Fast plays handled by the existing window guard.)

- [ ] **Step 1: Add the classifier**

In `game_state.rs`, on `impl Continuation`:

```rust
/// Whether this frame is resumed by a player [`ResolveInput`](crate::action::PlayerAction::ResolveInput).
/// Every current variant is — but Tier-B framework-resumed suspensions (added
/// with the keystone) will return `false`.
#[must_use]
pub fn awaits_resolve_input(&self) -> bool {
    match self {
        Continuation::Resolution(_)
        | Continuation::Choice(_)
        | Continuation::SkillTest(_) => true,
    }
}
```

(The `match` is exhaustive so each Task 2–6 variant addition forces a compile error here until classified — a deliberate tripwire.)

- [ ] **Step 2: Verify**

Run: `cargo build -p game-core` → PASS.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "engine: add Continuation::awaits_resolve_input classifier (#348)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Tasks 2–6: Migrate each mode (one task per mode)

**These five tasks are structurally identical.** The recipe below is the exemplar for **Task 2 (`HunterMove`)**; Tasks 3–6 apply the same recipe with the row's values from the per-mode table. Each task is its own subagent dispatch + review, and each must leave the full gauntlet green.

#### Recipe (worked for `hunter_move_pending` → `HunterMove(HunterChoice)`)

**Files:** `game_state.rs` (variant + classifier), the mode's suspend-site file, its resume-handler file, `dispatch/mod.rs` (dispatch arm + guard/cascade removal).

- [ ] **Step 1: Add the variant**

In `game_state.rs` `enum Continuation`, add:

```rust
    /// A suspended Hunter-movement choice (#128), migrated off the former
    /// `GameState::hunter_move_pending` field (#348). Resumed by
    /// [`resume_hunter_choice`](crate::engine) via `ResolveInput`.
    HunterMove(HunterChoice),
```

- [ ] **Step 2: Classify it**

Add `| Continuation::HunterMove(_)` to the `true` arm of `awaits_resolve_input` (Task 1). (Compile error here until done — the tripwire.)

- [ ] **Step 3: Migrate the suspend site** (`hunters.rs:397`)

```rust
// before
cx.state.hunter_move_pending = Some(choice);
// after
cx.state
    .continuations
    .push(crate::state::Continuation::HunterMove(choice));
```

(Leave the `AwaitingInput { … }` return that follows unchanged.)

- [ ] **Step 4: Migrate the resume handler** (`hunters::resume_hunter_choice`)

Replace the `cx.state.hunter_move_pending.take()` (or `.as_ref()`) read at the top of the handler with a pop of the `HunterMove` frame:

```rust
let choice = match cx.state.continuations.last() {
    Some(crate::state::Continuation::HunterMove(_)) => {
        match cx.state.continuations.pop() {
            Some(crate::state::Continuation::HunterMove(c)) => c,
            _ => unreachable!("checked HunterMove on top"),
        }
    }
    _ => {
        return EngineOutcome::Rejected {
            reason: "resume_hunter_choice: no HunterMove frame on top of the stack".into(),
        }
    }
};
```

(Match the handler's existing validation/early-return style; if it pops late after validating the response, keep that ordering — pop only once the response is accepted, to preserve validate-first. Read the existing handler body and adapt: the rule is *the payload now comes from the frame, and the frame is removed exactly where the field was previously cleared*.)

- [ ] **Step 5: Add the dispatch arm; remove the old cascade arm**

In `resolve_input` (`dispatch/mod.rs`), the routing is now top-frame. Add to the dispatch:

```rust
    if let Some(crate::state::Continuation::HunterMove(_)) = cx.state.continuations.last() {
        return hunters::resume_hunter_choice(cx, response);
    }
```

and **delete** the old `if cx.state.hunter_move_pending.is_some() { return hunters::resume_hunter_choice(cx, response); }` cascade arm.

- [ ] **Step 6: Remove the guard-ladder entry**

Delete the `if cx.state.hunter_move_pending.is_some() && !matches!(action, PlayerAction::ResolveInput { .. }) { return Rejected … }` block in `apply_player_action`. It is now covered by the unified frame guard — confirm that guard exists (add it in Task 2 if not yet present):

```rust
    // Unified: any frame awaiting a ResolveInput blocks every other action
    // (Fast plays during reaction windows are admitted by the window guard above).
    if cx
        .state
        .continuations
        .last()
        .is_some_and(crate::state::Continuation::awaits_resolve_input)
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "an input-awaiting suspension is on the stack; submit a \
                     PlayerAction::ResolveInput before any other action"
                .into(),
        };
    }
```

Place this guard **after** the existing mulligan and reaction-window guards (those stay). The first migrated mode (Task 2) introduces it; later tasks just delete their per-field guard entry.

- [ ] **Step 7: Remove the field**

Delete `pub hunter_move_pending: Option<HunterChoice>,` from `GameState` and its `builder.rs` initializer. Fix the mutual-exclusion `debug_assert!` in `resolve_input` (`dispatch/mod.rs:451`) by dropping the `hunter_move_pending` term (Task 6 removes the assert entirely once all four of its terms are gone).

- [ ] **Step 8: Compile, test, lint**

```bash
cargo build -p game-core --all-targets   # fix any remaining unit-field references
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check
```
Expected: PASS — the Hunter-movement tests (`hunters.rs` `#[cfg(test)]`, the #128 suite) unchanged.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "engine: migrate hunter_move_pending onto a HunterMove frame (#348)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

#### Task 3 — `spawn_engage_pending` → `SpawnEngage(SpawnEngagePending)`
Apply the recipe with the table row. Suspend site `encounter.rs:463`; resume `hunters::resume_spawn_engage`; tests: the #128 spawn-engage suite (`encounter.rs:1649` `resume_spawn_engage_rejects_bad_pick_and_preserves_pending` — update its `spawn_engage_pending` assertions to inspect the `SpawnEngage` frame instead). Commit subject: `engine: migrate spawn_engage_pending onto a SpawnEngage frame (#348)`.

#### Task 4 — `hand_size_discard_pending` → `HandSizeDiscard(HandSizeDiscard)`
Apply the recipe. **Two** suspend sites: the production one (`phases.rs:850`) **and** a builder staging helper (`builder.rs:222`, used to stage a paused upkeep state — convert it to push the frame onto the builder's `continuations`, or read how the builder stages other frames and mirror that). Resume `phases::resume_hand_size_discard`; tests: the #111 hand-size suite (`phases.rs` `resume_hand_size_discard_*`). Commit: `engine: migrate hand_size_discard_pending onto a HandSizeDiscard frame (#348)`.

#### Task 5 — `act_round_end_pending` → `ActRoundEnd(ActRoundEndPending)`
Apply the recipe. Suspend `phases.rs:706`; resume `phases::resume_act_round_end_advance`; tests: the #275 act-round-end suite (`phases.rs` `resume_confirm_*` / `resume_skip_*`). Commit: `engine: migrate act_round_end_pending onto an ActRoundEnd frame (#348)`.

#### Task 6 — `pending_substitution_prompt` → `SubstitutionPrompt { investigator }`
Apply the recipe, with two specifics:
- **Push above the `SkillTest` frame.** At the suspend site in `skill_test.rs` (the `pending_substitution_prompt = Some(investigator)` line), push `Continuation::SubstitutionPrompt { investigator }` — it lands on top of the already-present `SkillTest` frame (2a). `resume_substitution_choice` pops it, then calls `open_commit_window` as today.
- **Delete the substitution-first special-case.** Remove the `if cx.state.pending_substitution_prompt.is_some() { return resume_substitution_choice(…) }` block at the **head** of `resolve_input` — top-frame dispatch now handles ordering (the `SubstitutionPrompt` frame is on top, so it routes first by construction). Add the normal dispatch arm instead.
- Tests: the Mind-over-Matter suite (`skill_test.rs` `combat_test_with_substitution_*`, `substitution_choice_no_*`, and the 2a `substitution_prompt_keeps_the_test_on_its_frame`) — update any `pending_substitution_prompt` assertions to inspect the `SubstitutionPrompt` frame.
Commit: `engine: migrate pending_substitution_prompt onto a SubstitutionPrompt frame (#348)`.

---

### Task 7: Remove the emptied cascade scaffolding

After Tasks 2–6, the old per-field cascade arms and guard entries are gone. Clean up the remnants.

**Files:** `crates/game-core/src/engine/dispatch/mod.rs`

- [ ] **Step 1: Remove the mutual-exclusion `debug_assert!`**

The `debug_assert!` that asserted hunter/spawn/hand-size/act-round are mutually exclusive (`dispatch/mod.rs:451`) referenced fields that no longer exist. Delete it — top-frame dispatch makes the ordering structural; mutual exclusion is no longer a routing concern.

- [ ] **Step 2: Tidy `resolve_input`**

Confirm `resolve_input` is now a flat sequence of `if let Some(Continuation::X(_)) = last() { return resume_x(…) }` arms (one per frame variant) followed by the terminal `Rejected`. Collapse to a single `match cx.state.continuations.last() { … }` if it reads more cleanly. Confirm the head no longer has the substitution special-case.

- [ ] **Step 3: Full gauntlet**

Run all six commands from Global Constraints. Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "engine: collapse resolve_input cascade to top-frame dispatch (#348)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage (spec §A — migrate `pending_*` suspension modes; collapse the cascade + guard ladder):**
- 5 Tier-A modes → frames → Tasks 2–6. ✓
- Cascade arms + guard entries collapsed to top-frame dispatch → Tasks 1, 2 (guard), 7. ✓
- Substitution-first special-case retired via top-frame ordering → Task 6. ✓
- Tier B deferred to keystone, Tier C to 2c, `enemy_attack_pending` stays → recorded in scope. ✓

**Placeholder scan:** the recipe's Step 4 deliberately says "read the existing handler body and adapt" for the pop placement (validate-first ordering varies per handler — the subagent must preserve each handler's existing reject-before-mutate shape rather than blindly pop first). Every other step is concrete. The five modes share one recipe (DRY) + a per-mode parameter table; this mirrors the accepted approach in the 2a plan.

**Type consistency:** variant names (`HunterMove`/`SpawnEngage`/`HandSizeDiscard`/`ActRoundEnd`/`SubstitutionPrompt`) match the table, the `awaits_resolve_input` arms, and the dispatch arms. Payload types match the `pub struct` declarations surveyed (`HunterChoice`, `SpawnEngagePending`, `HandSizeDiscard`, `ActRoundEndPending`, `InvestigatorId`).

**Subagent-driven note:** Tasks are ordered so each leaves a green build. Task 1 is the seam; Tasks 2–6 are independent migrations (any order, but the guard is introduced in the first one run); Task 7 is terminal cleanup. Give each implementer subagent explicit git guardrails: stay on `engine/migrate-tier-a-suspensions`, never switch branches, never touch `main`; verify branch + `git log` after each task.
