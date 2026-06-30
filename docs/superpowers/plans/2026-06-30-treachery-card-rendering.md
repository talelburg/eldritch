# Threat-area Treachery Card Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render threat-area treacheries (Cover Up, Frozen in Fear) as cards via the existing `Card` component's generic arm, with a clues-on-card chip.

**Architecture:** `Card`'s generic (`None`) arm already renders Treachery name/traits/text/weakness; this adds the live-chip footer to that arm and a `clues N` chip to `live_state_chips`. The board renders each `inv.threat_area` `CardInPlay` as a `<Card>` in the threat `.card-row`.

**Tech Stack:** Rust, Leptos (CSR), Trunk, `wasm-bindgen-test` (headless Firefox), `card_registry`/`cards::REGISTRY`, `game_core::test_support::fixtures`.

## Global Constraints

- **Warnings are errors in CI** across seven jobs (native + wasm). Before pushing: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Clippy `pedantic` is on** (`module_name_repetitions` / `must_use_candidate` allowed); `doc_markdown` enforced.
- **wasm-only test files** carry crate-level `#![cfg(target_arch = "wasm32")]`; headless tests share one page (scope to last subtree).
- **`CardInPlay` is `#[non_exhaustive]`** — construct via `CardInPlay::enter_play(code, id)` then mutate public fields (`clues`).
- **Stats from the corpus / `CardInPlay`** — never hand-typed. Codes: Cover Up `01007` (treachery/weakness, traits "Task.", Revelation/reaction/Forced text, 3 clues).
- **Display-only:** no click handlers.

## File structure

- **Modify `crates/web/src/card.rs`** — `live_state_chips` gains a clues chip; the generic (`None`) arm renders the live-chip footer. Pure test in `#[cfg(test)] mod tests`.
- **Modify `crates/web/tests/card.rs`** — headless test: Cover Up renders via the generic arm with a `clues 3` chip.
- **Modify `crates/web/src/board.rs`** — threat-area treacheries render as `<Card>`s in the threat `.card-row`.
- **Modify `crates/web/style.css`** — remove the dead `.threat ul` rule.
- **Modify `crates/web/tests/board.rs`** — assert a threat-area treachery renders `.threat .card-row .card`.

Type/path notes (verified):
- `inv.threat_area: Vec<CardInPlay>`. `CardInPlay.clues: u8`.
- `live_state_chips(inst: &CardInPlay, kind: &CardKind) -> Vec<String>` (in `card.rs`) currently emits uses + soak chips.
- The `Card` component (`card.rs`): `live_views` is built once (line ~349) from `live_state_chips`; the `Some(face)` arm renders it in `card-footer`; the `None` arm (line ~413) renders `card--generic` with no footer.
- `Card`'s missing-metadata early return yields `card card--unknown` (the synthetic `_synth_treachery` has no metadata → that branch; still a `.card`).
- `tests/card.rs` already imports `CardCode`/`CardInPlay`/`CardInstanceId` and has `last_card_html()` + `last_card_classes()` helpers.

---

### Task 1: Clues chip + generic-arm live footer on `Card`

**Files:**
- Modify: `crates/web/src/card.rs`
- Modify: `crates/web/tests/card.rs`

**Interfaces:**
- `live_state_chips` now appends `"clues {n}"` when `inst.clues > 0` (after uses/soak).
- `Card`'s `None` (generic) arm renders a `card-footer` with the `card-live` chips.

- [ ] **Step 1: Write the failing pure test**

Add to the `#[cfg(test)] mod tests` block in `crates/web/src/card.rs`:

```rust
    #[test]
    fn live_state_chips_includes_clues_on_card() {
        // A treachery (Cover Up) in the threat area carries clues on the card.
        let meta = cards::by_code("01007").expect("Cover Up in corpus");
        let mut inst = TestCardInPlay::enter_play(CardCode::new("01007"), CardInstanceId(0));
        inst.clues = 3;
        assert_eq!(live_state_chips(&inst, &meta.kind), vec!["clues 3".to_string()]);
    }

    #[test]
    fn live_state_chips_omits_clues_when_zero() {
        let meta = cards::by_code("01007").expect("Cover Up in corpus");
        let inst = TestCardInPlay::enter_play(CardCode::new("01007"), CardInstanceId(0));
        assert!(live_state_chips(&inst, &meta.kind).is_empty());
    }
```

