# Anchor agenda-sourced forced effects to the agenda card — design

**Date:** 2026-07-16
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration) · **Issue:** #556 · **Umbrella:** #206 · **Follows:** the forced-effect card anchor (#553, PR #554), which anchored in-play instances, the act, and locations.

## Goal

The board-interactivity pass anchors a forced/triggered option to its source card
so the card glows and offers its action on the board. The **agenda** is the last
board card left out: it renders as a static `<article>` (`act_agenda.rs:73`) and
there is no `OptionTarget::Agenda`, so an agenda-sourced forced effect (What's
Going On?! 01105's on-advance reverse) falls to `OptionTarget::Global` and resolves
only from the flat prompt bar — while the act, right beside it, glows and menus
(`ActCard`). This closes that asymmetry by mirroring the act exactly.

## What surfaces when an agenda advances (verified)

`advance_reverse.rs`: an advance runs `AwaitAck` (a `Confirm`, no options — the
flip acknowledgement, out of scope) → `FireReverse` (emits the `AgendaAdvanced`
timing event, firing the agenda's `Trigger::OnEvent(AgendaAdvanced)` forced effect)
→ `Finalize` (`agenda_index += 1`). The forced effect fires at **FireReverse, before
Finalize bumps the index**, so at the moment its interactive forced-acknowledge
("Resolve") surfaces, `state.agenda_index` still points at the advancing agenda.

Two prompts result, only one in scope:

- **The forced-acknowledge "Resolve"** (`drive_acknowledge_forced`, #466): a
  one-option pick, `CandidateSource::Board`-sourced → today `Global`. **This is what
  we anchor.** Because the index hasn't bumped, `cand.code == current_agenda` holds —
  a code-equality anchor is correct with no timing hazard (symmetric to the act).
- **The effect's own `ChooseOne`** (01105: "each investigator discards 1 random
  card" *or* "lead takes 2 horror"), surfaced separately by the evaluator. Its
  branches aren't board entities, so they stay `Global` (flat bar). Anchoring
  effect-internal choices to their source card is general evaluator machinery —
  **out of scope, tracked in #555.**

## Architecture

### Engine (`game-core`)

**`OptionTarget::Agenda`** (`engine/outcome.rs`) — mirrors `OptionTarget::Act`, the
existing `#[non_exhaustive]` enum:

```rust
    /// The current agenda.
    Agenda,
```

**`current_agenda_code(state)`** (`reaction_windows.rs`) — a one-line mirror of
`current_act_code`:

```rust
pub(super) fn current_agenda_code(state: &GameState) -> Option<CardCode> {
    state
        .agenda_deck
        .get(state.agenda_index)
        .map(|agenda| agenda.code.clone())
}
```

**`candidate_anchor` gains an agenda param + arm.** Today the `Board` arm is
`if current_act == Some(&cand.code) { Act } else { Global }`. It becomes:

```rust
pub(super) fn candidate_anchor(
    cand: &ResolutionCandidate,
    current_act: Option<&CardCode>,
    current_agenda: Option<&CardCode>,
) -> crate::engine::OptionTarget {
    match cand.source {
        // …Hand, InPlay, Location unchanged…
        CandidateSource::Board => {
            if current_act == Some(&cand.code) {
                OptionTarget::Act
            } else if current_agenda == Some(&cand.code) {
                OptionTarget::Agenda
            } else {
                OptionTarget::Global
            }
        }
    }
}
```

Both callers supply the agenda code, so the single-hit ack **and** the 2+ ordered
run (`build_resolution_options`) anchor for free:

- `drive_acknowledge_forced` (`forced_triggers.rs`): passes
  `current_agenda_code(cx.state).as_ref()` alongside the existing act code.
- `build_resolution_options` (`reaction_windows.rs`): threads a `current_agenda`
  param (computed by its caller from the same state used for `current_act`).

Anchor stays **display-only**: resume still validates only the echoed `OptionId`.

### Web (`web`)

Extract an interactive **`AgendaCard`** component mirroring `ActCard`, replacing the
inline static agenda in `act_agenda_view`. Unlike `Act`, the doom counter lives on
`GameState` (`agenda_doom`), not the `Agenda` struct — so the component takes the
doom value as a second prop:

```rust
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn AgendaCard(agenda: Agenda, doom: u8) -> impl IntoView {
    // name_and_text(&agenda.code); threshold = agenda.doom_threshold;
    // pending → options_for(&pending, OptionTarget::Agenda);
    // actionable glow + wasm-only menu_layer — byte-for-byte the ActCard shape,
    // with the "doom {doom}/{threshold}" stat line kept.
}
```

`act_agenda_view` renders `<AgendaCard agenda=ag doom=game.agenda_doom/>` in place
of the old `<article>`.

## Testing

- **Engine unit:** `candidate_anchor` — a `Board` candidate whose code is the current
  agenda → `Agenda`; still `Act` for the act code, `Global` for neither. Extend the
  existing `candidate_anchor_maps_each_source` (its calls gain the new arg).
  `drive_acknowledge_forced` — a `Board` candidate whose code is the current agenda
  yields a one-option pick anchored to `Agenda` (a new state builder with an agenda
  in the deck).
- **Integration** (`crates/cards/tests/`): drive an agenda advance (What's Going On?!
  01105) with `interactive_acknowledge` on and assert the surfaced forced-ack option
  is `OptionTarget::Agenda`. Real registry, mirrors `forced_acknowledge.rs`.
- **Web headless** (`crates/web`): an `AgendaCard` given a pending `Agenda`-anchored
  option renders `.actionable` (glows) — mirrors the existing `ActCard` glow test.
- Full 7-job gauntlet green.

## What "done" looks like

When the agenda advances (or any agenda forced effect fires) in interactive play,
the agenda card glows and offers its "Resolve" on the board, exactly like the act —
no more agenda-only trip to the flat bar. The `CandidateSource → OptionTarget`
mapping stays in one place (`candidate_anchor`); anchors stay display-only.

## Out of scope

- Effect-internal `ChooseOne` anchoring (01105's discard-vs-horror) — #555.
- The advance-flip `Confirm` (`AwaitAck`) — no options to anchor.
- `OptionTarget::Act`/existing anchors — untouched beyond the shared helper's new arg.
