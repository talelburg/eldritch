# InputKind Discriminator (#205) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `InputRequest` an explicit `kind: InputKind` discriminator (plus an orthogonal `skippable: bool`) so the web client renders the correct control per prompt instead of guessing from `options.is_empty()` — unblocking the Mythos encounter draw (the phase-7 gate) and the reaction-window Skip affordance.

**Architecture:** Add an `InputKind` enum and two required fields to the single `InputRequest` struct (`crates/game-core/src/engine/outcome.rs`); replace the ambiguous `prompt()`/`choice()` constructors with typed `pick_single`/`pick_multiple`/`confirm` + a chainable `.skippable()`; migrate every engine construction site to the typed constructor that declares its kind; then switch `AwaitingInputView` on `kind` and render a Confirm button and a Skip button.

**Tech Stack:** Rust (workspace), `serde`, Leptos (`crates/web`, wasm32), `wasm-bindgen-test` (headless Firefox).

## Global Constraints

- **Required-on-the-wire, no `serde(default)`.** The two new fields are required; a stale serialized `InputRequest` must error loudly, not silently degrade (matches #453). No new SQL migration — `seed_outcome` is opaque JSON; recreate the local SQLite file if a stale game row exists.
- **CI gauntlet before push** (all seven jobs, warnings-as-errors). This touches `crates/web`, so `wasm-build`, `wasm-test`, and `wasm-clippy` all matter:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `wasm-pack test --headless --firefox crates/web`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Validate-first / mutate-second** is unaffected — this is a metadata-on-prompt change, no dispatch-handler control-flow changes.
- **Branch:** `engine/input-kind-discriminator` (already created; spec committed). One branch, follow-up commits, no force-push.
- Spec of record: `docs/superpowers/specs/2026-06-26-input-kind-discriminator-design.md`.

---

### Task 1: `InputKind` type, fields, constructors, and engine-wide migration

This is one atomic compile unit: adding required fields to `InputRequest` forces every construction site to declare its kind, and removing the ambiguous `prompt`/`choice` constructors forces the rename through. The web client still compiles unchanged after this task (it branches on `options.is_empty()`, which the field addition does not break); it is migrated in Task 2.

**Files:**
- Modify: `crates/game-core/src/engine/outcome.rs` (add `InputKind`, two fields, constructors; update the in-file tests)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs:589` (→ `confirm`)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:594` (→ `pick_multiple`); `:128` (→ `pick_single`)
- Modify: `crates/game-core/src/engine/dispatch/cards.rs:293` (→ `pick_multiple`)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs:1118` (→ `pick_multiple`)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs:113` (→ `pick_single`)
- Modify: `crates/game-core/src/engine/dispatch/hunters.rs:421` (→ `pick_single`)
- Modify: `crates/game-core/src/engine/dispatch/choice.rs:49` (→ `pick_single`)
- Modify: `crates/game-core/src/engine/dispatch/combat.rs:897,1139` (→ `pick_single`)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs:458` (→ `pick_single`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:587,887` (→ `pick_single` + `.skippable()` gated on `!is_forced`)
- Modify: `crates/game-core/src/test_support/fixtures.rs:131` (`awaiting_commit_input` → `pick_multiple`), `:147` (`awaiting_pick_single_input` → `pick_single`)
- Modify: `crates/game-core/src/test_support/resolver.rs:704` (`req` helper → `confirm`)

**Interfaces:**
- Produces (consumed by Task 2):
  - `pub enum InputKind { PickSingle, PickMultiple, Confirm }` (in `game_core::engine`, re-exported as `game_core::InputKind`)
  - `InputRequest { pub prompt: String, pub options: Vec<ChoiceOption>, pub kind: InputKind, pub skippable: bool }`
  - `InputRequest::pick_single(text: impl Into<String>, options: Vec<ChoiceOption>) -> Self`
  - `InputRequest::pick_multiple(text: impl Into<String>) -> Self`
  - `InputRequest::confirm(text: impl Into<String>) -> Self`
  - `InputRequest::skippable(self) -> Self`

- [ ] **Step 1: Write the failing constructor + serde tests**

In `crates/game-core/src/engine/outcome.rs`, replace the existing `#[cfg(test)] mod tests` block's two tests with these (the old `choice_input_request_round_trips` / `prompt_only_request_has_no_options` referenced the now-removed constructors):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_single_sets_kind_and_not_skippable() {
        let req = InputRequest::pick_single(
            "Choose one",
            vec![ChoiceOption { id: OptionId(0), label: "A".into() }],
        );
        assert_eq!(req.kind, InputKind::PickSingle);
        assert!(!req.skippable);
        assert_eq!(req.options.len(), 1);
    }

    #[test]
    fn pick_multiple_sets_kind_and_empty_options() {
        let req = InputRequest::pick_multiple("Commit cards");
        assert_eq!(req.kind, InputKind::PickMultiple);
        assert!(!req.skippable);
        assert!(req.options.is_empty());
    }

    #[test]
    fn confirm_sets_kind_and_empty_options() {
        let req = InputRequest::confirm("Draw");
        assert_eq!(req.kind, InputKind::Confirm);
        assert!(!req.skippable);
        assert!(req.options.is_empty());
    }

    #[test]
    fn skippable_flips_only_the_flag() {
        let base = InputRequest::pick_single("w", vec![]);
        let skip = InputRequest::pick_single("w", vec![]).skippable();
        assert!(!base.skippable);
        assert!(skip.skippable);
        assert_eq!(skip.kind, InputKind::PickSingle);
    }

    #[test]
    fn input_request_round_trips_with_kind_and_skippable() {
        let req = InputRequest::pick_single(
            "Choose one",
            vec![
                ChoiceOption { id: OptionId(0), label: "Take 2 horror".into() },
                ChoiceOption { id: OptionId(1), label: "Each discards 1".into() },
            ],
        )
        .skippable();
        let json = serde_json::to_string(&req).expect("serialize");
        let back: InputRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, req);
        assert_eq!(back.kind, InputKind::PickSingle);
        assert!(back.skippable);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core --lib engine::outcome::tests`
Expected: FAIL to compile — `InputKind` and the new constructors do not exist yet.

- [ ] **Step 3: Add `InputKind`, the fields, and the constructors; remove `prompt`/`choice`**

In `crates/game-core/src/engine/outcome.rs`, add the enum above the `InputRequest` struct:

```rust
/// Which [`InputResponse`](crate::action::InputResponse) variant the host must
/// echo back for a prompt. The variant names mirror `InputResponse` 1:1, so the
/// `kind` *is* the expected response — the host renders the matching control
/// without inspecting the prompt text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InputKind {
    /// Pick exactly one offered [`option`](InputRequest::options) →
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    PickSingle,
    /// Pick a subset (possibly empty) →
    /// [`InputResponse::PickMultiple`](crate::action::InputResponse::PickMultiple).
    PickMultiple,
    /// A binary acknowledge with no choice →
    /// [`InputResponse::Confirm`](crate::action::InputResponse::Confirm).
    Confirm,
}
```

Replace the `InputRequest` struct definition's fields and its `impl` block (the doc comment above the struct can stay; tighten its last sentence about "Remaining prompt-only callers" since `prompt`/`choice` are gone):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InputRequest {
    /// Human-readable text describing what the player must choose.
    pub prompt: String,
    /// Offered options for a [`PickSingle`](InputKind::PickSingle) prompt.
    /// Empty for [`PickMultiple`](InputKind::PickMultiple) (host derives
    /// hand-card candidates) and [`Confirm`](InputKind::Confirm).
    pub options: Vec<ChoiceOption>,
    /// Which response variant the host must send back.
    pub kind: InputKind,
    /// When true the host also offers a Skip/Pass control →
    /// [`InputResponse::Skip`](crate::action::InputResponse::Skip). Orthogonal
    /// to `kind` (e.g. a `PickSingle` reaction window that may also be passed).
    pub skippable: bool,
}

impl InputRequest {
    /// A single-selection choice over `options` →
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    #[must_use]
    pub fn pick_single(text: impl Into<String>, options: Vec<ChoiceOption>) -> Self {
        Self { prompt: text.into(), options, kind: InputKind::PickSingle, skippable: false }
    }

    /// A subset-selection prompt →
    /// [`InputResponse::PickMultiple`](crate::action::InputResponse::PickMultiple).
    ///
    /// `options` is left empty: every current consumer (skill-test commit,
    /// setup mulligan, hand-size discard) picks a subset of the *prompted
    /// investigator's hand*, and the host derives candidates from the hand,
    /// treating each `OptionId(i)` as hand index `i`. This hand-index
    /// convention only holds while `PickMultiple` decisions are hand-scoped; a
    /// future subset-pick over non-hand candidates (e.g. revealed cards,
    /// enemies) would need to carry them in `options` and render from there,
    /// like [`pick_single`](Self::pick_single).
    #[must_use]
    pub fn pick_multiple(text: impl Into<String>) -> Self {
        Self { prompt: text.into(), options: Vec::new(), kind: InputKind::PickMultiple, skippable: false }
    }

    /// A binary acknowledge prompt →
    /// [`InputResponse::Confirm`](crate::action::InputResponse::Confirm).
    #[must_use]
    pub fn confirm(text: impl Into<String>) -> Self {
        Self { prompt: text.into(), options: Vec::new(), kind: InputKind::Confirm, skippable: false }
    }

    /// Mark this prompt skippable (host renders a Skip/Pass control →
    /// [`InputResponse::Skip`](crate::action::InputResponse::Skip)).
    #[must_use]
    pub fn skippable(mut self) -> Self {
        self.skippable = true;
        self
    }
}
```

