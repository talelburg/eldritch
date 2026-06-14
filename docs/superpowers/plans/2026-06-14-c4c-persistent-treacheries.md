# C4c — Persistent threat-area / attachment treacheries — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement The Gathering's three persistent encounter treacheries (Frozen in Fear 01164, Dissonant Voices 01165, Obscuring Fog 01168), each staying in play, enforcing a constant restriction, and discarding itself at a forced timing point.

**Architecture:** Extend the inspectable DSL (`Stat::Shroud`, `Restriction`, `Effect::Restrict`, `Effect::DiscardSelf`, `SkillTest.on_success`) so the engine reads constant restrictions from `abilities_for` the way `constant_skill_modifier` already does. Add a location attachment zone, derive persistence from "has a non-Revelation ability," thread the firing instance through forced triggers, and track per-round action surcharges per source instance.

**Tech Stack:** Rust workspace (`card-dsl`, `game-core`, `cards`); serde; event-sourced engine with validate-first dispatch handlers.

**Spec:** `docs/superpowers/specs/2026-06-14-phase-7-slice-1-c4c-persistent-treacheries-design.md`

**CI gauntlet (run before push):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

---

## Task 1 — Location attachment zone

**Files:**
- Modify: `crates/game-core/src/state/location.rs` (add `attachments` field + `new` default + serde test)
- Modify: `crates/game-core/src/state/card.rs:62` (`Zone` enum — add `LocationAttachment`)
- Modify: `crates/game-core/src/event.rs` (add `Event::CardAttachedToLocation`)
- Modify: `crates/game-core/src/engine/dispatch/threat_area.rs` (add `attach_to_location`)

- [ ] **Step 1: Add the `attachments` field.** In `location.rs`, add to the `Location` struct (after `connections`):
```rust
    /// Encounter cards attached to this location (e.g. Obscuring Fog
    /// 01168 grants `+2` shroud while attached). Empty for the common
    /// case. Discarded back to the encounter discard via
    /// `Effect::DiscardSelf`.
    pub attachments: Vec<CardInPlay>,
```
Add the import: change `use super::card::CardCode;` to also import `CardInPlay` (`use super::card::{CardCode, CardInPlay};`). Set `attachments: Vec::new()` in `Location::new` and in the two struct literals in the existing `#[cfg(test)]` block.

- [ ] **Step 2: Add the `Zone` variant.** In `card.rs`, after the `ThreatArea` variant:
```rust
    /// A location's attachment zone — encounter cards attached to a
    /// location (Obscuring Fog 01168). Used as the `from` zone when an
    /// attachment is discarded.
    LocationAttachment,
```

- [ ] **Step 3: Add the event.** In `event.rs`, beside `CardEnteredThreatArea`, add (mirror its shape and doc style; `location: LocationId`):
```rust
    /// An encounter card was attached to a location (Obscuring Fog
    /// 01168's Revelation). The mirror of
    /// [`CardEnteredThreatArea`](Event::CardEnteredThreatArea) for the
    /// location attachment zone.
    CardAttachedToLocation {
        /// The location the card attached to.
        location: LocationId,
        /// The printed code of the attached card.
        code: CardCode,
        /// The minted in-play instance id.
        instance_id: CardInstanceId,
    },
```
Ensure `LocationId` is imported in `event.rs` (it is used elsewhere; confirm).

- [ ] **Step 4: Write the failing test for `attach_to_location`.** In `threat_area.rs`'s `#[cfg(test)] mod tests`, add (uses `test_location`):
```rust
    #[test]
    fn attach_mints_id_pushes_to_location_and_emits_event() {
        use crate::test_support::test_location;
        let mut state = GameStateBuilder::new()
            .with_location(test_location(7, "Study"))
            .build();
        let mut events = Vec::new();
        let id = {
            let mut cx = Cx { state: &mut state, events: &mut events };
            attach_to_location(&mut cx, LocationId(7), CardCode::new("01168"))
        };
        assert_eq!(id, Some(CardInstanceId(0)));
        let loc = &state.locations[&LocationId(7)];
        assert_eq!(loc.attachments.len(), 1);
        assert_eq!(loc.attachments[0].code.as_str(), "01168");
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CardAttachedToLocation { code, location, .. }
                if code.as_str() == "01168" && *location == LocationId(7)
        )));
    }
```
Add `LocationId` to the test module imports (`use crate::state::{... , LocationId};`).

- [ ] **Step 5: Run it — verify it fails to compile** (`attach_to_location` undefined).
Run: `cargo test -p game-core attach_mints_id`
Expected: compile error, unresolved `attach_to_location`.

- [ ] **Step 6: Implement `attach_to_location`.** In `threat_area.rs`, after `place_in_threat_area` (mirror it; keep the `#[cfg_attr(not(test), allow(dead_code))]` only until Task 9 wires the first production caller — Obscuring Fog calls it):
```rust
/// Attach `code` to `location` as a fresh in-play instance, minting an
/// instance id and emitting [`Event::CardAttachedToLocation`]. Returns
/// the minted id, or `None` if the location isn't in state.
///
/// **No limit enforcement** — "Limit 1 per location" is printed on
/// specific cards (Obscuring Fog 01168), not a property of all
/// attachments, so the limit lives in the card's Revelation, not here.
pub(super) fn attach_to_location(
    cx: &mut Cx,
    location: LocationId,
    code: CardCode,
) -> Option<CardInstanceId> {
    if !cx.state.locations.contains_key(&location) {
        return None;
    }
    let instance_id = CardInstanceId(cx.state.next_card_instance_id);
    cx.state.next_card_instance_id = cx.state.next_card_instance_id.saturating_add(1);
    let loc = cx
        .state
        .locations
        .get_mut(&location)
        .expect("existence checked above");
    loc.attachments
        .push(CardInPlay::enter_play(code.clone(), instance_id));
    cx.events.push(Event::CardAttachedToLocation {
        location,
        code,
        instance_id,
    });
    Some(instance_id)
}
```
Add `LocationId` to the module's `use crate::state::{...}` line.

