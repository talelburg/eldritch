# Legal-Action Enumerator — play/activate (slice 2a-ii-3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `legal_actions(state)` to enumerate PlayCard (one option per playable, non-prohibited hand card) and ActivateAbility (one option per activatable ability on each in-play card), by delegating to the handlers' own pure `check_*` predicates.

**Architecture:** A new `push_card_actions` helper in `engine/enumerate.rs` calls `check_play_card` + `play_is_prohibited` (per hand index) and `check_activate_ability` (per in-play card × ability index). These need card data, so the enumeration is registry-gated and its tests live in a new `crates/cards/tests/enumerate_actions.rs` (installs `cards::REGISTRY`, like the other card-integration tests) rather than the registry-less `game-core` unit tests. Read-only; nothing routes through it (2b).

**Tech Stack:** Rust, `game-core` (enumerator) + `cards` (integration tests). No new deps.

## Global Constraints

- **Build + expose, defer routing** (slice decision). Read-only enumerator; no handler rewired. Fidelity holds by **delegation** — the enumerator calls the exact `check_play_card`/`check_activate_ability` the handlers call, plus the same `play_is_prohibited` guard the PlayCard handler applies inline.
- **Registry-gated.** Both enumerations require `card_registry::current()`; without a registry they yield nothing (matching the handlers, which reject on `None`). Therefore the *unit* tests in `game-core` can't exercise presence — the presence + cross-check tests are integration tests in `crates/cards/tests/enumerate_actions.rs`.
- **Behaviour-preserving:** no handler changes. Full host gauntlet green each task: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- **Design of record:** umbrella spec §E; builds on 2a-ii-1/2 (PRs #402, #405, merged).
- **Commit footer** (every commit), verbatim:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
  ```
- **Branch:** `engine/enumerator-cards`. One commit per task.

## Reference: handler legality (delegated, not re-implemented)

- **PlayCard** (`cards.rs` `play_card`): `check_play_card(state, investigator, hand_index) -> Result<PlayCheckResult, Cow>` (pure; resolves card type/abilities, status, hand bounds, fast/turn gating, cost) **then** the inline guard `play_is_prohibited(state, reg, investigator, result.card_type)`. The enumerator mirrors *both*: `check_play_card(...).is_ok() && !play_is_prohibited(...)`.
- **ActivateAbility** (`abilities.rs` `activate_ability`): `check_activate_ability(state, investigator, instance_id, ability_index) -> Result<ActivateCheckResult, Cow>` (pure; in-play instance lookup, resolves the ability via `abilities_for(code)[ability_index]`, rejects non-`Activated` triggers, window + cost-payable gating). `ability_index` indexes the full `abilities_for(code)` Vec, so the enumerator iterates `0..abilities.len()` and `check_activate_ability` filters to the activated, payable, window-eligible ones.

`PlayerAction` constructors: `PlayCard { investigator, hand_index: u8 }`, `ActivateAbility { investigator, instance_id: CardInstanceId, ability_index: u8 }`. Registry: `card_registry::current() -> Option<&CardRegistry>`; `(reg.abilities_for)(&CardCode) -> Option<Vec<Ability>>`.

---

### Task 1: PlayCard enumeration

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` — `pub(super) mod reaction_windows` → `pub(crate) mod reaction_windows`; `PlayCheckResult` struct → `pub(crate)`.
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` — `check_play_card` → `pub(crate)`.
- Modify: `crates/game-core/src/engine/enumerate.rs` — add `push_card_actions` (PlayCard half) + call it from `legal_actions`.
- Create: `crates/cards/tests/enumerate_actions.rs` — registry-backed presence + cross-check.

**Interfaces:**
- Consumes: `legal_actions` (2a-ii-1); `check_play_card` (widened); `play_is_prohibited` (already `pub` in `evaluator`); `card_registry::current`.

- [ ] **Step 1: Write the failing test** (new integration-test file)

Create `crates/cards/tests/enumerate_actions.rs`:

```rust
//! Registry-backed tests for the legal-action enumerator's card actions
//! (PlayCard, ActivateAbility) — slice 2a-ii-3 (#393). These need real card
//! metadata/abilities, so they install `cards::REGISTRY` and live here rather
//! than in `game-core`'s registry-less unit tests.

use std::sync::Once;

use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation, InvestigationResume,
    InvestigatorId, Phase,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder};
use game_core::{legal_actions, Action, EngineOutcome, LocationId, PlayerAction};

