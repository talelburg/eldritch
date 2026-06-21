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
one shared mechanism (mid-action park/resume), now a sub-sliced arc **K1→K5**
(see Ordering step 4 + its design spec). **The foundation shipped:**
- ✅ [#293](https://github.com/talelburg/eldritch/issues/293) — AoO open cancel/soak windows (Guard Dog, Dodge). **Shipped — K1, PR #413** (`ActionResolution` frame + `drive_aoo`).
- ✅ [#379](https://github.com/talelburg/eldritch/issues/379) — Retaliate opens cancel/soak windows (Guard Dog, Dodge). **Shipped — K2, PR #414** (`drive_retaliate`; resume re-enters `drive_skill_test`).
- ✅ [#361](https://github.com/talelburg/eldritch/issues/361) — activated abilities provoke AoO (First Aid, Flashlight, Medical Texts, Old Book of Lore; Fight weapons exempt). **Shipped — K3, PR #415** (`provokes_aoo` gate + `ActionResume::ActivateAbility`).
- ✅ [#378](https://github.com/talelburg/eldritch/issues/378) — playing a non-fast card (asset or event) provokes AoO + costs an action (Dynamite Blast, Emergency Cache; the missing non-fast play-action charge folded in). **Shipped — K3, PR #416** (`ActionResume::PlayCard` + `check_play_action_available`).

**C. Enemy-attack-loop player agency:**
- ✅ [#143](https://github.com/talelburg/eldritch/issues/143) — player picks attack order with 2+ engaged enemies. **Shipped — K4, PR #419** (`AttackLoopStage::PickOrder` + `resume_attack_order_pick`; interleaved pick in the shared `drive_attack_loop`, both sites; enemy-phase frame now spans step 3.3).
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
2. **§1 continuation-stack cleanup — ✅ done.** #345 (PR #385) + #348 via parts 2a–2c + #380 (PRs #386–#392). The last piece, **#347** (token-routed resume / stale-submit rejection), **folds into #393** rather than landing standalone: the literal "token on `ResolveInput`, validated in the engine" is a ~145-site churn that #393 would rework, and stale-submit rejection is properly a *session* concern — the engine emits deterministic token *values*, the **server** rejects stale client echoes at the network boundary (the engine's `apply`/action-log stays token-free for replay). So token-routing is designed on #393's unified resume channel, at the right layer. (#348/#347 closed → #393.)
3. **Unified control-flow model (#393) — the foundation arc, before the rest of Tier-1. ✅ designed** ([`2026-06-20-unified-control-flow-model-design.md`](../superpowers/specs/2026-06-20-unified-control-flow-model-design.md)). Reify *every* step of control flow as a continuation frame (phases, turns, the open-action choice), so the main loop collapses to a single rule: **handle the top frame.** The `InvestigatorTurn` frame re-emits the player's legal actions as `OptionId`s while actions remain — so the stack is never empty during play (empty only at bootstrap and the terminal resolution). The spec scopes a **C checkpoint** (a step is a frame *iff* it suspends or loops; net-new surface = four per-phase anchors + `InvestigatorTurn` + `AttackLoop`) and three sequenced post-C **end-states**: **B** (every step a frame, reached content-driven), **2b** (eliminate typed `PlayerAction` → gameplay is `ResolveInput(OptionId)` only; committed for UX/#205), and the **EmitEvent-frame** (the `when/at/after × forced/reaction` ordering axis as nested frames; #212 successor). It **subsumes** the keystone's substrate, the three framework cursors (`enemy_attack_pending` / `pending_end_turn` / `pending_enemy_attack`), #384's engine half (#384 closed → #393), and **token-routing / stale-submit rejection (#347, folded in → server-side)**. The engine emits `OptionId`s + keeps the id→action map internal; option metadata / browser rendering is #205 at the capstone. Build the model **before** the rest of Tier-1 so each item lands on the final engine shape.
   - **Slice 3 — `AttackLoop` frame (cursor lift). ✅ shipped (PR #412, closes #411).** Lifted the last two framework cursors onto the stack: the parked enemy-attack loop is now a `Continuation::AttackLoop` frame (inserted *beneath* the reaction window it suspends on), and the per-investigator cursor is the `EnemyPhase` anchor's `attacking: Option<InvestigatorId>` field. Behaviour-preserving. **Deliberately Shape A:** the `AttackLoop` frame spans only the *parked suspension*, not the whole per-investigator step 3.3 — see the keystone caveat below.
4. **The keystone: mid-action suspend/resume — 🔨 in progress (K1–K4 shipped; K5 remains).** Designed in its own spec ([`2026-06-20-phase-7-keystone-mid-action-park-design.md`](../superpowers/specs/2026-06-20-phase-7-keystone-mid-action-park-design.md)) — §D of #393 expanded into a sub-sliced arc **K1→K5** collapsing #293/#379/#361/#378/#143/#44 (+#119), the highest-leverage item in the phase. The action parks its *triggering action* on a `Continuation::ActionResolution` frame (above `InvestigatorTurn`) with the AoO `AttackLoop` (slice 3 / #411) as its child; on the loop's pop an `on_child_pop` re-validation gate (actor-Active + the primary's target precondition) resumes the action's primary effect, aborting cleanly on a mid-action lapse.
   - **K1 — AoO open cancel/soak windows (#293). ✅ shipped (PR #413).** `ActionResolution` frame + `drive_aoo`; the five basic actions fire AoO through `drive_attack_loop`, so Dodge cancels and Guard Dog retaliates against an AoO; `fire_attacks_of_opportunity` deleted. RR p.7 AoO-non-exhaust source-gated.
   - **K2 — Retaliate opens cancel/soak windows (#379). ✅ shipped (PR #414).** `drive_retaliate` routes the failed-Fight retaliate through `drive_attack_loop` under `EnemyAttackSource::Retaliate`; the resume re-enters `drive_skill_test` (the retaliate's park point is the existing `SkillTest` frame, not an `ActionResolution` frame).
   - **K3 ✅ #361 (PR #415)** AoO from activated abilities (Fight-exempt by effect root; effect snapshotted on the resume frame) · **✅ #378 (PR #416)** non-fast card-play AoO + the folded-in play-action charge (`ActionResume::PlayCard`; both gate on `!is_fast`) · **K4 ✅ #143 (PR #419)** player attack-order — interleaved one-at-a-time pick at the top of the shared `drive_attack_loop` (`AttackLoopStage::PickOrder`), covering both the enemy phase and AoO; extends the enemy-phase `AttackLoop` to span the whole step 3.3 (resolved slice 3's Shape-A caveat) and confirmed the attacker snapshot frozen at loop entry · **K5 #44 (+#119)** player damage/soak distribution. Each rides the K1 substrate.
5. **Skill-test windows** (#374 + #64) — one reaction-window work-stream, offered as frame options on the #393 model. **Also the moment to move the skill-test path from Shape A toward end-state B:** today the intra-test sequence is an inline `FinishContinuation` cursor re-entered imperatively from `close_reaction_window_at` (see Architecture → "Skill-test control-flow shape (Shape A)"). #374/#64 insert player windows *between* those steps, so reify the steps as frames under uniform top-frame dispatch here, rather than deepening the enum with two more variants.
6. **Roland elder-sign** (#118).
7. **Edge correctness** (#300 after Engage, then #368, #353).
8. **Browser playable surface** (capstone) — once the above stabilizes; renders the enumerated actions / #205. See below.

**Simplifications:**
- **#300 does not need #363 (general fan-out).** Once Engage (#77) exists, Machete's "only enemy engaged with you" is a count==1 read — gate `extra_damage` on it; don't wait for multi-target Fight.
- **#367 is likely a wontfix for this gate** — Before-windows don't nest in 1-player scope, so the `bool` cancellation marker suffices.
- **#380 folds into #348** (continuation-stack cleanup) — see Refactor triage.

### Browser playable surface (the former "Slice D") — capstone

The gate's "done" is a solo human playing in the *browser*, not just a green
integration test. Once the Tier-1 fixes stabilize, this is the first follow-on:
the web client (shipped Phase 6) must drive the **real** Gathering scenario.

- **#205 — structured `AwaitingInput` discrimination + action rendering
  (load-bearing, needs-design).** The engine side is provided by #393: every
  player decision — including the open-turn's *enumerated legal actions* —
  surfaces as a frame offering `OptionId`s on the normalized `InputResponse` set
  (`PickSingle` / `PickMultiple` / `Confirm` / `Skip`). #205 is the **client
  half**: decide what option *metadata* the engine surfaces (labels, per-variant
  controls, action parameters — #393 keeps the id→action map internal for now)
  and render the right control per option, not prompt-string heuristics. (Absorbs
  the former #384 client half.) Keystone of the surface; pairs with #393's
  token-routing (#347, folded in → server-side stale-submit rejection).
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

- **§1 continuation-stack cleanup — ✅ done (#345 + #348 + #380); #347 folds
  into #393.** Designed in
  `docs/superpowers/specs/2026-06-19-continuation-stack-cleanup-design.md`.
  **Progress:** #345 shipped (PR #385); #348 landed incrementally (parts
  2a–2c via PRs #386–#391) — the `InputResponse` channel is now normalized
  (`CommitCards`/`DiscardCards` → `PickMultiple`, `PickLocation`/`PickInvestigator`
  → `PickSingle`, `Mulligan` → `PickMultiple`, `DrawEncounterCard` → `Confirm`),
  every player-facing suspension resumes through `ResolveInput`, and the bespoke
  `mulligan_pending`/`mythos_draw_pending` cursors + `in_flight_skill_test` are
  folded onto continuation frames. #380 (the `pending_revelation_discard`
  side-channel → an `EncounterCard` frame, PR #392) has also landed. The last
  piece, **#347** (token-routed resume), **folds into #393** rather than landing
  standalone (engine-level `ResolveInput.token` is ~145-site churn #393 would
  rework; stale-submit rejection is a session concern — engine emits token
  *values*, the server rejects stale echoes, the action log stays replay-clean).
  Likewise the **remaining framework cursors — `enemy_attack_pending` /
  `pending_end_turn` / `pending_enemy_attack` — move to #393** (the unified
  control-flow model, the foundation arc that follows §1 — see Ordering step 3):
  internal sequencing, never player-facing, they fall out when #393 reifies the
  phase/turn/attack-loop drivers as frames.
  #348 collapsed the
  fragile `if pending_X.is_some()` `resolve_input` cascade **and** the parallel
  `apply_player_action` guard ladder into top-frame dispatch
  (`clue_interrupt_pending` is already a window); #345 makes
  `EvalContext` serializable with **grouped optional bindings** snapshotted
  per-frame (the Vec / per-frame-enum / global-stack alternatives were evaluated
  and rejected — spec §D; innermost-only is corpus-moot, no TODO) so migrated
  frames snapshot context instead of re-storing ingredient tuples; #380 removed
  the `pending_revelation_discard` side-channel by making encounter-card
  resolution a frame whose framework teardown disposes of the card. **Token-
  routing (#347 → #393)** stays valuable for the browser surface — #205's client
  can submit against a superseded prompt and be rejected cleanly — but lands as
  part of #393's unified resume channel, at the session layer, rather than as a
  standalone engine pass.
- **EmitEvent-frame — the `when/at/after` ordering axis (a #393 end-state, #212
  successor).** `emit_event` (T5a chokepoint, PR #342) models only the RR p.2
  `forced → reaction` axis; the orthogonal `when → at → after` axis (RR "At"
  entry) is still hand-threaded per site. The #393 spec (§"named end-states")
  reifies it as two nested coordinator frames (`EmitEvent` over buckets,
  `TimingPoint` over forced/reaction) — built post-C on the proven model, since
  `emit_event` is the highest-blast-radius engine function. Re-open #212 (or a
  successor) scoped to this.
- **Upkeep round-end `when→at` ordering bug — surfaced by the #393 design; fix
  before the Upkeep-anchor slice.** `upkeep_phase_end` fires agenda 01107's `at`-
  the-end-of-round doom **before** act 01109's `when`-the-round-ends clue-spend
  window — inverted vs. the RR "At" rule (`when → at → after`). Consequential when
  the doom advances the agenda (loss on agenda 3). Cheap reorder + regression test
  (agenda-3 + act-2 at round end); file its own bug issue. (Spec §G.)
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
attackers as a `Continuation::AttackLoop` frame *beneath* the window (#411) and
returns `AwaitingInput`, resuming via `resume_enemy_attack` (which pops the frame;
the per-investigator cursor is the `EnemyPhase` anchor's `attacking` field,
advanced once via `after_enemy_phase_attacks`). **K4 (PR #419) lifted the enemy-phase
frame off Shape A in the multi-enemy case:** with 2+ engaged enemies `drive_attack_loop`
suspends on the player's attack-order pick (`AttackLoopStage::PickOrder`,
`resume_attack_order_pick`) before the first attack, so the `AttackLoop` frame spans
the whole step 3.3; the single-enemy case stays Shape A (frame pushed only on a
window suspend). The order pick reorders the stored `remaining_attackers` (snapshotted
in `EnemyId` order at loop entry), never re-scanning. **K1 (PR #413) shipped the AoO
half:** `fire_attacks_of_opportunity` is
gone; the five basic actions (Draw, Resource, Move, Investigate, Engage) now run as
a `Continuation::ActionResolution` frame and fire AoO via **`drive_aoo`** →
`drive_attack_loop` (so `EnemyAttackSource::AttackOfOpportunity` is now live), opening
the cancel (Dodge) and soak (Guard Dog) windows; on the loop's pop the `drive` loop
resumes the action frame under an actor-Active + target re-validation gate.
**K2 (PR #414) routed `fire_retaliate_if_any` through `drive_retaliate` →
`drive_attack_loop` (`EnemyAttackSource::Retaliate`), so a failed-Fight retaliate now
opens the cancel/soak windows too; its resume re-enters `drive_skill_test` (the
retaliate's park point is the `SkillTest` frame). No direct-`enemy_attack` window
bypass remains.** Exhaust rules differ by source:
enemy-phase always exhausts (cancelled too — RR p.6/p.25); AoO never (RR p.7 —
source-gated in `process_attacker_dealing`); Retaliate never (RR p.18).
**K3 added the activated-ability (PR #415) and card-play (PR #416) AoO sites:** a
non-fight action-cost activation parks its effect on an `ActionResolution` frame
and fires AoO via `drive_aoo` (the `provokes_aoo` gate exempts `Effect::Fight`
weapons; fast abilities never provoke); playing a **non-fast** card (asset or
event) likewise spends an action and parks on `ActionResume::PlayCard` before
firing AoO, with the card's effect resolving on resume. Fast plays stay free and
provoke nothing (both sites gate on `!is_fast`).

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

**Skill-test control-flow shape (Shape A — not yet end-state B).** The skill
test conforms to the #393 model on *storage* — `InFlightSkillTest` is folded
onto the `Continuation::SkillTest` frame (#348), no `*_pending` side-channels —
but its *control flow* is only partly on the stack:
- **Intra-test sequencing is an inline cursor, not a frame per step.** The
  `FinishContinuation` enum (`AwaitingCommit → PostFollowUp → PostRetaliate →
  PostOnResolution`) is a field on the one `SkillTest` frame, advanced by a
  `loop` in `drive_skill_test` — Shape A, the same compression as `AttackLoop`.
  `PostRetaliate` *can* suspend (its cancel/soak window), so it's a borderline
  step folded into the enum rather than reified.
- **The driver is re-entered imperatively, not by uniform top-frame dispatch.**
  `close_reaction_window_at` reaches *down* the stack — "if a skill test is
  mid-resolution, call `drive_skill_test`" — instead of popping the window back
  to the main loop and letting it dispatch on `SkillTest`-on-top. (`AttackLoop`
  does the same via `resume_enemy_attack`; codebase-wide pattern, still a
  divergence.)
- **The driver scans the stack to locate itself.** `drive_skill_test` does
  `rposition(SkillTest)` + `win_idx > st` to tell a window *above* it (mid-test
  → suspend) from a forced `Resolution` *below* it (#213 reentrancy → ignore); a
  clean top frame never reasons about relative positions. `current_skill_test` /
  `take_skill_test` are located-singleton reads, not `last()`.
- **Two entry points:** commit → `finish_skill_test`; the rest →
  `drive_skill_test` (`AwaitingCommit` is `unreachable!` there).

This is the intended C-checkpoint shape, **not drift.** Moving it toward
end-state B (each step a frame, driven by top-frame dispatch) belongs with
#374/#64 — see Ordering step 5.

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