- [ ] **Step 7: Run tests.** `cargo test -p game-core threat_area` → all pass (including the new test and the serde roundtrip in `location.rs`).

- [ ] **Step 8: Commit.**
```bash
git add crates/game-core/src/state/location.rs crates/game-core/src/state/card.rs crates/game-core/src/event.rs crates/game-core/src/engine/dispatch/threat_area.rs
git commit -m "engine: location attachment zone + attach_to_location helper"
```

---

## Task 2 — Derived persistence (no suppress flag)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (treachery arm in `resolve_encounter_card`, ~line 124-153)
- Test: same file's test module, or `crates/cards/tests/` if registry needed. Use a unit test with a synthetic registry is hard here; prefer an integration test in `crates/cards/tests/persistent_treachery.rs` once cards exist. For Task 2, add a **focused unit test** by extracting the persistence predicate.

- [ ] **Step 1: Add the persistence predicate (pure, unit-testable).** In `encounter.rs`, above `resolve_encounter_card`:
```rust
/// A treachery is **persistent** (stays in play after its Revelation,
/// owning its own disposition) iff it has at least one ability whose
/// trigger is not `Trigger::Revelation` — the ongoing `Constant`
/// restriction / `OnEvent` forced-discard abilities the three C4c
/// treacheries carry. One-shot treacheries have only a `Revelation`, so
/// they auto-discard after it resolves.
///
/// TODO: assumes every persistent treachery carries an ongoing ability
/// and every one-shot carries none (holds for all Core+Dunwich
/// treacheries). Revisit with an explicit persistence marker only if a
/// treachery must persist with no ongoing ability, or auto-discard
/// despite carrying one.
fn treachery_is_persistent(abilities: &[crate::dsl::Ability]) -> bool {
    abilities
        .iter()
        .any(|a| a.trigger != Trigger::Revelation)
}
```

- [ ] **Step 2: Write the failing unit test.** In `encounter.rs` test module:
```rust
    #[test]
    fn persistence_is_derived_from_non_revelation_abilities() {
        use card_dsl::dsl::{revelation, constant, modify, native, Ability};
        use card_dsl::card_data::{ModifierScope, Stat};
        // one-shot: only Revelation
        let one_shot: Vec<Ability> = vec![revelation(native("x:rev"))];
        assert!(!super::treachery_is_persistent(&one_shot));
        // persistent: has a Constant ability
        let persistent: Vec<Ability> = vec![
            revelation(native("y:rev")),
            constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay)),
        ];
        assert!(super::treachery_is_persistent(&persistent));
    }
```

- [ ] **Step 3: Run it.** `cargo test -p game-core persistence_is_derived` → PASS (predicate already added). (If the test module lacks the imports, add them.)

- [ ] **Step 4: Wire it into the treachery arm.** In `resolve_encounter_card`, replace the unconditional discard at the end of the `CardType::Treachery` arm. Current tail:
```rust
            cx.state.encounter_discard.push(code);
            EngineOutcome::Done
```
becomes:
```rust
            if treachery_is_persistent(&abilities) {
                // Persistent: the card placed itself (threat area /
                // attachment) during its Revelation and owns its own
                // disposition (including the Obscuring Fog limit-1
                // discard). Do not auto-discard.
                EngineOutcome::Done
            } else {
                cx.state.encounter_discard.push(code);
                EngineOutcome::Done
            }
```
Note `abilities` is already bound earlier in the arm (`let abilities = (registry.abilities_for)(&code).unwrap_or_default();`). The suspend path (`pending_revelation_discard`) is unchanged — none of these three test on Revelation, so persistence and suspension never co-occur here.

- [ ] **Step 5: Run the existing encounter tests** to confirm one-shot behavior is preserved.
Run: `cargo test -p game-core encounter`
Expected: PASS (one-shots still discard; nothing persistent in game-core unit tests yet).

- [ ] **Step 6: Commit.**
```bash
git add crates/game-core/src/engine/dispatch/encounter.rs
git commit -m "engine: derive treachery persistence from non-Revelation abilities"
```

---

## Task 3 — `Stat::Shroud` + `effective_shroud`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs:541` (`Stat` enum — add `Shroud`)
- Modify: `crates/game-core/src/engine/evaluator.rs` (add `effective_shroud`)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs:98` (read effective shroud)

- [ ] **Step 1: Add `Stat::Shroud`.** In `dsl.rs`, in `enum Stat`, add `Shroud,` after `MaxSanity`. Update the doc comment to mention "plus location shroud (Obscuring Fog)".

- [ ] **Step 2: Write the failing test for `effective_shroud`.** In `evaluator.rs` test module:
```rust
    #[test]
    fn effective_shroud_adds_attachment_shroud_modifiers() {
        // A location with printed shroud 2 and one attachment carrying
        // a Constant Modify(Shroud, +2) reads as effective shroud 4.
        // Uses a fake registry mapping the attachment code to that
        // ability. (Mirror the registry-install pattern used by other
        // evaluator tests in this module.)
        // ... build state with location shroud 2 + an attachment whose
        //     code resolves to constant(modify(Stat::Shroud, 2, WhileInPlay))
        // assert_eq!(effective_shroud(&state, reg, &location), 4);
    }
