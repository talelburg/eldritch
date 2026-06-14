# Phase 7 — The Gathering

## Status

🛠️ **Slice 1 in progress** (kickoff [#216](https://github.com/talelburg/eldritch/issues/216)).
Engine spine (A1/A2) and scenario plumbing (B1/B2) shipped; **Group C**
(the Gathering content) is decomposed into sub-slices C1–C7
([#227](https://github.com/talelburg/eldritch/issues/227)–[#245](https://github.com/talelburg/eldritch/issues/245),
kickoff [#246](https://github.com/talelburg/eldritch/issues/246)). Shipped:
C1a (board skeleton), C1b (Act-1 board build + Act-3 forced advance-on-defeat),
C2 (01104 symbol-token effects + location victory points),
C3a (Prey – Lowest remaining health + Retaliate keyword),
C3b (the six encounter enemies + pipeline keyword/spawn/health parsing),
C3c (agenda 01107 forced abilities), C3d (act-2 round-end window),
the act-2 reverse ([#280](https://github.com/talelburg/eldritch/issues/280):
spawn the set-aside Ghoul Priest + reveal the Parlor), the agenda
reverses ([#281](https://github.com/talelburg/eldritch/issues/281):
01105/01106 + the `AgendaAdvanced` forced point), C4a (threat-area
zone + shared scan source + `EndOfTurn`/`AfterLocationInvestigated`
forced points), the test-treachery engine prereq
([#286](https://github.com/talelburg/eldritch/issues/286):
`Effect::SkillTest` + `ForEachPointFailed` + suspendable-revelation
discard), C4b (the four one-shot Revelation treacheries —
01162/01163/01166/01167), and C4c (the three persistent threat-area /
attachment treacheries — Obscuring Fog 01168, Dissonant Voices 01165,
Frozen in Fear 01164 — with the location attachment zone, inspectable-DSL
constant restrictions, `Effect::DiscardSelf`, deterministic
simultaneous-forced-trigger resolution, and end-turn resume plumbing).
**Next: C5 → C7** (C6d also gates C7b).

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
| C1b | [#228](https://github.com/talelburg/eldritch/issues/228) | Act-1 (01108) reverse board-build + Act-3 (01110) forced advance-on-defeat (act-2 01109 objective → C3d) | ✅ PR #259 |
| C2 | [#229](https://github.com/talelburg/eldritch/issues/229) | 01104 symbol-token effects + victory points | ✅ PR #263 |
| C3a | [#230](https://github.com/talelburg/eldritch/issues/230) | Prey variants + Retaliate | ✅ PR #269 |
| C3b | [#231](https://github.com/talelburg/eldritch/issues/231) | the six encounter enemies | ✅ PR #272 |
| — | [#276](https://github.com/talelburg/eldritch/issues/276) | infra: `Effect::Native` card-local-Rust bridge (prerequisite for C3c's agenda + future bespoke cards) | ✅ PR #277 |
| C3c | [#232](https://github.com/talelburg/eldritch/issues/232) | agenda 01107 forced (movement + doom; +`RoundEnded`) | ✅ PR #278 |
| C3d | [#275](https://github.com/talelburg/eldritch/issues/275) | act-2 (01109) round-end clue-spend window (split from C3c) | ✅ PR #279 |
| C4a | [#233](https://github.com/talelburg/eldritch/issues/233) | threat-area zone + shared scan source (in-C consolidation seam) | ✅ PR #285 |
| — | [#286](https://github.com/talelburg/eldritch/issues/286) | infra: `Effect::SkillTest` + `ForEachPointFailed` + failure-side follow-up + suspendable-revelation discard (prerequisite for C4b's test treacheries) | ✅ PR #287 |
| C4b | [#234](https://github.com/talelburg/eldritch/issues/234) | one-shot Revelation treacheries (×4) | ✅ PR #288 |
| C4c | [#235](https://github.com/talelburg/eldritch/issues/235) | persistent threat-area / attachment treacheries (×3) | ✅ PR #289 |
| C5a | [#236](https://github.com/talelburg/eldritch/issues/236) | Cover Up before-timing interrupt + `GameEnd` | — |
| C5b | [#237](https://github.com/talelburg/eldritch/issues/237) | Guard Dog damage-from-enemy window | — |
| C5c | [#238](https://github.com/talelburg/eldritch/issues/238) | .38 Special signature + Cover Up content | — |
| C5d | [#239](https://github.com/talelburg/eldritch/issues/239) | Guardian L0 assets (×6) | — |
| C5e | [#240](https://github.com/talelburg/eldritch/issues/240) | Guardian L0 events + skill (×4) | — |
| C6a | [#241](https://github.com/talelburg/eldritch/issues/241) | Dr. Milan after-investigate window | — |
| C6b | [#242](https://github.com/talelburg/eldritch/issues/242) | Seeker deck cards | — |
| C6c | [#243](https://github.com/talelburg/eldritch/issues/243) | Neutral deck cards | — |
| C6d | [#284](https://github.com/talelburg/eldritch/issues/284) | encounter-deck assembly in `setup()` (quantity-aware, excludes set-aside) — gates C7b; makes Mythos draws + 01106's dig operate live | — |
| C7a | [#244](https://github.com/talelburg/eldritch/issues/244) | registry swap + web `SCENARIO_ID` repoint (B3) | — |
| C7b | [#245](https://github.com/talelburg/eldritch/issues/245) | end-to-end Won/Lost integration test (needs C6d) | — |

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

- **"Solo with 1–2 investigators" is the supported mode** for this phase — code should handle 1–2 investigators (lead-investigator tiebreaks, group clue pools), but multiplayer across machines is Phase 8.

- **Card stats come from the corpus — read via `cards::by_code` / `metadata_for`, never hand-typed** ([#254](https://github.com/talelburg/eldritch/issues/254) `CardKind` remodel, [#252](https://github.com/talelburg/eldritch/issues/252) ingestion). `CardMetadata` is an identity core + a `kind: CardKind` enum carrying type-specific printed stats (`Location { shroud, clues, victory }`, `Act { clue_threshold, victory }`, `Agenda { doom_threshold }`, `Enemy { fight, evade, damage, horror, health, victory, quantity, keywords, … }`); the 8 in-scope encounter files are ingested. `scenario`-type cards (e.g. ref card 01104) are **not** in the corpus — their effects live in `abilities()` impls / scenario hooks. Enemy keywords / spawn-location / per-investigator health are pipeline-parsed, so **future enemies need no hand-written impl** — they land via a snapshot bump + regen (`spawn_enemy` reads everything from `CardKind::Enemy`; out-of-scope keyword forms default + emit a build warning, never silently approximate).

- **`emit_event` unification (#212) lands *after* Group C, not before it; until #213, simultaneous triggers resolve in a fixed deterministic order.** C extends the existing `ForcedTriggerPoint` dispatcher + reaction-window pipeline with new timing points (`RoundEnded`, `EndOfTurn`, `AfterLocationInvestigated`, `GameEnd`, damage/investigate windows) as content demands; #212 then consolidates them into one emit-driven chokepoint, validated against C's real content. Front-loading #212 would design its event taxonomy before the cards defining its requirements exist.

- **Scenario chaos-symbol / reference-card effects live on a `ScenarioModule.resolve_symbol` hook, not card `abilities()` (C2, [#229](https://github.com/talelburg/eldritch/issues/229), PR #263).** A reference card is one-per-scenario, never a card-object, and board-dependent, so it doesn't fit the `abilities()` model; the scenario module owns it via `fn(ChaosToken, &SymbolCtx) -> SymbolOutcome` (a `modifier` applied before pass/fail + `immediate`/`on_fail` `TokenEffect`s after). **Future scenarios' reference cards add a `resolve_symbol` hook, not card impls** — no new DSL primitives.

- **Single-use card logic lives card-locally via `Effect::Native { tag }`; add a shared `card_dsl::Effect` variant only when ≥2 cards reuse the pattern ([#276](https://github.com/talelburg/eldritch/issues/276), PR #277).** The registry's `native_effect_for: fn(&str) -> Option<NativeEffectFn>` lets the `cards` crate supply Rust card-locally, dispatched by tag (`"<cardcode>:<name>"`); the bridge lives in the registry because `card-dsl` is below `game-core`. Genuinely-reused ops are shared variants (`AdvanceCurrentAct`, `SkillTest`, `ForEachPointFailed`); one-offs (Crypt Chill's asset discard, Ancient Evils' doom via `place_doom_on_current_agenda`) stay native. Orthogonal to #212 (which unifies *trigger dispatch*, not effect *supply*).

- **Forced "choose one" effects defer the interactive choice to #212, shipping a deterministic legal branch now (`TODO(#212)`) rather than building bespoke `AwaitingInput` suspension.** The engine lacks mid-forced-dispatch suspension (`ChooseOne` is a stub); #212 owns it. Used by agenda 01105 (the 2-horror branch; PR #283) and Crypt Chill 01167 (discard the first controlled asset). A recorded-randomness branch (01105's random discard) is rejected as a default because it needs replay-recorded randomness.

- **A card effect initiates a skill test via `Effect::SkillTest { skill, difficulty, on_fail }` with a margin-keyed `Effect::ForEachPointFailed` failure branch; a suspending Revelation discards via `GameState.pending_revelation_discard` (C4b prereq [#286](https://github.com/talelburg/eldritch/issues/286), PR #287).** `SkillTest` calls `start_skill_test` (always suspends at the commit window); `on_fail` (on `InFlightSkillTest.on_fail`, orthogonal to the success-side `follow_up`) runs on failure with the margin in `EvalContext::failed_by`. A test-initiating Revelation suspends, so `resolve_encounter_card` records the treachery in `pending_revelation_discard`, flushed at skill-test teardown (no-op for a plain Investigate — the seam C4c extends). **`Active` status = in-play, not turn ownership, so Mythos-phase treachery tests are legal.**

- **Threat-area scan source (C4a, [#233](https://github.com/talelburg/eldritch/issues/233), PR #285) + C4c ([#235](https://github.com/talelburg/eldritch/issues/235)) extension points.** `Investigator::controlled_card_instances()` (chains `cards_in_play` + `threat_area`) is the single scan source for both reaction-window and forced instance scans; cards enter/leave the threat area via `dispatch::threat_area::{place_in_threat_area, discard_from_threat_area}`. New forced points `EndOfTurn` and `AfterLocationInvestigated` (skill-test `PostOnResolution`, successful Investigate) landed. **C4c must:** thread the *source instance* into the forced `EvalContext` (`resolve_one` binds controller only); extend `AfterLocationInvestigated` to also scan the investigated *location's* attachment zone (Obscuring Fog 01168 attaches to a location, not the threat area); and extend `pending_revelation_discard` (above) for persistent treacheries that stay in the threat area instead of discarding. Suspension at these forced points is unmodeled (#212 reentrancy).

- **A persistent treachery is one with any non-Revelation ability; it owns its own disposition (C4c, [#235](https://github.com/talelburg/eldritch/issues/235), PR #289).** `resolve_encounter_card` auto-discards a treachery after its Revelation **only if every ability is `Trigger::Revelation`**; a card carrying a `Constant`/`OnEvent` ability places itself (threat area via `place_in_threat_area`, or location via `attach_to_location`) and discards itself later via the typed `Effect::DiscardSelf` (which finds the firing instance through `EvalContext::source`). No suppress-discard flag. **A new persistent treachery needs no routing change — give it an ongoing ability and a self-placement Revelation.** Constant restrictions extend the inspectable DSL (`Stat::Shroud`, `Restriction::{CannotPlay, ExtraActionCost}` under `Effect::Restrict`), read by `effective_shroud` / `play_is_prohibited` / `pending_action_surcharge` the way `constant_skill_modifier` already reads `Modify` — **not** via new registry query hooks. `ExtraActionCost`'s `first_each_round` stays a field (tracked per source instance in `Investigator.action_surcharge_spent_this_round`) until a second consumer needs the gate on a non-cost mechanism.

- **Simultaneous forced triggers resolve in a fixed deterministic order, and a suspending forced effect at end-of-turn resumes via `pending_end_turn` (C4c, PR #289).** `fire_forced_triggers` now resolves *all* collected hits in collection order (board cards before threat-area/attachment instances; `BTreeMap` order) instead of rejecting on 2+ — this is the partial #213 stand-in (player-chosen ordering is still #213). It lets Dissonant Voices' `RoundEnded` discard coexist with agenda 01107's `RoundEnded` doom. A hit that *suspends* abandons later hits (#212 reentrancy); safe while no point has 2+ simultaneous suspending hits. `Effect::SkillTest` gained a success-side `on_success` (mirror of `on_fail`); a suspending `EndOfTurn` forced effect (Frozen in Fear's willpower test) strands `end_turn` before rotation, so `end_turn` records `pending_end_turn` and the skill-test commit-resume path re-enters `resume_end_turn` (rotation / phase-end) — mirroring `spawn_engage_pending`/`resume_spawn_engage`.

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
