# Asset Slot Limits + Discard-to-Make-Room Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enforce asset slot limits on `PlayCard`, implementing the RR p.19 "discard occupying asset(s) to make room" rule with interactive player choice.

**Architecture:** Slots are already in card metadata (`CardKind::Asset.slots`). Add a slot-capacity table and deficit math (pure helpers), a `need > cap` reject in `check_play_card`, and a make-room step at the asset-enters-play moment (`dispose_play_from_hand`). When a slot is full the player discards occupying asset(s) — auto-resolved when forced, otherwise an interactive `PickSingle` per discard, mirroring the existing soak `DamageAssignment` driver.

**Tech Stack:** Rust (workspace crates `card-dsl`, `game-core`, `cards`), event-sourced engine, continuation-stack control flow.

## Global Constraints

- Verify card text/rules against ArkhamDB or the vendored snapshot — never paraphrase from memory. Card codes used here are verified: Beat Cop `01018` (Ally), Guard Dog `01021` (Ally), Machete `01020`/Knife `01086`/Flashlight `01087`/.45 Automatic `01016` (single Hand), Holy Rosary `01059` (Accessory), Magnifying Glass `01030` (Hand).
- RR p.19 slot defaults (verbatim): "1 accessory slot · 1 body slot · 1 ally slot · 2 hand slots · 2 arcane slots". A full slot does **not** block the play; the player "must choose and discard other assets under his or her control simultaneously with the new asset entering the slot."
- Validate-first / mutate-second: handlers check every precondition and return `Rejected` with state+events unchanged before any mutation.
- No silent approximation: a `need > cap` card rejects loudly (unreachable in the current corpus, but no silent no-op).
- Registry-free engine unit tests must keep passing: slot helpers that read the registry are no-ops when no registry is installed.
- Match CI's strict flags before pushing (see Final Task).
- Never hand-edit `crates/cards/src/generated/cards.rs` (generated).
- The design doc is `docs/superpowers/specs/2026-06-29-asset-slot-limits-design.md`.

---

### Task 1: Slot metadata accessor + pure slot math

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs` (add `Ord`/`PartialOrd` to `Slot`; add `CardMetadata::slots()`)
- Create: `crates/game-core/src/engine/dispatch/slots.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (register the module)

**Interfaces:**
- Produces (card-dsl): `CardMetadata::slots(&self) -> &[Slot]`
- Produces (game-core `dispatch::slots`):
  - `default_slot_capacity(slot: Slot) -> u8`
  - `type SlotCounts = std::collections::BTreeMap<Slot, u8>`
  - `count_slots(slots: &[Slot]) -> SlotCounts`
  - `deficit_from(occupied: &SlotCounts, need: &SlotCounts) -> SlotCounts`
  - `slot_need_exceeds_capacity(need: &SlotCounts) -> Option<Slot>`

- [ ] **Step 1: Add `Ord`/`PartialOrd` to `Slot`**

In `crates/card-dsl/src/card_data.rs`, the `Slot` enum derive (currently around line 57) becomes:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Slot {
    Hand,
    Accessory,
    Ally,
    Arcane,
    Body,
    Tarot,
}
```

(`Ord` lets `Slot` key a `BTreeMap` for deterministic iteration.)

- [ ] **Step 2: Add `CardMetadata::slots()` accessor**

In `crates/card-dsl/src/card_data.rs`, next to `play_cost()` / `is_fast()` on `impl CardMetadata`, add:

```rust
/// The equipment slots this card occupies while in play (Rules Reference
/// p.19). Only `Asset` cards carry slots; every other kind occupies none
/// (the empty slice). A slot-less asset (`Vec::new()`) also returns empty
/// — there is no limit on slot-less assets in play.
#[must_use]
pub fn slots(&self) -> &[Slot] {
    match &self.kind {
        CardKind::Asset { slots, .. } => slots,
        CardKind::Investigator { .. }
        | CardKind::Event { .. }
        | CardKind::Skill { .. }
        | CardKind::Enemy { .. }
        | CardKind::Treachery { .. }
        | CardKind::Location { .. }
        | CardKind::Act { .. }
        | CardKind::Agenda { .. } => &[],
    }
}
```

- [ ] **Step 3: Write the failing test for `slots()`**

In `crates/card-dsl/src/card_data.rs`, in the existing `#[cfg(test)] mod is_fast_tests` (or a new `mod slots_tests`), add:

```rust
#[test]
fn slots_reads_asset_slots_and_empty_elsewhere() {
    let two_handed = CardMetadata {
        code: "x".into(),
        name: "X".into(),
        text: None,
        traits: vec![],
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Guardian,
            cost: Some(5),
            xp: Some(4),
            slots: vec![Slot::Hand, Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    };
    assert_eq!(two_handed.slots(), &[Slot::Hand, Slot::Hand]);

    let event = CardMetadata {
        code: "y".into(),
        name: "Y".into(),
        text: None,
        traits: vec![],
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Seeker,
            cost: Some(1),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            play_only_during_turn: false,
        },
    };
    assert!(event.slots().is_empty());
}
```

- [ ] **Step 4: Run the card-dsl test to verify it fails, then passes**

Run: `cargo test -p card-dsl slots_reads_asset_slots -- --nocapture`
Expected: compiles and PASSES (the accessor was added in Step 2). If it fails to compile, fix the accessor/derive.

- [ ] **Step 5: Create `dispatch/slots.rs` with the pure helpers**

Create `crates/game-core/src/engine/dispatch/slots.rs`:

