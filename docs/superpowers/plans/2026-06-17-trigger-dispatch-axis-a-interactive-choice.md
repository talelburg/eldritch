# Trigger-dispatch Axis A — interactive choice: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un-stub interactive choice in the DSL evaluator — `Effect::ChooseOne`, `LocationTarget::ChosenByController`, `InvestigatorTarget::ChosenByController`, plus a native-leaf controller pick — via a `Continuation::Choice` frame on the Axis-B stack, single-pass suspend-and-replay, and a structured `PickSingle` input contract.

**Architecture:** A choice suspends by pushing a `Continuation::Choice` frame holding the picks-so-far (`decisions`), the offered option ids, and the root `Effect` being resolved (plus its `EvalContext` ingredients). `ResolveInput { PickSingle(id) }` validates membership, appends the pick, rebuilds the `EvalContext`, and **re-runs the effect from the top**, replaying `decisions` to reach the next un-ground choice. No mutation precedes any choice in scope, so re-run is safe; two loud guards (`apply_seq`, native-standalone) reject the deferred cases. Demonstrated by upgrading agenda 01105 (`ChooseOne` in a Forced run) and Crypt Chill 01167 (native instance pick).

**Tech Stack:** Rust, `game-core` kernel (no_std-ish, wasm-compatible), `cards` content crate, `serde`, the existing continuation stack from Axis B.

**Spec:** `docs/superpowers/specs/2026-06-17-trigger-dispatch-axis-a-interactive-choice-design.md`

**CI gauntlet (run before every commit that touches code):**
```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

---

## File map

- `crates/game-core/src/engine/outcome.rs` — `InputRequest` gains `options: Vec<ChoiceOption>`; new `ChoiceOption` + `OptionId` types. (Task 1)
- `crates/game-core/src/action.rs` — `InputResponse::PickSingle(OptionId)`. (Task 1)
- `crates/game-core/src/state/game_state.rs` — `Continuation::Choice(ChoiceFrame)` + `ChoiceFrame`. (Task 2)
- `crates/game-core/src/engine/dispatch/choice.rs` — **new**: `resume_choice`, the `0/1/2+` resolver helper, suspend helper. (Task 2–5)
- `crates/game-core/src/engine/dispatch/mod.rs` — route `Continuation::Choice` in `resolve_input`; declare `mod choice`. (Task 2)
- `crates/game-core/src/engine/evaluator.rs` — `Effect::ChooseOne`, `*::ChosenByController` resolution; `decisions` cursor; `apply_seq` guard. (Tasks 3–5)
- `crates/cards/src/impls/agenda_01105.rs` — upgrade to real `ChooseOne`. (Task 3)
- `crates/cards/src/impls/treachery_01167.rs` — upgrade native to suspend for a pick. (Task 5)
- `crates/scenarios/src/test_fixtures/synthetic.rs` — synthetic choice test cards. (Tasks 3–4)
- `crates/scenarios/tests/` — integration tests with real registry. (Tasks 3–5)
- `docs/phases/phase-7-the-gathering.md` — Decisions + Arc row. (Task 6)

---

## Task 1: Input contract — `OptionId`, `ChoiceOption`, structured `InputRequest`, `PickSingle`

**Files:**
- Modify: `crates/game-core/src/engine/outcome.rs`
- Modify: `crates/game-core/src/action.rs:325-366` (`InputResponse`)
- Modify (mechanical): the 11 `InputRequest { … }` construction sites (see Step 5)

- [ ] **Step 1: Write the failing test** — append to `crates/game-core/src/engine/outcome.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn choice_input_request_round_trips() {
    let req = InputRequest::choice(
        "Choose one",
        vec![
            ChoiceOption { id: OptionId(0), label: "Take 2 horror".into() },
            ChoiceOption { id: OptionId(1), label: "Each discards 1".into() },
        ],
    );
    let json = serde_json::to_string(&req).expect("serialize");
    let back: InputRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, req);
    assert_eq!(back.options.len(), 2);
    assert_eq!(back.options[1].id, OptionId(1));
}

#[test]
fn prompt_only_request_has_no_options() {
    let req = InputRequest::prompt("Submit PickIndex");
    assert!(req.options.is_empty());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core choice_input_request_round_trips`
Expected: FAIL — `ChoiceOption`, `OptionId`, `InputRequest::choice`, `InputRequest::prompt` not found.

- [ ] **Step 3: Implement** — in `crates/game-core/src/engine/outcome.rs`, replace the `InputRequest` struct (lines ~44-53) and add the new types:

```rust
/// Stable id for one offered option, scoped to a single `AwaitingInput`
/// prompt: the index into the request's `options` (and the frame's
/// offered set). A `u32` newtype for a host-pointer-width-independent
/// wire format; resume validates membership rather than trusting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OptionId(pub u32);

/// One selectable option in a structured choice prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    /// The id the host echoes back via [`InputResponse::PickSingle`].
    pub id: OptionId,
    /// Human-readable label for the host to render.
    pub label: String,
}