```
Implement the test concretely following the nearest existing registry-based test in `evaluator.rs` (search for `install` / `CardRegistry` usage in that module; reuse its fake-registry helper). The attachment goes in `location.attachments`.

- [ ] **Step 3: Run it — fails** (`effective_shroud` undefined).
Run: `cargo test -p game-core effective_shroud`

- [ ] **Step 4: Implement `effective_shroud`.** In `evaluator.rs`, near `unconditional_constant_stat_modifier`:
```rust
/// A location's **effective shroud**: its printed `shroud` plus every
/// `Stat::Shroud` `Modify(WhileInPlay)` on its attachments (Obscuring
/// Fog 01168's `+2`). Clamped to `0` on the low end and `u8::MAX` on the
/// high end. Read by `investigate` in place of the raw printed shroud.
#[must_use]
pub fn effective_shroud(
    state: &GameState,
    registry: &CardRegistry,
    location: &crate::state::Location,
) -> u8 {
    let mut delta: i32 = 0;
    for att in &location.attachments {
        let Some(abilities) = (registry.abilities_for)(&att.code) else {
            continue;
        };
        for ability in &abilities {
            if ability.trigger != Trigger::Constant {
                continue;
            }
            if let Effect::Modify { stat: Stat::Shroud, delta: d, scope: ModifierScope::WhileInPlay } =
                &ability.effect
            {
                delta += i32::from(*d);
            }
        }
    }
    let total = i32::from(location.shroud) + delta;
    u8::try_from(total.clamp(0, i32::from(u8::MAX))).unwrap_or(u8::MAX)
}
```
Confirm `Stat`, `ModifierScope`, `Effect`, `Trigger` are imported in `evaluator.rs` (they are used by the existing constant-modifier code).

- [ ] **Step 5: Read effective shroud in `investigate`.** In `actions.rs`, replace line ~98:
```rust
    let difficulty = i8::try_from(location.shroud).unwrap_or(i8::MAX);
```
with (the registry may be absent in bare unit tests — fall back to printed shroud):
```rust
    let shroud = match crate::card_registry::current() {
        Some(reg) => crate::engine::evaluator::effective_shroud(cx.state, reg, location),
        None => location.shroud,
    };
    let difficulty = i8::try_from(shroud).unwrap_or(i8::MAX);
```
Note: `location` is an immutable borrow here; `effective_shroud` only reads. If the borrow checker complains (because `cx.state` is borrowed via `location`), capture the needed fields first (`let printed = location.shroud; let loc_id = location.id;`) then call `effective_shroud` with a fresh `cx.state.locations[&loc_id]` lookup, or compute shroud before the mutate section. Resolve during implementation.

- [ ] **Step 6: Run tests.** `cargo test -p game-core effective_shroud` and `cargo test -p game-core investigate` → PASS.

- [ ] **Step 7: Commit.**
```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/actions.rs
git commit -m "engine: Stat::Shroud + effective_shroud read by investigate"
```

---

## Task 4 — `Effect::DiscardSelf`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`Effect` enum — add `DiscardSelf`; add `discard_self()` builder)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`apply_effect` arm + `discard_self` fn)

- [ ] **Step 1: Add the variant + builder.** In `dsl.rs`, in `enum Effect`, add:
```rust
    /// Discard the firing card instance (the `source` in
    /// [`EvalContext`](../../game_core/engine/evaluator/struct.EvalContext.html)).
    /// Used by persistent treacheries' `Forced` self-discard abilities.
    /// Locates the instance in a threat area or location attachment and
    /// discards it to the encounter discard.
    DiscardSelf,
```
And a builder near `native`:
```rust
/// Build an [`Effect::DiscardSelf`].
#[must_use]
pub fn discard_self() -> Effect {
    Effect::DiscardSelf
}
```

- [ ] **Step 2: Write the failing test.** In `evaluator.rs` test module:
```rust
    #[test]
    fn discard_self_removes_threat_area_instance_to_encounter_discard() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        // Put an instance into the threat area directly.
        let inst = CardInstanceId(5);
        state.investigators.get_mut(&InvestigatorId(1)).unwrap()
            .threat_area.push(CardInPlay::enter_play(CardCode::new("01165"), inst));
        let mut events = Vec::new();
        let outcome = {
            let mut cx = Cx { state: &mut state, events: &mut events };
            let mut ctx = EvalContext::for_controller(InvestigatorId(1));
            ctx.source = Some(inst);
            apply_effect(&mut cx, &Effect::DiscardSelf, ctx)
        };
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.investigators[&InvestigatorId(1)].threat_area.is_empty());
        assert_eq!(state.encounter_discard, vec![CardCode::new("01165")]);
        assert!(events.iter().any(|e| matches!(
            e, Event::CardDiscarded { from: Zone::ThreatArea, code, .. } if code.as_str() == "01165"
        )));
    }

    #[test]
    fn discard_self_removes_location_attachment_to_encounter_discard() {
        let mut state = GameStateBuilder::new()
            .with_location(test_location(3, "Study"))
            .build();
        let inst = CardInstanceId(9);
        state.locations.get_mut(&LocationId(3)).unwrap()
            .attachments.push(CardInPlay::enter_play(CardCode::new("01168"), inst));
        let mut events = Vec::new();
        let outcome = {
            let mut cx = Cx { state: &mut state, events: &mut events };
            let mut ctx = EvalContext::for_controller(InvestigatorId(1));
            ctx.source = Some(inst);
            apply_effect(&mut cx, &Effect::DiscardSelf, ctx)
        };
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.locations[&LocationId(3)].attachments.is_empty());
        assert_eq!(state.encounter_discard, vec![CardCode::new("01168")]);
        assert!(events.iter().any(|e| matches!(
            e, Event::CardDiscarded { from: Zone::LocationAttachment, code, .. } if code.as_str() == "01168"
        )));
    }

    #[test]
    fn discard_self_rejects_without_source() {
        let mut state = GameStateBuilder::new().with_investigator(test_investigator(1)).build();
        let mut events = Vec::new();
        let mut cx = Cx { state: &mut state, events: &mut events };
        let outcome = apply_effect(&mut cx, &Effect::DiscardSelf, EvalContext::for_controller(InvestigatorId(1)));
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    }
```
Add the needed imports to the test module (`CardInPlay`, `CardCode`, `CardInstanceId`, `LocationId`, `Zone`, `test_location`, `test_investigator`).

- [ ] **Step 3: Run — fails** (no `DiscardSelf` arm).
Run: `cargo test -p game-core discard_self`

- [ ] **Step 4: Implement the arm + helper.** In `apply_effect`, add:
```rust
        Effect::DiscardSelf => discard_self(cx, &eval_ctx),
