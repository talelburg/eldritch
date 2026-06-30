# In-Play Card Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render in-play assets as `Card` rectangles showing their printed face (minus the cost corner) plus live per-instance state — exhausted (dim + badge), uses chips, and soak chips — by extending the existing `Card` component with an optional `in_play` prop.

**Architecture:** Add a pure `live_state_chips` helper (uses + soak strings from a `CardInPlay` instance against the asset's capacity), give `Card` an optional `in_play: Option<CardInPlay>` prop that drops the cost corner and adds the exhausted styling/badge + live chips when present, and swap the board's in-play `<ul>` text list for a `.card-row` of `Card`s. Hand cards (the `None` path) are unchanged.

**Tech Stack:** Rust, Leptos (CSR), Trunk, `wasm-bindgen-test` (headless Firefox), the engine's `card_registry` + `cards::by_code` for metadata.

## Global Constraints

- **Warnings are errors in CI** across seven jobs (native + wasm). Before pushing, match: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Clippy `pedantic` is on** (`module_name_repetitions` / `must_use_candidate` allowed); `doc_markdown` enforced — backtick-quote type names / `ArkhamDB` in doc comments.
- **wasm-only test files** carry crate-level `#![cfg(target_arch = "wasm32")]`.
- **Headless tests share one browser page** (DOM accumulates); scope presence/absence assertions to the last mounted subtree / a specific container.
- **`UseKind` and `CardInPlay` are `#[non_exhaustive]`** — match `UseKind` with a `_ =>` arm; construct `CardInPlay` via `CardInPlay::enter_play(code, id)` then mutate public fields (never a struct literal).
- **Card stats come only from the corpus / instance** — never hand-typed. Verified codes: Beat Cop `01018` (Asset, `health: 2`, `sanity: 2`), Machete `01020` (Asset, no soak capacity).
- **Display-only:** no click handlers / no `OutboundTx`.

## File structure

- **Modify `crates/web/src/card.rs`** — add `live_state_chips` + `use_kind_label` (pure) with native tests; add the `in_play` prop + overlay rendering to the `Card` component.
- **Modify `crates/web/style.css`** — `.card--exhausted` dim, the `.card-exhausted` badge, optional `.chip--live` tint; (Task 3) remove the dead `.in-play ul` rule.
- **Modify `crates/web/tests/card.rs`** — headless tests for the in-play overlay.
- **Modify `crates/web/src/board.rs`** — in-play list renders `<Card>`s.
- **Modify `crates/web/tests/board.rs`** — assert the in-play section renders `.card-row .card`.

Type/path notes (verified):
- `use game_core::state::{CardCode, CardInPlay, CardInstanceId, UseKind};` — all re-exported from `game_core::state`.
- `CardInPlay` public fields used: `exhausted: bool`, `uses: BTreeMap<UseKind, u8>`, `accumulated_damage: u8`, `accumulated_horror: u8`, `code: CardCode`.
- Soak capacity: `CardKind::Asset { health: Option<u8>, sanity: Option<u8>, .. }`.
- `cards::by_code(code: &str) -> Option<&'static CardMetadata>` — direct corpus lookup, no registry install needed (native-test friendly).
- The `Card` component currently is `#[allow(clippy::needless_pass_by_value)] #[component] pub fn Card(code: CardCode) -> impl IntoView` at `card.rs:282`; its `Some(face)` arm builds `cost_view`/`fast_view`/`slot_views`/`skill_views` and the `<div class="card-head">` / `<div class="card-footer">`.

---

### Task 1: `live_state_chips` pure helper

The uses + soak chip strings for an in-play instance. Pure, native-testable.

**Files:**
- Modify: `crates/web/src/card.rs`

**Interfaces:**
- Consumes: `CardKind` (already imported), `CardInPlay`, `UseKind` (new imports).
- Produces:
  - `pub fn live_state_chips(inst: &CardInPlay, kind: &CardKind) -> Vec<String>` — uses chips first (`"{n} {kind}"`), then soak chips (`"dmg {dmg}/{cap}"`, `"hor {hor}/{cap}"`) only for an `Asset` with that capacity. `[]` for a plain asset.
  - private `use_kind_label(kind: UseKind) -> &'static str`.

- [ ] **Step 1: Extend the imports**

In `crates/web/src/card.rs`, change line 7 from:

```rust
use game_core::state::CardCode;
```

to:

```rust
use game_core::state::{CardCode, CardInPlay, UseKind};
```

- [ ] **Step 2: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `crates/web/src/card.rs`:

```rust
    use game_core::state::{CardInPlay as TestCardInPlay, CardInstanceId};

    #[test]
    fn live_state_chips_soak_uses_real_capacity() {
        // Beat Cop 01018 is an ally asset with health 2 / sanity 2.
        let meta = cards::by_code("01018").expect("Beat Cop in corpus");
        let mut inst = TestCardInPlay::enter_play(CardCode::new("01018"), CardInstanceId(0));
        inst.accumulated_damage = 1;
        assert_eq!(
            live_state_chips(&inst, &meta.kind),
            vec!["dmg 1/2".to_string(), "hor 0/2".to_string()]
        );
    }

    #[test]
    fn live_state_chips_lists_uses_without_soak_for_plain_asset() {
        // Machete 01020 is an asset with no soak capacity.
        let meta = cards::by_code("01020").expect("Machete in corpus");
        let mut inst = TestCardInPlay::enter_play(CardCode::new("01020"), CardInstanceId(0));
        inst.uses.insert(game_core::state::UseKind::Ammo, 2);
        assert_eq!(live_state_chips(&inst, &meta.kind), vec!["2 ammo".to_string()]);
    }

    #[test]
    fn live_state_chips_empty_for_plain_asset_no_uses() {
        let meta = cards::by_code("01020").expect("Machete in corpus");
        let inst = TestCardInPlay::enter_play(CardCode::new("01020"), CardInstanceId(0));
        assert!(live_state_chips(&inst, &meta.kind).is_empty());
    }
```

> `CardInPlay` is imported at file scope (Step 1) but the test module re-imports it as `TestCardInPlay` plus `CardInstanceId` to keep the test self-contained; `cards::by_code` is the corpus crate, available in tests.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p web card::tests::live_state`
Expected: FAIL — `live_state_chips` not found.

- [ ] **Step 4: Implement the helper**

Add to `crates/web/src/card.rs` (above the `#[cfg(test)]` module; e.g. right after `card_face`):

```rust
/// Display label for a uses pool kind. `UseKind` is `#[non_exhaustive]`, so a
/// future variant falls back to the generic `"uses"`.
fn use_kind_label(kind: UseKind) -> &'static str {
    match kind {
        UseKind::Charges => "charges",
        UseKind::Ammo => "ammo",
        UseKind::Secrets => "secrets",
        UseKind::Supplies => "supplies",
        _ => "uses",
    }
}

