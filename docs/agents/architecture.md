# Architecture

How this repo is put together: what each crate may depend on, how state changes, how a card becomes behavior, and how the corpus is built. Orientation, not a coding standard — the rules an author follows are in [`standards.md`](standards.md).

## Crate layering — strict kernel/content separation

```
card-dsl  ←  game-core   ←  cards          ←  scenarios
                ↑              ↑                  ↑
                └───────  server, web (consume both)
                └───────  card-data-pipeline (depends on no workspace crate — emits cards/src/generated/, which uses card-dsl)
```

- `card-dsl` — pure data types: the effect DSL (`Ability`, `Effect`, `Trigger`, builders) and static metadata (`CardMetadata`, `CardType`, `Class`, …). No I/O, state, or engine behavior. Both `game-core` and `cards` depend on it.
- `game-core` — the **kernel**: state, action/event enums, apply loop, evaluator. No I/O, no async, compiles to `wasm32`. Never depends on `cards`, `scenarios`, or anything above it.
- `cards` — **content**: pipeline-generated corpus + hand-written `Ability` declarations.

Why the direction matters: editing the engine must not recompile 5600 lines of generated card data, and scenarios/tests must consume the engine without the corpus. If you want `game-core` to call into `cards`, you want the **card registry** below.

## CardRegistry — the only cross-crate bridge

`game_core::card_registry::CardRegistry` is a `Copy` bundle of function pointers. The struct's fields are the list, and each field's doc says what it looks up. `native_condition_for` is the one read-only native slot: a `Condition::Native` resolves there whether it guards an effect or an ability's eligibility. A native *effect* gets a `NativeCx`, not the engine's `Cx`: full read access to the state, and the engine's exits as its only way to change the game. A native that suspends for a choice after calling an exit is `Rejected`, so its writes roll back with the apply (standards.md, "A native changes the game only through an engine exit"). `cards` provides `pub const REGISTRY`, whose slots are searches over `cards::impls::ALL`, one registration record per card (#986). The host hands it, with `scenarios::REGISTRY`, to every `apply` and `seat_and_open`:

```rust
const REGISTRIES: Registries = Registries { cards: &cards::REGISTRY, scenarios: &scenarios::REGISTRY };
let result = game_core::engine::apply(state, action, REGISTRIES);
```

The engine looks the scenario module up once per apply and carries both on `Cx`; a handler reads `cx.cards`, and a read-only query that needs card data takes `cards: &CardRegistry` as a parameter. There is no process global, so "no registry" cannot happen — the only absence is a registry that doesn't know a code — and each test chooses its registry: `.with_registries(..)` on the board builder, or `TestSession::new(state, regs)`. The fn pointers reference `card_dsl::{CardMetadata, Ability}` and `game_core::state::CardCode` directly.

## Event-sourced state — Action → apply → ApplyResult

`apply(state: GameState, action: Action, registries: Registries) -> ApplyResult { state, events, outcome }` (`crates/game-core/src/engine/mod.rs`) is the **only** entry point that mutates state. The action log is a flat `Vec<Action>`; replaying it over the same seed state reproduces state bit-for-bit. Replay is deterministic because every random draw (chaos tokens, deck shuffles) comes from the seeded `RngState` carried in `GameState` (`crates/game-core/src/rng.rs`), not because draws are logged: production code never constructs an `Action::Engine` record, and only tests issue one.

**Handler contract — validate-first / mutate-second.** Every dispatch handler in the `crates/game-core/src/engine/dispatch/` module tree:

1. Checks every precondition; on any failure returns `EngineOutcome::Rejected { reason }` with state and events **unchanged**.
2. Mutates state and pushes events only after all validations pass.

Backstopped structurally: `apply_via` (`crates/game-core/src/engine/mod.rs`) snapshots the pristine state before dispatch and restores it (state, events, and RNG position) whenever the outcome is `Rejected` — no handler, including the mutating DSL evaluator, can leak partial state past a rejection. Handlers still follow validate-first as a convention (it keeps rejection cheap and reasons precise; canonical shape: `actions::move::handle`, `actions::investigate::handle`, `cards::play_card`), but mid-resolution mutations before a reject are rolled back at the apply boundary, not merely event-cleared.

