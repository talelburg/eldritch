# Phase 7 Slice 1 — C4a: Threat-area zone + shared scan source Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-investigator threat-area zone, unify the forced-trigger and reaction-window scans onto one "controlled card instances" source spanning `cards_in_play` + the threat area, and add the `EndOfTurn` and `AfterLocationInvestigated` forced timing points with their firing sites — the foundational engine seam C4c's persistent treacheries (#235) build on.

**Architecture:** The threat area is a second `Vec<CardInPlay>` on `Investigator`, mirroring `cards_in_play`. A new `Investigator::controlled_card_instances()` method chains both zones; both the reaction scan (`scan_pending_triggers`) and the new forced instance-scan walk it, so threat-area cards are covered by both without duplicate code. Two new `EventPattern`/`ForcedTriggerPoint` variants fire from `end_turn` (EndOfTurn) and the skill-test resolution driver (AfterLocationInvestigated). No real card consumes the new points yet — C4c (#235) is the first consumer — so coverage uses mock-registry cards in `crates/game-core/tests/`, the established pattern (see `forced_triggers.rs`).

**Tech Stack:** Rust, `card-dsl` (pure DSL types) + `game-core` (kernel). Strict CI: `RUSTFLAGS="-D warnings"`, clippy `-D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings"`.

**Issue:** [#233](https://github.com/talelburg/eldritch/issues/233). Branch: `engine/threat-area-scan`.

**Scope notes / deliberate deferrals:**
- **Which treacheries persist in the threat area + the Revelation discard-vs-persist routing → C4c (#235).** C4a ships the zone, the enter/leave helpers, the scan, and the firing points only.
- **`AfterLocationInvestigated` scans the *investigator's* controlled instances** (threat area + in play) in C4a. The real consumer Obscuring Fog (01168) attaches to a *location*, not the threat area; C4c extends this point to also scan the investigated location's attachment zone. C4a's mock test uses a threat-area card so the new point + scan are exercised.
- **Suspension caveat.** Both firing sites propagate `AwaitingInput`, but neither `end_turn` nor the skill-test driver has resume plumbing for a *suspending* forced effect (one that starts a nested skill test). No C4a/C4c-scope consumer suspends here (Frozen in Fear's end-of-turn and Obscuring Fog's discard are verified against the snapshot in C4c); a suspending consumer is #212 reentrancy work. C4a's tests use non-suspending effects (`deal_horror` / `gain_resources`), and `AfterLocationInvestigated` asserts the outcome is `Done` loudly.

---

### Task 1: Threat-area zone + `controlled_card_instances` (state)

**Files:**
- Modify: `crates/game-core/src/state/card.rs` (add `Zone::ThreatArea`)
- Modify: `crates/game-core/src/state/investigator.rs` (add `threat_area` field, `controlled_card_instances` method, tests)

- [ ] **Step 1: Write the failing tests** (append to the test module at the bottom of `crates/game-core/src/state/investigator.rs`)

```rust
#[cfg(test)]
mod threat_area_tests {
    use super::*;
    use crate::state::{CardCode, CardInPlay, CardInstanceId};

    #[test]
    fn new_investigator_has_empty_threat_area() {
        let inv = crate::test_support::test_investigator(1);
        assert!(inv.threat_area.is_empty());
    }

    #[test]
    fn deserializes_when_threat_area_field_absent() {
        // A state serialized before `threat_area` existed must still
        // parse (serde default), proving forward-compat.
        let json = r#"{
            "id": 1, "name": "Test", "current_location": null,
            "skills": {"willpower":3,"intellect":3,"combat":3,"agility":3},
            "max_health": 8, "damage": 0, "max_sanity": 8, "horror": 0,
            "clues": 0, "resources": 0, "actions_remaining": 3,
            "status": "Active", "deck": [], "hand": [], "discard": [],
            "cards_in_play": []
        }"#;
        let inv: Investigator = serde_json::from_str(json).expect("deserialize");
        assert!(inv.threat_area.is_empty());
    }

    #[test]
    fn controlled_card_instances_yields_in_play_then_threat_area() {
        let mut inv = crate::test_support::test_investigator(1);
        inv.cards_in_play
            .push(CardInPlay::enter_play(CardCode::new("in-play"), CardInstanceId(1)));
        inv.threat_area
            .push(CardInPlay::enter_play(CardCode::new("threat"), CardInstanceId(2)));
        let codes: Vec<&str> = inv
            .controlled_card_instances()
            .map(|c| c.code.as_str())
            .collect();
        assert_eq!(codes, vec!["in-play", "threat"]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core threat_area_tests`
Expected: FAIL — `no field threat_area`, `no method controlled_card_instances`.

- [ ] **Step 3: Add the `Zone::ThreatArea` variant**

In `crates/game-core/src/state/card.rs`, the `Zone` enum (around line 62) currently has `Hand`, `Deck`, `InPlay`. Add:

```rust
    /// An investigator's threat area — the play area holding encounter
    /// cards engaged with / affecting them (Rules Reference p.20).
    /// Cards there are at the investigator's location. Used as the
    /// `from` zone when a threat-area card is discarded.
    ThreatArea,
```

- [ ] **Step 4: Add the `threat_area` field**

In `crates/game-core/src/state/investigator.rs`, immediately after the `cards_in_play` field (ends at line 81), add:

```rust
    /// Encounter cards in this investigator's threat area — persistent
    /// treacheries and weaknesses engaged with / affecting them (Rules
    /// Reference p.20: "a play area in which encounter cards currently
    /// engaged with and/or affecting an investigator are placed";
    /// cards there are at the investigator's location). Mirrors
    /// [`cards_in_play`](Self::cards_in_play) — same `CardInPlay`
    /// per-instance state — but holds scenario-bag content rather than
    /// player cards. Defaults to empty for backward-compat: states
    /// serialized before this field was added still deserialize.
    ///
    /// [`cards_in_play`]: Self::cards_in_play
    #[serde(default)]
    pub threat_area: Vec<CardInPlay>,
```

- [ ] **Step 5: Add the `controlled_card_instances` method**

In `crates/game-core/src/state/investigator.rs`, add an `impl Investigator` block after the struct definition (before the `Status` enum):

```rust
impl Investigator {
    /// Every in-play card instance this investigator controls that can
    /// carry a triggerable ability: cards in play, then threat-area
    /// cards. The single definition both the reaction-window scan and
    /// the forced instance-scan walk, so the threat area is covered by
    /// both dispatch paths without a duplicate walk. This is the
    /// shared scan source #212 later absorbs.
    pub fn controlled_card_instances(&self) -> impl Iterator<Item = &CardInPlay> {
        self.cards_in_play.iter().chain(self.threat_area.iter())
    }
}
```

Note: `CardInPlay` is already imported (`use super::card::{CardCode, CardInPlay};`).

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p game-core threat_area_tests`
Expected: PASS (3 tests).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/state/card.rs crates/game-core/src/state/investigator.rs
git commit -m "engine: threat-area zone + controlled_card_instances scan source

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Threat-area placement/discard helpers + `CardEnteredThreatArea` event

**Files:**
- Modify: `crates/game-core/src/event.rs` (add `CardEnteredThreatArea`)
- Create: `crates/game-core/src/engine/dispatch/threat_area.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (declare the module)

- [ ] **Step 1: Add the `CardEnteredThreatArea` event**

In `crates/game-core/src/event.rs`, add a variant to the `Event` enum (place it just after the `CardPlayed` variant, around line 318):

```rust
    /// An encounter card entered an investigator's threat area
    /// (persistent treachery / weakness). Mirror of the in-play entry
    /// path for player cards; the discard mirror reuses
    /// [`CardDiscarded`](Event::CardDiscarded) with
    /// `from: Zone::ThreatArea`.
    CardEnteredThreatArea {
        /// The investigator whose threat area the card entered.
        investigator: InvestigatorId,
        /// The card code that entered.
        code: CardCode,
        /// The minted in-play instance id.
        instance_id: CardInstanceId,
    },
```

If `CardInstanceId` is not already imported in `event.rs`, add it to the `use crate::state::{...}` line (check the existing imports at the top of the file; `CardCode` and `InvestigatorId` are already used by neighbouring variants).

- [ ] **Step 2: Write the failing helper tests** (create `crates/game-core/src/engine/dispatch/threat_area.rs`)

```rust
//! Threat-area zone helpers: placing an encounter card into an
//! investigator's threat area and discarding it back to the encounter
//! discard pile. C4a (#233) ships the mechanism; which treacheries
//! persist here (and the Revelation routing that places them) is C4c
//! (#235).

use crate::event::Event;
use crate::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, Zone};

use super::Cx;

/// Place `code` into `investigator`'s threat area as a fresh in-play
/// instance, minting an instance id from the per-state counter, and
/// emit [`Event::CardEnteredThreatArea`]. Returns the minted id.
///
/// No-op (returns `None`) if the investigator isn't in state — callers
/// in dispatch have already validated the investigator exists, but the
/// helper stays total so a misuse can't panic.
pub(super) fn place_in_threat_area(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
) -> Option<CardInstanceId> {
    if !cx.state.investigators.contains_key(&investigator) {
        return None;
    }
    let instance_id = CardInstanceId(cx.state.next_card_instance_id);
    cx.state.next_card_instance_id = cx.state.next_card_instance_id.saturating_add(1);
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("existence checked above");
    inv.threat_area
        .push(CardInPlay::enter_play(code.clone(), instance_id));
    cx.events.push(Event::CardEnteredThreatArea {
        investigator,
        code,
        instance_id,
    });
    Some(instance_id)
}

/// Remove the threat-area instance `instance_id` from `investigator`,
/// push its code onto the encounter discard pile, and emit
/// [`Event::CardDiscarded`] with `from: Zone::ThreatArea`. Returns
/// `true` if an instance was removed, `false` if none matched.
pub(super) fn discard_from_threat_area(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
) -> bool {
    let Some(inv) = cx.state.investigators.get_mut(&investigator) else {
        return false;
    };
    let Some(pos) = inv
        .threat_area
        .iter()
        .position(|c| c.instance_id == instance_id)
    else {
        return false;
    };
    let card = inv.threat_area.remove(pos);
    cx.state.encounter_discard.push(card.code.clone());
    cx.events.push(Event::CardDiscarded {
        investigator,
        code: card.code,
        from: Zone::ThreatArea,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn place_mints_id_pushes_instance_and_emits_event() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let id = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            place_in_threat_area(&mut cx, InvestigatorId(1), CardCode::new("01164"))
        };
        assert_eq!(id, Some(CardInstanceId(0)));
        let inv = &state.investigators[&InvestigatorId(1)];
        assert_eq!(inv.threat_area.len(), 1);
        assert_eq!(inv.threat_area[0].code.as_str(), "01164");
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CardEnteredThreatArea { code, .. } if code.as_str() == "01164"
        )));
    }

    #[test]
    fn discard_removes_instance_pushes_to_encounter_discard_and_emits() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let id = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            place_in_threat_area(&mut cx, InvestigatorId(1), CardCode::new("01164"))
                .expect("placed")
        };
        events.clear();
        let removed = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            discard_from_threat_area(&mut cx, InvestigatorId(1), id)
        };
        assert!(removed);
        assert!(state.investigators[&InvestigatorId(1)].threat_area.is_empty());
        assert_eq!(state.encounter_discard, vec![CardCode::new("01164")]);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { from: Zone::ThreatArea, code, .. } if code.as_str() == "01164"
        )));
    }

    #[test]
    fn discard_of_unknown_instance_is_a_no_op() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let removed = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            discard_from_threat_area(&mut cx, InvestigatorId(1), CardInstanceId(999))
        };
        assert!(!removed);
        assert!(events.is_empty());
        assert!(state.encounter_discard.is_empty());
    }
}
```

- [ ] **Step 3: Declare the module**

In `crates/game-core/src/engine/dispatch/mod.rs`, add `mod threat_area;` alongside the other `mod` declarations (the file declares `mod actions;`, `mod cards;`, etc. near the top — add it in alphabetical position, after `mod skill_test;` or wherever fits the existing order).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p game-core threat_area::tests`
Expected: PASS (3 tests).

Note: `place_in_threat_area` / `discard_from_threat_area` are `pub(super)` and unused by production code in C4a (C4c is the first caller). The `#[cfg(test)]` uses keep them from tripping `dead_code` under `-D warnings`, because they are exercised by the in-module tests. Confirm with the clippy gauntlet in Step 5.

- [ ] **Step 5: Run the clippy gauntlet for this crate**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean. If `place_in_threat_area`/`discard_from_threat_area` trip `dead_code` (they shouldn't — the test module references them), add `#[cfg_attr(not(test), allow(dead_code))]` with a `// C4c (#235) is the first production caller` comment rather than deleting; but verify the tests reference them first.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/event.rs crates/game-core/src/engine/dispatch/threat_area.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: threat-area place/discard helpers + CardEnteredThreatArea

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Reaction scan walks the shared source (threat-area reactions)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:96` (the `for card in &inv.cards_in_play` loop in `scan_pending_triggers`)
- Modify: `crates/game-core/tests/reaction_windows.rs` (new end-to-end test)

- [ ] **Step 1: Write the failing test** (append to `crates/game-core/tests/reaction_windows.rs`)

This mirrors `matching_reaction_opens_window_and_suspends`, but places the reacting card in the **threat area** instead of `cards_in_play` — proving the unified scan source covers it.

```rust
#[test]
fn reaction_trigger_in_threat_area_opens_window() {
    // The shared scan source spans cards_in_play + threat_area, so a
    // reaction ability on a threat-area card is offered just like one
    // in play. Build the standard fight-to-defeat scenario but seat
    // ROLAND_REACTION in the threat area.
    install_mock_registry();
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 3;
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new(ROLAND_REACTION),
        CardInstanceId(7),
    ));
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy.engaged_with = Some(inv_id);
    let mut loc = test_location(10, "Mock Location");
    loc.clues = 3;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let result = fight_through_commit_window(state, fight_action(inv_id, enemy_id));

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "a threat-area reaction must open a window, got {:?}",
        result.outcome,
    );
    let window = result
        .state
        .top_reaction_window()
        .expect("threat-area reaction must populate the window");
    assert_eq!(window.pending_triggers.len(), 1);
    assert_eq!(window.pending_triggers[0].controller, inv_id);
    assert_eq!(window.pending_triggers[0].instance_id, CardInstanceId(7));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p game-core --test reaction_windows reaction_trigger_in_threat_area_opens_window`
Expected: FAIL — no window opens (`AwaitingInput` not produced) because the scan only walks `cards_in_play`.

- [ ] **Step 3: Switch the scan to the shared source**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, in `scan_pending_triggers`, change the inner loop (currently `for card in &inv.cards_in_play {`, line 96) to:

```rust
        for card in inv.controlled_card_instances() {
```

No other change — the loop body already operates on `&CardInPlay`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p game-core --test reaction_windows reaction_trigger_in_threat_area_opens_window`
Expected: PASS. Also re-run the whole file to confirm no regression: `cargo test -p game-core --test reaction_windows`.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs crates/game-core/tests/reaction_windows.rs
git commit -m "engine: reaction scan walks controlled_card_instances (threat area)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: New `EventPattern` variants (card-dsl)

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (add two `EventPattern` variants)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (extend the exhaustive `false`-arm in `trigger_matches`)

- [ ] **Step 1: Add the variants**

In `crates/card-dsl/src/dsl.rs`, add to the `EventPattern` enum (after `RoundEnded`, around line 279):

```rust
    /// The investigator's turn ended (Rules Reference p.24 step 2.2.2,
    /// "Forced – At the end of your turn"). Fired forced via
    /// `ForcedTriggerPoint::EndOfTurn` from `end_turn`, scanning the
    /// ending investigator's controlled card instances (threat area +
    /// in play); binds controller = that investigator. First consumer:
    /// Frozen in Fear (01164), C4c (#235).
    EndOfTurn,
    /// A location was successfully investigated. Fired forced via
    /// `ForcedTriggerPoint::AfterLocationInvestigated` from the
    /// skill-test resolution driver after a successful Investigate;
    /// binds controller = the investigating investigator. In C4a the
    /// forced scan covers the investigator's controlled card instances;
    /// C4c (#235) extends it to the investigated location's attachment
    /// zone for Obscuring Fog (01168), the first consumer.
    AfterLocationInvestigated,
```

These are serde-derived, so the derive constructs each variant — no `dead_code` even before a consumer exists.

- [ ] **Step 2: Run the build to find the exhaustive-match breakage**

Run: `cargo build -p game-core`
Expected: FAIL — `trigger_matches` in `reaction_windows.rs` has a non-exhaustive match (the `false`-arm lists every `EventPattern`).

- [ ] **Step 3: Extend the exhaustive `false`-arm in `trigger_matches`**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, the second match arm (around lines 184–194) lists the patterns that never match a reaction window. Add the two new patterns to that list:

```rust
        (
            WindowKind::PlayerWindow(_) | WindowKind::AfterEnemyDefeated { .. },
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned
            | EventPattern::EnteredLocation
            | EventPattern::PhaseEnded { .. }
            | EventPattern::ActAdvanced
            | EventPattern::AgendaAdvanced
            | EventPattern::RoundEnded
            | EventPattern::EndOfTurn
            | EventPattern::AfterLocationInvestigated,
        ) => false,
```

(`EndOfTurn` / `AfterLocationInvestigated` are matched only by the forced dispatch path, never by player reaction windows — same as `PhaseEnded` / `RoundEnded`.)

- [ ] **Step 4: Run the build + workspace tests to verify green**

Run: `cargo build -p game-core` then `cargo test -p card-dsl`
Expected: build clean; `card-dsl` tests PASS. If any other crate has an exhaustive `EventPattern` match, the build error names it — add the two variants there too (a `grep -rn "EventPattern::" crates/` review found only `trigger_matches`).

- [ ] **Step 5: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "card-dsl: EventPattern::{EndOfTurn, AfterLocationInvestigated}

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: `ForcedTriggerPoint::EndOfTurn` + firing site + tests

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (variant + `collect_forced_hits` branch)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs:200` (fire in `end_turn`)
- Modify: `crates/game-core/src/test_support/mod.rs` (helper)
- Modify: `crates/game-core/tests/forced_triggers.rs` (mock card + tests)

- [ ] **Step 1: Write the failing tests** (append to `crates/game-core/tests/forced_triggers.rs`)

First, register a mock threat-area card. In `mock_abilities_for`, add a new constant and arm. Add near the other consts (after `DOUBLE_FORCED`, line 49):

```rust
/// Mock threat-area card: one `EventPattern::EndOfTurn` forced ability
/// dealing 1 horror to the controller. The Frozen-in-Fear-shape (C4c),
/// minus the skill test (kept non-suspending for the C4a firing path).
const END_OF_TURN_CARD: &str = "test-end-of-turn";

/// Mock threat-area card: one `EventPattern::AfterLocationInvestigated`
/// forced ability dealing 1 horror to the controller. The
/// Obscuring-Fog-shape (C4c), minus the location attachment.
const AFTER_INVESTIGATE_CARD: &str = "test-after-investigate";
```

In `mock_abilities_for`, add two arms before the final `else { None }`:

```rust
    } else if code.as_str() == END_OF_TURN_CARD {
        Some(vec![on_event(
            EventPattern::EndOfTurn,
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1),
        )])
    } else if code.as_str() == AFTER_INVESTIGATE_CARD {
        Some(vec![on_event(
            EventPattern::AfterLocationInvestigated,
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1),
        )])
```

Then add the EndOfTurn tests (the `AfterLocationInvestigated` tests land in Task 6, but registering both mock cards now keeps `mock_abilities_for` edited once):

```rust
// ── EndOfTurn tests ───────────────────────────────────────────────────────────

#[test]
fn fire_forced_at_end_of_turn_resolves_threat_area_ability() {
    use game_core::test_support::fire_forced_at_end_of_turn;
    use game_core::state::{CardInPlay, CardInstanceId};

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode(END_OF_TURN_CARD.into()),
        CardInstanceId(1),
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_at_end_of_turn(&mut state, &mut events, InvestigatorId(1));

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 1);
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn fire_forced_at_end_of_turn_no_op_without_threat_area_card() {
    use game_core::test_support::fire_forced_at_end_of_turn;

    install_mock_registry();
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_at_end_of_turn(&mut state, &mut events, InvestigatorId(1));

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 0);
    assert!(events.is_empty());
}

#[test]
fn end_turn_fires_end_of_turn_forced_for_the_ending_investigator() {
    // End-to-end: EndTurn for a lone investigator with an EndOfTurn
    // threat-area card fires its forced effect as part of ending the
    // turn. (Single investigator → phase cascades past Investigation,
    // but the horror lands during the turn-end step before rotation.)
    use game_core::state::{CardInPlay, CardInstanceId};

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 0;
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode(END_OF_TURN_CARD.into()),
        CardInstanceId(1),
    ));
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .build();

    let result = apply(state, Action::Player(PlayerAction::EndTurn));

    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })),
        "EndOfTurn forced effect must fire during EndTurn; events = {:?}",
        result.events
    );
    assert_eq!(result.state.investigators[&InvestigatorId(1)].horror, 1);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core --test forced_triggers end_of_turn`
Expected: FAIL — `fire_forced_at_end_of_turn` undefined; `ForcedTriggerPoint::EndOfTurn` undefined.

- [ ] **Step 3: Add the `ForcedTriggerPoint::EndOfTurn` variant + collect branch**

In `crates/game-core/src/engine/dispatch/forced_triggers.rs`, add to the `ForcedTriggerPoint` enum (after `RoundEnded`, around line 67):

```rust
    /// An investigator's turn ended (step 2.2.2). Scans that
    /// investigator's controlled card instances (threat area + in play)
    /// for `EventPattern::EndOfTurn` forced abilities; binds controller
    /// = that investigator. First consumer: Frozen in Fear (01164), C4c.
    EndOfTurn {
        /// The investigator whose turn ended.
        investigator: InvestigatorId,
    },
