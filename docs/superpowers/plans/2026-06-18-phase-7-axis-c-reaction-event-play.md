# Axis C — Reaction-Event-Play (Evidence! 01022) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a Fast event in hand (Evidence! 01022) be offered and played as an option inside the after-defeat reaction window, by migrating the reaction-window resume contract to the structured `PickSingle(OptionId)` surface.

**Architecture:** A reaction window's offered options become `{in-play pending triggers} ∪ {matching hand Fast-event plays}`, each a `ChoiceOption` resolved by `PickSingle(OptionId)`. The play-timing predicate is the *existing* `trigger_matches` `EventPattern` check — Evidence! is Roland 01001's after-defeat reaction, sourced from hand instead of from play. A picked hand-event option emits `CardPlayed`, runs the matched ability's effect, and discards via the existing `pending_played_event` flush.

**Tech Stack:** Rust workspace (`game-core` kernel, `cards` content, `card-dsl` types). Event-sourced `apply(state, action) -> ApplyResult`. Reaction windows live on the `Continuation::Resolution` stack (Axis B). Tests: in-crate unit tests + `crates/cards/tests/` integration tests (own process each, so `card_registry::install` is safe).

## Global Constraints

- **Spec:** `docs/superpowers/specs/2026-06-18-phase-7-axis-c-reaction-event-play-design.md`.
- **Issues:** closes #335 (Axis C) and #304 (Evidence!).
- **CI gauntlet (all must pass, warnings-as-errors), run from repo root before pushing:**
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Validate-first / mutate-second** handler contract holds for every new path.
- **Never hand-edit** `crates/cards/src/generated/cards.rs` (Evidence!'s metadata comes from the corpus; only `abilities()` is hand-written).
- **Card text is verbatim** from `data/arkhamdb-snapshot/pack/core/core.json`: Evidence! 01022 = "Fast. Play after you defeat an enemy.\nDiscover 1 clue at your location." (`type_code: event`, `traits: Insight.`, `cost: 1`).
- **Branch:** `engine/axis-c-reaction-event-play`. Commit subjects use `scope: description`. The spec file (already written) and the phase-doc update both ride this branch; the phase-doc commit is the **final** commit after CI is green (per `docs/phases/README.md`).
- **Out of scope:** the framework `open_fast_window` non-paused Fast-play path; Fast assets / Fast 0-action abilities as window options; Axis D cancellation (Dodge); any new DSL primitive; charging the event's resource cost (existing `play_card` does not — stay consistent).

---

### Task 0: Branch setup

- [ ] **Step 1: Create the feature branch**

Run from repo root:
```bash
git checkout -b engine/axis-c-reaction-event-play
git add docs/superpowers/specs/2026-06-18-phase-7-axis-c-reaction-event-play-design.md
git commit -m "docs: Axis C reaction-event-play design spec

Spec for #335 / #304. Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

(The spec file already exists in the working tree from the brainstorming step.)

---

### Task 1: Evidence! 01022 card + registration

Evidence! is Roland 01001's reaction minus the once-per-round limit (verified against `crates/cards/src/impls/roland_banks.rs` and the snapshot). It is wired into the registry so later tasks can scan it from hand. (The card is inert until Axis C machinery lands — `PlayCard` from hand outside a window is already gated by phase/timing, and no window offers it yet.)

**Files:**
- Create: `crates/cards/src/impls/evidence.rs`
- Modify: `crates/cards/src/impls/mod.rs` (module decl ~line 91 region; `abilities_for` match arm ~line 122, alphabetical between `emergency_cache` and `first_aid`)

**Interfaces:**
- Produces: `evidence::CODE: &str = "01022"`, `evidence::abilities() -> Vec<Ability>`.

- [ ] **Step 1: Write the failing card test**

Create `crates/cards/src/impls/evidence.rs`:
```rust
//! Evidence! (Neutral event, 01022).
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! ```text
//! Fast. Play after you defeat an enemy.
//! Discover 1 clue at your location.
//! ```
//!
//! # Scope
//!
//! Evidence! is Roland Banks 01001's `[reaction]` ("After you defeat an
//! enemy: Discover 1 clue at your location.") sourced from hand instead of
//! from play — the identical [`Trigger::OnEvent`] declaration, minus Roland's
//! once-per-round [`UsageLimit`]. Per Rules Reference p.11, a Fast event with
//! a "Play after …" instruction plays "as if the described timing point were a
//! triggering condition", so the play-timing predicate IS the `OnEvent`
//! pattern (Axis C, #335 / #304). The Fast/cost/Insight metadata comes from
//! the generated corpus.

use card_dsl::dsl::{
    discover_clue, reaction_on_event, Ability, EventPattern, EventTiming, LocationTarget,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01022";

/// Evidence!'s "Play after you defeat an enemy. / Discover 1 clue at your
/// location." — Roland 01001's reaction without the usage limit.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        },
        EventTiming::After,
        discover_clue(LocationTarget::YourLocation, 1),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        Effect, EventPattern, EventTiming, LocationTarget, Trigger, TriggerKind,
    };

    #[test]
    fn abilities_are_one_after_defeat_reaction_discovering_one_clue() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyDefeated {
                    by_controller: true,
                    code: None,
                },
                timing: EventTiming::After,
                kind: TriggerKind::Reaction,
            },
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::DiscoverClue {
                from: LocationTarget::YourLocation,
                count: 1,
            },
        ));
        assert!(
            abilities[0].usage_limit.is_none(),
            "Evidence! is a one-shot event — no per-round usage limit",
        );
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cards evidence`
Expected: FAIL to compile — `evidence` module not declared / `abilities_for` returns `None` for `"01022"`.

- [ ] **Step 3: Register the module**

In `crates/cards/src/impls/mod.rs`, add the module declaration alongside the others (alphabetical, near the `emergency_cache` / `first_aid` neighbours):
```rust
pub mod evidence;
```
And add the `abilities_for` match arm (alphabetical, between `emergency_cache` and `first_aid`):
```rust
        evidence::CODE => Some(evidence::abilities()),
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cards evidence`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add crates/cards/src/impls/evidence.rs crates/cards/src/impls/mod.rs
git commit -m "cards: Evidence! 01022 abilities + registration

Roland 01001's after-defeat reaction sourced from hand, minus the usage
limit. Inert until Axis C machinery (#335) offers it in a window.
Part of #304. Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Migrate the reaction/forced-window resume contract to `PickSingle(OptionId)`

Behavior-preserving refactor: the window emits structured `options` (one per pending in-play trigger) and resumes via `PickSingle(OptionId)` instead of `PickIndex`. No hand events yet. This task's "test" is the existing reaction/forced-window suite staying green after migrating its `pick(n)` calls to `pick_single(OptionId(n))`.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`open_queued_reaction_window`, `advance_resolution`, `resume_reaction_window`; add a private `build_resolution_options` helper)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (the apply-pause guard message at ~line 87-92 referencing `PickIndex`)
- Modify (test migration): `crates/cards/tests/roland_banks.rs`, `crates/cards/tests/dr_milan.rs`, `crates/cards/tests/guard_dog_soak.rs`, `crates/cards/tests/persistent_treachery.rs`, and any in-crate `reaction_windows.rs` / `mod.rs` test submitting `PickIndex` to a reaction window.

**Interfaces:**
- Consumes: `ChoiceOption`, `OptionId`, `InputRequest::choice` (from `engine/outcome.rs`); `ScriptedResolver::pick_single(OptionId)` (already exists in `test_support/resolver.rs`).
- Produces: `build_resolution_options(state, frame) -> Vec<ChoiceOption>` (private to `reaction_windows.rs`); reaction/forced windows now answer `InputResponse::PickSingle(OptionId)`.

- [ ] **Step 1: Migrate one existing test to the new contract (failing test)**

In `crates/cards/tests/roland_banks.rs`, change the two `resolver.commit_cards(&[]).pick(0)` calls (lines ~105, ~204) to:
```rust
    resolver.commit_cards(&[]).pick_single(game_core::engine::OptionId(0));
