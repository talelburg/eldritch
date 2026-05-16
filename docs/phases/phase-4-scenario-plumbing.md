# Phase 4 — Scenario plumbing

## Status

⏳ Planned. Issues filed; no work started. Begins after Phase 3 closes.

## Goal

Toy scenario plays setup to resolution in tests.

## Issues (8 open)

| # | Title | Notes |
|---|---|---|
| `#74` | scenario module skeleton: setup, detect_resolution, apply_resolution | The architectural anchor; everything else attaches to this shape. |
| `#72` | encounter deck state: shuffled deck of treacheries + enemies | The source mythos cards draw from. |
| `#69` | Mythos phase content: encounter deck draw + treachery resolution | Consumes `#72`. |
| `#70` | Upkeep phase content: ready cards, draw 1, gain 1 resource | Refreshes the round. |
| `#71` | Enemy phase content: enemy attacks + hunter movement | Reuses the `enemy_attack` machinery already shipped during Phase 3. |
| `#73` | act + agenda decks, doom counter, threshold-based agenda advance | The scenario-progression mechanic. |
| `#75` | campaign log + `Fact` enum + scenario sequencing | Bridges to Phase 9 campaigns. |
| `#103` | player windows + Fast-ability gating across windows | Filed during `#53` to lift the Investigation-phase-only gate once non-Investigation phases exist. |

## Ordering

⏳ **TBD.** Rough sketch based on dependency reasoning:

1. `#74` scenario module skeleton — defines the shape everything else conforms to.
2. `#72` encounter deck state — independent of #74's API beyond the state shape.
3. `#69` / `#70` / `#71` — phase content (Mythos / Upkeep / Enemy), each adding a phase to the round cycle. Probably tackle one at a time.
4. `#73` act + agenda + doom — scenario progression mechanic on top of phase content.
5. `#75` campaign log + `Fact` enum + sequencing — wraps the scenario as a campaign step.
6. `#103` player windows — lifts the Investigation-phase-only gate on `PlayCard` / `ActivateAbility` once non-Investigation phases are real.

Refine when this phase opens. The actual order may interleave depending on what a "toy scenario" demonstration ends up requiring.

## Decisions made

From the 2026-05-01/02 strategy phase, locked in:

- **Scenarios = code, not pure data.** Each scenario is a Rust module exposing `setup()`, `detect_resolution()`, `apply_resolution()`, and may register scenario-specific rules.
- **Act + agenda cards** use the standard card DSL with scenario-specific verbs (`advance_act_deck`, `add_doom`).
- **Encounter sets** are pure data — lists of card codes.
- **`arkham-cards-data` is a great structured reference** but cannot be fed directly into the simulator (it's written for a guide app, not an executing engine). Read manually, write our own scenario modules.
- **Mid-scenario resume** required. A scenario abandoned in progress must be resumable from the action log. (Already true at the engine level by virtue of the event-sourcing model.)

## Open questions

- **Toy scenario choice.** What's the simplest possible scenario for the "toy scenario plays setup to resolution" demonstration? Probably a custom 1-location, 1-enemy, 1-act, 1-agenda scenario used only for testing — not a real published scenario (those land in Phase 7).
- **Treachery resolution shape.** Treacheries from the encounter deck don't yet have a DSL representation. `Trigger::OnEvent` (`#54`) and the on-draw effect path need scoping.
- **Hunter movement rules.** Hunter enemies move toward the nearest investigator; how that target-selection plays with the existing location-connection graph needs design.
- **Location-state shape.** Locations today have shroud / clues / connections / revealed. Phase 4 will need to surface reveal-effects, on-enter triggers, etc. — see also Phase-3 `#56` Study, which is in the awkward position of being a card without a defined home for its abilities.
- **Player windows model (`#103`).** Filed but not yet designed in detail. Will share state shape with `#52` reaction windows (consider unifying).

## Dependencies

Phase 3 — needs the skill-test machinery, action handlers, and DSL evaluator. Specifically Phase-3 issues that gate Phase-4 work:
- `#52` reaction windows — Phase-4 phase content emits events that triggered abilities react to.
- `#54` `OnEvent` trigger — DSL extension for the above.
- Everything else in Phase 3 should be closed by the time Phase 4 starts.

## What "done" looks like

A custom toy scenario (a few locations, a handful of encounter cards, an act and agenda deck) plays from setup through a resolution in tests:

- Engine cycles through Mythos / Investigation / Enemy / Upkeep phases.
- An act advances when a clue threshold is met; the scenario resolves.
- Or doom advances on the agenda and the scenario resolves the other way.
- `detect_resolution()` fires at the right moment; `apply_resolution()` writes campaign-log facts.
- Mid-scenario state can be serialized + replayed identically.
