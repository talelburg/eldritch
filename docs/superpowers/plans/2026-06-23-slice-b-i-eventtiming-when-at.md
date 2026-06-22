# Slice B-i — `EventTiming::{When, At, After}` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename `EventTiming::Before → When` and add a dormant `At` variant, so the
card DSL carries the RR `when → at → after` timing axis as a first-class enum — the
enabler for the Slice-B coordinators. Behaviour-preserving.

**Architecture:** Pure enum-variant rename (compiler-enforced across the workspace) plus
one new dormant variant. No ability is tagged `At` yet — the round-end doom re-tag and the
coordinator that scans by bucket land in Slice B-iii. The forced scanner's hardcoded
`timing == After` filter is left untouched (correct while `At`/`When` forced abilities don't
exist); its bucket-parameterization defers to B-iii where a caller actually varies the
bucket.

**Tech Stack:** Rust workspace (`card-dsl`, `game-core`, `cards`, `scenarios`). Serde
derive on the enum (wire shape pinned by a round-trip test).

**Parent spec:** [`2026-06-23-emitevent-frame-slice-b-coordinators-design.md`](../specs/2026-06-23-emitevent-frame-slice-b-coordinators-design.md)
§"Why the DSL rework" + §"Sub-slicing → B-i". Issue: [#434](https://github.com/talelburg/eldritch/issues/434).

## Global Constraints

- **Behaviour-preserving.** No game-outcome or event-log change in this sub-slice. The full
  suite stays green; the only intended diffs are the rename and the dormant variant.
- **CI gauntlet before push** (warnings-as-errors), all from repo root:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Never hand-edit `crates/cards/src/generated/`** (none touched here).
- **Variant order `When, At, After`** in the enum (matches RR ordering; serde keys on the
  name, not position, so this is cosmetic but load-bearing for readers).
- Branch: `engine/slice-b-coordinators` (current); commits accumulate for the B-i PR.

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/card-dsl/src/dsl.rs` | the `EventTiming` enum + builders + unit tests | rename variant, add `At`, update docs + serde test |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | reaction-window candidate matching (`trigger_matches`) + tests | rename arm, add `At` arm, update doc + 4 test sites |
| `crates/game-core/src/engine/dispatch/forced_triggers.rs` | forced-trigger scan (`push_matching`) | rename one comment reference (filter unchanged) |
| `crates/cards/src/impls/dodge.rs` | Dodge 01023 ability | rename 2 construction sites |
| `crates/cards/src/impls/cover_up.rs` | Cover Up 01007 ability | rename 2 construction sites |
| `crates/scenarios/src/test_fixtures/synth_cards.rs` | synthetic test cards | rename 2 construction sites |

---

### Task 1: Rename `EventTiming::Before → When` workspace-wide

A variant rename does not compile until **every** site is updated, so this task edits all
sites and the green checkpoint is a clean workspace build. After this task the enum has
exactly two variants (`When`, `After`) — `At` is added in Task 2.

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (enum def + 3 doc refs + 2 test sites)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`trigger_matches` arm + doc + 4 tests)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (1 comment)
- Modify: `crates/cards/src/impls/dodge.rs` (2 sites)
- Modify: `crates/cards/src/impls/cover_up.rs` (2 sites)
- Modify: `crates/scenarios/src/test_fixtures/synth_cards.rs` (2 sites)

**Interfaces:**
- Produces: `card_dsl::dsl::EventTiming::When` (replaces `::Before`), same
  `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]`. Serde wire
  string changes `"Before" → "When"` (no persisted corpus depends on it — generated cards
  carry no `EventTiming::Before`).

- [ ] **Step 1: Rename the enum variant + update its doc.** In `crates/card-dsl/src/dsl.rs`,
  replace the `EventTiming` doc block + `Before` variant:

```rust
/// When an [`Trigger::OnEvent`] ability fires relative to the triggering
/// event finalizing — the RR "when → at → after" timing axis (the order
/// simultaneous abilities sharing a triggering condition resolve in).
///
/// - [`When`](Self::When) — the "Forced — when … would …" interrupt timing
///   that lets an effect interpose on an in-progress event (Dodge 01023's
///   cancel, Cover Up 01007's replacement).
/// - [`After`](Self::After) — most reaction cards ("After you defeat an
///   enemy …").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventTiming {
    /// Resolves as the triggering event would finalize (interrupt /
    /// replacement timing — "when … would …").
    When,
    /// Resolves after the triggering event has finalized.
    After,
}
```

- [ ] **Step 2: Update the two DSL doc references to `Before`.** In the same file, line ~323
  (the `WouldDiscoverClues` pattern doc): `(paired with [`EventTiming::Before`])` →
  `(paired with [`EventTiming::When`])`. There are no other prose `Before` references to the
  variant outside the test (Step 4).

- [ ] **Step 3: Rename the `trigger_matches` arm + its doc.** In
  `crates/game-core/src/engine/dispatch/reaction_windows.rs`, the doc comment line ~325
  `/// `EventTiming::Before` doesn't fire on these windows yet …` →
  `/// `EventTiming::When` interrupt timing fires only on the Before-windows below …`, and the
  match arm (~337):

