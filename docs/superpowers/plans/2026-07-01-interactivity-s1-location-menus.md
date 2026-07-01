# Interactivity S1 — location context menus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A map location the active investigator can act on glows and opens a context menu of its legal actions, submitting the chosen `ResolveInput` — the first consumer of S0's `OptionTarget` anchor.

**Architecture:** A new `web::interaction` module holds pure routing fns (`pending_options`, `options_for`) + a `PendingOptions` context type (non-gated, native-testable) and a wasm-gated `ContextMenu` component (needs the wasm-only `OutboundTx`). `app.rs` provides a derived `PendingOptions` signal; each `map.rs` node computes its options via `options_for(Location(id))`, glows when non-empty, and embeds a `ContextMenu`. The flat action bar is untouched (bar keeps everything until S6).

**Tech Stack:** Rust, Leptos 0.8 (CSR/wasm), `futures::mpsc` (wasm-only), `wasm-bindgen-test` headless Firefox.

## Global Constraints

- Issue: **#536** (interactivity S1). Umbrella: **#206**. Design: `docs/superpowers/specs/2026-07-01-interactivity-s1-location-menus-design.md`.
- Branch: `ui/interactivity-location-menus` (already created; the S1 spec is already committed on it). Commit scope prefix: `web:`.
- **No `input.rs` / action-bar change** — the bar keeps rendering every option (transition choice); S1 is purely additive.
- **No prompt banner** (deferred to S3/S4).
- `futures`/`OutboundTx` are **wasm-only** → the `ContextMenu` component and any submit code are `#[cfg(target_arch = "wasm32")]`; the pure routing fns + `PendingOptions` type are **not** gated (so `map.rs`, which compiles on host, references only the non-gated parts, and the pure fns get native tests).
- **Even a single option opens the menu** (no click-to-auto-execute).
- Match CI's strict flags before pushing (all seven jobs). Merge only after approval.

---

### Task 1: `interaction` routing substrate (pure fns + `PendingOptions`) + a test fixture

**Files:**
- Modify: `crates/game-core/src/test_support/fixtures.rs` (add `awaiting_pick_single_with`)
- Create: `crates/web/src/interaction.rs`
- Modify: `crates/web/src/lib.rs:12-13` (add `pub mod interaction;`)

**Interfaces:**
- Produces:
  - `game_core::test_support::fixtures::awaiting_pick_single_with(prompt: impl Into<String>, options: Vec<ChoiceOption>) -> EngineOutcome`
  - `web::interaction::pending_options(state: &ClientState) -> Vec<ChoiceOption>`
  - `web::interaction::options_for(options: &[ChoiceOption], target: OptionTarget) -> Vec<ChoiceOption>`
  - `web::interaction::PendingOptions(pub leptos::prelude::Signal<Vec<ChoiceOption>>)` (derives `Clone`)

- [ ] **Step 1: Add the game-core test fixture** in `crates/game-core/src/test_support/fixtures.rs` (right after `awaiting_pick_single_input`)

```rust
/// An [`AwaitingInput`](EngineOutcome::AwaitingInput) `PickSingle` outcome over
/// caller-supplied `options` — for host/UI tests that need a specific
/// [`OptionTarget`](crate::OptionTarget) anchor (the no-arg
/// [`awaiting_pick_single_input`] fixture is `Global`-only). `ResumeToken(0)`
/// matches the other fixtures (the UI never inspects it).
#[must_use]
pub fn awaiting_pick_single_with(
    prompt: impl Into<String>,
    options: Vec<ChoiceOption>,
) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, options),
        resume_token: ResumeToken(0),
    }
}
```

- [ ] **Step 2: Verify it compiles** (fixtures already import `ChoiceOption`, `EngineOutcome`, `InputRequest`, `ResumeToken`, `OptionId`)

Run: `cargo build -p game-core 2>&1 | tail -5`
Expected: builds clean.

- [ ] **Step 3: Write the failing `interaction` unit tests** — create `crates/web/src/interaction.rs` with only the tests (implementation comes next)