const HOLY_ROSARY: &str = "01059"; // Mystic asset, cost 2, constant +1 willpower.
const FLASHLIGHT: &str = "01087"; // Asset with an activated ability (uses: Supplies).
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// A single-investigator open-turn state (InvestigatorTurn frame on top of the
/// InvestigationPhase anchor) with `hand` in hand and `in_play` in play, 3
/// actions, 9 resources, on a revealed location, non-empty chaos bag.
fn open_turn_state(hand: &[&str], in_play: Vec<CardInPlay>) -> game_core::GameState {
    install_real_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LOC);
    inv.actions_remaining = 3;
    inv.resources = 9;
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    inv.cards_in_play = in_play;
    GameStateBuilder::default()
        .with_investigator_at(inv, LOC)
        .with_location(test_location(LOC.0, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(INV)
        .build()
}

#[test]
fn play_card_offered_for_a_playable_hand_card() {
    let state = open_turn_state(&[HOLY_ROSARY], Vec::new());
    assert!(legal_actions(&state).contains(&PlayerAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    }));
}

#[test]
fn every_enumerated_action_applies_without_rejection_with_registry() {
    // Cross-check, registry edition: with real card data the enumeration
    // includes PlayCard (Holy Rosary) alongside the basic actions; each applies
    // without Rejected (Done or AwaitingInput are both acceptance).
    let state = open_turn_state(&[HOLY_ROSARY], Vec::new());
    for action in legal_actions(&state) {
        let result = game_core::apply(state.clone(), Action::Player(action.clone()));
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "enumerated action {action:?} was rejected: {:?}",
            result.outcome,
        );
    }
}
```

Note for the implementer: confirm the `with_investigator_at` builder signature and that `test_location(LOC.0, …)` ids line up (mirror `crates/cards/tests/play_card.rs`'s helper); adjust the builder calls to match that file if they differ.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cards --test enumerate_actions`
Expected: FAIL — `play_card_offered_for_a_playable_hand_card` (PlayCard not enumerated). It may fail to compile first if `legal_actions` isn't re-exported at `game_core::legal_actions` (it is, from 2a-ii-1) — fix any import path to match.

- [ ] **Step 3: Widen visibility + implement the PlayCard half**

In `crates/game-core/src/engine/dispatch/mod.rs`: change `pub(super) mod reaction_windows;` to `pub(crate) mod reaction_windows;`, and the `PlayCheckResult` struct declaration `pub(super) struct PlayCheckResult` to `pub(crate) struct PlayCheckResult` (its fields are already `pub`).

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`: change `pub(super) fn check_play_card` to `pub(crate) fn check_play_card`.

In `crates/game-core/src/engine/enumerate.rs`, call the new helper from `legal_actions` (after `push_combat_engage_actions`):

```rust
    push_combat_engage_actions(state, investigator, &mut actions);
    push_card_actions(state, investigator, &mut actions);
    actions
