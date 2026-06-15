# C5c — Roland's .38 Special + Cover Up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Roland Banks' signature/weakness pair for The Gathering — `.38 Special` (01006) and `Cover Up` (01007) — as card content riding on the weapon-support engine (#295, merged) and the C5a interrupt/game-end machinery.

**Architecture:** Two card impls in `crates/cards/src/impls/`, registered in `impls/mod.rs`. `.38 Special` is a one-ability asset using `Effect::Fight` + `Cost::SpendUses`. `Cover Up` is a persistent treachery porting the C5a synthetic fixture's two native effects + adding a 3-clue threat-area Revelation, which needs a small engine extension: `Effect::PutIntoThreatArea` gains a `clues` field.

**Tech Stack:** Rust — `card-dsl` (the `PutIntoThreatArea` field), `game-core` (evaluator arm), `cards` (the two impls + tests). Issue **#238**.

**Branch:** `card/c5c-roland-signature`. **CI gauntlet** (run before each commit touching code):
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

---

## File Structure

- `crates/card-dsl/src/dsl.rs` — `Effect::PutIntoThreatArea { code, clues }`; `put_into_threat_area_with_clues(code, clues)` builder; existing `put_into_threat_area(code)` keeps `clues: 0`.
- `crates/game-core/src/engine/evaluator.rs` — the `PutIntoThreatArea` arm seeds `clues` on the minted instance.
- `crates/cards/src/impls/treachery_01007.rs` — **new**: Cover Up (Revelation + interrupt + game-end natives).
- `crates/cards/src/impls/roland_38_special.rs` — **new**: .38 Special (one activated Fight ability).
- `crates/cards/src/impls/mod.rs` — register both modules (`pub mod`, `abilities_for` arms, `native_effect_for` chain for Cover Up).
- `crates/cards/tests/weapon_38_special.rs` — **new**: integration test (real registry) for the +3/+1 clue paths.
- `crates/cards/tests/cover_up.rs` — **new**: integration test (real registry) for Revelation placement + interrupt + game-end trauma.

Reference impls to mirror: `crates/cards/src/impls/treachery_01165.rs` (persistent treachery shape), `crates/scenarios/src/test_fixtures/synth_cards.rs` (the Cover Up native bodies to port), `crates/scenarios/tests/cover_up_interrupt.rs` (the C5a test shapes).

---

## Task 1: Extend `Effect::PutIntoThreatArea` with a `clues` field

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (variant ~595, builder ~1045)
- Modify: `crates/game-core/src/engine/evaluator.rs` (arm ~220)
- Test: `crates/card-dsl/src/dsl.rs` + a game-core evaluator test

- [ ] **Step 1: Add the field.** In `Effect::PutIntoThreatArea`:

```rust
    PutIntoThreatArea {
        /// Printed `ArkhamDB` code of the card to place.
        code: String,
        /// Clues to seed on the placed instance ("with 3 clues on it",
        /// Cover Up 01007). `0` for cards that enter clue-less.
        clues: u8,
    },
```

- [ ] **Step 2: Update the builder + add the clue-bearing one.** Replace the existing `put_into_threat_area`:

```rust
/// Build an [`Effect::PutIntoThreatArea`] that enters clue-less.
#[must_use]
pub fn put_into_threat_area(code: impl Into<String>) -> Effect {
    Effect::PutIntoThreatArea { code: code.into(), clues: 0 }
}

/// Build an [`Effect::PutIntoThreatArea`] seeding `clues` on the instance
/// (Cover Up 01007: "Put Cover Up into play in your threat area, with 3
/// clues on it").
#[must_use]
pub fn put_into_threat_area_with_clues(code: impl Into<String>, clues: u8) -> Effect {
    Effect::PutIntoThreatArea { code: code.into(), clues }
}
```

- [ ] **Step 3: Fix the existing callers' pattern matches.** The Dissonant Voices test (`treachery_01165.rs`) matches `Effect::PutIntoThreatArea { code }` — update to `{ code, .. }`. Search:

Run: `grep -rn 'PutIntoThreatArea {' crates --include='*.rs' | grep -v '\.\.'`
Fix each non-`..` match/literal (the builder bodies above, and any test pattern) to include `clues`.

- [ ] **Step 4: Seed clues in the evaluator arm.** `place_in_threat_area` returns `Option<CardInstanceId>`; set the clue count on the minted instance:

