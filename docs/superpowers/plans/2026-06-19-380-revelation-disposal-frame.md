# #380 — Revelation disposal as an `EncounterCard` frame — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or subagent-driven-development) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the global `pending_revelation_discard` side-channel (flushed only by the skill-test driver) with a `Continuation::EncounterCard { card }` frame whose framework teardown disposes of the card after its Revelation's *whole* sub-resolution completes — covering suspension into a skill test **or a choice** (the latter is currently broken).

**Architecture:** Spec §E. `resolve_encounter_card`'s treachery branch pushes an `EncounterCard` frame, runs the Revelation, then calls a shared `teardown_encounter_card_if_top(cx)`: if the top frame is `EncounterCard`, dispose (one-shot → `encounter_discard`; persistent → skip) and pop, else no-op. It is called from two sites — inline in `resolve_encounter_card` (synchronous Revelation) and at the `resolve_input` chokepoint after a resume returns `Done` (suspended-then-resumed). No resume handler knows treacheries exist.

**Tech Stack:** Rust — `game-core` (engine), `scenarios` test fixtures.

## Global Constraints

- **CI gauntlet, warnings-as-errors** (all seven jobs) before every push.
- **No behavior change** for any in-scope card/scenario that already worked: `crates/cards/tests/revelation_treacheries.rs` (Grasping Hands 01162 suspended-into-skill-test discard, Crypt Chill 01167 fail-branch-choice-within-skill-test, Ancient Evils 01166) and the synthetic-treachery Mythos tests must pass unchanged. The **only** intended behavior change is fixing the *new* case: a Revelation that suspends **directly** into a choice now discards (today it never does).
- The treachery discard is **eventless** (a bare `encounter_discard.push(code)`), matching today's `resolve_encounter_card` path — no `CardDiscarded` event.

---

## File structure

