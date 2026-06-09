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
surfaces exactly the engine gaps real content needs — chiefly extending
the existing reaction-window machinery to forced effects and scenario-card
sources — and builds them once, properly.

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

### The trigger spine — *extend* the existing reaction-window machinery

**Reality check (verified 2026-06-10):** the OnEvent firing pipeline is
**not** greenfield. `engine/dispatch/reaction_windows.rs` already has a
full queue/scan/fire pipeline: `scan_pending_triggers` walks in-play
cards for `Trigger::OnEvent` abilities, matches them against a
`WindowKind`, honors per-instance usage limits, and fires them;
windows already open at `AfterEnemyDefeated` (exactly Roland's reaction
window) and several `PlayerWindow(PhaseStep::…)` points. `PendingTrigger`
even carries a `forced: bool` field, hardwired `false` today with a
comment that the DSL had no forced primitive yet. So this slice
**extends** that machinery rather than building a dispatcher from
scratch. Four concrete extensions:

**1. Widen the scan set.** `scan_pending_triggers` iterates only
`inv.cards_in_play`. Extend it to also scan the current **location(s),
act, and agenda** for `Trigger::OnEvent` abilities, resolved through the
registry by `CardCode` (same `abilities_for` call). This is what lets
location/act/agenda forced effects participate.

**2. Forced (mandatory) firing.** The `forced` field exists but is always
`false`. Forced abilities (Attic horror, agenda `01107` movement) must
fire **mandatorily** — no player "may" window, no `AwaitingInput`
suspension. This is a real semantic addition: reaction windows suspend
for a player choice; Forced effects auto-resolve in place. Recommended
shape: Forced abilities reuse the scan pipeline but take a direct
auto-fire path (evaluate the effect immediately, in resolution order)
instead of opening a suspending window. A `Trigger::OnEvent` ability is
Forced vs. reaction based on whether the printed text is "Forced —" vs.
"[reaction] … you may"; the card author encodes which.

**3. Open windows / fire forced triggers at new points.** Today windows
open at defeat + specific phase steps. Add trigger points after
`InvestigatorMoved` (location entry), at `PhaseEnded(Enemy)` (agenda
movement), and at end of round (agenda doom). These reuse the existing
events below — no new events needed for the slice.

**4. Add the `EventPattern` / `WindowKind` variants** the new trigger
points need (entered-this-location, phase-ended, round-end), matched by
the existing `trigger_matches`.

**Events already exist.** Audit confirms `InvestigatorMoved`,
`PhaseEnded { phase }`, `EnemyDefeated`, `DamageTaken`, `HorrorTaken`,
`ActAdvanced`, `AgendaAdvanced` are all present. "End of round" is the
`PhaseEnded(Upkeep)` window. So the slice needs **no new `Event`
variants** — only new `EventPattern`/`WindowKind` variants and the
trigger-point wiring. Roland's reaction likely already fires through the
existing `AfterEnemyDefeated` window, making it largely a content task.

**Risk note:** lower than greenfield, but the Forced-vs-reaction split
(extension 2) and agenda `01107` are the pieces to isolate and test hard.

**Acts/agendas carry `CardCode` (prerequisite for extension 1).**
Locations already have `code`; Act and Agenda gain one so the widened
scan can resolve their abilities through the registry. The thin structs
keep only mechanical **state** (clue/doom thresholds, resolution latch,
shroud, clues, connections); **behavior** comes from the registry.

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

**Group A — Engine spine** (unblocks everything; build and prove first).
*Extends the existing `reaction_windows.rs` machinery — not greenfield.*

1. DSL primitives: `Effect::DealDamage` / `Effect::DealHorror`;
   forced-on-enter `EventPattern`. Pure `card-dsl`.
2. Act/Agenda carry `CardCode` (prerequisite for the widened scan).
3. Widen `scan_pending_triggers` to also scan current location/act/agenda
   via the registry.
4. Forced (mandatory) auto-fire path (wire the existing `forced` field;
   no window suspension). **Riskiest piece** alongside agenda `01107`.
5. New `EventPattern` / `WindowKind` variants + trigger-point wiring at
   `InvestigatorMoved`, `PhaseEnded(Enemy)`, and round end
   (`PhaseEnded(Upkeep)`). No new `Event` variants needed.

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

Riskiest pieces to isolate and test hard: the Forced auto-fire path
(Group A #4) and agenda `01107` (Group C #8). Everything in C/D is mostly
mechanical once A/B exist.

## Open questions (settle at plan time; not blockers)

- **Equidistant-tie handling** in agenda `01107`'s movement: a
  deterministic tiebreak (e.g., lowest `LocationId` among equally-short
  first steps) for Slice 1, vs a player `AwaitingInput`. Lean
  deterministic now, `AwaitingInput` when multiplayer lands — any
  equidistant step is rules-legal, so this is an agency choice, not an
  outcome approximation.
- **Exact encounter-set card list and Roland deck list** — enumerated
  against the snapshot at plan time.
- **Trigger ordering & resolution UX (mostly a later Phase-7 slice).**
  Rules Reference p.17: *"If two or more forced abilities (including
  delayed effects) would resolve at the same time, the lead investigator
  determines the order in which the abilities resolve."* The target model
  (confirmed with the user):
  - The player chooses order **even in solo** (they are the lead).
  - **Iterative, not order-the-whole-list**: present the pending triggers
    → player names which resolves *first* → resolve it → re-present the
    remaining (minus that one) → repeat. More correct than an upfront
    total order, since resolving one trigger mutates state and can
    add/remove others.
  - Uniform across **forced and optional**, with **skip available only
    when every remaining trigger is optional** (forced are mandatory).
  - There is also a future click-to-resolve argument even for a *lone*
    forced trigger (agency / feel-in-charge) — out of scope now.

  This full pipeline lands with the **`emit_event` event-window
  restructure** — a later Phase-7 slice, not Slice 1. Slice 1's A2 ships
  only the **single-trigger path**: `fire_forced_triggers` resolves a lone
  forced trigger immediately and **rejects loudly (TODO)** if 2+ are ever
  simultaneously pending — no silently-chosen order. The slice's content
  never produces 2+ simultaneous forced triggers, so that guard is
  unreachable in practice; it exists to refuse rather than fake.

- **`emit_event` dispatch unification (later Phase-7 slice).** North-star
  architecture: make event emission itself the dispatch chokepoint — an
  `emit_event(cx, event)` that consults an event-keyed trigger registry
  (folding in the #117 index) and routes forced/optional listeners,
  rather than wiring windows by hand at each emission site. Deferred from
  Slice 1 because it needs reentrancy handling, mid-emit `AwaitingInput`
  suspension, and the iterative-ordering pipeline above — none of which
  Slice 1's content exercises (YAGNI). A1/A2 are forward-compatible: the
  explicit `fire_forced_triggers` is a clean subset `emit_event` can later
  absorb. File as a Phase-7 issue (or re-scope #117 to cover it).

## What "Slice 1 done" looks like

A solo human, in the browser, picks Roland, sets up The Gathering at
Standard, and plays to a real Won or Lost resolution. An integration test
drives both outcomes headlessly. The other investigators, difficulties,
and solo-with-two UX remain for later Phase-7 slices.