/// A prompt the engine emits when it needs player input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InputRequest {
    /// Human-readable text describing what the player must choose.
    pub prompt: String,
    /// Structured options for a single-selection choice. Empty for the
    /// legacy free-form prompts (reaction window `PickIndex`, commit
    /// windows) that have not migrated to the structured contract.
    pub options: Vec<ChoiceOption>,
}

impl InputRequest {
    /// A legacy prompt-only request (no structured options).
    #[must_use]
    pub fn prompt(text: impl Into<String>) -> Self {
        Self { prompt: text.into(), options: Vec::new() }
    }

    /// A structured single-selection choice request.
    #[must_use]
    pub fn choice(text: impl Into<String>, options: Vec<ChoiceOption>) -> Self {
        Self { prompt: text.into(), options }
    }
}
```

- [ ] **Step 4: Add `PickSingle` to `InputResponse`** — in `crates/game-core/src/action.rs`, inside `enum InputResponse` (after `PickIndex`, ~line 333):

```rust
    /// Pick one option from a structured choice prompt
    /// ([`InputRequest::choice`](crate::engine::outcome::InputRequest::choice)),
    /// echoing back its [`OptionId`](crate::engine::outcome::OptionId). The new
    /// single-selection family (Axis A); the legacy `PickIndex` /
    /// `PickLocation` / `PickInvestigator` stay on their existing paths.
    PickSingle(crate::engine::outcome::OptionId),