- `crates/game-core/src/state/game_state.rs` — add `Continuation::EncounterCard { card }` + classifier arms; add `current_encounter_card()` is **not** needed (framework-internal); remove the `pending_revelation_discard` field (Task 3).
- `crates/game-core/src/engine/dispatch/encounter.rs` — push the frame in `resolve_encounter_card`; add `teardown_encounter_card_if_top`; drop the `pending_revelation_discard` set-site.
- `crates/game-core/src/engine/dispatch/mod.rs` — `resolve_input` chokepoint teardown + the exhaustive-match `EncounterCard` arm.
- `crates/game-core/src/engine/dispatch/skill_test.rs` — remove the driver flush; delete/reframe the two slot unit tests.
- `crates/game-core/src/state/builder.rs` — remove the `pending_revelation_discard: None` init (Task 3).
- `crates/scenarios/src/test_fixtures/synth_cards.rs` — add a choice-Revelation synthetic treachery.
- `crates/scenarios/tests/revelation_choice.rs` — **new** integration test (the #380 motivating case).

---

## Task 1: Synthetic choice-Revelation treachery + RED test

A treachery whose Revelation is a top-level `ChooseOne` (two `gain_resources` branches) — distinct from Crypt Chill, whose choice is *nested inside a skill test*. Today drawing it leaves it un-discarded (the slot is set but never flushed); the new test asserts the discard and so **fails RED** until Task 2.

**Files:**
- Modify: `crates/scenarios/src/test_fixtures/synth_cards.rs`
- Create: `crates/scenarios/tests/revelation_choice.rs`

**Interfaces produced:**
- `synth_cards::SYNTH_CHOICE_TREACHERY_CODE: &str` (`"_synth_choice_treachery"`).
- `synth_cards::TEST_REGISTRY` resolves the new code's metadata + abilities.

- [ ] **Step 1: Add the fixture's code constant + metadata.** In `synth_cards.rs`, near `SYNTH_SURGE_TREACHERY_CODE`:

```rust
/// Code for a synthetic treachery whose Revelation is a top-level
/// `Effect::ChooseOne` (gain 2 vs gain 5 resources) — i.e. it suspends
/// **directly** into a choice, *not* nested inside a skill test (the Crypt
/// Chill 01167 shape). The #380 motivating case: today its disposal is
/// stranded because the slot is only flushed by the skill-test driver.
pub const SYNTH_CHOICE_TREACHERY_CODE: &str = "_synth_choice_treachery";
```

And a metadata builder modeled on `synth_treachery_metadata` (treachery type, one-shot):

```rust
fn synth_choice_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_CHOICE_TREACHERY_CODE.to_owned(),
        name: "Synthetic Choice Treachery".to_owned(),
        text: Some(
            "Revelation - Choose one: gain 2 resources; or gain 5 resources. \
             (Synthetic; not a printed card.)"
                .to_owned(),
        ),
        ..synth_treachery_metadata() // same CardType::Treachery shell
    }
}

fn synth_choice_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_choice_treachery_metadata)
}
```

> NOTE: if `synth_treachery_metadata` is not `..`-spreadable (non-`Default`/private fields), copy its literal body and change `code`/`name`/`text`. Verify the exact `CardMetadata` shape in `synth_treachery_metadata` first and mirror it.

- [ ] **Step 2: Add the abilities builder + register both lookups.** In `synth_cards.rs`, add to the abilities the `revelation(choose_one([...]))` (import `choose_one`, `gain_resources`, `revelation`, `InvestigatorTarget` as the file already imports the DSL prelude — confirm and extend the `use` line):

```rust
fn synth_choice_treachery_abilities() -> Vec<Ability> {
    vec![revelation(choose_one([
        gain_resources(InvestigatorTarget::You, 2),
        gain_resources(InvestigatorTarget::You, 5),
    ]))]
}
```

Wire the registry fns (`metadata_for` and `abilities_for` match arms — mirror the `SYNTH_TREACHERY_CODE` arms):

```rust
// metadata_for:
SYNTH_CHOICE_TREACHERY_CODE => Some(synth_choice_treachery_metadata_static()),
// abilities_for:
SYNTH_CHOICE_TREACHERY_CODE => Some(synth_choice_treachery_abilities()),
```

- [ ] **Step 3: Write the RED integration test.** Create `crates/scenarios/tests/revelation_choice.rs`:

```rust
//! #380: a treachery whose Revelation suspends **directly** into a choice
//! (not nested in a skill test) must still be discarded once the choice
//! resolves. Before the `EncounterCard`-frame fix the disposal was stranded
//! (the `pending_revelation_discard` slot was set but only the skill-test
//! driver flushed it).

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::state::CardCode;
use game_core::{Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::{SYNTH_CHOICE_TREACHERY_CODE, TEST_REGISTRY};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

/// Drive synthetic setup → mulligan → the last EndTurn into Mythos, leaving the
/// state at the encounter-draw prompt with only the choice-treachery on top of
/// the encounter deck.
fn at_mythos_draw_with_choice_treachery() -> game_core::state::GameState {
    install();
    let mut state = synthetic::setup();
    let (state2, _) = drive(
        state.clone(),
        vec![
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected: vec![] },
            }),
            Action::Player(PlayerAction::EndTurn),
        ],
    );
    state = state2;
    // Seed the controlled draw order AFTER StartScenario's shuffle.
    synthetic::with_encounter_deck(
        &mut state,
        vec![CardCode::new(SYNTH_CHOICE_TREACHERY_CODE)],
    );
    state
}

fn drive(
    initial: game_core::state::GameState,
    actions: Vec<Action>,
) -> (game_core::state::GameState, ()) {
    let mut state = initial;
    for action in actions {
        state = apply(state, action).state;
    }
    (state, ())
}

#[test]
fn revelation_suspending_into_a_choice_discards_after_the_pick() {
    let state = at_mythos_draw_with_choice_treachery();

    // Confirm the draw → the Revelation's ChooseOne suspends for the pick.
    let drawn = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(
        matches!(drawn.outcome, EngineOutcome::AwaitingInput { .. }),
        "the Revelation choice suspends, got {:?}",
        drawn.outcome
    );
    let res_before = drawn.state.investigators[&game_core::state::InvestigatorId(1)].resources;

    // Pick branch 0 (gain 2 resources). The choice resolves, and the framework
    // disposes of the treachery to encounter_discard.
    let resolved = apply(
        drawn.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert_eq!(
        resolved.state.investigators[&game_core::state::InvestigatorId(1)].resources,
        res_before + 2,
        "branch 0 granted 2 resources",
    );
    assert!(
        resolved
            .state
            .encounter_discard
            .contains(&CardCode::new(SYNTH_CHOICE_TREACHERY_CODE)),
        "the treachery discards once its directly-suspended choice resolves",
    );
}
```

> NOTE: confirm `synthetic::with_encounter_deck` exists with that signature (it is used in `mythos_phase.rs`); if its name differs, match the real helper. If `drive` already exists as a shared scenarios test helper, reuse it instead of redefining.

- [ ] **Step 4: Run the test — expect RED.**

```sh
cargo test -p scenarios --test revelation_choice revelation_suspending_into_a_choice_discards_after_the_pick
```

Expected: **FAIL** at the `encounter_discard.contains(...)` assertion (today the treachery is never discarded — the slot is set on suspend but no skill-test driver flushes it).

- [ ] **Step 5: Commit (RED test + fixture).**

```sh
git add crates/scenarios/src/test_fixtures/synth_cards.rs crates/scenarios/tests/revelation_choice.rs
git commit -m "test: failing #380 case — Revelation suspending directly into a choice never discards"
```

---

## Task 2: `EncounterCard` frame + teardown + chokepoint (GREEN)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (variant + classifier)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (push frame, teardown helper, drop slot-set)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (chokepoint + match arm)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (drop driver flush; delete obsolete unit test)

**Interfaces produced:**
- `Continuation::EncounterCard { card: CardCode }`.
- `encounter::teardown_encounter_card_if_top(cx: &mut Cx) -> EngineOutcome` (`pub(super)`).

- [ ] **Step 1: Add the `EncounterCard` variant + classifier arms.** In `game_state.rs`, after the `EncounterDraw` variant:

```rust
    /// A drawn encounter **treachery** whose Revelation is mid-resolution
    /// (#380). Pushed by `resolve_encounter_card` *before* it runs the
    /// Revelation; sits beneath any suspension the Revelation opens (skill
    /// test, choice, nested effect). When that sub-resolution completes and
    /// this frame is top again, the **framework** disposes of the card
    /// (one-shot → `encounter_discard`; persistent → it placed itself, skip)
    /// and pops. Suspension-reason-agnostic — replaces the former
    /// `pending_revelation_discard` slot, which only the skill-test driver
    /// flushed. Never emits `AwaitingInput`.
    EncounterCard {
        /// The drawn treachery's card code, disposed of at teardown.
        card: CardCode,
    },
```

Add `| Continuation::EncounterCard { .. } => None` to both `as_resolution` and `as_resolution_mut`.

- [ ] **Step 2: Add `teardown_encounter_card_if_top` to `encounter.rs`.** Near `advance_encounter_draw`:

```rust
/// If the top continuation frame is a [`Continuation::EncounterCard`], dispose
/// of its card per the framework default and pop the frame; otherwise a no-op.
/// Always returns [`EngineOutcome::Done`].
///
/// Disposal (RR p.18 default): a one-shot treachery is discarded to
/// `encounter_discard`; a **persistent** treachery (one carrying a
/// non-`Revelation` ability) placed itself during its Revelation and owns its
/// own disposition, so it is skipped. Persistence is re-derived from the
/// registry by card code — the frame stays payload-minimal (#380). The discard
/// is eventless, matching the synchronous path. The `while` loop covers
/// hypothetical nested encounter resolutions at no cost.
pub(super) fn teardown_encounter_card_if_top(cx: &mut Cx) -> EngineOutcome {
    while let Some(Continuation::EncounterCard { card }) = cx.state.continuations.last() {
        let card = card.clone();
        cx.state.continuations.pop();
        let persistent = card_registry::current()
            .and_then(|reg| (reg.abilities_for)(&card))
            .is_some_and(|abilities| treachery_is_persistent(&abilities));
        if !persistent {
            cx.state.encounter_discard.push(card);
        }
    }
    EngineOutcome::Done
}
```

- [ ] **Step 3: Wire `resolve_encounter_card`'s treachery branch.** Replace the treachery branch's revelation loop + tail. Push the frame before the loop; on suspend just return (the frame stays beneath); on synchronous completion call the teardown:

```rust
        CardType::Treachery => {
            let Some(registry) = card_registry::current() else {
                return EngineOutcome::Rejected {
                    reason: "encounter card resolution: no card registry installed".into(),
                };
            };
            let abilities = (registry.abilities_for)(&code).unwrap_or_default();
            let eval_ctx = EvalContext::for_controller(investigator);
            // Push the disposal frame BEFORE the Revelation so the framework
            // disposes of the card after the Revelation's whole sub-resolution
            // completes — even if it suspends into a skill test or a choice
            // (#380). A mid-Revelation Rejected is rolled back by the apply
            // loop's transactional snapshot, frame included.
            cx.state
                .continuations
                .push(Continuation::EncounterCard { card: code.clone() });
            for ability in abilities.iter().filter(|a| a.trigger == Trigger::Revelation) {
                match apply_effect(cx, &ability.effect, eval_ctx) {
                    EngineOutcome::Done => {}
                    // Suspended: the EncounterCard frame sits beneath the
                    // suspension; the resolve_input chokepoint disposes of it
                    // when the sub-resolution completes.
                    outcome @ EngineOutcome::AwaitingInput { .. } => return outcome,
                    outcome @ EngineOutcome::Rejected { .. } => return outcome,
                }
            }
            // Synchronous completion → dispose now (frame is still top).
            teardown_encounter_card_if_top(cx)
        }
```

Delete the old `pending_revelation_discard = Some(code.clone())` line and the trailing `if !treachery_is_persistent(&abilities) { cx.state.encounter_discard.push(code); } EngineOutcome::Done` (their logic moves into `teardown_encounter_card_if_top`). Ensure `Continuation` is imported in `encounter.rs` (it is, from Task b1 of 2c-iii-b).

- [ ] **Step 4: Add the `resolve_input` chokepoint + match arm in `mod.rs`.** In `resolve_input`, add an `EncounterCard` arm to the top-frame match (it never awaits input, so reject — defensive), and wrap the dispatch result so a completed resume disposes of an underlying `EncounterCard`:

```rust
    use crate::state::Continuation;
    let outcome = match cx.state.continuations.last() {
        // ... existing arms ...
        Some(Continuation::EncounterCard { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (encounter-card disposal is \
                     framework-internal)"
                .into(),
        },
        Some(Continuation::SkillTest(_)) => resume_skill_test_commit(cx, response),
        None => EngineOutcome::Rejected {
            reason: "ResolveInput: no AwaitingInput prompt is currently outstanding".into(),
        },
    };
    // A treachery Revelation that suspended (#380) parks its EncounterCard frame
    // beneath the suspension; once that sub-resolution completes (Done) the
    // frame is top again, so dispose of the card here — one generic site, no
    // resume handler aware of treacheries.
    if matches!(outcome, EngineOutcome::Done) {
        return encounter::teardown_encounter_card_if_top(cx);
    }
    outcome
```

(Keep the existing doc-comment on `resolve_input`; the `match` is no longer the final expression — bind it to `outcome`.)

- [ ] **Step 5: Remove the skill-test driver flush.** In `skill_test.rs` `finish_skill_test`'s `PostOnResolution` arm, delete the block:

```rust
                if let Some(code) = cx.state.pending_revelation_discard.take() {
                    cx.state.encounter_discard.push(code);
                }
```

and its preceding comment (the driver no longer knows treacheries exist).

- [ ] **Step 6: Delete the obsolete slot unit test.** In `skill_test.rs`, delete `revelation_skill_test_failure_deals_margin_damage_and_discards` (it simulated the removed slot + a direct `finish_skill_test` call that bypasses the new chokepoint; the real suspended-Revelation discard is integration-tested by `revelation_treacheries.rs::grasping_hands_*`). Leave `plain_skill_test_leaves_pending_revelation_discard_untouched` for Task 3.

- [ ] **Step 7: Run the new test + the regression guard.**

```sh
cargo test -p scenarios --test revelation_choice
cargo test -p cards --test revelation_treacheries
cargo test -p game-core
```

Expected: all **PASS** (the new choice case now discards; Grasping Hands / Crypt Chill / Ancient Evils unchanged).

- [ ] **Step 8: Commit.**

```sh
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/encounter.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/dispatch/skill_test.rs
git commit -m "engine: dispose of encounter treacheries via an EncounterCard frame, not pending_revelation_discard (#380)"
```

---

## Task 3: Remove the vestigial `pending_revelation_discard` field

The field is now always `None` (no set-site, no flush). Remove it and reframe the last unit test that reads it.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (field + doc)
- Modify: `crates/game-core/src/state/builder.rs` (init)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (reframe the plain-test unit test)

- [ ] **Step 1: Reframe the plain-test unit test.** In `skill_test.rs`, change `plain_skill_test_leaves_pending_revelation_discard_untouched` to drop the two `pending_revelation_discard` lines, keeping the `encounter_discard.is_empty()` assertion (the surviving intent: the skill-test driver never disposes of encounter cards). Rename it `plain_skill_test_disposes_of_no_encounter_card`:

```rust
    /// A plain (non-revelation) skill test disposes of no encounter card — the
    /// skill-test driver no longer touches encounter disposal at all (#380).
    #[test]
    fn plain_skill_test_disposes_of_no_encounter_card() {
        use crate::state::ChaosToken;
        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let mut cx = Cx { state: &mut state, events: &mut events };
        let out = perform_skill_test(&mut cx, inv, SkillKind::Intellect, 1);
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        let out = finish_skill_test(&mut cx, &[]);
        assert_eq!(out, EngineOutcome::Done);
        assert!(state.encounter_discard.is_empty());
    }
```

- [ ] **Step 2: Remove the field + builder init.** In `game_state.rs` delete the `pub pending_revelation_discard: Option<CardCode>,` field and its doc-comment (replace with a one-line `// removed in #380 …` breadcrumb if a neighbor's doc references it — grep first). In `builder.rs` delete `pending_revelation_discard: None,` from `build()`.

- [ ] **Step 3: Build to confirm no stragglers.**

```sh
cargo build -p game-core 2>&1 | rg "pending_revelation_discard" ; echo "(no output above = clean)"
grep -rn "pending_revelation_discard" crates/   # expect: nothing
```

- [ ] **Step 4: Full CI gauntlet.**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
wasm-pack test --headless --firefox crates/web
```

Expected: all green.

- [ ] **Step 5: Commit.**

```sh
git add -A
git commit -m "engine: remove the now-vestigial pending_revelation_discard field (#380)"
```

---

## Self-Review

**Spec coverage (§E):** `EncounterCard { card }` frame ✓ (Task 2.1); pushed in the treachery branch only ✓ (Task 2.3, enemies unaffected); teardown helper called inline + at the resolve_input chokepoint ✓ (Task 2.3/2.4); `pending_revelation_discard` slot + set-site + skill-test flush removed ✓ (Task 2.3/2.5, Task 3); one-shot→discard / persistent→skip ✓ (Task 2.2); revelation-suspends-into-a-choice test added ✓ (Task 1); `revelation_treacheries.rs` + Crypt Chill hold unchanged ✓ (Task 2.7 regression guard). `pending_played_event` explicitly out of scope (untouched).

**Placeholder scan:** the two `NOTE:` callouts in Task 1 flag fixture-shape facts to confirm against the real `synth_treachery_metadata` / `with_encounter_deck` at execution (not placeholders for behavior — the behavior code is concrete).

**Type consistency:** `teardown_encounter_card_if_top(&mut Cx) -> EngineOutcome` used identically in encounter.rs (definition + inline call) and mod.rs (chokepoint). `Continuation::EncounterCard { card: CardCode }` matched the same way in the helper, the classifier, and the resolve_input arm.

**Risk flags:** (1) The chokepoint fires `teardown_encounter_card_if_top` after *every* `Done` resume — verified no-op unless an `EncounterCard` is top, but run the full `game-core` + `scenarios` + `cards` suites to confirm no resume path leaves a stray `EncounterCard` top (Task 2.7 + Task 3.4). (2) `resolve_encounter_card` is reached from the EngineRecord path (`encounter_card_revealed`) and the Mythos chain; both are within an `apply` transactional snapshot, so a mid-Revelation `Rejected` rolls back the pushed frame (no manual pop needed) — confirmed against `apply_with_scenario_registry`'s pristine-restore. (3) Persistence re-derivation at teardown uses the registry by code; a persistent treachery (none in current corpus carry a *choice* Revelation, but Obscuring Fog is persistent) must still be skipped — covered by `treachery_is_persistent` parity with the old inline check.

**Out of scope:** `pending_played_event` (sibling, separate follow-up), `enemy_attack_pending` and the other framework cursors, the keystone.
