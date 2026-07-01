# Interactivity S2 — enemy context menus + fixed-at-cursor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Engaged enemies glow and open a cursor-anchored context menu of their Fight/Evade actions; the shared `ContextMenu` moves to `position: fixed` at the click coords so it escapes every `overflow`/positioning ancestor (resolving S1's clipping TODO) and behaves identically for all entity types.

**Architecture:** Refactor `ContextMenu.open` from `RwSignal<bool>` to `RwSignal<Option<(i32,i32)>>` (coords) and render `.context-menu` `position: fixed`. A new wasm-only `interaction::menu_layer(options, open)` renders a transparent `.menu-hit` click-capture layer (reads `ev.client_x()/client_y()`) + the `ContextMenu`, DRYing the trigger. `map.rs` (S1) and `enemy_card.rs` (S2) embed `menu_layer`; the `actionable` glow class + anchor `position:relative` stay non-gated.

**Tech Stack:** Rust, Leptos 0.8 (CSR/wasm), `web_sys::MouseEvent` (wasm-only), `wasm-bindgen-test` headless Firefox.

## Global Constraints

- Issue: **#537** (interactivity S2). Umbrella: **#206**. Design: `docs/superpowers/specs/2026-07-01-interactivity-s2-enemy-menus-design.md`.
- Branch: `ui/interactivity-enemy-menus` (created; S2 spec already committed on it). Commit scope: `web:`.
- **No `input.rs` / action-bar change; no prompt banner; no engine change** (S2 is web-only).
- Reading click coords needs `web_sys` → `menu_layer` + `ContextMenu` submit stay `#[cfg(target_arch = "wasm32")]`; the `actionable` class + `position:relative` are non-gated. The per-entity `open` signal is **wasm-gated** (its only references are in the wasm-only child — a non-gated `let open` would be an unused-variable warning on host).
- Match CI's strict flags before pushing (all seven jobs). Merge only after approval.

---

### Task 1: `ContextMenu` → fixed-at-cursor + `menu_layer` + migrate the S1 map

**Files:**
- Modify: `crates/web/Cargo.toml` (add `MouseEvent` to the wasm `web-sys` features)
- Modify: `crates/web/src/interaction.rs` (`ContextMenu.open` type + coords; add `menu_layer`)
- Modify: `crates/web/src/map.rs` (adopt `Option<coords>` open + `menu_layer`; drop the node `on:click`)
- Modify: `crates/web/style.css` (`.context-menu` → fixed; add `.menu-hit`, `.card.actionable`)
- Modify: `crates/web/tests/context_menu.rs` (new `open` type)
- Modify: `crates/web/tests/map.rs` (click `.menu-hit`; new `open` type)

**Interfaces:**
- Consumes: `pending_options` / `options_for` / `PendingOptions` (S1).
- Produces:
  - `ContextMenu` props: `options: Vec<ChoiceOption>`, `open: RwSignal<Option<(i32,i32)>>` (wasm-only).
  - `interaction::menu_layer(options: Vec<ChoiceOption>, open: RwSignal<Option<(i32,i32)>>) -> impl IntoView` (wasm-only).

- [ ] **Step 1: Add the `MouseEvent` web-sys feature** — in `crates/web/Cargo.toml`, the `[target.'cfg(target_arch = "wasm32")'.dependencies]` `web-sys` line, add `"MouseEvent"`:

```toml
web-sys = { version = "0.3", features = ["Storage", "Location", "Window", "Element", "HtmlElement", "HtmlDivElement", "MouseEvent"] }
```

- [ ] **Step 2: Update the `ContextMenu` direct test to the new `open` type** — in `crates/web/tests/context_menu.rs`, replace the `mount` helper's `open` construction and the final assertion:

Replace `let open = RwSignal::new(open_initial);` with:

```rust
    let open = RwSignal::new(if open_initial { Some((0, 0)) } else { None });
```

Change the `mount` signature return + call sites stay `(RwSignal<Option<(i32,i32)>>, _)` — update the fn signature:

```rust
async fn mount(open_initial: bool) -> (RwSignal<Option<(i32, i32)>>, mpsc::UnboundedReceiver<ClientMessage>) {
```

Replace the final assertion in `clicking_an_item_submits_pick_single_and_closes`:

```rust
    assert!(open.get_untracked().is_none(), "menu closes after a selection");
```

