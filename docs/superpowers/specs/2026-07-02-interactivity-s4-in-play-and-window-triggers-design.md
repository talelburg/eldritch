# Interactivity S4 — in-play Activate + reaction-window Trigger menus — design

**Date:** 2026-07-02
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); slice S4 of the board interactivity pass
**Issue:** #539 · **Umbrella:** #206 · **Depends on:** S0–S3 (all merged)
**Umbrella design:** `docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md` (Sections 2–3)

## Goal

In-play (and threat-area) cards get their actions on the board: **Activate**
(open-turn activated abilities) and **Trigger** (reaction / Fast-window candidates)
open a context menu on the card. Reaction triggers are anchored to their **source
card** (an engine change — the first since S0), and a reaction/Fast window's **Pass**
+ prompt move to the prominent bottom banner.

## Decisions (settled in brainstorm)

- **Anchor in-play *and* hand reaction candidates.** `InPlay` → the card instance;
  `Hand` (Fast events like Evidence! 01022) → the card **by code**, so every
  duplicate copy in hand glows (not one arbitrary index). `Board` candidates stay
  `Global` (no card; bar-reachable).
- **Window Pass moves to the banner.** The banner (amber strip) renders the prompt +
  Pass for skippable windows; `input.rs`'s Skip control is removed.

## Architecture

### Engine — `crates/game-core`

- **New `OptionTarget` variant** (`engine/outcome.rs`):
  `HandCardByCode { investigator: InvestigatorId, code: CardCode }`. `CardCode` is
  `String`-backed, so **`OptionTarget` drops `#[derive(Copy)]`** (keeps `Clone`,
  `PartialEq`, `Eq`, `Serialize`, `Deserialize`). Low-ripple: `options_for` filters
  by `==`, and every construction site moves the value (no `Copy` reliance in engine
  or web — verified). A code anchor (not `hand_index`) is deliberate: a queued Fast
  reaction event is code-identified (playing either copy is equivalent), and it lets
  every matching hand card highlight.
- **`build_resolution_options`** (`engine/dispatch/reaction_windows.rs`) replaces the
  S0 `::global` placeholder, mapping each candidate's `source` (+ `controller`):
  - `CandidateSource::InPlay(id)` → `OptionTarget::CardInstance(id)`;
  - `CandidateSource::Hand` → `OptionTarget::HandCardByCode { investigator: cand.controller, code: cand.code.clone() }`;
  - `CandidateSource::Board` → `OptionTarget::Global`.
- **`drive_fast_window`** (same file) maps its `TurnAction` options through S0's
  `a.target(cx.state)` (was `::global`): `PlayCard` → `HandCard`, `ActivateAbility`
  → `CardInstance` — so Fast plays already anchor correctly.
- **Open-turn `ActivateAbility` is already `CardInstance`-anchored** by S0's
  `turn_menu`/`TurnAction::target` — no change.

### Web — `crates/web`

- **New `InPlayCardView` wrapper** (`card.rs`, sibling of `HandCardView`; `Card`
  stays display-only). `#[component] pub fn InPlayCardView(instance: CardInPlay)`
  renders `<div class="card-slot"><Card code=instance.code in_play=instance/>
  …menu…</div>`. Reads `PendingOptions`; `menu_opts = options_for(pending,
  OptionTarget::CardInstance(instance.instance_id))` — one matcher covers **both**
  open-turn Activate and reaction Trigger options. When non-empty: `.card-slot`
  gets `actionable` + a wasm-only `menu_layer`. No selection mode.
- **`board.rs`:** the in-play **and** threat lists switch from `<Card in_play=c/>`
  to `<InPlayCardView instance=c/>`.