```

Add a branch to the `match point` in `collect_forced_hits` (after the `RoundEnded` arm, before the closing `}` of the match):

```rust
        ForcedTriggerPoint::EndOfTurn { investigator } => {
            let Some(inv) = state.investigators.get(investigator) else {
                return hits;
            };
            // Scan the ending investigator's controlled instances
            // (threat area + in play). Code-based registry lookup is
            // fine — abilities are static per code; C4c threads the
            // source instance when an effect needs to discard itself.
            for card in inv.controlled_card_instances() {
                push_matching(reg, &card.code, *investigator, &mut hits, |p| {
                    matches!(p, EventPattern::EndOfTurn)
                });
            }
        }
```

- [ ] **Step 4: Fire `EndOfTurn` from `end_turn`**

In `crates/game-core/src/engine/dispatch/phases.rs`, in `end_turn`, immediately after the `TurnEnded` push (the `cx.events.push(Event::TurnEnded { investigator: active_id });` block ending at line 202) and **before** the rotation `if let Some(next_id) = ...` block, insert:

```rust
    // Forced "at the end of your turn" abilities (threat-area cards
    // such as Frozen in Fear 01164) fire for the investigator whose
    // turn just ended, before the turn passes on. No real card
    // consumes this in C4a; C4c (#235) is the first consumer.
    //
    // Suspension caveat: a forced effect that itself initiates a skill
    // test would return AwaitingInput here, suspending end_turn before
    // rotation — which end_turn has no resume plumbing for. No C4c
    // consumer suspends at this point (verified in C4c); a suspending
    // one is #212 reentrancy work. We propagate the outcome rather than
    // swallow it so the gap is loud if it ever arises.
    let end_of_turn = super::forced_triggers::fire_forced_triggers(
        cx,
        &super::forced_triggers::ForcedTriggerPoint::EndOfTurn {
            investigator: active_id,
        },
    );
    if !matches!(end_of_turn, EngineOutcome::Done) {
        return end_of_turn;
    }
