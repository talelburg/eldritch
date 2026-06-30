# Enemy Card Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render engaged enemies as `EnemyCard` rectangles (combat stats, keyword chips, traits, ability text, exhausted dim/badge, red border) in the investigator's threat area, via a dedicated component reading from the `Enemy` state struct.

**Architecture:** A new `crates/web/src/enemy_card.rs` holds pure chip helpers (`enemy_stat_chips`, `enemy_keyword_chips`) and the `EnemyCard` component, which reuses the card CSS / chip vocabulary and the text renderer (`render_segments` promoted to `pub(crate)`). The board's engaged-enemy text list becomes a `.card-row` of `EnemyCard`s; treacheries and the map are untouched. Display-only.

**Tech Stack:** Rust, Leptos (CSR), Trunk, `wasm-bindgen-test` (headless Firefox), the engine's `card_registry` for enemy ability text, `game_core::test_support::fixtures::test_enemy` for tests.

## Global Constraints

- **Warnings are errors in CI** across seven jobs (native + wasm). Before pushing: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Clippy `pedantic` is on** (`module_name_repetitions` / `must_use_candidate` allowed); `doc_markdown` enforced — backtick-quote type names (`Enemy`, `Card`, `CardInPlay`, …) and `ArkhamDB` in doc comments.
- **wasm-only test files** carry crate-level `#![cfg(target_arch = "wasm32")]`.
- **Headless tests share one browser page** (DOM accumulates); scope presence/absence assertions to the last mounted subtree / a specific container.
- **`Enemy` is `#[non_exhaustive]`** — construct it in tests via `game_core::test_support::fixtures::test_enemy(id, name)` then mutate public fields, never a struct literal.
- **Stats come only from the `Enemy` struct / corpus** — never hand-typed. The `test_enemy` fixture defaults: `fight: 2, evade: 2, max_health: 2, damage: 0, attack_damage: 1, attack_horror: 0, hunter: false, retaliate: false, victory: None, exhausted: false`.
- **Display-only:** no click handlers / no `OutboundTx`.

## File structure

- **Create `crates/web/src/enemy_card.rs`** — `enemy_stat_chips` + `enemy_keyword_chips` (pure, native-tested) and the `EnemyCard` component.
- **Modify `crates/web/src/lib.rs`** — add `pub mod enemy_card;`.
- **Modify `crates/web/src/card.rs`** — promote `render_segments` from private to `pub(crate)` (Task 2).
- **Create `crates/web/tests/enemy_card.rs`** — headless render tests for `EnemyCard`.
- **Modify `crates/web/src/board.rs`** — engaged enemies render as `EnemyCard`s in a `.card-row`.
- **Modify `crates/web/style.css`** — `.card--enemy` border + an optional `.chip--keyword` tint.
- **Modify `crates/web/tests/board.rs`** — assert an engaged enemy renders as `.threat .card-row .card`.

Type/path notes (verified):
- `use game_core::state::Enemy;` — re-exported at `game_core::state::Enemy`. Fields used: `name: String`, `traits: Vec<String>`, `code: CardCode`, `fight: i8`, `evade: i8`, `max_health: u8`, `damage: u8`, `attack_damage: u8`, `attack_horror: u8`, `hunter: bool`, `retaliate: bool`, `victory: Option<u8>`, `exhausted: bool`, `engaged_with: Option<InvestigatorId>`.
- `crate::card::parse_card_text` is already `pub`; `crate::card::render_segments` is private — Task 2 makes it `pub(crate)`.
- Registry text lookup: `game_core::card_registry::current().and_then(|r| (r.metadata_for)(&enemy.code)).and_then(|m| m.text.as_deref())`.
- `GameStateBuilder::with_enemy(enemy)` exists (`builder.rs:155`); `test_investigator(1).id == InvestigatorId(1)`.
- The board engaged builder is at `board.rs:102`; the threat container at `board.rs:126` is `<div class="threat"><h4>"Threat area"</h4><ul>{threat}{engaged}</ul></div>`.

---

### Task 1: Pure enemy chip helpers

`enemy_stat_chips` + `enemy_keyword_chips`. Pure, native-testable. Creates the file and registers the module.