```
And the helper:
```rust
/// Resolve [`Effect::DiscardSelf`]: remove `eval_ctx.source` from
/// whichever threat area or location attachment holds it, push its code
/// to `encounter_discard`, and emit `CardDiscarded` with the matching
/// `from` zone. Rejects loudly if there is no source or the instance is
/// not found.
///
/// TODO: scoped to the two encounter zones (threat area / location
/// attachment → encounter discard). Extend to player-controlled zones
/// (cards_in_play → owner discard) when a player card first needs to
/// discard itself by source instance.
fn discard_self(cx: &mut Cx, eval_ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = eval_ctx.source else {
        return EngineOutcome::Rejected {
            reason: "DiscardSelf: no source instance in context".into(),
        };
    };
    // Threat areas.
    for (inv_id, inv) in cx.state.investigators.iter_mut() {
        if let Some(pos) = inv.threat_area.iter().position(|c| c.instance_id == source) {
            let card = inv.threat_area.remove(pos);
            cx.state.encounter_discard.push(card.code.clone());
            cx.events.push(Event::CardDiscarded {
                investigator: *inv_id,
                code: card.code,
                from: Zone::ThreatArea,
            });
            return EngineOutcome::Done;
        }
    }
    // Location attachments.
    for loc in cx.state.locations.values_mut() {
        if let Some(pos) = loc.attachments.iter().position(|c| c.instance_id == source) {
            let card = loc.attachments.remove(pos);
            cx.state.encounter_discard.push(card.code.clone());
            // CardDiscarded carries `investigator`; for a location
            // attachment use the controller as the bookkeeping owner.
            cx.events.push(Event::CardDiscarded {
                investigator: eval_ctx.controller,
                code: card.code,
                from: Zone::LocationAttachment,
            });
            return EngineOutcome::Done;
        }
    }
    EngineOutcome::Rejected {
        reason: format!("DiscardSelf: source instance {source:?} not found in any threat area or location attachment").into(),
    }
}
```
Note: `Event::CardDiscarded`'s exact fields (`investigator`, `code`, `from`) — confirm against `event.rs:336`. If the borrow of `cx.state.investigators.iter_mut()` conflicts with `cx.events.push`, collect the removal then push after the loop (capture `inv_id`/`code` in locals). Resolve during implementation.

- [ ] **Step 5: Run tests.** `cargo test -p game-core discard_self` → PASS.

- [ ] **Step 6: Commit.**
```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: Effect::DiscardSelf — source-instance-driven self-discard"
```

---

## Task 5 — Forced-trigger source threading + scan extensions

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`ForcedHit.source`, `resolve_one`, `RoundEnded`/`AfterLocationInvestigated` arms, `push_matching` signature)

- [ ] **Step 1: Add `source` to `ForcedHit`.** Change the struct to:
```rust
struct ForcedHit {
    code: CardCode,
    ability_index: usize,
    controller: InvestigatorId,
    source: Option<CardInstanceId>,
}
```
Add `use crate::state::CardInstanceId;` to the imports.

- [ ] **Step 2: Thread `source` into `push_matching`.** Add a `source: Option<CardInstanceId>` parameter and set it in the pushed `ForcedHit`. Update every `push_matching(...)` call: board scans (act/agenda in `PhaseEnded`/`ActAdvanced`/`AgendaAdvanced`/`EnemyDefeated`/`RoundEnded`-board, and `EnteredLocation`) pass `None`; instance scans pass `Some(card.instance_id)`.

- [ ] **Step 3: `resolve_one` uses the source.** Replace the final line:
```rust
    apply_effect(cx, &effect, EvalContext::for_controller(hit.controller))
```
with:
```rust
    let ctx = match hit.source {
        Some(src) => EvalContext::for_controller_with_source(hit.controller, src),
        None => EvalContext::for_controller(hit.controller),
    };
    apply_effect(cx, &effect, ctx)
```

- [ ] **Step 4: `EndOfTurn` / `AfterLocationInvestigated` bind source.** In both arms, the `for card in inv.controlled_card_instances()` loop already iterates instances — change `push_matching(reg, &card.code, *investigator, &mut hits, |p| ...)` to pass `Some(card.instance_id)` as the new source arg.

- [ ] **Step 5: Extend `AfterLocationInvestigated` to scan location attachments.** After the controlled-instances loop in that arm, add:
```rust
            if let Some(loc) = state.locations.get(_location) {
                for att in &loc.attachments {
                    push_matching(reg, &att.code, *investigator, Some(att.instance_id), &mut hits, |p| {
                        matches!(p, EventPattern::AfterLocationInvestigated)
                    });
                }
            }
```
Rename the `location: _location` binding to `location` so it's usable (drop the underscore; update the doc note that it's now read).

- [ ] **Step 6: Extend `RoundEnded` to scan threat areas.** After the act/agenda board scans in the `RoundEnded` arm, add (binding controller = the instance's owning investigator, source = the instance):
```rust
            for (inv_id, inv) in &state.investigators {
                for card in inv.controlled_card_instances() {
                    push_matching(reg, &card.code, *inv_id, Some(card.instance_id), &mut hits, |p| {
                        matches!(p, EventPattern::RoundEnded)
                    });
                }
            }