```

- [ ] **Step 5: Add the `fire_forced_at_end_of_turn` test helper**

In `crates/game-core/src/test_support/mod.rs`, add after `fire_forced_on_enemy_defeat` (line 113):

```rust
/// Test helper: fire `ForcedTriggerPoint::EndOfTurn` for `investigator`,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`. Exercises
/// the threat-area "at the end of your turn" forced path.
pub fn fire_forced_at_end_of_turn(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EndOfTurn { investigator },
    )
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p game-core --test forced_triggers end_of_turn`
Expected: PASS (3 tests). Also `cargo test -p game-core --test forced_triggers` (no regression).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/phases.rs crates/game-core/src/test_support/mod.rs crates/game-core/tests/forced_triggers.rs
git commit -m "engine: ForcedTriggerPoint::EndOfTurn fired from end_turn

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: `ForcedTriggerPoint::AfterLocationInvestigated` + firing site + tests

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (variant + `collect_forced_hits` branch)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (fire in the `PostOnResolution` driver step)
- Modify: `crates/game-core/src/test_support/mod.rs` (helper)
- Modify: `crates/game-core/tests/forced_triggers.rs` (tests; the mock card was registered in Task 5)

- [ ] **Step 1: Write the failing tests** (append to `crates/game-core/tests/forced_triggers.rs`)

```rust
// ── AfterLocationInvestigated tests ───────────────────────────────────────────

