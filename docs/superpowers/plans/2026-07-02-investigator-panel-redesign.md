# Investigator panel redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The investigator panel's bottom zone becomes a two-column row: the investigator card (skills + hp/sanity folded on) with an actions/resources/clues/status cluster beside it, and the hand filling the space to their right.

**Architecture:** Rework `investigators_panel` in `board.rs` — drop the under-the-name card block and the standalone `.inv-stats` line (and the `location` display), and rebuild `.inv-zones-bottom` as `.investigator-block` (the S4 `InPlayCardView` + a `.inv-vitals` footer + a `.inv-meta` cluster) beside `.hand`. Display-only; `InPlayCardView` reused untouched.

**Tech Stack:** Rust, Leptos 0.8 (CSR/wasm), `wasm-bindgen-test` headless Firefox.

## Global Constraints

- Issue: **#547**. Design: `docs/superpowers/specs/2026-07-02-investigator-panel-redesign-design.md`.
- Branch: `ui/investigator-panel-redesign` (created; spec committed on it). Commit scope: `web:`.
- **Display-only** — no engine / interaction / `InPlayCardView` change. `location` drops (shown on the map token).
- Match CI's strict flags before pushing (all seven jobs). Merge only after approval.

---

### Task 1: rebuild the investigator-panel bottom zone

**Files:**
- Modify: `crates/web/src/board.rs` (`investigators_panel`)
- Modify: `crates/web/style.css` (bottom-zone layout; remove `.inv-stats`)
- Modify: `crates/web/tests/board.rs` (`investigators_panel_renders_stats_and_hand`)

**Interfaces:** none new — reuses `InPlayCardView` and `Investigator` accessors (`skills.{willpower,intellect,combat,agility}`, `damage()`/`max_health()`, `horror()`/`max_sanity()`, `actions_remaining`, `resources`, `clues`, `status`).

- [ ] **Step 1: Update the board test to the new layout** — in `crates/web/tests/board.rs`, replace the body of `investigators_panel_renders_stats_and_hand`

```rust
async fn investigators_panel_renders_stats_and_hand() {
    use game_core::state::{CardCode, CardInPlay, CardInstanceId, Skills};

    let mut inv = test_investigator(1);
    inv.name = "Roland Banks".to_string();
    inv.skills = Skills {
        willpower: 5,
        intellect: 4,
        combat: 3,
        agility: 2,
    };
    inv.investigator_card.accumulated_damage = 2; // hp 2/8
    inv.investigator_card.accumulated_horror = 1; // san 1/8
    inv.clues = 3;
    inv.resources = 4;
    inv.actions_remaining = 2;
    inv.hand = vec![
        CardCode::new("_synth_fast_event"),
        CardCode::new("_synth_treachery"),
    ];
    inv.cards_in_play = vec![CardInPlay::enter_play(
        CardCode::new("_synth_asset"),
        CardInstanceId(0),
    )];
    let state = GameStateBuilder::new().with_investigator(inv).build();

    let html = render_state(state).await;
    let doc = leptos::prelude::document();

    // Identity + folded vitals (skills + hp/san) live in the investigator block.
    assert!(html.contains("Roland Banks"), "name missing: {html}");
    assert!(html.contains("W5"), "willpower missing: {html}");
    assert!(html.contains("I4"), "intellect missing: {html}");
    assert!(html.contains("C3"), "combat missing: {html}");
    assert!(html.contains("A2"), "agility missing: {html}");
    assert!(html.contains("2/8"), "hp missing: {html}");
    assert!(html.contains("1/8"), "san missing: {html}");
    // Meta cluster: actions as pips, resources, clues, status.
    let pips = doc
        .query_selector_all(".inv-meta .inv-actions .pip")
        .expect("query");
    assert_eq!(pips.length(), 2, "two action pips: {html}");
    assert!(html.contains("resources 4"), "resources missing: {html}");
    assert!(html.contains("clues 3"), "clues missing: {html}");
    // Location is no longer shown in the panel (it's on the map token).
    assert!(!html.contains("inv-location"), "location line should be gone: {html}");

    // Cards still render (hand + in-play).
    assert!(html.contains("_synth_fast_event"), "hand card missing: {html}");
    assert!(html.contains("_synth_asset"), "in-play card missing: {html}");
    assert!(
        doc.query_selector_all(".in-play .card-row .card")
            .expect("query")
            .length()
            >= 1,
        "in-play Card: {html}"
    );

    // Layout: in-play + threat in the top row; investigator block (card + vitals +
    // meta) and hand in the bottom row.
    for sel in [
        ".investigator .inv-zones-top .in-play",
        ".investigator .inv-zones-top .threat",
        ".investigator .inv-zones-bottom .investigator-block .investigator-card .card-slot",
        ".investigator .inv-zones-bottom .investigator-block .inv-vitals",
        ".investigator .inv-zones-bottom .investigator-block .inv-meta",
        ".investigator .inv-zones-bottom .hand",
    ] {
        assert!(
            doc.query_selector(sel).expect("query ok").is_some(),
            "expected layout element {sel}: {html}"
        );
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test board 2>&1 | tail -15`
Expected: `investigators_panel_renders_stats_and_hand` FAILs (no `.investigator-block` / `.pip` / skills; still has the old `.inv-stats` layout).

- [ ] **Step 3: Rebuild the panel** — in `crates/web/src/board.rs` `investigators_panel`:

Delete the now-unused `location` binding (the `let location = inv.current_location.map_or_else(...)` block).

Replace the `view!` block (from `<article class="investigator">` through its `</article>`) with:

```rust
            let vitals = view! {
                <div class="inv-vitals">
                    <span class="inv-skills">
                        "W" {inv.skills.willpower} " I" {inv.skills.intellect}
                        " C" {inv.skills.combat} " A" {inv.skills.agility}
                    </span>
                    <span class="inv-hp">"hp " {inv.damage()} "/" {inv.max_health()}</span>
                    <span class="inv-san">"san " {inv.horror()} "/" {inv.max_sanity()}</span>
                </div>
            };
            let pips: Vec<_> = (0..inv.actions_remaining)
                .map(|_| view! { <span class="pip">"●"</span> })
                .collect();
            view! {
                <article class="investigator">
                    <h3 class="inv-name">{inv.name.clone()}</h3>
                    <div class="inv-zones-top">
                        <div class="in-play"><h4>"In play"</h4><div class="card-row">{in_play}</div></div>
                        <div class="threat"><h4>"Threat area"</h4><div class="card-row">{threat}{engaged}</div></div>
                    </div>
                    <div class="inv-zones-bottom">
                        <div class="investigator-block">
                            <div class="investigator-card">
                                <crate::card::InPlayCardView instance=inv.investigator_card.clone()/>
                                {vitals}
                            </div>
                            <div class="inv-meta">
                                <span class="inv-actions">"actions " {pips}</span>
                                <span class="inv-resources">"resources " {inv.resources}</span>
                                <span class="inv-clues">"clues " {inv.clues}</span>
                                <span class="inv-status">{format!("{:?}", inv.status)}</span>
                            </div>
                        </div>
                        <div class="hand"><h4>"Hand"</h4><div class="card-row">{hand}</div></div>
                    </div>
                </article>
            }
```

- [ ] **Step 4: Update the CSS** — in `crates/web/style.css`:

Find the existing `.inv-zones-bottom` and `.inv-stats` rules. Replace the `.inv-stats` rule (and adjust `.inv-zones-bottom`) with:

```css
.inv-zones-bottom { display: flex; gap: 1rem; align-items: flex-start; }
.investigator-block { display: flex; gap: 0.6rem; align-items: flex-start; }
.inv-vitals {
    display: flex; flex-direction: column; gap: 0.1rem;
    font-size: 0.8rem; padding: 0.25rem 0.4rem;
    border: 1px solid #ccc; border-top: none; border-radius: 0 0 6px 6px;
}
.inv-skills { font-weight: bold; letter-spacing: 0.05rem; }
.inv-meta { display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.85rem; }
.inv-actions .pip { color: #c8a020; margin-right: 1px; }
```

(If the file has no `.inv-stats` rule, just add the block above near the other
`.inv-*` rules and update `.inv-zones-bottom`. If `.inv-zones-bottom` already sets
`display: flex`, keep the one definition — don't duplicate the selector.)

- [ ] **Step 5: Run the board test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web --test board 2>&1 | tail -15`
Expected: all `board` tests pass, including `investigators_panel_renders_stats_and_hand`.

- [ ] **Step 6: Regression — the S4 investigator-card reaction glow still works** (the card moved zones but is still an `InPlayCardView` under `.investigator-card`)

Run: `wasm-pack test --headless --firefox crates/web --test map 2>&1 | tail -8`
Expected: all `map` tests pass, including `investigator_card_glows_for_a_reaction_anchored_to_it`.

- [ ] **Step 7: Host clippy** (the removed `location` binding must leave no unused-var / unused-import)

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/web/src/board.rs crates/web/style.css crates/web/tests/board.rs
git commit -m "web: investigator card + folded skills/vitals beside the hand"
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

Watch for: an unused `location` / `crate::names::location_name` if the binding removal missed a use (grep confirms `location_name` is still used in `map.rs`, so only the `board.rs` local binding goes); a stale `.inv-stats` selector left in `style.css`.

## PR flow (after the gauntlet is green)

1. Push `ui/investigator-panel-redesign`; open the PR. Body: goal + `Closes #547.`
2. `gh pr checks <PR#> --watch`.
3. **Phase-doc note is optional here** — #547 is a display-only follow-up already
   referenced in the phase-7 doc's S5–S6 bullet; if anything, drop the "queued
   follow-up" clause once merged. Not a phase-issue closure, so no Closed-table move.

## Self-review notes

- **Spec coverage:** two-column bottom zone ✅; investigator card + `.inv-vitals` (skills + hp/san) ✅; `.inv-meta` (actions pips / resources / clues / status) ✅; hand to the right ✅; `.inv-stats` line + `location` removed ✅; card moved from under the name ✅; `InPlayCardView` untouched ✅; display-only ✅.
- **Testing:** panel renders skills / hp/san / actions-pips / resources / clues + layout selectors ✅; location-gone assertion ✅; S4 reaction-glow regression ✅.
- **Type consistency:** `inv.skills.{willpower,intellect,combat,agility}`, `inv.damage()/max_health()`, `inv.horror()/max_sanity()`, `inv.actions_remaining`, `inv.resources`, `inv.clues`, `inv.status`, `inv.investigator_card` — all existing `Investigator` members; `Skills` imported from `game_core::state` in the test.