```

Add a serde round-trip test in the `input_response_tests` module:

```rust
    #[test]
    fn pick_single_input_serde_roundtrip() {
        use crate::engine::outcome::OptionId;
        let original = InputResponse::PickSingle(OptionId(2));
        let json = serde_json::to_string(&original).expect("serialize");
        let back: InputResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
```

- [ ] **Step 5: Migrate the 11 `InputRequest { … }` literal sites to `InputRequest::prompt(...)`** — each currently builds `InputRequest { prompt: <expr> }`. Rewrite each as `InputRequest::prompt(<expr>)`. Sites:
  - `crates/game-core/src/engine/evaluator.rs:745`
  - `crates/game-core/src/engine/dispatch/encounter.rs:471`
  - `crates/game-core/src/engine/dispatch/phases.rs:703, 847`
  - `crates/game-core/src/engine/dispatch/hunters.rs:350`
  - `crates/game-core/src/engine/dispatch/reaction_windows.rs:290, 500`
  - plus any in `skill_test.rs` / others surfaced by the compiler.

  Find them all with: `grep -rn "InputRequest {" crates/game-core/src | grep -v "pub struct"`. For a multi-line `InputRequest { prompt: format!(…) }`, rewrite to `InputRequest::prompt(format!(…))`.

- [ ] **Step 6: Run the gauntlet**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features 2>&1 | tail -20`
Expected: PASS (compiler-guided fixes for any missed literal site).

Run: `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/outcome.rs crates/game-core/src/action.rs crates/game-core/src/engine/dispatch crates/game-core/src/engine/evaluator.rs
git commit -m "engine: structured InputRequest + PickSingle/OptionId (Axis-A input contract, #334)"
```

---

## Task 2: `Continuation::Choice` frame + `resume_choice` router skeleton + the `0/1/2+` resolver

This task adds the frame and the router wiring with a **trivial test consumer** (a bare `ChooseOne` over a single auto-resolving branch and a 2-branch suspend) exercised at the evaluator unit level. Full DSL `ChooseOne` evaluation is Task 3; here we land the frame, the suspend helper, the router, and the resolver helper.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`Continuation` enum, ~line 404)
- Create: `crates/game-core/src/engine/dispatch/choice.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (declare `mod choice`; route in `resolve_input`)

- [ ] **Step 1: Add the `Choice` variant + `ChoiceFrame`** — in `crates/game-core/src/state/game_state.rs`, extend the `Continuation` enum and add the struct after it:

```rust
pub enum Continuation {
    Resolution(ResolutionFrame),
    SkillTest,
    /// A controller choice is mid-resolution (Axis A): the effect tree is
    /// re-run from the top on each resume, replaying `decisions` to reach
    /// the next un-ground choice. See [`ChoiceFrame`].
    Choice(ChoiceFrame),
}
```

```rust
/// A controller choice paused mid-resolution (umbrella §3, Axis A).
///
/// The frame stores the picks made so far (`decisions`), the option ids
/// offered at the *current* suspend (`offered`, so resume validates
/// membership), the root `Effect` being resolved, and the `EvalContext`
/// ingredients to rebuild on resume (`controller` + `source`) — mirroring
/// how [`InFlightSkillTest`] stores `investigator` + `source` rather than a
/// non-serializable `EvalContext` (see the Axis-A spec §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChoiceFrame {
    /// Picks recorded so far, in choice-encounter (pre-order) order.
    pub decisions: Vec<crate::engine::outcome::OptionId>,
    /// Option ids offered at the current suspend; resume rejects an id
    /// not in this set.
    pub offered: Vec<crate::engine::outcome::OptionId>,
    /// Root effect being (re-)resolved. A native leaf is just one node.
    pub effect: card_dsl::dsl::Effect,
    /// `EvalContext.controller` ingredient.
    pub controller: InvestigatorId,
    /// `EvalContext.source` ingredient (`None` for scenario/forced effects
    /// with no originating instance).
    pub source: Option<CardInstanceId>,
}
```

Add `Choice` arms to the `as_resolution` / `as_resolution_mut` matches (return `None`).

- [ ] **Step 2: Failing test for the resolver helper** — create `crates/game-core/src/engine/dispatch/choice.rs` with a test module first:

```rust
//! Interactive-choice resolution (Axis A, #334): the `Continuation::Choice`
//! frame's suspend/resume and the uniform `0 ⇒ reject · 1 ⇒ auto · 2+ ⇒
//! suspend` resolver.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_zero_options_is_reject() {
        assert!(matches!(
            resolve_choice_count(0),
            ChoiceResolution::Empty
        ));
    }

    #[test]
    fn resolve_one_option_auto_binds_index_zero() {
        assert!(matches!(
            resolve_choice_count(1),
            ChoiceResolution::Auto(0)
        ));
    }

    #[test]
    fn resolve_two_options_suspends() {
        assert!(matches!(
            resolve_choice_count(2),
            ChoiceResolution::Suspend
        ));
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p game-core resolve_one_option_auto_binds`
Expected: FAIL — `resolve_choice_count` / `ChoiceResolution` not found.

- [ ] **Step 4: Implement the resolver helper** — at the top of `choice.rs`:

```rust
use crate::engine::evaluator::EvalContext;
use crate::engine::outcome::{ChoiceOption, EngineOutcome, InputRequest, OptionId};
use crate::engine::Cx;
use crate::state::{ChoiceFrame, Continuation};

/// Outcome of applying the uniform resolve convention to a count of legal
/// options (umbrella §3.4 / spec §5).
pub(crate) enum ChoiceResolution {
    /// Zero legal options — caller applies its printed fallback or rejects.
    Empty,
    /// Exactly one — auto-bind this index, no input.
    Auto(usize),
    /// Two or more — suspend with a `Continuation::Choice` frame.
    Suspend,
}

/// Map a legal-option count to the resolve convention.
pub(crate) fn resolve_choice_count(n: usize) -> ChoiceResolution {
    match n {
        0 => ChoiceResolution::Empty,
        1 => ChoiceResolution::Auto(0),
        _ => ChoiceResolution::Suspend,
    }
}
```

- [ ] **Step 5: Add the suspend helper + the resume router** — append to `choice.rs`:

```rust
/// Push a `Continuation::Choice` frame and return the matching
/// `AwaitingInput`. `labels` provides one render label per offered option,
/// in offered order; `OptionId(i)` is the index.
pub(crate) fn suspend_for_choice(
    cx: &mut Cx,
    prompt: impl Into<String>,
    labels: Vec<String>,
    decisions: Vec<OptionId>,
    effect: card_dsl::dsl::Effect,
    eval_ctx: EvalContext,
) -> EngineOutcome {
    let offered: Vec<OptionId> = (0..labels.len() as u32).map(OptionId).collect();
    let options: Vec<ChoiceOption> = offered
        .iter()
        .copied()
        .zip(labels)
        .map(|(id, label)| ChoiceOption { id, label })
        .collect();
    cx.state.continuations.push(Continuation::Choice(ChoiceFrame {
        decisions,
        offered: offered.clone(),
        effect,
        controller: eval_ctx.controller,
        source: eval_ctx.source,
    }));
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, options),
        resume_token: cx.next_resume_token(),
    }
}

/// Resume a `Continuation::Choice`: validate the pick is in the offered
/// set, append it to `decisions`, pop the frame, and re-run the effect
/// from the top (the evaluator replays `decisions`).
pub(crate) fn resume_choice(
    cx: &mut Cx,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    let crate::action::InputResponse::PickSingle(picked) = response else {
        return EngineOutcome::Rejected {
            reason: "ResolveInput: a choice is open; expected InputResponse::PickSingle".into(),
        };
    };
    let Some(Continuation::Choice(frame)) = cx.state.continuations.last() else {
        return EngineOutcome::Rejected {
            reason: "resume_choice: no Choice frame on top of the stack".into(),
        };
    };
    if !frame.offered.contains(picked) {
        return EngineOutcome::Rejected {
            reason: format!("ResolveInput: PickSingle({picked:?}) not in the offered set").into(),
        };
    }
    // Pop the frame, carry forward the recorded decisions + the just-made pick.
    let Some(Continuation::Choice(frame)) = cx.state.continuations.pop() else {
        unreachable!("checked Choice on top immediately above");
    };
    let mut decisions = frame.decisions;
    decisions.push(*picked);
    let eval_ctx = match frame.source {
        Some(src) => EvalContext::for_controller_with_source(frame.controller, src),
        None => EvalContext::for_controller(frame.controller),
    };
    crate::engine::evaluator::apply_effect_with_decisions(cx, &frame.effect, eval_ctx, decisions)
}
```

> NOTE: `cx.next_resume_token()` — confirm the existing helper name the reaction-window/skill-test paths use to mint a `ResumeToken`; reuse it. `apply_effect_with_decisions` lands in Task 3.

- [ ] **Step 6: Declare the module + route it** — in `crates/game-core/src/engine/dispatch/mod.rs`: add `mod choice;` with the other `mod` declarations, and add the route in `resolve_input` immediately after the `Resolution` check (after line 432):

```rust
    if matches!(
        cx.state.continuations.last(),
        Some(crate::state::Continuation::Choice(_))
    ) {
        return choice::resume_choice(cx, response);
    }
