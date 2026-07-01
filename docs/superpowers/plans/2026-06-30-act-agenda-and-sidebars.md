# Act/Agenda Cards + Turn Tracker + Collapsible Log Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three-column layout: Act/Agenda cards atop the board, a right-hand turn tracker (RR phase/sub-step/player-window outline, current phase highlighted), and a collapsible left event log.

**Architecture:** New `act_agenda.rs` (a `location_map`-style pub fn rendered in `BoardView`) and `turn_tracker.rs` (a store-reading component in the right column). `BoardView`'s `phase_bar` is dismantled (act/agenda → cards, phase/round → tracker). `EventLogView` gains a client `collapsed` signal. `app.rs` wires three columns.

**Tech Stack:** Rust, Leptos (CSR), Trunk, `wasm-bindgen-test` (headless Firefox), `card_registry`/`cards::REGISTRY`, `game_core::test_support::fixtures`.

## Global Constraints

- **Warnings are errors in CI** across seven jobs (native + wasm). Before pushing: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Clippy `pedantic` is on** (`module_name_repetitions` / `must_use_candidate` allowed); `doc_markdown` enforced — backtick-quote type names / `ArkhamDB` in doc comments.
- **wasm-only test files** carry crate-level `#![cfg(target_arch = "wasm32")]`; headless tests share one page (scope to last subtree). Registry install is **first-wins per process** — never mix `install_test_registry` and `cards::REGISTRY` in one test binary.
- **Stats/text from the corpus / `GameState`** — never hand-typed. Act card shows `clues to advance: {clue_threshold}` (no fake progress); agenda shows `doom {agenda_doom}/{doom_threshold}`.
- **Turn-tracker outline is authored from the Rules Reference** (verbatim, with the page citation in the module doc-comment) — the exact text is supplied in Task 2 below; do not paraphrase from memory.
- **Display-only:** no engine wiring / no `OutboundTx`.

## File structure

- **Create `crates/web/src/act_agenda.rs`** — `pub fn act_agenda_view(game: &GameState) -> impl IntoView` (mirrors `map::location_map`). (Task 1)
- **Create `crates/web/src/turn_tracker.rs`** — `#[component] pub fn TurnTrackerView()` + the static RR `ROUND` outline. (Task 2)
- **Modify `crates/web/src/lib.rs`** — register both new modules.
- **Modify `crates/web/src/board.rs`** — render `act_agenda_view` in `BoardView`; dismantle `phase_bar` (Task 1 removes act/agenda from it; Task 2 removes it entirely).
- **Modify `crates/web/src/event_log.rs`** — collapsible. (Task 3)
- **Modify `crates/web/src/app.rs`** — three-column layout (right tracker). (Task 2)
- **Modify `crates/web/style.css`** — act/agenda accents (Task 1), 3-column + tracker (Task 2), collapsed log (Task 3).
- **Create `crates/web/tests/act_agenda.rs`** (Task 1), **`crates/web/tests/turn_tracker.rs`** (Task 2); **modify `crates/web/tests/board.rs`** (Tasks 1–2), **`crates/web/tests/event_log.rs`** (Task 3).

Type notes (verified): `GameState.act_deck: Vec<Act>` / `act_index: usize`; `agenda_deck: Vec<Agenda>` / `agenda_index` / `agenda_doom: u8`. `Act { code: CardCode, clue_threshold: u8, resolution: Option<..> }`; `Agenda { code, doom_threshold: u8, resolution }`. `Phase` (`game_core::state::Phase`) is `Copy + PartialEq`, variants `Mythos`/`Investigation`/`Enemy`/`Upkeep`. `crate::card::parse_card_text` (pub) + `render_segments` (pub(crate)). `map::location_map` is the pub-fn-rendered-in-BoardView precedent.

---

### Task 1: Act/Agenda cards atop the board

**Files:**
- Create: `crates/web/src/act_agenda.rs`
- Modify: `crates/web/src/lib.rs`, `crates/web/src/board.rs`, `crates/web/style.css`
- Create: `crates/web/tests/act_agenda.rs`
- Modify: `crates/web/tests/board.rs`

