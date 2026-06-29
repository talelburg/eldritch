# Web Card Rendering (Hand Slice) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render hand cards in the web client as faithful mini-card rectangles (cost, name, traits, translated text, slots, skill icons, colour-coded by class) via a reusable `Card` component, display-only.

**Architecture:** A new `crates/web/src/card.rs` holds (a) pure, native-testable helpers — a card-text markup *parser* (`parse_card_text` → `Vec<TextSegment>`), field formatters, and a `card_face` normalizer — and (b) the `Card` Leptos component that renders them. The parser is split from rendering so the markup logic is unit-testable off-wasm. `board.rs` swaps the hand text list for a row of `Card`s. In-play/threat stay text this slice.

**Tech Stack:** Rust, Leptos (CSR), Trunk, `wasm-bindgen-test` (headless Firefox), the engine's `card_registry` for metadata lookup.

## Global Constraints

- **Warnings are errors in CI** across seven jobs. Before pushing, match the strict flags: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Clippy `pedantic` is on** (workspace lints), with `module_name_repetitions` and `must_use_candidate` allowed. `missing_errors_doc`/`missing_panics_doc` are enforced.
- **wasm-only test files** must carry crate-level `#![cfg(target_arch = "wasm32")]` (P6.3 convention) — else the native `test`/`clippy` jobs try to compile a browser-only test.
- **Headless tests share one browser page**; `mount_to_body` *appends*, so DOM accumulates across tests. Scope absence/presence assertions to the last mounted subtree (query `.card` and take the last). Never `set_inner_html("")` on `<body>`.
- **Card stats come only from the corpus** (`CardMetadata`/`CardKind` via the registry) — never hand-typed.
- **Display-only.** No click handlers / no `OutboundTx` in this slice.

## File structure

- **Create `crates/web/src/card.rs`** — `TextSegment`, `parse_card_text`, field formatters (`cost_label`, `class_css`, `slot_chips`, `skill_chips`), `CardFace` + `card_face`, the `Card` component, and `render_segments`. Native `#[cfg(test)] mod tests` for all pure helpers.
- **Create `crates/web/tests/card.rs`** — headless wasm render tests for the `Card` component.
- **Modify `crates/web/src/lib.rs`** — add `pub mod card;`.
- **Modify `crates/web/src/board.rs`** — hand list renders `<Card>`s.
- **Modify `crates/web/style.css`** — class palette, card box, chip styles.
- **Modify `crates/web/tests/board.rs`** — adjust the hand assertion if needed (fallback shows the raw code, so the existing `contains("_synth_...")` checks still hold; add a `.card` presence check).

Type/path notes (verified against the codebase):
- Metadata: `game_core::card_registry::current().and_then(|r| (r.metadata_for)(&code))` → `Option<&'static CardMetadata>`.
- `CardMetadata { name: String, traits: Vec<String>, text: Option<String>, weakness: bool, kind: CardKind, .. }`.
- `CardKind` variants used: `Asset { class, cost: Option<i8>, slots: Vec<Slot>, skill_icons: SkillIcons, is_fast: bool, .. }`, `Event { class, cost: Option<i8>, skill_icons, is_fast, .. }`, `Skill { class, skill_icons, .. }`. `CardKind` is **not** `#[non_exhaustive]` but other variants exist (Location/Enemy/Act/Agenda/Treachery/Investigator) — match them with a `_ =>` generic arm.
- `Class`, `Slot`, `SkillIcons` all derive `Copy` — so `*class`, `*cost`, `*is_fast` are fine.
- Imports: `use game_core::card_data::{CardKind, Class, SkillIcons, Slot};` and `use game_core::state::CardCode;` and `use leptos::prelude::*;`.

---

### Task 1: Card-text markup parser (`parse_card_text`)

Pure tokenizer turning ArkhamDB card-text markup into `Vec<TextSegment>`. No Leptos, no DOM — fully native-testable.

