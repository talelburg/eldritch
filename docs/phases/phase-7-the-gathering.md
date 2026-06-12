# Phase 7 — The Gathering

## Status

🛠️ **Slice 1 in progress** (kickoff [#216](https://github.com/talelburg/eldritch/issues/216)).
Engine spine (A1/A2) and scenario plumbing (B1/B2) shipped; **Group C**
(the Gathering content) is decomposed into sub-slices C1–C7
([#227](https://github.com/talelburg/eldritch/issues/227)–[#245](https://github.com/talelburg/eldritch/issues/245),
kickoff [#246](https://github.com/talelburg/eldritch/issues/246)).
Design specs:
[Gathering design](../superpowers/specs/2026-06-10-phase-7-slice-1-gathering-design.md),
[Group C decomposition](../superpowers/specs/2026-06-11-phase-7-slice-1-group-c-decomposition-design.md).

## Goal

First real scenario playable in browser, solo, all 5 investigators.

## Slice 1 — Roland through The Gathering

Vertical-slice-first: one investigator playable end-to-end (solo,
Standard, win/lose-path fidelity) before breadth. Deferred north-star
work: `emit_event` dispatch unification (`#212`) + iterative
trigger-ordering (`#213`), folding in `#117`.

| Order | Issue | Plan | State |
|---|---|---|---|
| — | [#216](https://github.com/talelburg/eldritch/issues/216) — kickoff: spec + engine-spine plans + breakdown | — | ✅ PR #217 |
| A1 | [#214](https://github.com/talelburg/eldritch/issues/214) — engine-spine primitives (DealDamage/DealHorror, EnteredLocation, Act/Agenda CardCode) | `plans/2026-06-10-…-engine-spine-primitives.md` | ✅ PR #218 |
| A2 | [#215](https://github.com/talelburg/eldritch/issues/215) — forced-trigger dispatch (`fire_forced_triggers`) — depends on A1 | `plans/2026-06-10-…-forced-trigger-dispatch.md` | ✅ PR #219 |
| B1 | [#220](https://github.com/talelburg/eldritch/issues/220) — `reference_card` field + symbol-token lookup plumbing | `plans/2026-06-10-…-reference-card-routing.md` | ✅ PR #223 |
| B2 | [#221](https://github.com/talelburg/eldritch/issues/221) — roster/seating step + `StartScenario` investigator selection (protocol change) | `plans/2026-06-10-…-b2-roster-seating.md` | ✅ PR #225 |
| B3 | [#222](https://github.com/talelburg/eldritch/issues/222) — registry-swap foundation (server installs real card registry; D5) | — | 🔀 folded into C (C7a [#244](https://github.com/talelburg/eldritch/issues/244)) |
| — | [#246](https://github.com/talelburg/eldritch/issues/246) — kickoff: Group C decomposition spec + issue breakdown | [`…group-c-decomposition…`](../superpowers/specs/2026-06-11-phase-7-slice-1-group-c-decomposition-design.md) | ✅ PR #247 |
| C | content: Gathering scenario cards + setup + Roland + signature/weakness + starter deck — **decomposed into C1–C7** ([#227](https://github.com/talelburg/eldritch/issues/227)–[#245](https://github.com/talelburg/eldritch/issues/245)); see breakdown below | [`…group-c-decomposition…`](../superpowers/specs/2026-06-11-phase-7-slice-1-group-c-decomposition-design.md) | 🛠️ in progress |
| D | integration & web: investigator/scenario picker (end-to-end Won/Lost gate is C7b [#245](https://github.com/talelburg/eldritch/issues/245)) | TBD | 📐 spec'd |

Group A *extends* the existing `reaction_windows.rs` OnEvent machinery
(not greenfield); forced scenario effects take a separate immediate path
(`fire_forced_triggers`) distinct from player reaction windows.

### Group C breakdown (C1–C7)

Decomposed in
[`…group-c-decomposition…`](../superpowers/specs/2026-06-11-phase-7-slice-1-group-c-decomposition-design.md).
Split along the engine-machinery / card-content seam. `C1a` (#227) is the
root dependency; C7 is the playable Won/Lost gate; #212 lands after C.

| Sub | Issue | What | State |
|---|---|---|---|
| C1a | [#227](https://github.com/talelburg/eldritch/issues/227) | `setup()` world-build + forced location effects | ✅ PR #250 |
| C1b | [#228](https://github.com/talelburg/eldritch/issues/228) | act-advancement objective types (01109 / 01110) | — |
| C2 | [#229](https://github.com/talelburg/eldritch/issues/229) | 01104 symbol-token effects + victory points | — |
| C3a | [#230](https://github.com/talelburg/eldritch/issues/230) | Prey variants + Retaliate | — |
| C3b | [#231](https://github.com/talelburg/eldritch/issues/231) | the six encounter enemies | — |
| C3c | [#232](https://github.com/talelburg/eldritch/issues/232) | agenda 01107 forced (movement + doom; +`RoundEnded`) | — |
| C4a | [#233](https://github.com/talelburg/eldritch/issues/233) | threat-area zone + shared scan source (in-C consolidation seam) | — |
| C4b | [#234](https://github.com/talelburg/eldritch/issues/234) | one-shot Revelation treacheries (×4) | — |
| C4c | [#235](https://github.com/talelburg/eldritch/issues/235) | persistent threat-area treacheries (×3) | — |
| C5a | [#236](https://github.com/talelburg/eldritch/issues/236) | Cover Up before-timing interrupt + `GameEnd` | — |
| C5b | [#237](https://github.com/talelburg/eldritch/issues/237) | Guard Dog damage-from-enemy window | — |
| C5c | [#238](https://github.com/talelburg/eldritch/issues/238) | .38 Special signature + Cover Up content | — |
| C5d | [#239](https://github.com/talelburg/eldritch/issues/239) | Guardian L0 assets (×6) | — |
| C5e | [#240](https://github.com/talelburg/eldritch/issues/240) | Guardian L0 events + skill (×4) | — |
| C6a | [#241](https://github.com/talelburg/eldritch/issues/241) | Dr. Milan after-investigate window | — |
| C6b | [#242](https://github.com/talelburg/eldritch/issues/242) | Seeker deck cards | — |
| C6c | [#243](https://github.com/talelburg/eldritch/issues/243) | Neutral deck cards | — |
| C7a | [#244](https://github.com/talelburg/eldritch/issues/244) | registry swap + web `SCENARIO_ID` repoint (B3) | — |
| C7b | [#245](https://github.com/talelburg/eldritch/issues/245) | end-to-end Won/Lost integration test | — |

## Future slices (after Slice 1)

Not yet specced/planned in detail — recorded here so the arc survives a
fresh session. Rough order; each becomes its own spec → plan → issues
when picked up.

- **Slice 2+ — investigator breadth.** The other four original-Core
  investigators (Daisy Walker, "Skids" O'Toole, Agnes Baker, Wendy
  Adams), each with their signature asset/weakness pair and starter
  deck — the same content shape as Roland in Slice 1, reusing the engine
  spine. Likely one slice per investigator (or grouped) once Slice 1
  proves the pipe. Goal: all five picker-eligible.
- **Difficulty selection.** Slice 1 ships **Standard** only. Add Easy /
  Hard / Expert chaos bags + a difficulty picker.
- **Solo-with-2 UX.** One client driving two investigators — how the
  picker, turn flow, and board present two characters under one player.
  Genuinely open design question (see Open questions).
- **Deferred optional Gathering content** (off the win/lose path, so cut
  from Slice 1): Lita Chantler's parley/take-control and the Parlor
  (`01115`) **Resign** action.
- **Engine north-star (cross-slice, may be its own slice).** `emit_event`
  dispatch unification (`#212`) + iterative simultaneous-trigger ordering
  (`#213`, RR p.17 — player picks order even in solo) + the trigger
  index (`#117`); plus the optional click-to-resolve UX for *lone* forced
  effects. Slice 1's `fire_forced_triggers` is a forward-compatible
  subset; this work replaces its single-trigger-only limitation.

Campaign sequencing beyond The Gathering (The Midnight Masks, The
Devourer Below, campaign log + `Fact` enum) is **Phase 9**, not Phase 7.

## Issues (filed)

| # | Title | Notes |
|---|---|---|
| `#65` | skill-test other-investigator commits | Needed for multi-investigator commit scenarios; tagged Phase 7 because that's the first real-card consumer. |
| `#77` | Parley + Engage actions | Basic player actions needed for full scenario coverage. |

## Decisions made

- **The Gathering** is the first scenario of the original Core Set's *Night of the Zealot* campaign. Three locations to start (Study + connections), with the campaign expanding from there.
- **"Solo with 1–2 investigators" is the supported mode** for this phase. Multiplayer (two human investigators on different machines) is Phase 8.
- **All 5 original-Core investigators implementable:** Roland Banks (`#55`, already filed in Phase 3), Daisy Walker, "Skids" O'Toole, Agnes Baker, Wendy Adams. Each needs their card impl + signature cards.
- **Investigator seating (B2):** stats are read from `CardMetadata` via the existing `CardRegistry` (no new registry); the deck is a player-supplied `RosterEntry { investigator, deck }` field on `StartScenario` (the Phase-9 decklist-import seam). `start_scenario` rejects unless ≥1 investigator is seated; the pre-seated synthetic-test path is tolerated until [#224](https://github.com/talelburg/eldritch/issues/224) migrates tests to roster seating and requires a non-empty roster.
- **Registry swap (B3, [#222](https://github.com/talelburg/eldritch/issues/222)) folds into Group C, not a standalone PR.** `server` already depends on `cards` and installs the real `scenarios::REGISTRY`; the only work is swapping the *card* registry (`synth_cards::TEST_REGISTRY` → `cards::REGISTRY`) in `server/src/lib.rs`. But that swap is coupled to C: the synthetic scenario's encounter deck draws synth-only card codes, so swapping with no real scenario to serve would break the `"synthetic"` web demo mid-play and `server/tests/registries.rs` for the whole B3→C window. So the swap + the web `SCENARIO_ID` repoint to `"the-gathering"` land in the Group C PR alongside the real scenario; synthetic registries stay for per-process tests.

- **Scenario investigator placement uses `GameState.starting_location` (C1a, [#227](https://github.com/talelburg/eldritch/issues/227)).** `setup()` can't seat investigators — they're created later by the `StartScenario` roster action — so it records the starting location on `GameState.starting_location` and the seating step places each seated investigator there (`None` keeps the legacy pre-seated path). Every scenario's `setup()` sets this; it's the generic placement channel for the `setup() → roster-seating` split. C1a also fixes the faithful **Study-only Act-1 board**: `setup()` builds *only* the Study (isolated); the four set-aside locations + the Act-1 "Door on the Floor" transition are **C1b** ([#228](https://github.com/talelburg/eldritch/issues/228)), which also replaces act 01110's placeholder clue threshold with its real "Ghoul Priest defeated" objective.

- **Card metadata is a `CardKind` enum, and encounter cards + their stats are in the corpus** ([#254](https://github.com/talelburg/eldritch/issues/254) remodel, [#252](https://github.com/talelburg/eldritch/issues/252) ingestion — both infra PRs, not phase-7 issues, but load-bearing for all of Group C). `CardMetadata` is now an identity core + a `kind: CardKind` enum (`Investigator`/`Asset`/`Event`/`Skill`/`Enemy`/`Location`/`Act`/`Agenda`/`Treachery`); read `card_type()`/`class()` via accessors and match on `kind` for type-specific stats. The 8 in-scope encounter files are ingested, so **locations/acts/agendas/enemies/treacheries carry their printed stats in the corpus** (`CardKind::Location { shroud, clues, victory }`, `Act { clue_threshold, victory }`, `Agenda { doom_threshold }`, `Enemy { fight, evade, damage, horror, health, victory, quantity, … }`). Consequences for Group C: read stats via `cards::by_code`/`metadata_for` instead of hand-typing (C1b set-aside locations, C2 victory points, C3 enemy stats, C4 treachery `quantity`); `scenario`-type cards (e.g. ref card `01104`) are **not** in the corpus — their effects live in `abilities()` impls; and C3b (#231) must wire `spawn_enemy` to read combat stats from `CardKind::Enemy` (it still hardcodes `fight: 1, evade: 1`). The `TestGame` builder is now `game_core::state::GameStateBuilder` ([#251](https://github.com/talelburg/eldritch/issues/251)); the DSL "you" target is `InvestigatorTarget::You` ([#248](https://github.com/talelburg/eldritch/issues/248)).

- **`emit_event` unification (#212) lands *after* Group C, not before it.** C is built on the existing `ForcedTriggerPoint` enum-dispatcher + reaction-window pipeline, extended with new timing points (`RoundEnded`, `EndOfTurn`, `AfterLocationInvestigated`, `GameEnd`, damage/investigate windows) as content demands them; #212 then consolidates those into one emit-driven chokepoint, validated against all of C's real content. The dispatch surface C adds is a handful of points through already-generic machinery (the 7 treacheries share one Revelation hook; locations and Beat Cop reuse existing paths), so front-loading #212 would design its event taxonomy before the cards defining its requirements exist. **#213** (player-choice simultaneous-trigger ordering) is deferred further still — until then simultaneous triggers resolve in a fixed **deterministic** order. C4a (#233) lands the one in-C consolidation seam (shared scan source over `cards_in_play` + threat area) that #212 later absorbs.

## Open questions

The Phase-6-era "scoping TBD" list is now addressed by the slice
structure above — the scenario module, encounter/act/agenda/location
impls, Roland, and Standard difficulty are **Slice 1** (kickoff `#216`);
the other investigators, difficulties, solo-2 UX, and optional content
map to **Future slices**. Genuinely-open design questions that remain:

- **Solo-with-2 UX.** One player controls two investigators; how does
  the client present that (picker, whose-turn, two boards vs. tabbed)?
  Unresolved — a Future-slice design question.
- **Story-asset/weakness shape.** Cover Up (Roland's, in Slice 1) is
  scoped, but the broader campaign-driven mods (Lita Chantler, Hospital
  Debts, …) need a pattern; revisit as they land.

## Dependencies

- Phase 4 (scenario plumbing) — the scenario module API.
- Phase 5 (server + persistence) — backing store.
- Phase 6 (web client v0) — UI.
- Phase 3 (`#55` Roland Banks, `#56` Study) — already filed there; these spill into Phase 7's coverage.

## What "done" looks like

A solo human, in the browser, picks an investigator, sets up The Gathering, plays through the scenario to a resolution. All five investigators are picker-eligible. The campaign log records the resolution's facts. Standard difficulty works correctly; harder difficulties may land here or in a polish pass.
