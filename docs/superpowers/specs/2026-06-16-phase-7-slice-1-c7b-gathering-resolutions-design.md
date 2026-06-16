# C7b — End-to-end Gathering resolutions (solo Roland, Won + Lost)

**Issue:** [#245](https://github.com/talelburg/eldritch/issues/245) — Phase 7, Slice 1, sub-slice C7b.
**Status:** design approved 2026-06-16.

## Goal

The "Slice 1 done" gate: a headless integration test that seats solo
Roland, sets up The Gathering on the real registries, and drives to **both**
a genuine **Won** (`Resolution::Won { R1 }`) and a genuine **Lost**
(`Resolution::Lost`) resolution — each latched by the real engine, not
hand-set.

## Approach — hybrid fidelity

Driving a faithful turn-by-turn run to both resolutions is enormous and
brittle (the Ghoul Priest is health-5/solo, fight-4, Hunter + Retaliate;
the agenda-doom loss needs a full Mythos cadence). Instead: **drive the
cheap, deterministic real progression and seed only the expensive
preconditions, so the resolution itself stays engine-latched.**

The acceptance criterion is *reaching a real resolution* — `state.resolution`
set via the engine's latch + `Event::ScenarioResolved` emitted. Every
seeded shortcut is a precondition, never the resolution itself.

## File

A new integration-test binary `crates/scenarios/tests/the_gathering_resolutions.rs`
(own process, so it installs the process-global `scenarios::REGISTRY` +
`cards::REGISTRY` without colliding with other test binaries). Two tests
(`won`, `lost`) sharing a setup helper. Mirrors the existing
`crates/scenarios/tests/closing_demo.rs` pattern.

## Shared setup

`the_gathering::setup()` → `PlayerAction::StartScenario` seating solo
Roland (01001). After StartScenario, Roland is Active in the Study, round 1,
Investigation phase.

Two documented **determinism stand-ins** (test-only; neither touches the
resolution latch):

1. **Controlled chaos bag.** The Standard NotZ bag contains `AutoFail`
   (forces the test total to 0), so *no* skill value guarantees a success —
   a deterministic test must control the draws. The test overrides the bag
   to a fixed token (`Numeric(0)`) so investigates/fights resolve
   predictably. Production serves the Standard bag; this is determinism
   only.
2. **Minimal roster deck.** The resolution paths don't exercise deck
   contents, so the roster seats a small valid deck rather than the full
   30-card suggested list. (Deviates from the issue's "full suggested
   deck," recorded as a stand-in.)

## Won path (R1 — Ghoul Priest defeated)

Drive the real act progression; seed only the combat.

1. **Act 1.** Roland investigates the Study (clues sourced from the Study's
   printed clues) to hold ≥ 2 clues, then `PlayerAction::AdvanceAct` spends
   the clue threshold (2) → act 1 (01108) reverse: Hallway/Attic/Cellar/
   Parlor enter play, investigators relocate to the Hallway, the Study is
   removed. *[real]*
2. **Act 2.** Acquire clues in the Hallway, end the round → the act-2
   (01109) round-end clue-spend window (C3d) advances act 2 → its reverse
   spawns the Ghoul Priest (01116) in the Hallway. *[real — exercises the
   C3d round-end window and the set-aside spawn]*
3. **Seed** the spawned Ghoul Priest's `damage` to `max_health − 1` (one
   hit from death). *[seed — the expensive combat]*
4. Roland `Fight`s the Ghoul Priest → it is defeated → `act_01110`'s forced
   `EnemyDefeated{code:01116}` → `AdvanceCurrentAct` on the terminal act →
   `Resolution::Won { R1 }`. Assert `Event::ScenarioResolved` with
   `Resolution::Won`. *[real win-trigger]*

Roland's combat (3) is below the Priest's fight (4); the test seeds his
combat (or commits a skill) so the defeating Fight succeeds under the
controlled bag. Retaliate damage is moot — the Priest dies on that hit.

### Act-2 risk / fallback

The act-2 round-end drive (step 2) is the fiddliest: it needs clues sourced
in the Hallway and the C3d round-end window to fire. If it proves too
brittle to drive deterministically, the documented fallback is to **also
seed past act 2** — set `act_index` to the terminal act and place the Ghoul
Priest directly — and drive only the defeating Fight. This degrades exactly
one step (act-2 advancement, already unit-tested in C3d) from "driven" to
"seeded"; the win-trigger (defeat → advance → Won) stays real. The
implementation picks driven if it lands cleanly, else the seeded fallback,
and documents which in the test.

## Lost path (all investigators defeated)

1. From setup + StartScenario, **seed** Roland's `damage` to
   `max_health − 1` and place an enemy (a Ghoul Minion or the Priest)
   engaged with him. *[seed]*
2. Drive `EndTurn` into the Enemy phase → the engaged enemy attacks Roland
   for 2 → Roland is defeated → `check_all_defeated` (no Active
   investigator remains) latches `Resolution::Lost` → `Event::ScenarioResolved`.
   Assert `Resolution::Lost`. *[real fatal attack → real loss]*

The all-defeated latch is the cleanest deterministic loss; the agenda-doom
loss (01107 threshold) would need the full agenda + Mythos cadence and is
out of scope for this test.

## What "done" looks like

- A headless test reaches `Resolution::Won { R1 }` via the real
  defeat → advance → win latch.
- A headless test reaches `Resolution::Lost` via the real
  attack → all-defeated → loss latch.
- Both assert `Event::ScenarioResolved` + the latched `state.resolution`.
- Full strict gauntlet green.

## Stand-ins (all test-determinism, none touching the resolution latch)

- Controlled chaos bag (vs random Standard).
- Minimal roster deck (vs full suggested deck).
- Seeded Ghoul Priest health (Won) / seeded Roland health + engaged enemy
  (Lost).
- Possibly seeded act-2 advancement (fallback only; see risk above).

## Out of scope

- Agenda-doom (01107) loss path.
- Full suggested-deck fidelity.
- The web UI driving the run (server/web wiring is C7a, already shipped).
- #224 (roster-seating test migration) — pairs with this issue but is
  separate.
