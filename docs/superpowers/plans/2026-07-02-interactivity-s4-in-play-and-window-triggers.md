# Interactivity S4 — in-play + reaction-window trigger menus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In-play/threat cards glow and open Activate/Trigger context menus; reaction-window triggers anchor to their source card (engine change), and a reaction/Fast window's Pass + prompt move to the prominent bottom banner.

**Architecture:** A new `OptionTarget::HandCardByCode` (drops `OptionTarget: Copy`) lets `build_resolution_options` anchor reaction candidates by source (`InPlay`→`CardInstance`, `Hand`→by-code, `Board`→`Global`); `drive_fast_window` reuses S0's `TurnAction::target`. A new `InPlayCardView` wrapper renders the in-play menu; `HandCardView` dual-matches Play + reaction-by-code; `PromptBanner` extends to skippable windows and `input.rs` drops its Skip.

**Tech Stack:** Rust, Leptos 0.8 (CSR/wasm), `wasm-bindgen-test` headless Firefox.

## Global Constraints

- Issue: **#539** (interactivity S4). Umbrella: **#206**. Design: `docs/superpowers/specs/2026-07-02-interactivity-s4-in-play-and-window-triggers-design.md`.
- Branch: `ui/interactivity-inplay-window-menus` (created; S4 spec committed on it). Commit scope: `engine:` (Task 1) / `web:` (Tasks 2–3).
- **`Board` reaction candidates stay `Global`** (bar-reachable); the bar keeps rendering window option buttons until S6.
- wasm-gating unchanged: `menu_layer` + banner submit are wasm-only; glow classes + `options_for*` are non-gated.
- Match CI's strict flags before pushing (all seven jobs). Merge only after approval.

---

### Task 1: Engine — anchor reaction-window + Fast-window options

**Files:**
- Modify: `crates/game-core/src/engine/outcome.rs` (add `HandCardByCode`; drop `Copy`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`build_resolution_options`, `drive_fast_window`, a unit test)

**Interfaces:**
- Produces: `OptionTarget::HandCardByCode { investigator: InvestigatorId, code: CardCode }`. `OptionTarget` is no longer `Copy` (still `Clone`/`PartialEq`/`Eq`/serde).

- [ ] **Step 1: Write the failing anchor test** — append inside the `#[cfg(test)] mod tests` in `crates/game-core/src/engine/dispatch/reaction_windows.rs`

```rust
    #[test]
    fn resolution_options_anchor_by_candidate_source() {
        use crate::engine::OptionTarget;
        use crate::state::{CardCode, CardInstanceId, InvestigatorId, ResolutionCandidate};
        let cands = vec![
            ResolutionCandidate {
                code: CardCode::new("_inplay"),
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::InPlay(CardInstanceId(9)),
            },
            ResolutionCandidate {
                code: CardCode::new("01022"),
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::Hand,
            },
            ResolutionCandidate {
                code: CardCode::new("_board"),
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::Board,
            },
        ];
        let opts = build_resolution_options(&cands);
        assert_eq!(opts[0].target, OptionTarget::CardInstance(CardInstanceId(9)));
        assert_eq!(
            opts[1].target,
            OptionTarget::HandCardByCode {
                investigator: InvestigatorId(1),
                code: CardCode::new("01022"),
            }
        );
        assert_eq!(opts[2].target, OptionTarget::Global);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core --lib resolution_options_anchor_by_candidate_source 2>&1 | tail -10`
Expected: compile error — no `HandCardByCode` variant / `opts[…].target` mismatch.

- [ ] **Step 3: Add the `HandCardByCode` variant + drop `Copy`** — in `crates/game-core/src/engine/outcome.rs`

Add `CardCode` to the state import:

```rust
use crate::state::{CardCode, CardInstanceId, EnemyId, InvestigatorId, LocationId};
```

Change the `OptionTarget` derive (remove `Copy`) and add the variant:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OptionTarget {
    /// No board anchor — a global / contextual control.
    Global,
    /// A location on the map.
    Location(LocationId),
    /// An enemy.
    Enemy(EnemyId),
    /// A card in an investigator's hand, by zero-based hand index.
    HandCard {
        /// The hand's owner.
        investigator: InvestigatorId,
        /// Zero-based position in that investigator's hand.
        hand_index: u8,
    },
    /// A card in an investigator's hand, matched by **code** (every copy) — for a
    /// queued Fast reaction event, which is code-identified (either copy plays), so
    /// all matching hand cards are actionable (#539).
    HandCardByCode {
        /// The hand's owner.
        investigator: InvestigatorId,
        /// The card code; all hand cards of this code are actionable.
        code: CardCode,
    },
    /// An in-play / threat-area / investigator card instance.
    CardInstance(CardInstanceId),
    /// The current act.
    Act,
}
```

- [ ] **Step 4: Anchor `build_resolution_options`** — in `crates/game-core/src/engine/dispatch/reaction_windows.rs`, replace the `.map(...)` body of `build_resolution_options`

```rust
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
                CandidateSource::Board => (
                    format!("Resolve reaction: {}", cand.code),
                    crate::engine::OptionTarget::Global,
                ),
            };
            ChoiceOption::new(id, label, target)
        })