- [ ] **Step 3: Run the context_menu test to verify it fails to compile** (old `ContextMenu` takes `RwSignal<bool>`)

Run: `cargo build -p web --target wasm32-unknown-unknown --tests --test context_menu 2>&1 | tail -6`
Expected: type-mismatch error on `open` (`Option<(i32,i32)>` vs `bool`).

- [ ] **Step 4: Refactor `ContextMenu` + add `menu_layer`** — in `crates/web/src/interaction.rs`, replace the whole `#[cfg(target_arch = "wasm32")] #[leptos::component] pub fn ContextMenu(...) { ... }` block with:

```rust
/// A popover of a board entity's offered options (#536, #537). When `open` is
/// `Some((x, y))`, renders a full-screen transparent `.menu-backdrop` (click →
/// close) and a `.context-menu` positioned `fixed` at viewport coords `(x, y)`
/// (so it escapes any `overflow`/positioning ancestor); a click submits
/// `ResolveInput(PickSingle(id))` and closes. wasm-only — submits via the
/// wasm-only `OutboundTx`.
#[cfg(target_arch = "wasm32")]
#[leptos::component]
pub fn ContextMenu(
    options: Vec<ChoiceOption>,
    open: leptos::prelude::RwSignal<Option<(i32, i32)>>,
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
            let Some((x, y)) = open.get() else {
                return ().into_any();
            };
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
                                open.set(None);
                            }
                        >
                            {label}
                        </button>
                    }
                })
                .collect();
            let style = format!("left:{x}px;top:{y}px;");
            view! {
                <div
                    class="menu-backdrop"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        open.set(None);
                    }
                ></div>
                <div class="context-menu" style=style>{buttons}</div>
            }
            .into_any()
        }}
    }
}

/// The interactive trigger for a board entity's context menu (#537), wasm-only.
/// A transparent hit-layer covering the anchor captures the open click's viewport
/// coords into `open`; the [`ContextMenu`] renders there. Embedded by each entity
/// under `#[cfg(target_arch = "wasm32")]` so no `web_sys` touches the host build;
/// the anchor supplies the `actionable` glow class + `position: relative`.
#[cfg(target_arch = "wasm32")]
pub fn menu_layer(
    options: Vec<ChoiceOption>,
    open: leptos::prelude::RwSignal<Option<(i32, i32)>>,
) -> impl leptos::prelude::IntoView {
    use leptos::prelude::*;
    view! {
        <div
            class="menu-hit"
            on:click=move |ev: web_sys::MouseEvent| {
                open.set(Some((ev.client_x(), ev.client_y())));
            }
        ></div>
        <ContextMenu options=options open=open/>
    }
}
```

- [ ] **Step 5: Migrate the map node** — in `crates/web/src/map.rs`, replace the `let open = RwSignal::new(false);` line (in the node closure) with a wasm-gated coords signal:

```rust
            #[cfg(target_arch = "wasm32")]
            let open = RwSignal::new(None::<(i32, i32)>);
```

Then replace the node `view!` (the `<div class=node_class ... on:click=... > ... </div>`) with the `on:click` **removed** and the menu child using `menu_layer`:

```rust
            view! {
                <div class=node_class data-loc=loc.name.clone() style=style>
                    {detail}
                    {unrevealed_head}
                    <span class="loc-revealed">{revealed_label}</span>
                    {invs}
                    {enemies}
                    {
                        // wasm-only: the menu trigger + menu read/submit via web_sys /
                        // the wasm-only OutboundTx. On host the block is empty; `menu_opts`
                        // is still used above by `actionable`, so no unused-var warning.
                        #[cfg(target_arch = "wasm32")]
                        actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
                    }
                </div>
            }