```

- [ ] **Step 7: Run + commit**

Run: `cargo test -p game-core resolve_ choice` then the gauntlet.
Expected: resolver tests PASS; full build green (the `apply_effect_with_decisions` reference will not exist yet — gate this commit on Task 3, OR stub `apply_effect_with_decisions` to delegate to `apply_effect` ignoring `decisions` so this task compiles standalone). Use the stub:

```rust
// in evaluator.rs, temporary until Task 3 wires the cursor:
pub(crate) fn apply_effect_with_decisions(
    cx: &mut Cx, effect: &Effect, eval_ctx: EvalContext, _decisions: Vec<crate::engine::outcome::OptionId>,
) -> EngineOutcome {
    apply_effect(cx, effect, eval_ctx)
}
```

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine
git commit -m "engine: Continuation::Choice frame + resume router + resolve convention (Axis A, #334)"
```

---

## Task 3: `Effect::ChooseOne` via the evaluator + `decisions` replay + `apply_seq` guard; agenda 01105

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`apply_effect_with_decisions`, `Effect::ChooseOne` arm, `apply_seq` guard)
- Modify: `crates/cards/src/impls/agenda_01105.rs`
- Test: `crates/scenarios/tests/choice_choose_one.rs` (new)

- [ ] **Step 1: Failing unit test for branch suspend/auto** — add to `evaluator.rs` tests:

```rust
#[test]
fn choose_one_two_branches_suspends_then_runs_picked() {
    use crate::engine::outcome::OptionId;
    // A ChooseOne of two GainResources branches: 2+ ⇒ suspend.
    let effect = Effect::ChooseOne(vec![
        gain_resources(InvestigatorTarget::You, 1),
        gain_resources(InvestigatorTarget::You, 3),
    ]);
    let mut game = /* TestGame with one investigator, 0 resources */;
    let mut cx = game.cx();
    let ctx = EvalContext::for_controller(game.active_id());
    let out = apply_effect_with_decisions(&mut cx, &effect, ctx, vec![]);
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
    // Replaying decision = branch 1 runs the +3 branch.
    let out = apply_effect_with_decisions(&mut cx, &effect, ctx, vec![OptionId(1)]);
    assert!(matches!(out, EngineOutcome::Done));
    assert_eq!(cx.state.investigators[&game.active_id()].resources, 3);
}
```