```

Add the helper (ActivateAbility half added in Task 2):

```rust
/// Append the card actions legal for `investigator` — PlayCard and (Task 2)
/// ActivateAbility (slice 2a-ii-3, #393). Both need card data, so they yield
/// nothing without a registry (matching the handlers, which reject on `None`).
/// Fidelity is by delegation: the enumerator calls the same `check_play_card` /
/// `check_activate_ability` the handlers call, plus the PlayCard handler's inline
/// `play_is_prohibited` guard.
fn push_card_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<PlayerAction>) {
    let Some(reg) = crate::card_registry::current() else {
        return;
    };
    let Some(inv) = state.investigators.get(&investigator) else {
        return;
    };

    // PlayCard: one option per hand card the handler would accept — playable
    // (`check_play_card`) and not forbidden by a constant restriction
    // (`play_is_prohibited`, e.g. Dissonant Voices 01165).
    let hand_len = inv.hand.len();
    for idx in 0..hand_len {
        let hand_index = u8::try_from(idx).unwrap_or(u8::MAX);
        if let Ok(check) =
            crate::engine::dispatch::reaction_windows::check_play_card(state, investigator, hand_index)
        {
            if !crate::engine::evaluator::play_is_prohibited(
                state,
                reg,
                investigator,
                check.card_type,
            ) {
                out.push(PlayerAction::PlayCard {
                    investigator,
                    hand_index,
                });
            }
        }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p cards --test enumerate_actions`
Expected: PASS (both tests).

- [ ] **Step 5: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "engine: enumerate PlayCard for playable hand cards (slice 2a-ii-3 of #393)

legal_actions offers PlayCard for each hand card the handler would accept,
delegating to check_play_card + the play_is_prohibited guard. Registry-gated
(no registry => no PlayCard, matching the handler). Tests are registry-backed
integration tests in cards/tests/enumerate_actions.rs. Widened check_play_card /
PlayCheckResult / mod reaction_windows to pub(crate).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

### Task 2: ActivateAbility enumeration

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` — `check_activate_ability` → `pub(crate)`.
- Modify: `crates/game-core/src/engine/enumerate.rs` — extend `push_card_actions` with the ActivateAbility loop.
- Modify: `crates/cards/tests/enumerate_actions.rs` — presence test + extend the cross-check with an in-play activatable asset.

**Interfaces:**
- Consumes: `check_activate_ability` (widened); `(reg.abilities_for)`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/cards/tests/enumerate_actions.rs`:

```rust
/// Flashlight in play with 3 Supplies uses, ready — its `ability_index: 0`
/// activated ability is usable.
fn flashlight_in_play(instance: CardInstanceId) -> CardInPlay {
    use game_core::state::UseKind;
    let mut torch = CardInPlay::enter_play(CardCode::new(FLASHLIGHT), instance);
    torch.uses.insert(UseKind::Supplies, 3);
    torch
}

#[test]
fn activate_offered_for_an_in_play_activated_ability() {
    let inst = CardInstanceId(0);
    let state = open_turn_state(&[], vec![flashlight_in_play(inst)]);
    assert!(legal_actions(&state).contains(&PlayerAction::ActivateAbility {
        investigator: INV,
        instance_id: inst,
        ability_index: 0,
    }));
}
```

And extend the existing cross-check to put Flashlight in play, so an ActivateAbility is enumerated and applied:

```rust
#[test]
fn every_enumerated_action_applies_without_rejection_with_registry() {
    // ... change the state line to include Flashlight in play:
    let state = open_turn_state(&[HOLY_ROSARY], vec![flashlight_in_play(CardInstanceId(0))]);
    // ... (loop unchanged)
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cards --test enumerate_actions`
Expected: FAIL — `activate_offered_for_an_in_play_activated_ability` (ActivateAbility not enumerated).

- [ ] **Step 3: Widen + implement the ActivateAbility loop**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`: change `pub(super) fn check_activate_ability` to `pub(crate) fn check_activate_ability`.

Append to `push_card_actions` in `enumerate.rs` (after the PlayCard loop, before the closing brace):

```rust
    // ActivateAbility: one option per activatable ability on each in-play card.
    // `ability_index` indexes the card's full ability list; `check_activate_ability`
    // filters to the activated, payable, window-eligible ones (so non-Activated
    // indices are simply not offered).
    for card in &inv.cards_in_play {
        let ability_count = (reg.abilities_for)(&card.code).map_or(0, |a| a.len());
        for idx in 0..ability_count {
            let ability_index = u8::try_from(idx).unwrap_or(u8::MAX);
            if crate::engine::dispatch::reaction_windows::check_activate_ability(
                state,
                investigator,
                card.instance_id,
                ability_index,
            )
            .is_ok()
            {
                out.push(PlayerAction::ActivateAbility {
                    investigator,
                    instance_id: card.instance_id,
                    ability_index,
                });
            }
        }
    }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p cards --test enumerate_actions`
Expected: PASS (presence + cross-check, now exercising ActivateAbility too).

- [ ] **Step 5: Run the gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
git add -A
git commit -m "engine: enumerate ActivateAbility for in-play abilities (slice 2a-ii-3 of #393)

legal_actions offers ActivateAbility for each in-play card ability the handler
would accept, iterating ability indices through check_activate_ability (which
filters to activated, payable, window-eligible ones). Registry-backed test with
Flashlight in play; cross-check extended to apply an activation. Widened
check_activate_ability to pub(crate).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## After the tasks

- **PR** against `main`; design-decisions paragraph: delegation-for-fidelity, registry-gated (tests in the cards crate), the `pub(crate)` widenings (`check_play_card`/`check_activate_ability`/`PlayCheckResult`/`mod reaction_windows`). Refs #393.
- **Phase/spec doc** (final commit once CI green): tick 2a-ii-3 in spec §E sequencing.
- **Next:** 2a-ii-4 — AdvanceAct + a final whole-enumeration sweep, closing slice 2a-ii.

## Self-review notes

- **Spec coverage:** §E enumerator over the play/activate group → Task 1 (PlayCard), Task 2 (ActivateAbility), both by delegation. Routing still deferred. ✅
- **Placeholder scan:** none.
- **Type consistency:** `PlayCard { investigator, hand_index: u8 }`, `ActivateAbility { investigator, instance_id: CardInstanceId, ability_index: u8 }`; `check_play_card(&GameState, InvestigatorId, u8) -> Result<PlayCheckResult, Cow>` (field `card_type` `pub`); `check_activate_ability(&GameState, InvestigatorId, CardInstanceId, u8) -> Result<_, Cow>`; `play_is_prohibited(&GameState, &CardRegistry, InvestigatorId, CardType) -> bool`; `(reg.abilities_for)(&CardCode) -> Option<Vec<Ability>>`. Match the source.
- **Registry-gating** is the key design point: tests are integration tests under `crates/cards/tests/` (process-isolated registry install). The `game-core` unit tests' existing `no_actions_when_not_the_open_turn` etc. stay valid (no registry ⇒ `push_card_actions` is a no-op).
- **Implementer caveats:** mirror `crates/cards/tests/play_card.rs` for the exact builder helper (`with_investigator_at` vs `with_investigator` + location), and `crates/cards/tests/flashlight.rs` for the Flashlight in-play setup (`CardInPlay::enter_play` + `uses.insert(UseKind::Supplies, 3)`); confirm `PlayCheckResult.card_type`'s field name before reading it; confirm `UseKind`/`CardInPlay` import paths.
