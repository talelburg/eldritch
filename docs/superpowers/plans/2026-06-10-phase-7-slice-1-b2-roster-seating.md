# Phase 7 Slice 1 B2 — Roster/seating + `StartScenario` selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `StartScenario` carry a roster of `{ investigator, deck }` entries; `start_scenario` resolves each investigator's stats from `CardMetadata` (existing `CardRegistry`), seats them with the payload deck, and rejects unless ≥1 investigator ends up seated — then the existing shuffle/deal runs unchanged.

**Architecture:** No new registry. A free helper `investigator_skills(&CardMetadata) -> Option<Skills>` (in `game-core`, since `Skills` is a `game-core` type and `card-dsl` can't depend on it) reinterprets an investigator card's `skill_icons` as base skills. `PlayerAction::StartScenario` becomes a struct variant `{ roster: Vec<RosterEntry> }`. Seating is validate-first: resolve+validate the whole roster before any mutation.

**Tech Stack:** Rust; `game-core` kernel, `protocol`/`web`/`server`/`scenarios` consumers, `cards` corpus for the integration test.

---

## Key facts (verified against the codebase)

- `Cx<'a> { state: &'a mut GameState, events: &'a mut Vec<Event> }` — `crates/game-core/src/engine/cx.rs`.
- Dispatch routing: `crates/game-core/src/engine/dispatch/mod.rs:142` `PlayerAction::StartScenario => phases::start_scenario(cx)`, and a `matches!` guard at `:54-56` `PlayerAction::Mulligan { .. } | PlayerAction::StartScenario`.
- `start_scenario(cx: &mut Cx) -> EngineOutcome` — `crates/game-core/src/engine/dispatch/phases.rs:19`. Order today: reject if `round != 0` → set `round=1`/`phase` → push `ScenarioStarted` → shuffle+deal each investigator → seed mulligan cursor → `reset_actions`.
- `Investigator` literal fields — `crates/game-core/src/test_support/fixtures.rs:36-59`. `Skills { willpower, intellect, combat, agility }` are `i8`; `SkillIcons { willpower, intellect, combat, agility, wild }` are `u8`.
- `card_registry::current() -> Option<&'static CardRegistry>`; `(reg.metadata_for)(&CardCode) -> Option<&'static CardMetadata>` — `crates/game-core/src/card_registry.rs:79,50`.
- `CardType`, `CardMetadata`, `SkillIcons`, `Class`, `Slot` reachable in `game-core` via `crate::card_data::*`. `Roland 01001`: `skill_icons { willpower:3, intellect:3, combat:4, agility:2, wild:0 }`, `health: Some(9)`, `sanity: Some(5)`. `01030` (Magnifying Glass) is an Asset — a valid non-investigator reject fixture.
- No existing test relies on `StartScenario` succeeding with zero investigators (the round-7 reject test trips the `round != 0` guard first). Every `start_scenario` happy-path test uses `with_investigator`.

## File map

- **Modify** `crates/game-core/src/action.rs` — add `RosterEntry`; change `StartScenario` to a struct variant.
- **Modify** `crates/game-core/src/engine/dispatch/mod.rs` — `matches!` guard + routing pass `roster`.
- **Modify** `crates/game-core/src/engine/dispatch/phases.rs` — `investigator_skills` helper + `start_scenario` signature/seating + unit tests.
- **Mechanical edits** (compiler-driven, `roster: vec![]` / `{ .. }`): every other `StartScenario` construction or match site across `game-core`, `scenarios`, `server`, `web`.
- **Create** `crates/cards/tests/roster_seating.rs` — integration test (installs `cards::REGISTRY`).

---

### Task 1: Protocol shape — `RosterEntry` + `StartScenario { roster }`, migrate all sites (behavior-preserving)

**Files:**
- Modify: `crates/game-core/src/action.rs:43-48`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs:54-56,142`
- Modify: every other `StartScenario` site (compiler-driven)

This task changes only the *shape*; `start_scenario` still ignores the roster, so all existing tests pass unchanged.

- [ ] **Step 1: Add `RosterEntry` and convert the variant**

In `crates/game-core/src/action.rs`, replace the unit `StartScenario` variant and add the struct. Confirm `CardCode` is imported (it is used elsewhere in this file; add `use crate::state::CardCode;` if not in scope):

```rust
    /// Begin a scenario session, seating the chosen investigators.
    ///
    /// `roster` pairs each investigator card code with the deck the
    /// player chose for them. Stats are resolved from card data at
    /// seat time (not carried here); the deck is taken verbatim — a
    /// free input that Phase 9's decklist import will populate.
    /// An empty roster seats no one; `start_scenario` rejects unless at
    /// least one investigator ends up seated.
    StartScenario { roster: Vec<RosterEntry> },
```

Add this struct near the enum (after it, before the next `///`-doc item):

```rust
/// One seat in a scenario: which investigator, and the deck the player
/// chose for them. Crosses the wire and lands in the action log, so the
/// deck composition replays deterministically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RosterEntry {
    /// Investigator card code (e.g. `"01001"` for Roland Banks).
    pub investigator: CardCode,
    /// The player's chosen deck, top-to-bottom. Taken verbatim by
    /// seating; deckbuilding-legality validation is Phase 9.
    pub deck: Vec<CardCode>,
}
```

- [ ] **Step 2: Update the dispatch match arm + guard (these are matches, not construction)**

In `crates/game-core/src/engine/dispatch/mod.rs`, the `matches!` guard (`:54-56`):

```rust
            PlayerAction::Mulligan { .. } | PlayerAction::StartScenario { .. }
```

and the routing arm (`:142`) — still ignore the roster this task:

```rust
        PlayerAction::StartScenario { .. } => phases::start_scenario(cx),
```

- [ ] **Step 3: Build to surface every remaining site, fix mechanically**

Run: `cargo build --all --all-features --tests 2>&1 | grep -E "error" | head -40`

For each error: a **construction** site (`PlayerAction::StartScenario` as a value) becomes `PlayerAction::StartScenario { roster: vec![] }`; a **match/`matches!`** site becomes `PlayerAction::StartScenario { .. }`. These span `crates/game-core/src/engine/mod.rs`, `state/game_state.rs`, `test_support/builder.rs`, `crates/scenarios/tests/*`, `crates/server/tests/*`, `crates/web/src/controls.rs`, `crates/web/tests/controls.rs`. (Note: `ActionControl::StartScenario` in `crates/web/src/legality.rs` is a *different* enum — leave it.) Re-run until the build is clean.

- [ ] **Step 4: Confirm green, no behavior change**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | tail -5`
Expected: all existing tests pass (roster is ignored; behavior identical).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "protocol: StartScenario carries a roster of RosterEntry

Struct variant { roster: Vec<RosterEntry> } + RosterEntry { investigator,
deck }. Roster ignored by start_scenario this commit (behavior-preserving
migration); seating wired in a follow-up commit.

Part of #221.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `investigator_skills` helper + unit test

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (helper + `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/game-core/src/engine/dispatch/phases.rs` (a `#[cfg(test)] mod tests` already exists there — append these). The local `meta` builder fills every `CardMetadata` field (the struct isn't `#[non_exhaustive]`):

```rust
    use crate::card_data::{CardMetadata, CardType, Class, SkillIcons};
    use crate::state::Skills;

    fn meta(card_type: CardType, icons: SkillIcons) -> CardMetadata {
        CardMetadata {
            code: "x".to_owned(),
            name: "x".to_owned(),
            class: Class::Guardian,
            card_type,
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_icons: icons,
            health: Some(9),
            sanity: Some(5),
            deck_limit: 0,
            quantity: 1,
            pack_code: "core".to_owned(),
            position: 1,
            is_fast: false,
            spawn: None,
            surge: false,
            peril: false,
        }
    }

    #[test]
    fn investigator_skills_reads_base_skills_for_investigator_cards() {
        let m = meta(
            CardType::Investigator,
            SkillIcons { willpower: 3, intellect: 3, combat: 4, agility: 2, wild: 0 },
        );
        assert_eq!(
            investigator_skills(&m),
            Some(Skills { willpower: 3, intellect: 3, combat: 4, agility: 2 }),
        );
    }

    #[test]
    fn investigator_skills_is_none_for_non_investigator_cards() {
        let m = meta(
            CardType::Asset,
            SkillIcons { willpower: 1, intellect: 0, combat: 0, agility: 0, wild: 0 },
        );
        assert_eq!(investigator_skills(&m), None);
    }
```

If `CardType::Asset` / `Class::Guardian` aren't the exact variant names, adjust to the real ones (check `crates/card-dsl/src/card_data.rs`); the *behavior* is what matters.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p game-core investigator_skills 2>&1 | grep -E "cannot find|error" | head`
Expected: FAIL — `cannot find function \`investigator_skills\``.

- [ ] **Step 3: Implement the helper**

Add near the top of `crates/game-core/src/engine/dispatch/phases.rs` (after the imports; extend the `use crate::card_data::…` / `use crate::state::…` lines as needed for `CardMetadata`, `CardType`, `Skills`):

```rust
/// Base skill values for an investigator card, or `None` for any other
/// card type. For investigator cards `skill_icons` carries the printed
/// base skills (`wild` is always 0 and ignored); this reinterprets them
/// as [`Skills`]. `i8::try_from` never fails for real base skills
/// (single-digit) — an impossible overflow yields `None` rather than a
/// panic. Lives here, not on `CardMetadata`, because `Skills` is a
/// `game-core` type and `card-dsl` must not depend on `game-core`.
fn investigator_skills(meta: &CardMetadata) -> Option<Skills> {
    if meta.card_type != CardType::Investigator {
        return None;
    }
    Some(Skills {
        willpower: i8::try_from(meta.skill_icons.willpower).ok()?,
        intellect: i8::try_from(meta.skill_icons.intellect).ok()?,
        combat: i8::try_from(meta.skill_icons.combat).ok()?,
        agility: i8::try_from(meta.skill_icons.agility).ok()?,
    })
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p game-core investigator_skills 2>&1 | grep -E "test result|investigator_skills"`
Expected: both tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/phases.rs
git commit -m "engine: investigator_skills helper (skill_icons -> Skills, type-guarded)

Part of #221.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Seat the roster in `start_scenario` (validate-first) + invariant

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs:19-64` (`start_scenario`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs:142` (pass roster)
- Test: `crates/game-core/src/engine/dispatch/phases.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing tests** (no card registry needed for these)

Append to the phases.rs test module. These exercise the registry-absent and zero-investigator paths, plus empty-roster passthrough:

```rust
    use crate::action::{PlayerAction, RosterEntry};
    use crate::engine::apply;
    use crate::action::Action;
    use crate::state::CardCode;
    use crate::test_support::builder::TestGame;
    use crate::test_support::fixtures::test_investigator;

    #[test]
    fn start_scenario_rejects_when_roster_would_seat_zero_investigators() {
        // Empty roster, no pre-seated investigators -> zero investigators.
        let state = TestGame::new().build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario { roster: vec![] }));
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.round, 0, "state unchanged on reject");
        assert!(result.events.is_empty(), "no events on reject");
    }

    #[test]
    fn start_scenario_empty_roster_passes_through_with_preseated_investigator() {
        // Pre-seated investigator + empty roster: ≥1 investigator, Done.
        let id = crate::state::InvestigatorId(1);
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario { roster: vec![] }));
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.round, 1);
    }

    #[test]
    fn start_scenario_rejects_non_empty_roster_when_no_registry_installed() {
        // game-core unit tests install no CardRegistry; resolving a code fails.
        let state = TestGame::new().build();
        let roster = vec![RosterEntry { investigator: CardCode::new("01001"), deck: vec![] }];
        let result = apply(state, Action::Player(PlayerAction::StartScenario { roster }));
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.round, 0, "state unchanged on reject");
        assert!(result.events.is_empty());
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p game-core start_scenario_rejects_when_roster 2>&1 | tail -15`
Expected: FAIL — the variant now needs `{ roster }` (compile error) and/or the zero-investigator reject doesn't exist yet (the current `start_scenario` returns `Done` with an empty `turn_order`).

- [ ] **Step 3: Thread the roster and implement validate-first seating**

In `crates/game-core/src/engine/dispatch/mod.rs:142`:

```rust
        PlayerAction::StartScenario { roster } => phases::start_scenario(cx, roster),
