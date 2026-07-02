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
Machete is `+1` only vs the sole engaged enemy (`Compare(EngagedEnemies, Eq, 1)`).
**#449 ✅ shipped** — `Effect::Fight` now picks among the engaged enemies
(auto-binds 1, suspends for a `PickSingle` on 2+; `single_engaged_enemy` retired),
so an investigator swarmed by 2+ enemies can activate a weapon and Machete's `+0`
branch is reachable. **#451 ✅ shipped (PR #455)** — widened that candidate scope
engaged → any co-located enemy (`combat::fight_target_scope()` = `At(Here)`, shared
by the pre-cost gate and the target grounding so they can't drift), matching #401's
basic-action fix and Machete's FAQ (you *can* attack an Aloof / other-player-engaged
enemy; you just forfeit the sole-engaged damage bonus).
- **#118 — Roland's elder-sign ✅ shipped (PR #454).** `Trigger::ElderSign { modifier:
  IntExpr }` + an ST.4 firing path: the bonus rides the chaos-token `Modifier` total
  (sourced from the investigator card via `elder_sign_modifier` — **not** `Effect::Modify`).
  Folded in the **investigator-card bridge** (`Investigator.card_code` at seating +
  `ability_usage` + a `scan_investigator_card_reactions` source / `CandidateSource::
  Investigator`), which also fixes Roland's **reaction** firing from a *seated*
  investigator (previously only via test card-injection). **Bridge retired by #448 ✅
  shipped (PR #457)** — the investigator card is now a real `CardInPlay`
  (`Investigator.investigator_card`) holding health/sanity + harm + identity + usage, so
  `card_code` / `ability_usage` / the bespoke `scan_investigator_card_reactions` source all
  collapse into the uniform `controlled_card_instances()` scan; this also fully resolves
  #453's `card_code`-sentinel question (no field left to default) and made the web client a
  registry host (`cards::REGISTRY` installed at startup, since capacity now reads from
  metadata). **#453 ✅ shipped (PR #456)** removed the `#[serde(default)]` convention the new fields
  follow — the non-`Option` fields are now required on the wire (a stale payload errors
  rather than silently degrading `card_code` to the empty sentinel); the two `Option`
  fields (`pending_played_event`, `usage_limit`) stay implicitly optional because serde
  defaults a missing `Option` to `None` regardless, so #453's concern #2 for
  `pending_played_event` is only partially met (forcing it needs a custom deserializer,
  deferred). His signature is in the "done" criteria.

