# Interactivity S5 — act advance + interactive soak + effect `ChooseOne` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-anchor the three remaining `OptionTarget::Global` framework prompts onto their board entities — Advance Act → the act card, interactive soak → the soak cards, effect `ChooseOne` → the chosen enemies/locations — so the board is their input surface.

**Architecture:** The engine already surfaces these options and resolves them by `OptionId` index; S5 stops discarding each option's *target* (umbrella approach A, display-only anchor). Three localized engine changes emit real `OptionTarget`s; the web side is one new glow-capable act card (mirroring `EnemyCard`) plus a banner filter — soak and `ChooseOne` glow for free through the existing `InPlayCardView` / `EnemyCard` / `location_map` matchers.

**Tech Stack:** Rust (workspace: `game-core` engine, `web` Leptos/WASM client), `wasm-bindgen-test` headless browser tests.

**Design spec:** `docs/superpowers/specs/2026-07-15-interactivity-s5-act-soak-effect-choices-design.md`

## Global Constraints

Copied verbatim from the design spec / `CLAUDE.md`. Every task's requirements implicitly include this section.

- **Anchors are display-only.** No resolve path may read `OptionTarget`; resolution keeps indexing `candidates[i]` / re-deriving targets by the echoed `OptionId`.
- **Labels stay byte-identical** to the pre-S5 output (soak = `DistributionTarget` debug repr; round-end reaction = `"Resolve reaction: {code}"`). Label polish is out of scope.
- **Do not touch `hunters::candidate_options`** — it is shared with hunter-move/engage prompts. Anchor at the soak call site only.
- **CI is 7 jobs, all warnings-as-errors.** Match locally before pushing:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `wasm-pack test --headless --firefox crates/web`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Validate-first / mutate-second** dispatch contract (unchanged here — no new mutation).
- Commit subjects: `scope: description`. Branch: `ui/interactivity-act-soak-choices` (already created).

## Coverage note (zero-code parts of the spec)

- **W2 (soak web) needs no web change.** Soak assets already render via `card.rs::InPlayCardView`, which matches `options_for(pending, OptionTarget::CardInstance(id))`. Task 1's anchor lights them up; the existing `crates/web/tests/in_play_card.rs::activatable_in_play_card_opens_a_menu_and_submits` already proves a `CardInstance`-anchored option glows the card and submits `PickSingle`. No new soak headless test.
- **W3 (`ChooseOne` web) needs no new component.** Enemy/location options glow via the existing `enemy_card.rs::EnemyCard` (`OptionTarget::Enemy`) and `map.rs::location_map` (`OptionTarget::Location`) matchers once Task 2 anchors them; those components' headless tests already cover glow+submit for their anchor.
- The banner's "N damage / M horror left" text is already part of the engine `prompt` string (`combat.rs::prompt_current_point`) — no banner data change for soak.

## File structure

- `crates/game-core/src/engine/dispatch/combat.rs` — Task 1 (soak option anchor).
- `crates/game-core/src/engine/dispatch/choice.rs` — Task 2 (target-aware `awaiting_choice_anchored`).
- `crates/game-core/src/engine/evaluator.rs` — Task 2 (thread anchor through `resolve_grounded_choice` + 4 callers).
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — Task 3 (round-end act anchor + thread current-act code).
- `crates/web/src/act_agenda.rs` + `crates/web/tests/act_agenda.rs` — Task 4 (act card glow).
- `crates/web/src/prompt_banner.rs` + `crates/web/tests/prompt_banner.rs` — Task 5 (banner filter).

Tasks 1–5 are mutually independent (disjoint files; each headless test builds its own outcome fixture). Any order; ordered here engine-first.

---

### Task 1: Soak options anchor to their card instances (E1)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs` (add `soak_options` near `prompt_current_point` ~L856; swap the builder call at ~L879; add a unit test to the module's `#[cfg(test)]`)

**Interfaces:**
- Consumes: `DistributionTarget` (`combat.rs` ~L765, `Investigator | Asset(CardInstanceId)`); `crate::engine::{ChoiceOption, OptionId, OptionTarget}`.
- Produces: `fn soak_options(targets: &[DistributionTarget]) -> Vec<crate::engine::ChoiceOption>` — option `i` is `targets[i]`, anchored `Asset → CardInstance`, `Investigator → Global`, label = the `DistributionTarget` debug repr.

