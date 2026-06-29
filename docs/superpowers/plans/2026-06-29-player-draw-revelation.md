# Player-Draw Weakness Revelation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a persistent treachery weakness (Cover Up 01007) is drawn from the player deck during play, reveal it and resolve its Revelation (Cover Up → controller's threat area with 3 clues) instead of leaving it in hand.

**Architecture:** A `resolve_drawn_weaknesses` helper removes a drawn persistent-treachery weakness from hand and pushes its Revelation onto the drive loop (mirroring the encounter path's `push_effect` of revelation effects). It's hooked into the two in-play draw entry points (`draw_one_with_deckout`, `draw_cards_effect`) — never the low-level `draw_cards` that setup/mulligan use (those set weaknesses aside per #508).

**Tech Stack:** Rust, the game-core engine (continuation/drive-loop model), `cards`/`card-dsl` corpus. Spec: `docs/superpowers/specs/2026-06-29-player-draw-revelation-design.md` (issue #509; deferrals tracked in #514).

## Global Constraints

- **Scope:** persistent treachery weaknesses only (Cover Up). Non-persistent treachery, weakness enemies, and weakness assets are **deferred to #514** — leave them in hand untouched (no regression).
- **Never resolve weaknesses on the setup/mulligan path.** The hook goes in `draw_one_with_deckout` and `draw_cards_effect` only — NOT `draw_cards` or `replace_opening_hand_weaknesses`. The #508 opening-hand tests (`crates/scenarios/tests/opening_hand_weaknesses.rs`) must still pass (setup *sets aside*, does not resolve).
- **Remove the drawn weakness from hand BEFORE pushing its Revelation** — `Effect::PutIntoThreatArea` spawns a fresh instance by code (`evaluator.rs:479`), so a copy left in hand would duplicate Cover Up.
- **No new randomness / `EngineRecord`.** Reuse `push_effect` + the drive loop; replay stays deterministic.
- Match CI: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, `cargo build -p web --target wasm32-unknown-unknown`, `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`. (No web change; the wasm-test browser job isn't required, but wasm build/clippy must pass.)
- Commit scope: `engine:`.

---

### Task 1: `resolve_drawn_weaknesses` helper + draw-path hooks

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (make `treachery_is_persistent` reusable)
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (helper + `draw_one_with_deckout` hook)
- Modify: `crates/game-core/src/engine/evaluator.rs` (`draw_cards_effect` hook)
- Test: `crates/cards/tests/player_draw_revelation.rs` (new)

**Interfaces:**
- Consumes: `card_registry::current()` → `metadata_for` / `abilities_for`; `CardMetadata::is_weakness()`, `CardMetadata::card_type()`; `encounter::treachery_is_persistent(&[Ability]) -> bool`; `evaluator::push_effect(cx, &Effect, EvalContext)`; `EvalContext::for_controller(InvestigatorId)`; `Event::CardRevealed { investigator, code, card_type }`.
- Produces: `cards::resolve_drawn_weaknesses(cx: &mut Cx, investigator: InvestigatorId)`.

- [ ] **Step 1: Make `treachery_is_persistent` reusable**

In `crates/game-core/src/engine/dispatch/encounter.rs`, change the helper's visibility from private to `pub(crate)` (signature otherwise unchanged):

```rust
pub(crate) fn treachery_is_persistent(abilities: &[crate::dsl::Ability]) -> bool {
    abilities.iter().any(|a| a.trigger != Trigger::Revelation)
}
```

- [ ] **Step 2: Write the failing integration test**

Create `crates/cards/tests/player_draw_revelation.rs`. It seats a solo investigator with Cover Up (01007) on top of the deck, takes the basic Draw action, and asserts Cover Up was revealed into the threat area (not left in hand). Mirror the registry-install + builder patterns in `crates/cards/tests/play_card.rs` and `crates/scenarios/tests/opening_hand_weaknesses.rs`.

```rust
//! #509: drawing a persistent treachery weakness (Cover Up 01007) from the
//! player deck during play reveals it and resolves its Revelation — Cover Up
//! enters the controller's threat area with 3 clues instead of staying in hand.

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};
use game_core::TurnAction;

const COVER_UP: &str = "01007";
const HOLY_ROSARY: &str = "01059"; // a non-weakness asset, for the negative case

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Solo investigator at a revealed location, mid-Investigation, 3 actions, no
/// enemies (so the Draw action's AoO loop is empty and resolves synchronously),
/// with `deck_top` as the top card of an otherwise-filler deck.
fn draw_state(deck_top: &str) -> (game_core::GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let loc = LocationId(101);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc);
    inv.actions_remaining = 3;
    // Top of deck is drawn first (draw_cards drains from the front).
    inv.deck = vec![
        CardCode::new(deck_top),
        CardCode::new(HOLY_ROSARY),
        CardCode::new(HOLY_ROSARY),
    ];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_location(test_location(101, "Study"))
        .build();
    (state, id)
}

#[test]
fn drawing_cover_up_reveals_it_into_the_threat_area() {
    let (state, id) = draw_state(COVER_UP);

    let result = dispatch_turn_action_unchecked(state, &TurnAction::Draw { investigator: id });

    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Draw must not be rejected; got {:?}",
        result.outcome,
    );
    let inv = &result.state.investigators[&id];
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == COVER_UP),
        "Cover Up must not stay in hand — it is revealed on draw",
    );
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .expect("Cover Up should be in the threat area after being drawn");
    assert_eq!(placed.clues, 3, "Cover Up enters the threat area with 3 clues");
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::CardRevealed { code, .. } if code.as_str() == COVER_UP)),
        "a CardRevealed event must fire for the drawn weakness",
    );
}

#[test]
fn drawing_a_non_weakness_leaves_it_in_hand() {
    let (state, id) = draw_state(HOLY_ROSARY);

    let result = dispatch_turn_action_unchecked(state, &TurnAction::Draw { investigator: id });

    let inv = &result.state.investigators[&id];
    assert!(
        inv.hand.iter().any(|c| c.as_str() == HOLY_ROSARY),
        "a normal drawn card stays in hand",
    );
    assert!(inv.threat_area.is_empty(), "nothing enters the threat area");
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::CardRevealed { .. })),
        "no reveal for a non-weakness draw",
    );
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p cards --test player_draw_revelation 2>&1 | tail -25`
Expected: `drawing_cover_up_reveals_it_into_the_threat_area` FAILS — Cover Up is still in hand / not in the threat area (the helper doesn't exist yet). `drawing_a_non_weakness_leaves_it_in_hand` should already pass.

(If the test fails to *compile* because `test_investigator`/`test_location`/`dispatch_turn_action_unchecked`/`GameStateBuilder`/`TurnAction` import paths differ, fix the `use` lines by matching `crates/cards/tests/play_card.rs` — do not change the assertions.)

- [ ] **Step 4: Implement `resolve_drawn_weaknesses`**

In `crates/game-core/src/engine/dispatch/cards.rs`, add the helper (place it near `replace_opening_hand_weaknesses`; reuse the existing imports — `CardCode`, `InvestigatorId`, `CardType`, `card_registry`, `Event`; add `use crate::dsl::{Effect, Trigger};` and the evaluator imports if not present):

```rust
/// Reveal-on-draw for a persistent treachery weakness drawn from the player
/// deck during play (RR Weakness keyword: a drawn weakness resolves its
/// Revelation immediately rather than staying a normal hand card). Scope:
/// **persistent treachery weaknesses** (Cover Up 01007). Each matching card is
/// removed from `investigator`'s hand, a [`Event::CardRevealed`] is emitted, and
/// its `Trigger::Revelation` effects are pushed for the drive loop (Cover Up's
/// `PutIntoThreatArea` then places it in the threat area).
///
/// Removal precedes the push deliberately: `PutIntoThreatArea` spawns a fresh
/// instance by code, so a copy left in hand would duplicate the card.
///
/// Non-persistent treachery weaknesses and weakness enemies/assets are **left in
/// hand untouched** — deferred to #514 (none reachable in the corpus draw path).
/// No-op without an installed registry (registry-free engine unit tests).
///
/// MUST NOT be called from the setup opening-hand / mulligan path — those *set
/// aside* weaknesses (#508), they do not resolve them.
pub(super) fn resolve_drawn_weaknesses(cx: &mut Cx, investigator: InvestigatorId) {
    let Some(reg) = card_registry::current() else {
        return;
    };
    // Collect indices of drawn persistent treachery weaknesses, in hand order.
    let matches: Vec<(usize, CardCode)> = {
        let Some(inv) = cx.state.investigators.get(&investigator) else {
            return;
        };
        inv.hand
            .iter()
            .enumerate()
            .filter(|(_, code)| {
                (reg.metadata_for)(code).is_some_and(|m| {
                    m.is_weakness() && m.card_type() == CardType::Treachery
                }) && super::encounter::treachery_is_persistent(
                    &(reg.abilities_for)(code).unwrap_or_default(),
                )
            })
            .map(|(i, code)| (i, code.clone()))
            .collect()
    };
    if matches.is_empty() {
        return;
    }
    // Remove from hand high-index-to-low so earlier indices stay valid.
    {
        let inv = cx
            .state
            .investigators
            .get_mut(&investigator)
            .expect("resolve_drawn_weaknesses: investigator exists");
        for &(i, _) in matches.iter().rev() {
            inv.hand.remove(i);
        }
    }
    // Reveal + push each Revelation, in original draw order.
    for (_, code) in &matches {
        cx.events.push(Event::CardRevealed {
            investigator,
            code: code.clone(),
            card_type: CardType::Treachery,
        });
        let effects: Vec<Effect> = (reg.abilities_for)(code)
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.trigger == Trigger::Revelation)
            .map(|a| a.effect)
            .collect();
        if !effects.is_empty() {
            super::super::evaluator::push_effect(
                cx,
                &Effect::Seq(effects),
                super::super::evaluator::EvalContext::for_controller(investigator),
            );
        }
    }
}
```

- [ ] **Step 5: Hook the in-play draw entry points**

In `crates/game-core/src/engine/dispatch/cards.rs`, at the end of `draw_one_with_deckout` (after both the `if deck_empty { … }` and `else { draw_cards(…) }` branches — i.e. as the last statement of the function body), add:

```rust
    // RR Weakness keyword: a weakness drawn during play reveals + resolves its
    // Revelation (#509). Setup's opening-hand draw uses `draw_cards` directly,
    // so it is unaffected (it sets aside instead, #508).
    resolve_drawn_weaknesses(cx, investigator);
```

In `crates/game-core/src/engine/evaluator.rs`, in `draw_cards_effect`, replace the final `draw_cards` + `Done` tail:

```rust
    crate::engine::dispatch::cards::draw_cards(cx, target_id, count);
    EngineOutcome::Done
```

with:

```rust
    crate::engine::dispatch::cards::draw_cards(cx, target_id, count);
    crate::engine::dispatch::cards::resolve_drawn_weaknesses(cx, target_id);
    EngineOutcome::Done
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p cards --test player_draw_revelation 2>&1 | tail -15`
Expected: both tests PASS.

- [ ] **Step 7: Confirm no #508 regression**

Run: `cargo test -p scenarios --test opening_hand_weaknesses 2>&1 | tail -8`
Expected: all pass (setup still *sets aside* — the new hook is not on the setup draw path).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/dispatch/encounter.rs \
        crates/game-core/src/engine/dispatch/cards.rs \
        crates/game-core/src/engine/evaluator.rs \
        crates/cards/tests/player_draw_revelation.rs
git commit -m "engine: drawing a weakness from the player deck resolves its Revelation (closes #509)"
```

---

### Task 2: Full gauntlet

**Files:** none (verification only).

- [ ] **Step 1: Run the full CI gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. Fix any clippy/fmt/doc issues with follow-up edits and re-run. In particular, watch for an unused-import warning if `cards.rs` already imported `Effect`/`Trigger` (dedupe) and confirm `treachery_is_persistent`'s new `pub(crate)` raises no dead-code/visibility lint.

- [ ] **Step 2: Commit any gauntlet fixes** (only if needed)

```bash
git add -A && git commit -m "engine: player-draw revelation gauntlet fixes (#509)"
```

---

## Self-Review

**Spec coverage:**
- Reveal persistent treachery weakness on draw → Task 1 (`resolve_drawn_weaknesses`). ✓
- Remove-from-hand before push (no duplication) → Task 1 Step 4 (removal loop precedes the reveal/push loop). ✓
- `push_effect` of Revelation effects via the drive loop → Task 1 Step 4. ✓
- Reuse `treachery_is_persistent` (made `pub(crate)`) → Task 1 Step 1. ✓
- Hook `draw_one_with_deckout` (Draw + Upkeep) and `draw_cards_effect` (DSL), never `draw_cards` → Task 1 Step 5. ✓
- Deferred non-persistent/enemy/asset left in hand (#514) → Task 1 Step 4 (filter only matches persistent treachery weaknesses). ✓
- Tests: Cover-Up-on-draw → threat area + 3 clues + CardRevealed; non-weakness untouched; #508 regression → Task 1 Steps 2/7. ✓
- Determinism (no new RNG/EngineRecord) → only `push_effect` used. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. ✓

**Type consistency:** `resolve_drawn_weaknesses(&mut Cx, InvestigatorId)` defined in Step 4, called identically in Step 5 and the evaluator hook. `treachery_is_persistent(&[Ability]) -> bool` reused from encounter.rs. `Event::CardRevealed { investigator, code, card_type }` matches the event def. ✓

**Verification notes for the implementer:** the `super::super::evaluator::` path from `cards.rs` may differ — use whatever path resolves (`crate::engine::evaluator::push_effect` / `EvalContext` is the absolute form); the test's `use` paths may need matching to `play_card.rs`. These are mechanical, compiler-guided fixes, not design changes.
