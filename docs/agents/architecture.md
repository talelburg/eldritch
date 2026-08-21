# Architecture

How this repo is put together: what each crate may depend on, how state changes, how a card becomes behaviour, and how the corpus is built. Orientation, not a coding standard — the rules an author follows are in [`standards.md`](standards.md).

## Crate layering — strict kernel/content separation

```
card-dsl  ←  game-core   ←  cards          ←  scenarios
                ↑              ↑                  ↑
                └───────  server, web (consume both)
                └───────  card-data-pipeline (consumes card-dsl only — emits cards/src/generated/)
```

- `card-dsl` — pure data types: the effect DSL (`Ability`, `Effect`, `Trigger`, builders) and static metadata (`CardMetadata`, `CardType`, `Class`, …). No I/O, state, or engine behavior. Both `game-core` and `cards` depend on it.
- `game-core` — the **kernel**: state, action/event enums, apply loop, evaluator. No I/O, no async, compiles to `wasm32`. Never depends on `cards`, `scenarios`, or anything above it. Re-exports `card_dsl::{dsl, card_data}` at the historical `game_core::dsl` / `game_core::card_data` paths.
- `cards` — **content**: pipeline-generated corpus + hand-written `Ability` declarations.

Why the direction matters: editing the engine must not recompile 5600 lines of generated card data, and scenarios/tests must consume the engine without the corpus. If you want `game-core` to call into `cards`, you want the **card registry** below.

The crates are **layers of one domain model**, not separate bounded contexts — which is why the repo is single-context ([`domain.md`](domain.md)).

## CardRegistry — the only cross-crate bridge

`game_core::card_registry` is a `OnceLock<CardRegistry>` holding two function pointers (`metadata_for: fn(&CardCode) -> Option<&'static CardMetadata>`, `abilities_for: fn(&CardCode) -> Option<Vec<Ability>>`). `cards` provides `pub const REGISTRY`; hosts install once at startup:

```rust
let _ = game_core::card_registry::install(cards::REGISTRY);
```

Engine handlers that need card data (`PlayCard`, future skill-test modifier queries) call `card_registry::current()` and reject cleanly on `None`. Tests that don't touch card data never install — most rejection paths short-circuit before the lookup. The fn pointers reference `card_dsl::{CardMetadata, Ability}` and `game_core::state::CardCode` directly (survived the `card-dsl` split, #93).

## Event-sourced state — Action → apply → ApplyResult

`apply(state: GameState, action: Action) -> ApplyResult { state, events, outcome }` is the **only** entry point that mutates state. The action log is a flat `Vec<Action>`; replaying it reproduces state bit-for-bit. Every randomness source (chaos draws, deck shuffles) is recorded as an explicit `EngineRecord` action so replay is deterministic.

**Handler contract — validate-first / mutate-second.** Every dispatch handler in `crates/game-core/src/engine/dispatch.rs`:

1. Checks every precondition; on any failure returns `EngineOutcome::Rejected { reason }` with state and events **unchanged**.
2. Mutates state and pushes events only after all validations pass.

Backstopped structurally since #161: `apply_via` (`crates/game-core/src/engine/mod.rs`) snapshots the pristine state before dispatch and restores it (state, events, and RNG position) whenever the outcome is `Rejected` — no handler, including the mutating DSL evaluator, can leak partial state past a rejection. Handlers still follow validate-first as a convention (it keeps rejection cheap and reasons precise; canonical shape: `move_action`, `investigate`, `play_card`), but mid-resolution mutations before a reject are rolled back at the apply boundary, not merely event-cleared.

`EngineOutcome` = `Done | AwaitingInput { … } | Rejected { reason }`; `AwaitingInput` round-trips via `PlayerAction::ResolveInput`.

## Hybrid card-effect DSL

`crates/card-dsl/src/dsl.rs` defines `Ability { trigger: Trigger, effect: Effect }`. Triggers: `Constant`, `OnPlay`, `OnCommit` (+ `OnEvent` / `Activated` / reaction triggers later). Effects: `GainResources`, `DiscoverClue`, `Modify`, `Seq`, `If`, `ForEach`, `ChooseOne`. The evaluator (`crates/game-core/src/engine/evaluator.rs`) walks effect trees under the same validate-first contract.

An `OnEvent` ability declares the **timing cell** its printed trigger word names (**Timing cell** in [`CONTEXT.md`](../../CONTEXT.md)). The conventions an author follows when writing one — checking the declared `EventTiming` against the quoted trigger word, and naming the cell in the module's prose — are in [`standards.md`](standards.md); the *why*, and the caller-owned conditions that remain, are in [ADR 0008](../adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md).

Cards are **Rust source** (typed, compiler-checked), not JSON: each is a module `crates/cards/src/impls/<name>.rs` exposing `CODE: &str` and `abilities() -> Vec<Ability>`. Cards needing primitives the DSL lacks get a Rust impl — and a primitive is added only once a second card wants the same pattern ([`standards.md`](standards.md)).

A card is **playable** iff it has an `abilities()` impl (`cards::is_playable(code)`); unimplemented cards appear in deckbuilding but are refused by the deck-import gate (Phase 9). `PlayCard` on an unimplemented card rejects loudly. On play: assets land in `cards_in_play` and stay (their `Trigger::Constant` abilities contribute via the registry while in play); events run their `OnPlay` effects then move to `discard` (emit `CardDiscarded { from: Zone::Hand, … }`). Every other `CardType` rejects.

**Horror soak isn't modelled by the DSL yet** — tracked in #44, and this paragraph goes when #44 lands.

## Card-data pipeline

`data/arkhamdb-snapshot/` is a manually-pinned subset of upstream `Kamalisk/arkhamdb-json-data`. **Never auto-sync** — a malformed upstream entry can't surprise the build. What it holds versus what the build compiles is the **Snapshot** / **Corpus** distinction in [`CONTEXT.md`](../../CONTEXT.md).

Every vendored pack file is sorted into `PACK_FILES` / `REFERENCE_FILES` / `OUT_OF_SCOPE_FILES`, and `classify` (`crates/card-data-pipeline/src/main.rs`) fails on any file in none of them *and* on any in-scope pack from `packs.json` with no vendored file. It runs both in the pipeline and as a test, so CI catches a mis-vendored bump. See `data/arkhamdb-snapshot/SOURCE.md`.

Ingestion translates ArkhamDB's `faction` field to `Class`, which is why nothing downstream of the pipeline says *faction*.

Making a pack playable: (1) move its files from `REFERENCE_FILES` to `PACK_FILES` (bumping the snapshot first only if you need fresher data), (2) `cargo run -p card-data-pipeline` regenerates `crates/cards/src/generated/cards.rs`, emitting unplayable stubs for cards without impls, (3) replace stubs with DSL/Rust impls, (4) write tests.