- [ ] **Step 4: Migrate the engine construction sites**

Apply these edits (constructor swap only; surrounding `format!`/args unchanged unless noted):

`encounter.rs:589` — `InputRequest::prompt(format!(` → `InputRequest::confirm(format!(`

`skill_test.rs:594` — `InputRequest::prompt(format!(` → `InputRequest::pick_multiple(format!(`
`skill_test.rs:128` — `InputRequest::choice(` → `InputRequest::pick_single(`

`cards.rs:293` — `InputRequest::prompt(format!(` → `InputRequest::pick_multiple(format!(`

`phases.rs:1118` — `InputRequest::prompt(format!(` → `InputRequest::pick_multiple(format!(`

`mod.rs:113` — `InputRequest::choice("Choose an action", options)` → `InputRequest::pick_single("Choose an action", options)`

`hunters.rs:421`, `choice.rs:49`, `combat.rs:897`, `combat.rs:1139`, `encounter.rs:458` — `InputRequest::choice(` → `InputRequest::pick_single(`

`reaction_windows.rs:587` and `:887` — swap `choice` → `pick_single` and append `.skippable()` gated on the *existing* `window.is_forced()` decision the `skip_hint` already computes. At `:586`–`:594`, replace:

```rust
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(
            format!(
                "Resolution window: {} option(s). \
                 Submit InputResponse::PickSingle(OptionId) to resolve one{skip_hint}.",
                options.len(),
            ),
            options,
        ),
        resume_token: ResumeToken(0),
    }
```

