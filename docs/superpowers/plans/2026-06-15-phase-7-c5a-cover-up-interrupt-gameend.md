# C5a — Cover Up interrupt window + GameEnd forced point — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the engine machinery for Cover Up 01007's optional before-timing clue-discovery replacement interrupt and its game-end mental-trauma forced point, data-driven (no `01007` literal in the engine) and verified against a synthetic fixture.

**Architecture:** A minimal card-local seam at the `discover_clue` chokepoint suspends with a yes/no `AwaitingInput` when an eligible interrupt card is controlled; the skill-test driver pre-advances its continuation so resume threads through the existing `in_flight_skill_test` state machine. A new `ForcedTriggerPoint::GameEnd` fires once from `fire_scenario_resolution`. Bespoke card effects (discard-from-self, suffer-trauma) stay `Effect::Native`, exercised via an extended `synth_cards::TEST_REGISTRY` in an integration test.

**Tech Stack:** Rust, `game-core` kernel + `card-dsl` + `scenarios` test fixtures. Strict CI: `cargo fmt`, `clippy -D warnings`, `RUSTFLAGS=-D warnings test`, `RUSTDOCFLAGS=-D warnings doc`, wasm build/clippy.

**Spec:** `docs/superpowers/specs/2026-06-15-phase-7-c5a-cover-up-interrupt-gameend-design.md`

**Branch:** `engine/cover-up-interrupt` (already created; spec committed).

---

## File structure

- `crates/game-core/src/state/card.rs` — add `CardInPlay.clues` field + `enter_play` default.
- `crates/game-core/src/event.rs` — add `Event::TraumaSuffered` + `TraumaKind`.
- `crates/card-dsl/src/dsl.rs` — add `EventPattern::WouldDiscoverClues` + `EventPattern::GameEnd`.
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — exclude the two new patterns from `trigger_matches`.
- `crates/game-core/src/engine/evaluator.rs` — `EvalContext.clue_discovery_count`; extract `perform_discovery`; add the interrupt scan + suspend in `discover_clue`.
- `crates/game-core/src/state/game_state.rs` — `ClueInterruptPending` struct + `clue_interrupt_pending` field.
- `crates/game-core/src/engine/dispatch/clue_interrupt.rs` — **new** — `resume_clue_interrupt`.
- `crates/game-core/src/engine/dispatch/mod.rs` — route + guard the new suspension mode; register the new module.
- `crates/game-core/src/engine/dispatch/skill_test.rs` — pre-advance continuation; propagate `AwaitingInput` from the Investigate follow-up.
- `crates/game-core/src/engine/dispatch/forced_triggers.rs` — `ForcedTriggerPoint::GameEnd` variant + collect arm.
- `crates/game-core/src/engine/mod.rs` — fire `GameEnd` from `fire_scenario_resolution`.
- `crates/scenarios/src/test_fixtures/synth_cards.rs` — Cover-Up-shaped fixture card + Native effects + `native_effect_for` wiring.
- `crates/scenarios/tests/cover_up_interrupt.rs` — **new** — integration tests.

Run the full gauntlet (below) before the final push. Per-task, the single-crate test command shown is enough for the red/green loop.

```sh
# Full gauntlet (run before push)
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTFLAGS="-D warnings" cargo test --all --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

---

## Task 1: Clue storage on a card instance

**Files:**
- Modify: `crates/game-core/src/state/card.rs` (struct `CardInPlay` ~136-174; `enter_play` ~209-219)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/game-core/src/state/card.rs`:

```rust
#[test]
fn enter_play_defaults_clues_to_zero() {
    let c = CardInPlay::enter_play(CardCode("_x".into()), CardInstanceId(1));
    assert_eq!(c.clues, 0);
}

#[test]
fn card_in_play_deserializes_when_clues_field_absent() {
    // A state serialized before `clues` existed must still load (field
    // defaults to 0), mirroring the `ability_usage` serde-default test.
    let json = r#"{
        "code": "_x", "instance_id": 1, "exhausted": false,
        "uses": {}, "accumulated_damage": 0, "accumulated_horror": 0,
        "ability_usage": {}
    }"#;
    let c: CardInPlay = serde_json::from_str(json).expect("deserialize");
    assert_eq!(c.clues, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core enter_play_defaults_clues_to_zero`
Expected: FAIL — `no field 'clues' on type 'CardInPlay'`.

- [ ] **Step 3: Add the field + default**

In the `CardInPlay` struct, after `accumulated_horror` (before `ability_usage`):

```rust
    /// Clues sitting on this card instance (Cover Up 01007 enters the
    /// threat area "with 3 clues on it"). Distinct from the investigator
    /// and location clue pools; defaults to 0. Most cards never carry
    /// clues, so absent on the wire → 0.
    #[serde(default)]
    pub clues: u8,
```