```rust
//! Asset slot limits (Rules Reference p.19, #498).
//!
//! Slots cap how many asset cards of a given type an investigator may have in
//! play. A full slot does **not** block a play: per RR the player "must choose
//! and discard other assets under his or her control simultaneously with the new
//! asset entering the slot." This module owns the capacity table, the deficit
//! math, and the interactive make-room driver invoked when an asset enters play.

use std::collections::BTreeMap;

use crate::card_data::Slot;

/// Per-type slot counts (a multiset). `BTreeMap` keeps iteration deterministic.
pub(super) type SlotCounts = BTreeMap<Slot, u8>;

/// The slots normally available to an investigator (Rules Reference p.19):
/// "1 accessory slot · 1 body slot · 1 ally slot · 2 hand slots · 2 arcane
/// slots". `Tarot` is not in the original Core Rules Reference (a later-product
/// slot) and no Core/Dunwich card uses it; we default it to 1 and treat it as
/// unreachable in scope.
///
/// TODO: slot-modifying cards (grant/remove a slot) — none in Core/Dunwich.
/// When the first lands, this becomes a per-investigator query reading their
/// in-play modifiers rather than a flat default.
pub(super) fn default_slot_capacity(slot: Slot) -> u8 {
    match slot {
        Slot::Accessory | Slot::Body | Slot::Ally | Slot::Tarot => 1,
        Slot::Hand | Slot::Arcane => 2,
    }
}

/// Tally a slot multiset (e.g. a two-handed weapon → `{Hand: 2}`).
pub(super) fn count_slots(slots: &[Slot]) -> SlotCounts {
    let mut counts = SlotCounts::new();
    for &slot in slots {
        *counts.entry(slot).or_insert(0) += 1;
    }
    counts
}

/// For each slot type the new card needs: `max(0, occupied + need - capacity)`.
/// Only types with a positive deficit are present in the result.
pub(super) fn deficit_from(occupied: &SlotCounts, need: &SlotCounts) -> SlotCounts {
    let mut deficit = SlotCounts::new();
    for (&slot, &n) in need {
        let cap = default_slot_capacity(slot);
        let occ = occupied.get(&slot).copied().unwrap_or(0);
        let d = occ.saturating_add(n).saturating_sub(cap);
        if d > 0 {
            deficit.insert(slot, d);
        }
    }
    deficit
}

/// The first slot type the card needs more of than the investigator has capacity
/// for — i.e. the play is unsatisfiable even after discarding every occupying
/// asset. `None` when every `need[T] <= cap[T]`. Unreachable in the current
/// corpus (max need is `Hand×2` = cap 2); exists for no-silent-approximation.
pub(super) fn slot_need_exceeds_capacity(need: &SlotCounts) -> Option<Slot> {
    need.iter()
        .find(|(&slot, &n)| n > default_slot_capacity(slot))
        .map(|(&slot, _)| slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_matches_rr_defaults() {
        assert_eq!(default_slot_capacity(Slot::Accessory), 1);
        assert_eq!(default_slot_capacity(Slot::Body), 1);
        assert_eq!(default_slot_capacity(Slot::Ally), 1);
        assert_eq!(default_slot_capacity(Slot::Hand), 2);
        assert_eq!(default_slot_capacity(Slot::Arcane), 2);
        assert_eq!(default_slot_capacity(Slot::Tarot), 1);
    }

    #[test]
    fn count_slots_tallies_multiset() {
        assert!(count_slots(&[]).is_empty());
        assert_eq!(count_slots(&[Slot::Ally]).get(&Slot::Ally), Some(&1));
        assert_eq!(
            count_slots(&[Slot::Hand, Slot::Hand]).get(&Slot::Hand),
            Some(&2)
        );
    }

    #[test]
    fn deficit_zero_when_room_exists() {
        // Ally cap 1, none occupied, need 1 → fits.
        let occ = count_slots(&[]);
        let need = count_slots(&[Slot::Ally]);
        assert!(deficit_from(&occ, &need).is_empty());
        // Hand cap 2, one occupied, need 1 → fits.
        let occ = count_slots(&[Slot::Hand]);
        let need = count_slots(&[Slot::Hand]);
        assert!(deficit_from(&occ, &need).is_empty());
    }

    #[test]
    fn deficit_one_when_cap_one_slot_full() {
        // Ally cap 1, one occupied, need 1 → deficit Ally:1.
        let occ = count_slots(&[Slot::Ally]);
        let need = count_slots(&[Slot::Ally]);
        let d = deficit_from(&occ, &need);
        assert_eq!(d.get(&Slot::Ally), Some(&1));
    }

    #[test]
    fn deficit_for_two_handed_over_full_hands() {
        // Hand cap 2, two occupied, need 2 (two-handed weapon) → deficit Hand:2.
        let occ = count_slots(&[Slot::Hand, Slot::Hand]);
        let need = count_slots(&[Slot::Hand, Slot::Hand]);
        assert_eq!(deficit_from(&occ, &need).get(&Slot::Hand), Some(&2));
        // Hand cap 2, two occupied, need 1 → deficit Hand:1.
        let need_one = count_slots(&[Slot::Hand]);
        assert_eq!(deficit_from(&occ, &need_one).get(&Slot::Hand), Some(&1));
    }

    #[test]
    fn need_exceeds_capacity_detects_overflow() {
        // need Hand:2 == cap 2 → satisfiable.
        assert!(slot_need_exceeds_capacity(&count_slots(&[Slot::Hand, Slot::Hand])).is_none());
        // need Ally:2 > cap 1 → unsatisfiable.
        assert_eq!(
            slot_need_exceeds_capacity(&count_slots(&[Slot::Ally, Slot::Ally])),
            Some(Slot::Ally)
        );
    }
}
```

- [ ] **Step 6: Register the module**

In `crates/game-core/src/engine/dispatch/mod.rs`, add `mod slots;` alongside the other `mod` declarations (e.g. near `mod cards;`, `mod combat;`). Keep alphabetical order if the file uses it.

- [ ] **Step 7: Run the slots unit tests**