```
Add the import if needed (the test already imports from `game_core::engine`; reference `game_core::engine::OptionId` inline as shown).

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cards --test roland_banks reaction_fires_after_roland_defeats_enemy_and_discovers_clue`
Expected: FAIL — the reaction window still expects `PickIndex`; a `PickSingle` response hits `resume_reaction_window`'s `other => Rejected` arm.

- [ ] **Step 3: Add the option-builder + migrate the window emit/resume**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`:

Add the imports `ChoiceOption`, `OptionId` to the existing `use crate::engine::...` group (they live in `crate::engine`).

Add the private helper:
```rust
/// Build the structured option list for a resolution frame: one
/// [`ChoiceOption`] per pending in-play trigger, in `pending_triggers`
/// order. `OptionId(i)` is the index into the returned list (the Axis-A
/// convention). Task 4 appends the frame's hand Fast-event plays after the
/// triggers.
fn build_resolution_options(_state: &GameState, frame: &ResolutionFrame) -> Vec<ChoiceOption> {
    let mut options = Vec::new();
    let mut next_id = 0u32;
    for cand in &frame.pending_triggers {
        options.push(ChoiceOption {
            id: OptionId(next_id),
            label: format!("Resolve reaction: {}", cand.code),
        });
        next_id += 1;
    }
    options
}
```

Rewrite `open_queued_reaction_window`'s `AwaitingInput` to use `choice`:
```rust
pub(super) fn open_queued_reaction_window(cx: &mut Cx) -> EngineOutcome {
    let window = cx
        .state
        .top_reaction_window()
        .expect("open_queued_reaction_window: caller checked is_some");
    let skip_hint = if window.is_forced() {
        " (forced — cannot skip; the lead orders them)"
    } else {
        ", or InputResponse::Skip to close"
    };
    let prompt = format!(
        "Resolution window: {} option(s). Submit InputResponse::PickSingle(OptionId){skip_hint}.",
        window.pending_triggers.len(),
    );
    let options = build_resolution_options(cx.state, window);
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, options),
        resume_token: ResumeToken(0),
    }
}
```

Rewrite `advance_resolution`'s re-prompt arm the same way (keep the close branch):
```rust
pub(super) fn advance_resolution(cx: &mut Cx, window_idx: usize) -> EngineOutcome {
    let window = cx.state.continuations[window_idx]
        .as_resolution()
        .expect("advance_resolution: window_idx is a Resolution frame");
    if window.pending_triggers.is_empty() {
        return close_reaction_window_at(cx, window_idx);
    }
    let skip_hint = if window.is_forced() {
        " (forced — cannot skip)"
    } else {
        ", or InputResponse::Skip to close"
    };
    let prompt = format!(
        "Resolution window: {} option(s). Submit InputResponse::PickSingle(OptionId){skip_hint}.",
        window.pending_triggers.len(),
    );
    let options = build_resolution_options(cx.state, window);
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, options),
        resume_token: ResumeToken(0),
    }
}
```

Rewrite `resume_reaction_window`'s match (the `PickIndex` arm becomes `PickSingle`; `Skip` unchanged; update the `other` message). `fire_pending_trigger` already takes a `u32` index into `pending_triggers`, so route the `OptionId`'s inner value through it after a bounds check:
```rust
pub(super) fn resume_reaction_window(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    match response {
        InputResponse::PickSingle(OptionId(i)) => fire_pending_trigger(cx, *i),
        InputResponse::Skip => {
            let idx = cx
                .state
                .top_reaction_window_index()
                .expect("resume_reaction_window: caller checked is_some");
            if cx.state.continuations[idx]
                .as_resolution()
                .is_some_and(ResolutionFrame::is_forced)
            {
                return EngineOutcome::Rejected {
                    reason: "ResolveInput::Skip: forced abilities are mandatory; submit \
                             InputResponse::PickSingle(OptionId) to resolve one (the lead orders them)"
                        .into(),
                };
            }
            close_reaction_window_at(cx, idx)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: reaction window expects InputResponse::PickSingle(OptionId) \
                 or InputResponse::Skip, got {other:?}",
            )
            .into(),
        },
    }
}
```

Update `fire_pending_trigger`'s out-of-bounds reject message (line ~377) from `PickIndex({i})` to `PickSingle(OptionId({i}))` for consistency, and update its doc-comment bullet that references `InputResponse::PickIndex(i)` to `PickSingle`.

In `crates/game-core/src/engine/dispatch/mod.rs`, update the apply-pause guard message (~line 92) that says `InputResponse::PickIndex` to read `InputResponse::PickSingle(OptionId)`.

- [ ] **Step 4: Run the migrated test to verify it passes**

Run: `cargo test -p cards --test roland_banks`
Expected: PASS once Step 5's sibling-test migrations are also applied (the file shares the contract). If only the one test was migrated in Step 1, migrate the remaining `pick(n)` calls in this file now (see Step 5) and re-run.

- [ ] **Step 5: Migrate the remaining reaction/forced-window test call sites**

Replace every `.pick(N)` that drives a reaction or forced window, and every raw `InputResponse::PickIndex(N)` submitted to one, with `pick_single(OptionId(N))` / `InputResponse::PickSingle(OptionId(N))`:
- `crates/cards/tests/roland_banks.rs` (remaining `pick` calls)
- `crates/cards/tests/dr_milan.rs` (raw `PickIndex`)
- `crates/cards/tests/guard_dog_soak.rs` (raw `PickIndex`)
- `crates/cards/tests/persistent_treachery.rs` (`.pick(`)
- Any `#[cfg(test)]` cases in `crates/game-core/src/engine/dispatch/reaction_windows.rs` and `crates/game-core/src/engine/dispatch/mod.rs` that submit `PickIndex` to a reaction/forced window.

Find them all:
```bash
grep -rn '\.pick(\|PickIndex' crates/cards/tests crates/scenarios/tests crates/game-core/src | grep -v 'resolver.rs\|action.rs'
```
For each hit that targets a reaction/forced window, switch to the `PickSingle`/`pick_single` form. Leave any `PickIndex` use that targets a *different* (non-reaction-window) prompt untouched — none should exist after Axis B, but verify each hit's context.

- [ ] **Step 6: Run the full workspace test suite**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS. If a forced-run test (e.g. `crates/scenarios/tests/the_gathering_resolutions.rs`, `the_gathering.rs`) drove a window via `pick`/`PickIndex`, it is now migrated and green.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "engine: migrate reaction/forced windows to PickSingle(OptionId)

Behavior-preserving: windows emit structured ChoiceOption lists (one per
pending trigger) and resume via PickSingle instead of PickIndex, aligning
the reaction-window contract with Axis A. Retires the PickIndex-while-paused
reaction-window path. Part of #335.
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `FastEventCandidate` type + `ResolutionFrame.fast_plays` field

Add the data the hand-event option needs. Pure state types, unit-tested without a registry.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (new `FastEventCandidate` struct near `ResolutionCandidate` ~line 1120; new `fast_plays` field on `ResolutionFrame` ~line 760; `new_empty` ~line 844; `has_pending_options` helper)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (every `ResolutionFrame { … }` literal gains `fast_plays: Vec::new()`: `open_fast_window` ~line 897, `open_forced_resolution` ~line 84)

**Interfaces:**
- Produces:
  - `pub struct FastEventCandidate { pub controller: InvestigatorId, pub code: CardCode, pub ability_index: u8 }`
  - `ResolutionFrame.fast_plays: Vec<FastEventCandidate>`
  - `ResolutionFrame::has_pending_options(&self) -> bool` (`!pending_triggers.is_empty() || !fast_plays.is_empty()`)

- [ ] **Step 1: Write the failing unit test**

In `game_state.rs`'s test module (or the nearest `#[cfg(test)] mod` covering `ResolutionFrame`), add:
```rust
#[test]
fn new_empty_frame_has_no_fast_plays_and_no_pending_options() {
    let frame = ResolutionFrame::new_empty(
        WindowKind::AfterEnemyDefeated { enemy: EnemyId(1), by: Some(InvestigatorId(1)) },
        FastActorScope::Any,
    );
    assert!(frame.fast_plays.is_empty());
    assert!(!frame.has_pending_options());
}

#[test]
fn fast_event_candidate_serde_round_trips() {
    let c = FastEventCandidate {
        controller: InvestigatorId(1),
        code: CardCode::new("01022"),
        ability_index: 0,
    };
    let json = serde_json::to_string(&c).expect("serialize");
    let back: FastEventCandidate = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, c);
}
```
(Use the imports already present in that test module; add `FastActorScope`, `EnemyId`, `FastEventCandidate` if missing.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core new_empty_frame_has_no_fast_plays_and_no_pending_options fast_event_candidate_serde_round_trips`
Expected: FAIL — `FastEventCandidate` / `fast_plays` / `has_pending_options` undefined.

- [ ] **Step 3: Add the type, field, and helper**

Near `ResolutionCandidate` (~line 1120) in `game_state.rs`:
```rust
/// One Fast event playable from hand that matches an open reaction
/// window's event (Axis C, #335). The window offers it as a `PickSingle`
/// option alongside in-play [`ResolutionCandidate`]s; picking it plays the
/// event and runs ability `ability_index`'s effect. Sourced from hand, so
/// there is no in-play instance id (unlike `ResolutionCandidate.source`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FastEventCandidate {
    /// The investigator whose hand holds the event and who plays it.
    pub controller: InvestigatorId,
    /// Printed code of the Fast event in hand.
    pub code: CardCode,
    /// Index into the card's abilities — the `OnEvent` ability whose
    /// pattern matched this window and whose effect resolves on play.
    pub ability_index: u8,
}

impl FastEventCandidate {
    /// Construct a candidate (mirrors the struct fields; provided so
    /// external integration tests can build one despite `#[non_exhaustive]`).
    #[must_use]
    pub fn new(controller: InvestigatorId, code: CardCode, ability_index: u8) -> Self {
        Self { controller, code, ability_index }
    }
}
```

Add the field to `ResolutionFrame` (~line 766, after `pending_triggers`):
```rust
    /// Fast events in hand that match this window's event (Axis C). Empty
    /// for the forced run and for windows opened before Axis C scanned
    /// hands. Offered as `PickSingle` options after `pending_triggers`.
    pub fast_plays: Vec<FastEventCandidate>,
```

Update `new_empty` (~line 844) to initialise it:
```rust
    pub fn new_empty(kind: WindowKind, fast_actors: FastActorScope) -> Self {
        Self {
            pending_triggers: Vec::new(),
            fast_plays: Vec::new(),
            kind: ResolutionKind::Window(WindowBinding { kind, fast_actors }),
        }
    }
```

Add the helper in `impl ResolutionFrame`:
```rust
    /// True while this frame still has an option to offer — a pending
    /// in-play trigger or a hand Fast-event play. The close condition for
    /// a resolution run is the negation.
    #[must_use]
    pub fn has_pending_options(&self) -> bool {
        !self.pending_triggers.is_empty() || !self.fast_plays.is_empty()
    }
```

In `reaction_windows.rs`, add `fast_plays: Vec::new()` to the two remaining `ResolutionFrame { … }` literals (`open_fast_window`, `open_forced_resolution`). `queue_reaction_window`'s literal is updated in Task 4.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p game-core new_empty_frame_has_no_fast_plays_and_no_pending_options fast_event_candidate_serde_round_trips`
Expected: PASS.

- [ ] **Step 5: Run the workspace suite (no behavior change yet)**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "engine: FastEventCandidate + ResolutionFrame.fast_plays field

The data a hand Fast-event option carries (controller + code + matched
ability index) and where it rides on the resolution frame. No behavior yet.
Part of #335. Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Scan hand for matching Fast events; open the window on a hand match; offer them as options

The after-defeat window now opens when *either* an in-play trigger or a hand Fast-event matches, and offers the hand events as options after the triggers. No play yet — `Skip` closes without discovering.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`scan_hand_fast_events`; `queue_reaction_window`; `build_resolution_options`; `advance_resolution` close condition)
- Create: `crates/cards/tests/evidence.rs` (integration test)

