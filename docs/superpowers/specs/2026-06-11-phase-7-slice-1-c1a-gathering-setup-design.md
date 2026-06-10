# Phase 7 Slice 1 — C1a: The Gathering `setup()` skeleton

**Date:** 2026-06-11
**Status:** Design approved; ready for plan.
**Milestone:** `phase-7-the-gathering`
**Issue:** #227 (`[scenario]` C1a)
**Depends on:** #248 (rename `InvestigatorTarget::Controller → You`) — land first.
**Parents:** `2026-06-11-phase-7-slice-1-group-c-decomposition-design.md`,
`2026-06-10-phase-7-slice-1-gathering-design.md`.

## Goal

Stand up the **Act-1 skeleton** of The Gathering: a real `ScenarioModule`
whose `setup()` builds the starting board, act/agenda decks, and Standard
chaos bag; plus the Attic/Cellar forced-on-enter card abilities. The scenario
becomes *structurally* present and reaches *a* resolution under test — full
rules-faithful playability is the rest of Group C.

## Scope decision: faithful Study-only start

The Gathering's Act 1 is "trapped in the Study": **only the Study is in play.**
The Hallway/Attic/Cellar/Parlor are set aside and come into play via the Act-1
*"Door on the Floor"* transition (act `01108` back: *"Put into play the
set-aside Hallway, Cellar, Attic, and Parlor. … Place each investigator in the
Hallway. Remove the Study from the game."*).

That transition is act-advancement logic → **C1b (#228)**. So `setup()` places
**only the Study**, isolated (no connections — faithful to the trapped state).
The four set-aside locations and the connection graph are **not** encoded
speculatively here; they land in C1b where they are actually put into play.
This honors the slice's fidelity bar (*never fake the win/lose path*) and keeps
C1a minimal (no dead location data).

Consequence: Attic/Cellar are not *reachable in live play* in C1a. Their card
abilities are still delivered and tested (card unit tests + the existing
`fire_forced_on_enter` integration path); live entry arrives with C1b.

## Card-text provenance

All card text below was read from
`data/arkhamdb-snapshot/pack/core/core_encounter.json` on 2026-06-11:

- **Study `01111`** — location, shroud 2, clues 2.
- **Hallway `01112`** — location, shroud 1, clues 0. *(C1b)*
- **Attic `01113`** — location, shroud 1, clues 2, victory 1. Text:
  *"**Forced** – After you enter the Attic: Take 1 horror."*
- **Cellar `01114`** — location, shroud 4, clues 2, victory 1. Text:
  *"**Forced** – After you enter the Cellar: Take 1 damage."*
- **Parlor `01115`** — location, shroud 2, clues 0. Resign action + Lita
  Chantler parley — out of scope. *(C1b / later)*
- **Acts:** `01108` Trapped (clues 2), `01109` The Barrier (clues 3,
  round-end clue-spend objective → C1b), `01110` What Have You Done? (clues
  null, "If the Ghoul Priest is Defeated, advance" objective → C1b).
- **Agendas:** `01105` (doom 3), `01106` (doom 7), `01107` (doom 10; forced
  Ghoul movement + doom → C3c).

Connection geometry is **not** in the snapshot (it is printed on card art), so
it is not asserted here. Irrelevant for C1a: the Study is isolated. The
Hallway-hub graph is sourced and built in C1b.

## Components

### 1. Scenario module — `crates/scenarios/src/the_gathering.rs`

A real (non-`test_fixtures`-gated) module mirroring the synthetic fixture's
shape.

- `pub const ID: &str = "the-gathering"`.
- `reference_card: "01104"` (symbol effects evaluated in C2; C1a only carries
  the code, which B1 routing already consumes).
- `setup() -> GameState`:
  - **Study** placed in `state.locations`: `code "01111"`, name "Study",
    shroud 2, clues 2, `revealed: true`, `connections: vec![]`.
  - **`starting_location = Some(study_id)`** (see component 2).
  - **Act deck** `[01108, 01109, 01110]` as `Act { code, clue_threshold,
    resolution }`. Clue thresholds: `01108`→2, `01109`→3 (real `clues`
    values); `01110`→placeholder (real objective is "Ghoul Priest defeated",
    C1b). Terminal act `01110` carries `Resolution::Won { id: … }`.
  - **Agenda deck** `[01105, 01106, 01107]` as `Agenda { code, doom_threshold,
    resolution }`. Doom thresholds **3 / 7 / 10** — the real snapshot values
    (faithful). Terminal agenda `01107` carries `Resolution::Lost { reason: …
    }`.
  - **Standard chaos bag** — the Standard Night-of-the-Zealot token set
    (enumerated against the rules/snapshot at plan time; symbol-token numeric
    modifiers via `TokenModifiers`).
  - `scenario_id`, round 0, phase Mythos — same ready-for-`StartScenario`
    shape the synthetic fixture produces.
- `apply_resolution` — no-op stub (matches synthetic; XP/trauma is Phase 9).
- **Registry wiring** (`crates/scenarios/src/lib.rs`): make `module_for` /
  `REGISTRY` unconditional (currently `test_fixtures`-gated); add the
  `the_gathering::ID => &the_gathering::MODULE` arm. The synthetic arm stays
  `cfg(test_fixtures)`-gated.

The placeholder thresholds + terminal-resolution latch reuse the **existing**
`advance_act_action` / `check_doom_threshold` machinery — the same structural
stand-in the synthetic fixture uses. C1b replaces the act objective *types*
with faithful behavior; C7b is the rules-faithful end-to-end gate. C1a does
**not** claim faithful win/lose semantics — only structural reachability.

### 2. Starting-location placement — small engine addition

B2 roster seating (`start_scenario`, `phases.rs`) creates each investigator
with `current_location: None` and never places them; the synthetic fixture
only works because it pre-seats inside `setup()` (the empty-roster test path).
The production `setup() → StartScenario { roster }` flow leaves investigators
unplaced, so Move/Investigate would reject. `setup()` cannot place them
itself: investigators do not exist until the later `StartScenario` action
creates them from the host roster.

Fix — a generic, minimal channel:

- Add `GameState.starting_location: Option<LocationId>`.
- `setup()` sets it (The Gathering → the Study's id).
- `start_scenario`'s seating loop sets each seated investigator's
  `current_location = state.starting_location`. `None` → unchanged
  (back-compat with the pre-seated synthetic path and existing tests).

Single `Option<LocationId>` (not per-investigator / multi-start) — YAGNI;
generalize when a scenario needs investigator-chosen starts.

### 3. Attic & Cellar card abilities — `cards` crate

Scenario-structure cards are **not** in the generated corpus (player-cards
only). C1a adds hand-written impls; only their *abilities* are needed (the
forced-trigger scan calls `abilities_for`; location *state* — name/shroud/clues
— lives in `setup()`, so no card metadata is required).

- `crates/cards/src/impls/attic.rs`: `CODE = "01113"`, one ability —
  `on_event(EnteredLocation, After, deal_horror(You, 1))`.
- `crates/cards/src/impls/cellar.rs`: `CODE = "01114"`, one ability —
  `on_event(EnteredLocation, After, deal_damage(You, 1))`.
- Register both in `crates/cards/src/impls/mod.rs` `abilities_for`.

(`is_playable("01113")` becomes `true` — harmless; location codes never enter a
player deck, and "has abilities" is the correct answer.)

## Testing

1. **Card unit tests** (`attic.rs` / `cellar.rs`): assert the ability shape
   (one `OnEvent`/`EnteredLocation`/`After` ability whose effect is
   `DealHorror`/`DealDamage` to `You`, amount 1).
2. **Integration test** `crates/scenarios/tests/the_gathering.rs` (own process
   — installs `cards::REGISTRY` + `scenarios::REGISTRY`):
   - **Placement + resolution:** `setup()` → `StartScenario { roster }` with a
     synthetic roster → assert the seated investigator's `current_location` is
     the Study → drive to a resolution (via the existing act/agenda
     threshold latch) and assert `state.resolution` is `Some`.
   - **Forced effects:** via the existing `fire_forced_on_enter` test-support
     helper, assert entering the Attic deals 1 horror and the Cellar deals 1
     damage to the entering investigator, through the real registered card
     abilities.

## Out of scope (deferred, not faked)

- The four set-aside locations + the Act-1 *Door on the Floor* transition +
  faithful act objective types (round-end clue-spend gate; Ghoul-Priest-
  defeated) → **C1b (#228)**.
- Reference-card `01104` symbol-token effects; Attic/Cellar **victory points**
  → **C2 (#229)**.
- Agenda `01107` forced Ghoul movement + doom → **C3c (#232)**.
- Lita Chantler parley / Parlor `01115` Resign action → later Slice-1 work.
- Difficulty beyond Standard.

## Decisions captured for later PRs

- **`setup()` builds only the Study** (faithful Act-1 board); the rest of the
  world is C1b's `Door on the Floor`. Don't encode the set-aside locations'
  data in C1a — it would be dead until C1b.
- **`starting_location: Option<LocationId>` is the placement channel** for the
  whole `setup() → roster-seating` split — not a Gathering-specific hack. Every
  future scenario uses it.
- **Act thresholds in C1a are structural placeholders** (except the real
  agenda doom 3/7/10); C1b/C7b own faithful objective semantics.
