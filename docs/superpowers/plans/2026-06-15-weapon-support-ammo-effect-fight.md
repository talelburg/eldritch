# Weapon Support (ammo/uses + `Effect::Fight`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the engine machinery a weapon-with-ammo asset needs — `Uses (N <kind>)` tracking, a `Cost::SpendUses` payment, and an inspectable `Effect::Fight` that initiates a Combat skill test with a (possibly clue-conditional) combat modifier and bonus damage — leaving the `.38 Special` / `Cover Up` card impls (C5c #238) as pure content.

**Architecture:** Ammo is pipeline-parsed from card text into `CardKind::Asset.uses`, mirrored onto each `CardInstance` as `uses_remaining`, and spent through the existing activated-ability cost flow. The new `Effect::Fight` resolves a value-level `IntExpr` modifier against state, auto-targets the single engaged enemy, and reuses the existing skill-test suspend/resume path; the in-flight test carries a flat `test_modifier` and the Fight follow-up deals `1 + extra_damage`. Validate-first: `check_activate_ability` rejects a Fight ability fired with ≠1 engaged enemy before charging anything.

**Tech Stack:** Rust workspace — `card-dsl` (types), `card-data-pipeline` (ingestion), `game-core` (engine). This is issue **#295**; the consumer is **#238**.

**CI gauntlet (run before every commit that touches code):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```
Run `cargo run -p card-data-pipeline` only in the task that changes the pipeline.

---

## File Structure

**`card-dsl` (types):**
- `crates/card-dsl/src/card_data.rs` — add `Uses` struct + `UsesKind` enum; add `uses: Option<Uses>` to `CardKind::Asset` (line 292).
- `crates/card-dsl/src/dsl.rs` — add `Cost::SpendUses` (enum at 360); `IntExpr` enum + builders; `Condition::LocationHasClues` (enum at 767); `Effect::Fight` (enum at 475) + `fight(...)` builder.

**`card-data-pipeline` (ingestion):**
- `crates/card-data-pipeline/src/main.rs` — `parse_uses()`; thread into the Asset `EmitCard` (line ~301) + the Asset emit format string (line 481).
- `crates/cards/src/generated/cards.rs` — regenerated (never hand-edited).

**`game-core` (engine):**
- `crates/game-core/src/state/card.rs` — `uses_remaining: Option<u8>` on `CardInPlay` (struct 136, `enter_play` 215).
- `crates/game-core/src/event.rs` — `Event::UsesSpent`.
- `crates/game-core/src/engine/dispatch/abilities.rs` — `Cost::SpendUses` validate (`check_cost_payable`) + pay (`pay_activation_costs`).
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — thread `source_uses_remaining` into `check_activate_ability` (938); add the Fight target precondition.
- `crates/game-core/src/engine/dispatch/cards.rs` — init `uses_remaining` from metadata on asset enter-play (~526).
- `crates/game-core/src/state/game_state.rs` — `test_modifier: i8` on `InFlightSkillTest` (408); `extra_damage: u8` on `SkillTestFollowUp::Fight` (588).
- `crates/game-core/src/engine/dispatch/skill_test.rs` — `start_skill_test` param (28); `sum_skill_value` reads `test_modifier` (400); Fight follow-up deals `1 + extra_damage` (592).
- `crates/game-core/src/engine/evaluator.rs` — `Effect::Fight` arm (`apply_effect` 151); `Condition::LocationHasClues` arm (`eval_condition` 353); `IntExpr` resolver.
- `crates/game-core/src/engine/dispatch/actions.rs` — base `PlayerAction::Fight` passes `extra_damage: 0` to the follow-up (457).

---

## Task 1: `Uses` / `UsesKind` metadata types + `CardKind::Asset.uses`

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs:292`
- Test: same file (`#[cfg(test)]`)

- [ ] **Step 1: Add the types and field.** Above the `CardKind` enum (near other metadata types), add:

```rust
/// Limited-use tokens an asset enters play with ("Uses (4 ammo)").
/// Spending them is a `Cost::SpendUses`; depletion blocks the ability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Uses {
    /// What the tokens are called on the card.
    pub kind: UsesKind,
    /// How many the asset enters play with.
    pub count: u8,
}

/// The named token type an asset's `Uses (N <kind>)` grants. Only the
/// kinds Slice-1 cards print are modeled; others land as their cards do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsesKind {
    /// "Uses (N ammo)" — firearms.
    Ammo,
}
```

Then add the field to `CardKind::Asset` (after `deck_limit` at line 310, keeping the trailing comma):

