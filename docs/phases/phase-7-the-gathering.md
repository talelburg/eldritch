# Phase 7 — The Gathering

## Status

**Slice 1 — Roland through The Gathering — is shipped.** Solo Roland plays the
scenario end-to-end to genuine Won + Lost resolutions against the real
registries (kickoff [#216](https://github.com/talelburg/eldritch/issues/216),
gate [#245](https://github.com/talelburg/eldritch/issues/245)/PR #326). The
engine spine, scenario plumbing, all Group C content (C1–C7), and the
five-axis trigger-dispatch rework (#212/#213 + Axes A–D + the choice-cluster
completion sub-slice) all landed — the detail lives in the closed issues and
git history; only the architecture a future PR builds on survives in
**Architecture to build on** below. Slice-1 design specs are in
`docs/superpowers/specs/2026-06-1*`.

**Now: Phase 7 is the 1-player solo rules-correctness gate.** The remaining
work is every place solo play diverges from the rules. Scope is deliberately
narrow — **1 player, 1 investigator, Standard** — and the categorized,
dependency-ordered plan is **The solo correctness gate** below. Investigator
breadth, difficulty, solo-2, and optional content are **Future slices**.

## Goal

A solo human, in the browser, picks an investigator, sets up The Gathering,
and plays it to a resolution — **rules-correct for 1-player Standard**.

## The solo correctness gate

In scope: anything that makes **1 player / 1 investigator / Standard** play
wrong. Out: multiplayer, solo-2, perf, refactor, later-scenario, UI. The
Gathering's encounter deck carries no Surge/Peril, so #138/#139 are not gated
here.

### Tier 1 — the work

**A. Missing basic actions** — ✅ **shipped (PR #383).** `PlayerAction::Resource`
([#141](https://github.com/talelburg/eldritch/issues/141), closed) + the
basic-action half of [#77](https://github.com/talelburg/eldritch/issues/77)
(`PlayerAction::Engage`). Both fire attacks of opportunity (RR p.5 exempts only
fight/evade/parley/resign); the pre-existing **Draw** AoO gap was fixed alongside,
and the shared five-check prologue extracted into `validate_basic_action`.
**Resign and Parley are NOT basic actions** (verified RR p.5: "Activate, Play,
Resign, and Parley are not basic actions") — they are action *types* granted only
by card/location abilities, so they live with the optional content in
[#258](https://github.com/talelburg/eldritch/issues/258) (the Parlor's Resign,
Lita's Parley), **not** the gate. #77 stays open for its Parley half.

**B. Attacks of opportunity + non-enemy-phase attack windows** — one cluster,
one shared mechanism (mid-action park/resume; see the keystone note):
- [#361](https://github.com/talelburg/eldritch/issues/361) — activated abilities don't provoke AoO (First Aid, Medical Texts, Flashlight).
- [#378](https://github.com/talelburg/eldritch/issues/378) — action-event play doesn't provoke AoO (Dynamite Blast, Emergency Cache).
- [#293](https://github.com/talelburg/eldritch/issues/293) — AoO opens no soak/cancel window (Guard Dog, Dodge).
- [#379](https://github.com/talelburg/eldritch/issues/379) — Retaliate opens no soak/cancel window (Guard Dog, Dodge).

**C. Enemy-attack-loop player agency:**
- [#143](https://github.com/talelburg/eldritch/issues/143) — player picks attack order with 2+ engaged enemies.
- [#44](https://github.com/talelburg/eldritch/issues/44) — player chooses damage/horror distribution across soakers + self (today soak-first auto).

**D. Skill-test player windows:**
[#374](https://github.com/talelburg/eldritch/issues/374) (ST.1/ST.2 fast-play
windows) + [#64](https://github.com/talelburg/eldritch/issues/64)
(after-resolution reaction window). Only the commit window exists today.

**E. Roland's signature:**
[#118](https://github.com/talelburg/eldritch/issues/118) — elder-sign "+1 per
clue" is stubbed; needs the dynamic skill-test-modifier DSL surface
(needs-design; see Open questions).

**F. Conditional / edge correctness:**
[#300](https://github.com/talelburg/eldritch/issues/300) (Machete only-engaged-enemy +1),
[#368](https://github.com/talelburg/eldritch/issues/368) (before-discover eligibility + count cap),
[#353](https://github.com/talelburg/eldritch/issues/353) (uses-depletion timing).

### Ordering, dependencies, simplifications

1. ~~**Basic actions first** (#141, #77)~~ — ✅ shipped (PR #383); Engage also unblocks #300's condition. (Resign/Parley aren't basic actions — see Tier-1 A.)
2. **§1 continuation-stack cleanup before the keystone** — the full #348 + #345 + #347 + #380 as one designed pass (see Refactor triage). The keystone *adds* suspension modes, so migrate the existing `pending_*` onto the one stack (with serializable context + token-routed resume) first rather than building the Nth ad-hoc route on top.
3. **The keystone: mid-action suspend/resume.** Tier-1 B **and** C all hinge on `drive_attack_loop` being able to park the triggering action, open a window, and resume. Build it once and #293/#379/#361/#378/#143/#44 collapse into a single attack-loop arc — the highest-leverage item in the phase. Fold #119 in for #44's soak (symmetric token storage).
4. **Skill-test windows** (#374 + #64) — one reaction-window work-stream.
5. **Roland elder-sign** (#118).
6. **Edge correctness** (#300 after Engage, then #368, #353).
7. **Browser playable surface** (capstone) — once the above stabilizes; see below.

**Simplifications:**
- **#300 does not need #363 (general fan-out).** Once Engage (#77) exists, Machete's "only enemy engaged with you" is a count==1 read — gate `extra_damage` on it; don't wait for multi-target Fight.
- **#367 is likely a wontfix for this gate** — Before-windows don't nest in 1-player scope, so the `bool` cancellation marker suffices.
- **#380 folds into #348** (continuation-stack cleanup) — see Refactor triage.

### Browser playable surface (the former "Slice D") — capstone

The gate's "done" is a solo human playing in the *browser*, not just a green
integration test. Once the Tier-1 fixes stabilize, this is the first follow-on:
the web client (shipped Phase 6) must drive the **real** Gathering scenario.

- **#205 — structured `AwaitingInput` discrimination (load-bearing, needs-design).**
  The Gathering's cards are the first to emit non-`CommitCards` prompts
  (`PickIndex` / `PickInvestigator` / `Confirm` / `Skip` / `DiscardCards`); the
  client must render the right control per variant from a machine-readable
  `InputKind`, not prompt-string heuristics. Keystone of the surface; pairs with
  #347 (token-routed resume → stale-submit rejection).
- **Investigator / scenario picker.** The seating protocol (B2 #221) + registry
  swap (C7a #244) exist engine-side; the browser picker driving `StartScenario`
  with a chosen investigator is the remaining UI.
- **End-to-end browser playthrough** of The Gathering to a resolution, driven
  through the client (the C7b coverage, but via the browser).

Positioned **after** Tier 1: every new Tier-1 input site (AoO targeting, attack
order, damage distribution, skill-test windows) adds an `AwaitingInput` shape the
surface must render, so building #205 before they exist designs against a moving
target.

### Refactor / tech-debt triage

Not rules bugs, but several simplify or de-risk the Tier-1 work — pull these in
rather than deferring wholesale:

- **§1 continuation-stack cleanup — the full #348 + #345 + #347 + #380, as one
  designed pass. DO-FIRST, before the keystone.** Designed in
  `docs/superpowers/specs/2026-06-19-continuation-stack-cleanup-design.md`.
  **Progress:** #345 shipped (PR #385); #347 → #348 → #380 follow as separate PRs.
  #348 migrates the remaining
  `pending_*` suspension modes (incl. `pending_enemy_attack`, `pending_end_turn`)
  onto the one continuation stack and collapses the
  fragile `if pending_X.is_some()` `resolve_input` cascade **and** the parallel
  `apply_player_action` guard ladder into top-frame dispatch (folding
  `Mulligan`/`DrawEncounterCard` into `ResolveInput` and `in_flight_skill_test`
  onto its frame; `clue_interrupt_pending` is already a window); #345 makes
  `EvalContext` serializable with **grouped optional bindings** snapshotted
  per-frame (the Vec / per-frame-enum / global-stack alternatives were evaluated
  and rejected — spec §D; innermost-only is corpus-moot, no TODO) so migrated
  frames snapshot context instead of re-storing ingredient tuples; #347 makes resume
  **token-routed** (deterministic counter, stamped on the awaiting frame) so
  routing becomes token → frame → dispatch-on-variant, with stale/double-submit
  rejection; #380 removes the `pending_revelation_discard` side-channel by making
  encounter-card resolution a frame whose framework teardown disposes of the card.
  Designed together (they share the seam). The keystone
  adds attack-loop suspension, so this lands first and the keystone rides one
  clean stack. **Token-routing (#347b) also de-risks the browser surface** —
  #205's client can submit against a superseded prompt and be rejected cleanly,
  so doing the full cleanup in-phase (rather than a focused subset) pays off at
  the Slice-D capstone too.
- **#119 — unify damage/horror/clues onto `CardInPlay`. DO-WITH #44.** #44 (soak
  distribution, Tier-1 C) needs symmetric investigator/asset token storage; #119
  makes the soak machinery symmetric instead of special-casing the investigator
  side.
- **Defer (no gate consumer):** #290 (mint encounter instances at reveal —
  simplifies #373/#371 but no 1p correctness need), #373 (Obscuring Fog
  attach-unify — single card, pairs with #290), #346 (two-pass for
  choice-after-`Seq` — no in-scope card), #363 (general fan-out — #300's
  simplification avoids it; Dunwich-era), #366 (replace-with-different-impact — no
  in-scope card).

### Housekeeping
- **Close #56** (the Study location is built and played end-to-end) and **#294** (unconstructible in scope — its own `debug_assert` guards it).

### Out of scope (defer past the gate)
- **Solo-2** (one player, two investigators): #65, #381, #359, #153, #371.
- **Phase 8 multiplayer:** #146, #151, #206.
- **Later scenarios:** #138, #139 (no Surge/Peril in The Gathering).
- **Perf / infra:** #117, #174, #224, #26, #31.
- **Optional content:** #258 (Lita parley / Parlor — minus the Resign action above).

(Refactor / tech-debt issues are triaged above, not here — several are pull-ins.)

## Architecture to build on

Only the facts a future PR-author needs that aren't obvious from the code or
the issues.

**Attack loop (keystone for Tier-1 B/C).** `enemy_attack` does `assign → place
→ defeat`, soak-first by `CardInstanceId` order (the #44 replacement point);
window-queuing lives in the caller `drive_attack_loop`, which parks remaining
attackers and returns `AwaitingInput` around a window, resuming via
`resume_enemy_attack` (the enemy-phase cursor advances once via
`after_enemy_phase_attacks`). **Both `fire_attacks_of_opportunity` (called
from the Move/Investigate handlers) and `fire_retaliate_if_any` call
`enemy_attack` directly, bypassing the loop — so they open no windows;
`EnemyAttackSource::AttackOfOpportunity` is the reserved-unconstructed variant
the fix wires up.** Exhaust rules differ by source: enemy-phase always
exhausts (cancelled too — RR p.6/p.25); AoO never (RR p.7); Retaliate never
(RR p.18). **Every non-exempt basic action — Draw, Resource, Move, Investigate,
Engage (PR #383) — already calls `fire_attacks_of_opportunity` (window-less),
so the keystone's window-upgrade must cover all of them uniformly**; activating
an ability or playing an event fire no AoO yet (#361/#378).

**Trigger spine.** `emit_event` is the one dispatch chokepoint (two-phase
forced-then-reaction, RR p.2). Simultaneous triggers resolve through the
`Continuation::Resolution` loop (lead-ordered, RR p.17); `ResolutionFrame.kind
= Window | Forced`. Reentrancy across a forced/window effect that suspends into
a skill test hinges on `drive_skill_test` reacting only to a window *above*
the in-flight `SkillTest` frame, never a forced frame below it. Reaction/forced
windows resume via `PickSingle(OptionId)` (the legacy `PickIndex` path is
retired).

**Choice & cancellation.** Interactive choice is single-pass suspend-and-replay
on a `Continuation::Choice` frame: `resolve_choice_count` (0 ⇒ reject/printed
fallback · 1 ⇒ auto-bind · 2+ ⇒ suspend), replayed pre-order via a
`DecisionCursor`; DSL targets bind through `ground_chosen_targets`, native
leaves read `EvalContext.chosen_option`. Spatial targets use the unified
`Choose<S> { scope }` surface (`LocationSet { Here, Anywhere }` /
`EntityScope`). Before-timing cancellation is a Before reaction window the
caller suspends on + an `Effect::Cancel` leaf that sets `pending_cancellation`,
which the emit site honors on window close; a `bool` suffices because
Before-windows don't nest in scope (#367). A reaction event (Evidence! 01022)
is a Fast event carried on the reaction window's candidate list and *played*
when picked; it is window-only at the play gate (`TriggerKind::Reaction`
`OnEvent`).

**Skill-test player windows are NOT modeled (#374).** Only the commit window
exists; the ST.1/ST.2 framework player windows and the after-resolution window
(#64) are absent — `OnCommit` / `OnSkillTestResolution` card triggers fire, but
a player cannot play a Fast card mid-test.

**Content patterns (mostly later slices).** Card stats come from the corpus
(`CardKind`; read via `cards::by_code` / `metadata_for`, never hand-typed) — a
future enemy/card lands via a snapshot bump + regen, no impl. Single-use card
logic is `Effect::Native { tag }` (promote to a shared `Effect` variant only at
≥2 reuses). Scenario chaos-symbol / reference-card effects live on the
`ScenarioModule.resolve_symbol` hook, not card `abilities()`.

## Future slices (after the gate)

- **Slice 2 — investigator breadth.** Daisy Walker, "Skids" O'Toole, Agnes
  Baker, Wendy Adams — each with their signature asset/weakness pair + starter
  deck, reusing the engine spine. Goal: all five picker-eligible. Not yet
  specced.
- **Difficulty selection.** Slice 1 ships Standard only; add Easy / Hard /
  Expert chaos bags + a picker.
- **Solo-with-2 UX.** One client driving two investigators — picker,
  whose-turn, two boards vs. tabbed. Open design question; the Tier-2
  correctness issues (#65, #381, #359, #153, #371) land here.
- **Optional Gathering content.** Lita Chantler's parley/take-control + the
  Parlor (01115) Resign action (#258).

Campaign sequencing beyond The Gathering (The Midnight Masks, The Devourer
Below, campaign log + `Fact` enum) is **Phase 9**.

## Open questions

- **Roland elder-sign DSL surface (#118).** The token effect is a *dynamic*,
  board-state-dependent skill-test modifier ("+1 for each clue on your
  location"); the DSL has no such surface yet. needs-design; gates Tier-1 E.
- **Solo-with-2 UX** — how one client presents two investigators. See Future
  slices.

## Dependencies

Phases 4 (scenario module), 5 (server + persistence), 6 (web client) — all
closed. Phase 3's Roland Banks (#55) shipped; the Study (#56) spilled here and
is now playable (close it).

## What "done" looks like

A solo human, in the browser, plays The Gathering to a resolution with
**1-player Standard rules correctness**: every basic action available, attacks
of opportunity / retaliate / soak resolving with proper player agency,
skill-test windows open, and Roland's signature firing. Investigator breadth,
difficulty, and solo-2 are Future slices.
