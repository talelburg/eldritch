# "New game" Button (#477) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "New game" button that forgets the saved `localStorage` game id and reloads, so `bootstrap()` re-enters the picker — no manual `localStorage` clearing.

**Architecture:** A wasm-only `transport::start_new_game()` (clear the saved id + `window.location().reload()`), surfaced as a wasm-gated button in `BoardView`. The reload re-bootstraps to the picker for free.

**Tech Stack:** Leptos (`crates/web`, wasm32), `web-sys`.

## Global Constraints

- **YAGNI:** no confirm/abandon prompt; no in-app re-bootstrap signal — a page reload re-bootstraps for free.
- **cfg gating:** `transport` is `#[cfg(target_arch = "wasm32")]`-only but `BoardView` also compiles for the host target, so the button + handler are wasm-gated (same `#[cfg]` split `app.rs` uses for the picker). Host builds render nothing there.
- **No new deps:** `clear_saved_id` already exists; `web-sys`'s `Window`/`Location` features are already enabled (`crates/web/Cargo.toml`).
- **CI gauntlet before push** (all seven jobs, warnings-as-errors) — `crates/web`, so `wasm-build`/`wasm-test`/`wasm-clippy` matter, and host `clippy` must still build `BoardView` with the wasm-gated button absent.
- **Branch:** `web/new-game-button` (already created; spec committed). One branch, follow-up commits, no force-push.
- Spec of record: `docs/superpowers/specs/2026-06-26-new-game-button-design.md`.

---

### Task 1: `start_new_game()` + the button

**Files:**
- Modify: `crates/web/src/transport.rs` (add `pub fn start_new_game()`)
- Modify: `crates/web/src/board.rs` (wasm-gated button in `BoardView`'s `<section class="board">`)
- Test: `crates/web/tests/board.rs` (render test: `.new-game` present)

**Interfaces:**
- Produces: `web::transport::start_new_game()` (wasm-only, `pub`).

- [ ] **Step 1: Add `start_new_game()` to the transport**

In `crates/web/src/transport.rs`, add (next to `clear_saved_id`, near the bottom — the whole file is already `#[cfg(target_arch = "wasm32")]`):
```rust
/// Forget the saved game id and reload, so `bootstrap()` re-enters the picker
/// (no saved id → `await_roster`). The server-side game persists; this only
/// drops the local pointer. Pairs with the "New game" button in `BoardView`.
pub fn start_new_game() {
    clear_saved_id();
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}
```

- [ ] **Step 2: Confirm it compiles for wasm**

Run: `cargo build -p web --target wasm32-unknown-unknown`
Expected: clean (`web-sys` `Window`/`Location` features are already enabled).

- [ ] **Step 3: Write the failing render test**

In `crates/web/tests/board.rs`, add a test mirroring the existing `empty_board_renders_placeholder_without_panels` mount (it mounts `BoardView` against a fresh store with no game). Add:
```rust
#[wasm_bindgen_test]
async fn board_renders_a_new_game_button() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    leptos::task::tick().await;

    // Scope to the last mounted .board (DOM accumulates across tests).
    let boards = leptos::prelude::document()
        .query_selector_all(".board")
        .expect("query_selector_all");
    let last = boards
        .item(boards.length() - 1)
        .expect("at least one .board section")
        .dyn_into::<web_sys::Element>()
        .expect("Element");
    assert!(
        last.query_selector(".new-game").expect("query").is_some(),
        "BoardView must render a .new-game button"
    );
}
```
(The file already imports `RwSignal`, `ClientState`, `BoardView`, `provide_context`, `wasm_bindgen::JsCast as _`, `web_sys` via the existing tests — reuse those imports; add only what the compiler says is missing.)

- [ ] **Step 4: Run it to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test board`
Expected: FAIL — no `.new-game` element yet.

- [ ] **Step 5: Add the wasm-gated button to `BoardView`**

In `crates/web/src/board.rs`, change the `view!` in `BoardView`:
```rust
    view! {
        <section class="board">
            <p class="status">"status: " {status}</p>
            <p class="rejection">"rejection: " {rejection}</p>
            {board}
        </section>
    }
```
to insert the button after the rejection line:
```rust
    view! {
        <section class="board">
            <p class="status">"status: " {status}</p>
            <p class="rejection">"rejection: " {rejection}</p>
            {
                #[cfg(target_arch = "wasm32")]
                {
                    view! {
                        <button
                            class="new-game"
                            on:click=move |_| crate::transport::start_new_game()
                        >
                            "New game"
                        </button>
                    }
                    .into_any()
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ().into_any()
                }
            }
            {board}
        </section>
    }
```
(`IntoAny` is already in scope via `leptos::prelude::*` — the `board` closure above already uses `.into_any()`.)

- [ ] **Step 6: Run the render test (green) + host build**

Run: `wasm-pack test --headless --firefox crates/web --test board`
Expected: PASS (the new test + existing board tests).
Run: `cargo clippy -p web --all-targets --all-features -- -D warnings`
Expected: clean — the host build sees the `#[cfg(not(wasm32))]` `().into_any()` arm and never references `crate::transport::start_new_game`.

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/transport.rs crates/web/src/board.rs crates/web/tests/board.rs
git commit -m "web: a New game button that clears the saved id and reloads (#477)

start_new_game() forgets the localStorage game id and reloads, so bootstrap()
re-enters the picker. Surfaced as a wasm-gated button in BoardView, so you no
longer have to clear localStorage by hand to start over.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Gauntlet, push, PR

- [ ] **Step 1: Full local gauntlet**

Run each (all green):
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Fix `cargo fmt` diffs by running `cargo fmt` and folding into the Task 1 commit.

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin web/new-game-button
gh pr create --fill
```
PR body: one-line what/why (a New game button: clear the saved id + reload → picker), the YAGNI scope (no confirm prompt, no in-app re-bootstrap), and the testing note (render-only; the click reloads, not harness-testable). Ensure the body has `Closes #477.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`. Fix failures with follow-up commits (no force-push).

- [ ] **Step 4: Phase doc**

No phase-7 doc change required: #477 is a `p2-later` QoL affordance, not gate-work.

- [ ] **Step 5: Merge only after explicit user approval**

Do not merge autonomously. On approval:
```bash
gh pr merge <PR#> --squash --delete-branch
```
Confirm #477 auto-closed; `git pull` on `main`.

## Self-Review

**Spec coverage:**
- `transport::start_new_game()` (clear id + reload) → Task 1, Step 1. ✓
- Wasm-gated button in `BoardView` → Task 1, Step 5. ✓
- cfg gating (host renders nothing; host clippy builds) → Task 1, Step 5/6 + Global Constraints. ✓
- Render test; click not harness-testable (noted) → Task 1, Step 3. ✓
- No deps / YAGNI (no prompt, no in-app re-bootstrap) → Global Constraints; nothing adds them. ✓
- Closes #477; no phase doc → Task 2. ✓

**Placeholder scan:** Every code step carries complete code. The Task 1 Step 3 note ("reuse the file's existing imports; add what the compiler flags") points at the real neighbouring test (`empty_board_renders_placeholder_without_panels`) to copy the mount/import shape — not inventable text. No "TBD"/"handle errors"/"similar to Task N".

**Type consistency:** `start_new_game()` (Task 1 Step 1) is the exact symbol the button's `on:click` calls (Step 5) and the test guards via `.new-game` (Step 3). The `#[cfg]` split matches `app.rs`'s established pattern.
