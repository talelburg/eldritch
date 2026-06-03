# #147 Mulligan Player-Order Cursor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the order-insensitive mulligan model (`GameState.mulligan_window: bool` + `Investigator.mulligan_used: bool` + an all-used completion scan) with a `turn_order`-driven `mulligan_pending: Option<InvestigatorId>` cursor that enforces "each player, in player order, may mulligan once" (Rules Reference p.16 / p.27).

**Architecture:** The cursor mirrors the existing `mythos_draw_pending` exactly. `start_scenario` seeds it via `first_active_investigator(state)`; the `mulligan()` handler validates `mulligan_pending == Some(investigator)` (a single check that subsumes window-open + already-used + wrong-player) and advances via `next_active_investigator_after`; `apply_player_action` kicks off the Investigation phase when the cursor reaches `None`. Because player order is fixed by `turn_order`, no interactive choice is needed — this is single-player-complete.

**Tech Stack:** Rust, `cargo test`. Engine crate `game-core`. Helpers `first_active_investigator` / `next_active_investigator_after` already exist in `crates/game-core/src/engine/dispatch.rs`.

**Spec:** `docs/superpowers/specs/2026-06-03-147-mulligan-player-order-cursor-design.md`

---

## Notes for the implementer

- **Rust compiles whole-crate.** `game-core`'s `test_support` (builder, fixtures) is unconditionally `pub`, so it's part of the **lib** build; the `#[cfg(test)]` inline tests only compile under `cargo test`. Task 1 therefore reaches a green **lib** build (`cargo build -p game-core`) partway through, then a green **test** build at the end. The workspace is intentionally red *between* steps within Task 1 — that's expected for a field-removal refactor; the single commit at the end of Task 1 is the first green checkpoint.
- **Do not** introduce a status check in the `mulligan()` handler. The cursor invariant (seed/advance only ever yield `Active` `turn_order` ids) makes it redundant; the design deliberately collapses the three old validations into the one cursor check.
- **Field placement:** keep `mulligan_pending` where `mulligan_window` lived (same struct position) — surgical, and serde keys on field names not order. The doc comment carries the `mythos_draw_pending`-mirror cross-reference.

---

## Task 1: Refactor mulligan to a player-order cursor (game-core)

This is one atomic change (field removal couples state + engine + inline tests). Single commit at the end.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (field + an intra-doc link)
- Modify: `crates/game-core/src/state/investigator.rs` (remove field + serde test JSON)
- Modify: `crates/game-core/src/test_support/fixtures.rs` (remove field init)
- Modify: `crates/game-core/src/test_support/builder.rs` (field + helper + build())
- Modify: `crates/game-core/src/engine/dispatch.rs` (seed + handler + gate + completion + inline tests)
- Modify: `crates/game-core/src/action.rs` (Mulligan doc comment)
- Modify: `crates/game-core/src/engine/mod.rs` (inline mulligan tests)

---

- [ ] **Step 1: Swap the `GameState` field**

In `crates/game-core/src/state/game_state.rs`, replace the `mulligan_window` field + its doc (currently lines ~66–73):

```rust
    /// Whether the mulligan setup window is open. Set true at the end
    /// of [`PlayerAction::StartScenario`](crate::action::PlayerAction::StartScenario)
    /// processing; cleared once every investigator has
    /// `mulligan_used == true`. While open, investigators may submit
    /// [`PlayerAction::Mulligan`](crate::action::PlayerAction::Mulligan)
    /// to redraw a subset of their starting hand; the engine rejects
    /// every non-Mulligan player action until the window closes.
    pub mulligan_window: bool,
```

with the cursor field:

```rust
    /// The investigator whose setup mulligan is pending, processed in
    /// player order (Rules Reference p.16 / p.27: "each player, in
    /// player order, may mulligan once"). Mirror of
    /// [`mythos_draw_pending`](Self::mythos_draw_pending):
    ///
    /// - Seeded to the first [`Status::Active`](crate::state::Status::Active)
    ///   investigator in [`turn_order`](Self::turn_order) at
    ///   [`PlayerAction::StartScenario`](crate::action::PlayerAction::StartScenario).
    /// - A [`PlayerAction::Mulligan`](crate::action::PlayerAction::Mulligan)
    ///   is valid only when `mulligan_pending == Some(that investigator)`;
    ///   on success the cursor advances to the next Active investigator
    ///   in `turn_order`.
    /// - `None` once every investigator has mulliganed — at which point
    ///   setup ends and the Investigation phase begins. While `Some`,
    ///   the engine rejects every non-Mulligan player action.
    pub mulligan_pending: Option<InvestigatorId>,
```