```

- [ ] **Step 7: Write a unit test** that an `EndOfTurn` forced ability on a threat-area card resolves with the source threaded. In `forced_triggers.rs` test module (or extend an existing C4a test): install a fake registry mapping a synthetic code to `on_event(EndOfTurn, After, discard_self())`, put that instance in an investigator's threat area, fire `ForcedTriggerPoint::EndOfTurn`, assert the instance is discarded (proving source reached `DiscardSelf`). Follow the registry-install pattern already in this module's tests.

- [ ] **Step 8: Run.** `cargo test -p game-core forced` → PASS.

- [ ] **Step 9: Commit.**
```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs
git commit -m "engine: thread firing instance through forced triggers + scan threat areas/attachments"
```

---

## Task 6 — Obscuring Fog (01168)

**Files:**
- Create: `crates/cards/src/impls/treachery_01168.rs`
- Modify: `crates/cards/src/impls/mod.rs` (register module + wire into `abilities_for` / `native_effect_for` / metadata as siblings do)
- Test: `crates/cards/tests/persistent_treachery.rs` (new integration test, installs `cards::REGISTRY`)

- [ ] **Step 1: Confirm the card text** in `data/arkhamdb-snapshot/pack/core/core_encounter.json` (01168): Revelation attach to your location; Limit 1 per location; +2 shroud; Forced after attached location successfully investigated → discard. (Already verified in spec.)

- [ ] **Step 2: Write the card impl.** Model on `treachery_01167.rs` for the native pattern. The placement native does limit-1:
```rust
//! Obscuring Fog (The Gathering treachery, 01168).
//!
//! ```text
//! Revelation - Attach to your location. Limit 1 per location.
//! Attached location gets +2 shroud.
//! Forced - After attached location is successfully investigated:
//!   Discard Obscuring Fog.
//! ```
use card_dsl::card_data::{ModifierScope, Stat};
use card_dsl::dsl::{
    constant, discard_self, modify, native, on_event, revelation, Ability, EventPattern, EventTiming,
};
use game_core::card_registry::NativeEffectFn;
use game_core::state::{CardCode, Zone};
use game_core::{attach_to_location_pub, Cx, EngineOutcome, EvalContext, Event};

pub const CODE: &str = "01168";
const LIMIT1_ATTACH: &str = "01168:limit1-attach";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(native(LIMIT1_ATTACH)),
        constant(modify(Stat::Shroud, 2, ModifierScope::WhileInPlay)),
        on_event(EventPattern::AfterLocationInvestigated, EventTiming::After, discard_self()),
    ]
}

pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == LIMIT1_ATTACH).then_some(limit1_attach as NativeEffectFn)
}

fn limit1_attach(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(loc_id) = cx
        .state
        .investigators
        .get(&ctx.controller)
        .and_then(|inv| inv.current_location)
    else {
        return EngineOutcome::Rejected {
            reason: "01168 limit1-attach: controller has no location".into(),
        };
    };
    // Limit 1 per location (printed): if an Obscuring Fog is already
    // attached here, this copy is discarded to the encounter discard
    // instead of attaching (a treachery that cannot enter play is
    // discarded).
    let already = cx.state.locations.get(&loc_id).is_some_and(|loc| {
        loc.attachments.iter().any(|c| c.code.as_str() == CODE)
    });
    if already {
        cx.state.encounter_discard.push(CardCode::new(CODE));
        cx.events.push(Event::CardDiscarded {
            investigator: ctx.controller,
            code: CardCode::new(CODE),
            from: Zone::LocationAttachment,
        });
        return EngineOutcome::Done;
    }
    game_core::attach_to_location_pub(cx, loc_id, CardCode::new(CODE));
    EngineOutcome::Done
}
```
**Note:** `attach_to_location` is `pub(super)` in `threat_area.rs`. Expose a thin `pub` re-export from `game-core` (e.g. `pub fn attach_to_location_pub(...)` in `lib.rs` delegating, or make the helper `pub` and re-export) so the `cards` crate can call it — mirror how `place_in_threat_area` is exposed for Dissonant Voices/Frozen in Fear (do this once in Task 6 and reuse). Match the existing public-surface convention (`spawn_set_aside_enemy`, `take_damage`, `place_doom_on_current_agenda` are precedents).

- [ ] **Step 3: Register the module.** In `crates/cards/src/impls/mod.rs`, add `pub mod treachery_01168;` and add its arms to whatever dispatch the crate uses for `abilities_for` / `native_effect_for` (grep for `treachery_01167` to find every site and mirror them).

- [ ] **Step 4: Write the integration test.** `crates/cards/tests/persistent_treachery.rs`:
```rust
//! C4c persistent-treachery integration tests (needs real card metadata
//! + abilities, so it installs cards::REGISTRY in its own process).
use game_core::card_registry;
// ... build a game with one investigator at a location (shroud 2),
//     reveal 01168 (or call resolve_encounter_card), assert:
//   - 01168 is attached to the investigator's location
//   - effective_shroud at that location is 4
//   - revealing a 2nd 01168 discards it (limit 1) — only one attachment
//   - firing AfterLocationInvestigated discards the attachment
fn install() { let _ = card_registry::install(cards::REGISTRY); }
```
Flesh out following `crates/cards/tests/play_card.rs` for the setup/registry/idiom. Assert attach, effective shroud 4, limit-1, and forced discard.

- [ ] **Step 5: Run — fails, then passes after impl/registration.**
Run: `cargo test -p cards --test persistent_treachery`
Also run the card's own unit test: `cargo test -p cards treachery_01168`.

- [ ] **Step 6: Commit.**
```bash
git add crates/cards/src/impls/treachery_01168.rs crates/cards/src/impls/mod.rs crates/cards/tests/persistent_treachery.rs crates/game-core/src/lib.rs crates/game-core/src/engine/dispatch/threat_area.rs
git commit -m "card: Obscuring Fog (01168) — location attachment + shroud + forced discard"
```

---

## Task 7 — `Restriction::CannotPlay` + `play_is_prohibited`, then Dissonant Voices (01165)

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`Restriction` enum, `Effect::Restrict`, `restrict()` builder)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`play_is_prohibited`; `Effect::Restrict` arm in `apply_effect` is a **no-op constant marker** — see step)
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` or wherever `play_card` validates (grep `fn play_card`)
- Create: `crates/cards/src/impls/treachery_01165.rs`; register in `mod.rs`; extend `persistent_treachery.rs`

