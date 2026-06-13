# Phase 7 C2 — Symbol tokens + location victory points Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve The Gathering reference card 01104's chaos-symbol effects (skull/cultist/tablet) during skill tests, and place victory-point locations (Attic/Cellar) into a victory display at scenario end.

**Architecture:** Symbol effects are scenario behaviour, modelled as a `resolve_symbol` function pointer on `ScenarioModule` (not card `abilities()`) that returns plain data (`SymbolOutcome { modifier, immediate, on_fail }`); the skill-test dispatch applies the modifier to the total and routes side effects through the existing damage/horror paths. This replaces B1's now-dead `reference_card` field. Victory points accumulate in a new `GameState.victory_display` zone via a generic engine scan at the resolution chokepoint.

**Tech Stack:** Rust workspace (`game-core` kernel, `scenarios` content, `cards` corpus). Tests: game-core `#[cfg(test)]` unit tests + `crates/scenarios/tests/` integration tests (own process, install both registries).

**Spec:** `docs/superpowers/specs/2026-06-13-phase-7-c2-symbol-tokens-and-victory-design.md`

**Branch:** `card/symbol-tokens-victory` (already created; spec already committed).

**CI gauntlet (run before any push):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

---

## File structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/game-core/src/scenario.rs` | `ScenarioModule` field swap; new `SymbolCtx`/`SymbolOutcome`/`TokenEffect`; `resolve_symbol_token` lookup helper; delete dead B1 lookup + tests | 1, 2 |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | `apply_symbol_outcome` helper; wire symbol path into `resolve_chaos_token_and_emit` | 2, 3 |
| `crates/game-core/src/state/game_state.rs`, `state/builder.rs` | `victory_display: Vec<CardCode>` field | 5 |
| `crates/game-core/src/event.rs` | `EnteredVictoryDisplay` event variant | 5 |
| `crates/game-core/src/engine/mod.rs` | VP scan in `fire_scenario_resolution`; update `STAMP_MODULE` literal | 1, 5 |
| `crates/scenarios/src/the_gathering.rs` | 01104 `resolve_symbol` hook + ghoul count; drop placeholder token modifiers | 4 |
| `crates/scenarios/src/test_fixtures/synthetic.rs`, `crates/game-core/src/scenario_registry.rs`, `crates/server/tests/common/mod.rs`, `crates/server/tests/game_session.rs` | `reference_card` → `resolve_symbol: None` in module literals | 1 |
| `crates/scenarios/src/lib.rs` | drop `reference_card` assertion | 1 |
| `crates/scenarios/tests/the_gathering_symbols.rs` (new) | integration tests: skull/cultist/tablet + VP | 4, 5 |

---

## Task 1: `ScenarioModule` field swap + new symbol types + delete dead B1 plumbing

Pure refactor: introduces the new types and the `resolve_symbol` field (set to `None` everywhere for now — the real hook lands in Task 4), and removes B1's unused `reference_card` field, `active_reference_card`, and `reference_card_with_registry`. No behaviour change; the build must stay green.

**Files:**
- Modify: `crates/game-core/src/scenario.rs`
- Modify: `crates/game-core/src/scenario_registry.rs:80`
- Modify: `crates/game-core/src/engine/mod.rs:4021-4025` (STAMP_MODULE)
- Modify: `crates/scenarios/src/the_gathering.rs:44-47, 175-179`
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs:136`
- Modify: `crates/scenarios/src/lib.rs:74`
- Modify: `crates/server/tests/common/mod.rs:39`
- Modify: `crates/server/tests/game_session.rs:30`

- [ ] **Step 1: Add the new types to `scenario.rs`**

Add near the top of `crates/game-core/src/scenario.rs` (after the existing `use` lines; you will need `ChaosToken`, `InvestigatorId`, `LocationId` in scope — add them to the `crate::state::{…}` import):

```rust
/// Read-only board view handed to a scenario's symbol-token hook
/// ([`ScenarioModule::resolve_symbol`]). Carries the testing investigator
/// and the live state so the hook can compute board-dependent values
/// (e.g. "number of Ghoul enemies at your location").
pub struct SymbolCtx<'a> {
    state: &'a GameState,
    investigator: InvestigatorId,
}

impl<'a> SymbolCtx<'a> {
    /// Construct a context for `investigator` over `state`.
    #[must_use]
    pub fn new(state: &'a GameState, investigator: InvestigatorId) -> Self {
        Self { state, investigator }
    }

    /// The full game state (read-only).
    #[must_use]
    pub fn state(&self) -> &GameState {
        self.state
    }