```

- [ ] **Step 5: Anchor `drive_fast_window`** — in the same file, replace its option-building `.map(...)` (currently `ChoiceOption::global(…, a.label(cx.state))`)

```rust
        .map(|(i, a)| {
            ChoiceOption::new(
                OptionId(u32::try_from(i).unwrap_or(u32::MAX)),
                a.label(cx.state),
                a.target(cx.state),
            )
        })
```

- [ ] **Step 6: Run the anchor test + the whole game-core suite** (catches any dropped-`Copy` fallout)

Run: `cargo test -p game-core 2>&1 | tail -12`
Expected: PASS. If a `use of moved value` error surfaces (a site that relied on `OptionTarget: Copy`), add a `.clone()` at that site — none are expected.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/outcome.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: anchor reaction/fast-window options to their source card (#539)"
```

---

### Task 2: Web — `InPlayCardView` + hand reaction-by-code

**Files:**
- Modify: `crates/web/src/interaction.rs` (`options_for_hand_card` + native test)
- Modify: `crates/web/src/card.rs` (`InPlayCardView`; `HandCardView` dual-match)
- Modify: `crates/web/src/board.rs` (in-play + threat → `InPlayCardView`)
- Modify: `crates/web/style.css` (`.card-slot`)
- Create: `crates/web/tests/in_play_card.rs` (headless)
- Modify: `crates/web/tests/card.rs` (hand reaction-by-code headless test)

**Interfaces:**
- Consumes: `OptionTarget::{CardInstance, HandCard, HandCardByCode}`, `menu_layer`, `options_for` (S1/S2), `PendingOptions`.
- Produces: `web::interaction::options_for_hand_card(options: &[ChoiceOption], investigator: InvestigatorId, index: u8, code: &CardCode) -> Vec<ChoiceOption>`; `web::card::InPlayCardView` (prop `instance: CardInPlay`).

- [ ] **Step 1: Write the failing native test** — append inside `interaction.rs`'s `#[cfg(test)] mod tests`

```rust
    #[test]
    fn options_for_hand_card_matches_index_and_code() {
        use game_core::state::{CardCode, InvestigatorId};
        let inv = InvestigatorId(1);
        let code = CardCode::new("01022");
        let opts = vec![
            ChoiceOption::new(OptionId(0), "Play", OptionTarget::HandCard { investigator: inv, hand_index: 0 }),
            ChoiceOption::new(OptionId(1), "Trigger", OptionTarget::HandCardByCode { investigator: inv, code: code.clone() }),
            ChoiceOption::new(OptionId(2), "Other", OptionTarget::HandCard { investigator: inv, hand_index: 5 }),
            ChoiceOption::new(OptionId(3), "OtherCode", OptionTarget::HandCardByCode { investigator: inv, code: CardCode::new("zzz") }),
        ];
        let got = options_for_hand_card(&opts, inv, 0, &code);
        let ids: Vec<u32> = got.iter().map(|o| o.id.0).collect();
        assert_eq!(ids, vec![0, 1]); // exact index 0 + matching code; not index 5 / other code
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p web --lib options_for_hand_card 2>&1 | tail -8`
Expected: compile error — `options_for_hand_card` not found.

- [ ] **Step 3: Add `options_for_hand_card`** — in `crates/web/src/interaction.rs`, after `options_for`

```rust
/// The options actionable for a specific hand card: those anchored to its exact
/// slot (`HandCard { investigator, hand_index }`, the Play menu) or to its code
/// (`HandCardByCode { investigator, code }`, a Fast reaction event — every copy).
/// Pure.
#[must_use]
pub fn options_for_hand_card(
    options: &[ChoiceOption],
    investigator: game_core::state::InvestigatorId,
    index: u8,
    code: &game_core::state::CardCode,
) -> Vec<ChoiceOption> {
    options
        .iter()
        .filter(|o| match &o.target {
            OptionTarget::HandCard {
                investigator: i,
                hand_index,
            } => *i == investigator && *hand_index == index,
            OptionTarget::HandCardByCode {
                investigator: i,
                code: c,
            } => *i == investigator && c == code,
            _ => false,
        })
        .cloned()
        .collect()
}
```