**Interfaces:**
- Produces: `pub fn act_agenda_view(game: &GameState) -> impl IntoView` rendering `<section class="act-agenda">` with an act card (`card card--act`, `clues to advance: N`) and an agenda card (`card card--agenda`, `doom d/N`), each name/text from the corpus by `code`. Empty when the respective deck is empty.

- [ ] **Step 1: Register the module**

In `crates/web/src/lib.rs`, add (alphabetically, after `pub mod app;`):

```rust
pub mod act_agenda;
```

- [ ] **Step 2: Write the failing headless test**

Create `crates/web/tests/act_agenda.rs`:

```rust
//! Headless render test for the Act/Agenda cards. Own binary so it installs the
//! real `cards::REGISTRY` (registry install is first-wins per process); mounts
//! `act_agenda_view` directly (no investigator panel → no TEST_INV lookup).
#![cfg(target_arch = "wasm32")]

use game_core::state::{Act, Agenda, CardCode, GameStateBuilder};
use leptos::prelude::document;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn section_text() -> String {
    let nodes = document().query_selector_all(".act-agenda").expect("query ok");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn act_and_agenda_render_name_text_and_thresholds() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Act 01109 "The Barrier" (Objective text); Agenda 01107 "They're Getting
    // Out!" (Forced text).
    let mut state = GameStateBuilder::new().build();
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 2,
        resolution: None,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_doom = 1;

    leptos::mount::mount_to_body(move || web::act_agenda::act_agenda_view(&state));
    leptos::task::tick().await;

    let text = section_text();
    assert!(text.contains("The Barrier"), "act name missing: {text}");
    assert!(text.contains("clues to advance: 2"), "act threshold missing: {text}");
    assert!(text.contains("Objective"), "act ability text missing: {text}");
    assert!(text.contains("They're Getting Out!"), "agenda name missing: {text}");
    assert!(text.contains("doom 1/3"), "agenda doom missing: {text}");
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test act_agenda`
Expected: FAIL — `web::act_agenda::act_agenda_view` doesn't exist (compile error).

- [ ] **Step 4: Implement `act_agenda_view`**

Create `crates/web/src/act_agenda.rs`:

```rust
//! Act + Agenda cards atop the board (above the map). A pure render of
//! `GameState` (mirrors `map::location_map`): the current act and agenda render
//! as cards — name + ability text from the corpus by `code`, thresholds from the
//! `Act`/`Agenda` structs. The act has no running clue counter (clues sit on
//! locations/investigators), so it shows `clues to advance: N` rather than a
//! fake `0/N`. Display-only.

use game_core::state::{CardCode, GameState};
use leptos::prelude::*;

use crate::card::{parse_card_text, render_segments};

/// Name (printed, or the raw code when no metadata) + rendered ability text for
/// an act/agenda card code.
fn name_and_text(code: &CardCode) -> (String, Option<Vec<AnyView>>) {
    let meta = game_core::card_registry::current().and_then(|r| (r.metadata_for)(code));
    let name = meta.map_or_else(|| code.to_string(), |m| m.name.clone());
    let text = meta
        .and_then(|m| m.text.as_deref())
        .map(|t| render_segments(parse_card_text(t)));
    (name, text)
}

/// The current act + agenda as cards. Each is omitted when its deck is empty
/// (fixtures may carry neither).
pub fn act_agenda_view(game: &GameState) -> impl IntoView {
    let act = game.act_deck.get(game.act_index).map(|act| {
        let (name, text) = name_and_text(&act.code);
        let threshold = act.clue_threshold;
        view! {
            <article class="card card--act">
                <div class="card-head">
                    <span class="card-kind">"Act"</span>
                    <span class="card-name">{name}</span>
                </div>
                <div class="card-text">{text}</div>
                <div class="loc-stats">
                    <span>{format!("clues to advance: {threshold}")}</span>
                </div>
            </article>
        }
    });
    let agenda = game.agenda_deck.get(game.agenda_index).map(|ag| {
        let (name, text) = name_and_text(&ag.code);
        let doom = game.agenda_doom;
        let threshold = ag.doom_threshold;
        view! {
            <article class="card card--agenda">
                <div class="card-head">
                    <span class="card-kind">"Agenda"</span>
                    <span class="card-name">{name}</span>
                </div>
                <div class="card-text">{text}</div>
                <div class="loc-stats">
                    <span>{format!("doom {doom}/{threshold}")}</span>
                </div>
            </article>
        }
    });
    view! { <section class="act-agenda">{act}{agenda}</section> }
}
```

