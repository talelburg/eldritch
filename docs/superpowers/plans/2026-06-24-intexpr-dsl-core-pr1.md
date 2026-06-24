# IntExpr DSL core + #426 + #300 (PR 1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the card DSL with state-reading dynamic values (`IntExpr::Count` over a shared `Quantity` vocabulary, `Condition::Compare`), then fix Grasping Hands / Rotting Remains (#426) and Machete (#300).

**Architecture:** Add a `Quantity` enum read by one `eval_quantity` helper that backs both `IntExpr::Count` (value) and `Condition::Compare` (predicate). Widen the `Deal.amount` / `Fight.extra_damage` effect fields to `IntExpr` with `From`/`Into` builders so existing literal call-sites are untouched. Migrate the two failure-margin treacheries to a single `Count(SkillTestFailedBy)` deal and delete the now-dead `ForEachPointFailed`.

**Tech Stack:** Rust workspace. DSL types in `crates/card-dsl`; evaluator in `crates/game-core`; cards in `crates/cards`.

**Spec:** `docs/superpowers/specs/2026-06-24-intexpr-dynamic-value-cluster-design.md` (Sections 1 + 3). PR 2 (#118 + bridge, Section 2) is a separate plan.

## Global Constraints

- CI runs warnings-as-errors. Before any commit, the changed crates must pass: `RUSTFLAGS="-D warnings" cargo test --all --all-features` and `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check`.
- DSL crate (`card-dsl`) is pure data — no I/O, no engine logic. `Quantity`/`CmpOp`/`IntExpr`/`Condition` derive `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize` (match the existing `IntExpr` derives, `crates/card-dsl/src/dsl.rs:1101`).
- Never hand-edit `crates/cards/src/generated/`. The cards touched here are hand-written impls under `crates/cards/src/impls/`.
- Branch: `engine/intexpr-dynamic-values` (already created; the design commit is already on it).

## File Structure

- `crates/card-dsl/src/dsl.rs` — add `Quantity`, `CmpOp`; add `IntExpr::Count`; replace `Condition::LocationHasClues` with `Condition::Compare`; add `From<i8>/From<u8> for IntExpr`; widen `Effect::Deal.amount` + `Effect::Fight.extra_damage` to `IntExpr`; update `deal_damage`/`deal_horror`/`fight` builders to `impl Into<IntExpr>`; delete `Effect::ForEachPointFailed` + `for_each_point_failed`.
- `crates/game-core/src/engine/evaluator.rs` — add `eval_quantity`; thread `&EvalContext` into `eval_int_expr`/`eval_condition`; add the `Count`/`Compare` arms; eval `Deal.amount`/`Fight.extra_damage`; delete the `ForEachPointFailed` frame arms.
- `crates/game-core/src/state/game_state.rs` — delete `EffectFrame::ForEachPointFailed`.
- `crates/cards/src/impls/roland_38_special.rs` — migrate `LocationHasClues` → `Compare`.
- `crates/cards/src/impls/grasping_hands.rs`, `rotting_remains.rs` — `Count(SkillTestFailedBy)`.
- `crates/cards/src/impls/machete.rs` — conditional `extra_damage`.

---

### Task 1: `Quantity` + `CmpOp` enums and the `eval_quantity` helper

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (after the `Condition` enum, ~line 1095)
- Modify: `crates/game-core/src/engine/evaluator.rs` (thread `&EvalContext`; add `eval_quantity`)

**Interfaces:**
- Produces: `card_dsl::dsl::Quantity { CluesAtControllerLocation, EngagedEnemies, SkillTestFailedBy }`, `card_dsl::dsl::CmpOp { Eq, Ne, Lt, Le, Gt, Ge }`.
- Produces: `fn eval_quantity(state: &GameState, eval_ctx: &EvalContext, q: Quantity) -> i8` (private to `evaluator.rs`).
- Produces (changed): `fn eval_int_expr(state: &GameState, eval_ctx: &EvalContext, expr: &IntExpr) -> Result<i8, String>` and `fn eval_condition(state: &GameState, eval_ctx: &EvalContext, condition: &Condition) -> Result<bool, String>` — `controller` now read from `eval_ctx.controller`.

- [ ] **Step 1: Add the enums to `dsl.rs`.** Immediately after the closing `}` of `enum Condition` (`dsl.rs:1095`):

```rust
/// A non-negative count read off game state, usable as a value
/// ([`IntExpr::Count`]) or compared in a predicate ([`Condition::Compare`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quantity {
    /// Clues on the controller's current location.
    CluesAtControllerLocation,
    /// Enemies engaged with the controller.
    EngagedEnemies,
    /// Failure margin of the resolving skill test (0 outside one).
    SkillTestFailedBy,
}

/// Comparison operator for [`Condition::Compare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}
```

- [ ] **Step 2: Build (types only).**

Run: `cargo build -p card-dsl`
Expected: PASS (new enums unused yet — they're `pub`, no dead-code warning).

- [ ] **Step 3: Write the failing `eval_quantity` test.** In `crates/game-core/src/engine/evaluator.rs` `#[cfg(test)]` module, add:

