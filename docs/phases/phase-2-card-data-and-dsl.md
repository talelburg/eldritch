# Phase 2 — Card data + DSL

## Status

✅ Closed (5/8). Cards `#37` Magnifying Glass, `#38` Hyperawareness, `#39` Deduction, and the per-skill-test scope extension `#45` moved to phase-3-skill-test-end-to-end where their blocking primitives land naturally with the engine evaluator.

## Goal

Framework + DSL v0 + 2 validating cards.

## Issues

| Issue | Status |
|---|---|
| `#32` — commit pinned ArkhamDB JSON snapshot | ✅ closed |
| `#33` — `card-data-pipeline` binary + generated module layout | ✅ closed |
| `#34` — DSL v0 primitive set | ✅ closed |
| `#35` — Holy Rosary (01059) | ✅ closed |
| `#36` — Working a Hunch (01037) | ✅ closed |
| `#37` — Magnifying Glass (01030) | ➡️ moved to Phase 3 (closed there) |
| `#38` — Hyperawareness (01034) | ➡️ moved to Phase 3 (closed there) |
| `#39` — Deduction (01039) | ➡️ moved to Phase 3 (still open there) |
| `#45` — per-skill-test-kind modifier scope | ➡️ moved to Phase 3 (closed there) |

## Decisions made

- **Card-data sourcing** is a pinned snapshot of `Kamalisk/arkhamdb-json-data` under `data/arkhamdb-snapshot/`. Updates are manual, not auto-synced — a malformed upstream entry can't surprise the build.
- **Scope** is original Core + Dunwich Legacy cycle only. Other packs land in later phases.
- **Generated card metadata** lives in `crates/cards/src/generated/cards.rs`, produced by `cargo run -p card-data-pipeline`. Hand-edits are overwritten on the next pipeline run; the file is marked GENERATED.
- **DSL primitives v0**:
  - Triggers: `Constant`, `OnPlay`, `OnCommit`. (Phase-3 adds `Activated`; later phases add `OnEvent`, `OnLeavePlay`, reactions.)
  - Effects: `GainResources`, `DiscoverClue`, `Modify`, `Seq`, `If`, `ForEach`, `ChooseOne`.
  - Scopes: `WhileInPlay`, `ThisSkillTest`, `ThisTurn`. (Phase-3 adds `WhileInPlayDuring(SkillTestKind)`.)
  - Targets: `Controller`, `Active`, `ChosenByController` (investigator); `ControllerLocation`, `ChosenByController` (location).
- **Cards are Rust source**, typed and compiler-checked. Each card has a `crates/cards/src/impls/<name>.rs` module exposing `CODE: &str` + `abilities() -> Vec<Ability>`. The registry dispatch `cards::abilities_for(code)` is the single source of "what does this card do."
- **A card is *playable* iff it has an `abilities()` implementation.** Cards without one are in the corpus (for deckbuilding visibility) but refused by PlayCard and the future deck-import gate.

## Dependencies

Phases 0–1.

## What "done" looked like

`cards::abilities_for("01059")` returns Holy Rosary's `+1 willpower while in play` ability; `cards::abilities_for("01037")` returns Working a Hunch's `on-play discover-clue` ability. Pipeline regeneration is idempotent.

## Retrospective notes

The `#37`/`#38`/`#39`/`#45` move to Phase 3 was the right call. The DSL extensions they needed (per-skill-test scope, `Activated` trigger, `OnCommit` consumer) belong with the evaluator wire-up that Phase 3 was already going to do — separating them would have meant duplicate test scaffolding.