```

Rewrite `start_scenario` in `phases.rs`. Change the signature and insert resolve→invariant→seat **before** any mutation. Add `use crate::action::RosterEntry;` and `use crate::state::{CardCode, Investigator, Skills, Status};` imports as needed (some are already present):

```rust
pub(super) fn start_scenario(cx: &mut Cx, roster: &[RosterEntry]) -> EngineOutcome {
    if cx.state.round != 0 {
        return EngineOutcome::Rejected {
            reason: "StartScenario applied to a state that is already in progress".into(),
        };
    }

    // Validate-first: resolve every roster entry's stats from card data
    // before mutating anything. Any failure rejects with state unchanged.
    let registry = crate::card_registry::current();
    let mut resolved: Vec<(Skills, u8, u8, String, Vec<CardCode>)> = Vec::with_capacity(roster.len());
    for entry in roster {
        let Some(reg) = registry else {
            return EngineOutcome::Rejected {
                reason: "no card registry installed; cannot resolve investigator stats".into(),
            };
        };
        let Some(meta) = (reg.metadata_for)(&entry.investigator) else {
            return EngineOutcome::Rejected {
                reason: format!("unknown investigator code {}", entry.investigator),
            };
        };
        let (Some(skills), Some(health), Some(sanity)) =
            (investigator_skills(meta), meta.health, meta.sanity)
        else {
            return EngineOutcome::Rejected {
                reason: format!("card {} is not a seatable investigator", entry.investigator),
            };
        };
        resolved.push((skills, health, sanity, meta.name.clone(), entry.deck.clone()));
    }

    // A scenario requires at least one investigator (pre-seated or
    // roster-seated). In production setup() seats none, so this makes the
    // roster mandatory: an empty roster rejects. The pre-seated test path
    // (≥1 already present, empty roster) passes — temporary scaffolding
    // until TODO(#224) migrates tests to roster seating and tightens this
    // to require a non-empty roster.
    if cx.state.investigators.is_empty() && resolved.is_empty() {
        return EngineOutcome::Rejected {
            reason: "a scenario requires at least one investigator".into(),
        };
    }

    // --- mutate (all validations passed) ---
    // Seat resolved investigators. Ids are sequential (1-based) in roster
    // order; production seats into an empty investigator set.
    for (idx, (skills, health, sanity, name, deck)) in resolved.into_iter().enumerate() {
        let id = InvestigatorId(u32::try_from(idx).unwrap_or(0) + 1);
        cx.state.investigators.insert(
            id,
            Investigator {
                id,
                name,
                current_location: None,
                skills,
                max_health: health,
                damage: 0,
                max_sanity: sanity,
                horror: 0,
                clues: 0,
                resources: 5,
                actions_remaining: 0,
                status: Status::Active,
                deck,
                hand: Vec::new(),
                discard: Vec::new(),
                cards_in_play: Vec::new(),
                removed_from_game: Vec::new(),
            },
        );
        cx.state.turn_order.push(id);
    }

    cx.state.round = 1;
    cx.state.phase = Phase::Investigation;
    cx.events.push(Event::ScenarioStarted);

    let inv_ids: Vec<InvestigatorId> = cx.state.investigators.keys().copied().collect();
    for inv_id in inv_ids {
        super::cards::shuffle_player_deck(cx, inv_id);
        super::cards::draw_cards(cx, inv_id, super::cards::INITIAL_HAND_SIZE);
    }

    cx.state.mulligan_pending = super::cursor::first_active_investigator(cx.state);
    reset_actions(cx);
    EngineOutcome::Done
}
```

(Keep the existing explanatory comments from the original body where they still apply — round-1/Mythos-skip, mulligan-cursor seeding, action seed. They're elided above for brevity but should remain.)

- [ ] **Step 4: Run the new tests + the full game-core suite**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core 2>&1 | tail -8`
Expected: the three new tests pass; all pre-existing `start_scenario` tests still pass (they pre-seat investigators, empty roster).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/phases.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: seat StartScenario roster from card data (validate-first)