```rust
#[test]
fn eval_quantity_reads_clues_engaged_and_margin() {
    use card_dsl::dsl::Quantity;
    // clues at location
    let (state, inv) = state_with_cards_in_play(&[]);
    let ctx = EvalContext::for_controller(inv);
    // helper `with_clues(n)` already exists in this module; reuse it:
    assert_eq!(eval_quantity(&with_clues(2), &ctx, Quantity::CluesAtControllerLocation), 2);
    assert_eq!(eval_quantity(&with_clues(0), &ctx, Quantity::CluesAtControllerLocation), 0);
    // failure margin from the ctx binding
    let mut ctx2 = EvalContext::for_controller(inv);
    ctx2.set_failed_by(3);
    assert_eq!(eval_quantity(&state, &ctx2, Quantity::SkillTestFailedBy), 3);
    assert_eq!(eval_quantity(&state, &ctx, Quantity::SkillTestFailedBy), 0);
}
```

Note: `with_clues(n)` is the existing test helper used at `evaluator.rs:2230`; `state_with_cards_in_play` is at `evaluator.rs:3951`. If `with_clues` returns a state whose controller id differs from `inv`, use that state's controller id for `EvalContext::for_controller`.

- [ ] **Step 4: Run it — verify it fails.**

Run: `cargo test -p game-core eval_quantity_reads_clues_engaged_and_margin`
Expected: FAIL — `eval_quantity` not found.

- [ ] **Step 5: Thread `&EvalContext` into `eval_int_expr` / `eval_condition` and add `eval_quantity`.** In `evaluator.rs`:

Change the signature of `eval_condition` (`:1046`) from `(state, controller: InvestigatorId, condition)` to:

```rust
fn eval_condition(
    state: &GameState,
    eval_ctx: &EvalContext,
    condition: &Condition,
) -> Result<bool, String> {
```

and inside it replace every use of `controller` with `eval_ctx.controller`. Change `eval_int_expr` (`:1089`) the same way (param `eval_ctx: &EvalContext`, body uses `eval_ctx.controller`); its `Cond` arm call becomes `eval_condition(state, eval_ctx, when)?`. Update the three callers:
- `:447` `eval_condition(cx.state, eval_ctx.controller, condition)` → `eval_condition(cx.state, eval_ctx, condition)`
- `:819` `eval_int_expr(cx.state, eval_ctx.controller, combat_modifier)` → `eval_int_expr(cx.state, eval_ctx, combat_modifier)`
- `:873` `eval_int_expr(cx.state, eval_ctx.controller, shroud_modifier)` → `eval_int_expr(cx.state, eval_ctx, shroud_modifier)`

Then add the helper (place it directly above `eval_int_expr`):

```rust
/// Resolve a [`Quantity`] against current state for the controller.
/// Always non-negative; returned as `i8` to compose in [`IntExpr`].
fn eval_quantity(state: &GameState, eval_ctx: &EvalContext, q: Quantity) -> i8 {
    let controller = eval_ctx.controller;
    let n: usize = match q {
        Quantity::CluesAtControllerLocation => state
            .investigators
            .get(&controller)
            .and_then(|inv| inv.current_location)
            .and_then(|loc| state.locations.get(&loc))
            .map_or(0, |l| usize::from(l.clues)),
        Quantity::EngagedEnemies => state
            .enemies
            .values()
            .filter(|e| e.engaged_with == Some(controller))
            .count(),
        Quantity::SkillTestFailedBy => usize::from(eval_ctx.failed_by().unwrap_or(0)),
    };
    i8::try_from(n).unwrap_or(i8::MAX)
}
```

Add `Quantity` to the `card_dsl::dsl` import line at the top of `evaluator.rs` (the line importing `Condition, Effect, …`, `:63`).

