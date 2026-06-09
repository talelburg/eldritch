# Phase 7 Slice 1 — Engine-Spine Primitives (Plan A1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the three mechanical, self-contained engine primitives The Gathering needs — `Effect::DealDamage` / `Effect::DealHorror`, an inert `EventPattern::EnteredLocation` DSL variant, and a `CardCode` on `Act`/`Agenda` — each independently tested and mergeable.

**Architecture:** Pure additive changes following existing patterns. The two effects mirror `Effect::GainResources` (enum variant + `apply_effect` match arm + builder fn + a helper that delegates to the existing numeric/defeat helpers in `dispatch`). The `EventPattern` variant lands inert exactly as `CardRevealed`/`EnemySpawned` did (DSL surface only; firing wired in Plan A2). The `Act`/`Agenda` `code` field mirrors `Location.code`.

**Tech Stack:** Rust workspace (`card-dsl`, `game-core`); `cargo test`/`clippy`/`fmt`/`doc` under the strict CI flags in `CLAUDE.md`.

**Scope note:** This is Plan A1 of Group A. The forced-trigger *dispatch extension* (widening `scan_pending_triggers` to location/act/agenda sources, the forced auto-fire path, new trigger windows) is **Plan A2** — it reshapes `PendingTrigger`/`fire_pending_trigger` (which key on in-play card `instance_id`s) to address non-instance scenario-structure sources, and is designed separately. A1 deliberately leaves the new `EventPattern` variant inert.

---

## File Structure

- `crates/card-dsl/src/dsl.rs` — add `Effect::DealDamage`/`DealHorror` variants, `deal_damage`/`deal_horror` builder fns, and `EventPattern::EnteredLocation` variant. One file: the DSL data types and their builders already live here.
- `crates/game-core/src/engine/dispatch/elimination.rs` — add a `pub(crate) take_damage` twin to the existing `take_horror`; widen `take_horror` to `pub(crate)`.
- `crates/game-core/src/engine/evaluator.rs` — add `apply_effect` match arms + `deal_damage`/`deal_horror` evaluator helpers (mirror `gain_resources`).
- `crates/game-core/src/state/game_state.rs` — add `code: CardCode` to `Act` and `Agenda`.
- `crates/scenarios/src/test_fixtures/synthetic.rs` — set the new `code` on the synthetic fixture's acts/agendas (construction site that must compile).

---

## Task 1: `Effect::DealDamage` / `Effect::DealHorror` primitives

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (Effect enum ~line 375; builder fns ~line 672)
- Modify: `crates/game-core/src/engine/dispatch/elimination.rs:186` (`take_horror`; add `take_damage`)
- Modify: `crates/game-core/src/engine/evaluator.rs:131` (`apply_effect` match; add helpers)
- Test: inline `#[cfg(test)]` in `crates/game-core/src/engine/evaluator.rs`

- [ ] **Step 1: Add the two `Effect` variants**

In `crates/card-dsl/src/dsl.rs`, inside `pub enum Effect`, after the `DiscoverClue` variant:

```rust
    /// Deal `amount` damage to the resolved target investigator,
    /// applying defeat if the new total reaches their max health.
    /// `amount == 0` is a no-op (no event, no target resolution).
    DealDamage { target: InvestigatorTarget, amount: u8 },
    /// Deal `amount` horror to the resolved target investigator,
    /// applying defeat if the new total reaches their max sanity.
    /// `amount == 0` is a no-op.
    DealHorror { target: InvestigatorTarget, amount: u8 },
```

- [ ] **Step 2: Add builder fns**

In `crates/card-dsl/src/dsl.rs`, after the `discover_clue` builder (~line 678):

```rust
/// Build an [`Effect::DealDamage`] against `target` for `amount`.
#[must_use]
pub fn deal_damage(target: InvestigatorTarget, amount: u8) -> Effect {
    Effect::DealDamage { target, amount }
}

/// Build an [`Effect::DealHorror`] against `target` for `amount`.
#[must_use]
pub fn deal_horror(target: InvestigatorTarget, amount: u8) -> Effect {
    Effect::DealHorror { target, amount }
}
```

- [ ] **Step 3: Write the failing evaluator test**

