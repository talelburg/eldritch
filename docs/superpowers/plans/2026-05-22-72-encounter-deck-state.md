# #72 — encounter deck state — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the shared encounter deck + discard to `GameState`, primitive draw/shuffle/reshuffle helpers, and an explicit `EngineRecord::EncounterDeckShuffled` action so #126's on-draw resolution can draw from the deck.

**Architecture:** Mirror the existing `shuffle_player_deck` pattern in `crates/game-core/src/engine/dispatch.rs`. Encounter deck is `VecDeque<CardCode>` on `GameState`; discard is `Vec<CardCode>`. Additive sibling variants on `EngineRecord` and `Event` (no rename / no tagged refactor). Mid-handler reshuffles call helpers directly without pushing log entries — replay determinism comes from the seeded `RngState`.

**Tech Stack:** Rust 2021 edition, `serde`, `rand_chacha::ChaCha8Rng` (via `crate::rng::RngState`).

**Spec:** `docs/superpowers/specs/2026-05-22-72-encounter-deck-state-design.md` is the authoritative design — re-read it when starting.

**PR procedure:** CLAUDE.md's 8-step PR procedure applies. This plan covers steps 1 (local CI gauntlet), 2 (commits on a feature branch), 7 (phase-doc update as last commit), and the PR-open + review handoff. Steps 4–6 (CI watch + review-agent + addressing CI) and 8 (merge) are out of plan scope — they're driven by the human after the PR opens.

---

## File map

- **Create:** `crates/scenarios/tests/encounter_reveal.rs` — NOT in this plan; lands in #126.
- **Modify:**
  - `.gitignore` — add `docs/superpowers/` (one-time, first commit).
  - `crates/game-core/src/state/game_state.rs` — add `encounter_deck` + `encounter_discard` fields to `GameState`; add `#[cfg(test)]` tests for serde + state fixtures.
  - `crates/game-core/src/state/mod.rs` — add `CardCode` to the existing `card::` re-export if not already present (it is; verify).
  - `crates/game-core/src/event.rs` — add `Event::EncounterDeckShuffled` variant.
  - `crates/game-core/src/action.rs` — add `EngineRecord::EncounterDeckShuffled` variant.
  - `crates/game-core/src/engine/dispatch.rs` — add three `pub(super)` helpers (`shuffle_encounter_deck`, `reshuffle_encounter_discard`, `draw_encounter_top`) + the `encounter_deck_shuffled` dispatch handler + arm in `apply_engine_record`.
  - `crates/game-core/src/test_support/builder.rs` — extend `TestGame::build()` to populate the new fields with defaults (empty `VecDeque` / empty `Vec`).
  - Any other in-crate sites that construct `GameState` via struct literal — the compiler will flag them after the field add. Known: `crates/game-core/src/test_support/resolver.rs` (look for `empty_state` and friends).
  - `docs/phases/phase-4-scenario-plumbing.md` — LAST commit only; do not touch mid-PR.

