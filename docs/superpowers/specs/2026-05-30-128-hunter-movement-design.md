# Hunter movement + unified prey resolver (#128) — design

**Status:** Approved (design pass complete), ready for planning
**Issue:** #128 — `[engine] hunter movement: Prey enum + BFS pathfinding + engage-on-arrival`
**Phase:** 4 — Scenario plumbing
**Date:** 2026-05-30

## Goal

Implement the Hunter keyword's movement during Enemy-phase step 3.2: each ready,
unengaged hunter moves one connecting location along a shortest path toward the
nearest investigator, and engages on arrival. Ambiguous choices (which connecting
location to move to; which co-located investigator to engage) are resolved by the
**lead investigator** via `AwaitingInput`. The prey-target resolver built here is
shared across all three sites that need it, clearing the multi-investigator
engagement-on-spawn case that `#127` deferred.

## Rules grounding (Rules Reference, verbatim)

**Hunter (p.12):** "During the enemy phase (in framework step 3.2), each ready,
unengaged enemy with the hunter keyword moves to a connecting location, along the
shortest path towards the nearest investigator. Enemies at a location with one or
more investigators do not move. … If there are multiple equidistant investigators
who qualify as 'the nearest investigator,' the enemy moves towards the one of those
who best meets its prey instructions. If none do, or if the enemy has no prey
instructions, the lead investigator may choose an investigator for the enemy to move
towards. … If a hunter enemy would be compelled to a location to which the move is
blocked by a card ability, the enemy does not move."

**Prey (p.17):** "If an enemy that is moving towards the nearest investigator has a
choice between multiple equidistant investigators, that enemy must select among those
investigators the one who best meets its 'prey' instructions. (If multiple
equidistant investigators meet the prey criteria, the lead investigator decides among
those investigators.)" And for engagement: "If an enemy that is about to automatically
engage an investigator at its location has multiple options of whom to engage, that
enemy engages the investigator who best meets its 'prey' instructions (if multiple
investigators are tied … the lead investigator may decide among them)."

**Enemy Engagement (p.10):** "Any time a ready unengaged enemy is at the same location
as an investigator, it engages that investigator … If there are multiple investigators
at the same location as a ready unengaged enemy, follow the enemy's prey instructions
to determine which investigator is engaged."

Two load-bearing nuances:

1. **Prey only disambiguates among the *equidistant-nearest* set — it never overrides
   distance.** With `Prey – Highest [combat]`, if a closer investigator exists the hunter
   pursues the closer one regardless of combat; combat breaks a *distance* tie only.
   (The `... only` qualifier — "as if it were the only investigator in play" — is a
   different rule and is out of scope.)
2. **The chosen prey investigator does not persist.** Nothing on the enemy records "who
   am I hunting"; `Prey` is a fixed card property and "nearest" is re-derived each Enemy
   phase. So the *only* lasting consequence of the whole movement decision is **which
   connecting location the hunter ends at**.

## Design decisions

### Scope: "B" — full lead-decides resolver + multiplayer fixture

All three multi-investigator choice points (move destination, engage-on-arrival,
engage-on-spawn) require 2+ investigators to arise. We build the full interactive
resolver and add a **2-investigator** test fixture so the lead-decides branches are
actually exercised (a single-investigator fixture would ship those branches untested).
This also clears `#127`'s deferred multi-investigator engagement-on-spawn.

### Movement is a single location choice, not investigator-then-path

Because the chosen prey doesn't persist (nuance 2 above), "pick an investigator, then
pick a shortest path toward them" and "pick a destination location directly" are
**outcome-equivalent**. We surface movement as a single `PickLocation` over the
prey-filtered destination set; the investigator selection is instrumental (used only to
build the set) and never surfaced. The engine computes:

```
nearest      = investigators at minimum BFS distance from the hunter's location
valid_prey   = prey_filter(nearest)          // Default → all; HighestStat → max-stat subset
destinations = ⋃ shortest_first_steps(hunter_loc → p.loc)  for p in valid_prey
```

- `destinations` empty → no move (unreachable / already co-located / blocked-by-rule).
- `destinations` len 1 → move deterministically, no prompt.
- `destinations` len >1 → lead picks via `AwaitingInput` (`PickLocation`).

This subsumes both the equidistant-investigator tie *and* the (rules-implicit)
multiple-shortest-paths-to-one-investigator tie — both just yield `len(destinations) > 1`.

Engagement-on-arrival does **not** collapse: once the destination is fixed, choosing
whom to engage among co-located investigators is irreducibly a `PickInvestigator`.