**Files:**
- Create: `crates/web/src/card.rs`
- Modify: `crates/web/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub enum TextSegment { Text(String), LineBreak, Symbol(String), Trait(String), Bold(String), Italic(String), Unknown(String) }` (derives `Debug, Clone, PartialEq, Eq`).
  - `pub fn parse_card_text(text: &str) -> Vec<TextSegment>`.
  - `Symbol(s)` carries the bare token (e.g. `"combat"`); `Unknown(s)` carries the bare token of an *unrecognized* `[token]`; the renderer re-adds the brackets so it pops out.

- [ ] **Step 1: Register the module**

In `crates/web/src/lib.rs`, add alongside the other `pub mod` lines:

```rust
pub mod card;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/web/src/card.rs` with only the parser + tests:

```rust
//! Visual card rendering for the web client. Pure helpers (`parse_card_text`,
//! field formatters, `card_face`) are native-testable; the `Card` component
//! assembles them into a faithful mini-card rectangle. Display-only (no click
//! handlers) — interactivity is a later slice.

/// A parsed run of card text. `Symbol`/`Unknown` carry the bare token without
/// brackets; the renderer re-adds brackets for `Unknown` so unmapped tokens are
/// visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSegment {
    Text(String),
    LineBreak,
    Symbol(String),
    Trait(String),
    Bold(String),
    Italic(String),
    Unknown(String),
}

/// ArkhamDB game symbols we render as chips. Anything else in `[..]` is left
/// verbatim (with brackets) so it pops out for us to add a mapping.
const KNOWN_SYMBOLS: &[&str] = &[
    "willpower", "intellect", "combat", "agility", "wild", "action", "reaction",
    "fast", "free", "elder_sign", "skull", "cultist", "tablet", "auto_fail",
    "bless", "curse",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_one_segment() {
        assert_eq!(
            parse_card_text("Deal 1 damage."),
            vec![TextSegment::Text("Deal 1 damage.".into())]
        );
    }

    #[test]
    fn newline_becomes_line_break() {
        assert_eq!(
            parse_card_text("a\nb"),
            vec![
                TextSegment::Text("a".into()),
                TextSegment::LineBreak,
                TextSegment::Text("b".into()),
            ]
        );
    }

    #[test]
    fn known_symbol_becomes_symbol_segment() {
        assert_eq!(
            parse_card_text("+1 [combat]"),
            vec![
                TextSegment::Text("+1 ".into()),
                TextSegment::Symbol("combat".into()),
            ]
        );
    }

    #[test]
    fn unknown_token_is_preserved_without_brackets_in_segment() {
        // The bare token is captured; the renderer re-adds brackets.
        assert_eq!(
            parse_card_text("[mystery]"),
            vec![TextSegment::Unknown("mystery".into())]
        );
    }

    #[test]
    fn double_bracket_is_a_trait() {
        assert_eq!(
            parse_card_text("[[Tome]] only"),
            vec![
                TextSegment::Trait("Tome".into()),
                TextSegment::Text(" only".into()),
            ]
        );
    }

    #[test]
    fn bold_and_italic_runs() {
        assert_eq!(
            parse_card_text("<b>Fight.</b> <i>x</i>"),
            vec![
                TextSegment::Bold("Fight.".into()),
                TextSegment::Text(" ".into()),
                TextSegment::Italic("x".into()),
            ]
        );
    }

    #[test]
    fn unterminated_marker_is_literal_text() {
        // No closing bracket ⇒ the '[' is ordinary text, never panics.
        assert_eq!(
            parse_card_text("a [b"),
            vec![TextSegment::Text("a [b".into())]
        );
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p web card::tests`
Expected: FAIL — `parse_card_text` not found.

- [ ] **Step 4: Implement the parser**

Add to `crates/web/src/card.rs` (above the `#[cfg(test)]` module):

