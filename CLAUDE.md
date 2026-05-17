# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

CI runs five jobs (`fmt`, `clippy`, `test`, `doc`, `wasm-build`) all with warnings as errors. Match the CI flags locally before pushing — `cargo test` alone won't catch broken intra-doc links or clippy lints CI fails on.

```sh
# Match CI's strict flags
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown

# Single test (use the binary name from `cargo test` output)
cargo test -p game-core <test_fn_name>
cargo test -p cards --test play_card <test_fn_name>     # integration tests in crates/cards/tests/

# Regenerate the card corpus from the pinned snapshot (only after bumping data/arkhamdb-snapshot)
cargo run -p card-data-pipeline

# Dev loop (two terminals)
cargo run -p server                                       # Axum on :8000
cd crates/web && trunk serve --proxy-backend=http://localhost:8000   # WASM on :3000
```

## Architecture

### Crate layering — strict kernel/content separation

```
game-core   ←  cards          ←  scenarios
     ↑           ↑                  ↑
     └───  server, web (consume both)
     └───  card-data-pipeline (consumes game-core only — emits cards/src/generated/)
```

`game-core` is the **kernel**: state, action enum, event enum, apply loop, DSL types, evaluator. No I/O, no async, compiles to `wasm32`. Never depends on `cards`, `scenarios`, or anything above it. `cards` is **content** built atop the kernel.

Why the direction matters: editing the engine must not trigger a recompile of 5600 lines of generated card data. Scenarios and tests must be able to consume the engine without the full corpus. If you find yourself wanting `game-core` to call into `cards` directly, you want the **card registry** (below).

### CardRegistry — the only cross-crate bridge

`game_core::card_registry` is a `OnceLock<CardRegistry>` holding two function pointers (`metadata_for: fn(&CardCode) -> Option<&'static CardMetadata>` and `abilities_for: fn(&CardCode) -> Option<Vec<Ability>>`). The `cards` crate provides `pub const REGISTRY: CardRegistry` wrapping its own `by_code` / `abilities_for`. Hosts install it once at startup:

```rust
let _ = game_core::card_registry::install(cards::REGISTRY);
```

Engine handlers that need card data (`PlayCard`, future constant-modifier queries during skill tests) call `card_registry::current()` and reject cleanly on `None`. Tests that don't touch card data never install — most rejection paths short-circuit before the registry lookup.

A future `card-dsl` crate split is tracked in #93; the registry survives that refactor unchanged.

### Event-sourced state — Action → apply → ApplyResult

`apply(state: GameState, action: Action) -> ApplyResult { state, events, outcome }` is the **only** entry point that mutates state. The action log is a flat `Vec<Action>`; replaying it from initial state reproduces the current state bit-for-bit. Every randomness source (chaos token draws, deck shuffles) is recorded as an explicit `EngineRecord` action so replay is deterministic.

**Handler contract:** every dispatch handler in `crates/game-core/src/engine/dispatch.rs` follows **validate-first / mutate-second**:

1. Check every precondition. If any fails, return `EngineOutcome::Rejected { reason }` with state and events **unchanged** from input.
2. Only after all validations pass, mutate state and push events.

The apply loop has a belt-and-suspenders `events.clear()` on `Rejected` — but handlers should never push then bail. Read `move_action`, `investigate`, or `play_card` for the canonical shape (long if-chains of validations, then a single mutation block).

Note this is enforced **by convention, not yet structurally** — the `apply()` doc tracks a TODO to refactor to a structural two-phase shape. `play_card` itself has a documented caveat: it emits `Event::CardPlayed` and runs on-play effects *before* removing the card from hand, so if a future on-play effect rejects mid-resolution it leaves partial state. Safe for the Phase-3 on-play effects in scope (`DiscoverClue`, `GainResources`) because they can't reject after the standard prefix passes; broader hardening deferred.

`EngineOutcome` is `Done | AwaitingInput { ... } | Rejected { reason }`. `AwaitingInput` round-trips via `PlayerAction::ResolveInput`; the ChoiceResolver plumbing for this lands in #19.

### Hybrid card-effect DSL

`crates/game-core/src/dsl.rs` defines the DSL: `Ability { trigger: Trigger, effect: Effect }`. Triggers: `Constant`, `OnPlay`, `OnCommit` (with `OnEvent` / `Activated` / reaction triggers landing in later issues). Effects: `GainResources`, `DiscoverClue`, `Modify`, `Seq`, `If`, `ForEach`, `ChooseOne`. The evaluator (`crates/game-core/src/engine/evaluator.rs`) walks effect trees and mutates state, with the same validate-first contract.