- [ ] **Step 1: Write the failing test**

Add to `combat.rs`'s existing `#[cfg(test)]` module:

```rust
#[test]
fn soak_options_anchor_assets_to_card_instances() {
    use crate::engine::OptionTarget;
    use crate::state::CardInstanceId;
    let targets = vec![
        DistributionTarget::Investigator,
        DistributionTarget::Asset(CardInstanceId(7)),
    ];
    let opts = soak_options(&targets);
    // Anchors: the investigator has no card home; a soaker asset points at its card.
    assert_eq!(opts[0].target, OptionTarget::Global);
    assert_eq!(opts[1].target, OptionTarget::CardInstance(CardInstanceId(7)));
    // Labels unchanged from the former `hunters::candidate_options` debug repr.
    assert_eq!(opts[0].label, "Investigator");
    assert_eq!(opts[1].label, "Asset(CardInstanceId(7))");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core soak_options_anchor_assets_to_card_instances`
Expected: FAIL — `cannot find function 'soak_options'`.

- [ ] **Step 3: Write minimal implementation**

Add `soak_options` immediately above `fn prompt_current_point` (~L856) in `combat.rs`:

```rust
/// Build the per-point soak options, anchoring each to its board home so a host
/// renders it on the right card (S5, #540): a soaker asset to its card instance,
/// the investigator to `Global` (no card). Labels match the former
/// `hunters::candidate_options` debug repr, so the flat bar is byte-unchanged.
fn soak_options(targets: &[DistributionTarget]) -> Vec<crate::engine::ChoiceOption> {
    use crate::engine::{ChoiceOption, OptionId, OptionTarget};
    targets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let id = OptionId(u32::try_from(i).expect("soak target count fits u32"));
            let target = match t {
                DistributionTarget::Asset(instance) => OptionTarget::CardInstance(*instance),
                DistributionTarget::Investigator => OptionTarget::Global,
            };
            ChoiceOption::new(id, format!("{t:?}"), target)
        })
        .collect()
}
```

Then change the option builder in `prompt_current_point` (~L879) from:

```rust
        request: InputRequest::pick_single(prompt, super::hunters::candidate_options(&targets)),
```

to:

```rust
        request: InputRequest::pick_single(prompt, soak_options(&targets)),
```

- [ ] **Step 4: Run the test + the soak suite to verify pass**

Run: `cargo test -p game-core soak_options_anchor_assets_to_card_instances`
Expected: PASS.
Run: `cargo test -p game-core -- damage_assignment soak distribution`
Expected: PASS (resume still indexes by `OptionId`; labels/prompt unchanged).

- [ ] **Step 5: Clippy + commit**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean.

```bash
git add crates/game-core/src/engine/dispatch/combat.rs
git commit -m "engine: anchor interactive soak options to their card instances (S5)"
```

---

### Task 2: Effect `ChooseOne` anchors to Enemy/Location (E2)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/choice.rs` (add `awaiting_choice_anchored`; refactor `awaiting_choice` to delegate; add import; add 2 unit tests)
- Modify: `crates/game-core/src/engine/evaluator.rs` (add a `target` param to `resolve_grounded_choice`; supply it at the 4 `ground_*` callers; add 2 unit tests)

**Interfaces:**
- Produces: `choice::awaiting_choice_anchored(prompt: impl Into<String>, options: Vec<(String, OptionTarget)>) -> EngineOutcome` — `ChoiceOption { id: OptionId(i), label, target }` per option, in order.
- Changes: `resolve_grounded_choice<Id: Copy>(..., label, target: impl Fn(&Id) -> crate::engine::OptionTarget, bind, interactive)` — `target` inserted between `label` and `bind`.
- Callers supply: location → `OptionTarget::Location(*id)`, enemy & fight-target → `OptionTarget::Enemy(*id)`, investigator → `OptionTarget::Global`.

- [ ] **Step 1: Write the failing choice.rs tests**

Add to `choice.rs`'s `#[cfg(test)] mod tests`:

```rust
#[test]
fn awaiting_choice_anchored_carries_per_option_targets() {
    use crate::engine::OptionTarget;
    use crate::state::EnemyId;
    let out = awaiting_choice_anchored(
        "Choose an enemy",
        vec![
            ("Ghoul".into(), OptionTarget::Enemy(EnemyId(1))),
            ("Nobody".into(), OptionTarget::Global),
        ],
    );
    let EngineOutcome::AwaitingInput { request, .. } = out else {
        panic!("expected AwaitingInput");
    };
    assert_eq!(request.options[0].id, OptionId(0));
    assert_eq!(request.options[0].target, OptionTarget::Enemy(EnemyId(1)));
    assert_eq!(request.options[1].target, OptionTarget::Global);
}

#[test]
fn awaiting_choice_defaults_every_option_to_global() {
    use crate::engine::OptionTarget;
    let out = awaiting_choice("Pick", vec!["x".into(), "y".into()]);
    let EngineOutcome::AwaitingInput { request, .. } = out else {
        panic!("expected AwaitingInput");
    };
    assert!(request.options.iter().all(|o| o.target == OptionTarget::Global));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core awaiting_choice_anchored_carries_per_option_targets`
Expected: FAIL — `cannot find function 'awaiting_choice_anchored'`.

- [ ] **Step 3: Implement `awaiting_choice_anchored` + delegate `awaiting_choice`**

In `choice.rs`, change the import (top of file):

```rust
use crate::engine::{ChoiceOption, Cx, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken};
```

Replace the body of `awaiting_choice` and add the anchored variant above it:

```rust
/// Build the `AwaitingInput` for a controller choice from one `(label, anchor)`
/// per offered option, in offered order (`OptionId(i)` is the index). Like
/// [`awaiting_choice`] but each option carries its board [`OptionTarget`] so a
/// host renders it on the chosen entity (S5, #540). Pushes nothing — the
/// suspending node's own `Leaf` frame stays on the stack as the prompt (#422).
pub(crate) fn awaiting_choice_anchored(
    prompt: impl Into<String>,
    options: Vec<(String, OptionTarget)>,
) -> EngineOutcome {
    let options: Vec<ChoiceOption> = options
        .into_iter()
        .enumerate()
        .map(|(i, (label, target))| {
            ChoiceOption::new(
                OptionId(u32::try_from(i).expect("offered option count fits in u32")),
                label,
                target,
            )
        })
        .collect();
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, options),
        resume_token: ResumeToken(0),
    }
}

/// Build the `AwaitingInput` for a controller choice from one render label per
/// offered option, each anchored `Global` (no board home). Delegates to
/// [`awaiting_choice_anchored`]. Used by native-leaf choices and the effect-branch
/// `ChooseOne`, whose options are not board entities.
pub(crate) fn awaiting_choice(prompt: impl Into<String>, labels: Vec<String>) -> EngineOutcome {
    awaiting_choice_anchored(
        prompt,
        labels
            .into_iter()
            .map(|l| (l, OptionTarget::Global))
            .collect(),
    )
}
```

- [ ] **Step 4: Run the choice.rs tests to verify pass**

Run: `cargo test -p game-core -- awaiting_choice`
Expected: PASS (both new tests + existing `resolve_*` count tests unaffected).

- [ ] **Step 5: Write the failing evaluator.rs tests**

Add to `evaluator.rs`'s `#[cfg(test)] mod tests` (imports `EvalContext`, `resolve_grounded_choice` are in-module):