> Fill the `TestGame` builder per the existing evaluator-test fixtures (`crates/game-core/src/engine/evaluator.rs` tests use `for_controller` + a `TestGame`; copy a nearby test's setup).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core choose_one_two_branches_suspends`
Expected: FAIL — `apply_effect_with_decisions` ignores `decisions`, `ChooseOne` still stubs to `awaiting_input_stub`.

- [ ] **Step 3: Implement the decisions cursor + `ChooseOne` arm.** Replace the temporary `apply_effect_with_decisions` stub with a real cursor-threaded walk. Add a `DecisionCursor` that yields the next recorded pick or signals "suspend here":

```rust
/// A replay cursor over a `Continuation::Choice` frame's recorded picks.
/// Choice nodes consume picks in pre-order; the first node with no recorded
/// pick triggers a suspend.
struct DecisionCursor {
    decisions: Vec<crate::engine::outcome::OptionId>,
    next: usize,
}

impl DecisionCursor {
    fn new(decisions: Vec<crate::engine::outcome::OptionId>) -> Self {
        Self { decisions, next: 0 }
    }
    /// The pick recorded for the choice now being evaluated, if any.
    fn take(&mut self) -> Option<crate::engine::outcome::OptionId> {
        let v = self.decisions.get(self.next).copied();
        if v.is_some() { self.next += 1; }
        v
    }
}
```

Thread an `&mut DecisionCursor` through a new internal `apply_effect_inner(cx, effect, ctx, cursor)`; `apply_effect` becomes `apply_effect_inner(cx, effect, ctx, &mut DecisionCursor::new(vec![]))`, and `apply_effect_with_decisions` becomes `apply_effect_inner(cx, effect, ctx, &mut DecisionCursor::new(decisions))`. In the `ChooseOne` arm:

```rust
Effect::ChooseOne(branches) => {
    match crate::engine::dispatch::choice::resolve_choice_count(branches.len()) {
        ChoiceResolution::Empty => EngineOutcome::Rejected {
            reason: "ChooseOne with no branches".into(),
        },
        ChoiceResolution::Auto(i) => apply_effect_inner(cx, &branches[i], eval_ctx, cursor),
        ChoiceResolution::Suspend => match cursor.take() {
            Some(OptionId(i)) => {
                apply_effect_inner(cx, &branches[i as usize], eval_ctx, cursor)
            }
            None => {
                let labels = branches.iter().map(branch_label).collect();
                crate::engine::dispatch::choice::suspend_for_choice(
                    cx, "Choose one", labels,
                    cursor.recorded_so_far(), effect.clone(), eval_ctx,
                )
            }
        },
    }
}
```

> Add `DecisionCursor::recorded_so_far(&self) -> Vec<OptionId>` returning `self.decisions[..self.next].to_vec()` (the picks already consumed before this suspend). Add a small `fn branch_label(e: &Effect) -> String` (e.g. `format!("{e:?}")` is acceptable for v0 labels; refine only if a card needs prettier text).

- [ ] **Step 4: Add the `apply_seq` guard** — in `apply_seq` (line 855), track whether any earlier effect ran, and reject if a later one suspends:

```rust
fn apply_seq(cx: &mut Cx, effects: &[Effect], eval_ctx: EvalContext, cursor: &mut DecisionCursor) -> EngineOutcome {
    for (i, effect) in effects.iter().enumerate() {
        let outcome = apply_effect_inner(cx, effect, eval_ctx, cursor);
        match outcome {
            EngineOutcome::Done => {}
            EngineOutcome::AwaitingInput { .. } if i > 0 => {
                return EngineOutcome::Rejected {
                    reason: "TODO(#NNN): a choice after an earlier Seq step is not yet \
                             supported (Axis-A single-pass replay; the two-pass split is deferred)".into(),
                };
            }
            other => return other,
        }
    }
    EngineOutcome::Done
}
```

> Replace `#NNN` with the issue filed in Step 7.

- [ ] **Step 5: Run unit test**

Run: `cargo test -p game-core choose_one_two_branches_suspends`
Expected: PASS.

- [ ] **Step 6: Upgrade agenda 01105** — rewrite `crates/cards/src/impls/agenda_01105.rs` `abilities()` to the real `ChooseOne`, with the random-discard branch as an `Effect::Native` leaf. Replace the deferred deal_horror with:

```rust
use card_dsl::dsl::{
    choose_one, deal_horror, for_each, forced_on_event, native, Ability, EventPattern,
    EventTiming, InvestigatorTarget, InvestigatorTargetSet,
};

const RANDOM_DISCARD_EACH: &str = "01105:random-discard-each";

pub fn abilities() -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::AgendaAdvanced,
        EventTiming::After,
        choose_one(vec![
            // Branch A: each investigator discards 1 random card from hand.
            for_each(InvestigatorTargetSet::AllInvestigators, native(RANDOM_DISCARD_EACH)),
            // Branch B: the lead takes 2 horror.
            deal_horror(InvestigatorTarget::You, 2),
        ]),
    )]
}
```

> Confirm the exact `for_each` builder signature + `InvestigatorTargetSet` variant name (`grep -n "InvestigatorTargetSet" crates/card-dsl/src/dsl.rs`). Wire `RANDOM_DISCARD_EACH` into the card's `native_effect_for` (mirror Crypt Chill's `native_effect_for` wiring + the crate registry hook). Implement the native:

```rust
fn random_discard_each(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    // `ForEach` binds the current investigator as the controller (confirm
    // against the evaluator's ForEach body context).
    let inv_id = ctx.controller;
    let Some(inv) = cx.state.investigators.get_mut(&inv_id) else {
        return EngineOutcome::Rejected { reason: "01105 random-discard: investigator gone".into() };
    };
    if inv.hand.is_empty() {
        return EngineOutcome::Done;
    }
    let idx = cx.state.rng.next_index(inv.hand.len());
    let inv = cx.state.investigators.get_mut(&inv_id).expect("present");
    let code = inv.hand.remove(idx);
    inv.discard.push(code.clone());
    cx.events.push(Event::CardDiscarded { investigator: inv_id, code, from: Zone::Hand });
    EngineOutcome::Done
}
```

> Confirm `rng.next_index` visibility from the `cards` crate — it's `pub(crate)` in `game-core`. If not reachable, the discard must route through a `pub(in crate::engine)` helper that the native calls; check how Crypt Chill's native mutates state and follow the same access path. If `next_index` is unreachable, add a thin `pub` engine helper `discard_random_from_hand(cx, inv_id)` in `game-core` and call it from the native (file it as the minimal surface).

- [ ] **Step 7: File the deferred-Seq follow-up issue** and backfill `#NNN` in Step 4:

```bash
gh issue create --title "[engine] Two-pass evaluator for a choice after a Seq mutation" \
  --label "engine,p2-later" \
  --body "Axis A (#334) ships single-pass suspend-and-replay; \`apply_seq\` rejects a choice that suspends after an earlier Seq step (re-run would double-apply the earlier mutation). When a card needs a choice mid-Seq, build the umbrella §4-A two-pass planning/execution split. No card needs it today."
```