Cards are declared in **Rust source files** (typed, compiler-checked), not JSON. Each card has a module in `crates/cards/src/impls/<name>.rs` exposing a `CODE: &str` and an `abilities() -> Vec<Ability>` function. The DSL handles common patterns; cards needing primitives the DSL doesn't yet support get a Rust trait impl until the DSL grows the relevant verbs. **Do not add DSL primitives speculatively** — wait until two or more hand-written cards want the same pattern.

A card is **playable** iff it has an `abilities()` implementation (`cards::is_playable(code)`). Cards in the corpus without one appear in deckbuilding tools but are refused by the deck-import gate (Phase 9). When asked to play an unimplemented card from hand, `PlayCard` rejects loudly — never silently no-op.

When a card *is* played: assets land in `cards_in_play` and stay there (their `Trigger::Constant` abilities contribute via the registry while in play); events resolve their `Trigger::OnPlay` effects then move to `discard`, emitting `Event::CardDiscarded { from: Zone::Hand, … }`. Every other `CardType` rejects.

### Test layering

Three layers, in this order of importance:

1. **Card tests** (per-card, in `crates/cards/src/impls/<name>.rs`). Each card needs at least one test.
2. **Engine unit tests** in `crates/game-core/src/engine/mod.rs` and per-module `#[cfg(test)]` blocks. Use the `TestGame` builder (`game-core/src/test_support/`) — fluent `.with_phase(...).with_investigator(...).with_active_investigator(...).build()` shape with `test_investigator(id)` / `test_location(id, name)` / `test_enemy(id, name)` fixtures. **Use the event-assertion macros**: `assert_event!`, `assert_no_event!`, `assert_event_count!`, `assert_event_sequence!` — order-insensitive by default; the `_sequence` variant for subsequence-in-order checks. Use `assert_eq!` on the events slice only when you need exact contiguous order.
3. **Integration tests in `crates/cards/tests/`**. Each file is a separate cargo binary, gets its own process, so it can `install(cards::REGISTRY)` without colliding with other test runs. This is the right home for any test that needs real card metadata + abilities — `game-core` itself can't reach the corpus by crate-dependency direction. See `crates/cards/tests/play_card.rs` for the pattern.

`game-core::test_support` is unconditionally `pub` — integration tests in `tests/*.rs` and downstream crates (e.g. `cards`) use it without any feature flag.

### Card-data pipeline

`data/arkhamdb-snapshot/` is a manually-pinned subset of upstream `Kamalisk/arkhamdb-json-data`. **Never auto-sync** — a malformed upstream entry can't surprise the build. Scope is original Core + Dunwich Legacy only (old-format files); the user plays the gameplay-equivalent revised products physically. See `data/arkhamdb-snapshot/SOURCE.md` for the full inclusion/exclusion list.

Workflow for adding a card pack: (1) bump the pinned snapshot, (2) `cargo run -p card-data-pipeline` regenerates `crates/cards/src/generated/cards.rs` (and emits unplayable stubs for cards without effect implementations), (3) replace stubs with DSL or Rust impls, (4) write tests. The pipeline emits a header comment marking the file as generated — never hand-edit `cards.rs`.

### Domain knowledge that's load-bearing but not visible in the code

Several Arkham mechanics have non-obvious shapes that have already caused mistakes in PR review. Key ones:

- **Horror soak ≠ max-sanity boost.** Asset cards with `sanity: N` (Holy Rosary, Beat Cop) are horror-soak containers, not stat modifiers. Not modeled by the DSL yet — tracked in #44.
- **Only Asset and Event are playable from hand.** Skills are *committed* to skill tests via a separate flow. Investigator cards represent the player character, never enter hand. Everything else (Treachery, Enemy, Location, Agenda, Act, Scenario, Story) is scenario-bag content. `PlayCard`'s dispatch only needs two playable arms.
- **Skill-test totals clamp at 0, AutoFail forces total to 0.** Same numeric outcome, different `FailureReason`. Some card effects key off which one fired.
- **"Fast" is a play-cost concern, not a DSL concern.** `Trigger::Activated { action_cost: 0 }` is a different "fast." Both exist; don't conflate.
- **ArkhamDB calls factions "factions"; the rulebook calls them "classes."** The pipeline translates at ingestion; internally we use `Class`.