    /// The investigator whose skill test drew the symbol.
    #[must_use]
    pub fn investigator(&self) -> InvestigatorId {
        self.investigator
    }

    /// The testing investigator's current location, if placed.
    #[must_use]
    pub fn investigator_location(&self) -> Option<LocationId> {
        self.state
            .investigators
            .get(&self.investigator)
            .and_then(|inv| inv.current_location)
    }
}

/// What a drawn chaos **symbol** token does this skill test: a numeric
/// modifier plus side effects, split by resolution timing.
///
/// The `modifier` is applied to the skill total *before* success/failure
/// is computed; `immediate` effects apply regardless of outcome (e.g.
/// 01104 tablet's board-gated damage); `on_fail` effects apply only when
/// the test fails (e.g. 01104 cultist's horror). The hook is evaluated
/// once at token reveal, so board-gated branches are decided up front.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolOutcome {
    /// Added to the test's skill total.
    pub modifier: i8,
    /// Applied to the testing investigator regardless of pass/fail.
    pub immediate: Vec<TokenEffect>,
    /// Applied to the testing investigator only if the test fails.
    pub on_fail: Vec<TokenEffect>,
}

/// A symbol token's side effect on the testing investigator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenEffect {
    /// Deal N damage to the testing investigator.
    Damage(u8),
    /// Deal N horror to the testing investigator.
    Horror(u8),
}
```

- [ ] **Step 2: Swap the `ScenarioModule` field**

In `crates/game-core/src/scenario.rs`, in `pub struct ScenarioModule`, **delete** the `reference_card: &'static str` field (and its doc comment), and **add**:

```rust
    /// Resolve a drawn chaos **symbol** token (Skull/Cultist/Tablet/
    /// ElderThing) against live board state. `None` means this scenario
    /// has no reference-card symbol effects (test fixtures); the engine
    /// then falls back to the static [`TokenModifiers`](crate::state::TokenModifiers)
    /// table. Never called for Numeric/AutoFail/ElderSign tokens.
    pub resolve_symbol: Option<fn(crate::state::ChaosToken, &SymbolCtx) -> SymbolOutcome>,
```

- [ ] **Step 3: Delete the dead B1 lookup functions and their tests**

In `crates/game-core/src/scenario.rs`, delete `pub fn active_reference_card` and `fn reference_card_with_registry` entirely. In the `#[cfg(test)] mod tests` of that file, delete the tests that exercise them (`returns_reference_card_for_active_scenario`, and any test whose body calls `reference_card_with_registry` or references a `registry()` fixture built solely for them) and the `reference_card: "01104"` test fixture. Keep any unrelated tests.

- [ ] **Step 4: Update every `ScenarioModule` literal**

Replace `reference_card: <x>,` with `resolve_symbol: None,` in each literal:
- `crates/game-core/src/scenario_registry.rs:80`
- `crates/game-core/src/engine/mod.rs` `STAMP_MODULE` (remove `reference_card: "",`, add `resolve_symbol: None,`)
- `crates/scenarios/src/test_fixtures/synthetic.rs:136`
- `crates/server/tests/common/mod.rs:39`
- `crates/server/tests/game_session.rs:30`

In `crates/scenarios/src/the_gathering.rs`: delete the `REFERENCE_CARD` const (lines ~46-47) and its doc comment; in `MODULE` replace `reference_card: REFERENCE_CARD,` with `resolve_symbol: None,` (Task 4 flips this to `Some(...)`).

In `crates/scenarios/src/lib.rs`, delete the assertion line `assert_eq!(module.reference_card, "01104");` (around line 74). If that leaves a test asserting nothing meaningful, replace the assertion with `assert!(module.resolve_symbol.is_none());` to keep a lookup smoke-test.

- [ ] **Step 5: Add a unit test for the new types**

In `crates/game-core/src/scenario.rs` `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn symbol_outcome_default_is_inert() {
    let out = SymbolOutcome::default();
    assert_eq!(out.modifier, 0);
    assert!(out.immediate.is_empty());
    assert!(out.on_fail.is_empty());
}

#[test]
fn token_effect_variants_construct() {
    assert_eq!(TokenEffect::Damage(1), TokenEffect::Damage(1));
    assert_ne!(TokenEffect::Damage(1), TokenEffect::Horror(1));
}
```

- [ ] **Step 6: Build and test**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS. (If a deleted test or `reference_card` reference still lingers, the compiler points at it — remove it.)

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "scenario: swap reference_card for resolve_symbol hook + symbol types