```rust
//! Board interactivity routing (#536): map an `AwaitingInput`'s options to the
//! board entity each acts on (via S0's `OptionTarget`), plus the `ContextMenu`
//! that renders a chosen entity's options. The routing fns are pure and
//! native-tested; `ContextMenu` is wasm-only (it submits via the wasm-only
//! `OutboundTx`).

use game_core::{ChoiceOption, EngineOutcome, OptionTarget};
use leptos::prelude::Signal;

use crate::store::ClientState;

/// The live prompt's offered options — the `AwaitingInput` request's `options`,
/// else empty (`Done` / `Rejected` / no outcome). Pure.
#[must_use]
pub fn pending_options(state: &ClientState) -> Vec<ChoiceOption> {
    match &state.outcome {
        Some(EngineOutcome::AwaitingInput { request, .. }) => request.options.clone(),
        _ => Vec::new(),
    }
}

/// The options anchored to `target`, in offered order. Pure; a linear scan
/// (option counts are tiny, so `OptionTarget` needs no `Hash`).
#[must_use]
pub fn options_for(options: &[ChoiceOption], target: OptionTarget) -> Vec<ChoiceOption> {
    options
        .iter()
        .filter(|o| o.target == target)
        .cloned()
        .collect()
}

/// Context newtype carrying the derived pending-options signal, so any entity
/// reads it without prop-drilling. A distinct type so it can't collide with
/// other `Signal` contexts.
#[derive(Clone)]
pub struct PendingOptions(pub Signal<Vec<ChoiceOption>>);

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::{EnemyId, LocationId};
    use game_core::OptionId;

    fn opt(id: u32, target: OptionTarget) -> ChoiceOption {
        ChoiceOption::new(OptionId(id), format!("opt{id}"), target)
    }

    #[test]
    fn pending_options_empty_when_not_awaiting() {
        assert!(pending_options(&ClientState::default()).is_empty());
    }

    #[test]
    fn pending_options_returns_the_awaiting_requests_options() {
        let mut state = ClientState::default();
        state.outcome = Some(game_core::test_support::fixtures::awaiting_pick_single_with(
            "x",
            vec![opt(0, OptionTarget::Location(LocationId(10)))],
        ));
        assert_eq!(pending_options(&state).len(), 1);
    }

    #[test]
    fn options_for_returns_only_the_matching_anchor() {
        let opts = vec![
            opt(0, OptionTarget::Location(LocationId(10))),
            opt(1, OptionTarget::Enemy(EnemyId(7))),
            opt(2, OptionTarget::Global),
            opt(3, OptionTarget::Location(LocationId(11))),
        ];
        let got = options_for(&opts, OptionTarget::Location(LocationId(10)));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, OptionId(0));
    }
}
```

- [ ] **Step 4: Register the module** — in `crates/web/src/lib.rs`, add `pub mod interaction;` between `pub mod event_log;` and `pub mod map;`

```rust
pub mod event_log;
pub mod interaction;
pub mod map;
```

- [ ] **Step 5: Run the native tests to verify they pass**

Run: `cargo test -p web interaction 2>&1 | tail -12`
Expected: PASS — `pending_options_empty_when_not_awaiting`, `pending_options_returns_the_awaiting_requests_options`, `options_for_returns_only_the_matching_anchor`.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/test_support/fixtures.rs crates/web/src/interaction.rs crates/web/src/lib.rs
git commit -m "web: interaction routing substrate (pending_options / options_for / PendingOptions)"
```

---

### Task 2: `ContextMenu` component (wasm) + headless test

**Files:**
- Modify: `crates/web/src/interaction.rs` (append the wasm-gated `ContextMenu`)
- Create: `crates/web/tests/context_menu.rs`

**Interfaces:**
- Consumes: `PendingOptions` / routing fns (Task 1); `crate::store::use_store`; `crate::transport::OutboundTx`.
- Produces: `#[cfg(target_arch = "wasm32")] web::interaction::ContextMenu` — props `options: Vec<ChoiceOption>`, `open: RwSignal<bool>`. When `open()`, renders `.menu-backdrop` (click → close) + `.context-menu` of `.menu-item` buttons; a click submits `ResolveInput(PickSingle(id))` and closes.

- [ ] **Step 1: Write the failing headless test** — create `crates/web/tests/context_menu.rs`

