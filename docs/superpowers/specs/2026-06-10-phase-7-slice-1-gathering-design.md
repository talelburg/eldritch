# Phase 7 — Slice 1: Roland through The Gathering: design spec

**Date:** 2026-06-10
**Status:** Design approved; issues to be filed.
**Milestone:** `phase-7-the-gathering`

## Goal

A solo human picks **Roland Banks**, plays **The Gathering** at **Standard**
difficulty in the browser, and reaches a real **Won** or **Lost**
resolution. Reconnecting mid-scenario restores the board (Phase-6
machinery already provides this).

This is the first **vertical slice** of Phase 7. Breadth — the other four
investigators, the other difficulties, and solo-with-two UX — comes in
later Phase-7 slices. The slice exists to de-risk the scenario +
registry + trigger-dispatch integration on a small card surface before
committing to the full deck-card corpus.

## Strategy: vertical slice, not layer-by-layer

Get **one** investigator playable end-to-end before going wide. The
engine framework is already substantial (combat, encounter draws,
hunters, the enemy phase, skill tests, act/agenda advancement,
elimination, reaction windows all exist), so Phase 7 is far more a
**content + integration** phase than an engine-building one. The slice
surfaces exactly the engine gaps real content needs — chiefly the
Forced/reaction trigger dispatcher — and builds them once, properly.

## Fidelity bar

**Everything on the win/lose path is rules-faithful.** No silent
approximation: anything we don't fully model is **out of scope and
explicitly deferred**, never faked.

**In scope (full fidelity):**

- The Gathering scenario: 5 starting locations (Study, Hallway, Attic,
  Cellar, Parlor) + connections; the 3-act / 3-agenda decks; the real
  encounter sets; Standard chaos bag.
- Forced location effects (Attic `01113`: 1 horror on enter; Cellar
  `01114`: 1 damage on enter), victory points (Attic/Cellar).
- Agenda forced effects: `01107` Ghoul-movement-toward-Parlor (at end of
  the **enemy phase**) and doom-per-Ghoul-in-Hallway/Parlor (at end of
  the **round**); the doom clock generally.
- Act objectives: the clue-spend gate (`01109` Act 2) and "if the Ghoul
  Priest is defeated, advance" (`01110` Act 3).
- Scenario symbol-token effects on reference card `01104`: skull
  `-X` (X = Ghoul enemies at your location), cultist `-1` + 1 horror on
  failure, tablet `-2` + 1 damage if a Ghoul is at your location.
- Roland's real Core starter deck + his signature asset (Roland's .45
  Automatic), his weakness (Cover Up), and his reaction ability.
- The encounter cards a solo-Roland Standard run can actually draw
  (Ghouls, Rats, Striking Fear, Ancient Evils, Chilling Cold sets).

**Out of scope — explicit follow-up issues, not faked:**

- Lita Chantler's parley/take-control and the Parlor (`01115`) **Resign**
  action — both genuinely optional, off the win/lose path.
- The other four investigators; Easy/Hard/Expert chaos bags;
  solo-with-two UX — later Phase-7 slices.

> **Card-text provenance:** every card quoted above was read from
> `data/arkhamdb-snapshot/pack/core/core_encounter.json` /
> `core.json` on 2026-06-10. The exact encounter-set card list and
> Roland's deck list are enumerated against the snapshot at plan time,
> not in this spec.

## Architecture

### Principle: every effect is owned by the card it's printed on

The engine already routes enemy/treachery behavior through the
`CardRegistry` by `CardCode` (`encounter.rs` calls `abilities_for`).
Locations, acts, and agendas are encounter cards too (`01111`, `01110`,
`01107`, …), so they get the **same** treatment: their behavior comes
from `abilities_for(code)`, not from a monolithic per-scenario switch.
Attic's "after you enter: 1 horror" is a Forced ability on `01113`;
agenda `01107`'s Ghoul-movement is a Forced ability on `01107`.

Consequence: the `ScenarioModule` collapses to **`setup` +
`apply_resolution` + `reference_card`**. No per-scenario trigger switch.
This is more local and generalizes to every future scenario.

Two orthogonal axes, kept separate:

- **Ownership** — always the card the effect is printed on.
- **Expression** — DSL `Effect` where it fits (generic/simple);
  hand-written Rust where the DSL lacks the primitive (complex /
  scenario-specific). Per the existing rule: *don't add DSL primitives
  until 2+ cards want the pattern.*

### The trigger spine (the load-bearing new machinery)

**1. OnEvent/Forced dispatcher (kernel).** The DSL already defines
`Trigger::OnEvent { pattern, timing }` and `EventPattern` / `Timing`,
but nothing *fires* OnEvent abilities yet. We build one dispatcher: after
each event is emitted into the buffer, scan active cards' `OnEvent`
abilities whose `pattern` matches and run their `Effect` via the existing
evaluator, under the validate-first contract.

- **Scan set:** in-play assets + encounter cards + the current
  location(s) + the current act + the current agenda.
- This single piece fires *all* Forced/reaction effects uniformly —
  treachery/enemy forced effects, location/act/agenda forced effects,
  **and** Roland's reaction signature.
- Highest-risk engine piece in the slice; isolate with focused tests.

**2. Kernel trigger windows.** For a card's Forced ability to listen,
the engine must emit the events: `LocationEntered`, `PhaseEnded(phase)`,
`RoundEnded`, `EnemyDefeated`, etc. The *effect* lives on the card; the
*window* that fires it is a kernel concern. Audit which events exist
today; add the gaps.

**3. Acts/agendas carry `CardCode`.** Locations already have `code`; Act
and Agenda gain one so the dispatcher can resolve their abilities through
the registry. The thin structs keep only mechanical **state** (clue/doom
thresholds, resolution latch, shroud, clues, connections); **behavior**
comes from the registry.

### Symbol-token resolution — through the scenario, owned by the reference card

Each scenario has exactly one reference card, whose chaos symbols are
printed on it. `ScenarioModule` gains a `reference_card: CardCode` field
(plain data). The skill-test resolver already has `scenario_id →
module`, so on a symbol token it asks the module for its reference card,
then evaluates *that card's* symbol ability against current state.
Ownership stays on the card (`01104` owns skull/cultist/tablet); access
flows through the scenario.

The "skull = `-(Ghouls at your location)`" board-count logic is a Rust
impl on `01104` for now (only the reference card wants it). A second
scenario with board-count-driven symbols is the trigger to add a DSL
dynamic-count primitive.

### Forced-on-enter as a DSL variant

"After you enter this location, take N damage/horror" clears the "2+
cards plus known future demand" bar (Attic, Cellar, plus many in future
scenarios), so it's promoted to **pure DSL**, not Rust:

- A Forced trigger (`Trigger::OnEvent` with an "entered the location this
  ability is printed on" pattern, `timing: After`); the dispatcher binds
  *you* = the entering investigator and *this location* = the card's own
  location.
- Two new `Effect` primitives — `DealDamage` and `DealHorror` to a target
  investigator — needed broadly anyway (treacheries, combat).

So Attic/Cellar become declarative data.

### Agenda 01107 — separate forced movement, not a pathfinding override

`01107` has **two** forced abilities at **two** windows:

- *"At the end of the enemy phase: each unengaged Ghoul moves 1 toward
  the Parlor"* → fires on `PhaseEnded(Enemy)`.
- *"At the end of the round: place 1 doom per Ghoul in the Hallway or
  Parlor"* → fires on `RoundEnded`.

The movement is **not** a modification of the enemy-phase hunter step
(3.2). It is a discrete forced movement that runs *after* the enemy
phase's normal steps. Mechanically: the agenda's Rust ability iterates
unengaged Ghoul-trait enemies and, for each, calls the existing shared
primitive `engine::pathfinding::shortest_first_steps(state, ghoul_loc,
PARLOR)` and moves it one step — an independent caller of the same BFS
helper the hunter code uses, with a fixed destination, at a different
window. The hunter step is untouched.

The data confirms this is simpler than it looks: only the Ghoul Priest
(`01116`) is a Hunter; Flesh-Eater, Icy Ghoul, Ghoul Minion, and
Ravenous Ghoul have no Hunter keyword and never move during 3.2 — the
agenda is their *only* mover, so there is nothing to override. The Priest
can move twice in one enemy phase (hunting the highest-combat
investigator during 3.2, then toward the Parlor at phase-end) — two
moves, two targets, both callers of the same primitive. Rules-correct.

It stays a **Rust-backed ability on `01107`** (trait-filtered mass
movement + conditional doom is too scenario-specific to DSL-ify for one
card).

### Investigator seating

`ScenarioModule.setup: fn() -> GameState` takes no arguments, so there is
no channel for "which investigator." Split responsibilities: the scenario
module builds the **world** (locations, decks, bag); a separate
**roster/seating step** takes host-supplied investigator selection, loads
each chosen investigator's deck, and seats them. `StartScenario` carries
the selection through the protocol (a protocol change). This keeps
deck-loading scenario-agnostic and reusable across scenarios rather than
overloading `setup` with a roster parameter. `StartScenario` already
shuffles each investigator's `deck` and deals the opening hand, so seating
must populate `deck` before that step runs.

## Decomposition & build order

**Group A — Engine spine** (unblocks everything; build and prove first)

1. DSL primitives: `Effect::DealDamage` / `Effect::DealHorror`;
   forced-on-enter trigger pattern. Pure `card-dsl`.
2. Kernel trigger windows: emit `LocationEntered`, `PhaseEnded(phase)`,
   `RoundEnded`, `EnemyDefeated`. Audit + fill gaps.
3. OnEvent/Forced dispatcher (scan set above; validate-first; focused
   tests incl. the Roland-reaction case). **Riskiest piece.**
4. Act/Agenda carry `CardCode`.

**Group B — Scenario plumbing**

5. `ScenarioModule += reference_card: CardCode`; symbol resolution routes
   resolver → module → reference card ability.
6. Roster/seating step; `StartScenario` carries investigator selection
   (protocol change).
7. Registry-swap foundation (D5): server installs `cards::REGISTRY` + the
   real scenario registry instead of the synthetic set. (Synthetic stays
   for existing per-process tests.)

**Group C — Content** (largest; ordered Act 1 → full run at plan time)

8. The Gathering scenario cards: 5 locations (forced-on-enter as DSL on
   Attic/Cellar), 3 acts (objectives incl. Ghoul-Priest-defeated on
   `01110`), 3 agendas (`01107` Rust movement@`PhaseEnded(Enemy)` +
   doom@`RoundEnded`), reference card `01104` (symbols), reachable
   enemies (Ghoul Priest, Flesh-Eater, Icy Ghoul, Ghoul Minion, Ravenous
   Ghoul) and treacheries (Ancient Evils / Striking Fear / Chilling Cold).
9. The Gathering `setup()`: locations + connections, act/agenda decks,
   encounter deck from the real sets, Standard chaos bag.
10. Roland + his reaction ability + Roland's .45 Automatic + Cover Up.
11. Roland's starter-deck player cards not yet implemented (enumerate
    against the corpus at plan time).

**Group D — Integration & web**

12. Web picker: minimal investigator + scenario selection (Roland + The
    Gathering) wired into `StartScenario`; resolution surfacing already
    exists.
13. End-to-end gate: integration test driving a solo-Roland Standard run
    to **both** a Won and a Lost resolution, plus the browser demo.

Riskiest pieces to isolate and test hard: the OnEvent dispatcher (#3) and
agenda `01107` (#8). Everything in C/D is mostly mechanical once A/B
exist.

## Open questions (settle at plan time; not blockers)

- **Equidistant-tie handling** in agenda `01107`'s movement: a
  deterministic tiebreak (e.g., lowest `LocationId` among equally-short
  first steps) for Slice 1, vs a player `AwaitingInput`. Lean
  deterministic now, `AwaitingInput` when multiplayer lands — any
  equidistant step is rules-legal, so this is an agency choice, not an
  outcome approximation.
- **Exact encounter-set card list and Roland deck list** — enumerated
  against the snapshot at plan time.

## What "Slice 1 done" looks like

A solo human, in the browser, picks Roland, sets up The Gathering at
Standard, and plays to a real Won or Lost resolution. An integration test
drives both outcomes headlessly. The other investigators, difficulties,
and solo-with-two UX remain for later Phase-7 slices.