### `hunter` and `prey` are separate fields on the runtime `Enemy`

The corpus proves they're independent: Ravenous Ghoul has `Prey – Lowest remaining
health` but no Hunter; Swarm of Rats has `Hunter` but no prey line.

- `Enemy.hunter: bool` — the keyword; gates movement (step 3.2).
- `Enemy.prey: Prey` — targeting; drives engagement-target selection for hunters **and**
  non-hunters. Defaults to `Prey::Default`.

```rust
// card-dsl, beside Spawn
#[non_exhaustive]
pub enum Prey {
    Default,            // nearest; lead breaks ties (RR p.12)
    HighestStat(Stat),  // e.g. Ghoul Priest: Prey – Highest [combat]
}
```

`Default` + one concrete variant. `HighestStat(Stat)` is non-speculative — it's literally
Ghoul Priest's instruction (The Gathering boss, the next content milestone). `Lowest`,
`Bearer only`, `Most clues`, etc. land with their first card consumer (`#[non_exhaustive]`
keeps that additive).

### No `CardMetadata` / pipeline changes

No generated enemy is instantiated as a runtime hunter yet (encounter enemies enter only
via the synthetic fixture / future scenario setup). The fixture sets `hunter`/`prey`
directly; `spawn_enemy` defaults spawned enemies to `hunter: false, prey: Prey::Default`.
The first real spawning hunter forces the metadata fields then — mirrors `#127` deferring
the pipeline side until a consumer exists.

### The shared prey resolver

```rust
enum PreyResolution { One(InvestigatorId), Tie(Vec<InvestigatorId>), None }
fn resolve_prey(state, prey: &Prey, candidates: &[InvestigatorId]) -> PreyResolution
```

- `Default` → `One` if `candidates.len() == 1`, else `Tie`.
- `HighestStat(s)` → keep max-stat candidates → `One`/`Tie`.

Three callers differ only in the candidate set:

1. **Move target** (p.12): candidates = equidistant-nearest reachable investigators.
2. **Engage-on-arrival** (p.10/p.17): candidates = investigators at the destination.
3. **Engage-on-spawn** (`#127`'s deferred case): candidates = investigators at the spawn location.

`Tie` → lead decides via `AwaitingInput`. "Lead" = `first_active_investigator(state)`
(existing helper; already handles an eliminated lead).

### BFS pathfinding (pure helpers)

```rust
fn shortest_first_steps(state, from: LocationId, to: LocationId) -> Vec<LocationId>
fn bfs_distance(state, from: LocationId, to: LocationId) -> Option<u32>
```

`shortest_first_steps` BFS over `Location.connections`; returns every neighbor of `from`
on *a* shortest path (`dist(neighbor, to) == dist(from, to) - 1`); empty = unreachable.

### Suspend/resume state

```rust
#[non_exhaustive]
enum HunterChoice {
    Move   { enemy: EnemyId, candidates: Vec<LocationId> },      // movement destination
    Engage { enemy: EnemyId, candidates: Vec<InvestigatorId> },  // whom to engage on arrival
}
// GameState.hunter_move_pending: Option<HunterChoice>
```

Rationale (vs. a bare `Option<EnemyId>` cursor like `mythos_draw_pending`):

- The investigator cursors stay bare because (a) "next" is re-derivable from `turn_order`,
  (b) the suspension *detail* lives in the `open_windows` entry, and (c) each item has one
  uniform interaction. Hunter choices are **not** windows and come in two distinct shapes
  (`PickLocation` vs `PickInvestigator`), so the tag is needed for resume routing.
- **No `queue`** — "next hunter" is re-derivable, exactly like the investigator cursor:
  process in ascending `EnemyId` order and, on resume, scan `state.enemies` (a `BTreeMap`,
  already id-sorted) for the next eligible hunter with id **strictly greater** than the
  current. Forward-only ordering prevents reprocessing a hunter that moved-but-didn't-engage
  (still `unengaged`, but id ≤ current). The `EnemyId` inside the choice *is* the cursor.
- The field is `Some` **only while suspended on a tie** — most hunters need no interaction,
  so the loop runs synchronously within one `apply`.
- `candidates` is stored (not recomputed on resume): validation is a trivial `.contains()`,
  it avoids recompute-vs-original drift, and gives the host the option list. (It *is*
  recomputable; storing is the deliberate explicit-over-clever choice.)

`hunter_move_pending` serializes with `GameState`; replay rebuilds it because `enemy_phase`
re-runs deterministically and the `PickLocation`/`PickInvestigator` responses are in the
action log. `InputRequest` stays `{ prompt }` (the candidate set lives in state and the
resolver validates against it) — structured options remain the already-noted future
enrichment.

### Per-hunter processing stages

Stage helpers each return `Resolved | Tie | NoMove/None`:

1. **Move:** if any investigator is at the hunter's location → no move (p.12). Else compute
   `destinations` (above). `One` → move. `Tie` → `HunterChoice::Move`. `None` → skip.
2. **Engage on arrival:** after the move, candidates = investigators at the new location.
   `resolve_prey` → `One` engages; `Tie` → `HunterChoice::Engage`; `None` → no engagement.

`drive_hunter_moves` runs the front (lowest-id unprocessed) eligible hunter start→finish;
on completion advances to the next; on a tie sets `hunter_move_pending` + returns
`AwaitingInput`. `resume_hunter_choice` validates the response against the stored
`candidates`, applies it, finishes that hunter's remaining stages, then falls back into
`drive_hunter_moves`. When the scan is exhausted, clears the field and calls
`enemy_attack_kickoff`.

### `enemy_phase` restructure

`enemy_phase` becomes: emit `PhaseStarted(Enemy)` → `drive_hunter_moves(...)`. The existing
tail (seed `enemy_attack_pending`, open the first/final attack window) is extracted into
`enemy_attack_kickoff`, called when hunter movement completes. The `EndTurn` cascade must
propagate `AwaitingInput` out of the Investigation→Enemy transition (precedent: the Mythos
driver already pauses the cascade); thread the `EngineOutcome` through the phase-step path.

### `resolve_input` routing

Add a branch: when `state.hunter_move_pending` is `Some`, route `PickLocation` /
`PickInvestigator` to `resume_hunter_choice` (rejecting a mismatched response kind). During
hunter movement no `open_windows` / `in_flight_skill_test` are outstanding, so the branches
are mutually exclusive; guard explicitly for clarity.

### Spawn path: option (i), uniform suspend

`spawn_enemy`'s engagement uses `resolve_prey` over the investigators at the spawn location,
replacing `#127`'s `reject`. On a `Tie`, the spawn path **suspends for the lead's
`PickInvestigator`** too — threading `AwaitingInput` through the Mythos encounter-draw spawn
path. (Chosen over the bounded "auto-assign to lead at spawn" so ties resolve identically
at spawn and during movement.) `#127`'s engage-on-spawn for 0/1 investigators is unchanged.

## Test plan

**game-core unit tests** (`TestGame`, no registry — `hunter`/`prey` are runtime fields):

- Hunter at a connected location moves one step toward the sole investigator and engages on
  arrival.
- Hunter with no path does not move; hunter already co-located does not move.
- Exhausted / already-engaged hunter is skipped.
- `HighestStat` picks the higher-stat investigator deterministically (no prompt).
- `Default` with equidistant investigators → `AwaitingInput`; `PickLocation` resumes and
  the hunter ends at the chosen location; invalid pick rejected.
- Multiple equally-short paths to one investigator → `AwaitingInput` → `PickLocation`.
- Engage-on-arrival with 2 investigators at the destination → `AwaitingInput` →
  `PickInvestigator`; prey instruction auto-resolves when it discriminates.
- Multi-hunter: one suspends, resumes, the next is processed (forward-id ordering).
- After movement completes, the cascade continues into the attack loop (`enemy_attack_kickoff`).

**Integration tests** (`crates/scenarios/tests`, `TEST_REGISTRY`, 2-investigator + diamond
map A→{B,C}→D):

- `#127` multi-investigator engagement-on-spawn now resolves (deterministic via prey; tie via
  lead `PickInvestigator`) instead of rejecting.
- Replay-equality across a `PickLocation` round-trip (mid-scenario serialize + replay).

## Out of scope

- `Lowest` / `Bearer only` / `Most clues` / `Lowest remaining health|sanity` / `Fewest cards`
  prey variants — land with their first card consumer.
- `CardMetadata` / pipeline `hunter` + `prey` fields.
- Aloof, Retaliate, Massive keywords; the `Prey ... only` qualifier.
- Multi-step movement (Phase-4 hunters move one location per Enemy phase).
- Move blocked by a card ability (RR p.12 final clause) — no consumer; the "no move on empty
  `destinations`" path is the natural seam when one arrives.
- Structured `InputRequest` option lists (engine-wide deferral, unchanged here).

## Open questions

None blocking. The blocked-move clause and structured input options are deferred with named
seams above.