Every commit must compile cleanly with the full CI gauntlet (`RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, `cargo build -p web --target wasm32-unknown-unknown`).

---

## Task 1: Set up the feature branch

**Files:** none modified.

- [ ] **Step 1: Create the feature branch from main**

```bash
git checkout main
git pull
git checkout -b engine/encounter-deck-state
```

- [ ] **Step 2: Verify `.gitignore` already excludes `docs/superpowers/`**

Run:
```bash
grep -n "docs/superpowers" .gitignore
```

Expected: a line near the bottom matching `docs/superpowers/`.

If missing (out-of-band edit lost), add it now:
```
# Brainstorming / planning scratch — local-only, not part of the repo
docs/superpowers/
```

No commit yet — `.gitignore` rides Task 2's commit.

---

## Task 2: Add encounter_deck + encounter_discard to GameState

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs`
- Modify: `crates/game-core/src/test_support/builder.rs`
- Modify: any other in-crate `GameState { ... }` struct-literal sites (compiler-flagged)
- Modify: `.gitignore` (the new entry rides this commit)

- [ ] **Step 1: Write the failing serde roundtrip test**

Append to the `#[cfg(test)] mod open_window_tests` block at the bottom of `crates/game-core/src/state/game_state.rs`, OR add a new sibling `mod encounter_deck_tests`. Prefer the latter for clarity:

```rust
#[cfg(test)]
mod encounter_deck_tests {
    use super::*;
    use crate::state::CardCode;
    use crate::test_support::TestGame;

    #[test]
    fn encounter_deck_and_discard_serde_roundtrip() {
        let mut state = TestGame::new().build();
        state.encounter_deck.push_back(CardCode("01001".into()));
        state.encounter_deck.push_back(CardCode("01002".into()));
        state.encounter_discard.push(CardCode("01099".into()));

        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.encounter_deck.len(), 2);
        assert_eq!(back.encounter_deck[0], CardCode("01001".into()));
        assert_eq!(back.encounter_deck[1], CardCode("01002".into()));
        assert_eq!(back.encounter_discard.len(), 1);
        assert_eq!(back.encounter_discard[0], CardCode("01099".into()));
    }

    #[test]
    fn fresh_state_has_empty_encounter_deck_and_discard() {
        let state = TestGame::new().build();
        assert!(state.encounter_deck.is_empty());
        assert!(state.encounter_discard.is_empty());
    }
}
```

- [ ] **Step 2: Run the test to verify compile failure**

Run:
```bash
cargo test -p game-core encounter_deck_tests 2>&1 | head -40
```

Expected: compile error — `no field 'encounter_deck' on type GameState`. Confirms the test is exercising the new surface.

- [ ] **Step 3: Add the fields to `GameState`**

In `crates/game-core/src/state/game_state.rs`, extend the imports at the top to include `std::collections::VecDeque` and `CardCode`:

```rust
use std::collections::{BTreeMap, VecDeque};
// ... existing imports ...
use super::{
    card::{CardCode, CardInstanceId},   // add CardCode
    // ... rest unchanged ...
};
```

Then add the two fields inside the `pub struct GameState { ... }` (after `scenario_id`, or wherever feels least disruptive — match neighbouring field doc-comment density):

```rust
    /// Shared encounter deck (top = front). Built at scenario setup
    /// from encounter-set codes; drawn from during Mythos. When the
    /// deck runs out, [`draw_encounter_top`](crate::engine::dispatch::draw_encounter_top)
    /// transparently reshuffles `encounter_discard` back in via the
    /// deterministic RNG path.
    ///
    /// Empty at the start of every scenario; populated by scenario
    /// setup (the first wiring lands in #126 alongside the synthetic
    /// fixture's encounter-set composition).
    pub encounter_deck: VecDeque<CardCode>,
    /// Encounter discard pile. Treacheries land here after Revelation
    /// resolves; defeated enemies (and other "discarded from play"
    /// encounter content) land here in later issues.
    ///
    /// Drained back into [`encounter_deck`](Self::encounter_deck) by
    /// [`reshuffle_encounter_discard`](crate::engine::dispatch::reshuffle_encounter_discard)
    /// when the deck runs empty.
    pub encounter_discard: Vec<CardCode>,
```

- [ ] **Step 4: Update `TestGame::build()` to populate empty defaults**

In `crates/game-core/src/test_support/builder.rs:250-269`, add the two fields to the `GameState { ... }` struct literal:

```rust
    pub fn build(self) -> GameState {
        GameState {
            // ... existing fields ...
            scenario_id: self.scenario_id,
            encounter_deck: std::collections::VecDeque::new(),
            encounter_discard: Vec::new(),
        }
    }
```

- [ ] **Step 5: Run `cargo check` to find any other struct-literal sites**

Run:
```bash
cargo check --all --all-features 2>&1 | grep -E "error|--> " | head -30
```

For each `error[E0063]: missing field` (or similar), open the flagged file and add the two fields to the `GameState { ... }` literal. Likely candidates per `grep -rn 'GameState {' crates/game-core/src/`:
- `crates/game-core/src/test_support/resolver.rs` — `empty_state()` and `state_with_in_flight_hand()`.

Repeat until `cargo check --all --all-features` exits 0.

- [ ] **Step 6: Run the test to verify it passes**

Run:
```bash
cargo test -p game-core encounter_deck_tests
```

Expected: 2 tests pass.

- [ ] **Step 7: Run the full CI-equivalent test suite for game-core**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
```

Expected: all tests pass with no warnings.

- [ ] **Step 8: Commit (bundles `.gitignore`)**

```bash
git add .gitignore crates/game-core/src/state/game_state.rs crates/game-core/src/test_support/builder.rs crates/game-core/src/test_support/resolver.rs
git commit -m "$(cat <<'EOF'
engine: add encounter deck + discard state fields

Adds GameState.encounter_deck (VecDeque<CardCode>) and
encounter_discard (Vec<CardCode>) — the shared piles Mythos will
draw from. Both empty at scenario setup; populated by scenario
modules in #126.

Also ignores docs/superpowers/ (local brainstorming/planning
scratch).

Refs #72.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

(Add any additional resolver/test-support files the compiler flagged in step 5 to the `git add` list.)

---

## Task 3: Add Event::EncounterDeckShuffled variant

**Files:**
- Modify: `crates/game-core/src/event.rs`

`Event` is `#[non_exhaustive]` so adding a variant is non-breaking for external matches, but in-crate exhaustive matches still need an arm. The engine module currently uses `match` on `Event` in a few places (e.g. assertion-helper macros in `mod.rs`); the compiler will flag any that need updating.

- [ ] **Step 1: Write the failing event serde test**

Append to the existing test module in `crates/game-core/src/event.rs` (or add `#[cfg(test)] mod tests { ... }` if none exists):

```rust
#[cfg(test)]
mod encounter_deck_event_tests {
    use super::*;

    #[test]
    fn encounter_deck_shuffled_serde_roundtrip() {
        let ev = Event::EncounterDeckShuffled;
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }
}
```

- [ ] **Step 2: Run the test to confirm compile failure**

Run:
```bash
cargo test -p game-core encounter_deck_event_tests 2>&1 | head -20
```

Expected: compile error — `no variant named EncounterDeckShuffled found for enum Event`.

- [ ] **Step 3: Add the variant**

In `crates/game-core/src/event.rs`, find the existing `Event::DeckShuffled` variant and add the sibling immediately after it:

```rust
    /// A shuffle of the shared encounter deck occurred. Emitted by
    /// [`shuffle_encounter_deck`](crate::engine::dispatch::shuffle_encounter_deck)
    /// iff the deck had ≥ 2 cards (a 0- or 1-card shuffle is a no-op
    /// and emits nothing). Has no payload — the encounter deck is
    /// shared, so no investigator ID is needed.
    EncounterDeckShuffled,
```

- [ ] **Step 4: Fix exhaustive matches the compiler flags**

Run:
```bash
cargo check --all --all-features 2>&1 | grep -E "non-exhaustive|error" | head -30
```

For each error, add an arm or wildcard for `Event::EncounterDeckShuffled`. Keep wildcards (`_ => ...`) for handler match statements where the new variant doesn't need special treatment; add explicit arms where the match is making structural decisions (assertion helpers, etc.).

- [ ] **Step 5: Run the test, verify pass**

Run:
```bash
cargo test -p game-core encounter_deck_event_tests
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/event.rs
git commit -m "$(cat <<'EOF'
engine: add Event::EncounterDeckShuffled

Sibling to Event::DeckShuffled (which stays player-deck-only).
Emitted by the encounter-deck shuffle helper landing in the
next commit when the deck had ≥ 2 cards.

Refs #72.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add shuffle_encounter_deck helper

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

- [ ] **Step 1: Write the failing test**

Add a new `#[cfg(test)]` module at the bottom of `crates/game-core/src/engine/dispatch.rs` (or sibling — match the file's existing test placement):

```rust
#[cfg(test)]
mod encounter_deck_helper_tests {
    use super::*;
    use crate::event::Event;
    use crate::rng::RngState;
    use crate::state::CardCode;
    use crate::test_support::TestGame;

    #[test]
    fn shuffle_encounter_deck_emits_event_when_two_or_more_cards() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(42);
        state.encounter_deck.push_back(CardCode("a".into()));
        state.encounter_deck.push_back(CardCode("b".into()));
        state.encounter_deck.push_back(CardCode("c".into()));

        let mut events = Vec::new();
        shuffle_encounter_deck(&mut state, &mut events);

        assert!(matches!(events.as_slice(), [Event::EncounterDeckShuffled]));
        assert_eq!(state.encounter_deck.len(), 3);
        // Codes are preserved (only order changes)
        let mut codes: Vec<_> = state.encounter_deck.iter().cloned().collect();
        codes.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(codes, vec![CardCode("a".into()), CardCode("b".into()), CardCode("c".into())]);
    }

    #[test]
    fn shuffle_encounter_deck_is_silent_on_zero_or_one_card() {
        for n in 0..=1 {
            let mut state = TestGame::new().build();
            for i in 0..n {
                state.encounter_deck.push_back(CardCode(format!("c{i}")));
            }
            let mut events = Vec::new();
            shuffle_encounter_deck(&mut state, &mut events);
            assert!(events.is_empty(), "expected no event for n={n} deck");
        }
    }
}
```

`RngState::new(seed: u64)` is the seeded-RNG constructor (`crates/game-core/src/rng.rs:52`).

- [ ] **Step 2: Run test, confirm compile failure**

```bash
cargo test -p game-core encounter_deck_helper_tests 2>&1 | head -10
```

Expected: `cannot find function 'shuffle_encounter_deck'`.

- [ ] **Step 3: Implement `shuffle_encounter_deck`**

In `crates/game-core/src/engine/dispatch.rs`, find `shuffle_player_deck` at line 203 and add the encounter-deck sibling immediately after (around line 240, after `shuffle_player_deck` ends):

```rust
/// Fisher-Yates shuffle of the shared encounter deck using the
/// shared deterministic RNG. Used by [`encounter_deck_shuffled`] and
/// by [`reshuffle_encounter_discard`].
///
/// Emits [`Event::EncounterDeckShuffled`] iff the deck had at least
/// 2 cards (a 0- or 1-card deck has nothing to permute).
pub(super) fn shuffle_encounter_deck(
    state: &mut GameState,
    events: &mut Vec<Event>,
) {
    let deck_len = state.encounter_deck.len();
    if deck_len < 2 {
        return;
    }
    // Mirror shuffle_player_deck's "collect swaps then apply" pattern:
    // RngState::next_index borrows &mut state.rng, which would conflict
    // with a &mut borrow on state.encounter_deck inline.
    let mut swaps: Vec<(usize, usize)> = Vec::with_capacity(deck_len - 1);
    let mut i = deck_len - 1;
    while i >= 1 {
        let j = state.rng.next_index(i + 1);
        swaps.push((i, j));
        i -= 1;
    }
    for (a, b) in swaps {
        state.encounter_deck.swap(a, b);
    }
    events.push(Event::EncounterDeckShuffled);
}
```

- [ ] **Step 4: Run the tests, verify pass**

```bash
cargo test -p game-core encounter_deck_helper_tests
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add shuffle_encounter_deck helper

Fisher-Yates shuffle of GameState.encounter_deck via the seeded
RngState, mirroring shuffle_player_deck. Emits
Event::EncounterDeckShuffled iff ≥ 2 cards (no-op otherwise).

Refs #72.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Add reshuffle_encounter_discard helper

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

- [ ] **Step 1: Write the failing test**

Append to `mod encounter_deck_helper_tests` in `crates/game-core/src/engine/dispatch.rs`:

```rust
    #[test]
    fn reshuffle_encounter_discard_moves_discard_into_deck_and_shuffles() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(7);
        for i in 0..5 {
            state.encounter_discard.push(CardCode(format!("d{i}")));
        }

        let mut events = Vec::new();
        reshuffle_encounter_discard(&mut state, &mut events);

        assert!(state.encounter_discard.is_empty(), "discard should be drained");
        assert_eq!(state.encounter_deck.len(), 5, "all 5 cards moved into deck");
        assert!(
            matches!(events.as_slice(), [Event::EncounterDeckShuffled]),
            "expected EncounterDeckShuffled (≥ 2 cards moved)"
        );
    }

    #[test]
    fn reshuffle_encounter_discard_is_silent_when_discard_has_one_card() {
        let mut state = TestGame::new().build();
        state.encounter_discard.push(CardCode("solo".into()));

        let mut events = Vec::new();
        reshuffle_encounter_discard(&mut state, &mut events);

        assert!(state.encounter_discard.is_empty());
        assert_eq!(state.encounter_deck.len(), 1);
        assert!(events.is_empty(), "1-card shuffle emits no event");
    }