- [ ] **Step 8: Integration test** — create `crates/scenarios/tests/choice_choose_one.rs`: install `cards::REGISTRY`, drive an agenda advance that fires 01105's forced `ChooseOne`, assert `AwaitingInput` with two `options`, then `ResolveInput(PickSingle(OptionId(1)))` → lead has 2 horror; in a second case `PickSingle(OptionId(0))` with a seeded hand → a card moved hand→discard. Model the test on `crates/scenarios/tests/cover_up_interrupt.rs` (registry install + drive-to-window pattern).

- [ ] **Step 9: Update 01105's unit test** — the existing `abilities_are_one_forced_on_advance_lead_two_horror` test asserts the deferred `DealHorror`; rewrite it to assert the `ChooseOne` shape (two branches: a `ForEach`-wrapped `Native` and `DealHorror`).

- [ ] **Step 10: Gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check
git add crates/game-core/src/engine/evaluator.rs crates/cards/src/impls/agenda_01105.rs crates/scenarios/tests/choice_choose_one.rs
git commit -m "engine: Effect::ChooseOne interactive resolution + Seq guard; agenda 01105 (Axis A, #334)"
```

---

## Task 4: `LocationTarget::ChosenByController` + `InvestigatorTarget::ChosenByController` + synthetic cards

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`resolve_location_target`, `resolve_investigator_target` — lines 898-920)
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs` (synthetic choice cards)
- Test: `crates/scenarios/tests/choice_targets.rs` (new)

- [ ] **Step 1: Failing unit test** for an investigator target that suspends on 2 candidates and auto-binds on 1. Add to `evaluator.rs` tests a test that evaluates `gain_resources(InvestigatorTarget::ChosenByController, 1)` (or a synthetic effect) with two investigators present → `AwaitingInput`; with one → `Done` and that one gained.

> The target resolvers currently return `Result<Id, &str>`. To suspend, the target arm must reach the cursor + suspend helper. Plumb the cursor into target resolution: add `apply_effect_inner` to resolve a target-bearing effect by first grounding its target via the cursor (the same `resolve_choice_count` → `Auto`/`Suspend` convention over the legal-id list), then running the effect with the ground id. Decide the enumeration source per target type:
> - `InvestigatorTarget::ChosenByController` → investigators at the controller's location (the cards in scope all say "an investigator at your location"); for the synthetic, use all investigators. Confirm the helper for "investigators at location".
> - `LocationTarget::ChosenByController` → for Dynamite Blast it's "your location or a connecting location"; for the synthetic and the general stub, offer all locations. The *restricted* set (your + connecting) is a card-level concern deferred to Dynamite Blast (Axis E); Axis A grounds the general "any location" form. Note this in the arm with a `TODO` referencing Dynamite Blast.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core chosen_by_controller`
Expected: FAIL — resolvers still `Err("requires AwaitingInput plumbing")`.

- [ ] **Step 3: Implement** — replace the two `ChosenByController` arms. Because grounding a target can suspend, target resolution moves into the cursor-threaded `apply_effect_inner` rather than the pure `resolve_*_target` helpers. Concretely, for an effect carrying a `ChosenByController` target, enumerate legal ids → `resolve_choice_count`:
  - `Empty` → reject (no legal target);
  - `Auto(0)` → bind the single id, run the effect;
  - `Suspend` → `cursor.take()`: `Some(OptionId(i))` binds the i-th enumerated id; `None` → `suspend_for_choice` with one label per candidate (e.g. the investigator/location id rendered).

Keep the pure `resolve_*_target` helpers for the non-choice variants (`You`, `Active`, `YourLocation`, `TestedLocation`); the `ChosenByController` variant is handled in the cursor-threaded path before those are consulted.

- [ ] **Step 4: Add synthetic test cards** — in `crates/scenarios/src/test_fixtures/synthetic.rs`, add:
  - a card whose ability is `gain_resources(InvestigatorTarget::ChosenByController, 1)`;
  - a card whose ability targets `LocationTarget::ChosenByController`;
  - a **2-choice** card whose ability is `choose_one(vec![ gain_resources(InvestigatorTarget::ChosenByController, 1), gain_resources(InvestigatorTarget::ChosenByController, 2) ])` (branch + target = two sequential suspends), to exercise multi-`decisions` replay.

  Wire them into the synthetic `TEST_REGISTRY` the way `synthetic.rs` registers its existing cards.

- [ ] **Step 5: Integration test** `crates/scenarios/tests/choice_targets.rs` — install the synthetic registry, two investigators; activate the 2-choice card; assert: first `AwaitingInput` (branch), `PickSingle` → second `AwaitingInput` (investigator), `PickSingle` → `Done` with the right investigator's resources changed by the right branch amount. This is the multi-`decisions` replay end-to-end.