- [ ] **Step 5: Render it in `BoardView` + drop act/agenda from `phase_bar`**

In `crates/web/src/board.rs`, in the `Some(game)` board view, insert the act/agenda section after the resolution banner:

```rust
            <div class="game">
                {resolution_banner(&game)}
                {crate::act_agenda::act_agenda_view(&game)}
                {phase_bar(&game)}
                <div class="board-main">
                    {crate::map::location_map(&game)}
                    {investigators_panel(&game)}
                </div>
            </div>
```

Then remove the act + agenda lines from `phase_bar` (keep phase + round for now — Task 2 removes the rest). The `phase_bar` fn becomes:

```rust
fn phase_bar(game: &GameState) -> impl IntoView {
    let phase = format!("{:?}", game.phase);
    let round = game.round;
    view! {
        <header class="phase-bar">
            <span class="phase">{phase}</span>
            <span class="round">"round " {round}</span>
        </header>
    }
}
```

- [ ] **Step 6: Update the board test's act/agenda assertion**

In `crates/web/tests/board.rs`, `phase_bar_renders_phase_round_act_agenda` currently asserts `"clues 0/2"`. Act/agenda now render via `act_agenda_view` (synthetic registry → `_test_act`/`_test_agenda` have no metadata, so names fall back to codes, thresholds come from the structs). Replace the act assertion:

```rust
    assert!(
        html.contains("clues 0/2") || html.contains("0/2"),
        "act threshold missing: {html}"
    );
```

with:

```rust
    assert!(
        html.contains("clues to advance: 2"),
        "act threshold missing: {html}"
    );
```

(The `doom 1/5` assertion stays — the agenda card renders it. The `Investigation` + `round 3` assertions stay — `phase_bar` still shows them.)

- [ ] **Step 7: Add the CSS**

In `crates/web/style.css`, after the card-palette block (near `.card--unknown, .card--generic`), add:

```css
.act-agenda { display: flex; flex-wrap: wrap; gap: 0.5rem; margin-bottom: 0.5rem; }
.card--act { border-color: #c8a020; }
.card--agenda { border-color: #7a2f2f; }
.card-kind { font-size: 0.7rem; text-transform: uppercase; opacity: 0.7; margin-right: 0.3rem; }
```

(The act/agenda articles reuse `.card` from the existing rules; these add the accents + the kind label.)

- [ ] **Step 8: Run the tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web --test act_agenda` (PASS), then `wasm-pack test --headless --firefox crates/web --test board` (PASS — updated assertion).

- [ ] **Step 9: Verify clippy (both targets) + fmt + build**

Run: `cargo clippy -p web --all-targets --all-features -- -D warnings` and `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings` and `cargo fmt --check` and `cd crates/web && trunk build`.
Expected: clean / builds.

- [ ] **Step 10: Commit**

```bash
git add crates/web/src/act_agenda.rs crates/web/src/lib.rs crates/web/src/board.rs crates/web/style.css crates/web/tests/act_agenda.rs crates/web/tests/board.rs
git commit -m "web: render Act/Agenda as cards atop the board

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 2: Right-hand turn tracker + 3-column layout

**Files:**
- Create: `crates/web/src/turn_tracker.rs`
- Modify: `crates/web/src/lib.rs`, `crates/web/src/app.rs`, `crates/web/src/board.rs`, `crates/web/style.css`
- Create: `crates/web/tests/turn_tracker.rs`
- Modify: `crates/web/tests/board.rs`

**Interfaces:**
- Produces: `#[component] pub fn TurnTrackerView()` rendering `<aside class="turn-tracker">` with `Round {n}` and the four phases; the phase matching `game.phase` carries a `current` class; player windows render as `<li class="tracker-window">`.

- [ ] **Step 1: Register the module**

In `crates/web/src/lib.rs`, add `pub mod turn_tracker;` (alphabetically — after `skill_test_result` is fine; keep the list sorted).