- [ ] **Step 1: Add the DSL.** In `dsl.rs`:
```rust
/// A constant prohibition or cost increase a card imposes while in play.
/// Read by the engine at the relevant decision point (play / action
/// cost). Carried by a [`Trigger::Constant`] ability via
/// [`Effect::Restrict`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Restriction {
    /// The controller cannot play cards of this type (Dissonant Voices
    /// 01165: assets and events — one `CannotPlay` per type).
    CannotPlay(crate::card_data::CardType),
    /// Performing one of `actions` costs `1` additional action. When
    /// `first_each_round` is set, only the first such action each round
    /// is surcharged (Frozen in Fear 01164).
    ///
    /// TODO: the `first_each_round` gate also appears on non-cost
    /// mechanisms (suppressing attacks of opportunity on the first
    /// action each round; a forced trigger on the first move each turn).
    /// Promote it to a shared "first-applicable each round/turn" scope
    /// spanning constant modifiers and forced triggers once a second
    /// mechanism needs the same gate — not while action cost is its only
    /// consumer.
    ExtraActionCost {
        /// Which actions are surcharged.
        actions: ActionClassSet,
        /// Gate to the first matching action each round.
        first_each_round: bool,
    },
}

/// The action kinds an [`Restriction::ExtraActionCost`] can surcharge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActionClassSet {
    /// Surcharge the move action.
    pub move_: bool,
    /// Surcharge the fight action.
    pub fight: bool,
    /// Surcharge the evade action.
    pub evade: bool,
}

/// One action kind, for querying [`ActionClassSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    /// The move action.
    Move,
    /// The fight action.
    Fight,
    /// The evade action.
    Evade,
}

impl ActionClassSet {
    /// Whether this set includes `class`.
    #[must_use]
    pub fn contains(self, class: ActionClass) -> bool {
        match class {
            ActionClass::Move => self.move_,
            ActionClass::Fight => self.fight,
            ActionClass::Evade => self.evade,
        }
    }
}
```
Add to `enum Effect`:
```rust
    /// A constant restriction (under [`Trigger::Constant`]). Inert as an
    /// executed effect — the engine *inspects* it at decision points
    /// (see `play_is_prohibited` / `pending_action_surcharge`), it is
    /// never "run".
    Restrict(Restriction),
```
And the builder:
```rust
/// Build an [`Effect::Restrict`].
#[must_use]
pub fn restrict(restriction: Restriction) -> Effect {
    Effect::Restrict(restriction)
}
```

- [ ] **Step 2: `apply_effect` arm for `Restrict` is a no-op.** Constant restrictions are inspected, not executed; if one is ever reached as an executed effect, that's a misuse:
```rust
        Effect::Restrict(_) => EngineOutcome::Rejected {
            reason: "Effect::Restrict is a constant marker, inspected not executed".into(),
        },
```

- [ ] **Step 3: Write the failing test for `play_is_prohibited`.** In `evaluator.rs` test module: build an investigator with a threat-area instance whose code resolves (fake registry) to `constant(restrict(CannotPlay(CardType::Asset)))`; assert `play_is_prohibited(state, reg, inv, CardType::Asset)` is true and `CardType::Event` is false.

- [ ] **Step 4: Implement `play_is_prohibited`.** In `evaluator.rs`:
```rust
/// Whether `investigator` is currently forbidden from playing a card of
/// `card_type` by an active `Restriction::CannotPlay` constant ability on
/// any of their controlled instances (Dissonant Voices 01165).
#[must_use]
pub fn play_is_prohibited(
    state: &GameState,
    registry: &CardRegistry,
    investigator: InvestigatorId,
    card_type: crate::card_data::CardType,
) -> bool {
    let Some(inv) = state.investigators.get(&investigator) else {
        return false;
    };
    inv.controlled_card_instances().any(|c| {
        (registry.abilities_for)(&c.code)
            .into_iter()
            .flatten()
            .any(|a| {
                a.trigger == Trigger::Constant
                    && matches!(&a.effect, Effect::Restrict(Restriction::CannotPlay(t)) if *t == card_type)
            })
    })
}
```

- [ ] **Step 5: Wire into `play_card`.** Grep `fn play_card`. After the card-type is known to be Asset or Event and before the on-play mutation, add a rejection:
```rust
    if let Some(reg) = crate::card_registry::current() {
        if crate::engine::evaluator::play_is_prohibited(cx.state, reg, investigator, card_type) {
            return EngineOutcome::Rejected {
                reason: format!("PlayCard: {investigator:?} cannot play {card_type:?} (a constant restriction forbids it)").into(),
            };
        }
    }
```
Place it among the existing validate-first checks (before any mutation). Confirm the local variable holding the card's type name.

- [ ] **Step 6: Write Dissonant Voices.** `treachery_01165.rs` (CODE `"01165"`), abilities:
```rust
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(native(TO_THREAT_AREA)),
        constant(restrict(Restriction::CannotPlay(CardType::Asset))),
        constant(restrict(Restriction::CannotPlay(CardType::Event))),
        on_event(EventPattern::RoundEnded, EventTiming::After, discard_self()),
    ]
}
```
Placement native `TO_THREAT_AREA` = `"01165:to-threat-area"` calling the public `place_in_threat_area` re-export with `ctx.controller` + `CardCode::new(CODE)`. (Expose `place_in_threat_area` publicly the same way as `attach_to_location` in Task 6 if not already.)

- [ ] **Step 7: Register + extend the integration test.** Add `pub mod treachery_01165;` + dispatch arms in `mod.rs`. In `persistent_treachery.rs`, add a case: reveal 01165 → in threat area; `play_card` of an asset/event rejects; firing `RoundEnded` discards it.

- [ ] **Step 8: Run.** `cargo test -p cards --test persistent_treachery` + `cargo test -p game-core play_is_prohibited` → PASS.

- [ ] **Step 9: Commit.**
```bash
git add -A
git commit -m "card: Dissonant Voices (01165) + Restriction::CannotPlay play gate"
```

---

## Task 8 — `on_success` skill test + per-round action surcharge

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`Effect::SkillTest` gains `on_success`; `skill_test` builder + a `skill_test_full` or extend signature)
- Modify: `crates/game-core/src/state/game_state.rs` (`InFlightSkillTest` gains `on_success` + `source`)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`start_skill_test` signature; `finish_skill_test` success branch)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`Effect::SkillTest` arm passes `on_success` + `eval_ctx.source`)
- Modify: `crates/game-core/src/state/investigator.rs` (`action_surcharge_spent_this_round` field)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (reset the set at round increment)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs` + `combat.rs` (move/fight/evade cost) and a `pending_action_surcharge` helper in `evaluator.rs`

