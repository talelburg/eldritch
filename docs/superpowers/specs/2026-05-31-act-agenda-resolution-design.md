# Act + agenda decks, doom counter, push-model resolution — design

**Issue:** #73 (`engine: act + agenda decks, doom counter, threshold-based agenda advance`)
**Phase:** 4 — Scenario plumbing (ordering slot #12)
**Date:** 2026-05-31

## Goal

Add the act + agenda decks — the win/lose timer every scenario runs on — and
the resolution mechanism they feed. Doom accumulates on the agenda each round;
when it meets the agenda's threshold the agenda advances. Investigators spend
clues to advance the act. Reaching a printed resolution point on an act/agenda
(or the last investigator being eliminated) ends the scenario.

This is the last content slot before the Phase-4 closing demo (slot #13): with
it, the synthetic fixture can play setup → resolution end-to-end.

## Key rules findings (verified against the Rules Reference)

Two — and only two — things resolve a scenario, and **both are discrete push
events**, not conditions you poll for:

1. **A resolution point `(→R#)` is reached** (RR p.3): *"Some instructions in
   the act and agenda decks (as well as on other encounter cardtypes) contain
   resolution points, in the format of '(→R#).' If a resolution point is
   reached, the scenario ends."* Fired by resolving that card's instruction
   text at a specific moment.
2. **The last investigator is eliminated** (RR p.10, Elimination step 6): *"If
   there are no remaining players, the scenario ends. Refer to 'no resolution
   was reached' entry for that scenario in the campaign guide."* Fired at the
   point we already emit `Event::AllInvestigatorsDefeated`.

There is no "scan the board and notice the scenario is over" case. The agenda
deck never runs dry in well-formed data — the final agenda's reverse carries
the resolution point that ends the scenario *while advancing it*, never *past*
it (RR p.3 advance steps; RR p.24 step 1.3 doom check).

**Consequence for this PR:** PR #74's `detect_resolution` (a *pull* hook the
engine calls every `Done` apply) is the wrong shape. This PR replaces it with a
push model fired at the two trigger classes above. This also closes #131 (the
deferred idempotency latch) for free — a push fires once by construction.

## Decisions

- **Full replace of the pull model.** Drop the `detect_resolution` fn-pointer
  from `ScenarioModule`. Keep `setup` and `apply_resolution`. (Confirmed with
  user 2026-05-31.)
