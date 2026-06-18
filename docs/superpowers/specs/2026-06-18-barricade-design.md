# Barricade 01038 — event-attaches-to-location + enemy-movement block + leave-location forced

**Date:** 2026-06-18
**Phase:** 7 (The Gathering) — Slice-1 follow-up, Axis-E carved Seeker card
**Issue:** [#323](https://github.com/talelburg/eldritch/issues/323)
**Deferred follow-up filed:** [#371](https://github.com/talelburg/eldritch/issues/371) (location-attachment ownership)
**Ships as:** one PR (three engine pieces + the card).

## Problem

Barricade was carved out of C6b (#242) for the engine machinery it needs.
Verbatim text (`data/arkhamdb-snapshot/pack/core/core.json`):

> **Barricade** (Insight. Tactic. Seeker event, cost 0):
> "Attach to your location.
> Non-[[Elite]] enemies cannot move into attached location.
> **Forced** - When an investigator leaves attached location: Discard
> Barricade."

Three capabilities the engine lacks: a played event that attaches to a
location and persists; a constant "non-Elite enemies cannot move into this
location" movement restriction; and a "when an investigator leaves a
location" forced trigger.

## Rules grounding

- **Hunter movement (RR p.12):** a Hunter enemy "moves … toward the
  nearest investigator … via the shortest possible path." A location that
  is impassable to the enemy is therefore excluded from the path *and* from
  the distance computation that picks the nearest investigator.
- **Elite:** `Elite` is a printed **trait** (Ghoul Priest 01116:
  "Humanoid. Monster. Ghoul. Elite."), already ingested into
  `CardMetadata.traits`. The Gathering's set-aside boss Ghoul Priest is
  Elite *and* a Hunter, so the non-Elite exemption is load-bearing in
  scope: Barricade must block Ghoul Minions (01160) but not the Ghoul
  Priest.

## Design

### Component 1 — `Effect::AttachSelfToLocation` (one card, no duplicate)

A played event already moves hand → `pending_played_event`
(`begin_event_play`), which the apply loop flushes to the owner's discard
on completion (`flush_pending_played_event`). Barricade's `OnPlay` effect
**re-homes that same card** instead of letting it discard:

```rust
Effect::AttachSelfToLocation
```

Resolution: take `pending_played_event`'s `(investigator, code)`, call the
existing `attach_to_location(cx, that investigator's current_location,
code)` (mints the instance into `location.attachments`, emits
`CardAttachedToLocation`), and **clear `pending_played_event`** so the
flush does not also discard it. One card throughout: hand → location
attachment → (on the Forced) player discard. Rejects if the controller is
between locations or no event is mid-play.

**No new `PlayDestination` and `play_card` is untouched** — the event flows
through `begin_event_play` normally; the `OnPlay` effect redirects its
disposition by consuming the pending entry. This is *not* the
`PutIntoThreatArea`-by-code pattern (which spawns a fresh instance because
an encounter card has no instance at Revelation time, `TODO(#290)`) — a
played event is a tracked card with a single disposition, so spawning a
copy would duplicate it.

### Component 2 — `Restriction::EnemyMovementBlocked` + hunter pathfinding

A new inspectable `Restriction` variant (joining `CannotPlay` /
`ExtraActionCost`), carried by the Barricade attachment as
`constant(restrict(Restriction::EnemyMovementBlocked))` and **inspected,
not executed** (like the other restrictions).

Hunter pathfinding in `hunters.rs` treats a location impassable to the
moving enemy as **absent from the graph** — graph-level, not a
post-filter on the final step. A barricaded location is impassable to an
enemy iff the enemy is **non-Elite**
(`!metadata_for(enemy.code).traits.contains("Elite")`). This affects:

- **`bfs_distance` / reachability** — so an investigator only reachable
  *through* a barricaded location is farther (or unreachable), changing
  **which investigator is nearest** (prey selection).
- **the shortest-first-step computation** — the chosen destination falls
  out of the pruned graph.

Implementation: thread an `is_passable(state, loc, enemy) -> bool`
predicate (barricaded ∧ non-Elite ⇒ impassable) through the BFS and
step enumerators; an Elite enemy and all non-barricaded locations pass
unchanged. "Barricaded" = the location's `attachments` carry a card whose
registry abilities include a `Constant` `Restrict(EnemyMovementBlocked)` —
read the same way `play_is_prohibited` reads constant restrictions.

Scope: Slice-1 enemy movement is Hunter-only (enemy phase), so `hunters.rs`
is the sole read site. Non-hunter enemy movement (none in scope) would
consult the same predicate when it lands.

### Component 3 — `LeftLocation` forced trigger

A played-side mirror of the existing on-enter forced path:

- **`EventPattern::LeftLocation`** (bare, forced; the engine binds the
  leaving investigator and the location).
- **`ForcedTriggerPoint::LeftLocation { investigator, location }`** —
  `collect_forced_hits` scans `state.locations[location].attachments` for
  `Forced` `OnEvent(LeftLocation)` abilities, threading the source instance
  into the `EvalContext` (mirroring C4c's location-attachment scan for
  `AfterLocationInvestigated`).
- **Emitted from `move_action`** for the *from* location after the move
  resolves (current_location set, engaged enemies moved). Emitted before
  the existing `EnteredLocation` emit (you leave, then arrive); the move's
  outcome is the chained result. In scope only Barricade's deterministic
  self-discard fires here, so no suspension arises (2+-forced suspension at
  this point is out of Slice-1 scope, consistent with the other forced
  points).

Barricade's `Forced` ability is `DiscardSelf`, which already removes a
location attachment via `EvalContext::source`. **`DiscardSelf` must route a
player-card attachment to the owner's player discard** (vs. the encounter
discard for an encounter-owned attachment like Obscuring Fog). In solo the
owner is the leaving investigator (the forced controller), so routing to
the firing controller's discard is exact; **multiplayer ownership tracking
on attachments is deferred to [#371](https://github.com/talelburg/eldritch/issues/371)**
(`TODO(#371)`).

### Component 4 — the card (data)

- **Barricade 01038** — two abilities:
  - `on_play(Effect::AttachSelfToLocation)`.
  - `forced_on_event(EventPattern::LeftLocation, EventTiming::After, discard_self())`
    — the Forced self-discard.
  - The constant restriction is carried by the *attachment* instance, so
    it's a third ability: `constant(restrict(Restriction::EnemyMovementBlocked))`.

  All three abilities live on 01038; the constant restriction only takes
  effect once the card is the location attachment (the same instance the
  scan reads), and the Forced only fires while it's there.

## Testing

Card test: abilities shape (OnPlay attach, Forced LeftLocation discard,
Constant restrict).

Integration tests (`crates/cards/tests/barricade.rs`, real
`cards::REGISTRY`):
- **Attach:** playing Barricade attaches `01038` to the controller's
  location (`CardAttachedToLocation`), the event does **not** discard
  (no spurious `CardDiscarded`), and there is exactly one `01038` in state.
- **Block (non-Elite):** a non-Elite Hunter whose only path to its prey
  runs through the barricaded location does not enter it (stays / re-routes
  per the impassable graph); assert it did not move into the barricaded
  location.
- **Elite exemption:** an Elite Hunter (Ghoul Priest 01116) moves into the
  barricaded location normally.
- **Nearest-prey shift:** an investigator reachable only through the
  barricade is treated as farther, so a non-Elite hunter targets a
  different (truly-nearest) investigator. (Demonstrates graph-level
  impassability, not a final-step filter.)
- **Leave discards:** when an investigator moves out of the barricaded
  location, the `Forced` discards Barricade to the player discard, and the
  location's `attachments` no longer contains it.

## Decisions

- **Single-copy via `AttachSelfToLocation`, not `PutIntoThreatArea`-by-code.**
  A played event is a tracked card; re-home it (consume
  `pending_played_event`) rather than spawning a duplicate. The encounter
  path's place-by-code is not a duplication bug (the drawn card is a bare
  code with one disposition); `TODO(#290)` tracks unifying it.
- **Graph-level impassability, not a final-step filter** — the barricade
  changes nearest-prey selection, so it must be excluded from BFS distance,
  not just the chosen step.
- **Non-Elite via the `Elite` metadata trait** — no new keyword parsing.
- **`DiscardSelf` routes Barricade to the owner's player discard** (solo:
  the leaving investigator); multiplayer ownership is
  [#371](https://github.com/talelburg/eldritch/issues/371).

## Out of scope / deferred

- Multiplayer location-attachment ownership ([#371](https://github.com/talelburg/eldritch/issues/371)).
- Minting encounter instances at reveal to unify self-placement (`#290`) —
  untouched; Barricade does not use that path.
- Mind over Matter 01036 (#322) — the other carved Seeker card, a separate
  sub-slice (skill-substitution + round-duration modifier).