Run: `cargo test -p game-core slots::tests`
Expected: all 6 tests PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/card-dsl/src/card_data.rs crates/game-core/src/engine/dispatch/slots.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: slot-capacity table + deficit math (#498)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 2: Consolidate the in-play-asset discard helper (#119 nudge)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (move the helper here)
- Modify: `crates/game-core/src/engine/dispatch/abilities.rs` (remove its copy, call the moved one)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (`defeat_overflowed_assets` uses the helper)

**Interfaces:**
- Produces: `cards::discard_card_from_play(cx: &mut Cx, investigator: InvestigatorId, instance_id: CardInstanceId)` (moved, `pub(in crate::engine)`)
- Consumes: nothing new.

This is a behavior-preserving refactor — the existing tests are the safety net (no new test).

- [ ] **Step 1: Move `discard_card_from_play` into `cards.rs`**

Cut the function from `crates/game-core/src/engine/dispatch/abilities.rs` (the `pub(super) fn discard_card_from_play(...)` block, including its doc-comment) and paste it into `crates/game-core/src/engine/dispatch/cards.rs` (near `discard_random_from_hand`). Change visibility to `pub(in crate::engine)` and keep the body identical:

```rust
/// Discard `instance_id` from `investigator`'s `cards_in_play` to their discard
/// pile, emitting [`Event::CardDiscarded`] `{ from: Zone::InPlay }`. Shared by
/// [`Cost::DiscardSelf`](crate::dsl::Cost::DiscardSelf) payment, uses-depletion
/// auto-discard, soak-defeat asset removal, and slot make-room (#498/#119). A
/// missing instance is a state-corruption invariant violation (callers locate it
/// first).
pub(in crate::engine) fn discard_card_from_play(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
) {
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("discard_card_from_play: investigator present");
    let pos = inv
        .cards_in_play
        .iter()
        .position(|c| c.instance_id == instance_id)
        .unwrap_or_else(|| {
            unreachable!("discard_card_from_play: instance {instance_id:?} not in cards_in_play")
        });
    let card = inv.cards_in_play.remove(pos);
    inv.discard.push(card.code.clone());
    cx.events.push(Event::CardDiscarded {
        investigator,
        code: card.code,
        from: crate::state::Zone::InPlay,
    });
}
```

Add `use crate::state::CardInstanceId;` to `cards.rs` if not already imported.

- [ ] **Step 2: Update the `abilities.rs` callers**

In `crates/game-core/src/engine/dispatch/abilities.rs`, the two call sites (around lines 267 and 270) become `super::cards::discard_card_from_play(cx, investigator, instance_id)`. Remove the now-deleted local function. Ensure `CardInstanceId` is still imported there (it is used in the surrounding signatures).

- [ ] **Step 3: Refactor `defeat_overflowed_assets` to use the helper**

In `crates/game-core/src/engine/dispatch/combat.rs`, replace the inline remove/push/emit in the `for (inst, code) in defeated` loop (around lines 256-277) with a call to the helper. The new loop body:

```rust
for (inst, _code) in defeated {
    // RR p.7: a defeated asset goes to its owner's discard pile.
    super::cards::discard_card_from_play(cx, investigator, inst);
}
```

The helper locates the instance and emits the same `CardDiscarded { from: Zone::InPlay }` event, so behavior is preserved. (The `_code` is no longer needed; you may change `defeated` to collect just `CardInstanceId` if the code field becomes unused — confirm with the compiler.)

- [ ] **Step 4: Run the affected suites to verify no behavior change**

Run: `cargo test -p game-core combat`
Run: `cargo test -p cards --test guard_dog_soak --test soak_distribution --test non_attack_soak`
Expected: all PASS (these exercise asset defeat/discard via soak).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/cards.rs crates/game-core/src/engine/dispatch/abilities.rs crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: consolidate in-play-asset discard helper into cards (#119)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 3: Extract `enter_asset_into_play` from `dispose_play_from_hand`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/cards.rs`

**Interfaces:**
- Produces: `cards::enter_asset_into_play(cx: &mut Cx, investigator: InvestigatorId, hand_index: u8)` — removes the asset from hand at `hand_index`, mints + seeds its in-play instance, pushes it to `cards_in_play`, and emits the `EnteredPlay` timing event (discarding the emit outcome, exactly as today; the surrounding `drive` loop opens any after-enters-play window).
- Consumes: nothing new.

Behavior-preserving refactor — existing `play_card.rs` tests are the safety net (no new test).

- [ ] **Step 1: Add `enter_asset_into_play`**

In `crates/game-core/src/engine/dispatch/cards.rs`, add (near `dispose_play_from_hand`):

```rust
/// Move an asset from `investigator`'s hand at `hand_index` into play: mint +
/// seed its in-play instance, push it to `cards_in_play`, and announce it via the
/// `EnteredPlay` timing event. The emit outcome is intentionally discarded — the
/// frame driving this call is already popped, so the `drive` loop opens any
/// after-enters-play reaction window (Research Librarian 01032) itself. Shared by
/// `dispose_play_from_hand` (no slot conflict) and the slot make-room path
/// (#498).
pub(in crate::engine) fn enter_asset_into_play(
    cx: &mut Cx,
    investigator: InvestigatorId,
    hand_index: u8,
) {
    let played = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("enter_asset_into_play: investigator present")
        .hand
        .remove(usize::from(hand_index));
    let in_play = super::threat_area::new_in_play_instance(cx, played);
    let instance = in_play.instance_id;
    cx.state
        .investigators
        .get_mut(&investigator)
        .expect("enter_asset_into_play: investigator present")
        .cards_in_play
        .push(in_play);
    let _ = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::EnteredPlay {
            instance,
            controller: investigator,
        },
    );
}
```

- [ ] **Step 2: Call it from `dispose_play_from_hand`**

In `dispose_play_from_hand`, replace the body of the `super::PlayDestination::InPlay => { ... }` arm (the remove-from-hand → mint → push → emit block) with:

```rust
super::PlayDestination::InPlay => {
    enter_asset_into_play(cx, investigator, hand_index);
}
```

Leave the `Discard` arm and the trailing `EngineOutcome::Done` unchanged.

- [ ] **Step 3: Run the play_card integration tests**

Run: `cargo test -p cards --test play_card`
Expected: all PASS — in particular `asset_play_enters_play_through_the_frame`, `play_holy_rosary_emits_card_played_and_lands_in_play`, `two_copies_of_magnifying_glass_get_distinct_instance_ids`, `research_librarian` (run `--test research_librarian` too).

Run: `cargo test -p cards --test research_librarian`
Expected: PASS (the after-enters-play window still opens).

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/cards.rs
git commit -m "engine: extract enter_asset_into_play from dispose_play_from_hand (#498)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 4: Reject `need > cap` in `check_play_card`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/slots.rs` (registry-reading wrapper)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`check_play_card`)
- Test: `crates/cards/tests/play_card.rs`

**Interfaces:**
- Produces: `slots::card_slot_need(code: &CardCode) -> SlotCounts` and `slots::unsatisfiable_slot(code: &CardCode) -> Option<Slot>` (registry-reading; empty / `None` when no registry or unknown code).
- Consumes: `slots::{count_slots, slot_need_exceeds_capacity}` (Task 1).

- [ ] **Step 1: Add the registry-reading need helpers to `slots.rs`**

In `crates/game-core/src/engine/dispatch/slots.rs`, add:

```rust
use crate::card_registry;
use crate::state::CardCode;

/// The slot multiset `code` needs to enter play, read from the installed
/// registry. Empty when no registry is installed (registry-free engine unit
/// tests), the code is unknown, or it is a non-asset / slot-less asset.
pub(super) fn card_slot_need(code: &CardCode) -> SlotCounts {
    card_registry::current()
        .and_then(|reg| (reg.metadata_for)(code))
        .map(|meta| count_slots(meta.slots()))
        .unwrap_or_default()
}

/// The slot type `code` needs more of than the investigator has capacity for, or
/// `None` if it can fit (possibly after discarding occupiers). See
/// [`slot_need_exceeds_capacity`]. Empty-need cards (slot-less, non-asset,
/// registry-free) always return `None`.
pub(super) fn unsatisfiable_slot(code: &CardCode) -> Option<Slot> {
    slot_need_exceeds_capacity(&card_slot_need(code))
}
```

- [ ] **Step 2: Wire the reject into `check_play_card`**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, inside `check_play_card`, after `card_type` is known to be an asset and before the timing gate (a good spot is right after `check_event_play_changes_state(...)?;`, near line 1384), add:

```rust
// RR p.19 slots (#498): reject only when the card needs more of a slot type
// than the investigator has capacity for — unsatisfiable even after discarding
// every occupying asset. A merely-full slot is NOT rejected here; the play
// proceeds and discards occupiers to make room at enter-play time. Unreachable
// in the current corpus (max need is Hand×2 = cap 2); no silent no-op.
if card_type == CardType::Asset {
    if let Some(slot) = super::slots::unsatisfiable_slot(&code) {
        return Err(format!(
            "PlayCard: {code} needs more {slot:?} slots than the investigator has \
             (slot capacity exceeded; RR p.19)."
        )
        .into());
    }
}
```

Confirm `CardType` is already imported in this file (it is — used throughout).

- [ ] **Step 3: Write the boundary integration test (need == cap still plays)**

In `crates/cards/tests/play_card.rs`, add a constant and a test. Machete `01020` is single-Hand; to reach `need == cap` for Hand (2), play a two-handed weapon. The only implemented two-handed weapon may not exist yet — instead assert the boundary via two single-Hand assets filling both slots, which is the existing `two_copies_of_magnifying_glass_get_distinct_instance_ids` test (already passing). Add an explicit assertion that a single-Hand asset is NOT rejected when no Hand slot is occupied:

```rust
#[test]
fn play_single_hand_asset_is_not_slot_rejected_on_empty_hands() {
    // Sanity: the new slot gate must not over-reject a normal single-slot asset
    // with both Hand slots free.
    let (state, id, _loc) = play_state(vec![MACHETE]); // single Hand slot
    state_with_resources(state, id, 10);
    let (state, id, _loc) = play_state(vec![MACHETE]);
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.investigators[&id].cards_in_play.len(), 1);
}
```

(If `MACHETE`/`state_with_resources` are not yet defined helpers, `MACHETE` already exists at the bottom of the file; drop the `state_with_resources` line — `play_state` gives default resources and Machete cost 3 is affordable by `test_investigator`'s default. Verify by running.)

- [ ] **Step 4: Run the test and the existing play_card suite**

Run: `cargo test -p cards --test play_card`
Expected: all PASS, including the new `play_single_hand_asset_is_not_slot_rejected_on_empty_hands` and the unchanged `two_copies_of_magnifying_glass_get_distinct_instance_ids` (need==cap for Hand).

Run: `cargo test -p game-core slots::tests`
Expected: PASS (helpers unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/slots.rs crates/game-core/src/engine/dispatch/reaction_windows.rs crates/cards/tests/play_card.rs
git commit -m "engine: reject playing an asset whose slot need exceeds capacity (#498)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 5: Auto make-room (single-candidate auto-discard) — the core bug fix

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/slots.rs` (occupancy + the make-room driver)
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (`dispose_play_from_hand` calls the driver)
- Test: `crates/cards/tests/asset_slots.rs` (new integration test file)

**Interfaces:**
- Produces:
  - `slots::occupied_slots(state: &GameState, investigator: InvestigatorId) -> SlotCounts`
  - `slots::slot_deficit(state: &GameState, investigator: InvestigatorId, code: &CardCode) -> SlotCounts`
  - `slots::make_room_candidates(state, investigator, deficit: &SlotCounts) -> Vec<(CardInstanceId, CardCode)>`
  - `slots::enter_asset_making_room(cx, investigator, hand_index: u8, code: &CardCode) -> EngineOutcome`
- Consumes: `cards::{enter_asset_into_play, discard_card_from_play}` (Tasks 2, 3); `card_slot_need` (Task 4).

- [ ] **Step 1: Add occupancy, deficit, candidates, and the driver to `slots.rs`**

Add to `crates/game-core/src/engine/dispatch/slots.rs` (add imports `use crate::state::{CardInstanceId, GameState, InvestigatorId}; use crate::engine::outcome::EngineOutcome; use super::Cx;`):

```rust
/// Slots occupied by `investigator`'s in-play assets. The investigator card is
/// deliberately not in `cards_in_play`, so it is correctly excluded; slot-less
/// and non-asset in-play cards contribute nothing. Empty when no registry is
/// installed.
pub(super) fn occupied_slots(state: &GameState, investigator: InvestigatorId) -> SlotCounts {
    let mut occ = SlotCounts::new();
    let Some(reg) = card_registry::current() else {
        return occ;
    };
    let Some(inv) = state.investigators.get(&investigator) else {
        return occ;
    };
    for card in &inv.cards_in_play {
        if let Some(meta) = (reg.metadata_for)(&card.code) {
            for &slot in meta.slots() {
                *occ.entry(slot).or_insert(0) += 1;
            }
        }
    }
    occ
}

/// Per-type shortfall for playing `code` now: `max(0, occupied + need - cap)`.
/// Empty when the asset fits without discarding (or registry-free / unknown).
pub(super) fn slot_deficit(
    state: &GameState,
    investigator: InvestigatorId,
    code: &CardCode,
) -> SlotCounts {
    let need = card_slot_need(code);
    if need.is_empty() {
        return SlotCounts::new();
    }
    deficit_from(&occupied_slots(state, investigator), &need)
}

/// `investigator`'s in-play assets occupying at least one slot type currently in
/// `deficit` — the assets eligible to be discarded to make room. Returned in
/// `cards_in_play` order so an `OptionId` index is stable between the prompt and
/// its resume.
pub(super) fn make_room_candidates(
    state: &GameState,
    investigator: InvestigatorId,
    deficit: &SlotCounts,
) -> Vec<(CardInstanceId, CardCode)> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    let Some(inv) = state.investigators.get(&investigator) else {
        return Vec::new();
    };
    inv.cards_in_play
        .iter()
        .filter_map(|card| {
            let meta = (reg.metadata_for)(&card.code)?;
            let occupies_deficit = meta.slots().iter().any(|s| deficit.contains_key(s));
            occupies_deficit.then(|| (card.instance_id, card.code.clone()))
        })
        .collect()
}

/// Bring the asset `code` (in `investigator`'s hand at `hand_index`) into play,
/// discarding occupying assets to make room per RR p.19 (#498). Recursive:
///
/// - no deficit → enter directly;
/// - a deficit with exactly one candidate → auto-discard it (forced) and recurse;
/// - a deficit with 2+ candidates → (Task 6) suspend for a player `PickSingle`.
///
/// `check_play_card`'s `need <= cap` gate guarantees a candidate exists whenever
/// a deficit does (occupied[T] >= deficit[T] > 0), so the recursion makes
/// progress and terminates.
pub(super) fn enter_asset_making_room(
    cx: &mut Cx,
    investigator: InvestigatorId,
    hand_index: u8,
    code: &CardCode,
) -> EngineOutcome {
    let deficit = slot_deficit(cx.state, investigator, code);
    if deficit.is_empty() {
        super::cards::enter_asset_into_play(cx, investigator, hand_index);
        return EngineOutcome::Done;
    }
    let candidates = make_room_candidates(cx.state, investigator, &deficit);
    debug_assert!(
        !candidates.is_empty(),
        "slot deficit with no candidate to discard — check_play_card's need<=cap \
         gate should make this unreachable (code {code}, deficit {deficit:?})"
    );
    // Task 6 inserts the 2+-candidate interactive suspend here.
    let (inst, _) = candidates[0];
    super::cards::discard_card_from_play(cx, investigator, inst);
    enter_asset_making_room(cx, investigator, hand_index, code)
}
```

- [ ] **Step 2: Route `dispose_play_from_hand`'s InPlay branch through the driver**

In `crates/game-core/src/engine/dispatch/cards.rs`, change the `dispose_play_from_hand` InPlay arm (from Task 3) so the function returns the driver's outcome. Restructure the tail of `dispose_play_from_hand`:

```rust
    match destination {
        super::PlayDestination::Discard => {
            flush_pending_played_event(cx);
            EngineOutcome::Done
        }
        super::PlayDestination::InPlay => {
            super::slots::enter_asset_making_room(cx, investigator, hand_index, &code)
        }
    }
}
```

Remove the trailing standalone `EngineOutcome::Done` (each arm now returns its own outcome). `code` and `hand_index` are already bound earlier in the function (the `PlayFromHand` destructure). Confirm the function signature still returns `EngineOutcome`.

- [ ] **Step 3: Write the failing core test (Beat Cop → Guard Dog)**

Create `crates/cards/tests/asset_slots.rs`:

```rust
//! Asset slot limits + discard-to-make-room (#498), against the real corpus.
//!
//! Mirrors the `play_card.rs` harness: a process-global registry install and a
//! one-investigator mid-investigation state.

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, Phase, Zone};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, InputResponse, LocationId, PlayerAction, TurnAction};