**Interfaces:**
- Consumes: `trigger_matches` (existing), `card_registry::current()`, `metadata.is_fast()` / `metadata.card_type()`.
- Produces: `scan_hand_fast_events(state: &GameState, kind: WindowKind) -> Vec<FastEventCandidate>` (private).

- [ ] **Step 1: Write the failing integration test**

Create `crates/cards/tests/evidence.rs`:
```rust
//! End-to-end tests for Evidence! 01022 (Axis C reaction-event-play, #304)
//! against the real `cards::REGISTRY`.
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//! > Fast. Play after you defeat an enemy.
//! > Discover 1 clue at your location.

use std::sync::Once;

use game_core::engine::{EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, InvestigatorId, LocationId, Phase, TokenModifiers,
    WindowKind,
};
use game_core::test_support::{
    drive, test_enemy, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event, assert_no_event, Action, PlayerAction};

const EVIDENCE: &str = "01022";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Solo investigator (NOT Roland — no in-play reaction) engaged with a
/// 1-HP enemy at a location with `location_clues` clues, holding Evidence!
/// in hand. A successful Combat test defeats the enemy and opens the
/// after-defeat window.
fn investigator_with_evidence_and_enemy(
    location_clues: u8,
) -> (InvestigatorId, EnemyId, LocationId, game_core::GameState) {
    install_real_registry();
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4;
    inv.hand.push(CardCode::new(EVIDENCE));

    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;
    enemy.engaged_with = Some(inv_id);

    let mut loc = test_location(10, "Study");
    loc.clues = location_clues;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(0)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, loc_id, state)
}

fn fight_action(inv: InvestigatorId, enemy: EnemyId) -> Action {
    Action::Player(PlayerAction::Fight { investigator: inv, enemy })
}

#[test]
fn after_defeat_window_opens_and_offers_evidence_with_no_in_play_reaction() {
    let (inv_id, enemy_id, loc_id, state) = investigator_with_evidence_and_enemy(2);

    // Commit nothing to the Fight test, then SKIP the reaction window
    // (Task 4: the option is offered but playing it lands in Task 5).
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]).skip();
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    // The window opens even though no in-play card reacts — the hand match
    // alone opens it.
    assert_event!(
        result.events,
        Event::WindowOpened {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );
    // Skipped → no clue discovered, Evidence! still in hand.
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.locations[&loc_id].clues, 2);
    assert!(result.state.investigators[&inv_id]
        .hand
        .iter()
        .any(|c| c.as_str() == EVIDENCE));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cards --test evidence after_defeat_window_opens_and_offers_evidence_with_no_in_play_reaction`
