# P6.7a Core-Loop Action Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a wasm-only `ActionControls` Leptos component that lets the player click the toy scenario's core-loop actions (Investigate, EndTurn, DrawEncounterCard, AdvanceAct, Move, PlayCard, Mulligan), each gated by the P6.6 legality helper and submitting the correct `ClientMessage::Submit { action }`.

**Architecture:** One new module `crates/web/src/controls.rs` (wasm-only, like `input`/`transport`). The `ActionControls` component reads the store reactively, computes `legality::enabled_controls`, and renders buttons whose `disabled` binds to that set. Zero-payload actions are plain buttons; Move and PlayCard use inline pickers; Mulligan has its own multi-select hand. `board.rs` stays read-only. Headless `wasm-bindgen-test`s mount the component with an `mpsc` outbound channel and assert the submitted frame — mirroring `crates/web/tests/input.rs`.

**Tech Stack:** Rust, Leptos (CSR), `wasm-bindgen-test` (headless Firefox), `futures::channel::mpsc`, `protocol`, `game-core`.

**Spec:** [`docs/superpowers/specs/2026-06-09-p6-7a-action-controls-design.md`](../specs/2026-06-09-p6-7a-action-controls-design.md)

---

## Reference patterns (read before starting)

- `crates/web/src/input.rs` — the `AwaitingInputView` component: reactive `move || { store.get() … }.into_any()` body, `OutboundTx` read as `Option` from context, multi-select via a component-scoped `RwSignal<BTreeSet<u32>>`, `tx.unbounded_send(ClientMessage::Submit { … })`.
- `crates/web/tests/input.rs` — the headless harness: `mount_to_body` providing `store` + `OutboundTx` context, drive state via `reduce(s, ServerMessage::Hello { … })`, `leptos::task::tick().await`, then `rx.try_recv()`. The DOM accumulates across tests in one page, so absence/selection assertions scope to the **last** mounted `.controls` section.
- `crates/web/src/legality.rs` — `enabled_controls(&GameState, &EngineOutcome) -> BTreeSet<ActionControl>` and the `ActionControl` enum (note: `ActionControl::DrawEncounter`, but the action is `PlayerAction::DrawEncounterCard`).
- `crates/web/src/board.rs` — read-only render; do **not** modify it.

## CI gauntlet for this crate (run after each task before committing)

```sh
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
wasm-pack test --headless --firefox crates/web
```

The host `fmt`/`clippy`/`test` jobs do not compile the wasm-only `controls` module, but run `cargo fmt --check` before committing regardless.

## File structure

- **Create:** `crates/web/src/controls.rs` — the `ActionControls` component + a private `submit_button` helper.
- **Modify:** `crates/web/src/lib.rs` — declare the wasm-only module.
- **Modify:** `crates/web/src/app.rs` — mount `<ActionControls/>`, remove the obsolete `DebugSubmit`.
- **Create:** `crates/web/tests/controls.rs` — headless component tests.

---

## Task 1: Module scaffold, zero-payload buttons, wiring, and harness

**Files:**
- Create: `crates/web/src/controls.rs`
- Modify: `crates/web/src/lib.rs:16-17`
- Modify: `crates/web/src/app.rs:24-48`
- Create: `crates/web/tests/controls.rs`

This task stands up the module with the four zero-payload buttons (Investigate, EndTurn, DrawEncounterCard, AdvanceAct), wires it into the app, and establishes the test harness with one submit test per button plus a gating test.

- [ ] **Step 1: Create the test file with the harness and the first failing test**

Create `crates/web/tests/controls.rs`:

```rust
//! Headless tests for `ActionControls` (P6.7a): feed a `GameState` +
//! `EngineOutcome` through the store, then assert each control, when
//! legal, submits the matching `ClientMessage::Submit { action }`.
//! wasm32-only (browser DOM + the wasm-only transport types).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::builder::TestGame;
use game_core::test_support::fixtures::{test_investigator, test_location};
use game_core::{EngineOutcome, PlayerAction};
use leptos::prelude::*;
use protocol::{ClientMessage, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::controls::ActionControls;
use web::store::{reduce, ClientState};
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `ActionControls` with a fresh store + outbound channel, feed one
/// `Hello { state, outcome }`, tick, and return the receiver.
async fn mount(
    state: game_core::state::GameState,
    outcome: EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        leptos::view! { <ActionControls/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome,
            },
        );
    });
    leptos::task::tick().await;
    rx
}

/// The last mounted `.controls` section (DOM accumulates across tests).
fn last_controls() -> web_sys::Element {
    let secs = leptos::prelude::document()
        .query_selector_all(".controls")
        .expect("query");
    secs.item(secs.length() - 1)
        .expect("at least one controls section")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
}

/// Click the first element matching `selector` within `section`.
fn click_in(section: &web_sys::Element, selector: &str) {
    section
        .query_selector(selector)
        .expect("query")
        .expect("element present")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
}

/// An Investigation-phase game with one active investigator.
fn investigation_game() -> game_core::state::GameState {
    TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .build()
}

#[wasm_bindgen_test]
async fn end_turn_submits_end_turn() {
    let mut rx = mount(investigation_game(), EngineOutcome::Done).await;
    click_in(&last_controls(), ".end-turn");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    assert_eq!(
        frame,
        ClientMessage::Submit {
            action: PlayerAction::EndTurn
        }
    );
}
```

- [ ] **Step 2: Run the test to verify it fails to compile**

Run: `wasm-pack test --headless --firefox crates/web 2>&1 | tail -20`
Expected: compile error — `unresolved import web::controls` / `ActionControls` not found.

- [ ] **Step 3: Create the module with the `submit_button` helper and the four zero-payload buttons**

Create `crates/web/src/controls.rs`:

```rust
//! Core-loop action controls (P6.7a, wasm-only). Buttons that submit the
//! toy scenario's actions, each `disabled` per the P6.6 legality helper
//! ([`enabled_controls`](crate::legality::enabled_controls)) — a UX
//! affordance, not a correctness gate (the server stays authoritative).
//! Move/PlayCard use inline pickers; Mulligan has its own multi-select.
//! `board.rs` stays read-only — all interactivity lives here.

use std::collections::BTreeSet;

use game_core::{EngineOutcome, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::legality::{enabled_controls, ActionControl};
use crate::store::use_store;
use crate::transport::OutboundTx;

/// A single zero-payload action button. `class` carries a test-stable hook
/// (e.g. `"action end-turn"`); `disabled` reflects legality; the click
/// submits `action` when an `OutboundTx` is present (absent in
/// render-only contexts → no-op, matching `AwaitingInputView`).
fn submit_button(
    class: &'static str,
    label: &'static str,
    disabled: bool,
    tx: Option<OutboundTx>,
    action: PlayerAction,
) -> impl IntoView {
    view! {
        <button
            class=class
            disabled=disabled
            on:click=move |_| {
                if let Some(tx) = tx.clone() {
                    let _ = tx.unbounded_send(ClientMessage::Submit { action: action.clone() });
                }
            }
        >
            {label}
        </button>
    }
}

/// All core-loop action controls. Reads the store reactively; nothing
/// renders until both a `game` and an `outcome` are present.
#[component]
pub fn ActionControls() -> impl IntoView {
    let store = use_store();
    let tx = use_context::<OutboundTx>();

    view! {
        {move || {
            let state = store.get();
            let (Some(game), Some(outcome)) = (state.game.clone(), state.outcome.clone()) else {
                return ().into_any();
            };
            let enabled = enabled_controls(&game, &outcome);
            let has = |c: ActionControl| enabled.contains(&c);
            let active = game.active_investigator;

            // Investigator-bearing buttons only render with an active
            // investigator; the toy scenario always has one in Investigation.
            let investigate = active.map(|inv| {
                submit_button(
                    "action investigate",
                    "Investigate",
                    !has(ActionControl::Investigate),
                    tx.clone(),
                    PlayerAction::Investigate { investigator: inv },
                )
            });
            let advance_act = active.map(|inv| {
                submit_button(
                    "action advance-act",
                    "Advance act",
                    !has(ActionControl::AdvanceAct),
                    tx.clone(),
                    PlayerAction::AdvanceAct { investigator: inv },
                )
            });

            view! {
                <section class="controls">
                    {investigate}
                    {advance_act}
                    {submit_button(
                        "action end-turn",
                        "End turn",
                        !has(ActionControl::EndTurn),
                        tx.clone(),
                        PlayerAction::EndTurn,
                    )}
                    {submit_button(
                        "action draw-encounter",
                        "Draw encounter",
                        !has(ActionControl::DrawEncounter),
                        tx.clone(),
                        PlayerAction::DrawEncounterCard,
                    )}
                </section>
            }
            .into_any()
        }}
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

In `crates/web/src/lib.rs`, add a `controls` declaration next to `input` (both wasm-only). The end of the file becomes:

```rust
#[cfg(target_arch = "wasm32")]
pub mod transport;

