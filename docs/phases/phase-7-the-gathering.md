# Phase 7 — The Gathering

## Status

✅ **Slice 1 complete** (kickoff [#216](https://github.com/talelburg/eldritch/issues/216), gate [#245](https://github.com/talelburg/eldritch/issues/245)/PR #326).
Solo Roland plays The Gathering end-to-end to genuine Won + Lost
resolutions against the real registries; all Group C content reachable on
today's engine shipped, with the rest carved to tracked follow-ups (the
#212/#213 choice cluster + the per-card prereqs). Slice 2+ (investigator
breadth) is the next arc — see "Future slices" below.
Engine spine (A1/A2) and scenario plumbing (B1/B2) shipped; **Group C**
(the Gathering content, decomposed into C1–C7, kickoff
[#246](https://github.com/talelburg/eldritch/issues/246)) is done through
**C5c**, with **C5d** partially shipped (three engine-free Guardian
assets, PR #303) and **C5e** shipped down to its one implementable card:
its engine prereq (`OnCommit` firing + bonus-attack-damage, PR #308) plus
Vicious Blow 01025 (PR #309, closing #240); its other three cards are all
choice-/cancellation-/reaction-blocked. See the Group C breakdown table
below for per-sub-slice state. **Strategy: ship every card implementable
on today's engine, file follow-ups for the rest, then build the deferred
machinery (the #212/#213 choice cluster and friends) and return for
them.** Deferred so far: C5d's Beat Cop + First Aid
([#301](https://github.com/talelburg/eldritch/issues/301) /
[#302](https://github.com/talelburg/eldritch/issues/302)); C5e's Evidence!
/ Dodge / Dynamite Blast
([#304](https://github.com/talelburg/eldritch/issues/304)–[#306](https://github.com/talelburg/eldritch/issues/306)).
**C6 is complete** (C6a window, C6b Dr. Milan, C6c neutral cards, C6d
encounter deck). **Next: C7** — C7a (registry swap + web `SCENARIO_ID`
repoint) → C7b (the end-to-end Won/Lost integration test that closes
Slice 1).

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
| C5a | [#236](https://github.com/talelburg/eldritch/issues/236) | Cover Up before-timing interrupt + `GameEnd` | ✅ PR #291 |
| C5b | [#237](https://github.com/talelburg/eldritch/issues/237) | Guard Dog reaction + enemy-attack soak mechanic | ✅ PR #292 |
| — | [#295](https://github.com/talelburg/eldritch/issues/295) | infra: weapon support — ammo/uses (`Cost::SpendUses`) + inspectable `Effect::Fight` (`IntExpr` modifier + bonus damage) (prerequisite for C5c's .38 Special) | ✅ PR #297 |
| C5c | [#238](https://github.com/talelburg/eldritch/issues/238) | .38 Special signature + Cover Up content | ✅ PR #298 |
| C5d | [#239](https://github.com/talelburg/eldritch/issues/239) | Guardian L0 assets — .45 Automatic, Physical Training, Machete (PR #303); Guard Dog (C5b); **Beat Cop + First Aid** (PR #357, on the #301/#302 prereqs) | ✅ PR #303 + #357 |
| — | [#307](https://github.com/talelburg/eldritch/issues/307) | infra: `Trigger::OnCommit` firing + `Effect::BoostAttackDamage` / `InFlightSkillTest.bonus_attack_damage` (prerequisite for C5e's Vicious Blow) | ✅ PR #308 |
| C5e | [#240](https://github.com/talelburg/eldritch/issues/240) | Guardian L0 events + skill (×4) — **only Vicious Blow 01025 was implementable; shipped (PR #309).** Engine prereq landed (PR #308); Evidence! ([#304](https://github.com/talelburg/eldritch/issues/304), reaction-event-play), Dodge ([#305](https://github.com/talelburg/eldritch/issues/305), attack-cancellation), Dynamite Blast ([#306](https://github.com/talelburg/eldritch/issues/306), location-choice = #212/#213) carved to follow-ups | ✅ PR #309 |
| C6a | [#241](https://github.com/talelburg/eldritch/issues/241) | Dr. Milan after-investigate window | ✅ PR #318 |
| C6b | [#242](https://github.com/talelburg/eldritch/issues/242) | Seeker deck cards — **only Dr. Milan 01033 implementable** (its window: C6a); Old Book of Lore ([#319](https://github.com/talelburg/eldritch/issues/319)), Research Librarian ([#320](https://github.com/talelburg/eldritch/issues/320)), Medical Texts ([#321](https://github.com/talelburg/eldritch/issues/321)), Mind over Matter ([#322](https://github.com/talelburg/eldritch/issues/322)), Barricade ([#323](https://github.com/talelburg/eldritch/issues/323)) carved to follow-ups | ✅ PR #324 |
| — | [#310](https://github.com/talelburg/eldritch/issues/310) | infra: `Effect::DrawCards` primitive (prerequisite for C6c's draw-skills) | ✅ PR #314 |
| — | [#311](https://github.com/talelburg/eldritch/issues/311) | infra: enforce "Max N committed per skill test" commit cap (prerequisite for C6c's skills) | ✅ PR #315 |
| C6c | [#243](https://github.com/talelburg/eldritch/issues/243) | Neutral deck cards — **Emergency Cache 01088 + 5 skills** shipped on prereqs #310/#311; Knife 01086 ([#312](https://github.com/talelburg/eldritch/issues/312), discard-self-asset cost) + Flashlight 01087 ([#313](https://github.com/talelburg/eldritch/issues/313), `Effect::Investigate` + shroud) carved to follow-ups | ✅ PR #316 |
| C6d | [#284](https://github.com/talelburg/eldritch/issues/284) | encounter-deck assembly in `setup()` (quantity-aware, excludes set-aside) — gates C7b; makes Mythos draws + 01106's dig operate live | ✅ PR #317 |
| C7a | [#244](https://github.com/talelburg/eldritch/issues/244) | registry swap + web `SCENARIO_ID` repoint (B3) | ✅ PR #325 |
| C7b | [#245](https://github.com/talelburg/eldritch/issues/245) | end-to-end Won/Lost integration test (needs C6d) | ✅ PR #326 |

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
- **Engine north-star — trigger-dispatch rework (cross-slice; kickoff
  [#327](https://github.com/talelburg/eldritch/issues/327)).** `emit_event`
  dispatch unification (`#212`) + iterative simultaneous-trigger ordering
  (`#213`) + the trigger index (`#117`), unblocking the deferred-card
  choice/reaction cluster. Designed in
  [umbrella](../superpowers/specs/2026-06-16-trigger-dispatch-rework-umbrella-design.md)
  + [Axis-B foundation](../superpowers/specs/2026-06-16-trigger-dispatch-rework-axis-b-foundation-design.md)
  specs ([Axis-B plan](../superpowers/plans/2026-06-16-trigger-dispatch-axis-b-foundation.md)).
  Five axes: **B** trigger-dispatch spine (one continuation stack +
  two-phase forced-then-reaction `emit_event`; tasks
  [#328](https://github.com/talelburg/eldritch/issues/328)–[#332](https://github.com/talelburg/eldritch/issues/332)).
  **Substantive work done** — T1–T5 (#328–#332) shipped: the `emit_event`
  chokepoint + `TimingEvent` closed #212; the iterative lead-ordered forced
  run + reentrancy + the forced-before-reaction investigate collapse closed
  #213 (T5b, PR #343). The #117 **event-keyed trigger index is deferred** to
  Phase 4+ (zero perf return at Slice-1 board sizes vs. real index-invariant
  carrying cost; tracked in #117, plan recorded there) — its Axis-B task
  wrapper #333 was closed as redundant. #294 was re-examined and **kept open**
  (its multi-soak-window state is unconstructible in scope — see Decisions),
  not closed by Axis B. **A** interactive choice
  ([#334](https://github.com/talelburg/eldritch/issues/334)) — **✅ PR #350**:
  `Effect::ChooseOne` + `Location`/`Investigator::ChosenByController` + native-leaf
  picks on a `Continuation::Choice` frame (agenda 01105 + Crypt Chill 01167
  upgraded; see Decisions). **C**
  reaction-event-play ([#335](https://github.com/talelburg/eldritch/issues/335)),
  **D** cancellation/replacement ([#336](https://github.com/talelburg/eldritch/issues/336)),
  **E** orthogonal card prereqs (#301/#302/#306/#312/#313/#319/#320/#322/#323).
  The reachable subset of E (the six cards unblocked by Axes A+B) is sequenced
  as the **choice-cluster completion** sub-slice — 8 PRs mapped to existing
  issues, see
  [decomposition](../superpowers/specs/2026-06-17-phase-7-choice-cluster-completion-decomposition-design.md).
  Its keystone, the unified `Choose` surface (#349), shipped: **✅ PR #351**
  (see Decisions; supersedes Axis A's "ChosenByController offers all" note).
  PR-2 (#301 — `Cost::DiscardSelf` + the enemy variety + `Effect::DealDamageToEnemy`,
  Beat Cop's engine prereqs) shipped: **✅ PR #352**.
  PR-3 (#302 — `Effect::Heal` + uses-depletion auto-discard, First Aid's engine
  prereqs) shipped: **✅ PR #355**. PR-4 (#239 — the Beat Cop + First Aid
  *cards*) shipped: **✅ PR #357**, **closing C5d** (the last open Group-C
  sub-slice). The orthogonal #354 (`DealDamage`/`DealHorror` → `Effect::Deal`
  consolidation) also shipped (PR #356). PR-5 (#312 — Knife 01086, two
  `[action]` Fight abilities on existing primitives + `Cost::DiscardSelf`)
  shipped: **✅ PR #358**. Remaining cluster PRs: #321 (Medical Texts,
  on #302), #313 (Flashlight), #306 (Dynamite).
  Slice 1's `fire_forced_triggers` is a forward-compatible subset Axis B
  replaces. **Note:** #213's "one mixed pool" framing was corrected to
  RR-accurate two-phase (forced-all-before-reaction, RR p.2; issue text
  amended).

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

- **Before-timing clue-discovery interrupt is a card-local seam at the `discover_clue` chokepoint, not a general before-timing reaction-window subsystem (C5a, [#236](https://github.com/talelburg/eldritch/issues/236), PR #291).** When the controller holds a `WouldDiscoverClues` (`EventTiming::Before`) reaction, `discover_clue` suspends with a yes/no `AwaitingInput` (`GameState.clue_interrupt_pending`); `resume_clue_interrupt` (routed before the skill-test path in `resolve_input`) runs the card-local `Effect::Native` replacement on `Confirm` (count threaded via `EvalContext.clue_discovery_count`) or the deferred discovery on `Skip`. Reentrancy: `finish_skill_test` **pre-advances** its continuation to `PostFollowUp` before the Investigate follow-up, so a suspending discovery resumes through `in_flight_skill_test` without re-running the follow-up — **bounded to terminal-position discovery** (the base Investigate follow-up, the only Slice-1 clue source; nested-in-`Seq` is #212). **A future before-timing interrupt reuses this seam.** The seam's `card.clues > 0` eligibility gate is a **single-consumer stand-in for RR p.2's "ability must have potential to change the game state"** — the engine models this nowhere; lift it into a card-provided per-ability predicate when a 2nd `WouldDiscoverClues` card lands (`TODO(#212)`). Bespoke effects (discard-from-self, suffer-trauma) stay `Effect::Native`, integration-tested via `synth_cards::TEST_REGISTRY`.

- **`ForcedTriggerPoint::GameEnd` fires once from `fire_scenario_resolution` on the resolution latch; game-end trauma is `Event::TraumaSuffered`-only (C5a, PR #291).** It scans every investigator's `controlled_card_instances()` for `EventPattern::GameEnd` forced abilities, before the scenario-module `apply_resolution` hook (so it runs even with no module). Trauma persistence (campaign log, max-stat reduction) is **Phase 9** — C5a emits the event and mutates no state.

- **Enemy-attack damage/horror soak is `assign → place → defeat → window`; assignment is soak-first deterministic, with interactive distribution deferred to a reframed #44 (C5b, [#237](https://github.com/talelburg/eldritch/issues/237), PR #292).** `enemy_attack` builds soakers (controlled assets with `CardKind::Asset` remaining `health`/`sanity` capacity), `assign_attack` fills them by `CardInstanceId` order before the investigator (symmetric for damage/horror), `place_assignment` places simultaneously (RR p.7) then defeats overflowed assets (`accumulated_* >= printed stat` → discard), and returns surviving damaged assets. The window-queuing lives in the **caller** (`drive_attack_loop`), not `enemy_attack`, so the enemy phase opens reaction windows while attacks of opportunity don't (see next entry). **#44's remaining scope is now just the interactive `{target → points}` distribution** (replacing the soak-first `assign_attack` body, `TODO(#44)`); soak-first is the only deterministic default that makes a soak reaction observable. A new soak reaction adds an `EnemyAttackDamagedSelf` ability (bare; self-bound to the soaked instance via `scan_pending_triggers`) — **no routing change**; the attacking enemy reaches the effect via `EvalContext.attacking_enemy`. Guard Dog's retaliate is `Effect::Native` (first card to damage a specific enemy from a reaction; public entry `deal_damage_to_enemy`).

- **The enemy-phase attack loop suspends/resumes around a soak reaction window via `pending_enemy_attack`; attacks of opportunity soak but do NOT yet open the window (C5b, PR #292).** `drive_attack_loop` parks the remaining attackers and returns `AwaitingInput` when an attack opens a soak window; `resume_enemy_attack` (from the `AfterEnemyAttackDamagedAsset` window-close continuation) re-enters at the next attacker, advancing the enemy-phase cursor exactly once via the extracted `after_enemy_phase_attacks`. **AoO is the deferred gap:** full AoO reactions need a new mechanism to suspend/resume the *triggering action* (Move's relocation, Investigate's already-suspending skill test), so `fire_attacks_of_opportunity` deliberately drops the soak-window survivors (window-safe; Guard Dog soaks AoO damage but doesn't retaliate). The fast-follow ([#293](https://github.com/talelburg/eldritch/issues/293)) routes `fire_attacks_of_opportunity` through `drive_attack_loop` (`EnemyAttackSource::AttackOfOpportunity` is the reserved-but-unconstructed variant) + action suspension. Multi-soak-window-per-attack resume ([#294](https://github.com/talelburg/eldritch/issues/294)) is `debug_assert`-guarded (unreachable in Slice 1: only Guard Dog reacts, two copies need two illegal Ally slots; coordinates with #213).

- **A weapon needs no engine work — it's `Cost::SpendUses` + `Effect::Fight` data, with ammo from the corpus (C5c prereq [#295](https://github.com/talelburg/eldritch/issues/295), PR #297).** `Uses (N <kind>)` is pipeline-parsed into `CardKind::Asset.uses`; the kind enum (`UseKind`) lives in `card-dsl` so the printed metadata and the engine's `CardInPlay.uses` runtime pool share one type. A firearm's ability is `activated(cost, vec![Cost::SpendUses { kind, count }], fight(IntExpr::cond(LocationHasClues, hi, lo), extra_damage))` — the inspectable `Effect::Fight` auto-targets the single engaged enemy, snapshots its modifier onto `InFlightSkillTest.test_modifier`, and reuses the skill-test suspend/resume path; the Fight follow-up deals `1 + extra_damage`. **`Effect::Fight` is typed, not `Native`,** so `check_activate_ability` can reject a fire with ≠1 engaged enemy before charging (multi-target selection deferred to the #212/#213 cluster; `effect_initiates_fight` is top-level-only, `TODO(#212/#213)` for a `Seq`/`If`-nested Fight). Conditional numeric values use `IntExpr { Lit, Cond }` over the general `Condition` (e.g. `LocationHasClues`) rather than duplicating the effect in an `Effect::If`. **A future weapon (breadth slices) lands via corpus + this data — no new engine primitives.** Instance-id-mint / put-into-play helper consolidation surfaced here is deferred to [#296](https://github.com/talelburg/eldritch/issues/296).

- **C5d ships only the engine-free Guardian assets; Beat Cop + First Aid are split to engine follow-ups (C5d, PR #303).** .45 Automatic (01016), Physical Training (01017), and Machete (01020) are pure corpus + existing-primitive data (`Effect::Fight` / `Cost::SpendUses` / `ThisSkillTest` `Modify`). The other two need new primitives, so they're carved out the way #276/#286/#295 carved earlier C prereqs: Beat Cop's fast ability wants choose-target "deal damage to an enemy at your location" + a discard-self cost ([#301](https://github.com/talelburg/eldritch/issues/301)); First Aid wants `Effect::Heal` + uses-depletion auto-discard ([#302](https://github.com/talelburg/eldritch/issues/302)). #239 stays open tracking that content; Guard Dog (01021) already shipped in C5b. **Machete's "+1 damage if the attacked enemy is the only enemy engaged with you" is encoded as unconditional `extra_damage: 1`** — exact while Fight is single-target (`Effect::Fight` auto-targets the lone engaged enemy and rejects ≠1 before cost), with [#300](https://github.com/talelburg/eldritch/issues/300) revisiting when multi-target Fight lands.

- **C5e ships only Vicious Blow — the engine prereq ([#307](https://github.com/talelburg/eldritch/issues/307), PR #308) plus the card (PR #309, closing #240); the other three cards are blocked.** `Trigger::OnCommit` was never fired (compiled + serde-round-tripped, but no engine path ran a committed card's effect); `fire_on_commit` now runs committed cards' `OnCommit` effects at the commit step, **before** chaos resolution (committing precedes resolution), mirroring `fire_on_skill_test_resolution`. Vicious Blow's "+1 damage" is `Effect::BoostAttackDamage(u8)`, accumulated onto `InFlightSkillTest.bonus_attack_damage`; the Fight follow-up deals `1 + extra_damage + bonus_attack_damage`. **`OnCommit`, not `OnSkillTestResolution` (the Deduction trigger): ordering forces it** — the Fight follow-up deals the attack's damage *during* resolution, before `fire_on_skill_test_resolution` runs, so a post-resolution trigger can't modify "that attack" without dealing a separate damage instance (extra event, wrong semantics). `OnCommit` parameterizes the resolution instead. **The "during an attack" qualifier is a card-level kind gate** — `on_commit(if_(Condition::SkillTestKind(Fight), boost_attack_damage(1)))`, symmetric to Deduction's "while investigating"; every attack (Fight action *or* `Effect::Fight` weapon) runs a `SkillTestKind::Fight` test, so the gate captures exactly "an attack." Gating the accumulate (vs. relying on the Fight follow-up being the only reader) keeps the buff from leaking if a second reader of `bonus_attack_damage` lands. **"If successful" stays intrinsic** — the Fight follow-up consumes the bonus only on success, and the outcome isn't known at commit. The other three cards were carved to follow-ups blocked on bigger machinery — Evidence! ([#304](https://github.com/talelburg/eldritch/issues/304)) on reaction-event-play (the play-card gate defers card play-timing restrictions), Dodge ([#305](https://github.com/talelburg/eldritch/issues/305)) on a new attack-cancellation subsystem, Dynamite Blast ([#306](https://github.com/talelburg/eldritch/issues/306)) on the #212/#213 location-choice.

- **C6c's two prereqs: `Effect::DrawCards` (#310, PR #314) + commit-cap enforcement (#311, PR #315).** `Effect::DrawCards { target, count }` wraps the existing `cards::draw_cards` helper (widened to `pub(in crate::engine)`); `count == 0` is a no-op. The **"Max N committed per skill test"** cap is **pipeline-parsed metadata** (`CardKind::Skill.commit_limit: Option<u8>`, Skill-only in scope) enforced in `validate_commit_indices`, which now does a registry `metadata_for` lookup (count committed cards by code, reject over cap; no-op without a registry). A future non-Skill capped card moves `commit_limit` onto the relevant `CardKind` arm. With both prereqs landed, C6c's content (#243) — Emergency Cache + the five skills — is unblocked; Knife (#312) / Flashlight (#313) remain carved out.

- **"After you successfully investigate" is a reaction window (`AfterSuccessfulInvestigate`) distinct from the forced `AfterLocationInvestigated`, because the engine has no `Trigger::Forced` (C6a, [#241](https://github.com/talelburg/eldritch/issues/241), PR #318).** Dr. Milan 01033's `[reaction]` needs a player "may" window; the Investigate follow-up (success-only) queues `WindowKind::AfterSuccessfulInvestigate { investigator }`, which suspends/resumes through the existing reaction pipeline (`queue_reaction_window` → `close_reaction_window_at` re-enters the skill-test driver). It pairs with a **new** `EventPattern::SuccessfullyInvestigated` rather than reusing `AfterLocationInvestigated` (Obscuring Fog's forced twin): with no `Trigger::Forced`, the engine routes forced-vs-reaction **by pattern** — the forced one auto-fires via `fire_after_location_investigated`, the reaction one opens a window — so sharing a pattern would auto-fire a reaction (it scans the investigator's controlled instances). Unifying forced + reaction at one ordered window is #212/#213. Window is controller-scoped ("after *you* investigate"); a Cover-Up-suspended discovery doesn't queue it (`TODO(#212)`, out of Slice-1 scope). The Dr. Milan *card* ships in C6b (#242).

- **The Gathering's encounter deck is six sets, assembled in `setup()` and shuffled at `StartScenario` (C6d, [#284](https://github.com/talelburg/eldritch/issues/284), PR #317).** Per the vendored campaign guide (`data/campaign-guides/…notz…pdf` p.2) the gathered sets are **The Gathering, Rats, Ghouls, Striking Fear, Ancient Evils, Chilling Cold** — six, not four (Ancient Evils + Chilling Cold *are* in scenario I; verify against the guide, not memory). `setup()` seeds `encounter_deck` from `the_gathering::ENCOUNTER_DECK_CODES` (a guide-sourced code list — the corpus carries no `encounter_code`) × `CardKind::{Enemy,Treachery}.quantity` = 26 cards, excluding the set-aside Ghoul Priest (01116) / Lita (01117) and all structural cards by construction. **The shuffle lives in `start_scenario`** (alongside the player-deck shuffle, same scenario-start RNG), so `setup()`'s construction order isn't load-bearing and the deck is replayable — C7b can drive a real Mythos cadence. Tests that need a controlled draw order must seed `encounter_deck` *after* `StartScenario` (post-shuffle).

- **Slice 1 closes with a hybrid end-to-end Won/Lost test that drives the real act progression and seeds only off-resolution preconditions (C7b, [#245](https://github.com/talelburg/eldritch/issues/245), PR #326).** `crates/scenarios/tests/the_gathering_resolutions.rs` seats solo Roland via `setup()` + `StartScenario`, then: **Won** drives act 1 (`AdvanceAct`) and act 2 (the C3d round-end clue-spend window + `Confirm`) for real — act 2's reverse spawns the *real* Ghoul Priest — and fights that spawned enemy → `act_01110`'s forced advance → `Resolution::Won { R1 }`; **Lost** seeds Roland one-from-death + an engaged enemy and drives an Enemy-phase attack → `check_all_defeated` → `Resolution::Lost`. Both assert the genuine `state.resolution` latch + `Event::ScenarioResolved`. **Seeds are deliberately off the resolution path:** a controlled `Numeric(0)` chaos bag (the Standard bag's `AutoFail` makes determinism impossible), a minimal roster deck, seeded clues (clue-acquisition is unit-tested elsewhere; the Cellar's shroud 4 also exceeds Roland's intellect 3), the spawned Priest's health (solo Roland can't out-damage a 5-health Retaliate Hunter dealing 2 horror/attack without going insane), and one benign seeded Mythos draw (Ancient Evils, 1 doom). The earlier instinct to seed *past* act 2 was dropped in review — it duplicated `act_advancement.rs` and skipped the act-2 machinery the capstone exists to exercise.

- **2+ simultaneous forced abilities resolve through the *same* `Continuation::Resolution` loop as reaction windows (lead orders them, RR p.17), and that loop is reentrant across a forced effect that suspends into a skill test (Axis-B T5b, #213/#332, PR #343).** Supersedes the C4c fixed-deterministic-order stand-in *and* its abandon-on-suspend gap (both above). `ResolutionFrame.kind` is `Window(WindowBinding) | Forced(ForcedContinuation)`; a forced run carries a `ForcedContinuation` (`Terminal | UpkeepAfterRoundEnded | EndOfTurnAfterForced { investigator }`) naming the framework tail to resume on close. **`TimingEvent::forced_continuation()` is exhaustive and returns `None` for any non-terminal site with no wired continuation; `emit_event`'s 2+ branch turns `None` into `unreachable!`** — a loud guard, never a silently-dropped tail (no site produces 2+ forced in the current pool; wiring a *dual* site's continuation must also re-surface its queued reaction window, noted on that arm). Reentrancy hinges on **`drive_skill_test` reacting only to a reaction window *above* the in-flight `SkillTest` frame**, never a forced-run frame below it; a suspended candidate parks and its siblings resume via `advance_resolution` + the `resume_skill_test_commit` re-entry. End-of-turn rotation now has two mutually-exclusive mechanisms — the forced-run `EndOfTurnAfterForced` continuation (2+ hits) vs `pending_end_turn` (single hit; `end_turn` sets it only when no forced run is open). The successful-investigate dual site collapses into one `emit_event(SuccessfullyInvestigated)` so Obscuring Fog's forced discard precedes Dr. Milan's reaction window (RR p.2), **superseding the C6a by-pattern forced/reaction split** (above). #294's multi-soak-window resume stays a loud `debug_assert` (unconstructible in scope: Guard Dog is the only soak reactor / single Ally slot / `assign_attack` fills each soaker to capacity, defeating non-final ones); the issue is reworded and **kept open** for when player-chosen damage distribution lands.

- **Axis A interactive choice is single-pass suspend-and-replay on a `Continuation::Choice` frame, not the umbrella's two-pass split (#334, PR #350).** A choice node (`Effect::ChooseOne`, `Location`/`Investigator::ChosenByController`, or a native leaf) applies the canonical `resolve_choice_count` convention (`0 ⇒ reject/printed fallback · 1 ⇒ auto-bind · 2+ ⇒ suspend`); on suspend the `ChoiceFrame` records the picks-so-far + the **root** effect + the `EvalContext` ingredients (`controller`/`source`), and resume re-runs the whole tree from the top, replaying picks in pre-order via a `DecisionCursor` (which carries the root so a choice nested in a `ChooseOne` branch records the whole tree). **Two guards bound scope, both loud:** `apply_seq` rejects a choice that suspends after an earlier step (single-pass can't replay past a mutation → two-pass deferred to #346), and `apply_native`'s `debug_assert` rejects a native suspending after DSL picks (native↔DSL interleaving deferred). **The input contract is `InputResponse::PickSingle(OptionId)` + structured `InputRequest.options`** — a *new* family; the legacy `PickIndex` reaction-window path is untouched (`PickMultiple` has no Axis-A consumer). **DSL targets bind via `ground_chosen_targets`** (run before each handler) into `EvalContext.chosen_investigator`/`chosen_location`; **native leaves read `EvalContext.chosen_option`** and re-enumerate+index (Crypt Chill 01167, via the `pub` `suspend_for_native_choice`). A suspending skill-test `on_fail` (Crypt Chill) reuses the clue-interrupt reentrancy: `finish_skill_test` returns the `AwaitingInput` with its continuation pre-advanced to `PostFollowUp`, and `resume_choice` re-enters `drive_skill_test` for teardown. **`ChosenByController` offers *all* candidates** — the restricted "at your location" / chooser-is-the-lead forms are a deferred uniform-`Choose`-surface redesign (#349), triggered by the first constrained Axis-E card. Other follow-ups surfaced: serializable `EvalContext` (#345), `ResumeToken` routing/stale-submit (#347), continuation-stack cleanup of the remaining `pending_*` modes (#348).

- **The unified `Choose` surface (#349) unifies on the *spatial vocabulary*, not the monolithic `{variety, constraint, chooser}` (#349, PR #351).** `Choose<S> { scope }` wraps a variety-specific scope; the shared `LocationSet { Here, Anywhere }` is the chooser-relative spatial vocabulary, reused directly by location-picks and via `EntityScope { At(LocationSet) }` by entity-position-filters — so "your location" (`Here`) is defined **once** and illegal pairs (a location "at your location") are *unrepresentable*, no runtime guard. The target enums carry `Chosen(Choose<…>)` (replacing `ChosenByController` = `Chosen(…Anywhere)`, behavior-preserving); `ground_chosen_targets` forwards `scope` to per-variety enumerators (`Anywhere ⇒ all`, `At(Here)`/`Here` ⇒ controller's location, empty-⇒-reject when between locations), reusing Axis A's `Choice` frame/convention **unchanged**. **A future Axis-E entity choice adds an `EntityScope` arm (`Engaged`/`WithTrait`) additively — location-picks never see it; a new spatial term adds a `LocationSet` variant, available to both roles.** Deferred to consumers: the **enemy** variety + `chosen_enemy` (#301), `LocationSet::YourOrConnecting` + adjacency (#306), an explicit `chooser` (multiplayer / a non-lead chooser — latent in solo, where 01105's lead choice rides the forced `controller = lead` binding).

- **`Cost::DiscardSelf` discards the source asset in play (sole source-cost, paid last); `Effect::DealDamageToEnemy` is typed for a pre-cost target check (#301, PR #352).** `DiscardSelf` removes the source from `cards_in_play` → owner's discard (`CardDiscarded { InPlay }`), reusing the defeat-discard path; combining it with `Exhaust`/`SpendUses` is a loud reject (`reject_incompatible_costs`) and a missing source at payment is a loud `unreachable!`. The enemy variety ships as `EnemyTarget::Chosen(Choose<EntityScope>)`, reusing the keystone's `EntityScope::At`; `chosen_enemy` binds it; `combat::enemies_in_scope` is shared by the evaluator's grounding and the activation pre-cost check (`check_effect_target_available`, folded with the Fight check), which rejects 0-enemies-in-scope **before** paying — the reason `DealDamageToEnemy` is typed, not `Native` (Beat Cop can't pay its discard-self cost for no legal target). `≥1` proceeds; `2+` suspends via the Choose resolver. Beat Cop's content (PR-4 #239) and Knife's discard-self cost (PR-5 #312) are now unblocked.

- **`Effect::Heal` (the engine's first heal, reusing `InvestigatorTarget::Chosen`) + uses-depletion auto-discard via a pipeline-parsed `Uses.discard_when_empty` flag (#302, PR #355).** `Heal { kind: HarmKind, target, count }` saturating-reduces damage/horror (`Event::Healed`, only when something heals); First Aid's "damage or horror" is `ChooseOne([Heal{Damage}, Heal{Horror}])`. `HarmKind { Damage, Horror }` is shared — the `DealDamage`/`DealHorror` consolidation reusing it is **#354**. `Uses.discard_when_empty` is parsed from the templated `If <name> has no <kind>, discard it` clause (RR p.27; true for First Aid 01019 / Forbidden Knowledge 01058 / Grotesque Statue 01071); the depletion-discard checks at `SpendUses` payment via a `discard_card_from_play` helper shared with `Cost::DiscardSelf`. **First-Aid-correct; rules-precise post-resolution timing + effect-depletion cards (Forbidden Knowledge depletes via *effect*, not cost) + the mid-payment source-removal hazard are deferred to #353.** First Aid (PR-4 #239) and Medical Texts' heal (PR-6 #321) are now unblocked.

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