```

- [ ] **Step 2: Run, confirm compile failure**

```bash
cargo test -p game-core encounter_deck_helper_tests 2>&1 | head -10
```

Expected: `cannot find function 'reshuffle_encounter_discard'`.

- [ ] **Step 3: Implement `reshuffle_encounter_discard`**

In `crates/game-core/src/engine/dispatch.rs`, immediately after `shuffle_encounter_deck`:

```rust
/// Drain `state.encounter_discard` into `state.encounter_deck` and
/// shuffle the resulting deck. Called by
/// [`draw_encounter_top`] when the deck runs empty.
///
/// Does NOT push an `EngineRecord::EncounterDeckShuffled` to the
/// action log — mid-handler reshuffles rely on RNG determinism for
/// replay rather than log entries, mirroring the existing
/// player-deck pattern. The `EngineRecord` variant is reserved for
/// explicit shuffle actions (future "shuffle X into the encounter
/// deck" effects).
pub(super) fn reshuffle_encounter_discard(
    state: &mut GameState,
    events: &mut Vec<Event>,
) {
    state.encounter_deck.extend(state.encounter_discard.drain(..));
    shuffle_encounter_deck(state, events);
}
```

- [ ] **Step 4: Run, verify pass**

```bash
cargo test -p game-core encounter_deck_helper_tests
```

Expected: 4 tests pass total in this module.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add reshuffle_encounter_discard helper

Drains encounter_discard into encounter_deck and shuffles. Called
internally by draw_encounter_top on empty-deck. Does not push an
EngineRecord — replay determinism comes from the seeded RNG,
matching the player-deck mid-handler reshuffle pattern.

Refs #72.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Add draw_encounter_top helper

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

- [ ] **Step 1: Write the failing tests**

Append to `mod encounter_deck_helper_tests`:

```rust
    #[test]
    fn draw_encounter_top_drains_deck_then_returns_none() {
        let mut state = TestGame::new().build();
        state.encounter_deck.push_back(CardCode("a".into()));
        state.encounter_deck.push_back(CardCode("b".into()));
        state.encounter_deck.push_back(CardCode("c".into()));

        let mut events = Vec::new();

        assert_eq!(draw_encounter_top(&mut state, &mut events), Some(CardCode("a".into())));
        assert_eq!(draw_encounter_top(&mut state, &mut events), Some(CardCode("b".into())));
        assert_eq!(draw_encounter_top(&mut state, &mut events), Some(CardCode("c".into())));
        assert_eq!(draw_encounter_top(&mut state, &mut events), None);
        // No shuffle on the trivial path (discard was empty).
        assert!(events.is_empty(), "no events when draining a non-empty deck");
    }

    #[test]
    fn draw_encounter_top_reshuffles_discard_on_empty_deck() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(13);
        state.encounter_discard.push(CardCode("x".into()));
        state.encounter_discard.push(CardCode("y".into()));
        state.encounter_discard.push(CardCode("z".into()));

        let mut events = Vec::new();
        let drawn = draw_encounter_top(&mut state, &mut events);

        assert!(drawn.is_some(), "should reshuffle and draw");
        assert_eq!(state.encounter_deck.len(), 2, "2 cards remain in deck post-draw");
        assert!(state.encounter_discard.is_empty(), "discard drained");
        assert!(
            matches!(events.as_slice(), [Event::EncounterDeckShuffled]),
            "reshuffle emits one event"
        );
    }

    #[test]
    fn draw_encounter_top_returns_none_when_deck_and_discard_both_empty() {
        let mut state = TestGame::new().build();
        let mut events = Vec::new();
        assert_eq!(draw_encounter_top(&mut state, &mut events), None);
        assert!(events.is_empty(), "no events on empty-on-both");
    }