- [ ] **Step 4: Run the native test to verify it passes**

Run: `cargo test -p web --lib options_for_hand_card 2>&1 | grep -E "test result|options_for_hand_card"`
Expected: PASS.

- [ ] **Step 5: Switch `HandCardView` to the dual matcher + add `InPlayCardView`** — in `crates/web/src/card.rs`

In `HandCardView`'s non-multiselect branch, replace the `menu_opts` line:

```rust
    let menu_opts = crate::interaction::options_for_hand_card(&pending, investigator, index, &code);
```

(The `&code` borrow is released before `<Card code=code/>` moves it.)

Add `InPlayCardView` after `HandCardView`:

```rust
/// Interactive wrapper for an in-play / threat-area card (#539). Keeps [`Card`]
/// display-only: reads `PendingOptions` and, via the card's `CardInstance` anchor,
/// offers an Activate (open-turn) / Trigger (reaction-window) menu. No selection
/// mode — that is hand-only.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn InPlayCardView(instance: CardInPlay) -> impl IntoView {
    let code = instance.code.clone();
    let instance_id = instance.instance_id;
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = crate::interaction::options_for(
        &pending,
        game_core::OptionTarget::CardInstance(instance_id),
    );
    let actionable = !menu_opts.is_empty();
    #[cfg(target_arch = "wasm32")]
    let open = RwSignal::new(None::<(i32, i32)>);
    view! {
        <div class="card-slot" class:actionable=actionable>
            <Card code=code in_play=instance/>
            {
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
            }
        </div>
    }
}
```

- [ ] **Step 6: Wire `board.rs`** — replace the in-play and threat `.map(...)` bodies (both currently `view! { <crate::card::Card code=code in_play=c/> }`)

```rust
            let in_play: Vec<_> = inv
                .cards_in_play
                .iter()
                .cloned()
                .map(|c| view! { <crate::card::InPlayCardView instance=c/> })
                .collect();
```

and

```rust
            let threat: Vec<_> = inv
                .threat_area
                .iter()
                .cloned()
                .map(|c| view! { <crate::card::InPlayCardView instance=c/> })
                .collect();
```

(The `let code = c.code.clone();` lines that preceded each map body are now unused — delete them.)

- [ ] **Step 7: Add the CSS** — append to `crates/web/style.css`

```css
/* Interactivity S4 (#539): in-play/threat card wrapper glow. */
.card-slot { position: relative; }
.card-slot.actionable { box-shadow: 0 0 0 2px #e0b84c; cursor: pointer; }
```

- [ ] **Step 8: Write the headless tests** — create `crates/web/tests/in_play_card.rs`

```rust
//! Headless tests for `InPlayCardView` (interactivity S4, #539): a card whose
//! `CardInstance` anchor has an option glows and opens a menu that submits
//! `PickSingle`; an inert instance has no glow.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{CardCode, CardInPlay, CardInstanceId};
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::card::InPlayCardView;
use web::interaction::PendingOptions;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

fn last_slot() -> web_sys::Element {
    let slots = document().query_selector_all(".card-slot").expect("query");
    slots
        .item(slots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .card-slot")
}

/// Mount `InPlayCardView` (Machete 01020, instance 3) with a store carrying
/// `outcome`, a derived `PendingOptions`, and a capturing channel.
async fn mount(outcome: game_core::EngineOutcome) -> mpsc::UnboundedReceiver<ClientMessage> {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    let inst = CardInPlay::enter_play(CardCode::new("01020"), CardInstanceId(3));
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        view! { <InPlayCardView instance=inst.clone()/> }
    });
    leptos::task::tick().await;
    rx
}

#[wasm_bindgen_test]
async fn activatable_in_play_card_opens_a_menu_and_submits() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Activate",
            OptionTarget::CardInstance(CardInstanceId(3)),
        )],
    );
    let mut rx = mount(outcome).await;
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
    assert_eq!(item.text_content().unwrap_or_default(), "Activate");
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
async fn inert_in_play_card_has_no_glow() {
    // Option anchors to a different instance → this card stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Activate",
            OptionTarget::CardInstance(CardInstanceId(99)),
        )],
    );
    let _ = mount(outcome).await;
    assert!(!last_slot().class_name().contains("actionable"));
}
```

Append the hand reaction-by-code test to `crates/web/tests/card.rs` (imports `CardCode` etc. already present from S3):