**Files:**
- Create: `crates/web/src/enemy_card.rs`
- Modify: `crates/web/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub fn enemy_stat_chips(enemy: &Enemy) -> Vec<String>` — `["fight {fight}", "evade {evade}", "health {damage}/{max_health}", "attack: {attack_damage} dmg · {attack_horror} hor"]`.
  - `pub fn enemy_keyword_chips(enemy: &Enemy) -> Vec<String>` — `"Hunter"` if `hunter`, `"Retaliate"` if `retaliate`, `"Victory {n}"` if `victory == Some(n)`; that order; `[]` if none.

- [ ] **Step 1: Register the module**

In `crates/web/src/lib.rs`, add alongside the other non-gated `pub mod` lines (e.g. after `pub mod card;`):

```rust
pub mod enemy_card;
```

- [ ] **Step 2: Write the file with the failing tests**

Create `crates/web/src/enemy_card.rs`:

```rust
//! Visual rendering of enemies for the web client. Enemies are a distinct data
//! source — the `Enemy` state struct carries stats *and* live state — so they
//! get a dedicated `EnemyCard` component rather than reusing `Card` (which is
//! built around registry lookup + a `CardInPlay`). Shares the card CSS / chip
//! vocabulary and the text renderer. Display-only.

use game_core::state::Enemy;

/// Combat stat chips for an enemy: fight, evade, health (damage/max), attack
/// (damage + horror), in that order.
#[must_use]
pub fn enemy_stat_chips(enemy: &Enemy) -> Vec<String> {
    vec![
        format!("fight {}", enemy.fight),
        format!("evade {}", enemy.evade),
        format!("health {}/{}", enemy.damage, enemy.max_health),
        format!(
            "attack: {} dmg · {} hor",
            enemy.attack_damage, enemy.attack_horror
        ),
    ]
}

/// Keyword / victory chips present on an enemy: `"Hunter"`, `"Retaliate"`,
/// `"Victory {n}"` — only those that apply, in that order.
#[must_use]
pub fn enemy_keyword_chips(enemy: &Enemy) -> Vec<String> {
    let mut chips = Vec::new();
    if enemy.hunter {
        chips.push("Hunter".to_string());
    }
    if enemy.retaliate {
        chips.push("Retaliate".to_string());
    }
    if let Some(n) = enemy.victory {
        chips.push(format!("Victory {n}"));
    }
    chips
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::test_support::fixtures::test_enemy;

    #[test]
    fn stat_chips_in_order() {
        let mut e = test_enemy(1, "Ghoul");
        e.fight = 3;
        e.evade = 2;
        e.max_health = 3;
        e.damage = 1;
        e.attack_damage = 1;
        e.attack_horror = 1;
        assert_eq!(
            enemy_stat_chips(&e),
            vec![
                "fight 3".to_string(),
                "evade 2".to_string(),
                "health 1/3".to_string(),
                "attack: 1 dmg · 1 hor".to_string(),
            ]
        );
    }

    #[test]
    fn keyword_chips_only_when_present() {
        let mut e = test_enemy(1, "Ghoul Priest");
        e.hunter = true;
        e.retaliate = true;
        e.victory = Some(2);
        assert_eq!(
            enemy_keyword_chips(&e),
            vec![
                "Hunter".to_string(),
                "Retaliate".to_string(),
                "Victory 2".to_string(),
            ]
        );
    }

    #[test]
    fn keyword_chips_empty_for_plain_enemy() {
        let e = test_enemy(2, "Swarm of Rats");
        assert!(enemy_keyword_chips(&e).is_empty());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p web enemy_card::tests`
Expected: FAIL — module / functions not found (until Step 1 + 2 compile; if Step 1 lands first, the failure is the missing functions resolving in the test).

- [ ] **Step 4: (Implementation is in Step 2)** Run the tests to verify they pass

Run: `cargo test -p web enemy_card::tests`
Expected: PASS (3 tests). Also run `cargo clippy -p web --all-targets --all-features -- -D warnings` — clean.