```

- [ ] **Step 2: Run, confirm compile failure**

```bash
cargo test -p game-core encounter_deck_helper_tests 2>&1 | head -10
```

Expected: `cannot find function 'draw_encounter_top'`.

- [ ] **Step 3: Implement `draw_encounter_top`**

In `crates/game-core/src/engine/dispatch.rs`, immediately after `reshuffle_encounter_discard`:

```rust
/// Draw the top card of the encounter deck, transparently reshuffling
/// the discard back in if the deck is empty.
///
/// Returns `Some(code)` when a card was available (either from the
/// deck directly or after the reshuffle). Returns `None` when both
/// the deck and the discard are empty — callers decide how to
/// interpret this (#69's Mythos loop treats it as a scenario
/// condition rather than an engine error).
pub(super) fn draw_encounter_top(
    state: &mut GameState,
    events: &mut Vec<Event>,
) -> Option<CardCode> {
    if state.encounter_deck.is_empty() {
        if state.encounter_discard.is_empty() {
            return None;
        }
        reshuffle_encounter_discard(state, events);
    }
    state.encounter_deck.pop_front()
}
```

You may need to add a `use` for `CardCode` at the top of `dispatch.rs` if it isn't already in scope (likely is — verify with `cargo check`).

- [ ] **Step 4: Run, verify pass**

```bash
cargo test -p game-core encounter_deck_helper_tests
```

Expected: 7 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add draw_encounter_top helper

Pops the top of GameState.encounter_deck; on empty, transparently
calls reshuffle_encounter_discard and retries. Returns None when
both deck and discard are empty (caller-interpreted scenario
condition, not an engine error).

Refs #72.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Add EngineRecord::EncounterDeckShuffled + dispatch handler

**Files:**
- Modify: `crates/game-core/src/action.rs`
- Modify: `crates/game-core/src/engine/dispatch.rs`

- [ ] **Step 1: Write the failing apply-driven test**

Add a new test module at the bottom of `crates/game-core/src/engine/dispatch.rs`, or extend `encounter_deck_helper_tests`:

```rust
    #[test]
    fn engine_record_encounter_deck_shuffled_drives_shuffle() {
        use crate::action::{Action, EngineRecord};
        use crate::engine::apply;

        let mut state = TestGame::new().build();
        state.rng = RngState::new(99);
        for i in 0..4 {
            state.encounter_deck.push_back(CardCode(format!("c{i}")));
        }
        let original: Vec<_> = state.encounter_deck.iter().cloned().collect();

        let result = apply(state, Action::Engine(EngineRecord::EncounterDeckShuffled));

        assert!(
            matches!(result.outcome, crate::EngineOutcome::Done),
            "expected Done, got {:?}",
            result.outcome
        );
        // Codes preserved
        let mut shuffled: Vec<_> = result.state.encounter_deck.iter().cloned().collect();
        let mut orig_sorted = original.clone();
        shuffled.sort_by(|a, b| a.0.cmp(&b.0));
        orig_sorted.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(shuffled, orig_sorted);
        // Event emitted (≥ 2 cards)
        assert!(result.events.iter().any(|e| matches!(e, Event::EncounterDeckShuffled)));
    }