Then fix the now-dangling intra-doc link in the `in_flight_skill_test` doc (currently line ~107): change

```rust
    /// rejects (mirrors the [`mulligan_window`](Self::mulligan_window)
    /// guard).
```

to

```rust
    /// rejects (mirrors the [`mulligan_pending`](Self::mulligan_pending)
    /// guard).
```

- [ ] **Step 2: Remove `mulligan_used` from `Investigator`**

In `crates/game-core/src/state/investigator.rs`, delete the field + doc (lines ~81–85):

```rust
    /// Whether this investigator has used their one-shot mulligan
    /// during scenario setup. Set true after a successful Mulligan
    /// action; remains true for the rest of the scenario so a second
    /// mulligan rejects.
    pub mulligan_used: bool,
```

In the same file, the serde test `deserializes_when_field_absent` (line ~200) has `"mulligan_used": false` in its JSON literal. Remove that key so the JSON matches the struct. Change:

```rust
            "cards_in_play": [], "mulligan_used": false
```

to:

```rust
            "cards_in_play": []
```

- [ ] **Step 3: Remove `mulligan_used` from the fixture**

In `crates/game-core/src/test_support/fixtures.rs`, delete the line (line ~57):

```rust
        mulligan_used: false,
```

- [ ] **Step 4: Update the test builder**

In `crates/game-core/src/test_support/builder.rs`:

1. Field (line ~53): `mulligan_window: bool,` → `mulligan_pending: Option<InvestigatorId>,`
2. `new()` default (line ~75): `mulligan_window: false,` → `mulligan_pending: None,`
3. Replace the helper (lines ~199–207):

```rust
    /// Open the mulligan window. By default the window is closed so
    /// tests don't accidentally exercise Mulligan paths; opt in by
    /// calling this on the builder when a test wants to fire the
    /// Mulligan action directly without going through
    /// `StartScenario`.
    pub fn with_mulligan_window_open(mut self) -> Self {
        self.mulligan_window = true;
        self
    }
```

with:

```rust
    /// Seed the mulligan cursor to `id`. By default the cursor is
    /// `None` so tests don't accidentally exercise Mulligan paths; opt
    /// in when a test wants to fire the Mulligan action directly
    /// without going through `StartScenario`. The investigator must be
    /// in `turn_order` (set via [`with_turn_order`](Self::with_turn_order))
    /// for the cursor to advance correctly after the mulligan.
    pub fn with_mulligan_pending(mut self, id: InvestigatorId) -> Self {
        self.mulligan_pending = Some(id);
        self
    }
```

4. `build()` (line ~262): `mulligan_window: self.mulligan_window,` → `mulligan_pending: self.mulligan_pending,`

- [ ] **Step 5: Seed the cursor in `start_scenario`**

In `crates/game-core/src/engine/dispatch.rs`, replace the window-open line in `start_scenario` (currently lines ~894–899):

```rust
    // Open the mulligan window. Each investigator may now submit a
    // single `PlayerAction::Mulligan` to redraw a subset of their
    // starting hand. The window closes once every investigator has
    // `mulligan_used == true` (see `apply_player_action`); other
    // player actions are rejected until then.
    state.mulligan_window = true;
```

with:

```rust
    // Seed the mulligan cursor to the first Active investigator in
    // player order. Each investigator submits a single
    // `PlayerAction::Mulligan` in turn; the cursor advances after each
    // and reaches `None` once all have gone (see `apply_player_action`),
    // at which point setup ends. Other player actions are rejected while
    // the cursor is `Some`. An empty/all-eliminated `turn_order` seeds
    // `None` — the same degenerate no-op as the Mythos draw cursor.
    state.mulligan_pending = first_active_investigator(state);
```

- [ ] **Step 6: Rewrite the `mulligan()` handler prologue + advance the cursor**