#[cfg(target_arch = "wasm32")]
pub mod input;

#[cfg(target_arch = "wasm32")]
pub mod controls;
```

- [ ] **Step 5: Mount `<ActionControls/>` in `app.rs` and remove `DebugSubmit`**

In `crates/web/src/app.rs`, replace the wasm-only branch that renders `AwaitingInputView` + `DebugSubmit`:

```rust
            {
                #[cfg(target_arch = "wasm32")]
                { view! { <crate::input::AwaitingInputView/><crate::controls::ActionControls/> }.into_any() }
                #[cfg(not(target_arch = "wasm32"))]
                { ().into_any() }
            }
```

Then delete the entire `DebugSubmit` function (the `#[cfg(target_arch = "wasm32")] #[component] fn DebugSubmit() { … }` block at the bottom of the file) — its doc-comment names P6.7 as its replacement, so this change obsoletes it.

- [ ] **Step 6: Run the test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web 2>&1 | tail -20`
Expected: PASS for `end_turn_submits_end_turn` (and the existing `input`/`board`/`store`/`smoke` tests still pass).

- [ ] **Step 7: Add submit tests for the other three zero-payload buttons**

Append to `crates/web/tests/controls.rs`:

```rust
#[wasm_bindgen_test]
async fn investigate_submits_investigate_for_active() {
    let mut rx = mount(investigation_game(), EngineOutcome::Done).await;
    click_in(&last_controls(), ".investigate");
    leptos::task::tick().await;
    assert_eq!(
        rx.try_recv().expect("a frame after tick"),
        ClientMessage::Submit {
            action: PlayerAction::Investigate {
                investigator: InvestigatorId(1)
            }
        }
    );
}

#[wasm_bindgen_test]
async fn advance_act_submits_advance_act_for_active() {
    let mut rx = mount(investigation_game(), EngineOutcome::Done).await;
    click_in(&last_controls(), ".advance-act");
    leptos::task::tick().await;
    assert_eq!(
        rx.try_recv().expect("a frame after tick"),
        ClientMessage::Submit {
            action: PlayerAction::AdvanceAct {
                investigator: InvestigatorId(1)
            }
        }
    );
}

#[wasm_bindgen_test]
async fn draw_encounter_submits_draw_encounter_card() {
    // Mythos phase with this investigator on the draw cursor: only
    // DrawEncounter is legal. `mythos_draw_pending` has no builder setter,
    // so mutate the built state directly (legality.rs tests do the same).
    let mut game = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Mythos)
        .build();
    game.mythos_draw_pending = Some(InvestigatorId(1));

    let mut rx = mount(game, EngineOutcome::Done).await;
    click_in(&last_controls(), ".draw-encounter");
    leptos::task::tick().await;
    assert_eq!(
        rx.try_recv().expect("a frame after tick"),
        ClientMessage::Submit {
            action: PlayerAction::DrawEncounterCard
        }
    );
}
```

- [ ] **Step 8: Add the gating test (disabled buttons carry the attribute and do not submit)**

Append to `crates/web/tests/controls.rs`:

```rust
#[wasm_bindgen_test]
async fn illegal_controls_are_disabled_and_do_not_submit() {
    // Mythos + draw cursor: only DrawEncounter is legal, so End turn is
    // disabled and DrawEncounter is not.
    let mut game = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Mythos)
        .build();
    game.mythos_draw_pending = Some(InvestigatorId(1));

    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    let end_turn = controls
        .query_selector(".end-turn")
        .expect("query")
        .expect("end-turn present");
    let draw = controls
        .query_selector(".draw-encounter")
        .expect("query")
        .expect("draw-encounter present");
    assert!(end_turn.has_attribute("disabled"), "End turn should be disabled");
    assert!(!draw.has_attribute("disabled"), "Draw encounter should be enabled");

    // A disabled button does not fire click → no frame.
    click_in(&controls, ".end-turn");
    leptos::task::tick().await;
    assert!(rx.try_recv().is_err(), "disabled button must not submit");
}
```

- [ ] **Step 9: Run the full crate gauntlet**

Run:
```sh
cargo fmt --check
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
wasm-pack test --headless --firefox crates/web 2>&1 | tail -25
```
Expected: fmt clean, build clean, clippy clean, all `controls` tests pass plus the pre-existing tests.

- [ ] **Step 10: Commit**

```sh
git add crates/web/src/controls.rs crates/web/src/lib.rs crates/web/src/app.rs crates/web/tests/controls.rs
git commit -m "ui: core-loop action controls — zero-payload buttons (P6.7a)

