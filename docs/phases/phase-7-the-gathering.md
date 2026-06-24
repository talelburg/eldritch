# Phase 7 — The Gathering

## Status

**The engine foundation for the solo gate is complete.** Slice 1 (solo Roland
playing The Gathering end-to-end to Won + Lost, kickoff #216 / gate #245 /
PR #326) shipped, and so did every architectural arc the gate needed (see
**What shipped** below). What remains is a small **rules-correctness cluster**
plus the **browser capstone** — the detail is in **Remaining gate work**.

**Phase 7 is the 1-player solo rules-correctness gate.** Scope is deliberately
narrow — **1 player, 1 investigator, Standard**. Investigator breadth,
difficulty, solo-2, and optional content are **Future slices**.

## Goal

A solo human, in the browser, picks an investigator, sets up The Gathering,
and plays it to a resolution — **rules-correct for 1-player Standard**.

## What shipped (retrospective)

The blow-by-blow lives in the closed issues, git history, and the
`docs/superpowers/specs/2026-06-*` design docs; only the load-bearing residue is
in **Architecture to build on**. In dependency order, the arcs that landed:

1. **Continuation-stack cleanup** (#345/#348/#380) — normalized the
   `InputResponse` channel (`PickSingle`/`PickMultiple`/`Confirm`/`Skip`), folded
   every `*_pending` side-channel onto continuation frames.
2. **#393 unified control-flow model** (C checkpoint) — every suspending/looping
   step is a continuation frame; the main loop's one rule is **handle the top
   frame**. `InvestigatorTurn` re-emits legal actions as `OptionId`s, so the stack
   is non-empty during play. (`AttackLoop` cursor-lift, PR #412.)
3. **Keystone — mid-action park/resume** (K1–K5b, #293/#379/#361/#378/#143/#44) —
   AoO, retaliate, activated-ability & non-fast card-play AoO, player attack-order,
   and interactive damage/horror soak distribution all park their triggering action
   on an `ActionResolution`/`AttackLoop`/`DamageAssignment` frame and resume under a
   re-validation gate. PR #424 reified the **effect evaluator as continuation
   frames** (retiring suspend-and-replay + `DecisionCursor` + `Continuation::Choice`).
4. **Skill-test player windows** — #374 (ST.1/ST.2 fast-play windows; Hyperawareness,
   Magnifying Glass) and #64 (after-resolution reaction window; Dr. Milan 01033).
5. **EmitEvent-frame arc A→D** (#435 umbrella, #433/#434/#431/#423) — event
   emission, windows, and the `when/at/after × forced/reaction` matrix are all
   `drive`-loop-dispatched frames. Final slice PR #446 deleted `apply_effect` /
   `drive_effect_to_base`; every effect site is now top-frame dispatched.

## Remaining gate work

In dependency-friendly order.

**1. `IntExpr` correctness cluster.** **DSL core + #300 + #426 ✅ shipped (PR #450).**
A shared `Quantity` vocabulary (`CluesAtControllerLocation`, `EngagedEnemies`,
`SkillTestFailedBy`) backs both `IntExpr::Count` (value) and `Condition::Compare`/
`CmpOp` (predicate, retiring `LocationHasClues`); `Effect::Deal.amount` +
`Effect::Fight.extra_damage` widened to `IntExpr` with `From`/`Into` builders
(literals untouched). **#426** — Grasping Hands 01162 / Rotting Remains 01163 deal
one `Count(SkillTestFailedBy)` instance (`ForEachPointFailed` deleted). **#300** —
Machete is `+1` only vs the sole engaged enemy (`Compare(EngagedEnemies, Eq, 1)`);
correct modelling, but its `+0` branch isn't *reachable* until activated `Effect::Fight`
can resolve with 2+ engaged enemies (**#449** — give `Effect::Fight` a `Choose`-resolved
target; a p1-next gate gap, separate from #401's basic-action fix).
- **Remaining: #118 — Roland's elder-sign** ("+1 per clue on your location"). The
  clue-*count* term `IntExpr::Count(Quantity::CluesAtControllerLocation)` already
  ships (PR #450); #118 (PR 2) adds `Trigger::ElderSign` + the ST.4 firing path (the
  bonus rides the chaos-token `Modifier` total path, sourced from the investigator
  card — **not** `Effect::Modify`) + the investigator-card-handle bridge (spec §2).
  His signature is in the "done" criteria.

**2. #368 — before-discover eligibility (p1-next, needs-design).** Lift the
hardcoded scan-suppression stand-ins (Cover Up 01007 `card.clues == 0`; act 01109
round-end clue-threshold) into a declarative **trigger-level eligibility
`Condition`** (RR p.2: an ability can't initiate if its effect won't change game
state). Two consumers already; designed when the 3rd lands (Lone Wolf 02188,
Burned Ruins 02205). Item 2 (capped discovery count) is independent and latent.

**3. Browser capstone — the gate-closer.** Positioned last so it designs against
the now-stable set of input shapes:
- **#447 — 2b: typed `PlayerAction` elimination** (engine half). Route open-turn
  gameplay through `ResolveInput(PickSingle(OptionId))` only; id→action map fully
  internal. The committed/scheduled #393 end-state (§E), pairs with #205.
- **#205 — structured input rendering** (client half). Render the right control per
  offered option from the structured `InputRequest.options`, not prompt-string
  heuristics. Needs-design (client metadata schema).
- **Investigator/scenario picker.** Seating (#221) + registry swap (#244) exist
  engine-side; the browser picker driving `StartScenario` is the remaining UI.
- **End-to-end browser playthrough** of The Gathering to a resolution.

**Deferred past the gate:** #353 (uses-depletion — no Gathering card; gated on
Forbidden Knowledge / Grotesque Statue), #294 (multi-soak-window drain —
unconstructible in scope, `debug_assert` guards it), #427/#429 (native-loop soak
residue — rare in 1p), #119/#26 (behaviour-preserving cleanups — fold in
opportunistically).

## Frame-model end-states (#393)

For a future author who sees the partial state and wonders what's "missing":
- **C checkpoint** ✅ and **EmitEvent-frame** (3rd checkpoint) ✅ — both shipped.
- **2b** (typed `PlayerAction` → `OptionId`-only) — outstanding, **#447**, lands
  with the capstone.
- **B** (every straight-line step a frame) — **intentionally dormant**, reached
  *content-driven* (a card making a step a decision). No Core+Dunwich card forces
  it; B's marginal frames "earn nothing operationally." The visible remnant is the
  intra-skill-test `SkillTestStep` cursor — **not a gap**, leave it until a card
  puts a decision mid-test.

## Architecture to build on

Only the durable facts a future PR-author needs that aren't obvious from the code.

**Attack loop (keystone for damage/soak work).** `enemy_attack` does `assign →
place → defeat`; window-queuing lives in `drive_attack_loop`, which parks remaining
attackers as a `Continuation::AttackLoop` frame *beneath* the window (#411) and
resumes via `resume_enemy_attack`. With 2+ engaged enemies the player picks attack
order first (`AttackLoopStage::PickOrder`), so the frame spans the whole enemy-phase
step 3.3; single-enemy stays Shape A. The five basic actions + activated abilities +
non-fast card plays park on a `Continuation::ActionResolution` frame and fire AoO via
`drive_aoo` → `drive_attack_loop`; retaliate routes via `drive_retaliate`. Exhaust
differs by source: enemy-phase always (even cancelled, RR p.6/p.25); AoO never
(RR p.7); Retaliate never (RR p.18). `provokes_aoo` exempts `Effect::Fight` weapons;
fast plays/abilities provoke nothing (gate on `!is_fast`). Soak-first by
`CardInstanceId` order is the interactive-distribution entry point.

**Trigger spine.** `emit_event` is the one dispatch chokepoint (two-phase
forced-then-reaction, RR p.2; simultaneous triggers lead-ordered via a
`TimingPointWindow { Forced }` run, RR p.17). Reentrancy resolves by **top-frame
dispatch** (C-plumbing, PR #443): the loop dispatches whatever is on top — a mid-test
window above the `SkillTest`, then the `SkillTest`, then a forced run beneath — so no
driver distinguishes "above" from "below". Reaction/forced windows resume via
`PickSingle(OptionId)`. The `when → at → after` axis is a `Continuation::EmitEvent`/
`TimingPoint` coordinator that re-scans each cell fresh (the per-cell re-scan,
`tests/round_end_rescan.rs`).

**Choice & cancellation.** Interactive choice runs inside the **effect evaluator's
`Continuation::Effect` frames** (#422 / PR #424): `resolve_choice_count` (0 ⇒
reject/auto · 1 ⇒ auto-bind · 2+ ⇒ suspend); a node needing a choice **suspends in
place** and resume **re-steps the same leaf** with `chosen_option` set — no replay,
no `DecisionCursor`. DSL targets bind through `ground_chosen_targets`
(`chosen_investigator`/`location`/`enemy`); native leaves read `chosen_option`.
Spatial targets use `Choose<S> { scope }` (`LocationSet { Here, Anywhere }` /
`EntityScope`). Before-timing cancellation is a Before window the caller suspends on
+ an `Effect::Cancel` leaf setting `pending_cancellation` (a `bool` suffices —
Before-windows don't nest in scope, #367), honored on window close. A reaction event
(Evidence! 01022) rides the window's candidate list and is *played* when picked
(`TriggerKind::Reaction` `OnEvent`, window-only).

**Skill-test control-flow shape.** Storage is on the stack (`InFlightSkillTest`
folded onto the `Continuation::SkillTest` frame, #348). Dispatch is top-frame
(C-plumbing): the `drive` loop's `SkillTest` arm calls `advance` when the frame is on
top; a mid-test window makes `advance` yield `Done`, and the loop re-dispatches
`SkillTest` on window close. **Intra-test sequencing is still an inline cursor** —
the `SkillTestStep` enum (`PreCommitWindow → AwaitingCommit → PreTokenWindow →
Resolving → …`) is a field advanced by a `loop` in `advance`. That's the remaining
Shape-A compression (= the dormant end-state B); reifying each step is unpaid for
until a card demands it. Two entry points: the commit hop (`finish_skill_test`) and
the loop's `SkillTest` arm.

**`IntExpr` dynamic-expression substrate.** Board-state-dependent values are an
`IntExpr` AST (`card-dsl/src/dsl.rs`: `Lit(i8)` + `Cond { when: Condition, then,
otherwise }`) — **shipped and wired into `Effect::Fight.combat_modifier`** (Roland's
.38 Special 01006: `IntExpr::cond(Condition::LocationHasClues, 3, 1)`). So the
"dynamic skill-test modifier surface" is a settled `IntExpr`, **not** a needs-design
question. The #118/#300/#426 cluster each extend it the same way (add a `Condition`/
term + plumb `IntExpr` into one more `Effect` field).

**Content patterns.** Card stats come from the corpus (`CardKind`; read via
`cards::by_code` / `metadata_for`, never hand-typed) — a future enemy/card lands via
a snapshot bump + regen, no impl. Single-use card logic is `Effect::Native { tag }`
(promote to a shared `Effect` variant only at ≥2 reuses). Scenario chaos-symbol /
reference-card effects live on the `ScenarioModule.resolve_symbol` hook, not card
`abilities()`.

## Future slices (after the gate)

Captured but **unfiled** (no issues yet) — filed when the gate closes.

- **Slice 2 — investigator breadth.** Daisy Walker, "Skids" O'Toole, Agnes Baker,
  Wendy Adams — each with their signature asset/weakness pair + starter deck. Goal:
  all five picker-eligible.
- **Difficulty selection.** Add Easy / Hard / Expert chaos bags + a picker (Slice 1
  is Standard only).
- **Solo-with-2 UX.** One client driving two investigators (picker, whose-turn, two
  boards vs. tabbed). Open design question; the Tier-2 correctness issues (#65, #381,
  #359, #153, #371) land here.
- **Optional Gathering content (#258).** Lita Chantler's parley/take-control + the
  Parlor (01115) Resign action. #258 is also the home for the **Parley/Resign
  action-type mechanisms** (not basic actions — RR p.5; both AoO-exempt), landing
  with their first granting card.

Campaign sequencing (The Midnight Masks, The Devourer Below, campaign log + `Fact`
enum) is **Phase 9** — including the first real Peril/Surge cards (Hunting Shadow
01135 et al.; #138/#139 re-milestoned there).

## Open questions

- **Roland elder-sign DSL surface (#118).** Mostly answered: the `IntExpr` AST
  exists (.38 Special is the live consumer) and the design spec is settled (see
  `docs/superpowers/specs/2026-06-24-intexpr-dynamic-value-cluster-design.md`
  Section 2). The remaining work is the clue-*count* `IntExpr` term
  (`IntExpr::Count(Quantity::CluesAtControllerLocation)`), the `Trigger::ElderSign`
  / ST.4 firing path, and the `Investigator.card_code` bridge. The elder-sign bonus
  flows through the existing chaos-token `Modifier` total path (sourced from the
  investigator card) — **not** through `Effect::Modify` or a new
  `Effect::ModifySkillTestTotal`; `Effect::Modify.delta` stays `i8` and is not
  touched by #118.
- **Solo-with-2 UX** — how one client presents two investigators. See Future slices.

## Dependencies

Phases 4 (scenario module), 5 (server + persistence), 6 (web client) — all closed.
Phase 3's Roland Banks (#55) shipped.

## What "done" looks like

A solo human, in the browser, plays The Gathering to a resolution with **1-player
Standard rules correctness**: every basic action available, attacks of opportunity /
retaliate / soak resolving with proper player agency, skill-test windows open, and
Roland's signature firing. Investigator breadth, difficulty, and solo-2 are Future
slices.
