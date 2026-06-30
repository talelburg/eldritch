# Location Card Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Evolve the spatial map's location nodes into location cards (name, shroud, clues, traits, ability text, victory) with compact investigator/enemy tokens inside, and normalize the grid so a removed location leaves no dead column.

**Architecture:** All in `crates/web/src/map.rs`. `layout_positions` gains a leading-offset normalization (pure, unit-tested). `location_map`'s node rendering gains card fields looked up from the corpus by `loc.code` (reusing `crate::card::{parse_card_text, render_segments}`), only for revealed nodes. The existing `tests/map.rs` (synthetic registry) keeps covering shroud/clues/tokens/revealed; a new `tests/location_card.rs` (real `cards::REGISTRY`, mounting `location_map` directly) covers the metadata fields.

**Tech Stack:** Rust, Leptos (CSR), Trunk, `wasm-bindgen-test` (headless Firefox), the engine's `card_registry`, `cards::REGISTRY`/`cards::by_code`, `game_core::test_support::fixtures`.

## Global Constraints

- **Warnings are errors in CI** across seven jobs (native + wasm). Before pushing: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Clippy `pedantic` is on** (`module_name_repetitions` / `must_use_candidate` allowed); `doc_markdown` enforced — backtick-quote type names / `ArkhamDB` in doc comments.
- **wasm-only test files** carry crate-level `#![cfg(target_arch = "wasm32")]`.
- **Registry-per-process is first-wins (`OnceLock`)** — never mix `install_test_registry` and `cards::REGISTRY` in one test binary. `tests/map.rs` uses the synthetic registry; the new metadata test lives in its own file `tests/location_card.rs` and installs `cards::REGISTRY`.
- **Stats from the `Location`/`Enemy`/`Investigator` structs + corpus metadata** — never hand-typed. Verified codes: Attic `01113` (victory 1, Forced text "Take 1 horror"), Miskatonic University `01129` (traits "Arkham.", victory 1).
- **Display-only:** no click handlers.
- **Unrevealed nodes withhold hidden info** (name + "unrevealed" only).

## File structure

- **Modify `crates/web/src/map.rs`** — `layout_positions` normalization (Task 1); node-as-location-card rendering + larger node/cell dims (Task 2). Pure tests in its `#[cfg(test)] mod tests`.
- **Modify `crates/web/style.css`** — `.map-location` card layout + `.loc-stats` (Task 2).
- **Create `crates/web/tests/location_card.rs`** — headless metadata-field test (Task 2).
- `tests/map.rs` is **unchanged** — its existing tests must keep passing (the redesign is additive).

Type/path notes (verified against the codebase):
- `map.rs` imports today: `use game_core::state::{CardCode, GameState, LocationId};` — add `use game_core::card_data::CardKind;`.
- `layout_positions(locations: &[(LocationId, CardCode)]) -> BTreeMap<LocationId, (u16, u16)>` is `pub(crate)`; `location_map(game: &GameState) -> impl IntoView` is `pub`.
- Node consts today: `CELL_W = 200`, `CELL_H = 150`, `NODE_W = 170`, `NODE_H = 120`.
- `Location` fields: `code: CardCode`, `name: String`, `shroud: u8`, `clues: u8`, `revealed: bool`. `CardKind::Location { shroud, printed_clues, victory: Option<u8> }`.
- `crate::card::parse_card_text` is `pub`; `crate::card::render_segments` is `pub(crate)` (from the enemy slice).
- `tests/map.rs` helper `node_text(loc_name)` returns the `textContent` of `.map-location[data-loc="<name>"]` in the last `.map` — keep `data-loc` on the node's outer `<div>`.

---

### Task 1: Normalize the grid to the origin

`layout_positions` shifts placed nodes so their min col/row is 0, removing the dead column a departed location (the Study) leaves behind. Pure, unit-tested.

**Files:**
- Modify: `crates/web/src/map.rs`