```rust
#[wasm_bindgen_test]
async fn hand_card_glows_for_a_reaction_anchored_by_code() {
    // A HandCardByCode-anchored option (a Fast reaction event) glows the hand
    // card of that code (Machete 01020 as a stand-in) and opens its menu.
    let outcome = awaiting_pick_single_with(
        "You may play a card",
        vec![ChoiceOption::new(
            OptionId(0),
            "Play Machete from hand",
            OptionTarget::HandCardByCode {
                investigator: InvestigatorId(1),
                code: CardCode::new("01020"),
            },
        )],
    );
    let (_selected, mut rx) = mount_hand(outcome).await;
    let slot = last_slot();
    assert!(slot.class_name().contains("actionable"), "reaction card glows by code");
    slot.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    leptos::task::tick().await;
    slot.query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item")
        .click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}
```

- [ ] **Step 9: Run the headless tests + host clippy**

Run: `wasm-pack test --headless --firefox crates/web --test in_play_card --test card 2>&1 | tail -15`
Expected: `in_play_card` 2/2 and `card` (9 + the new one) pass.
Run: `cargo clippy -p web --all-targets --all-features -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add crates/web/src/interaction.rs crates/web/src/card.rs crates/web/src/board.rs \
        crates/web/style.css crates/web/tests/in_play_card.rs crates/web/tests/card.rs
git commit -m "web: in-play/threat cards glow + open Activate/Trigger menus; hand reaction-by-code"
```

---

### Task 3: Web — banner window Pass + drop the bar's Skip

**Files:**
- Modify: `crates/web/src/prompt_banner.rs` (render for skippable windows)
- Modify: `crates/web/src/input.rs` (remove `skip_button`)
- Modify: `crates/web/tests/prompt_banner.rs` (skippable-window Pass test)
- Modify: `crates/web/tests/awaiting_input.rs` (drop the Skip tests)

**Interfaces:**
- Consumes: `MultiSelect` (S3); `InputRequest.skippable`/`.kind`.

- [ ] **Step 1: Write the failing banner window test** — append to `crates/web/tests/prompt_banner.rs`

```rust
#[wasm_bindgen_test]
async fn skippable_window_shows_prompt_and_pass_submits_skip() {
    // A skippable PickSingle (reaction/Fast window) → banner with prompt + Pass.
    let outcome =
        game_core::test_support::fixtures::awaiting_skippable_pick_single_input("You may trigger");
    let mut rx = mount(outcome, &[]).await;
    assert!(
        last_banner()
            .text_content()
            .unwrap_or_default()
            .contains("You may trigger"),
        "window prompt shows in the banner"
    );
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

#[wasm_bindgen_test]
async fn non_skippable_pick_single_renders_no_banner() {
    // The open turn (non-skippable PickSingle) is not a banner concern.
    let outcome = game_core::test_support::fixtures::awaiting_pick_single_input("Choose an action");
    let _rx = mount(outcome, &[]).await;
    assert!(
        last_root()
            .query_selector(".prompt-banner")
            .expect("query")
            .is_none(),
        "no banner for a non-skippable PickSingle"
    );
}
```

(`awaiting_skippable_pick_single_input` / `awaiting_pick_single_input` already exist in `fixtures.rs`.)

- [ ] **Step 2: Run it to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test prompt_banner 2>&1 | tail -15`
Expected: `skippable_window_shows_prompt_and_pass_submits_skip` FAILs (no `.prompt-banner` — the banner is PickMultiple-only).

- [ ] **Step 3: Extend `PromptBanner`** — in `crates/web/src/prompt_banner.rs`, replace the reactive-closure body of the `view!`

```rust
        {move || {
            let state = store.get();
            let Some(EngineOutcome::AwaitingInput { request, .. }) = state.outcome else {
                return ().into_any();
            };
            let is_multi = request.kind == InputKind::PickMultiple;
            // Rendered for a multi-select commit or any skippable window; other
            // prompts (open-turn PickSingle, encounter Confirm) stay in the flat bar.
            if !is_multi && !request.skippable {
                return ().into_any();
            }
            let prompt = request.prompt.clone();

            // Confirm — PickMultiple only (submits the MultiSelect selection).
            let confirm_btn = is_multi
                .then(|| ms.clone())
                .flatten()
                .map(|ms| {
                    let selected = ms.selected;
                    let tx = tx.clone();
                    let confirm = move |_| {
                        if let Some(tx) = tx.clone() {
                            let sel: Vec<OptionId> =
                                selected.get_untracked().into_iter().map(OptionId).collect();
                            store.update(|s| {
                                s.pending_label = Some(format!("Commit {} card(s)", sel.len()));
                            });
                            let _ = tx.unbounded_send(ClientMessage::Submit {
                                action: PlayerAction::ResolveInput {
                                    response: InputResponse::PickMultiple { selected: sel },
                                },
                            });
                            selected.set(BTreeSet::new());
                        }
                    };
                    view! { <button class="confirm" on:click=confirm>"Confirm"</button> }
                });

            // Pass — whenever the request is skippable.
            let pass_btn = request.skippable.then(|| {
                let tx = tx.clone();
                let pass = move |_| {
                    if let Some(tx) = tx.clone() {
                        store.update(|s| s.pending_label = Some("Skip".to_string()));
                        let _ = tx.unbounded_send(ClientMessage::Submit {
                            action: PlayerAction::ResolveInput { response: InputResponse::Skip },
                        });
                    }
                };
                view! { <button class="pass" on:click=pass>"Pass"</button> }
            });

            view! {
                <div class="prompt-banner">
                    <span class="prompt">{prompt}</span>
                    {confirm_btn}
                    {pass_btn}
                </div>
            }
            .into_any()
        }}