Every `apply` ends in an `EngineOutcome` (`crates/game-core/src/engine/outcome.rs`); read its variants there rather than a list here. `AwaitingInput` round-trips via `PlayerAction::ResolveInput`. Its request is built from the frame on top by the prompt table (`crates/game-core/src/engine/dispatch/prompt.rs`), and nowhere else: a step that suspends rests its frame and returns `prompt::suspend`, and a step that pushes a prompt frame returns `Done` for the `drive` loop to surface it.

## Hybrid card-effect DSL

`crates/card-dsl/src/dsl.rs` defines `Ability`, which pairs a `Trigger` (when the ability is active) with an `Effect` tree (what it does). An `OnEvent` trigger names the event it listens for with an `EventPattern`. The variants are the enums themselves — read `Trigger`, `Effect` and `EventPattern` in `dsl.rs` rather than a list here. The evaluator (`crates/game-core/src/engine/evaluator.rs`) walks effect trees under the same validate-first contract, and `step_leaf` there is the one exhaustive match that dispatches every `Effect` variant.

An `OnEvent` ability declares the **timing cell** its printed trigger word names (**Timing cell** in [`GLOSSARY.md`](../../GLOSSARY.md)). The conventions an author follows when writing one — checking the declared `EventTiming` against the quoted trigger word, and naming the cell in the module's prose — are in [`standards.md`](standards.md); the *why* is in [ADR 0008](../adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md), and which conditions are still caller-owned is `TimingEvent::condition_resolution` (`crates/game-core/src/engine/dispatch/emit.rs`).

Cards are **Rust source** (typed, compiler-checked), not JSON: each is a module `crates/cards/src/impls/<name>.rs` exposing `CODE: &str`, `abilities() -> Vec<Ability>` and a `CARD` registration record listed in `impls::ALL`; the `impls` module header gives the add-a-card procedure. Cards needing primitives the DSL lacks get a Rust impl — and a primitive is added only once a second card wants the same pattern ([`standards.md`](standards.md)).

A card is **playable** iff it has a record in `cards::impls::ALL` (`cards::is_playable(code)`); unimplemented cards appear in deckbuilding but are refused by the deck-import gate: scenario start rejects a roster whose investigator or deck holds one, naming every such code. `PlayCard` on an unimplemented card rejects loudly. On play: assets land in `cards_in_play` and stay (their `Trigger::Constant` abilities contribute via the registry while in play); events run their `OnPlay` effects then move to `discard` (emit `CardDiscarded { from: Zone::Hand, … }`), or to the bottom of the deck while their owner controls Wendy's Amulet 01014 (emit `CardPlacedOnBottomOfDeck` instead). Every other `CardType` rejects.

## Card-data pipeline

`data/arkhamdb-snapshot/` is a manually-pinned subset of upstream `Kamalisk/arkhamdb-json-data`. **Never auto-sync** — a malformed upstream entry can't surprise the build. What it holds versus what the build compiles is the **Snapshot** / **Corpus** distinction in [`GLOSSARY.md`](../../GLOSSARY.md).

Every vendored pack file is sorted into `PACK_FILES` / `REFERENCE_FILES` / `OUT_OF_SCOPE_FILES`, and `classify` (`crates/card-data-pipeline/src/main.rs`) fails on any file in none of them *and* on any in-scope pack from `packs.json` with no vendored file. It runs both in the pipeline and as a test, so CI catches a mis-vendored bump. See `data/arkhamdb-snapshot/SOURCE.md`.

Ingestion translates ArkhamDB's `faction` field to `Class`, which is why nothing downstream of the pipeline says *faction*.

Making a pack playable: (1) move its files from `REFERENCE_FILES` to `PACK_FILES` (bumping the snapshot first only if you need fresher data), (2) `cargo run -p card-data-pipeline` regenerates `crates/cards/src/generated/cards.rs`, emitting unplayable stubs for cards without impls, (3) replace stubs with DSL/Rust impls, (4) write tests.