> The helpers and tests are written together in Step 2 (the file is new). The RED state is "the file/functions don't exist yet"; confirm by checking out the failing compile before Step 2 if following strict TDD, or treat Step 2 as the GREEN landing and rely on the assertions to prove behavior.

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/enemy_card.rs crates/web/src/lib.rs
git commit -m "web: enemy stat + keyword chip helpers

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 2: The `EnemyCard` component

The component reading from `&Enemy`, reusing the card CSS + text renderer. Verified by a headless wasm test.

**Files:**
- Modify: `crates/web/src/enemy_card.rs`
- Modify: `crates/web/src/card.rs` (promote `render_segments` to `pub(crate)`)
- Modify: `crates/web/style.css`
- Create: `crates/web/tests/enemy_card.rs`

**Interfaces:**
- Consumes: `enemy_stat_chips` / `enemy_keyword_chips` (Task 1); `crate::card::{parse_card_text, render_segments}`.
- Produces: `#[component] pub fn EnemyCard(enemy: Enemy) -> impl IntoView` — renders `<div class="card card--enemy">` (+ ` card--exhausted` when exhausted), header (name + `Exhausted` badge), traits, ability text, and a footer of stat + keyword chips.

- [ ] **Step 1: Write the failing headless tests**

Create `crates/web/tests/enemy_card.rs`:

```rust
//! Headless render tests for the `EnemyCard` component. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use game_core::test_support::fixtures::test_enemy;
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::enemy_card::EnemyCard;

wasm_bindgen_test_configure!(run_in_browser);

fn last_card() -> web_sys::Element {
    let cards = leptos::prelude::document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
}

#[wasm_bindgen_test]
async fn engaged_enemy_renders_stats_keywords_exhausted() {
    let mut e = test_enemy(1, "Ghoul Priest");
    e.fight = 4;
    e.evade = 4;
    e.max_health = 2;
    e.damage = 0;
    e.hunter = true;
    e.retaliate = true;
    e.exhausted = true;
    leptos::mount::mount_to_body(move || view! { <EnemyCard enemy=e.clone()/> });
    leptos::task::tick().await;

    let card = last_card();
    let classes = card.class_name();
    assert!(classes.contains("card--enemy"), "enemy class missing: {classes}");
    assert!(classes.contains("card--exhausted"), "exhausted class missing: {classes}");
    let html = card.inner_html();
    assert!(html.contains("Ghoul Priest"), "name missing: {html}");
    assert!(html.contains("fight 4"), "fight chip missing: {html}");
    assert!(html.contains("health 0/2"), "health chip missing: {html}");
    assert!(html.contains("Hunter"), "hunter chip missing: {html}");
    assert!(html.contains("Retaliate"), "retaliate chip missing: {html}");
    assert!(html.contains("Exhausted"), "exhausted badge missing: {html}");
}

#[wasm_bindgen_test]
async fn ready_enemy_is_not_dimmed() {
    let e = test_enemy(2, "Swarm of Rats");
    leptos::mount::mount_to_body(move || view! { <EnemyCard enemy=e.clone()/> });
    leptos::task::tick().await;
    assert!(
        !last_card().class_name().contains("card--exhausted"),
        "ready enemy must not be dimmed"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web -- enemy_card`
Expected: FAIL — `web::enemy_card::EnemyCard` not found (compile error).

- [ ] **Step 3: Promote `render_segments` to `pub(crate)`**

In `crates/web/src/card.rs`, change the signature at `card.rs:434` from:

```rust
fn render_segments(segments: Vec<TextSegment>) -> Vec<AnyView> {
```

to:

```rust
pub(crate) fn render_segments(segments: Vec<TextSegment>) -> Vec<AnyView> {
```

- [ ] **Step 4: Implement the `EnemyCard` component**

Add to `crates/web/src/enemy_card.rs` (above the `#[cfg(test)]` module). Add the imports at the top of the file (after the existing `use game_core::state::Enemy;`):

```rust
use leptos::prelude::*;

use crate::card::{parse_card_text, render_segments};
```