```rust
    match timing {
        EventTiming::When => {
            return matches!(
                (kind, pattern),
                (
                    WindowKind::BeforeEnemyAttack { .. },
                    EventPattern::EnemyAttacks
                ) | (
                    WindowKind::BeforeDiscoverClues { .. },
                    EventPattern::WouldDiscoverClues
                )
            );
        }
        EventTiming::After => {}
    }
```

- [ ] **Step 4: Rename the remaining construction/test sites.** Replace `EventTiming::Before`
  with `EventTiming::When` at each:
  - `crates/cards/src/impls/dodge.rs:33` and `:50`
  - `crates/cards/src/impls/cover_up.rs:41` and `:138`
  - `crates/scenarios/src/test_fixtures/synth_cards.rs:331` and `:414` (the latter is
    `game_core::dsl::EventTiming::Before` → `::When`)
  - `crates/game-core/src/engine/dispatch/reaction_windows.rs:1664`, `:1674`, `:1694`, `:1762`
  - `crates/card-dsl/src/dsl.rs:1986` — also rename the local binding `before_controller` →
    `when_controller` (and the `assert_ne!(after_controller, before_controller)` ~line 1994).
  - `crates/card-dsl/src/dsl.rs:2005` — the serde-test loop array `[EventTiming::After,
    EventTiming::Before]` → `[EventTiming::When, EventTiming::After]`, and its doc comment
    (~line 1999) `Both [`EventTiming`] variants (`After` and `Before`)` → `Both [`EventTiming`]
    variants (`When` and `After`)`.

- [ ] **Step 5: Update the forced-scanner comment.** In
  `crates/game-core/src/engine/dispatch/forced_triggers.rs` (~line 376), replace the comment
  above the `if *timing == EventTiming::After` filter:

```rust
            // Only `After` timing is handled in this slice; no in-scope Forced
            // card uses `When` ("when X would Y") timing, and `At`-timed forced
            // abilities don't exist until Slice B-iii routes them through the
            // EmitEvent coordinator. Revisit the filter there.
```

  (The `if *timing == EventTiming::After && want(pattern)` line itself is unchanged.)

- [ ] **Step 6: Build the workspace to confirm the rename is complete.**

Run: `cargo build --all --all-features`
Expected: clean build. A `no variant named `Before`` error means a site was missed — grep
`rg "EventTiming::Before" crates` should return nothing.

- [ ] **Step 7: Run the full test suite (behaviour-preserving check).**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS — same counts as before the rename (the serde round-trip test now exercises
`When` instead of `Before`).

- [ ] **Step 8: Commit.**

