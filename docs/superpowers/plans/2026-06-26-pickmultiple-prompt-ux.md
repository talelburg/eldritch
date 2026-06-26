# PickMultiple Prompt UX (#468 + #469) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the upkeep hand-size discard prompt render the investigator's hand (so cards are selectable), and replace the three `PickMultiple` prompts' developer-facing copy + the hardcoded "Commit" button with player-facing text and a neutral "Confirm".

**Architecture:** Add a `GameState::current_hand_size_discard()` accessor mirroring `current_mulligan()`, and extend `active_hand`'s fallback chain in the web client (#468). Rewrite the three engine prompt strings to player copy and change the `PickMultiple` button label (#469).

**Tech Stack:** Rust (`game-core`), Leptos (`crates/web`, wasm32), `wasm-bindgen-test`.

## Global Constraints

- **Solo-scope:** rewritten prompts drop the investigator reference (the prompted player is the active/sole one). No per-investigator naming.
- **YAGNI:** no `confirm_label`/per-context button verbs; no new `InputRequest` fields. A single neutral `"Confirm"` + contextual prompt.
- **CI gauntlet before push** (all seven jobs, warnings-as-errors) — touches `game-core` + `web`, so `wasm-build`/`wasm-test`/`wasm-clippy` matter:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `wasm-pack test --headless --firefox crates/web`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Branch:** `ui/pickmultiple-prompt-ux` (already created; spec committed). One branch, follow-up commits, no force-push.
- Spec of record: `docs/superpowers/specs/2026-06-26-pickmultiple-prompt-ux-design.md`.

---

