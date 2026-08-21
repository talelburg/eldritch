# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository. This file is the index plus the rules you would break before knowing to look them up; everything else is one hop away and each pointer below says what you'd get wrong by not taking it.

## Workflow

Work runs on the `mattpocock-skills` suite. `/ask-matt` maps the flows when it's unclear which applies. Pure questions and trivial one-liners skip all of this — use judgment.

**The flow entry points are the user's to start.** A suite skill missing from your available-skills listing is user-invocation-only — ask the user to run it and wait ([ADR 0001](docs/adr/0001-workflow-runs-on-mattpocock-skills.md) for why the boundary sits there). The skills that *are* listed should be **actually invoked** via the Skill tool, since a skill you follow from memory is a skill you follow approximately.

**Planning a phase.** Pick the entry point by how much fog the phase has, gauged by its own **Open questions** section: `/wayfinder` when the route to the destination isn't visible yet (architectural questions unresolved), `/grill-with-docs` when it is and only ordering and scope are open. Wayfinder is slow and dense — never reach for it for a well-scoped feature.

**Gates.** Two kinds of interruption, treated differently. **Permission questions** — "want me to push?", "shall I open the PR?" — are already answered: branching, the local gauntlet, pushing, and `gh pr create` proceed uninterrupted, and merging (step 7) is the single exception. **Decision questions** are the gates, and they belong *during* the work rather than after it. Stop and put the decision to the user when:

1. **A seam is unconfirmed.** No test is written at a seam the user hasn't agreed to (the `tdd` skill's rule).
2. **A card's text or a rules question is ambiguous** — the sources disagree, or a ruling doesn't settle the case.
3. **Code contradicts a phase doc or an ADR.** Surface it rather than silently overriding (`docs/agents/domain.md`).
4. **A ticket turns out to be a different size or shape than specified** — that invalidates a breakdown the user already approved.

Every gate arrives as **options with a recommendation**, never an open question. Stopping to ask something you could have looked up in the snapshot or the rules reference is a bug in the gate, not diligence.

Not every judgement call is a gate. Where a rule is already **pre-decided** — the DSL-primitive threshold in `docs/agents/standards.md`, the missing-card-source rule under the citation mandate below — apply the rule rather than asking.

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