Expected: FAIL — no `WindowOpened` (defeating the enemy with no in-play reaction queues no window today: `queue_reaction_window` bails on empty `pending_triggers`).

- [ ] **Step 3: Add the hand scan and the open-on-hand-match logic**

In `reaction_windows.rs`, add the scan (mirrors `scan_pending_triggers`' investigator ordering and registry fallback):
```rust
/// Scan every window-eligible investigator's hand for Fast **events**
/// carrying an `OnEvent` ability whose pattern matches `kind` (Axis C,
/// #335). The play-timing predicate is the same `trigger_matches` used for
/// in-play reactions — a Fast reaction event is its in-play twin sourced
/// from hand (RR p.11). Returns active-investigator-first / turn-order
/// order, like `scan_pending_triggers`. Empty when no registry is installed.
fn scan_hand_fast_events(state: &GameState, kind: WindowKind) -> Vec<FastEventCandidate> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    let mut order: Vec<InvestigatorId> = Vec::with_capacity(state.turn_order.len());
    if let Some(active) = state.active_investigator {
        order.push(active);
    }
    for id in &state.turn_order {
        if Some(*id) != state.active_investigator {
            order.push(*id);
        }
    }

    let mut plays = Vec::new();
    for id in order {
        let Some(inv) = state.investigators.get(&id) else {
            continue;
        };
        for code in &inv.hand {
            let Some(meta) = (reg.metadata_for)(code) else {
                continue;
            };
            if !meta.is_fast() || meta.card_type() != CardType::Event {
                continue;
            }
            let Some(abilities) = (reg.abilities_for)(code) else {
                continue;
            };
            for (idx, ability) in abilities.iter().enumerate() {
                let Trigger::OnEvent { pattern, timing, .. } = &ability.trigger else {
                    continue;
                };
                if !trigger_matches(kind, pattern, *timing, id) {
                    continue;
                }
                let ability_index = u8::try_from(idx)
                    .expect("abilities vec exceeds u8::MAX — card-impl bug, abilities are tiny");
                plays.push(FastEventCandidate { controller: id, code: code.clone(), ability_index });
                // One option per card: a card with two matching abilities is
                // still played once. No in-scope card has two.
                break;
            }
        }
    }
    plays
}
```
Add `CardType` and `FastEventCandidate` to the `use` lists at the top of the file as needed.

Update `queue_reaction_window` to scan both sources and store `fast_plays`:
```rust
pub(super) fn queue_reaction_window(cx: &mut Cx, kind: WindowKind) {
    let pending_triggers = scan_pending_triggers(cx.state, kind);
    let fast_plays = scan_hand_fast_events(cx.state, kind);
    if pending_triggers.is_empty() && fast_plays.is_empty() {
        return;
    }
    cx.events.push(Event::WindowOpened { kind });
    cx.state
        .continuations
        .push(Continuation::Resolution(ResolutionFrame {
            pending_triggers,
            fast_plays,
            kind: ResolutionKind::Window(WindowBinding {
                kind,
                fast_actors: FastActorScope::Any,
            }),
        }));
}
```

Extend `build_resolution_options` to append the hand plays after the triggers:
```rust
    for play in &frame.fast_plays {
        options.push(ChoiceOption {
            id: OptionId(next_id),
            label: format!("Play {} from hand", play.code),
        });
        next_id += 1;
    }
    options
```

Update `advance_resolution`'s close condition to use the helper (so a window with only a remaining hand play stays open):
```rust
    if !window.has_pending_options() {
        return close_reaction_window_at(cx, window_idx);
    }
```
And update both `open_queued_reaction_window` and `advance_resolution` prompt counts from `window.pending_triggers.len()` to `build_resolution_options(cx.state, window).len()` (so the prompt reflects all offered options).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cards --test evidence after_defeat_window_opens_and_offers_evidence_with_no_in_play_reaction`
Expected: PASS — the window opens on the hand match and `Skip` closes it without discovering.

- [ ] **Step 5: Run the workspace suite (regression)**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS. Roland's tests still pass: with Roland in play the window already opened; now `build_resolution_options` includes his trigger plus any hand events (none in those fixtures).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "engine: open reaction window on a matching hand Fast-event

queue_reaction_window now scans hands for Fast events whose OnEvent pattern
matches the window (reusing trigger_matches) and opens when either an in-play
trigger or a hand event matches; hand events are offered as PickSingle options
after the triggers. Playing them lands next. Part of #335 / #304.
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Play a picked hand Fast-event option

Route a `PickSingle` whose id falls past the in-play triggers to playing the event: emit `CardPlayed`, run the matched ability's effect, discard via `pending_played_event`.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`resume_reaction_window` id-splitting; new `play_fast_event_in_window`)
- Modify: `crates/cards/tests/evidence.rs` (add the play test + the both-sources test)

**Interfaces:**
- Consumes: `apply_effect` (evaluator), `EvalContext::for_controller`, `pending_played_event` slot, `advance_resolution`.
- Produces: `play_fast_event_in_window(cx, window_idx, fast_play_idx) -> EngineOutcome` (private).

- [ ] **Step 1: Write the failing play test**

In `crates/cards/tests/evidence.rs`, add:
```rust
#[test]
fn picking_evidence_plays_it_and_discovers_a_clue() {
    let (inv_id, enemy_id, loc_id, state) = investigator_with_evidence_and_enemy(2);

    // Commit nothing, then pick the single offered option (OptionId(0) =
    // the hand Evidence! play; there is no in-play trigger).
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]).pick_single(OptionId(0));
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::CardPlayed { investigator, code } if *investigator == inv_id && code.as_str() == EVIDENCE
    );
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::CardDiscarded { investigator, code, from: game_core::state::Zone::Hand }
            if *investigator == inv_id && code.as_str() == EVIDENCE
    );
    assert_event!(
        result.events,
        Event::WindowClosed {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );

    // 1 clue moved from the Study to the investigator; Evidence! is in discard.
    assert_eq!(result.state.locations[&loc_id].clues, 1);
    assert_eq!(result.state.investigators[&inv_id].clues, 1);
    let inv = &result.state.investigators[&inv_id];
    assert!(!inv.hand.iter().any(|c| c.as_str() == EVIDENCE));
    assert!(inv.discard.iter().any(|c| c.as_str() == EVIDENCE));
}
```
(Import `game_core::state::Zone` if the `from:` matcher needs it — add to the `use game_core::state::{…}` group.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cards --test evidence picking_evidence_plays_it_and_discovers_a_clue`
Expected: FAIL — `OptionId(0)` routes to `fire_pending_trigger(0)`, but `pending_triggers` is empty, so it rejects out-of-bounds (no in-play trigger). The hand play isn't wired.