```rust
#[test]
fn grounded_choice_anchors_enemy_options() {
    use crate::engine::{EngineOutcome, OptionTarget};
    use crate::state::{EnemyId, InvestigatorId};
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    let cands = [EnemyId(4), EnemyId(9)];
    let out = resolve_grounded_choice(
        ctx,
        &cands,
        "empty",
        "Choose an enemy",
        |id| format!("{id:?}"),
        |id| OptionTarget::Enemy(*id),
        |_id| ctx,
        false, // 2 candidates → suspend regardless of the flag
    );
    match out {
        Err(EngineOutcome::AwaitingInput { request, .. }) => {
            assert_eq!(request.options[0].target, OptionTarget::Enemy(EnemyId(4)));
            assert_eq!(request.options[1].target, OptionTarget::Enemy(EnemyId(9)));
        }
        other => panic!("2 candidates suspend for a pick, got {other:?}"),
    }
}

#[test]
fn grounded_choice_investigator_stays_global() {
    use crate::engine::{EngineOutcome, OptionTarget};
    use crate::state::InvestigatorId;
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    let cands = [InvestigatorId(1), InvestigatorId(2)];
    let out = resolve_grounded_choice(
        ctx,
        &cands,
        "empty",
        "Choose an investigator",
        |id| format!("{id:?}"),
        |_id| OptionTarget::Global, // out of scope for S5
        |_id| ctx,
        false,
    );
    match out {
        Err(EngineOutcome::AwaitingInput { request, .. }) => {
            assert!(request.options.iter().all(|o| o.target == OptionTarget::Global));
        }
        other => panic!("2 candidates suspend, got {other:?}"),
    }
}
```

- [ ] **Step 6: Run to verify failure**

Run: `cargo test -p game-core grounded_choice_anchors_enemy_options`
Expected: FAIL — `resolve_grounded_choice` takes 7 args, test passes 8 (arity/type mismatch).

- [ ] **Step 7: Add the `target` param + supply it at the 4 callers**

In `evaluator.rs`, change the local `use` inside `resolve_grounded_choice` (~L1606) from `awaiting_choice` to the anchored variant:

```rust
    use crate::engine::dispatch::choice::{
        awaiting_choice_anchored, resolve_choice_count, ChoiceResolution,
    };
```

Add the `target` parameter to the signature (between `label` and `bind`, ~L1602):

```rust
fn resolve_grounded_choice<Id: Copy>(
    eval_ctx: EvalContext,
    candidates: &[Id],
    empty_reason: &'static str,
    prompt: &'static str,
    label: impl Fn(&Id) -> String,
    target: impl Fn(&Id) -> crate::engine::OptionTarget,
    bind: impl Fn(Id) -> EvalContext,
    interactive: bool,
) -> Result<EvalContext, EngineOutcome> {
```

Change the suspend-else branch (~L1626-1629) from labels-only to `(label, target)` pairs:

```rust
            } else {
                let options = candidates
                    .iter()
                    .map(|id| (label(id), target(id)))
                    .collect();
                Err(awaiting_choice_anchored(prompt, options))
            }
```

Supply the `target` closure at each caller, immediately after its existing `label` closure `|id| format!("{id:?}"),`:

- `ground_location_choice` (~L1674): add `|id| crate::engine::OptionTarget::Location(*id),`
- `ground_enemy_choice` (~L1699): add `|id| crate::engine::OptionTarget::Enemy(*id),`
- `ground_fight_target_choice` (~L1739): add `|id| crate::engine::OptionTarget::Enemy(*id),`
- `ground_investigator_choice` (~L1649): add `|_id| crate::engine::OptionTarget::Global,`

(Each is inserted as a new argument line between the `label` closure and the `bind` closure.)

- [ ] **Step 8: Run to verify pass**

Run: `cargo test -p game-core -- grounded_choice`
Expected: PASS (both evaluator tests).
Run: `cargo test -p game-core`
Expected: PASS (no resolve-path regression — anchors are display-only).

- [ ] **Step 9: Clippy + commit**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean.

```bash
git add crates/game-core/src/engine/dispatch/choice.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: anchor effect ChooseOne options to their enemy/location targets (S5)"
```

---

### Task 3: Round-end act-advance reaction anchors to the act card (E3)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (add `current_act_code`; add a `current_act` param to `build_resolution_options`; thread it at the 2 callers; update the existing anchor test; add a new test)

**Interfaces:**
- Consumes: `state.act_deck: Vec<Act>` + `state.act_index: usize`; `Act.code: CardCode`.
- Changes: `build_resolution_options(candidates: &[ResolutionCandidate], current_act: Option<&CardCode>) -> Vec<ChoiceOption>` — the `Board` arm emits `OptionTarget::Act` when `current_act == Some(&cand.code)`, else `Global`.
- Produces: `fn current_act_code(state: &GameState) -> Option<CardCode>`.

