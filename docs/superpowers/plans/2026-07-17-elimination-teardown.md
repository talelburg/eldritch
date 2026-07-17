# Elimination Teardown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An eliminated investigator stops driving the engine — abandon their in-flight skill test instead of panicking on the hand elimination drained (#564), drain their threat area to the right pile, and stop scanning them for round-end/game-end forced triggers (#567).

**Architecture:** Three independent changes behind one rules story (RR p.10 "Elimination"). (A) A `Status::Active` gate at the head of `skill_test::advance`'s loop abandons the test — mirroring the documented `AttackLoop` early-break at `combat.rs:601` — plus defensive totality on the three hand-indexing helpers. (B) `run_elimination_steps` partitions the threat area: `weakness: true` → `removed_from_game` (step 1), everything else → `encounter_discard` (step 4, via the existing `discard_from_threat_area`). (C) `Status::Active` filters on the `RoundEnded`/`GameEnd` forced scans.

**Tech Stack:** Rust (workspace), `cargo test`/`clippy`/`fmt`, `game-core` (kernel, no I/O) + `cards` (content + integration tests).

**Design doc:** `docs/superpowers/specs/2026-07-17-elimination-teardown-design.md` — **read its "The rules reading" section before touching Part B.** Both issues' suggested fixes are wrong in ways that are easy to re-introduce.

## Global Constraints