(`TestCardInPlay` is the test module's existing alias for `CardInPlay`; `CardInstanceId`/`CardCode` are already imported there.)

- [ ] **Step 2: Run the pure test to verify it fails**

Run: `cargo test -p web card::tests::live_state_chips_includes_clues`
Expected: FAIL — `live_state_chips` returns `[]` (no clues chip yet).

- [ ] **Step 3: Add the clues chip to `live_state_chips`**

In `crates/web/src/card.rs`, in `live_state_chips`, add the clues chip after the `Asset` soak block, before `chips` is returned:

```rust
    if let CardKind::Asset { health, sanity, .. } = kind {
        if let Some(cap) = health {
            chips.push(format!("dmg {}/{cap}", inst.accumulated_damage));
        }
        if let Some(cap) = sanity {
            chips.push(format!("hor {}/{cap}", inst.accumulated_horror));
        }
    }
    if inst.clues > 0 {
        chips.push(format!("clues {}", inst.clues));
    }
    chips
}
```

- [ ] **Step 4: Run the pure test to verify it passes**

Run: `cargo test -p web card::tests`
Expected: PASS (the two new tests + all prior card tests).

- [ ] **Step 5: Write the failing headless test**

Add to `crates/web/tests/card.rs`:

```rust
#[wasm_bindgen_test]
async fn treachery_renders_generic_face_with_clues() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Cover Up 01007: treachery/weakness, traits "Task.", Revelation text;
    // enters the threat area with clues on the card.
    let mut inst = CardInPlay::enter_play(CardCode::new("01007"), CardInstanceId(0));
    inst.clues = 3;
    leptos::mount::mount_to_body(move || view! { <Card code=CardCode::new("01007") in_play=inst.clone()/> });
    leptos::task::tick().await;

    assert!(last_card_classes().contains("card--generic"), "treachery should use the generic arm");
    let html = last_card_html();
    assert!(html.contains("Cover Up"), "name missing: {html}");
    assert!(html.contains("Task"), "trait missing: {html}");
    assert!(html.contains("clues 3"), "clues-on-card chip missing: {html}");
}
```

- [ ] **Step 6: Run the headless test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test card`
Expected: FAIL — the generic arm has no footer, so `clues 3` is absent.

- [ ] **Step 7: Render the live footer in the generic arm**

In `crates/web/src/card.rs`, replace the `None` arm (the `card--generic` block + its comment):

```rust
        None => {
            // In-play overlays (exhausted dim/badge, live chips) apply only to
            // the Asset face above; only assets reach `cards_in_play` today, so a
            // non-asset in-play card renders its plain face here.
            view! {
                <div class="card card--generic">
                    <div class="card-head">
                        <span class="card-name">{name}</span>
                        {weakness_view}
                    </div>
                    <div class="card-traits">{traits}</div>
                    <div class="card-text">{text_view}</div>
                </div>
            }
            .into_any()
        }
```

with (add the live-chip footer; `exhausted` dim/badge stays Asset-only — no in-scope non-asset exhausts):

```rust
        None => {
            // The generic face (Treachery / Location / …). Per-instance state
            // surfaces as the live-chip footer (e.g. a threat-area treachery's
            // clues-on-card); the exhausted dim/badge stays Asset-only — no
            // in-scope non-asset card exhausts.
            view! {
                <div class="card card--generic">
                    <div class="card-head">
                        <span class="card-name">{name}</span>
                        {weakness_view}
                    </div>
                    <div class="card-traits">{traits}</div>
                    <div class="card-text">{text_view}</div>
                    <div class="card-footer">
                        <span class="card-live">{live_views}</span>
                    </div>
                </div>
            }
            .into_any()
        }
```

(`live_views` is already computed before the `match` at line ~349; it is moved into whichever arm runs.)

- [ ] **Step 8: Run the tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web --test card`
Expected: PASS (the new treachery test + existing card tests). Confirm native: `cargo test -p web card::tests` (PASS).

- [ ] **Step 9: Verify clippy (both targets) + fmt**

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings` and `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` and `cargo fmt --check`.
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add crates/web/src/card.rs crates/web/tests/card.rs
git commit -m "web: clues-on-card chip + generic-arm live footer on Card

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 2: Render threat-area treacheries as cards

**Files:**
- Modify: `crates/web/src/board.rs` (the threat builder + threat container)
- Modify: `crates/web/style.css`
- Modify: `crates/web/tests/board.rs`

**Interfaces:**
- Consumes: the `Card` component (now renders treachery live chips, Task 1).

- [ ] **Step 1: Write the failing board test**

In `crates/web/tests/board.rs`, add a new test:

```rust
#[wasm_bindgen_test]
async fn threat_area_treachery_renders_as_card() {
    use game_core::state::{CardCode, CardInPlay, CardInstanceId};

    let mut inv = test_investigator(1);
    inv.threat_area = vec![CardInPlay::enter_play(
        CardCode::new("_synth_treachery"),
        CardInstanceId(0),
    )];
    let state = GameStateBuilder::new().with_investigator(inv).build();

    let html = render_state(state).await;

    let card = leptos::prelude::document()
        .query_selector(".threat .card-row .card")
        .expect("query_selector");
    assert!(card.is_some(), "threat-area treachery should render as a card: {html}");
}
```

(`render_state` installs the synthetic registry; `_synth_treachery` has no metadata → `Card` renders its `card--unknown` fallback, still a `.card`.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test board`
Expected: FAIL — treacheries still render as `<li class="card-line">` inside the threat `<ul>`, so `.threat .card-row .card` does not exist.

- [ ] **Step 3: Render treacheries as `Card`s in `board.rs`**

In `crates/web/src/board.rs`, replace the `threat` builder:

```rust
            let threat: Vec<_> = inv
                .threat_area
                .iter()
                .map(|c| view! { <li class="card-line">{crate::names::card_name(&c.code)}</li> })
                .collect();
```

with:

```rust
            let threat: Vec<_> = inv
                .threat_area
                .iter()
                .cloned()
                .map(|c| {
                    let code = c.code.clone();
                    view! { <crate::card::Card code=code in_play=c/> }
                })
                .collect();
```

Then change the threat container from:

```rust
                    <div class="threat"><h4>"Threat area"</h4><ul>{threat}</ul><div class="card-row">{engaged}</div></div>
```

to (treacheries + engaged enemies in one card-row):

```rust
                    <div class="threat"><h4>"Threat area"</h4><div class="card-row">{threat}{engaged}</div></div>
```

- [ ] **Step 4: Remove the dead `.threat ul` CSS rule**

In `crates/web/style.css`, delete the line:

```css
.threat ul { list-style: none; padding-left: 0; margin: 0; }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web --test board`
Expected: PASS (all board tests, including the new threat-treachery card test).

- [ ] **Step 6: Verify clippy + build**

Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` and `cargo fmt --check` and `cd crates/web && trunk build`.
Expected: clean / builds.

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/board.rs crates/web/style.css crates/web/tests/board.rs
git commit -m "web: render threat-area treacheries as Card rectangles

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

Extend the visual-card-rendering bullet in `docs/phases/phase-7-the-gathering.md` (browser-capstone section) with slice 5: threat-area treacheries now render via the `Card` generic arm (name/traits/text/weakness) with a clues-on-card chip (Cover Up's 3 clues), in the threat `.card-row` alongside the engaged-enemy cards; `live_state_chips` gained a clues chip and the generic arm a live-chip footer. Note act/agenda is the remaining zone and the interactivity pass is still pending. Reference the new spec/plan.

- [ ] **Step 3: Open the PR**

Branch is `web/treachery-cards`. File an issue first (issue-first convention), push, open the PR with `gh pr create`; `Closes` the issue. Design-decisions paragraph: reuse `Card`'s generic arm (treachery = code + `CardInPlay`); clues-on-card via a `live_state_chips` chip + a generic-arm footer; treacheries join the threat `.card-row`.

---

## Self-review notes

- **Spec coverage:** reuse `Card` generic arm for treacheries (Task 2) ✓; `live_state_chips` clues chip (Task 1) ✓; generic-arm live footer (Task 1) ✓; board threat treacheries → `.card-row`, dead `.threat ul` removed (Task 2) ✓; pure + headless (Cover Up) + board tests (Task 1/2) ✓; exhaust/attachments out of scope (generic arm doesn't add exhausted) ✓.
- **Type consistency:** `live_state_chips` signature unchanged; `live_views` (built pre-match) consumed in the generic arm; `Card` prop `in_play` reused at the board call site; `CardInPlay.clues` field name correct. Tests build `CardInPlay` via `enter_play` + field mutation.
- **Out of scope (unchanged):** act/agenda, treachery exhaust/attachments, clickable cards, the icon font.