```

The exact `apply` import and `Action` wrapping (`Action::Engine(...)`) follow whatever pattern other in-crate tests use — search `crates/game-core/src/engine/mod.rs` for the existing `EngineRecord::DeckShuffled` test (line 1133-ish per the earlier grep) and mirror its shape exactly.

- [ ] **Step 2: Run, confirm compile failure**

```bash
cargo test -p game-core engine_record_encounter_deck_shuffled 2>&1 | head -10
```

Expected: `no variant named EncounterDeckShuffled found for enum EngineRecord`.

- [ ] **Step 3: Add the EngineRecord variant**

In `crates/game-core/src/action.rs`, find the existing `EngineRecord::DeckShuffled` variant (around line 252) and add the sibling immediately after:

```rust
    /// Shuffle the shared encounter deck. Reserved for explicit
    /// shuffle effects ("shuffle X into the encounter deck") — the
    /// empty-deck reshuffle inside [`draw_encounter_top`](crate::engine::dispatch::draw_encounter_top)
    /// happens as an in-handler side effect and does NOT push this
    /// variant. No payload — the deck is shared.
    EncounterDeckShuffled,
```

- [ ] **Step 4: Add the handler and dispatch arm**

In `crates/game-core/src/engine/dispatch.rs`, extend `apply_engine_record` at line 173:

```rust
pub fn apply_engine_record(
    state: &mut GameState,
    events: &mut Vec<Event>,
    record: &EngineRecord,
) -> EngineOutcome {
    match record {
        EngineRecord::DeckShuffled { investigator } => deck_shuffled(state, events, *investigator),
        EngineRecord::EncounterDeckShuffled => encounter_deck_shuffled(state, events),
    }
}
```

Add the handler immediately after `deck_shuffled` (around line 195):

```rust
/// Handler for [`EngineRecord::EncounterDeckShuffled`].
///
/// Permutes the shared encounter deck via the deterministic RNG and
/// emits [`Event::EncounterDeckShuffled`] (when ≥ 2 cards). No
/// validation — the encounter deck is shared, so there's no
/// per-investigator existence check.
fn encounter_deck_shuffled(
    state: &mut GameState,
    events: &mut Vec<Event>,
) -> EngineOutcome {
    shuffle_encounter_deck(state, events);
    EngineOutcome::Done
}
```

- [ ] **Step 5: Run, verify pass**

```bash
cargo test -p game-core encounter_deck
```

Expected: all `encounter_deck_*` tests pass (helpers + the new apply-driven test).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/action.rs crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add EngineRecord::EncounterDeckShuffled + handler

Sibling to EngineRecord::DeckShuffled. Wires the explicit
"shuffle the encounter deck" action through apply() to the
shuffle_encounter_deck helper. The empty-deck reshuffle path
inside draw_encounter_top remains a silent side effect (no
record pushed) per the existing player-deck pattern.

Refs #72.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Determinism integration test

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

- [ ] **Step 1: Write the determinism test**

Append to `mod encounter_deck_helper_tests`:

```rust
    #[test]
    fn encounter_deck_shuffle_is_deterministic_from_seed() {
        fn shuffle_with_seed(seed: u64) -> Vec<CardCode> {
            let mut state = TestGame::new().build();
            state.rng = RngState::new(seed);
            for i in 0..10 {
                state.encounter_deck.push_back(CardCode(format!("c{i:02}")));
            }
            let mut events = Vec::new();
            shuffle_encounter_deck(&mut state, &mut events);
            state.encounter_deck.iter().cloned().collect()
        }

        let a = shuffle_with_seed(2026);
        let b = shuffle_with_seed(2026);
        assert_eq!(a, b, "same seed must produce same shuffle order");

        let c = shuffle_with_seed(42);
        assert_ne!(a, c, "different seeds should produce different orders (smoke test)");
    }