Replace B1's dead reference_card field (zero call sites) with a
resolve_symbol fn pointer on ScenarioModule, and add the SymbolCtx /
SymbolOutcome / TokenEffect data types. No behaviour change yet; every
module sets resolve_symbol: None."
```

---

## Task 2: `apply_symbol_outcome` + `resolve_symbol_token` helpers

The two helpers the wiring (Task 3) needs: one applies a `SymbolOutcome`'s side effects via the existing elimination paths; the other looks up the active scenario's hook through the global scenario registry.

**Files:**
- Modify: `crates/game-core/src/scenario.rs`
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs`
- Test: `crates/game-core/src/engine/dispatch/skill_test.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Add the lookup helper to `scenario.rs`**

```rust
/// Resolve a drawn chaos symbol token against the active scenario's
/// reference-card effects, if any. Routes
/// `state.scenario_id` → installed scenario registry → `module_for` →
/// [`ScenarioModule::resolve_symbol`]. Returns `None` when there is no
/// active scenario, no registry, an unknown id, or the module has no
/// symbol hook — callers then fall back to the static
/// [`TokenModifiers`](crate::state::TokenModifiers) path.
#[must_use]
pub fn resolve_symbol_token(
    state: &GameState,
    token: crate::state::ChaosToken,
    investigator: InvestigatorId,
) -> Option<SymbolOutcome> {
    let id = state.scenario_id.as_ref()?;
    let registry = crate::scenario_registry::current()?;
    let module = (registry.module_for)(id)?;
    let hook = module.resolve_symbol?;
    Some(hook(token, &SymbolCtx::new(state, investigator)))
}
```

- [ ] **Step 2: Write the failing test for `apply_symbol_outcome`**

In `crates/game-core/src/engine/dispatch/skill_test.rs` `#[cfg(test)] mod tests` (check the existing test module's imports; add `SymbolOutcome`, `TokenEffect` from `crate::scenario`, and `Cx` from `crate::engine::cx` as needed):

```rust
#[test]
fn apply_symbol_outcome_runs_immediate_always_and_on_fail_only_on_failure() {
    use crate::scenario::{SymbolOutcome, TokenEffect};
    let inv = InvestigatorId(1);

    // Helper to run one outcome against a fresh state at a given outcome.
    let run = |succeeded: bool, outcome: SymbolOutcome| {
        let mut state = crate::test_support::TestGame::new()
            .with_investigator(crate::test_support::test_investigator(1))
            .build();
        let mut events = Vec::new();
        let mut cx = crate::engine::cx::Cx { state: &mut state, events: &mut events };
        super::apply_symbol_outcome(&mut cx, inv, &outcome, succeeded);
        events
    };

    // immediate Damage(1) applies on success.
    let ev = run(true, SymbolOutcome { modifier: 0, immediate: vec![TokenEffect::Damage(1)], on_fail: vec![] });
    assert_event!(ev, Event::DamageTaken { investigator, amount: 1, .. } if *investigator == inv);

    // on_fail Horror(1) does NOT apply on success.
    let ev = run(true, SymbolOutcome { modifier: 0, immediate: vec![], on_fail: vec![TokenEffect::Horror(1)] });
    assert_no_event!(ev, Event::HorrorTaken { .. });

    // on_fail Horror(1) DOES apply on failure.
    let ev = run(false, SymbolOutcome { modifier: 0, immediate: vec![], on_fail: vec![TokenEffect::Horror(1)] });
    assert_event!(ev, Event::HorrorTaken { investigator, amount: 1, .. } if *investigator == inv);
}
```

