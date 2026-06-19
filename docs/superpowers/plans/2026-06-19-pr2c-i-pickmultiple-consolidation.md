# PR 2c-i (#348, part 3a) — `PickMultiple` consolidation + delete vestigial `PickIndex` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce the umbrella's `InputResponse::PickMultiple { selected: Vec<OptionId> }` and fold the legacy `CommitCards { indices }` / `DiscardCards { indices }` into it, and delete the vestigial `PickIndex` — moving `InputResponse` toward its standard `PickSingle · PickMultiple · Skip · Confirm` shape.

**Architecture:** A wire-format change scoped to the *variant taxonomy*. `OptionId` wraps the hand index for these windows (so `selected: Vec<OptionId>` is the former `indices: Vec<u32>`); the commit/discard resume handlers read the ids back as indices; min/exact-count constraints stay where they already are (computed in the resume handlers / carried on the frame), **not** on the variant. Populating the *offered options* on the request (hand-as-`ChoiceOption`s, for client rendering) is **out of scope — deferred to #205**.

**Tech Stack:** Rust — `game-core` (engine + `test_support`), `web` (client builds `CommitCards`), scenario/integration tests.

This is **PR 2c-i**, the first of the reshaped-2c normalization (2c-i `PickMultiple`; 2c-ii `PickSingle` consolidation; 2c-iii action fold). Umbrella §3 (the standard `InputResponse` shape); spec: `docs/superpowers/specs/2026-06-19-continuation-stack-cleanup-design.md`. Series: #345 ✅ → #348 (2a ✅ · 2b ✅ · **2c-i/ii/iii**) → #347 → #380.

## Global Constraints

- **CI gauntlet, warnings-as-errors** (all before pushing): `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **No behavior change.** `OptionId(i)` for a commit/discard window means hand index `i`, exactly the old `indices` semantics; validation logic (count, duplicates, bounds, icon-sum) is unchanged — only the *carrier* changes.
- **Wire/replay-contract change.** `CommitCards`/`DiscardCards`/`PickIndex` leave the serialized `InputResponse`; old action logs with them won't replay. Acceptable pre-1.0 (already flagged spec-wide).
- **Branch:** `engine/input-pickmultiple` off fresh `main`. Commit per task; push only when the full gauntlet is green.

## Surface (surveyed)

- `CommitCards` — 37 sites: produced at `web/src/input.rs:70` + many tests; consumed in `resume_skill_test_commit` (`skill_test.rs`).
- `DiscardCards` — 15 sites: consumed in `resume_hand_size_discard` (`phases.rs:897`) + tests; the prompt string at `phases.rs:860` names it.
- `PickIndex` — 3 sites, **all test infra** (`outcome.rs:140` test prompt, `test_support/resolver.rs:116/118/437`). No production consumer → delete.

---

### Task 1: Add `PickMultiple`; delete `PickIndex`

**Files:**
- Modify: `crates/game-core/src/action.rs` (`InputResponse`: add `PickMultiple`, remove `PickIndex`; serde round-trip test)
- Modify: `crates/game-core/src/test_support/resolver.rs` (drop the `PickIndex` helper + its test); `crates/game-core/src/engine/outcome.rs:140` (test prompt string)

**Interfaces:**
- Produces: `InputResponse::PickMultiple { selected: Vec<crate::engine::OptionId> }`. Removes `InputResponse::PickIndex(u32)`.

- [ ] **Step 1: Add the variant + remove `PickIndex`**

In `crates/game-core/src/action.rs` `enum InputResponse`, add:

```rust
    /// Select a subset of the offered options, echoing back their
    /// [`OptionId`](crate::engine::OptionId)s (umbrella §3). The multi-selection
    /// family — commit windows and the upkeep hand-size discard fold into this;
    /// min/exact-count constraints live on the request/frame, not here. For
    /// those windows an `OptionId(i)` denotes hand index `i`.
    PickMultiple {
        /// The chosen option ids (hand indices, for commit/discard windows).
        selected: Vec<crate::engine::OptionId>,
    },