```rust
        /// Maximum copies per deck.
        deck_limit: u8,
        /// Limited-use tokens granted on enter-play ("Uses (N ammo)"),
        /// or `None`. Pipeline-parsed from card text.
        uses: Option<Uses>,
```

- [ ] **Step 2: Fix every `CardKind::Asset { … }` literal in this crate.** The non-`..` constructor in the card_data tests (line ~517) needs `uses: None`. Search:

Run: `grep -rn 'CardKind::Asset {' crates/card-dsl/src | grep -v '\.\.'`
For each hit, add `uses: None,` before the closing brace.

- [ ] **Step 3: Add a round-trip test.** In the `#[cfg(test)]` module:

```rust
#[test]
fn asset_uses_round_trips() {
    let uses = Some(Uses { kind: UsesKind::Ammo, count: 4 });
    let json = serde_json::to_string(&uses).unwrap();
    let back: Option<Uses> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, uses);
}
```

- [ ] **Step 4: Compile + test the crate.**

Run: `cargo test -p card-dsl`
Expected: PASS (other crates won't compile yet — that's Task 2+).

- [ ] **Step 5: Commit.**

```bash
git add crates/card-dsl/src/card_data.rs
git commit -m "card-dsl: add Uses/UsesKind + CardKind::Asset.uses field"
```

---

## Task 2: Pipeline parses `Uses (N <kind>)`

**Files:**
- Modify: `crates/card-data-pipeline/src/main.rs` (parse fn + Asset `EmitCard` build ~301 + emit string ~481)
- Test: same file (`#[cfg(test)]` at ~714)

- [ ] **Step 1: Write the failing parser test.** In the test module:

```rust
#[test]
fn parse_uses_reads_ammo_count() {
    assert_eq!(
        parse_uses("Uses (4 ammo).\n[action] Spend 1 ammo: Fight."),
        Some((4u8, "Ammo"))
    );
    assert_eq!(parse_uses("Some other card text."), None);
}
```

- [ ] **Step 2: Run it — fails (no `parse_uses`).**

Run: `cargo test -p card-data-pipeline parse_uses_reads_ammo_count`
Expected: FAIL — cannot find function `parse_uses`.

- [ ] **Step 3: Implement `parse_uses`.** Add near `parse_spawn_name` (~648). Returns the count and the Rust enum-variant string for emission; unknown kinds emit a build warning and yield `None` (no silent approximation):

```rust
/// Parse a printed `Uses (N <kind>)` clause into `(count, variant)` where
/// `variant` is the `UsesKind` Rust variant name for code emission. Returns
/// `None` when absent; warns + returns `None` for an unmodeled kind.
fn parse_uses(text: &str) -> Option<(u8, &'static str)> {
    let plain = strip_html_bold(text);
    let start = plain.find("Uses (")? + "Uses (".len();
    let inner = &plain[start..];
    let end = inner.find(')')?;
    let body = inner[..end].trim(); // e.g. "4 ammo"
    let (num, kind) = body.split_once(' ')?;
    let count: u8 = num.trim().parse().ok()?;
    let variant = match kind.trim().to_ascii_lowercase().as_str() {
        "ammo" => "Ammo",
        other => {
            eprintln!("warning: unmodeled Uses kind {other:?}; emitting uses: None");
            return None;
        }
    };
    Some((count, variant))
}
```

- [ ] **Step 4: Run it — passes.**

Run: `cargo test -p card-data-pipeline parse_uses_reads_ammo_count`
Expected: PASS.

- [ ] **Step 5: Thread `uses` into the Asset emit.** The `EmitCard` intermediate struct carries an extra field for assets. Add `uses: Option<(u8, &'static str)>` to the struct (~230) defaulting `None`, set it for assets where the Asset branch builds (`uses: parse_uses(raw.text.as_deref().unwrap_or(""))`, ~301), and extend the `"Asset"` emit format string (line 481) to append `, uses: {}` rendering either `None` or `Some(Uses {{ kind: UsesKind::{}, count: {} }})`. Add a small helper:

```rust
fn uses_lit(uses: Option<(u8, &'static str)>) -> String {
    match uses {
        None => "None".to_owned(),
        Some((count, variant)) => {
            format!("Some(Uses {{ kind: UsesKind::{variant}, count: {count} }})")
        }
    }
}
```

Ensure the generated file's `use` line imports `Uses, UsesKind` (the emit prelude — find where `SkillIcons`/`Slot` are imported in the generated header string and add `Uses, UsesKind`).

- [ ] **Step 6: Regenerate the corpus.**