Resolve each roster entry's stats via CardRegistry/CardMetadata, seat with
the payload deck, reject unless >=1 investigator ends up seated. Pre-seated
empty-roster test path tolerated until #224.

Part of #221.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Integration test — seat Roland from the real corpus

**Files:**
- Create: `crates/cards/tests/roster_seating.rs`

- [ ] **Step 1: Write the test**

```rust
//! B2: seating a roster resolves investigator stats from the real corpus
//! (CardRegistry) and takes the deck from the payload. Integration test so
//! it can install `cards::REGISTRY` in its own process (per CLAUDE.md test
//! layering).

use game_core::action::{Action, PlayerAction, RosterEntry};
use game_core::engine::apply;
use game_core::state::{CardCode, InvestigatorId, Skills};
use game_core::test_support::builder::TestGame;
use game_core::EngineOutcome;

fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

#[test]
fn seats_roland_with_corpus_stats_and_payload_deck() {
    install_registry();
    let deck = vec![CardCode::new("01030"), CardCode::new("01030")];
    let roster = vec![RosterEntry { investigator: CardCode::new("01001"), deck: deck.clone() }];
    let state = TestGame::new().build();

    let result = apply(state, Action::Player(PlayerAction::StartScenario { roster }));

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = result
        .state
        .investigators
        .get(&InvestigatorId(1))
        .expect("Roland seated at id 1");
    assert_eq!(inv.name, "Roland Banks");
    assert_eq!(inv.skills, Skills { willpower: 3, intellect: 3, combat: 4, agility: 2 });
    assert_eq!(inv.max_health, 9);
    assert_eq!(inv.max_sanity, 5);
    // Deck (2) + hand were sourced from the payload; combined they account
    // for the 2 supplied cards (5-card opening hand draws what's available).
    assert_eq!(inv.deck.len() + inv.hand.len(), deck.len());
}

#[test]
fn rejects_non_investigator_code() {
    install_registry();
    // 01030 (Magnifying Glass) is an Asset, not an investigator.
    let roster = vec![RosterEntry { investigator: CardCode::new("01030"), deck: vec![] }];
    let state = TestGame::new().build();
    let result = apply(state, Action::Player(PlayerAction::StartScenario { roster }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0);
    assert!(result.events.is_empty());
}

#[test]
fn rejects_unknown_code() {
    install_registry();
    let roster = vec![RosterEntry { investigator: CardCode::new("99999"), deck: vec![] }];
    let state = TestGame::new().build();
    let result = apply(state, Action::Player(PlayerAction::StartScenario { roster }));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0);
    assert!(result.events.is_empty());
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p cards --test roster_seating 2>&1 | tail -12`
Expected: all three PASS. If `inv.skills`/field names differ, reconcile against `crates/game-core/src/state/investigator.rs`. If `game_core::engine::apply` / `game_core::EngineOutcome` import paths differ, match `crates/cards/tests/play_card.rs`'s imports.

