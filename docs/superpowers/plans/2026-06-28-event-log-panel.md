# Event Log Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a read-only event-log panel left of the board that accumulates the full game's events, newest at the bottom, each batch headed by the menu-choice label the player submitted.

**Architecture:** Client-only slice. The store accumulates a `Vec<LogBatch>` and holds a one-shot `pending_label`; the input view sets `pending_label` (from the chosen option's label) at submit time; the reducer consumes it into the next `Applied` batch. A new `EventLogView` renders the log; a wasm-only effect auto-scrolls to the bottom. No protocol or server change.

**Tech Stack:** Rust, Leptos 0.8 (CSR), `web-sys`, `wasm-bindgen-test`. Spec: `docs/superpowers/specs/2026-06-28-event-log-panel-design.md` (issue #505).

## Global Constraints

- Match CI's strict flags before declaring done: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, `cargo build -p web --target wasm32-unknown-unknown`, `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`, and `wasm-pack test --headless --firefox crates/web`.
- `store.rs` and the new `event_log.rs` compile on **both** native and wasm; `input.rs` is wasm-only (`#[cfg(target_arch = "wasm32")] pub mod input;`). Keep all `web_sys`/DOM calls behind `#[cfg(target_arch = "wasm32")]`.
- Raw `Debug` (`{:?}`) for event bodies. No card-name enrichment in event lines.
- Commit after each task (one logical change per commit), scope `web:`.

---

### Task 1: Store accumulates the event log

**Files:**
- Modify: `crates/web/src/store.rs`

**Interfaces:**
- Produces: `pub struct LogBatch { pub header: String, pub events: Vec<game_core::Event> }`; new `ClientState` fields `pub log: Vec<LogBatch>` and `pub pending_label: Option<String>`.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `crates/web/src/store.rs`:

```rust
#[test]
fn applied_pushes_a_log_batch_using_pending_label() {
    let mut s = ClientState {
        pending_label: Some("Move to Cellar".into()),
        ..Default::default()
    };
    reduce(
        &mut s,
        ServerMessage::Applied {
            state: Box::new(sample_state()),
            events: vec![game_core::Event::ScenarioStarted],
            outcome: EngineOutcome::Done,
        },
    );
    assert_eq!(s.log.len(), 1);
    assert_eq!(s.log[0].header, "Move to Cellar");
    assert_eq!(s.log[0].events, vec![game_core::Event::ScenarioStarted]);
    assert_eq!(s.pending_label, None, "pending_label is consumed");
}

#[test]
fn applied_without_pending_label_uses_generic_header() {
    let mut s = ClientState::default();
    reduce(
        &mut s,
        ServerMessage::Applied {
            state: Box::new(sample_state()),
            events: Vec::new(),
            outcome: EngineOutcome::Done,
        },
    );
    assert_eq!(s.log.len(), 1);
    assert_eq!(s.log[0].header, "(action)");
}

#[test]
fn consecutive_applied_accumulate_in_order() {
    let mut s = ClientState::default();
    for label in ["first", "second"] {
        s.pending_label = Some(label.to_string());
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: Vec::new(),
                outcome: EngineOutcome::Done,
            },
        );
    }
    let headers: Vec<&str> = s.log.iter().map(|b| b.header.as_str()).collect();
    assert_eq!(headers, vec!["first", "second"]);
}

#[test]
fn rejected_clears_pending_label_without_pushing_a_batch() {
    let mut s = ClientState {
        pending_label: Some("Move to Cellar".into()),
        ..Default::default()
    };
    reduce(&mut s, ServerMessage::Rejected { reason: "nope".into() });
    assert!(s.log.is_empty(), "rejection pushes no batch");
    assert_eq!(s.pending_label, None, "rejection clears the stale label");
}

#[test]
fn hello_clears_log_and_pending_label() {
    let mut s = ClientState {
        pending_label: Some("stale".into()),
        ..Default::default()
    };
    s.log.push(LogBatch { header: "old".into(), events: Vec::new() });
    reduce(
        &mut s,
        ServerMessage::Hello {
            state: Box::new(sample_state()),
            outcome: EngineOutcome::Done,
        },
    );
    assert!(s.log.is_empty());
    assert_eq!(s.pending_label, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p web --lib store:: 2>&1 | tail -20`
Expected: FAIL to compile — `LogBatch` / `log` / `pending_label` do not exist.

- [ ] **Step 3: Add the `LogBatch` type and fields**

In `crates/web/src/store.rs`, after the `ConnStatus` enum, add:

```rust
/// One applied submit's worth of events, for the event-log view (#505).
#[derive(Debug, Clone, PartialEq)]
pub struct LogBatch {
    /// Human label of the menu choice that produced this batch
    /// (e.g. "Play 01059 from hand"); a generic fallback when unknown.
    pub header: String,
    /// The events emitted by that submit, in order.
    pub events: Vec<game_core::Event>,
}
```

Add two fields to `ClientState` (after `last_skill_test_difficulty`):

```rust
    /// Full accumulated event history, grouped per applied submit, oldest
    /// first. Cleared by `Hello`. The event-log panel (#505) renders this.
    pub log: Vec<LogBatch>,
    /// Header label for the *next* `Applied` batch, set by the input view at
    /// submit time and taken when that batch arrives. Cleared on `Rejected`
    /// (the submit produced no batch) and `Hello`.
    pub pending_label: Option<String>,
```

- [ ] **Step 4: Update the reducer**

In `reduce`, the `Hello` arm — add after `state.last_skill_test_difficulty = None;`:

```rust
            state.log = Vec::new();
            state.pending_label = None;
```

In the `Applied` arm — replace the final `state.last_events = events;` with:

```rust
            let header = state.pending_label.take().unwrap_or_else(|| "(action)".into());
            state.log.push(LogBatch { header, events: events.clone() });
            state.last_events = events;
```

In the `Rejected` arm — add after `state.last_rejection = Some(reason);`:

```rust
            state.pending_label = None;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p web --lib store:: 2>&1 | tail -20`
Expected: PASS (all store tests, including the pre-existing ones).

- [ ] **Step 6: Commit**

```bash
git add crates/web/src/store.rs
git commit -m "web: accumulate event log + pending header in the client store (#505)"
```

---

### Task 2: `response_label` helper + `event_log` module

**Files:**
- Create: `crates/web/src/event_log.rs`
- Modify: `crates/web/src/lib.rs`

**Interfaces:**
- Consumes: `game_core::{InputRequest, InputResponse, OptionId}`, `game_core::engine::OptionId` (re-exported as `game_core::OptionId`).
- Produces: `pub(crate) fn response_label(request: &game_core::InputRequest, response: &game_core::InputResponse) -> String`.

- [ ] **Step 1: Create the module with a failing test**

Create `crates/web/src/event_log.rs`:

```rust
//! Event-log panel (#505): a read-only, accumulating view of the game's events,
//! left of the board, newest at the bottom, grouped per submitted action.

use game_core::{InputRequest, InputResponse};

/// The event-log header for a submitted response, given the prompt it answered.
///
/// - `PickSingle(id)` → that option's `label` (fallback `"Pick <n>"` if absent).
/// - `Confirm`        → `"Confirm"`.
/// - `Skip`           → `"Skip"`.
/// - `PickMultiple`   → `"Commit <n> card(s)"`.
pub(crate) fn response_label(request: &InputRequest, response: &InputResponse) -> String {
    match response {
        InputResponse::PickSingle(id) => request
            .options
            .iter()
            .find(|o| o.id == *id)
            .map(|o| o.label.clone())
            .unwrap_or_else(|| format!("Pick {}", id.0)),
        InputResponse::Confirm => "Confirm".to_string(),
        InputResponse::Skip => "Skip".to_string(),
        InputResponse::PickMultiple { selected } => {
            format!("Commit {} card(s)", selected.len())
        }
        // `InputResponse` is `#[non_exhaustive]`; a future variant gets a generic
        // header rather than failing to compile.
        _ => "(action)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::engine::OptionId;
    use game_core::{ChoiceOption, InputKind};

    fn request_with_options(opts: Vec<(u32, &str)>) -> InputRequest {
        InputRequest {
            prompt: "choose".into(),
            options: opts
                .into_iter()
                .map(|(i, l)| ChoiceOption { id: OptionId(i), label: l.to_string() })
                .collect(),
            kind: InputKind::PickSingle,
            skippable: false,
        }
    }

    #[test]
    fn pick_single_uses_the_chosen_option_label() {
        let req = request_with_options(vec![(0, "Move to Cellar"), (1, "Play 01059 from hand")]);
        let label = response_label(&req, &InputResponse::PickSingle(OptionId(1)));
        assert_eq!(label, "Play 01059 from hand");
    }

    #[test]
    fn pick_single_unknown_id_falls_back() {
        let req = request_with_options(vec![(0, "Move to Cellar")]);
        let label = response_label(&req, &InputResponse::PickSingle(OptionId(7)));
        assert_eq!(label, "Pick 7");
    }

    #[test]
    fn confirm_skip_and_commit_have_fixed_labels() {
        let req = request_with_options(vec![]);
        assert_eq!(response_label(&req, &InputResponse::Confirm), "Confirm");
        assert_eq!(response_label(&req, &InputResponse::Skip), "Skip");
        assert_eq!(
            response_label(&req, &InputResponse::PickMultiple { selected: vec![OptionId(0), OptionId(1)] }),
            "Commit 2 card(s)"
        );
    }
}
```

- [ ] **Step 2: Declare the module**

In `crates/web/src/lib.rs`, add to the both-targets block (after `pub mod board;`):

```rust
pub mod event_log;
```

- [ ] **Step 3: Run tests to verify they fail, then pass**

Run: `cargo test -p web --lib event_log:: 2>&1 | tail -20`
Expected: PASS (the module is written with its implementation; if `InputRequest`/`InputResponse`/`OptionId` import paths are wrong the compile error names the right path — fix the `use` and re-run). If `InputResponse` is NOT `#[non_exhaustive]`, clippy may flag the `_` arm as unreachable; in that case remove the `_ => ...` arm.

Note: verify the field path for `OptionId`'s inner value — the tests use `OptionId(7)` and `id.0`. If `OptionId` is a named-field struct rather than a tuple, adjust `id.0` and the constructors to match (grep `pub struct OptionId` in `crates/game-core/src/engine/outcome.rs`).

- [ ] **Step 4: Commit**

```bash
git add crates/web/src/event_log.rs crates/web/src/lib.rs
git commit -m "web: response_label helper + event_log module (#505)"
```

---

### Task 3: `EventLogView` component, layout, and styles

**Files:**
- Modify: `crates/web/src/event_log.rs` (add the component)
- Modify: `crates/web/src/app.rs` (layout)
- Modify: `crates/web/style.css` (styles)
- Modify: `crates/web/Cargo.toml` (web-sys `Element` features for the scroll effect)
- Create: `crates/web/tests/event_log.rs` (wasm render test)

**Interfaces:**
- Consumes: `crate::store::{use_store, LogBatch}`.
- Produces: `#[component] pub fn EventLogView() -> impl IntoView`.

- [ ] **Step 1: Write the failing wasm render test**

Create `crates/web/tests/event_log.rs`:

```rust
//! Headless render test for `EventLogView` (#505): seed the store with a couple
//! of `LogBatch`es and assert the panel renders each header and its events as
//! Debug text, oldest-first. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::event_log::EventLogView;
use web::store::{ClientState, LogBatch};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn renders_batches_with_headers_and_event_debug() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <EventLogView/> }
    });
    store.update(|s| {
        s.log.push(LogBatch {
            header: "Move to Cellar".into(),
            events: vec![game_core::Event::ScenarioStarted],
        });
    });
    leptos::task::tick().await;

    let logs = leptos::prelude::document()
        .query_selector_all(".event-log")
        .expect("query");
    let panel = logs
        .item(logs.length() - 1)
        .expect("an .event-log panel")
        .dyn_into::<web_sys::Element>()
        .expect("Element");
    let text = panel.text_content().unwrap_or_default();
    assert!(text.contains("Move to Cellar"), "header rendered: {text}");
    assert!(text.contains("ScenarioStarted"), "event Debug rendered: {text}");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test event_log 2>&1 | tail -20`
Expected: FAIL to compile — `EventLogView` does not exist yet.

- [ ] **Step 3: Add the `web-sys` Element features**

In `crates/web/Cargo.toml`, change the `[target.'cfg(target_arch = "wasm32")'.dependencies]` `web-sys` line (line ~26) to:

```toml
web-sys = { version = "0.3", features = ["Storage", "Location", "Window", "Element", "HtmlElement", "HtmlDivElement"] }
```

- [ ] **Step 4: Implement `EventLogView`**

Append to `crates/web/src/event_log.rs` (after `response_label`):

```rust
use leptos::prelude::*;

use crate::store::use_store;

/// Read-only event log, left of the board. Renders every accumulated `LogBatch`
/// oldest-first (newest at the bottom); a header line per batch then one Debug
/// line per event. On wasm, auto-scrolls to the bottom as the log grows.
#[component]
pub fn EventLogView() -> impl IntoView {
    let store = use_store();
    let scroll_ref = NodeRef::<leptos::html::Div>::new();

    // Auto-scroll to the newest line whenever the batch count changes.
    #[cfg(target_arch = "wasm32")]
    {
        Effect::new(move |_| {
            let _ = store.with(|s| s.log.len());
            if let Some(el) = scroll_ref.get() {
                el.set_scroll_top(el.scroll_height());
            }
        });
    }

    let batches = move || {
        store
            .get()
            .log
            .into_iter()
            .map(|batch| {
                let events: Vec<_> = batch
                    .events
                    .iter()
                    .map(|e| view! { <div class="log-event">{format!("{e:?}")}</div> })
                    .collect();
                view! {
                    <div class="log-batch">
                        <div class="log-action">{format!("▸ {}", batch.header)}</div>
                        {events}
                    </div>
                }
            })
            .collect::<Vec<_>>()
    };

    view! {
        <aside class="event-log">
            <h2>"Event log"</h2>
            <div class="log-scroll" node_ref=scroll_ref>
                {batches}
            </div>
        </aside>
    }
}
```

- [ ] **Step 5: Wire the layout in `app.rs`**

In `crates/web/src/app.rs`, replace the `<BoardView/>` line in the `view!` with a flex wrapper:

```rust
            <div class="layout">
                <crate::event_log::EventLogView/>
                <BoardView/>
            </div>
```

(Leave the `#[cfg(target_arch = "wasm32")]` picker/skill-test/input block below unchanged.)

- [ ] **Step 6: Add styles**

Append to `crates/web/style.css`:

```css
/* Event log panel (#505): fixed-width column left of the board, scrolls with
   the newest entry pinned to the bottom. Monospace so raw Debug lines align. */
.layout { display: flex; gap: 1rem; align-items: flex-start; }
.event-log { flex: 0 0 auto; width: 22rem; }
.event-log h2 { font-size: 1rem; margin: 0 0 0.25rem; }
.log-scroll { max-height: 80vh; overflow-y: auto; font-family: ui-monospace, monospace; font-size: 0.8rem; border: 1px solid #ccc; border-radius: 4px; padding: 0.5rem; }
.log-batch { border-top: 1px solid #eee; padding: 0.25rem 0; }
.log-batch:first-child { border-top: none; }
.log-action { font-weight: 600; color: #2a4d69; }
.log-event { white-space: pre-wrap; word-break: break-word; color: #333; }
```

- [ ] **Step 7: Run the wasm render test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web --test event_log 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 8: Verify native + wasm builds**

Run: `cargo build -p web 2>&1 | tail -5` (native — confirms `EventLogView` compiles off-wasm with the scroll effect gated out).
Run: `cargo build -p web --target wasm32-unknown-unknown 2>&1 | tail -5`
Expected: both succeed. If native fails on `NodeRef`/`leptos::html::Div`, that type is target-agnostic in leptos 0.8; re-read the error — most likely a missing `use leptos::prelude::*;` already covers it.

- [ ] **Step 9: Commit**

```bash
git add crates/web/src/event_log.rs crates/web/src/app.rs crates/web/style.css crates/web/Cargo.toml crates/web/tests/event_log.rs
git commit -m "web: EventLogView panel left of the board, auto-scrolled (#505)"
```

---

### Task 4: Input view captures the menu-choice label

**Files:**
- Modify: `crates/web/src/input.rs`
- Modify: `crates/web/tests/awaiting_input.rs` (assert the submit sets `pending_label`)

**Interfaces:**
- Consumes: `crate::event_log::response_label`, `crate::store::use_store`.

- [ ] **Step 1: Add a failing test that a PickSingle submit sets the header**

Open `crates/web/tests/awaiting_input.rs`. Its `mount` helper returns the outbound `rx` but also has access to the store via context. Add a test that reads the store's `pending_label` after a click. First, adjust `mount` to also return the store (find `async fn mount(` and change it to return the store handle alongside `rx`), OR add a sibling test that constructs its own mount. Add this self-contained test at the end of the file:

```rust
#[wasm_bindgen_test]
async fn picking_an_option_sets_the_pending_log_header() {
    use game_core::test_support::fixtures::awaiting_pick_single_input;

    let store = RwSignal::new(ClientState::default());
    let (tx, _rx) = mpsc::unbounded::<ClientMessage>();
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx.clone());
        leptos::view! { <AwaitingInputView/> }
    });
    // A PickSingle prompt whose first option has a known label.
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(base_game()),
                outcome: awaiting_pick_single_input(),
            },
        );
    });
    leptos::task::tick().await;

    // Click the first rendered option button.
    let section = last_section();
    let buttons = section.query_selector_all(".option").expect("options");
    buttons
        .item(0)
        .expect("an option button")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;

    let header = store.with_untracked(|s| s.pending_label.clone());
    assert!(
        header.is_some(),
        "clicking an option must set the event-log header"
    );
}
```

Note: confirm `awaiting_pick_single_input()` (already imported elsewhere in this file) yields options with a `.option` button; the existing tests in this file click `.option` buttons, so the selector is correct. If `ClientState` is not already imported in this test file, add `use web::store::ClientState;`.

- [ ] **Step 2: Run it to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test awaiting_input 2>&1 | tail -20`
Expected: FAIL — `pending_label` is `None` after the click (the input view doesn't set it yet).

- [ ] **Step 3: Set `pending_label` at each submit site**

In `crates/web/src/input.rs`, capture `store` for the closures and set the label before each send. The component already has `let store = use_store();` at the top (`RwSignal` is `Copy`, so it can be moved into closures freely).

**Skip button** (`skip_button` closure) — before the `tx.unbounded_send(... Skip ...)`:

```rust
                                    store.update(|s| s.pending_label = Some("Skip".to_string()));
                                    let _ = tx.unbounded_send(ClientMessage::Submit {
                                        action: PlayerAction::ResolveInput {
                                            response: InputResponse::Skip,
                                        },
                                    });
```

**PickSingle option button** — the chosen option's `label` is already destructured (`let ChoiceOption { id, label } = opt;`). Clone it for the closure and set it as the header before sending:

```rust
                            let header = label.clone();
                            // ...inside on:click, before the send:
                                        store.update(|s| s.pending_label = Some(header.clone()));
                                        let _ = tx.unbounded_send(ClientMessage::Submit {
                                            action: PlayerAction::ResolveInput {
                                                response: InputResponse::PickSingle(id),
                                            },
                                        });
```

**Confirm button** — before the send:

```rust
                                    store.update(|s| s.pending_label = Some("Confirm".to_string()));
                                    let _ = tx.unbounded_send(ClientMessage::Submit {
                                        action: PlayerAction::ResolveInput {
                                            response: InputResponse::Confirm,
                                        },
                                    });
```

**PickMultiple commit** (`on_commit`) — compute the label from the selection count before sending:

```rust
                        let header = format!("Commit {} card(s)", selected_ids.len());
                        store.update(|s| s.pending_label = Some(header));
                        let _ = tx.unbounded_send(ClientMessage::Submit {
                            action: PlayerAction::ResolveInput {
                                response: InputResponse::PickMultiple { selected: selected_ids },
                            },
                        });
```

These mirror `response_label`'s four arms (Skip/PickSingle-label/Confirm/`Commit N`); the PickSingle and PickMultiple sites use the value directly since it's already in scope, keeping the busy click closures free of the `request` borrow. (`response_label` remains the single tested source of truth for the same mapping and is what `store.rs`'s fallback documents.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web --test awaiting_input 2>&1 | tail -20`
Expected: PASS (the new test and all pre-existing ones).

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/input.rs crates/web/tests/awaiting_input.rs
git commit -m "web: input view sets the event-log header on submit (#505)"
```

---

### Task 5: Full gauntlet + manual check

**Files:** none (verification only).

- [ ] **Step 1: Run the full CI gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
wasm-pack test --headless --firefox crates/web
```

Expected: all green. Fix any clippy/fmt issues with follow-up edits and re-run.

- [ ] **Step 2: Manual smoke (optional but recommended)**

```bash
cd crates/web && trunk build && cd ../.. && cargo run -p server
# open http://localhost:8000, start a game, take a few actions:
# the left panel shows "▸ <menu choice>" headers with the events under each,
# newest at the bottom, auto-scrolled into view.
```

- [ ] **Step 3: Commit any gauntlet fixes**

```bash
git add -A && git commit -m "web: event-log gauntlet fixes (#505)"   # only if needed
```

---

## Self-Review

**Spec coverage:**
- Raw Debug event lines → Task 3 (`format!("{e:?}")`). ✓
- Full accumulated history, cleared on Hello → Task 1 (`log` push; Hello clears). ✓
- Newest at bottom + auto-scroll → Task 3 (oldest-first render + wasm scroll effect). ✓
- Header = menu-choice label → Task 4 (sets `pending_label`) + Task 1 (consumes it). ✓
- Generic fallback when unknown / multiplayer → Task 1 (`"(action)"`). ✓
- Rejected must not bleed a label → Task 1 (`Rejected` clears `pending_label`). ✓
- `response_label` mapping (all four responses) → Task 2 (helper + tests). ✓
- Layout left of board → Task 3 (app.rs `.layout` flex, log first). ✓
- No protocol/server change → none present. ✓
- Out-of-scope items (filtering, timestamps, enrichment, phase sub-segmentation, persistence) → not implemented. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. ✓

**Type consistency:** `LogBatch { header: String, events: Vec<Event> }` defined in Task 1, consumed identically in Tasks 3 & the Task 3 test. `pending_label: Option<String>` set in Task 4, consumed in Task 1. `response_label(&InputRequest, &InputResponse) -> String` defined in Task 2; its mapping is mirrored inline in Task 4 (documented). ✓

**Known verification points flagged for the implementer:** `OptionId` tuple-vs-named field (Task 2 Step 3 note); `InputResponse` `#[non_exhaustive]` wildcard (Task 2 Step 3 note); native compile of `NodeRef` (Task 3 Step 8).