- [ ] **Step 6: Gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check
git add crates/game-core/src/engine/evaluator.rs crates/scenarios/src/test_fixtures/synthetic.rs crates/scenarios/tests/choice_targets.rs
git commit -m "engine: *::ChosenByController target choice + multi-decision replay (Axis A, #334)"
```

---

## Task 5: Native instance-pick path + Crypt Chill 01167

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (`EvalContext.chosen_instance`; native-standalone guard)
- Modify: `crates/cards/src/impls/treachery_01167.rs`
- Test: extend `crates/cards/tests/` or a scenarios integration test

- [ ] **Step 1: Add `chosen_option` to `EvalContext`** — in `evaluator.rs`, add the field (after `attacking_enemy`, line 101) and `None` it in both constructors (lines ~114, ~132). This is the *general* native-pick primitive: the native re-enumerates its candidates and indexes by the picked `OptionId` (so it works for any native pick, not just card instances), mirroring how C5a threads `clue_discovery_count`:

```rust
    /// The option a controller picked, bound only while re-invoking a native
    /// leaf that suspended for a choice (Crypt Chill 01167). The native
    /// re-enumerates its candidates and indexes by this id. `None` outside
    /// that window. Mirrors `clue_discovery_count`.
    pub chosen_option: Option<crate::engine::outcome::OptionId>,
```

- [ ] **Step 2: Failing card test** — in `treachery_01167.rs` tests, assert that with 2+ controlled assets the fail-branch native suspends (`AwaitingInput`), and resuming with a `PickSingle` discards the chosen asset; with 1 asset it auto-discards; with 0 it deals 2 damage. Use the `crates/cards/tests/` integration harness (real registry) — model on the existing 01167 test path. (The unit test in the impl can assert the `abilities()` shape stays a willpower(4) test with a native `on_fail`.)

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p cards crypt_chill`
Expected: FAIL — native still discards the first asset deterministically.

- [ ] **Step 4: Rewrite `crypt_chill_fail`** to use the choice path:

```rust
fn crypt_chill_fail(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let controller = ctx.controller;
    let Some(inv) = cx.state.investigators.get(&controller) else {
        return EngineOutcome::Rejected { reason: "01167: controller not in state".into() };
    };
    // Enumerate controlled asset instances (in play order).
    let assets: Vec<CardInstanceId> = inv.cards_in_play.iter()
        .filter(|c| matches!(crate::by_code(&c.code.0).map(|m| &m.kind), Some(CardKind::Asset { .. })))
        .map(|c| c.instance_id)   // confirm the instance-id accessor on CardInPlay
        .collect();

    // Resume path: a pick was threaded in (re-enumerate, index by OptionId).
    if let Some(picked) = ctx.chosen_option {
        let Some(chosen) = assets.get(picked.0 as usize).copied() else {
            return EngineOutcome::Rejected { reason: "01167: chosen_option out of range".into() };
        };
        return discard_asset_instance(cx, controller, chosen);
    }

    match game_core::engine::dispatch::choice::resolve_choice_count(assets.len()) {
        // 0 ⇒ printed fallback: take 2 damage.
        ChoiceResolution::Empty => { take_damage(cx, controller, 2); EngineOutcome::Done }
        // 1 ⇒ auto-discard.
        ChoiceResolution::Auto(0) => discard_asset_instance(cx, controller, assets[0]),
        // 2+ ⇒ suspend; on resume the native re-runs with chosen_instance set.
        ChoiceResolution::Suspend => {
            let labels = assets.iter().map(|id| format!("{id:?}")).collect();
            // Native-standalone guard: a native may only suspend as the whole
            // effect (no prior DSL decisions). Enforced by passing empty
            // decisions; the resume threads chosen_instance, not a cursor pick.
            game_core::engine::dispatch::choice::suspend_for_native_choice(
                cx, "Choose an asset to discard", labels, assets, CRYPT_CHILL_FAIL, ctx,
            )
        }
        ChoiceResolution::Auto(_) => unreachable!("Auto only returns index 0"),
    }
}
```

- [ ] **Step 5: Add `suspend_for_native_choice` + the native resume branch** in `choice.rs`. The native case differs from the DSL case: the frame's `effect` is the `Effect::Native { tag }`, and on resume we rebuild the ctx, set `chosen_instance` to the picked instance (mapped from `OptionId` → the instance id list the frame stored), and re-invoke the effect. Store the instance-id list on the frame (extend `ChoiceFrame` with `native_instances: Vec<CardInstanceId>`, `None`/empty for DSL choices) OR encode the mapping by re-enumerating in the native (preferred — the native re-enumerates the same assets in the same order, so `OptionId(i)` → `assets[i]`; no extra frame field). Use the re-enumerate approach:

```rust
pub(crate) fn suspend_for_native_choice(
    cx: &mut Cx, prompt: impl Into<String>, labels: Vec<String>,
    _instances: Vec<CardInstanceId>, tag: &str, ctx: &EvalContext,
) -> EngineOutcome {
    // Native-standalone guard: no DSL decisions may precede a native pick.
    suspend_for_choice(cx, prompt, labels, vec![], card_dsl::dsl::native(tag), *ctx)
}
```