Refs #188.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Move picker (connected-destination buttons)

**Files:**
- Modify: `crates/web/src/controls.rs`
- Modify: `crates/web/tests/controls.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/web/tests/controls.rs`:

```rust
#[wasm_bindgen_test]
async fn move_picker_submits_move_to_connected_destination() {
    // Investigator at location 1, which connects to location 2.
    let mut loc1 = test_location(1, "Study");
    loc1.connections = vec![LocationId(2)];
    let loc2 = test_location(2, "Hallway");
    let game = TestGame::new()
        .with_investigator_at(test_investigator(1), LocationId(1))
        .with_active_investigator(InvestigatorId(1))
        .with_location(loc1)
        .with_location(loc2)
        .with_phase(Phase::Investigation)
        .build();

    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    // The single destination button is labeled by the destination name.
    let dest = controls
        .query_selector(".move-dest")
        .expect("query")
        .expect("a move destination button");
    assert!(dest.text_content().unwrap_or_default().contains("Hallway"));

    click_in(&controls, ".move-dest");
    leptos::task::tick().await;
    assert_eq!(
        rx.try_recv().expect("a frame after tick"),
        ClientMessage::Submit {
            action: PlayerAction::Move {
                investigator: InvestigatorId(1),
                destination: LocationId(2),
            }
        }
    );
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web 2>&1 | tail -20`
Expected: FAIL — no `.move-dest` element (`expected "a move destination button"` panics).

- [ ] **Step 3: Add the Move picker to the component**

In `crates/web/src/controls.rs`, inside the reactive closure in `ActionControls`, after `advance_act` and before the returned `view!`, build the destination buttons:

```rust
            // Move picker: one button per connected destination, labeled
            // by the destination's name. Renders only when Move is legal
            // and the active investigator has a current location.
            let move_dests: Vec<_> = if has(ActionControl::Move) {
                active
                    .and_then(|inv| game.investigators.get(&inv))
                    .and_then(|inv| inv.current_location)
                    .and_then(|loc_id| game.locations.get(&loc_id))
                    .map(|loc| loc.connections.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|dest_id| {
                        let inv = active?;
                        let name = game
                            .locations
                            .get(&dest_id)
                            .map_or_else(|| format!("loc {}", dest_id.0), |l| l.name.clone());
                        let tx = tx.clone();
                        Some(view! {
                            <button
                                class="move-dest"
                                on:click=move |_| {
                                    if let Some(tx) = tx.clone() {
                                        let _ = tx.unbounded_send(ClientMessage::Submit {
                                            action: PlayerAction::Move {
                                                investigator: inv,
                                                destination: dest_id,
                                            },
                                        });
                                    }
                                }
                            >
                                "Move to " {name}
                            </button>
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };
```

Then add `<div class="move-picker">{move_dests}</div>` into the returned `<section class="controls">` (after the four buttons).

- [ ] **Step 4: Run it to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web 2>&1 | tail -20`
Expected: PASS for `move_picker_submits_move_to_connected_destination`.

- [ ] **Step 5: Gauntlet + commit**

```sh
cargo fmt --check
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
wasm-pack test --headless --firefox crates/web 2>&1 | tail -25
git add crates/web/src/controls.rs crates/web/tests/controls.rs
git commit -m "ui: Move picker — connected-destination buttons (P6.7a)

Refs #188.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: PlayCard picker (per-hand-card play button)

**Files:**
- Modify: `crates/web/src/controls.rs`
- Modify: `crates/web/tests/controls.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/web/tests/controls.rs`:

```rust
#[wasm_bindgen_test]
async fn play_picker_submits_play_card_by_hand_index() {
    let mut inv = test_investigator(1);
    inv.hand = vec![
        CardCode::new("_synth_event_a"),
        CardCode::new("_synth_event_b"),
    ];
    let game = TestGame::new()
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .build();

    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    // Click the second card's Play button → hand_index 1.
    let buttons = controls.query_selector_all(".play-card").expect("query");
    buttons
        .item(1)
        .expect("second play button")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;
    assert_eq!(
        rx.try_recv().expect("a frame after tick"),
        ClientMessage::Submit {
            action: PlayerAction::PlayCard {
                investigator: InvestigatorId(1),
                hand_index: 1,
            }
        }
    );
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web 2>&1 | tail -20`
Expected: FAIL — no `.play-card` elements (`expected "second play button"` panics).

- [ ] **Step 3: Add the PlayCard picker to the component**

In `crates/web/src/controls.rs`, inside the reactive closure after `move_dests`, build the hand play buttons:

```rust
            // PlayCard picker: a "Play" button per card in the active
            // investigator's hand (hand_index = position). Renders only
            // when PlayCard is legal.
            let play_buttons: Vec<_> = if has(ActionControl::PlayCard) {
                active
                    .and_then(|inv| game.investigators.get(&inv))
                    .map(|inv_state| {
                        inv_state
                            .hand
                            .iter()
                            .enumerate()
                            .map(|(idx, code)| {
                                let hand_index = u8::try_from(idx).expect("hand fits in u8");
                                let inv = active.expect("active present in this branch");
                                let label = code.to_string();
                                let tx = tx.clone();
                                view! {
                                    <li>
                                        <button
                                            class="play-card"
                                            on:click=move |_| {
                                                if let Some(tx) = tx.clone() {
                                                    let _ = tx.unbounded_send(ClientMessage::Submit {
                                                        action: PlayerAction::PlayCard {
                                                            investigator: inv,
                                                            hand_index,
                                                        },
                                                    });
                                                }
                                            }
                                        >
                                            "Play " {label}
                                        </button>
                                    </li>
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
```

Then add `<ul class="play-picker">{play_buttons}</ul>` into the returned `<section class="controls">` (after `move-picker`).

- [ ] **Step 4: Run it to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web 2>&1 | tail -20`
Expected: PASS for `play_picker_submits_play_card_by_hand_index`.

- [ ] **Step 5: Gauntlet + commit**

```sh
cargo fmt --check
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
wasm-pack test --headless --firefox crates/web 2>&1 | tail -25
git add crates/web/src/controls.rs crates/web/tests/controls.rs
git commit -m "ui: PlayCard picker — per-hand-card play buttons (P6.7a)

Refs #188.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Mulligan multi-select

**Files:**
- Modify: `crates/web/src/controls.rs`
- Modify: `crates/web/tests/controls.rs`

- [ ] **Step 1: Write the two failing tests**

Append to `crates/web/tests/controls.rs`:

```rust
/// A setup game where investigator 1 is on the mulligan cursor with a
/// two-card hand.
fn mulligan_game() -> game_core::state::GameState {
    let mut inv = test_investigator(1);
    inv.hand = vec![
        CardCode::new("_synth_event_a"),
        CardCode::new("_synth_event_b"),
    ];
    TestGame::new()
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .with_mulligan_pending(InvestigatorId(1))
        .build()
}

#[wasm_bindgen_test]
async fn mulligan_submits_selected_indices() {
    let mut rx = mount(mulligan_game(), EngineOutcome::Done).await;
    let controls = last_controls();
    // Select the first card, then submit.
    let cards = controls.query_selector_all(".mull-card").expect("query");
    cards
        .item(0)
        .expect("first mull-card")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;
    click_in(&controls, ".mulligan-submit");
    leptos::task::tick().await;
    assert_eq!(
        rx.try_recv().expect("a frame after tick"),
        ClientMessage::Submit {
            action: PlayerAction::Mulligan {
                investigator: InvestigatorId(1),
                indices_to_redraw: vec![0],
            }
        }
    );
}

#[wasm_bindgen_test]
async fn mulligan_with_no_selection_keeps_hand() {
    let mut rx = mount(mulligan_game(), EngineOutcome::Done).await;
    click_in(&last_controls(), ".mulligan-submit");
    leptos::task::tick().await;
    assert_eq!(
        rx.try_recv().expect("a frame after tick"),
        ClientMessage::Submit {
            action: PlayerAction::Mulligan {
                investigator: InvestigatorId(1),
                indices_to_redraw: vec![],
            }
        }
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `wasm-pack test --headless --firefox crates/web 2>&1 | tail -20`
Expected: FAIL — no `.mull-card` / `.mulligan-submit` elements.

- [ ] **Step 3: Add a component-scoped selection signal and the Mulligan picker**

In `crates/web/src/controls.rs`, add the selection signal at the top of `ActionControls` (after `let tx = …`):

```rust
    // Mulligan's own selection signal — kept separate from the P6.6 commit
    // window (the shapes diverge; see the design spec). Cleared on submit.
    let mulligan_sel = RwSignal::new(BTreeSet::<u32>::new());