- [ ] **Step 2: Write the failing headless test**

Create `crates/web/tests/turn_tracker.rs`:

```rust
//! Headless render tests for the turn tracker. wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::state::{GameStateBuilder, Phase};
use game_core::test_support::fixtures::test_investigator;
use game_core::EngineOutcome;
use leptos::prelude::{document, provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::store::{reduce, ClientState};
use web::turn_tracker::TurnTrackerView;

wasm_bindgen_test_configure!(run_in_browser);

async fn mount_at(phase: Phase, round: u32) {
    game_core::test_support::install_test_registry();
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_phase(phase)
        .with_round(round)
        .build();
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <TurnTrackerView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
    });
    leptos::task::tick().await;
}

fn last_tracker() -> web_sys::Element {
    let nodes = document().query_selector_all(".turn-tracker").expect("query ok");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .turn-tracker")
}

#[wasm_bindgen_test]
async fn lists_all_phases_substeps_and_round() {
    mount_at(Phase::Investigation, 2).await;
    let t = last_tracker().text_content().unwrap_or_default();
    assert!(t.contains("Round 2"), "round missing: {t}");
    assert!(t.contains("Mythos"), "Mythos missing: {t}");
    assert!(t.contains("Investigation"), "Investigation missing: {t}");
    assert!(t.contains("Enemy"), "Enemy missing: {t}");
    assert!(t.contains("Upkeep"), "Upkeep missing: {t}");
    assert!(t.contains("Place 1 doom on the current agenda."), "a Mythos sub-step missing: {t}");
    assert!(t.contains("player window"), "player windows missing: {t}");
}

#[wasm_bindgen_test]
async fn current_phase_is_highlighted() {
    mount_at(Phase::Enemy, 1).await;
    let tracker = last_tracker();
    // Exactly one phase block carries `current`, and it is the Enemy block.
    let current = tracker
        .query_selector(".tracker-phase.current")
        .expect("query ok")
        .expect("a current phase block")
        .text_content()
        .unwrap_or_default();
    assert!(current.contains("Enemy"), "current phase should be Enemy: {current}");
    assert!(!current.contains("Mythos"), "only the Enemy block is current: {current}");
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test turn_tracker`
Expected: FAIL — `web::turn_tracker::TurnTrackerView` doesn't exist.

- [ ] **Step 4: Implement `TurnTrackerView` with the RR outline**

Create `crates/web/src/turn_tracker.rs`. The `ROUND` constant is transcribed **verbatim from the FFG Rules Reference, Appendix II "Timing and Gameplay"** — the Phase Sequence timing chart + Framework Event Details (`data/rules-reference/ahc01_rules_reference_web.pdf`, pp. 23–25): grey boxes are framework events (`Step::Framework`), red boxes are player windows (`Step::Window`).

