# Act/Agenda cards + turn tracker + collapsible log — design

**Date:** 2026-06-30
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); sixth slice of the card-rendering / layout rework

## Goal

Three related layout changes:
1. Render the current **Act and Agenda as cards** at the top of the board
   (above the map), replacing the terse phase-bar act/agenda text.
2. Move the **phase indicator into a right-hand turn tracker** that outlines the
   round's phases, their Rules-Reference sub-steps, and the structural player
   windows, highlighting the current phase.
3. Make the left **event log collapsible**.

Together these turn the page into a three-column layout. Display-only.

## Scope decisions

- **Display-only.** No click handlers / engine wiring.
- **Three-column layout:** left = event log (collapsible), center = act/agenda
  cards → board (map stacked above the investigators panel) → sticky action bar,
  right = turn tracker.
- **Turn tracker is a static RR-sourced cheat-sheet.** It lists the round's four
  phases, each with its RR sub-steps and the structural player/reaction windows,
  and highlights only the **current phase** (`game.phase`) — the engine exposes
  the coarse phase, not sub-step position. The outline content is authored from
  the Rules Reference and cited in the plan / component doc-comment — **never from
  memory** (per the repo's rules-verification rule).
- **Act has no running clue counter.** The current phase-bar's "clues 0/N"
  hardcodes the 0; clues live on locations/investigators, not on the act. The act
  card shows `clues to advance: {clue_threshold}` (no fake progress). The agenda
  card shows real progress `doom {agenda_doom}/{doom_threshold}`.
- **In scope:** the three layout pieces. The interactivity pass (clickable cards,
  retiring the action bar) is a separate, later effort.

## Architecture

### New component `crates/web/src/act_agenda.rs` — `ActAgendaView`

Reads the store. Renders the current Act (`act_deck[act_index]`) and Agenda
(`agenda_deck[agenda_index]`, with `agenda_doom`) as two cards:

- Name + ability text from the corpus by `code` (`metadata_for`), text via
  `crate::card::parse_card_text` + `crate::card::render_segments`.
- Act: a `clues to advance: {clue_threshold}` line. Agenda:
  `doom {agenda_doom}/{doom_threshold}`.
- Rendered only when the respective deck is non-empty (fixtures may omit them).
- Reuses the card CSS vocabulary; act/agenda get distinct accents.

Rendered at the top of `BoardView` (above the map), after the resolution banner.

### New component `crates/web/src/turn_tracker.rs` — `TurnTrackerView`

Reads the store (`game.phase`, `game.round`). Renders:

- `Round {n}`.
- A static outline: the four phases (Mythos, Investigation, Enemy, Upkeep), each
  with its RR sub-steps and structural player windows, as a nested list. The
  content is a module-level constant authored from the Rules Reference.
- The phase whose label matches `game.phase` carries a `current` CSS class; the
  others do not.

Lives in the right column.

### `EventLogView` collapse (`crates/web/src/event_log.rs`)

A client-side `collapsed` `RwSignal` (default expanded) + a toggle button. When
collapsed, the log body is hidden and a compact "show log" affordance remains.
No engine involvement; the auto-scroll effect stays.

### `BoardView` (`crates/web/src/board.rs`)

The `phase_bar` is dismantled: act/agenda move to `ActAgendaView` (top), phase +
round move to `TurnTrackerView` (right). `BoardView` renders the resolution
banner, then `ActAgendaView`, then `board-main` (map + investigators). The
`phase_bar` fn and its `.phase-bar` usage are removed.

### `app.rs` + `style.css`

`app.rs` wires the three columns: `<EventLogView/>` (left) · `main-column`
(BoardView + action bar) · `<TurnTrackerView/>` (right). `style.css` adds the
three-column layout, the tracker styling (+ `current`-phase highlight), the
act/agenda accents, and the collapsed-log state.

## Field / content summary

| Piece | Source | Rendering |
|---|---|---|
| Act card | `act_deck[act_index]` + registry | name, text, `clues to advance: {clue_threshold}` |
| Agenda card | `agenda_deck[agenda_index]` + `agenda_doom` + registry | name, text, `doom {agenda_doom}/{doom_threshold}` |
| Turn tracker | static RR outline + `game.phase`/`game.round` | `Round n`; 4 phases w/ sub-steps + player windows; current phase highlighted |
| Event log | client `collapsed` signal | toggle hides/shows the log body |

## Testing

- **`ActAgendaView`** — own-binary headless test (`crates/web/tests/act_agenda.rs`,
  real `cards::REGISTRY`, mounts `ActAgendaView` directly; registry install is
  first-wins per process, so it must be its own binary): a state with a real Act
  and Agenda code renders both names, their ability text, `clues to advance: N`,
  and `doom d/N`. Act/agenda codes verified against the snapshot at plan time.
- **`TurnTrackerView`** — headless (synthetic registry fine): all four phase
  labels and their sub-steps render; with `game.phase = Investigation`, the
  Investigation entry carries the `current` class and the others do not.
- **`EventLogView`** — headless click test: the log body is visible initially;
  after clicking the toggle it is hidden (and a show affordance is present);
  clicking again restores it.
- **Smoke** — the three columns (`.event-log`, `.main-column`, the tracker) all
  mount.

Stats/text read from the corpus / `GameState` — never hand-typed. The turn-tracker
outline is authored from the Rules Reference and cited in the plan.

## What "done" looks like (this slice)

- Act + Agenda render as cards at the top of the board; the terse phase-bar
  act/agenda text is gone.
- A right-hand turn tracker outlines the round (phases + RR sub-steps + player
  windows), highlighting the current phase, with the round number.
- The left event log can be collapsed/expanded.
- Hand/in-play/enemy/location/treachery cards unchanged.
- Native + headless tests pass; the full 7-job CI gauntlet is green.

## Out of scope (later slices)

- Clickable act/agenda or any interactivity (the interactivity pass).
- Live sub-step highlighting in the tracker (engine exposes only the coarse phase).
- Per-investigator turn / action-point tracking in the tracker.
- The ArkhamDB icon font (still deferred).