const BEAT_COP: &str = "01018"; // Guardian Ally
const GUARD_DOG: &str = "01021"; // Guardian Ally
const MACHETE: &str = "01020"; // single Hand
const KNIFE: &str = "01086"; // single Hand
const FLASHLIGHT: &str = "01087"; // single Hand

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// A one-investigator scenario, mid-investigation, with `hand` in hand, plenty
/// of resources and actions.
fn play_state(hand: Vec<&str>) -> (game_core::GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.resources = 20;
    inv.actions_remaining = 6;
    inv.hand = hand.into_iter().map(CardCode::new).collect();

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_location(test_location(101, "Study"))
        .build();
    (state, id)
}

fn play(state: game_core::GameState, id: InvestigatorId) -> game_core::ApplyResult {
    dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    )
}

#[test]
fn playing_a_second_ally_auto_discards_the_first() {
    let (state, id) = play_state(vec![BEAT_COP, GUARD_DOG]);

    // Beat Cop enters (Ally slot now full).
    let r1 = play(state, id);
    assert_eq!(r1.outcome, EngineOutcome::Done);
    assert_eq!(r1.state.investigators[&id].cards_in_play.len(), 1);
    assert_eq!(
        r1.state.investigators[&id].cards_in_play[0].code,
        CardCode::new(BEAT_COP)
    );

    // Guard Dog (the only card left in hand, index 0) — Ally slot full, single
    // candidate (Beat Cop) → auto-discard Beat Cop, Guard Dog enters.
    let r2 = play(r1.state, id);
    assert_eq!(r2.outcome, EngineOutcome::Done);
    let inv = &r2.state.investigators[&id];
    assert_eq!(
        inv.cards_in_play.len(),
        1,
        "only one Ally remains in play: {:?}",
        inv.cards_in_play
    );
    assert_eq!(inv.cards_in_play[0].code, CardCode::new(GUARD_DOG));
    assert_eq!(
        inv.discard,
        vec![CardCode::new(BEAT_COP)],
        "the displaced Ally went to discard"
    );

    // The displaced Beat Cop emitted CardDiscarded { from: InPlay }; Guard Dog
    // emitted EnteredPlay-side CardPlayed earlier. Assert the make-room discard.
    assert!(
        r2.events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { code, from: Zone::InPlay, investigator }
                if *investigator == id && code.as_str() == BEAT_COP
        )),
        "Beat Cop discarded from play: {:?}",
        r2.events
    );
}

