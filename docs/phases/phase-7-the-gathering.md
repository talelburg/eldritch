# Phase 7 — The Gathering

## Status

🛠️ **Slice 1 in progress** (kickoff [#216](https://github.com/talelburg/eldritch/issues/216)).
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
**Next: C6** (C6d gates C7b) **→ C7.**

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
| C5d | [#239](https://github.com/talelburg/eldritch/issues/239) | Guardian L0 assets — **3 engine-free shipped** (.45 Automatic, Physical Training, Machete); Beat Cop + First Aid deferred to [#301](https://github.com/talelburg/eldritch/issues/301) / [#302](https://github.com/talelburg/eldritch/issues/302); Guard Dog already in C5b | 🛠️ PR #303 (partial; #239 open) |
| — | [#307](https://github.com/talelburg/eldritch/issues/307) | infra: `Trigger::OnCommit` firing + `Effect::BoostAttackDamage` / `InFlightSkillTest.bonus_attack_damage` (prerequisite for C5e's Vicious Blow) | ✅ PR #308 |
| C5e | [#240](https://github.com/talelburg/eldritch/issues/240) | Guardian L0 events + skill (×4) — **only Vicious Blow 01025 was implementable; shipped (PR #309).** Engine prereq landed (PR #308); Evidence! ([#304](https://github.com/talelburg/eldritch/issues/304), reaction-event-play), Dodge ([#305](https://github.com/talelburg/eldritch/issues/305), attack-cancellation), Dynamite Blast ([#306](https://github.com/talelburg/eldritch/issues/306), location-choice = #212/#213) carved to follow-ups | ✅ PR #309 |
| C6a | [#241](https://github.com/talelburg/eldritch/issues/241) | Dr. Milan after-investigate window | — |
| C6b | [#242](https://github.com/talelburg/eldritch/issues/242) | Seeker deck cards | — |
| — | [#310](https://github.com/talelburg/eldritch/issues/310) | infra: `Effect::DrawCards` primitive (prerequisite for C6c's draw-skills) | ✅ PR #314 |
| — | [#311](https://github.com/talelburg/eldritch/issues/311) | infra: enforce "Max N committed per skill test" commit cap (prerequisite for C6c's skills) | ✅ PR #315 |
| C6c | [#243](https://github.com/talelburg/eldritch/issues/243) | Neutral deck cards — **Emergency Cache 01088 + 5 skills** shippable (prereqs #310/#311); Knife 01086 ([#312](https://github.com/talelburg/eldritch/issues/312), discard-self-asset cost) + Flashlight 01087 ([#313](https://github.com/talelburg/eldritch/issues/313), `Effect::Investigate` + shroud) carved to follow-ups | — |
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

- **Before-timing clue-discovery interrupt is a card-local seam at the `discover_clue` chokepoint, not a general before-timing reaction-window subsystem (C5a, [#236](https://github.com/talelburg/eldritch/issues/236), PR #291).** When the controller holds a `WouldDiscoverClues` (`EventTiming::Before`) reaction, `discover_clue` suspends with a yes/no `AwaitingInput` (`GameState.clue_interrupt_pending`); `resume_clue_interrupt` (routed before the skill-test path in `resolve_input`) runs the card-local `Effect::Native` replacement on `Confirm` (count threaded via `EvalContext.clue_discovery_count`) or the deferred discovery on `Skip`. Reentrancy: `finish_skill_test` **pre-advances** its continuation to `PostFollowUp` before the Investigate follow-up, so a suspending discovery resumes through `in_flight_skill_test` without re-running the follow-up — **bounded to terminal-position discovery** (the base Investigate follow-up, the only Slice-1 clue source; nested-in-`Seq` is #212). **A future before-timing interrupt reuses this seam.** The seam's `card.clues > 0` eligibility gate is a **single-consumer stand-in for RR p.2's "ability must have potential to change the game state"** — the engine models this nowhere; lift it into a card-provided per-ability predicate when a 2nd `WouldDiscoverClues` card lands (`TODO(#212)`). Bespoke effects (discard-from-self, suffer-trauma) stay `Effect::Native`, integration-tested via `synth_cards::TEST_REGISTRY`.

- **`ForcedTriggerPoint::GameEnd` fires once from `fire_scenario_resolution` on the resolution latch; game-end trauma is `Event::TraumaSuffered`-only (C5a, PR #291).** It scans every investigator's `controlled_card_instances()` for `EventPattern::GameEnd` forced abilities, before the scenario-module `apply_resolution` hook (so it runs even with no module). Trauma persistence (campaign log, max-stat reduction) is **Phase 9** — C5a emits the event and mutates no state.

- **Enemy-attack damage/horror soak is `assign → place → defeat → window`; assignment is soak-first deterministic, with interactive distribution deferred to a reframed #44 (C5b, [#237](https://github.com/talelburg/eldritch/issues/237), PR #292).** `enemy_attack` builds soakers (controlled assets with `CardKind::Asset` remaining `health`/`sanity` capacity), `assign_attack` fills them by `CardInstanceId` order before the investigator (symmetric for damage/horror), `place_assignment` places simultaneously (RR p.7) then defeats overflowed assets (`accumulated_* >= printed stat` → discard), and returns surviving damaged assets. The window-queuing lives in the **caller** (`drive_attack_loop`), not `enemy_attack`, so the enemy phase opens reaction windows while attacks of opportunity don't (see next entry). **#44's remaining scope is now just the interactive `{target → points}` distribution** (replacing the soak-first `assign_attack` body, `TODO(#44)`); soak-first is the only deterministic default that makes a soak reaction observable. A new soak reaction adds an `EnemyAttackDamagedSelf` ability (bare; self-bound to the soaked instance via `scan_pending_triggers`) — **no routing change**; the attacking enemy reaches the effect via `EvalContext.attacking_enemy`. Guard Dog's retaliate is `Effect::Native` (first card to damage a specific enemy from a reaction; public entry `deal_damage_to_enemy`).

- **The enemy-phase attack loop suspends/resumes around a soak reaction window via `pending_enemy_attack`; attacks of opportunity soak but do NOT yet open the window (C5b, PR #292).** `drive_attack_loop` parks the remaining attackers and returns `AwaitingInput` when an attack opens a soak window; `resume_enemy_attack` (from the `AfterEnemyAttackDamagedAsset` window-close continuation) re-enters at the next attacker, advancing the enemy-phase cursor exactly once via the extracted `after_enemy_phase_attacks`. **AoO is the deferred gap:** full AoO reactions need a new mechanism to suspend/resume the *triggering action* (Move's relocation, Investigate's already-suspending skill test), so `fire_attacks_of_opportunity` deliberately drops the soak-window survivors (window-safe; Guard Dog soaks AoO damage but doesn't retaliate). The fast-follow ([#293](https://github.com/talelburg/eldritch/issues/293)) routes `fire_attacks_of_opportunity` through `drive_attack_loop` (`EnemyAttackSource::AttackOfOpportunity` is the reserved-but-unconstructed variant) + action suspension. Multi-soak-window-per-attack resume ([#294](https://github.com/talelburg/eldritch/issues/294)) is `debug_assert`-guarded (unreachable in Slice 1: only Guard Dog reacts, two copies need two illegal Ally slots; coordinates with #213).

- **A weapon needs no engine work — it's `Cost::SpendUses` + `Effect::Fight` data, with ammo from the corpus (C5c prereq [#295](https://github.com/talelburg/eldritch/issues/295), PR #297).** `Uses (N <kind>)` is pipeline-parsed into `CardKind::Asset.uses`; the kind enum (`UseKind`) lives in `card-dsl` so the printed metadata and the engine's `CardInPlay.uses` runtime pool share one type. A firearm's ability is `activated(cost, vec![Cost::SpendUses { kind, count }], fight(IntExpr::cond(LocationHasClues, hi, lo), extra_damage))` — the inspectable `Effect::Fight` auto-targets the single engaged enemy, snapshots its modifier onto `InFlightSkillTest.test_modifier`, and reuses the skill-test suspend/resume path; the Fight follow-up deals `1 + extra_damage`. **`Effect::Fight` is typed, not `Native`,** so `check_activate_ability` can reject a fire with ≠1 engaged enemy before charging (multi-target selection deferred to the #212/#213 cluster; `effect_initiates_fight` is top-level-only, `TODO(#212/#213)` for a `Seq`/`If`-nested Fight). Conditional numeric values use `IntExpr { Lit, Cond }` over the general `Condition` (e.g. `LocationHasClues`) rather than duplicating the effect in an `Effect::If`. **A future weapon (breadth slices) lands via corpus + this data — no new engine primitives.** Instance-id-mint / put-into-play helper consolidation surfaced here is deferred to [#296](https://github.com/talelburg/eldritch/issues/296).

- **C5d ships only the engine-free Guardian assets; Beat Cop + First Aid are split to engine follow-ups (C5d, PR #303).** .45 Automatic (01016), Physical Training (01017), and Machete (01020) are pure corpus + existing-primitive data (`Effect::Fight` / `Cost::SpendUses` / `ThisSkillTest` `Modify`). The other two need new primitives, so they're carved out the way #276/#286/#295 carved earlier C prereqs: Beat Cop's fast ability wants choose-target "deal damage to an enemy at your location" + a discard-self cost ([#301](https://github.com/talelburg/eldritch/issues/301)); First Aid wants `Effect::Heal` + uses-depletion auto-discard ([#302](https://github.com/talelburg/eldritch/issues/302)). #239 stays open tracking that content; Guard Dog (01021) already shipped in C5b. **Machete's "+1 damage if the attacked enemy is the only enemy engaged with you" is encoded as unconditional `extra_damage: 1`** — exact while Fight is single-target (`Effect::Fight` auto-targets the lone engaged enemy and rejects ≠1 before cost), with [#300](https://github.com/talelburg/eldritch/issues/300) revisiting when multi-target Fight lands.

- **C5e ships only Vicious Blow — the engine prereq ([#307](https://github.com/talelburg/eldritch/issues/307), PR #308) plus the card (PR #309, closing #240); the other three cards are blocked.** `Trigger::OnCommit` was never fired (compiled + serde-round-tripped, but no engine path ran a committed card's effect); `fire_on_commit` now runs committed cards' `OnCommit` effects at the commit step, **before** chaos resolution (committing precedes resolution), mirroring `fire_on_skill_test_resolution`. Vicious Blow's "+1 damage" is `Effect::BoostAttackDamage(u8)`, accumulated onto `InFlightSkillTest.bonus_attack_damage`; the Fight follow-up deals `1 + extra_damage + bonus_attack_damage`. **`OnCommit`, not `OnSkillTestResolution` (the Deduction trigger): ordering forces it** — the Fight follow-up deals the attack's damage *during* resolution, before `fire_on_skill_test_resolution` runs, so a post-resolution trigger can't modify "that attack" without dealing a separate damage instance (extra event, wrong semantics). `OnCommit` parameterizes the resolution instead. **The "during an attack" qualifier is a card-level kind gate** — `on_commit(if_(Condition::SkillTestKind(Fight), boost_attack_damage(1)))`, symmetric to Deduction's "while investigating"; every attack (Fight action *or* `Effect::Fight` weapon) runs a `SkillTestKind::Fight` test, so the gate captures exactly "an attack." Gating the accumulate (vs. relying on the Fight follow-up being the only reader) keeps the buff from leaking if a second reader of `bonus_attack_damage` lands. **"If successful" stays intrinsic** — the Fight follow-up consumes the bonus only on success, and the outcome isn't known at commit. The other three cards were carved to follow-ups blocked on bigger machinery — Evidence! ([#304](https://github.com/talelburg/eldritch/issues/304)) on reaction-event-play (the play-card gate defers card play-timing restrictions), Dodge ([#305](https://github.com/talelburg/eldritch/issues/305)) on a new attack-cancellation subsystem, Dynamite Blast ([#306](https://github.com/talelburg/eldritch/issues/306)) on the #212/#213 location-choice.

- **C6c's two prereqs: `Effect::DrawCards` (#310, PR #314) + commit-cap enforcement (#311, PR #315).** `Effect::DrawCards { target, count }` wraps the existing `cards::draw_cards` helper (widened to `pub(in crate::engine)`); `count == 0` is a no-op. The **"Max N committed per skill test"** cap is **pipeline-parsed metadata** (`CardKind::Skill.commit_limit: Option<u8>`, Skill-only in scope) enforced in `validate_commit_indices`, which now does a registry `metadata_for` lookup (count committed cards by code, reject over cap; no-op without a registry). A future non-Skill capped card moves `commit_limit` onto the relevant `CardKind` arm. With both prereqs landed, C6c's content (#243) — Emergency Cache + the five skills — is unblocked; Knife (#312) / Flashlight (#313) remain carved out.

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