```

Then, inside the reactive closure after `play_buttons`, build the Mulligan view:

```rust
            // Mulligan multi-select: setup-only (gated on the
            // `mulligan_pending` cursor via the legality helper). Toggling a
            // card flips its index in `mulligan_sel`; submitting sends the
            // selected indices (empty = legal "keep my hand"). The cursor's
            // investigator owns the redraw, not necessarily `active`.
            let mulligan_view = if has(ActionControl::Mulligan) {
                let cursor = game.mulligan_pending;
                let hand: Vec<String> = cursor
                    .and_then(|id| game.investigators.get(&id))
                    .map(|inv| inv.hand.iter().map(ToString::to_string).collect())
                    .unwrap_or_default();
                let cards: Vec<_> = hand
                    .into_iter()
                    .enumerate()
                    .map(|(idx, code)| {
                        let i = u32::try_from(idx).expect("hand fits in u32");
                        view! {
                            <li>
                                <button
                                    class="mull-card"
                                    class:selected=move || mulligan_sel.get().contains(&i)
                                    on:click=move |_| mulligan_sel.update(|s| {
                                        if !s.remove(&i) {
                                            s.insert(i);
                                        }
                                    })
                                >
                                    {code}
                                </button>
                            </li>
                        }
                    })
                    .collect();
                let tx = tx.clone();
                let on_submit = move |_| {
                    if let Some(inv) = cursor {
                        let indices: Vec<u8> = mulligan_sel
                            .get()
                            .into_iter()
                            .map(|i| u8::try_from(i).expect("hand fits in u8"))
                            .collect();
                        if let Some(tx) = tx.clone() {
                            let _ = tx.unbounded_send(ClientMessage::Submit {
                                action: PlayerAction::Mulligan {
                                    investigator: inv,
                                    indices_to_redraw: indices,
                                },
                            });
                        }
                    }
                    mulligan_sel.set(BTreeSet::new());
                };
                view! {
                    <section class="mulligan">
                        <ul>{cards}</ul>
                        <button class="mulligan-submit" on:click=on_submit>"Mulligan"</button>
                    </section>
                }
                .into_any()
            } else {
                ().into_any()
            };
```

Then add `{mulligan_view}` into the returned `<section class="controls">` (after `play-picker`).

- [ ] **Step 4: Run them to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web 2>&1 | tail -25`
Expected: PASS for both mulligan tests.

- [ ] **Step 5: Final gauntlet + commit**

```sh
cargo fmt --check
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
wasm-pack test --headless --firefox crates/web 2>&1 | tail -30
git add crates/web/src/controls.rs crates/web/tests/controls.rs
git commit -m "ui: Mulligan multi-select control (P6.7a)

Refs #188.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Manual verification (after Task 4)

Per the dev loop in `CLAUDE.md` (server on :8000, `trunk serve` on :3000), open the client, create a game, and confirm the toy scenario is clickable end-to-end to a **Won** state: Mulligan through setup, then Investigate → accumulate clues → AdvanceAct. The Won/Lost banner itself is P6.8; here "Won" is observed via the resulting board state (act advanced).

## After all tasks: PR + phase doc

Follow `CLAUDE.md`'s PR procedure: push `ui/action-controls`, open the PR with `Closes #188`, watch CI (the `wasm-build`/`wasm-test`/`wasm-clippy` jobs are the load-bearing ones here), then — only once CI is green — update `docs/phases/phase-6-web-client-v0.md` as the final commit (move #188 to a Closed/✅ state, flip the P6.7a ordering rows, add a Decisions-made entry only if load-bearing for a future PR — e.g. the keep-Mulligan-separate / interactive-board-deferred calls if not already captured by the spec). Merge only after explicit approval.
```

