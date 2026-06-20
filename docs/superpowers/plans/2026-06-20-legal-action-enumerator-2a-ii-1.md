# Legal-Action Enumerator — scaffold + basic actions (slice 2a-ii-1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the `legal_actions(state)` enumerator and cover the **basic actions** (EndTurn, Resource, Draw, Investigate, Move), built on shared "is-this-legal?" predicates so the enumeration matches handler-acceptance by construction.

**Architecture:** New read-only engine module `engine/enumerate.rs` exposing `pub fn legal_actions(state: &GameState) -> Vec<PlayerAction>` — the legal `PlayerAction`s for the active investigator at the open turn, in stable order (position = the future `OptionId`). It returns empty unless an `InvestigatorTurn` frame is on top. It reuses the existing pure validators (`validate_basic_action`) and a new pure `action_cost` extracted from `charge_action`. **Nothing in production dispatches through it yet** (typed handlers keep validating; routing is slice 2b) — its consumer this slice is the test suite, including a **cross-check** that every enumerated action applies without `Rejected`.

**Tech Stack:** Rust, `game-core` kernel crate. No new deps.

## Global Constraints

- **Build + expose, defer routing (decision, this slice).** The enumerator is `pub` and read-only; no handler is rewired to gate on it. "Accepted iff in the offered set" holds *by construction* — the enumerator calls the same legality predicates the handlers use. (Spec §E, 2a sub-checkpoint.)
- **Behaviour-preserving.** No handler's accept/reject behaviour changes. The `action_cost` extraction is a pure refactor of `charge_action`. Full host gauntlet green at every task: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- **Match current handler behaviour, not the rules, where they diverge.** The enumerator mirrors what each handler *currently* accepts (behaviour-preserving by construction). Known divergence in a *later* sub-slice: Fight requires engagement (handler) vs. any co-located enemy (rules) — tracked in **#401**; not in this slice (no Fight here).
- **Design of record:** umbrella spec `docs/superpowers/specs/2026-06-20-unified-control-flow-model-design.md` §E ("Enumerated-action input"). Slice 2a-i shipped the `InvestigatorTurn` frame (#400); this builds on it.
- **Commit-message footer** (every commit), verbatim:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
  ```
- **Branch:** `engine/legal-action-enumerator`. One commit per task.

## Reference: `PlayerAction` constructors (from `crates/game-core/src/action.rs`)

`EndTurn`, `Resource { investigator }`, `Draw { investigator }`, `Investigate { investigator }`, `Move { investigator, destination }`. (`investigator: InvestigatorId`, `destination: LocationId`.)

## Reference: current basic-action legality (what the enumerator must mirror)

- **EndTurn** (`phases.rs` `end_turn`): legal whenever `active_investigator` is `Some`. At the open turn that is always true → EndTurn is always offered.
- **Resource / Draw** (`actions.rs` `resource_action`, `cards.rs` `draw`): `validate_basic_action` only (Investigation phase + active + `Status::Active` + `actions_remaining >= 1`). Draw does **not** gate on deck emptiness.
- **Investigate** (`actions.rs` `investigate`): `validate_basic_action` + `inv.current_location` is `Some(loc)` + `state.locations[loc].revealed`.
- **Move** (`actions.rs` `move_action`): phase Investigation + active + `Status::Active` + `inv.current_location` is `Some(from)` + for a destination `d`: `d != from` + `d` in `state.locations` + `from.connections.contains(d)` + affordable (`action_cost(...) <= actions_remaining`). Move uses its own prefix (not `validate_basic_action`) because the action-point check is folded into `charge_action`.

---

### Task 1: Module scaffold + `legal_actions` skeleton + EndTurn

Establish `engine/enumerate.rs`, the public signature, the open-turn gate (empty unless an `InvestigatorTurn` frame is on top), EndTurn (the always-available singleton), and the **cross-check test harness** that later tasks extend.

**Files:**
- Create: `crates/game-core/src/engine/enumerate.rs`
- Modify: `crates/game-core/src/engine/mod.rs` — add `pub mod enumerate;` and re-export.
- Modify: `crates/game-core/src/lib.rs` — re-export `pub use engine::enumerate::legal_actions;` (match the crate's existing top-level re-export style; if the crate re-exports engine items elsewhere, follow that).

**Interfaces:**
- Produces: `pub fn legal_actions(state: &GameState) -> Vec<PlayerAction>`.

- [ ] **Step 1: Write the failing test**

Create `crates/game-core/src/engine/enumerate.rs` with only a test module:

```rust
//! The legal-action enumerator (slice 2a-ii, #393): the legal `PlayerAction`s
//! for the active investigator at the open turn. Read-only; nothing dispatches
//! through it yet (routing is 2b) — it shares the handlers' legality predicates
//! so the enumeration matches handler-acceptance by construction.

#[cfg(test)]
mod tests {
    use crate::engine::enumerate::legal_actions;
    use crate::state::{Continuation, InvestigationResume, InvestigatorId, Phase};
    use crate::test_support::{test_investigator, GameStateBuilder};
    use crate::action::PlayerAction;

    /// Build a single-investigator open-turn state (InvestigatorTurn frame on
    /// top of the InvestigationPhase anchor), the shape `legal_actions` enumerates.
    fn open_turn_state() -> crate::state::GameState {
        GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_phase_anchor(Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build()
    }

    #[test]
    fn end_turn_is_always_offered_at_the_open_turn() {
        let state = open_turn_state();
        assert!(legal_actions(&state).contains(&PlayerAction::EndTurn));
    }

    #[test]
    fn no_actions_when_not_the_open_turn() {
        // No InvestigatorTurn frame on top (empty stack) → nothing to offer.
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .build();
        assert!(legal_actions(&state).is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core enumerate::tests 2>&1 | head`
Expected: FAIL — `legal_actions` not found / module not declared.

- [ ] **Step 3: Implement the skeleton + EndTurn**

Prepend to `crates/game-core/src/engine/enumerate.rs` (above the test module):

```rust
use crate::action::PlayerAction;
use crate::state::{Continuation, GameState, InvestigatorId};

/// The legal [`PlayerAction`]s the active investigator may take at the open
/// turn, in stable order (position = the future `OptionId`). Empty unless an
/// [`InvestigatorTurn`](Continuation::InvestigatorTurn) frame is on top — the
/// only point gameplay actions are taken (slice 2a-ii-1, #393).
///
/// Read-only and side-effect-free. Each action is included iff the same
/// legality predicate the handler uses accepts it, so the enumeration matches
/// handler-acceptance by construction (routing typed dispatch through it is 2b).
#[must_use]
pub fn legal_actions(state: &GameState) -> Vec<PlayerAction> {
    let Some(Continuation::InvestigatorTurn { investigator, .. }) = state.continuations.last()
    else {
        return Vec::new();
    };
    let investigator = *investigator;
    let mut actions = Vec::new();
    push_basic_actions(state, investigator, &mut actions);
    actions
}

/// Append the basic actions legal for `investigator`. EndTurn is always legal at
/// the open turn (the handler only needs an active investigator, guaranteed
/// here). Later tasks add Resource/Draw/Investigate/Move.
fn push_basic_actions(_state: &GameState, _investigator: InvestigatorId, out: &mut Vec<PlayerAction>) {
    out.push(PlayerAction::EndTurn);
}
```

In `crates/game-core/src/engine/mod.rs`, add the module declaration (alongside the other `mod`/`pub mod` lines):

```rust
pub mod enumerate;
```

In `crates/game-core/src/lib.rs`, re-export (place with the other `pub use` re-exports of engine items):

```rust
pub use engine::enumerate::legal_actions;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core enumerate::tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "engine: legal-action enumerator scaffold + EndTurn (slice 2a-ii-1 of #393)

New read-only engine::enumerate::legal_actions(state) returns the active
investigator's legal PlayerActions at the open turn (empty unless an
InvestigatorTurn frame is on top); EndTurn is the first, always-offered action.
Nothing dispatches through it yet (routing is 2b).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

### Task 2: Resource, Draw, Investigate + the cross-check test

Add the three `validate_basic_action`-gated actions and the **cross-check**: every enumerated action applies without `Rejected`. Investigate adds the `current_location` + `revealed` gate.

**Files:**
- Modify: `crates/game-core/src/engine/enumerate.rs`

**Interfaces:**
- Consumes: `validate_basic_action(state, action_name, investigator) -> Result<&Investigator, EngineOutcome>` (`crates/game-core/src/engine/dispatch/actions.rs`, `pub(super)` — reachable from `engine::enumerate` as `crate::engine::dispatch::actions::validate_basic_action`; if not `pub(crate)`-visible, widen its visibility to `pub(crate)` in this task — a pure read-only validator, safe to share).

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `enumerate.rs`:

```rust
    #[test]
    fn basic_actions_offered_with_a_revealed_location_and_an_action() {
        let mut state = open_turn_state();
        // Place the investigator on a revealed location so Investigate is legal.
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state.locations.get_mut(&loc_id).unwrap().revealed = true;
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(loc_id);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().actions_remaining = 3;

        let actions = legal_actions(&state);
        assert!(actions.contains(&PlayerAction::Resource { investigator: InvestigatorId(1) }));
        assert!(actions.contains(&PlayerAction::Draw { investigator: InvestigatorId(1) }));
        assert!(actions.contains(&PlayerAction::Investigate { investigator: InvestigatorId(1) }));
    }

    #[test]
    fn no_action_points_offers_only_end_turn() {
        let mut state = open_turn_state();
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().actions_remaining = 0;
        // With 0 actions, only EndTurn (which needs no action point) is legal.
        assert_eq!(legal_actions(&state), vec![PlayerAction::EndTurn]);
    }

    #[test]
    fn investigate_absent_on_an_unrevealed_location() {
        let mut state = open_turn_state();
        let mut loc = crate::test_support::test_location(10, "Study");
        loc.revealed = false;
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(loc_id);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().actions_remaining = 3;
        assert!(!legal_actions(&state)
            .contains(&PlayerAction::Investigate { investigator: InvestigatorId(1) }));
    }

    #[test]
    fn every_enumerated_action_is_accepted_by_its_handler() {
        // The cross-check that makes "defer routing" safe: each enumerated
        // action applies without Rejected (Done or AwaitingInput both mean
        // "accepted"). Apply to a fresh clone per action.
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state.locations.get_mut(&loc_id).unwrap().revealed = true;
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(loc_id);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().actions_remaining = 3;

        for action in legal_actions(&state) {
            let result = crate::apply(state.clone(), crate::Action::Player(action.clone()));
            assert!(
                !matches!(result.outcome, crate::EngineOutcome::Rejected { .. }),
                "enumerated action {action:?} was rejected by its handler: {:?}",
                result.outcome,
            );
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p game-core enumerate::tests`
Expected: FAIL — Resource/Draw/Investigate not yet offered; `no_action_points_offers_only_end_turn` fails (EndTurn-only not yet enforced because the three aren't gated).

- [ ] **Step 3: Implement the three actions**

Replace `push_basic_actions` in `enumerate.rs`:

```rust
fn push_basic_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<PlayerAction>) {
    // EndTurn: always legal at the open turn (no action point required).
    out.push(PlayerAction::EndTurn);

    // Resource / Draw / Investigate share the basic-action prologue (phase +
    // active + Status::Active + actions_remaining >= 1). Investigate adds a
    // revealed-current-location gate.
    use crate::engine::dispatch::actions::validate_basic_action;
    let Ok(inv) = validate_basic_action(state, "enumerate", investigator) else {
        return;
    };
    out.push(PlayerAction::Resource { investigator });
    out.push(PlayerAction::Draw { investigator });
    if let Some(loc_id) = inv.current_location {
        if state.locations.get(&loc_id).is_some_and(|l| l.revealed) {
            out.push(PlayerAction::Investigate { investigator });
        }
    }
}
```

If the compiler reports `validate_basic_action` is not visible from `engine::enumerate`, change its declaration in `crates/game-core/src/engine/dispatch/actions.rs` from `pub(super) fn validate_basic_action` to `pub(crate) fn validate_basic_action` (pure read-only validator; widening is safe).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core enumerate::tests`
Expected: PASS (all enumerate tests, including the cross-check).

- [ ] **Step 5: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "engine: enumerate Resource/Draw/Investigate + handler cross-check (slice 2a-ii-1 of #393)

legal_actions now offers the validate_basic_action-gated basic actions
(Resource, Draw, Investigate with its revealed-location gate). Adds the
cross-check test: every enumerated action applies without Rejected — pinning
enumerator <=> handler-acceptance without routing dispatch through it.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

### Task 3: Move + the `action_cost` extraction

Add Move (one option per legal connected destination), extracting a pure `action_cost` out of `charge_action` so affordability is checkable without mutating.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` — extract `action_cost`; `charge_action` calls it.
- Modify: `crates/game-core/src/engine/enumerate.rs` — enumerate Move.

**Interfaces:**
- Produces: `pub(crate) fn action_cost(state: &GameState, investigator: InvestigatorId, action_class: crate::dsl::ActionClass) -> u8` — base 1 + the Frozen-in-Fear surcharge (reading `card_registry::current()`; falls back to 1 with no registry). Pure (no mutation).
- Consumes: `action_cost` (in `enumerate.rs`).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `enumerate.rs`:

```rust
    #[test]
    fn move_offers_one_option_per_connected_destination() {
        let mut state = open_turn_state();
        let mut a = crate::test_support::test_location(10, "A");
        let b = crate::test_support::test_location(11, "B");
        a.connections = vec![b.id];
        let (a_id, b_id) = (a.id, b.id);
        state.locations.insert(a_id, a);
        state.locations.insert(b_id, b);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(a_id);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().actions_remaining = 3;

        let actions = legal_actions(&state);
        assert!(actions.contains(&PlayerAction::Move {
            investigator: InvestigatorId(1),
            destination: b_id,
        }));
        // No self-move.
        assert!(!actions.contains(&PlayerAction::Move {
            investigator: InvestigatorId(1),
            destination: a_id,
        }));
    }

    #[test]
    fn move_absent_when_unaffordable() {
        let mut state = open_turn_state();
        let mut a = crate::test_support::test_location(10, "A");
        let b = crate::test_support::test_location(11, "B");
        a.connections = vec![b.id];
        let (a_id, b_id) = (a.id, b.id);
        state.locations.insert(a_id, a);
        state.locations.insert(b_id, b);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(a_id);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().actions_remaining = 0;
        assert!(!legal_actions(&state).iter().any(|a| matches!(a, PlayerAction::Move { .. })));
    }
```

(The existing `every_enumerated_action_is_accepted_by_its_handler` cross-check now also covers the offered Move, since the `open_turn_state` board has no connections — extend it: after Step 3, the cross-check board gains a connected destination so a Move is enumerated and applied. Add that in Step 3.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p game-core enumerate::tests`
Expected: FAIL — Move never offered.

- [ ] **Step 3: Extract `action_cost`, enumerate Move, extend the cross-check**

In `crates/game-core/src/engine/dispatch/actions.rs`, extract the cost computation. Replace the cost lines inside `charge_action` (the `let (extra, to_mark) = …; let cost = 1u8.saturating_add(extra);` region) so `charge_action` delegates, and add the pure function:

```rust
/// The action-point cost of `action_class` for `investigator`: base 1 plus any
/// Frozen-in-Fear `ExtraActionCost` surcharge (Rules Reference; #164). Pure —
/// reads `card_registry::current()` for the surcharge, falling back to 1 with no
/// registry installed (bare unit tests). The enumerator uses this for Move/Fight/
/// Evade affordability; `charge_action` uses it then spends.
pub(crate) fn action_cost(
    state: &GameState,
    investigator: InvestigatorId,
    action_class: crate::dsl::ActionClass,
) -> u8 {
    let extra = match crate::card_registry::current() {
        Some(reg) => {
            crate::engine::evaluator::pending_action_surcharge(state, reg, investigator, action_class).0
        }
        None => 0,
    };
    1u8.saturating_add(extra)
}
```

Then in `charge_action`, replace its inline `(extra, to_mark)` cost derivation so the **cost** comes from `action_cost` while `to_mark` (the surcharge sources to mark spent) still comes from `pending_action_surcharge` — i.e. keep the existing `pending_action_surcharge` call for `to_mark`, and compute `let cost = action_cost(cx.state, investigator, action_class);` instead of re-deriving from `extra`. (Behaviour identical: same registry, same number.) Confirm `charge_action` still marks `to_mark` spent on success.

In `crates/game-core/src/engine/enumerate.rs`, enumerate Move inside `push_basic_actions` (after the `validate_basic_action` block — but Move uses its *own* prefix, so compute independently; place it before the early `return` cannot apply, so structure as below). Replace `push_basic_actions` with:

```rust
fn push_basic_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<PlayerAction>) {
    // EndTurn: always legal at the open turn (no action point required).
    out.push(PlayerAction::EndTurn);

    use crate::engine::dispatch::actions::{action_cost, validate_basic_action};

    if let Ok(inv) = validate_basic_action(state, "enumerate", investigator) {
        out.push(PlayerAction::Resource { investigator });
        out.push(PlayerAction::Draw { investigator });
        if let Some(loc_id) = inv.current_location {
            if state.locations.get(&loc_id).is_some_and(|l| l.revealed) {
                out.push(PlayerAction::Investigate { investigator });
            }
        }
    }

    // Move uses its own prefix (the action-point check folds into the cost):
    // phase Investigation + active + Status::Active + a current location +
    // affordable, with one option per connected destination in state.
    let Some(inv) = state.investigators.get(&investigator) else {
        return;
    };
    if state.phase != crate::state::Phase::Investigation
        || state.active_investigator != Some(investigator)
        || inv.status != crate::state::Status::Active
    {
        return;
    }
    let Some(from) = inv.current_location else {
        return;
    };
    if action_cost(state, investigator, crate::dsl::ActionClass::Move) > inv.actions_remaining {
        return;
    }
    let Some(from_loc) = state.locations.get(&from) else {
        return;
    };
    for &dest in &from_loc.connections {
        if dest != from && state.locations.contains_key(&dest) {
            out.push(PlayerAction::Move { investigator, destination: dest });
        }
    }
}
```

Extend `every_enumerated_action_is_accepted_by_its_handler` so its board has a connected destination (so a Move is enumerated and cross-checked): build the location as `A` with `connections = vec![B.id]`, both revealed, investigator on `A` — mirroring `move_offers_one_option_per_connected_destination`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p game-core enumerate::tests`
Expected: PASS.

- [ ] **Step 5: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "engine: enumerate Move + extract pure action_cost (slice 2a-ii-1 of #393)

legal_actions offers one Move per connected, affordable destination. Extracts a
pure action_cost (base 1 + Frozen-in-Fear surcharge) out of charge_action so
affordability is checkable without mutating; charge_action delegates to it.
Behaviour-preserving. Cross-check extended to a board with a Move.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## After the tasks

- **PR:** open against `main` with the repo template; design-decisions paragraph: build+expose+defer-routing (the cross-check is what makes that safe), the `action_cost` extraction, and that the enumerator mirrors *current handler behaviour* (Fight engagement divergence → #401, no Fight here). Reference #393; not a `Closes` (sub-slice of a closed issue).
- **Phase/spec doc** (final commit, once CI green): annotate spec §E / Sequencing §2's 2a-ii line as "scaffold + basic actions shipped (PR #NN)", mirroring 2a-i.
- **Follow-on sub-slices (own plans + PRs when reached):**
  - **2a-ii-2 — combat/engage:** Fight (engaged enemies — match handler; #401 widens later), Evade (engaged enemies), Engage (co-located enemies where `engaged_with != Some(active)`, per Rules Reference p.11 — includes enemies engaged with *others*). Fold the fight/evade difficulty-sign + `validate_engaged_action` predicates into the shared set.
  - **2a-ii-3 — play/activate:** PlayCard (per hand index; consolidate `check_play_card` + the inline `play_is_prohibited`), ActivateAbility (per in-play ability; extract an activate-legality predicate).
  - **2a-ii-4 — AdvanceAct + sweep:** AdvanceAct (act-advanceable predicate); final whole-enumeration cross-check across a rich board.

## Self-review notes

- **Spec coverage:** §E "the enumerator emits the legal-action enumeration … source of truth is the existing per-action precondition checks, callable in is-legal mode" → Tasks 1–3 build it on `validate_basic_action` + the extracted `action_cost` (basic actions); later sub-slices extend. "typed PlayerAction still accepted" → unchanged (no routing). ✅
- **Placeholder scan:** none — every step has concrete code/commands.
- **Type consistency:** `legal_actions(&GameState) -> Vec<PlayerAction>`; `action_cost(&GameState, InvestigatorId, ActionClass) -> u8`; `validate_basic_action(&GameState, &str, InvestigatorId) -> Result<&Investigator, EngineOutcome>`. `PlayerAction` constructors match `action.rs`.
- **Behaviour-preservation gate:** full suite green every task; `action_cost` extraction asserted equivalent by the unchanged Move/Fight/Evade handler tests + the cross-check.
- **Verify-before-coding caveats for the implementer:** confirm `validate_basic_action`'s actual visibility (widen to `pub(crate)` if needed); confirm `pending_action_surcharge`'s return shape (`.0` is `extra: u8`) before wiring `action_cost`; confirm the `test_location` builder sets `revealed` (set it explicitly in tests regardless).