- **CI gauntlet, all warnings-as-errors.** Every one must pass before push:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
- **Handler contract:** validate-first / mutate-second. Check every precondition, return `EngineOutcome::Rejected { reason }` with state+events unchanged, mutate only after.
- **Determinism:** no `HashMap`/`HashSet` in `game-core` — `BTreeMap`/`BTreeSet`/`Vec` only. Filtering a `BTreeMap` iteration preserves the frozen enumeration order (#570's contract).
- **No silent approximation.** A should-never-happen state gets a `debug_assert!` tripwire, never a silent skip.
- **Never hand-edit** `crates/cards/src/generated/cards.rs`.
- **No new public API for tests.** `apply_investigator_defeat` is `pub(super)` within `dispatch` and stays that way — every integration test below reaches elimination through the **real** path (a lethal Grasping Hands revelation driven via `apply`), which is also how the repo's other `crates/cards/tests/` fixtures work.
- **Test registry limitation (load-bearing for Part B's tests):** `game_core::test_support::install_test_registry`'s `metadata_for` returns `Some` **only** for `TEST_INV` (and its `abilities_for` always returns `None`). A `game-core` unit test therefore **cannot** observe `weakness: true` on a synthetic card. All weakness/encounter routing tests live in `crates/cards/tests/`, where the real `cards::REGISTRY` is installed.
- **Real investigator code in `crates/cards/tests/`:** `test_investigator(n)` sets `investigator_card.code = TEST_INV`, which the *real* registry doesn't know — `max_health()` would panic on `.expect("investigator card code absent from registry")`. Every fixture must override it: `inv.investigator_card.code = CardCode::new("01001");` (Roland Banks, health 9 / sanity 5). `Investigator.skills` is a plain struct field and is **not** derived from that code, so the fixture's `agility: 3` stands.
- **Event assertions** use `assert_event!` / `assert_no_event!` / `assert_event_count!` (order-insensitive) from `game_core`.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/game-core/src/engine/dispatch/skill_test.rs` | skill-test driver | Modify: gate in `advance` (~:714), new `abandon_test`, totality at `:1053`/`:1236`/`:1296` |
| `crates/game-core/src/engine/dispatch/elimination.rs` | RR p.10 steps | Modify: step 1 weakness drain (~:81-86), real step 4 (~:135-140) |
| `crates/game-core/src/engine/dispatch/threat_area.rs` | threat-area zone helpers | Modify: drop dead-code attr + stale comment on `discard_from_threat_area` (:117) |
| `crates/game-core/src/engine/dispatch/forced_triggers.rs` | forced-trigger scans | Modify: `Status::Active` filters at `:322` and `:415` |
| `crates/cards/tests/elimination_teardown.rs` | integration tests (real corpus) | **Create** |
| `docs/audits/2026-07-17-audit.md` | audit record | Modify: annotate #564/#567 as closed |
| `docs/superpowers/specs/2026-07-17-elimination-teardown-design.md` | design | Modify: correct the test-layering claim |

---

### Task 1: Part A — abandon the in-flight test (#564)

**Files:**
- Create: `crates/cards/tests/elimination_teardown.rs`
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs`

**Interfaces:**
- Consumes: `GameState::take_skill_test() -> Option<InFlightSkillTest>` (`state/game_state.rs:1767`); `GameState::has_skill_test_in_flight() -> bool`; `Event::SkillTestEnded { investigator }`; `Status::Active`.
- Produces:
  - `fn abandon_test(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome` — private to `skill_test.rs`; no later task calls it.
  - `fn board_at_lethal_range(damage: u8, hand: &[&str], threat: &[(&str, u8)]) -> game_core::GameState` — test fixture reused by **every** later task.
  - `fn reveal_committing(state: game_core::GameState, commit: &[&str]) -> game_core::ApplyResult` — test driver reused by every later task.

**Background:** Grasping Hands 01162's Revelation is an Agility(3) test whose `on_fail` deals `Count(SkillTestFailedBy)` damage. With `agility: 3` and a rigged `Numeric(-2)` token the total is `1` vs `3` → fail by 2 → 2 damage. Preloading `accumulated_damage = 8` against Roland's health 9 makes that lethal, so the tester is eliminated **inside** the `ApplyResultEffect` step — with `FireOnResolution` and the teardown discard still ahead of the cursor, both of which index the hand elimination just drained.

- [ ] **Step 1: Write the failing test**

Create `crates/cards/tests/elimination_teardown.rs`:

```rust
//! Elimination teardown (#564, #567): an eliminated investigator stops driving
//! the engine — their in-flight skill test is abandoned rather than resolved
//! against the hand elimination drained, and their threat area is emptied to the
//! right pile.
//!
//! Lives in `crates/cards/tests/` because every assertion needs real card
//! metadata (`CardMetadata::weakness`) and abilities — `game-core` can't reach
//! the corpus by crate direction, and `install_test_registry` resolves metadata
//! for `TEST_INV` only.
//!
//! Elimination is reached through the **real** path (a lethal Grasping Hands
//! revelation driven via `apply`); `apply_investigator_defeat` stays `pub(super)`.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-07-17)
//!
//! **Grasping Hands (01162):** "<b>Revelation</b> - Test [agility] (3). If you
//! fail, take 1 damage for each point you failed by."
//! **Overpower (01091):** a plain skill card — two [combat] icons, no triggered
//! ability. Contributes 0 to an agility test, so committing it leaves the fail
//! margin intact while still exercising the committed-index path.
//! **Cover Up (01007):** "<b>Revelation</b> - Put Cover Up into play in your
//! threat area, with 3 clues on it. […] <b>Forced</b> - When the game ends, if
//! there are any clues on Cover Up: You suffer 1 mental trauma."
//! **Dissonant Voices (01165):** "<b>Revelation</b> - Put Dissonant Voices into
//! play in your threat area. You cannot play assets or events. <b>Forced</b> -
//! At the end of the round: Discard Dissonant Voices."

use game_core::action::EngineRecord;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosToken, InvestigatorId, LocationId, Status,
};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event_count, Action, EngineOutcome};

/// Roland Banks — health 9, sanity 5.
const ROLAND: &str = "01001";
const GRASPING_HANDS: &str = "01162";
const OVERPOWER: &str = "01091";
const COVER_UP: &str = "01007";
const DISSONANT_VOICES: &str = "01165";

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Roland at a location with `damage` already on him, `hand` in hand, and
/// `threat` (code, clues) in his threat area (instance ids 1, 2, …). Grasping
/// Hands sits on top of the encounter deck with a rigged `Numeric(-2)` token, so
/// `reveal_committing` puts him through an Agility(3) test he fails by 2.
fn board_at_lethal_range(damage: u8, hand: &[&str], threat: &[(&str, u8)]) -> game_core::GameState {
    let mut inv = test_investigator(1);
    // Real investigator code so max_health() reads from the installed cards
    // registry (#448 cp2a). Roland Banks (01001, 9/5).
    inv.investigator_card.code = CardCode::new(ROLAND);
    inv.investigator_card.accumulated_damage = damage;
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    inv.threat_area = threat
        .iter()
        .enumerate()
        .map(|(i, (code, clues))| {
            let mut card = CardInPlay::enter_play(
                CardCode::new(*code),
                CardInstanceId(u32::try_from(i).expect("fits") + 1),
            );
            card.clues = *clues;
            card
        })
        .collect();
    let mut state = GameStateBuilder::new()
        .with_investigator_at(inv, LocationId(20))
        .with_location(test_location(20, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(-2)];
    state.encounter_deck.push_back(CardCode::new(GRASPING_HANDS));
    state
}

/// Reveal the top encounter card for investigator 1, committing `commit` at the
/// revelation skill-test window.
fn reveal_committing(state: game_core::GameState, commit: &[&str]) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&commit.iter().map(|c| CardCode::new(*c)).collect::<Vec<_>>());
    drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    )
}

#[test]
fn tester_eliminated_mid_test_abandons_the_test_without_panicking() {
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage.
    // Roland at 8/9 damage → lethal → elimination drains the hand while the
    // SkillTest frame is still live at FireOnResolution / the teardown discard.
    let r = reveal_committing(board_at_lethal_range(8, &[OVERPOWER], &[]), &[OVERPOWER]);

    assert_eq!(r.outcome, EngineOutcome::Done, "test abandoned cleanly");

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed, "lethal damage eliminated Roland");

    // RR p.10 step 1: the committed card was in hand, so it is removed from the
    // game — NOT discarded by the skill-test teardown.
    assert!(inv.hand.is_empty(), "hand drained by elimination");
    assert!(
        inv.discard.is_empty(),
        "committed card must not be discarded after elimination; discard = {:?}",
        inv.discard
    );
    assert!(
        inv.removed_from_game
            .iter()
            .any(|c| c.as_str() == OVERPOWER),
        "committed card removed from game; removed = {:?}",
        inv.removed_from_game
    );

    // The frame is gone and the test closed exactly once.
    assert!(
        !r.state.has_skill_test_in_flight(),
        "SkillTest frame torn down"
    );
    assert_event_count!(r.events, Event::SkillTestEnded { .. }, 1);
}

#[test]
fn surviving_tester_still_discards_committed_cards() {
    // Control: same board, no preloaded damage → 2 damage is survivable → the
    // normal teardown runs and the committed card goes to the discard.
    let r = reveal_committing(board_at_lethal_range(0, &[OVERPOWER], &[]), &[OVERPOWER]);

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Active, "2 damage is not lethal at 0/9");
    assert!(
        inv.discard.iter().any(|c| c.as_str() == OVERPOWER),
        "surviving tester discards committed cards; discard = {:?}",
        inv.discard
    );
    assert_event_count!(r.events, Event::SkillTestEnded { .. }, 1);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cards --test elimination_teardown tester_eliminated_mid_test_abandons_the_test_without_panicking`

Expected: **FAIL with a panic**, not an assertion failure — `index out of bounds: the len is 0 but the index is 0` (or `removal index (is 0) should be < len (is 0)`) originating in `skill_test.rs`. That panic *is* the bug. `surviving_tester_still_discards_committed_cards` should already PASS.

- [ ] **Step 3: Add the gate to `advance`**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, in `advance`, immediately **after** the `let (continuation, investigator, indices_u8) = { … };` block and **before** `match continuation {`:

```rust
        // RR p.10 Elimination step 1 removed every card this investigator owns
        // from the game — including the hand `indices_u8` points into. Abandon
        // the test rather than resolve it on behalf of someone who has left the
        // scenario. Mirrors `drive_attack_loop`'s early-break (`combat.rs`),
        // which drops remaining attackers on the same signal (#564).
        //
        // This is the single choke point: both the `drive` loop's `SkillTest`
        // arm and the `finish_skill_test` commit hop reach the driver here.
        if cx
            .state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.status != Status::Active)
        {
            return abandon_test(cx, investigator);
        }
```

- [ ] **Step 4: Add `abandon_test`**

In the same file, immediately **after** the `advance` function's closing brace:

```rust
/// Tear down an in-flight skill test whose tester was eliminated mid-resolution
/// (#564), and pop its frame.
///
/// Mirrors the [`PostOnResolution`](SkillTestStep::PostOnResolution) teardown
/// **minus the committed-card discard**: Rules Reference p.10 Elimination step 1
/// ("The cards he or she controls in play and all of the cards in his or her
/// out-of-play areas (such as hand, deck, discard pile) are removed from the
/// game") already removed the committed cards — they were still in hand, since
/// the driver only discards at teardown. Discarding here would resurrect them
/// into a pile.
///
/// [`Event::SkillTestEnded`] still fires: the test *is* over, and it is the
/// documented "test is fully over" signal downstream listeners key on.
fn abandon_test(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    cx.events.push(Event::SkillTestEnded { investigator });
    // ModifierScope::ThisSkillTest contributions expire with the test. Drain
    // this investigator's pending entries only — entries queued for other
    // investigators' future tests stay (as in the normal teardown).
    cx.state
        .pending_skill_modifiers
        .retain(|m| m.investigator != investigator);
    let taken = cx.state.take_skill_test();
    debug_assert!(
        taken.is_some(),
        "abandon_test: no SkillTest frame on the continuation stack",
    );
    EngineOutcome::Done
}
```

- [ ] **Step 5: Run both tests to verify they pass**

Run: `cargo test -p cards --test elimination_teardown`
Expected: PASS (2 passed).

- [ ] **Step 6: Run the game-core suite for regressions**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS. The gate is a no-op for every `Active` tester, so nothing should move.

- [ ] **Step 7: Commit**

```bash
git add crates/cards/tests/elimination_teardown.rs crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: abandon an in-flight skill test when its tester is eliminated

RR p.10 Elimination step 1 drains the tester's hand, but nothing tore down
the live SkillTest frame — the driver then indexed the drained hand with
pre-elimination commit indices and panicked. A panic is not covered by
apply_via's Rejected-only rollback: it kills the apply and replays from the
persisted log every time.

Gate at advance()'s loop head (the driver's single entry point, covering
both the drive loop and the finish_skill_test commit hop), mirroring
drive_attack_loop's documented early-break. abandon_test mirrors the normal
teardown minus the discard — step 1 already removed the committed cards from
the game.

Refs #564.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Part A — defensive totality on the hand-indexing helpers (#564)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`:1053`, `:1236`, `:1296`)

**Interfaces:**
- Consumes: nothing from Task 1 (independent edit to the same file — rebase on Task 1's commit).
- Produces: nothing. All three helper signatures are unchanged.

**Why:** Unreachable once Task 1's gate lands. This is a structural backstop so no future path can re-introduce a panic in a production `apply`. The tripwire (rather than a silent skip) keeps it inside the no-silent-approximation rule.

**Do NOT touch `skill_test.rs:940`.** It is a fourth `inv.hand[usize::from(i)]`, but it sits in the commit-time validation path immediately behind an explicit `if (i as usize) >= hand_len` bounds check and runs while the tester is necessarily `Active`. Not a panic site.

- [ ] **Step 1: Make `discard_committed_cards` total**

In `discard_committed_cards`, replace the `.map(|&idx| { … })` closure body:

```rust
        sorted
            .iter()
            .filter_map(|&idx| {
                // Unreachable: `advance`'s Status gate (#564) abandons the test
                // before teardown if the tester was eliminated, and elimination
                // is the only path that drains a live tester's hand. Total
                // rather than indexing so a future path can't panic a
                // production apply (a panic escapes apply_via's rollback).
                if usize::from(idx) >= inv.hand.len() {
                    debug_assert!(
                        false,
                        "discard_committed_cards: committed index {idx} out of bounds \
                         (hand size {})",
                        inv.hand.len(),
                    );
                    return None;
                }
                let code = inv.hand.remove(usize::from(idx));
                inv.discard.push(code.clone());
                Some(code)
            })
            .collect()
```

- [ ] **Step 2: Make `collect_on_skill_test_resolution` total**

Replace its `indices_u8.iter().map(…)` chain:

```rust
        indices_u8
            .iter()
            .filter_map(|&i| {
                let code = inv.hand.get(usize::from(i));
                debug_assert!(
                    code.is_some(),
                    "collect_on_skill_test_resolution: committed index {i} out of bounds \
                     (hand size {})",
                    inv.hand.len(),
                );
                code.cloned()
            })
            .collect()
```

- [ ] **Step 3: Make `collect_on_commit` total**

Replace its `indices_u8.iter().map(…)` chain:

```rust
        indices_u8
            .iter()
            .filter_map(|&i| {
                let code = inv.hand.get(usize::from(i));
                debug_assert!(
                    code.is_some(),
                    "collect_on_commit: committed index {i} out of bounds (hand size {})",
                    inv.hand.len(),
                );
                code.cloned()
            })
            .collect()
```

- [ ] **Step 4: Run the tests**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core && cargo test -p cards`
Expected: PASS. Behaviour is unchanged for every in-bounds index; `debug_assert!` is compiled out of release and never fires in tests (the gate prevents the state).

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: make the skill-test hand-indexing helpers total

Backstop for #564: the three helpers that index the hand by committed index
now skip with a debug_assert tripwire instead of panicking. Unreachable once
advance()'s Status gate lands, but a panic escapes apply_via's Rejected-only
rollback, so the structural guarantee is worth the three lines. A tripwire,
not a silent skip.

skill_test.rs:940 is deliberately untouched — it is behind an explicit
bounds check in the commit-time validation path.

Refs #564.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Part B — partition the threat area on elimination (#567)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/elimination.rs` (step 1 at `:81-86`, step 4 at `:135-140`)
- Modify: `crates/game-core/src/engine/dispatch/threat_area.rs` (`:117`)
- Modify: `crates/cards/tests/elimination_teardown.rs` (append tests)

**Interfaces:**
- Consumes: `board_at_lethal_range` / `reveal_committing` (Task 1); `threat_area::discard_from_threat_area(cx: &mut Cx, investigator: InvestigatorId, instance_id: CardInstanceId) -> bool` (`threat_area.rs:118`) — **unchanged**; it already removes by instance, pushes to `encounter_discard`, and emits `CardDiscarded { from: Zone::ThreatArea }`.
- Produces: nothing new. `run_elimination_steps` stays private.

**Read the design doc's "The rules reading" section first.** Summary of the destinations, which are *not* what #567's body says:

| Card | `weakness` | Owner | Destination | Step |
|---|---|---|---|---|
| Cover Up 01007 | `true` | Roland | `removed_from_game` | 1 |
| Frozen in Fear 01164 | `false` | the scenario | `encounter_discard` | 4 |
| Dissonant Voices 01165 | `false` | the scenario | `encounter_discard` | 4 |

- [ ] **Step 1: Write the failing tests**

**First** extend the file's import block — Task 1's list is minimal by design (an
unused import fails the `-D warnings` gate), so these two arrive with their first
user:

```rust
// `Zone` joins the `game_core::state::{…}` list:
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosToken, InvestigatorId, LocationId, Status, Zone,
};
// `assert_event` joins the `game_core::{…}` list:
use game_core::{assert_event, assert_event_count, Action, EngineOutcome};
```

Then append to `crates/cards/tests/elimination_teardown.rs`:

```rust
#[test]
fn elimination_removes_a_player_owned_weakness_from_the_game() {
    // RR p.10 step 1 + the design's reading: Cover Up is owned by Roland, whose
    // discard pile step 1 just removed from the game — so "the appropriate
    // discard pile" no longer exists and the card is removed.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[(COVER_UP, 3)]), &[]);

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed);
    assert!(inv.threat_area.is_empty(), "threat area drained");
    assert!(
        inv.removed_from_game.iter().any(|c| c.as_str() == COVER_UP),
        "player-owned weakness removed from game; removed = {:?}",
        inv.removed_from_game
    );
    assert!(
        !r.state
            .encounter_discard
            .iter()
            .any(|c| c.as_str() == COVER_UP),
        "a player-owned weakness must NOT go to the encounter discard"
    );
}

#[test]
fn elimination_discards_an_encounter_treachery_to_the_encounter_discard() {
    // RR p.10 step 4: Dissonant Voices is owned by the scenario, so Roland's
    // elimination must not remove it from the game.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[(DISSONANT_VOICES, 0)]), &[]);

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed);
    assert!(inv.threat_area.is_empty(), "threat area drained");
    assert!(
        r.state
            .encounter_discard
            .iter()
            .any(|c| c.as_str() == DISSONANT_VOICES),
        "encounter treachery goes to the encounter discard; discard = {:?}",
        r.state.encounter_discard
    );
    assert!(
        !inv.removed_from_game
            .iter()
            .any(|c| c.as_str() == DISSONANT_VOICES),
        "a scenario-owned card must NOT be removed from the game by an \
         investigator's elimination"
    );
    assert_event!(r.events, Event::CardDiscarded { code, from: Zone::ThreatArea, .. }
        if code.as_str() == DISSONANT_VOICES);
}

#[test]
fn elimination_routes_a_mixed_threat_area_both_ways() {
    let r = reveal_committing(
        board_at_lethal_range(8, &[], &[(COVER_UP, 3), (DISSONANT_VOICES, 0)]),
        &[],
    );

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert!(inv.threat_area.is_empty(), "threat area fully drained");
    assert!(inv.removed_from_game.iter().any(|c| c.as_str() == COVER_UP));
    assert!(r
        .state
        .encounter_discard
        .iter()
        .any(|c| c.as_str() == DISSONANT_VOICES));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cards --test elimination_teardown elimination_`
Expected: **FAIL** — all three, on `threat area drained`. `run_elimination_steps` never touches `threat_area`, so it still holds the cards.

- [ ] **Step 3: Implement step 1's weakness drain**

In `crates/game-core/src/engine/dispatch/elimination.rs`, replace the step-1 block (from `let inv = cx` through `inv.removed_from_game = removed;`) with:

```rust
    // Step 1: remove every card this investigator controls in play and
    // owns in out-of-play areas (hand/deck/discard) from the game.
    //
    // Threat-area cards split by ownership (Rules Reference p.7 names the axis:
    // a defeated card is "placed in the encounter discard pile (or in its
    // owner's discard pile if it is a weakness)"). A **weakness** is owned by
    // this player: step 4 would place it in "the appropriate discard pile", but
    // step 1 removes that very pile from the game one step earlier — so the
    // pile no longer exists and the card is removed. Everything else in the
    // threat area is scenario-owned and is step 4's business (#567).
    let weakness_in_threat_area = |card: &CardInPlay| {
        crate::card_registry::current()
            .and_then(|reg| (reg.metadata_for)(&card.code))
            .is_some_and(|m| m.weakness)
    };
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "run_elimination_steps: investigator {investigator:?} not in map; state corruption"
            )
        });
    // Build the pile in an owned local so each mutation borrows only one
    // field of `inv` at a time (mutating `inv.removed_from_game` directly
    // while borrowing `inv.hand` etc. would double-borrow `inv` — rejected
    // by the borrow checker).
    let mut removed = std::mem::take(&mut inv.removed_from_game);
    removed.extend(inv.cards_in_play.drain(..).map(|c| c.code));
    // Partition the threat area: owned weaknesses leave with their owner here;
    // the rest stay for step 4. No registry installed (engine-only tests with
    // synthetic threat-area cards) ⇒ not a weakness ⇒ step 4.
    let (owned, scenario_owned): (Vec<CardInPlay>, Vec<CardInPlay>) =
        std::mem::take(&mut inv.threat_area)
            .into_iter()
            .partition(weakness_in_threat_area);
    inv.threat_area = scenario_owned;
    removed.extend(owned.into_iter().map(|c| c.code));
    removed.append(&mut inv.hand);
    removed.append(&mut inv.deck);
    removed.append(&mut inv.discard);
    inv.removed_from_game = removed;
```

Move `CardInPlay` and `CardInstanceId` into the file's non-test imports (both are currently `#[cfg(test)]`-only):

```rust
use crate::state::{CardInPlay, CardInstanceId, DefeatCause, EnemyId, InvestigatorId, Status};

#[cfg(test)]
use crate::state::{CardCode, LocationId, Phase};
```

- [ ] **Step 4: Implement step 4's encounter drain**

Replace the stale step-4 comment block (the whole `// Step 4: place other (non-enemy) threat-area cards … Fix tracked in #567.` paragraph) with:

```rust
    // Step 4: "All other cards in the eliminated investigator's threat area are
    // placed in the appropriate discard pile" (Rules Reference p.10). What
    // survives step 1's partition is scenario-owned (Frozen in Fear 01164,
    // Dissonant Voices 01165), so the appropriate pile is the encounter discard
    // — an investigator's elimination must not remove the *scenario's* cards
    // from the game. Engaged enemies are step 3's business, not this drain:
    // they live in `enemies` keyed by `engaged_with`, not in `threat_area`.
    let remaining: Vec<CardInstanceId> = cx
        .state
        .investigators
        .get(&investigator)
        .map(|inv| inv.threat_area.iter().map(|c| c.instance_id).collect())
        .unwrap_or_default();
    for instance_id in remaining {
        let removed = super::threat_area::discard_from_threat_area(cx, investigator, instance_id);
        debug_assert!(
            removed,
            "elimination step 4: threat-area instance {instance_id:?} vanished mid-drain",
        );
    }
```

- [ ] **Step 5: Retire the dead-code attribute on `discard_from_threat_area`**

In `crates/game-core/src/engine/dispatch/threat_area.rs`, delete this line entirely:

```rust
#[cfg_attr(not(test), allow(dead_code))] // C4c (#235) is the first production caller
```

and extend the doc-comment above it (which currently ends at "…`false` if none matched.") with:

```rust
///
/// The encounter discard is unconditionally correct here: the only cards that
/// reach this helper are scenario-owned. A card's own Revelation-placed
/// self-discard (Frozen in Fear 01164, Dissonant Voices 01165) routes through
/// `Effect::DiscardSelf`, and Rules Reference p.10 Elimination step 4 sends an
/// eliminated investigator's *scenario-owned* threat-area cards here — their
/// **owned** weaknesses leave at step 1 instead, so they never arrive (#567).
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p cards --test elimination_teardown`
Expected: PASS (5 passed).

- [ ] **Step 7: Add the registry-absent unit test**

In `elimination.rs`'s `#[cfg(test)] mod elimination_tests`, append:

```rust
    #[test]
    fn elimination_without_registry_treats_threat_area_as_scenario_owned() {
        // No registry ⇒ metadata_for is None ⇒ not a weakness ⇒ step 4. The
        // weakness→removed_from_game routing needs real metadata and is covered
        // by `crates/cards/tests/elimination_teardown.rs` (install_test_registry
        // resolves TEST_INV only).
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.threat_area = vec![CardInPlay::enter_play(
            CardCode::new("01165"),
            CardInstanceId(1),
        )];

        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            DefeatCause::Damage,
        );

        assert!(
            state.investigators[&id].threat_area.is_empty(),
            "threat area drained"
        );
        assert_eq!(
            state.encounter_discard.len(),
            1,
            "no registry ⇒ routed to the encounter discard"
        );
        assert!(state.investigators[&id].removed_from_game.is_empty());
    }
```

**Note:** this test runs without `install_test_registry()`, so `max_health()` is never reached — `apply_investigator_defeat` flips status from the caller's `cause`, it does not re-derive lethality. If a registry-install collision surfaces, that is a signal the test drifted; do not add `install_test_registry()` to "fix" it, since that would make `metadata_for` resolve `TEST_INV` only and leave `01165` still `None`.

- [ ] **Step 8: Run the game-core suite**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/dispatch/elimination.rs \
        crates/game-core/src/engine/dispatch/threat_area.rs \
        crates/cards/tests/elimination_teardown.rs
git commit -m "engine: drain an eliminated investigator's threat area (RR p.10 steps 1+4)

Elimination never touched threat_area, so a dead investigator's Cover Up
kept its GameEnd trauma and Dissonant Voices kept firing round-end forceds.
The step-4 comment claiming threat-area cards were 'not modeled yet' was
stale since place_in_threat_area shipped.

Routes by ownership, which is not what #567 suggested. RR p.7 names the axis
('placed in the encounter discard pile (or in its owner's discard pile if it
is a weakness)'): an owned weakness leaves at step 1, since step 4's 'the
appropriate discard pile' is the owner's — which step 1 removed from the game
one step earlier. Scenario-owned treacheries go to the encounter discard at
step 4; removing *those* from the game would be wrong.

discard_from_threat_area finally has a production caller, so its dead-code
attribute and its stale '#235 is the first production caller' comment go.

Refs #567.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Part C — Status-filter the forced scans (#567)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`:322`, `:415`)
- Modify: `crates/cards/tests/elimination_teardown.rs` (append tests)

**Interfaces:**
- Consumes: `board_at_lethal_range` / `reveal_committing` (Task 1); `game_core::test_support::fire_forced_on_round_end(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome` (**already exists**, `test_support/mod.rs:150`); `Status::Active`.
- Produces: nothing.

**Why this is not belt-and-suspenders** (contra #567's body): `controlled_card_instances()` yields `investigator_card` **+** `cards_in_play` **+** `threat_area`. Step 1 drains `cards_in_play`, Task 3 drains `threat_area` — but `investigator_card` is a non-`Option` field carrying identity/harm/usage (#448) and **cannot** be drained (`max_health()`/`max_sanity()` read it). The filter is its only guard.

**Testability, stated honestly:** the filter has **no observable test in the current corpus.** The only cards it could save us from are exactly the ones Task 3's drain already removes; the one instance the drain can't reach — `investigator_card` — carries no `RoundEnded`/`GameEnd` forced in Core+Dunwich (Roland's is a reaction). So Step 4 below pins the filter's *premise* (that `investigator_card` survives elimination) rather than pretending to observe the filter firing. Do not contort a test to manufacture coverage; the filter is cheap, correct, and guards a real future gap.

- [ ] **Step 1: Write the acceptance tests**

**First** extend the file's import block with `assert_no_event` (its first user —
an unused import fails the `-D warnings` gate):

```rust
use game_core::{assert_event, assert_event_count, assert_no_event, Action, EngineOutcome};
```

Then append to `crates/cards/tests/elimination_teardown.rs`:

```rust
#[test]
fn eliminated_investigator_fires_no_game_end_trauma() {
    // #567's acceptance: Cover Up's Forced ("When the game ends, if there are
    // any clues on Cover Up: You suffer 1 mental trauma") must not fire for a
    // dead Roland — RR p.10 step 1 took the card with him. Solo, so his death
    // latches Resolution::Lost and the game-end forced scan runs for real.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[(COVER_UP, 3)]), &[]);

    // Both asserted so the no-trauma claim below can't pass vacuously: the
    // scenario must actually have ended for the GameEnd scan to have run at all.
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, Event::AllInvestigatorsDefeated)),
        "solo death latches Resolution::Lost; events = {:?}",
        r.events
    );
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "the Lost latch must reach ScenarioResolved, which is what fires the \
         GameEnd forced scan (cf. crates/cards/tests/cover_up.rs, which pins the \
         positive case via AdvanceAct); events = {:?}",
        r.events
    );
    assert_no_event!(r.events, Event::TraumaSuffered { .. });
}

#[test]
fn eliminated_investigator_fires_no_further_round_end_forced() {
    // #567's acceptance: Dissonant Voices' Forced ("At the end of the round:
    // Discard Dissonant Voices") must not fire again for a dead investigator —
    // step 4 already discarded it.
    let mut r = reveal_committing(board_at_lethal_range(8, &[], &[(DISSONANT_VOICES, 0)]), &[]);
    assert_eq!(r.state.investigators[&InvestigatorId(1)].status, Status::Killed);
    let before = r.state.encounter_discard.len();

    let mut events = Vec::new();
    let _ = game_core::test_support::fire_forced_on_round_end(&mut r.state, &mut events);

    assert_eq!(
        r.state.encounter_discard.len(),
        before,
        "no further round-end forced for a dead investigator"
    );
    assert_no_event!(events, Event::CardDiscarded { .. });
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p cards --test elimination_teardown eliminated_investigator`
Expected: **PASS already** — Task 3's drain removed the cards, so the scans find nothing. That is expected: these pin #567's stated acceptance criteria and guard the drain against regression. The filter itself is Step 3.

- [ ] **Step 3: Add the filters**

In `crates/game-core/src/engine/dispatch/forced_triggers.rs`, in **both** the `ForcedTriggerPoint::RoundEnded` arm (the loop at `:322`) and the `ForcedTriggerPoint::GameEnd` arm (the loop at `:415`), replace

```rust
            for (inv_id, inv) in &state.investigators {
```

with

```rust
            // Skip eliminated investigators: Rules Reference p.10 Elimination
            // removes their cards from the game (step 1) / to the encounter
            // discard (step 4), so their in-play instances are already gone —
            // except `investigator_card`, which is a non-Option identity/harm
            // field (#448) that cannot be drained. This filter is that card's
            // only guard (#567). `investigators` is a BTreeMap, so filtering
            // preserves the frozen enumeration order (#570).
            for (inv_id, inv) in state
                .investigators
                .iter()
                .filter(|(_, inv)| inv.status == Status::Active)
            {
```

Confirm `Status` is imported in the file; add `use crate::state::Status;` if not.

- [ ] **Step 4: Pin the filter's premise**

Append to `crates/cards/tests/elimination_teardown.rs`:

```rust
#[test]
fn elimination_does_not_drain_the_investigator_card() {
    // The premise of the Status filter (#567): `investigator_card` is a
    // non-Option field carrying identity + harm (#448) and is read by
    // max_health(), so elimination cannot drain it. Without the filter it would
    // keep contributing GameEnd/RoundEnded forced candidates for a dead
    // investigator. No in-scope investigator card carries such a forced
    // (Roland's is a reaction), so the filter has no observable test today —
    // this pins the premise that makes it necessary.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[]), &[]);

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed);
    assert_eq!(
        inv.investigator_card.code.as_str(),
        ROLAND,
        "the investigator card survives elimination — the premise of the \
         Status filter on the RoundEnded/GameEnd scans",
    );
}
```

- [ ] **Step 5: Run the full suite**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs \
        crates/cards/tests/elimination_teardown.rs
git commit -m "engine: skip eliminated investigators in the RoundEnded/GameEnd forced scans

Both arms iterated every investigator regardless of Status. #567 calls this
belt-and-suspenders with the threat-area drain; it is not. The drain empties
cards_in_play and threat_area, but controlled_card_instances() also yields
investigator_card — a non-Option identity/harm field (#448) that cannot be
drained because max_health() reads it. The filter is its only guard.

No in-scope investigator card carries a RoundEnded/GameEnd forced (Roland's
is a reaction), so the filter has no observable test today; the added test
pins its premise — that the investigator card survives elimination — rather
than manufacturing coverage.

Closes #567.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Replay determinism + docs + the full gauntlet

**Files:**
- Modify: `crates/cards/tests/elimination_teardown.rs`
- Modify: `docs/audits/2026-07-17-audit.md`
- Modify: `docs/superpowers/specs/2026-07-17-elimination-teardown-design.md`

**Interfaces:**
- Consumes: `board_at_lethal_range` / `reveal_committing` (Task 1); `GameState: PartialEq + Eq` (`state/game_state.rs:36`).
- Produces: nothing.

**Why:** #564's third acceptance criterion — "Replay of a log containing the interleaving reproduces the fixed state bit-for-bit." Today the interleaving panics on *every* replay of the persisted log; this pins that it now replays cleanly and deterministically.

- [ ] **Step 1: Write the test**

Append to `crates/cards/tests/elimination_teardown.rs`:

```rust
#[test]
fn the_elimination_interleaving_replays_bit_for_bit() {
    // Driving the same log twice from an identical initial state must land on an
    // identical final state — the property the persisted action log depends on.
    // Before #564 this log panicked deterministically on every replay.
    let first = reveal_committing(board_at_lethal_range(8, &[OVERPOWER], &[(COVER_UP, 3)]), &[OVERPOWER]);
    let second = reveal_committing(board_at_lethal_range(8, &[OVERPOWER], &[(COVER_UP, 3)]), &[OVERPOWER]);

    assert_eq!(first.outcome, EngineOutcome::Done);
    assert_eq!(
        first.state, second.state,
        "the elimination interleaving must replay bit-for-bit"
    );
    assert_eq!(first.events, second.events, "events replay identically too");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p cards --test elimination_teardown the_elimination_interleaving_replays_bit_for_bit`
Expected: PASS.

- [ ] **Step 3: Correct the design doc's test-layering claim**

The spec's Testing section says the weakness/encounter routing is covered by "**Engine unit** (`elimination.rs` `#[cfg(test)]`, `test_support` + test registry)". That is **not possible** — `install_test_registry` resolves metadata for `TEST_INV` only. Replace that whole bullet list with:

```markdown
**Engine unit** (`elimination.rs` `#[cfg(test)]`):

- Registry-absent default: a threat-area card with no metadata is treated as
  scenario-owned → `encounter_discard`, no panic. (The `weakness: true` →
  `removed_from_game` routing needs real metadata and lives in
  `crates/cards/tests/elimination_teardown.rs` — `install_test_registry`'s
  `metadata_for` resolves `TEST_INV` only, and its `abilities_for` always
  returns `None`.)
- `threat_area` empty after elimination.
```

Also correct the integration bullet that says elimination is driven directly: every test reaches it through a lethal Grasping Hands revelation via `apply`, because `apply_investigator_defeat` is `pub(super)` and stays that way.

- [ ] **Step 4: Update the audit record**

In `docs/audits/2026-07-17-audit.md`, "Where the findings went" table, annotate the two closed issues in the engine rows — mirroring the existing `#591 (docs+TODO sweep — **closed via PR #594**)` convention:

- `#564 (p0, mid-test elimination panic)` → `#564 (p0, mid-test elimination panic — **closed via PR #NNN**)`
- `#567 (elimination threat-area)` → `#567 (elimination threat-area — **closed via PR #NNN**)`

Substitute the real PR number once `gh pr create` returns it.

- [ ] **Step 5: Run the full CI gauntlet**

Run each; all must be clean before pushing:

```bash
RUSTFLAGS="-D warnings"    cargo test --all --all-features
                           cargo clippy --all-targets --all-features -- -D warnings
                           cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
                           cargo build -p web --target wasm32-unknown-unknown
                           cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all PASS. (`wasm-pack test --headless --firefox crates/web` is unaffected — no web change — but run it if the environment allows.)

- [ ] **Step 6: Commit**

```bash
git add crates/cards/tests/elimination_teardown.rs \
        docs/audits/2026-07-17-audit.md \
        docs/superpowers/specs/2026-07-17-elimination-teardown-design.md
git commit -m "test: pin bit-for-bit replay of the mid-test elimination interleaving

Closes #564.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Notes for the PR

- Branch `engine/elimination-teardown` already exists and carries the design-doc commit.
- **No phase-doc update.** #564/#567 are unmilestoned (audit fallout), and `docs/phases/README.md` scopes phase docs to milestoned work. The audit record's table is the tracker here (Task 5, Step 4).
- The PR body should carry the rules derivation in brief and link the design doc — the destinations contradict both issues' suggested fixes, and a reviewer working from the issue text alone will think Part B is wrong.
- Correction comments are already posted on [#567](https://github.com/talelburg/eldritch/issues/567#issuecomment-5002744355) and [#564](https://github.com/talelburg/eldritch/issues/564#issuecomment-5002744518).
- `Closes #564.` and `Closes #567.` both belong in the PR body.
</content>