```

- [ ] **Step 2: Run, verify pass**

```bash
cargo test -p game-core encounter_deck_shuffle_is_deterministic
```

Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
test: encounter-deck shuffle is deterministic from seed

Adds a regression test asserting that two identical setups with
the same RngState seed produce identical post-shuffle order, and
smoke-checks that different seeds diverge.

Refs #72.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Run the full local CI gauntlet

**Files:** none modified.

CI runs five jobs with strict flags (CLAUDE.md §Commands). Plain `cargo test` is NOT sufficient — it misses `-D warnings`, doc-link checks, clippy lints, formatting, and the wasm build.

- [ ] **Step 1: Run all five locally, in order**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```

Expected: all five exit 0.

- [ ] **Step 2: Fix anything that fails**

Most likely failure modes:
- `cargo fmt --check` flags unformatted code → `cargo fmt` and amend the most recent commit (or add a small "fix: formatting" follow-up commit; the user prefers new commits over amends per CLAUDE.md).
- `cargo doc` flags broken intra-doc links → look for `[`...`]` references to types that moved or don't exist. Common culprits: `[`encounter_deck`]` without the `Self::` prefix, or `[`shuffle_encounter_deck`]` without the full module path.
- `cargo clippy` flags new lints → fix them; do not add `#[allow(...)]` unless documented as intentional.

