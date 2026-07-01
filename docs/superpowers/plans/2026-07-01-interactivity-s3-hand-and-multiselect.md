# Interactivity S3 — hand menus + multi-select + prompt banner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Playable hand cards glow and open a "Play" menu; the `PickMultiple` prompts (mulligan / commit / hand-size discard) become click-to-select on the hand cards with a bottom-fixed prompt banner carrying Confirm/Pass — retiring the flat bar's commit UI.

**Architecture:** A new `HandCardView` wrapper (keeps `Card` display-only) reads `PendingOptions` + a new `MultiSelect` context and branches: multi-select active → click-to-toggle selection ring; else → the S1/S2 `menu_layer` "Play" menu. A wasm-only `PromptBanner` (bottom-fixed) submits the multi-select. `input.rs`'s `PickMultiple` arm is removed.

**Tech Stack:** Rust, Leptos 0.8 (CSR/wasm), `wasm-bindgen-test` headless Firefox.

## Global Constraints

- Issue: **#538** (interactivity S3). Umbrella: **#206**. Design: `docs/superpowers/specs/2026-07-01-interactivity-s3-hand-and-multiselect-design.md`.
- Branch: `ui/interactivity-hand-menus` (created; S3 spec committed on it). Commit scope: `web:`.
- **Move `PickMultiple` off the bar** (agreed deviation from "bar keeps everything"): remove `input.rs`'s `PickMultiple` arm; the board hand + banner replace it. Bar keeps `PickSingle` / `Confirm` / `Skip`.
- **No engine change; no `OptionTarget` change; no prompt banner beyond `PickMultiple`** this slice.
- wasm-gating: the selection-toggle click is **non-gated** (no coords); the Play `menu_layer` + `PromptBanner` (submit via `OutboundTx`) are wasm-only. Per-entity `open` signal is wasm-gated.
- Match CI's strict flags before pushing (all seven jobs). Merge only after approval.

---

### Task 1: `MultiSelect` context + `is_multi_select`

**Files:**
- Modify: `crates/web/src/interaction.rs` (add `MultiSelect` + `is_multi_select` + native tests)

**Interfaces:**
- Produces:
  - `web::interaction::MultiSelect { active: Signal<bool>, selected: RwSignal<BTreeSet<u32>> }` (derives `Clone`)
  - `web::interaction::is_multi_select(state: &ClientState) -> bool`

- [ ] **Step 1: Write the failing native test** — append inside `interaction.rs`'s `#[cfg(test)] mod tests`

```rust
    #[test]
    fn is_multi_select_true_only_for_pick_multiple() {
        use game_core::EngineOutcome;
        let mut state = ClientState::default();
        assert!(!is_multi_select(&state)); // no outcome

        state.outcome = Some(EngineOutcome::Done);
        assert!(!is_multi_select(&state));

        state.outcome = Some(game_core::test_support::fixtures::awaiting_commit_input("Commit"));
        assert!(is_multi_select(&state));

        state.outcome = Some(game_core::test_support::fixtures::awaiting_pick_single_with(
            "x",
            vec![opt(0, OptionTarget::Global)],
        ));
        assert!(!is_multi_select(&state));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p web --lib interaction::tests::is_multi_select_true_only_for_pick_multiple 2>&1 | tail -8`
Expected: compile error — `is_multi_select` / `MultiSelect` not found.

- [ ] **Step 3: Add `MultiSelect` + `is_multi_select`** — in `crates/web/src/interaction.rs`, after the `PendingOptions` struct