/// Live per-instance state of an in-play card as chip strings: uses pools first
/// (`"2 ammo"`), then soak (`"dmg 1/2"` / `"hor 0/2"`) for an `Asset` that has
/// that capacity. Empty for a plain asset with no uses and no soak capacity.
#[must_use]
pub fn live_state_chips(inst: &CardInPlay, kind: &CardKind) -> Vec<String> {
    let mut chips = Vec::new();
    for (use_kind, n) in &inst.uses {
        chips.push(format!("{n} {}", use_kind_label(*use_kind)));
    }
    if let CardKind::Asset { health, sanity, .. } = kind {
        if let Some(cap) = health {
            chips.push(format!("dmg {}/{cap}", inst.accumulated_damage));
        }
        if let Some(cap) = sanity {
            chips.push(format!("hor {}/{cap}", inst.accumulated_horror));
        }
    }
    chips
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p web card::tests`
Expected: PASS (the three new tests + all prior card tests).

- [ ] **Step 6: Commit**

```bash
git add crates/web/src/card.rs
git commit -m "web: live_state_chips helper for in-play uses + soak

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 2: `in_play` prop + overlay rendering on `Card`

Add the optional prop; when present, drop the cost corner, dim + badge an exhausted card, and append the live chips. Plus the CSS. Verified by a headless wasm test.

**Files:**
- Modify: `crates/web/src/card.rs` (the `Card` component, `card.rs:282`)
- Modify: `crates/web/style.css`
- Modify: `crates/web/tests/card.rs`

**Interfaces:**
- Consumes: `live_state_chips` (Task 1).
- Produces: `Card` now accepts `#[prop(optional)] in_play: Option<CardInPlay>`. Renders `card--exhausted` on the root + an `Exhausted` badge when the instance is exhausted; omits `.card-cost`; appends `.chip chip--live` chips to the footer.

- [ ] **Step 1: Write the failing headless tests**

Add to `crates/web/tests/card.rs`. First extend its imports — change `use game_core::state::CardCode;` to:

```rust
use game_core::state::{CardCode, CardInPlay, CardInstanceId};
```

Then add these tests (alongside the existing ones):

```rust
/// Class list of the last mounted `.card` element.
fn last_card_classes() -> String {
    let cards = leptos::prelude::document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
        .class_name()
}

#[wasm_bindgen_test]
async fn in_play_exhausted_asset_dims_badges_and_shows_soak() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Beat Cop 01018: ally asset, health 2 / sanity 2.
    let mut inst = CardInPlay::enter_play(CardCode::new("01018"), CardInstanceId(0));
    inst.exhausted = true;
    inst.accumulated_damage = 1;
    leptos::mount::mount_to_body(move || view! { <Card code=CardCode::new("01018") in_play=inst.clone()/> });
    leptos::task::tick().await;

    assert!(last_card_classes().contains("card--exhausted"), "exhausted class missing");
    let html = last_card_html();
    assert!(html.contains("Exhausted"), "exhausted badge missing: {html}");
    assert!(html.contains("dmg 1/2"), "soak chip missing: {html}");
    assert!(!html.contains("card-cost"), "in-play card must not show a cost corner: {html}");
}

#[wasm_bindgen_test]
async fn in_play_ready_asset_is_not_dimmed() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let inst = CardInPlay::enter_play(CardCode::new("01018"), CardInstanceId(0));
    leptos::mount::mount_to_body(move || view! { <Card code=CardCode::new("01018") in_play=inst.clone()/> });
    leptos::task::tick().await;
    assert!(!last_card_classes().contains("card--exhausted"), "ready card must not be dimmed");
}
```

> `last_card_html` already exists in this file (from the hand slice). `dyn_into` needs `wasm_bindgen::JsCast`, already imported as `use wasm_bindgen::JsCast as _;`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `wasm-pack test --headless --firefox crates/web -- card`
Expected: FAIL — `Card` has no `in_play` prop (compile error).

- [ ] **Step 3: Add the prop and compute the in-play locals**

In `crates/web/src/card.rs`, update the component signature and add the in-play locals. Change the `#[allow]` comment + signature at `card.rs:282`:

```rust
// `code` and `in_play` are taken by value: Leptos `#[component]` generates a
// props struct requiring owned fields, so a reference would need a lifetime the
// macro can't express.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn Card(code: CardCode, #[prop(optional)] in_play: Option<CardInPlay>) -> impl IntoView {
```

Then, immediately after the `weakness_view` binding (currently ending at `card.rs:307`) and before `match card_face(&meta.kind) {`, insert:

```rust
    let is_in_play = in_play.is_some();
    let exhausted = in_play.as_ref().is_some_and(|c| c.exhausted);
    let exhausted_badge =
        exhausted.then(|| view! { <span class="card-exhausted">"Exhausted"</span> });
    let live_views: Vec<_> = in_play
        .as_ref()
        .map(|inst| live_state_chips(inst, &meta.kind))
        .unwrap_or_default()
        .into_iter()
        .map(|s| view! { <span class="chip chip--live">{s}</span> })
        .collect();
```

- [ ] **Step 4: Wire the locals into the `Some(face)` arm**

In the `Some(face)` arm, replace the `cost_view` binding:

```rust
            let cost_view = cost_corner.map(|c| view! { <span class="card-cost">{c}</span> });
```

with (cost corner suppressed in play):

```rust
            let cost_view = if is_in_play {
                None
            } else {
                cost_corner.map(|c| view! { <span class="card-cost">{c}</span> })
            };
            let root_class = if exhausted {
                format!("card {class_css} card--exhausted")
            } else {
                format!("card {class_css}")
            };
```

Then change the arm's opening `<div class=format!("card {class_css}")>` to `<div class=root_class>`, add `{exhausted_badge}` to the header after `{fast_view}`, and add the live chips to the footer. The arm's `view!` becomes:

```rust
            view! {
                <div class=root_class>
                    <div class="card-head">
                        {cost_view}
                        <span class="card-name">{name}</span>
                        {fast_view}
                        {exhausted_badge}
                        {weakness_view}
                    </div>
                    <div class="card-traits">{traits}</div>
                    <div class="card-text">{text_view}</div>
                    <div class="card-footer">
                        <span class="card-slots">{slot_views}</span>
                        <span class="card-skills">{skill_views}</span>
                        <span class="card-live">{live_views}</span>
                    </div>
                </div>
            }
            .into_any()
```

> The `None` (generic / non-Asset) arm is unchanged: in-play assets are always `CardKind::Asset`, so the overlay path lives only in the `Some(face)` arm. `exhausted_badge` / `live_views` are consumed only there; the compiler does not warn (they are used in one match arm).

- [ ] **Step 5: Add the CSS**

In `crates/web/style.css`: add `.card-exhausted` to the header-marker selector. Change:

```css
.card-fast, .card-weakness {
```

to:

```css
.card-fast, .card-weakness, .card-exhausted {
```

Then, after the `.card-weakness { color: #c0392b; }` line, add:

```css
.card-exhausted { color: #b06f00; }
.chip--live { background: #e3ecf5; }
```

And after the class-palette block (after the `.card--unknown, .card--generic { ... }` line), add:

```css
/* Exhausted in-play asset: dimmed + dashed so it reads as unavailable.
   Declared after the class palette so its border-color wins over the class hue. */
.card--exhausted { opacity: 0.6; border-style: dashed; border-color: #aaa; }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web -- card`
Expected: PASS (the two new in-play tests + the existing card tests). Confirm native still builds: `cargo test -p web card::tests` (PASS).

- [ ] **Step 7: Verify clippy (both targets) + build**

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings` and `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` and `cargo fmt --check` and `cd crates/web && trunk build`.
Expected: all clean / builds.

- [ ] **Step 8: Commit**

```bash
git add crates/web/src/card.rs crates/web/style.css crates/web/tests/card.rs
git commit -m "web: in_play prop on Card — exhausted dim/badge, uses + soak chips

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 3: Render the in-play list as cards

Swap the board's in-play `<ul>` text list for a `.card-row` of `Card`s carrying the instance; clean up the dead CSS rule.

**Files:**
- Modify: `crates/web/src/board.rs` (the in-play builder + container, `board.rs:88-91` and `:121`)
- Modify: `crates/web/style.css`
- Modify: `crates/web/tests/board.rs`

**Interfaces:**
- Consumes: the `in_play` prop on `Card` (Task 2).

- [ ] **Step 1: Update the board test to expect in-play cards**

In `crates/web/tests/board.rs`, in `investigators_panel_renders_stats_and_hand`, add after the existing in-play assertions (which check `"In play"` and `_synth_asset`):

```rust
    // In-play assets now render as Card rectangles in their own card-row.
    let in_play_cards = leptos::prelude::document()
        .query_selector_all(".in-play .card-row .card")
        .expect("query_selector_all");
    assert!(in_play_cards.length() >= 1, "in-play should render a Card: {html}");
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web -- board`
Expected: FAIL — the in-play section still renders `<li>` text, no `.card-row .card` under `.in-play`.

- [ ] **Step 3: Render in-play as a card row in `board.rs`**

In `crates/web/src/board.rs`, replace the `in_play` builder (currently maps each card to `<li class="card-line">{card_name}</li>`):

```rust
            let in_play: Vec<_> = inv
                .cards_in_play
                .iter()
                .map(|c| view! { <li class="card-line">{crate::names::card_name(&c.code)}</li> })
                .collect();
```

with:

```rust
            let in_play: Vec<_> = inv
                .cards_in_play
                .iter()
                .cloned()
                .map(|c| {
                    let code = c.code.clone();
                    view! { <crate::card::Card code=code in_play=c/> }
                })
                .collect();
```

Then change the in-play container line from:

```rust
                    <div class="in-play"><h4>"In play"</h4><ul>{in_play}</ul></div>
```

to:

```rust
                    <div class="in-play"><h4>"In play"</h4><div class="card-row">{in_play}</div></div>
```

Leave the `threat` builder and its `<ul>` unchanged.

- [ ] **Step 4: Remove the now-dead `.in-play ul` CSS rule**

In `crates/web/style.css`, delete the line:

```css
.in-play ul { list-style: none; padding-left: 0; margin: 0; }
```

(The `.threat ul { ... }` rule is separate and stays.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web -- board`
Expected: PASS (all board tests, including the new in-play `.card-row .card` check).

- [ ] **Step 6: Verify build + clippy**

Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` and `cargo fmt --check` and `cd crates/web && trunk build`.
Expected: clean / builds.

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/board.rs crates/web/style.css crates/web/tests/board.rs
git commit -m "web: render in-play assets as Card rectangles

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 4: Full CI gauntlet + phase doc + PR

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

Expected: all green. Fix any clippy/doc findings (wasm-clippy is the one that sees the new component code).

- [ ] **Step 2: Update the phase-7 doc — only when the PR is ready**

Per the repo convention, the `docs/phases/phase-7-the-gathering.md` update is the **final** commit. Extend the visual-card-rendering bullet (added by the hand slice in the browser-capstone section) to note that **in-play assets** now also render as `Card` rectangles (printed face minus cost corner, plus exhausted dim/badge and uses/soak chips), and that threat-area / locations / enemies / act-agenda remain later slices. Reference the new spec/plan. Keep it to load-bearing residue.

- [ ] **Step 3: Open the PR**

Branch is `web/in-play-cards`. File an issue first (issue-first convention), push, and open the PR with `gh pr create`; `Closes` the issue. Design-decisions paragraph: extend `Card` with an optional `in_play` prop (no parallel component); cost corner dropped in play; exhausted = dim + badge; uses/soak as footer chips reusing `.chip`.

---

## Self-review notes

- **Spec coverage:** `Card` optional `in_play` prop (Task 2) ✓; cost corner dropped in play (Task 2 Step 4) ✓; exhausted dim + badge (Task 2) ✓; uses chips + soak chips with capacity (Task 1 `live_state_chips`) ✓; `live_state_chips` pure + native-tested (Task 1) ✓; board in-play → `.card-row` of `Card`s (Task 3) ✓; dead `.in-play ul` cleanup (Task 3 Step 4) ✓; hand path unchanged (`None` path untouched) ✓; threat/locations/enemies/act-agenda out of scope ✓; native + headless tests ✓.
- **Type consistency:** `live_state_chips(&CardInPlay, &CardKind) -> Vec<String>` defined in Task 1, consumed in Task 2 Step 3. `in_play: Option<CardInPlay>` prop name matches between the signature (Task 2 Step 3) and the call site (`in_play=c`, Task 3 Step 3). `use_kind_label` covers all four `UseKind` variants + `_`. Test construction uses `enter_play` + field mutation (both `#[non_exhaustive]`).
- **Out of scope (unchanged):** threat-area cards, the spatial map (locations/enemies), act/agenda, clickable cards, the icon font, attached cards.
