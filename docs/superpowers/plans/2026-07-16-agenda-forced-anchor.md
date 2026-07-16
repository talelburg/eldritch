# Anchor agenda-sourced forced effects to the agenda card — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Anchor an agenda's forced-acknowledge (and any agenda `Board` candidate in the ordered run) to the agenda card on the board, mirroring the act (S5) — the agenda glows and offers "Resolve" instead of only the flat bar.

**Architecture:** Add `OptionTarget::Agenda`; add `current_agenda_code`; extend the shared `candidate_anchor` with an agenda param + arm; thread the agenda code through both callers. Web: extract an interactive `AgendaCard` mirroring `ActCard`. Anchor is display-only.

**Tech Stack:** Rust (`game-core` engine + `web` Leptos/wasm).

**Design spec:** `docs/superpowers/specs/2026-07-16-agenda-forced-anchor-design.md`

## Global Constraints

- **Anchors are display-only.** No resolve path reads the anchor; resume validates only the echoed `OptionId`.
- **Mirror `ActCard` exactly** in the web component (glow + `menu_layer`); keep the `doom {doom}/{threshold}` stat line.
- **Timing (verified):** the `AgendaAdvanced` forced fires at `FireReverse`, before `Finalize` bumps `agenda_index`, so `cand.code == current_agenda` holds at ack time — a code-equality anchor is correct.
- **CI is 7 warnings-as-errors jobs.** Match locally before pushing:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `wasm-pack test --headless --firefox crates/web`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- Commit subjects: `scope: description`. Branch `web/agenda-anchor` (already created).

## File structure

- `crates/game-core/src/engine/outcome.rs` — Task 1 (`OptionTarget::Agenda`).
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — Task 2 (`current_agenda_code`; `candidate_anchor` param+arm; `build_resolution_options` param; 2 real + 2 test callers).
- `crates/game-core/src/engine/dispatch/forced_triggers.rs` — Task 2 (`drive_acknowledge_forced` passes the agenda code; add a unit test).
- `crates/cards/tests/agenda_forced_anchor.rs` — Task 3 (integration: real agenda advance → `Agenda` anchor).
- `crates/web/src/act_agenda.rs` — Task 4 (`AgendaCard` component + `act_agenda_view` swap; web headless test).

Task 2 depends on Task 1's variant. Task 3 depends on Task 2. Task 4 depends only on Task 1 (the `OptionTarget::Agenda` variant) but is cleanest after Task 2.

---

### Task 1: Add `OptionTarget::Agenda`

**Files:**
- Modify: `crates/game-core/src/engine/outcome.rs` (variant, after `Act`)

- [ ] **Step 1: Add the variant**

In `outcome.rs`, in `enum OptionTarget`, after the `Act` variant:

```rust
    /// The current act.
    Act,
    /// The current agenda.
    Agenda,
```