```

- [ ] **Step 6: Update the CSS** — in `crates/web/style.css`, change the `.context-menu` rule from `position: absolute; top: 4px; right: 4px;` to `position: fixed;` (coords come inline), and add the hit-layer + shared card glow. Replace the S1 interactivity block:

```css
/* Interactivity S1/S2 (#536/#537): actionable glow + cursor-anchored menu. */
.map-location.actionable { box-shadow: 0 0 0 2px #e0b84c; cursor: pointer; }
.card.actionable { position: relative; box-shadow: 0 0 0 2px #e0b84c; cursor: pointer; }
.menu-hit { position: absolute; inset: 0; z-index: 5; }
.menu-backdrop { position: fixed; inset: 0; z-index: 15; }
.context-menu {
    position: fixed; z-index: 20;
    display: flex; flex-direction: column; gap: 2px;
    background: #1b1b1b; border: 1px solid #666; padding: 4px;
}
.context-menu .menu-item { display: block; width: 100%; text-align: left; cursor: pointer; }
```

- [ ] **Step 7: Update the map headless test to click `.menu-hit`** — in `crates/web/tests/map.rs`, in `actionable_location_glows_opens_menu_and_submits`, replace the node-open click (the block that clicks `.map-location[data-loc="Study"]`) with a click on its `.menu-hit`:

```rust
    // Clicking the node's hit-layer opens its menu (events bubble up, so the
    // hit-layer — not the node — carries the open handler).
    last.query_selector(".map-location[data-loc=\"Study\"] .menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit layer")
        .click();
    leptos::task::tick().await;
```

(The rest — asserting `.context-menu .menu-item` text and clicking it to submit — is unchanged; `.click()` synthesizes coords `(0,0)`, so `open` becomes `Some((0,0))` and the menu renders.)

- [ ] **Step 8: Run the shared refactor's tests** (context_menu + map headless)

Run: `wasm-pack test --headless --firefox crates/web --test context_menu --test map 2>&1 | tail -20`
Expected: `context_menu` 3/3 and `map` 11/11 pass.

- [ ] **Step 9: Verify host build is warning-clean** (the wasm-gated `open` + cfg menu child)

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings 2>&1 | tail -5`
Expected: no warnings (host: `open` absent, `menu_opts` used by `actionable`).

- [ ] **Step 10: Commit**

```bash
git add crates/web/Cargo.toml crates/web/src/interaction.rs crates/web/src/map.rs \
        crates/web/style.css crates/web/tests/context_menu.rs crates/web/tests/map.rs
git commit -m "web: ContextMenu opens fixed at the cursor via a shared menu_layer"
```

---

### Task 2: `EnemyCard` context menus

**Files:**
- Modify: `crates/web/src/enemy_card.rs` (glow + `menu_layer` when the enemy has offered actions)
- Modify: `crates/web/tests/enemy_card.rs` (headless glow/menu/submit test)

**Interfaces:**
- Consumes: `menu_layer` / `options_for` / `PendingOptions` (Task 1 / S1); `OptionTarget::Enemy`.

- [ ] **Step 1: Write the failing headless test** — append to `crates/web/tests/enemy_card.rs`

First extend the imports at the top:

```rust
use futures::channel::mpsc;
use game_core::state::EnemyId;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use game_core::test_support::fixtures::awaiting_pick_single_with;
use protocol::ClientMessage;
use web::interaction::{pending_options, PendingOptions};
use web::store::ClientState;
use web::transport::OutboundTx;
```

Then append:

```rust
/// Mount an `EnemyCard` with a store-derived `PendingOptions` signal + an
/// outbound channel, set the store's `outcome` directly (`pending_options` reads
/// `outcome`, not `game`, so no `GameState` is needed), and return the
/// submitted-frame receiver.
async fn mount_enemy(
    enemy: game_core::state::Enemy,
    outcome: game_core::EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(pending_options));
        provide_context(PendingOptions(pending));
        view! { <EnemyCard enemy=enemy.clone()/> }
    });
    store.update(|s| s.outcome = Some(outcome));
    leptos::task::tick().await;
    rx
}

#[wasm_bindgen_test]
async fn actionable_enemy_glows_opens_menu_and_submits() {
    let e = test_enemy(7, "Ghoul");
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Fight", OptionTarget::Enemy(EnemyId(7)))],
    );
    let mut rx = mount_enemy(e, outcome).await;

    let card = last_card();
    assert!(card.class_name().contains("actionable"), "enemy card glows");

    card.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit layer")
        .click();
    leptos::task::tick().await;

    let item = card
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Fight");
    item.click();
    leptos::task::tick().await;

    let msg = rx.try_recv().expect("a frame was sent after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn enemy_without_a_matching_option_is_inert() {
    let e = test_enemy(7, "Ghoul");
    // Option anchors to a different enemy → this card stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Fight", OptionTarget::Enemy(EnemyId(8)))],
    );
    let _ = mount_enemy(e, outcome).await;
    assert!(!last_card().class_name().contains("actionable"));
}
```

(`Signal`, `RwSignal`, `provide_context`, `.with`, `Get`/`Update` all come from the file's existing `use leptos::prelude::*;`. `last_card()` and `test_enemy` are already defined/imported in the file.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo build -p web --target wasm32-unknown-unknown --tests --test enemy_card 2>&1 | tail -8`
Expected: compile error / the card lacks `actionable` + `.menu-hit` (EnemyCard has no interactivity yet).

- [ ] **Step 3: Wire `EnemyCard`** — in `crates/web/src/enemy_card.rs`, inside `EnemyCard`, after `let exhausted = enemy.exhausted;` add the routing:

```rust
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts =
        crate::interaction::options_for(&pending, game_core::OptionTarget::Enemy(enemy.id));
    let actionable = !menu_opts.is_empty();
    #[cfg(target_arch = "wasm32")]
    let open = RwSignal::new(None::<(i32, i32)>);
```

Replace the `root_class` block:

```rust
    let root_class = if exhausted {
        "card card--enemy card--exhausted"
    } else {
        "card card--enemy"
    };
```

with a dynamic `String`:

```rust
    let mut root_class = String::from("card card--enemy");
    if exhausted {
        root_class.push_str(" card--exhausted");
    }
    if actionable {
        root_class.push_str(" actionable");
    }
```

Then add the wasm-only menu child inside the root `view!` — replace the closing `</div>` of the root card with the menu child before it:

```rust
            <div class="card-footer enemy-stats">
                {stat_views}
                {keyword_views}
            </div>
            {
                // wasm-only trigger + menu (web_sys / OutboundTx); host: empty,
                // `menu_opts` used above by `actionable` (no unused-var warning).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
            }
        </div>
```

- [ ] **Step 4: Run the enemy_card test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web --test enemy_card 2>&1 | tail -12`
Expected: all `enemy_card` tests pass (the 2 display tests + the 2 new interactivity tests).

- [ ] **Step 5: Verify host build warning-clean**

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings 2>&1 | tail -5`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/web/src/enemy_card.rs crates/web/tests/enemy_card.rs
git commit -m "web: engaged enemies glow and open a Fight/Evade context menu"
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

Watch for: a `doc_lazy_continuation` clippy lint if a new doc comment has a line starting with `+`/`-` (reword); the host `web` clippy job exercises the wasm-gated `open` (must be absent-not-unused on host).

## PR flow (after the gauntlet is green)

1. Push `ui/interactivity-enemy-menus`; open the PR. Body: goal + `Closes #537.`
2. `gh pr checks <PR#> --watch`.
3. **Phase-doc update is the final commit, pushed only after CI is green** (record S2 in the phase-7 doc; tick #537 in the #206 checklist after merge) — then wait for CI green again, merge on approval.

## Self-review notes

- **Spec coverage:** `ContextMenu` fixed-at-cursor + `open: Option<coords>` ✅ (T1); `menu_layer` shared trigger ✅ (T1); S1 map migration + TODO(#206) resolved ✅ (T1); `EnemyCard` glow + menu ✅ (T2); CSS `.card.actionable`/`.menu-hit`/`.context-menu` fixed ✅ (T1); `web-sys MouseEvent` feature ✅ (T1); scope (EnemyCard = engaged enemies; map-token enemies deferred) honored — no map-token wiring added; flat bar / no banner / no engine change ✅.
- **Testing:** `ContextMenu` direct test migrated to `Option<coords>` ✅ (T1 S2); map regression migrated to `.menu-hit` ✅ (T1 S7); enemy glow/menu/submit + inert ✅ (T2); display tests untouched ✅.
- **Type consistency:** `open: RwSignal<Option<(i32,i32)>>` everywhere (`ContextMenu`, `menu_layer`, map, enemy, both tests); `menu_layer(Vec<ChoiceOption>, RwSignal<Option<(i32,i32)>>)`; submit path `ClientMessage::Submit { action: PlayerAction::ResolveInput { response: InputResponse::PickSingle(id) } }` in `ContextMenu` + both tests. `options_for(&pending, OptionTarget::Enemy(enemy.id))` matches the `Enemy(EnemyId)` variant.