In `enter_play`, add `clues: 0,` to the constructed `Self { … }` (after `accumulated_horror: 0,`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p game-core -- card::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/card.rs
git commit -m "engine: clue storage on CardInPlay (CardInPlay.clues)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: `Event::TraumaSuffered` + `TraumaKind`

**Files:**
- Modify: `crates/game-core/src/event.rs` (enum `Event` starts ~31; add near `DamageTaken` ~91)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/game-core/src/event.rs` (or create one if absent — match the file's existing test style; if none, append):

```rust
#[test]
fn trauma_suffered_round_trips() {
    let e = Event::TraumaSuffered {
        investigator: crate::state::InvestigatorId(1),
        kind: TraumaKind::Mental,
        amount: 1,
    };
    let json = serde_json::to_string(&e).expect("serialize");
    let back: Event = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(e, back);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core trauma_suffered_round_trips`
Expected: FAIL — `no variant named TraumaSuffered` / `cannot find type TraumaKind`.

- [ ] **Step 3: Add the variant + enum**

In `event.rs`, add the `TraumaKind` enum just above `pub enum Event` (keep derives matching the file's other public enums):

```rust
/// Which trauma track a [`Event::TraumaSuffered`] applies to. Trauma is a
/// cross-scenario campaign concept (Phase 9 owns persistence / sanity
/// reduction); this event makes it observable now without modeling state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraumaKind {
    /// Physical trauma (reduces max health in campaign play).
    Physical,
    /// Mental trauma (reduces max sanity in campaign play).
    Mental,
}
```

Add the `Event` variant after `DamageTaken { … }`:

```rust
    /// An investigator suffered trauma. Emitted by Cover Up 01007's
    /// game-end Forced ability. Observable + replay-visible; persistence
    /// (campaign log, max-stat reduction) is Phase 9 — no state mutation.
    TraumaSuffered {
        /// Who suffered the trauma.
        investigator: InvestigatorId,
        /// Physical or mental.
        kind: TraumaKind,
        /// How many trauma.
        amount: u8,
    },
```

If `TraumaKind` needs re-export, add it to the `pub use` list in `crates/game-core/src/lib.rs` alongside the other `event::` exports (grep `pub use crate::event` / `event::{` to find the line; add `TraumaKind`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p game-core trauma_suffered_round_trips`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/event.rs crates/game-core/src/lib.rs
git commit -m "engine: Event::TraumaSuffered + TraumaKind

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: DSL trigger patterns `WouldDiscoverClues` + `GameEnd`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (enum `EventPattern` ~190-295)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`trigger_matches` ~142-200)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/card-dsl/src/dsl.rs` (the module already round-trips `EventPattern` variants):

```rust
#[test]
fn would_discover_clues_and_game_end_round_trip() {
    for p in [EventPattern::WouldDiscoverClues, EventPattern::GameEnd] {
        let json = serde_json::to_string(&p).expect("serialize");
        let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p card-dsl would_discover_clues_and_game_end_round_trip`
Expected: FAIL — `no variant named WouldDiscoverClues`.

- [ ] **Step 3: Add the variants**

In `EventPattern`, after `AfterLocationInvestigated`:

```rust
    /// An investigator is about to discover one or more clues. Matched
    /// **only** by the clue-discovery interrupt seam in `discover_clue`
    /// (paired with [`EventTiming::Before`]), never by the general
    /// reaction-window pipeline — `trigger_matches` returns `false` for
    /// it, like the forced-only patterns above. First consumer: Cover Up
    /// 01007's "[reaction] When you would discover 1 or more clues at your
    /// location: Discard that many clues from Cover Up instead." (C5a #236.)
    WouldDiscoverClues,
    /// The game ended (a scenario resolution latched). Fired forced via
    /// `ForcedTriggerPoint::GameEnd` from `fire_scenario_resolution`,
    /// scanning every investigator's controlled card instances; binds
    /// controller = each instance's controller. First consumer: Cover Up
    /// 01007's "Forced - When the game ends, if there are any clues on
    /// Cover Up: You suffer 1 mental trauma." (C5a #236.)
    GameEnd,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p card-dsl would_discover_clues_and_game_end_round_trip`
Expected: PASS.

- [ ] **Step 5: Exclude both from `trigger_matches`**

`trigger_matches` in `reaction_windows.rs` matches `OnEvent` patterns against player reaction windows. Both new patterns are seam/forced-only and must never match a player window. Find the block that already returns `false` for the forced-only patterns (`EnteredLocation | PhaseEnded | … | EndOfTurn | AfterLocationInvestigated`) and add the two new variants to it. Run `cargo build -p game-core` first to let the non-exhaustive-match warning point you at the exact arm. The edit adds `| EventPattern::WouldDiscoverClues | EventPattern::GameEnd` to that false-returning group.

- [ ] **Step 6: Add a regression test for `trigger_matches`**

Add to the `reaction_windows.rs` test module a test asserting an `OnEvent { WouldDiscoverClues, Before }` ability does not match a player window (mirror the nearest existing `trigger_matches` test — grep `fn trigger_matches` in that module's tests for the shape; assert the function returns `false`).

- [ ] **Step 7: Run tests**

Run: `cargo test -p game-core -- reaction_windows`
Expected: PASS. Also `cargo build -p game-core` clean (no non-exhaustive warning).

- [ ] **Step 8: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: EventPattern::WouldDiscoverClues + GameEnd (seam/forced-only)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: `EvalContext.clue_discovery_count`

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`EvalContext` ~75-122)

- [ ] **Step 1: Write the failing test**

Add to the evaluator's `#[cfg(test)]` module:

```rust
#[test]
fn eval_context_defaults_clue_discovery_count_to_none() {
    let ctx = EvalContext::for_controller(crate::state::InvestigatorId(1));
    assert_eq!(ctx.clue_discovery_count, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core eval_context_defaults_clue_discovery_count_to_none`
Expected: FAIL — `no field clue_discovery_count`.

- [ ] **Step 3: Add the field + thread through constructors**

In `EvalContext`, after `failed_by`:

```rust
    /// The clue count a before-timing discovery interrupt is replacing,
    /// set only while resolving an `EventPattern::WouldDiscoverClues`
    /// ability's effect (so the card-local "discard that many" Native
    /// reads it). `None` outside that window. Mirrors `failed_by`.
    pub clue_discovery_count: Option<u8>,
```

Add `clue_discovery_count: None,` to the `Self { … }` in both `for_controller` and `for_controller_with_source`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p game-core eval_context_defaults_clue_discovery_count_to_none`
Expected: PASS. (If other call sites construct `EvalContext` with a struct literal, the compiler will flag them; there are none outside the two constructors — verify with `cargo build -p game-core`.)

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs
git commit -m "engine: EvalContext.clue_discovery_count for the discovery interrupt

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: `ClueInterruptPending` state

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (pending fields ~210-235; add a struct near `HandSizeDiscard`/`SpawnEngagePending` definitions)

- [ ] **Step 1: Write the failing test**

Add to the `game_state.rs` test module:

```rust
#[test]
fn clue_interrupt_pending_defaults_none_and_absent_field_loads() {
    let s = GameStateBuilder::new().build();
    assert!(s.clue_interrupt_pending.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core clue_interrupt_pending_defaults_none_and_absent_field_loads`
Expected: FAIL — `no field clue_interrupt_pending`.

- [ ] **Step 3: Add the struct + field**

Near the other suspension structs (e.g. just before or after `SpawnEngagePending`), add:

```rust
/// Suspended before-timing clue-discovery interrupt (C5a, #236). `Some`
/// while `discover_clue` is paused offering the controller the choice to
/// replace a discovery (Cover Up 01007's `[reaction]`). The resume path
/// (`resume_clue_interrupt`) applies the choice, then re-enters
/// `drive_skill_test` if a test is mid-flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClueInterruptPending {
    /// The investigator who would discover (and controls the interrupt card).
    pub controller: InvestigatorId,
    /// The location the discovery is from.
    pub location: LocationId,
    /// How many clues the discovery would move.
    pub count: u8,
    /// The interrupting card instance (Cover Up) — `source` for its effect.
    pub source: CardInstanceId,
    /// Index of the `WouldDiscoverClues` ability on the source card, so the
    /// resume runs the right effect on `Confirm`.
    pub ability_index: usize,
}
```

Add the field to `GameState`, alongside the other `pending_*` fields:

```rust
    /// Suspended clue-discovery interrupt (C5a, #236). See [`ClueInterruptPending`].
    #[serde(default)]
    pub clue_interrupt_pending: Option<ClueInterruptPending>,
```

Add `clue_interrupt_pending: None,` to wherever `GameState` is constructed with explicit field defaults (grep for `hand_size_discard_pending: None` to find the construction site(s) — typically the builder and/or a `Default`-like fn; mirror it). Re-export `ClueInterruptPending` from `lib.rs` if the other pending structs are re-exported (grep `SpawnEngagePending` in `lib.rs`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p game-core clue_interrupt_pending_defaults_none_and_absent_field_loads`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/lib.rs
git commit -m "engine: ClueInterruptPending suspension state

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Interrupt seam in `discover_clue`

Extract the discovery mutation into a reusable helper, then add the eligibility scan + suspend ahead of it. Without an installed registry (game-core unit tests), the scan finds nothing and discovery proceeds unchanged — so existing behavior is preserved and unit-tested here; the firing path is integration-tested in Task 11.

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`discover_clue` ~466-542)

- [ ] **Step 1: Write the failing test**

Add to the evaluator test module:

```rust
#[test]
fn discover_clue_without_registry_discovers_normally() {
    // No registry installed (game-core unit context) → no interrupt scan
    // hit → discovery proceeds exactly as before. Regression guard for the
    // seam's "fall through when no eligible interrupt" path.
    use crate::dsl::{discover_clue, LocationTarget};
    // Build: investigator at a location with 1 clue. (Mirror the existing
    // `discover_clue_moves_one_clue_from_location_to_controller` setup in
    // this module for the exact builder calls.)
    // ... construct `cx`, `eval_ctx`, location with 1 clue ...
    let outcome = apply_effect(
        &mut cx,
        &discover_clue(LocationTarget::YourLocation, 1),
        eval_ctx,
    );
    assert!(matches!(outcome, EngineOutcome::Done));
    // location -1, investigator +1 (assert via the same accessors the
    // sibling test uses).
}
```

Copy the exact builder/`cx`/`eval_ctx` construction from the existing `discover_clue_moves_one_clue_from_location_to_controller` test in this module (read it first) so this compiles; the only assertion that matters is that the outcome is `Done` and the clue moved.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core discover_clue_without_registry_discovers_normally`
Expected: FAIL initially only if the helper refactor breaks compilation; otherwise it should pass once the seam is added and is a regression guard. Run it now to confirm it compiles + passes against current code (it documents current behavior). If it passes pre-change, that's expected — it guards the refactor.

- [ ] **Step 3: Extract `perform_discovery`**

In `discover_clue`, the mutation block (currently ~519-541: write location count, add to investigator, push `CluePlaced` + `LocationCluesChanged`, return `Done`) becomes a `pub(crate)` helper. Add:

```rust
/// Move `count` clues (capped at availability) from `location_id` to
/// `controller`, emitting `CluePlaced` + `LocationCluesChanged`. The
/// committed mutation half of `discover_clue`, factored out so the
/// clue-discovery interrupt's `Skip` resume can perform the deferred
/// discovery (C5a #236). Caller guarantees both ids exist and the
/// location has clues.
pub(crate) fn perform_discovery(
    cx: &mut Cx,
    location_id: crate::state::LocationId,
    count: u8,
    controller: crate::state::InvestigatorId,
) {
    let location = cx.state.locations.get(&location_id).expect("location exists");
    let actually_taken = count.min(location.clues);
    let new_location_count = location.clues - actually_taken;
    cx.state
        .locations
        .get_mut(&location_id)
        .expect("checked above")
        .clues = new_location_count;
    let investigator = cx
        .state
        .investigators
        .get_mut(&controller)
        .expect("checked above");
    investigator.clues = investigator.clues.saturating_add(actually_taken);
    cx.events.push(Event::CluePlaced {
        investigator: controller,
        count: actually_taken,
    });
    cx.events.push(Event::LocationCluesChanged {
        location: location_id,
        new_count: new_location_count,
    });
}
```

Replace the original mutation block in `discover_clue` (after its existing validations) with `perform_discovery(cx, location_id, count, eval_ctx.controller); EngineOutcome::Done`.

- [ ] **Step 4: Add the interrupt scan + suspend**

In `discover_clue`, **after** the existing `location.clues == 0` no-op check and the controller-exists check, **before** the `perform_discovery` call, insert the eligibility scan:

```rust
    // Before-timing clue-discovery interrupt (Cover Up 01007, C5a #236).
    // Offer the controller a chance to replace this discovery iff they
    // control a card with a `WouldDiscoverClues` reaction, that card holds
    // >= 1 clue (RR p.2 — the reaction needs game-state potential), and the
    // discovery is at the controller's own location ("at your location").
    // No registry (unit context) or no eligible card → fall through to the
    // normal discovery below.
    if let Some(reg) = crate::card_registry::current() {
        let at_your_location = cx
            .state
            .investigators
            .get(&eval_ctx.controller)
            .and_then(|i| i.current_location)
            == Some(location_id);
        if at_your_location {
            if let Some(inv) = cx.state.investigators.get(&eval_ctx.controller) {
                for card in inv.controlled_card_instances() {
                    if card.clues == 0 {
                        continue;
                    }
                    if let Some(abilities) = (reg.abilities_for)(&card.code) {
                        if let Some(idx) = abilities.iter().position(|a| {
                            matches!(
                                &a.trigger,
                                crate::dsl::Trigger::OnEvent {
                                    pattern: crate::dsl::EventPattern::WouldDiscoverClues,
                                    timing: crate::dsl::EventTiming::Before,
                                }
                            )
                        }) {
                            cx.state.clue_interrupt_pending =
                                Some(crate::state::ClueInterruptPending {
                                    controller: eval_ctx.controller,
                                    location: location_id,
                                    count,
                                    source: card.instance_id,
                                    ability_index: idx,
                                });
                            return EngineOutcome::AwaitingInput {
                                request: crate::engine::outcome::InputRequest {
                                    prompt: "You would discover clue(s). Use the interrupt to \
                                             discard that many from the source card instead? \
                                             Confirm = replace, Skip = discover normally."
                                        .to_owned(),
                                },
                                resume_token: crate::engine::outcome::ResumeToken(0),
                            };
                        }
                    }
                }
            }
        }
    }
```

(If the borrow checker objects to the nested immutable borrow of `inv` while later mutating `cx.state.clue_interrupt_pending`, collect the `(instance_id, idx)` into a local `Option` first, then set `clue_interrupt_pending` after the loop — the loop only reads.)

- [ ] **Step 5: Run tests**

Run: `cargo test -p game-core -- evaluator`
Expected: PASS (all existing `discover_clue_*` tests + the new regression test).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/evaluator.rs
git commit -m "engine: clue-discovery interrupt seam in discover_clue

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: `resume_clue_interrupt` + routing + guard

**Files:**
- Create: `crates/game-core/src/engine/dispatch/clue_interrupt.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (module decl; `resolve_input` ~335-379; guard block ~70-160)

- [ ] **Step 1: Write the new module**

Create `crates/game-core/src/engine/dispatch/clue_interrupt.rs`:

```rust
//! Resume path for the before-timing clue-discovery interrupt (C5a #236).
//!
//! `discover_clue` suspends with `clue_interrupt_pending` set when an
//! eligible `WouldDiscoverClues` reaction is controlled. On resume:
//! `Confirm` runs the interrupt card's effect (the card-local Native
//! "discard that many from self", with the replaced count threaded via
//! `EvalContext.clue_discovery_count`) and discovers nothing; `Skip`
//! performs the deferred discovery. Either way, if a skill test is
//! mid-flight, re-enter `drive_skill_test` (its continuation was
//! pre-advanced to `PostFollowUp` before the follow-up suspended).

use crate::action::InputResponse;
use crate::card_registry;
use crate::engine::evaluator::{apply_effect, perform_discovery, EvalContext};
use crate::engine::outcome::EngineOutcome;
use super::Cx;

pub(crate) fn resume_clue_interrupt(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let pending = match cx.state.clue_interrupt_pending.take() {
        Some(p) => p,
        None => {
            return EngineOutcome::Rejected {
                reason: "resume_clue_interrupt: no clue interrupt pending".into(),
            }
        }
    };
    match response {
        InputResponse::Confirm => {
            // Run the WouldDiscoverClues ability's effect (Native discard
            // from self), threading the replaced count + source instance.
            let Some(reg) = card_registry::current() else {
                return EngineOutcome::Rejected {
                    reason: "resume_clue_interrupt: registry vanished".into(),
                };
            };
            // Resolve the card code from the source instance.
            let Some(inv) = cx.state.investigators.get(&pending.controller) else {
                return EngineOutcome::Rejected {
                    reason: "resume_clue_interrupt: controller vanished".into(),
                };
            };
            let Some(card) = inv
                .controlled_card_instances()
                .find(|c| c.instance_id == pending.source)
            else {
                return EngineOutcome::Rejected {
                    reason: "resume_clue_interrupt: source instance vanished".into(),
                };
            };
            let code = card.code.clone();
            let Some(abilities) = (reg.abilities_for)(&code) else {
                return EngineOutcome::Rejected {
                    reason: "resume_clue_interrupt: source has no abilities".into(),
                };
            };
            let effect = abilities[pending.ability_index].effect.clone();
            let mut ctx = EvalContext::for_controller_with_source(pending.controller, pending.source);
            ctx.clue_discovery_count = Some(pending.count);
            let outcome = apply_effect(cx, &effect, ctx);
            if !matches!(outcome, EngineOutcome::Done) {
                return outcome;
            }
        }
        InputResponse::Skip => {
            // Decline the reaction: the discovery resolves normally.
            perform_discovery(cx, pending.location, pending.count, pending.controller);
        }
        other => {
            // Restore the pending so a retry with a valid response works.
            // (The apply loop also restores state on Rejected, but be
            // explicit since we already `take()`-d.)
            cx.state.clue_interrupt_pending = Some(pending);
            return EngineOutcome::Rejected {
                reason: format!(
                    "resume_clue_interrupt: expected Confirm or Skip, got {other:?}"
                )
                .into(),
            };
        }
    }
    // If a skill test was mid-flight (the dominant path: Investigate's
    // follow-up discovery), resume its driver. Its continuation was
    // pre-advanced to PostFollowUp by `finish_skill_test` before the
    // follow-up suspended, so this picks up at the right step.
    if cx.state.in_flight_skill_test.is_some() {
        super::skill_test::drive_skill_test(cx)
    } else {
        EngineOutcome::Done
    }
}
```

If `drive_skill_test` is `pub(super)` (it is, within the `skill_test` module), it is reachable as `super::skill_test::drive_skill_test`. If the visibility is narrower, widen it to `pub(crate)` in `skill_test.rs`.

- [ ] **Step 2: Register the module + route + guard**

In `crates/game-core/src/engine/dispatch/mod.rs`:

1. Add the module declaration near the other `mod` lines: `mod clue_interrupt;`
2. In `resolve_input`, add the route **before** the reaction-window / skill-test resume checks (after the `act_round_end_pending` route, before `top_reaction_window`):

```rust
    // Before-timing clue-discovery interrupt (C5a #236): arises mid-skill-
    // test (during the Investigate follow-up), so route it before the
    // reaction-window and skill-test resume paths.
    if cx.state.clue_interrupt_pending.is_some() {
        return clue_interrupt::resume_clue_interrupt(cx, response);
    }
```

3. In the `apply_player_action` guard block, add a guard **after** the reaction-window guard and **before** the `in_flight_skill_test` guard (mirror their shape):

```rust
    // A pending clue-discovery interrupt (C5a #236) blocks every action but
    // `ResolveInput`. It coexists with an in-flight skill test (it suspends
    // mid-follow-up), so it must precede the skill-test guard.
    if cx.state.clue_interrupt_pending.is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a clue-discovery interrupt is pending; submit a PlayerAction::ResolveInput \
                     with InputResponse::Confirm (replace) or Skip (discover normally) before \
                     any other action"
                .into(),
        };
    }
```

Also add `clue_interrupt_pending.is_some()` to the `debug_assert!` mutual-exclusivity list at the top of `resolve_input` **only if** it is genuinely exclusive — it is NOT (it coexists with `in_flight_skill_test`), so do **not** add it there. Leave that assertion as-is.

- [ ] **Step 3: Run build + tests**

Run: `cargo test -p game-core -- dispatch`
Expected: PASS (compiles; existing dispatch tests unaffected). Full firing path is covered in Task 11.

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/clue_interrupt.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: resume_clue_interrupt + routing/guard for the interrupt

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: Pre-advance the skill-test continuation across the interrupt

Make the Investigate follow-up's discovery suspendable: pre-advance the continuation to `PostFollowUp` before running the follow-up, and propagate `AwaitingInput` instead of asserting `Done`.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`finish_skill_test` ~200-238; `apply_skill_test_follow_up` Investigate arm ~560-575)

- [ ] **Step 1: Propagate `AwaitingInput` from the Investigate follow-up**

`apply_skill_test_follow_up` is a free function returning `()` today; its `Investigate` arm builds `EvalContext::for_controller(investigator)` and `unreachable!`s on rejection. Change the function to return `EngineOutcome` and propagate the Investigate arm's outcome so a before-timing interrupt suspend reaches the driver. Replace the whole function so the arms are consistent:

```rust
fn apply_skill_test_follow_up(
    cx: &mut Cx,
    investigator: InvestigatorId,
    follow_up: SkillTestFollowUp,
) -> EngineOutcome {
    match follow_up {
        SkillTestFollowUp::None => EngineOutcome::Done,
        SkillTestFollowUp::Investigate => {
            let effect = discover_clue(LocationTarget::YourLocation, 1);
            // discover_clue may suspend on a before-timing interrupt
            // (Cover Up 01007). Propagate AwaitingInput; the Investigate
            // follow-up has no source card, so for_controller is correct.
            // The only rejection path ("controller between locations")
            // can't occur post-Investigate (the action validated a
            // location), so a Rejected here is still an invariant violation.
            let eval_ctx = EvalContext::for_controller(investigator);
            let outcome = apply_effect(cx, &effect, eval_ctx);
            if let EngineOutcome::Rejected { reason } = &outcome {
                unreachable!(
                    "Investigate follow-up: discover_clue rejected unexpectedly after \
                     validation: {reason}"
                );
            }
            outcome
        }
        SkillTestFollowUp::Fight { enemy } => {
            super::combat::damage_enemy(cx, enemy, 1, Some(investigator));
            EngineOutcome::Done
        }
        SkillTestFollowUp::Evade { enemy } => {
            let e = cx.state.enemies.get_mut(&enemy).unwrap_or_else(|| {
                unreachable!(
                    "Evade follow-up: enemy {enemy:?} vanished while test was in flight; \
                     this is a state-corruption invariant violation"
                )
            });
            e.engaged_with = None;
            e.exhausted = true;
            cx.events.push(Event::EnemyDisengaged { enemy, investigator });
            cx.events.push(Event::EnemyExhausted { enemy });
            EngineOutcome::Done
        }
    }
}
```

- [ ] **Step 2: Pre-advance the continuation in `finish_skill_test`**

In `finish_skill_test`, the `if succeeded { apply_skill_test_follow_up(...); ... }` block runs before the continuation is set to `PostFollowUp`. Restructure so the continuation is advanced **before** the follow-up, and a suspend returns early:

```rust
    // Pre-advance the continuation BEFORE running the follow-up, so that a
    // follow-up that suspends on a clue-discovery interrupt (Cover Up
    // 01007) resumes at PostFollowUp rather than re-running the follow-up.
    // on_success never co-occurs with a suspending follow-up in scope
    // (Investigate sets on_success=None; SkillTest-effect tests set
    // follow_up=None), so running on_success after the follow-up is safe.
    cx.state
        .in_flight_skill_test
        .as_mut()
        .expect("in_flight_skill_test was Some immediately above")
        .continuation = FinishContinuation::PostFollowUp { succeeded };

    if succeeded {
        let outcome = apply_skill_test_follow_up(cx, investigator, follow_up);
        if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
            return outcome;
        }
        debug_assert!(
            matches!(outcome, EngineOutcome::Done),
            "skill-test follow-up must resolve to Done or AwaitingInput: {outcome:?}"
        );
        if let Some(effect) = &on_success {
            let outcome = apply_effect(cx, effect, card_ctx(investigator));
            debug_assert!(
                matches!(outcome, EngineOutcome::Done),
                "skill-test on_success must resolve to Done in scope: {outcome:?}"
            );
        }
    } else if let Some(effect) = &on_fail {
        let mut ctx = card_ctx(investigator);
        ctx.failed_by = Some(failed_by);
        let outcome = apply_effect(cx, effect, ctx);
        debug_assert!(
            matches!(outcome, EngineOutcome::Done),
            "revelation on_fail must resolve to Done in C4b scope: {outcome:?}"
        );
    }

    drive_skill_test(cx)
```

Remove the now-duplicate `continuation = PostFollowUp` assignment that previously sat just before `drive_skill_test(cx)` (the pre-advance above replaces it).

- [ ] **Step 3: Run the skill-test tests (regression)**

Run: `cargo test -p game-core -- skill_test`
Expected: PASS — the reorder is behavior-preserving for all in-scope tests (no card has both a suspending follow-up and an `on_success`).

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: pre-advance skill-test continuation across the clue interrupt

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 9: `ForcedTriggerPoint::GameEnd` + fire at resolution

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (enum ~26-90; `collect_forced_hits` ~144-301)
- Modify: `crates/game-core/src/engine/mod.rs` (`fire_scenario_resolution` ~184-233)

- [ ] **Step 1: Add the variant**

In `ForcedTriggerPoint`, after `AfterLocationInvestigated { … }`:

```rust
    /// The game ended (a scenario resolution latched). Scans every
    /// investigator's controlled card instances (threat area + in play)
    /// for `EventPattern::GameEnd` forced abilities; binds controller =
    /// each instance's controller. First consumer: Cover Up 01007's
    /// game-end mental-trauma forced (C5a #236).
    GameEnd,
```

- [ ] **Step 2: Add the collect arm**

In `collect_forced_hits`, add a match arm for `ForcedTriggerPoint::GameEnd`:

```rust
        ForcedTriggerPoint::GameEnd => {
            // Scan every investigator's controlled instances; bind
            // controller = each card's controller, source = the instance.
            for (inv_id, inv) in &state.investigators {
                for card in inv.controlled_card_instances() {
                    push_matching(
                        reg,
                        &card.code,
                        *inv_id,
                        Some(card.instance_id),
                        &mut hits,
                        |p| matches!(p, EventPattern::GameEnd),
                    );
                }
            }
        }
```

(`state.investigators` is a `BTreeMap`, so iteration order is deterministic — consistent with `fire_forced_triggers`' fixed-order contract.)

- [ ] **Step 3: Fire from `fire_scenario_resolution`**

In `engine/mod.rs`, `fire_scenario_resolution` runs after the resolution latches. After the victory-display scan block and **before** the `apply_resolution` module hook (so trauma is observed as part of the resolution), add:

```rust
    // Fire game-end Forced abilities (Cover Up 01007's mental trauma, C5a
    // #236). Non-interactive in scope; a suspending GameEnd hit is #212
    // reentrancy work. Runs even with no scenario module registered.
    let _ = crate::engine::dispatch::forced_triggers::fire_forced_triggers(
        cx,
        &crate::engine::dispatch::forced_triggers::ForcedTriggerPoint::GameEnd,
    );
```

Confirm the path to `fire_forced_triggers` / `ForcedTriggerPoint` is reachable from `mod.rs` (both are `pub(crate)` in the `forced_triggers` module). Adjust the `use`/path to match how `mod.rs` already references `dispatch` items.

- [ ] **Step 4: Write a game-core unit test (no registry → no-op; with a hand-rolled registry → fires)**

The cleanest no-registry assertion: `GameEnd` firing is a no-op when nothing matches. Add to the `engine/mod.rs` test module a test that latches a resolution with no controlled cards and asserts no `TraumaSuffered` event and that the existing `ScenarioResolved` still fires (regression that the new call doesn't break the resolution path). The firing-with-trauma path is integration-tested in Task 11.

```rust
#[test]
fn game_end_forced_point_is_noop_without_matching_cards() {
    // Latch a resolution (mirror an existing resolution test in this
    // module for the setup), apply the triggering action, and assert
    // ScenarioResolved fires and no TraumaSuffered is emitted.
    // ... setup per the nearest existing resolution test ...
    assert!(events.iter().any(|e| matches!(e, Event::ScenarioResolved { .. })));
    assert!(!events.iter().any(|e| matches!(e, Event::TraumaSuffered { .. })));
}
```

Read the nearest existing resolution test in `engine/mod.rs` (grep `ScenarioResolved`) and copy its setup.

- [ ] **Step 5: Run tests**

Run: `cargo test -p game-core -- forced_triggers` and `cargo test -p game-core game_end_forced_point_is_noop_without_matching_cards`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/mod.rs
git commit -m "engine: ForcedTriggerPoint::GameEnd fired at scenario resolution

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 10: Cover-Up-shaped synthetic fixture + Native effects

**Files:**
- Modify: `crates/scenarios/src/test_fixtures/synth_cards.rs` (codes, metadata, `abilities_for`, `TEST_REGISTRY`, tests)

- [ ] **Step 1: Add the fixture code + metadata**

After the existing `pub const SYNTH_*_CODE` lines, add:

```rust
/// Code for the synthetic Cover-Up-shaped treachery (C5a #236). Carries a
/// `WouldDiscoverClues` before-timing interrupt + a `GameEnd` forced
/// trauma, both backed by Native effects on [`TEST_REGISTRY`]. Underscore
/// prefix guarantees no collision with real ArkhamDB codes.
pub const SYNTH_COVER_UP_CODE: &str = "_synth_cover_up";

/// Native-effect tags for the synthetic Cover Up (C5a #236).
pub const SYNTH_COVER_UP_DISCARD_TAG: &str = "_synth_cover_up:discard_clues";
pub const SYNTH_COVER_UP_TRAUMA_TAG: &str = "_synth_cover_up:trauma";
```

Add a metadata fn (mirror `synth_surge_treachery_metadata`):

```rust
fn synth_cover_up_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_COVER_UP_CODE.to_owned(),
        name: "Synthetic Cover Up".to_owned(),
        text: Some(
            "Reaction: when you would discover clues at your location, \
             discard that many from this card instead. Forced: at game end, \
             if any clues remain, suffer 1 mental trauma. (Synthetic.)"
                .to_owned(),
        ),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

fn synth_cover_up_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_cover_up_metadata)
}
```

Add `SYNTH_COVER_UP_CODE => Some(synth_cover_up_metadata_static()),` to `metadata_for`.

- [ ] **Step 2: Add the abilities**

Extend the imports at the top of the file:

```rust
use game_core::dsl::{
    native, on_event, EventPattern, EventTiming, // add these
    gain_resources, on_play, revelation, Ability, InvestigatorTarget,
};
```

(`native(tag)` builds `Effect::Native { tag }` — see `card_dsl::dsl::native`. `on_event(pattern, timing, effect)` builds an `OnEvent` ability — see `dsl.rs:828`.)

Add to `abilities_for`:

```rust
        SYNTH_COVER_UP_CODE => Some(vec![
            on_event(
                EventPattern::WouldDiscoverClues,
                EventTiming::Before,
                native(SYNTH_COVER_UP_DISCARD_TAG),
            ),
            on_event(
                EventPattern::GameEnd,
                EventTiming::After,
                native(SYNTH_COVER_UP_TRAUMA_TAG),
            ),
        ]),
```

- [ ] **Step 3: Add the Native effects + wire `native_effect_for`**

Add two native functions (signature `fn(&mut Cx, &EvalContext) -> EngineOutcome`). Add imports: `use game_core::card_registry::NativeEffectFn; use game_core::engine::{Cx, EngineOutcome, EvalContext}; use game_core::event::{Event, TraumaKind};`.

```rust
/// Native: discard the replaced clue count from the interrupting card
/// instance (Cover Up 01007's "discard that many from Cover Up instead").
fn synth_cover_up_discard(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let count = ctx.clue_discovery_count.unwrap_or(0);
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "synth_cover_up_discard: no source instance".into(),
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

/// Native: at game end, if the source card holds any clues, suffer 1
/// mental trauma (Cover Up 01007's Forced).
fn synth_cover_up_trauma(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "synth_cover_up_trauma: no source instance".into(),
        };
    };
    let has_clues = cx
        .state
        .investigators
        .get(&ctx.controller)
        .map(|inv| {
            inv.controlled_card_instances()
                .any(|c| c.instance_id == source && c.clues > 0)
        })
        .unwrap_or(false);
    if has_clues {
        cx.events.push(Event::TraumaSuffered {
            investigator: ctx.controller,
            kind: TraumaKind::Mental,
            amount: 1,
        });
    }
    EngineOutcome::Done
}

/// `native_effect_for` function pointer used by [`TEST_REGISTRY`].
fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        SYNTH_COVER_UP_DISCARD_TAG => Some(synth_cover_up_discard),
        SYNTH_COVER_UP_TRAUMA_TAG => Some(synth_cover_up_trauma),
        _ => None,
    }
}
```

Change `TEST_REGISTRY` to use it:

```rust
pub const TEST_REGISTRY: CardRegistry = CardRegistry {
    metadata_for,
    abilities_for,
    native_effect_for,
};
```

- [ ] **Step 4: Add fixture unit tests**

```rust
#[test]
fn cover_up_fixture_has_interrupt_and_gameend_abilities() {
    let code = CardCode(SYNTH_COVER_UP_CODE.into());
    let abilities = abilities_for(&code).expect("cover up abilities");
    assert_eq!(abilities.len(), 2);
    assert!(matches!(
        abilities[0].trigger,
        game_core::dsl::Trigger::OnEvent {
            pattern: game_core::dsl::EventPattern::WouldDiscoverClues,
            timing: game_core::dsl::EventTiming::Before,
        }
    ));
}

#[test]
fn native_effect_for_resolves_cover_up_tags() {
    assert!(native_effect_for(SYNTH_COVER_UP_DISCARD_TAG).is_some());
    assert!(native_effect_for(SYNTH_COVER_UP_TRAUMA_TAG).is_some());
    assert!(native_effect_for("nope").is_none());
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p scenarios -- synth_cards`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/scenarios/src/test_fixtures/synth_cards.rs
git commit -m "test: synthetic Cover-Up fixture + Native effects on TEST_REGISTRY

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 11: Integration tests — the firing paths

**Files:**
- Create: `crates/scenarios/tests/cover_up_interrupt.rs`

Drive a real successful Investigate with the fixture in the investigator's threat area, exercising both `Confirm` and `Skip`, plus the `GameEnd` trauma. Use a deterministic chaos bag (`ChaosToken::Numeric(0)`) and Intellect ≥ shroud so the test always succeeds.

- [ ] **Step 1: Write the test scaffold + helpers**

Create `crates/scenarios/tests/cover_up_interrupt.rs`:

```rust
//! C5a (#236) integration: Cover Up's before-timing clue-discovery
//! interrupt and its game-end mental-trauma forced point, against the
//! synthetic Cover-Up fixture. Own process → installs TEST_REGISTRY.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::{Event, TraumaKind};
use game_core::state::{
    CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameState, InvestigatorId, LocationId,
    Phase,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder};
use game_core::{Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::{SYNTH_COVER_UP_CODE, TEST_REGISTRY};

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}
```

Read `crates/game-core/src/test_support/builder.rs` and an existing engine investigate test to confirm the exact builder API (`with_investigator`, `with_active_investigator`, `with_phase`, `test_location`, chaos-bag setter). Adjust the imports/calls below to match. The state to build for the interrupt tests:
- Investigation phase, one active investigator at a location.
- The location has ≥1 clue and a shroud the investigator's Intellect beats.
- `chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)])`.
- A `CardInPlay` for `SYNTH_COVER_UP_CODE` in the investigator's `threat_area` with `clues = 3`.

- [ ] **Step 2: Test — `Confirm` replaces the discovery**

```rust
#[test]
fn confirm_replaces_discovery_with_discard_from_cover_up() {
    install();
    let mut state = /* build per Step 1: loc with 2 clues, cover up with 3 clues in threat area */;
    let inv = /* the active investigator id */;
    let loc = /* the location id */;

    // Investigate → commit nothing → reach the interrupt AwaitingInput.
    let r = apply(state, Action::Player(PlayerAction::Investigate { investigator: inv }));
    state = r.state;
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput { response: InputResponse::CommitCards { indices: vec![] } }),
    );
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected interrupt prompt, got {:?}", r.outcome
    );
    state = r.state;

    // Confirm: discover nothing; discard 1 from Cover Up (the base
    // Investigate discovers 1).
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput { response: InputResponse::Confirm }),
    );
    assert!(matches!(r.outcome, EngineOutcome::Done), "got {:?}", r.outcome);
    state = r.state;

    assert_eq!(state.locations[&loc].clues, 2, "location clues unchanged");
    assert_eq!(state.investigators[&inv].clues, 0, "investigator discovered nothing");
    let cover_up = state.investigators[&inv].threat_area.iter()
        .find(|c| c.code.as_str() == SYNTH_COVER_UP_CODE).expect("cover up present");
    assert_eq!(cover_up.clues, 2, "1 clue discarded from Cover Up");
}
```

- [ ] **Step 3: Test — `Skip` discovers normally**

```rust
#[test]
fn skip_discovers_normally() {
    install();
    let mut state = /* same build: loc 2 clues, cover up 3 clues */;
    let inv = /* id */; let loc = /* id */;
    let r = apply(state, Action::Player(PlayerAction::Investigate { investigator: inv }));
    state = r.state;
    let r = apply(state, Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::CommitCards { indices: vec![] } }));
    state = r.state;
    let r = apply(state, Action::Player(PlayerAction::ResolveInput { response: InputResponse::Skip }));
    assert!(matches!(r.outcome, EngineOutcome::Done));
    state = r.state;
    assert_eq!(state.locations[&loc].clues, 1, "location -1");
    assert_eq!(state.investigators[&inv].clues, 1, "investigator +1");
    let cover_up = state.investigators[&inv].threat_area.iter()
        .find(|c| c.code.as_str() == SYNTH_COVER_UP_CODE).unwrap();
    assert_eq!(cover_up.clues, 3, "Cover Up untouched on Skip");
}
```

- [ ] **Step 4: Test — ineligible when Cover Up has 0 clues**

```rust
#[test]
fn no_interrupt_when_cover_up_has_no_clues() {
    install();
    let mut state = /* build: cover up with clues = 0 */;
    let inv = /* id */;
    let r = apply(state, Action::Player(PlayerAction::Investigate { investigator: inv }));
    state = r.state;
    // commit window resolves straight to Done — no interrupt offered.
    let r = apply(state, Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::CommitCards { indices: vec![] } }));
    assert!(matches!(r.outcome, EngineOutcome::Done), "no interrupt; got {:?}", r.outcome);
}
```

- [ ] **Step 5: Test — GameEnd trauma fires iff clues remain**

Drive (or hand-latch) a resolution. The simplest deterministic latch: build a state whose act deck advances to a Won resolution, or directly construct a one-act board and `AdvanceAct` to terminal. Read `crates/scenarios/tests/closing_demo.rs` for the act-advance-to-Won pattern. Then:

```rust
#[test]
fn game_end_emits_trauma_when_cover_up_has_clues() {
    install();
    let mut state = /* build a board that resolves on the next action, cover up clues = 3 */;
    // ... apply the resolution-latching action ...
    let r = apply(state, /* resolution-latching action */);
    assert!(r.events.iter().any(|e| matches!(
        e, Event::TraumaSuffered { kind: TraumaKind::Mental, amount: 1, .. }
    )), "expected mental trauma at game end; events = {:?}", r.events);
}

#[test]
fn game_end_emits_no_trauma_when_cover_up_empty() {
    install();
    let mut state = /* same, but cover up clues = 0 */;
    let r = apply(state, /* resolution-latching action */);
    assert!(!r.events.iter().any(|e| matches!(e, Event::TraumaSuffered { .. })));
}
```

If building a resolving board is heavy, an acceptable alternative for the trauma half is a focused game-core integration test that calls the resolution path via the lowest-friction latch available (e.g. last-investigator-defeated `Lost`, per `elimination.rs`'s `last_investigator_defeated_latches_lost_resolution`). Prefer the act-advance Won path if the builder makes it easy.

- [ ] **Step 6: Run the integration tests**

Run: `cargo test -p scenarios --test cover_up_interrupt`
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/scenarios/tests/cover_up_interrupt.rs
git commit -m "test: integration coverage for Cover Up interrupt + GameEnd trauma

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 12: Full gauntlet + push + PR

- [ ] **Step 1: Run the full strict gauntlet**

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTFLAGS="-D warnings" cargo test --all --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. Fix any clippy/doc issues (e.g. intra-doc links for the new types) inline.

- [ ] **Step 2: Push + open PR**

```bash
git push -u origin engine/cover-up-interrupt
gh pr create --fill --base main
```

PR body: one-paragraph design summary (the seam at `discover_clue`, pre-advance reentrancy, GameEnd at `fire_scenario_resolution`, Native effects + integration test), the verified Cover Up card text, the RR p.2 optional-reaction citation, and `Closes #236.` Link the spec.

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch` (background). Fix failures with follow-up commits to the same branch.

- [ ] **Step 4: Phase-doc update (final commit, only once CI is green)**

Per `docs/phases/README.md`: in `docs/phases/phase-7-the-gathering.md`, flip the C5a row in the Group-C-breakdown table to `✅ PR #<N>`, update the Status paragraph's "Shipped:" list + "Next:" marker (C5a done → next C5b/C5c per the within-C5 ordering `{C5a,C5b}→{C5c,C5d}→C5e`), and add a **Decisions made** entry only if load-bearing for a future PR — candidate: "before-timing clue-discovery interrupt is a card-local seam at `discover_clue` with a `clue_interrupt_pending` suspension mode + pre-advanced skill-test continuation; bespoke effects stay `Effect::Native` integration-tested via `synth_cards::TEST_REGISTRY`; game-end trauma emits `Event::TraumaSuffered` only (persistence Phase 9)." Apply the "would a future PR-author choose differently without this entry?" test; keep it to one tight entry.

- [ ] **Step 5: Merge after explicit user approval**

```bash
gh pr merge <PR#> --squash --delete-branch
```

Confirm #236 auto-closed and `git pull` on `main`.

---

## Self-review notes (for the executor)

- **Borrow-checker hotspots:** the interrupt scan in Task 6 reads `inv.controlled_card_instances()` then writes `cx.state.clue_interrupt_pending` — collect `(instance_id, ability_index)` into a local before the write if needed. The Native discard in Task 10 iterates `threat_area`/`cards_in_play` mutably — use `iter_mut().chain(...)`.
- **`apply_skill_test_follow_up` return type:** Task 8 may require changing it from `()` to `EngineOutcome`; update the `Fight`/`Evade`/`None` arms and the call site together (compiler-guided).
- **Reentrancy bound (documented, in scope):** the interrupt may suspend only where `discover_clue` is the terminal effect of its eval context — true for the base Investigate follow-up, the sole clue-discovery source in Roland's Slice-1 deck (Deduction 01039 is not in it). Nested-in-`Seq` discovery suspension is #212.
- **`GameEnd` non-suspension:** the trauma Native returns `Done`; `fire_forced_triggers`' abandon-later-hits-on-suspend caveat is not exercised. Consistent with the C4c deterministic-order stand-in.