**Interfaces:**
- `layout_positions` keeps its signature; its returned positions are now normalized (min col = 0, min row = 0).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/web/src/map.rs`:

```rust
    #[test]
    fn positions_are_normalized_to_origin() {
        // Post-Study Gathering set — all authored at cols 2-3 (Study's col 0/1
        // is gone). Hallway 01112 (2,1), Attic 01113 (2,0), Cellar 01114 (3,1),
        // Parlor 01115 (2,2).
        let locs = vec![
            (LocationId(1), CardCode::new("01112")),
            (LocationId(2), CardCode::new("01113")),
            (LocationId(3), CardCode::new("01114")),
            (LocationId(4), CardCode::new("01115")),
        ];
        let pos = layout_positions(&locs);
        let min_col = pos.values().map(|(c, _)| *c).min().unwrap();
        let min_row = pos.values().map(|(_, r)| *r).min().unwrap();
        assert_eq!(min_col, 0, "leading empty column not removed: {pos:?}");
        assert_eq!(min_row, 0, "leading empty row not removed: {pos:?}");
        // Relative offset preserved: Cellar one column right of Hallway.
        assert_eq!(
            pos[&LocationId(3)].0,
            pos[&LocationId(1)].0 + 1,
            "relative column offset not preserved: {pos:?}"
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p web map::tests::positions_are_normalized_to_origin`
Expected: FAIL — `min_col` is 2 (Study's column still reserved), not 0.

- [ ] **Step 3: Implement normalization**

In `crates/web/src/map.rs`, in `layout_positions`, replace the final `out` return:

```rust
        out.insert(*id, pos);
    }
    out
}
```

with a normalization pass before returning:

```rust
        out.insert(*id, pos);
    }
    // Normalize: shift so the placed nodes start at (0, 0), dropping any leading
    // empty columns/rows a departed location leaves behind (e.g. the Study's
    // column once Act 1 removes it). Interior gaps are not collapsed (no
    // Core/Dunwich layout has them).
    let min_col = out.values().map(|(c, _)| *c).min().unwrap_or(0);
    let min_row = out.values().map(|(_, r)| *r).min().unwrap_or(0);
    for (col, row) in out.values_mut() {
        *col -= min_col;
        *row -= min_row;
    }
    out
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p web map::tests`
Expected: PASS — the new test plus the existing `layout_positions` tests (they place a fallback node at `(0,0)`, so their min is already 0 and normalization leaves them unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/map.rs
git commit -m "web: normalize the map grid to the origin (no dead column)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 2: Render location nodes as cards

Add the corpus-driven card fields (traits, ability text, victory) to revealed nodes, enrich the enemy token, enlarge the node dimensions, style it, and add the metadata headless test.

**Files:**
- Modify: `crates/web/src/map.rs` (the node consts + `location_map`'s node rendering; add the `CardKind` import)
- Modify: `crates/web/style.css`
- Create: `crates/web/tests/location_card.rs`

**Interfaces:**
- Consumes: `crate::card::{parse_card_text, render_segments}`; the registry.
- Produces: revealed `.map-location` nodes render `shroud N` (chip), `clues N`, traits, ability text, and a `Victory n` chip (when present); `data-loc` stays on the node's outer `<div>`.

- [ ] **Step 1: Write the failing metadata test**

Create `crates/web/tests/location_card.rs`:

```rust
//! Headless render test for the location-card fields that come from the corpus
//! (traits / ability text / victory). Its own binary so it can install the real
//! `cards::REGISTRY` without colliding with `tests/map.rs`'s synthetic registry
//! (registry install is first-wins per process). Mounts `location_map` directly
//! (no investigator panel → no TEST_INV capacity lookup). wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::state::{CardCode, GameStateBuilder};
use game_core::test_support::fixtures::test_location;
use leptos::prelude::document;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// `textContent` of the last-mounted map node whose `data-loc` equals `name`.
fn node_text(name: &str) -> String {
    let maps = document().query_selector_all(".map").expect("query ok");
    let last = maps
        .item(maps.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .map section");
    last.query_selector(&format!(".map-location[data-loc=\"{name}\"]"))
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn revealed_location_shows_metadata_text_and_victory() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Attic 01113: victory 1, Forced text "After you enter the Attic: Take 1 horror."
    let mut attic = test_location(1, "Attic");
    attic.code = CardCode::new("01113");
    attic.revealed = true;
    let game = GameStateBuilder::new().with_location(attic).build();
    leptos::mount::mount_to_body(move || web::map::location_map(&game));
    leptos::task::tick().await;

    let text = node_text("Attic");
    assert!(text.contains("Victory 1"), "victory chip missing: {text}");
    assert!(text.contains("horror"), "ability text missing: {text}");
}

#[wasm_bindgen_test]
async fn revealed_location_shows_metadata_traits() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Miskatonic University 01129: traits "Arkham."
    let mut misk = test_location(2, "Miskatonic University");
    misk.code = CardCode::new("01129");
    misk.revealed = true;
    let game = GameStateBuilder::new().with_location(misk).build();
    leptos::mount::mount_to_body(move || web::map::location_map(&game));
    leptos::task::tick().await;

    assert!(
        node_text("Miskatonic University").contains("Arkham"),
        "traits missing: {}",
        node_text("Miskatonic University")
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test location_card`
Expected: FAIL — nodes don't render traits/text/victory yet (no "Victory 1"/"horror"/"Arkham" in the node).

- [ ] **Step 3: Enlarge the node dimensions + import `CardKind`**

In `crates/web/src/map.rs`, add the import near the other `use` lines:

```rust
use game_core::card_data::CardKind;
```

Then bump the node/cell consts so a card-sized node with a few fields fits (`overflow: hidden` stays a backstop). Replace:

```rust
const CELL_W: u16 = 200;
const CELL_H: u16 = 150;
const NODE_W: u16 = 170;
const NODE_H: u16 = 120;
```

with:

```rust
const CELL_W: u16 = 260;
const CELL_H: u16 = 250;
const NODE_W: u16 = 230;
const NODE_H: u16 = 220;
```

- [ ] **Step 4: Render the card fields**

In `crates/web/src/map.rs`, in `location_map`, replace the node-rendering block (the `head` binding through the node `view!`):

```rust
            let head = if loc.revealed {
                format!("{} (shroud {} · clues {})", loc.name, loc.shroud, loc.clues)
            } else {
                loc.name.clone()
            };
            view! {
                <div class=node_class data-loc=loc.name.clone() style=style>
                    <div class="loc-head">{head}</div>
                    <span class="loc-revealed">{revealed_label}</span>
                    {invs}
                    {enemies}
                </div>
            }
```

with:

```rust
            // Revealed: a location card (name + shroud chip, traits, ability
            // text, clues + victory chip), with traits/text/victory from the
            // corpus by code (absent when no metadata — synthetic registry /
            // unknown code). Unrevealed: name only (hidden info withheld).
            let detail = loc.revealed.then(|| {
                let meta = game_core::card_registry::current()
                    .and_then(|r| (r.metadata_for)(&loc.code));
                let traits = meta
                    .map(|m| {
                        if m.traits.is_empty() {
                            String::new()
                        } else {
                            format!("{}.", m.traits.join(". "))
                        }
                    })
                    .unwrap_or_default();
                let text_view = meta
                    .and_then(|m| m.text.as_deref())
                    .map(|t| crate::card::render_segments(crate::card::parse_card_text(t)));
                let victory = meta.and_then(|m| match &m.kind {
                    CardKind::Location { victory, .. } => *victory,
                    _ => None,
                });
                let victory_chip =
                    victory.map(|n| view! { <span class="chip">{format!("Victory {n}")}</span> });
                view! {
                    <div class="loc-card">
                        <div class="loc-head">
                            {loc.name.clone()}
                            <span class="chip">{format!("shroud {}", loc.shroud)}</span>
                        </div>
                        <div class="card-traits">{traits}</div>
                        <div class="card-text">{text_view}</div>
                        <div class="loc-stats">
                            <span>{format!("clues {}", loc.clues)}</span>
                            {victory_chip}
                        </div>
                    </div>
                }
            });
            let unrevealed_head = (!loc.revealed)
                .then(|| view! { <div class="loc-head">{loc.name.clone()}</div> });
            view! {
                <div class=node_class data-loc=loc.name.clone() style=style>
                    {detail}
                    {unrevealed_head}
                    <span class="loc-revealed">{revealed_label}</span>
                    {invs}
                    {enemies}
                </div>
            }
```

- [ ] **Step 5: Enrich the unengaged-enemy token**

In `crates/web/src/map.rs`, in `location_map`, replace the enemy-token builder body:

```rust
                    view! {
                        <div class="enemy-token">
                            {e.name.clone()} " " {e.damage} "/" {e.max_health}
                        </div>
                    }
```

with (label the health and mark exhaustion):

```rust
                    view! {
                        <div class="enemy-token">
                            {e.name.clone()} " health " {e.damage} "/" {e.max_health}
                            {e.exhausted.then(|| view! { <span>" (exhausted)"</span> })}
                        </div>
                    }
```

- [ ] **Step 6: Add the CSS**

In `crates/web/style.css`, replace the `.map-location` rule:

```css
.map-location { position: absolute; z-index: 1; box-sizing: border-box; border: 1px solid #333; border-radius: 4px; background: #fff; padding: 0.25rem; font-size: 0.8rem; overflow: hidden; }
```

with a flex-column card layout (same box, plus stacking) and a stats row:

```css
.map-location { position: absolute; z-index: 1; box-sizing: border-box; border: 1px solid #333; border-radius: 4px; background: #fff; padding: 0.35rem; font-size: 0.8rem; overflow: hidden; display: flex; flex-direction: column; gap: 0.2rem; }
.loc-card { display: flex; flex-direction: column; gap: 0.2rem; }
.loc-stats { display: flex; gap: 0.4rem; align-items: baseline; }
```

- [ ] **Step 7: Run the new + existing map tests**

Run: `wasm-pack test --headless --firefox crates/web --test location_card`
Expected: PASS (2 tests — Victory 1 / horror / Arkham now render).
Run: `wasm-pack test --headless --firefox crates/web --test map`
Expected: PASS (existing map tests still green — the redesign keeps the `revealed`/`unrevealed` labels, the `shroud`/`clues` text, `data-loc`, and the tokens).
Run: `cargo test -p web map::tests`
Expected: PASS (Task 1's pure tests).

- [ ] **Step 8: Verify clippy (both targets) + build**

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings` and `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` and `cargo fmt --check` and `cd crates/web && trunk build`.
Expected: all clean / builds.

- [ ] **Step 9: Commit**

```bash
git add crates/web/src/map.rs crates/web/style.css crates/web/tests/location_card.rs
git commit -m "web: render map nodes as location cards (traits, text, victory)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 3: Full CI gauntlet + phase doc + PR

- [ ] **Step 1: Run every CI job locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green.

- [ ] **Step 2: Update the phase-7 doc — only when the PR is ready**

Extend the visual-card-rendering bullet in `docs/phases/phase-7-the-gathering.md` (browser-capstone section) with slice 4: the map's nodes now render as location cards (name, shroud, clues, traits, ability text, victory) with compact investigator/enemy tokens inside; the grid is normalized to the origin (no dead column when the Study leaves play); the unengaged-enemy tokens deferred from the enemy slice now render here. Note interior-gap collapse and clickable locations remain out of scope. Reference the new spec/plan.

- [ ] **Step 3: Open the PR**

Branch is `web/location-cards`. File an issue first (issue-first convention), push, open the PR with `gh pr create`; `Closes` the issue. Design-decisions paragraph: keep the spatial map; leading-offset grid normalization; nodes-as-location-cards with corpus fields by code; tokens stay compact; metadata test in its own binary (registry first-wins).

---

## Self-review notes

- **Spec coverage:** grid normalization (Task 1) ✓; nodes-as-location-cards with name/shroud/clues/traits/text/victory (Task 2) ✓; tokens compact + unengaged enemies in nodes (Task 2 Step 5 + existing render) ✓; unrevealed withholds info (Task 2 Step 4 — `detail` only when revealed) ✓; connection lines follow normalized positions (derive from `layout_positions`) ✓; pure normalization test + headless metadata test + existing map tests stay green (Task 1/2) ✓.
- **Type consistency:** `layout_positions` signature unchanged; `CardKind::Location { victory, .. }` matches the corpus enum; `render_segments`/`parse_card_text` paths match the card module; `data-loc` retained for `node_text`. The new test file mounts `web::map::location_map` (pub) directly.
- **Registry discipline:** `tests/location_card.rs` is a separate binary installing `cards::REGISTRY`; `tests/map.rs` keeps the synthetic registry — never mixed in one process.
- **Out of scope (unchanged):** treachery/act/agenda cards, interior-gap collapse, clickable locations, full cards in nodes, the icon font.