```rust
/// Multi-select (`PickMultiple`) UI state, shared so hand cards toggle it and the
/// prompt banner reads it. `active` is true iff a `PickMultiple` prompt is live.
#[derive(Clone)]
pub struct MultiSelect {
    /// True iff the live outcome is `AwaitingInput { kind: PickMultiple }`.
    pub active: Signal<bool>,
    /// The chosen hand indices (each `OptionId(i)` = hand index `i`).
    pub selected: leptos::prelude::RwSignal<std::collections::BTreeSet<u32>>,
}

/// True iff the live outcome is an `AwaitingInput` whose kind is `PickMultiple`
/// (mulligan / skill-test commit / hand-size discard). Pure.
#[must_use]
pub fn is_multi_select(state: &ClientState) -> bool {
    matches!(
        &state.outcome,
        Some(EngineOutcome::AwaitingInput { request, .. })
            if request.kind == game_core::InputKind::PickMultiple
    )
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p web --lib interaction 2>&1 | grep -E "test result|is_multi_select"`
Expected: PASS (the new test + the existing interaction tests).

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/interaction.rs
git commit -m "web: MultiSelect context + is_multi_select derivation"
```

---

### Task 2: `HandCardView` wrapper (Play menu + selection ring) + board wiring + CSS

**Files:**
- Modify: `crates/web/src/card.rs` (add `HandCardView`)
- Modify: `crates/web/src/board.rs` (wrap hand cards)
- Modify: `crates/web/style.css` (`.hand-slot` glow/selected)
- Modify: `crates/web/tests/card.rs` (headless Play-menu + selection tests)

**Interfaces:**
- Consumes: `MultiSelect` / `is_multi_select` (T1); `menu_layer` / `options_for` / `PendingOptions` (S1/S2); `OptionTarget::HandCard`.
- Produces: `web::card::HandCardView` — props `code: CardCode`, `investigator: InvestigatorId`, `index: u8`.

- [ ] **Step 1: Write the failing headless tests** — append to `crates/web/tests/card.rs`

Add imports at the top:

```rust
use futures::channel::mpsc;
use game_core::state::InvestigatorId;
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use protocol::ClientMessage;
use std::collections::BTreeSet;
use web::card::HandCardView;
use web::interaction::{MultiSelect, PendingOptions};
use web::store::ClientState;
use web::transport::OutboundTx;
```

Then append:

```rust
/// The last-mounted `.hand-slot`.
fn last_slot() -> web_sys::Element {
    let slots = leptos::prelude::document()
        .query_selector_all(".hand-slot")
        .expect("query");
    slots
        .item(slots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .hand-slot")
}

/// Mount a `HandCardView` (Machete 01020, investigator 1, index 0) with a store
/// carrying `outcome`, `PendingOptions` derived from it, a `MultiSelect` whose
/// `active` reflects the outcome, and a capturing outbound channel.
async fn mount_hand(
    outcome: game_core::EngineOutcome,
) -> (RwSignal<BTreeSet<u32>>, mpsc::UnboundedReceiver<ClientMessage>) {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let selected = RwSignal::new(BTreeSet::<u32>::new());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
        provide_context(MultiSelect { active, selected });
        view! {
            <HandCardView code=CardCode::new("01020") investigator=InvestigatorId(1) index=0/>
        }
    });
    leptos::task::tick().await;
    (selected, rx)
}

