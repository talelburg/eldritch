# Phase 7 Slice 1 — C1b: Act advancement (reverse effects + objective types)

Design for **C1b** ([#228](https://github.com/talelburg/eldritch/issues/228)) of
Phase 7 Slice 1. Companion to the Group C decomposition
(`2026-06-11-phase-7-slice-1-group-c-decomposition-design.md`) and the C1a
skeleton it builds on (`crates/scenarios/src/the_gathering.rs`, PR #250).

## Goal

Make The Gathering's act spine real on the existing forced-trigger rails:
the **Act-1 board transition** (the isolated Study expands into the four-
location map) and the **Act-3 forced objective** (advance when the Ghoul Priest
is defeated). Act 2's round-end objective is built later, in C3c (#232),
alongside that slice's round-end dispatch — see Scope.

## Scope

**In scope**

- **Pillar 1** — Act-1 (01108) reverse effect: the board world-build, modeled
  as a forced `OnEvent` ability *on the act card* (Option C below).
- **Pillar 3** — Act-3 (01110) objective: a **forced** advance when the Ghoul
  Priest (01116) is defeated, replacing C1a's placeholder clue threshold.

Both ride the existing forced-trigger rails (`fire_forced_triggers` /
`ForcedTriggerPoint`) with **no new window/suspend machinery**.

**Out of scope (deferred, tracked — see Deferrals & follow-ups)**

- **Pillar 2 — Act-2 (01109) round-end objective → moved to C3c (#232).**
  Planning found that a faithful "may, as a group, spend clues *when the round
  ends*" needs a suspendable round-end **player window** (the engine must pause
  at round end to offer the choice) — substantial new suspend machinery that
  directly overlaps C3c, which is *already* adding the round-end dispatch point
  (`ForcedTriggerPoint::RoundEnded`) for the agenda's forced doom. Building the
  round-end window once, in C3c, shared by act-2's optional advance and the
  agenda doom, avoids building it twice and avoids rework against #212's
  emit_event restructure. Act 2 keeps its current functional clue-spend
  (action-driven `AdvanceAct`, threshold 3) in the interim — playable, not yet
  round-end-faithful.
- Location reveal-on-entry + per-investigator clue placement → **#257**.
- Act-2 back content: Ghoul Priest spawn → **#231** (C3b); Lita / Parlor
  barrier → **#258**.
- Act-3 back content: the R1/R2 resolution choice and its campaign-log
  consequences → **Phase 9**.
- End-to-end defeat→Won fidelity → **C7b** (#245); C1b proves act 3 with a
  synthetic enemy.

## Rules grounding

Verified against the Rules Reference (`data/rules-reference/…`) and the card
text (`data/arkhamdb-snapshot/pack/core/core_encounter.json`).

**Forced vs. optional advancement — the card text draws the line.** Rules
Reference p.3 (*Act Deck and Agenda Deck*): the act deck advances when the
group spends the requisite clues, "normally done as a [Free] player ability …
If the act has an 'Objective –' instruction, that instruction overrides or adds
additional requirements." The two Gathering objectives are phrased
deliberately differently:

- 01109: "**Objective** – When the round ends, investigators in the hallway
  **may**, as a group, spend the requisite number of clues to advance." → the
  "may" makes it an **optional** player choice, timed to round end, restricted
  to the Hallway.
- 01110: "**Objective** – If the Ghoul Priest is Defeated, advance." → no
  "may", and the act carries **no clue threshold** (`clues: null`), so the
  optional clue-spend path does not exist. The bare conditional "advance" is
  **forced**: the moment the Ghoul Priest is defeated, act 3 advances. Players
  cannot choose to delay.

**Reveal mechanic deferred.** Rules Reference p.14: locations enter play
unrevealed and reveal (placing clues = clue value × #investigators) on first
*investigator* entry. This is unmodeled today (`Location.revealed` is a dormant
field; clues are flat). C1a already shipped the Study revealed with flat clues;
C1b enters the set-aside locations the same way. The real mechanic is **#257**.

## Design

All three pillars build on the **existing forced-trigger + card-registry
rails** (`engine/dispatch/forced_triggers.rs`): forced abilities are
`Trigger::OnEvent { pattern, timing: After }` printed on scenario-structure
cards, fired via `fire_forced_triggers(ForcedTriggerPoint)`. Single-trigger
path (2+ simultaneous hits reject loudly, #213); each Gathering act back is one
ability, so this holds.

### Reverse effects live on the act cards (Option C)

The `Act` struct's `code` field already exists specifically to "resolve the
act's `Trigger::OnEvent` abilities through the card registry" — the
architecture already anticipates registry-resolved act abilities. So an act's
reverse effect is an `OnEvent` ability in `cards`, looked up by code, exactly
like a playable card's abilities. New DSL primitives are scoped to **only what
01108's back needs now** (act-2's spawn is C3-coupled; act-3's back is
Phase-9), keeping the world-build vocabulary honest until a second scenario's
acts inform it.

### Pillar 1 — Act-1 (01108) reverse effect

01108 back (verbatim): *"Put into play the set-aside Hallway, Cellar, Attic,
and Parlor. Discard each enemy in the Study. Place each investigator in the
Hallway. Remove the Study from the game."*

**Dispatch.** Add `EventPattern::ActAdvanced` and
`ForcedTriggerPoint::ActAdvanced { code }`. In `advance_act`, fire the
**leaving** act's forced ability *before* moving the cursor (the reverse side
resolves, then the next act becomes current — Rules Reference p.3 step order).

**Ability.** `cards/src/impls/act_01108.rs` (or equivalent) exposes
`abilities() -> [Ability { trigger: OnEvent { pattern: ActAdvanced, timing:
After }, effect: Seq([...]) }]`.

**New `Effect` primitives** (each addressing locations by card code, resolved
to `LocationId` by the evaluator):

- `PutSetAsideLocationsIntoPlay` — drains `state.set_aside_locations` into
  `state.locations`.
- `RelocateAllInvestigators { to: CardCode }` — moves every investigator to the
  named location (here `"01112"`, the Hallway).
- `RemoveLocationFromGame { location: CardCode }` — removes the named location
  (here `"01111"`, the Study).
- *"Discard each enemy in the Study"* → **deliberately omitted** as a faithful
  no-op: nothing can spawn into the isolated Study in Slice-1 scope. Noted in
  the ability's doc comment, not silently dropped.

So 01108's effect is `Seq([PutSetAsideLocationsIntoPlay,
RelocateAllInvestigators { to: "01112" }, RemoveLocationFromGame { location:
"01111" }])`.

**Set-aside representation.** Add `set_aside_locations: Vec<Location>` to
`GameState` — an explicit Arkham "set aside, out of play" zone that later
scenarios reuse (chosen over a `revealed`/`in_play` flag on `Location`).
`setup()`:

- Builds all five Location structs with the full Gathering connection graph
  (scenario knowledge — no connection data exists in the corpus):
  **Hallway (01112)** ↔ Attic (01113), Cellar (01114), Parlor (01115); the
  **Study (01111)** isolated.
- Puts the Study in `state.locations`; the other four in
  `state.set_aside_locations`. All enter **revealed** with corpus clue counts
  (reveal mechanic deferred to #257).

The connections are wired at setup time against the LocationIds assigned to all
five up front, so the four enter play already connected when act 1 advances.

### Pillar 2 — Act-2 (01109) round-end objective → C3c (#232)

01109 front (verbatim): *"When the round ends, investigators in the hallway
may, as a group, spend the requisite number of clues to advance."*

**Moved out of C1b during planning.** Faithfully, the engine must *pause at
round end* to offer the optional "may" choice — i.e. a suspendable round-end
**player window** threaded through `upkeep_phase_end` (today a `()`-returning,
non-suspending step) plus `AdvanceAct` re-gating and a Hallway-restricted
contributor filter. That is substantial new suspend/window machinery, and it
lands on the **same round-end point** C3c (#232) is already adding
(`ForcedTriggerPoint::RoundEnded`, for the agenda's forced doom). Building the
window once in C3c — shared by act-2's optional advance and the agenda doom —
avoids duplicate machinery and rework against #212. C1b leaves act 2 on its
current action-driven `AdvanceAct` (threshold 3): functional, not yet
round-end-faithful.

### Pillar 3 — Act-3 (01110) forced objective

01110 front (verbatim): *"If the Ghoul Priest is Defeated, advance."* No clue
threshold.

**Dispatch.** Add `ForcedTriggerPoint::EnemyDefeated { code }`, fired from the
enemy-defeat path in `combat.rs` (which already emits `Event::EnemyDefeated`).
It scans the **current act** for a matching ability.

**Pattern narrowing.** Extend `EventPattern::EnemyDefeated` with an optional
`code: Option<CardCode>` narrow (so the act fires only on the Ghoul Priest's
defeat, not any enemy's). `None` preserves today's any-defeat behavior.

**Effect.** Add `Effect::AdvanceCurrentAct` — advances the current act (firing
its terminal resolution if any). 01110's ability: `OnEvent { pattern:
EnemyDefeated { code: Some("01116") }, timing: After } → AdvanceCurrentAct`.
Advancing 01110 hits its terminal `Resolution::Won { id: "R1" }` — replacing
C1a's placeholder clue threshold of 2.

**Dormant until C3, tested now.** The Ghoul Priest (01116) does not exist until
C3b (#231), so this path is **unit-tested in C1b with a synthetic enemy bearing
code 01116**; the real defeat→Won is proven end-to-end in C7b (#245). The
single Won/R1 latch is retained; the R1-vs-R2 choice (01110 back) is Phase-9.

## State & DSL surface added

- `GameState.set_aside_locations: Vec<Location>`; `Enemy.code: CardCode` (the
  `Enemy` struct carries no printed code today — needed so Act 3 can route on
  the *defeated* enemy's code after it leaves `state.enemies`).
- `card-dsl`: `EventPattern::ActAdvanced`; `EventPattern::EnemyDefeated.code:
  Option<CardCode>` (drops `Copy` from `EventPattern`/`Trigger` — see note);
  `Effect::{PutSetAsideLocationsIntoPlay, RelocateAllInvestigators,
  RemoveLocationFromGame, AdvanceCurrentAct}`.
- `engine`: `ForcedTriggerPoint::{ActAdvanced, EnemyDefeated}` + their
  `collect_forced_hits` arms; evaluator arms for the new effects; `advance_act`
  and the `damage_enemy` defeat path fire their forced points (the `()`-return
  sites use the established `debug_assert!(matches!(_, Done))` guard).
- `cards`: `01108` abilities (reverse), `01110` abilities (objective).

**`Copy` note:** adding a `CardCode` (`String`-backed) field to
`EventPattern::EnemyDefeated` removes the `Copy` derive from `EventPattern` and
`Trigger`. The ~4 sites that move out of `ability.trigger` by `Copy`
(`forced_triggers`, `reaction_windows`, `abilities`, `skill_test`) switch to
`&ability.trigger` with reference patterns; `push_matching`'s `want:
impl Fn(EventPattern)` becomes `Fn(&EventPattern)`. Bounded, and honest DSL
growth (string-bearing patterns are inevitable).
- `scenarios`: `setup()` builds five locations + graph + set-aside split; drops
  the 01110 placeholder threshold.

## Deferrals & follow-ups

Every deferral gets a `TODO(#NN)` at the code site and a note in the phase doc.

| Deferred | Surfaces at | Tracking |
|---|---|---|
| Act-2 round-end objective (the window) | act 2 front | C3c (**#232**) |
| Ghoul Priest spawn in Hallway | act-2 back | `TODO(#231)` (C3b) |
| Lita Chantler / Parlor barrier / Resign | act-2 back, 01115, R1 | `TODO(#258)` |
| R1/R2 resolution choice + consequences | act-3 back | `TODO` → Phase 9 |
| Location reveal-on-entry + per-inv clues | set-aside locations, Study | `TODO(#257)` |
| End-to-end defeat→Won | act-3 objective | C7b (#245) |

## Testing

Per the test-layering convention (cards → engine unit → integration):

- **Engine unit** (`act_agenda.rs`, `forced_triggers.rs`, `evaluator.rs`):
  - Act-1 advance fires the world-build: set-aside locations move into play with
    connections, all investigators relocate to the Hallway, the Study is
    removed.
  - Act-3: a synthetic 01116 defeat advances 01110 → Won/R1 latch; an unrelated
    enemy's defeat does not (code narrowing).
- **Card tests** — 01108 and 01110 abilities (per the per-card convention).
- **Integration** (`crates/cards/tests/`, real registry): act-1 reverse fires
  through `fire_forced_triggers` end-to-end; defeating a real corpus 01116
  enemy advances act 3 to Won.
- **`the_gathering.rs` setup tests** — updated for the set-aside split and the
  real 01110 objective (drop the placeholder-threshold assertions).
- **Scenario integration** (`scenarios/tests/the_gathering.rs`) — drive setup →
  advance act 1 → assert the four-location board, investigators in the Hallway,
  Study gone, connections correct.

## Open questions

None blocking. Act 2's round-end objective is the deferred surface (→ C3c
#232); C1b's two pillars both extend the existing single-trigger forced path,
so the only genuinely new engine surface is the four DSL effects and the
`EventPattern` `Copy`→`Clone` adjustment.