#[test]
fn two_handed_weapon_auto_frees_both_hand_slots() {
    // Two single-Hand weapons fill both Hand slots; playing another single-Hand
    // weapon needs to free 1 → auto-discards the first candidate (Task 5 has no
    // player choice yet; Task 6 makes the multi-candidate case interactive, so
    // this test is REPLACED in Task 6 — keep it minimal here).
    let (state, id) = play_state(vec![MACHETE, KNIFE]);
    let r1 = play(state, id);
    let r2 = play(r1.state, id);
    assert_eq!(r2.state.investigators[&id].cards_in_play.len(), 2);
    assert!(KNIFE.is_empty() == false); // keep imports used
    let _ = FLASHLIGHT;
}
```

Note: the `two_handed_weapon_auto_frees_both_hand_slots` test is a placeholder kept tiny because Task 6 converts the 2+-candidate path to interactive — it is rewritten in Task 6 Step 5. Its only job in Task 5 is to confirm two single-Hand assets coexist (cap 2). If the trailing `assert!`/`let _` lines trip clippy, delete them and instead `assert_eq!` on `cards_in_play.len()` only.

- [ ] **Step 4: Run the new test**

Run: `cargo test -p cards --test asset_slots`
Expected: `playing_a_second_ally_auto_discards_the_first` PASS; `two_handed_weapon_auto_frees_both_hand_slots` PASS.

- [ ] **Step 5: Run the broader play suites for regressions**

Run: `cargo test -p cards --test play_card --test beat_cop --test guard_dog_soak`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/slots.rs crates/game-core/src/engine/dispatch/cards.rs crates/cards/tests/asset_slots.rs
git commit -m "engine: auto-discard to make room for a slot-conflicting asset (#498)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 6: Interactive make-room (player chooses which asset to discard)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`Continuation::SlotDiscard` variant)
- Modify: `crates/game-core/src/engine/dispatch/slots.rs` (suspend branch + prompt + resume)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` arm)
- Test: `crates/cards/tests/asset_slots.rs`

