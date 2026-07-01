# Interactivity S0 — `OptionTarget` anchor on `ChoiceOption` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enrich each wire `ChoiceOption` with a structured `OptionTarget` anchor and have the open-turn action menu carry real anchors, so a host can later render options on the board entity they act on.

**Architecture:** Add an `OptionTarget` enum + a `target` field to `ChoiceOption` (with `new`/`global` constructors to keep call sites terse). Every existing option-builder emits `OptionTarget::Global` (unchanged behavior); only `turn_menu` derives real anchors, via a new `TurnAction::target(&state)` mirroring `TurnAction::label`. No web behavior change — the flat action bar still reads `label`.

**Tech Stack:** Rust workspace (`game-core`, `protocol`, `web`); serde for the wire; leptos in `web` (untouched here beyond one destructure fix).

## Global Constraints

- Issue: **#535** (interactivity S0). Umbrella: **#206**. Design spec: `docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md` (Section 1).
- The new `ChoiceOption.target` field is **required** on the wire (no `#[serde(default)]`) — a stale payload errors rather than silently degrading (the #453 precedent).
- `label` stays the **full, unambiguous** engine-authored string — do **not** shorten it engine-side.
- Match CI's strict flags before pushing (all seven jobs, warnings-as-errors) — see the Verification section. Commit scope prefix: `engine:`.
- Branch: `engine/interactivity-optiontarget` (one branch per issue). Commit only; do not merge. The design spec + this plan are committed on this branch too (the "doc in the same PR" pattern).

---

### Task 1: `OptionTarget` type, `ChoiceOption.target` field + constructors, migrate all sites to `Global`

**Files:**
- Modify: `crates/game-core/src/engine/outcome.rs` (add enum, field, constructors, imports; update two in-module test literals; add field round-trip tests)
- Modify: `crates/game-core/src/engine/mod.rs:36` (re-export `OptionTarget`)
- Modify: `crates/game-core/src/lib.rs:51` (root re-export `OptionTarget`)
- Modify: `crates/game-core/src/engine/dispatch/hunters.rs:386-396` (`candidate_options`)
- Modify: `crates/game-core/src/engine/dispatch/choice.rs:43-50` (`awaiting_choice`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:598-618` and `:1804-1811`
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs:538-543`
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:133-141`
- Modify: `crates/game-core/src/test_support/fixtures.rs:148-159` and `:183-189`
- Modify: `crates/web/src/input.rs:104` (destructure fix — compile break in the wasm build)

**Interfaces:**
- Produces:
  - `game_core::OptionTarget` (re-exported at crate root and `game_core::engine::OptionTarget`): `#[non_exhaustive]` enum — `Global`, `Location(LocationId)`, `Enemy(EnemyId)`, `HandCard { investigator: InvestigatorId, hand_index: u8 }`, `CardInstance(CardInstanceId)`, `Act`. Derives `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`.
  - `ChoiceOption { id: OptionId, label: String, target: OptionTarget }`.
  - `ChoiceOption::new(id: OptionId, label: impl Into<String>, target: OptionTarget) -> ChoiceOption`.
  - `ChoiceOption::global(id: OptionId, label: impl Into<String>) -> ChoiceOption` (target = `Global`).

- [ ] **Step 1: Write the failing tests** (append inside the existing `mod tests` in `crates/game-core/src/engine/outcome.rs`, before its closing `}`)

```rust
    #[test]
    fn global_constructor_sets_global_target() {
        let opt = ChoiceOption::global(OptionId(3), "End turn");
        assert_eq!(opt.id, OptionId(3));
        assert_eq!(opt.label, "End turn");
        assert_eq!(opt.target, OptionTarget::Global);
    }

    #[test]
    fn awaiting_input_round_trips_option_target() {
        use crate::state::EnemyId;
        let outcome = EngineOutcome::AwaitingInput {
            request: InputRequest::pick_single(
                "Choose an action",
                vec![
                    ChoiceOption::global(OptionId(0), "End turn"),
                    ChoiceOption::new(OptionId(1), "Fight Ghoul", OptionTarget::Enemy(EnemyId(7))),
                ],
            ),
            resume_token: ResumeToken(0),
        };
        let json = serde_json::to_string(&outcome).expect("serialize");
        let back: EngineOutcome = serde_json::from_str(&json).expect("deserialize");
        let EngineOutcome::AwaitingInput { request, .. } = back else {
            panic!("expected AwaitingInput, got {back:?}");
        };
        assert_eq!(request.options[0].target, OptionTarget::Global);
        assert_eq!(request.options[1].target, OptionTarget::Enemy(EnemyId(7)));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core --lib engine::outcome 2>&1 | tail -20`
Expected: compile error — `OptionTarget` not found / `ChoiceOption` has no `global`/`target`.

- [ ] **Step 3: Add imports + the `OptionTarget` enum + the field + constructors** in `crates/game-core/src/engine/outcome.rs`

Add to the imports near the top (after `use serde::{Deserialize, Serialize};`):

```rust
use crate::state::{CardInstanceId, EnemyId, InvestigatorId, LocationId};
```

Add the enum immediately above `pub struct ChoiceOption`:

```rust
/// The board surface an offered [`ChoiceOption`] acts on, letting a host render
/// the option on the entity it targets rather than in a flat list. `Global`
/// means no board anchor (e.g. End turn, a Confirm). Anchors are derived from
/// the engine's own action / candidate targets, so a host never re-computes
/// legality (#535, #206).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OptionTarget {
    /// No board anchor — a global / contextual control.
    Global,
    /// A location on the map.
    Location(LocationId),
    /// An enemy.
    Enemy(EnemyId),
    /// A card in an investigator's hand, by zero-based hand index.
    HandCard {
        /// The hand's owner.
        investigator: InvestigatorId,
        /// Zero-based position in that investigator's hand.
        hand_index: u8,
    },
    /// An in-play / threat-area / investigator card instance.
    CardInstance(CardInstanceId),
    /// The current act.
    Act,
}
```

Change the struct to add `target` and give it doc:

```rust
/// One selectable option in a structured choice prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    /// The id the host echoes back via
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    pub id: OptionId,
    /// Human-readable label for the host to render (full and unambiguous,
    /// e.g. `"Fight Ghoul"`; a host may shorten it for display).
    pub label: String,
    /// The board surface this option acts on (`Global` if none).
    pub target: OptionTarget,
}

impl ChoiceOption {
    /// An option anchored to `target`.
    #[must_use]
    pub fn new(id: OptionId, label: impl Into<String>, target: OptionTarget) -> Self {
        Self {
            id,
            label: label.into(),
            target,
        }
    }

    /// An option with no board anchor ([`OptionTarget::Global`]).
    #[must_use]
    pub fn global(id: OptionId, label: impl Into<String>) -> Self {
        Self::new(id, label, OptionTarget::Global)
    }
}
```

Update the two existing in-module test literals so the module compiles:
- In `pick_single_sets_kind_and_not_skippable`, replace `vec![ChoiceOption { id: OptionId(0), label: "A".into() }]` with `vec![ChoiceOption::global(OptionId(0), "A")]`.
- In `input_request_round_trips_with_kind_and_skippable`, replace the two `ChoiceOption { id: OptionId(0), label: "Take 2 horror".into() }` / `ChoiceOption { id: OptionId(1), label: "Each discards 1".into() }` literals with `ChoiceOption::global(OptionId(0), "Take 2 horror")` and `ChoiceOption::global(OptionId(1), "Each discards 1")`.

- [ ] **Step 4: Re-export `OptionTarget`**

In `crates/game-core/src/engine/mod.rs:36`, add `OptionTarget` to the `pub use outcome::{...}` list:

```rust
pub use outcome::{
    ChoiceOption, EngineOutcome, InputKind, InputRequest, OptionId, OptionTarget, ResumeToken,
};
```

In `crates/game-core/src/lib.rs:51`, add `OptionTarget` next to `ChoiceOption` in the root re-export (keep the surrounding names as-is):

```rust
    suspend_for_native_choice, take_damage, ApplyResult, ChoiceOption, ChoiceResolution, Cx,
    OptionTarget,
```
(Place `OptionTarget` in the existing `pub use engine::{…}` grouping — adjust the exact line to keep alphabetical-ish order consistent with the file; the requirement is that `game_core::OptionTarget` resolves.)

- [ ] **Step 5: Run the two new tests to verify they pass**

Run: `cargo test -p game-core --lib engine::outcome 2>&1 | tail -20`
Expected: PASS (`global_constructor_sets_global_target`, `awaiting_input_round_trips_option_target`, and the pre-existing outcome tests).

- [ ] **Step 6: Migrate every other `ChoiceOption` construction site to `::global`**

`crates/game-core/src/engine/dispatch/hunters.rs` — in `candidate_options`, replace the `.map(...)` body:

```rust
        .map(|(i, c)| {
            ChoiceOption::global(
                OptionId(u32::try_from(i).expect("candidate count fits u32")),
                format!("{c:?}"),
            )
        })
```

`crates/game-core/src/engine/dispatch/choice.rs` — in `awaiting_choice`:

```rust
        .map(|(i, label)| {
            ChoiceOption::global(
                OptionId(u32::try_from(i).expect("offered option count fits in u32")),
                label,
            )
        })
```

`crates/game-core/src/engine/dispatch/reaction_windows.rs` — in `build_resolution_options`, replace the trailing `ChoiceOption { id: …, label }` with:

```rust
            ChoiceOption::global(
                OptionId(u32::try_from(i).expect("option count fits in u32")),
                label,
            )
```

and in the Fast-window builder near line 1807, replace the `.map(...)` body:

```rust
        .map(|(i, a)| {
            ChoiceOption::global(
                OptionId(u32::try_from(i).unwrap_or(u32::MAX)),
                a.label(cx.state),
            )
        })
```

`crates/game-core/src/engine/dispatch/forced_triggers.rs` — replace the `vec![ChoiceOption { id: OptionId(0), label: "Resolve".into() }]` with:

```rust
            vec![ChoiceOption::global(OptionId(0), "Resolve")],
```

`crates/game-core/src/engine/dispatch/skill_test.rs` — replace the two literals:

```rust
                vec![
                    ChoiceOption::global(OptionId(0), format!("Use {use_skill:?}")),
                    ChoiceOption::global(OptionId(1), format!("Keep {skill:?}")),
                ],
```

`crates/game-core/src/test_support/fixtures.rs` — in `awaiting_pick_single_input`:

```rust
            vec![
                ChoiceOption::global(OptionId(0), "End turn"),
                ChoiceOption::global(OptionId(1), "Investigate"),
            ],
```

and in `awaiting_skippable_pick_single_input`:

```rust
            vec![ChoiceOption::global(OptionId(0), "Resolve")],
```

`crates/web/src/input.rs:104` — the destructure gains `..` (S0 does not consume `target` in web yet):

```rust
                            let ChoiceOption { id, label, .. } = opt;
```

- [ ] **Step 7: Run the full game-core + protocol suites to verify the migration compiles and passes**

Run: `cargo test -p game-core -p protocol 2>&1 | tail -20`
Expected: PASS, no compile errors.

- [ ] **Step 8: Verify the wasm build compiles (the `web/input.rs` destructure fix)**

Run: `cargo build -p web --target wasm32-unknown-unknown 2>&1 | tail -20`
Expected: builds clean.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/outcome.rs crates/game-core/src/engine/mod.rs \
        crates/game-core/src/lib.rs crates/game-core/src/engine/dispatch/hunters.rs \
        crates/game-core/src/engine/dispatch/choice.rs \
        crates/game-core/src/engine/dispatch/reaction_windows.rs \
        crates/game-core/src/engine/dispatch/forced_triggers.rs \
        crates/game-core/src/engine/dispatch/skill_test.rs \
        crates/game-core/src/test_support/fixtures.rs crates/web/src/input.rs
git commit -m "engine: add OptionTarget anchor to ChoiceOption (Global everywhere)"
```

---

### Task 2: `TurnAction::target` + wire real anchors into the open-turn menu

**Files:**
- Modify: `crates/game-core/src/engine/enumerate.rs` (add `TurnAction::target`; add a mapping test)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs:109-119` (`turn_menu` uses `target`; make `turn_menu` `pub(crate)`; add a propagation test)

**Interfaces:**
- Consumes: `game_core::OptionTarget`, `ChoiceOption::new` (Task 1).
- Produces: `TurnAction::target(&self, state: &GameState) -> OptionTarget`. `turn_menu` becomes `pub(crate)`.

- [ ] **Step 1: Write the failing mapping test** (append inside the existing `mod tests` in `crates/game-core/src/engine/enumerate.rs`)

```rust
    #[test]
    fn target_maps_each_variant() {
        use crate::engine::OptionTarget;
        use crate::state::{CardInstanceId, EnemyId, LocationId};

        // A state where investigator 1 stands on a location, so Investigate's
        // implicit anchor resolves to that location.
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        let inv = InvestigatorId(1);

        assert_eq!(TurnAction::EndTurn.target(&state), OptionTarget::Global);
        assert_eq!(
            TurnAction::Resource { investigator: inv }.target(&state),
            OptionTarget::Global
        );
        assert_eq!(
            TurnAction::Draw { investigator: inv }.target(&state),
            OptionTarget::Global
        );
        assert_eq!(
            TurnAction::Move { investigator: inv, destination: LocationId(11) }.target(&state),
            OptionTarget::Location(LocationId(11))
        );
        assert_eq!(
            TurnAction::Investigate { investigator: inv }.target(&state),
            OptionTarget::Location(loc_id)
        );
        assert_eq!(
            TurnAction::Fight { investigator: inv, enemy: EnemyId(7) }.target(&state),
            OptionTarget::Enemy(EnemyId(7))
        );
        assert_eq!(
            TurnAction::Evade { investigator: inv, enemy: EnemyId(7) }.target(&state),
            OptionTarget::Enemy(EnemyId(7))
        );
        assert_eq!(
            TurnAction::Engage { investigator: inv, enemy: EnemyId(7) }.target(&state),
            OptionTarget::Enemy(EnemyId(7))
        );
        assert_eq!(
            TurnAction::PlayCard { investigator: inv, hand_index: 2 }.target(&state),
            OptionTarget::HandCard { investigator: inv, hand_index: 2 }
        );
        assert_eq!(
            TurnAction::ActivateAbility {
                investigator: inv,
                instance_id: CardInstanceId(5),
                ability_index: 0,
            }
            .target(&state),
            OptionTarget::CardInstance(CardInstanceId(5))
        );
        assert_eq!(
            TurnAction::AdvanceAct { investigator: inv }.target(&state),
            OptionTarget::Act
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core --lib engine::enumerate::tests::target_maps_each_variant 2>&1 | tail -15`
Expected: compile error — no method `target` on `TurnAction`.

- [ ] **Step 3: Implement `TurnAction::target`** (add to `impl TurnAction` in `crates/game-core/src/engine/enumerate.rs`, right after the `label` method)

```rust
    /// The board surface this action anchors to, for host rendering (#535).
    /// Mirrors [`label`](Self::label): it takes `state` because some actions'
    /// anchors are implicit — Investigate acts at the investigator's current
    /// location, which is not a field on the variant.
    #[must_use]
    pub fn target(&self, state: &GameState) -> crate::engine::OptionTarget {
        use crate::engine::OptionTarget;
        match self {
            TurnAction::EndTurn
            | TurnAction::Resource { .. }
            | TurnAction::Draw { .. } => OptionTarget::Global,
            TurnAction::Move { destination, .. } => OptionTarget::Location(*destination),
            TurnAction::Investigate { investigator } => state
                .investigators
                .get(investigator)
                .and_then(|inv| inv.current_location)
                .map_or(OptionTarget::Global, OptionTarget::Location),
            TurnAction::Fight { enemy, .. }
            | TurnAction::Evade { enemy, .. }
            | TurnAction::Engage { enemy, .. } => OptionTarget::Enemy(*enemy),
            TurnAction::PlayCard {
                investigator,
                hand_index,
            } => OptionTarget::HandCard {
                investigator: *investigator,
                hand_index: *hand_index,
            },
            TurnAction::ActivateAbility { instance_id, .. } => {
                OptionTarget::CardInstance(*instance_id)
            }
            TurnAction::AdvanceAct { .. } => OptionTarget::Act,
        }
    }
```

- [ ] **Step 4: Run the mapping test to verify it passes**

Run: `cargo test -p game-core --lib engine::enumerate::tests::target_maps_each_variant 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: Write the failing propagation test** (append inside `mod tests` in `crates/game-core/src/engine/dispatch/mod.rs`; if the file has no `#[cfg(test)] mod tests`, add one at the end)

```rust
#[cfg(test)]
mod turn_menu_tests {
    use super::turn_menu;
    use crate::engine::enumerate::legal_actions;
    use crate::engine::OptionTarget;
    use crate::state::{Continuation, InvestigationResume, InvestigatorId, Phase};
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};

    #[test]
    fn turn_menu_carries_action_targets() {
        // An open-turn state with a co-located, engaged enemy so the menu holds
        // at least one Enemy-anchored option (Fight/Evade), proving turn_menu
        // propagates each action's target — not just Global.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_chaos_bag(crate::state::ChaosBag::new([crate::state::ChaosToken::Numeric(0)]))
            .with_phase_anchor(Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build();
        let loc = test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state.locations.get_mut(&loc_id).unwrap().revealed = true;
        {
            let inv = state.investigators.get_mut(&InvestigatorId(1)).unwrap();
            inv.current_location = Some(loc_id);
            inv.actions_remaining = 3;
        }
        let mut e = test_enemy(7, "Ghoul");
        e.engaged_with = Some(InvestigatorId(1));
        e.current_location = Some(loc_id);
        state.enemies.insert(e.id, e);

        let actions = legal_actions(&state);
        let menu = turn_menu(&state);
        assert_eq!(menu.options.len(), actions.len());
        for (i, action) in actions.iter().enumerate() {
            assert_eq!(menu.options[i].target, action.target(&state));
        }
        assert!(
            menu.options.iter().any(|o| matches!(o.target, OptionTarget::Enemy(_))),
            "expected at least one Enemy-anchored option, got {:?}",
            menu.options.iter().map(|o| o.target).collect::<Vec<_>>()
        );
    }
}
```

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test -p game-core --lib turn_menu_tests 2>&1 | tail -15`
Expected: compile error — `turn_menu` is private (not importable via `super::turn_menu`).

- [ ] **Step 7: Make `turn_menu` `pub(crate)` and wire in `target`** in `crates/game-core/src/engine/dispatch/mod.rs`

Change the signature `fn turn_menu(` → `pub(crate) fn turn_menu(`, and replace the `.map(...)` body:

```rust
    let options = crate::engine::enumerate::legal_actions(state)
        .iter()
        .enumerate()
        .map(|(i, a)| {
            crate::engine::ChoiceOption::new(
                crate::engine::OptionId(u32::try_from(i).unwrap_or(u32::MAX)),
                a.label(state),
                a.target(state),
            )
        })
        .collect();
```

- [ ] **Step 8: Run the propagation test to verify it passes**

Run: `cargo test -p game-core --lib turn_menu_tests 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/enumerate.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: open-turn menu options carry their board-entity anchors"
```

---

### Task 3: protocol envelope round-trip for the new field

**Files:**
- Modify: `crates/protocol/src/lib.rs` (add one test in the existing `mod tests`)

**Interfaces:**
- Consumes: `game_core::OptionTarget` (Task 1), `game_core::test_support::fixtures::awaiting_pick_single_input`.

- [ ] **Step 1: Write the failing test** (append inside `mod tests` in `crates/protocol/src/lib.rs`)

```rust
    #[test]
    fn applied_round_trips_awaiting_input_option_target() {
        use game_core::OptionTarget;

        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        // The fixture's options are Global-anchored; this proves the new field
        // survives the ServerMessage envelope (game-core covers a non-Global
        // value directly). See the interactivity S0 plan.
        let outcome =
            game_core::test_support::fixtures::awaiting_pick_single_input("Choose an action");
        let msg = ServerMessage::Applied {
            state: Box::new(state),
            events: Vec::new(),
            outcome,
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ServerMessage = serde_json::from_str(&json).expect("deserialize");

        let ServerMessage::Applied { outcome, .. } = back else {
            panic!("expected Applied, got {back:?}");
        };
        let game_core::EngineOutcome::AwaitingInput { request, .. } = outcome else {
            panic!("expected AwaitingInput outcome");
        };
        assert!(
            request.options.iter().all(|o| o.target == OptionTarget::Global),
            "option targets survive the envelope"
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p protocol applied_round_trips_awaiting_input_option_target 2>&1 | tail -15`
Expected: compile error — `game_core::OptionTarget` unresolved *until* Task 1's re-export is in place; if Task 1 landed, the test compiles and PASSES immediately (it's a characterization test). If it passes on first run, that is acceptable — the value is regression protection for the envelope.

- [ ] **Step 3: (If it failed to compile for another reason) fix imports**

Ensure the test refers to `game_core::OptionTarget` and `game_core::EngineOutcome` by full path (as written). No production code changes are expected in this task.

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p protocol applied_round_trips_awaiting_input_option_target 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol/src/lib.rs
git commit -m "protocol: regression-test OptionTarget survives the ServerMessage envelope"
```

---

## Verification (full CI gauntlet, before pushing)

Run all seven jobs with the strict flags (from `CLAUDE.md`). All must be green:

```sh
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
                            wasm-pack test --headless --firefox crates/web
                            cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Watch for: an unused-import warning if a migrated module no longer needs `OptionId` in scope for a literal (it still does — `::global` takes an `OptionId`); the `web` clippy job is the one that exercises `input.rs`'s destructure fix.

## PR flow (after the gauntlet is green)

1. Also stage the design spec + this plan on the branch:
   `git add docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md docs/superpowers/plans/2026-07-01-interactivity-s0-optiontarget.md` and commit (`docs: interactivity pass design + S0 plan`).
2. Push `engine/interactivity-optiontarget`; open the PR with `gh pr create` using the repo template. Body: the Section-1 summary + `Closes #535.`
3. Watch CI: `gh pr checks <PR#> --watch`.
4. **Do not update the phase-7 doc yet** and **do not merge** — stop for review/approval (per the repo workflow).

## Self-review notes

- **Spec coverage (Section 1):** `OptionTarget` enum ✅ (Task 1); `ChoiceOption.target` required field ✅ (Task 1); `turn_menu` derives anchors from `TurnAction` ✅ (Task 2); other builders emit `Global` ✅ (Task 1); label stays full ✅ (constructors preserve the passed label verbatim); protocol recompile + wire test ✅ (Task 3); required-field / #453 precedent ✅ (no `serde(default)`).
- **Testing (spec):** per-`TurnAction`→`OptionTarget` assertion ✅ (Task 2 Step 1); serde round-trip ✅ (Task 1 Step 1 in game-core + Task 3 envelope). The spec also mentions extending `every_enumerated_action_is_accepted_by_its_handler` with an anchor assertion — `turn_menu_carries_action_targets` (Task 2) covers the same ground more directly (menu option `target` == `action.target`), so that extension is intentionally omitted as redundant.
- **Type consistency:** `OptionTarget::HandCard { investigator, hand_index }`, `CardInstance(CardInstanceId)`, `Enemy(EnemyId)`, `Location(LocationId)`, `Act`, `Global` used identically in the enum, `TurnAction::target`, and every test. `ChoiceOption::new`/`::global` signatures match all call sites (all pass `OptionId` + a `String`/`&str`).