- [ ] **Step 6: Fix the two existing test call-sites.** At `evaluator.rs:2230` and `:2234`, the calls `eval_condition(&with_clues(1), inv_id, &Condition::LocationHasClues)` must become `eval_condition(&with_clues(1), &EvalContext::for_controller(inv_id), &Condition::Compare { quantity: Quantity::CluesAtControllerLocation, op: CmpOp::Gt, value: 0 })`. (These are migrated fully in Task 2; for now just change the signature to pass a context and keep `LocationHasClues` — i.e. `eval_condition(&with_clues(1), &EvalContext::for_controller(inv_id), &Condition::LocationHasClues)`.)

- [ ] **Step 7: Run the new test + the crate.**

Run: `cargo test -p game-core eval_quantity_reads_clues_engaged_and_margin`
Expected: PASS.
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS.

- [ ] **Step 8: Commit.**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: Quantity vocabulary + eval_quantity; thread EvalContext into eval helpers"
```

---

### Task 2: `IntExpr::Count` + `Condition::Compare` (retire `LocationHasClues`) + `From`/`Into`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs`
- Modify: `crates/game-core/src/engine/evaluator.rs`
- Modify: `crates/cards/src/impls/roland_38_special.rs`

**Interfaces:**
- Produces: `IntExpr::Count(Quantity)`; `Condition::Compare { quantity: Quantity, op: CmpOp, value: i8 }`; `impl From<i8> for IntExpr` / `impl From<u8> for IntExpr`.
- Consumes: `Quantity`, `CmpOp`, `eval_quantity` (Task 1).

- [ ] **Step 1: Write the failing eval test.** In `evaluator.rs` tests:

```rust
#[test]
fn eval_count_and_compare_over_clues() {
    use card_dsl::dsl::{CmpOp, Condition, IntExpr, Quantity};
    let (_s, inv) = state_with_cards_in_play(&[]);
    let ctx = EvalContext::for_controller(inv);
    // Count
    assert_eq!(eval_int_expr(&with_clues(2), &ctx, &IntExpr::Count(Quantity::CluesAtControllerLocation)).unwrap(), 2);
    // Compare: clues > 0
    let has = Condition::Compare { quantity: Quantity::CluesAtControllerLocation, op: CmpOp::Gt, value: 0 };
    assert!(eval_condition(&with_clues(1), &ctx, &has).unwrap());
    assert!(!eval_condition(&with_clues(0), &ctx, &has).unwrap());
}
```

- [ ] **Step 2: Run — verify it fails.**

Run: `cargo test -p game-core eval_count_and_compare_over_clues`
Expected: FAIL — `IntExpr::Count` / `Condition::Compare` not found.

- [ ] **Step 3: Update the DSL types in `dsl.rs`.**

In `enum IntExpr`, add a variant after `Cond { … }`:

```rust
    /// A state-read count ([`Quantity`]).
    Count(Quantity),
```

In `enum Condition`, **delete** the `LocationHasClues` variant (`:1093-1094`) and add:

```rust
    /// Compare a [`Quantity`] against `value` under `op`.
    /// Replaces the old `LocationHasClues` (now `Compare { CluesAtControllerLocation, Gt, 0 }`).
    Compare {
        quantity: Quantity,
        op: CmpOp,
        value: i8,
    },
```

After the `impl IntExpr` block, add:

```rust
impl From<i8> for IntExpr {
    fn from(n: i8) -> Self {
        IntExpr::Lit(n)
    }
}

impl From<u8> for IntExpr {
    fn from(n: u8) -> Self {
        IntExpr::Lit(i8::try_from(n).unwrap_or(i8::MAX))
    }
}
```

- [ ] **Step 4: Add the eval arms in `evaluator.rs`.**

In `eval_int_expr`, add to the match: `IntExpr::Count(q) => Ok(eval_quantity(state, eval_ctx, *q)),`.

In `eval_condition`, **remove** the `Condition::LocationHasClues => { … }` arm and add:

```rust
        Condition::Compare { quantity, op, value } => {
            let lhs = eval_quantity(state, eval_ctx, *quantity);
            let rhs = *value;
            Ok(match op {
                CmpOp::Eq => lhs == rhs,
                CmpOp::Ne => lhs != rhs,
                CmpOp::Lt => lhs < rhs,
                CmpOp::Le => lhs <= rhs,
                CmpOp::Gt => lhs > rhs,
                CmpOp::Ge => lhs >= rhs,
            })
        }
```

Add `CmpOp` to the `card_dsl::dsl` import at the top of `evaluator.rs`.