In `crates/game-core/src/engine/evaluator.rs`, inside the existing `#[cfg(test)] mod tests`, add (mirror the `gain_resources` tests already there; confirm the exact `TestGame` setup helpers against a neighbouring test before finalising):

```rust
#[test]
fn deal_damage_adds_damage_and_emits_event() {
    let mut state = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let outcome = apply_effect(
        &mut cx,
        &deal_damage(InvestigatorTarget::Controller, 2),
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].damage, 2);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::DamageTaken { investigator, amount: 2 } if *investigator == InvestigatorId(1)
    )));
}

#[test]
fn deal_horror_adds_horror_and_emits_event() {
    let mut state = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx { state: &mut state, events: &mut events };
    let outcome = apply_effect(
        &mut cx,
        &deal_horror(InvestigatorTarget::Controller, 1),
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 1);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    )));
}
```

Add any missing `use` lines the test needs (`deal_damage`, `deal_horror`, `Event`, `InvestigatorTarget`, `Cx`, `test_investigator`) following the imports the neighbouring `gain_resources` tests use.

- [ ] **Step 4: Run the test to verify it fails to compile**

Run: `cargo test -p game-core deal_damage_adds_damage_and_emits_event 2>&1 | tail -20`
Expected: compile error — `Effect::DealDamage` has no `apply_effect` arm (non-exhaustive match) / `deal_damage` unresolved if builder not imported.

- [ ] **Step 5: Add the `take_damage` twin and widen visibility**

In `crates/game-core/src/engine/dispatch/elimination.rs`, change `take_horror`'s signature to `pub(crate)` and add the `take_damage` twin immediately after it:

```rust
/// Apply `amount` damage to `investigator` via the numeric helper,
/// then apply defeat (cause [`DefeatCause::Damage`]) if it was lethal.
/// The single-source-damage twin of [`take_horror`] — the first such
/// caller is [`Effect::DealDamage`]'s evaluator.
pub(crate) fn take_damage(cx: &mut Cx, investigator: InvestigatorId, amount: u8) {
    if super::combat::apply_damage_numeric(cx, investigator, amount) {
        apply_investigator_defeat(cx, investigator, DefeatCause::Damage);
    }
}
```

Change `pub(super) fn take_horror` → `pub(crate) fn take_horror`.

- [ ] **Step 6: Add the `apply_effect` match arms + evaluator helpers**

In `crates/game-core/src/engine/evaluator.rs`, add to the `apply_effect` match (after `DiscoverClue`):

```rust
        Effect::DealDamage { target, amount } => deal_damage_effect(cx, eval_ctx, *target, *amount),
        Effect::DealHorror { target, amount } => deal_horror_effect(cx, eval_ctx, *target, *amount),
```

Then add the helpers (mirror `gain_resources`, ~line where `gain_resources` is defined):

```rust
fn deal_damage_effect(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: InvestigatorTarget,
    amount: u8,
) -> EngineOutcome {
    if amount == 0 {
        return EngineOutcome::Done;
    }
    let target_id = match resolve_investigator_target(cx.state, eval_ctx, target) {
        Ok(id) => id,
        Err(reason) => return EngineOutcome::Rejected { reason: reason.into() },
    };
    if !cx.state.investigators.contains_key(&target_id) {
        return EngineOutcome::Rejected {
            reason: format!("DealDamage: investigator {target_id:?} is not in the state").into(),
        };
    }
    crate::engine::dispatch::elimination::take_damage(cx, target_id, amount);
    EngineOutcome::Done
}

fn deal_horror_effect(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: InvestigatorTarget,
    amount: u8,
) -> EngineOutcome {
    if amount == 0 {
        return EngineOutcome::Done;
    }
    let target_id = match resolve_investigator_target(cx.state, eval_ctx, target) {
        Ok(id) => id,
        Err(reason) => return EngineOutcome::Rejected { reason: reason.into() },
    };
    if !cx.state.investigators.contains_key(&target_id) {
        return EngineOutcome::Rejected {
            reason: format!("DealHorror: investigator {target_id:?} is not in the state").into(),
        };
    }
    crate::engine::dispatch::elimination::take_horror(cx, target_id, amount);
    EngineOutcome::Done
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p game-core deal_damage_adds_damage_and_emits_event deal_horror_adds_horror_and_emits_event 2>&1 | tail -20`
Expected: both PASS.

