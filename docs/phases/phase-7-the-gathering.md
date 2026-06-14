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
spawn the set-aside Ghoul Priest + reveal the Parlor), and the agenda
reverses ([#281](https://github.com/talelburg/eldritch/issues/281):
01105/01106 + the `AgendaAdvanced` forced point). **Next: C4 → C7.**

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
- **Registry swap (B3, [#222](https://github.com/talelburg/eldritch/issues/222)) folds into Group C, not a standalone PR.** `server` already depends on `cards` and installs the real `scenarios::REGISTRY`; the only work is swapping the *card* registry (`synth_cards::TEST_REGISTRY` → `cards::REGISTRY`) in `server/src/lib.rs`. But that swap is coupled to C: the synthetic scenario's encounter deck draws synth-only card codes, so swapping with no real scenario to serve would break the `"synthetic"` web demo mid-play and `server/tests/registries.rs` for the whole B3→C window. So the swap + the web `SCENARIO_ID` repoint to `"the-gathering"` land in the Group C PR alongside the real scenario; synthetic registries stay for per-process tests.

- **Scenario investigator placement uses `GameState.starting_location` (C1a, [#227](https://github.com/talelburg/eldritch/issues/227)).** `setup()` can't seat investigators — they're created later by the `StartScenario` roster action — so it records the starting location on `GameState.starting_location` and the seating step places each seated investigator there (`None` keeps the legacy pre-seated path). Every scenario's `setup()` sets this; it's the generic placement channel for the `setup() → roster-seating` split. C1a also fixes the faithful **Study-only Act-1 board**: `setup()` builds *only* the Study (isolated); the four set-aside locations + the Act-1 "Door on the Floor" transition are **C1b** ([#228](https://github.com/talelburg/eldritch/issues/228)), which also replaces act 01110's placeholder clue threshold with its real "Ghoul Priest defeated" objective.

- **Card metadata is a `CardKind` enum, and encounter cards + their stats are in the corpus** ([#254](https://github.com/talelburg/eldritch/issues/254) remodel, [#252](https://github.com/talelburg/eldritch/issues/252) ingestion — both infra PRs, not phase-7 issues, but load-bearing for all of Group C). `CardMetadata` is now an identity core + a `kind: CardKind` enum (`Investigator`/`Asset`/`Event`/`Skill`/`Enemy`/`Location`/`Act`/`Agenda`/`Treachery`); read `card_type()`/`class()` via accessors and match on `kind` for type-specific stats. The 8 in-scope encounter files are ingested, so **locations/acts/agendas/enemies/treacheries carry their printed stats in the corpus** (`CardKind::Location { shroud, clues, victory }`, `Act { clue_threshold, victory }`, `Agenda { doom_threshold }`, `Enemy { fight, evade, damage, horror, health, victory, quantity, … }`). Consequences for Group C: read stats via `cards::by_code`/`metadata_for` instead of hand-typing (C1b set-aside locations, C2 victory points, C3 enemy stats, C4 treachery `quantity`); `scenario`-type cards (e.g. ref card `01104`) are **not** in the corpus — their effects live in `abilities()` impls; and C3b (#231) must wire `spawn_enemy` to read combat stats from `CardKind::Enemy` (it still hardcodes `fight: 1, evade: 1`).

- **`emit_event` unification (#212) lands *after* Group C, not before it.** C is built on the existing `ForcedTriggerPoint` enum-dispatcher + reaction-window pipeline, extended with new timing points (`RoundEnded`, `EndOfTurn`, `AfterLocationInvestigated`, `GameEnd`, damage/investigate windows) as content demands them; #212 then consolidates those into one emit-driven chokepoint, validated against all of C's real content. The dispatch surface C adds is a handful of points through already-generic machinery (the 7 treacheries share one Revelation hook; locations and Beat Cop reuse existing paths), so front-loading #212 would design its event taxonomy before the cards defining its requirements exist. **#213** (player-choice simultaneous-trigger ordering) is deferred further still — until then simultaneous triggers resolve in a fixed **deterministic** order. C4a (#233) lands the one in-C consolidation seam (shared scan source over `cards_in_play` + threat area) that #212 later absorbs.

- **Act-2 (01109)'s round-end objective moved out of C1b; it is now its own sub-slice C3d ([#275](https://github.com/talelburg/eldritch/issues/275)), split from C3c. C1b (#228, PR #259) shipped Pillars 1+3 only.** A faithful "when the round ends, investigators in the hallway *may*, as a group, spend clues to advance" requires the engine to *pause at round end* — a suspendable round-end **player window** (threading `upkeep_phase_end`, which returns `()` today, plus `AdvanceAct` re-gating + a Hallway contributor filter). It was briefly folded into C3c (both want round-end timing), but on review the agenda's round-end **doom is a fire-and-forget forced effect that does not need the suspendable window**, so the two are cleanly separable: **C3c (#232) is agenda-only**; the act-2 window is **C3d**. C1b left act 2 on the interim action-driven `AdvanceAct`. C1b's two pillars (Act-1 reverse board-build via a Forced `OnEvent(ActAdvanced)` ability on the act card; Act-3 forced advance on the Ghoul Priest's defeat via `ForcedTriggerPoint::EnemyDefeated` → `Effect::AdvanceCurrentAct`) both ride the existing forced-trigger rails — adding `EventPattern::EnemyDefeated`'s code narrow dropped `Copy` from `EventPattern`/`Trigger`, and `Enemy` gained a `code` field. New deferral issues filed: **#257** (location reveal-on-entry + per-investigator clues — currently the `revealed` field is dormant) and **#258** (Lita Chantler / Parlor barrier / Resign).

- **Scenario chaos-symbol effects live on `ScenarioModule.resolve_symbol`, not card `abilities()` (C2, [#229](https://github.com/talelburg/eldritch/issues/229), PR #263).** A reference card is one-per-scenario, never a card-object (never played/revealed/moved), and board-dependent, so it doesn't fit the `abilities()` model; the scenario module — already the context-taking home for `setup`/`apply_resolution` — owns it via a plain-Rust `fn(ChaosToken, &SymbolCtx) -> SymbolOutcome` hook returning a `modifier` (applied to the total before pass/fail) plus `immediate`/`on_fail` `TokenEffect`s (applied after, routed through the existing `DealDamage`/`DealHorror` paths). **No new DSL primitives** — 01104's Ghoul-count is inline Rust in `the_gathering.rs`. **Future scenarios' reference cards add a `resolve_symbol` hook, not card impls.** This removed B1's dead `reference_card` field + `active_reference_card` lookup ([#223](https://github.com/talelburg/eldritch/issues/223)) in the same PR; the static `TokenModifiers` path remains for hook-less fixtures.

- **Location victory points are placed at scenario resolution, not on clear (C2, [#229](https://github.com/talelburg/eldritch/issues/229), PR #263).** Per RR p.21 ("at the end of a scenario, place each victory point location that is in play, revealed, and with no clues on it in the victory display"), the engine generically scans `state.locations` at the `fire_scenario_resolution` chokepoint and places qualifying victory-bearing locations (reading `CardKind::Location { victory }` from the corpus) into a new `GameState.victory_display: Vec<CardCode>`. The victory-point **enemy** path (place as defeated) landed in **PR #274 ([#273](https://github.com/talelburg/eldritch/issues/273))**: enemies carry `victory` on the `Enemy` struct (set at spawn from `CardKind::Enemy`), and `damage_enemy` pushes a defeated victory-bearing enemy into `victory_display` at defeat time (it can't be scanned at resolution like locations — the enemy is removed on defeat). Phase 9 sums the zone for XP.

- **Enemy keywords / spawn-location / per-investigator health are parsed in the pipeline into the corpus, not hand-written per-enemy (C3b, [#231](https://github.com/talelburg/eldritch/issues/231), PR #272).** `CardKind::Enemy` gained `hunter`/`retaliate`/`prey`, and `health: Option<u8>` became `Option<HealthValue>` (a new enum mirroring `ClueValue`; polarity flipped — ArkhamDB `health_per_investigator` defaults false → `Fixed`). The pipeline parses these from card text (incl. resolving `Spawn - <name>` to a location code via a name→code index over Location cards); `spawn_enemy` reads all stats/keywords from the corpus and scales `PerInvestigator` health by the in-game investigator count (same source as the per-investigator clue path in `reveal.rs`). **Future enemies need no hand-written impl** — they land via the snapshot + a regen. Out-of-scope keyword forms not yet modeled (e.g. `Prey - Most clues`, `Spawn - Engaged with Prey`) default + emit a build warning rather than failing or silently approximating. `surge`/`peril` remain unparsed (#138).

- **Bespoke card logic lives card-locally via `Effect::Native { tag }`, not new shared `Effect` variants ([#276](https://github.com/talelburg/eldritch/issues/276), PR #277).** The registry's only behavior hook is `abilities_for → Vec<Ability>` and an `Ability`'s behavior is an `Effect`, so "implement in Rust" previously meant adding a variant to the shared `card_dsl::Effect` enum — accreting single-use scenario logic (C1b added three such variants). `Effect::Native { tag }` (serializable; `Effect` keeps its serde contract) + a new `CardRegistry.native_effect_for: fn(&str) -> Option<NativeEffectFn>` (`NativeEffectFn = fn(&mut Cx, &EvalContext) -> EngineOutcome`) lets the `cards` crate supply the Rust card-locally, dispatched by tag (convention `"<cardcode>:<name>"`). The bridge lives in the registry because `card-dsl` is below `game-core` and can't name `GameState`/`Cx`; `Cx` + `location_id_by_code`/`reveal_location` are now `pub`. **Add an `Effect` variant only when logic is genuinely reused across cards** (e.g. `AdvanceCurrentAct`); single-use logic is a native fn. C1b's three variants were migrated to `act_01108`'s `01108:board-build` native fn and removed. Orthogonal to #212 (that unifies *trigger dispatch*, not effect *supply*).

- **`upkeep_phase_end` is now suspendable; act round-end objectives are a kernel `Act.round_end_advance` field (C3d, [#275](https://github.com/talelburg/eldritch/issues/275), PR #279).** `upkeep_phase_end` returns `EngineOutcome` (both callers propagate) and, after the round-end forced dispatch, opens a Confirm/Skip window when the current act carries `round_end_advance: Some(RoundEndAdvance { contributor_location })` **and** the investigators at that location can afford the act's `clue_threshold`. Suspension uses `act_round_end_pending` + an action-gate guard + `resolve_input` routing + `resume_act_round_end_advance`, mirroring hand-size discard; Confirm spends from the contributor-location investigators and `advance_act`s, Skip continues, either way Upkeep→Mythos. `AdvanceAct` is re-gated to reject for round-end-advance acts (act-1 still uses it; act-3 uses its forced `EnemyDefeated`). **Modeling:** the threshold stays corpus-sourced (`CardKind::Act`), but the objective shape is **content-set** in `the_gathering.rs` — ArkhamDB has no structured field for it, it's a single consumer, and sibling act objectives (01108/01110) are likewise hand-authored. A card-local native effect was rejected: the window lifecycle + Upkeep→Mythos continuation are inherently kernel, and suspendable forced/native dispatch is the #212/#213 north-star. **Future round-end act objectives reuse this field + window**; multi-investigator clue *allocation* stays the deterministic spend (#153).

- **`RoundEnded` is a distinct framework timing point, separate from `PhaseEnded { Upkeep }` (C3c, [#232](https://github.com/talelburg/eldritch/issues/232), PR #278).** `EventPattern::RoundEnded` + `ForcedTriggerPoint::RoundEnded` fire in `upkeep_phase_end` *after* the upkeep-phase-end forced dispatch ("Upkeep phase ends. Round ends.", RR p.24). Kept separate so an end-of-upkeep-phase and an end-of-round card can coexist without conflation. Agenda 01107's two abilities are card-local native fns (per the #276 decision): `01107:move-ghouls` (enemy-phase-end, unengaged Ghouls step toward the Parlor — deterministic lowest-`LocationId` tie-break, unreachable on this star map; engagement-on-arrival unmodeled) and `01107:round-end-doom` (1 doom per Ghoul in Hallway/Parlor, no threshold check — RR checks doom at Mythos 1.3). **C3d ([#275](https://github.com/talelburg/eldritch/issues/275)) reuses this `RoundEnded` point** for act-2's round-end window. `shortest_first_steps` is now `pub`.

- **Set-aside *enemies* record a code only; the `Enemy` is minted at spawn (#280, PR #282).** Set-aside *locations* are fully built in `setup()` (`set_aside_locations: Vec<Location>`), but an enemy's per-investigator health (Ghoul Priest 01116 = 5×N) depends on the investigator count, unknown at `setup()` — so `set_aside_enemies: Vec<CardCode>` stores codes and `spawn_set_aside_enemy` mints stats from the corpus when a card effect brings the enemy into play. The shared spawn core is `spawn_enemy_at(cx, controller, code, metadata, location)`, factored out of `spawn_enemy` and reused by both the encounter-draw path (location from the card's spawn rule) and the set-aside path (location named by the bringing effect). **Future set-aside enemies reuse this field + helper, not a new mechanism.** Act-2's reverse (`act_01109`) is the first consumer; "put Lita into play" stays deferred to #258.

- **Agenda reverses fire via an `AgendaAdvanced` forced point mirroring `ActAdvanced`; 01105's interactive choice is deferred to #212 (#281, PR #283).** `advance_agenda` now fires `ForcedTriggerPoint::AgendaAdvanced { code }` (lead-bound) the way `advance_act` fires `ActAdvanced` — the forward-compatible subset #212 will absorb. **01106** is fully deterministic (reshuffle discard → dig the encounter deck discarding non-Ghoul cards → lead draws the Ghoul via the now-public `resolve_encounter_card`/`reshuffle_encounter_discard`); it is a faithful no-op until the encounter deck is assembled (a later C-sub), and is tested against a seeded deck. **01105 is a lead *choice*** (each discards 1 random / lead takes 2 horror) that needs suspendable mid-forced-dispatch `AwaitingInput` — which the engine lacks (`ChooseOne` is a stub; the Mythos 1.3 doom-check resolves inline) and which **#212** ("mid-emit `AwaitingInput` suspension"; absorbs `fire_forced_triggers`) owns. So 01105 ships the deterministic 2-horror branch (a legal outcome, like act-3's R1/R2 single-latch deferral), `TODO(#212)`; the random-discard branch was rejected as the default because it needs *recorded* randomness for replay. **Pattern for future forced "choose one" reverses: defer the interactive choice to #212 with a deterministic legal branch rather than building bespoke suspension now.**

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
