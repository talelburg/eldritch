# Phase 10 — Dunwich + iteration

## Status

📐 Architecture only. No issues filed.

## Goal

Full Core + Dunwich coverage; ongoing polish.

## Decisions made

From the 2026-05-01/02 strategy phase:

- **Scope (POC):** Core Set + Dunwich Legacy cycle only. Don't pre-build infrastructure for cycles beyond Dunwich, but the effect system must be extensible enough that adding cycles later is mechanical work, not a rewrite.
- **Old-format Dunwich** (deluxe expansion + 6 mythos packs: `dwl`, `tmm`, `bota`, `uau`, `wda`, `litas`, `tece`) is what `data/arkhamdb-snapshot/` carries. Old format is the source of truth.
- **Dunwich-specific mechanics that didn't exist in Core:** investigator-skill commits across distances, more complex enemy abilities, multi-phase scenarios with new structural patterns. Each gets scoped when its first card / scenario needs it.

## Open questions

⏳ **Scoping TBD.** When Phase 9 closes, file:

- **The Dunwich Legacy campaign module.** Eight scenarios: Extracurricular Activity, The House Always Wins, The Miskatonic Museum, The Essex County Express, Blood on the Altar, Undimensioned and Unseen, Where Doom Awaits, Lost in Time and Space. Plus the Lita Chantler / Jenny Barnes etc. campaign-wide investigator availability.
- **Dunwich card implementations.** Substantial volume — hundreds of cards across the cycle. Per the project's "no manual-resolution fallback" rule, every card a player wants to use needs an implementation; the deck-import gate enforces this. Iterative.
- **Encounter sets specific to Dunwich.** New encounter sets per scenario.
- **New DSL primitives surfaced by Dunwich cards.** Some Dunwich cards will need primitives that Core didn't (horror redirect / damage soak with state — `#44` already filed; conditional `Effect::If` consumers with richer predicates; etc.).
- **Polish backlog.** Whatever rough edges Phases 5–9 left. UX improvements, error-message tweaks, performance.
- **CI strictness expansion.** Add test coverage gates? Property-test infrastructure?

## Dependencies

- All prior phases. This is the endgame for the POC.

## What "done" looks like

A friend group can pick any investigator from Core + Dunwich, build a legal deck on arkham.build, import it, and play through any Core or Dunwich Legacy scenario. The Dunwich Legacy campaign can be played start-to-finish. The simulator becomes a credible "play Arkham Horror with my friends online without owning the cards" experience.

## Beyond Phase 10

Out of scope for the POC; revisit only if the project grows past friends-only:

- The Path to Carcosa (later cycle).
- The Forgotten Age, Circle Undone, etc.
- 2026 Chapter 2 Core Set (new investigators, new mechanics — separate game generation).
- Revised Core Set (functionally identical to original Core; would just be a card-code remap).
- The new-format Investigator Expansion / Campaign Expansion split.
- Public hosting, account self-service, abuse handling.
- Mobile UX.

These all stay deferred until they're concretely needed, per the "build to a 'make game night work' bar" posture.