(Confirm the exact `DamageTaken`/`HorrorTaken` field names against `crates/game-core/src/event.rs` and adjust the patterns if the fields differ.)

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p game-core apply_symbol_outcome_runs_immediate`
Expected: FAIL — `apply_symbol_outcome` not found.

- [ ] **Step 4: Implement `apply_symbol_outcome`**

In `crates/game-core/src/engine/dispatch/skill_test.rs` (non-test module):

```rust
/// Apply a resolved symbol token's side effects to the testing
/// investigator: `immediate` effects always, `on_fail` effects only when
/// the test failed. Routes through the same elimination paths as
/// `Effect::DealDamage` / `Effect::DealHorror`, so defeat handling and
/// the `DamageTaken` / `HorrorTaken` events are reused.
fn apply_symbol_outcome(
    cx: &mut Cx,
    investigator: InvestigatorId,
    outcome: &crate::scenario::SymbolOutcome,
    succeeded: bool,
) {
    use crate::scenario::TokenEffect;
    let mut effects: Vec<TokenEffect> = outcome.immediate.clone();
    if !succeeded {
        effects.extend(outcome.on_fail.iter().copied());
    }
    for effect in effects {
        match effect {
            TokenEffect::Damage(n) => {
                crate::engine::dispatch::elimination::take_damage(cx, investigator, n);
            }
            TokenEffect::Horror(n) => {
                crate::engine::dispatch::elimination::take_horror(cx, investigator, n);
            }
        }
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p game-core apply_symbol_outcome_runs_immediate`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "skill-test: symbol-outcome application + scenario hook lookup

apply_symbol_outcome routes a SymbolOutcome's immediate/on_fail effects
through the existing damage/horror elimination paths; resolve_symbol_token
looks the active scenario's hook up via the global scenario registry."
```

---

## Task 3: Wire the symbol path into `resolve_chaos_token_and_emit`

When a symbol token is drawn and the active scenario has a hook, use the hook's modifier as the token resolution and apply its side effects after success/failure is known. Otherwise the existing static path runs unchanged.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:384-421`

- [ ] **Step 1: Rewrite `resolve_chaos_token_and_emit`'s resolution computation**

Replace the body from the `let resolution = resolve_token(...)` line through the success/fail emission with:

```rust
    let token_idx = cx.state.rng.next_index(cx.state.chaos_bag.tokens.len());
    let token = cx.state.chaos_bag.tokens[token_idx];

    // Symbol tokens may route to the active scenario's reference-card
    // effects (modifier + deferred side effects). Numeric/AutoFail/
    // ElderSign never do; nor do scenarios without a hook (static path).
    let symbol_outcome = match token {
        ChaosToken::Skull | ChaosToken::Cultist | ChaosToken::Tablet | ChaosToken::ElderThing => {
            crate::scenario::resolve_symbol_token(cx.state, token, investigator)
        }
        _ => None,
    };

    let resolution = match &symbol_outcome {
        Some(outcome) => TokenResolution::Modifier(outcome.modifier),
        None => resolve_token(token, &cx.state.token_modifiers),
    };
    cx.events
        .push(Event::ChaosTokenRevealed { token, resolution });

    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => (skill_value.saturating_add(n).max(0), None),
        TokenResolution::ElderSign => (skill_value.max(0), None),
        TokenResolution::AutoFail => (0, Some(FailureReason::AutoFail)),
    };
    let margin = total.saturating_sub(difficulty);
    let succeeded = margin >= 0 && fail_reason.is_none();
    if succeeded {
        cx.events.push(Event::SkillTestSucceeded { investigator, skill, margin });
    } else {
        let reason = fail_reason.unwrap_or(FailureReason::Total);
        let by = difficulty.saturating_sub(total);
        cx.events.push(Event::SkillTestFailed { investigator, skill, reason, by });
    }

    // Symbol side effects resolve after success/failure is known.
    if let Some(outcome) = symbol_outcome {
        apply_symbol_outcome(cx, investigator, &outcome, succeeded);
    }

    succeeded
```

(Ensure `ChaosToken` is imported in `skill_test.rs` — add to the `use super::{…}` / `crate::state::{…}` imports if not already present.)

- [ ] **Step 2: Run the regression test**

Run: `cargo test -p game-core perform_skill_test_symbol_token_modifier_applies`
Expected: PASS — with no scenario registry installed in game-core unit tests, the Skull token still falls through to the static `night_of_the_zealot_standard()` path (`Modifier(-1)`), proving backward compatibility.

- [ ] **Step 3: Run the full game-core suite**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "skill-test: route symbol tokens through the scenario hook

Symbol draws now consult the active scenario's resolve_symbol hook for a
dynamic modifier + deferred side effects; hook-less scenarios keep the
static TokenModifiers path (regression-guarded)."
```

---

## Task 4: 01104 hook in The Gathering + symbol integration tests

Implement The Gathering's actual skull/cultist/tablet effects in plain Rust and prove them end-to-end with the real corpus (Ghoul metadata) and scenario module installed.

**Files:**
- Modify: `crates/scenarios/src/the_gathering.rs`
- Create: `crates/scenarios/tests/the_gathering_symbols.rs`

- [ ] **Step 1: Implement the hook + ghoul count in `the_gathering.rs`**

Add to `crates/scenarios/src/the_gathering.rs` (import `ChaosToken`, `InvestigatorId`, `LocationId`, and `SymbolCtx`, `SymbolOutcome`, `TokenEffect` from `game_core`):

```rust
use game_core::scenario::{SymbolCtx, SymbolOutcome, TokenEffect};
use game_core::state::InvestigatorId;

/// Number of Ghoul-trait enemies at the testing investigator's location.
fn ghoul_count_at_investigator_location(cx: &SymbolCtx) -> u8 {
    let Some(loc) = cx.investigator_location() else {
        return 0;
    };
    let n = cx
        .state()
        .enemies
        .values()
        .filter(|e| e.current_location == Some(loc) && e.traits.iter().any(|t| t == "Ghoul"))
        .count();
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// 01104 The Gathering chaos-symbol effects (verified card text):
/// `[skull]` −X (X = Ghouls at your location); `[cultist]` −1, 1 horror
/// on failure; `[tablet]` −2, 1 damage if a Ghoul is at your location.
/// The Gathering's Standard bag has no Elder Thing token.
fn resolve_symbol(token: ChaosToken, cx: &SymbolCtx) -> SymbolOutcome {
    let ghouls = ghoul_count_at_investigator_location(cx);
    match token {
        ChaosToken::Skull => SymbolOutcome {
            modifier: -(i8::try_from(ghouls).unwrap_or(i8::MAX)),
            ..SymbolOutcome::default()
        },
        ChaosToken::Cultist => SymbolOutcome {
            modifier: -1,
            on_fail: vec![TokenEffect::Horror(1)],
            ..SymbolOutcome::default()
        },
        ChaosToken::Tablet => SymbolOutcome {
            modifier: -2,
            immediate: if ghouls > 0 { vec![TokenEffect::Damage(1)] } else { vec![] },
            ..SymbolOutcome::default()
        },
        _ => SymbolOutcome::default(),
    }
}
```

- [ ] **Step 2: Wire the hook into `MODULE` and drop placeholder modifiers**

In `MODULE`, set `resolve_symbol: Some(resolve_symbol),`. In `setup()`, delete the four placeholder `token_modifiers.skull/cultist/tablet/elder_thing` assignments and the `with_token_modifiers(token_modifiers)` builder call (and the now-unused `TokenModifiers` import / `token_modifiers` local). The static table is no longer consulted for The Gathering (the hook owns symbols; Numeric tokens never used it).

- [ ] **Step 3: Run the scenario unit tests**

Run: `RUSTFLAGS="-D warnings" cargo test -p scenarios --lib`
Expected: PASS (the existing `setup_*` tests don't assert token modifiers; if one does, remove that assertion).

- [ ] **Step 4: Write the integration test file**

Create `crates/scenarios/tests/the_gathering_symbols.rs`:

```rust
//! C2: 01104 reference-card symbol-token effects, end-to-end through the
//! real card registry (Ghoul metadata) + the installed scenario module.
//! Own process so the global registries can be installed once.

use game_core::action::{Action, PlayerAction};
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
    TokenResolution,
};
use game_core::test_support::{test_enemy, test_investigator, test_location};
use scenarios::REGISTRY;

fn install_registries() {
    let _ = game_core::scenario_registry::install(REGISTRY);
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Build a Gathering state: one investigator at one location, a single
/// chaos token in the bag, optionally `ghouls` Ghoul enemies co-located.
fn gathering_state(token: ChaosToken, ghouls: u8) -> game_core::state::GameState {
    let inv = InvestigatorId(1);
    let loc = LocationId(1);
    let mut investigator = test_investigator(1);
    investigator.current_location = Some(loc);
    // Generous stats so the modifier alone decides pass/fail in tests.
    let mut state = GameStateBuilder::new()
        .with_phase(game_core::state::Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .with_chaos_bag(ChaosBag::new([token]))
        .with_scenario_id(game_core::scenario::ScenarioId::new(scenarios::the_gathering::ID))
        .build();
    state.locations.insert(loc, test_location(1, "Study"));
    for i in 0..ghouls {
        let mut e = test_enemy(EnemyId(u32::from(i) + 1), "Ghoul");
        e.traits = vec!["Ghoul".to_string()];
        e.current_location = Some(loc);
        state.enemies.insert(e.id, e);
    }
    state
}

fn perform(state: game_core::state::GameState, difficulty: i8) -> game_core::engine::ApplyResult {
    let r = apply(
        state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            difficulty,
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    r
}

#[test]
fn skull_subtracts_ghoul_count_at_location() {
    install_registries();
    // 0 Ghouls → modifier 0; 2 Ghouls → modifier -2.
    let r0 = perform(gathering_state(ChaosToken::Skull, 0), 0);
    assert!(r0.events.iter().any(|e| matches!(
        e,
        Event::ChaosTokenRevealed { token: ChaosToken::Skull, resolution: TokenResolution::Modifier(0) }
    )));
    let r2 = perform(gathering_state(ChaosToken::Skull, 2), 0);
    assert!(r2.events.iter().any(|e| matches!(
        e,
        Event::ChaosTokenRevealed { token: ChaosToken::Skull, resolution: TokenResolution::Modifier(-2) }
    )));
}

#[test]
fn cultist_is_minus_one_and_horror_only_on_failure() {
    install_registries();
    // -1 modifier always. Difficulty 99 forces failure → 1 horror.
    let fail = perform(gathering_state(ChaosToken::Cultist, 0), 99);
    assert!(fail.events.iter().any(|e| matches!(
        e,
        Event::ChaosTokenRevealed { token: ChaosToken::Cultist, resolution: TokenResolution::Modifier(-1) }
    )));
    assert!(fail.events.iter().any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })));
    // Difficulty 0 → success → no horror.
    let win = perform(gathering_state(ChaosToken::Cultist, 0), 0);
    assert!(!win.events.iter().any(|e| matches!(e, Event::HorrorTaken { .. })));
}

#[test]
fn tablet_is_minus_two_and_damage_iff_ghoul_present() {
    install_registries();
    // Ghoul present → 1 damage regardless of pass/fail (difficulty 0 → success).
    let with_ghoul = perform(gathering_state(ChaosToken::Tablet, 1), 0);
    assert!(with_ghoul.events.iter().any(|e| matches!(
        e,
        Event::ChaosTokenRevealed { token: ChaosToken::Tablet, resolution: TokenResolution::Modifier(-2) }
    )));
    assert!(with_ghoul.events.iter().any(|e| matches!(e, Event::DamageTaken { amount: 1, .. })));
    // No Ghoul → no damage.
    let no_ghoul = perform(gathering_state(ChaosToken::Tablet, 0), 0);
    assert!(!no_ghoul.events.iter().any(|e| matches!(e, Event::DamageTaken { .. })));
}
```

(Verify `test_location`'s signature in `crates/game-core/src/test_support/`; it is `test_location(id: u32, name: &str) -> Location`. Adjust the call if the arity differs. Also confirm `scenarios::the_gathering` and `::ID` are publicly reachable; if `the_gathering` is private, add `pub use` or reference `scenarios::REGISTRY` lookup instead, matching the existing `tests/the_gathering.rs` import of `the_gathering`.)

- [ ] **Step 5: Run the integration tests**

Run: `cargo test -p scenarios --test the_gathering_symbols`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "the-gathering: 01104 skull/cultist/tablet symbol effects

skull -X (Ghoul count at your location), cultist -1 + horror on fail,
tablet -2 + damage if a Ghoul is co-located. Plain-Rust hook on the
scenario module; integration-tested through the real corpus."
```

---

## Task 5: Location victory points at scenario end

Add the victory-display zone and a generic engine scan that places in-play, revealed, clue-less victory locations into it when the scenario resolves.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (field + doc)
- Modify: `crates/game-core/src/state/builder.rs:256-290` (initialise field)
- Modify: `crates/game-core/src/event.rs` (new variant)
- Modify: `crates/game-core/src/engine/mod.rs` (`fire_scenario_resolution`)
- Test: `crates/game-core/src/engine/mod.rs` (`#[cfg(test)]`); `crates/scenarios/tests/the_gathering_symbols.rs`

- [ ] **Step 1: Add the `victory_display` field**

In `crates/game-core/src/state/game_state.rs`, add to `struct GameState` (near `resolution`):

```rust
    /// The victory display (Rules Reference p.21): an out-of-play zone of
    /// cards worth experience, scored at scenario end. Victory-point
    /// locations are placed here when the scenario resolves (in play +
    /// revealed + no clues); victory-point enemies enter as defeated
    /// (C3). Phase 9 sums these cards' corpus victory values for XP.
    pub victory_display: Vec<CardCode>,
```

In `crates/game-core/src/state/builder.rs`, in the `GameState { … }` literal inside `build()`, add `victory_display: Vec::new(),`.

- [ ] **Step 2: Add the `EnteredVictoryDisplay` event**

In `crates/game-core/src/event.rs`, add a variant (place near `ScenarioResolved`):

```rust
    /// A card was placed in the victory display (Rules Reference p.21).
    /// Emitted for each victory-point location at scenario resolution.
    EnteredVictoryDisplay {
        /// The placed card's printed code.
        code: CardCode,
        /// Its corpus victory value.
        victory: u8,
    },
```

- [ ] **Step 3: Write the failing game-core unit test (graceful no-registry path)**

In `crates/game-core/src/engine/mod.rs` tests, add (reuse `terminal_act_state` + `stamp_module_for` patterns already present):

```rust
#[test]
fn resolution_places_no_victory_without_card_registry() {
    // No card registry installed in this unit-test process → victory
    // metadata is unreachable → nothing is placed, no event, no panic.
    let state = terminal_act_state(Some("stamp"));
    let reg = ScenarioRegistry { module_for: stamp_module_for };
    let result = super::apply_with_scenario_registry(
        state,
        Action::Player(PlayerAction::AdvanceAct { investigator: InvestigatorId(1) }),
        Some(&reg),
    );
    assert!(result.state.victory_display.is_empty());
    assert_no_event!(result.events, Event::EnteredVictoryDisplay { .. });
}
```

- [ ] **Step 4: Run it to verify it fails**

Run: `cargo test -p game-core resolution_places_no_victory_without_card_registry`
Expected: FAIL — `victory_display` / `EnteredVictoryDisplay` not yet referenced by the scan (compile error or assertion); this also confirms the field/event exist.

- [ ] **Step 5: Add the VP scan to `fire_scenario_resolution`**

In `crates/game-core/src/engine/mod.rs`, in `fire_scenario_resolution`, after the `ScenarioResolved` event is pushed (and independent of the scenario-module lookup that follows), insert:

```rust
    // Place victory-point locations in the victory display (RR p.21:
    // "at the end of a scenario, place each victory point location that
    // is in play, revealed, and with no clues on it"). Generic across
    // scenarios; reads victory values from the card registry. No registry
    // → no metadata → nothing placed (graceful).
    if let Some(card_reg) = crate::card_registry::current() {
        let placed: Vec<(crate::state::CardCode, u8)> = cx
            .state
            .locations
            .values()
            .filter(|loc| loc.revealed && loc.clues == 0)
            .filter_map(|loc| {
                let meta = (card_reg.metadata_for)(&loc.code)?;
                match meta.kind {
                    crate::card_data::CardKind::Location { victory: Some(v), .. } if v > 0 => {
                        Some((loc.code.clone(), v))
                    }
                    _ => None,
                }
            })
            .collect();
        for (code, victory) in placed {
            cx.state.victory_display.push(code.clone());
            cx.events.push(Event::EnteredVictoryDisplay { code, victory });
        }
    }
```

(`locations` is a `BTreeMap` keyed by `LocationId`, so `.values()` iterates in deterministic id order. Confirm the `CardKind` import path — it is `crate::card_data::CardKind`, re-exported per the kernel's `card_data` alias.)

- [ ] **Step 6: Run the unit test to verify it passes**

Run: `cargo test -p game-core resolution_places_no_victory_without_card_registry`
Expected: PASS.

- [ ] **Step 7: Add the integration test (corpus victory values)**

Append to `crates/scenarios/tests/the_gathering_symbols.rs`:

```rust
use game_core::state::{Act, CardCode, Resolution};

/// A terminal-act Gathering state holding one clue, with `attic` either
/// cleared-and-revealed or not, so a single AdvanceAct latches Won and
/// triggers the victory-display scan.
fn resolvable_state_with_attic(revealed: bool, clues: u8) -> game_core::state::GameState {
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 1;
    let mut state = GameStateBuilder::new()
        .with_phase(game_core::state::Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .with_scenario_id(game_core::scenario::ScenarioId::new(scenarios::the_gathering::ID))
        .build();
    // Attic 01113 (corpus victory = 1).
    let mut attic = test_location(1, "Attic");
    attic.code = CardCode("01113".into());
    attic.revealed = revealed;
    attic.clues = clues;
    state.locations.insert(attic.id, attic);
    state.act_deck = vec![Act {
        code: CardCode("_test_act".into()),
        clue_threshold: 1,
        resolution: Some(Resolution::Won { id: "R1".into() }),
    }];
    state
}

fn advance_to_resolution(state: game_core::state::GameState) -> game_core::engine::ApplyResult {
    let r = apply(
        state,
        Action::Player(PlayerAction::AdvanceAct { investigator: InvestigatorId(1) }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    r
}

#[test]
fn cleared_revealed_victory_location_enters_victory_display() {
    install_registries();
    let r = advance_to_resolution(resolvable_state_with_attic(true, 0));
    assert!(r.state.victory_display.contains(&CardCode("01113".into())));
    assert!(r.events.iter().any(|e| matches!(
        e,
        Event::EnteredVictoryDisplay { code, victory: 1 } if code.as_str() == "01113"
    )));
}

#[test]
fn unrevealed_or_clued_victory_location_is_not_placed() {
    install_registries();
    // Has clues → not placed.
    let clued = advance_to_resolution(resolvable_state_with_attic(true, 2));
    assert!(clued.state.victory_display.is_empty());
    // Unrevealed → not placed.
    let unrevealed = advance_to_resolution(resolvable_state_with_attic(false, 0));
    assert!(unrevealed.state.victory_display.is_empty());
}
```

(Confirm `Location` exposes public `code`/`revealed`/`clues` fields for post-construction mutation — it does per `state/location.rs`. Confirm `CardCode::as_str` exists — it does. If `AdvanceAct` needs the investigator at a location or other preconditions, mirror whatever `terminal_act_state`-based tests in `engine/mod.rs` rely on; adjust the builder accordingly.)

- [ ] **Step 8: Run the integration tests**

Run: `cargo test -p scenarios --test the_gathering_symbols`
Expected: PASS (5 tests total).

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "engine: place victory-point locations in the victory display

At scenario resolution, scan in-play/revealed/clue-less locations and
place victory-bearing ones (Attic/Cellar) into a new GameState
victory_display zone (RR p.21), emitting EnteredVictoryDisplay. Generic
across scenarios; enemy victory path is C3."
```

---

## Task 6: Full gauntlet + phase doc (final, after CI green)

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md` (Group C table row for C2; Decisions if load-bearing)

- [ ] **Step 1: Run the complete CI gauntlet locally**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green. Fix any clippy/fmt/doc issues with follow-up edits before pushing.

- [ ] **Step 2: Push the branch and open the PR**

```bash
git push -u origin card/symbol-tokens-victory
gh pr create --fill
```
PR body: summarise the two deliverables; note (a) symbol effects modelled as a `ScenarioModule` hook not card abilities, with the rationale; (b) the deliberate partial reversal of B1's `reference_card`; (c) the RR p.21 "scenario-end, not on-clear" timing for location VP, quoting the clause. Include `Closes #229.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch` (background). Fix failures with follow-up commits to the same branch.

- [ ] **Step 4: Update the phase doc — ONLY after CI is green**

In `docs/phases/phase-7-the-gathering.md`, in the Group C breakdown table, flip the **C2 (#229)** row's State from `—` to `✅ PR #<n>`. Add a **Decisions made** entry only if it passes the "would a future PR-author choose differently without this entry?" test — a candidate: *"Scenario chaos-symbol effects live on `ScenarioModule.resolve_symbol` (plain-Rust hook returning `SymbolOutcome`), not card `abilities()`; B1's `reference_card` field was removed as dead. Location victory points are placed at scenario resolution (RR p.21), not on clear, into `GameState.victory_display`."* Keep it to 1–2 sentences. Commit as the final commit:

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: mark C2 (#229) shipped in phase-7 plan"
git push
```

- [ ] **Step 5: Merge only after explicit user approval**

Do not merge autonomously. Once the user approves: `gh pr merge <PR#> --squash --delete-branch`, then confirm #229 auto-closed and `git pull` on `main`.

---

## Self-review notes

- **Spec coverage:** symbol mechanism (Tasks 1–4), B1 reversal (Task 1), 01104 skull/cultist/tablet (Task 4), victory points scenario-end scan (Task 5), all integration tests in `scenarios` (corpus reachable) — covered. RR/issue wording correction is carried into the PR body (Task 6 Step 2).
- **No new DSL primitives** (per spec) — confirmed: all symbol logic is plain Rust on the scenario module; side effects reuse `elimination::take_damage`/`take_horror`.
- **Type consistency:** `SymbolOutcome { modifier, immediate, on_fail }`, `TokenEffect::{Damage,Horror}`, `SymbolCtx::{new, state, investigator, investigator_location}`, `resolve_symbol_token`, `apply_symbol_outcome`, `EnteredVictoryDisplay { code, victory }`, `victory_display` — used identically across tasks.
- **Verification gaps flagged inline** for the implementer to confirm against the live tree (event field names; `test_location`/`test_enemy` arities; `scenarios::the_gathering` visibility; `AdvanceAct` preconditions) rather than assumed silently.
```