Run: `cargo run -p card-data-pipeline`
Then: `grep -n '"01006"' -A6 crates/cards/src/generated/cards.rs`
Expected: the 01006 Asset now ends with `…, deck_limit: 1, uses: Some(Uses { kind: UsesKind::Ammo, count: 4 }) }`. Spot-check a non-uses asset (01008) shows `uses: None`.

- [ ] **Step 7: Build the cards crate.**

Run: `cargo build -p cards`
Expected: compiles (the new field is populated for every asset).

- [ ] **Step 8: Commit.**

```bash
git add crates/card-data-pipeline/src/main.rs crates/cards/src/generated/cards.rs
git commit -m "card-data-pipeline: parse Uses (N kind) into CardKind::Asset.uses"
```

---

## Task 3: `CardInPlay.uses_remaining` + init on asset enter-play

**Files:**
- Modify: `crates/game-core/src/state/card.rs:136,215`
- Modify: `crates/game-core/src/engine/dispatch/cards.rs:~526`
- Test: `crates/game-core/src/state/card.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Add the field.** In `struct CardInPlay` (after `clues` at 162):

```rust
    /// Remaining limited-use tokens ("ammo") for an asset that entered
    /// play with `CardKind::Asset.uses`. `None` for cards without uses
    /// and for non-asset in-play instances (threat-area/attachments).
    pub uses_remaining: Option<u8>,
```

In `enter_play` (215), initialize `uses_remaining: None` (the asset enter-play path sets the real value — see Step 3, keeping `enter_play` generic for threat-area/attachment callers).

- [ ] **Step 2: Write the failing init test.** In `cards.rs`-adjacent engine tests is heavier; instead test the state default here and the init in Task-3 Step 4. Add to `card.rs` tests:

```rust
#[test]
fn enter_play_defaults_uses_remaining_to_none() {
    let c = CardInPlay::enter_play(CardCode::new("01006"), CardInstanceId(1));
    assert_eq!(c.uses_remaining, None);
}
```

- [ ] **Step 3: Init from metadata on asset enter-play.** In `cards.rs` `play_card`, immediately after the asset is pushed to `cards_in_play` (~526), set its uses from the registry metadata:

```rust
// Seed limited-use tokens (ammo) from the asset's printed `uses`.
let initial_uses = card_registry::current()
    .and_then(|reg| (reg.metadata_for)(&code))
    .and_then(|m| match &m.kind {
        crate::card_data::CardKind::Asset { uses, .. } => uses.as_ref().map(|u| u.count),
        _ => None,
    });