with (note `let mut request` + conditional `.skippable()`, reusing `window.is_forced()` so the button and the prompt's `skip_hint` cannot drift):

```rust
    let mut request = InputRequest::pick_single(
        format!(
            "Resolution window: {} option(s). \
             Submit InputResponse::PickSingle(OptionId) to resolve one{skip_hint}.",
            options.len(),
        ),
        options,
    );
    if !window.is_forced() {
        request = request.skippable();
    }
    EngineOutcome::AwaitingInput {
        request,
        resume_token: ResumeToken(0),
    }
```

Apply the identical transform at `:887` (its `skip_hint` text differs — `" (forced — cannot skip)"` — but the `window.is_forced()` gate is the same; keep that site's existing `format!` text).

Then the test-support sites:
- `fixtures.rs:131` — `InputRequest::prompt(prompt)` → `InputRequest::pick_multiple(prompt)`
- `fixtures.rs:147` — `InputRequest::choice(` → `InputRequest::pick_single(`
- `resolver.rs:704` — `InputRequest::prompt(prompt)` → `InputRequest::confirm(prompt)` (the resolver returns scripted responses regardless of `kind`; `confirm` is an arbitrary neutral choice)

- [ ] **Step 5: Run the full game-core suite and strict gates**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --all-features`
Expected: PASS (all existing dispatch/encounter/reaction tests green; the new constructor tests green).

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 6: Add an engine-level kind assertion to the existing Mythos-draw test**

In `crates/game-core/src/engine/dispatch/encounter.rs`, find the test that prompts the encounter draw (around `:1904`, where `resume_encounter_draw` is exercised). Locate where it pattern-matches the `EngineOutcome::AwaitingInput { request, .. }` for the draw prompt and add, alongside the existing assertions:

```rust
assert_eq!(request.kind, crate::engine::InputKind::Confirm);
assert!(!request.skippable);
```

If the existing test consumes the outcome without binding `request`, bind it (`EngineOutcome::AwaitingInput { request, .. }`) to assert. Run: `cargo test -p game-core --lib dispatch::encounter`
Expected: PASS.

- [ ] **Step 7: Add a reaction-window skippable assertion**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs` `#[cfg(test)]` tests, find (or add) a test that opens a non-forced resolution window via `open_queued_reaction_window` and assert the emitted request:

```rust
// Non-forced reaction window must offer Skip.
assert_eq!(request.kind, crate::engine::InputKind::PickSingle);
assert!(request.skippable);
```

If an existing test already drives a non-forced window to an `AwaitingInput`, extend it; otherwise add a focused test mirroring the nearest existing window-opening test in that module's test block. Run: `cargo test -p game-core --lib dispatch::reaction_windows`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/outcome.rs \
        crates/game-core/src/engine/dispatch/ \
        crates/game-core/src/test_support/fixtures.rs \
        crates/game-core/src/test_support/resolver.rs
git commit -m "engine: InputKind discriminator on InputRequest; migrate prompt sites

Replace the ambiguous prompt()/choice() constructors with typed
pick_single/pick_multiple/confirm + an orthogonal .skippable(), so a prompt
declares which InputResponse it expects instead of the host guessing from
options-empty. Migrate every engine construction site. The Mythos encounter
draw now emits kind: Confirm; non-forced reaction windows emit skippable: true.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Web client renders by `kind`, plus Confirm and Skip controls

**Files:**
- Modify: `crates/game-core/src/test_support/fixtures.rs` (add `awaiting_confirm_input`, `awaiting_skippable_pick_single_input` — needed because `ResumeToken`'s inner is `pub(crate)`, so wasm tests outside `game-core` cannot build `AwaitingInput` outcomes directly)
- Modify: `crates/web/src/input.rs` (switch `AwaitingInputView` on `kind`; add Confirm + Skip controls; rewrite the module/`fn` doc comments)
- Modify: `crates/web/tests/awaiting_input.rs` (add Confirm + Skip tests)

**Interfaces:**
- Consumes (from Task 1): `InputKind::{PickSingle, PickMultiple, Confirm}`, `InputRequest::{confirm, pick_single}`, `InputRequest::skippable`.
- Produces: `AwaitingInputView` renders a `.confirm` button for `kind: Confirm` and a `.skip` button when `skippable`; fixtures `awaiting_confirm_input(prompt)` and `awaiting_skippable_pick_single_input(prompt)`.

- [ ] **Step 1: Add the two fixtures**

In `crates/game-core/src/test_support/fixtures.rs`, after `awaiting_pick_single_input`, add:

```rust
/// A sample [`Confirm`](crate::InputKind::Confirm)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome, for client/UI
/// fixtures. Models the Mythos encounter-draw prompt.
#[must_use]
pub fn awaiting_confirm_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::confirm(prompt),
        resume_token: ResumeToken(0),
    }
}

/// A sample skippable [`PickSingle`](crate::InputKind::PickSingle)
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) outcome, for client/UI
/// fixtures. Models a non-forced reaction window: one option plus a Skip
/// affordance.
#[must_use]
pub fn awaiting_skippable_pick_single_input(prompt: impl Into<String>) -> EngineOutcome {
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            prompt,
            vec![ChoiceOption { id: OptionId(0), label: "Resolve".into() }],
        )
        .skippable(),
        resume_token: ResumeToken(0),
    }
}
```

Run: `cargo test -p game-core --lib test_support::fixtures` (or `cargo build -p game-core`)
Expected: compiles.

- [ ] **Step 2: Write the failing wasm tests**

In `crates/web/tests/awaiting_input.rs`, extend the imports on the `use game_core::test_support::fixtures::{...}` line to add `awaiting_confirm_input` and `awaiting_skippable_pick_single_input`. Add a helper next to `pick_single_id` to recognize `Confirm`/`Skip` frames:

```rust
/// True if the frame is `ResolveInput(Confirm)`.
fn is_confirm(frame: &ClientMessage) -> bool {
    matches!(
        frame,
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response: InputResponse::Confirm },
        }
    )
}

/// True if the frame is `ResolveInput(Skip)`.
fn is_skip(frame: &ClientMessage) -> bool {
    matches!(
        frame,
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response: InputResponse::Skip },
        }
    )
}
```

Then add these tests at the end of the file:

```rust
#[wasm_bindgen_test]
async fn confirm_renders_confirm_button_and_submits_confirm() {
    let mut rx = mount(base_game(), awaiting_confirm_input("Draw an encounter card")).await;
    let section = last_section();

    let confirm = section.query_selector(".confirm").expect("query");
    assert!(confirm.is_some(), "Confirm prompt must render a .confirm button");
    // No hand-commit UI for a Confirm prompt.
    assert!(section.query_selector(".commit-hand").expect("query").is_none());

    click_in(&section, ".confirm", 0);
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after clicking Confirm");
    assert!(is_confirm(&frame), "expected ResolveInput(Confirm), got {frame:?}");
}

#[wasm_bindgen_test]
async fn skippable_window_renders_skip_button_and_submits_skip() {
    let mut rx = mount(
        base_game(),
        awaiting_skippable_pick_single_input("Reaction window"),
    )
    .await;
    let section = last_section();

    assert!(
        section.query_selector(".skip").expect("query").is_some(),
        "skippable prompt must render a .skip button"
    );
    // The option list is still present for a PickSingle window.
    assert_eq!(
        section.query_selector_all(".option").expect("query").length(),
        1
    );

    click_in(&section, ".skip", 0);
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after clicking Skip");
    assert!(is_skip(&frame), "expected ResolveInput(Skip), got {frame:?}");
}

#[wasm_bindgen_test]
async fn non_skippable_pick_single_has_no_skip_button() {
    let _rx = mount(base_game(), awaiting_pick_single_input("Choose an action")).await;
    let section = last_section();
    assert!(
        section.query_selector(".skip").expect("query").is_none(),
        "a non-skippable prompt must not render a .skip button"
    );
}
```

- [ ] **Step 3: Run the wasm tests to verify they fail**

Run: `wasm-pack test --headless --firefox crates/web --test awaiting_input`
Expected: FAIL — no `.confirm`/`.skip` elements render (the client still branches on `options.is_empty()` and has no Confirm/Skip control).

- [ ] **Step 4: Rewrite `AwaitingInputView` to switch on `kind`**

In `crates/web/src/input.rs`, update the import to bring in `InputKind`:

```rust
use game_core::{ChoiceOption, EngineOutcome, InputKind, InputResponse, OptionId, PlayerAction};
```

Replace the two-branch body (the `if !request.options.is_empty() { ... }` PickSingle branch and the trailing PickMultiple branch) with a `match request.kind` that also renders a Skip button when `request.skippable`. Build a reusable Skip button first, then the per-kind body:

```rust
let skippable = request.skippable;
let skip_button = {
    let tx = tx.clone();
    move || {
        if !skippable {
            return ().into_any();
        }
        let tx = tx.clone();
        view! {
            <button
                class="skip"
                on:click=move |_| {
                    if let Some(tx) = tx.clone() {
                        let _ = tx.unbounded_send(ClientMessage::Submit {
                            action: PlayerAction::ResolveInput {
                                response: InputResponse::Skip,
                            },
                        });
                    }
                }
            >
                "Skip"
            </button>
        }
        .into_any()
    }
};

match request.kind {
    InputKind::PickSingle => {
        let tx = tx.clone();
        let buttons: Vec<_> = request
            .options
            .iter()
            .cloned()
            .map(|opt: ChoiceOption| {
                let ChoiceOption { id, label } = opt;
                let tx = tx.clone();
                view! {
                    <button
                        class="option"
                        on:click=move |_| {
                            if let Some(tx) = tx.clone() {
                                let _ = tx.unbounded_send(ClientMessage::Submit {
                                    action: PlayerAction::ResolveInput {
                                        response: InputResponse::PickSingle(id),
                                    },
                                });
                            }
                        }
                    >
                        {label}
                    </button>
                }
            })
            .collect();
        view! {
            <section class="awaiting-input">
                <p class="prompt">{request.prompt.clone()}</p>
                <div class="option-list">{buttons}</div>
                {skip_button()}
            </section>
        }
        .into_any()
    }
    InputKind::Confirm => {
        let tx = tx.clone();
        view! {
            <section class="awaiting-input">
                <p class="prompt">{request.prompt.clone()}</p>
                <button
                    class="confirm"
                    on:click=move |_| {
                        if let Some(tx) = tx.clone() {
                            let _ = tx.unbounded_send(ClientMessage::Submit {
                                action: PlayerAction::ResolveInput {
                                    response: InputResponse::Confirm,
                                },
                            });
                        }
                    }
                >
                    "Confirm"
                </button>
                {skip_button()}
            </section>
        }
        .into_any()
    }
    InputKind::PickMultiple => {
        let cards: Vec<_> = active_hand(&game)
            .into_iter()
            .enumerate()
            .map(|(idx, code)| {
                let i = u32::try_from(idx).expect("hand fits in u32");
                view! {
                    <li>
                        <button
                            class="hand-card"
                            class:selected=move || selected.get().contains(&i)
                            on:click=move |_| selected.update(|s| {
                                if !s.remove(&i) {
                                    s.insert(i);
                                }
                            })
                        >
                            {code}
                        </button>
                    </li>
                }
            })
            .collect();

        let tx = tx.clone();
        let on_commit = move |_| {
            let selected_ids: Vec<OptionId> =
                selected.get().into_iter().map(OptionId).collect();
            if let Some(tx) = tx.clone() {
                let _ = tx.unbounded_send(ClientMessage::Submit {
                    action: PlayerAction::ResolveInput {
                        response: InputResponse::PickMultiple { selected: selected_ids },
                    },
                });
            }
            selected.set(BTreeSet::new());
        };

        view! {
            <section class="awaiting-input">
                <p class="prompt">{request.prompt}</p>
                <ul class="commit-hand">{cards}</ul>
                <button class="commit" on:click=on_commit>"Commit"</button>
                {skip_button()}
            </section>
        }
        .into_any()
    }
}
```

Notes for the implementer:
- The leptos closure/clone ergonomics above mirror the file's existing style (each `on:click` gets its own `tx` clone). If the borrow checker complains about `tx`/`skip_button` move order, clone `tx` once more per arm — match the existing pattern, do not restructure the component.
- `#[non_exhaustive]` on `InputKind` means the `match` needs no catch-all *within this crate* for the three known variants, but a `_ => ().into_any()` arm may be required to satisfy the non-exhaustive lint across the crate boundary. Add it only if the compiler demands it, with a comment that it is the non-exhaustive guard.
- Rewrite the module-level doc comment (lines 1–13) and the `AwaitingInputView` doc (lines 25–36): replace the "two rendering branches / options-empty heuristic" description with the three-way `kind` switch + the orthogonal `skippable` Skip affordance.

- [ ] **Step 5: Run the wasm tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web --test awaiting_input`
Expected: PASS (Confirm, Skip, non-skippable, and the existing PickSingle tests all green).

Also confirm the PickMultiple branch still works:
Run: `wasm-pack test --headless --firefox crates/web --test input`
Expected: PASS.

- [ ] **Step 6: Run the wasm clippy + build gates**

Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
Run: `cargo build -p web --target wasm32-unknown-unknown`
Expected: both clean.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/test_support/fixtures.rs crates/web/src/input.rs crates/web/tests/awaiting_input.rs
git commit -m "web: render AwaitingInput by InputKind; add Confirm + Skip controls

Switch AwaitingInputView on request.kind instead of options-empty: Confirm
prompts render a Confirm button (unblocking the Mythos encounter draw), and a
Skip button renders whenever request.skippable (the reaction/fast-window decline
path). PickSingle/PickMultiple branches unchanged in behavior.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Full CI gauntlet, push, PR, then phase-7 doc

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md` (the "End-to-end browser playthrough" bullet — stall resolved) — **final commit, only after CI is green on the opened PR**.

- [ ] **Step 1: Run the complete local gauntlet**

Run each, expecting all green:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
If `cargo fmt --check` flags anything, run `cargo fmt` and fold into the relevant commit.

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin engine/input-kind-discriminator
gh pr create --fill
```
PR body: include a short design-decisions paragraph (typed constructors replace the ambiguous `prompt`/`choice`; `skippable` orthogonal to `kind`; required fields / no `serde(default)` / no SQL migration per #453; quote the Mythos-draw rejection that motivated it). Ensure the body has `Closes #205.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`
Fix any failures with follow-up commits to the same branch (no force-push).

- [ ] **Step 4: Update the phase-7 doc as the final commit (only once CI is green)**

In `docs/phases/phase-7-the-gathering.md`, update the "**End-to-end browser playthrough**" bullet (the one describing the stall at the Mythos encounter draw): note the stall is resolved by #205's `InputKind` discriminator (`kind: Confirm` renders a Confirm button; `skippable` renders Skip), and flip its status from "blocked on #205" to done/the gate's final demonstration. Add a one-line **Decisions made** entry only if it passes the test in `docs/phases/README.md` ("would a future PR-author choose differently without this entry?") — e.g. the `kind`-mirrors-`InputResponse` + orthogonal-`skippable` shape. Remove #205 from remaining gate work.

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — #205 closes the browser-playthrough stall"
git push
```

- [ ] **Step 5: Merge only after explicit user approval**

Do not merge autonomously. Surface CI-green + the review, and wait. On approval:
```bash
gh pr merge <PR#> --squash --delete-branch
```
Then confirm #205 auto-closed and `git pull` on `main`.

## Self-Review

**Spec coverage:**
- `InputKind` + two fields + constructors → Task 1, Steps 1–3. ✓
- `pick_multiple` hand-index caveat (plain note, no TODO) → Task 1, Step 3 (doc on the constructor). ✓
- Engine call-site migration table (all 11 sites + fixtures + resolver) → Task 1, Step 4. ✓
- Reaction-window `.skippable()` gated on `!is_forced` → Task 1, Step 4 + assertion Step 7. ✓
- Mythos-draw `kind: Confirm` assertion → Task 1, Step 6. ✓
- Client three-way `kind` switch + Confirm + Skip → Task 2, Step 4. ✓
- Backcompat (required, no `serde(default)`, no migration, recreate DB) → Global Constraints. ✓
- Tests: outcome serde/constructor units, engine assertions, wasm Confirm/Skip → Tasks 1–2. ✓
- Phase-7 doc update as final commit → Task 3, Step 4. ✓

**Placeholder scan:** No "TBD"/"handle edge cases"/"similar to" — every code step carries full code. The two "add the assertion to the existing test" steps (1.6, 1.7) name the file, approximate line, the exact assertion code, and a fallback if the binding is absent. ✓

**Type consistency:** `InputKind::{PickSingle, PickMultiple, Confirm}`, `pick_single`/`pick_multiple`/`confirm`/`skippable`, `is_confirm`/`is_skip` used consistently across tasks. The web `match` consumes exactly the `InputKind` produced in Task 1. ✓