- [ ] **Step 1: Update the existing test to the new signature + add the failing new test**

In `reaction_windows.rs`, module `resolution_option_anchor_tests` (~L2107): change the existing call `build_resolution_options(&cands)` (~L2135) to `build_resolution_options(&cands, None)` (the `_board` code matches no act → still `Global`; the `opts[2].target == Global` assertion is unchanged). Then add:

```rust
    #[test]
    fn board_candidate_matching_current_act_anchors_to_act() {
        use crate::engine::OptionTarget;
        use crate::state::{CardCode, InvestigatorId, ResolutionCandidate};
        let act = CardCode::new("01109");
        let cands = vec![
            ResolutionCandidate {
                code: act.clone(), // the round-end act-advance reaction
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::Board,
            },
            ResolutionCandidate {
                code: CardCode::new("_other_board"), // some other board-wide reaction
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::Board,
            },
        ];
        let opts = build_resolution_options(&cands, Some(&act));
        assert_eq!(opts[0].target, OptionTarget::Act);
        assert_eq!(opts[1].target, OptionTarget::Global);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core board_candidate_matching_current_act_anchors_to_act`
Expected: FAIL — `build_resolution_options` takes 1 arg, tests pass 2 (arity mismatch on both the edited and new test).

- [ ] **Step 3: Add `current_act_code` + the `current_act` param + the Board-arm branch**

In `reaction_windows.rs`, add near `build_resolution_options` (~L592):

```rust
/// The current act's card code, if an act deck is loaded — used to anchor the
/// round-end act-advance reaction (a `CandidateSource::Board` candidate whose
/// code is the act) to the act card (S5, #540). `None` for fixtures with no act.
fn current_act_code(state: &GameState) -> Option<CardCode> {
    state
        .act_deck
        .get(state.act_index)
        .map(|act| act.code.clone())
}
```

Change `build_resolution_options` (~L598) to take `current_act` and branch the `Board` arm:

```rust
fn build_resolution_options(
    candidates: &[ResolutionCandidate],
    current_act: Option<&CardCode>,
) -> Vec<ChoiceOption> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, cand)| {
            let id = OptionId(u32::try_from(i).expect("option count fits in u32"));
            let (label, target) = match cand.source {
                CandidateSource::Hand => (
                    format!("Play {} from hand", cand.code),
                    crate::engine::OptionTarget::HandCardByCode {
                        investigator: cand.controller,
                        code: cand.code.clone(),
                    },
                ),
                CandidateSource::InPlay(instance_id) => (
                    format!("Resolve reaction: {}", cand.code),
                    crate::engine::OptionTarget::CardInstance(instance_id),
                ),
                CandidateSource::Board => {
                    // The round-end act-advance reaction is the one Board candidate
                    // whose code is the current act → anchor it to the act card
                    // (#540). Any other board-wide reaction has no card home.
                    let target = if current_act == Some(&cand.code) {
                        crate::engine::OptionTarget::Act
                    } else {
                        crate::engine::OptionTarget::Global
                    };
                    (format!("Resolve reaction: {}", cand.code), target)
                }
            };
            ChoiceOption::new(id, label, target)
        })
        .collect()
}
```

- [ ] **Step 4: Thread the current-act code at the two callers**

In `open_queued_reaction_window` (~L648), replace `build_resolution_options(<candidates expr>)` with a two-arg call. Compute the code first:

```rust
    let current_act = current_act_code(cx.state);
    let options = build_resolution_options(
        window
            .pending_candidates()
            .expect("open_queued_reaction_window: top window has candidates"),
        current_act.as_ref(),
    );
```

In `advance_resolution` (~L963), likewise:

```rust
    let current_act = current_act_code(cx.state);
    let options = build_resolution_options(candidates, current_act.as_ref());
```

(Both `window`/`candidates` and `current_act_code(cx.state)` are shared borrows of `cx.state`, so they coexist; `current_act` is owned, so it outlives the borrow.)

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p game-core -- resolution_option_anchor_tests`
Expected: PASS (edited + new test).
Run: `cargo test -p game-core`
Expected: PASS (round-end advance still resolves by index; other reaction-window tests unaffected).

- [ ] **Step 6: Clippy + commit**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean.

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: anchor the round-end act-advance reaction to the act card (S5)"
```

