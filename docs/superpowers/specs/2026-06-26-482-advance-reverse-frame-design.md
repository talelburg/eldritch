# #482 — Resumable act/agenda advance with a gated acknowledge

**Issue:** [#482](https://github.com/talelburg/eldritch/issues/482) — Advancing
an agenda/act with a *suspending* on-advance Forced effect strands the choice
(the Mythos doom cascade panics on 01105's interactive `ChooseOne`). Labels:
`bug`, `engine`, `p1-next`.

## Problem (root cause, proven)

When an act/agenda advances, its leaving card's **Forced on-advance reverse**
fires via `emit_event`. Per the engine's forced-trigger contract,
`fire_forced_triggers` **pushes the reverse's effect frame for the `drive` loop
to own and returns `Done` without driving it** — and "callers with post-forced
work must arm a resumption frame before emitting." `advance_agenda` /
`advance_act` instead do synchronous post-forced work (bump `index`), and the
Mythos cascade (`mythos_phase`) then pushes the **1.4 `EncounterDraw` frame on
top of** the not-yet-driven reverse.

For The Gathering's agenda **01105** ("What's Going On?!"), the reverse is the
lead's interactive `ChooseOne` (each investigator random-discards / the lead
takes 2 horror), which **suspends**. So the Mythos cascade strands the
`ChooseOne` mid-stack:

```
[0] MythosPhase { resume: AfterDraws }
[1] Effect(Leaf { ChooseOne([01105:random-discard-each, Deal Horror 2]) })   ← STRANDED
[2] EncounterDraw { remaining: [inv1] }
```

The encounter-draw prompt becomes the live outcome; the agenda choice is never
presented; and a later window-close `anchor_on_child_pop` finds the `Effect`
frame where it expects a phase anchor → panic (release) / `debug_assert`
(debug). Pre-existing; #476 only flipped the downstream symptom from a silent
strand to a loud panic. The act path (`advance_act_action`, `round_end_advance`)
does **not** strand today — its callers push no frame on top of the reverse —
but it carries the same latent synchronous-`Done` assumption.

## Goal

1. Fix the bug: an act/agenda advance whose reverse suspends (interactive)
   resolves cleanly, and the Mythos 1.4 draws wait for it.
2. Add a **gated acknowledge**: when an act/agenda advances, surface a `Confirm`
   to the player **before** the reverse resolves ("Agenda 1 advanced —
   acknowledge"), reusing #478's `interactive_acknowledge` flag. (A direct
   instance of #466; the future card-text immersion is a follow-up.)

Both are delivered by making the advance a small **resumable sub-process**, so
the acknowledge and the suspending reverse compose uniformly across all three
advance call sites (Mythos agenda, AdvanceAct action, round-end act).

## Design

### 1. `Continuation::AdvanceReverse` — the unifying frame

A new continuation frame representing "an act/agenda is advancing," with a step
cursor (mirrors the `SkillTest` / `EncounterDraw` frame idiom):

```text
AdvanceReverse {
    deck:         AdvanceDeck (Act | Agenda),
    from:         usize,            // index of the leaving card
    leaving_code: CardCode,
    step:         AdvanceStep,      // AwaitAck | FireReverse | Finalize
}
```

Driven by the `drive` loop (a new arm) and resumed by `resolve_input` (a new
arm), through its steps:

- **`AwaitAck`** — push the observable `Event::ActAdvanced` / `AgendaAdvanced`
  (today emitted inline by `advance_*`). If `interactive_acknowledge` is set,
  pre-advance the cursor to `FireReverse` and return
  `AwaitingInput { InputRequest::confirm(<descriptive prompt>), … }`. If the
  flag is off, advance to `FireReverse` and continue (no pause).
- **`FireReverse`** — pre-advance the cursor to `Finalize`, then fire the
  leaving card's Forced reverse via `emit_event(TimingEvent::{Act,Agenda}Advanced
  { code: leaving_code })`. That queues the reverse's frames for the drive loop
  (Vicious-Blow-style non-suspending natives, or 01105's suspending `ChooseOne`).
  Return its outcome (the loop drives the reverse; a suspend round-trips).
- **`Finalize`** — the reverse has fully resolved. Do the bookkeeping
  (`index += 1`; for agendas `agenda_doom = 0`), assert non-terminal (the
  existing past-the-end `unreachable!`), and pop the frame. The caller's frame
  beneath is re-exposed by the loop.

This composes the two suspension points (acknowledge, then reverse) uniformly,
and — bonus — bookkeeping now runs **after** the reverse resolves (the correct
RR order: "flip the card, follow the reverse, then the next card becomes
current"), retiring the synchronous-`index`-bump quirk and the `debug_assert(…
did not resolve to Done … 2+ needs #213)`.

`advance_agenda` / `advance_act` shrink to: capture `from` + `leaving_code`,
push an `AdvanceReverse` frame, return `Done`. `check_doom_threshold` /
`advance_act_action` / `round_end_advance` push the frame and let the loop drive
it; their post-advance work moves onto the appropriate resumption (the Mythos
anchor for draws; the open-turn / round-end frames re-expose naturally).

### 2. Gated acknowledge — no client change

The acknowledge reuses the **existing** `GameState.interactive_acknowledge` flag
(#478): the server already sets it on for human play; tests and headless
consumers leave it off → no acknowledge pause and no test churn. The `Confirm`
prompt string is descriptive (e.g. `"Agenda 1 advanced — acknowledge."`, using
`from + 1`), so the **existing `Confirm` button** in `crates/web/src/input.rs`
renders it — **no web changes**. The future card-text panel (the client already
receives the `…Advanced` event, so it can render a rich panel like #478's
`SkillTestResultView`) is explicitly out of scope here.

### 3. Mythos cascade defers the 1.4 draws (the bug fix proper)

Add `MythosResume::Draws` (between `Entry` and `AfterDraws`):

- `mythos_phase` (the opening, run from `advance_phase_entry`): push the
  `MythosPhase` anchor at **`Draws`** (not `AfterDraws`), `place_doom_on_agenda`
  (1.2), `check_doom_threshold` (1.3 — pushes an `AdvanceReverse` frame if
  advancing), then **return `Done`** — it no longer pushes `EncounterDraw`
  inline.
- A new `anchor_on_child_pop` arm for `MythosPhase{Draws}`: set the anchor's
  resume to `AfterDraws`, then run the **relocated** 1.4-draw logic (the
  no-Active-investigators inline-window path, or push `EncounterDraw` +
  `prompt_encounter_draw`); return its outcome.

Because the `AdvanceReverse` frame (if any) sits **above** the `MythosPhase`
anchor, the loop drives it to completion — including the acknowledge and the
`ChooseOne` — before re-exposing the anchor at `Draws` to run the draws. Works
uniformly whether or not the agenda advanced and whether or not the reverse
suspended.

### 4. Act path

`advance_act_action` and `round_end_advance` already do `spend_clues →
advance_act → return Done` with no frame pushed on top; with `advance_act` now
pushing an `AdvanceReverse` frame, the loop drives it (acknowledge → reverse →
finalize) and re-exposes the open turn / round-end coordinator beneath. So act
advances gain the acknowledge + suspending-reverse safety with no special-casing.

## Testing

- **Regression (the bug)** — real registries, The Gathering + Roland, agenda
  doom one below threshold, lead holding a card (so the random-discard branch is
  legal and the `ChooseOne` genuinely suspends), `EndTurn` → Mythos:
  - flag **off**: the agenda advances and the `ChooseOne` is the live prompt;
    resolving it proceeds into the 1.4 encounter draw → the open turn. No panic,
    no stranded frame.
  - flag **on**: an acknowledge `Confirm` precedes the `ChooseOne`; `Confirm`
    then the choice resolves identically.
- **Act-path proof** — a synthetic *interactive* act reverse (a test act card
  whose reverse is a `ChooseOne`) advanced via `advance_act_action` presents the
  choice and resumes the open turn cleanly.
- **Acknowledge gating** — flag on emits the advance `Confirm`; flag off does
  not — for both act and agenda advances.
- **Blast radius** — existing advance tests (e.g. `agenda_reverses.rs`,
  `the_gathering.rs`, `act_advancement.rs`, the Mythos suite) now drive through
  the `AdvanceReverse` frame. Flag-off adds one drive step with no acknowledge;
  scripted tests that fire the reverse directly via `fire_forced_on_agenda_advance`
  may need to account for the frame. Quantified during planning by running the
  full suite.

## Out of scope

- The card-text immersion panel (client-side, future) — the engine emits a
  descriptive `Confirm`; the rich panel is a follow-up on the `…Advanced` event.
- Deferring/queuing reaction windows opened *by* a reverse beyond what the
  existing forced/effect machinery already handles.
- Terminal advances (a card carrying a resolution point) — those go through
  `request_resolution` (the scenario-end latch), not `AdvanceReverse`; unchanged.