- [ ] **Step 3: Split the id in `resume_reaction_window` and add the play routine**

In `resume_reaction_window`, replace the `PickSingle` arm with id-splitting against the active frame's `pending_triggers.len()`:
```rust
        InputResponse::PickSingle(OptionId(i)) => {
            let window_idx = cx
                .state
                .top_reaction_window_index()
                .expect("resume_reaction_window: caller checked is_some");
            let trigger_count = u32::try_from(
                cx.state.continuations[window_idx]
                    .as_resolution()
                    .expect("top_reaction_window_index points at a Resolution frame")
                    .pending_triggers
                    .len(),
            )
            .expect("pending_triggers length fits in u32");
            if *i < trigger_count {
                fire_pending_trigger(cx, *i)
            } else {
                play_fast_event_in_window(cx, window_idx, (*i - trigger_count) as usize)
            }
        }
```

Add the play routine (mirrors `fire_pending_trigger`'s snapshot-then-apply shape and `play_card`'s `pending_played_event` discard; charges no resource cost, matching `play_card`):
```rust
/// Play the `fast_play_idx`-th hand Fast-event in the resolution frame at
/// `window_idx`: emit `CardPlayed`, stash the event in `pending_played_event`
/// (RR Appendix I step 3 — leaves hand at play-start), run the matched
/// ability's effect, then advance the run (close → the apply loop flushes the
/// event to discard on completion, RR Appendix I step 4). Mirrors the
/// suspending-event discard path Dynamite Blast 01024 already uses.
fn play_fast_event_in_window(
    cx: &mut Cx,
    window_idx: usize,
    fast_play_idx: usize,
) -> EngineOutcome {
    // Snapshot + remove the candidate before resolving, so a suspending
    // effect's resume drives the remaining options, not this one again.
    let candidate = {
        let frame = cx.state.continuations[window_idx]
            .as_resolution_mut()
            .expect("play_fast_event_in_window: window_idx is a Resolution frame");
        if fast_play_idx >= frame.fast_plays.len() {
            return EngineOutcome::Rejected {
                reason: format!(
                    "ResolveInput: PickSingle hand-play index {fast_play_idx} out of bounds \
                     (fast_plays size {})",
                    frame.fast_plays.len(),
                )
                .into(),
            };
        }
        frame.fast_plays.remove(fast_play_idx)
    };

    // Find the event in the controller's hand by code (first match — copies
    // are fungible; this avoids stale hand indices after a prior play).
    let controller = candidate.controller;
    let hand_idx = cx
        .state
        .investigators
        .get(&controller)
        .and_then(|inv| inv.hand.iter().position(|c| *c == candidate.code))
        .unwrap_or_else(|| {
            unreachable!(
                "play_fast_event_in_window: Evidence-style candidate {candidate:?} vanished \
                 from {controller:?}'s hand between scan and play",
            )
        });

    cx.events.push(Event::CardPlayed {
        investigator: controller,
        code: candidate.code.clone(),
    });
    let card = cx
        .state
        .investigators
        .get_mut(&controller)
        .expect("controller exists (checked above)")
        .hand
        .remove(hand_idx);
    cx.state.pending_played_event = Some((controller, card));

    // Run the matched OnEvent ability's effect under the playing investigator.
    let reg = card_registry::current().unwrap_or_else(|| {
        unreachable!("play_fast_event_in_window: registry installed at scan time is now missing")
    });
    let abilities = (reg.abilities_for)(&candidate.code).unwrap_or_else(|| {
        unreachable!("play_fast_event_in_window: registry lost abilities for {:?}", candidate.code)
    });
    let effect = abilities
        .get(usize::from(candidate.ability_index))
        .expect("ability_index validated at scan time")
        .effect
        .clone();
    let eval_ctx = EvalContext::for_controller(controller);

    match apply_effect(cx, &effect, eval_ctx) {
        EngineOutcome::Rejected { reason } => unreachable!(
            "Fast-event play: effect for {:?} rejected unexpectedly: {reason}",
            candidate.code,
        ),
        suspended @ EngineOutcome::AwaitingInput { .. } => suspended,
        EngineOutcome::Done => advance_resolution(cx, window_idx),
    }
}
```
Add any missing imports (`EvalContext`, `apply_effect` — both already used elsewhere in this file).