# Dev loop (two terminals) — hot-reload on :3000, proxying to the server
cargo run -p server                                  # API + WS on :8000
cd crates/web && trunk serve                         # WASM + hot-reload on :3000
# then open http://localhost:3000 — proxy config is in crates/web/Trunk.toml
#
# Single-port alternative (no hot-reload; what production serves):
#   cd crates/web && trunk build  &&  cargo run -p server   # open :8000
```

## Architecture

**The crate layering is strict and one-directional: `card-dsl ← game-core ← cards ← scenarios`.** `game-core` is the kernel and never depends on `cards`, `scenarios`, or anything above it; the only cross-crate bridge is `game_core::card_registry`, a `OnceLock` of function pointers that hosts install at startup. Reach the other way and you'll write code that compiles locally and inverts the layering the whole build is arranged around. Everything else about how the pieces fit — the registry, the event-sourced `Action → apply → ApplyResult` loop and its validate-first handler contract, the card-effect DSL, and the card-data pipeline — is in [`docs/agents/architecture.md`](docs/agents/architecture.md). Read it before touching the engine or adding a card; the rules an author follows while doing so are in [`docs/agents/standards.md`](docs/agents/standards.md).

## Cite card text and rules from the vendored sources

**The sources for card text and for rules are vendored in-repo. Read them locally — never fetch, and never paraphrase from memory.** A summarising fetch cannot be quoted verbatim: `WebFetch` converts a page to markdown and then answers a prompt against it with a small fast model, so what comes back is a summary, and a summarised rules clause is a wrong rules clause. Everything below assumes you open a file and read all of it.

**Whenever you reference or quote a card's text or effect — in code, comments, commit messages, PR descriptions, or chat — you MUST look it up first** (memory is unreliable; PR review has caught real divergences — renamed traits, off-by-one stats, dropped sub-clauses). Two files, both local:

- **Printed text and metadata: the pinned snapshot, `data/arkhamdb-snapshot/pack/*/`.** It is what the build compiles from, so it is authoritative for corpus membership and pipeline-ingested metadata alike.
- **Rulings: `data/arkhamdb-faq/<pack>/<code>.md`. ALWAYS read these too, not just the printed text** — rulings are load-bearing and routinely *not* derivable from the text alone (e.g. Mind over Matter 01036: substituting makes it an Intellect *test*, so intellect icons/bonuses apply, and the choice is made before the test begins). **A card with no file has no rulings** — `data/arkhamdb-faq/no-rulings.txt` records that explicitly, so absence is an answer rather than a gap. Cite a ruling as `https://arkhamdb.com/card/<code>`, the page it came from.

Copy text verbatim where it appears in a quote.

When implementing or citing **rules behavior** — ability timing, trigger windows, framework events, skill-test resolution, action structure, anything procedural — verify before asserting; don't trust memory. **Read `data/rules-reference/rules/`**, ArkhamDB's rules reference ingested verbatim: the printed Rules Reference plus the rules added by deluxe expansions and by the official FAQ, one small file per section and per glossary entry, indexed by [`data/rules-reference/rules/README.md`](data/rules-reference/rules/README.md). Filenames are ArkhamDB anchor ids, so an old reference to `#Skill_Test_Timing` names its own file. Read the whole entry — every file is small enough to. Quote the load-bearing clause verbatim in PR descriptions and engine doc-comments where the rule shapes behavior; elision is fine for decorative surrounding clauses, but never substitute words. Lower-tier mirrors (Rulepop, fan wikis) lag and sometimes disagree — avoid them, and see **Rules text** in `CONTEXT.md` for why the vendored PDF is provenance only. When the user asks for a rules-based judgment call, the citation belongs in the answer.

**If the answer genuinely isn't in the vendored text, stop and say so.** Don't fetch it, and don't reconstruct it. A real gap means the data is stale or the question was misunderstood, and both want the maintainer's attention — a network fetch papers over exactly the problem the vendoring solves. Provenance and refresh procedures: [`data/rules-reference/SOURCE.md`](data/rules-reference/SOURCE.md) and [`data/arkhamdb-faq/SOURCE.md`](data/arkhamdb-faq/SOURCE.md).

## Phase plan, milestones, and PR procedure

Work is tracked against GitHub milestones (`phase-0-foundations` → `phase-10-dunwich-and-iteration`). Each phase has a plan doc at **`docs/phases/phase-N-<slug>.md`** (ordered work, status, open questions) — read the relevant one when picking up an issue; `docs/phases/README.md` indexes the arc and unmilestoned work. Design decisions live in `docs/adr/`, not in the phase docs. PRs squash-merge; commit subjects follow `scope: description` (e.g. `engine: cards-registry binding via static OnceLock`); the PR template's `Closes #` line auto-closes the issue. **Every PR closes at least one issue** — file it first, including for infrastructure and bootstrap work, so the tracker stays a trustworthy record of what's been done.

Follow this order for every non-trivial PR — skipping steps has cost real iterations. The **gates** under Workflow interrupt this order wherever they fire: resolve the gate, then resume.

1. **Run `scripts/ci-local.sh` before pushing** (see Commands, which says why `cargo test` alone isn't enough).
2. **Commit and push** to a feature branch `<scope>/<short-slug>` (`<scope>` matches the commit scope; slug is a 2–4-word hyphenated descriptor, e.g. `engine/play-card`). One branch per issue. Commit body explains the *why* and ends with `Closes #NN.`
3. **Open the PR** with `gh pr create` using the repo template; include a brief design-decisions paragraph for any non-obvious choice.
4. **Watch CI** via `gh pr checks <PR#> --watch` (background). Code review for routine PRs happens **before push** — `/implement` closes out by running `code-review` — so skip the post-push `review-agent` then. Reserve a post-push review for: PRs prepared without a pre-push review, an explicit request for a second look, or escalation skills (`/security-review` for sensitive areas, `/ultrareview` at milestone exits) — all user-triggered.

   Whenever a review reports back — pre-push, post-push, or an escalation — **surface its findings to the user in full, in the reviewer's own severity buckets**, and only then say which you're actioning and which you're skipping, with your reasoning where it differs. A finding you disagree with is still signal for the merge decision, and the file:line citations and rationale are what the user reads a review for; a condensed verdict throws away both. Even a clean approval gets its reasoning surfaced.
5. **Fix CI failures with follow-up commits to the same branch** — don't amend/force-push unless asked.
6. **Update the relevant `docs/phases/phase-N-<slug>.md` as the final commit, and only once CI is green on the PR.** Open the PR with code only; if CI fails, the fixes land first so the doc describes what actually ships rather than what CI just rejected, and the doc commit triggers its own quick re-run. Never put phase-doc edits in earlier commits (churn + drift). **What that commit contains — and the three-part test a load-bearing choice must pass to become an ADR instead — is specified by [`docs/phases/README.md`](docs/phases/README.md), "Maintaining these docs".**
7. **Merge only after explicit user approval**, via `gh pr merge <PR#> --squash --delete-branch`. Confirm the issue auto-closed and `git pull` on `main`.

## Agent skills

**Every dispatch prompt for a write-capable subagent says the subagent does all the work itself and delegates to no subagent of its own.** The prompt is the only lever: the default `general-purpose` type carries the Agent tool, and the built-in types that exclude it (`Explore`, `Plan`) are read-only, so none can implement. Without the line, an implementer dispatch recursed roughly six levels deep in PR #460.

- **`docs/agents/standards.md`** — how code is written here, and what `code-review`'s Standards axis reads. It carries the rules a card or engine PR is reviewed against, including ones no compiler checks.
- **`docs/agents/writing.md`** — the house style for the docs an agent reads. Read it before **adding a rule to this file** (there is an admission bar, and the PR names which route the rule passes) or **writing an ADR** (this repo overrides the skill suite's `ADR-FORMAT.md` on size, and `writing.md` is the spec that wins).
- **[`CONTEXT.md`](CONTEXT.md)** — the domain glossary, plus `docs/adr/` for decisions. **Read it before naming a domain concept**, in code, tests, issue titles, or chat: every term in it has already caused a mistake in PR review, and it is the single home for them. Check `docs/adr/` before working in an area it touches. See `docs/agents/domain.md`.
- **[`docs/product-decisions.md`](docs/product-decisions.md)** — the standing product, legal, and hosting posture. Read it before proposing anything that changes what the project *is* to its players — its scope, how they get in, how decks arrive, or what it depends on from third parties.
- **`docs/agents/issue-tracker.md`** (GitHub Issues on `talelburg/eldritch` via the `gh` CLI, and the repo's label taxonomy) and **`docs/agents/triage-labels.md`** (the five canonical triage roles, each label string equal to its name).