---

### Task 4: Act card glows + opens an Advance-act menu (W1)

**Files:**
- Modify: `crates/web/src/act_agenda.rs` (extract the act into a glow-capable `#[component] ActCard`; keep agenda display-only)
- Modify: `crates/web/tests/act_agenda.rs` (add an interactivity mount + 2 headless tests)

**Interfaces:**
- Consumes: `crate::interaction::{PendingOptions, options_for, menu_layer}`; `game_core::OptionTarget::Act`; `game_core::state::Act`.
- Produces: `#[component] pub fn ActCard(act: Act) -> impl IntoView` — renders `<article class="card card--act[ actionable]">`; glows + embeds `menu_layer` (wasm-only) when an `OptionTarget::Act` option is live.

- [ ] **Step 1: Write the failing headless tests**

Append to `crates/web/tests/act_agenda.rs` (the existing display-only test stays). Add these imports at the top (below the current `use`s):

```rust
use futures::channel::mpsc;
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use web::interaction::PendingOptions;
use web::store::ClientState;
use web::transport::OutboundTx;
```

Add the mount helper + tests:

```rust
fn act_card() -> web_sys::Element {
    document()
        .query_selector(".card--act")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .card--act")
}

/// Mount `act_agenda_view` (act 01109) with a store carrying `outcome`, a derived
/// `PendingOptions`, an `OutboundTx`, and a capturing channel.
async fn mount_with_prompt(
    outcome: game_core::EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let mut state = GameStateBuilder::new().build();
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 2,
        resolution: None,
    }];
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        web::act_agenda::act_agenda_view(&state)
    });
    leptos::task::tick().await;
    rx
}

#[wasm_bindgen_test]
async fn act_card_glows_and_advances_via_menu() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Advance act", OptionTarget::Act)],
    );
    let mut rx = mount_with_prompt(outcome).await;
    let card = act_card();
    assert!(card.class_name().contains("actionable"), "act card glows");
    card.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    leptos::task::tick().await;
    let item = card
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Advance act");
    item.click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn act_card_inert_without_an_act_anchored_option() {
    // Option anchors Global (not Act) → the act card stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "End turn", OptionTarget::Global)],
    );
    let _rx = mount_with_prompt(outcome).await;
    assert!(!act_card().class_name().contains("actionable"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `wasm-pack test --headless --firefox crates/web --test act_agenda`
Expected: FAIL — `act_card_glows_and_advances_via_menu` finds no `actionable` class / no `.menu-hit` (the act card is still display-only).

- [ ] **Step 3: Implement the `ActCard` component**

In `crates/web/src/act_agenda.rs`, change the import line:

```rust
use game_core::state::{Act, CardCode, GameState};
```

Add the `ActCard` component (after `name_and_text`):

```rust
/// The current act as a card. Glows and opens an "Advance act" context menu when
/// the live prompt anchors an option to the act (`OptionTarget::Act`) — both the
/// open-turn Advance action and the round-end advance reaction (S5, #540). The
/// agenda stays display-only (agenda advance is doom-forced, not player-chosen).
// `act` is taken by value: Leptos `#[component]` generates an owned props struct.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn ActCard(act: Act) -> impl IntoView {
    let (name, text) = name_and_text(&act.code);
    let threshold = act.clue_threshold;
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = crate::interaction::options_for(&pending, game_core::OptionTarget::Act);
    let actionable = !menu_opts.is_empty();
    #[cfg(target_arch = "wasm32")]
    let open = RwSignal::new(None::<(i32, i32)>);
    let mut root_class = String::from("card card--act");
    if actionable {
        root_class.push_str(" actionable");
    }
    view! {
        <article class=root_class>
            <div class="card-head">
                <span class="card-kind">"Act"</span>
                <span class="card-name">{name}</span>
            </div>
            <div class="card-text">{text}</div>
            <div class="loc-stats">
                <span>{format!("clues to advance: {threshold}")}</span>
            </div>
            {
                // wasm-only trigger + menu; host build: empty, `menu_opts` consumed
                // above by `actionable` (no unused-var warning).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
            }
        </article>
    }
}
```

Replace the `act` branch of `act_agenda_view` (the whole `let act = ...` binding) with a delegation to `ActCard`:

```rust
    let act = game
        .act_deck
        .get(game.act_index)
        .cloned()
        .map(|act| view! { <ActCard act=act/> });