On resume, `resume_choice` re-runs `Effect::Native { tag }` via `apply_effect_with_decisions` with `decisions = [picked]`. The evaluator's `Effect::Native` arm must, when a decision is present, set `eval_ctx.chosen_instance` before invoking the native fn. Implement in the `Native` arm:

```rust
Effect::Native { tag } => {
    let mut ctx = eval_ctx;
    if let Some(OptionId(i)) = cursor.take() {
        // A native instance pick is being resumed: re-enumeration in the
        // native maps OptionId(i) → its i-th candidate. We expose the index
        // via chosen_instance only if the native asked; simplest is to let
        // the native re-enumerate and index. Thread the index through a
        // dedicated field to avoid coupling to a global order:
        ctx.chosen_instance = native_pick_to_instance(cx, tag, i);
    }
    // ... existing native dispatch via the registry ...
}
```

The native maps the picked id back to its candidate by re-enumerating in the same order: `let chosen = assets[ctx.chosen_option.expect("native resume sets chosen_option").0 as usize];`. The evaluator's `Effect::Native` arm sets `chosen_option` from the cursor before invoking the native fn:

```rust
Effect::Native { tag } => {
    let mut ctx = eval_ctx;
    if let Some(picked) = cursor.take() {
        ctx.chosen_option = Some(picked);
    }
    // ... existing native dispatch via the registry, passing `ctx` ...
}
```

- [ ] **Step 6: Add `discard_asset_instance` helper** in `treachery_01167.rs` (extract the discard from the old body, keyed by instance id):

```rust
fn discard_asset_instance(cx: &mut Cx, controller: InvestigatorId, instance: CardInstanceId) -> EngineOutcome {
    let inv = cx.state.investigators.get_mut(&controller).expect("controller present");
    let Some(pos) = inv.cards_in_play.iter().position(|c| c.instance_id == instance) else {
        return EngineOutcome::Rejected { reason: "01167: chosen asset no longer in play".into() };
    };
    let code = inv.cards_in_play.remove(pos).code;
    inv.discard.push(code.clone());
    cx.events.push(Event::CardDiscarded { investigator: controller, code, from: Zone::InPlay });
    EngineOutcome::Done
}
```

- [ ] **Step 7: Run the card test**

Run: `cargo test -p cards crypt_chill`
Expected: PASS (0 → 2 damage, 1 → auto-discard, 2+ → suspend then discard chosen).

- [ ] **Step 8: Gauntlet + commit**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check
git add crates/game-core/src/engine crates/cards/src/impls/treachery_01167.rs
git commit -m "engine: native-leaf controller pick (chosen_option) + Crypt Chill 01167 (Axis A, #334)"
```

---

## Task 6: Phase-7 doc update (final commit, after CI is green on the PR)

**Files:** Modify `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1:** In the "Future slices → trigger-dispatch rework" entry, flip the Axis-A row to `✅ PR #N` (Axis A — interactive choice, #334).
- [ ] **Step 2:** Add a **Decisions made** entry (apply the test: would a future PR-author choose differently without it?). Candidate, trimmed to load-bearing facts:

  > **Axis A ships single-pass suspend-and-replay, not the umbrella's two-pass split — no card has a choice inside a `Seq` (#334, PR #N).** A `Continuation::Choice` frame stores picks-so-far + the root effect + `EvalContext` ingredients; resume re-runs the effect from the top, replaying `decisions`. Two loud guards bound scope: `apply_seq` rejects a choice after an earlier step (→ two-pass, #NNN); a native pick may not follow DSL decisions. `PickSingle(OptionId)` + structured `InputRequest.options` is the new single-selection contract (legacy `PickIndex` reaction-window path unchanged). Native picks thread `EvalContext.chosen_option` (the native re-enumerates + indexes), mirroring C5a's `clue_discovery_count`. 01105's old "needs recorded randomness" note was wrong — the `(seed, draws)` model makes a replayed random discard deterministic.

- [ ] **Step 3:** Remove the Axis-A line from any "open" list; confirm #334 moves to the closed/done column with counts bumped per `docs/phases/README.md`.
- [ ] **Step 4: Commit** (this is the final commit on the branch, after CI is green):

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — Axis A interactive choice done (#334)"
```

---

## Self-review notes (carried into execution)

- **One name the executor must confirm against the codebase:** `cx.next_resume_token()` — the real token-minting helper the reaction-window / skill-test paths use (Task 2 Step 5). Reuse it rather than inventing one.
- **`rng.next_index` reachability from `cards`** (Task 3 Step 6): if `pub(crate)`-scoped, add a thin `pub` engine helper rather than widening visibility ad hoc.
- **Backfill `#NNN`** (deferred-Seq follow-up) in Task 3 Step 4 + Task 6 once filed in Task 3 Step 7.
- **Branch:** `engine/axis-a-interactive-choice` (already created; the design spec commit is its first commit). Open the PR after Task 5's gauntlet is green; Task 6 lands as the final commit per the phase-doc rule.
