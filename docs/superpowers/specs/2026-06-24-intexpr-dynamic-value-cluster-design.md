# IntExpr dynamic-value cluster — design

**Date:** 2026-06-24
**Issues:** #118 (Roland elder-sign), #300 (Machete), #426 (Grasping Hands / Rotting Remains). Related: #448 (investigator-card-as-permanent epic — retires #118's bridge).

## Motivation

Three p1-next gate-correctness issues all need the same thing: **board-state-dependent numeric values** the DSL can't yet express. The `IntExpr` AST (`Lit`/`Cond`) already feeds `Effect::Fight.combat_modifier` and `Effect::Investigate.shroud_modifier` (Roland's .38 Special 01006 is the live consumer), but it can't read *counts* off state, and the other numeric `Effect` fields are literal. This spec extends `IntExpr` minimally, unifies conditions over a shared count vocabulary, and applies it to the three cards.

A corpus survey (Core + Dunwich Legacy) confirmed a broad future trajectory (per-clue/resource/horror modifiers, success/failure margins, clamp, multiply) but **none of Clamp/Scaled is exercised by these three** — so they're deferred. This stays inside the repo's "no speculative DSL primitives; wait for 2+ consumers" norm.

## Section 1 — core model

### `Quantity` — a shared state-reading vocabulary

```rust
pub enum Quantity {
    /// Clues on the controller's current location.
    CluesAtControllerLocation,
    /// Enemies engaged with the controller.
    EngagedEnemies,
    /// Failure margin of the resolving skill test (0 outside one).
    SkillTestFailedBy,
}
```

Backed by one helper, used by **both** consumers below:

```rust
fn eval_quantity(state, ctx: &EvalContext, controller, q: Quantity) -> i8
// CluesAtControllerLocation -> clues at controller's current location
// EngagedEnemies            -> count of enemies engaged with controller
// SkillTestFailedBy         -> ctx.failed_by().unwrap_or(0)   (existing plumbing, evaluator.rs:214)
```

### `IntExpr` grows one variant

```rust
pub enum IntExpr {
    Lit(i8),
    Cond { when: Condition, then: i8, otherwise: i8 },
    Count(Quantity),   // NEW
}
```

`Clamp`/`Scaled` (multiply, min/max) are **deferred** — real future consumers (Shotgun, Chicago Typewriter, "I've got a plan!"), none among these three. The enum stays open for them.

### `Condition` — comparisons over `Quantity`

Replace the bespoke `LocationHasClues` with a comparison, so one `Quantity` vocabulary serves both value and predicate roles:

```rust
pub enum Condition {
    SkillTestKind(SkillTestKind),
    SkillTest { outcome: TestOutcome },        // (unchanged; still stubbed)
    Compare { quantity: Quantity, op: CmpOp, value: i8 },   // NEW (replaces LocationHasClues)
}

pub enum CmpOp { Eq, Ne, Lt, Le, Gt, Ge }
```

`CmpOp` is the full small closed set — trivial, total, zero-risk to evaluate, and saves re-touching when no-clues (`Eq 0`) / margin-threshold (`Ge N`) cards land.

`eval_condition`'s `Compare` arm evaluates `eval_quantity(quantity)` against `value` under `op`. `IntExpr::Cond { when }` reuses the same `Condition`, so `Cond` automatically gains `Compare`.

### Field widening + builder ergonomics