- [ ] **Step 2: Verify it compiles (exhaustiveness surfaces the next task's sites)**

Run: `cargo build -p game-core`
Expected: clean (adding a variant to a `#[non_exhaustive]` enum with no new matches yet compiles; `candidate_anchor` produces it in Task 2). Any `match` on `OptionTarget` that is now non-exhaustive is a compile error to fix — none expected in `game-core` (matches are in `web`, which builds separately).

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/engine/outcome.rs
git commit -m "engine: add OptionTarget::Agenda"
```

---

### Task 2: Anchor agenda `Board` candidates via `candidate_anchor`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`current_agenda_code`; `candidate_anchor` param+arm; `build_resolution_options` param; callers; extend anchor test)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (`drive_acknowledge_forced`; add unit test)

**Interfaces:**
- Produces: `pub(super) fn current_agenda_code(state: &GameState) -> Option<CardCode>`.
- Changes: `candidate_anchor(cand, current_act)` → `candidate_anchor(cand, current_act, current_agenda)`.
- Changes: `build_resolution_options(candidates, current_act)` → `(candidates, current_act, current_agenda)`.

- [ ] **Step 1: Write the failing unit test (candidate_anchor agenda arm)**

In `reaction_windows.rs`, `resolution_option_anchor_tests::candidate_anchor_maps_each_source`, add an agenda case. First the calls need the new arg — update every existing `candidate_anchor(&x, Some(&act))` in that test to `candidate_anchor(&x, Some(&act), Some(&agenda))`, define `let agenda = CardCode::new("01105");`, add a `board_agenda` candidate, and assert:

```rust
        let agenda = CardCode::new("01105");
        let board_agenda = ResolutionCandidate::new(
            agenda.clone(),
            InvestigatorId(1),
            0,
            CandidateSource::Board,
        );
        assert_eq!(
            candidate_anchor(&board_agenda, Some(&act), Some(&agenda)),
            OptionTarget::Agenda
        );
        // an act-coded board candidate still wins Act even with an agenda present
        assert_eq!(
            candidate_anchor(&board_act, Some(&act), Some(&agenda)),
            OptionTarget::Act
        );
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core --lib candidate_anchor_maps_each_source`
Expected: FAIL — compile error: `candidate_anchor` takes 2 args, 3 supplied.

- [ ] **Step 3: Add `current_agenda_code`, extend `candidate_anchor` + `build_resolution_options`, update callers**

**3a.** In `reaction_windows.rs`, below `current_act_code`, add:

```rust
/// The current agenda's printed code, if the agenda deck is non-empty. The
/// mirror of [`current_act_code`]; used to anchor an agenda-sourced forced
/// effect to the agenda card (#556).
pub(super) fn current_agenda_code(state: &GameState) -> Option<CardCode> {
    state
        .agenda_deck
        .get(state.agenda_index)
        .map(|agenda| agenda.code.clone())
}
```

**3b.** Change `candidate_anchor`'s signature and `Board` arm:

```rust
pub(super) fn candidate_anchor(
    cand: &ResolutionCandidate,
    current_act: Option<&CardCode>,
    current_agenda: Option<&CardCode>,
) -> crate::engine::OptionTarget {
    use crate::engine::OptionTarget;
    match cand.source {
        CandidateSource::Hand => OptionTarget::HandCardByCode {
            investigator: cand.controller,
            code: cand.code.clone(),
        },
        CandidateSource::InPlay(instance_id) => OptionTarget::CardInstance(instance_id),
        CandidateSource::Location(location_id) => OptionTarget::Location(location_id),
        CandidateSource::Board => {
            if current_act == Some(&cand.code) {
                OptionTarget::Act
            } else if current_agenda == Some(&cand.code) {
                OptionTarget::Agenda
            } else {
                OptionTarget::Global
            }
        }
    }
}
```

Update the doc line above it to mention the agenda (append: "a board-wide effect to the act or agenda card when its code matches, else no card home").

**3c.** Change `build_resolution_options`'s signature and its `candidate_anchor` call:

```rust
fn build_resolution_options(
    candidates: &[ResolutionCandidate],
    current_act: Option<&CardCode>,
    current_agenda: Option<&CardCode>,
) -> Vec<ChoiceOption> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, cand)| {
            let id = OptionId(u32::try_from(i).expect("option count fits in u32"));
            let label = match cand.source {
                CandidateSource::Hand => format!("Play {} from hand", cand.code),
                CandidateSource::InPlay(_)
                | CandidateSource::Board
                | CandidateSource::Location(_) => {
                    format!("Resolve reaction: {}", cand.code)
                }
            };
            ChoiceOption::new(id, label, candidate_anchor(cand, current_act, current_agenda))
        })
        .collect()
}
```

**3d.** The two real callers (currently `let current_act = current_act_code(cx.state); build_resolution_options(candidates, current_act.as_ref())` — around lines 678 and 995). At each, add the agenda code and pass it:

```rust
    let current_act = current_act_code(cx.state);
    let current_agenda = current_agenda_code(cx.state);
    let options = build_resolution_options(candidates, current_act.as_ref(), current_agenda.as_ref());