**2. #368 — trigger-level eligibility ✅ shipped (PR #472, also closes #470).**
The hardcoded scan-suppression stand-ins (Cover Up 01007 `card.clues == 0`; act
01109 round-end clue-threshold — the latter actually *missing* from the offer
scan post-#434, i.e. bug #470) are lifted into a per-ability **native eligibility
predicate** evaluated at reaction-scan time (RR p.2: an ability can't initiate if
its effect won't change game state). Resolved as a native hook (`Ability.
eligibility` tag → `CardRegistry::native_eligibility_for` →
`fn(&GameState, &EvalContext) -> bool`), **not** a declarative `Condition`: both
live consumers are single-consumer + heterogeneous, so declarative DSL vocab
would be speculative — promote to a `Condition` when a predicate recurs (Lone
Wolf 02188, Burned Ruins 02205). The Barrier's offer + resolve share one
`round_end_advance_affordable` helper so they can't drift. **Item 2 (capped
discovery count) moved to #471** — it becomes live only once Deduction 01039 is
fixed to modify a single discovery's count (FAQ-confirmed) rather than spawn a
second discovery.

**3. Browser capstone — the gate-closer.** Positioned last so it designs against
the now-stable set of input shapes:
- **#447 — 2b: typed `PlayerAction` elimination ✅ shipped (PR #460).** Open-turn
  gameplay now flows through `ResolveInput(PickSingle(OptionId))` against an
  open-turn `AwaitingInput` action menu (the engine surfaces `legal_actions` as
  the menu; `InvestigatorTurn::awaits_input` → true). The 11 typed gameplay
  variants are gone — the wire surface is `StartScenario` + `ResolveInput`; an
  internal `TurnAction` id→action map (`dispatch_turn_action`) is the sole
  gameplay path, re-enumerated at resolve (not cached). The test-only
  `PerformSkillTest` was removed too (→ `test_support::perform_skill_test*`). The
  web client lost its bespoke open-turn controls — gameplay renders through
  `AwaitingInputView`'s `PickSingle` option list (flat labels; richer per-option
  metadata beyond `label` was explicitly out of #205's scope — a future enrichment).
  **Split out:** **#458** (deterministic resume-token, §F — `ResumeToken(0)` stays
  for now) and **#459 ✅ shipped (PR #461)** — see the picker bullet below.
- **#205 — structured input rendering ✅ shipped (PR #462).** `InputRequest` gained an
  `InputKind { PickSingle, PickMultiple, Confirm }` discriminator (variant names mirror
  `InputResponse` 1:1) plus an orthogonal `skippable: bool`; the ambiguous
  `prompt`/`choice` constructors were replaced by `pick_single`/`pick_multiple`/`confirm`
  + a chainable `.skippable()`, and every engine prompt site declares its kind.
  `AwaitingInputView` switches on `kind` (Confirm → a Confirm button — the gate fix) and
  renders a Skip button whenever `skippable` (the reaction/fast-window decline path, which
  previously had no control). Richer per-option metadata beyond `label` is **not** in
  scope — this PR is the discriminator only.
- **Investigator/scenario picker ✅ shipped (PR #461 — #459 + #224).** Seating moved
  out of `PlayerAction::StartScenario` into the non-logged engine fn `seat_and_open`;
  the server seats at game-creation and persists the seated, mulligan-pending state as
  the seed (`CreateGameRequest` carries the roster; a bad roster → **422**, no orphan
  row). `PlayerAction` collapsed to a single `ResolveInput` variant — the **action log
  is `ResolveInput`-only**, and the setup shuffle is baked into the frozen seed
  `RngState` (replay no longer re-runs setup RNG). Migration `0002` persists the seed's
  `EngineOutcome` so `load` restores an `AwaitingInput` seed from an empty log. The
  browser **picker** (`picker.rs`) collects an investigator + scenario and drives
  creation (`ConnStatus::AwaitingRoster`; Roland seats with a placeholder default deck
  of implemented cards); the old Start-scenario button (`controls.rs`/`legality.rs`) is
  deleted. **#224** folded in: a non-empty roster is mandatory (single seating path),
  and the ~37 `StartScenario` test sites migrated to `seat_and_open` (game-core's own
  tests seat synthetic `TEST_INV` via the test registry, preserving crate layering).
- **End-to-end browser playthrough** of The Gathering to a resolution — the sole
  remaining gate item. The Mythos-encounter-draw stall is **resolved** by #205's
  `InputKind` discriminator (PR #462): the draw now renders a Confirm button and resolves,
  and skippable windows render a Skip control. The picker → seating → mulligan →
  investigation → Mythos flow all works in-browser (PR #461). What remains is *exercising*
  the full playthrough to a resolution end-to-end — no known engine/client blocker.

  > **Dev-loop note (not a gate blocker):** the wire-format change in #205 means a stale
  > server binary + freshly-rebuilt client silently hangs at `<no game>` — the client drops
  > the un-parseable old-shape `Hello` (`transport.rs` `if let Ok(msg)`), leaving `game:
  > None`. Restart both processes after a wire change. Surfacing a visible
  > version-mismatch status instead of the silent drop is a possible future hardening
  > (out of #205 scope).
- **Visual card rendering (#519, PR #520 — display-only).** Hand cards now render as
  faithful mini-card rectangles (cost / name / traits / translated text / slots / skill
  icons, class-coloured) via a reusable `Card` component (`crates/web/src/card.rs`),
  replacing the flat name list. First slice of a **zone-by-zone** rework — in-play, threat,
  locations, enemies, and act/agenda stay text until their own slices. ArkhamDB text markup
  (`[symbol]` / `[[trait]]` / `<b>`) is translated to text **chips** by a pure
  `parse_card_text` (split from rendering so it's native-testable); unknown tokens render
  verbatim *with brackets* to surface unmapped markup. The ArkhamDB **icon font is deferred**
  on provenance grounds (vendored-asset discipline, cf. the P6.4 leptos-use deferral), with
  the chip→glyph seam built so it drops in without restructuring — revisit near a future
  merge. Spec/plan: `docs/superpowers/specs/2026-06-29-web-card-rendering-design.md`,
  `docs/superpowers/plans/2026-06-29-web-card-rendering-hand.md`.
  - **Slice 2 — in-play assets (#521, PR #522).** `Card` gained an optional
    `in_play: Option<CardInPlay>` prop (extend, don't fork): the printed face minus the
    cost corner, plus live per-instance state — exhausted (`card--exhausted` dim + badge),
    uses chips, and soak chips (`dmg`/`hor` vs the asset's health/sanity) built by a pure
    `live_state_chips`. The board's in-play list is now a `.card-row` of `Card`s. Still
    display-only; threat area, the spatial map (locations/enemies), and act/agenda remain
    later slices. Spec/plan: `docs/superpowers/specs/2026-06-30-in-play-card-rendering-design.md`,
    `docs/superpowers/plans/2026-06-30-in-play-card-rendering.md`.
  - **Slice 3 — engaged enemies (#523, PR #524).** A **dedicated `EnemyCard`** component
    (fork, not `Card`): enemies are a different data source — the `Enemy` *state struct*
    carries stats + live state, vs `Card`'s `code`→registry + `CardInPlay`. Renders combat
    stats (fight/evade/health/attack), keyword chips (Hunter/Retaliate/Victory), traits,
    ability text (looked up by code via the registry, reusing `parse_card_text` +
    the now-`pub(crate)` `render_segments`), and the `card--exhausted` dim + badge; red
    `card--enemy` border. Engaged enemies render as a `.card-row` in the threat area; the
    map's enemy tokens and threat-area treacheries stay later slices, and `prey` display is
    deferred (moot in 1p). Spec/plan:
    `docs/superpowers/specs/2026-06-30-enemy-card-rendering-design.md`,
    `docs/superpowers/plans/2026-06-30-enemy-card-rendering.md`.
  - **Slice 4 — location cards / the map (#527, PR #528).** The spatial map's nodes now
    render as location cards (name, `shroud` chip, `clues`, traits, ability text, `Victory`
    chip — traits/text/victory from the corpus by `loc.code`); unrevealed nodes withhold
    that info *structurally* (`loc.revealed.then(...)`). The grid is **normalized to the
    origin** (`layout_positions` subtracts min col/row) so a departed location (the Study,
    post-Act-1) leaves no dead column; connection lines + `map_extent` derive from the same
    positions. Unengaged-enemy tokens (deferred from slice 3) render in the nodes. Two
    layout fixes shipped alongside (PRs #526): `.board-main` stacks the map above the
    investigators panel (the map's absolutely-positioned nodes overflowed a shrunk flex
    row), and a sticky `.action-bar` keeps the controls reachable on the now-tall board.
    Registry-discipline note for future map tests: metadata-dependent rendering is tested
    in its own binary (`tests/location_card.rs`, real `cards::REGISTRY`, mounts
    `location_map` directly) since registry install is first-wins per process — `tests/map.rs`
    keeps the synthetic registry. Spec/plan:
    `docs/superpowers/specs/2026-06-30-location-card-rendering-design.md`,
    `docs/superpowers/plans/2026-06-30-location-card-rendering.md`. Interior-gap collapse,
    full cards inside nodes, and clickable locations stay out of scope.
  - **Slice 5 — threat-area treacheries (#529, PR #530).** Threat-area treacheries (Cover
    Up, Frozen in Fear) render via the **existing `Card` generic (`None`) arm** (no new
    component — a treachery is a `CardInPlay`, exactly `Card`'s model): name/traits/text/
    weakness + a clues-on-card chip (Cover Up's 3 clues). `live_state_chips` gained a
    `clues N` chip and the generic arm gained a `card-live` footer (exhausted dim/badge
    stays Asset-only — no in-scope non-asset exhausts). Treacheries render in the threat
    `.card-row` alongside the engaged-enemy cards; dead `.threat ul` removed. **This
    completes the display-only card coverage** for every zone (hand, in-play, enemies,
    locations, act/agenda terse-only, threat area). Remaining web work: act/agenda cards
    (terse phase-bar today) and the **interactivity pass** (cards/locations/enemies grow
    their own action buttons; retire the sticky `.action-bar`). Spec/plan:
    `docs/superpowers/specs/2026-06-30-treachery-card-rendering-design.md`,
    `docs/superpowers/plans/2026-06-30-treachery-card-rendering.md`.
  - **Slice 6 — act/agenda cards + turn tracker + collapsible log (#532, PR #533).** A
    three-column layout. **Act/Agenda render as cards** atop the board (`act_agenda.rs`, a
    `location_map`-style pure fn in `BoardView` — act shows `clues to advance: N` since the
    act has no running clue counter; agenda shows real `doom d/N`). A **right-hand
    `TurnTrackerView`** outlines the round's four phases with their RR sub-steps + structural
    player windows (the `ROUND` const is transcribed from RR Appendix II pp. 23-25 — step
    labels, loop tails elided — and cited in the module doc; reviewer-verified against the
    pinned PDF), highlighting the current phase. The **left event log is collapsible**. The
    `phase_bar` is **retired** (phase/round → tracker, act/agenda → cards). This finishes
    the display-only card/layout pass for every zone. The remaining web work is the
    **interactivity pass** (next bullet). Spec/plan:
    `docs/superpowers/specs/2026-06-30-act-agenda-and-sidebars-design.md`,
    `docs/superpowers/plans/2026-06-30-act-agenda-and-sidebars.md`.
  - **Interactivity pass (#206 umbrella; slices S0–S6 = #535–#541).** Retires the flat
    `.action-bar`: actionable board entities glow and open a **context menu** of their legal
    actions; multi-select (mulligan/commit/discard) is click-to-select on the hand; windows /
    soak / effect choices resolve on their source cards; a slim prompt banner carries prompt
    text + Confirm/Pass. Engine-authoritative — each option the board offers *is* an option the
    engine enumerated as legal, so the board can never surface an action the server rejects (no
    client-side legality re-computation; the drift #206 warned of is structurally impossible).
    Design (whole-model umbrella): `docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md`.
    - **S0 — `OptionTarget` anchor on `ChoiceOption` (#535, PR #542).** Each wire `ChoiceOption`
      gains a structured `OptionTarget` (`Global` / `Location` / `Enemy` / `HandCard` /
      `CardInstance` / `Act`); `turn_menu` derives real anchors from a new `TurnAction::target`,
      every other option-builder emits `Global` for now. `label` stays the full engine-authored
      string. Required wire field (#453 precedent). Engine + protocol only — no web behavior
      change (the bar still reads `label`). Plan:
      `docs/superpowers/plans/2026-07-01-interactivity-s0-optiontarget.md`.
    - **S1 — web plumbing + location context menus (#536, PR #543).** The shared routing seam:
      a `web::interaction` module (pure `pending_options` / `options_for` + a `PendingOptions`
      context signal, native-tested) and a wasm-only `ContextMenu` component (backdrop + a
      button per option; a click submits `ResolveInput(PickSingle)` and closes). Map nodes glow
      (`.map-location.actionable`) and open their menu; the flat action bar is untouched (bar
      keeps everything until S6), so S1 is purely additive. Per-entity placement, shared
      component; `ContextMenu` is wasm-gated (submits via the wasm-only `OutboundTx`) while the
      glow/open/`on:click` stay non-gated so the node compiles on host. Plan:
      `docs/superpowers/plans/2026-07-01-interactivity-s1-location-menus.md`.
    - **S2 — enemy menus + fixed-at-cursor (#537, PR #544).** `EnemyCard` glows and opens a
      Fight/Evade context menu (`options_for(Enemy(id))`). The shared `ContextMenu` moved to
      **`position: fixed` at the cursor** — `open` is now `RwSignal<Option<(i32,i32)>>` and a
      wasm-only `interaction::menu_layer` (a `.menu-hit` coord-capture layer + the menu) DRYs
      the trigger; S1's map node migrated to it. This **resolves S1's `overflow` clipping**
      (fixed escapes overflow) — but not `z-index` *stacking*: `.map-location` sets `z-index:1`,
      so `.map-location.actionable` gets `z-index:20` to float its menu above the sticky
      `.action-bar` (cards set no `z-index`, so theirs escape to root). **Deferred:** map-token
      (co-located/unengaged) enemy menus — rare in 1p (enemies auto-engage). Plan:
      `docs/superpowers/plans/2026-07-01-interactivity-s2-enemy-menus.md`.
    - **S3 — hand Play menu + multi-select + prompt banner (#538, PR #545).** A `HandCardView`
      wrapper (keeps `Card` display-only) gives a playable hand card a "Play" `menu_layer`, and —
      when a `PickMultiple` is live — turns hand cards into click-to-select (`.hand-slot.selected`
      ring). Introduces the deferred **prompt banner** (`prompt_banner.rs`, bottom-fixed): for a
      `PickMultiple` it renders prompt text + Confirm (submits the selection) + Pass (Skip). New
      `MultiSelect` context ({`active` derived from the outcome, `selected` set}) + a pure
      `is_multi_select`. **`input.rs`'s `PickMultiple` arm is removed** (the board hand + banner
      replace it; bar keeps `PickSingle`/`Confirm`/`Skip`; `tests/input.rs` deleted) — the agreed
      deviation from "bar keeps everything", since two selection UIs would collide. Selection
      click is non-gated (no coords). Plan:
      `docs/superpowers/plans/2026-07-01-interactivity-s3-hand-and-multiselect.md`.
    - **S4 — in-play/investigator card menus + reaction-window triggers (#539, PR #546).** The
      first engine change since S0: `build_resolution_options` anchors reaction candidates by
      `CandidateSource` (`InPlay`→`CardInstance`, `Hand`→a new `OptionTarget::HandCardByCode` — every
      copy of a Fast reaction event, so `OptionTarget` drops `Copy`, `Board`→`Global`); `drive_fast_window`
      reuses `TurnAction::target`. The anchor is **display-only** (the resolve path indexes
      `candidates[i]` by the echoed `OptionId`, never the anchor). Web: `InPlayCardView` wraps in-play,
      threat, **and the investigator card** (so Roland's signature reaction glows — a review catch);
      `HandCardView` dual-matches via `options_for_hand_card`. `PromptBanner` extended to skippable
      windows (prompt + Pass); `input.rs`'s Skip removed (the bar keeps window *options* so `Board`/
      `Global` stays reachable until S6). Plan:
      `docs/superpowers/plans/2026-07-02-interactivity-s4-in-play-and-window-triggers.md`.
    - **S5–S6** (#540–#541) queued: act/soak/effect-choices (S5), global-action homes + bar
      retirement (S6). The shared `menu_layer` / fixed-at-cursor + `PromptBanner` (now also skippable
      windows) are the seams they extend; new anchors inside a `z-index`ed ancestor must float their
      menu like the map node. A queued follow-up (display-only) reworks the **investigator panel**
      (card + folded skills/vitals + actions/resources beside the hand).

**Deferred past the gate:** #353 (uses-depletion — no Gathering card; gated on
Forbidden Knowledge / Grotesque Statue), #294 (multi-soak-window drain —
unconstructible in scope, `debug_assert` guards it), #427/#429 (native-loop soak
residue — rare in 1p), #119/#26 (behaviour-preserving cleanups — fold in
opportunistically).

## Frame-model end-states (#393)

For a future author who sees the partial state and wonders what's "missing":
- **C checkpoint** ✅ and **EmitEvent-frame** (3rd checkpoint) ✅ — both shipped.
- **2b** (typed `PlayerAction` → `OptionId`-only) ✅ — shipped (PR #460). The open
  turn is an `AwaitingInput` menu; gameplay is `ResolveInput(PickSingle(OptionId))`
  dispatched via the internal `TurnAction` map. `PlayerAction` = `StartScenario` +
  `ResolveInput` (single-variant end-state deferred to #459).
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

**Asset slots (PR #516, #498).** Slot limits are enforced at the RR "entering the
slot" moment (`dispatch/slots.rs`; `dispose_play_from_hand`'s InPlay branch →
`enter_asset_making_room`), **not** at validation. `default_slot_capacity` holds the
RR p.19 defaults (Ally/Body/Accessory 1, Hand/Arcane 2); a full slot does *not* block
the play (`check_play_card` rejects only `need > cap`, unreachable in corpus) — instead
occupying assets are discarded to make room: forced single-candidate auto-discards,
2+ candidates suspend on a `Continuation::SlotDiscard` `PickSingle` (mirrors the soak
`DamageAssignment` driver). A slot-modifying card (none in Core/Dunwich) turns
`default_slot_capacity` into a per-investigator query. The in-play-asset discard sequence
is now one helper, `cards::discard_card_from_play` (#119, reused by soak-defeat,
uses-depletion, `Cost::DiscardSelf`, make-room).

**Seating & the seed (PR #461).** Seating is **not** a player action — it's the engine
fn `seat_and_open(setup_state, &roster) -> ApplyResult` (wraps the internal
`start_scenario` + `drive` via `apply_via`). Hosts call it at game-creation and persist
the **seated, mulligan-pending** result as the seed; the action log is `ResolveInput`-only.
Two consequences a future persistence/replay PR must respect: (1) the setup shuffle is
baked into the seed's frozen `RngState`, so replay never re-runs setup RNG — don't
re-seed; (2) the seed can itself be `AwaitingInput`, so the seed's `EngineOutcome` is
persisted alongside `seed_state` (server migration `0002` / `seed_outcome` column) and
`load` initializes the outcome from it before replaying — there is no `state → pending
outcome` reconstruction, so a paused seed with an empty log would otherwise load as
`Done`. A roster is mandatory (`seat_and_open` rejects an empty one); seating always
seats investigators `Active`, so "mulligan excludes an eliminated investigator" is
defensive (covered by a direct `active_investigators_in_turn_order` unit test, not via
seating).

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
