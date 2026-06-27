# Surface single-option auto-binds as choices — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `interactive_acknowledge` is on (human play), a single legal option surfaces as a one-option `PickSingle` *before* it resolves — instead of auto-binding silently — covering both effect-level choices and no-choice forced effects (the Attic/Cellar examples in #466).

**Architecture:** Two flag-aware tweaks. (A) `resolve_choice_count` returns `Suspend` for n=1 when interactive, so every effect-level choice site (`ChooseOne`, `Effect::Fight` target, asset/location discard natives, deck search) surfaces its lone option. (B) A new `Continuation::AcknowledgeForced { source }` frame, pushed by `resolve_one` above a forced effect when interactive, surfaces a one-option pick before the forced effect resolves — mirroring the existing `AdvanceReverse`/`AwaitAck` pause and preserving the synchronous-return contract emit callers rely on.

**Tech Stack:** Rust workspace (`game-core` kernel, `cards` content). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-06-27-surface-single-option-choices-design.md`

## Global Constraints

- **CI gauntlet (all warnings-as-errors), run before pushing:** `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Flag-off determinism is sacros.** `interactive_acknowledge` defaults `false`. With it off, behavior must be byte-identical to today (single options auto-bind). The whole existing suite runs flag-off and must stay green.
- **Crate layering.** `game-core` is the kernel: no I/O, compiles to wasm, never depends on `cards`. Card data is reached only through `card_registry::current()`.
- **Validate-first / mutate-second.** Every handler checks all preconditions and returns `EngineOutcome::Rejected { reason }` with state+events unchanged before mutating.
- **Never paraphrase card text from memory.** The only cards touched (Attic 01113, Cellar 01114) are already implemented; do not change their effects. If you must cite text, read `crates/cards/src/impls/attic.rs` / `cellar.rs` or ArkhamDB.
- **No speculative DSL.** Add no new effect/DSL primitives; this is engine plumbing only.

---