- [ ] **Step 5: Migrate .38 Special.** In `crates/cards/src/impls/roland_38_special.rs`, change the import to add `CmpOp, Condition, Quantity` and replace the ability:

```rust
        fight(IntExpr::cond(
            Condition::Compare {
                quantity: Quantity::CluesAtControllerLocation,
                op: CmpOp::Gt,
                value: 0,
            },
            3,
            1,
        ), 1),
```

Update its unit test assertion (`roland_38_special.rs:62`) to the same `Condition::Compare { … }` expression.

- [ ] **Step 6: Migrate the Task-1 test stubs.** At the two call-sites edited in Task 1 Step 6 (`evaluator.rs` ~2230/2234), replace `&Condition::LocationHasClues` with `&Condition::Compare { quantity: Quantity::CluesAtControllerLocation, op: CmpOp::Gt, value: 0 }`.

- [ ] **Step 7: Run tests.**

Run: `cargo test -p game-core eval_count_and_compare_over_clues`
Expected: PASS.
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core -p cards -p card-dsl`
Expected: PASS (`LocationHasClues` fully gone; .38 Special behaviour identical).

- [ ] **Step 8: Commit.**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs crates/cards/src/impls/roland_38_special.rs
git commit -m "engine: IntExpr::Count + Condition::Compare over Quantity; retire LocationHasClues"
```

---

### Task 3: Widen `Deal.amount` + `Fight.extra_damage` to `IntExpr`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (field types + builders)
- Modify: `crates/game-core/src/engine/evaluator.rs` (eval the fields)

**Interfaces:**
- Produces (changed): `Effect::Deal { kind, target, amount: IntExpr }`; `Effect::Fight { combat_modifier: IntExpr, extra_damage: IntExpr }`; `deal_damage(target, amount: impl Into<IntExpr>)`, `deal_horror(target, amount: impl Into<IntExpr>)`, `fight(combat_modifier: impl Into<IntExpr>, extra_damage: impl Into<IntExpr>)`.

- [ ] **Step 1: Write the failing test (dynamic Deal amount).** In `evaluator.rs` tests:

```rust
#[test]
fn deal_amount_can_be_a_count_of_failure_margin() {
    use card_dsl::dsl::{deal_damage, IntExpr, Quantity, InvestigatorTarget};
    // Build a Deal whose amount is the failure margin, fail-by 2 -> 2 damage.
    let effect = deal_damage(InvestigatorTarget::You, IntExpr::Count(Quantity::SkillTestFailedBy));
    // (Drive it through the evaluator with a ctx whose failed_by = 2 and assert 2 damage.
    //  Mirror the existing for_each_point_failed_scales_body_by_margin test at evaluator.rs:4499,
    //  which sets ctx.set_failed_by(2) and asserts inv.damage == 2.)
    let _ = effect;
}
```

Replace the comment body with the same harness as `for_each_point_failed_scales_body_by_margin` (`evaluator.rs:4499`), substituting the `deal_damage(You, IntExpr::Count(SkillTestFailedBy))` effect and asserting `inv.damage == 2`.

- [ ] **Step 2: Run — verify it fails to compile.**

Run: `cargo test -p game-core deal_amount_can_be_a_count_of_failure_margin`
Expected: FAIL — `deal_damage` second arg type mismatch (still `u8`).

- [ ] **Step 3: Change the field types in `dsl.rs`.**

`Effect::Deal.amount: u8` → `amount: IntExpr`. `Effect::Fight.extra_damage: u8` → `extra_damage: IntExpr`.

Update builders:

```rust
pub fn deal_damage(target: InvestigatorTarget, amount: impl Into<IntExpr>) -> Effect {
    Effect::Deal { kind: HarmKind::Damage, target, amount: amount.into() }
}
pub fn deal_horror(target: InvestigatorTarget, amount: impl Into<IntExpr>) -> Effect {
    Effect::Deal { kind: HarmKind::Horror, target, amount: amount.into() }
}
pub fn fight(combat_modifier: impl Into<IntExpr>, extra_damage: impl Into<IntExpr>) -> Effect {
    Effect::Fight { combat_modifier: combat_modifier.into(), extra_damage: extra_damage.into() }
}
```