Widen the numeric `Effect` fields these issues drive to `IntExpr`:
- `Effect::Deal.amount: u8 -> IntExpr` (#426)
- `Effect::Fight.extra_damage: u8 -> IntExpr` (#300)

`Effect::Modify.delta` stays `i8` (the elder-sign does **not** route through `Modify` — see Section 2; no other consumer among these three).

Add `From<i8>`/`From<u8> for IntExpr` (→ `IntExpr::Lit`) and make the builders (`fight`, `deal_damage`, `deal_horror`) take `impl Into<IntExpr>`. **Every existing flat call keeps compiling** (`deal_damage(You, 1)` auto-converts); literals stay bare, no `Lit(..)` noise.

### Migrate the existing consumer

.38 Special (01006): `cond(LocationHasClues, 3, 1)` → `cond(Compare(CluesAtControllerLocation, Gt, 0), 3, 1)`. Behaviour-preserving; update its test.

## Section 2 — Roland's elder-sign (#118)

### Architecture — the elder-sign is the investigator's symbol

Symbol tokens source their outcome from the scenario (`resolve_symbol_token` → `SymbolOutcome { modifier, immediate, on_fail }`, then `Some(o) => TokenResolution::Modifier(o.modifier)`). The elder-sign is the **investigator's** symbol — same pipeline, sourced from the **investigator card** instead of the scenario. The bonus flows through the existing `Modifier` total path; **no** `Effect::ModifySkillTestTotal`.

### Representation

```rust
Trigger::ElderSign { modifier: IntExpr }   // config-on-trigger, like Activated { action_cost }
```

Roland's `abilities()` (01001) gains `Trigger::ElderSign { modifier: IntExpr::Count(Quantity::CluesAtControllerLocation) }`. Reached through the existing `abilities_for` — no new registry pointer.

### Firing path (ST.4)

The `TokenResolution::ElderSign` arm (currently `+0`, `skill_test.rs:309`) calls `elder_sign_modifier(state, reg, controller)`, which looks up the controller's investigator card directly (via the bridge handle below), reads its `Trigger::ElderSign { modifier }`, returns `eval_int_expr(modifier)` — **0 if none** (every other investigator resolves exactly as today). The arm becomes `(skill_value.saturating_add(bonus).max(0), Total)`, keeping the `ElderSign` resolution label for observability.

### The bridge (#118 scope; retired by #448)

The investigator's own card code is currently **dropped at seating** (`phases.rs:75-94` builds `Investigator` with no `card_code` and `cards_in_play: Vec::new()`), so investigator-card abilities don't fire in a seated game — Roland's *reaction* only fires today because tests hand-inject the card (`evidence.rs:232`). #118 adds the minimal handle and **folds in the seated-reaction fix**:

- `Investigator.card_code: CardCode` — set at seating from `RosterEntry.investigator`. Elder-sign looks it up directly (`abilities_for(card_code)` → the `Trigger::ElderSign` ability); no scan.
- `Investigator.ability_usage: BTreeMap<u8, AbilityUsageRecord>` — mirrors `CardInPlay.ability_usage`, a usage-tracking home for usage-limited investigator-card abilities (Roland's reaction is once-per-round).
- A `scan_investigator_card_reactions` source in the reaction scan, **mirroring `scan_act_agenda_reactions`** (`reaction_windows.rs:302`) — candidate keyed by code, the usage check/bump pointed at `Investigator.ability_usage`. Makes Roland's reaction fire from a seated investigator.

The investigator card stays **out of `cards_in_play`**, so none of soak / asset-slot / discard logic sees it (no phantom-soaker hazard). **#448** later unifies the investigator card as a real `CardInPlay` (health/sanity/soak too), retiring `card_code` + `ability_usage` + the bespoke scan source into the uniform path — #118's bridge is deliberately small and documented as sunset-by-#448.

### Deferred (doc-comment notes)

Elder-signs that also run an *effect* (Daisy's per-Tome draw, Agnes's optional damage) — the inline path handles only the modifier; when the first lands, consider building a full `SymbolOutcome` from the investigator card for uniformity with the scenario path (and possibly `IntExpr`-ifying symbol effects so the scenario hard-codes less). Substitute-test / reveal-another-token elder-signs are also deferred. Roland is pure-modifier, so none of this is needed now.

## Section 3 — applications + cleanup

### #300 Machete (01020)

`machete.rs`: `fight(1, cond(Compare(EngagedEnemies, Eq, 1), 1, 0))` — `extra_damage` is `+1` iff the attacked enemy is the sole engaged one. Drops the unconditional-damage `TODO(#300)`. Multi-target Fight (#401) already landed, so the condition is load-bearing.

### #426 Grasping Hands (01162) / Rotting Remains (01163)

`grasping_hands.rs`: `deal_damage(You, Count(SkillTestFailedBy))`; `rotting_remains.rs`: `deal_horror(You, Count(SkillTestFailedBy))`. This deals **one simultaneous N-point instance** (the correctness fix) instead of N separate 1-point deals.

### Delete `ForEachPointFailed` (now dead)

After the #426 migration, `Effect::ForEachPointFailed`'s only consumers were those two cards. Delete:
- `Effect::ForEachPointFailed` + the `for_each_point_failed` builder (`dsl.rs:657,1513`)
- `EffectFrame::ForEachPointFailed` + its evaluator arms (`game_state.rs:713`, `evaluator.rs:321,359,433`)
- its tests (replaced by `eval_quantity(SkillTestFailedBy)` tests)

**Keep** the `failed_by` / `SkillTestBinding` margin plumbing — now consumed by `Quantity::SkillTestFailedBy`. Net: the per-point-loop machinery is replaced by a count-valued `Deal`, simplifying the evaluator. (Required regardless — warnings-as-errors would flag the dead builder/variant.)

## Testing

- **Per-card:** Roland elder-sign at 0/1/2 clues at his location; Machete sole-engaged (+1 dmg) vs 2-engaged (no +1); Grasping/Rotting fail-by 0/1/2 → exactly N as one instance.
- **Engine:** `eval_quantity` per term; `eval_condition` `Compare` (`Eq`/`Gt`); .38 Special migration byte-identical; elder-sign firing (`0` when no elder-sign ability); the seated-reaction fold-in (Roland's reaction fires from a seated investigator with no manual injection, and respects once-per-round via `ability_usage`).
- **Integration:** Roland seated → elder-sign token → total gains his location's clue count; his reaction fires seated.

## Scope / ordering

Implementation slices, dependency-ordered:
1. **DSL core** (Section 1): `Quantity` + `eval_quantity`, `IntExpr::Count`, `Condition::Compare` + `CmpOp`, field widening + `From`/`Into` builders, .38 Special migration.
2. **#426** + delete `ForEachPointFailed`.
3. **#300** (adds `EngagedEnemies` eval — already in `Quantity`).
4. **#118** + the bridge (the larger slice: `Trigger::ElderSign`, the ST.4 firing path, `card_code`/`ability_usage`/seated-reaction scan source).

Slices 2/3 are pure DSL with no investigator-card dependency; slice 4 carries the bridge.

## Deferred / out of scope

- `IntExpr::Clamp`, `IntExpr::Scaled` (multiply) — future consumers, none here.
- `Cond` branches as `IntExpr` (margin-gated "N instead of M") — future (Deduction reprint 02150); cheap to add then.
- More `Quantity` terms (resources, horror-on-character, success-margin, etc.) — added one at a time per consumer.
- #448 (investigator-card-as-permanent) — retires #118's bridge; its own design.
