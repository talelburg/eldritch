# Entity Names in the Web UI (#484) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render card and location *names* (e.g. "Magnifying Glass", "Study") in the web board and commit-hand UI instead of raw codes / `loc {id}`, with a code/id fallback.

**Architecture:** Add a small unconditional `crate::names` web module with two pure helpers — `card_name` (via the already-installed `cards::REGISTRY` through `game_core::card_registry`) and `location_name` (via `GameState::locations`) — and swap the raw-code/id render sites in `board.rs` and `input.rs` to use them.

**Tech Stack:** Rust, Leptos (web, wasm32 + native reducer build), `game_core::card_registry` (`CardMetadata.name`), `wasm-bindgen-test`.

## Global Constraints

- **CI gauntlet (warnings-as-errors).** Before pushing run all of: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **UI display only.** No engine prompt strings, no engine/server changes.
- **Name only.** Render the name; fall back to the raw code / `loc {id}` only when unavailable.
- **`crate::names` is unconditional** (not wasm-gated): `board.rs` (unconditional) and the wasm-only `input.rs` both consume it.

---

### Task 1: `crate::names` helpers

**Files:**
- Create: `crates/web/src/names.rs`
- Modify: `crates/web/src/lib.rs` (add `pub mod names;`)
- Test: `crates/web/src/names.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `pub fn card_name(code: &game_core::state::CardCode) -> String`
  - `pub fn location_name(game: &game_core::state::GameState, id: game_core::state::LocationId) -> String`

- [ ] **Step 1: Write the module with failing tests**

Create `crates/web/src/names.rs`:

```rust
//! Display-name helpers for the web UI (#484): resolve entity codes/ids to their
//! printed names, falling back to the raw code/id when unavailable. UI display
//! only — never used for engine input.

use game_core::state::{CardCode, GameState, LocationId};

/// Printed card name for `code`, or the raw code when the name is unavailable —
/// an unimplemented-stub card (no metadata) or the card registry not installed
/// (e.g. a headless/native render path). The registry is installed by the web
/// binary at startup (`main.rs`).
pub fn card_name(code: &CardCode) -> String {
    game_core::card_registry::current()
        .and_then(|r| (r.metadata_for)(code))
        .map(|m| m.name.clone())
        .unwrap_or_else(|| code.to_string())
}