```

(The `agenda` binding and the final `view! { <section class="act-agenda">{act}{agenda}</section> }` are unchanged.)

- [ ] **Step 4: Run the headless tests to verify pass**

Run: `wasm-pack test --headless --firefox crates/web --test act_agenda`
Expected: PASS — all 3 tests (the display-only render test still passes: no `PendingOptions` context → `use_context` is `None` → not actionable).

- [ ] **Step 5: Host build + both clippy passes + fmt**

Run: `cargo build -p web --target wasm32-unknown-unknown`
Expected: builds.
Run: `cargo clippy -p web --all-targets --all-features -- -D warnings`
Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
Expected: both clean (the `#[cfg]`-gated `open`/`menu_layer` compile-check on the wasm target).
Run: `cargo fmt --check`

- [ ] **Step 6: Commit**

```bash
git add crates/web/src/act_agenda.rs crates/web/tests/act_agenda.rs
git commit -m "web: act card glows + opens an Advance-act menu (S5)"
```

---

### Task 5: Prompt banner renders only un-anchored options (W4)

**Files:**
- Modify: `crates/web/src/prompt_banner.rs` (filter the option-button list to `OptionTarget::Global`; update doc comments)
- Modify: `crates/web/tests/prompt_banner.rs` (add a mixed-anchor test)

**Interfaces:**
- Consumes: `game_core::OptionTarget::Global`.
- Behavior: a skippable window's `PickSingle` options render as banner buttons **only** when `target == Global`; anchored options (now including the round-end act-advance) have card homes and are omitted.

- [ ] **Step 1: Write the failing test**

Append to `crates/web/tests/prompt_banner.rs`:

```rust
#[wasm_bindgen_test]
async fn banner_renders_only_unanchored_options() {
    // S5 (#540): once the round-end advance is anchored to the act card, the banner
    // stops duplicating anchored options — it renders only Global-anchored ones.
    use game_core::{ChoiceOption, EngineOutcome, InputRequest, OptionTarget, ResumeToken};
    let outcome = EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            "You may advance",
            vec![
                ChoiceOption::new(OptionId(0), "Advance act", OptionTarget::Act),
                ChoiceOption::new(OptionId(1), "Some global", OptionTarget::Global),
            ],
        )
        .skippable(),
        resume_token: ResumeToken(0),
    };
    let mut rx = mount(outcome, &[]).await;
    let banner = last_banner();
    let btns = banner.query_selector_all(".banner-option").expect("query");
    assert_eq!(btns.length(), 1, "only the Global option renders as a button");
    let btn = btns
        .item(0)
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("the one banner button");
    assert_eq!(btn.text_content().unwrap_or_default(), "Some global");
    btn.click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(1))),
        other @ ClientMessage::Submit { .. } => panic!("expected PickSingle, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `wasm-pack test --headless --firefox crates/web --test prompt_banner`
Expected: FAIL — `banner_renders_only_unanchored_options`: two `.banner-option` buttons render (Act + Global), so `btns.length()` is 2, not 1.

- [ ] **Step 3: Filter the option list to `Global`**

In `crates/web/src/prompt_banner.rs`, change the import to add `OptionTarget`:

```rust
use game_core::{ChoiceOption, EngineOutcome, InputKind, InputResponse, OptionId, OptionTarget, PlayerAction};
```

Insert a `.filter(...)` into the `option_btns` builder (the `request.options.iter()` chain, ~L45-48):

```rust
            let option_btns: Vec<_> = request
                .options
                .iter()
                .filter(|opt| opt.target == OptionTarget::Global)
                .cloned()
                .map(|opt: ChoiceOption| {
```

Update the two doc comments to match: the `#[component]` doc (~L18-21) and the inline comment (~L41-44). Replace both mentions of rendering "the window's `PickSingle` options" with the filtered intent, e.g. the inline comment becomes:

```rust
            // Option buttons — a skippable window's **un-anchored** (`Global`)
            // `PickSingle` options only; anchored options have board homes (S5,
            // #540). This still homes a genuinely-`Global`/`Board` window option
            // that lives nowhere else (the catch-all the bar retirement relies on).
```

and the component doc's second sentence becomes:

```rust
/// renders the window's un-anchored (`Global`) `PickSingle` options as buttons
/// (#549/#540), so an option reachable nowhere else has a home.
```

- [ ] **Step 4: Run the banner suite to verify pass**

Run: `wasm-pack test --headless --firefox crates/web --test prompt_banner`
Expected: PASS — the new test plus the existing `skippable_window_renders_options_that_submit_pick_single` (its fixture's "Resolve" option is `Global`, so it still renders).

- [ ] **Step 5: Host build + both clippy passes + fmt**

Run: `cargo build -p web --target wasm32-unknown-unknown`
Run: `cargo clippy -p web --all-targets --all-features -- -D warnings`
Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
Run: `cargo fmt --check`
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add crates/web/src/prompt_banner.rs crates/web/tests/prompt_banner.rs
git commit -m "web: prompt banner renders only un-anchored options (S5)"
```

---

### Task 6: Full CI gauntlet + PR

**Files:** none (verification + PR); the `docs/phases/phase-7-the-gathering.md` update is the final commit at PR-ready time per `CLAUDE.md` step 6.

- [ ] **Step 1: Run the full 7-job gauntlet locally**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green.

- [ ] **Step 2: Push + open the PR**

```bash
git push -u origin ui/interactivity-act-soak-choices
gh pr create --fill
```

The PR body follows the repo template; include a design-decisions paragraph noting the two scope calls (#492 deferred; round-end advance anchored → banner filtered) and the `Closes #540` line.

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`
Expected: 7 jobs green. Fix any failure with a follow-up commit to the same branch (do not amend/force-push).

- [ ] **Step 4: Phase-doc update (only once the PR is green + ready to merge)**

Update `docs/phases/phase-7-the-gathering.md` per `docs/phases/README.md` ("Maintaining these docs"): record S5 in the interactivity-pass section (flip the S5–S6 row to `S5 ✅ PR #<N>`), drop the settled round-end/#550 sequencing note now that S5 landed, and note the W4 banner-filter + the deferred #492 / investigator-choice `Global` as the remaining pre-S6 state. Commit as the final commit on the branch.

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: record interactivity S5 in the phase-7 plan"
git push
```

- [ ] **Step 5: Merge only after explicit user approval**

```bash
gh pr merge <PR#> --squash --delete-branch
```

Confirm #540 auto-closed and `git pull` on `main`.

---

## Self-review

**Spec coverage:**
- E1 soak → CardInstance → **Task 1** ✅
- E2 `ChooseOne` → Enemy/Location → **Task 2** ✅ (investigator-choice stays `Global`, tested)
- E3 round-end act → Act → **Task 3** ✅
- W1 act card glow → **Task 4** ✅
- W2 soak web (zero-code) → Coverage note + Task 1 anchor + existing `in_play_card.rs` ✅
- W3 `ChooseOne` web (zero-code) → Coverage note + Task 2 anchors + existing `enemy_card.rs`/`map.rs` ✅
- W4 banner filter → **Task 5** ✅
- Testing (engine native, web headless, gauntlet) → per-task steps + **Task 6** ✅
- Out-of-scope (#492, investigator-choice, agenda Board, `step_choose_one`) → not implemented; the `Global` cases are asserted (Task 2 investigator test) or untouched ✅

**Placeholder scan:** no TBD/TODO/"handle edge cases"/"similar to Task N" — every code step carries full code.

**Type consistency:** `soak_options(&[DistributionTarget]) -> Vec<ChoiceOption>`; `awaiting_choice_anchored(prompt, Vec<(String, OptionTarget)>)`; `resolve_grounded_choice(..., target: impl Fn(&Id) -> OptionTarget, ...)`; `build_resolution_options(candidates, Option<&CardCode>)`; `current_act_code(&GameState) -> Option<CardCode>`; `ActCard(act: Act)` — names/signatures match across the tasks that call them.
