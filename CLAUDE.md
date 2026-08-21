# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## Workflow

Work runs on the `mattpocock-skills` suite. `/ask-matt` maps the flows when it's unclear which applies. Pure questions and trivial one-liners skip all of this — use judgment.

**The flow entry points are the user's to start.** A suite skill missing from your available-skills listing is user-invocation-only — ask the user to run it and wait. The skills that *are* listed should be **actually invoked** via the Skill tool, since a skill you follow from memory is a skill you follow approximately.

**Planning a phase.** Pick the entry point by how much fog the phase has, gauged by its own **Open questions** section: `/wayfinder` when the route to the destination isn't visible yet (architectural questions unresolved), `/grill-with-docs` when it is and only ordering and scope are open. Wayfinder is slow and dense — never reach for it for a well-scoped feature.

**Gates.** Two kinds of interruption, treated differently. **Permission questions** — "want me to push?", "shall I open the PR?" — are already answered: branching, the local gauntlet, pushing, and `gh pr create` proceed uninterrupted, and merging (step 7) is the single exception. **Decision questions** are the gates, and they belong *during* the work rather than after it. Stop and put the decision to the user when:

1. **A seam is unconfirmed.** No test is written at a seam the user hasn't agreed to (the `tdd` skill's rule).
2. **A card's text or a rules question is ambiguous** — the sources disagree, or a ruling doesn't settle the case.
3. **Code contradicts a phase doc or an ADR.** Surface it rather than silently overriding (`docs/agents/domain.md`).
4. **A ticket turns out to be a different size or shape than specified** — that invalidates a breakdown the user already approved.

Every gate arrives as **options with a recommendation**, never an open question. Stopping to ask something you could have looked up in the snapshot or the rules reference is a bug in the gate, not diligence.

Not every judgement call is a gate. Where this file already **pre-decides** — the DSL-primitive threshold under Architecture, the missing-card-source rule under the citation mandates — apply the rule rather than asking.

## Commands

CI runs seven jobs (`fmt`, `clippy`, `test`, `doc`, `wasm-build`, `wasm-test`, `wasm-clippy`), all warnings-as-errors. **Before pushing, run `scripts/ci-local.sh`** — it diffs against `origin/main` and runs the subset of those seven the change can plausibly break, using CI's exact invocations and strict flags.

```sh
scripts/ci-local.sh              # the jobs this diff implicates
scripts/ci-local.sh --list       # print the plan, run nothing
scripts/ci-local.sh --all        # force the full seven-job gauntlet
scripts/ci-local.sh --base <ref> # diff against <ref> instead of origin/main
```

The posture: **local catches what the diff predicts; pushed CI is the guardrail.** Reach for `--all` when the diff is unusual enough that the mapping's assumptions may not hold — a merge with a long-lived branch, or anything whose blast radius you can't picture. A change to `.github/workflows/`, `.cargo/`, or `rust-toolchain.toml` forces the full gauntlet on its own, since those invalidate the mapping wholesale.

Two ways a local pass is weaker than a CI pass, both reported at the end of a run rather than left implicit: CI pins `trunk@0.21.14` and `wasm-pack@0.15.0` while the script takes them from `$PATH`, and if `trunk` is missing entirely the `wasm-build` job falls back to a debug `cargo build` and is flagged as degraded.

Don't skip the script and run `cargo test` by hand: it passes even when `doc`/`clippy` fail in CI, and the host `clippy` job never sees `#[cfg(target_arch = "wasm32")]` code (only `wasm-clippy` does). The scoping rule is written against the reverse-dependency closure rather than the touched paths, because `web` sits downstream of `game-core`, `protocol`, and `cards` — see the header comment in `scripts/ci-local.sh`, which is where that mapping is maintained.

The underlying invocations, if you need to run one directly:

```sh
# Match CI's strict flags
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --all -- --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown   # quick check; CI's wasm-build job actually runs `trunk build --release` (release profile + asset pipeline — can fail where the debug cargo build passes)
                            wasm-pack test --headless --firefox crates/web   # headless browser tests (6th CI job)
                            cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings   # lints wasm-only code (7th CI job)

# Single test (binary name from `cargo test` output)
cargo test -p game-core <test_fn_name>
cargo test -p cards --test play_card <test_fn_name>     # integration tests in crates/cards/tests/

# Regenerate the card corpus (only after bumping data/arkhamdb-snapshot)
cargo run -p card-data-pipeline

# Dev loop (two terminals) — hot-reload on :3000, proxying to the server
cargo run -p server                                  # API + WS on :8000
cd crates/web && trunk serve                         # WASM + hot-reload on :3000
# then open http://localhost:3000
#   Proxy config lives in crates/web/Trunk.toml (REST /games + WS /ws, the
#   latter needing a ws:// backend); a root proxy panics on trunk 0.21.x.
#
# Single-port alternative (no hot-reload; what production serves): build the
# bundle and let the server serve it on one origin —
#   cd crates/web && trunk build  &&  cargo run -p server   # open :8000
```

## Architecture

### Crate layering — strict kernel/content separation

```
card-dsl  ←  game-core   ←  cards          ←  scenarios
                ↑              ↑                  ↑
                └───────  server, web (consume both)
                └───────  card-data-pipeline (consumes card-dsl only — emits cards/src/generated/)
```

- `card-dsl` — pure data types: the effect DSL (`Ability`, `Effect`, `Trigger`, builders) and static metadata (`CardMetadata`, `CardType`, `Class`, …). No I/O, state, or engine behavior. Both `game-core` and `cards` depend on it.
- `game-core` — the **kernel**: state, action/event enums, apply loop, evaluator. No I/O, no async, compiles to `wasm32`. Never depends on `cards`, `scenarios`, or anything above it. Re-exports `card_dsl::{dsl, card_data}` at the historical `game_core::dsl` / `game_core::card_data` paths.
- `cards` — **content**: pipeline-generated corpus + hand-written `Ability` declarations.

Why the direction matters: editing the engine must not recompile 5600 lines of generated card data, and scenarios/tests must consume the engine without the corpus. If you want `game-core` to call into `cards`, you want the **card registry** (below).

### CardRegistry — the only cross-crate bridge

`game_core::card_registry` is a `OnceLock<CardRegistry>` holding two function pointers (`metadata_for: fn(&CardCode) -> Option<&'static CardMetadata>`, `abilities_for: fn(&CardCode) -> Option<Vec<Ability>>`). `cards` provides `pub const REGISTRY`; hosts install once at startup:

```rust
let _ = game_core::card_registry::install(cards::REGISTRY);
```

Engine handlers that need card data (`PlayCard`, future skill-test modifier queries) call `card_registry::current()` and reject cleanly on `None`. Tests that don't touch card data never install — most rejection paths short-circuit before the lookup. The fn pointers reference `card_dsl::{CardMetadata, Ability}` and `game_core::state::CardCode` directly (survived the `card-dsl` split, #93).

### Event-sourced state — Action → apply → ApplyResult

`apply(state: GameState, action: Action) -> ApplyResult { state, events, outcome }` is the **only** entry point that mutates state. The action log is a flat `Vec<Action>`; replaying it reproduces state bit-for-bit. Every randomness source (chaos draws, deck shuffles) is recorded as an explicit `EngineRecord` action so replay is deterministic.

**Handler contract — validate-first / mutate-second.** Every dispatch handler in `crates/game-core/src/engine/dispatch.rs`:
1. Checks every precondition; on any failure returns `EngineOutcome::Rejected { reason }` with state and events **unchanged**.
2. Mutates state and pushes events only after all validations pass.

Backstopped structurally since #161: `apply_via` (`crates/game-core/src/engine/mod.rs`) snapshots the pristine state before dispatch and restores it (state, events, and RNG position) whenever the outcome is `Rejected` — no handler, including the mutating DSL evaluator, can leak partial state past a rejection. Handlers still follow validate-first as a convention (it keeps rejection cheap and reasons precise; canonical shape: `move_action`, `investigate`, `play_card`), but mid-resolution mutations before a reject are rolled back at the apply boundary, not merely event-cleared.