/// Display name for a location id, or "loc {id}" when it is not in state.
pub fn location_name(game: &GameState, id: LocationId) -> String {
    game.locations
        .get(&id)
        .map(|l| l.name.clone())
        .unwrap_or_else(|| format!("loc {}", id.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::GameStateBuilder;
    use game_core::test_support::fixtures::test_location;

    #[test]
    fn card_name_returns_printed_name_with_registry() {
        // The web crate depends on `cards`; installing its registry is idempotent
        // (OnceLock, first-wins) and safe in the web lib test binary, which has no
        // competing installer.
        let _ = game_core::card_registry::install(cards::REGISTRY);
        assert_eq!(card_name(&CardCode::new("01030")), "Magnifying Glass");
    }

    #[test]
    fn card_name_falls_back_to_code_for_unknown() {
        // Unknown code ⇒ no metadata ⇒ the raw code is shown (registry-agnostic).
        assert_eq!(card_name(&CardCode::new("99999")), "99999");
    }

    #[test]
    fn location_name_returns_state_name_then_falls_back() {
        let state = GameStateBuilder::new()
            .with_location(test_location(10, "Study"))
            .build();
        assert_eq!(location_name(&state, LocationId(10)), "Study");
        assert_eq!(location_name(&state, LocationId(99)), "loc 99");
    }
}
```

In `crates/web/src/lib.rs`, add alongside the other unconditional modules (e.g. after `pub mod board;`):

```rust
pub mod names;
```

- [ ] **Step 2: Run the tests to verify they fail/pass appropriately**

Run: `cargo test -p web --lib names`
Expected: the module compiles and all three tests PASS. (They are written against the final impl, which is in this same file — there is no separate red step for a pure helper module; if `cards::REGISTRY` or `CardMetadata.name` were mis-referenced the module would fail to compile, which is the red signal.)

If `card_name_returns_printed_name_with_registry` fails because another web lib test installed a different registry first (it shouldn't — only this module installs one), move that single assertion into the Task 2 wasm test (which installs `cards::REGISTRY` in its own binary) and keep only the fallback tests here.

- [ ] **Step 3: Run the web lib suite + clippy**

Run: `RUSTFLAGS="-D warnings" cargo test -p web --lib` then `cargo clippy -p web --all-targets --all-features -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/web/src/names.rs crates/web/src/lib.rs
git commit -m "web: card_name / location_name display helpers (#484)"
```

---

### Task 2: Render names at the board + commit-hand sites

**Files:**
- Modify: `crates/web/src/board.rs` (lines 3, 109, 113, 118, 157)
- Modify: `crates/web/src/input.rs` (the `PickMultiple` hand-card label, ~line 177)
- Test: `crates/web/tests/entity_names.rs` (create — own binary, installs `cards::REGISTRY`)

**Interfaces:**
- Consumes: `crate::names::card_name`, `crate::names::location_name` (Task 1).

- [ ] **Step 1: Write the failing wasm render test**

Create `crates/web/tests/entity_names.rs` (its own binary, so it installs the real `cards::REGISTRY` without colliding with `tests/board.rs`'s synthetic registry):

```rust
//! #484: the board renders card/location *names*, not raw codes/ids.
//! wasm32-only (browser DOM). Own test binary so it can install the real
//! `cards::REGISTRY` (a code→name source) without colliding with other binaries.
#![cfg(target_arch = "wasm32")]

use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::fixtures::{test_investigator, test_location};
use game_core::EngineOutcome;
use leptos::prelude::{provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn board_renders_card_and_location_names() {
    // Install the real corpus registry (the code→name source). Idempotent
    // (OnceLock, first-wins); `web` has no `ctor` dev-dep, so install in-test.
    let _ = game_core::card_registry::install(cards::REGISTRY);

    let inv_id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.hand.push(CardCode::new("01030")); // Magnifying Glass

    let state = GameStateBuilder::new()
        .with_active_investigator(inv_id)
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .build();

    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
            },
        );
    });
    leptos::task::tick().await;

    let text = leptos::prelude::document()
        .query_selector(".board")
        .expect("query")
        .expect(".board present")
        .text_content()
        .unwrap_or_default();
    assert!(text.contains("Magnifying Glass"), "hand card name shown: {text}");
    assert!(!text.contains("01030"), "raw card code must not appear: {text}");
    assert!(text.contains("Study"), "location name shown: {text}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test entity_names`
Expected: FAIL — the board still renders `01030` and `loc 10` (the assertions on "Magnifying Glass" / no-`01030` / "Study" fail).

- [ ] **Step 3: Swap the `board.rs` render sites**

In `crates/web/src/board.rs`:

Remove the stale clause in the module doc (line 3) — change:

```rust
//! `CardCode` strings — the client has no card-name source.
```

to:

```rust
//! card/location names via `crate::names` (the client installs `cards::REGISTRY`).
```

Hand cards (line ~113) — change `{code.to_string()}` to `{crate::names::card_name(code)}`:

```rust
                .map(|code| view! { <li class="card">{crate::names::card_name(code)}</li> })
```

Cards in play (line ~118) — change `{c.code.to_string()}`:

```rust
                .map(|c| view! { <li class="card">{crate::names::card_name(&c.code)}</li> })
```

Investigator location (line ~109) — change the `map_or_else`:

```rust
            let location = inv
                .current_location
                .map_or_else(|| "—".to_string(), |id| crate::names::location_name(game, id));
```

Enemy location (line ~157) — change the `map_or_else`:

```rust
            let location = e
                .current_location
                .map_or_else(|| "—".to_string(), |id| crate::names::location_name(game, id));
```

(Both panel fns already take `game: &GameState`, so `game` is in scope. The `use game_core::state::{… LocationId}` import may now be unused in `board.rs` — if clippy flags it, drop `LocationId` from the import.)

- [ ] **Step 4: Swap the `input.rs` commit-hand label**

In `crates/web/src/input.rs`, the `PickMultiple` branch labels each hand button with the raw code (`{code}`, ~line 177). `active_hand` returns `Vec<String>` (codes). Change the button label to the card name (construct a `CardCode` from the code string):

```rust
                                        {crate::names::card_name(&game_core::state::CardCode::new(code))}
```

(The `code` String is otherwise only used for this label; the selection uses the hand index `i`.)

- [ ] **Step 5: Run the render test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web --test entity_names`
Expected: PASS.

- [ ] **Step 6: Run the full web suites**

Run: `cargo test -p web` then `wasm-pack test --headless --firefox crates/web`
Expected: PASS. The existing `tests/board.rs` uses the synthetic registry (no metadata for real codes) and asserts on its synthetic fixtures — confirm it still passes; if any assertion looked for a raw code that is now name-resolved, it would only be affected for codes the synthetic registry knows (it knows none of the real corpus, so hand/in-play codes fall back to the code unchanged). Fix any genuinely affected assertion.

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/board.rs crates/web/src/input.rs crates/web/tests/entity_names.rs
git commit -m "web: render card/location names on the board + commit hand (#484)"
```

---

### Final: full CI gauntlet

- [ ] **Run the complete gauntlet** (all seven jobs, from Global Constraints). Fix any `fmt`/`clippy`/`doc` findings (watch for a now-unused `LocationId` import in `board.rs`).
- [ ] **No phase-doc update.** #484 is an unmilestoned `ui`/`p2-later` QoL issue not tracked in any `docs/phases/*` doc (consistent with the sibling web-QoL issues).

## Notes for the implementer

- **The name source already exists** — `crates/web/src/main.rs` installs `cards::REGISTRY` at startup, and `web` depends on `cards` (wasm-buildable). `card_name` reads it via `game_core::card_registry::current()`; no new plumbing.
- **Fallbacks are load-bearing** for tests and unimplemented-stub cards: `card_name` → raw code, `location_name` → `loc {id}`. Never panic on a missing name.
- Design doc: `docs/superpowers/specs/2026-06-27-484-entity-names-in-ui-design.md`.
```