```

- [ ] **Step 4: Remove `input.rs`'s Skip control** — in `crates/web/src/input.rs`:
  1. Delete the `let skippable = request.skippable;` line and the whole `let skip_button = { … };` closure (the comment through its closing `};`).
  2. Delete each `{skip_button()}` call — in the `PickSingle` arm, the `Confirm` arm, and the `_` fallback arm.
  3. Update the `_` fallback comment ("falls back to the prompt + any Skip control") to just "falls back to the prompt".

- [ ] **Step 5: Move the Skip tests out of `awaiting_input.rs`** — in `crates/web/tests/awaiting_input.rs`, delete the two tests that assert the bar's Skip: `skippable_window_renders_skip_button_and_submits_skip` and `non_skippable_pick_single_has_no_skip_button` (their coverage is now `tests/prompt_banner.rs`). Keep the `pick_single_*` and `confirm_*` tests. If deleting `skippable_window_…` leaves an import (e.g. `awaiting_skippable_pick_single_input`) or a helper unused, remove it.

- [ ] **Step 6: Run the banner + awaiting_input tests + wasm clippy**

Run: `wasm-pack test --headless --firefox crates/web --test prompt_banner --test awaiting_input 2>&1 | tail -15`
Expected: `prompt_banner` (5 + 2 new) and `awaiting_input` (remaining) pass.
Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/prompt_banner.rs crates/web/src/input.rs \
        crates/web/tests/prompt_banner.rs crates/web/tests/awaiting_input.rs
git commit -m "web: reaction/Fast window prompt + Pass move to the banner; drop the bar's Skip"
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

Watch for: a `use of moved value` from dropping `OptionTarget: Copy` (add `.clone()` where flagged — none expected); an unused import in `awaiting_input.rs` after removing the Skip tests; `doc_lazy_continuation` on new doc comments.

## PR flow (after the gauntlet is green)

1. Push `ui/interactivity-inplay-window-menus`; open the PR. Body: goal + `Closes #539.`
2. `gh pr checks <PR#> --watch`.
3. **Phase-doc update is the final commit, pushed only after CI is green** (record S4; tick #539 in the #206 checklist after merge) — then CI green again, merge on approval.

## Self-review notes

- **Spec coverage:** `HandCardByCode` + `Copy` drop ✅ (T1); `build_resolution_options` source→anchor ✅ (T1); `drive_fast_window` `.target` ✅ (T1); `InPlayCardView` + board wiring ✅ (T2); `HandCardView` dual-match via `options_for_hand_card` ✅ (T2); `.card-slot` CSS ✅ (T2); banner extends to skippable windows ✅ (T3); `input.rs` Skip removed ✅ (T3); `Board`→`Global` bar-reachable (bar keeps options) ✅; open-turn Activate needs no engine change ✅.
- **Testing:** engine anchor unit test ✅ (T1); `options_for_hand_card` native ✅ (T2); `InPlayCardView` headless + hand reaction-by-code headless ✅ (T2); banner window Pass + no-banner-for-non-skippable ✅ (T3); `awaiting_input` Skip tests relocated ✅ (T3).
- **Type consistency:** `OptionTarget::HandCardByCode { investigator: InvestigatorId, code: CardCode }` used in engine (build_resolution_options), `options_for_hand_card`, and tests; `InPlayCardView { instance: CardInPlay }`; `options_for(CardInstance(id))` for in-play; submit paths `PickSingle`/`PickMultiple`/`Skip` consistent; `ResolutionCandidate { code, controller, ability_index, source }` field names match the engine's builders.