### Task 1: #468 — render the prompted hand in hand-size discard

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add `current_hand_size_discard()` + a unit test)
- Modify: `crates/web/src/input.rs` (extend `active_hand`'s fallback chain)
- Test: `crates/web/tests/input.rs` (wasm test: hand renders during hand-size discard)

**Interfaces:**
- Produces: `GameState::current_hand_size_discard(&self) -> Option<InvestigatorId>`.

- [ ] **Step 1: Write the failing accessor unit test**

In `crates/game-core/src/state/game_state.rs`, find the `#[cfg(test)] mod tests` block (search for `mod tests` in the file) and add:
```rust
#[test]
fn current_hand_size_discard_reads_the_frame() {
    use super::{Continuation, GameStateBuilder, HandSizeDiscard, InvestigatorId};
    // No frame → None.
    assert_eq!(
        GameStateBuilder::new().build().current_hand_size_discard(),
        None
    );
    // Top HandSizeDiscard frame → its first remaining investigator.
    let mut state = GameStateBuilder::new().build();
    state
        .continuations
        .push(Continuation::HandSizeDiscard(HandSizeDiscard {
            remaining: vec![InvestigatorId(2), InvestigatorId(3)],
        }));
    assert_eq!(
        state.current_hand_size_discard(),
        Some(InvestigatorId(2))
    );
}
```
(Adjust the `use super::{…}` path to whatever the existing tests in that module import — they likely already bring `GameStateBuilder`/`InvestigatorId` into scope; add only `Continuation` and `HandSizeDiscard` if missing.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core --lib current_hand_size_discard_reads_the_frame`
Expected: FAIL to compile — no method `current_hand_size_discard`.

- [ ] **Step 3: Add the accessor**

In `crates/game-core/src/state/game_state.rs`, directly after `current_mulligan()` (around line 1668), add:
```rust
    /// The investigator currently prompted to discard down to the hand-size
    /// limit, if an upkeep hand-size discard is in progress; `None` otherwise.
    /// Reads the top [`Continuation::HandSizeDiscard`] frame's `remaining[0]`
    /// — the frame is only the top while the discard is pending, so `.last()`
    /// is correct (mirrors [`current_mulligan`](Self::current_mulligan)).
    #[must_use]
    pub fn current_hand_size_discard(&self) -> Option<InvestigatorId> {
        match self.continuations.last() {
            Some(Continuation::HandSizeDiscard(h)) => h.remaining.first().copied(),
            _ => None,
        }
    }
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p game-core --lib current_hand_size_discard_reads_the_frame`
Expected: PASS.

- [ ] **Step 5: Extend `active_hand`'s fallback (web)**

In `crates/web/src/input.rs`, update `active_hand`:
```rust
fn active_hand(game: &GameState) -> Vec<String> {
    game.active_investigator
        .or_else(|| game.current_mulligan())
        .or_else(|| game.current_hand_size_discard())
        .and_then(|id| game.investigators.get(&id))
        .map(|inv| inv.hand.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}
```
Update the `active_hand` doc comment's "Falls back to the setup mulligan's prompted investigator … when there is no active investigator" sentence to also mention the hand-size discard frame.

- [ ] **Step 6: Write the failing wasm test (hand renders during discard)**

In `crates/web/tests/input.rs`, add (the file already has `mount`, `awaiting_commit_input`, `test_investigator`, `GameStateBuilder`; add `Continuation`/`HandSizeDiscard` to the `game_core::state` import):
```rust
#[wasm_bindgen_test]
async fn hand_size_discard_renders_the_prompted_hand() {
    use game_core::state::{Continuation, HandSizeDiscard};
    // Upkeep hand-size discard: NO active investigator, but a HandSizeDiscard
    // frame names inv 1; the hand must still render (#468).
    let mut state = GameStateBuilder::new()
        .with_investigator({
            let mut inv = test_investigator(1);
            inv.hand = vec![
                game_core::state::CardCode::new("01088"),
                game_core::state::CardCode::new("01089"),
            ];
            inv
        })
        .build(); // note: NO .with_active_investigator(...)
    state
        .continuations
        .push(Continuation::HandSizeDiscard(HandSizeDiscard {
            remaining: vec![InvestigatorId(1)],
        }));

    let _rx = mount(state).await;
    let section = last_section();
    let cards = section.query_selector_all(".hand-card").expect("query");
    assert_eq!(
        cards.length(),
        2,
        "the prompted investigator's 2 hand cards must render during hand-size discard"
    );
}
```
Implementer note: match the file's existing helpers — `mount(state)` feeds `awaiting_commit_input(...)` and ticks; `last_section()` scopes to the latest `.awaiting-input`. If `mount` doesn't already feed a `PickMultiple` outcome for an arbitrary state, use the same call the existing mulligan/commit tests in this file use to present the `PickMultiple` branch (search the file for `awaiting_commit_input`).

- [ ] **Step 7: Run the wasm test (red → green)**

Run: `wasm-pack test --headless --firefox crates/web --test input`
Expected: the new test FAILS before Step 5's `active_hand` change is in (0 `.hand-card`), PASSES after. (Steps 5 and 6 land together in this task; run once — it should pass. To see it fail first, temporarily revert Step 5.)

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/web/src/input.rs crates/web/tests/input.rs
git commit -m "engine/ui: render the prompted hand during hand-size discard (#468)

active_hand fell back to active_investigator -> current_mulligan(), but the
upkeep hand-size discard has neither set, so no cards rendered. Add a
current_hand_size_discard() accessor (mirror of current_mulligan) and extend
active_hand's fallback chain.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: #469 — player-facing prompts + neutral button

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (mulligan prompt)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (commit prompt)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (hand-size discard prompt + a prompt-content test)
- Modify: `crates/web/src/input.rs` (button label)
- Test: `crates/web/tests/input.rs` (button text)

- [ ] **Step 1: Write the failing prompt-content test (hand-size discard)**

In `crates/game-core/src/engine/dispatch/phases.rs`'s test module (the one with `Cx { state, events }` setups — search for `park_hand_size_discard` or an existing `Cx {` test), add:
```rust
#[test]
fn hand_size_discard_prompt_is_player_facing() {
    use crate::state::{GameStateBuilder, InvestigatorId};
    let mut state = GameStateBuilder::new()
        .with_investigator(crate::test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let outcome = super::park_hand_size_discard(
        &mut crate::engine::Cx { state: &mut state, events: &mut events },
        vec![InvestigatorId(1)],
    );
    let crate::engine::EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("park_hand_size_discard suspends");
    };
    for forbidden in ["InputResponse", "option ids", "InvestigatorId("] {
        assert!(
            !request.prompt.contains(forbidden),
            "prompt must be player-facing, found {forbidden:?} in: {}",
            request.prompt
        );
    }
    assert!(request.prompt.contains("discard down to 8"));
}
```
(If `park_hand_size_discard` / `Cx` / `EngineOutcome` are reachable by a shorter path in that module's tests, use it; the test must call the real prompt builder.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core --lib hand_size_discard_prompt_is_player_facing`
Expected: FAIL — the current prompt contains `InputResponse`, `option ids`, and `InvestigatorId(`.

- [ ] **Step 3: Rewrite the three prompts**

`crates/game-core/src/engine/dispatch/cards.rs` (mulligan) — replace:
```rust
        request: InputRequest::pick_multiple(format!(
            "Setup mulligan: {next:?} may mulligan; submit InputResponse::PickMultiple with the \
             hand indices (as option ids) to redraw (an empty selection keeps the hand).",
        )),
```
with:
```rust
        request: InputRequest::pick_multiple(
            "Mulligan: choose cards to redraw (an empty selection keeps your hand).",
        ),
```
(The `{next:?}` is gone, so the `format!` becomes a plain `&str` — drop `format!(...)`. Verify `next` isn't used elsewhere in the function; if it becomes unused, remove its `let next = …` binding or prefix with `_`.)

`crates/game-core/src/engine/dispatch/skill_test.rs` (commit) — replace:
```rust
        request: InputRequest::pick_multiple(format!(
            "Commit cards from hand for {investigator:?}'s {skill:?} skill test \
             (difficulty {difficulty}); submit InputResponse::PickMultiple with the \
             hand indices as option ids. Empty selection commits no cards.",
        )),
```
with:
```rust
        request: InputRequest::pick_multiple(format!(
            "Commit cards to the {skill:?} test (difficulty {difficulty}). \
             An empty selection commits no cards.",
        )),
```
(Drop `{investigator:?}`. `{skill:?}` → `Intellect`/`Combat`/… is player-readable; `difficulty` is a number. If `investigator` becomes unused in the function, prefix with `_`.)

`crates/game-core/src/engine/dispatch/phases.rs` (hand-size discard) — replace:
```rust
        request: InputRequest::pick_multiple(format!(
            "Upkeep step 4.5: {next:?} has more than {HAND_SIZE_LIMIT} cards in hand; \
             submit InputResponse::PickMultiple with the hand indices (as option ids) to \
             discard down to {HAND_SIZE_LIMIT}.",
        )),
```
with:
```rust
        request: InputRequest::pick_multiple(format!(
            "You have more than {HAND_SIZE_LIMIT} cards in hand — choose cards to discard \
             down to {HAND_SIZE_LIMIT}.",
        )),
```
(Drop `{next:?}` / `Upkeep step 4.5`. If `next` becomes unused, prefix with `_`.)

- [ ] **Step 4: Run the prompt-content test**

Run: `cargo test -p game-core --lib hand_size_discard_prompt_is_player_facing`
Expected: PASS.
Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean (catches any now-unused `next`/`investigator` binding — fix per Step 3's notes).

- [ ] **Step 5: Change the button label (web)**

In `crates/web/src/input.rs`, in the `InputKind::PickMultiple` arm, change:
```rust
                            <button class="commit" on:click=on_commit>"Commit"</button>
```
to:
```rust
                            <button class="commit" on:click=on_commit>"Confirm"</button>
```
(Keep the `commit` CSS class — only the visible text changes.)

- [ ] **Step 6: Write/adjust the wasm button-text test**

In `crates/web/tests/input.rs`, add (or adjust an existing button assertion):
```rust
#[wasm_bindgen_test]
async fn pick_multiple_button_reads_confirm() {
    let _rx = mount(two_card_game()).await;
    let section = last_section();
    let btn = section
        .query_selector(".commit")
        .expect("query")
        .expect(".commit button present");
    assert_eq!(btn.text_content().unwrap_or_default().trim(), "Confirm");
}
```
If an existing test already asserts the button text is `"Commit"`, update that assertion to `"Confirm"` instead of adding a duplicate (search the file for `"Commit"`).

- [ ] **Step 7: Run the wasm tests**

Run: `wasm-pack test --headless --firefox crates/web --test input`
Expected: PASS (button reads "Confirm"; the #468 hand-render test still green).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/dispatch/cards.rs crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/src/engine/dispatch/phases.rs crates/web/src/input.rs crates/web/tests/input.rs
git commit -m "engine/ui: player-facing PickMultiple prompts + neutral Confirm button (#469)

Rewrite the mulligan / skill-commit / hand-size-discard prompts to player copy
(drop the InputResponse/option-ids wire text and the InvestigatorId Debug), and
change the PickMultiple button from 'Commit' to a neutral 'Confirm' (the prompt
now carries the per-context meaning).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Gauntlet, push, PR

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
Fix `cargo fmt` diffs by running `cargo fmt` and folding into the relevant commit.

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin ui/pickmultiple-prompt-ux
gh pr create --fill
```
PR body: note #468 (functional — hand renders) and #469 (cosmetic — player copy + Confirm), and the YAGNI scope (neutral button, no per-context verbs). Ensure the body has `Closes #468.` and `Closes #469.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`. Fix failures with follow-up commits (no force-push).

- [ ] **Step 4: Phase doc**

No phase-7 doc change required: #468/#469 are `p1-next`/`p2-later` polish, not tracked as a gate-work line item. Skip unless a one-liner feels worth it.

- [ ] **Step 5: Merge only after explicit user approval**

Do not merge autonomously. On approval:
```bash
gh pr merge <PR#> --squash --delete-branch
```
Confirm #468 and #469 auto-closed; `git pull` on `main`.

## Self-Review

**Spec coverage:**
- `current_hand_size_discard()` accessor → Task 1, Steps 1–4. ✓
- `active_hand` fallback extension → Task 1, Step 5. ✓
- 3 prompt rewrites → Task 2, Step 3. ✓
- Button "Commit" → "Confirm" → Task 2, Step 5. ✓
- Tests: accessor unit, #468 wasm hand-render, #469 prompt-content + wasm button → Tasks 1–2. ✓
- Solo-scope / YAGNI (no investigator naming, no confirm_label) → Global Constraints; nothing adds them. ✓
- Closes #468 + #469; no phase doc → Task 3. ✓

**Placeholder scan:** Code steps carry full before/after. The two "match the file's existing helper" notes (Task 1 Step 6 `mount`; Task 2 Step 1 `Cx`/path) point at concrete neighbours to copy rather than inventable code, because the exact local harness import paths must match the file — every literal (prompt text, button text, assertions) is concrete. No "TBD"/"handle errors".

**Type consistency:** `current_hand_size_discard() -> Option<InvestigatorId>`, `Continuation::HandSizeDiscard(HandSizeDiscard { remaining })`, `active_hand`, `park_hand_size_discard`, button class `commit` / text `"Confirm"` — used consistently across tasks.