#[test]
fn fire_forced_after_investigate_resolves_threat_area_ability() {
    use game_core::test_support::fire_forced_after_location_investigated;
    use game_core::state::{CardInPlay, CardInstanceId};

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode(AFTER_INVESTIGATE_CARD.into()),
        CardInstanceId(1),
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_after_location_investigated(
        &mut state,
        &mut events,
        InvestigatorId(1),
        LocationId(10),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 1);
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn fire_forced_after_investigate_no_op_without_threat_area_card() {
    use game_core::test_support::fire_forced_after_location_investigated;

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_turn_order([InvestigatorId(1)])
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_after_location_investigated(
        &mut state,
        &mut events,
        InvestigatorId(1),
        LocationId(10),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 0);
    assert!(events.is_empty());
}
```

For the end-to-end firing-site test (drive a successful Investigate to fire the forced point), append a separate `#[test]` that mirrors the chaos/skill setup in `crates/game-core/tests/on_skill_test_resolution.rs` (single-`Numeric(0)` bag, shroud 0 so a 3-intellect investigator always succeeds). It drives `PlayerAction::Investigate` through its commit window with empty commits via the `apply_no_commits` helper:

```rust
#[test]
fn successful_investigate_fires_after_location_investigated_forced() {
    use game_core::state::{
        CardInPlay, CardInstanceId, ChaosBag, ChaosToken, TokenModifiers,
    };
    use game_core::test_support::apply_no_commits;

    install_mock_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.skills.intellect = 3;
    inv.actions_remaining = 1;
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode(AFTER_INVESTIGATE_CARD.into()),
        CardInstanceId(1),
    ));
    let mut loc = test_location(10, "Study");
    loc.shroud = 0; // difficulty 0 → 3 intellect + Numeric(0) token always succeeds
    loc.clues = 1;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        .with_investigator(inv)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::Investigate {
            investigator: InvestigatorId(1),
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })),
        "AfterLocationInvestigated forced effect must fire on a successful \
         investigate; events = {:?}",
        result.events
    );
    assert_eq!(result.state.investigators[&InvestigatorId(1)].horror, 1);
}
```

Note: confirm `PlayerAction::Investigate`'s field name is `investigator` (it is — see `dispatch/mod.rs` routing). If `test_location`'s `shroud` field needs setting differently, read `crates/game-core/src/test_support/fixtures.rs` for the constructor; `test_location` returns a revealed location with `shroud` you can overwrite.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core --test forced_triggers after_investigate` and `... successful_investigate`
Expected: FAIL — `fire_forced_after_location_investigated` undefined; `ForcedTriggerPoint::AfterLocationInvestigated` undefined; end-to-end produces no `HorrorTaken`.

- [ ] **Step 3: Add the `AfterLocationInvestigated` variant + collect branch**

In `crates/game-core/src/engine/dispatch/forced_triggers.rs`, add to the `ForcedTriggerPoint` enum (after the `EndOfTurn` variant from Task 5):

```rust
    /// A location was successfully investigated. Scans the investigating
    /// investigator's controlled card instances (threat area + in play)
    /// for `EventPattern::AfterLocationInvestigated` forced abilities;
    /// binds controller = that investigator. C4c (#235) extends the scan
    /// to the investigated location's attachments for Obscuring Fog
    /// (01168), the first real consumer.
    AfterLocationInvestigated {
        /// The investigator who investigated.
        investigator: InvestigatorId,
        /// The location that was investigated. Unused by the C4a scan
        /// (which keys off the investigator); C4c reads it to scan the
        /// location's attachment zone.
        location: LocationId,
    },
```

Add a branch to `collect_forced_hits` (after the `EndOfTurn` arm). The `location` field is bound but unused in C4a — prefix with `_` to satisfy `-D warnings`:

```rust
        ForcedTriggerPoint::AfterLocationInvestigated {
            investigator,
            location: _location,
        } => {
            let Some(inv) = state.investigators.get(investigator) else {
                return hits;
            };
            // C4a scans the investigator's controlled instances; C4c
            // extends to `_location`'s attachment zone (Obscuring Fog).
            for card in inv.controlled_card_instances() {
                push_matching(reg, &card.code, *investigator, &mut hits, |p| {
                    matches!(p, EventPattern::AfterLocationInvestigated)
                });
            }
        }
```

`LocationId` is already imported in `forced_triggers.rs` (`use crate::state::{CardCode, InvestigatorId, LocationId, Phase};`).

- [ ] **Step 4: Fire from the skill-test resolution driver**

In `crates/game-core/src/engine/dispatch/skill_test.rs`, add a private helper near `fire_retaliate_if_any` (after that function, around line 596):

```rust
/// Fire `ForcedTriggerPoint::AfterLocationInvestigated` if the
/// just-resolved test was a *successful Investigate*. Runs at the
/// `PostOnResolution` step (after on-resolution triggers and retaliate,
/// "after applying all results"). No-op unless the test succeeded and
/// its follow-up was `Investigate`.
///
/// In-scope consumers (Obscuring Fog 01168 discards itself) neither
/// suspend nor produce 2+ simultaneous triggers, so a non-`Done`
/// outcome is a contract violation, surfaced loudly — matching the
/// `fire_on_skill_test_resolution` policy. A suspending consumer here
/// is #212 reentrancy work.
fn fire_after_location_investigated(cx: &mut Cx, investigator: InvestigatorId, succeeded: bool) {
    if !succeeded {
        return;
    }
    let follow_up = cx.state.in_flight_skill_test.as_ref().map(|t| t.follow_up);
    if !matches!(follow_up, Some(SkillTestFollowUp::Investigate)) {
        return;
    }
    let Some(location) = cx
        .state
        .investigators
        .get(&investigator)
        .and_then(|i| i.current_location)
    else {
        return;
    };
    let outcome = super::forced_triggers::fire_forced_triggers(
        cx,
        &super::forced_triggers::ForcedTriggerPoint::AfterLocationInvestigated {
            investigator,
            location,
        },
    );
    if !matches!(outcome, EngineOutcome::Done) {
        unreachable!(
            "AfterLocationInvestigated forced trigger returned non-Done ({outcome:?}); \
             slice-1 content (Obscuring Fog discards, no suspension / 2+ simultaneous). \
             A suspending consumer needs the #212 reentrancy work."
        );
    }
}
```

Then call it at the **top** of the `PostOnResolution` arm of `drive_skill_test` (around line 263), before `discard_committed_cards` and before `in_flight_skill_test` is cleared (so `follow_up` is still readable):

```rust
            FinishContinuation::PostOnResolution { succeeded } => {
                fire_after_location_investigated(cx, investigator, succeeded);
                discard_committed_cards(cx, investigator, &indices_u8);
                cx.events.push(Event::SkillTestEnded { investigator });
                // ... rest unchanged ...
```

Note: the arm currently binds `succeeded: _`. Change it to `succeeded` so the value is usable. `SkillTestFollowUp` is already imported in `skill_test.rs` (`use crate::state::{... SkillTestFollowUp ...}` — confirm; it's used by `apply_skill_test_follow_up`).

- [ ] **Step 5: Add the `fire_forced_after_location_investigated` test helper**

In `crates/game-core/src/test_support/mod.rs`, after `fire_forced_at_end_of_turn`:

```rust
/// Test helper: fire `ForcedTriggerPoint::AfterLocationInvestigated`,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`. Exercises
/// the threat-area "after successfully investigated" forced path.
pub fn fire_forced_after_location_investigated(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
    location: crate::state::LocationId,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::AfterLocationInvestigated {
            investigator,
            location,
        },
    )
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p game-core --test forced_triggers after_investigate` and `... successful_investigate`
Expected: PASS. Then the whole file: `cargo test -p game-core --test forced_triggers`.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/src/test_support/mod.rs crates/game-core/tests/forced_triggers.rs
git commit -m "engine: ForcedTriggerPoint::AfterLocationInvestigated on successful investigate

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Full CI gauntlet + PR

**Files:** none (verification + PR).

- [ ] **Step 1: Run the full strict gauntlet locally** (CLAUDE.md "Commands")

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. Fix any failure before pushing. (`wasm-pack test` is unaffected by this engine change but is part of CI; run if available.)

- [ ] **Step 2: Push the branch and open the PR**

```bash
git push -u origin engine/threat-area-scan
gh pr create --fill
```

PR body: summarize the threat-area zone + shared scan + two forced points; cite RR p.20 for the threat-area definition; note the deferrals (C4c owns persist routing + location-attachment scan + the suspending-consumer plumbing). End with `Closes #233.` and the Claude Code generated-with footer.

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`
Fix any CI failure with follow-up commits to the same branch.

- [ ] **Step 4: Update the phase doc (final commit, only after CI is green)**

Per CLAUDE.md, edit `docs/phases/phase-7-the-gathering.md` as the **last** commit: move #233 to the Group-C "Shipped" prose / flip the C4a row in the breakdown table to `✅ PR #N`, and add a **Decisions made** entry only if load-bearing for C4c (e.g. "AfterLocationInvestigated scans the investigator's controlled instances in C4a; C4c extends to location attachments" + "controlled_card_instances is the shared scan seam"). Then push that commit. Do **not** merge — wait for explicit user approval, then `gh pr merge <PR#> --squash --delete-branch`.

---

## Self-Review

**Spec coverage (issue #233 acceptance):**
- ✅ "Threat-area zone modeled; cards enter/leave it." → Task 1 (field + `Zone::ThreatArea`) + Task 2 (place/discard helpers + event).
- ✅ "Single scan source covers cards_in_play + threat area." → Task 1 (`controlled_card_instances`) consumed by Task 3 (reaction scan) and Tasks 5–6 (forced instance scans).
- ✅ "ForcedTriggerPoint extended; engine tests cover the new points." → Task 4 (EventPattern), Task 5 (EndOfTurn + firing + tests), Task 6 (AfterLocationInvestigated + firing + tests).
- ✅ "Full strict gauntlet green." → Task 7.

**Placeholder scan:** No TBD/TODO; every code step shows full code. The only intentional "later" references are the documented C4c deferrals (persist routing, location-attachment scan, suspending-consumer plumbing), which are out of C4a scope per the decomposition spec.

**Type consistency:** `controlled_card_instances` (Task 1) is used verbatim in Tasks 3/5/6. `place_in_threat_area`/`discard_from_threat_area` (Task 2) match their test calls. Helper names `fire_forced_at_end_of_turn` / `fire_forced_after_location_investigated` match their test-call sites. `ForcedTriggerPoint::EndOfTurn { investigator }` and `AfterLocationInvestigated { investigator, location }` field names are consistent between definition, collect branch, firing site, and helper. `EventPattern::{EndOfTurn, AfterLocationInvestigated}` match the `matches!` closures and the `trigger_matches` false-arm.

**Risk notes for the implementer:** (1) Adding `EventPattern` variants breaks any exhaustive match — the build in Task 4 Step 2 surfaces every site (only `trigger_matches` known). (2) The `succeeded: _` → `succeeded` rebinding in `drive_skill_test`'s `PostOnResolution` arm (Task 6 Step 4) is easy to miss. (3) Confirm `SkillTestFollowUp` and `LocationId`/`CardInstanceId` imports exist in the files being edited; add them if the build complains.