When citing card text or behavior, verify against `data/arkhamdb-snapshot/pack/*/` before asserting — memory of card text isn't reliable.

### Phase plan and milestones

Work is tracked against GitHub milestones (`phase-0-foundations` → `phase-10-dunwich-and-iteration`). Each phase has a plan doc at **`docs/phases/phase-N-<slug>.md`** capturing the ordered work, design decisions made along the way, and open questions — read the relevant one when picking up a new issue. The index at `docs/phases/README.md` covers the full arc and the unmilestoned cross-cutting work. Issues are labeled by priority (`p0-blocker` / `p1-next` / `p2-later`) and category (`engine` / `card` / `scenario` / `infra` / `test`). The PR template's `Closes #` line auto-closes the issue on merge.

PRs use squash-merge; commit subject convention is `scope: description` (e.g. `engine: cards-registry binding via static OnceLock`).

### PR procedure

Follow this order for every non-trivial PR. Skipping steps has cost real iterations:

1. **Run the full CI-equivalent gauntlet locally before pushing.** All five jobs, with the same strict flags CI uses:
   - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo fmt --check`
   - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
   - `cargo build -p web --target wasm32-unknown-unknown`

   Plain `cargo test` will pass even when `doc` or `clippy` fail in CI — `-D warnings` and intra-doc-link checks only fire under the strict flags. The `doc` job in particular has caught broken intra-doc links to `#[macro_export]`-ed items that local test runs miss.

2. **Commit and push** to a feature branch named `<scope>/<short-slug>`, where `<scope>` matches the commit-message scope (`engine`, `cards`, `infra`, `test`, `scenario`) and `<short-slug>` is a hyphenated 2–4-word descriptor of the work (e.g. `engine/cards-registry`, `engine/play-card`, `infra/dependabot-auto-merge`). One branch per issue. Commit message follows `scope: description` with a body explaining the *why* and a `Closes #NN.` line.

3. **Open the PR** with `gh pr create` using the repo template. Include a brief design-decisions paragraph if any non-obvious choice was made.

4. **Watch CI in the background and spawn the review-agent in parallel.** Run `gh pr checks <PR#> --watch` as a background task and concurrently invoke the `review-agent` subagent with the PR number, branch, and context (the design decisions, what to review, what's intentionally out of scope). They run concurrently — don't block on CI before asking for review.

5. **Always present the review-agent's findings to the user, including when CI fails.** A failing CI doesn't make the review irrelevant; the review may flag the same issue or unrelated issues that need attention regardless. Surface the review summary in user-facing text before asking for the merge decision. Action review feedback that's clearly correct and within scope; for anything where the call isn't obvious (trade-offs, scope decisions, "is this worth doing now or as a follow-up"), surface it to the user and ask rather than deciding silently.

6. **Address CI failures by pushing follow-up commits to the same branch.** Don't amend / force-push unless the user asks. CI re-runs automatically; the second watch can usually be foregrounded since it's only one job re-running.

7. **Update the relevant `docs/phases/phase-N-<slug>.md` once the PR is ready to merge.** Land it as a final commit on the same branch so the doc reflects what actually ships — including review-driven fixes, scope changes, or new decisions surfaced during review. Skipping this step has cost real iterations; the next person hits a stale doc and re-derives context. Specifically:
   - Move the closing issue from the **Open** table to the **Closed** table; bump the closed/open counts in the Status section.
   - Flip the Ordering / Arc table's row to `✅ PR #N`.
   - Add an entry to **Decisions made** ONLY for choices that are load-bearing for future PRs — design-shape choices that constrain later work, intentional divergences from the issue body, significant scope splits, or review-surfaced rephrasings that shift the meaning. Skip routine field additions, mirroring of existing patterns, internal trade-offs settled within the PR, and anything a future contributor could grep from the code. If a future PR-author wouldn't need to know this to make their next decision, leave it out. Decisions entries are not a changelog; they're a context-saver for the next person. Include the PR number on each kept entry.
   - Remove any **Open question** the PR settled.

   The `docs/phases/README.md` "Maintaining these docs" section is the authoritative spec; this step is the enforcement.

8. **Merge only after explicit user approval**, via `gh pr merge <PR#> --squash --delete-branch`. Confirm the issue auto-closed and `git pull` on `main` to sync.
