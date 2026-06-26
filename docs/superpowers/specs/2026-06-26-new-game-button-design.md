# #477 — "New game" button

**Date:** 2026-06-26
**Issue:** #477 (QoL: start a fresh game without clearing `localStorage` by hand).

## Problem

The browser client persists the active game id in `localStorage` (`eldritch_game_id`), and `bootstrap()` reuses it on load. To start over you must manually clear `localStorage` — annoying, especially while testing.

## Solution

A **"New game"** button that drops the local pointer and re-pickers via a page reload.

- **`crates/web/src/transport.rs`** (already `#[cfg(target_arch = "wasm32")]`-only): add
  ```rust
  /// Forget the saved game id and reload, so `bootstrap()` re-enters the
  /// picker (no saved id → `await_roster`). The server-side game persists;
  /// this only drops the local pointer.
  pub fn start_new_game() {
      clear_saved_id();
      if let Some(w) = web_sys::window() {
          let _ = w.location().reload();
      }
  }
  ```
  (`clear_saved_id` already exists; `web-sys`'s `Location`/`Window` features are already enabled — no dep change.)
- **`crates/web/src/board.rs` (`BoardView`)**: render a small `New game` button (class `new-game`) near the status line whose `on:click` calls `crate::transport::start_new_game()`.

### cfg gating

`transport` is wasm-only, but `BoardView` also compiles for native (host clippy / non-wasm). So the button + its handler are wasm-gated using the same `#[cfg(target_arch = "wasm32")] { … .into_any() } #[cfg(not(...))] { ().into_any() }` split `app.rs` already uses for the picker. Native builds render nothing in that slot.

## Scope (YAGNI)

- **No confirm/abandon prompt.** The old game persists server-side; this only drops the local pointer and returns to the picker. Solo-scope.
- **No in-app re-bootstrap** (a "new game" signal threaded into the `connect_once` `select!`). A page reload re-bootstraps for free; the reload flash is acceptable for a dev/solo tool.

## Testing

- **wasm (`crates/web/tests/`)**: a render test asserting the `.new-game` button is present in `BoardView`.
- The **click** reloads the page, which can't be exercised in the `wasm-bindgen-test` harness (it would reload the test page) — verified manually. Called out, not faked.
- Full CI gauntlet (touches `crates/web`, so `wasm-build`/`wasm-test`/`wasm-clippy` matter; host clippy must still build `BoardView` with the wasm-gated button absent).

## Done criteria

- A "New game" button is visible in the browser; clicking it returns to the investigator/scenario picker without manually clearing `localStorage`.
- All seven CI jobs green.