- [ ] **Step 3: Commit**

```bash
git add crates/cards/tests/roster_seating.rs
git commit -m "test: integration — seat Roland (01001) from corpus + reject paths

Part of #221.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Full strict gauntlet, PR, phase doc

**Files:** none (verification), then phase doc.

- [ ] **Step 1: Run the full gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. `RosterEntry`'s public fields and the new `StartScenario` variant carry doc comments (doc job). Fix any clippy lint on the new code (e.g. prefer `let-else`, which is already used).

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin engine/roster-seating
gh pr create --fill --label engine
```

PR body: design-decisions paragraph — no new registry (stats from `CardMetadata` via `CardRegistry`); deck is player-supplied payload input (Phase-9 forward-compatible); reject-if-zero-investigators invariant with the synthetic pre-seeded test path tolerated until #224. End with `Closes #221.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch` (background). Expected: all seven jobs green.

- [ ] **Step 4: Phase doc (final commit, only once CI is green)**

In `docs/phases/phase-7-the-gathering.md`: flip the B2 row to `✅ PR #<PR#>` with the plan link. Add a **Decisions made** entry only if it passes the would-a-future-author-choose-differently test — candidate: *"Investigator stats are read from `CardMetadata` (existing `CardRegistry`), not a new registry; the deck is a player-supplied `RosterEntry` payload field (Phase-9 import seam). `start_scenario` rejects unless ≥1 investigator is seated; the pre-seated test path is tolerated until #224."*