(Existing callers like `fight(IntExpr::Lit(1), 1)` keep working — `IntExpr` Into-s to itself, `1: u8/i8` Into-s via Task 2's `From`.)

- [ ] **Step 4: Eval the fields in `evaluator.rs`.**

`Deal` dispatch (`:420-424`): the arm currently passes `*amount` to `deal_effect(cx, eval_ctx, *kind, *target, *amount)`. Replace with an eval that clamps to `u8`:

```rust
        Effect::Deal { kind, target, amount } => {
            let n = match eval_int_expr(cx.state, eval_ctx, amount) {
                Ok(v) => u8::try_from(v.max(0)).unwrap_or(u8::MAX),
                Err(reason) => return EngineOutcome::Rejected { reason: reason.into() },
            };
            deal_effect(cx, eval_ctx, *kind, *target, n)
        }
```

`apply_fight` (`:806+`): `extra_damage: u8` param becomes `extra_damage: &IntExpr`; eval it next to `combat_modifier` (clamp to `u8` as above) before building the Fight follow-up that deals `1 + extra_damage`. Update the call-site that passes `extra_damage` into `apply_fight` to pass the `&IntExpr`.

- [ ] **Step 5: Run tests.**

Run: `cargo test -p game-core deal_amount_can_be_a_count_of_failure_margin`
Expected: PASS.
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core -p cards -p card-dsl`
Expected: PASS — existing literal call-sites (`.45 Automatic`, Knife, Flashlight, Machete, the deal-damage cards) compile via `Into`/`From` and behave identically.

- [ ] **Step 6: Commit.**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: widen Deal.amount + Fight.extra_damage to IntExpr (Into builders, literals untouched)"
```

---

### Task 4: #426 — `Count(SkillTestFailedBy)` deals + delete `ForEachPointFailed`

**Files:**
- Modify: `crates/cards/src/impls/grasping_hands.rs`, `crates/cards/src/impls/rotting_remains.rs`
- Modify: `crates/card-dsl/src/dsl.rs` (delete `Effect::ForEachPointFailed` + `for_each_point_failed`)
- Modify: `crates/game-core/src/engine/evaluator.rs` (delete frame arms)
- Modify: `crates/game-core/src/state/game_state.rs` (delete `EffectFrame::ForEachPointFailed`)

**Interfaces:**
- Consumes: `deal_damage`/`deal_horror` with `impl Into<IntExpr>` (Task 3); `IntExpr::Count` (Task 2).
- Removes: `Effect::ForEachPointFailed`, `for_each_point_failed`, `EffectFrame::ForEachPointFailed`.

- [ ] **Step 1: Update Grasping Hands.** In `grasping_hands.rs`, change the import (drop `for_each_point_failed`, add `IntExpr, Quantity`) and the effect:

```rust
        Some(deal_damage(
            InvestigatorTarget::You,
            IntExpr::Count(Quantity::SkillTestFailedBy),
        )),
```

Rewrite its unit test (`revelation_tests_agility_3_then_damage_per_point`) to assert `on_fail` is `Some(Effect::Deal { kind: HarmKind::Damage, amount: IntExpr::Count(Quantity::SkillTestFailedBy), .. })`.

- [ ] **Step 2: Update Rotting Remains** identically in `rotting_remains.rs` with `deal_horror(... Count(SkillTestFailedBy))` and `HarmKind::Horror`.

- [ ] **Step 3: Add a card test proving one simultaneous N-instance.** In each card's tests, add a behaviour test (mirror `crates/cards/tests/` patterns, e.g. `grasping_hands` integration if present, else an engine-level drive): fail the test by 2 → assert exactly 2 damage/horror applied and a single `Deal` resolution (no per-point loop). If the card has an integration test file under `crates/cards/tests/`, add the assertion there; otherwise assert via the evaluator harness used in Task 3 Step 1.

- [ ] **Step 4: Run — the two cards still compile against the (about-to-be-removed) `ForEachPointFailed`? No — they no longer reference it.**

Run: `cargo test -p cards grasping rotting`
Expected: PASS for the two card unit tests.

- [ ] **Step 5: Delete `ForEachPointFailed`.**
- `dsl.rs`: delete the `ForEachPointFailed(Box<Effect>)` variant (`:657`) and the `for_each_point_failed` builder (`:1513-1515`).
- `game_state.rs`: delete the `EffectFrame::ForEachPointFailed { … }` variant (`:713`).
- `evaluator.rs`: delete the `Effect::ForEachPointFailed(body) => EffectFrame::ForEachPointFailed { … }` arm in `frame_of` (`:321`); delete the `EffectFrame::ForEachPointFailed { remaining, body, ctx } => { … }` driver arm (`:359`); in the `Effect::Seq(_) | Effect::ForEachPointFailed(_) =>` arm (`:433`) drop the `| Effect::ForEachPointFailed(_)` so it reads `Effect::Seq(_) =>`.
- Delete the now-orphaned tests `for_each_point_failed_scales_body_by_margin` (`:4499`) and `for_each_point_failed_with_no_margin_is_a_noop` (`:4523`) — their coverage moved to Task 1/Task 3 (`eval_quantity` margin + dynamic `Deal`). Remove `for_each_point_failed` from the test import at `evaluator.rs:2104`.

**Keep** `EvalContext::failed_by` / `set_failed_by` / `SkillTestBinding` — now consumed by `Quantity::SkillTestFailedBy`.

- [ ] **Step 6: Run the strict gauntlet for the touched crates.**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS — no `ForEachPointFailed` references remain; clippy/dead-code clean.
Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS.

- [ ] **Step 7: Commit.**

```bash
git add crates/cards/src/impls/grasping_hands.rs crates/cards/src/impls/rotting_remains.rs crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs crates/game-core/src/state/game_state.rs
git commit -m "card: Grasping Hands/Rotting Remains deal one Count(SkillTestFailedBy) instance; delete ForEachPointFailed (#426)"
```

---

### Task 5: #300 — Machete conditional `extra_damage`

**Files:**
- Modify: `crates/cards/src/impls/machete.rs`

**Interfaces:**
- Consumes: `fight(impl Into<IntExpr>, impl Into<IntExpr>)` (Task 3); `IntExpr::cond` + `Condition::Compare` + `Quantity::EngagedEnemies` (Tasks 1–2).

- [ ] **Step 1: Write the failing behaviour test.** In `machete.rs` tests (or `crates/cards/tests/` if Machete has an integration file), add: with exactly one enemy engaged with the actor, a successful Machete Fight deals `1 + 1 = 2` damage; with two enemies engaged, it deals `1 + 0 = 1`. Build the state with `test_enemy` fixtures engaged to the actor (`engaged_with = Some(actor)`), drive the activated Fight, assert enemy damage. Also keep/adjust the existing structural unit test to assert `extra_damage == IntExpr::cond(Condition::Compare { quantity: Quantity::EngagedEnemies, op: CmpOp::Eq, value: 1 }, 1, 0)`.

- [ ] **Step 2: Run — verify it fails.**

Run: `cargo test -p cards machete`
Expected: FAIL — current impl is unconditional `extra_damage: 1`.

- [ ] **Step 3: Update the impl.** In `machete.rs`, change the import to add `CmpOp, Condition, Quantity` and:

```rust
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![],
        fight(
            1,
            IntExpr::cond(
                Condition::Compare {
                    quantity: Quantity::EngagedEnemies,
                    op: CmpOp::Eq,
                    value: 1,
                },
                1,
                0,
            ),
        ),
    )]
}
```

Remove the unconditional-damage doc-comment / `TODO(#300)` in the module header.

- [ ] **Step 4: Run tests.**

Run: `cargo test -p cards machete`
Expected: PASS — +1 damage only when the attacked enemy is the sole engaged one.

- [ ] **Step 5: Full strict gauntlet.**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add crates/cards/src/impls/machete.rs
git commit -m "card: Machete deals +1 damage only vs the sole engaged enemy (#300)"
```

---

## Self-Review

**Spec coverage (Sections 1 + 3):** `Quantity` + `eval_quantity` (T1) · `IntExpr::Count` (T2) · `Condition::Compare`/`CmpOp` + retire `LocationHasClues` (T2) · `From`/`Into` builders (T2/T3) · widen `Deal.amount` + `extra_damage` (T3) · .38 Special migration (T2) · #426 + delete `ForEachPointFailed` (T4) · #300 (T5). Section 2 (#118 + bridge) is deliberately out — PR 2. ✓

**Placeholder scan:** T4 Step 3 and T5 Step 1 describe behaviour tests by reference to existing harnesses rather than full code — acceptable because they reuse named, located patterns (`for_each_point_failed_scales_body_by_margin` at `:4499`; `test_enemy` fixtures). All type/DSL/card edits show complete code. ✓

**Type consistency:** `eval_int_expr`/`eval_condition` gain `eval_ctx: &EvalContext` in T1 and are called that way in T2–T3. `Quantity`/`CmpOp` variant names match across dsl.rs and evaluator arms. Builder signatures (`impl Into<IntExpr>`) consistent T3→T4→T5. ✓