If a fix requires significant changes, add a new commit. Do NOT amend any of Tasks 2–8's commits — amend can rewrite already-pushed history if a push happened.

---

## Task 10: Phase-doc update (LAST commit before pushing)

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

Per CLAUDE.md §PR procedure step 7, the phase doc is touched exactly once per PR, as the final commit. Do not interleave with implementation commits.

- [ ] **Step 1: Read the current phase-4 doc**

Open `docs/phases/phase-4-scenario-plumbing.md` and note:
- Status section (line ~5): `#72` listed in the "Remaining" enumeration.
- Issues table (line ~13): `#72` row in the main Open table.
- Closed table (line ~26): currently `#103` and `#74` only.
- Ordering table (line ~42): row 3 is `#72`.
- Decisions made (line ~60): existing entries for `#103` and `#74`.

- [ ] **Step 2: Make the four updates**

**A. Status section:** Update the "Remaining" enumeration to drop `#72`:
- Before: `Remaining: #72, #126, #127, #69, #70, #71, #128, #73.`
- After: `Remaining: #126, #127, #69, #70, #71, #128, #73.`
- Also adjust the lead-in sentence ("First two PRs merged" → "First three PRs merged", adjust "this → #126 → #127" framing if present elsewhere).

**B. Issues table:** Remove the `#72` row from the Open table.

**C. Closed table:** Add `#72` row at the bottom (preserve `#103`/`#74` rows above; chronological insertion):

```markdown
| `#72` | encounter deck state | #<PR-number> | `GameState.encounter_deck: VecDeque<CardCode>` + `encounter_discard: Vec<CardCode>`. Helpers `shuffle_encounter_deck`/`reshuffle_encounter_discard`/`draw_encounter_top` mirror the existing player-deck pattern. `EngineRecord::EncounterDeckShuffled` + `Event::EncounterDeckShuffled` sibling variants. |
```

**D. Ordering table:** Flip row 3:
- Before: `| 3 | #72 encounter deck state | Independent of #74's API beyond GameState. Sets up the data Mythos will draw from. |`
- After: `| 3 | #72 encounter deck state | ✅ PR #<PR-number>. Sets up the data Mythos will draw from. Helpers in `crates/game-core/src/engine/dispatch.rs` mirror the existing player-deck shape. |`

**E. Decisions made:** Insert a new entry near the bottom of the "Decisions made" section (after the existing `#74` entries):

```markdown
- **Additive sibling for `DeckShuffled` (`#72`, PR #<PR-number>).** Encounter deck shuffles ride a new `EngineRecord::EncounterDeckShuffled` / `Event::EncounterDeckShuffled` rather than renaming or tagging the existing `DeckShuffled` (which stays player-deck-only). Trade-off: one variant says "player" implicitly, the other says "encounter" explicitly. Worth re-examining if act/agenda decks join the family — at that point a tagged `DeckKind` refactor becomes load-bearing. The mid-handler reshuffle path (`reshuffle_encounter_discard` called from `draw_encounter_top` on empty deck) does NOT push the `EngineRecord` — mirrors the player-deck pattern where replay determinism comes from the seeded RNG, not log entries.
```

Replace `<PR-number>` with the actual number after `gh pr create` returns it. If the PR isn't open yet, leave the placeholder, push, open the PR, then amend (or follow-up commit) with the real number.

- [ ] **Step 3: Verify the changes render correctly**

```bash
git diff docs/phases/phase-4-scenario-plumbing.md
```

Expected: the four updates above, no other changes.

- [ ] **Step 4: Commit**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: phase-4 plan — close #72 (encounter deck state)

Marks #72 closed in the phase plan. Adds a Decision entry on the
additive-sibling pattern for DeckShuffled and the mid-handler
no-EngineRecord reshuffle convention.

Refs #72.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Push and open the PR

**Files:** none modified.

Follows CLAUDE.md §PR procedure steps 2–4.

- [ ] **Step 1: Push the branch**

```bash
git push -u origin engine/encounter-deck-state
```

- [ ] **Step 2: Open the PR via `gh pr create`**

```bash
gh pr create --title "engine: encounter deck state (#72)" --body "$(cat <<'EOF'
## Summary
- Adds `GameState.encounter_deck: VecDeque<CardCode>` + `encounter_discard: Vec<CardCode>`.
- Adds private helpers `shuffle_encounter_deck`, `reshuffle_encounter_discard`, `draw_encounter_top` in `engine/dispatch.rs`, mirroring the existing `shuffle_player_deck` shape.
- Adds `EngineRecord::EncounterDeckShuffled` + `Event::EncounterDeckShuffled` as additive siblings to the existing `DeckShuffled` variants (which stay player-deck-only).
- Adds the `encounter_deck_shuffled` dispatch handler for the explicit-shuffle action.

