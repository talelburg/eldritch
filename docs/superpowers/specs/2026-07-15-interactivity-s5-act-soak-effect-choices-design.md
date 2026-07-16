# Interactivity S5 — act advance + interactive soak + effect `ChooseOne` on the board — design

**Date:** 2026-07-15
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); slice S5 of the board interactivity pass
**Issue:** #540 · **Umbrella:** #206 · **Depends on:** S0–S4 (all merged)
**Umbrella design:** `docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md` (Sections 2–3)

## Goal

The three remaining framework prompts that still reach the wire as
`OptionTarget::Global` find their board homes: **Advance Act** on the act card,
**interactive soak** as click-to-assign on the soak cards, and effect
**`ChooseOne`** anchoring to the chosen enemies / locations. This clears the last
framework prompts that block bar retirement; the genuinely-global open-turn actions
(End turn / Gain resource / Draw) and the encounter draw get their homes in S6, the
closer. A few narrow `Global` cases are deliberately left (investigator-choice,
effect-branch choices, non-act `Board` reactions) — see Out of scope.

This is the umbrella's approach **A** (the engine enriches each option's anchor;
the board routes off it) applied to the last three prompt families. It is almost
entirely a **re-anchoring** slice: the options are already surfaced and already
resolve correctly by `OptionId` index — S5 only stops discarding their target so
the board can render them on the right entity.

## Decisions (settled in brainstorm)

- **Keep S5 focused; #492 is a separate follow-up.** Issue #540 flags #492
  (surface single-option soak/attack-order auto-binds as choices when interactive)
  as adjacent — "coordinate if it lands together." They are cleanly separable:
  S5's re-anchoring touches only options the engine *already* surfaces, whereas
  #492 changes the **surfacing gate** (soak currently prompts only when a point has
  2+ eligible targets — `combat.rs::advance_distribution`). S5 introduces no
  behavior change; #492 (a rules-review surface: when should a forced single-target
  soak pause?) ships on its own. Consequence: soak glows only when there is a real
  choice (2+ eligible); a single-eligible-target point keeps auto-assigning
  silently — correct, nothing to choose.
- **Anchor the round-end act-advance reaction to the act card too.** The round-end
  act-advance (Act 01109) reaches the wire as a `CandidateSource::Board` candidate →
  `OptionTarget::Global`, homed in the prompt banner (the #549/#550 fix). S5 anchors
  it to `OptionTarget::Act` so **every** "advance the act" — open-turn action *and*
  round-end reaction — lives on the act card under one matcher. This is a small step
  beyond #540's literal text but is the direct consequence of the choice above (see
  W4: without it the option would render twice).

## Architecture

### Engine — `crates/game-core`

Three anchor changes; none touches resolution (every resolve path indexes
`candidates[i]` / re-derives targets by the echoed `OptionId`, never the anchor —
the anchor is display-only, exactly as in S4).

- **E1 · Soak → `CardInstance`.** `dispatch/combat.rs::prompt_current_point`
  (fn ~L858) builds the per-point `PickSingle` via
  `super::hunters::candidate_options(&targets)` (`hunters.rs::candidate_options`,
  fn ~L385), which is generic over any `Debug` candidate and calls
  `ChoiceOption::global` (~L391), throwing the id away. The soak card's
  `CardInstanceId` is already in hand — each `targets` element is a
  `DistributionTarget` (`combat.rs` ~L765): `Asset(CardInstanceId)` or
  `Investigator`. **Build the options inline at the soak call site** over
  `DistributionTarget`: `Asset(id) → OptionTarget::CardInstance(id)`,
  `Investigator → OptionTarget::Global`. Labels stay byte-identical to the current
  `candidate_options` output (the "N damage / M horror left" count is part of the
  `prompt` string, not the labels — unaffected). **Do not** change
  `hunters::candidate_options` — it is shared with hunter-move / engage prompts;
  specialize only the soak caller. `resume_damage_assignment` (fn ~L891) is
  unchanged: it re-derives `targets` and validates the pick by index.