`EngineOutcome` = `Done | AwaitingInput { … } | Rejected { reason }`; `AwaitingInput` round-trips via `PlayerAction::ResolveInput`.

### Hybrid card-effect DSL

`crates/card-dsl/src/dsl.rs` defines `Ability { trigger: Trigger, effect: Effect }`. Triggers: `Constant`, `OnPlay`, `OnCommit` (+ `OnEvent` / `Activated` / reaction triggers later). Effects: `GainResources`, `DiscoverClue`, `Modify`, `Seq`, `If`, `ForEach`, `ChooseOne`. The evaluator (`crates/game-core/src/engine/evaluator.rs`) walks effect trees under the same validate-first contract.

An `OnEvent` ability declares the **timing cell** its printed trigger word names (`CONTEXT.md`), and a `When` declaration is rejected on a triggering condition whose resolve step has not been migrated to the coordinator yet — migrating that condition is the fix, not retagging the card; see [ADR 0008](docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md).

**Check the declared `EventTiming` against the trigger word in the module's own verbatim card-text block, and say in prose which cell the ability resolves in and why.** Two conventions, one rule each:

- **The tag must match the quoted word** — which cell each word names is `CONTEXT.md`'s **Timing cell** entry. Every card module opens with the printed text verbatim; that block is the evidence, and reading it against the `EventTiming` twenty lines below is a named review step (`docs/agents/standards.md`). The one licensed mismatch is the rule above: a card printing *"when"* on a condition still owned by its emitting caller stays tagged `After` until that condition migrates, because declaring the cell it prints would reject the action carrying it. **No card holds that licence any more** — Barricade 01038 gave it up when #721 migrated `LeftLocation`, and Guard Dog 01021, the last holder, gave it up when #727/#722 replaced the soak condition with the assigned/placed pair (ADR 0009). The mechanism survives for the caller-owned conditions that remain, but a mismatch in the corpus is now simply a bug.
- **The module's prose names the cell.** Write *"the `at` cell of the `EnemyDefeated` condition"*, with the reason, in the paragraph under the quoted text — not a description of the engine window the ability happens to ride. A contradiction between *"the after cell"* and quoted text reading *"When an enemy attack deals damage"* is one a reviewer cannot miss; a bare enum far below it is one they demonstrably can. **The form is a bold inline lead-in opening *"Cell: …"*, not a `# Cell` rustdoc heading**, so that the paragraph sits in the header's flow next to the quoted text rather than opening a section a reader can scroll past. A module declaring two cells writes two such paragraphs, each naming which ability it is for (`the_barrier`, `cover_up`).

**There is no automated check, and rebuilding one is not the fix.** The trigger word is not mechanically derivable from the printed text: *"if … would …"* is when-tier while a bare *"if"* on a settled state is at-tier, because *"would"* outranks the leading word. That tiering is a conclusion drawn from reading `glossary/Instead.md` against `glossary/At.md`, not a clause either states — `CONTEXT.md` carries the reasoning and says whose conclusion it is. A parser would encode one reading of the rules and then be trusted as though it had checked them. The six mis-tags #694 audited were not a gap in the evidence — every one of those modules quoted its own trigger word directly above the wrong enum. The failure was an unassigned reading, which is what the standards row assigns.

Cards are **Rust source** (typed, compiler-checked), not JSON: each is a module `crates/cards/src/impls/<name>.rs` exposing `CODE: &str` and `abilities() -> Vec<Ability>`. Cards needing primitives the DSL lacks get a Rust impl. **Don't add DSL primitives speculatively** — wait until two or more hand-written cards want the same pattern.