```

Delete the `PickIndex(u32)` variant and its doc-comment.

- [ ] **Step 2: Fix the `PickIndex` test-infra references**

- `crates/game-core/src/test_support/resolver.rs`: delete the `pick_index` helper (around `:116-118`) and the `PickIndex(7)` assertion test (around `:437`).
- `crates/game-core/src/engine/outcome.rs:140`: the test prompt string `"Submit PickIndex"` → `"Submit PickSingle"` (or delete if the test is `PickIndex`-specific — read it; if it only exercises `InputRequest::prompt`, just reword).

- [ ] **Step 3: serde round-trip test for `PickMultiple`**

Add to the `input_response_tests` mod in `action.rs` (mirror the existing `discard_cards_input_serde_roundtrip`):

```rust
#[test]
fn pick_multiple_input_serde_roundtrip() {
    use crate::engine::OptionId;
    let original = InputResponse::PickMultiple {
        selected: vec![OptionId(0), OptionId(3)],
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: InputResponse = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}
```

- [ ] **Step 4: Build + test + lint**

Run: `cargo build -p game-core --all-targets` (expect errors only at the now-removed `PickIndex` sites if any were missed); then `RUSTFLAGS="-D warnings" cargo test -p game-core`; `cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "engine: add InputResponse::PickMultiple; delete vestigial PickIndex (#348)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Fold `CommitCards` into `PickMultiple` (skill-test commit window)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`resume_skill_test_commit` reads `PickMultiple` instead of `CommitCards`; the commit-window prompt string)
- Modify: `crates/web/src/input.rs:70` (client builds `PickMultiple` instead of `CommitCards`)
- Modify: all test sites constructing `InputResponse::CommitCards { indices }`

**Interfaces:**
- Consumes: `InputResponse::PickMultiple` (Task 1).

- [ ] **Step 1: Migrate the resume handler**

In `resume_skill_test_commit` (`skill_test.rs`), replace the `let InputResponse::CommitCards { indices } = response else { … }` destructure with:

```rust
    let InputResponse::PickMultiple { selected } = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: skill-test commit expects InputResponse::PickMultiple, got {response:?}",
            )
            .into(),
        };
    };
    // For the commit window, each OptionId is a hand index.
    let indices: Vec<u32> = selected.iter().map(|o| o.0).collect();
```

Leave the downstream index validation (icon-sum, bounds) untouched — it consumes `indices` exactly as before.

- [ ] **Step 2: Migrate the client**

In `crates/web/src/input.rs:70`, build `InputResponse::PickMultiple { selected: indices.into_iter().map(OptionId).collect() }` instead of `CommitCards { indices }` (import `OptionId`). Read the surrounding code first — adapt the variable that held `indices` (it's `Vec<u32>` from the UI; wrap each in `OptionId`).

- [ ] **Step 3: Migrate the test sites**

Mechanical transform at every `InputResponse::CommitCards { indices: vec![a, b, …] }` (game-core unit tests, `test_support`, scenario/integration tests):

```rust
// before
InputResponse::CommitCards { indices: vec![0, 2] }
// after
InputResponse::PickMultiple { selected: vec![OptionId(0), OptionId(2)] }
```

Enumerate with `grep -rn "InputResponse::CommitCards" crates --include=*.rs`. Where a `test_support` resolver helper wraps commit (e.g. `commit_cards(indices)`), update the helper body once and callers stay unchanged. Use the right path for `OptionId` per crate (`crate::engine::OptionId` internally, `game_core::engine::OptionId` in integration tests — match each file's existing imports).

- [ ] **Step 4: Update the commit-window prompt string**

In `open_commit_window` (`skill_test.rs`), the prompt mentions `CommitCards`; reword to `PickMultiple` (hand indices as option ids).

- [ ] **Step 5: Gauntlet**

`RUSTFLAGS="-D warnings" cargo test --all --all-features`; clippy; fmt; `cargo build -p web --target wasm32-unknown-unknown`; `cargo clippy -p web …`. Expected: PASS — every skill-test / commit test green.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "engine: fold CommitCards into PickMultiple (#348)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Fold `DiscardCards` into `PickMultiple` (hand-size discard)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (`resume_hand_size_discard` reads `PickMultiple`; the discard prompt at `:860`)
- Modify: all test sites constructing `InputResponse::DiscardCards { indices }` (game-core `phases.rs` tests; `crates/scenarios/tests/upkeep_hand_size.rs`)
- Modify: `crates/web/src/*` only if it builds `DiscardCards` (grep to confirm; the survey showed the client builds `CommitCards` but check for `DiscardCards`)

**Interfaces:**
- Consumes: `InputResponse::PickMultiple` (Task 1).

- [ ] **Step 1: Migrate the resume handler**

In `resume_hand_size_discard` (`phases.rs:897`), replace the `InputResponse::DiscardCards { indices }` destructure with the `PickMultiple { selected }` form + `let indices: Vec<u32> = selected.iter().map(|o| o.0).collect();` (mirror Task 2 Step 1; message names hand-size discard). Downstream count/duplicate/bounds validation is unchanged.

- [ ] **Step 2: Update the discard prompt string** (`phases.rs:860`) — `DiscardCards` → `PickMultiple`.

- [ ] **Step 3: Migrate the test sites** — same mechanical transform as Task 2 Step 3, for `InputResponse::DiscardCards { indices: … }` → `PickMultiple { selected: … }`. Enumerate with `grep -rn "InputResponse::DiscardCards" crates --include=*.rs`. Includes `crates/scenarios/tests/upkeep_hand_size.rs` (game_core:: path).

- [ ] **Step 4: Gauntlet** — full six commands. Expected: PASS (the #111 hand-size suite green).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "engine: fold DiscardCards into PickMultiple (#348)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Remove the `CommitCards` / `DiscardCards` variants

**Files:** `crates/game-core/src/action.rs` (remove both variants + their doc-comments + the `discard_cards_input_serde_roundtrip` test, or repoint it to `PickMultiple`).

- [ ] **Step 1: Remove the variants**

Delete `CommitCards { indices }` and `DiscardCards { indices }` from `InputResponse`. The existing `discard_cards_input_serde_roundtrip` test (`action.rs:433`) now references a removed variant — delete it (Task 1's `pick_multiple_input_serde_roundtrip` covers the multi-select serde).

- [ ] **Step 2: Compile — confirm nothing still constructs them**

`cargo build -p game-core --all-targets` then `cargo build -p web --target wasm32-unknown-unknown`. Any error is a missed Task-2/3 site; fix it (→ `PickMultiple`).

- [ ] **Step 3: Full gauntlet** — all six. Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "engine: remove CommitCards/DiscardCards (folded into PickMultiple) (#348)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec/umbrella coverage:** `PickMultiple { Vec<OptionId> }` added (Task 1) and the two legacy index-variants folded (Tasks 2–4); `PickIndex` deleted (Task 1); constraints stay on the frame/handler (no variant change). Offered-options population explicitly deferred to #205 (stated in Architecture). ✓

**Placeholder scan:** the client step (Task 2 Step 2) says "read the surrounding code first" because the exact UI variable wiring isn't surveyed line-by-line — the transform (wrap `Vec<u32>` indices in `OptionId`) is concrete. Mechanical test migrations give one worked example + the enumerating grep (DRY, as in 2a/2b).

**Type consistency:** `PickMultiple { selected: Vec<OptionId> }` field name `selected` is used identically in the resume handlers (Tasks 2–3), the client (Task 2), and tests (Tasks 2–3). `OptionId(i).0` (the `u32`) is the hand index throughout.

**Out of scope (later sub-PRs):** `PickSingle` consolidation of `PickLocation`/`PickInvestigator` (2c-ii); `Mulligan`/`DrawEncounterCard` action fold + mulligan/mythos cursors (2c-iii); offered-options population (#205); tokens (#347); revelation disposal (#380).