In `crates/game-core/src/engine/dispatch.rs`, in `mulligan()` replace the four validation blocks (currently lines ~4057–4082):

```rust
    if !state.mulligan_window {
        return EngineOutcome::Rejected {
            reason: "Mulligan: setup window has closed (every investigator has already \
                     mulliganed and normal play has begun)"
                .into(),
        };
    }
    let Some(inv) = state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("Mulligan: investigator {investigator:?} is not in state").into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "Mulligan: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if inv.mulligan_used {
        return EngineOutcome::Rejected {
            reason: format!("Mulligan: {investigator:?} has already used their mulligan").into(),
        };
    }
```

with the single cursor check (the cursor invariant guarantees the investigator is Active and present in the map, so `inv` is fetched infallibly, mirroring `end_turn`'s `unreachable!` for a cursor/active id missing from the map):

```rust
    // One check subsumes the three old ones: the cursor only ever holds
    // an Active `turn_order` id, so a mismatch covers setup-over (`None`),
    // wrong-player / too-early, and already-went (cursor moved past you).
    if state.mulligan_pending != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Mulligan: it is not {investigator:?}'s turn to mulligan \
                 (pending: {:?})",
                state.mulligan_pending,
            )
            .into(),
        };
    }
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "mulligan_pending {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
    });
```

Then in the mutate section, delete the line that sets the removed flag (currently line ~4112):

```rust
    inv_mut.mulligan_used = true;
```

Finally, advance the cursor just before the `Done` return. Replace the tail (currently lines ~4121–4125):

```rust
    events.push(Event::MulliganPerformed {
        investigator,
        redrawn_count,
    });
    EngineOutcome::Done
}
```

with:

```rust
    events.push(Event::MulliganPerformed {
        investigator,
        redrawn_count,
    });
    // Advance to the next Active investigator in player order (or `None`
    // when this was the last). The completion check in
    // `apply_player_action` keys off `None` to end setup.
    state.mulligan_pending = next_active_investigator_after(state, investigator);
    EngineOutcome::Done
}
```

Also update the `mulligan()` doc comment (lines ~4040–4050) so it no longer says "Validates the mulligan window is open … hasn't already mulliganed" — describe the cursor check instead:

```rust
/// Per the Rules Reference, the redrawn cards shuffle directly back
/// into the deck (not via the discard pile). Validates that it is this
/// investigator's turn to mulligan (`mulligan_pending == Some(investigator)`,
/// Rules Reference p.16 player order) and that the redraw indices are in
/// bounds and unique.
///
/// On success: move named hand cards to the deck, shuffle, draw the
/// same count back, advance `mulligan_pending` to the next investigator
/// in player order, emit `MulliganPerformed`. An empty `indices_to_redraw`
/// is a legal "keep my hand" mulligan that consumes the turn without
/// touching the deck.
```

- [ ] **Step 7: Update the outer gate + completion kickoff in `apply_player_action`**

In `crates/game-core/src/engine/dispatch.rs`, the setup gate (currently line ~68):

```rust
    if state.mulligan_window
        && !matches!(
            action,
            PlayerAction::Mulligan { .. } | PlayerAction::StartScenario
        )
    {
```

becomes:

```rust
    if state.mulligan_pending.is_some()
        && !matches!(
            action,
            PlayerAction::Mulligan { .. } | PlayerAction::StartScenario
        )
    {
```

The completion block (currently lines ~194–212):

```rust
    if matches!(outcome, EngineOutcome::Done)
        && matches!(action, PlayerAction::Mulligan { .. })
        && state.investigators.values().all(|inv| inv.mulligan_used)
    {
        state.mulligan_window = false;
        // Setup complete — "the game begins" (Rules Reference p.27).
```

becomes (drop the `mulligan_window = false` line; the cursor is already `None`):

```rust
    if matches!(outcome, EngineOutcome::Done)
        && matches!(action, PlayerAction::Mulligan { .. })
        && state.mulligan_pending.is_none()
    {
        // Setup complete — "the game begins" (Rules Reference p.27).
```

Leave the rest of that block (the `investigation_phase(state, events);` call and its surrounding comment) intact.

- [ ] **Step 8: Update the `Mulligan` action doc comment**

In `crates/game-core/src/action.rs`, the `Mulligan` variant doc (lines ~136–139) currently says the window "opens at `StartScenario` and closes once every investigator has `mulligan_used == true`". Replace that prose so it describes the cursor:

```rust
    /// Setup-only redraw. Valid only when this investigator is the one
    /// the `mulligan_pending` cursor points at — mulligans happen in
    /// player order (Rules Reference p.16 / p.27). The cursor is seeded
    /// at `StartScenario` and advances after each mulligan; when it
    /// reaches `None` setup ends and the game begins.
```

(Keep the rest of the variant's doc — the `indices_to_redraw` / empty-mulligan description — unchanged. Also update the "Must be `Status::Active` and not have already used their mulligan this scenario." line on the `investigator` field to: "Must be the investigator the `mulligan_pending` cursor currently points at.")

- [ ] **Step 9: Verify the lib compiles**

Run: `cargo build -p game-core`
Expected: PASS (the `#[cfg(test)]` inline tests are not built here; they're fixed in the next steps).

- [ ] **Step 10: Update the inline mulligan tests in `engine/mod.rs`**

In `crates/game-core/src/engine/mod.rs`:

**(a)** `mulligan_scenario()` (lines ~2816–2820) — seed the cursor + turn_order:

```rust
        let state = TestGame::new()
            .with_investigator(inv)
            .with_rng_seed(2026)
            .with_turn_order([id])
            .with_mulligan_pending(id)
            .build();
```

**(b)** In `mulligan_redraw_subset_swaps_named_cards`, `mulligan_redraw_none_keeps_hand_and_consumes_one_shot`, and `multi_investigator_real_redraw_plus_empty_mulligan_combo`, delete the now-invalid `assert!(inv.mulligan_used);` / `assert!(inv2_after.mulligan_used);` lines (lines ~2845, ~2879, ~3208).

**(c)** `mulligan_after_window_closed_is_rejected` (line ~2947): replace `state.mulligan_window = false;` with `state.mulligan_pending = None;`.

**(d)** Replace `mulligan_by_defeated_investigator_is_rejected` (lines ~2959–2972) entirely — the cursor never points at a defeated investigator, so the rejection is by cursor mismatch:

```rust
    #[test]
    fn mulligan_by_defeated_investigator_is_rejected() {
        // The seed/advance helpers skip non-Active investigators, so the
        // cursor never points at a defeated one. With inv1 Killed the
        // cursor sits on inv2; a Mulligan from the defeated inv1 is
        // rejected by the cursor mismatch.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        a.status = Status::Killed;
        let b = test_investigator(2);
        let state = TestGame::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_turn_order([inv1, inv2])
            .with_mulligan_pending(inv2)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }
```

**(e)** Replace `start_scenario_opens_mulligan_window` (lines ~3002–3014) — rename + assert the seed:

```rust
    #[test]
    fn start_scenario_seeds_mulligan_cursor() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = make_test_deck(10);
        let state = TestGame::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.mulligan_pending, Some(id));
    }
```

**(f)** `non_mulligan_action_during_mulligan_window_is_rejected` (lines ~3016–3046): rename to `non_mulligan_action_while_mulligan_pending_is_rejected` and replace `.with_mulligan_window_open()` (line ~3035) with `.with_turn_order([id]).with_mulligan_pending(id)`.

**(g)** `solo_mulligan_closes_the_window` (lines ~3048–3063): rename to `solo_mulligan_clears_the_cursor` and change the final assertion `assert!(!result.state.mulligan_window);` to `assert_eq!(result.state.mulligan_pending, None);`.

**(h)** Replace `multi_investigator_first_mulligan_keeps_window_open` (lines ~3065–3102) with a cursor-advance test:

```rust
    #[test]
    fn multi_investigator_mulligan_advances_cursor_in_player_order() {
        // Two investigators; the cursor advances inv1 → inv2 → None as
        // each mulligans in player order.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        let mut b = test_investigator(2);
        a.hand = vec![CardCode::new("a-0")];
        b.hand = vec![CardCode::new("b-0")];
        let state = TestGame::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_turn_order([inv1, inv2])
            .with_mulligan_pending(inv1)
            .build();

        let after_first = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(after_first.outcome, EngineOutcome::Done);
        assert_eq!(after_first.state.mulligan_pending, Some(inv2));

        let after_second = apply(
            after_first.state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(after_second.outcome, EngineOutcome::Done);
        assert_eq!(after_second.state.mulligan_pending, None);
    }
```

**(i)** Replace `multi_investigator_mulligan_order_does_not_matter` (lines ~3104–3137) with the inverted order-enforcement test:

```rust
    #[test]
    fn multi_investigator_mulligan_out_of_order_is_rejected() {
        // Cursor is on inv1 (first in turn_order). inv2 trying to
        // mulligan out of turn is rejected, and the cursor is unmoved
        // (Rules Reference p.16: mulligans in player order).
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        let mut b = test_investigator(2);
        a.hand = vec![CardCode::new("a-0")];
        b.hand = vec![CardCode::new("b-0")];
        let state = TestGame::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_turn_order([inv1, inv2])
            .with_mulligan_pending(inv1)
            .build();

        let out_of_order = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
        );
        assert!(matches!(out_of_order.outcome, EngineOutcome::Rejected { .. }));
        assert!(out_of_order.events.is_empty());
        assert_eq!(out_of_order.state.mulligan_pending, Some(inv1));
    }
```

**(j)** `multi_investigator_real_redraw_plus_empty_mulligan_combo` (lines ~3139–3209): add `.with_turn_order([inv1, inv2]).with_mulligan_pending(inv1)` to the builder (line ~3160–3165, alongside `.with_rng_seed(99)`), replace `assert!(after_inv1.state.mulligan_window);` (line ~3177) with `assert_eq!(after_inv1.state.mulligan_pending, Some(inv2));`, and replace `assert!(!after_inv2.state.mulligan_window);` (line ~3198) with `assert_eq!(after_inv2.state.mulligan_pending, None);`. (The `mulligan_used` assert removal from (b) also applies here.) inv1 redraws first, matching the cursor — no reorder needed.

- [ ] **Step 11: Update the dispatch.rs completion test**

In `crates/game-core/src/engine/dispatch.rs`, `mulligan_completion_kicks_off_investigation_phase` (lines ~6079–6112):

- Replace `state.mulligan_window = true;` (line ~6089) and its `// test_investigator(1) already defaults mulligan_used = false; …` comment with:

```rust
        state.mulligan_pending = Some(InvestigatorId(1));
```

- Replace the assertion block (lines ~6104–6107):

```rust
        assert!(
            !state.mulligan_window,
            "mulligan window closes once every investigator has mulliganed"
        );
```

with:

```rust
        assert_eq!(
            state.mulligan_pending, None,
            "mulligan cursor clears once every investigator has mulliganed"
        );
```

- [ ] **Step 12: Run game-core tests**

Run: `cargo test -p game-core`
Expected: PASS (all mulligan tests green; no references to `mulligan_window` / `mulligan_used` remain in `game-core`).

If anything fails to compile with "no field `mulligan_window`/`mulligan_used`", grep `crates/game-core/src` for the symbol and fix the straggler the same way (assertion → `mulligan_pending`).

- [ ] **Step 13: Commit**

```bash
git add crates/game-core/src/state/game_state.rs \
        crates/game-core/src/state/investigator.rs \
        crates/game-core/src/test_support/fixtures.rs \
        crates/game-core/src/test_support/builder.rs \
        crates/game-core/src/engine/dispatch.rs \
        crates/game-core/src/engine/mod.rs \
        crates/game-core/src/action.rs
git commit -m "engine: mulligan player-order cursor (mulligan_pending)

Replace mulligan_window + per-investigator mulligan_used with a
mulligan_pending: Option<InvestigatorId> cursor mirroring
mythos_draw_pending. Mulligans now resolve strictly in turn_order
(Rules Reference p.16 / p.27). The single cursor check subsumes the
old window-open / already-used / wrong-player validations; setup ends
and the Investigation phase begins when the cursor reaches None.

Closes #147.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Verify downstream test crates

The `crates/scenarios/tests/*` and `game-core/tests/reaction_windows.rs` files drive `Mulligan` through full `StartScenario` cycles and already submit in `turn_order` order (multi-investigator tests push `InvestigatorId(2)` after a base `turn_order` of `[1]`, then mulligan 1 then 2). They reference neither `mulligan_window` nor `mulligan_used`, so they should pass **unchanged**. This task confirms that.

**Files:** (verification only; edits only if a test surfaces an order mismatch)
- `crates/scenarios/tests/synthetic_resolution.rs`
- `crates/scenarios/tests/upkeep_phase.rs`
- `crates/scenarios/tests/mythos_phase.rs`
- `crates/game-core/tests/reaction_windows.rs`

- [ ] **Step 1: Run the full workspace test suite**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS.

If a downstream test rejects a `Mulligan` (a multi-investigator test mulliganing out of `turn_order`), fix it by reordering the `Mulligan` actions to match that test's `turn_order`. Do **not** change engine behavior. Commit any such fix:

```bash
git add crates/scenarios/tests crates/game-core/tests
git commit -m "test: order mulligans by player order in downstream scenario tests

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

If Step 1 passes with no edits, skip the commit and proceed.

---

## Task 3: Full CI gauntlet + phase-doc update (final commit)

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

- [ ] **Step 1: Run the full CI gauntlet locally**

Run each, all must pass:

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```

Expected: all PASS. The `doc` job in particular catches the `mulligan_window` → `mulligan_pending` intra-doc link fix from Task 1 Step 1.

- [ ] **Step 2: Update the phase doc**

In `docs/phases/phase-4-scenario-plumbing.md` (per `docs/phases/README.md` "Maintaining these docs"):

- **Status** (line ~5): `#147` is the last open follow-up; reflect that it's now closed via this PR. Update the trailing "Open follow-up (not a Phase-4-done blocker): `#147` …" note to drop `#147`.
- **Issues** heading (line ~11): change "open: 1 — `#147`" to "open: 0".
- **Issues table** (lines ~13–15): remove the `#147` row from the open table.
- **Closed table** (after line ~33): add a row:

```markdown
| `#147` | Mulligan in player order: cursor model | #<PR> | `#137` follow-up. `mulligan_pending: Option<InvestigatorId>` cursor replaces `mulligan_window` + per-investigator `mulligan_used`; mulligans resolve strictly in `turn_order` (RR p.16 / p.27). The single cursor check subsumes window-open / already-used / wrong-player; the `#137` kickoff trigger now keys off the cursor reaching `None`. Single-player-complete (player order is fixed by `turn_order`; no interactive choice). |
```

(Fill `#<PR>` with the real PR number once opened.)

- Add a **Decisions made** entry only if it passes the README test (would a future PR-author choose differently without it?). The cursor pattern is already documented by `mythos_draw_pending`'s precedent and discoverable in-code, so **likely omit** — note in the PR description instead.

- [ ] **Step 3: Commit the phase-doc update (final commit on the branch)**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "docs: close #147 in phase-4 plan

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 4: Push + open the PR** (per CLAUDE.md PR procedure)

Push the `engine/mulligan-player-order` branch and open the PR with `gh pr create` against `main`, `Closes #147`, including a short design-decisions paragraph (single cursor check; inverted order tests; `mulligan_used` removed). Then watch CI with `gh pr checks <PR#> --watch`. Backfill the PR number into the phase-doc Closed row (amend the final commit or a follow-up commit). **Do not merge** without explicit user approval.

---

## Self-review

- **Spec coverage:** state field swap (T1 S1), `mulligan_used` removal incl. serde + fixtures (T1 S2–S3), builder helper (T1 S4), seed (T1 S5), single-check handler + advance (T1 S6), gate + completion (T1 S7), docs (T1 S1/S8), inverted + new tests (T1 S10), downstream verification (T2), gauntlet + non-goals respected (no AwaitingInput, no empty-turn_order special-case, no interactive choice), phase doc (T3). All spec sections map to a task.
- **Type consistency:** field is `mulligan_pending: Option<InvestigatorId>` everywhere; builder helper `with_mulligan_pending(id: InvestigatorId)`; helpers `first_active_investigator` / `next_active_investigator_after` used as they exist in dispatch.rs (verified signatures). Test names referenced consistently.
- **Placeholders:** none — every code step shows full old→new text. The only `<PR>` placeholder is intentional (the PR number doesn't exist until T3 S4) and called out.