- [ ] **Step 8: Run the strict gauntlet for the touched crates**

Run:
```sh
RUSTFLAGS="-D warnings" cargo test -p card-dsl -p game-core --all-features 2>&1 | tail -15
cargo clippy -p card-dsl -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -15
RUSTDOCFLAGS="-D warnings" cargo doc -p card-dsl -p game-core --no-deps --all-features 2>&1 | tail -10
cargo fmt --check
```
Expected: all clean. (Builder fns are `#[must_use]` to satisfy clippy; doc links resolve.)

- [ ] **Step 9: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/elimination.rs
git commit -m "dsl,engine: DealDamage/DealHorror effect primitives

First single-source-damage caller (the take_horror doc anticipated it);
adds the take_damage twin and the two evaluator helpers mirroring
GainResources. Needed by Cellar (01114) / Attic (01113) forced-on-enter
and treachery damage/horror in The Gathering.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: inert `EventPattern::EnteredLocation` DSL variant

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`EventPattern` enum, ~line 196)
- Test: inline `#[cfg(test)]` in `crates/card-dsl/src/dsl.rs`

This lands the DSL surface only — exactly as `CardRevealed`/`EnemySpawned` did. The engine does **not** match it yet; firing is Plan A2. `game-core`'s `trigger_matches` exhaustively matches `EventPattern`, so adding a variant forces a deliberate `_ => false` (or explicit arm) there.

- [ ] **Step 1: Write the failing round-trip test**

In `crates/card-dsl/src/dsl.rs` `#[cfg(test)] mod tests` (mirror any existing serde round-trip test for `EventPattern`; if none, this is the first):

```rust
#[test]
fn entered_location_pattern_round_trips() {
    let p = EventPattern::EnteredLocation;
    let json = serde_json::to_string(&p).unwrap();
    let back: EventPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p card-dsl entered_location_pattern_round_trips 2>&1 | tail -10`
Expected: compile error — `EventPattern::EnteredLocation` does not exist.

- [ ] **Step 3: Add the variant**

In `crates/card-dsl/src/dsl.rs`, inside `pub enum EventPattern`, after `EnemySpawned`:

```rust
    /// An investigator entered the location this ability is printed on
    /// (Forced "after you enter <location>" effects: Attic `01113`
    /// takes 1 horror, Cellar `01114` takes 1 damage).
    ///
    /// Intentionally bare: the engine binds *you* = the entering
    /// investigator and *this location* = the ability's own location
    /// from the trigger context — no narrowing fields needed.
    ///
    /// DSL surface only in Plan A1; the matching + forced auto-fire
    /// wiring lands in Plan A2. Until then the engine ignores it.
    EnteredLocation,
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p card-dsl entered_location_pattern_round_trips 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Keep `game-core` compiling (exhaustive match)**

Build `game-core` to surface the now-non-exhaustive `trigger_matches`:

Run: `cargo build -p game-core 2>&1 | tail -15`
Expected: compile error in `crates/game-core/src/engine/dispatch/reaction_windows.rs` at `trigger_matches` (non-exhaustive `EventPattern` match).

Fix it by adding an explicit inert arm in `trigger_matches` (find the `match pattern` there):

```rust
        // Plan A2 wires the entered-location forced window; until then
        // this pattern never matches any open WindowKind.
        EventPattern::EnteredLocation => false,
```

Run again: `cargo build -p game-core 2>&1 | tail -5` → clean.

- [ ] **Step 6: Strict gauntlet + commit**

```sh
RUSTFLAGS="-D warnings" cargo test -p card-dsl -p game-core --all-features 2>&1 | tail -10
cargo clippy -p card-dsl -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -10
cargo fmt --check
```
Then:
```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "dsl: EventPattern::EnteredLocation (inert; firing in Plan A2)

DSL surface for Forced after-you-enter-location effects, landed inert
per the CardRevealed/EnemySpawned precedent. trigger_matches gets an
explicit false arm; Plan A2 wires the window + forced auto-fire.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: `CardCode` on `Act` and `Agenda`

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`Act` ~line 261, `Agenda` ~line 244)
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs:83-105` (construction sites)
- Test: inline `#[cfg(test)]` in `crates/game-core/src/state/game_state.rs`