**Interfaces:**
- Produces: `Continuation::SlotDiscard { investigator: InvestigatorId, code: CardCode, hand_index: u8 }`; `slots::resume_slot_discard(cx, response: &InputResponse) -> EngineOutcome`.
- Consumes: `slots::{enter_asset_making_room, make_room_candidates, slot_deficit}` (Task 5); `hunters::candidate_options`, `InputRequest::pick_single`, `ResumeToken` (existing).

- [ ] **Step 1: Add the `SlotDiscard` continuation variant**

In `crates/game-core/src/state/game_state.rs`, add a variant to `enum Continuation` (near `PlayFromHand`):

```rust
/// A slot-conflicting asset play paused for the player to choose which
/// occupying asset to discard to make room (RR p.19, #498). Pushed by
/// `slots::enter_asset_making_room` when 2+ co-controlled assets occupy a
/// slot type the new asset needs; the pending asset stays in
/// `investigator`'s hand at `hand_index` until the deficit is cleared, then
/// enters play. Resumed by `slots::resume_slot_discard` via a
/// `PickSingle(OptionId)` indexing the candidate list. Awaits input (covered
/// by the `awaits_input` catch-all; not a phase anchor).
SlotDiscard {
    /// The investigator playing the asset.
    investigator: InvestigatorId,
    /// The pending asset's code (still in hand at `hand_index`).
    code: CardCode,
    /// Hand slot of the pending asset (enters play once room is made).
    hand_index: u8,
},
```

- [ ] **Step 2: Let the compiler find non-exhaustive matches**

Run: `cargo build -p game-core 2>&1 | head -40`
Expected: errors only at exhaustive `match self`/`match ...` over `Continuation` that lack a catch-all. Confirm:
- `Continuation::awaits_input` ends in `other => !other.is_phase_anchor()` → `SlotDiscard` returns `true` (correct, no arm needed).
- `Continuation::is_phase_anchor` uses `matches!(self, ...anchors...)` → `SlotDiscard` returns `false` (correct).
- The `drive` loop's outer `match` ends in `_ => return EngineOutcome::Done` → `SlotDiscard` on top while suspended falls here (correct; it only advances via `resolve_input`).
- `resolve_input`'s `match` — add the explicit arm in Step 4.