- **E2 · Effect `ChooseOne` → `Enemy` / `Location`.** Only
  `evaluator.rs::resolve_grounded_choice<Id: Copy>` (fn ~L1597) — the
  enemy/location/investigator chooser — is in scope. It currently drops the
  candidate ids because its shared renderer `dispatch/choice.rs::awaiting_choice`
  (fn ~L43) takes labels only and calls `ChoiceOption::global` (~L47). The chosen
  ids are the `candidates: &[Id]` slice, fully in scope at build time. Thread a
  per-candidate `Id → OptionTarget` mapper from each typed caller into a
  target-aware `awaiting_choice` (add `awaiting_choice_anchored(prompt,
  (label, target) pairs)` or extend the existing helper):
  - `ground_location_choice` (~L1663) → `OptionTarget::Location(id)`
  - `ground_enemy_choice` (~L1687) → `OptionTarget::Enemy(id)`
  - `ground_fight_target_choice` (~L1725) → `OptionTarget::Enemy(id)`
  - `ground_investigator_choice` (~L1638) → **stays `Global`** (out of scope below).
  `step_choose_one` (fn ~L522) chooses between *effect branches*, not entities —
  no id exists, stays `Global`.

- **E3 · Round-end act-advance → `Act`.** `dispatch/reaction_windows.rs::build_resolution_options`
  (fn ~L598) maps `CandidateSource::Board` (game_state.rs ~L1620: "a scenario board
  card — act / agenda; fires by `code`") to `OptionTarget::Global` (~L621). Change
  that arm to emit `OptionTarget::Act` **iff `cand.code` equals the current act's
  code**, else `Global`. Thread the current-act code into `build_resolution_options`
  (its callers — e.g. `open_queued_reaction_window` ~L648 — hold `cx.state`); pass
  `current_act: Option<&CardCode>` to keep the fn unit-testable. Codes are unique so
  this cannot misfire; `OptionTarget::Act` is nullary (one current act) so it needs
  no id. No new `CandidateSource` variant — the identification is localized to the
  one option-builder, using authoritative state, not registry/type sniffing.

### Web — `crates/web`

One new render; the rest rides seams built in S1–S4.

- **W1 · Act card glows** — the only new component work.
  `act_agenda.rs::act_agenda_view` (fn ~L26) renders `<article class="card
  card--act">` (~L33) as **plain display-only**: no `PendingOptions`, no
  `actionable`, no `menu_layer`. Adopt the `EnemyCard` / location-node pattern
  (`enemy_card.rs::EnemyCard` ~L53; `map.rs::location_map` per-node ~L183):
  `use_context::<PendingOptions>()`, `options_for(&pending, OptionTarget::Act)`,
  add the `actionable` class when non-empty, and `menu_layer(menu_opts, open)`.
  **One matcher serves both** the open-turn "Advance act" (`TurnAction::AdvanceAct
  → Act`, wired in S0 — `enumerate.rs::TurnAction::target` ~L160) *and* the
  round-end reaction (E3): both are `OptionTarget::Act`. The act card sets no
  `z-index`, so its fixed-at-cursor menu escapes to root like the enemy/asset
  cards (no `.map-location`-style float needed).

- **W2 · Soak — zero web change.** Soak assets are in-play cards already rendered
  via `card.rs::InPlayCardView` (~L496), which matches
  `options_for(pending, OptionTarget::CardInstance(instance_id))` (~L502). E1's
  anchor lights them up through the existing component. The banner's remaining
  damage/horror text already comes from the engine `prompt` string
  (`prompt_current_point`); `prompt_banner.rs` shows `request.prompt` verbatim.

- **W3 · `ChooseOne` targets — no new component.** Enemy/location `ChooseOne`
  options glow via the existing `EnemyCard` (`OptionTarget::Enemy`) and
  `location_map` (`OptionTarget::Location`) matchers once E2 anchors them.

- **W4 · Banner filters to un-anchored options.** `prompt_banner.rs::PromptBanner`
  (~L45–65) currently renders **all** of a skippable window's `PickSingle` options
  as `<button class="banner-option">` (the #549/#550 fix, so `Board`/`Global`
  options reachable nowhere else have a home). Filter that list to
  `target == OptionTarget::Global`. The round-end act-advance is now `Act`-anchored
  → act card, so the banner stops duplicating it; genuinely-`Global`/`Board` window
  options still land in the banner (preserving #550's guarantee and matching S6's
  end-state where the banner is the catch-all for un-homed options). Prompt text,
  Confirm, and Pass are unchanged.

## Data flow (round-end act advance, the newly-unified path)

engine queues the Act 01109 round-end reaction → `open_queued_reaction_window`
builds a skippable `PickSingle` → `build_resolution_options` sees a
`CandidateSource::Board` candidate whose `code` is the current act → anchors it
`OptionTarget::Act` → `act_agenda_view` glows the act card and opens an "Advance
act" menu → click submits `ResolveInput(PickSingle)` → engine advances. The amber
banner shows the window prompt + Pass; it no longer renders the advance as a
button (W4). The open-turn "Advance act" reaches the same act-card menu via the
turn-menu `Act` anchor — one home, two sources.

## Testing

- **Engine (native):**
  - Soak: unit `prompt_current_point`'s option build — synthetic soakers →
    `Asset(id)` targets anchor `CardInstance(id)`, the `Investigator` target
    anchors `Global`; labels unchanged from the pre-S5 output.
  - `ChooseOne`: unit `resolve_grounded_choice` via each `ground_*` caller —
    `ground_location_choice` → `Location` anchors, `ground_enemy_choice` /
    `ground_fight_target_choice` → `Enemy` anchors, `ground_investigator_choice` →
    `Global`.
  - Act: extend the existing `build_resolution_options` test (the `Board →
    Global` assertion at `reaction_windows.rs` ~L2132) — a `Board` candidate whose
    code **is** the current act → `Act`; a `Board` candidate whose code is not →
    `Global`.
- **Web (native):** act-card option matching is a pure fn over
  `(game, pending)` — assert glow iff an `Act`-anchored option is live; extend
  `interaction` tests only if a new matcher shape is introduced (it is not).
- **Web (headless):** act card glows and opens an "Advance act" menu that submits
  `PickSingle` — cover **both** the open-turn action and the round-end reaction
  (real-registry test binary, the `location_card.rs` first-wins-registry
  precedent); soak glow rides the existing `InPlayCardView` headless test; extend
  `tests/prompt_banner.rs` — an anchored option is **not** rendered as a banner
  button, a `Global` option still is.
- Full 7-job CI gauntlet green.

## What "done" looks like (this slice)

- An advanceable act glows and opens an "Advance act" menu that advances it —
  from the open-turn menu *and* the round-end window, one card home.
- An interactive soak prompt (2+ eligible soakers) glows the soak cards; clicking
  one assigns to it; the banner shows the remaining damage/horror.
- An effect `ChooseOne` over enemies / locations glows the candidate cards/nodes
  and submits on selection.
- The banner renders prompt text + Confirm/Pass + only genuinely-`Global` options.
- Native + headless tests pass; full 7-job CI gauntlet green.

## Out of scope (documented deferrals)

- **#492** — surface single-option soak/attack-order auto-binds as choices when
  interactive. A behavior change to the surfacing gate; its own PR.
- **`ground_investigator_choice` anchoring** — stays `Global`. No in-scope
  multi-investigator `ChooseOne` in solo (a single candidate = you, which
  auto-binds unless interactive). The investigator-card `CardInstance` anchor
  (available since #448) is a trivial later add if a card needs it.
- **Agenda `Board` reactions** — no `OptionTarget::Agenda`; agenda advance is
  doom-forced, not player-triggered, so no agenda option needs a card home.
- **`step_choose_one` effect-branch choices** — the choice is between `Effect`
  branches, not board entities; no anchor exists.
- **Global-action homes + `.action-bar` retirement** — S6 (the closer). The flat
  bar keeps rendering every option (including the now-anchored ones) until then, so
  S5 lands as a purely additive slice with the bar as a working fallback.