- [ ] **Step 4: Run the play test to verify it passes**

Run: `cargo test -p cards --test evidence picking_evidence_plays_it_and_discovers_a_clue`
Expected: PASS — Evidence! plays, 1 clue discovered, event discarded, window closed.

- [ ] **Step 5: Add the both-sources test**

In `crates/cards/tests/evidence.rs`, add a fixture variant that ALSO puts Roland 01001 in play (so the window offers two options: his in-play trigger at `OptionId(0)` and the hand Evidence! at `OptionId(1)`), then pick the Evidence! option:
```rust
#[test]
fn window_offers_both_in_play_reaction_and_hand_evidence() {
    use game_core::state::{CardInPlay, CardInstanceId};
    install_real_registry();
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4;
    inv.hand.push(CardCode::new(EVIDENCE));
    // Roland's investigator card in play → his after-defeat reaction also matches.
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new("01001"), CardInstanceId(1)));

    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;
    enemy.engaged_with = Some(inv_id);

    let mut loc = test_location(10, "Study");
    loc.clues = 2;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(0)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    // Two options: OptionId(0) = Roland's in-play reaction, OptionId(1) =
    // hand Evidence!. Pick the hand play, then skip the remaining reaction.
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]).pick_single(OptionId(1)).skip();
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::CardPlayed { investigator, code } if *investigator == inv_id && code.as_str() == EVIDENCE
    );
    // Evidence! discovered its clue; Roland's reaction was skipped.
    assert_event!(result.events, Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id);
    assert_eq!(result.state.investigators[&inv_id].clues, 1);
}
```