Mirrors `Location.code`. The field is the prerequisite for Plan A2's widened scan (resolving act/agenda abilities through the registry by code). Pure data plumbing — no behavior yet.

- [ ] **Step 1: Write the failing test**

In `crates/game-core/src/state/game_state.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn act_and_agenda_carry_card_code() {
    let act = Act { code: CardCode::new("01108"), clue_threshold: 2, resolution: None };
    let agenda = Agenda { code: CardCode::new("01105"), doom_threshold: 3, resolution: None };
    assert_eq!(act.code, CardCode::new("01108"));
    assert_eq!(agenda.code, CardCode::new("01105"));
}
```

(Confirm the exact `CardCode` constructor — `CardCode::new` vs `CardCode("…".into())` — against existing uses in this file before finalising.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core act_and_agenda_carry_card_code 2>&1 | tail -10`
Expected: compile error — `Act`/`Agenda` have no `code` field.

- [ ] **Step 3: Add the `code` field to both structs**

In `crates/game-core/src/state/game_state.rs`, add as the first field of `Agenda`:

```rust
    /// The encounter-card code this agenda is printed on (e.g.
    /// `01105`). Lets the trigger dispatcher resolve the agenda's
    /// `Trigger::OnEvent` abilities through the card registry — the
    /// agenda owns its Forced effects like any other card.
    pub code: CardCode,
```

And as the first field of `Act`:

```rust
    /// The encounter-card code this act is printed on (e.g. `01108`).
    /// Lets the trigger dispatcher resolve the act's `Trigger::OnEvent`
    /// abilities through the card registry.
    pub code: CardCode,
```

- [ ] **Step 4: Fix the synthetic fixture construction sites**

In `crates/scenarios/src/test_fixtures/synthetic.rs`, the `state.agenda_deck = vec![...]` and `state.act_deck = vec![...]` literals (~lines 83 and 95) construct `Agenda`/`Act` directly. Add `code:` to each literal, reusing the synthetic codes already in `synth_cards` if present, or literal placeholders:

```rust
    // in each Agenda { ... } literal:
    code: CardCode("synth-agenda".into()),
    // in each Act { ... } literal:
    code: CardCode("synth-act".into()),
```

(Match the `CardCode` constructor form used elsewhere in this file.)

- [ ] **Step 5: Run the test + verify the fixture compiles**

Run: `cargo test -p game-core act_and_agenda_carry_card_code 2>&1 | tail -10`
Run: `cargo build -p scenarios --features test_fixtures 2>&1 | tail -10`
Expected: test PASS; scenarios builds.

- [ ] **Step 6: Full strict gauntlet (touches state shape → broad)**

Run:
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | tail -15
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features 2>&1 | tail -10
cargo fmt --check
cargo build -p web --target wasm32-unknown-unknown 2>&1 | tail -10
```
Expected: all clean. (State shape is serde-derived; the new required field may surface in any test that builds an `Act`/`Agenda` literal — fix each by adding `code:`.)

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/scenarios/src/test_fixtures/synthetic.rs
git commit -m "engine: Act/Agenda carry CardCode

Mirrors Location.code. Prerequisite for Plan A2's widened trigger scan,
which resolves act/agenda Forced abilities through the registry by code.
Pure data plumbing; no behavior yet.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** A1 covers the spec's Group A items 1 (DSL primitives — DealDamage/DealHorror + forced-on-enter `EventPattern`) and 2 (Act/Agenda `CardCode`). Group A items 3–5 (widen scan, forced auto-fire, new windows) are explicitly deferred to Plan A2 and called out in the header. No silent gaps.
- **Placeholder scan:** no TODO/TBD; every code step shows complete code. The synthetic-fixture `code:` values are real string literals, not placeholders.
- **Type consistency:** `Effect::DealDamage { target, amount }` / `DealHorror` field names match across enum, builders, match arms, and helpers. `take_damage`/`take_horror` signatures match their call sites in the evaluator helpers. `EventPattern::EnteredLocation` is bare in both the variant and the `trigger_matches` arm. `Act`/`Agenda` `code: CardCode` matches the test and fixture literals.
- **Open verification points flagged inline:** exact `CardCode` constructor form, exact `TestGame` helper names, and the neighbouring-test import set — each step says to confirm against an adjacent example before finalising, since those are the spots most likely to drift from this plan's assumptions.