- [ ] **Step 1: `Effect::SkillTest.on_success`.** In `dsl.rs`, change the variant to add `on_success: Option<Box<Effect>>` (beside `on_fail`). Update the `skill_test(skill, difficulty, on_fail)` builder to set `on_success: None`, and add:
```rust
/// Build an [`Effect::SkillTest`] with both success- and failure-side
/// follow-ups (Frozen in Fear 01164 discards itself on success).
#[must_use]
pub fn skill_test_with_success(
    skill: crate::card_data::SkillKind,
    difficulty: u8,
    on_success: Effect,
    on_fail: Effect,
) -> Effect {
    Effect::SkillTest {
        skill,
        difficulty,
        on_success: Some(Box::new(on_success)),
        on_fail: Box::new(on_fail),
    }
}
```
Check `Effect::SkillTest`'s current `on_fail` type — the existing builder boxes it (`native(...)` is `Effect`). Match it. For "no on_fail" (Frozen in Fear has none), provide a no-op: use `Effect::Seq(vec![])` as the empty failure branch (confirm `apply_seq` treats empty as Done), or make `on_fail` itself `Option`. **Decision:** keep `on_fail` as-is; pass `Effect::Seq(vec![])` for Frozen in Fear's empty failure side. Verify `Seq(vec![])` → `Done`.

- [ ] **Step 2: Update the `Effect::SkillTest` destructuring** everywhere it's matched (evaluator arm ~line 188; card test in `treachery_01167.rs` and `treachery_0116x` C4b tests that destructure `Effect::SkillTest { skill, difficulty, on_fail }`). Add `on_success` to each pattern. Run `grep -rn "Effect::SkillTest" crates/` to find them all.

- [ ] **Step 3: `InFlightSkillTest` carries `on_success` + `source`.** Add fields:
```rust
    /// Effect to run **on success** after the chaos token resolves (the
    /// success-side mirror of [`on_fail`](Self::on_fail)). Frozen in Fear
    /// 01164 discards itself on a successful end-of-turn willpower test.
    pub on_success: Option<card_dsl::dsl::Effect>,
    /// The firing card instance, threaded so `on_success`/`on_fail`
    /// eval-contexts can resolve `Effect::DiscardSelf` across the
    /// suspend/resume boundary.
    pub source: Option<crate::state::CardInstanceId>,
```

- [ ] **Step 4: `start_skill_test` signature.** Add `on_success: Option<Effect>` and `source: Option<CardInstanceId>` params after `on_fail`; store them in the `InFlightSkillTest` literal. Update **all callers** (`investigate`, `fight`, `evade`, the `Effect::SkillTest` evaluator arm, any test). Action callers pass `None, None`.

- [ ] **Step 5: Run `on_success` in `finish_skill_test`.** In the success branch (line ~182), after `apply_skill_test_follow_up`, capture `on_success` + `source` from the snapshot (like `on_fail`) and run:
```rust
    if succeeded {
        apply_skill_test_follow_up(cx, investigator, follow_up);
        if let Some(effect) = &on_success {
            let ctx = match source {
                Some(src) => EvalContext::for_controller_with_source(investigator, src),
                None => EvalContext::for_controller(investigator),
            };
            let outcome = apply_effect(cx, effect, ctx);
            debug_assert!(matches!(outcome, EngineOutcome::Done),
                "skill-test on_success must resolve to Done in scope: {outcome:?}");
        }
    } else if let Some(effect) = &on_fail {
        let mut ctx = match source {
            Some(src) => EvalContext::for_controller_with_source(investigator, src),
            None => EvalContext::for_controller(investigator),
        };
        ctx.failed_by = Some(failed_by);
        // ... existing apply_effect + debug_assert
    }
```
Bind `on_success`/`source` locals from `in_flight` next to the existing `on_fail` snapshot (line ~157).

- [ ] **Step 6: Evaluator passes `on_success` + source.** In the `Effect::SkillTest` arm, destructure `on_success` and pass `on_success.as_deref().cloned()` (or `.map(|b| (**b).clone())`) and `eval_ctx.source` into `start_skill_test`.

- [ ] **Step 7: Per-round surcharge field.** In `investigator.rs`, add:
```rust
    /// Source instances whose [`Restriction::ExtraActionCost`] with
    /// `first_each_round` has already surcharged an action this round.
    /// Reset at the round boundary. Keyed by instance so multiple
    /// surcharge sources track independently.
    pub action_surcharge_spent_this_round: std::collections::BTreeSet<CardInstanceId>,
```
Default it in the investigator constructor / `test_investigator` fixture (grep for the struct literal(s) and `Investigator::new`).

- [ ] **Step 8: Reset at round increment.** Grep `phases.rs` for the round-counter increment (the `round_increments_on_mythos_entry_via_driver` test points at it). At that site, for every investigator: `inv.action_surcharge_spent_this_round.clear();`.

- [ ] **Step 9: `pending_action_surcharge` helper.** In `evaluator.rs`:
```rust
/// Extra action cost for `investigator` performing `action_class`, plus
/// the `first_each_round` source instances to mark spent on commit.
/// Sums `Restriction::ExtraActionCost` deltas (1 each) from active
/// constant abilities whose `actions` include `action_class`; a
/// `first_each_round` source already in
/// `action_surcharge_spent_this_round` contributes 0.
#[must_use]
pub fn pending_action_surcharge(
    state: &GameState,
    registry: &CardRegistry,
    investigator: InvestigatorId,
    action_class: crate::dsl::ActionClass,
) -> (u8, Vec<CardInstanceId>) {
    let Some(inv) = state.investigators.get(&investigator) else {
        return (0, Vec::new());
    };
    let mut extra: u8 = 0;
    let mut to_mark = Vec::new();
    for card in inv.controlled_card_instances() {
        let Some(abilities) = (registry.abilities_for)(&card.code) else { continue; };
        for a in &abilities {
            if a.trigger != Trigger::Constant { continue; }
            let Effect::Restrict(Restriction::ExtraActionCost { actions, first_each_round }) = &a.effect
                else { continue; };
            if !actions.contains(action_class) { continue; }
            if *first_each_round {
                if inv.action_surcharge_spent_this_round.contains(&card.instance_id) { continue; }
                to_mark.push(card.instance_id);
            }
            extra = extra.saturating_add(1);
        }
    }
    (extra, to_mark)
}
```