- **`HandCardView` dual-match** (`card.rs`): its non-multiselect branch swaps
  `options_for(HandCard{index})` for a new
  `interaction::options_for_hand_card(options, investigator, index, code) ->
  Vec<ChoiceOption>` that returns options whose target is
  `HandCard { investigator, hand_index: index }` **or**
  `HandCardByCode { investigator, code }`. So a playable hand card still shows
  "Play"; during a reaction window every copy of a Fast reaction event glows.
  (Reaction windows are `PickSingle`, so `multi_select.active` is false and this
  branch runs.)
- **`PromptBanner` extends to windows** (`prompt_banner.rs`): render when the live
  outcome is `PickMultiple` (Confirm + prompt + [Pass if skippable], as S3) **or**
  `skippable` (prompt + **Pass**). The Pass submits `ResolveInput(Skip)`. A
  reaction/Fast window's prompt + Pass thus surface in the amber banner.
- **`input.rs`:** the `skip_button` (Skip control) is **removed** — Pass lives in
  the banner. The bar keeps rendering the window's `PickSingle` option buttons (so
  `Board`/unanchored options stay reachable while the bar coexists) but no longer
  its own Skip.
- **CSS:** `.card-slot { position: relative; }` + `.card-slot.actionable` (mirrors
  `.hand-slot`).

## Data flow (a reaction window, e.g. Roland's after defeating an enemy)

engine queues the trigger → `emit_event` opens a skippable `PickSingle` window →
`build_resolution_options` anchors the candidate to `CardInstance(Roland's card)` →
`InPlayCardView` for Roland's card glows and opens a "Trigger" menu → click submits
`ResolveInput(PickSingle)` → engine fires the reaction. Meanwhile the amber banner
shows the window prompt + Pass.

## Testing

- **Engine (native):** unit-test `build_resolution_options` — synthetic `InPlay` /
  `Hand` / `Board` candidates → assert `CardInstance` / `HandCardByCode` / `Global`
  anchors; a `TurnAction::target`-anchor check for the Fast-window path if not
  already covered by S0's `target_maps_each_variant`.
- **Native (web):** `options_for_hand_card` — returns the `HandCard{index}` option
  and the `HandCardByCode{code}` option for a matching card; excludes a
  different-code/-index/-investigator anchor.
- **Headless — `InPlayCardView`** (new `tests/in_play_card.rs`, real registry): a
  `CardInstance`-anchored option glows the card and opens a menu that submits
  `PickSingle`; an inert instance has no glow. A `Card` rendered bare (not wrapped)
  is unchanged.
- **Headless — hand reaction-by-code** (extend `tests/card.rs`): a
  `HandCardByCode`-anchored option glows the matching hand card and opens a menu.
- **Headless — banner window Pass** (extend `tests/prompt_banner.rs`): a skippable
  `PickSingle` outcome renders the banner with the prompt + a Pass → `Skip`; a
  non-skippable `PickSingle` (open turn) renders no banner.
- **Regression — `input.rs`:** `tests/awaiting_input.rs`'s skippable-Skip tests
  (`skippable_window_renders_skip_button_and_submits_skip`,
  `non_skippable_pick_single_has_no_skip_button`) move to the banner tests (the bar
  no longer renders a Skip); the `PickSingle` option-list + `Confirm` tests stay.

## What "done" looks like (this slice)

- An in-play asset with an activatable ability glows and opens an "Activate" menu
  that performs it; a card with a live reaction (Roland/Dr. Milan) glows and opens a
  "Trigger" menu during its window; every hand copy of a Fast reaction event glows.
- A reaction/Fast window's prompt + Pass show in the amber banner; the flat bar no
  longer shows a Skip.
- Native + headless tests pass; full 7-job CI gauntlet green.

## Out of scope (later slices)

- Act advance + soak + effect-choices (S5), global-action homes + bar retirement (S6).
- `Board` reaction candidates getting a non-`Global` home (no card; deferred).
- Moving the window's *option list* fully off the bar (a `Board` option would be
  unreachable) — the bar keeps rendering options until S6.
- Multi-investigator selection scoping (still solo).