A card is **playable** iff it has an `abilities()` impl (`cards::is_playable(code)`); unimplemented cards appear in deckbuilding but are refused by the deck-import gate (Phase 9). `PlayCard` on an unimplemented card rejects loudly — never silently no-op. On play: assets land in `cards_in_play` and stay (their `Trigger::Constant` abilities contribute via the registry while in play); events run their `OnPlay` effects then move to `discard` (emit `CardDiscarded { from: Zone::Hand, … }`). Every other `CardType` rejects.

### Test layering (in order of importance)

1. **Card tests** — per-card in `crates/cards/src/impls/<name>.rs`; each card needs at least one.
2. **Engine unit tests** — `crates/game-core/src/engine/mod.rs` + per-module `#[cfg(test)]`. Use the `TestGame` builder (`.with_phase(…).with_investigator(…).with_active_investigator(…).build()`, with `test_investigator(id)` / `test_location(id, name)` / `test_enemy(id, name)` fixtures) and the **event-assertion macros** `assert_event!` / `assert_no_event!` / `assert_event_count!` / `assert_event_sequence!` (order-insensitive by default; `_sequence` for in-order subsequence). Use `assert_eq!` on the events slice only when you need exact contiguous order.
3. **Integration tests** — `crates/cards/tests/`; each file is its own cargo binary/process, so it can `install(cards::REGISTRY)` without colliding. The right home for anything needing real card metadata + abilities (`game-core` can't reach the corpus by crate direction). Pattern: `crates/cards/tests/play_card.rs`.

`game-core::test_support` is unconditionally `pub` (no feature flag).

### Card-data pipeline

`data/arkhamdb-snapshot/` is a manually-pinned subset of upstream `Kamalisk/arkhamdb-json-data`. **Never auto-sync** — a malformed upstream entry can't surprise the build.

**Snapshot ≠ corpus** — both are glossary entries in `CONTEXT.md`, and the difference is load-bearing. The **snapshot** is all of Chapter 1 (everything upstream except the Chapter 2 cycles `core_ch2` / `investigator_decks_ch2`), vendored as *planning input* so DSL and engine decisions can be made against the full set of cards we'll eventually support. The **corpus** is the subset `PACK_FILES` ingests and the build compiles — Core + Dunwich only. A card being in the snapshot means nothing at runtime; `cards::metadata_for` answers only for the corpus.

Every vendored pack file is sorted into `PACK_FILES` / `REFERENCE_FILES` / `OUT_OF_SCOPE_FILES`, and `classify` (`crates/card-data-pipeline/src/main.rs`) fails on any file in none of them *and* on any in-scope pack from `packs.json` with no vendored file. It runs both in the pipeline and as a test, so CI catches a mis-vendored bump. See `data/arkhamdb-snapshot/SOURCE.md`.

Making a pack playable: (1) move its files from `REFERENCE_FILES` to `PACK_FILES` (bumping the snapshot first only if you need fresher data), (2) `cargo run -p card-data-pipeline` regenerates `crates/cards/src/generated/cards.rs` (emitting unplayable stubs for cards without impls), (3) replace stubs with DSL/Rust impls, (4) write tests. **Never hand-edit `cards.rs`** (generated; carries a header comment).

### Domain knowledge that's load-bearing but not visible in the code

Arkham terminology is defined in [`CONTEXT.md`](CONTEXT.md) — **read it before naming a domain concept**, in code, tests, issue titles, or chat. Every term in it has already caused a mistake in PR review; the glossary is the single home for them, so don't restate definitions here.

Three mechanics that aren't glossary entries but shape engine behavior:

- **Skill-test totals clamp at 0; AutoFail forces total to 0.** Same numeric outcome, different `FailureReason` — some card effects key off which fired.
- **Horror soak isn't modeled by the DSL yet** — tracked in #44.
- **The pipeline translates ArkhamDB's `faction` to `Class` at ingestion.**

**The sources for card text and for rules are vendored in-repo. Read them locally — never fetch, and never paraphrase from memory.** A summarising fetch cannot be quoted verbatim: `WebFetch` converts a page to markdown and then answers a prompt against it with a small fast model, so what comes back is a summary, and a summarised rules clause is a wrong rules clause. Everything below assumes you open a file and read all of it.

**Whenever you reference or quote a card's text or effect — in code, comments, commit messages, PR descriptions, or chat — you MUST look it up first** (memory is unreliable; PR review has caught real divergences — renamed traits, off-by-one stats, dropped sub-clauses). Two files, both local:

- **Printed text and metadata: the pinned snapshot, `data/arkhamdb-snapshot/pack/*/`.** It is what the build compiles from, so it is authoritative for corpus membership and pipeline-ingested metadata alike.
- **Rulings: `data/arkhamdb-faq/<pack>/<code>.md`. ALWAYS read these too, not just the printed text** — rulings are load-bearing and routinely *not* derivable from the text alone (e.g. Mind over Matter 01036: substituting makes it an Intellect *test*, so intellect icons/bonuses apply, and the choice is made before the test begins). **A card with no file has no rulings** — `data/arkhamdb-faq/no-rulings.txt` records that explicitly, so absence is an answer rather than a gap. Cite a ruling as `https://arkhamdb.com/card/<code>`, the page it came from.

Copy text verbatim where it appears in a quote.

When implementing or citing **rules behavior** — ability timing, trigger windows, framework events, skill-test resolution, action structure, anything procedural — verify before asserting; don't trust memory. **Read `data/rules-reference/rules/`**, ArkhamDB's rules reference ingested verbatim: the printed Rules Reference plus the rules added by deluxe expansions and by the official FAQ, one small file per section and per glossary entry, indexed by [`data/rules-reference/rules/README.md`](data/rules-reference/rules/README.md). Filenames are ArkhamDB anchor ids, so an old reference to `#Skill_Test_Timing` names its own file. Read the whole entry — every file is small enough to. Quote the load-bearing clause verbatim in PR descriptions and engine doc-comments where the rule shapes behavior; elision is fine for decorative surrounding clauses, but never substitute words. The vendored PDF (`data/rules-reference/ahc01_rules_reference_web.pdf`) is the pinned publisher original, retained for provenance only — it is the 2016 Core Set edition and predates most of Chapter 1, so don't read it. Lower-tier mirrors (Rulepop, fan wikis) lag and sometimes disagree — avoid them. When the user asks for a rules-based judgment call, the citation belongs in the answer.

**If the answer genuinely isn't in the vendored text, stop and say so.** Don't fetch it, and don't reconstruct it. A real gap means the data is stale or the question was misunderstood, and both want the maintainer's attention — a network fetch papers over exactly the problem the vendoring solves. Provenance and refresh procedures: [`data/rules-reference/SOURCE.md`](data/rules-reference/SOURCE.md) and [`data/arkhamdb-faq/SOURCE.md`](data/arkhamdb-faq/SOURCE.md).

## Phase plan, milestones, and PR procedure

Work is tracked against GitHub milestones (`phase-0-foundations` → `phase-10-dunwich-and-iteration`). Each phase has a plan doc at **`docs/phases/phase-N-<slug>.md`** (ordered work, status, open questions) — read the relevant one when picking up an issue; `docs/phases/README.md` indexes the arc and unmilestoned work. Design decisions live in `docs/adr/`, not in the phase docs. Issues carry priority (`p0-blocker` / `p1-next` / `p2-later`) and category (`engine` / `card` / `scenario` / `infra` / `test`) labels. PRs squash-merge; commit subjects follow `scope: description` (e.g. `engine: cards-registry binding via static OnceLock`); the PR template's `Closes #` line auto-closes the issue. **Every PR closes at least one issue** — file it first, including for infrastructure and bootstrap work, so the tracker stays a trustworthy record of what's been done.

Follow this order for every non-trivial PR — skipping steps has cost real iterations. The **gates** under Workflow interrupt this order wherever they fire: resolve the gate, then resume.

1. **Run `scripts/ci-local.sh` before pushing** (see Commands). It runs the subset of the seven CI jobs the diff can break, with CI's strict flags; `--all` forces the full gauntlet when the diff is unusual. Plain `cargo test` passes even when `doc`/`clippy` fail in CI; the `doc` job has caught broken intra-doc links local runs miss.
2. **Commit and push** to a feature branch `<scope>/<short-slug>` (`<scope>` matches the commit scope; slug is a 2–4-word hyphenated descriptor, e.g. `engine/play-card`). One branch per issue. Commit body explains the *why* and ends with `Closes #NN.`
3. **Open the PR** with `gh pr create` using the repo template; include a brief design-decisions paragraph for any non-obvious choice.
4. **Watch CI** via `gh pr checks <PR#> --watch` (background). Code review for routine PRs happens **before push** — `/implement` closes out by running `code-review` — so skip the post-push `review-agent` then. Reserve a post-push review for: PRs prepared without a pre-push review, an explicit request for a second look, or escalation skills (`/security-review` for sensitive areas, `/ultrareview` at milestone exits) — all user-triggered.

   Whenever a review reports back — pre-push, post-push, or an escalation — **surface its findings to the user in full, in the reviewer's own severity buckets**, and only then say which you're actioning and which you're skipping, with your reasoning where it differs. A finding you disagree with is still signal for the merge decision, and the file:line citations and rationale are what the user reads a review for; a condensed verdict throws away both. Even a clean approval gets its reasoning surfaced.
5. **Fix CI failures with follow-up commits to the same branch** — don't amend/force-push unless asked.
6. **Update the relevant `docs/phases/phase-N-<slug>.md` once the PR is ready to merge, and ONLY then** — as the final commit, so it reflects the actually-shipping state (PR # known, review fixes folded in). Open the PR with code only and **push the doc commit only once CI is green on the PR** — if CI fails, the fixes land first and the doc then describes what actually ships rather than what CI just rejected; the doc commit triggers its own quick re-run. Never put phase-doc edits in earlier commits (churn + drift). Move the closing issue to the **Closed** table (bump counts), flip the Ordering/Arc row to `✅ PR #N`, and remove any **Open question** the PR settled. **Design decisions no longer live in the phase doc** — a load-bearing choice becomes an ADR under `docs/adr/` in the same commit. Most PRs need none. **`docs/phases/README.md` ("Maintaining these docs") is the authoritative spec for this step, including the three-part test an ADR must pass.**
7. **Merge only after explicit user approval**, via `gh pr merge <PR#> --squash --delete-branch`. Confirm the issue auto-closed and `git pull` on `main`.

## Agent skills

**Every dispatch prompt for a write-capable subagent says the subagent does all the work itself and delegates to no subagent of its own.** The prompt is the only lever: the default `general-purpose` type carries the Agent tool, and the built-in types that exclude it (`Explore`, `Plan`) are read-only, so none can implement. Without the line, an implementer dispatch recursed roughly six levels deep in PR #460.

### Issue tracker

GitHub Issues on `talelburg/eldritch`, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage roles, each label string equal to its name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — [`CONTEXT.md`](CONTEXT.md) (the domain glossary) and `docs/adr/` at the repo root. **Read `CONTEXT.md` before naming a domain concept**, and check `docs/adr/` before working in an area it touches. See `docs/agents/domain.md`.

### Product decisions

[`docs/product-decisions.md`](docs/product-decisions.md) — the standing product, legal, and hosting posture: audience and scale, licensing and card art, the "Eldritch" naming constraint, hosting and invite-only auth, deck import instead of a deckbuilder, and the frontend's exit ramp. Read it before proposing anything that changes what the project *is* to its players — its scope, how they get in, how decks arrive, or what it depends on from third parties.

### Coding standards

`docs/agents/standards.md` — the index of how code is written here. It points at the standards documented in this file and defines the ones with no other home. It is what `code-review`'s Standards axis reads.

### Documentation standards

`docs/agents/writing.md` — the house style for the docs an agent reads. Read it before **adding a rule to this file** (there is an admission bar, and the PR names which route the rule passes) or **writing an ADR** (this repo overrides the skill suite's `ADR-FORMAT.md` on size, and `writing.md` is the spec that wins).