---

## Self-Review

**Spec coverage:**
- Component 1 (`investigator_skills`, layering-correct as a `game-core` free fn) → Task 2. ✅
- Component 2 (`StartScenario { roster }`, `RosterEntry`, deck-in-payload, serde) → Task 1. ✅
- Component 3 (validate-first seating, resolve via `CardRegistry`/`CardMetadata`, ids/turn-order/location/resources, ≥1-investigator invariant) → Task 3. ✅
- Error handling (all reject paths, state unchanged) → Task 3 (no-registry, zero-investigator) + Task 4 (unknown, non-investigator). ✅
- Scaffolding/#224 tolerance → Task 3 comment + Task 5 phase-doc decision. ✅
- Testing (integration seats 01001; unit reject/passthrough) → Tasks 3 & 4. ✅
- Deferrals (placement at `None`, Roland deck contents) → seating sets `current_location: None`; deck supplied inline by the test, real contents are Group C. ✅

**Placeholder scan:** No TBD/TODO-as-instruction; every code step shows full code; the only `TODO(#224)` is a deliberate source comment, not a plan gap. ✅

**Type consistency:** `RosterEntry { investigator: CardCode, deck: Vec<CardCode> }`, `investigator_skills(&CardMetadata) -> Option<Skills>`, `start_scenario(cx, roster: &[RosterEntry])`, and `StartScenario { roster }` are used identically across Tasks 1–4. `Skills`/`SkillIcons` field names match the codebase. ✅