```

(One site binds `let options = build_resolution_options(` across lines; the other is a one-liner — update both to the 3-arg form.)

**3e.** The two test callers in `resolution_option_anchor_tests` (around lines 2171, 2205): `build_resolution_options(&cands, None)` → `build_resolution_options(&cands, None, None)`; `build_resolution_options(&cands, Some(&act))` → `build_resolution_options(&cands, Some(&act), None)`.

**3f.** In `forced_triggers.rs`, `drive_acknowledge_forced` — pass the agenda code:

```rust
    let act = super::reaction_windows::current_act_code(cx.state);
    let agenda = super::reaction_windows::current_agenda_code(cx.state);
    let anchor = super::reaction_windows::candidate_anchor(candidate, act.as_ref(), agenda.as_ref());
```

- [ ] **Step 4: Run to verify the unit test passes**

Run: `cargo build -p game-core` — expected: clean.
Run: `cargo test -p game-core --lib -- candidate_anchor_maps_each_source` — expected: PASS.

- [ ] **Step 5: Add the `drive_acknowledge_forced` agenda unit test**

In `forced_triggers.rs` `mod tests`, mirror the location anchor test with an agenda `Board` source. The test needs an agenda in the deck so `current_agenda_code` resolves — use the builder's agenda support:

```rust
    #[test]
    fn acknowledge_forced_anchors_an_agenda_source_to_the_agenda_card() {
        use crate::engine::OptionTarget;
        use crate::state::{Agenda, Continuation};
        use crate::test_support::GameStateBuilder;

        let mut state = GameStateBuilder::default().build();
        state.agenda_deck = vec![Agenda {
            code: CardCode::new("01105"),
            doom_threshold: 3,
            resolution: None,
        }];
        state.agenda_index = 0;
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01105"),
                InvestigatorId(1),
                0,
                CandidateSource::Board,
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match super::drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1);
                assert_eq!(request.options[0].target, OptionTarget::Agenda);
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }
```

(Verify `Agenda`'s field set against `state/*.rs` at write time — `{ code, doom_threshold, resolution }`. If `Agenda` is `#[non_exhaustive]` or the builder exposes an agenda helper, prefer that; adjust to whatever constructs an agenda in existing tests — grep `agenda_deck =` / `Agenda {` in tests.)

- [ ] **Step 6: Run + clippy + commit**

Run: `cargo test -p game-core --lib -- acknowledge_forced_anchors_an_agenda` — expected: PASS.
Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings` — expected: clean.

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs \
        crates/game-core/src/engine/dispatch/forced_triggers.rs
git commit -m "engine: anchor agenda-sourced forced effects to the agenda (#556)"
```

---

### Task 3: Integration test — a real agenda advance anchors to the agenda

**Files:**
- Create: `crates/cards/tests/agenda_forced_anchor.rs`

**Interfaces:**
- Consumes: `game_core::apply`, the real `cards::REGISTRY`, an agenda-advance driver.

- [ ] **Step 1: Find the agenda-advance test pattern**

Before writing, grep existing integration tests for how an agenda is advanced interactively and how the forced reverse is driven: `crates/cards/tests/agenda_reverses.rs`, `crates/cards/tests/whats_going_on*`, and `test_support` for an agenda-advance / doom helper (e.g. `fire_forced_*`, `advance_agenda`, or placing doom to threshold then ending the round). Reuse that harness — do **not** hand-roll the advance.

- [ ] **Step 2: Write the test (red)**

New file `crates/cards/tests/agenda_forced_anchor.rs`. Install `cards::REGISTRY` via `#[ctor::ctor]` (mirror `forced_acknowledge.rs`). Set up What's Going On?! (01105) as the current agenda with `interactive_acknowledge = true`, drive the advance so its `AgendaAdvanced` forced fires, and assert the surfaced `AwaitingInput` request's first option target is `OptionTarget::Agenda`:

```rust
use game_core::engine::{EngineOutcome, OptionTarget};
// …install REGISTRY; build state with agenda 01105 current, interactive on…
match out {
    EngineOutcome::AwaitingInput { request, .. } => {
        assert_eq!(
            request.options[0].target,
            OptionTarget::Agenda,
            "an agenda forced-on-advance ack anchors to the agenda card (#556)"
        );
    }
    other => panic!("expected the forced-acknowledge suspend, got {other:?}"),
}
```

(Exact setup mirrors whichever harness Step 1 finds. If the cleanest entry is `test_support::fire_forced_*` for `AgendaAdvanced`, use it directly — the same shape as `fire_forced_on_enter` in `forced_acknowledge.rs`. Confirm 01105 emits a **single** forced hit so `drive_acknowledge_forced` is the path; if it fans out to 2+, the ordered run applies instead — still `Agenda`-anchored via `build_resolution_options`, adjust the assertion to inspect that option list.)

- [ ] **Step 3: Run to verify failure, then it passes against Task 2's engine**

Run: `cargo test -p cards --test agenda_forced_anchor`
Expected: PASS (Task 2 already anchors agenda `Board` sources). If this is the first run after Task 2, it should pass directly; if run before Task 2 it would show `Global`.

- [ ] **Step 4: Commit**

```bash
git add crates/cards/tests/agenda_forced_anchor.rs
git commit -m "test: agenda forced-on-advance anchors to the agenda card (#556)"
```

---

### Task 4: Interactive `AgendaCard` web component

**Files:**
- Modify: `crates/web/src/act_agenda.rs` (`AgendaCard` component; `act_agenda_view` swap; headless glow test)

**Interfaces:**
- Consumes: `game_core::OptionTarget::Agenda` (Task 1); `crate::interaction::{PendingOptions, options_for, menu_layer}` (as `ActCard` uses).

- [ ] **Step 1: Write the failing headless test**

Find the existing `ActCard` glow test (grep `actionable` / `card--act` / `ActCard` in `crates/web/src`). Mirror it for the agenda: mount an `AgendaCard` with a `PendingOptions` context carrying one `OptionTarget::Agenda` option, assert the rendered root has class `actionable`. Remember DOM accumulates across wasm tests in a binary — select the **last** matching element (`query_selector_all` + last), per the `last_slot()` precedent.

(If no `ActCard` headless glow test exists, add a minimal one for `AgendaCard` only: render with an agenda-anchored pending option present vs absent, assert `.actionable` present/absent respectively.)

- [ ] **Step 2: Run to verify failure**

Run: `wasm-pack test --headless --firefox crates/web` (or the single test if the runner supports filtering)
Expected: FAIL — `AgendaCard` doesn't exist / agenda `<article>` never carries `actionable`.

- [ ] **Step 3: Add `AgendaCard`, swap `act_agenda_view`**

In `act_agenda.rs`, add the component (mirror `ActCard`, keep the doom stat line):

```rust
/// The current agenda as a card. Glows and opens a "Resolve" context menu when the
/// live prompt anchors an option to the agenda (`OptionTarget::Agenda`, #556) — an
/// agenda-sourced forced effect (What's Going On?! 01105's on-advance reverse). The
/// doom counter lives on `GameState`, so it arrives as a second prop.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn AgendaCard(agenda: Agenda, doom: u8) -> impl IntoView {
    let (name, text) = name_and_text(&agenda.code);
    let threshold = agenda.doom_threshold;
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = crate::interaction::options_for(&pending, game_core::OptionTarget::Agenda);
    let actionable = !menu_opts.is_empty();
    #[cfg(target_arch = "wasm32")]
    let open = RwSignal::new(None::<(i32, i32)>);
    let mut root_class = String::from("card card--agenda");
    if actionable {
        root_class.push_str(" actionable");
    }
    view! {
        <article class=root_class>
            <div class="card-head">
                <span class="card-kind">"Agenda"</span>
                <span class="card-name">{name}</span>
            </div>
            <div class="card-text">{text}</div>
            <div class="loc-stats">
                <span>{format!("doom {doom}/{threshold}")}</span>
            </div>
            {
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
            }
        </article>
    }
}
```

Add `Agenda` to the `use game_core::state::{…}` import. Replace the inline agenda block in `act_agenda_view`:

```rust
    let agenda = game
        .agenda_deck
        .get(game.agenda_index)
        .cloned()
        .map(|ag| view! { <AgendaCard agenda=ag doom=game.agenda_doom/> });
```

Update the module doc comment (line 27 in `ActCard`'s doc and the file header's "Display-only.") — the agenda is no longer display-only.

- [ ] **Step 4: Run to verify pass**

Run: `wasm-pack test --headless --firefox crates/web`
Expected: PASS (agenda glow test + the existing phase-bar tests).

- [ ] **Step 5: wasm clippy + commit**

Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` — expected: clean.

```bash
git add crates/web/src/act_agenda.rs
git commit -m "web: interactive AgendaCard mirrors ActCard (#556)"
```

---

### Task 5: Full gauntlet + PR

**Files:** none (verification + PR); phase-doc note is the final commit at PR-ready time.

- [ ] **Step 1: Run the full 7-job gauntlet** (all commands from Global Constraints). Expected: all green.

- [ ] **Step 2: Push + open the PR**

```bash
git push -u origin web/agenda-anchor
gh pr create --fill
```

PR body: the act↔agenda asymmetry closed, the shared-`candidate_anchor` agenda arm, the verified advance timing (index bumps after the forced fires), the display-only anchor, `#555` scoped out. `Closes #556`.

- [ ] **Step 3: Watch CI** — `gh pr checks <PR#> --watch`; fix failures with follow-up commits.

- [ ] **Step 4: Phase-doc note (only once CI is green + ready to merge)**

Add a bullet to the interactivity section of `docs/phases/phase-7-the-gathering.md` recording the agenda anchor (#556, PR #N) as the act↔agenda parity follow-up; note #555 as the deferred effect-internal-choice machinery. Commit as the final commit.

- [ ] **Step 5: Merge only after explicit user approval** — `gh pr merge <PR#> --squash --delete-branch`; confirm #556 auto-closed and `git pull` on `main`.

---

## Self-review

**Spec coverage:**
- `OptionTarget::Agenda` → **Task 1** ✅
- `current_agenda_code` → **Task 2 (3a)** ✅
- `candidate_anchor` agenda param + arm → **Task 2 (3b)** ✅
- Both real callers + build_resolution_options threaded → **Task 2 (3c/3d)** ✅
- `drive_acknowledge_forced` passes the agenda code → **Task 2 (3f)** ✅
- Engine unit tests (candidate_anchor + drive_acknowledge) → **Task 2** ✅
- Integration (real agenda advance → Agenda) → **Task 3** ✅
- Interactive `AgendaCard` + view swap → **Task 4** ✅
- Web headless glow test → **Task 4** ✅

**Placeholder scan:** Task 3's exact harness and Task 4's exact test-mirror are pinned to "grep the existing pattern first" rather than invented — deliberate, since the repo's agenda-advance driver and `ActCard` glow test are the ground truth to copy. All code steps that add production code carry full code.

**Type consistency:** `candidate_anchor(&ResolutionCandidate, Option<&CardCode>, Option<&CardCode>)` defined in Task 2, called in Task 2 (build_resolution_options + drive_acknowledge_forced) with matching types; `build_resolution_options(_, _, _)` 3-arg form consistent across both real callers and both test callers; `AgendaCard(agenda: Agenda, doom: u8)` consumed by `act_agenda_view` with `doom=game.agenda_doom`.
