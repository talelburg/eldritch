# Legal-Action Enumerator — AdvanceAct + sweep (slice 2a-ii-4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the legal-action enumerator — add AdvanceAct (the last open-turn action), by extracting a `check_advance_act` predicate from the handler, and close slice 2a-ii with a whole-enumeration sweep test.

**Architecture:** Extract `check_advance_act(state, investigator) -> Result<u8, Cow>` from `advance_act_action` (matching the `check_play_card`/`check_activate_ability` pattern: pure validation returning the clue threshold on success, the handler's exact rejection reason on failure). The enumerator delegates to it (registry-free, so a `game-core` unit test). A final integration sweep builds a maximal board and cross-checks the full enumeration.

**Tech Stack:** Rust, `game-core` (enumerator + handler) + `cards` (sweep test). No new deps.

## Global Constraints

- **Build + expose, defer routing** (slice decision). Read-only enumerator; AdvanceAct delegates to `check_advance_act` — fidelity by construction.
- **Behaviour-preserving:** `advance_act_action` keeps its exact accept/reject behaviour (same rejection reasons, same mutation). Full host gauntlet green each task: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- **Design of record:** umbrella spec §E; closes slice 2a-ii (after #402/#405/#406).
- **Commit footer** (every commit), verbatim:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
  ```
- **Branch:** `engine/enumerator-act`. One commit per task.

## Reference: current AdvanceAct legality (`advance_act_action`, act_agenda.rs:115)

Rejects (in order) on: phase != Investigation; `act_deck.is_empty()`; the current act `act_deck[act_index].round_end_advance.is_some()` (it advances at round end, not via the action); group clues `< clue_threshold` (summed over `clue_contributors(state, investigator)`). On success spends `clue_threshold` clues and advances / requests resolution. `Act { code, clue_threshold: u8, resolution: Option<Resolution>, round_end_advance: Option<RoundEndAdvance> }`; `state.act_deck: Vec<Act>`, `state.act_index: usize` (default 0). `PlayerAction::AdvanceAct { investigator }`.

---

### Task 1: Extract `check_advance_act` + enumerate AdvanceAct

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/act_agenda.rs` — extract `check_advance_act`; `advance_act_action` delegates.
- Modify: `crates/game-core/src/engine/enumerate.rs` — enumerate AdvanceAct + tests (presence + cross-check extension).

**Interfaces:**
- Produces: `pub(crate) fn check_advance_act(state: &GameState, investigator: InvestigatorId) -> Result<u8, std::borrow::Cow<'static, str>>` — `Ok(clue_threshold)` if the AdvanceAct action is legal now, else `Err(reason)` (the handler's exact reason).

- [ ] **Step 1: Write the failing tests** (in `enumerate.rs` `tests`)

```rust
    /// An open-turn state with an advanceable act (threshold `t`) and the
    /// investigator holding `clues`.
    fn open_turn_with_act(threshold: u8, clues: u8) -> crate::state::GameState {
        use crate::state::{Act, CardCode};
        let mut state = open_turn_state();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .clues = clues;
        state.act_deck = vec![Act {
            code: CardCode("_test_act".into()),
            clue_threshold: threshold,
            resolution: None,
            round_end_advance: None,
        }];
        state
    }

    #[test]
    fn advance_act_offered_when_clues_meet_threshold() {
        let state = open_turn_with_act(2, 2);
        assert!(legal_actions(&state).contains(&PlayerAction::AdvanceAct {
            investigator: InvestigatorId(1),
        }));
    }

    #[test]
    fn advance_act_absent_when_clues_insufficient() {
        let state = open_turn_with_act(2, 1);
        assert!(!legal_actions(&state).contains(&PlayerAction::AdvanceAct {
            investigator: InvestigatorId(1),
        }));
    }

    #[test]
    fn advance_act_absent_with_no_act_deck() {
        // open_turn_state has an empty act_deck → AdvanceAct not offered.
        let state = open_turn_state();
        assert!(!legal_actions(&state).contains(&PlayerAction::AdvanceAct {
            investigator: InvestigatorId(1),
        }));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core enumerate::tests`
Expected: FAIL — `advance_act_offered_when_clues_meet_threshold` (AdvanceAct not enumerated). The `_test_act` import path / `Act` fields must match `crate::state::Act` (mirror act_agenda.rs:457).

- [ ] **Step 3: Extract `check_advance_act` (act_agenda.rs)**

Replace the validation prefix of `advance_act_action` (act_agenda.rs:115, the four `if … return Rejected` blocks + the `threshold`/`total_clues` derivation) so the handler delegates. Add above `advance_act_action`:

```rust
/// Validate the [`AdvanceAct`](crate::action::PlayerAction::AdvanceAct) action
/// without mutating: Investigation phase, a modeled act deck, the current act
/// advances via the action (not a round-end objective), and the group holds at
/// least the act's clue threshold. Returns the threshold on success (so the
/// handler can spend it) or the rejection reason on failure. The enumerator
/// (slice 2a-ii-4, #393) calls this in "is-legal?" mode; `advance_act_action`
/// calls it then mutates.
pub(crate) fn check_advance_act(
    state: &GameState,
    investigator: InvestigatorId,
) -> Result<u8, std::borrow::Cow<'static, str>> {
    if state.phase != Phase::Investigation {
        return Err(format!(
            "AdvanceAct is only valid during the Investigation phase (was {:?})",
            state.phase
        )
        .into());
    }
    if state.act_deck.is_empty() {
        return Err("AdvanceAct: no act deck is modeled for this scenario".into());
    }
    if state.act_deck[state.act_index].round_end_advance.is_some() {
        return Err("this act advances only at the end of the round (its round-end \
                    objective), not via the AdvanceAct action"
            .into());
    }
    let threshold = state.act_deck[state.act_index].clue_threshold;
    let total_clues: u32 = clue_contributors(state, investigator)
        .into_iter()
        .filter_map(|id| state.investigators.get(&id))
        .map(|i| u32::from(i.clues))
        .sum();
    if total_clues < u32::from(threshold) {
        return Err(format!(
            "AdvanceAct: act requires {threshold} clues, group holds {total_clues}"
        )
        .into());
    }
    Ok(threshold)
}
```

Rewrite `advance_act_action` to delegate:

```rust
pub(super) fn advance_act_action(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let threshold = match check_advance_act(cx.state, investigator) {
        Ok(t) => t,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };

    // All validations passed — mutate.
    spend_clues(cx.state, investigator, threshold);
    match cx.state.act_deck[cx.state.act_index].resolution.clone() {
        Some(resolution) => request_resolution(cx.state, resolution),
        None => advance_act(cx),
    }
    EngineOutcome::Done
}
```

(Confirm `std::borrow::Cow` is in scope in act_agenda.rs — add `use std::borrow::Cow;` and use `Cow<'static, str>` if the file already imports it, else fully-qualify as written.)

- [ ] **Step 4: Enumerate AdvanceAct (enumerate.rs)**

Add the call in `legal_actions` (after `push_card_actions`):

```rust
    push_card_actions(state, investigator, &mut actions);
    push_act_actions(state, investigator, &mut actions);
    actions
```

Add the helper:

```rust
/// Append the AdvanceAct action if legal (slice 2a-ii-4, #393) — delegated to
/// `check_advance_act`, registry-free (act decks are scenario state, not card
/// data).
fn push_act_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<PlayerAction>) {
    if crate::engine::dispatch::act_agenda::check_advance_act(state, investigator).is_ok() {
        out.push(PlayerAction::AdvanceAct { investigator });
    }
}
```

- [ ] **Step 5: Extend the game-core cross-check with an act**

In `every_enumerated_action_is_accepted_by_its_handler`, give the investigator clues and a two-act deck so an AdvanceAct is enumerated and applied (advancing act 0 → act 1, a clean `Done`):

```rust
        // An advanceable act (threshold met) → AdvanceAct enumerated; a second
        // act so advancing is a clean transition, not a terminal resolution.
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .clues = 2;
        state.act_deck = vec![
            crate::state::Act {
                code: crate::state::CardCode("_act1".into()),
                clue_threshold: 2,
                resolution: None,
                round_end_advance: None,
            },
            crate::state::Act {
                code: crate::state::CardCode("_act2".into()),
                clue_threshold: 99,
                resolution: None,
                round_end_advance: None,
            },
        ];
```

(Place before the `for action in legal_actions(&state)` loop.)

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test -p game-core enumerate::tests`
Expected: PASS.

- [ ] **Step 7: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "engine: enumerate AdvanceAct via an extracted check_advance_act (slice 2a-ii-4 of #393)

Extracts a pure check_advance_act -> Result<u8, Cow> from advance_act_action
(phase / act-deck / round-end / clue-threshold checks, returning the threshold);
the handler delegates to it then mutates. legal_actions offers AdvanceAct iff
check_advance_act is Ok. Registry-free, so unit-tested in game-core; cross-check
extended with a two-act deck. Behaviour-preserving (same rejection reasons).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

### Task 2: Whole-enumeration sweep + close the slice

A single integration test (registry) on a maximal open-turn board exercising **every** action category at once, asserting (a) the enumeration applies without `Rejected`, and (b) at least one action of each category is present — closing slice 2a-ii. Plus a `legal_actions` doc-comment listing the full action coverage.

**Files:**
- Modify: `crates/cards/tests/enumerate_actions.rs` — the sweep test.
- Modify: `crates/game-core/src/engine/enumerate.rs` — `legal_actions` doc-comment lists the covered actions.

- [ ] **Step 1: Write the failing test**

Add to `crates/cards/tests/enumerate_actions.rs` (extend the helper to take enemies + an act, or build inline). Add an inline maximal-board test:

```rust
#[test]
fn full_enumeration_covers_every_action_category_and_all_apply() {
    use game_core::state::{Act, EnemyId};

    let inst = CardInstanceId(0);
    let mut state = open_turn_state(&[HOLY_ROSARY], vec![flashlight_in_play(inst)]);
    // A connected destination (Move), an engaged enemy (Fight/Evade), a
    // co-located unengaged enemy (Engage), and an advanceable act (AdvanceAct).
    let mut other = test_location(11, "Hall");
    other.revealed = true;
    state
        .locations
        .get_mut(&LOC)
        .unwrap()
        .connections
        .push(other.id);
    let other_id = other.id;
    state.locations.insert(other_id, other);

    let mut foe = game_core::test_support::test_enemy(7, "Ghoul");
    foe.engaged_with = Some(INV);
    foe.current_location = Some(LOC);
    state.enemies.insert(EnemyId(7), foe);
    let mut rat = game_core::test_support::test_enemy(8, "Rat");
    rat.current_location = Some(LOC);
    state.enemies.insert(EnemyId(8), rat);

    state.investigators.get_mut(&INV).unwrap().clues = 2;
    state.act_deck = vec![
        Act {
            code: CardCode("_act1".into()),
            clue_threshold: 2,
            resolution: None,
            round_end_advance: None,
        },
        Act {
            code: CardCode("_act2".into()),
            clue_threshold: 99,
            resolution: None,
            round_end_advance: None,
        },
    ];

    let actions = legal_actions(&state);

    // Every category is represented.
    let has = |p: fn(&PlayerAction) -> bool| actions.iter().any(p);
    assert!(actions.contains(&PlayerAction::EndTurn), "EndTurn");
    assert!(has(|a| matches!(a, PlayerAction::Move { .. })), "Move");
    assert!(has(|a| matches!(a, PlayerAction::Investigate { .. })), "Investigate");
    assert!(has(|a| matches!(a, PlayerAction::Resource { .. })), "Resource");
    assert!(has(|a| matches!(a, PlayerAction::Draw { .. })), "Draw");
    assert!(has(|a| matches!(a, PlayerAction::Fight { .. })), "Fight");
    assert!(has(|a| matches!(a, PlayerAction::Evade { .. })), "Evade");
    assert!(has(|a| matches!(a, PlayerAction::Engage { .. })), "Engage");
    assert!(has(|a| matches!(a, PlayerAction::PlayCard { .. })), "PlayCard");
    assert!(has(|a| matches!(a, PlayerAction::ActivateAbility { .. })), "ActivateAbility");
    assert!(has(|a| matches!(a, PlayerAction::AdvanceAct { .. })), "AdvanceAct");

    // And all of them apply without Rejected.
    for action in actions {
        let result = game_core::apply(state.clone(), Action::Player(action.clone()));
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "enumerated action {action:?} was rejected: {:?}",
            result.outcome,
        );
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cards --test enumerate_actions`
Expected: At this point (Task 1 merged the AdvanceAct enumeration) it may already PASS. If so, this test is a *characterization* of the now-complete enumerator — note it, keep it (it is the slice's closing guarantee). If a category assertion fails, fix the board setup until every category is present (e.g. ensure enough resources/actions). If it fails to compile, add the missing imports (`EnemyId`, `Act`).

- [ ] **Step 3: Add the `legal_actions` coverage doc**

In `enumerate.rs`, extend the `legal_actions` doc-comment to record the now-complete coverage:

```rust
/// The legal [`PlayerAction`]s the active investigator may take at the open
/// turn, in stable order (position = the future `OptionId`). Empty unless an
/// [`InvestigatorTurn`](Continuation::InvestigatorTurn) frame is on top — the
/// only point gameplay actions are taken (slice 2a-ii, #393).
///
/// Covers the full open-turn surface: EndTurn, Resource, Draw, Investigate, Move
/// (basic); Fight, Evade, Engage (combat/engage); PlayCard, ActivateAbility
/// (cards, registry-gated); AdvanceAct. Read-only and side-effect-free; each
/// action is included iff the same legality predicate the handler uses accepts
/// it, so the enumeration matches handler-acceptance by construction (routing
/// typed dispatch through it is 2b).
```

(Replace the existing doc paragraph; keep `#[must_use]`.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p cards --test enumerate_actions`
Expected: PASS (sweep + the Task-1 tests).

- [ ] **Step 5: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "test: whole-enumeration sweep closing slice 2a-ii (#393)

A maximal open-turn board (hand card, in-play asset, engaged + co-located
enemies, connected location, advanceable act) asserts every action category is
enumerated AND every enumerated action applies without Rejected — the closing
guarantee for the legal-action enumerator. legal_actions doc lists the full
coverage.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## After the tasks

- **PR** against `main`; design-decisions paragraph: `check_advance_act` extraction (matching the check-fn pattern, preserving rejection reasons); the sweep as the slice's closing characterization. Refs #393.
- **Phase/spec doc** (final commit once CI green): mark **2a-ii complete** in spec §E sequencing (all four sub-slices shipped); the enumerator is done, next is slice 3 (`AttackLoop`).

## Self-review notes

- **Spec coverage:** §E enumerator — AdvanceAct (Task 1, by delegation to the extracted `check_advance_act`) completes the open-turn action set; Task 2 is the closing sweep. Routing still deferred (2b). ✅
- **Placeholder scan:** none.
- **Type consistency:** `check_advance_act(&GameState, InvestigatorId) -> Result<u8, Cow<'static, str>>`; `Act { code: CardCode, clue_threshold: u8, resolution: Option<Resolution>, round_end_advance: Option<RoundEndAdvance> }`; `AdvanceAct { investigator }`. Match `act_agenda.rs` / `state`.
- **Behaviour-preservation:** `advance_act_action`'s existing tests (act_agenda.rs `#[cfg(test)]`, incl. `advance_act_rejects_when_clues_insufficient`) must stay green — they pin the exact rejection reasons `check_advance_act` now returns.
- **Implementer caveats:** confirm `clue_contributors` is in scope for `check_advance_act` (same module — yes); confirm `Cow` import in act_agenda.rs; the Task 2 sweep may pass immediately after Task 1 (it characterizes the complete enumerator) — that's expected, keep it as the closing guarantee.