```rust
        Effect::PutIntoThreatArea { code, clues } => {
            let inst = crate::engine::dispatch::threat_area::place_in_threat_area(
                cx,
                eval_ctx.controller,
                crate::state::CardCode::new(code.clone()),
            );
            if *clues > 0 {
                if let Some(id) = inst {
                    if let Some(card) = cx
                        .state
                        .investigators
                        .get_mut(&eval_ctx.controller)
                        .and_then(|inv| inv.threat_area.iter_mut().find(|c| c.instance_id == id))
                    {
                        card.clues = *clues;
                    }
                }
            }
            EngineOutcome::Done
        }
```

- [ ] **Step 5: Test the clue seeding.** In `evaluator.rs` tests, add a test that applying `put_into_threat_area_with_clues("01007", 3)` for a controller leaves a threat-area instance with `clues == 3`. Use the `GameStateBuilder` + `apply_effect` pattern the other evaluator tests use (build a `Cx`, call `apply_effect`, inspect `state.investigators[&id].threat_area`).

- [ ] **Step 6: Gauntlet + commit.**

Run the full gauntlet. Expected: green (all `PutIntoThreatArea` callers updated).

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs
git commit -m "dsl+engine: PutIntoThreatArea seeds clues on the placed instance"
```

---

## Task 2: `.38 Special` (01006) impl

**Files:**
- Create: `crates/cards/src/impls/roland_38_special.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`pub mod` + `abilities_for` arm)
- Test: in-module unit test + `crates/cards/tests/weapon_38_special.rs`

Card text (verified `data/arkhamdb-snapshot/pack/core/core.json`, 2026-06-15):
`Uses (4 ammo). [action] Spend 1 ammo: Fight. You get +1 [combat] for this attack (if there are 1 or more clues on your location, you get +3 [combat], instead). This attack deals +1 damage.`

- [ ] **Step 1: Write the module with an ability-shape unit test first.** Create `roland_38_special.rs`:

```rust
//! Roland's .38 Special (Roland Banks signature asset, 01006).
//!
//! ```text
//! Roland Banks deck only.
//! Uses (4 ammo).
//! [action] Spend 1 ammo: Fight. You get +1 [combat] for this attack
//! (if there are 1 or more clues on your location, you get +3 [combat],
//! instead). This attack deals +1 damage.
//! ```
//!
//! Ammo (4) comes from the corpus (`CardKind::Asset.uses`, pipeline-
//! parsed); the ability spends 1 per use via `Cost::SpendUses` and fights
//! through `Effect::Fight`, whose combat modifier is `+3` when the
//! investigator's location holds a clue, `+1` otherwise (`IntExpr::cond`),
//! dealing `1 + 1` damage on success.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{activated, fight, Ability, Condition, Cost, IntExpr};

/// `ArkhamDB` code for Roland's .38 Special.
pub const CODE: &str = "01006";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Ammo,
            count: 1,
        }],
        fight(IntExpr::cond(Condition::LocationHasClues, 3, 1), 1),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn one_activated_fight_ability_spending_ammo() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert_eq!(
            abilities[0].costs,
            vec![Cost::SpendUses {
                kind: UseKind::Ammo,
                count: 1
            }]
        );
        let Effect::Fight {
            combat_modifier,
            extra_damage,
        } = &abilities[0].effect
        else {
            panic!("expected Effect::Fight");
        };
        assert_eq!(*extra_damage, 1);
        assert_eq!(
            *combat_modifier,
            IntExpr::cond(Condition::LocationHasClues, 3, 1)
        );
    }
}
```

- [ ] **Step 2: Run the unit test.**

Run: `cargo test -p cards --lib one_activated_fight_ability_spending_ammo` (or the impls path)
Expected: PASS once registered (Step 3) — module must be reachable.

- [ ] **Step 3: Register in `impls/mod.rs`.** Add `pub mod roland_38_special;` (alphabetical-ish, near `roland_banks`) and an `abilities_for` arm:

```rust
        roland_38_special::CODE => Some(roland_38_special::abilities()),
```

Match the exact dispatch style already in `abilities_for` (it dispatches `code` against each module's `CODE`).

- [ ] **Step 4: Integration test — the clue-conditional end to end.** Create `crates/cards/tests/weapon_38_special.rs`, modelled on `crates/game-core/tests/weapon_fight.rs` but installing the **real** registry (`cards::REGISTRY`) and using code `"01006"`. Put a `.38 Special` in play with `uses = {Ammo: 4}` (seeded — either play it from hand or set directly), the investigator at a location, engaged with one enemy (fight 3, health 3), combat 3, a `Numeric(0)` chaos bag. Two cases:

```rust
// Clue on the location → +3 combat: total 3 + 3 = 6 vs fight 3, deals 2.
// (No clue → +1: total 4 vs 3, still hits here; assert the modifier via
//  the in-flight test or pick an enemy fight value that distinguishes —
//  e.g. fight 5: +3 path (8) succeeds, +1 path (4) fails.)
```

Use **fight 5** so the two modifier branches are observable: with a clue (+3 → total 8 ≥ 5) the enemy takes 2 damage; without (+1 → total 4 < 5) the Fight fails and deals 0. Assert `EnemyDamaged { amount: 2, .. }` in the clue case and `SkillTestFailed` / no `EnemyDamaged` in the no-clue case. Assert ammo decremented 4 → 3 in both (ammo is spent on activation regardless of hit).

- [ ] **Step 5: Run integration test + gauntlet, commit.**

Run: `cargo test -p cards --test weapon_38_special`
Expected: PASS. Then full gauntlet.

```bash
git add crates/cards/src/impls/roland_38_special.rs crates/cards/src/impls/mod.rs \
        crates/cards/tests/weapon_38_special.rs