#[wasm_bindgen_test]
async fn playable_hand_card_opens_a_play_menu_and_submits() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Play Machete",
            OptionTarget::HandCard { investigator: InvestigatorId(1), hand_index: 0 },
        )],
    );
    let (_selected, mut rx) = mount_hand(outcome).await;

    let slot = last_slot();
    assert!(slot.class_name().contains("actionable"), "slot glows");
    slot.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    leptos::task::tick().await;
    let item = slot
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Play Machete");
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
async fn multi_select_active_makes_hand_card_toggle_selected() {
    let (selected, _rx) = mount_hand(
        game_core::test_support::fixtures::awaiting_commit_input("Commit cards"),
    )
    .await;
    let slot = last_slot();
    assert!(!slot.class_name().contains("actionable"), "no Play menu in select mode");
    slot.clone()
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;
    assert!(selected.get_untracked().contains(&0), "index 0 selected");
    assert!(last_slot().class_name().contains("selected"), "selected ring shown");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo build -p web --target wasm32-unknown-unknown --tests --test card 2>&1 | tail -6`
Expected: compile error — `HandCardView` not found.

- [ ] **Step 3: Add `HandCardView`** — in `crates/web/src/card.rs`, after the `Card` component

```rust
/// Interactive wrapper for a hand card (#538). Keeps [`Card`] display-only:
/// reads `PendingOptions` + `MultiSelect` and either enters selection mode (a
/// `PickMultiple` is live — click toggles `.hand-slot.selected`) or offers a
/// "Play …" [`menu_layer`](crate::interaction::menu_layer) via the card's
/// `HandCard` anchor. The two modes are mutually exclusive.
#[component]
pub fn HandCardView(
    code: CardCode,
    investigator: game_core::state::InvestigatorId,
    index: u8,
) -> impl IntoView {
    let idx = u32::from(index);
    if let Some(ms) = use_context::<crate::interaction::MultiSelect>() {
        if ms.active.get() {
            let selected = ms.selected;
            return view! {
                <div
                    class="hand-slot"
                    class:selected=move || selected.get().contains(&idx)
                    on:click=move |_| selected.update(|s| {
                        if !s.remove(&idx) {
                            s.insert(idx);
                        }
                    })
                >
                    <Card code=code/>
                </div>
            }
            .into_any();
        }
    }
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = crate::interaction::options_for(
        &pending,
        game_core::OptionTarget::HandCard {
            investigator,
            hand_index: index,
        },
    );
    let actionable = !menu_opts.is_empty();
    #[cfg(target_arch = "wasm32")]
    let open = RwSignal::new(None::<(i32, i32)>);
    view! {
        <div class="hand-slot" class:actionable=actionable>
            <Card code=code/>
            {
                // wasm-only Play trigger + menu (web_sys / OutboundTx).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
            }
        </div>
    }
    .into_any()
}
```

- [ ] **Step 4: Wire the hand in `board.rs`** — replace the hand render (in `investigators_panel`)

```rust
            let inv_id = inv.id;
            let hand: Vec<_> = inv
                .hand
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, code)| {
                    let index = u8::try_from(i).unwrap_or(u8::MAX);
                    view! {
                        <crate::card::HandCardView code=code investigator=inv_id index=index/>
                    }
                })
                .collect();