```rust
//! Right-hand turn tracker: the round's four phases with their Rules-Reference
//! sub-steps and structural player windows, highlighting the current phase.
//!
//! The `ROUND` outline is transcribed verbatim from the FFG Rules Reference,
//! Appendix II "Timing and Gameplay" — the Phase Sequence timing chart and
//! Framework Event Details (`data/rules-reference/ahc01_rules_reference_web.pdf`,
//! pp. 23-25). Grey boxes are framework events; red boxes are player windows.
//! The engine exposes only the coarse phase, so only the current *phase* is
//! highlighted. Display-only.

use game_core::state::Phase;
use leptos::prelude::*;

use crate::store::use_store;

/// One entry in a phase's ordered outline.
enum Step {
    /// A framework event (mandatory, grey box).
    Framework(&'static str),
    /// A structural player window (red box).
    Window,
}

struct PhaseOutline {
    phase: Phase,
    label: &'static str,
    steps: &'static [Step],
}

use Step::{Framework, Window};

const ROUND: &[PhaseOutline] = &[
    PhaseOutline {
        phase: Phase::Mythos,
        label: "Mythos",
        steps: &[
            Framework("1.1 Round begins. Mythos phase begins."),
            Framework("1.2 Place 1 doom on the current agenda."),
            Framework("1.3 Check doom threshold."),
            Framework("1.4 Each investigator draws 1 encounter card."),
            Window,
            Framework("1.5 Mythos phase ends."),
        ],
    },
    PhaseOutline {
        phase: Phase::Investigation,
        label: "Investigation",
        steps: &[
            Framework("2.1 Investigation phase begins."),
            Window,
            Framework("2.2 Next investigator's turn begins."),
            Window,
            Framework("2.2.1 Active investigator may take an action, if able."),
            Framework("2.2.2 Investigator's turn ends."),
            Framework("2.3 Investigation phase ends."),
        ],
    },
    PhaseOutline {
        phase: Phase::Enemy,
        label: "Enemy",
        steps: &[
            Framework("3.1 Enemy phase begins."),
            Framework("3.2 Hunter enemies move."),
            Window,
            Framework("3.3 Next investigator resolves engaged enemy attacks."),
            Window,
            Framework("3.4 Enemy phase ends."),
        ],
    },
    PhaseOutline {
        phase: Phase::Upkeep,
        label: "Upkeep",
        steps: &[
            Framework("4.1 Upkeep phase begins."),
            Window,
            Framework("4.2 Reset actions."),
            Framework("4.3 Ready each exhausted card."),
            Framework("4.4 Each investigator draws 1 card and gains 1 resource."),
            Framework("4.5 Each investigator checks hand size."),
            Framework("4.6 Upkeep phase ends. Round ends."),
        ],
    },
];

#[component]
pub fn TurnTrackerView() -> impl IntoView {
    let store = use_store();
    move || {
        let game = store.get().game;
        let current = game.as_ref().map(|g| g.phase);
        let round = game.as_ref().map(|g| g.round);
        let phases: Vec<_> = ROUND
            .iter()
            .map(|p| {
                let cls = if current == Some(p.phase) {
                    "tracker-phase current"
                } else {
                    "tracker-phase"
                };
                let steps: Vec<_> = p
                    .steps
                    .iter()
                    .map(|s| match s {
                        Step::Framework(t) => view! { <li class="tracker-step">{*t}</li> }.into_any(),
                        Step::Window => {
                            view! { <li class="tracker-window">"player window"</li> }.into_any()
                        }
                    })
                    .collect();
                view! {
                    <div class=cls>
                        <div class="tracker-phase-label">{p.label}</div>
                        <ul>{steps}</ul>
                    </div>
                }
            })
            .collect();
        view! {
            <aside class="turn-tracker">
                <h2>"Turn"</h2>
                {round.map(|r| view! { <div class="tracker-round">{format!("Round {r}")}</div> })}
                {phases}
            </aside>
        }
        .into_any()
    }
}
```

- [ ] **Step 5: Wire the right column in `app.rs` + remove `phase_bar`**

In `crates/web/src/app.rs`, add the tracker as the third column of `.layout` (after `main-column`):

```rust
            <div class="layout">
                <crate::event_log::EventLogView/>
                <div class="main-column">
                    <BoardView/>
                    {
                        #[cfg(target_arch = "wasm32")]
                        { view! {
                            <div class="action-bar">
                                <crate::picker::PickerView/>
                                <crate::skill_test_result::SkillTestResultView/>
                                <crate::input::AwaitingInputView/>
                            </div>
                        }.into_any() }
                        #[cfg(not(target_arch = "wasm32"))]
                        { ().into_any() }
                    }
                </div>
                <crate::turn_tracker::TurnTrackerView/>
            </div>
```

In `crates/web/src/board.rs`, remove the `phase_bar` call from the board view and delete the `phase_bar` fn entirely (phase + round now live in the tracker):

```rust
            <div class="game">
                {resolution_banner(&game)}
                {crate::act_agenda::act_agenda_view(&game)}
                <div class="board-main">
                    {crate::map::location_map(&game)}
                    {investigators_panel(&game)}
                </div>
            </div>
```

Delete the entire `fn phase_bar(...) { ... }` definition.

- [ ] **Step 6: Update the board test (phase/round move to the tracker)**