git commit -m "card: Roland's .38 Special 01006 — clue-conditional weapon-fight"
```

---

## Task 3: `Cover Up` (01007) impl

**Files:**
- Create: `crates/cards/src/impls/treachery_01007.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`pub mod` + `abilities_for` arm + `native_effect_for` chain)
- Test: in-module unit test + `crates/cards/tests/cover_up.rs`

Card text (verified, 2026-06-15):
`Revelation - Put Cover Up into play in your threat area, with 3 clues on it. [reaction] When you would discover 1 or more clues at your location: Discard that many clues from Cover Up instead. Forced - When the game ends, if there are any clues on Cover Up: You suffer 1 mental trauma.`

- [ ] **Step 1: Write the module.** Port the two native bodies from `synth_cards.rs` (`synth_cover_up_discard`, `synth_cover_up_trauma`) verbatim except the tag constants. Create `treachery_01007.rs`:

```rust
//! Cover Up (Roland Banks signature weakness, 01007).
//!
//! ```text
//! Revelation - Put Cover Up into play in your threat area, with 3 clues
//!   on it.
//! [reaction] When you would discover 1 or more clues at your location:
//!   Discard that many clues from Cover Up instead.
//! Forced - When the game ends, if there are any clues on Cover Up:
//!   You suffer 1 mental trauma.
//! ```
//!
//! Persistent treachery: the Revelation self-places into the threat area
//! with 3 clues (`Effect::PutIntoThreatArea`), so `resolve_encounter_card`
//! does not auto-discard it. The before-timing clue interrupt and the
//! game-end forced trauma ride the C5a seam (`WouldDiscoverClues` /
//! `GameEnd`), backed by the two native effects below.

use card_dsl::dsl::{
    native, on_event, put_into_threat_area_with_clues, revelation, Ability, EventPattern,
    EventTiming,
};
use game_core::card_registry::NativeEffectFn;
use game_core::engine::{Cx, EngineOutcome, EvalContext};
use game_core::event::{Event, TraumaKind};

/// `ArkhamDB` code for Cover Up.
pub const CODE: &str = "01007";

/// Native tag: discard the replaced clue count from Cover Up.
const DISCARD_TAG: &str = "01007:discard_clues";
/// Native tag: suffer 1 mental trauma at game end if clues remain.
const TRAUMA_TAG: &str = "01007:trauma";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(put_into_threat_area_with_clues(CODE, 3)),
        on_event(
            EventPattern::WouldDiscoverClues,
            EventTiming::Before,
            native(DISCARD_TAG),
        ),
        on_event(EventPattern::GameEnd, EventTiming::After, native(TRAUMA_TAG)),
    ]
}

#[must_use]
pub fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        DISCARD_TAG => Some(discard_clues),
        TRAUMA_TAG => Some(trauma),
        _ => None,
    }
}

/// "Discard that many clues from Cover Up instead" — discard the replaced
/// count (threaded via `clue_discovery_count`) from the firing instance.
fn discard_clues(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    debug_assert!(
        ctx.clue_discovery_count.is_some(),
        "cover_up discard: clue_discovery_count not threaded"
    );
    let count = ctx.clue_discovery_count.unwrap_or(0);
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "cover_up discard: no source instance".into(),
        };
    };
    if let Some(inv) = cx.state.investigators.get_mut(&ctx.controller) {
        for card in inv.threat_area.iter_mut().chain(inv.cards_in_play.iter_mut()) {
            if card.instance_id == source {
                let take = count.min(card.clues);
                card.clues -= take;
                break;
            }
        }
    }
    EngineOutcome::Done
}