```rust
/// One engaged enemy rendered as a card (red border via `card--enemy`), reusing
/// the card CSS and the `card--exhausted` dim from the asset slice. Reads from
/// the `Enemy` state struct; ability text is looked up by code via the registry.
/// Display-only — no click handlers.
// `enemy` is taken by value: Leptos `#[component]` generates a props struct
// requiring owned fields, so a reference would need a lifetime the macro can't
// express.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn EnemyCard(enemy: Enemy) -> impl IntoView {
    let name = enemy.name.clone();
    let traits = if enemy.traits.is_empty() {
        String::new()
    } else {
        format!("{}.", enemy.traits.join(". "))
    };
    let text_view = game_core::card_registry::current()
        .and_then(|r| (r.metadata_for)(&enemy.code))
        .and_then(|m| m.text.as_deref())
        .map(|t| render_segments(parse_card_text(t)));
    let exhausted = enemy.exhausted;
    let exhausted_badge =
        exhausted.then(|| view! { <span class="card-exhausted">"Exhausted"</span> });
    let stat_views: Vec<_> = enemy_stat_chips(&enemy)
        .into_iter()
        .map(|s| view! { <span class="chip chip--enemy-stat">{s}</span> })
        .collect();
    let keyword_views: Vec<_> = enemy_keyword_chips(&enemy)
        .into_iter()
        .map(|s| view! { <span class="chip chip--keyword">{s}</span> })
        .collect();
    let root_class = if exhausted {
        "card card--enemy card--exhausted"
    } else {
        "card card--enemy"
    };
    view! {
        <div class=root_class>
            <div class="card-head">
                <span class="card-name">{name}</span>
                {exhausted_badge}
            </div>
            <div class="card-traits">{traits}</div>
            <div class="card-text">{text_view}</div>
            <div class="card-footer enemy-stats">
                {stat_views}
                {keyword_views}
            </div>
        </div>
    }
}
```

- [ ] **Step 5: Add the CSS**

In `crates/web/style.css`, add the enemy border to the class palette — directly after the `.card--mythos   { border-color: #555; }` line (so the later `.card--exhausted` rule still wins its border when an enemy is exhausted):

```css
.card--enemy    { border-color: #a3261b; }
```

And after the `.chip--live { background: #e3ecf5; }` line (added in the in-play slice), add a keyword-chip tint:

```css
.chip--keyword { background: #f3dede; }
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web -- enemy_card`
Expected: PASS (2 tests). Confirm native still builds: `cargo test -p web enemy_card::tests` (PASS).

- [ ] **Step 7: Verify clippy (both targets) + build**

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings` and `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` and `cargo fmt --check` and `cd crates/web && trunk build`.
Expected: all clean / builds.

- [ ] **Step 8: Commit**

```bash
git add crates/web/src/enemy_card.rs crates/web/src/card.rs crates/web/style.css crates/web/tests/enemy_card.rs
git commit -m "web: EnemyCard component — enemy stats, keywords, exhausted dim/badge

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 3: Render engaged enemies as cards in the board

Swap the board's engaged-enemy text list for a `.card-row` of `EnemyCard`s; treacheries stay text.

**Files:**
- Modify: `crates/web/src/board.rs` (the engaged builder at `:102`, the threat container at `:126`)
- Modify: `crates/web/tests/board.rs`

**Interfaces:**
- Consumes: `EnemyCard` (Task 2).

- [ ] **Step 1: Write the failing board test**

In `crates/web/tests/board.rs`, add a new test (alongside the others):

```rust
#[wasm_bindgen_test]
async fn engaged_enemy_renders_as_card_in_threat_area() {
    use game_core::state::InvestigatorId;
    use game_core::test_support::fixtures::{test_enemy, test_investigator};

    let mut enemy = test_enemy(1, "Ghoul Priest");
    enemy.engaged_with = Some(InvestigatorId(1));
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();

    let html = render_state(state).await;

    let card = leptos::prelude::document()
        .query_selector(".threat .card-row .card")
        .expect("query_selector");
    assert!(card.is_some(), "engaged enemy should render as a card: {html}");
    assert!(html.contains("Ghoul Priest"), "enemy name missing: {html}");
}
```

> `render_state` already installs the synthetic test registry, so `_test_enemy_1` has no metadata → the enemy's ability text is simply absent; the stat/keyword chips still render from the struct.

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web -- board`
Expected: FAIL — engaged enemies still render as `<li class="enemy-engaged">` inside the threat `<ul>`, so `.threat .card-row .card` does not exist.

- [ ] **Step 3: Render engaged enemies as `EnemyCard`s**

In `crates/web/src/board.rs`, replace the `engaged` builder (currently maps each engaged enemy to `<li class="enemy-engaged">…</li>`):

```rust
            let engaged: Vec<_> = game
                .enemies
                .values()
                .filter(|e| e.engaged_with == Some(inv.id))
                .map(|e| {
                    view! {
                        <li class="enemy-engaged">
                            {e.name.clone()} " " {e.damage} "/" {e.max_health}
                        </li>
                    }
                })
                .collect();
```

with:

```rust
            let engaged: Vec<_> = game
                .enemies
                .values()
                .filter(|e| e.engaged_with == Some(inv.id))
                .cloned()
                .map(|e| view! { <crate::enemy_card::EnemyCard enemy=e/> })
                .collect();
```

Then change the threat container (`board.rs:126`) from:

```rust
                    <div class="threat"><h4>"Threat area"</h4><ul>{threat}{engaged}</ul></div>
```

to:

```rust
                    <div class="threat"><h4>"Threat area"</h4><ul>{threat}</ul><div class="card-row">{engaged}</div></div>
```

Leave the `threat` (treachery) builder unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web -- board`
Expected: PASS (all board tests, including the new engaged-enemy card test).

- [ ] **Step 5: Verify clippy + build**

Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` and `cargo fmt --check` and `cd crates/web && trunk build`.
Expected: clean / builds.

- [ ] **Step 6: Commit**

```bash
git add crates/web/src/board.rs crates/web/tests/board.rs
git commit -m "web: render engaged enemies as EnemyCard rectangles

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

Expected: all green. Fix any clippy/doc findings (wasm-clippy lints the new component).

- [ ] **Step 2: Update the phase-7 doc — only when the PR is ready**

Extend the visual-card-rendering bullet in `docs/phases/phase-7-the-gathering.md` (browser-capstone section) with slice 3: engaged enemies now render as `EnemyCard` rectangles (a dedicated component reading the `Enemy` struct — combat stats, keyword chips, exhausted dim/badge, red border) in the threat area; the map's enemy tokens and threat-area treacheries remain later slices; `prey` display deferred (moot in 1p). Reference the new spec/plan. Load-bearing residue only.

- [ ] **Step 3: Open the PR**

Branch is `web/enemy-cards`. File an issue first (issue-first convention), push, open the PR with `gh pr create`; `Closes` the issue. Design-decisions paragraph: dedicated `EnemyCard` (enemies are a different data source — the `Enemy` struct, not registry+instance); shares card CSS + the now-`pub(crate)` `render_segments`; engaged-only this slice; `prey` skipped in 1p.

---

## Self-review notes

- **Spec coverage:** dedicated `EnemyCard` reading `&Enemy` (Task 2) ✓; `enemy_stat_chips` fight/evade/health/attack (Task 1) ✓; `enemy_keyword_chips` Hunter/Retaliate/Victory (Task 1) ✓; ability text via registry + `parse_card_text`/`render_segments` (Task 2) ✓; exhausted dim + badge reuse (Task 2) ✓; `card--enemy` red border (Task 2 CSS) ✓; engaged enemies → `.card-row` in threat area, treacheries unchanged (Task 3) ✓; `render_segments` promoted to `pub(crate)` (Task 2 Step 3) ✓; native + headless + board tests ✓; `prey` skipped (not implemented anywhere) ✓.
- **Type consistency:** `enemy_stat_chips`/`enemy_keyword_chips` defined Task 1, consumed in the component Task 2. `EnemyCard(enemy: Enemy)` prop name matches the board call site `enemy=e` (Task 3). `render_segments` made `pub(crate)` before the component uses it. Tests build `Enemy` via `test_enemy` + field mutation (it's `#[non_exhaustive]`).
- **Out of scope (unchanged):** map enemy tokens, threat-area treacheries, locations/act-agenda, `prey`, clickable enemies, the icon font.
