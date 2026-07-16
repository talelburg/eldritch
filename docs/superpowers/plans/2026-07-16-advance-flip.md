# Act/agenda advance-flip — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Model an act/agenda advance as an on-card **flip → resolve**: the card flips to its reverse (showing the reverse's name + effect text), then the reverse is clicked to resolve — replacing the flat-bar advance `Confirm`.

**Architecture:** Three sequential PRs. **Slice 1 (pipeline):** ingest the reverse side (`back_name`/`back_text`) into `CardMetadata`. **Slice 2 (engine):** the `AdvanceReverse` frame carries a `trigger` (Forced/Deliberate); `AwaitAck` becomes an on-card `PickSingle` anchored to the act/agenda for *forced* advances, and is skipped for *chosen* ones. **Slice 3 (web):** render front vs. reverse by the frame's `step`. Slice 4 (01110 `#466` suppression) is deferred.

**Tech Stack:** Rust — `card-dsl` + `card-data-pipeline` + `cards` (slice 1), `game-core` (slice 2), `web` Leptos/wasm (slice 3).

**Design spec:** `docs/superpowers/specs/2026-07-16-advance-flip-design.md`

## Global Constraints

- **Guiding principle (fire-once, on-card):** the interactive-resolve convention fires exactly once per advance, anchored on the card. This plan re-homes the advance ack onto the card; it does **not** refactor `resolve_choice_count` (that's #555 / S6).
- **Display-only anchors.** No resolve path reads the anchor; `advance_reverse::resume` validates only the echoed `OptionId`.
- **Forced vs. chosen, not act vs. agenda.** `advance_agenda` → Forced; `advance_act` from the action / round-end objective → Deliberate; `advance_act` from the evaluator (01110) → Forced.
- **Timing (verified, #557):** the reverse forced fires at `FireReverse`, before `Finalize` bumps the index, so click-2's anchor holds.
- **Never hand-edit `crates/cards/src/generated/cards.rs`** — regenerate via `cargo run -p card-data-pipeline`.
- **CI is 7 warnings-as-errors jobs.** Match locally before pushing:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `wasm-pack test --headless --firefox crates/web`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- Commit subjects `scope: description`. One branch/PR per slice.

## File structure

- `crates/card-dsl/src/card_data.rs` — `CardMetadata` gains `back_name`/`back_text` (slice 1).
- `crates/card-data-pipeline/src/main.rs` — `RawCard`/`NormalizedCard`/normalize/`render_card` map the back side (slice 1).
- `crates/cards/src/generated/cards.rs` — regenerated (slice 1).
- `crates/game-core/src/state/game_state.rs` — `AdvanceTrigger` enum + `AdvanceReverse.trigger` field (slice 2).
- `crates/game-core/src/engine/dispatch/act_agenda.rs` — `advance_act`/`advance_agenda` thread `trigger`; callers set it (slice 2).
- `crates/game-core/src/engine/dispatch/advance_reverse.rs` — `AwaitAck` pick/skip; `resume` accepts `PickSingle` (slice 2).
- `crates/web/src/act_agenda.rs` — front/reverse face by `step` (slice 3).

---

# Slice 1 (PR 1) — pipeline: ingest the reverse side

Branch `engine/advance-flip` (already carries the design + plan docs — PR 1 = docs + slice 1). Delivers `CardMetadata.back_name`/`back_text` for every act/agenda. Not user-visible alone.

### Task 1.1: `CardMetadata` gains the back-side fields

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs` (`CardMetadata` struct + its constructor test literals)

- [ ] **Step 1: Add the fields**

In `CardMetadata`, after `pub text: Option<String>,`:

```rust
    /// Reverse-side display name (double-sided act/agenda cards — the "1b" face,
    /// e.g. "A Lapse in Time"). `None` for single-sided cards. From ArkhamDB
    /// `back_name`.
    pub back_name: Option<String>,
    /// Reverse-side rules text (the on-advance effect printed on the "1b" face).
    /// `None` for single-sided cards. From ArkhamDB `back_text`.
    pub back_text: Option<String>,
```

- [ ] **Step 2: Fix the constructor literals in this file's tests**

Every `CardMetadata { … }` literal in `card_data.rs`'s `#[cfg(test)]` module needs the two new fields (add `back_name: None, back_text: None,` — these tests are non-act/agenda fixtures). Run `cargo test -p card-dsl` and add the fields wherever the compiler flags a missing-field error.

Run: `cargo test -p card-dsl`
Expected: PASS after the literals compile.

- [ ] **Step 3: Commit**

```bash
git add crates/card-dsl/src/card_data.rs
git commit -m "card-dsl: CardMetadata carries the reverse side (back_name/back_text)"
```

### Task 1.2: Pipeline maps the back side

**Files:**
- Modify: `crates/card-data-pipeline/src/main.rs` (`RawCard`, `NormalizedCard`, the normalize return, `render_card`)

- [ ] **Step 1: Add a failing pipeline test**

The pipeline has unit tests (`normalized(code, name, type)` helper). Add a test that a raw card with `back_text` normalizes with it carried. First inspect the existing `RawCard`-deserialize / normalize tests near `fn normalized` (grep `fn normalized`), then add:

```rust
    #[test]
    fn normalize_carries_back_name_and_text() {
        // A raw agenda with a reverse side keeps back_name/back_text through normalize.
        let raw = RawCard {
            code: "01105".into(),
            name: Some("What's Going On?!".into()),
            text: None,
            back_name: Some("A Lapse in Time".into()),
            back_text: Some("The lead investigator must decide…".into()),
            type_code: Some("agenda".into()),
            pack_code: "core".into(),
            ..raw_card_default()   // if no default helper exists, fill the Option<_> fields with None / required with sensible values — mirror an existing RawCard literal in the tests
        };
        let n = normalize(raw).expect("agenda normalizes");
        assert_eq!(n.back_name.as_deref(), Some("A Lapse in Time"));
        assert_eq!(n.back_text.as_deref(), Some("The lead investigator must decide…"));
    }
```

(If there is no `RawCard` literal precedent in the tests — the tests may go through JSON — instead write the test as a JSON string deserialized into `RawCard` then normalized. Match whichever pattern the file already uses; do **not** invent a `raw_card_default` if the file builds `RawCard` from JSON.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p card-data-pipeline normalize_carries_back_name_and_text`
Expected: FAIL — `RawCard`/`NormalizedCard` have no `back_name`/`back_text`.

- [ ] **Step 3: Add the fields + mapping**

In `RawCard` (after `text: Option<String>,`):

```rust
    back_name: Option<String>,
    back_text: Option<String>,
```

In `NormalizedCard` (after `text: Option<String>,`):

```rust
    back_name: Option<String>,
    back_text: Option<String>,
```

In the normalize return (`Ok(NormalizedCard { … })`, after `text: raw.text,`):

```rust
        back_name: raw.back_name,
        back_text: raw.back_text,
```

In `render_card` (after the `text:` emit block):

```rust
    let _ = writeln!(
        out,
        "            back_name: {},",
        opt_owned_str(c.back_name.as_deref())
    );
    let _ = writeln!(
        out,
        "            back_text: {},",
        opt_owned_str(c.back_text.as_deref())
    );
```

If a stub-card fallback constructs `NormalizedCard` (grep `name: Some(format!("Card` / the stub path near line 955), add `back_name: None, back_text: None,` there too.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p card-data-pipeline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/card-data-pipeline/src/main.rs
git commit -m "card-data-pipeline: map ArkhamDB back_name/back_text"
```

### Task 1.3: Regenerate the corpus + verify an act/agenda carries the back side

**Files:**
- Modify (regenerate): `crates/cards/src/generated/cards.rs`
- Add: an assertion via an existing `cards` test path (or a new integration test) that 01105's metadata has `back_text`.

- [ ] **Step 1: Regenerate**

Run: `cargo run -p card-data-pipeline`
Then confirm the generated file changed and still compiles: `cargo build -p cards`.

- [ ] **Step 2: Add a corpus assertion**

In `crates/cards/tests/` (new file `back_side.rs` or an existing metadata test), install the registry and assert the back side is present:

```rust
#[ctor::ctor(unsafe)]
fn install() { let _ = game_core::card_registry::install(cards::REGISTRY); }

#[test]
fn agenda_01105_carries_its_reverse() {
    let reg = game_core::card_registry::current().expect("registry");
    let m = (reg.metadata_for)(&game_core::state::CardCode::new("01105")).expect("01105 metadata");
    assert_eq!(m.back_name.as_deref(), Some("A Lapse in Time"));
    assert!(m.back_text.as_deref().unwrap_or_default().contains("discard"));
}
```

Run: `cargo test -p cards --test back_side`
Expected: PASS.

- [ ] **Step 3: Full gauntlet + PR**

Run the 7-job gauntlet (Global Constraints). Then:

```bash
git add crates/cards/src/generated/cards.rs crates/cards/tests/back_side.rs
git commit -m "cards: regenerate corpus with act/agenda reverse text (#558)"
git push -u origin card/act-agenda-back-text
gh pr create --fill   # body: slice 1 of #558; ingest reverse side, not user-visible until the flip lands
```

PR body notes: first slice of the advance-flip (#558); adds `back_name`/`back_text`; consumed by slice 3. Does **not** `Closes #558` (later slices do the visible work) — reference `#558` without closing.

- [ ] **Step 4: Phase-doc note (final commit, once CI green + ready to merge).** One line under the phase-7 interactivity section: slice 1 of the advance-flip, reverse-side ingestion. Merge only after explicit user approval.

---

# Slice 2 (PR 2) — engine: the on-card advance-flip pick

Branch `engine/advance-flip-ack`. Delivers: forced advances (agenda; act 01110) prompt the flip as an on-card `PickSingle`; chosen advances (act action / round-end objective) skip the ack.

### Task 2.1: `AdvanceTrigger` + `AdvanceReverse.trigger`

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (new enum + frame field)

- [ ] **Step 1: Add the enum + field**

Near `AdvanceDeck`/`AdvanceStep`:

```rust
/// Why an act/agenda is advancing — decides whether the acknowledge prompts.
/// A **forced** advance (agenda doom; act 01110 on Ghoul Priest defeat) prompts
/// the on-card flip; a **deliberate** advance (the `AdvanceAct` action or the
/// round-end objective) was already the player's choice, so the ack is skipped
/// (#558).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvanceTrigger {
    /// Game-forced (doom threshold / a forced ability). Prompts the flip-click.
    Forced,
    /// Player-chosen (spend clues). The advance action was the flip; ack skipped.
    Deliberate,
}
```

In `Continuation::AdvanceReverse`, add after `step: AdvanceStep,`:

```rust
        /// Whether this advance was forced (prompts the flip) or deliberate.
        trigger: AdvanceTrigger,
```

- [ ] **Step 2: Verify it fails to compile (every AdvanceReverse literal now needs `trigger`)**

Run: `cargo build -p game-core`
Expected: errors at each `AdvanceReverse { … }` construction — the two production pushes (`advance_act`/`advance_agenda`) and any test literals. These are fixed in the next tasks / compiler-guided.

### Task 2.2: Thread `trigger` through `advance_act`/`advance_agenda` and callers

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/act_agenda.rs` (`advance_act`/`advance_agenda` signatures + the two frame pushes + the three `advance_act` callers)
- Modify: `crates/game-core/src/engine/evaluator.rs` (the `apply_advance_current_act` caller)

**Interfaces:**
- Changes: `advance_agenda(cx)` → pushes `trigger: Forced`. `advance_act(cx)` → `advance_act(cx, trigger: AdvanceTrigger)`.

- [ ] **Step 1: `advance_agenda` — always Forced**

In `advance_agenda`'s `AdvanceReverse { … }` push, add `trigger: crate::state::AdvanceTrigger::Forced,`.

- [ ] **Step 2: `advance_act` gains a `trigger` param**

```rust
pub(crate) fn advance_act(cx: &mut Cx, trigger: crate::state::AdvanceTrigger) {
```

and its `AdvanceReverse { … }` push gets `trigger,`.

- [ ] **Step 3: Set each caller's trigger**

- `act_agenda.rs:~200` (`advance_act_action`, the `AdvanceAct` action): `None => advance_act(cx, AdvanceTrigger::Deliberate),`
- `act_agenda.rs:~315` (`round_end_advance`, the objective): `advance_act(cx, AdvanceTrigger::Deliberate);`
- `evaluator.rs:~1516` (`apply_advance_current_act`): `None => advance_act(cx, AdvanceTrigger::Forced),` (01110's Ghoul-Priest advance is forced; import `AdvanceTrigger`).

- [ ] **Step 4: Fix test literals + build**

Run: `cargo build -p game-core` and add `trigger: AdvanceTrigger::Forced` (or the case-appropriate value) to any `AdvanceReverse { … }` test literal the compiler flags.

Run: `cargo test -p game-core --lib -- advance` — expected: PASS (behavior unchanged so far; the frame just carries the new field).

- [ ] **Step 5: Commit (Tasks 2.1–2.2 together — the field is inert until 2.3)**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/act_agenda.rs crates/game-core/src/engine/evaluator.rs
git commit -m "engine: AdvanceReverse carries its trigger (forced/deliberate) (#558)"
```

### Task 2.3: `AwaitAck` — on-card pick for forced, skip for deliberate

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/advance_reverse.rs` (`drive`'s `AwaitAck` arm, `resume`, the `AdvanceStep::AwaitAck` doc)

- [ ] **Step 1: Write the failing tests**

`advance_reverse.rs` has `#[cfg(test)]` tests (e.g. `advance_reverse_pauses_for_acknowledge_when_interactive`). Add two:

```rust
    #[test]
    fn forced_advance_prompts_on_card_pick_anchored_to_the_deck() {
        use crate::engine::OptionTarget;
        // interactive + Forced → a one-option PickSingle anchored to Agenda, not a Confirm.
        let mut state = /* build state with an agenda advancing, interactive on */;
        state.continuations.push(Continuation::AdvanceReverse {
            deck: AdvanceDeck::Agenda, from: 0, leaving_code: CardCode::new("01105"),
            step: AdvanceStep::AwaitAck, trigger: AdvanceTrigger::Forced,
        });
        let mut events = Vec::new();
        let out = drive(&mut Cx { state: &mut state, events: &mut events });
        match out {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1);
                assert_eq!(request.options[0].target, OptionTarget::Agenda);
            }
            other => panic!("expected an on-card pick, got {other:?}"),
        }
    }

    #[test]
    fn deliberate_advance_skips_the_ack_and_falls_through() {
        // interactive + Deliberate → no pause; step moves straight to FireReverse.
        let mut state = /* interactive on */;
        state.continuations.push(Continuation::AdvanceReverse {
            deck: AdvanceDeck::Act, from: 0, leaving_code: CardCode::new("01109"),
            step: AdvanceStep::AwaitAck, trigger: AdvanceTrigger::Deliberate,
        });
        let mut events = Vec::new();
        let out = drive(&mut Cx { state: &mut state, events: &mut events });
        assert!(matches!(out, EngineOutcome::Done), "deliberate: no pause, got {out:?}");
        // cursor advanced past AwaitAck
        assert!(matches!(top_step(&state), AdvanceStep::FireReverse));
    }
```

(Fill the `/* build state */` with whatever the file's existing AwaitAck test uses; add a `top_step` read or inspect `state.continuations.last()`. Match existing helpers.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core --lib -- forced_advance_prompts deliberate_advance_skips`
Expected: FAIL — today AwaitAck always emits a `Confirm` when interactive, ignoring `trigger`.

- [ ] **Step 3: Rewrite the `AwaitAck` arm**

Replace the current arm (`InputRequest::confirm(ack_prompt(...))` when interactive) with:

```rust
        AdvanceStep::AwaitAck => {
            cx.events.push(advanced_event(deck, from));
            // Fire-once (#558): a FORCED advance prompts the flip as an on-card pick;
            // a DELIBERATE advance was already the player's choice, so skip the ack.
            let (_, _, _, _) = (deck, from, &leaving_code, step); // fields in scope via top()
            if cx.state.interactive_acknowledge && matches!(trigger, AdvanceTrigger::Forced) {
                let anchor = match deck {
                    AdvanceDeck::Act => OptionTarget::Act,
                    AdvanceDeck::Agenda => OptionTarget::Agenda,
                };
                return EngineOutcome::AwaitingInput {
                    request: InputRequest::pick_single(
                        ack_prompt(deck, from),
                        vec![ChoiceOption::new(OptionId(0), "Advance", anchor)],
                    ),
                    resume_token: ResumeToken(0),
                };
            }
            set_step(cx, AdvanceStep::FireReverse);
            EngineOutcome::Done
        }
```

`top()` must now also return `trigger`; widen its tuple (or read `trigger` in the match). Add the imports (`ChoiceOption`, `OptionId`, `OptionTarget`, `AdvanceTrigger`).

- [ ] **Step 4: Update `resume` to accept the pick**

In `resume`, replace the `Confirm` check with a one-option `PickSingle` check:

```rust
    if !matches!(response, InputResponse::PickSingle(OptionId(0))) {
        return EngineOutcome::Rejected {
            reason: format!("advance acknowledge: expected the single advance option, got {response:?}").into(),
        };
    }
```

(Import `OptionId`. `resume` only runs when a pause happened — i.e. forced — so `PickSingle(0)` is the sole valid response.)

- [ ] **Step 5: Run + fix the existing AwaitAck test**

The existing `advance_reverse_pauses_for_acknowledge_when_interactive` (and its resume) used `Confirm`. Migrate it: its frame needs `trigger: Forced`, and its resume response becomes `PickSingle(OptionId(0))`.

Run: `cargo test -p game-core` — expected: PASS.

- [ ] **Step 6: Update the `AdvanceStep::AwaitAck` doc** in `game_state.rs` (it says "suspend with a `Confirm`") → "for a forced advance, suspend with a one-option on-card pick; a deliberate advance skips it".

- [ ] **Step 7: Clippy + commit + full gauntlet + PR**

```bash
cargo clippy -p game-core --all-targets --all-features -- -D warnings
git add crates/game-core/src/engine/dispatch/advance_reverse.rs crates/game-core/src/state/game_state.rs
git commit -m "engine: advance ack is an on-card pick for forced advances, skipped for deliberate (#558)"
```

Run the 7-job gauntlet; push `engine/advance-flip-ack`; open the PR (body: slice 2 of #558; forced→on-card pick, deliberate→skip; references #558, not closing). Phase-doc note as the final commit once CI green. Merge only after explicit user approval.

**Integration check (manual, in the PR description):** with the server's `interactive_acknowledge` on, an agenda advance now surfaces the flip pick anchored to `Agenda`; a deliberate act advance goes straight to the reverse's forced-ack. (The client renders the flip in slice 3; here the anchor is correct on the wire.)

---

# Slice 3 (PR 3) — web: render the flip

Branch `web/advance-flip-render`. Delivers the visible flip: front while `AwaitAck`, reverse (`back_name`/`back_text`) from `FireReverse`.

### Task 3.1: A face helper + reverse rendering

**Files:**
- Modify: `crates/web/src/act_agenda.rs` (a `deck_face(game, deck)` helper; `ActCard`/`AgendaCard` render the reverse when advancing; headless tests)

**Interfaces:**
- Consumes: `CardMetadata.back_name`/`back_text` (slice 1), `AdvanceReverse.step`/`trigger` (slice 2), `OptionTarget::Act`/`Agenda`.

- [ ] **Step 1: Write the failing headless test**

In `crates/web/tests/act_agenda.rs`, add a test: mount `act_agenda_view` with a state whose `continuations` hold an `AdvanceReverse { deck: Agenda, step: FireReverse, … }` for agenda 01105, and assert the rendered agenda card shows the **reverse** name (`A Lapse in Time`) / `back_text`, not the front. Mirror the existing render test's `section_text()` helper.

```rust
#[wasm_bindgen_test]
async fn agenda_shows_reverse_face_while_advancing() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let mut state = GameStateBuilder::new().build();
    state.agenda_deck = vec![Agenda { code: CardCode::new("01105"), doom_threshold: 3, resolution: None }];
    state.continuations.push(Continuation::AdvanceReverse {
        deck: AdvanceDeck::Agenda, from: 0, leaving_code: CardCode::new("01105"),
        step: AdvanceStep::FireReverse, trigger: AdvanceTrigger::Forced,
    });
    leptos::mount::mount_to_body(move || web::act_agenda::act_agenda_view(&state));
    leptos::task::tick().await;
    let text = section_text();
    assert!(text.contains("A Lapse in Time"), "reverse name shown: {text}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `wasm-pack test --headless --firefox crates/web`
Expected: FAIL — the card renders the front (`name`), not the reverse.

- [ ] **Step 3: Implement the face helper + reverse render**

Add to `act_agenda.rs`:

```rust
/// Which face of an advancing act/agenda to show: `Reverse` once the advance has
/// passed its acknowledge (`step >= FireReverse`), else `Front` (#558).
enum Face { Front, Reverse }

fn deck_face(game: &GameState, deck: game_core::state::AdvanceDeck) -> Face {
    use game_core::state::{AdvanceStep, Continuation};
    for c in &game.continuations {
        if let Continuation::AdvanceReverse { deck: d, step, .. } = c {
            if *d == deck {
                return match step {
                    AdvanceStep::AwaitAck => Face::Front,
                    AdvanceStep::FireReverse | AdvanceStep::Finalize => Face::Reverse,
                };
            }
        }
    }
    Face::Front
}
```

`name_and_text` gains a reverse variant (or a `face` arg): when `Reverse`, read `back_name`/`back_text` from metadata instead of `name`/`text`. `ActCard`/`AgendaCard` take the computed `Face` (from `act_agenda_view`, which has `game`) and render accordingly; the glow/menu still comes from `options_for(pending, OptionTarget::{Act,Agenda})` (front: the flip pick; reverse: the forced-ack "Resolve").

- [ ] **Step 4: Run to verify pass**

Run: `wasm-pack test --headless --firefox crates/web`
Expected: PASS (the new reverse-face test + all existing act/agenda tests).

- [ ] **Step 5: wasm clippy + commit + gauntlet + PR**

```bash
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
git add crates/web/src/act_agenda.rs crates/web/tests/act_agenda.rs
git commit -m "web: act/agenda flip to the reverse face while advancing (#558)"
```

Run the 7-job gauntlet; push `web/advance-flip-render`; open the PR — this one **`Closes #558`** (the flip is now user-visible). Phase-doc note (final commit, CI green): the advance-flip shipped (slices 1–3), slice 4 (01110) deferred. Merge only after explicit user approval.

---

# Slice 4 (deferred, #562) — 01110 `#466` suppression

Not planned in detail here. When picked up: a forced ability whose sole effect is an act/agenda advance suppresses its `#466 AcknowledgeForced` (the advance's on-card `AwaitAck` pick is the single flip). Tracked in #562; terminal, once-per-scenario.

---

## Self-review

**Spec coverage:**
- `back_name`/`back_text` ingestion → **Slice 1** ✅
- `AdvanceReverse.trigger` (forced/deliberate) → **Slice 2 (2.1–2.2)** ✅
- `AwaitAck` on-card pick for forced / skip for deliberate → **Slice 2 (2.3)** ✅
- `resume` accepts the pick → **Slice 2 (2.3 Step 4)** ✅
- Front/reverse face by `step` → **Slice 3** ✅
- 01110 suppression → **Slice 4 (deferred)** ✅ (explicitly out of the first cut)
- Card art/animation, #555, `resolve_choice_count` refactor → out of scope, per spec ✅

**Placeholder scan:** the test-`build state` fills and the pipeline test's `RawCard` construction are pinned to "match the file's existing precedent" rather than invented, because those harnesses (the AwaitAck test's state builder; whether the pipeline tests use literals or JSON) are the ground truth to copy — deliberate, not a gap. All production-code steps carry full code.

**Type consistency:** `AdvanceTrigger { Forced, Deliberate }` defined in 2.1, threaded through `advance_act(cx, trigger)` / `advance_agenda` (Forced) in 2.2, read in `AwaitAck` in 2.3; `Face`/`deck_face` defined and consumed in 3.1. `OptionTarget::Act`/`Agenda` (existing) used for the pick anchor (2.3) and the web filter (3.1).