/// "When the game ends, if there are any clues on Cover Up: You suffer 1
/// mental trauma."
fn trauma(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "cover_up trauma: no source instance".into(),
        };
    };
    let has_clues = cx
        .state
        .investigators
        .get(&ctx.controller)
        .is_some_and(|inv| {
            inv.controlled_card_instances()
                .any(|c| c.instance_id == source && c.clues > 0)
        });
    if has_clues {
        cx.events.push(Event::TraumaSuffered {
            investigator: ctx.controller,
            kind: TraumaKind::Mental,
            amount: 1,
        });
    }
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn revelation_places_with_three_clues_plus_interrupt_and_gameend() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 3);
        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert!(matches!(
            &abilities[0].effect,
            Effect::PutIntoThreatArea { code, clues: 3 } if code == CODE
        ));
        assert!(matches!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::WouldDiscoverClues,
                timing: EventTiming::Before,
            }
        ));
        assert!(matches!(
            abilities[2].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::GameEnd,
                ..
            }
        ));
    }

    #[test]
    fn native_tags_resolve() {
        assert!(native_effect_for(DISCARD_TAG).is_some());
        assert!(native_effect_for(TRAUMA_TAG).is_some());
        assert!(native_effect_for("nope").is_none());
    }
}
```

(Confirm `native`, `on_event`, `revelation`, `EventPattern`, `EventTiming` import paths against `treachery_01165.rs` / `synth_cards.rs`; `controlled_card_instances` and `EvalContext` fields against `synth_cards.rs`.)

- [ ] **Step 2: Register in `impls/mod.rs`.** Add `pub mod treachery_01007;`, an `abilities_for` arm (`treachery_01007::CODE => Some(treachery_01007::abilities())`), and chain its `native_effect_for` into the module's `native_effect_for` (`.or_else(|| treachery_01007::native_effect_for(tag))`).

- [ ] **Step 3: Run the unit tests.**

Run: `cargo test -p cards --lib treachery_01007`
Expected: PASS.

- [ ] **Step 4: Integration test.** Create `crates/cards/tests/cover_up.rs`, modelled on `crates/scenarios/tests/cover_up_interrupt.rs` but installing `cards::REGISTRY` and using code `"01007"`. Cover:
  - **Revelation placement**: resolve the encounter card (`resolve_encounter_card` path, the way the C4c persistent-treachery integration test drives it) → assert a `01007` instance is in the threat area with `clues == 3` and it was **not** auto-discarded.
  - **Interrupt**: with Cover Up holding clues, an Investigate that would discover at the location offers the interrupt; `Confirm` discards from Cover Up instead (location/investigator clues unchanged); `Skip` discovers normally. (Mirror `confirm_replaces_discovery_with_discard_from_cover_up` / `skip_discovers_normally`.)
  - **Game-end trauma**: at scenario resolution, `TraumaSuffered { Mental, 1 }` fires iff Cover Up holds clues. (Mirror `game_end_emits_trauma_when_cover_up_has_clues` / `..._empty`.)

  Reuse the `resolve_encounter_card` / `AdvanceAct`-to-resolution helpers from the C4c (`persistent_treachery.rs`) and C5a (`cover_up_interrupt.rs`) integration tests; the only change is the registry + the real code.

- [ ] **Step 5: Run integration test + gauntlet, commit.**

Run: `cargo test -p cards --test cover_up`
Expected: PASS. Then full gauntlet.

```bash
git add crates/cards/src/impls/treachery_01007.rs crates/cards/src/impls/mod.rs \
        crates/cards/tests/cover_up.rs
git commit -m "card: Cover Up 01007 — 3-clue threat-area Revelation + interrupt + game-end trauma"
```

---

## Task 4: Doc/wasm sweep + phase doc

- [ ] **Step 1: Full CI gauntlet** — test, clippy, fmt, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, `cargo build -p web --target wasm32-unknown-unknown`, `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`. Fix any doc-link nits on the new public items.

- [ ] **Step 2: (after PR is open + CI green) phase doc** — flip C5c (#238) to `✅ PR #NN` in the Group C breakdown, update the Status "Next" line (C5c → C5d/C6…), and add a Decisions entry only if load-bearing (likely none — the prereq decision already covers weapons; Cover Up is a straight port). Commit as the final commit.

---

## Self-review notes (coverage vs. spec)

- `.38 Special`: Task 2 — clue-conditional +3/+1 + ammo, via `Effect::Fight` + `Cost::SpendUses`. ✅
- `Cover Up`: Task 3 — 3-clue Revelation (Task 1's `PutIntoThreatArea { clues }`) + interrupt + game-end trauma, ported from the synth fixture. ✅
- `PutIntoThreatArea { clues }` engine extension: Task 1 (spec puts it in C5c, not the prereq). ✅
- Card tests + integration tests: Tasks 2–3. ✅
- Out of scope (per spec): multi-enemy targeting, trauma persistence — unchanged from PR 1.