### Task 1: Make `resolve_choice_count` flag-aware and thread the flag through all call sites

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/choice.rs:26-32` (signature + body), and its unit tests `:114-127`
- Modify: `crates/game-core/src/engine/evaluator.rs:544` (`ChooseOne`), `:688` (deck search), `:1595-1606` (`resolve_grounded_choice` signature + match), `:1641,1665,1689,1728` (its 4 callers)
- Modify: `crates/cards/src/impls/crypt_chill.rs:82`, `crates/cards/src/impls/dynamite_blast.rs:93`

**Interfaces:**
- Produces: `resolve_choice_count(n: usize, interactive: bool) -> ChoiceResolution` — `0 => Empty`, `2+ => Suspend`, `1 => Suspend` when `interactive` else `Auto(0)`. Callers pass `cx.state.interactive_acknowledge`.

- [ ] **Step 1: Update the resolver's unit tests to the new signature (failing)**

In `crates/game-core/src/engine/dispatch/choice.rs`, replace the three tests at `:114-127`:

```rust
    #[test]
    fn resolve_zero_options_is_reject() {
        assert!(matches!(resolve_choice_count(0, false), ChoiceResolution::Empty));
        assert!(matches!(resolve_choice_count(0, true), ChoiceResolution::Empty));
    }

    #[test]
    fn resolve_one_option_auto_binds_when_not_interactive() {
        assert!(matches!(resolve_choice_count(1, false), ChoiceResolution::Auto(0)));
    }

    #[test]
    fn resolve_one_option_suspends_when_interactive() {
        // #466: a lone option surfaces as a one-option pick in human play.
        assert!(matches!(resolve_choice_count(1, true), ChoiceResolution::Suspend));
    }

    #[test]
    fn resolve_two_options_suspends_regardless_of_flag() {
        assert!(matches!(resolve_choice_count(2, false), ChoiceResolution::Suspend));
        assert!(matches!(resolve_choice_count(2, true), ChoiceResolution::Suspend));
    }
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p game-core --lib dispatch::choice 2>&1 | tail -5`
Expected: compile error — `resolve_choice_count` takes 1 argument, not 2.

- [ ] **Step 3: Change `resolve_choice_count`'s signature and body**

In `crates/game-core/src/engine/dispatch/choice.rs`, replace `:25-32`:

```rust
/// Map a legal-option count to the resolve convention. When `interactive` is set
/// (human play, `interactive_acknowledge`), a single option surfaces as a
/// one-option pick (`Suspend`) instead of auto-binding silently (#466).
pub fn resolve_choice_count(n: usize, interactive: bool) -> ChoiceResolution {
    match n {
        0 => ChoiceResolution::Empty,
        1 if interactive => ChoiceResolution::Suspend,
        1 => ChoiceResolution::Auto(0),
        _ => ChoiceResolution::Suspend,
    }
}
```

- [ ] **Step 4: Thread the flag through the three evaluator call sites**

In `crates/game-core/src/engine/evaluator.rs`:

At `:544` change `match resolve_choice_count(branches.len()) {` to:
```rust
    match resolve_choice_count(branches.len(), cx.state.interactive_acknowledge) {
```

At `:688` change `let chosen_deck_index: Option<usize> = match resolve_choice_count(eligible.len()) {` to:
```rust
    let chosen_deck_index: Option<usize> = match resolve_choice_count(
        eligible.len(),
        cx.state.interactive_acknowledge,
    ) {
```

For `resolve_grounded_choice` at `:1595`, add an `interactive: bool` parameter (place it last, after `bind`) and use it at the match. Change the signature's closing to include it:
```rust
fn resolve_grounded_choice<Id: Copy>(
    eval_ctx: EvalContext,
    candidates: &[Id],
    empty_reason: &'static str,
    prompt: &'static str,
    label: impl Fn(&Id) -> String,
    bind: impl Fn(Id) -> EvalContext,
    interactive: bool,
) -> Result<EvalContext, EngineOutcome> {
```
and at `:1606` change `match resolve_choice_count(candidates.len()) {` to:
```rust
    match resolve_choice_count(candidates.len(), interactive) {
```

- [ ] **Step 5: Pass the flag from `resolve_grounded_choice`'s four callers**

Each of `ground_investigator_choice` (`:1635`), `ground_location_choice` (`:1659`), `ground_enemy_choice` (`:1682`), and the co-located fight-target caller (`:1728`) takes `cx`. In each `resolve_grounded_choice(...)` call (at `:1641,1665,1689,1728`), add a final argument `cx.state.interactive_acknowledge,` (after the `bind` closure).

- [ ] **Step 6: Thread the flag through the two card natives**

In `crates/cards/src/impls/crypt_chill.rs:82` change `match resolve_choice_count(assets.len()) {` to:
```rust
    match resolve_choice_count(assets.len(), cx.state.interactive_acknowledge) {
```

In `crates/cards/src/impls/dynamite_blast.rs:93` change `match resolve_choice_count(locations.len()) {` to:
```rust
    match resolve_choice_count(locations.len(), cx.state.interactive_acknowledge) {
```

- [ ] **Step 7: Run the resolver unit tests + full suite**

Run: `cargo test -p game-core --lib dispatch::choice 2>&1 | tail -5`
Expected: PASS (4 tests).

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | grep -E "test result: FAILED|error\[" ; echo done`
Expected: no `FAILED`/`error[` lines (whole suite green — it runs flag-off, so behavior is unchanged).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/dispatch/choice.rs crates/game-core/src/engine/evaluator.rs crates/cards/src/impls/crypt_chill.rs crates/cards/src/impls/dynamite_blast.rs
git commit -m "engine: make resolve_choice_count flag-aware (single option surfaces when interactive) (#466)"
```

---

### Task 2: Verify Mechanism A surfaces a single effect-choice option under the flag

**Files:**
- Test: `crates/game-core/src/engine/dispatch/choice.rs` (add to `#[cfg(test)] mod tests`)
- Test: `crates/cards/src/impls/crypt_chill.rs` (add a card test in its `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: flag-aware `resolve_choice_count` from Task 1; `Effect::ChooseOne`; `interactive_acknowledge` (a `pub` field on `GameState`).

- [ ] **Step 1: Write a failing integration test — one-branch ChooseOne suspends under the flag**

Add to `crates/game-core/src/engine/dispatch/choice.rs` tests (the existing test module already imports `Effect`, `push_effect`, `EvalContext`, `GameStateBuilder`, `Cx`, `drive`):

```rust
    #[test]
    fn single_branch_choose_one_surfaces_under_interactive_flag() {
        use crate::state::InvestigatorId;
        use crate::test_support::GameStateBuilder;

        // One ChooseOne branch: today it auto-binds. With interactive_acknowledge
        // on it must surface as a one-option pick (#466).
        let effect = Effect::ChooseOne(vec![Effect::Seq(vec![])]);
        let ctx = EvalContext::for_controller(InvestigatorId(1));

        let mut state = GameStateBuilder::default().build();
        state.interactive_acknowledge = true;
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx { state: &mut state, events: &mut events };
            push_effect(&mut cx, &effect, ctx);
            crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done)
        };
        match out {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "lone branch surfaces as one option");
            }
            other => panic!("expected a one-option suspend, got {other:?}"),
        }
    }

    #[test]
    fn single_branch_choose_one_auto_binds_when_flag_off() {
        use crate::state::InvestigatorId;
        use crate::test_support::GameStateBuilder;

        let effect = Effect::ChooseOne(vec![Effect::Seq(vec![])]);
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let mut state = GameStateBuilder::default().build(); // flag defaults false
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx { state: &mut state, events: &mut events };
            push_effect(&mut cx, &effect, ctx);
            crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done)
        };
        assert!(matches!(out, EngineOutcome::Done), "flag off: auto-binds, no suspend");
    }
```

- [ ] **Step 2: Run to verify the flag-off test passes and the flag-on test passes**

Run: `cargo test -p game-core --lib single_branch_choose_one 2>&1 | tail -6`
Expected: both PASS (Task 1 already provides the behavior — this test pins it). If the flag-on test fails with `Done`, Task 1's `ChooseOne` site was not threaded — fix `evaluator.rs:544`.

- [ ] **Step 3: Write a Crypt Chill card test — one asset surfaces under the flag**

Read `crates/cards/src/impls/crypt_chill.rs` first to mirror its existing test setup (controller + one in-play asset + the fail-path entry). Add to its `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn single_asset_discard_surfaces_under_interactive_flag() {
        // With exactly one discardable asset and interactive_acknowledge on,
        // the discard must surface as a one-option pick rather than auto-discard.
        // (Mirror the existing fail-path test's setup; set
        // state.interactive_acknowledge = true before invoking the fail handler,
        // and assert the outcome is AwaitingInput with one option.)
    }
```
Replace the comment body with the concrete setup copied from the nearest existing Crypt Chill fail-path test (same registry install, one asset in `cards_in_play`, call `crypt_chill_fail` via its test entry), adding `state.interactive_acknowledge = true;` and asserting:
```rust
        match out {
            EngineOutcome::AwaitingInput { request, .. } => assert_eq!(request.options.len(), 1),
            other => panic!("expected one-option discard, got {other:?}"),
        }
```

- [ ] **Step 4: Run the Crypt Chill tests**

Run: `cargo test -p cards crypt_chill 2>&1 | tail -8`
Expected: PASS (existing + new).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/choice.rs crates/cards/src/impls/crypt_chill.rs
git commit -m "test: Mechanism A surfaces single effect-choice options under the flag (#466)"
```

---

### Task 3: Add the `AcknowledgeForced` frame + drive/resume handlers

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add the `Continuation::AcknowledgeForced` variant near `AdvanceReverse`, `:459`)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (add `drive_acknowledge_forced`, `resume_acknowledge_forced`, and a `forced_source_name` helper)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (drive arm ~`:255`, resume arm ~`:614`)
- Test: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`#[cfg(test)] mod`)

**Interfaces:**
- Produces: `Continuation::AcknowledgeForced { source: CardCode }`; `forced_triggers::drive_acknowledge_forced(cx) -> EngineOutcome` (suspends a one-option `PickSingle`); `forced_triggers::resume_acknowledge_forced(cx, &InputResponse) -> EngineOutcome` (validates `PickSingle(0)`, pops the frame, returns `Done`).

- [ ] **Step 1: Add the continuation variant**

In `crates/game-core/src/state/game_state.rs`, immediately after the `AdvanceReverse { ... }` variant (ends near `:468`), add:

```rust
    /// A no-choice forced ability is about to resolve and the game is in
    /// interactive mode (`interactive_acknowledge`): surface it as a one-option
    /// pick so the player "performs" it before it lands (#466). Pushed by
    /// `resolve_one` above the forced effect's root frame; the `drive` loop
    /// suspends here, and on resume pops, letting the effect frame beneath
    /// resolve. `source` is the card the forced ability is printed on (for the
    /// prompt's display name).
    AcknowledgeForced { source: CardCode },
```

- [ ] **Step 2: Write a failing unit test for the frame's drive + resume**

Add to `crates/game-core/src/engine/dispatch/forced_triggers.rs` `#[cfg(test)] mod` (create the module if absent; import `Continuation`, `CardCode`, `GameStateBuilder`, `Cx`, `EngineOutcome`, `InputResponse`, `OptionId`):

```rust
    #[test]
    fn acknowledge_forced_suspends_then_pops_on_pick() {
        use crate::action::InputResponse;
        use crate::engine::OptionId;
        use crate::state::{CardCode, Continuation};
        use crate::test_support::GameStateBuilder;

        let mut state = GameStateBuilder::default().build();
        state
            .continuations
            .push(Continuation::AcknowledgeForced { source: CardCode("01113".into()) });
        let mut events = Vec::new();
        let mut cx = Cx { state: &mut state, events: &mut events };

        // Drive: one-option suspend.
        let out = super::drive_acknowledge_forced(&mut cx);
        match out {
            EngineOutcome::AwaitingInput { request, .. } => assert_eq!(request.options.len(), 1),
            other => panic!("expected one-option suspend, got {other:?}"),
        }

        // Resume with the single option: frame pops, returns Done.
        let out = super::resume_acknowledge_forced(&mut cx, &InputResponse::PickSingle(OptionId(0)));
        assert!(matches!(out, EngineOutcome::Done));
        assert!(
            cx.state.continuations.is_empty(),
            "the AcknowledgeForced frame must be popped on resume"
        );
    }
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p game-core --lib acknowledge_forced_suspends 2>&1 | tail -6`
Expected: compile error — `drive_acknowledge_forced` / `resume_acknowledge_forced` not found.

- [ ] **Step 4: Implement the helpers in `forced_triggers.rs`**

Add (top-level in `forced_triggers.rs`; the file already imports `Cx`, `EngineOutcome`, `card_registry`, `CardCode`):

```rust
/// Display name for the card a forced ability is printed on, for the
/// `AcknowledgeForced` prompt. Resolved via the registry; falls back to the raw
/// code when no registry/metadata is available (tests).
fn forced_source_name(code: &CardCode) -> String {
    crate::card_registry::current()
        .and_then(|r| (r.metadata_for)(code))
        .map_or_else(|| code.0.clone(), |m| m.name.clone())
}

/// Drive a [`Continuation::AcknowledgeForced`] frame (#466): suspend with a
/// one-option `PickSingle` naming the source. The pick precedes the forced
/// effect's resolution ("confirm before the effect"). Mirrors
/// `advance_reverse::drive`'s `AwaitAck` suspend.
pub(crate) fn drive_acknowledge_forced(cx: &mut Cx) -> EngineOutcome {
    use crate::engine::{ChoiceOption, InputRequest, OptionId, ResumeToken};
    let Some(crate::state::Continuation::AcknowledgeForced { source }) =
        cx.state.continuations.last()
    else {
        return EngineOutcome::Rejected {
            reason: "drive_acknowledge_forced: top frame is not AcknowledgeForced".into(),
        };
    };
    let name = forced_source_name(source);
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            format!("Forced — {name}"),
            vec![ChoiceOption { id: OptionId(0), label: "Resolve".into() }],
        ),
        resume_token: ResumeToken(0),
    }
}

/// Resume an [`AcknowledgeForced`] frame: validate the single option, pop the
/// frame, and return `Done` so the `drive` loop resolves the effect beneath.
pub(crate) fn resume_acknowledge_forced(
    cx: &mut Cx,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    use crate::engine::OptionId;
    if !matches!(response, crate::action::InputResponse::PickSingle(OptionId(0))) {
        return EngineOutcome::Rejected {
            reason: "resume_acknowledge_forced: expected the single forced-resolution option".into(),
        };
    }
    debug_assert!(matches!(
        cx.state.continuations.last(),
        Some(crate::state::Continuation::AcknowledgeForced { .. })
    ));
    cx.state.continuations.pop();
    EngineOutcome::Done
}
```

- [ ] **Step 5: Wire the drive arm in `mod.rs`**

In `crates/game-core/src/engine/dispatch/mod.rs`, in the `drive` loop match (near the `AdvanceReverse` arm at `:255`), add:

```rust
            Some(Continuation::AcknowledgeForced { .. }) => {
                return forced_triggers::drive_acknowledge_forced(cx)
            }
```
(Match the surrounding arms' early-return shape — the `AwaitingInput` propagates out of the loop, exactly like other suspending arms.)

- [ ] **Step 6: Wire the resume arm in `mod.rs`**

In the resume dispatch match (near the `AdvanceReverse` resume arm at `:614`), add:

```rust
        Some(Continuation::AcknowledgeForced { .. }) => {
            forced_triggers::resume_acknowledge_forced(cx, response)
        }
```

- [ ] **Step 7: Run the unit test + full suite**

Run: `cargo test -p game-core --lib acknowledge_forced_suspends 2>&1 | tail -6`
Expected: PASS.

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | grep -E "test result: FAILED|error\[" ; echo done`
Expected: no failures (the frame is not pushed anywhere yet, so nothing else changes).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/forced_triggers.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: AcknowledgeForced continuation frame (one-option forced-effect pause) (#466)"
```

---

### Task 4: Push `AcknowledgeForced` from `resolve_one` when interactive

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs:456-477` (`resolve_one`)
- Test: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`#[cfg(test)] mod`)

**Interfaces:**
- Consumes: `Continuation::AcknowledgeForced` and the drive/resume from Task 3; `interactive_acknowledge`.

- [ ] **Step 1: Modify `resolve_one` to push the ack frame above the effect when interactive**

In `crates/game-core/src/engine/dispatch/forced_triggers.rs`, change the tail of `resolve_one` (currently `push_effect(cx, &effect, ctx); EngineOutcome::Done`) to:

```rust
    push_effect(cx, &effect, ctx);
    // #466: in interactive play, surface the forced effect as a one-option pick
    // *before* it resolves. Pushed above the effect frame so the `drive` loop
    // hits it first, suspends, and on resume pops it — then resolves the effect.
    // resolve_one still returns Done (push-frame contract), so emit callers that
    // do post-emit work stay correct.
    if cx.state.interactive_acknowledge {
        cx.state
            .continuations
            .push(crate::state::Continuation::AcknowledgeForced { source: hit.code.clone() });
    }
    EngineOutcome::Done
```

- [ ] **Step 2: Write a test — a single forced hit pushes the ack frame under the flag**

Add to `forced_triggers.rs` tests. Use the existing `fire_forced_on_enemy_defeat` / a registered forced source if a helper exists; otherwise drive `fire_forced_triggers` directly with a registered single-forced point. The minimal, dependency-light assertion drives `emit_event` for entering a location with a forced ability and checks the suspend. Prefer reusing the cards-crate integration path (Task 5) for the real-card assertion and keep this unit test focused on the frame-push:

```rust
    #[test]
    fn single_forced_pushes_acknowledge_when_interactive() {
        // With interactive_acknowledge on, resolving one forced hit must leave an
        // AcknowledgeForced frame on top (the one-option pause) above the effect.
        // Build a state with the flag on, a registered forced source, drive the
        // forced trigger, and assert the top frame is AcknowledgeForced.
        // (If forced_triggers has no in-crate registry helper, assert this in the
        // cards integration test in Task 5 instead and delete this unit test.)
    }
```
If `game-core` cannot register a forced source without the `cards` registry (likely — `collect_forced_hits` returns empty with no registry), **delete this unit test** and rely on Task 5's real-card integration coverage; note that in the commit message. Do not fake a registry in a `game-core` lib test.

- [ ] **Step 3: Run the full suite (flag-off regression)**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | grep -E "test result: FAILED|error\[" ; echo done`
Expected: no failures. The flag is off everywhere in the existing suite, so `resolve_one` pushes nothing extra and forced resolution is unchanged.

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/forced_triggers.rs
git commit -m "engine: surface single forced effects as a one-option pause when interactive (#466)"
```

---

### Task 5: Real-card integration test — Attic/Cellar acknowledge before harm

**Files:**
- Create: `crates/cards/tests/forced_acknowledge.rs`

**Interfaces:**
- Consumes: `Continuation::AcknowledgeForced`, the drive/resume from Tasks 3–4, the real `cards::REGISTRY`, the Attic (01113) and Cellar (01114) location forced abilities.

- [ ] **Step 1: Write the failing integration test**

First read `crates/cards/tests/act_advancement.rs` for the `#[ctor::ctor]` registry-install pattern and `crates/cards/src/impls/attic.rs` + `cellar.rs` to confirm the forced trigger (`EnteredLocation`, `After`) and the harm (1 horror / 1 damage). Then create `crates/cards/tests/forced_acknowledge.rs`:

```rust
//! #466: a no-choice forced location ability (the Attic's 1 horror, the Cellar's
//! 1 damage) surfaces a one-option acknowledge *before* the harm lands when
//! interactive_acknowledge is on, and resolves synchronously when it is off.

use game_core::engine::EngineOutcome;
use game_core::state::Continuation;

#[ctor::ctor]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

// Helper: build a one-investigator state standing on the given location code,
// with the flag set, then emit EnteredLocation and drive. Returns (outcome,
// final state). Mirror the move/emit path used by the engine's location tests;
// if a `test_support` helper to emit EnteredLocation exists, use it, otherwise
// construct the location in play, place the investigator on it, and call the
// engine's emit entry for EnteredLocation.
```

Implement two tests. For **flag on**: after emitting `EnteredLocation` for the Attic, drive once; assert the outcome is `AwaitingInput` with one option **and** the investigator's accumulated horror is still 0 (harm not yet applied); then resume with `PickSingle(OptionId(0))` and assert horror is now 1 and no `AcknowledgeForced` frame remains. For **flag off**: emit + drive resolves synchronously (no suspend) and horror is 1 immediately. Repeat the on/off pair for the Cellar (1 damage).

Use the actual harm-accumulator fields from `attic.rs`/`cellar.rs`'s tests (read them — likely `Investigator` sanity/horror and health/damage counters) for the assertions.

- [ ] **Step 2: Run to verify the flag-on test fails first if any wiring is missing**

Run: `cargo test -p cards --test forced_acknowledge 2>&1 | tail -15`
Expected: PASS once Tasks 3–4 are in. If the flag-on test sees harm applied with no suspend, `resolve_one`'s push (Task 4) is not reached on this path — verify the Attic's forced routes through `fire_forced_triggers` (single hit) and that `interactive_acknowledge` is set on the state.

- [ ] **Step 3: Run the gauntlet**

Run the full Global-Constraints gauntlet. Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add crates/cards/tests/forced_acknowledge.rs
git commit -m "test: Attic/Cellar forced effects acknowledge before harm when interactive (#466)"
```

---

### Task 6: File the deferred follow-ups (non-code)

- [ ] **Step 1: File the descriptive-effect-text follow-up issue**

```bash
gh issue create --title "Descriptive player-facing text for forced/auto-resolved effects" \
  --label engine,ui,p2-later \
  --body "Follow-up from #466. The #466 acknowledge names the *source* (\"Forced — The Attic\") but not the effect (\"…takes 1 horror\"). Build player-facing prose from an effect tree / emitted events so prompts and a future event feed can describe what happened. Overlaps #469 (player-facing copy, not protocol strings)."
```

- [ ] **Step 2: Note the framework-harm case on #429**

```bash
gh issue comment 429 --body "From #466: the draw-from-empty-deck horror penalty is pure-framework harm (no card forced ability), so #466's AcknowledgeForced (which hooks card forced abilities via resolve_one) does not reach it. When this interactive soak/harm work lands, it should likewise surface an acknowledge before the harm, for parity with #466."
```

- [ ] **Step 3: No commit** (issue/comment only).

---

## Self-review notes

- **Spec coverage:** Mechanism A → Tasks 1–2; Mechanism B (dedicated `AcknowledgeForced` frame) → Tasks 3–5; prompt copy (`"Forced — {name}"`, registry name, code fallback) → Task 3 Step 4; flag reuse → throughout; deferred follow-ups → Task 6. Web client needs no change (spec §Web client; `InputKind::PickSingle` already renders one button per option).
- **Flag-off determinism:** every task's regression step runs the full suite flag-off; Task 1 changes only the n=1-interactive branch; Task 4 guards the push behind the flag.
- **Type consistency:** `resolve_choice_count(n, interactive)` is used identically at all 7 call sites; `Continuation::AcknowledgeForced { source: CardCode }` matches `forced_source_name(&CardCode)` and `hit.code` (a `CardCode`); `resume_acknowledge_forced` expects `PickSingle(OptionId(0))`, the same option `drive_acknowledge_forced` emits.