```

- [ ] **Step 5: Add the CSS** — append to `crates/web/style.css`

```css
/* Interactivity S3 (#538): hand-card wrapper — glow (Play) / ring (select). */
.hand-slot { position: relative; }
.hand-slot.actionable { box-shadow: 0 0 0 2px #e0b84c; cursor: pointer; }
.hand-slot.selected { box-shadow: 0 0 0 3px #4a90d9; cursor: pointer; }
```

- [ ] **Step 6: Run the headless card tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web --test card 2>&1 | tail -15`
Expected: all `card` tests pass, including the two new ones.

- [ ] **Step 7: Host clippy clean**

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings 2>&1 | tail -5`
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/web/src/card.rs crates/web/src/board.rs crates/web/style.css crates/web/tests/card.rs
git commit -m "web: HandCardView wrapper — Play menu + click-to-select ring"
```

---

### Task 3: `PromptBanner` + provide `MultiSelect` + move `PickMultiple` off the bar

**Files:**
- Create: `crates/web/src/prompt_banner.rs`
- Modify: `crates/web/src/lib.rs` (register `prompt_banner`, wasm-gated)
- Modify: `crates/web/src/app.rs` (provide `MultiSelect`; mount `PromptBanner`)
- Modify: `crates/web/src/input.rs` (remove the `PickMultiple` arm; skip `PickMultiple`)
- Modify: `crates/web/style.css` (`.prompt-banner`)
- Create: `crates/web/tests/prompt_banner.rs`
- Delete: `crates/web/tests/input.rs` (its 7 tests are all the moved `PickMultiple` commit UI; `PickSingle`/`Confirm`/`Skip` coverage lives in `tests/awaiting_input.rs`)

**Interfaces:**
- Consumes: `MultiSelect` (T1); `HandCardView` selection (T2).
- Produces: `web::prompt_banner::PromptBanner` (wasm-only).

- [ ] **Step 1: Write the failing banner headless test** — create `crates/web/tests/prompt_banner.rs`

```rust
//! Headless tests for `PromptBanner` (interactivity S3, #538): a live PickMultiple
//! prompt renders a bottom-fixed banner whose Confirm submits the toggled
//! selection and whose Pass (when skippable) submits Skip.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::test_support::fixtures::awaiting_commit_input;
use game_core::{InputResponse, OptionId, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use std::collections::BTreeSet;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::MultiSelect;
use web::prompt_banner::PromptBanner;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `PromptBanner` with a store carrying `outcome`, a `MultiSelect` whose
/// `selected` starts as `preselected`, and a capturing channel.
async fn mount(
    outcome: game_core::EngineOutcome,
    preselected: &[u32],
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let selected = RwSignal::new(preselected.iter().copied().collect::<BTreeSet<u32>>());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
        provide_context(MultiSelect { active, selected });
        view! { <PromptBanner/> }
    });
    leptos::task::tick().await;
    rx
}

fn last_banner() -> web_sys::Element {
    let bs = document().query_selector_all(".prompt-banner").expect("query");
    bs.item(bs.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .prompt-banner")
}

fn click(sel: &str) {
    last_banner()
        .query_selector(sel)
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("element")
        .click();
}

#[wasm_bindgen_test]
async fn confirm_submits_the_selected_indices() {
    let mut rx = mount(awaiting_commit_input("Commit"), &[0, 2]).await;
    click(".confirm");
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(
            response,
            InputResponse::PickMultiple {
                selected: vec![OptionId(0), OptionId(2)]
            }
        ),
        other @ ClientMessage::Submit { .. } => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn confirm_with_no_selection_submits_empty() {
    let mut rx = mount(awaiting_commit_input("Commit"), &[]).await;
    click(".confirm");
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickMultiple { selected: vec![] }),
        other @ ClientMessage::Submit { .. } => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn skippable_prompt_shows_pass_that_submits_skip() {
    let mut rx = mount(awaiting_commit_input("Commit").skippable_for_test(), &[]).await;
    click(".pass");
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Skip),
        other @ ClientMessage::Submit { .. } => panic!("expected Skip, got {other:?}"),
    }
}
```

Note: `awaiting_commit_input` returns a non-skippable `PickMultiple`. The
`skippable` test needs a skippable one. Add a tiny game-core fixture helper in
this task's Step 2 rather than a `skippable_for_test()` shim — see Step 2.

- [ ] **Step 2: Add a skippable `PickMultiple` fixture** — in `crates/game-core/src/test_support/fixtures.rs`, after `awaiting_pick_single_with`

```rust
/// A skippable [`PickMultiple`](crate::InputKind::PickMultiple)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome — for UI tests of the
/// Pass/Skip control on a multi-select prompt.
#[must_use]
pub fn awaiting_skippable_commit_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_multiple(prompt).skippable(),
        resume_token: ResumeToken(0),
    }
}
```

Then in the test, replace `awaiting_commit_input("Commit").skippable_for_test()` with
`game_core::test_support::fixtures::awaiting_skippable_commit_input("Commit")` and add
its import.

- [ ] **Step 3: Run the banner test to verify it fails**

Run: `cargo build -p web --target wasm32-unknown-unknown --tests --test prompt_banner 2>&1 | tail -6`
Expected: compile error — `web::prompt_banner` not found.

- [ ] **Step 4: Create `PromptBanner`** — `crates/web/src/prompt_banner.rs`

```rust
//! Bottom-fixed prompt banner (interactivity S3, #538): for a live `PickMultiple`
//! prompt, renders its text + a Confirm (submits the `MultiSelect` selection) and,
//! when skippable, a Pass (submits Skip). wasm-only — submits via `OutboundTx`.
//! Other prompt kinds stay in the flat bar until later slices.

use std::collections::BTreeSet;

use game_core::{EngineOutcome, InputKind, InputResponse, OptionId, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::interaction::MultiSelect;
use crate::store::use_store;
use crate::transport::OutboundTx;

/// The bottom-fixed multi-select banner. Renders nothing unless a `PickMultiple`
/// prompt is live and a [`MultiSelect`] context is present.
#[component]
pub fn PromptBanner() -> impl IntoView {
    let store = use_store();
    let tx = use_context::<OutboundTx>();
    let ms = use_context::<MultiSelect>();
    view! {
        {move || {
            let state = store.get();
            let Some(EngineOutcome::AwaitingInput { request, .. }) = state.outcome else {
                return ().into_any();
            };
            if request.kind != InputKind::PickMultiple {
                return ().into_any();
            }
            let Some(ms) = ms.clone() else {
                return ().into_any();
            };
            let selected = ms.selected;
            let prompt = request.prompt.clone();
            let skippable = request.skippable;

            let tx_c = tx.clone();
            let confirm = move |_| {
                if let Some(tx) = tx_c.clone() {
                    let sel: Vec<OptionId> =
                        selected.get_untracked().into_iter().map(OptionId).collect();
                    store.update(|s| s.pending_label = Some(format!("Commit {} card(s)", sel.len())));
                    let _ = tx.unbounded_send(ClientMessage::Submit {
                        action: PlayerAction::ResolveInput {
                            response: InputResponse::PickMultiple { selected: sel },
                        },
                    });
                    selected.set(BTreeSet::new());
                }
            };

            let tx_s = tx.clone();
            let pass = move |_| {
                if let Some(tx) = tx_s.clone() {
                    store.update(|s| s.pending_label = Some("Skip".to_string()));
                    let _ = tx.unbounded_send(ClientMessage::Submit {
                        action: PlayerAction::ResolveInput { response: InputResponse::Skip },
                    });
                }
            };
            let pass_btn =
                skippable.then(|| view! { <button class="pass" on:click=pass>"Pass"</button> });

            view! {
                <div class="prompt-banner">
                    <span class="prompt">{prompt}</span>
                    <button class="confirm" on:click=confirm>"Confirm"</button>
                    {pass_btn}
                </div>
            }
            .into_any()
        }}
    }
}
```

- [ ] **Step 5: Register the module** — in `crates/web/src/lib.rs`, after the `#[cfg(target_arch = "wasm32")] pub mod input;` block

```rust
#[cfg(target_arch = "wasm32")]
pub mod prompt_banner;
```

- [ ] **Step 6: Provide `MultiSelect` + mount the banner in `app.rs`** — in `crates/web/src/app.rs`, extend the context-provision block (after the `PendingOptions` provide)

```rust
    // Multi-select (PickMultiple) selection state, shared by the hand cards and
    // the prompt banner; cleared whenever a PickMultiple isn't live (#538).
    let selected = RwSignal::new(std::collections::BTreeSet::<u32>::new());
    let multi_active = Signal::derive(move || store.with(crate::interaction::is_multi_select));
    Effect::new(move |_| {
        if !multi_active.get() {
            selected.set(std::collections::BTreeSet::new());
        }
    });
    provide_context(crate::interaction::MultiSelect {
        active: multi_active,
        selected,
    });
```

And mount `<PromptBanner/>` in the wasm-only view block — inside the existing
`#[cfg(target_arch = "wasm32")]` `view!` (alongside the `action-bar`), add it as a
sibling after the `action-bar` `div`:

```rust
                            <div class="action-bar">
                                <crate::picker::PickerView/>
                                <crate::skill_test_result::SkillTestResultView/>
                                <crate::input::AwaitingInputView/>
                            </div>
                            <crate::prompt_banner::PromptBanner/>
```

- [ ] **Step 7: Remove `input.rs`'s `PickMultiple` arm** — in `crates/web/src/input.rs`:
  1. Delete the `let selected = RwSignal::new(BTreeSet::<u32>::new());` line and its comment.
  2. After the `let (Some(EngineOutcome::AwaitingInput { request, .. }), Some(game)) = … else { … };` destructure, add an early skip (the banner owns multi-select):

```rust
            // PickMultiple is rendered by the bottom prompt banner (#538), not here.
            if request.kind == InputKind::PickMultiple {
                return ().into_any();
            }
```

  3. Delete the entire `InputKind::PickMultiple => { … }` match arm.
  4. Delete the `active_hand` fn (now unused).
  5. Remove now-unused imports: `use std::collections::BTreeSet;`, `OptionId` from the `game_core::{…}` import, and `GameState` if the `game` binding is now unused (it was only read by `active_hand`; if the remaining arms don't use `game`, change the destructure to `let (Some(EngineOutcome::AwaitingInput { request, .. }), _) = (state.outcome.clone(), state.game.clone()) else {…}` — or drop the `game` fetch entirely). Verify with clippy in Step 9.

- [ ] **Step 8: Delete the obsolete input tests + add banner CSS**

```bash
git rm crates/web/tests/input.rs
```

Append to `crates/web/style.css`:

```css
.prompt-banner {
    position: fixed; bottom: 0; left: 0; right: 0; z-index: 25;
    display: flex; align-items: center; gap: 0.5rem;
    background: #1b1b1b; color: #eee; padding: 0.4rem 0.75rem;
}
```

- [ ] **Step 9: Run the banner test + host clippy + wasm clippy**

Run: `wasm-pack test --headless --firefox crates/web --test prompt_banner 2>&1 | tail -12`
Expected: 3 banner tests pass.
Run: `cargo clippy -p web --all-targets --all-features -- -D warnings 2>&1 | tail -5`
Expected: clean (no unused `selected`/`active_hand`/`BTreeSet`/`OptionId`/`game`).
Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add crates/game-core/src/test_support/fixtures.rs crates/web/src/prompt_banner.rs \
        crates/web/src/lib.rs crates/web/src/app.rs crates/web/src/input.rs \
        crates/web/style.css crates/web/tests/prompt_banner.rs
git add -u crates/web/tests/input.rs
git commit -m "web: bottom prompt banner drives multi-select; drop the bar's commit UI"
```

---

## Verification (full CI gauntlet, before pushing)

```sh
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
                            wasm-pack test --headless --firefox crates/web
                            cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Watch for: `doc_lazy_continuation` on new doc comments; the host `web` clippy job (the wasm-gated `open`); a stray unused binding in `input.rs` after the arm removal.

## PR flow (after the gauntlet is green)

1. Push `ui/interactivity-hand-menus`; open the PR. Body: goal + `Closes #538.`
2. `gh pr checks <PR#> --watch`.
3. **Phase-doc update is the final commit, pushed only after CI is green** (record S3; tick #538 in the #206 checklist after merge) — then CI green again, merge on approval.

## Self-review notes

- **Spec coverage:** `MultiSelect` + `is_multi_select` ✅ (T1); `HandCardView` Play menu + selection mode ✅ (T2); board wiring ✅ (T2); `.hand-slot` CSS ✅ (T2); `PromptBanner` bottom-fixed Confirm/Pass ✅ (T3); `MultiSelect` provided + cleared ✅ (T3); `input.rs` `PickMultiple` arm removed + `PickMultiple` skipped ✅ (T3); no engine change except a test fixture ✅; single-option-opens-menu ✅ (Play is one item, still opens); solo scope honored.
- **Testing:** native `is_multi_select` ✅ (T1); headless Play menu + selection toggle ✅ (T2); banner Confirm(selected)/Confirm(empty)/Pass→Skip ✅ (T3); obsolete `tests/input.rs` removed, `PickSingle`/`Confirm`/`Skip` coverage retained in `tests/awaiting_input.rs` ✅.
- **Type consistency:** `MultiSelect { active: Signal<bool>, selected: RwSignal<BTreeSet<u32>> }` used identically in T1/T2/T3 and app.rs; `HandCardView { code, investigator, index }`; submit paths `PickMultiple { selected: Vec<OptionId> }` / `Skip` in banner + tests; `OptionTarget::HandCard { investigator, hand_index }` matches the enum variant; `is_multi_select`/`pending_options` both read `state.outcome`.