```rust
/// Translate ArkhamDB card-text markup into renderable segments.
///
/// Handles `\n`, `<b>..</b>`, `<i>..</i>`, `[[Trait]]`, and `[symbol]` tokens.
/// Bold/italic content is taken as literal text (not recursively parsed) — card
/// emphasis runs are short labels. An unterminated marker (no closing bracket /
/// tag) degrades to literal text.
#[must_use]
pub fn parse_card_text(text: &str) -> Vec<TextSegment> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            flush(&mut buf, &mut out);
            out.push(TextSegment::LineBreak);
            i += 1;
        } else if c == '[' && chars.get(i + 1) == Some(&'[') {
            if let Some((inner, next)) = read_until(&chars, i + 2, "]]") {
                flush(&mut buf, &mut out);
                out.push(TextSegment::Trait(inner));
                i = next;
            } else {
                buf.push(c);
                i += 1;
            }
        } else if c == '[' {
            if let Some((inner, next)) = read_until(&chars, i + 1, "]") {
                flush(&mut buf, &mut out);
                out.push(symbol_or_unknown(inner));
                i = next;
            } else {
                buf.push(c);
                i += 1;
            }
        } else if starts_with(&chars, i, "<b>") {
            if let Some((inner, next)) = read_until(&chars, i + 3, "</b>") {
                flush(&mut buf, &mut out);
                out.push(TextSegment::Bold(inner));
                i = next;
            } else {
                buf.push(c);
                i += 1;
            }
        } else if starts_with(&chars, i, "<i>") {
            if let Some((inner, next)) = read_until(&chars, i + 3, "</i>") {
                flush(&mut buf, &mut out);
                out.push(TextSegment::Italic(inner));
                i = next;
            } else {
                buf.push(c);
                i += 1;
            }
        } else {
            buf.push(c);
            i += 1;
        }
    }
    flush(&mut buf, &mut out);
    out
}

/// Push the accumulated text buffer as a `Text` segment, if non-empty.
fn flush(buf: &mut String, out: &mut Vec<TextSegment>) {
    if !buf.is_empty() {
        out.push(TextSegment::Text(std::mem::take(buf)));
    }
}

/// Whether `chars[i..]` starts with `pat`.
fn starts_with(chars: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, pc)| chars.get(i + k) == Some(&pc))
}

/// Read from `start` up to (not including) the first `pat`; return the inner
/// string and the index just past `pat`. `None` if `pat` never occurs.
fn read_until(chars: &[char], start: usize, pat: &str) -> Option<(String, usize)> {
    let pat_len = pat.chars().count();
    let mut j = start;
    while j < chars.len() {
        if starts_with(chars, j, pat) {
            let inner: String = chars[start..j].iter().collect();
            return Some((inner, j + pat_len));
        }
        j += 1;
    }
    None
}

/// Classify a single-bracket token as a known `Symbol` or an `Unknown`.
fn symbol_or_unknown(inner: String) -> TextSegment {
    if KNOWN_SYMBOLS.contains(&inner.as_str()) {
        TextSegment::Symbol(inner)
    } else {
        TextSegment::Unknown(inner)
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p web card::tests`
Expected: PASS (7 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/web/src/card.rs crates/web/src/lib.rs
git commit -m "web: card-text markup parser (parse_card_text)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 2: Pure field helpers + `card_face`

Formatters for cost, class CSS, slot chips, and skill chips, plus a `card_face` normalizer that maps a hand-eligible `CardKind` to a render-ready struct (and `None` for non-hand types). All pure, native-testable.

**Files:**
- Modify: `crates/web/src/card.rs`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `pub fn cost_label(cost: Option<i8>) -> String` — `"X"` for `None`, else the number.
  - `pub fn class_css(class: Class) -> &'static str` — `"card--guardian"` etc.
  - `pub fn slot_chips(slots: &[Slot]) -> Vec<String>` — first-seen order, `"Arcane ×2"` when repeated.
  - `pub fn skill_chips(icons: &SkillIcons) -> Vec<(String, u8)>` — one `(name, count)` per non-zero icon, order willpower/intellect/combat/agility/wild.
  - `pub struct CardFace { pub class_css: &'static str, pub cost_corner: Option<String>, pub slot_chips: Vec<String>, pub skill_chips: Vec<(String, u8)>, pub is_fast: bool }`.
  - `pub fn card_face(kind: &CardKind) -> Option<CardFace>` — `Some` for Asset/Event/Skill (Skill ⇒ `cost_corner: None`), `None` otherwise.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `crates/web/src/card.rs`:

```rust
    use game_core::card_data::{CardKind, Class, SkillIcons, Slot};

    #[test]
    fn cost_label_handles_value_and_x() {
        assert_eq!(cost_label(Some(3)), "3");
        assert_eq!(cost_label(None), "X");
    }

    #[test]
    fn class_css_maps_each_class() {
        assert_eq!(class_css(Class::Guardian), "card--guardian");
        assert_eq!(class_css(Class::Mystic), "card--mystic");
        assert_eq!(class_css(Class::Neutral), "card--neutral");
    }

    #[test]
    fn slot_chips_collapse_duplicates_with_counts() {
        assert_eq!(slot_chips(&[Slot::Hand]), vec!["Hand".to_string()]);
        assert_eq!(
            slot_chips(&[Slot::Arcane, Slot::Arcane]),
            vec!["Arcane ×2".to_string()]
        );
        assert!(slot_chips(&[]).is_empty());
    }

    #[test]
    fn skill_chips_lists_nonzero_icons_in_order() {
        let icons = SkillIcons { willpower: 0, intellect: 2, combat: 1, agility: 0, wild: 1 };
        assert_eq!(
            skill_chips(&icons),
            vec![("intellect".to_string(), 2), ("combat".to_string(), 1), ("wild".to_string(), 1)]
        );
    }

    #[test]
    fn card_face_asset_has_cost_slots_icons() {
        let kind = CardKind::Asset {
            class: Class::Guardian,
            cost: Some(3),
            xp: None,
            slots: vec![Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons { combat: 1, ..SkillIcons::default() },
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        };
        let face = card_face(&kind).expect("asset has a face");
        assert_eq!(face.class_css, "card--guardian");
        assert_eq!(face.cost_corner.as_deref(), Some("3"));
        assert_eq!(face.slot_chips, vec!["Hand".to_string()]);
        assert_eq!(face.skill_chips, vec![("combat".to_string(), 1)]);
    }

    #[test]
    fn card_face_skill_has_no_cost_corner() {
        let kind = CardKind::Skill {
            class: Class::Seeker,
            xp: None,
            skill_icons: SkillIcons { intellect: 1, ..SkillIcons::default() },
            deck_limit: 2,
            commit_limit: Some(1),
        };
        let face = card_face(&kind).expect("skill has a face");
        assert_eq!(face.cost_corner, None);
        assert!(face.slot_chips.is_empty());
    }

    #[test]
    fn card_face_is_none_for_non_hand_kinds() {
        let kind = CardKind::Agenda { doom_threshold: 3 };
        assert!(card_face(&kind).is_none());
    }
```

> Note: the `Asset` literal lists every field — if a field name/shape differs from the corpus definition, copy it from `crates/card-dsl/src/card_data.rs`'s `CardKind::Asset` arm. The build will tell you immediately.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p web card::tests`
Expected: FAIL — `cost_label` / `class_css` / `slot_chips` / `skill_chips` / `card_face` / `CardFace` not found.

- [ ] **Step 3: Implement the helpers**

Add to `crates/web/src/card.rs` (above the test module). Add the imports at the top of the file:

```rust
use game_core::card_data::{CardKind, Class, SkillIcons, Slot};
```

```rust
/// Resource-cost label: the number, or `"X"` for an X-cost card.
#[must_use]
pub fn cost_label(cost: Option<i8>) -> String {
    cost.map_or_else(|| "X".to_string(), |c| c.to_string())
}

/// CSS modifier class for a card's class colour.
#[must_use]
pub fn class_css(class: Class) -> &'static str {
    match class {
        Class::Guardian => "card--guardian",
        Class::Seeker => "card--seeker",
        Class::Rogue => "card--rogue",
        Class::Mystic => "card--mystic",
        Class::Survivor => "card--survivor",
        Class::Neutral => "card--neutral",
        Class::Mythos => "card--mythos",
    }
}

/// Human slot name.
fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Hand => "Hand",
        Slot::Accessory => "Accessory",
        Slot::Ally => "Ally",
        Slot::Arcane => "Arcane",
        Slot::Body => "Body",
        Slot::Tarot => "Tarot",
    }
}

/// Slot chips in first-seen order, collapsing duplicates to `"Name ×N"`.
#[must_use]
pub fn slot_chips(slots: &[Slot]) -> Vec<String> {
    let mut counts: Vec<(Slot, usize)> = Vec::new();
    for &s in slots {
        if let Some(entry) = counts.iter_mut().find(|(k, _)| *k == s) {
            entry.1 += 1;
        } else {
            counts.push((s, 1));
        }
    }
    counts
        .into_iter()
        .map(|(s, n)| {
            let name = slot_name(s);
            if n > 1 {
                format!("{name} ×{n}")
            } else {
                name.to_string()
            }
        })
        .collect()
}

/// `(name, count)` for each non-zero skill icon, in canonical order.
#[must_use]
pub fn skill_chips(icons: &SkillIcons) -> Vec<(String, u8)> {
    [
        ("willpower", icons.willpower),
        ("intellect", icons.intellect),
        ("combat", icons.combat),
        ("agility", icons.agility),
        ("wild", icons.wild),
    ]
    .into_iter()
    .filter(|&(_, n)| n > 0)
    .map(|(name, n)| (name.to_string(), n))
    .collect()
}

/// Render-ready normalization of a hand-eligible card's type data.
pub struct CardFace {
    pub class_css: &'static str,
    /// `None` ⇒ no cost corner (Skill cards are committed, not played).
    pub cost_corner: Option<String>,
    pub slot_chips: Vec<String>,
    pub skill_chips: Vec<(String, u8)>,
    pub is_fast: bool,
}

/// Map a hand-eligible `CardKind` (Asset/Event/Skill) to a `CardFace`; `None`
/// for other kinds (rendered as a generic rectangle this slice).
#[must_use]
pub fn card_face(kind: &CardKind) -> Option<CardFace> {
    match kind {
        CardKind::Asset { class, cost, slots, skill_icons, is_fast, .. } => Some(CardFace {
            class_css: class_css(*class),
            cost_corner: Some(cost_label(*cost)),
            slot_chips: slot_chips(slots),
            skill_chips: skill_chips(skill_icons),
            is_fast: *is_fast,
        }),
        CardKind::Event { class, cost, skill_icons, is_fast, .. } => Some(CardFace {
            class_css: class_css(*class),
            cost_corner: Some(cost_label(*cost)),
            slot_chips: Vec::new(),
            skill_chips: skill_chips(skill_icons),
            is_fast: *is_fast,
        }),
        CardKind::Skill { class, skill_icons, .. } => Some(CardFace {
            class_css: class_css(*class),
            cost_corner: None,
            slot_chips: Vec::new(),
            skill_chips: skill_chips(skill_icons),
            is_fast: false,
        }),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p web card::tests`
Expected: PASS (all Task 1 + Task 2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/card.rs
git commit -m "web: card field formatters + card_face normalizer

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 3: The `Card` component

Assemble the helpers into the faithful mini-card view: cost corner, name, fast/weakness markers, traits, translated text, footer (slots + skill icons), colour-coded by class. Missing metadata ⇒ a bare rectangle with the raw code. Non-hand kinds ⇒ a generic rectangle (name/traits/text). Verified by a headless wasm test.

**Files:**
- Modify: `crates/web/src/card.rs`
- Create: `crates/web/tests/card.rs`

**Interfaces:**
- Consumes: `parse_card_text` (Task 1); `card_face`, `CardFace` (Task 2).
- Produces: `#[component] pub fn Card(code: CardCode) -> impl IntoView`. Renders a `<div class="card …">`. Helper `render_segments(Vec<TextSegment>) -> Vec<AnyView>`.

- [ ] **Step 1: Write the failing test**

Create `crates/web/tests/card.rs`:

```rust
//! Headless render tests for the `Card` component. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use game_core::state::CardCode;
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::card::Card;

wasm_bindgen_test_configure!(run_in_browser);

/// Inner HTML of the last mounted `.card` (DOM accumulates across tests on the
/// shared page — scope to the latest subtree).
fn last_card_html() -> String {
    let cards = leptos::prelude::document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_ref::<web_sys::Element>()
        .expect("Element")
        .inner_html()
}

async fn mount_card(code: &str) -> String {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let code = CardCode::new(code);
    leptos::mount::mount_to_body(move || view! { <Card code=code.clone()/> });
    leptos::task::tick().await;
    last_card_html()
}

#[wasm_bindgen_test]
async fn asset_renders_cost_name_traits_text_icons() {
    // Machete 01020: Guardian, cost 3, Hand slot, 1 combat icon, text with
    // [action], <b>Fight.</b>, and [combat].
    let html = mount_card("01020").await;
    assert!(html.contains("Machete"), "name missing: {html}");
    assert!(html.contains('3'), "cost missing: {html}");
    assert!(html.contains("Weapon"), "traits missing: {html}");
    assert!(html.contains("Fight."), "bold text missing: {html}");
    // [combat] / [action] become chips; assert the chip class is present.
    assert!(html.contains("chip--combat"), "combat chip missing: {html}");
}

#[wasm_bindgen_test]
async fn guardian_card_carries_class_modifier() {
    let _ = mount_card("01020").await;
    let cards = leptos::prelude::document()
        .query_selector_all(".card--guardian")
        .expect("query_selector_all");
    assert!(cards.length() >= 1, "guardian class modifier missing");
}

#[wasm_bindgen_test]
async fn unknown_code_falls_back_to_raw_code() {
    let html = mount_card("99999").await;
    assert!(html.contains("99999"), "raw code fallback missing: {html}");
}
```

> If `01020`'s pipeline-emitted text ever changes, re-verify against `https://arkhamdb.com/card/01020` / the snapshot before adjusting assertions — never paraphrase from memory.

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web -- card`
Expected: FAIL — `web::card::Card` not found (component not yet defined).

- [ ] **Step 3: Implement the component**

Add to `crates/web/src/card.rs`. Add to the top-of-file imports:

```rust
use game_core::state::CardCode;
use leptos::prelude::*;
```

```rust
/// One card rendered as a faithful mini-card rectangle, colour-coded by class.
///
/// Looks metadata up via the installed registry; a card with no metadata
/// (unimplemented stub, or registry absent in a render-only path) falls back to
/// a bare rectangle showing the raw code. Non-hand card kinds render as a
/// generic rectangle (name/traits/text) — their detailed faces are later slices.
/// Display-only: no click handlers.
#[component]
pub fn Card(code: CardCode) -> impl IntoView {
    let Some(meta) = game_core::card_registry::current().and_then(|r| (r.metadata_for)(&code))
    else {
        return view! {
            <div class="card card--unknown">
                <span class="card-name">{code.to_string()}</span>
            </div>
        }
        .into_any();
    };

    let name = meta.name.clone();
    let traits = if meta.traits.is_empty() {
        String::new()
    } else {
        format!("{}.", meta.traits.join(". "))
    };
    let text_view = meta
        .text
        .as_deref()
        .map(|t| render_segments(parse_card_text(t)));
    let weakness_view = meta
        .weakness
        .then(|| view! { <span class="card-weakness">"Weakness"</span> });

    match card_face(&meta.kind) {
        Some(face) => {
            let CardFace { class_css, cost_corner, slot_chips, skill_chips, is_fast } = face;
            let cost_view = cost_corner.map(|c| view! { <span class="card-cost">{c}</span> });
            let fast_view = is_fast.then(|| view! { <span class="card-fast">"Fast"</span> });
            let slot_views: Vec<_> = slot_chips
                .into_iter()
                .map(|s| view! { <span class="chip chip--slot">{s}</span> })
                .collect();
            let skill_views: Vec<_> = skill_chips
                .into_iter()
                .map(|(name, n)| {
                    let label = if n > 1 { format!("{name} ×{n}") } else { name.clone() };
                    let cls = format!("chip chip--{name}");
                    view! { <span class=cls>{label}</span> }
                })
                .collect();
            view! {
                <div class=format!("card {class_css}")>
                    <div class="card-head">
                        {cost_view}
                        <span class="card-name">{name}</span>
                        {fast_view}
                        {weakness_view}
                    </div>
                    <div class="card-traits">{traits}</div>
                    <div class="card-text">{text_view}</div>
                    <div class="card-footer">
                        <span class="card-slots">{slot_views}</span>
                        <span class="card-skills">{skill_views}</span>
                    </div>
                </div>
            }
            .into_any()
        }
        None => view! {
            <div class="card card--generic">
                <div class="card-head">
                    <span class="card-name">{name}</span>
                    {weakness_view}
                </div>
                <div class="card-traits">{traits}</div>
                <div class="card-text">{text_view}</div>
            </div>
        }
        .into_any(),
    }
}

/// Render parsed card text to views (known symbols → chips; unknown tokens keep
/// their brackets in a `.unknown-token` span so they pop out).
fn render_segments(segments: Vec<TextSegment>) -> Vec<AnyView> {
    segments
        .into_iter()
        .map(|seg| match seg {
            TextSegment::Text(s) => view! { {s} }.into_any(),
            TextSegment::LineBreak => view! { <br/> }.into_any(),
            TextSegment::Symbol(tok) => {
                let cls = format!("chip chip--{tok}");
                view! { <span class=cls>{tok}</span> }.into_any()
            }
            TextSegment::Trait(t) => {
                view! { <span class="card-trait-ref">{t}</span> }.into_any()
            }
            TextSegment::Bold(s) => view! { <b>{s}</b> }.into_any(),
            TextSegment::Italic(s) => view! { <i>{s}</i> }.into_any(),
            TextSegment::Unknown(inner) => {
                view! { <span class="unknown-token">{format!("[{inner}]")}</span> }.into_any()
            }
        })
        .collect()
}
```

> If clippy flags the component as `too_many_lines`, add `#[allow(clippy::too_many_lines)]` above `pub fn Card` with a one-line reason comment, matching `input.rs`'s `AwaitingInputView`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web -- card`
Expected: PASS (3 tests). Also confirm native still builds: `cargo test -p web card::tests` (PASS).

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/card.rs crates/web/tests/card.rs
git commit -m "web: Card component rendering the faithful mini-card

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 4: Wire hand into the board + style the cards

Replace the hand text list in `board.rs` with a row of `Card`s, and add the CSS (class palette, card box, chips). In-play/threat stay text.

**Files:**
- Modify: `crates/web/src/board.rs:73-132` (the `investigators_panel` hand section)
- Modify: `crates/web/style.css`
- Modify: `crates/web/tests/board.rs` (add a `.card` presence assertion)

**Interfaces:**
- Consumes: `Card` (Task 3).

- [ ] **Step 1: Update the board test to expect cards**

In `crates/web/tests/board.rs`, the `investigators_panel_renders_stats_and_hand` test currently asserts the hand card *codes* render. The fallback rectangle still shows the raw code (the test registry has no metadata for `_synth_*`), so those asserts stay. Add a `.card` presence check at the end of that test:

```rust
    // Hand cards now render as Card rectangles (fallback to raw code without
    // metadata in the test registry).
    let cards = leptos::prelude::document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    assert!(cards.length() >= 1, "hand should render Card rectangles: {html}");
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web -- board`
Expected: FAIL — no `.card` element yet (hand still renders `<li>` text).

- [ ] **Step 3: Render the hand as cards in `board.rs`**

In `crates/web/src/board.rs`, in `investigators_panel`, replace the `hand` builder:

```rust
            let hand: Vec<_> = inv
                .hand
                .iter()
                .map(|code| view! { <li class="card">{crate::names::card_name(code)}</li> })
                .collect();
```

with:

```rust
            let hand: Vec<_> = inv
                .hand
                .iter()
                .cloned()
                .map(|code| view! { <crate::card::Card code=code/> })
                .collect();
```

And change the hand container so it is not a `<ul>` of `<li>`s — update the hand `div` (in the returned `view!`) from:

```rust
                    <div class="hand"><h4>"Hand"</h4><ul>{hand}</ul></div>
```

to:

```rust
                    <div class="hand"><h4>"Hand"</h4><div class="card-row">{hand}</div></div>
```

Leave the `in_play` and `threat` builders and their `<ul>` containers unchanged.

- [ ] **Step 4: Add the CSS**

Append to `crates/web/style.css`:

```css
/* --- Cards (hand slice) ------------------------------------------------ */
.card-row {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
.card {
  width: 12rem;
  border: 2px solid #888;
  border-radius: 6px;
  padding: 0.4rem 0.5rem;
  background: #1b1b1b;
  font-size: 0.8rem;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}
.card-head {
  display: flex;
  align-items: baseline;
  gap: 0.4rem;
}
.card-cost {
  font-weight: bold;
  border: 1px solid currentColor;
  border-radius: 50%;
  padding: 0 0.4rem;
}
.card-name { font-weight: bold; }
.card-fast, .card-weakness {
  margin-left: auto;
  font-size: 0.7rem;
  text-transform: uppercase;
}
.card-weakness { color: #d44; }
.card-traits { font-style: italic; opacity: 0.85; }
.card-text { line-height: 1.3; }
.card-footer { display: flex; justify-content: space-between; margin-top: auto; }
.chip {
  display: inline-block;
  padding: 0 0.3rem;
  border: 1px solid #999;
  border-radius: 3px;
  font-size: 0.7rem;
  margin: 0 0.1rem;
}
.card-trait-ref { font-style: italic; font-weight: bold; }
.unknown-token { color: #d44; font-weight: bold; }

/* Class palette (border colour). */
.card--guardian { border-color: #2f6fb3; }
.card--seeker   { border-color: #c8a020; }
.card--rogue    { border-color: #3a8f4f; }
.card--mystic   { border-color: #7a4fb0; }
.card--survivor { border-color: #c0432f; }
.card--neutral  { border-color: #888; }
.card--mythos   { border-color: #555; }
.card--unknown, .card--generic { border-style: dashed; }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web -- board`
Expected: PASS (all board tests, including the new `.card` check).

- [ ] **Step 6: Verify the bundle builds and eyeball it**

Run: `cd crates/web && trunk build`
Expected: builds with no errors. (Optional live check: `cargo run -p server` then open the app, start a game, and confirm hand cards render as rectangles with cost/name/traits/text/icons.)

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/board.rs crates/web/style.css crates/web/tests/board.rs
git commit -m "web: render hand cards as Card rectangles + card styling

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 5: Full CI gauntlet + phase doc

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

Expected: all green. Fix any clippy/doc findings (the wasm-clippy job is the one that sees the `Card` component's wasm code).

- [ ] **Step 2: Update the phase doc — only when the PR is ready**

Per the repo convention, the `docs/phases/phase-7-the-gathering.md` update is the **final** commit, reflecting the actually-shipping state (PR # known, review fixes folded). Add a short note under the web-capstone area that hand cards now render as visual `Card` rectangles (display-only), with in-play/threat/locations/enemies/act-agenda as later slices, and the icon font deferred (seam built). Keep it to load-bearing residue only.

- [ ] **Step 3: Open the PR**

Branch is `web/card-rendering`. Push and open the PR with `gh pr create` using the repo template; design-decisions paragraph: faithful mini-card, parser split from rendering for native testability, text chips with the icon-font seam deferred, unknown tokens kept verbatim to surface gaps. `Closes` the relevant issue if one is filed (file one first if not — issue-first convention).

---

## Self-review notes

- **Spec coverage:** reusable component (Task 3) ✓; hand integration (Task 4) ✓; cost/name/traits/text/slots/skill-icons/fast/weakness fields (Tasks 2–3) ✓; class colour (Task 2 `class_css` + Task 4 CSS) ✓; markup translation incl. `\n`/`<b>`/`<i>`/`[[trait]]`/symbols/unknown-verbatim (Task 1 + `render_segments`) ✓; missing-metadata fallback (Task 3) ✓; generic rectangle for non-hand kinds (Task 3) ✓; native + headless tests (all tasks) ✓; icon-font deferral with a clean seam (`render_segments`/`skill_chips` are the only two seams) ✓.
- **Type consistency:** `card_face` returns `CardFace`; `Card` destructures the exact field names (`class_css`, `cost_corner`, `slot_chips`, `skill_chips`, `is_fast`). `skill_chips` returns `(String, u8)` consumed identically in Task 2 test and Task 3 render. `TextSegment` variants match between parser and `render_segments`.
- **Out of scope (unchanged this slice):** in-play/threat/enemy/location/act/agenda faces, clickable cards, the icon font, card art.