```bash
git add -A
git commit -m "$(cat <<'EOF'
engine: rename EventTiming::Before → When (Slice B-i task 1)

The "Forced — when … would …" interrupt timing is the RR `when` bucket;
rename it so the DSL names the when/at/after axis directly. Pure rename,
compiler-enforced across the workspace, behaviour-preserving.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 2: Add the dormant `At` variant

`At` is the RR "at the …" bucket, ordered between `When` and `After`. No ability is tagged
`At` in this sub-slice — the round-end doom re-tag is B-iii. This task adds the variant, its
exhaustive-match handling, and serde coverage, proving it round-trips and is unused.

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (enum + serde test)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`trigger_matches` arm)

**Interfaces:**
- Consumes: `EventTiming::{When, After}` from Task 1.
- Produces: `EventTiming::At` — constructible, serde-round-trippable, ordered second. No
  forced/reaction scan matches it yet (`trigger_matches` treats it like `After`; the forced
  filter's `== After` excludes it).

- [ ] **Step 1: Extend the serde round-trip test to cover `At` (write the failing test
  first).** In `crates/card-dsl/src/dsl.rs`, change the serde-test loop array to include `At`:

```rust
        for timing in [EventTiming::When, EventTiming::At, EventTiming::After] {
```

  and update its doc comment (~line 1999) `Both [`EventTiming`] variants (`When` and `After`)`
  → `All three [`EventTiming`] variants (`When`, `At`, `After`)`.

- [ ] **Step 2: Run the test to verify it fails.**

Run: `cargo test -p card-dsl on_event_ability_round_trips_through_serde_json`
Expected: FAIL — compile error `no variant named `At`` (the variant doesn't exist yet).

- [ ] **Step 3: Add the `At` variant to the enum.** In `crates/card-dsl/src/dsl.rs`, insert
  `At` between `When` and `After`, and extend the enum doc block to mention it:

```rust
    /// Resolves as the triggering event would finalize (interrupt /
    /// replacement timing — "when … would …").
    When,
    /// Resolves between `when` and `after` abilities with the same
    /// triggering condition ("at the …"). Dormant until Slice B-iii
    /// re-tags the round-end doom onto it.
    At,
    /// Resolves after the triggering event has finalized.
    After,
```

  And add to the doc block (after the `When` bullet, before `After`):

```rust
/// - [`At`](Self::At) — "at the …" timing, resolving between `when` and
///   `after` abilities sharing a triggering condition. Dormant: no ability
///   is tagged `At` until Slice B-iii.
```

- [ ] **Step 4: Make `trigger_matches` exhaustive over `At`.** In
  `crates/game-core/src/engine/dispatch/reaction_windows.rs`, combine `At` with the `After`
  arm (behaviour-preserving — no `At`-timed reaction exists, so it never reaches a window):

```rust
        EventTiming::When => {
            return matches!(
                (kind, pattern),
                (
                    WindowKind::BeforeEnemyAttack { .. },
                    EventPattern::EnemyAttacks
                ) | (
                    WindowKind::BeforeDiscoverClues { .. },
                    EventPattern::WouldDiscoverClues
                )
            );
        }
        // No `At`-timed reaction exists until Slice B-iii; treat it like
        // `After` (fall through to pattern matching). Dormant.
        EventTiming::At | EventTiming::After => {}
```

- [ ] **Step 5: Build to confirm exhaustiveness is satisfied everywhere.**

Run: `cargo build --all --all-features`
Expected: clean build. A `non-exhaustive patterns: `EventTiming::At` not covered` error names
any other `match timing` site that needs the same `At | After` treatment — apply it there.

- [ ] **Step 6: Run the serde test (now passing) + the full suite.**

Run: `cargo test -p card-dsl on_event_ability_round_trips_through_serde_json`
Expected: PASS (all three variants round-trip).
Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS — unchanged behaviour; `At` is exercised only by the serde test.

- [ ] **Step 7: Commit.**

```bash
git add -A
git commit -m "$(cat <<'EOF'
engine: add dormant EventTiming::At variant (Slice B-i task 2)

Adds the RR "at the …" bucket between When and After. No ability is tagged
At yet — the round-end doom re-tag and bucket-scanning coordinator land in
Slice B-iii. trigger_matches treats At like After (no At reaction exists);
the forced scanner's `== After` filter excludes it. Serde round-trip now
covers all three variants.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 3: CI gauntlet + PR

**Files:** none (verification + PR).

- [ ] **Step 1: Run the full CI gauntlet** (all six jobs from Global Constraints). Expected:
  all green. Fix any clippy/doc/fmt finding with a follow-up edit before proceeding.

- [ ] **Step 2: Push the branch.**

```bash
git push -u origin engine/slice-b-coordinators
```

- [ ] **Step 3: Open the PR** with `gh pr create` using the repo template. Title:
  `engine: EventTiming::{When, At, After} (Slice B-i)`. Body: one design-decisions paragraph
  — the rename names the RR `when` bucket; `At` is dormant (B-iii populates it); the forced
  scanner's `== After` filter is intentionally left for B-iii. Reference the parent spec and
  note "Part of #434 (Slice B-i)."

- [ ] **Step 4: Watch CI** via `gh pr checks <PR#> --watch` (background); fix failures with
  follow-up commits to the same branch.

- [ ] **Step 5: Phase-doc update is deferred** — B-i is one sub-slice of Slice B (#434); the
  `docs/phases/phase-7-the-gathering.md` Ordering step 6 + Slice-A note get updated when the
  whole of Slice B lands, not per sub-slice. (Do **not** edit the phase doc here.)

## Self-Review notes

- **Spec coverage:** B-i = "rename Before→When; add At (dormant); forced filter unchanged." ✅
  Tasks 1–2 cover the rename + dormant variant; the filter generalization is explicitly
  deferred to B-iii (documented in Task 1 Step 5's comment and the plan header).
- **No `At` consumer in B-i:** confirmed — `At` appears only in the enum, the serde test, and
  the dormant `trigger_matches` arm. The forced scanner cannot fire it (`== After`).
- **Exhaustiveness safety net:** `cargo build` (Task 2 Step 5) catches any `match timing` site
  beyond `trigger_matches`. The only known exhaustive match is `trigger_matches`; the forced
  scanner uses an `==` comparison, not a match.