```rust
//! Headless tests for `ContextMenu` (interactivity S1, #536): an open menu
//! renders one button per option; clicking one submits the matching
//! `ResolveInput(PickSingle)` and closes the menu; a closed menu renders nothing.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::LocationId;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::ContextMenu;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount a `ContextMenu` with one `Location(10)`-anchored option, a fresh store
/// + outbound channel, and the given initial `open` state. Returns the `open`
/// signal and the receiver for submitted frames.
async fn mount(open_initial: bool) -> (RwSignal<bool>, mpsc::UnboundedReceiver<ClientMessage>) {
    let store = RwSignal::new(ClientState::default());
    let open = RwSignal::new(open_initial);
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    let options = vec![ChoiceOption::new(
        OptionId(0),
        "Investigate",
        OptionTarget::Location(LocationId(10)),
    )];
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        leptos::view! { <ContextMenu options=options.clone() open=open/> }
    });
    leptos::task::tick().await;
    (open, rx)
}

/// The last mounted `.context-menu`'s `.menu-item` buttons (DOM accumulates
/// across tests in one page).
fn menu_items() -> web_sys::NodeList {
    let menus = document().query_selector_all(".context-menu").expect("query");
    if menus.length() == 0 {
        return document().query_selector_all(".none").expect("query");
    }
    menus
        .item(menus.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("Element")
        .query_selector_all(".menu-item")
        .expect("query")
}

#[wasm_bindgen_test]
async fn open_menu_renders_a_button_per_option() {
    let _ = mount(true).await;
    let items = menu_items();
    assert_eq!(items.length(), 1);
    assert_eq!(
        items.item(0).and_then(|n| n.text_content()).unwrap_or_default(),
        "Investigate"
    );
}

#[wasm_bindgen_test]
async fn closed_menu_renders_no_context_menu() {
    let _ = mount(false).await;
    // No `.context-menu` was rendered by the just-mounted (closed) menu — the
    // count did not grow. Assert the freshly mounted subtree has no items.
    assert_eq!(menu_items().length(), 0);
}

#[wasm_bindgen_test]
async fn clicking_an_item_submits_pick_single_and_closes() {
    let (open, mut rx) = mount(true).await;
    let items = menu_items();
    items
        .item(0)
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;

    let msg = rx.try_next().expect("a frame was sent").expect("frame present");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
    }
    assert!(!open.get(), "menu closes after a selection");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo build -p web --target wasm32-unknown-unknown --tests 2>&1 | tail -8`
Expected: compile error — no `ContextMenu` in `web::interaction`.

- [ ] **Step 3: Implement `ContextMenu`** — append to `crates/web/src/interaction.rs`

```rust
/// A popover of a board entity's offered options (#536). When `open`, renders a
/// full-screen transparent backdrop (click → close, the no-document-listener
/// dismiss) and a button per option; a click submits
/// `ResolveInput(PickSingle(id))` and closes. wasm-only — it submits via the
/// wasm-only `OutboundTx` (mirrors the `input.rs` submit path, which S6 folds in).
#[cfg(target_arch = "wasm32")]
#[leptos::component]
pub fn ContextMenu(
    options: Vec<ChoiceOption>,
    open: leptos::prelude::RwSignal<bool>,
) -> impl leptos::prelude::IntoView {
    use leptos::prelude::*;

    use game_core::{InputResponse, PlayerAction};
    use protocol::ClientMessage;

    use crate::store::use_store;
    use crate::transport::OutboundTx;

    let store = use_store();
    let tx = use_context::<OutboundTx>();

    view! {
        {move || {
            if !open.get() {
                return ().into_any();
            }
            let tx = tx.clone();
            let buttons: Vec<_> = options
                .iter()
                .cloned()
                .map(|opt| {
                    let ChoiceOption { id, label, .. } = opt;
                    let tx = tx.clone();
                    let header = label.clone();
                    view! {
                        <button
                            class="menu-item"
                            on:click=move |ev| {
                                ev.stop_propagation();
                                if let Some(tx) = tx.clone() {
                                    store.update(|s| s.pending_label = Some(header.clone()));
                                    let _ = tx.unbounded_send(ClientMessage::Submit {
                                        action: PlayerAction::ResolveInput {
                                            response: InputResponse::PickSingle(id),
                                        },
                                    });
                                }
                                open.set(false);
                            }
                        >
                            {label}
                        </button>
                    }
                })
                .collect();
            view! {
                <div
                    class="menu-backdrop"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        open.set(false);
                    }
                ></div>
                <div class="context-menu">{buttons}</div>
            }
            .into_any()
        }}
    }
}
```

- [ ] **Step 4: Run the headless test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web --test context_menu 2>&1 | tail -15`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/interaction.rs crates/web/tests/context_menu.rs
git commit -m "web: ContextMenu component (submits ResolveInput, dismiss-on-backdrop)"
```

---

### Task 3: wire location nodes + provide the signal + CSS + headless test

**Files:**
- Modify: `crates/web/src/app.rs` (provide `PendingOptions`)
- Modify: `crates/web/src/map.rs:136-181,229-237` (glow class + `on:click` + `ContextMenu` child)
- Modify: `crates/web/style.css` (glow + menu CSS)
- Modify: `crates/web/tests/map.rs` (headless glow/menu/submit test)

**Interfaces:**
- Consumes: `interaction::{pending_options, options_for, PendingOptions, ContextMenu}` (Tasks 1–2); `provide_store` returning `StoreSignal`.

- [ ] **Step 1: Write the failing headless test** — append to `crates/web/tests/map.rs`