## Design decisions
- **Additive siblings** over rename / tagged refactor for `DeckShuffled`. Cheapest blast radius; the existing player-deck variant doesn't churn. A future tagged `DeckKind` refactor becomes worthwhile when act / agenda decks land.
- **Mid-handler reshuffle does NOT push an `EngineRecord`.** `reshuffle_encounter_discard` is called as a side effect from inside `draw_encounter_top` when the deck runs empty; replay determinism comes from the seeded `RngState`, not log entries. Mirrors how player-deck mid-handler reshuffles work today.

## Out of scope
- Scenario-setup wiring for populating the encounter deck — lands incidentally in #126 alongside the synthetic fixture.
- "Discard X cards from the encounter deck" effects — land with the card that needs them.
- Act / agenda decks — separate issue when content forces them.

## Test plan
- [x] State serde roundtrip with non-empty encounter deck + discard.
- [x] `draw_encounter_top` drains to empty, returns `None` past the last card.
- [x] `draw_encounter_top` reshuffles discard back when deck empties; emits `Event::EncounterDeckShuffled`.
- [x] Empty-on-both → `None`, no event.
- [x] `shuffle_encounter_deck` silent on 0- or 1-card deck.
- [x] `apply(Action::Engine(EngineRecord::EncounterDeckShuffled))` drives the shuffle.
- [x] Determinism: same seed → same shuffle order.
- [x] Full CI gauntlet locally (test / clippy / fmt / doc / wasm build).

Closes #72.
EOF
)"
```

- [ ] **Step 3: Update the phase doc with the real PR number**

After `gh pr create` returns the PR URL, extract the number and either:
- amend the phase-doc commit (only if you haven't been asked to keep history granular), or
- add a small follow-up commit: `docs: phase-4 plan — fill in #72 PR number`.

Push the update.

- [ ] **Step 4: Watch CI in the background and spawn the review-agent in parallel**

Per CLAUDE.md §PR procedure step 4, these run concurrently:

In the foreground (one command, background process):
```bash
gh pr checks <PR-number> --watch
```

In parallel, spawn the `review-agent` subagent with the PR number, branch name, and design-decisions paragraph from the PR body. (See CLAUDE.md and the user's `feedback_pr_review_process.md` memory for the exact handoff shape.)

- [ ] **Step 5: Present review-agent findings to the user**

Per CLAUDE.md §PR procedure step 5 AND the user's `feedback_present_review_agent_notes.md` memory: surface every finding verbatim, severity-bucketed. Do not pre-digest into a paragraph. CI status (pass / fail) is reported separately; review findings stand on their own.

- [ ] **Step 6: Address any CI failures via follow-up commits**

Do not amend / force-push. Add `fix: ...` commits to the same branch. CI re-runs automatically.

- [ ] **Step 7: Wait for explicit user approval before merging**

Once CI is green and review feedback is addressed, surface the merge decision to the user. Do NOT merge autonomously.

When approved:
```bash
gh pr merge <PR-number> --squash --delete-branch
git checkout main && git pull
```

Verify the issue auto-closed via `gh issue view 72`.

---

## Self-review notes for the executor

Before submitting Task 11's PR, double-check:

- Every commit message ends with `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>`.
- No commit mixes phase-doc edits with implementation (Task 10 is the only phase-doc commit).
- Every commit compiles cleanly under the full CI gauntlet (Task 9 runs at the end, but ideally each commit individually compiles too — bisecting depends on it).
- No commit force-pushes / amends previously-pushed commits.
- `Event::CardRevealed`, `EngineRecord::EncounterCardRevealed`, and the `encounter_card_revealed` handler are NOT in this PR — they belong to #126.