if let Some(inv) = cx.state.investigators.get_mut(&investigator) {
    if let Some(card) = inv.cards_in_play.last_mut() {
        card.uses_remaining = initial_uses;
    }
}
```

(Use the variable name the surrounding code already binds for the played card's code / investigator id; adjust if they differ.)

- [ ] **Step 4: Run state test + build.**

Run: `cargo test -p game-core enter_play_defaults_uses_remaining_to_none`
Expected: PASS.
Run: `cargo build -p game-core`
Expected: compiles.

- [ ] **Step 5: Commit.**

```bash
git add crates/game-core/src/state/card.rs crates/game-core/src/engine/dispatch/cards.rs
git commit -m "game-core: CardInPlay.uses_remaining + init from metadata on asset enter-play"
```

---

## Task 4: `Cost::SpendUses` + `Event::UsesSpent` (validate + pay)

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs:360` (Cost variant)
- Modify: `crates/game-core/src/event.rs` (new event)
- Modify: `crates/game-core/src/engine/dispatch/abilities.rs` (`check_cost_payable`, `pay_activation_costs`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:938` (thread `source_uses_remaining`)
- Test: `crates/game-core/src/engine/dispatch/abilities.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Add the `Cost` variant.** In `enum Cost` (360):

```rust
    /// Spend `n` limited-use tokens from the source asset's
    /// `uses_remaining`. Insufficient remaining rejects the activation.
    SpendUses(u8),
```

- [ ] **Step 2: Add the event.** In `event.rs`, beside `ResourcesPaid` (135):

```rust
    /// `n` limited-use tokens were spent from a source asset's
    /// `uses_remaining` to pay a `Cost::SpendUses`.
    UsesSpent {
        /// Investigator who activated the ability.
        investigator: InvestigatorId,
        /// The source asset instance whose uses were spent.
        instance_id: CardInstanceId,
        /// How many were spent.
        amount: u8,
    },
```

- [ ] **Step 3: Thread `source_uses_remaining` to the validator.** `check_cost_payable` currently takes `(cost, inv, source_exhausted)`. Add a `source_uses_remaining: Option<u8>` parameter. In `check_activate_ability` (reaction_windows.rs:938), read the source card's `uses_remaining` (it already locates the source instance for `source_exhausted`) and pass it through. Add the `SpendUses` arm to `check_cost_payable`:

```rust
        Cost::SpendUses(n) => match source_uses_remaining {
            Some(rem) if rem >= *n => Ok(()),
            Some(rem) => Err(format!(
                "ActivateAbility: needs {n} uses; source has {rem} remaining"
            )),
            None => Err(
                "ActivateAbility: SpendUses on a source with no uses tokens".to_string(),
            ),
        },
```

- [ ] **Step 4: Pay it.** In `pay_activation_costs`, add the `SpendUses` arm (it has `investigator`, `instance_id`, `in_play_pos`):

```rust
            Cost::SpendUses(n) => {
                let card = &mut cx
                    .state
                    .investigators
                    .get_mut(&investigator)
                    .expect("validated above")
                    .cards_in_play[in_play_pos];
                card.uses_remaining =
                    Some(card.uses_remaining.unwrap_or(0).saturating_sub(*n));
                cx.events.push(Event::UsesSpent {
                    investigator,
                    instance_id,
                    amount: *n,
                });
            }
```

- [ ] **Step 5: Write tests.** In `abilities.rs` tests, add unit coverage for the validator (pure, no registry needed):

```rust
#[test]
fn spend_uses_payable_only_with_enough_remaining() {
    let inv = Investigator::test_investigator(InvestigatorId(1));
    assert!(check_cost_payable(&Cost::SpendUses(1), &inv, false, Some(4)).is_ok());
    assert!(check_cost_payable(&Cost::SpendUses(1), &inv, false, Some(0)).is_err());
    assert!(check_cost_payable(&Cost::SpendUses(1), &inv, false, None).is_err());
}
```

(Match the `test_investigator` constructor the file already uses; if `check_cost_payable` isn't in scope, `use super::check_cost_payable`.)

- [ ] **Step 6: Run + full gauntlet.**

Run: `cargo test -p game-core spend_uses_payable_only_with_enough_remaining`
Expected: PASS.
Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features` then `cargo clippy --all-targets --all-features -- -D warnings` then `cargo fmt --check`
Expected: all green (existing `check_cost_payable` callers updated for the new param; any non-`..` `Event` matches handle `UsesSpent`).

- [ ] **Step 7: Commit.**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/event.rs \
        crates/game-core/src/engine/dispatch/abilities.rs \
        crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "game-core: Cost::SpendUses validate+pay + Event::UsesSpent"
```

---

## Task 5: `IntExpr` + `Condition::LocationHasClues`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`IntExpr` near Condition ~759; `Condition::LocationHasClues` at 767)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`eval_condition` 353; `IntExpr` resolver)
- Test: both files

- [ ] **Step 1: Add `Condition::LocationHasClues`.** In `enum Condition` (767):

```rust
    /// Holds when the controller's current location has ≥1 clue.
    /// "if there are 1 or more clues on your location" (.38 Special).
    LocationHasClues,
```

- [ ] **Step 2: Add `IntExpr` + builders.** Below the `Condition` enum:

```rust
/// An integer computed at effect-evaluation time. Lets a numeric field
/// carry a condition-gated value without duplicating the surrounding
/// effect (".38 Special": +1, or +3 instead if clues on your location).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntExpr {
    /// A literal value.
    Lit(i8),
    /// `then` if `when` holds at eval time, else `otherwise`.
    Cond {
        /// Predicate evaluated against current state.
        when: Condition,
        /// Value when the predicate holds.
        then: i8,
        /// Value when it does not.
        otherwise: i8,
    },
}

/// Build an [`IntExpr::Cond`].
#[must_use]
pub fn int_cond(when: Condition, then: i8, otherwise: i8) -> IntExpr {
    IntExpr::Cond { when, then, otherwise }
}
```

- [ ] **Step 3: Write the failing eval test.** In `evaluator.rs` tests. `eval_condition` gains a `controller` parameter in Step 5, so call it with three args. Build the state with the crate's verified `GameStateBuilder` + location fixtures (mirror how the Investigate/Deduction tests in `engine/mod.rs` stand up a location with clues and an investigator located there — set `location.clues = 1`, `investigator.current_location = Some(loc_id)`):

```rust
#[test]
fn location_has_clues_condition_tracks_clue_count() {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(1);
    // Helper inline: build a state with the investigator at `loc_id`,
    // which has `clue_count` clues. (Use the same Location/investigator
    // fixtures the Investigate tests use.)
    let with_clues = |clue_count: u8| {
        let mut inv = test_investigator(1);
        inv.current_location = Some(loc_id);
        let mut loc = test_location(loc_id, "Study");
        loc.clues = clue_count;
        GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .build()
    };
    assert_eq!(
        eval_condition(&with_clues(1), inv_id, &Condition::LocationHasClues),
        Ok(true)
    );
    assert_eq!(
        eval_condition(&with_clues(0), inv_id, &Condition::LocationHasClues),
        Ok(false)
    );
}
```

(If the exact location fixture differs — e.g. `test_location` takes different args or `with_location` is named differently — match the signatures the Investigate tests already call; the assertion shape is the load-bearing part.)

- [ ] **Step 4: Run — fails (unhandled variant).**

Run: `cargo test -p game-core location_has_clues_condition_tracks_clue_count`
Expected: FAIL (non-exhaustive match or assertion).

- [ ] **Step 5: Implement the eval arm.** `eval_condition` (353) currently takes `&GameState`. `LocationHasClues` needs the controller; thread the controller in via the call site (it runs under `apply_effect`, which has `eval_ctx.controller`). Change `eval_condition` to also accept `controller: InvestigatorId` (update its one caller in the `Effect::If` arm), and add:

```rust
        Condition::LocationHasClues => {
            let has = state
                .investigators
                .get(&controller)
                .and_then(|inv| inv.current_location)
                .and_then(|loc| state.locations.get(&loc))
                .is_some_and(|l| l.clues > 0);
            Ok(has)
        }
```

- [ ] **Step 6: Add the `IntExpr` resolver.** Near `eval_condition`:

```rust
/// Resolve an [`IntExpr`] against current state for `controller`.
pub(crate) fn eval_int_expr(
    state: &GameState,
    controller: InvestigatorId,
    expr: &IntExpr,
) -> Result<i8, String> {
    match expr {
        IntExpr::Lit(n) => Ok(*n),
        IntExpr::Cond { when, then, otherwise } => {
            Ok(if eval_condition(state, controller, when)? { *then } else { *otherwise })
        }
    }
}
```

- [ ] **Step 7: Run + gauntlet.**

Run: `cargo test -p game-core location_has_clues_condition_tracks_clue_count`
Expected: PASS.
Run the full gauntlet (test/clippy/fmt). Expected: green.

- [ ] **Step 8: Commit.**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs
git commit -m "dsl+engine: IntExpr value-expr + Condition::LocationHasClues"
```

---

## Task 6: `InFlightSkillTest.test_modifier` + parameterized Fight follow-up

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs:408,588`
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:28,177,400,592`
- Modify: `crates/game-core/src/engine/dispatch/actions.rs:457` (base Fight passes `extra_damage: 0`)
- Test: `crates/game-core/src/engine/dispatch/skill_test.rs` / engine tests

- [ ] **Step 1: Add `test_modifier`.** In `struct InFlightSkillTest` (after `continuation`, 476):

```rust
    /// A flat modifier applied to the test total, snapshotted by the
    /// initiating effect (`Effect::Fight`'s combat modifier). `0` for
    /// player-action tests, which take their modifiers from cards in
    /// play. Distinct from constant/pending modifiers — this is the
    /// one-shot "+N for this attack" a weapon grants.
    pub test_modifier: i8,
```

- [ ] **Step 2: Add `extra_damage` to the Fight follow-up.** In `SkillTestFollowUp::Fight` (588):

```rust
    Fight {
        /// The enemy the Fight action targeted.
        enemy: EnemyId,
        /// Bonus damage beyond the base 1 (weapons). `0` for a basic Fight.
        extra_damage: u8,
    },
```

- [ ] **Step 3: Add `test_modifier` to `start_skill_test`.** Add a `test_modifier: i8` parameter (skill_test.rs:28) after `source`, set it in the `InFlightSkillTest { … }` literal (83), and update **every** caller (`actions.rs` fight/evade/investigate, the bare `PerformSkillTest`, `Effect::SkillTest` in the evaluator) to pass `0` except the new `Effect::Fight` path (Task 7).

- [ ] **Step 4: Apply it in the total.** In `sum_skill_value` (400) — it already takes `state` — add the in-flight modifier:

```rust
    let test_mod = state
        .in_flight_skill_test
        .as_ref()
        .map_or(0, |t| t.test_modifier);
    base.saturating_add(constant_mod)
        .saturating_add(pending_mod)
        .saturating_add(icon_mod)
        .saturating_add(test_mod)
```

- [ ] **Step 5: Deal `1 + extra_damage` in the follow-up.** In the `SkillTestFollowUp::Fight` arm (592) and the retaliate guard (645), destructure `{ enemy, extra_damage }`; change the damage call:

```rust
        SkillTestFollowUp::Fight { enemy, extra_damage } => {
            super::combat::damage_enemy(cx, enemy, 1 + extra_damage, Some(investigator));
            EngineOutcome::Done
        }
```

In the retaliate guard (`let Some(SkillTestFollowUp::Fight { enemy }) = …`), change the pattern to `Fight { enemy, .. }`.

- [ ] **Step 6: Base Fight passes `0`.** In `actions.rs` `fight` (484), change the follow-up to `SkillTestFollowUp::Fight { enemy: enemy_id, extra_damage: 0 }` and the `start_skill_test` call to pass the new `test_modifier: 0` argument.

- [ ] **Step 7: Regression test — base Fight unchanged.** The existing Fight tests in `engine/mod.rs` (~1858) already assert combat 3 vs fight 3 → 1 damage. Run them:

Run: `cargo test -p game-core fight`
Expected: PASS (unchanged behavior: modifier 0, deals 1).

- [ ] **Step 8: Full gauntlet.** Run test/clippy/fmt. Expected: green (all `start_skill_test` callers + all `SkillTestFollowUp::Fight` matches updated).

- [ ] **Step 9: Commit.**

```bash
git add crates/game-core/src/state/game_state.rs \
        crates/game-core/src/engine/dispatch/skill_test.rs \
        crates/game-core/src/engine/dispatch/actions.rs
git commit -m "game-core: InFlightSkillTest.test_modifier + Fight follow-up extra_damage"
```

---

## Task 7: `Effect::Fight` (builder + evaluator: auto-target, snapshot modifier, start test)

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs:475` (variant + `fight` builder)
- Modify: `crates/game-core/src/engine/evaluator.rs:151` (`apply_effect` arm)
- Test: `crates/cards/tests/` (integration — real registry) + an engine unit test

- [ ] **Step 1: Add the `Effect` variant + builder.** In `enum Effect` (475), beside `PutIntoThreatArea`:

```rust
    /// Initiate a Fight against the single enemy engaged with the
    /// controller, applying a (possibly conditional) combat modifier
    /// for this attack and dealing `1 + extra_damage` on success.
    /// Auto-targets when exactly one enemy is engaged; the activation
    /// check rejects ≠1 engaged before any cost is paid.
    Fight {
        /// Combat modifier for this attack, resolved at eval.
        combat_modifier: IntExpr,
        /// Bonus damage beyond the base 1.
        extra_damage: u8,
    },
```

Builder (near `put_into_threat_area`, ~1001):

```rust
/// Build an [`Effect::Fight`].
#[must_use]
pub fn fight(combat_modifier: IntExpr, extra_damage: u8) -> Effect {
    Effect::Fight { combat_modifier, extra_damage }
}
```

- [ ] **Step 2: Write the failing unit test for `single_engaged_enemy`** (pure helper, added in Step 4). In `crates/game-core/src/engine/dispatch/combat.rs` tests, using the verified `GameStateBuilder` / `test_enemy` fixtures (same as `engine/mod.rs`'s `fight_evade_scenario`):

```rust
#[test]
fn single_engaged_enemy_some_for_one_none_for_zero_or_two() {
    let inv_id = InvestigatorId(1);
    let mut e1 = test_enemy(100, "A");
    e1.engaged_with = Some(inv_id);
    // one engaged → Some
    let s1 = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_enemy(e1.clone())
        .build();
    assert_eq!(single_engaged_enemy(&s1, inv_id), Some(EnemyId(100)));
    // two engaged → None (deferred multi-target)
    let mut e2 = test_enemy(101, "B");
    e2.engaged_with = Some(inv_id);
    let s2 = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_enemy(e1)
        .with_enemy(e2)
        .build();
    assert_eq!(single_engaged_enemy(&s2, inv_id), None);
    // zero engaged → None
    let s0 = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    assert_eq!(single_engaged_enemy(&s0, inv_id), None);
}
```

The end-to-end `Effect::Fight` behavior (suspend → commit → `1 + extra_damage` damage) is exercised through a real activated weapon in the integration test in Step 6, the registry-backed home CLAUDE.md prescribes — not a direct `apply_effect` call, which would need hand-built `Cx` scaffolding.

- [ ] **Step 3: Run — fails (no `single_engaged_enemy`).**

Run: `cargo test -p game-core single_engaged_enemy_some_for_one`
Expected: FAIL — cannot find function.

- [ ] **Step 4: Implement the evaluator arm.** In `apply_effect` (152), add:

```rust
        Effect::Fight { combat_modifier, extra_damage } => {
            let modifier = match eval_int_expr(cx.state, eval_ctx.controller, combat_modifier) {
                Ok(m) => m,
                Err(reason) => return EngineOutcome::Rejected { reason: reason.into() },
            };
            let Some(enemy_id) = single_engaged_enemy(cx.state, eval_ctx.controller) else {
                return EngineOutcome::Rejected {
                    reason: "Effect::Fight: expected exactly one engaged enemy".into(),
                };
            };
            let fight_difficulty = cx.state.enemies.get(&enemy_id).map_or(0, |e| e.fight);
            crate::engine::dispatch::skill_test::start_skill_test(
                cx,
                eval_ctx.controller,
                SkillKind::Combat,
                SkillTestKind::Fight,
                fight_difficulty,
                SkillTestFollowUp::Fight { enemy: enemy_id, extra_damage: *extra_damage },
                None,
                None,
                eval_ctx.source,
                modifier, // test_modifier
            )
        }
```

Add a shared helper (used again by the activation check in Task 8) near the top of `evaluator.rs` or in `combat.rs` — put it in `combat.rs` and `pub(crate)` it so both sites call one implementation:

```rust
/// The single enemy engaged with `investigator`, or `None` if zero or
/// 2+ are engaged (the 2+ case is a deferred interactive choice).
pub(crate) fn single_engaged_enemy(
    state: &GameState,
    investigator: InvestigatorId,
) -> Option<EnemyId> {
    let mut engaged = state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator))
        .map(|(id, _)| *id);
    let first = engaged.next()?;
    if engaged.next().is_some() { None } else { Some(first) }
}
```

- [ ] **Step 5: Run — passes.**

Run: `cargo test -p game-core single_engaged_enemy_some_for_one`
Expected: PASS.

- [ ] **Step 6: End-to-end integration test (registry-backed).** Create `crates/cards/tests/weapon_fight.rs`. Define a synthetic weapon registry following the `crates/scenarios/src/test_fixtures/synth_cards.rs` template: a `_synth_weapon` asset whose `metadata_for` returns `CardKind::Asset { …, uses: Some(Uses { kind: UsesKind::Ammo, count: 4 }) }` and whose `abilities_for` returns `vec![activated(1, vec![Cost::SpendUses(1)], fight(IntExpr::Lit(1), 1))]`. Install it via `game_core::card_registry::install`, put the weapon in play (with one engaged enemy of `fight = 3`, `max_health = 3`, the active investigator at `combat = 3` in the Investigation phase), then:

```rust
// Activate the weapon → suspends at the commit window.
let r1 = apply(state, Action::Player(PlayerAction::ActivateAbility {
    investigator: inv_id, instance_id: weapon_inst, ability_index: 0,
}));
assert!(matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }));
// Ammo was spent up front (4 → 3).
assert_eq!(r1.state.investigators[&inv_id].cards_in_play
    .iter().find(|c| c.instance_id == weapon_inst).unwrap().uses_remaining, Some(3));
// Commit no cards → auto-success token (combat 3 + modifier 1 vs fight 3).
let r2 = apply(r1.state, Action::Player(PlayerAction::ResolveInput {
    response: InputResponse::CommitCards { indices: vec![] },
}));
// Base 1 + extra_damage 1 = 2 damage.
assert_event!(r2.events, Event::EnemyDamaged { amount: 2, .. });
```

(Use the `apply` / chaos-bag / commit helpers the existing `crates/cards/tests/play_card.rs` integration test uses; a deterministic 0-modifier bag makes combat 3 + mod 1 ≥ fight 3 succeed.)

- [ ] **Step 7: Run integration test + gauntlet, then commit.**

Run: `cargo test -p cards --test weapon_fight`
Expected: PASS. Then the full gauntlet (test/clippy/fmt). Expected: green.

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs \
        crates/game-core/src/engine/dispatch/combat.rs crates/cards/tests/weapon_fight.rs
git commit -m "dsl+engine: Effect::Fight — auto-target single enemy, snapshot modifier, bonus damage"
```

---

## Task 8: Validate-first target check in `check_activate_ability`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:938`
- Test: `crates/cards/tests/weapon_fight.rs` (extend Task 7's integration test — registry-backed, the home CLAUDE.md prescribes for activation checks needing real abilities)

- [ ] **Step 1: Write the failing test.** Extend `crates/cards/tests/weapon_fight.rs` (same `_synth_weapon` registry as Task 7). Activating the weapon while the investigator is engaged with **two** enemies must reject **without** charging action or ammo:

```rust
#[test]
fn weapon_fight_rejects_when_two_enemies_engaged() {
    // _synth_weapon in play; active investigator engaged with TWO enemies.
    let before_actions = state.investigators[&inv_id].actions_remaining;
    let r = apply(state, Action::Player(PlayerAction::ActivateAbility {
        investigator: inv_id, instance_id: weapon_inst, ability_index: 0,
    }));
    assert!(matches!(r.outcome, EngineOutcome::Rejected { .. }));
    // Nothing charged: ammo still 4, actions unchanged.
    assert_eq!(r.state.investigators[&inv_id].cards_in_play
        .iter().find(|c| c.instance_id == weapon_inst).unwrap().uses_remaining, Some(4));
    assert_eq!(r.state.investigators[&inv_id].actions_remaining, before_actions);
}
```

Add a companion `weapon_fight_rejects_when_no_enemy_engaged` (0 engaged → reject, nothing charged).

- [ ] **Step 2: Run — fails (no target precondition yet; activation proceeds and charges).**

Run: `cargo test -p cards --test weapon_fight weapon_fight_rejects`
Expected: FAIL (currently the activation suspends/charges instead of rejecting).

- [ ] **Step 3: Add the precondition.** In `check_activate_ability` (938), after the effect is resolved and before returning `Ok(ActivateCheckResult { … })`, add:

```rust
    // Validate-first: a Fight-initiating ability needs exactly one engaged
    // enemy, checked before any action/ammo is charged. 0 → illegal (no
    // target); 2+ → deferred interactive target selection (#212/#213).
    if effect_initiates_fight(&effect)
        && super::combat::single_engaged_enemy(state, investigator).is_none()
    {
        return Err(
            "ActivateAbility: a Fight ability needs exactly one engaged enemy \
             (0 = no target; 2+ multi-target selection deferred with #212/#213)"
                .to_string(),
        );
    }
```

Add the introspection helper in the same module (top-level node suffices for Slice 1; a Fight nested only inside a `Seq`/`If` branch is a documented TODO):

```rust
/// True if `effect` initiates a Fight at its top level. TODO(#212/#213):
/// recurse `Seq`/`If` once a card fights in only one branch — no Slice-1
/// card does (.38 Special's IntExpr branches both fight).
fn effect_initiates_fight(effect: &crate::dsl::Effect) -> bool {
    matches!(effect, crate::dsl::Effect::Fight { .. })
}
```

- [ ] **Step 4: Run — passes.**

Run: `cargo test -p cards --test weapon_fight`
Expected: PASS — the reject tests now pass, and Task 7's happy-path test (exactly one engaged enemy → suspends + spends ammo) still passes, covering the `Ok` branch.

- [ ] **Step 5: Gauntlet + commit.**

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs \
        crates/cards/tests/weapon_fight.rs
git commit -m "game-core: validate-first Fight target check before charging activation"
```

---

## Task 9: Doc-comment sweep + final gauntlet

**Files:**
- Modify: doc-comments on the new public items (warnings-as-errors `cargo doc`).

- [ ] **Step 1: Run the doc job.**

Run: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
Expected: PASS — fix any broken intra-doc links on the new `Uses`, `IntExpr`, `Effect::Fight`, `Cost::SpendUses`, `Event::UsesSpent`, `Condition::LocationHasClues` items.

- [ ] **Step 2: Run the wasm jobs** (the engine compiles to wasm; new code must too).

Run: `cargo build -p web --target wasm32-unknown-unknown`
Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
Expected: PASS.

- [ ] **Step 3: Full gauntlet one more time.**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features` · `cargo clippy --all-targets --all-features -- -D warnings` · `cargo fmt --check`
Expected: all green.

- [ ] **Step 4: Commit anything outstanding** (doc fixes).

```bash
git add -A && git commit -m "engine: doc-comment fixes for weapon-support surface"
```

---

## Self-review notes (coverage vs. #295)

- Ammo/uses: Tasks 1–4 (types → pipeline → instance field → cost). ✅
- `Effect::Fight` + `IntExpr` + `Condition::LocationHasClues`: Tasks 5–7. ✅
- This-test modifier + bonus-damage follow-up: Task 6. ✅
- Validate-first target check (0/2+ engaged): Task 8. ✅
- Base `PlayerAction::Fight` unchanged: Task 6 Step 6–7 (regression). ✅
- Out of scope (multi-target select, nested-branch Fight, `PutIntoThreatArea { clues }`): deferred / lands in C5c PR 2 — not in this plan. ✅

The `.38 Special` and `Cover Up` card impls (C5c #238) ride on this and get their own plan once #295 merges.