First add these imports at the top of `crates/web/tests/map.rs` (alongside the existing ones):

```rust
use futures::channel::mpsc;
use game_core::state::CardCode;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use game_core::test_support::fixtures::awaiting_pick_single_with;
use leptos::prelude::Signal;
use protocol::ClientMessage;
use web::transport::OutboundTx;
```

Then append the mount helper + tests:

```rust
/// Mount `BoardView` with a store, an outbound channel, and a `PendingOptions`
/// signal derived from the store (as `app.rs` does), then feed one `Hello`
/// carrying `state` + `outcome`. Returns the submitted-frame receiver.
async fn mount_interactive(
    state: game_core::state::GameState,
    outcome: EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    game_core::test_support::install_test_registry();
    let store = RwSignal::new(ClientState::default());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(|s| web::interaction::pending_options(s)));
        provide_context(web::interaction::PendingOptions(pending));
        leptos::view! { <BoardView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome,
                events: Vec::new(),
            },
        );
    });
    leptos::task::tick().await;
    rx
}

/// A one-location ("Study", id 10) game with investigator 1 standing on it.
fn study_game() -> game_core::state::GameState {
    let mut loc = test_location(10, "Study");
    loc.revealed = true;
    loc.code = CardCode::new("01111");
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut game = GameStateBuilder::new()
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .build();
    game.locations.insert(LocationId(10), loc);
    game
}

/// The class attribute of the last-mounted map node named `loc_name`.
fn node_class(loc_name: &str) -> String {
    let maps = document().query_selector_all(".map").expect("query");
    let last = maps
        .item(maps.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("last .map");
    let sel = format!(".map-location[data-loc=\"{loc_name}\"]");
    last.query_selector(&sel)
        .expect("query")
        .and_then(|el| el.get_attribute("class"))
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn actionable_location_glows_opens_menu_and_submits() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Investigate",
            OptionTarget::Location(LocationId(10)),
        )],
    );
    let mut rx = mount_interactive(study_game(), outcome).await;

    // Glows.
    assert!(node_class("Study").contains("actionable"), "node has the actionable class");

    // Clicking the node opens its menu.
    let maps = document().query_selector_all(".map").expect("query");
    let last = maps
        .item(maps.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("last .map");
    last.query_selector(".map-location[data-loc=\"Study\"]")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;

    let item = last
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item rendered");
    assert_eq!(item.text_content().unwrap_or_default(), "Investigate");

    // Clicking the item submits the anchored option.
    item.click();
    leptos::task::tick().await;
    let msg = rx.try_next().expect("a frame was sent").expect("frame present");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
    }
}

#[wasm_bindgen_test]
async fn location_without_a_matching_option_is_not_actionable() {
    // The only option anchors to a DIFFERENT location — the Study node stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Investigate",
            OptionTarget::Location(LocationId(11)),
        )],
    );
    let _ = mount_interactive(study_game(), outcome).await;
    assert!(!node_class("Study").contains("actionable"));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test map 2>&1 | tail -15`
Expected: the two new tests FAIL (node lacks the `actionable` class / no `.context-menu`); the pre-existing map tests still pass.

- [ ] **Step 3: Provide `PendingOptions` in `app.rs`** — replace the body's top of `crates/web/src/app.rs`

```rust
#[component]
pub fn App() -> impl IntoView {
    let store = provide_store();

    // Derive the live prompt's options and expose them so board entities can
    // route each option to itself and open a context menu (#536).
    let pending = Signal::derive(move || store.with(|s| crate::interaction::pending_options(s)));
    provide_context(crate::interaction::PendingOptions(pending));

    // Spawn the browser transport only on wasm; native/headless-reducer
    // builds render from a signal that tests drive directly.
    #[cfg(target_arch = "wasm32")]
    {
        crate::transport::start(store);
    }

    view! {
```

(The `use crate::store::provide_store;` import already exists; `provide_store` returns the `StoreSignal`. Leave the rest of the `view!` unchanged. `Signal`, `provide_context`, and the `.with` method come from the existing `use leptos::prelude::*;`.)

- [ ] **Step 4: Wire the map nodes** — in `crates/web/src/map.rs`, after `let positions = layout_positions(&locs);` (around line 142) add:

```rust
    // The live prompt's options, for glow + per-node context menus (#536).
    // Absent (native / no prompt) → empty → no node is actionable.
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
```

Then replace the node-class block (currently):

```rust
            let node_class = if loc.revealed {
                "map-location"
            } else {
                "map-location unrevealed"
            };
```

with:

```rust
            let menu_opts =
                crate::interaction::options_for(&pending, game_core::OptionTarget::Location(loc.id));
            let actionable = !menu_opts.is_empty();
            let open = RwSignal::new(false);
            let base = if loc.revealed {
                "map-location"
            } else {
                "map-location unrevealed"
            };
            let node_class = if actionable {
                format!("{base} actionable")
            } else {
                base.to_string()
            };
```

Then replace the node `view!` (currently `<div class=node_class data-loc=loc.name.clone() style=style> … </div>`) with:

```rust
            view! {
                <div
                    class=node_class
                    data-loc=loc.name.clone()
                    style=style
                    on:click=move |_| {
                        if actionable {
                            open.update(|o| *o = !*o);
                        }
                    }
                >
                    {detail}
                    {unrevealed_head}
                    <span class="loc-revealed">{revealed_label}</span>
                    {invs}
                    {enemies}
                    {
                        // wasm-only: ContextMenu submits via the wasm-only OutboundTx.
                        // On host the block is empty (`menu_opts` is still used above
                        // by `actionable`, so no unused-variable warning).
                        #[cfg(target_arch = "wasm32")]
                        actionable.then(|| view! {
                            <crate::interaction::ContextMenu options=menu_opts open=open/>
                        })
                    }
                </div>
            }
```

- [ ] **Step 5: Add the CSS** — append to `crates/web/style.css`

```css
/* Interactivity S1 (#536): actionable-entity glow + context menu. */
.map-location.actionable { box-shadow: 0 0 0 2px #e0b84c; cursor: pointer; }
.menu-backdrop { position: fixed; inset: 0; z-index: 15; }
.context-menu {
    position: absolute; top: 4px; right: 4px; z-index: 20;
    display: flex; flex-direction: column; gap: 2px;
    background: #1b1b1b; border: 1px solid #666; padding: 4px;
}
.context-menu .menu-item { display: block; width: 100%; text-align: left; cursor: pointer; }
```

- [ ] **Step 6: Run the map tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web --test map 2>&1 | tail -15`
Expected: all map tests pass, including `actionable_location_glows_opens_menu_and_submits` and `location_without_a_matching_option_is_not_actionable`.

- [ ] **Step 7: Verify the host build is warning-clean** (the `menu_opts`/`open`/cfg interplay)

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings 2>&1 | tail -8`
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/web/src/app.rs crates/web/src/map.rs crates/web/style.css crates/web/tests/map.rs
git commit -m "web: location nodes glow and open a context menu of their actions"
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

Watch for: an unused-import warning in `tests/map.rs` if a newly-added import isn't exercised (all listed imports are used by the new tests); the host `web` clippy job exercises the `map.rs` cfg block (the empty-on-host menu child).

## PR flow (after the gauntlet is green)

1. Push `ui/interactivity-location-menus`; open the PR with the template. Body: the goal + `Closes #536.`
2. `gh pr checks <PR#> --watch`.
3. **Phase-doc update is the final commit on the branch, pushed only after CI is green** (tick #536 in the #206 umbrella checklist / note S1 shipped) — then wait for CI green again, then merge on approval.

## Self-review notes

- **Spec coverage:** `interaction` module w/ `pending_options` + `options_for` + `PendingOptions` ✅ (Task 1); `ContextMenu` (backdrop dismiss, submit, single-option-still-opens) ✅ (Task 2); `app.rs` provision ✅, map node glow + `on:click` + menu child ✅, CSS ✅ (Task 3); flat bar untouched ✅ (no `input.rs` edit); no banner ✅; no engine change ✅ (only a `test_support` fixture, which is test infra); native tests for pure fns ✅ (Task 1), headless for `ContextMenu` ✅ (Task 2) and the node end-to-end ✅ (Task 3).
- **wasm-gating:** only `ContextMenu` and the map menu-child are `#[cfg(target_arch = "wasm32")]`; `pending_options`/`options_for`/`PendingOptions` are non-gated so host compiles + native-tests. `menu_opts` is used on host by `actionable = !menu_opts.is_empty()`, so no unused-variable warning when the menu child is cfg'd out.
- **Type consistency:** `PendingOptions(Signal<Vec<ChoiceOption>>)`, `options_for(&[ChoiceOption], OptionTarget) -> Vec<ChoiceOption>`, `ContextMenu { options: Vec<ChoiceOption>, open: RwSignal<bool> }`, `awaiting_pick_single_with(prompt, Vec<ChoiceOption>)` used identically across tasks and tests. Submit path is `ClientMessage::Submit { action: PlayerAction::ResolveInput { response: InputResponse::PickSingle(id) } }` in both `ContextMenu` and the tests.
