# Coding standards

What this repo expects of the code itself. The `code-review` skill's **Standards** axis reads this file; so should anything else asking "how is code written here?"

Standards live in exactly one place each. A few have a home elsewhere — this file points at those rather than restating them, because a standard copied twice is a standard that drifts. Everything else is defined below.

## Documented elsewhere

| Standard | Where |
|---|---|
| Validate-first / mutate-second handler contract, and the `apply_via` rollback that backstops it | [`architecture.md`](architecture.md) → Event-sourced state |
| Card text and rules citation policy (read the vendored text locally, always read the FAQ, never fetch) | `CLAUDE.md` → Cite card text and rules from the vendored sources |
| Running local checks with CI's exact strict flags | `CLAUDE.md` → Commands |
| Domain vocabulary — use the glossary's words in names and test titles | `GLOSSARY.md` |
| How the docs are written, not the code — the file to read before adding a rule to `CLAUDE.md` or writing an ADR, since both have a bar this file does not state | [`docs/agents/writing.md`](writing.md) |

## Defined here

### Match a card's declared `EventTiming` to its quoted trigger word

Every card module opens with the printed text verbatim. That block is the evidence: **read the declared `EventTiming` against the trigger word the module itself quotes**, and which cell each word names is **Timing cell** in `GLOSSARY.md`. A mismatch in the corpus is a bug.

**Say in prose which cell the ability resolves in, and why.** Write *"the `at` cell of the `EnemyDefeated` condition"* in the paragraph under the quoted text — not a description of the engine window the ability happens to ride. The form is a **bold inline lead-in opening *"Cell: …"***, not a `# Cell` rustdoc heading, so the paragraph sits in the header's flow next to the quoted text rather than opening a section a reader can scroll past. A module declaring two cells writes two such paragraphs, each naming which ability it is for (`the_barrier`, `cover_up`).

A card declaring `EventTiming::When` on a triggering condition whose resolve step has not been migrated to coordinator-owned resolution is **rejected**; migrating that condition is the fix, not retagging the card. See [ADR 0008](../adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md) and `ConditionResolution::Caller` (`crates/game-core/src/engine/dispatch/emit.rs`) for the per-condition migration and its cost. **No card is licensed to declare one cell and resolve in another**, and none can be.

**Why, and why there is no automated check:** the trigger word is not mechanically derivable from the printed text — *"if … would …"* is when-tier while a bare *"if"* on a settled state is at-tier, a tiering `GLOSSARY.md` argues from the rules rather than quoting. A parser would encode one reading and then be trusted as though it had checked. The six mis-tags #694 audited were not a gap in the evidence: every one of those modules quoted its own trigger word directly above the wrong enum. The failure was an unassigned reading, and this section is where it is assigned.

### Don't add DSL primitives speculatively

A new `Effect` (or `EventPattern`) variant waits until **two or more hand-written cards want the same pattern**. Until then the card gets a Rust impl or a card-local native tag. *"Place N doom on the current agenda"* is the worked case: Ancient Evils 01166 and Silver Twilight Acolyte 01102 each carried a byte-identical `<code>:place-doom` tag until #716 made the second consumer graduate it to `Effect::PlaceDoomOnCurrentAgenda`.

**Why:** a variant added for one card fixes the DSL's shape around a sample of one, and the kernel then has to keep resolving it. One consumer is a card-local detail; two is a pattern.

### Never hand-edit `crates/cards/src/generated/cards.rs`

It is pipeline output and carries a header comment saying so. Change the impl, or the snapshot and `PACK_FILES`, then re-run `cargo run -p card-data-pipeline` — see [`architecture.md`](architecture.md) → Card-data pipeline.

**Why:** the next pipeline run silently reverts the edit, and nothing between now and then tells you.

### Test layering

In order of importance:

1. **Card tests** — per-card, in `crates/cards/src/impls/<name>.rs` or split out beneath it (below); **each card needs at least one.**
2. **Engine unit tests** — per-module `#[cfg(test)]`, inline or split out beneath the module (below). Use `GameStateBuilder` (`.with_phase(…).with_investigator(…).with_active_investigator(…).build()` — a production type, reached as `game_core::state::GameStateBuilder`, with `test_support`'s `test_investigator(id)` / `test_location(id, name)` / `test_enemy(id, name)` fixtures) and the **event-assertion macros** `assert_event!` / `assert_no_event!` / `assert_event_count!` / `assert_event_sequence!` (order-insensitive by default; `_sequence` for in-order subsequence). Use `assert_eq!` on the events slice only when you need exact contiguous order.
3. **Integration tests** — `crates/cards/tests/`; each file is its own cargo binary/process, so it can install `cards::REGISTRY` without colliding — through `test_support::install_registry_with_test_cards`, which composes `TEST_INV` and the synthetic terminal cards on top. The right home for anything needing real card metadata + abilities, which `game-core` can't reach by crate direction. Pattern: `crates/cards/tests/play_card.rs`.
4. **Scenario end-to-end tests** — `crates/scenarios/tests/`; the same per-binary process isolation, and reserved for tests that exercise **real scenario data in situ** — a Gathering walk, its agenda's doom cascade, its reference card's symbol tokens. The line is *does it drive scenario content*, not *is it a full walk*. A test that needs no scenario content belongs at layer 2 or 3.

**New tests drive the engine through `test_support::TestSession`.** Construction settles the built state to where the engine would rest (`GameStateBuilder::open_turn` for an open turn), and each step applies once and drains to the next rest. Take turn actions with `take(&TurnAction)`. Answer a prompt with `pick(OptionTarget)`, naming the board entity the option is anchored to, and keep `pick_nth` for a Decision ([ADR 0015](../adr/0015-a-choice-printed-on-one-card-is-a-decision-not-a-selection.md)), where printed order is the meaning. Script the prompts you don't step through with `resolve_choices`, before the step that opens them. Assert on `prompt()`, `state()`, `events()` and `expect_rejected()`, never on the continuation stack. Existing hand-built `ResolveInput` sites and positional `PickSingle(OptionId(n))` picks migrate when next touched. A prompt whose board-entity options carry no anchor is answered by `pick_unanchored()` when exactly one option is un-anchored, and positionally through `apply` otherwise, until the engine anchors it (#950).

**Why:** about 15 test files each carried their own `pick` / `resolve` helper, and several matched on option labels, the string coupling [ADR 0011](../adr/0011-the-engine-names-the-surface-a-prompt-renders-on.md) rejects (#938). A positional pick names nothing a reviewer can check, and a test that reads the stack breaks on a kernel refactor that changed no behaviour.

**A unit test module moves out of its file once the file's test code passes 500 lines** — counted across all of the file's `#[cfg(test)]` modules, and all of them move together, so no file has some tests inline and some not. The file keeps a single `#[cfg(test)] mod tests;`. A single topic goes in a flat `foo/tests.rs`. Several topics go in a `foo/tests/` directory, one file per topic named without a `_tests` suffix (`encounter/tests/spawn_enemy.rs`), with `tests/mod.rs` holding only the shared `use` block, the module declarations, and any shared helpers; `use super::*;` at each level carries the parent's names down, so a moved test body is unchanged. (A `mod.rs` or crate root puts them beside itself: `engine/tests/`, `card-data-pipeline/src/tests/`.) **Split into topics only when the split is simple:** each group exercises a nameable production item (a function, handler, or type) with roughly three or more tests, and no test body needs editing to move. Banners are hints; the production item decides. Every group that passes splits off, and the rest pool into one remainder file — named for its theme if it genuinely has one, `other.rs` if not. A new test goes to the topic it exercises; the remainder is never the default. Unit tests stay unit tests — moving one into `crates/<crate>/tests/` would cost it access to private items.

**Why:** inline modules grew until they buried the code they test. `engine/mod.rs` was 325 lines of engine followed by a 4,857-line `mod tests`, and `encounter.rs` spread six test modules through its production functions (#915).

`game-core::test_support` is unconditionally `pub` (no feature flag).

**A synthetic fixture may model an engine primitive; it may not impersonate a printed card.** If the thing you are hand-writing has a code in `data/arkhamdb-snapshot/pack/`, use the real card and the real `cards::REGISTRY`. Behind the line sits the criterion that decides what it doesn't reach: **a test's substrate is chosen by what its failure should mean.** A test asking *does the hunter-movement rule work* wants an enemy whose only interesting property is `Hunter`; a test asking *does The Gathering play* wants The Gathering. Realism is not the axis.

Synthetic material sorts into three kinds, each with its own rule:

- **Primitive builders are shared, unconditional and mandatory.** `game_core::test_support::fixtures` builds entities, not cards. `Investigator` and `Location` are `#[non_exhaustive]`, so downstream test crates cannot construct them by literal, and `game-core` cannot reach `cards` by crate direction — there is no real-card alternative at the kernel. **A real investigator code is never a placeholder**: `test_investigator` resolves under every installed registry, so a test seats a printed investigator only when it is about that investigator (#934).
- **Probe cards are test-local**, defined in the one binary that reads them, with a per-binary code prefix (`_tc_*`, `_cd_*`, `SRC*` — see `crates/cards/tests/timing_cells.rs`). A probe exists to fire one cell in isolation, which no printed card does cleanly. Never promote one into a shared fixture module.
- **A mock scenario shares its module shell, never its state.** The no-op `ScenarioModule` is shared; the `setup()` state is composed per test with `GameStateBuilder`. `crates/server/tests/common/mod.rs:19-42` is the model.

**A fixture that is transitional says so inline and names its terminal condition** — the ticket that retires it, and which tests flip when it lands.

**Why:** the synthetic Cover Up fixture cloned real 01007's eligibility predicate byte-for-byte and dropped the card's printed Revelation; a later author hit the gap and wrote a comment about it rather than fixing it. *"Never silently approximate a card"* below is this same rule on the production side. [ADR 0016](../adr/0016-a-synthetic-fixture-models-a-primitive-never-a-printed-card.md) carries the rest of the evidence and the reasoning.

### Stub deferred functionality with a TODO that names the issue

When a variant, handler, or effect can't be implemented yet because the supporting infrastructure doesn't exist, return `EngineOutcome::Rejected` (or the analogous rejection) with a message in the form `TODO(#NN): <variant> needs <thing> (lands with #MM)`. Where several variants share a blocker, share a small helper rather than copy-pasting the prose.

Reserve `unreachable!()` for invariant violations — corruption, not unimplemented work. A `todo!()` panic and a silent no-op are both wrong: the first crashes on a path the engine should reject cleanly, the second pretends the feature works.

**Why:** each new piece of infrastructure depends on later infrastructure, so the gaps are numerous and long-lived. A loud rejection carrying a precise pointer keeps every gap visible and greppable.

### Never silently approximate a card

When a card can't be honestly expressed in the current DSL, there are two acceptable moves: ship the parts the DSL *can* express and document the gap in a `# Module gap` section in the card's module, or leave the card unimplemented and note the dependency. File the missing primitive as a follow-up issue either way.

Approximating is the one thing that isn't allowed. The playability gate would then hand a player a card the simulator resolves incorrectly — a wrong answer presented as a right one.

**Why:** caught twice in Phase 2. Holy Rosary's `sanity: 2` was read as +2 max sanity when it is horror-soak capacity, and Magnifying Glass's "+1 [intellect] while investigating" was flattened to a permanent +1 intellect, which over-applies to every other intellect test. Both were caught by the user, not by tooling.

### Verify card data against the snapshot before implementing

Before writing a card impl — or a card issue's body — confirm the card's code, name, and text against `data/arkhamdb-snapshot/pack/`. When the plan, the issue, or your recollection disagrees with the snapshot, **the snapshot wins**.

**Why:** during Phase-2 issue creation, 4 of 5 planned card codes were wrong — 01054 was Leo De Luca rather than Holy Rosary, 01045 was Burglary rather than Hyperawareness, 01039 was Deduction rather than Working a Hunch. Each would have produced a confidently-implemented wrong card. A single grep catches all of them.

### Prefer no example to a wrong one

When citing a card by name to illustrate a pattern — in a comment, doc, issue, or PR body — verify it first, per the citation policy above. If a quick check doesn't surface a card that genuinely exemplifies the pattern, write the generic description instead. "Card-derived investigate effects" beats naming a card that turns out not to do that.

Treat card citations in existing comments and docs as unverified until checked, particularly ones an agent wrote.

**Why:** a confabulated "Magnifying Glass's *Action: Investigate*" reached both a memory file and a code comment before being caught. Wrong examples are worse than absent ones — they propagate into reviewers' mental models and become facts the project has to unlearn.

### Emit a timing point in tail position; put post-emit work on a frame

`queue_event` (and `queue_forced_triggers` beneath it) **queues** an ability — it pushes a continuation frame — and returns. It does not resolve anything, so a returned `EngineOutcome::Done` means *queued*, not *happened*. Any work a handler does after the emit is pushed **above** the abilities it just queued and therefore runs **first**.

So a call site with post-emit work arms its own resume point *before* emitting, and emits as the last thing it does: re-park a phase anchor at a new resume (`enemy_phase_end`, `upkeep_phase_end`), flag the frame it is already riding (`end_turn`'s `InvestigatorTurn { ending: true }`), or push a dedicated frame for the tail (`move_primary_effect`'s `MoveEnter`). Inspecting the returned outcome is not a substitute for any of that — `if !matches!(out, Done) { … }` and `debug_assert!(matches!(out, Done))` both pass in the ordinary single-ability case, while the ability sits unresolved on the stack.

A debug assertion on `ContinuationStack::push` backstops the class: pushing a phase anchor while a queued ability frame is on the stack panics at the push. See `docs/adr/0003-emitting-a-timing-point-queues-abilities.md`, and **Queued ability** in `GLOSSARY.md`.

**Why:** four call sites believed the emit resolved, and each carried a comment asserting a loud guard that had not existed since the effect-frame migration. The worst pushed the Upkeep phase anchor over agenda 01107's forced Ghoul movement, stranding it at the bottom of the stack — that ability never fired in a real game of The Gathering (#569).

### Insert a fn above another by matching its doc block, not its signature

Rust `///` comments attach to the *next* item, so an `Edit` whose `old_string` matches only the existing function's signature line drops the new function **between** that function's doc block and its `fn` — silently re-attaching the existing doc to the new function and leaving the existing one undocumented. Either include the whole `///` block in `old_string` and place the new function cleanly before it, or insert after an unambiguous boundary (the prior function's closing `}` plus a blank line) and then check that every `fn` still carries its own doc.

**Why:** nothing is broken, only misattributed, so `RUSTDOCFLAGS="-D warnings" cargo doc` says nothing — this is caught by eye or not at all. Review caught the same mistake twice: `drive_fast_window` inserted above `enumerate_fast_plays` (#476), and `run_mythos_draws` above `anchor_on_child_pop` (#482).

### Let an absent derive speak for itself

When a type deliberately omits a derive — `PartialEq` on `GameState`, say, because comparing large trees is expensive — don't add a comment explaining the omission. If the reason matters, it belongs in the commit message or PR description, where archaeology will find it.

**Why:** comments about code that isn't there go stale, can't be checked, and imply a positive assertion where there is only a default.

### Import types by full path; reach functions through their parent module

Six rules. The first is adopted verbatim from The Rust Programming Language ch. 7.4, "Creating Idiomatic use Paths"; the rest are house rules the book says nothing about.

> Bringing the function's parent module into scope with `use` means we have to specify the parent module when calling the function. Specifying the parent module when calling the function makes it clear that the function isn't locally defined while still minimizing repetition of the full path. […] On the other hand, when bringing in structs, enums, and other items with `use`, it's idiomatic to specify the full path.

1. **Noun/verb split.** Types — structs, enums, traits — are imported by full path and used bare: `use crate::state::Continuation;`, then `Continuation`. Functions are reached through their parent: `use crate::engine::enumerate;`, then `enumerate::options(…)`. This holds across the crate seam too: `game_core::state::GameState` used in `cards` is imported once and written bare, so the layering is stated in the `use` block rather than at every mention. **The escape hatch is a genuine same-name collision in one scope**, where both items go module-qualified (`fmt::Result` / `io::Result`) — the rule never asks you to choose between it and code that compiles. **The verb half yields where the function's own name is already in scope** — a re-export (`engine::mod.rs`'s `apply_player_action`) or a `use super::*;` test module — because `unused_qualifications` rejects the module prefix there; the bare call is correct, and this is the one place the two disagree.
2. **Placement.** Imports sit at the top of the module that uses them, and **never inside a function** — the sole exception is a trait imported solely so one function's method call resolves. A `#[cfg(test)]` module keeps its own imports, `use super::*;` included, and they belong at *the module's* top, not in each `#[test]` fn: hoist on the second use rather than letting every test re-import the same name. Check what `use super::*;` already reaches before adding one — a name the parent module imports is in scope already. (#892 found `use crate::state::CardCode;` repeated in six separate test fns of `phases.rs`'s `hand_size_tests`, and deleted 98 such re-imports across `engine/dispatch/`.)
3. **`super::` is for `use super::*;` in a `#[cfg(test)]` module, and nothing else.** That one is relative on purpose — it names *the module I am testing* without restating its path, so it survives the module moving. Every other `super::` is written as an absolute `crate::` path — not only the shapes that stack (`use super::super::Cx`) or descend (`use super::card::CardCode`), but plain `use super::Item;` too, which compiles and is still worse: `use super::Cx;` stood in 18 `engine/dispatch/` files and resolved only because `dispatch/mod.rs` happened to import `Cx` privately. An absolute path resolves without knowing how deep the reading module sits, or what its parent chose to import.
4. **`as _` for method-only trait imports.** A trait imported purely for method resolution claims no name: `use wasm_bindgen::JsCast as _;`.
5. **One `use` per base path per scope.** `use crate::engine::dispatch::{cards, combat, cursor};`, not three lines — `combat.rs` carried seven consecutive `use crate::engine::dispatch::…;` before #892. Imports fall into at most three groups, in this order, separated by one blank line and with no blank lines inside: `std`, then external crates (first-party workspace crates like `game_core` included), then `crate::`. That is `rustfmt`'s `StdExternalCrate` order — `server/src/ws.rs` is a worked example. The one forced duplicate: a `#[cfg(…)]` attribute attaches to a single `use` statement, so an import gated differently from its siblings takes its own line under the same base path (`web/src/app.rs` carries two `use crate::decision::…`).
6. **Reach an item through the module that defines it, not through a re-export** — `crate::engine::outcome::EngineOutcome`, `crate::engine::dispatch::emit::TimingEvent`, `evaluator::location_id_by_code` — unless the defining module is private and the re-export is the only way in (`crate::engine::Cx`; `engine::cx` is not `pub`). `game_core::test_support` is built on that escape: its helper submodules are private, so `test_support::test_investigator` is the one spelling. A re-export across a crate seam gets no pass — `card_dsl::dsl` is `pub`, so it is reached as `card_dsl::dsl`, never through a `game_core` re-export. (#900 found that module spelled three ways — `crate::dsl`, `game_core::dsl`, `card_dsl::dsl` — across some 140 files, all through re-exports of one `pub` module.) Two spellings of one item read as two items, and they collide the moment they meet: hoisting a function-body import in #892 produced `error[E0252]: the name OptionId is defined multiple times`, because the file already had it from `crate::engine::outcome` and the inner `use` took it from the `crate::engine` re-export.

**Ordering is `rustfmt`'s job; grouping and granularity are yours.** `reorder_imports` is on by default on stable and sorts both the statements in a group and the names inside a brace, so never hand-order either — that is also why the groups need blank lines between them: without one, `crate::` sorts above every external crate. The order *of* the groups is not something stable `rustfmt` checks, so it is yours to get right. `imports_granularity` and `group_imports`, which would automate rule 5 entirely, are nightly-only and this repo pins stable.

`unused_qualifications` is on in `[workspace.lints]` and catches the narrow redundant case — `foo::Bar` where `Bar` is already imported in that file. It is a backstop, not an enforcer: it is blind to a fully-qualified inline path the file never imported, which is the bulk of rule 1.

**Why:** rule 1's evidence is adoption, not incident (rules 2, 3, 5 and 6 carry their own, inline). The book is candid that the idiom has no deeper justification — *"There's no strong reason behind this idiom: It's just the convention that has emerged, and folks have gotten used to reading and writing Rust code this way."* That convention is exactly what is being bought: a shape every Rust reader already parses without effort, which no house style beats by being cleverer. Before #891 this repo had no written import rule at all, so each new file picked one from whichever neighbour its author read, and a reviewer had only a preference to express.