In `crates/web/tests/board.rs`, the `phase_bar_renders_phase_round_act_agenda` test now over-asserts: `BoardView` no longer renders the phase/round (they're in `TurnTrackerView`, which this test does not mount). Rename it and drop the phase/round assertions, keeping the act/agenda ones:

```rust
#[wasm_bindgen_test]
async fn act_agenda_cards_render_name_and_thresholds() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_phase(Phase::Investigation)
        .with_round(3)
        .build();
    state.act_deck = vec![Act {
        code: CardCode("_test_act".into()),
        clue_threshold: 2,
        resolution: None,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode("_test_agenda".into()),
        doom_threshold: 5,
        resolution: None,
    }];
    state.agenda_doom = 1;

    let html = render_state(state).await;

    assert!(html.contains("doom 1/5"), "agenda doom missing: {html}");
    assert!(
        html.contains("clues to advance: 2"),
        "act threshold missing: {html}"
    );
}
```

(The `Phase`/`with_round` setup stays valid — `with_phase`/`with_round` are still on the builder; the test just no longer asserts the phase/round text, which the tracker test now covers. If `Phase` is now unused elsewhere in the file, keep the `use ... Phase` import since this test still references `Phase::Investigation`.)

- [ ] **Step 7: Add the 3-column + tracker CSS**

In `crates/web/style.css`, add (the `.layout` flex already exists; these size the columns + style the tracker):

```css
.turn-tracker { flex: 0 0 auto; width: 18rem; font-size: 0.8rem; }
.turn-tracker h2 { font-size: 1rem; margin: 0 0 0.25rem; }
.tracker-round { font-weight: 600; margin-bottom: 0.4rem; }
.tracker-phase { border-left: 3px solid transparent; padding: 0.15rem 0 0.15rem 0.4rem; margin-bottom: 0.3rem; }
.tracker-phase.current { border-left-color: #2f6fb3; background: #eef4fb; }
.tracker-phase-label { font-weight: 600; }
.tracker-phase ul { list-style: none; padding-left: 0.5rem; margin: 0.1rem 0; }
.tracker-step { color: #444; }
.tracker-window { color: #8a5a00; font-style: italic; }
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `wasm-pack test --headless --firefox crates/web --test turn_tracker` (PASS), then `--test board` (PASS), then `--test act_agenda` (PASS).

- [ ] **Step 9: Verify clippy (both targets) + fmt + build**

Run the two clippy targets, `cargo fmt --check`, and `cd crates/web && trunk build`. Expected: clean / builds.

- [ ] **Step 10: Commit**

```bash
git add crates/web/src/turn_tracker.rs crates/web/src/lib.rs crates/web/src/app.rs crates/web/src/board.rs crates/web/style.css crates/web/tests/turn_tracker.rs crates/web/tests/board.rs
git commit -m "web: right-hand turn tracker (RR phase outline) + 3-column layout

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01W8C6W8YhY8Lvo6aoKVDmcM"
```

---

### Task 3: Collapsible event log

**Files:**
- Modify: `crates/web/src/event_log.rs`, `crates/web/style.css`
- Modify: `crates/web/tests/event_log.rs`

**Interfaces:**
- `EventLogView` renders a toggle button (`.log-toggle`); a client `collapsed` signal hides the `.log-scroll` body (`hidden` class) when set.

- [ ] **Step 1: Write the failing headless test**

Read `crates/web/tests/event_log.rs` for its mount helper, then add (adapt the mount to the file's existing harness — it mounts `EventLogView` with a store):

```rust
#[wasm_bindgen_test]
async fn log_collapses_and_expands() {
    // Mount EventLogView (use the file's existing mount helper / store setup).
    let store = leptos::prelude::RwSignal::new(web::store::ClientState::default());
    leptos::mount::mount_to_body(move || {
        leptos::prelude::provide_context(store);
        leptos::view! { <web::event_log::EventLogView/> }
    });
    leptos::task::tick().await;

    let doc = leptos::prelude::document();
    let scroll = doc
        .query_selector(".event-log .log-scroll")
        .expect("query ok")
        .expect(".log-scroll present");
    assert!(
        !scroll.class_name().contains("hidden"),
        "log body should start visible: {}",
        scroll.class_name()
    );

    let toggle = doc
        .query_selector(".event-log .log-toggle")
        .expect("query ok")
        .expect(".log-toggle present")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement");
    toggle.click();
    leptos::task::tick().await;
    assert!(
        scroll.class_name().contains("hidden"),
        "log body should be hidden after collapse: {}",
        scroll.class_name()
    );
}
```

Ensure the test file's imports include `use wasm_bindgen::JsCast as _;` (for `dyn_into`) and `use wasm_bindgen_test::*;`; if the file lacks them, add them.

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test event_log`
Expected: FAIL — no `.log-toggle` exists.

- [ ] **Step 3: Add the collapse toggle**

In `crates/web/src/event_log.rs`, add a `collapsed` signal at the top of the component (after `let store = use_store();`):

```rust
    let collapsed = RwSignal::new(false);
```

Replace the returned `view!` with a header carrying the toggle and a `hidden`-gated body:

```rust
    view! {
        <aside class="event-log">
            <div class="event-log-head">
                <h2>"Event log"</h2>
                <button
                    class="log-toggle"
                    on:click=move |_| collapsed.update(|c| *c = !*c)
                >
                    {move || if collapsed.get() { "show" } else { "hide" }}
                </button>
            </div>
            <div
                class="log-scroll"
                class:hidden=move || collapsed.get()
                node_ref=scroll_ref
            >
                {batches}
            </div>
        </aside>
    }
```

- [ ] **Step 4: Add the CSS**

In `crates/web/style.css`, add:

```css
.event-log-head { display: flex; align-items: baseline; justify-content: space-between; }
.log-toggle { font-size: 0.75rem; }
.hidden { display: none; }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web --test event_log`
Expected: PASS (collapse test + any existing event-log tests).

- [ ] **Step 6: Verify clippy (both targets) + fmt + build**

Run the two clippy targets, `cargo fmt --check`, and `cd crates/web && trunk build`. Expected: clean / builds.

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/event_log.rs crates/web/style.css crates/web/tests/event_log.rs
git commit -m "web: make the event log collapsible

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

Expected: all green.

- [ ] **Step 2: Update the phase-7 doc — only when the PR is ready**

Extend the visual-card-rendering bullet in `docs/phases/phase-7-the-gathering.md` with this slice: Act/Agenda render as cards atop the board (act shows `clues to advance: N` — no running counter; agenda shows real `doom d/N`); a right-hand `TurnTrackerView` outlines the round (4 phases + RR sub-steps + structural player windows, transcribed verbatim from RR Appendix II pp. 23-25, current phase highlighted); the left event log is collapsible; the page is now three columns and the `phase_bar` is retired. Note the interactivity pass remains. Reference the new spec/plan.

- [ ] **Step 3: Open the PR**

Branch `web/act-agenda-sidebars`. File an issue first, push, open the PR; `Closes` it. Design-decisions paragraph: act/agenda via a `location_map`-style pure fn (own-binary metadata test); turn-tracker outline transcribed verbatim from RR Appendix II (cited); collapsible log via a client signal; `phase_bar` retired into the cards + tracker.

---

## Self-review notes

- **Spec coverage:** act/agenda cards atop board, no fake clue progress (Task 1) ✓; turn tracker w/ RR sub-steps + player windows + current-phase highlight + round (Task 2) ✓; collapsible log (Task 3) ✓; 3-column layout (Task 2 app.rs + CSS) ✓; `phase_bar` retired (Task 2) ✓; own-binary act/agenda metadata test + tracker test + log click test + board-test evolution ✓.
- **Type consistency:** `act_agenda_view(&GameState) -> impl IntoView` (pub fn) called in BoardView + mounted in its test; `TurnTrackerView` component mounted in app.rs + its test; `Phase` `Copy`+`PartialEq` used for `current == Some(p.phase)`; `Act`/`Agenda` field names (`code`/`clue_threshold`/`doom_threshold`) match `game_state.rs`; CSS classes (`act-agenda`, `card--act/agenda`, `turn-tracker`, `tracker-phase.current`, `tracker-window`, `log-toggle`, `hidden`) consistent between components, CSS, and tests.
- **RR fidelity:** the `ROUND` outline is transcribed verbatim from RR Appendix II (pp. 23-25), cited in the module doc-comment — not from memory.
- **Out of scope (unchanged):** interactivity/clickable cards, live sub-step highlighting, per-investigator turn tracking, the icon font.