Add arms only where the compiler demands (a match with no catch-all). If `pending_candidates`/`pending_candidates_mut` or a serde-adjacent match complains, add `Continuation::SlotDiscard { .. } => None` / the inert arm matching its neighbors (e.g. `PlayFromHand`).

- [ ] **Step 3: Add the suspend branch + prompt + resume to `slots.rs`**

In `crates/game-core/src/engine/dispatch/slots.rs`, add imports `use crate::action::InputResponse; use crate::engine::outcome::{InputRequest, ResumeToken}; use crate::engine::OptionId; use crate::state::Continuation;`.

Insert the interactive branch into `enter_asset_making_room`, replacing the comment `// Task 6 inserts the 2+-candidate interactive suspend here.`:

```rust
    if candidates.len() >= 2 {
        // Genuine choice: the player picks which occupier to discard. Park the
        // pending play (asset stays in hand at hand_index) and prompt.
        cx.state.continuations.push(Continuation::SlotDiscard {
            investigator,
            code: code.clone(),
            hand_index,
        });
        return prompt_slot_discard(cx, investigator, &deficit);
    }
```

Add the prompt + resume functions:

```rust
/// Build the `PickSingle` over the co-controlled assets occupying a still-deficit
/// slot type (the top `SlotDiscard` frame must already be in place).
fn prompt_slot_discard(
    cx: &mut Cx,
    investigator: InvestigatorId,
    deficit: &SlotCounts,
) -> EngineOutcome {
    let candidates = make_room_candidates(cx.state, investigator, deficit);
    let codes: Vec<CardCode> = candidates.into_iter().map(|(_, code)| code).collect();
    let prompt = format!(
        "Investigator {investigator:?}: choose an asset to discard to make room \
         (slots needed: {deficit:?})."
    );
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, super::hunters::candidate_options(&codes)),
        resume_token: ResumeToken(0),
    }
}

/// Resume a slot make-room choice: discard the chosen occupier, then continue
/// making room (re-prompt if still contested, auto-discard a forced last
/// candidate, or enter the pending asset once the deficit clears). An invalid
/// pick rejects and keeps the frame (the `DamageAssignment` / `HunterMove`
/// contract).
pub(super) fn resume_slot_discard(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let Some(Continuation::SlotDiscard {
        investigator,
        code,
        hand_index,
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("resume_slot_discard: top frame is not SlotDiscard");
    };
    let InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!("ResolveInput: slot make-room expects PickSingle, got {response:?}")
                .into(),
        };
    };
    let deficit = slot_deficit(cx.state, investigator, &code);
    let candidates = make_room_candidates(cx.state, investigator, &deficit);
    let Some(&(inst, _)) = candidates.get(*i as usize) else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: slot make-room option {i} out of range (0..{})",
                candidates.len()
            )
            .into(),
        };
    };
    // Valid: pop the frame we validated against, discard the choice, continue.
    cx.state.continuations.pop();
    super::cards::discard_card_from_play(cx, investigator, inst);
    enter_asset_making_room(cx, investigator, hand_index, &code)
}
```

- [ ] **Step 4: Route `resolve_input` to the resume**

In `crates/game-core/src/engine/dispatch/mod.rs`, in `resolve_input`'s `match`, add (near the `DamageAssignment` arm):

```rust
        // The interactive slot make-room choice (#498): the `SlotDiscard` frame
        // is the top prompt, resumed by its `PickSingle`.
        Some(Continuation::SlotDiscard { .. }) => slots::resume_slot_discard(cx, response),
```

Confirm `slots` is reachable from `mod.rs` (it is — sibling module; use `slots::` since `mod slots;` is declared there).

- [ ] **Step 5: Rewrite the multi-candidate test as interactive**

In `crates/cards/tests/asset_slots.rs`, replace `two_handed_weapon_auto_frees_both_hand_slots` with the interactive tests. Add a `pick`/`resolve` helper at the top of the file (mirroring `soak_distribution.rs`):

```rust
fn resolve(state: game_core::GameState, id: game_core::OptionId) -> game_core::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(id),
        }),
    )
}

/// Find the option whose label contains `needle` in an AwaitingInput outcome.
fn pick(outcome: &EngineOutcome, needle: &str) -> game_core::OptionId {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected AwaitingInput, got {outcome:?}");
    };
    request
        .options
        .iter()
        .find(|o| o.label.contains(needle))
        .unwrap_or_else(|| panic!("no option matching {needle:?} in {:?}", request.options))
        .id
}
```

(`OptionId` label is the `CardCode` Debug repr, e.g. `CardCode("01020")` — so `needle` is the bare code string like `"01020"`.)

Then the tests:

```rust
#[test]
fn third_hand_asset_prompts_to_choose_which_to_discard() {
    // Two distinct single-Hand assets fill both Hand slots; playing a third
    // single-Hand asset must free 1 — a genuine 2-candidate choice.
    let (state, id) = play_state(vec![MACHETE, KNIFE, FLASHLIGHT]);
    let r1 = play(state, id); // Machete enters (Hand 1/2)
    let r2 = play(r1.state, id); // Knife enters (Hand 2/2)
    assert_eq!(r2.state.investigators[&id].cards_in_play.len(), 2);

    // Flashlight (index 0) — Hand full, 2 candidates → suspend for a choice.
    let r3 = play(r2.state, id);
    assert!(
        matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected a make-room prompt, got {:?}",
        r3.outcome
    );

    // Discard Machete to make room.
    let r4 = resolve(r3.state, pick(&r3.outcome, MACHETE));
    assert_eq!(r4.outcome, EngineOutcome::Done);
    let inv = &r4.state.investigators[&id];
    let codes: Vec<&str> = inv.cards_in_play.iter().map(|c| c.code.as_str()).collect();
    assert_eq!(
        codes,
        vec![KNIFE, FLASHLIGHT],
        "Machete discarded, Knife + Flashlight in play"
    );
    assert_eq!(inv.discard, vec![CardCode::new(MACHETE)]);
}

#[test]
fn out_of_range_make_room_pick_is_rejected_and_keeps_the_prompt() {
    let (state, id) = play_state(vec![MACHETE, KNIFE, FLASHLIGHT]);
    let r1 = play(state, id);
    let r2 = play(r1.state, id);
    let r3 = play(r2.state, id);
    assert!(matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }));

    // Option 99 is out of range → Rejected, the prompt persists.
    let r4 = resolve(r3.state, game_core::OptionId(99));
    assert!(
        matches!(r4.outcome, EngineOutcome::Rejected { .. }),
        "out-of-range pick rejects: {:?}",
        r4.outcome
    );
    // Still mid-investigation with both Hand assets and the pending Flashlight in
    // hand — nothing was discarded.
    let inv = &r4.state.investigators[&id];
    assert_eq!(inv.cards_in_play.len(), 2);
    assert!(inv.discard.is_empty());
    assert!(inv.hand.contains(&CardCode::new(FLASHLIGHT)));
}
```

- [ ] **Step 6: Run the interactive tests**

Run: `cargo test -p cards --test asset_slots`
Expected: `playing_a_second_ally_auto_discards_the_first`, `third_hand_asset_prompts_to_choose_which_to_discard`, `out_of_range_make_room_pick_is_rejected_and_keeps_the_prompt` all PASS.

- [ ] **Step 7: Confirm `OptionId` is exported where the test uses it**

The test uses `game_core::OptionId`. If it is not re-exported at the crate root, change references to its real path (`game_core::engine::OptionId`) — confirm via:
Run: `cargo test -p cards --test asset_slots 2>&1 | head -20`
Fix any path error and re-run.

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/slots.rs crates/game-core/src/engine/dispatch/mod.rs crates/cards/tests/asset_slots.rs
git commit -m "engine: interactive discard-to-make-room for slot conflicts (#498)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Final Task: Full CI gauntlet, phase doc, PR

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md` (only once the PR is ready — see CLAUDE.md)

- [ ] **Step 1: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green. Fix any clippy/doc/fmt issues (e.g. `BTreeMap` import ordering, doc-links) before pushing.

- [ ] **Step 2: Pre-push review**

Invoke `superpowers:requesting-code-review` (the execution flow's pre-push pass) and address findings. Surface every finding to the user, severity-bucketed and verbatim.

- [ ] **Step 3: Push and open the PR**

```bash
git push -u origin engine/asset-slot-limits
gh pr create --fill
```
The PR body must: cite RR p.19 verbatim for the make-room rule; explain the auto-vs-interactive split and the `need > cap` reject's unreachability; note the #119 helper consolidation. End the body with the Claude Code attribution footer.

- [ ] **Step 4: Watch CI**

Run: `gh pr checks <PR#> --watch`
Fix failures with follow-up commits to the same branch (no force-push).

- [ ] **Step 5: Update the phase doc (final commit, only when green + review-clean)**

In `docs/phases/phase-7-the-gathering.md`, add a brief note under "Remaining gate work" / the appropriate section that #498 (asset slot limits + discard-to-make-room) shipped, since it was found during the phase-7 browser playtest. Add a **Decisions made** entry only if it passes the test in `docs/phases/README.md` ("would a future PR-author choose differently without this entry?") — e.g. the make-room timing (at enter-play) and the auto-bind-single / interactive-2+ split. Keep it terse.

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: record asset slot limits in phase-7 plan (#498)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
git push
```

- [ ] **Step 6: Merge only after explicit user approval**

```bash
gh pr merge <PR#> --squash --delete-branch
```
Confirm #498 auto-closed and `git pull` on `main`.

---

## Self-Review

**Spec coverage:**
- Slot capacity table → Task 1 (`default_slot_capacity`). ✓
- Occupancy / deficit → Tasks 1 (pure) + 5 (registry-wired). ✓
- Validation gate (`need > cap`) → Task 4. ✓
- Enter-play make-room (deficit 0 / auto / interactive) → Tasks 3 (extract), 5 (auto), 6 (interactive). ✓
- `SlotDiscard` frame + resume + `resolve_input` routing → Task 6. ✓
- Discard helper extraction (#119) → Task 2. ✓
- Edge cases (slot-less, two-handed over-free, cap-1 auto, multi-option, mid-play defeat, registry-free) → covered by Task 1 unit tests + Task 5/6 integration tests; mid-play-defeat is unchanged (the asset never reaches `dispose_play_from_hand`). ✓
- Tests: card/integration in `asset_slots.rs`, engine unit in `slots.rs`. ✓
- Out of scope (slot-modifying cards, interactive-acknowledge of auto-discard, "gain control of") → documented in `slots.rs` TODO + design doc; not implemented. ✓

**Placeholder scan:** The Task 5 `two_handed_weapon_auto_frees_both_hand_slots` test is explicitly a temporary placeholder rewritten in Task 6 Step 5 — flagged, not a silent gap. No other TBD/TODO-in-tests.

**Type consistency:** `SlotCounts = BTreeMap<Slot, u8>` used consistently; `enter_asset_making_room(cx, investigator, hand_index: u8, code: &CardCode) -> EngineOutcome` and `resume_slot_discard(cx, response: &InputResponse) -> EngineOutcome` match between definition (Tasks 5/6) and call sites (`dispose_play_from_hand`, `resolve_input`). `discard_card_from_play` / `enter_asset_into_play` signatures match across Tasks 2/3/5. `Continuation::SlotDiscard { investigator, code, hand_index }` fields match between the variant (Step 1), the push (Step 3), and the resume destructure (Step 3).