- [ ] **Step 6: Run the play tests + workspace suite**

Run: `cargo test -p cards --test evidence`
Expected: PASS (all three).

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "engine: play a picked hand Fast-event in a reaction window

A PickSingle id past the in-play triggers routes to play_fast_event_in_window:
emit CardPlayed, run the matched ability's effect, discard via
pending_played_event. Closes Evidence! 01022 (#304) and the Axis C
reaction-event-play machinery (#335).
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Full gauntlet, PR, and phase-doc update

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md` (final commit, after CI green)

- [ ] **Step 1: Run the complete CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green. Fix any clippy/doc/fmt issues with follow-up edits (e.g. intra-doc links for new items; `cargo fmt` to format).

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin engine/axis-c-reaction-event-play
gh pr create --fill
```
PR body: summarise the contract migration + the hand-event option + Evidence!; note the scope boundary (framework `open_fast_window` untouched; Fast assets/abilities not offered; Axis D deferred). Include `Closes #335` and `Closes #304`. End the body with the Claude Code attribution line.

- [ ] **Step 3: Watch CI**

```bash
gh pr checks <PR#> --watch
```
Fix failures with follow-up commits to the same branch (no force-push).

- [ ] **Step 4: Update the phase doc as the FINAL commit (only once CI is green)**

In `docs/phases/phase-7-the-gathering.md`, in the "Future slices" → Axis C area / the Group-C breakdown:
- Mark Axis C (#335) and Evidence! (#304) shipped (`✅ PR #<n>`).
- Add a **Decisions made** entry capturing only what a future PR-author would choose differently without it. Candidates (keep to the load-bearing ones):
  - "A Fast reaction event is its in-play-reaction twin sourced from hand; the play-timing predicate is the existing `OnEvent`/`trigger_matches` match, not a new field (RR p.11). Evidence! reuses Roland 01001's declaration minus the usage limit."
  - "Reaction/forced windows resolve via `PickSingle(OptionId)`; options = `{in-play triggers} ∪ {matching hand Fast-events}`. The `PickIndex`-while-paused reaction-window path is retired (variant kept for other callers). A window opens when *either* source matches."
  - "Scope boundary: the framework `open_fast_window` non-paused Fast-play path and Fast-asset/Fast-ability window options are deferred — no Slice-1 card needs them. Dodge 01023 still needs Axis D (cancellation, #336)."
- Remove any now-settled open question this PR closes.

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — Axis C reaction-event-play + Evidence! shipped

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push
```

- [ ] **Step 5: Merge only after explicit user approval**

```bash
gh pr merge <PR#> --squash --delete-branch
```
Then confirm #335 / #304 auto-closed and `git checkout main && git pull`.

---

## Notes for the implementer

- **Why no new predicate language:** Evidence! and Roland 01001 compile to the same `OnEvent { EnemyDefeated { by_controller: true, code: None }, After, Reaction }` + `discover_clue(YourLocation, 1)`. The only difference is the source zone; the engine already matches that pattern via `trigger_matches`.
- **Why `pending_played_event` (not immediate discard):** Evidence!'s `DiscoverClue` can itself suspend (a Cover-Up-style `WouldDiscoverClues` interrupt), so the event must discard on *completion*, not at play-start. The single-slot `pending_played_event` + the apply loop's flush-on-`Done` is the existing mechanism (Dynamite Blast 01024).
- **Why find-by-code in `play_fast_event_in_window` (not a stored hand index):** playing one event shifts later hand indices; resolving the card by code keeps a second queued play (exotic; two Evidence! copies) correct without index bookkeeping.
- **Borrow discipline:** mirror `fire_pending_trigger` — snapshot/remove from the frame first, then mutate `state`/run `apply_effect`; `apply_effect` pushes no continuations, so `window_idx` stays valid across it.
- **Registry-dependent behavior is integration-tested** in `crates/cards/tests/evidence.rs` (own process → `install` is safe); pure logic (`trigger_matches`, frame helpers) is unit-tested in-crate.
```