- [ ] **Step 10: Apply surcharge in move/fight/evade.** In each handler (`actions.rs` `move_action`; `combat.rs` fight + evade — grep `fn fight`/`fn evade`), replace the `actions_remaining < 1` check + `spend_one_action` with:
```rust
    let (extra, to_mark) = match crate::card_registry::current() {
        Some(reg) => crate::engine::evaluator::pending_action_surcharge(cx.state, reg, investigator, crate::dsl::ActionClass::Move /* or Fight/Evade */),
        None => (0, Vec::new()),
    };
    let cost = 1u8.saturating_add(extra);
    // ... in the validate block:
    if inv.actions_remaining < cost {
        return EngineOutcome::Rejected { reason: format!("<action> requires {cost} action point(s)").into() };
    }
    // ... in the mutate block, replace spend_one_action with:
    spend_actions(cx, investigator, cost);
    if let Some(inv) = cx.state.investigators.get_mut(&investigator) {
        inv.action_surcharge_spent_this_round.extend(to_mark);
    }
```
Add `spend_actions(cx, inv, n)` near `spend_one_action` in `actions.rs` (generalize it; have `spend_one_action` call `spend_actions(cx, inv, 1)` to stay DRY). Confirm whether `fight`/`evade` currently call `spend_one_action`.

- [ ] **Step 11: Tests.** In `evaluator.rs`: `pending_action_surcharge` charges first move then 0 the second (same `inv` with the instance marked), resets when the set is cleared, and two sources each charge. Add an `on_success` skill-test unit/integration test (success runs the effect). Run `cargo test -p game-core surcharge` and `cargo test -p game-core skill_test`.

- [ ] **Step 12: Commit.**
```bash
git add -A
git commit -m "engine: SkillTest on_success + per-instance per-round action surcharge"
```

---

## Task 9 — Frozen in Fear (01164)

**Files:**
- Create: `crates/cards/src/impls/treachery_01164.rs`; register in `mod.rs`; extend `persistent_treachery.rs`

- [ ] **Step 1: Confirm text** (01164, `core_encounter.json`) — verified in spec.

- [ ] **Step 2: Write the impl.** CODE `"01164"`, native `TO_THREAT_AREA = "01164:to-threat-area"` (calls public `place_in_threat_area`). Abilities:
```rust
pub fn abilities() -> Vec<Ability> {
    let actions = ActionClassSet { move_: true, fight: true, evade: true };
    vec![
        revelation(native(TO_THREAT_AREA)),
        constant(restrict(Restriction::ExtraActionCost { actions, first_each_round: true })),
        on_event(
            EventPattern::EndOfTurn,
            EventTiming::After,
            skill_test_with_success(SkillKind::Willpower, 3, discard_self(), Effect::Seq(vec![])),
        ),
    ]
}
```

- [ ] **Step 3: Register** in `mod.rs` (module + dispatch arms; no `native_effect_for` beyond `TO_THREAT_AREA`).

- [ ] **Step 4: Extend the integration test.** In `persistent_treachery.rs`: reveal 01164 → threat area; first move that round costs 2 actions, second move costs 1; surcharge resets next round; fire `EndOfTurn` and resolve the willpower(3) test — on success the card is discarded, on failure it remains. (Drive the chaos bag / commit window via the harness used by other skill-test integration tests; seed a deterministic token for the success and failure cases.)

- [ ] **Step 5: Run.** `cargo test -p cards --test persistent_treachery` + `cargo test -p cards treachery_01164` → PASS.

- [ ] **Step 6: Commit.**
```bash
git add -A
git commit -m "card: Frozen in Fear (01164) — action surcharge + end-of-turn willpower discard"
```

---

## Task 10 — Full gauntlet + phase doc

- [ ] **Step 1: Run the full CI gauntlet** (all six commands at the top). Fix every warning/lint/doc-link failure.

- [ ] **Step 2: Update the phase doc** `docs/phases/phase-7-the-gathering.md`: flip the C4c row to `✅ PR #NN`, update the Status paragraph ("Next: C5 → C7"), and add a **Decisions made** entry only if load-bearing for future PRs — candidates: derived persistence rule; `Effect::DiscardSelf` source-instance model; the `first_each_round` generalization TODO. Apply the "would a future PR-author choose differently without this entry?" test; keep to 1–2 entries. **Do this as the final commit, after CI is green on the opened PR** (per `feedback_phase_doc_updates`).

- [ ] **Step 3: Commit + open PR** (`gh pr create`, template, `Closes #235.`), watch CI, then update the phase doc as the last commit.

---

## Self-review notes (author)

- **Spec coverage:** Seam 1→T1, Seam 2→T2, Seam 3 (shroud)→T3 / (CannotPlay)→T7 / (ExtraActionCost)→T8, Seam 4→T4, Seam 5→T8, Seam 6→T5, Seam 7→T8; cards→T6/T7/T9. All seams mapped.
- **Cross-task type consistency:** `attach_to_location` (T1) exposed publicly in T6; `discard_self()`/`Effect::DiscardSelf` (T4) used by T5/T6/T7/T9; `ForcedHit.source` (T5) feeds `DiscardSelf` (T4); `pending_action_surcharge`/`ActionClass`/`ActionClassSet` (T8) used by T9; `skill_test_with_success` (T8) used by T9.
- **Known implementation-time confirmations (flagged inline):** exact `Event::CardDiscarded` fields; borrow-checker shuffles in `discard_self`/`effective_shroud`; the round-increment site in `phases.rs`; whether `fight`/`evade` use `spend_one_action`; the `cards` crate's dispatch wiring sites (grep `treachery_01167`); `Effect::Seq(vec![])` ⇒ `Done`.
