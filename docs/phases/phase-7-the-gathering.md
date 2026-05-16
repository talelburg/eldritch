# Phase 7 — The Gathering

## Status

📐 Architecture only. Two issues filed (`#65`, `#77`) tagged here; the rest TBD.

## Goal

First real scenario playable in browser, solo, all 5 investigators.

## Issues (filed)

| # | Title | Notes |
|---|---|---|
| `#65` | skill-test other-investigator commits | Needed for multi-investigator commit scenarios; tagged Phase 7 because that's the first real-card consumer. |
| `#77` | Parley + Engage actions | Basic player actions needed for full scenario coverage. |

## Decisions made

- **The Gathering** is the first scenario of the original Core Set's *Night of the Zealot* campaign. Three locations to start (Study + connections), with the campaign expanding from there.
- **"Solo with 1–2 investigators" is the supported mode** for this phase. Multiplayer (two human investigators on different machines) is Phase 8.
- **All 5 original-Core investigators implementable:** Roland Banks (`#55`, already filed in Phase 3), Daisy Walker, "Skids" O'Toole, Agnes Baker, Wendy Adams. Each needs their card impl + signature cards.

## Open questions

⏳ **Scoping TBD.** When Phase 6 closes, file:

- **Scenario module: The Gathering.** Locations, encounter set wiring, act/agenda decks, resolution conditions.
- **Card implementations** for every card in The Gathering's encounter sets and every card in the five investigators' starter decks. Substantial volume.
- **Investigator card implementations** for the 5 original-Core investigators. Each has stats, max-health/sanity, signature card pairings.
- **Story-asset/weakness implementations.** Cover Up, Lita Chantler, Hospital Debts, etc. — the campaign-driven mods.
- **Difficulty selection.** Easy / Standard / Hard / Expert chaos bags.
- **Solo-with-2 UX.** One player controls two investigators; how does the client present that?

## Dependencies

- Phase 4 (scenario plumbing) — the scenario module API.
- Phase 5 (server + persistence) — backing store.
- Phase 6 (web client v0) — UI.
- Phase 3 (`#55` Roland Banks, `#56` Study) — already filed there; these spill into Phase 7's coverage.

## What "done" looks like

A solo human, in the browser, picks an investigator, sets up The Gathering, plays through the scenario to a resolution. All five investigators are picker-eligible. The campaign log records the resolution's facts. Standard difficulty works correctly; harder difficulties may land here or in a polish pass.