- **Resolution point is data, not effect DSL.** A `resolution: Option<Resolution>`
  field on each act/agenda entry models the printed `(→R#)`. Per-scenario *card
  effects* stay out of scope (issue's explicit non-goal); the structural
  pointer is all this PR needs.
- **Act advances via a new `PlayerAction::AdvanceAct` — a prototype.** This is
  the rules-faithful shape (RR p.3: spend the requisite clues, "normally a Fast
  player ability"). It is built minimally for the synthetic demo and **must take
  final form once real consumers (Phase 7 The Gathering) exercise it** — flat
  threshold only, single-step spend, no `Objective –` handling. Noted in-source
  with a `TODO`.
- **Clue-spend allocation on a surplus is a deterministic default.** RR p.3:
  "Any or all investigators may contribute any number of clues." When the group
  holds more clues than the threshold, the prototype spends a fixed order — the
  acting investigator's clues first, then the remaining investigators in
  `turn_order`, taking only as many as needed. No player choice. Letting players
  choose who contributes is out of scope (follow-up issue below); the default is
  fine for this PR and is the only reachable behavior single-player anyway.
- **Doom is agenda-only for now.** No corpus card carries doom, so RR p.24 step
  1.3's "doom on each other card in play" sums to zero — correct-by-absence.
  `TODO` for the sum when a doom-bearing card lands.
- **Synthetic fixture grows to 2 agendas + 2 acts** so an advance is observable
  (agenda 1→2 emits `AgendaAdvanced`, threshold on agenda 2 fires `Lost`; act
  1→2 then act 2 fires `Won`).

## State shape (`GameState`)

```rust
pub struct Agenda {
    /// Total doom in play required to advance this agenda (RR p.24 1.3).
    /// Flat value only for now; per-investigator scaling deferred.
    pub doom_threshold: u8,
    /// The printed (→R#) resolution point on this card's reverse, if any.
    /// `Some` on a final/terminal agenda; `None` on agendas that advance
    /// to a next card.
    pub resolution: Option<Resolution>,
}

pub struct Act {
    /// Clues the group must spend to advance this act (RR p.3). Flat only.
    pub clue_threshold: u8,
    pub resolution: Option<Resolution>,
}

// on GameState:
pub agenda_deck: Vec<Agenda>,
pub agenda_index: usize,   // cursor into agenda_deck (current = [agenda_index])
pub agenda_doom: u8,       // doom on the current agenda
pub act_deck: Vec<Act>,
pub act_index: usize,
/// Fire-once resolution latch. `None` until a resolution fires; set by
/// `request_resolution`. Closes #131.
pub resolution: Option<Resolution>,
```

Empty decks (`vec![]`, index 0) are the default for tests/fixtures that don't
care about act/agenda — every new helper short-circuits on an empty deck, the
same way the resolution hook short-circuits on `scenario_id == None`.

## Resolution-firing mechanism

The push sites live in `dispatch.rs`, which has no `ScenarioRegistry` (it's
only in `mod.rs::apply`, where `apply_resolution` runs). So split the work:

- **Dispatch sites set the latch.** Helper `request_resolution(state, res)`:
  `if state.resolution.is_none() { state.resolution = Some(res); }` — pure
  state, first-writer-wins.
- **`mod.rs::apply` emits + applies.** Capture `resolution_before =
  state.resolution.clone()` at the top. After dispatch (on any non-`Rejected`
  outcome), if `resolution_before.is_none() && state.resolution.is_some()`,
  emit `Event::ScenarioResolved { resolution }` then call the module's
  `apply_resolution` via the registry. Replaces `fire_scenario_resolution`.

This keeps event emission + `apply_resolution` adjacent and registry-local,
fires exactly once (the latch only transitions None→Some on the firing apply),
and orders `ScenarioResolved` after the events that caused it (the
`AgendaAdvanced` / `AllInvestigatorsDefeated` that triggered it).

Running on any non-`Rejected` outcome (not just `Done`) covers the narrow case
where a resolution is requested during an apply that ends `AwaitingInput`.

## Three push sites

1. **Mythos 1.3 — `check_doom_threshold`** (fills the existing `TODO(#73)` stub).
   `place_doom_on_agenda` (step 1.2 stub) does `agenda_doom += 1` first. Then:
   if `agenda_doom >= agenda_deck[agenda_index].doom_threshold`:
   - current agenda has `resolution: Some(r)` → `request_resolution(state, r)`.
   - else → emit `AgendaAdvanced`, `agenda_doom = 0`, `advance_agenda(state)`.
   `advance_agenda` increments the cursor; the "next agenda becomes current"
   step is `unreachable!()` if there is no next (a final agenda lacking a
   resolution = malformed scenario data — matches the #69 surge-chain
   precedent).

2. **Act advance — `PlayerAction::AdvanceAct { investigator }`** (new, prototype).
   Validate-first: pooled clues across investigators ≥ current act's
   `clue_threshold`; reject otherwise. Then spend clues (decrement
   `Investigator.clues`; clues return to the pool, not tracked on the act),
   and: current act has `resolution: Some(r)` → `request_resolution`; else emit
   `ActAdvanced` + `advance_act` (same `unreachable!()` invariant).

3. **Elimination step 6** — at the `check_all_defeated` site that emits
   `AllInvestigatorsDefeated`, also `request_resolution(state, Resolution::Lost
   { reason: "no resolution was reached".into() })`. This makes #137's
   no-active-investigator "park" branch unreachable (the `TODO(#144)` /
   `TODO(#73)` the phase doc flagged) — remove the park's TODO accordingly.

## New events

- `Event::AgendaAdvanced { from: usize }` (cursor before advance)
- `Event::ActAdvanced { from: usize }`

Both bare-ish; `ScenarioResolved` already exists and is reused for the terminal
outcome. No new `ScenarioWon` / `ScenarioLost` pair (the phase-doc wording
predates #74's `ScenarioResolved`).

## `ScenarioModule` rework

```rust
pub struct ScenarioModule {
    pub setup: fn() -> GameState,
    pub apply_resolution: fn(&Resolution, &mut GameState, &mut Vec<Event>),
    // detect_resolution: REMOVED
}
```

Synthetic fixture: `setup` seeds the 2-agenda / 2-act decks (final entries carry
`resolution`); `detect_resolution` deleted; `apply_resolution` stays a no-op.

## Synthetic fixture decks

- Agenda 0: `doom_threshold: 2`, `resolution: None`.
- Agenda 1: `doom_threshold: 2`, `resolution: Some(Lost { reason: "agenda".into() })`.
- Act 0: `clue_threshold: 2`, `resolution: None`.
- Act 1: `clue_threshold: 2`, `resolution: Some(Won { id: "demo".into() })`.

## Test plan

Engine unit tests (`game-core`, mock state — no registry needed for the latch
set; the emit/apply path is covered in integration):

- doom accumulation: two Mythos entries → `agenda_doom` 1 then 2.
- threshold advance: doom meets agenda 0's threshold → `AgendaAdvanced { from: 0 }`,
  doom reset, cursor at 1.
- terminal agenda: doom meets agenda 1's threshold → `request_resolution(Lost)`,
  no `AgendaAdvanced`, no panic.
- `advance_agenda` past last → `unreachable!()` (covered structurally; a test
  on a deck whose final agenda lacks a resolution would panic — assert via the
  malformed-data path only if cheap, else rely on the invariant).
- `AdvanceAct`: insufficient clues → `Rejected`; sufficient → clues spent +
  `ActAdvanced` / terminal act → resolution.
- elimination step 6: last investigator defeated → `AllInvestigatorsDefeated`
  + resolution latch set to `Lost`.

Integration tests (`crates/scenarios/tests/`, real registry install):

- rewrite `synthetic_resolution.rs` for the push model.
- doom-to-Lost playthrough: cycle rounds, accumulate doom, assert `ScenarioResolved
  { Lost }` fires once.
- clues-to-Won playthrough: discover + spend clues via `AdvanceAct`, assert
  `ScenarioResolved { Won }`.
- replay determinism: action-log replay reproduces the resolved state.

## Out of scope

- Act/agenda *card effects* (per-scenario content; Phase 7).
- Per-investigator (`󲆃`) act/agenda thresholds; `Objective –` overrides.
- Summing doom on non-agenda cards (no consumer; `TODO`).
- Locking out further player actions after a resolution fires (the latch
  prevents re-firing; gating play is a separate concern, deferred).
- `AdvanceAct`'s final form (prototype; Phase 7 consumers drive it).
- **Player-chosen clue allocation on a surplus** — when the group holds more
  clues than the act's threshold, letting players choose who contributes how
  many (via `AwaitingInput`). This PR uses the deterministic default above.
  **File a follow-up issue** (Phase 8 territory, alongside the other
  multiplayer interactive-choice deferrals like #151) — only reachable
  multiplayer, and a fixed order is outcome-equivalent single-player.

## Affected files

- `crates/game-core/src/state/game_state.rs` — new fields + `Agenda` / `Act`.
- `crates/game-core/src/scenario.rs` — drop `detect_resolution`.
- `crates/game-core/src/event.rs` — `AgendaAdvanced` / `ActAdvanced`.
- `crates/game-core/src/action.rs` — `PlayerAction::AdvanceAct`.
- `crates/game-core/src/engine/dispatch.rs` — fill 1.2/1.3 stubs, `advance_agenda`
  / `advance_act` / `request_resolution`, `AdvanceAct` handler, elimination-step-6
  wiring, drop #137 park TODO.
- `crates/game-core/src/engine/mod.rs` — rework `fire_scenario_resolution` into
  the before/after latch hook.
- `crates/scenarios/src/test_fixtures/synthetic.rs` — decks + module rework.
- `crates/scenarios/tests/synthetic_resolution.rs` — push-model rewrite.
