# Phase 7 Slice 1 — C3a: Prey variants + Retaliate

Engine primitives for two enemy keywords The Gathering's encounter enemies
need. Sub-slice C3a of Group C ([#230](https://github.com/talelburg/eldritch/issues/230)),
the root of the C3 enemy chain (`C3a → C3b → C3c`). Companion to the
[Group C decomposition](2026-06-11-phase-7-slice-1-group-c-decomposition-design.md).

## Scope

C3a is the **engine** half of the seam: it adds two reusable engine
capabilities and exercises them with synthetic enemies + engine unit tests.
It does **not** wire the real enemies. The two consumers are:

- **Ravenous Ghoul (01161)** — "Prey – Lowest remaining health."
- **Ghoul Priest (01116)** — "Hunter. Retaliate." (Hunter + highest-combat
  prey already exist; Retaliate is the new piece.)

### Out of scope (C3b, [#231](https://github.com/talelburg/eldritch/issues/231))

- Populating `hunter` / `prey` / `retaliate` onto spawned enemies. `spawn_enemy`
  keeps its hardcoded `hunter: false`, `prey: Prey::Default`, `retaliate: false`
  (the keywords live only in printed text today; mapping text → structured
  `Enemy` state is C3b's job, alongside reading `fight`/`evade`/`damage`/`horror`
  from `CardKind::Enemy`). C3a's only `spawn_enemy` touch is adding
  `retaliate: false` to the struct literal so it keeps compiling.
- The six enemy card impls and their tests.

## Card text & rules (verified against snapshot + Rules Reference)

- Ravenous Ghoul `01161`: `<b>Prey</b> - Lowest remaining health.`
- Ghoul Priest `01116`: `<b>Prey</b> - Highest [combat].\nHunter. Retaliate.`
- **Remaining health** (RR p.12): *"A card's 'remaining health' is its base
  health minus the amount of damage on it, plus or minus any active health
  modifiers."* No health modifiers are modeled yet, so for an investigator this
  is `max_health − damage`.
- **Retaliate** (RR p.18): *"Retaliate is a keyword ability. Each time an
  investigator fails a skill test while attacking a ready enemy with the
  retaliate keyword, after applying all results for that skill test, that enemy
  performs an attack against the attacking investigator. An enemy does not
  exhaust after performing a retaliate attack. ==This attack occurs whether the
  enemy is engaged with the attacking investigator or not."*
- **Skill-test steps** (RR p.26): ST.7 *Apply skill test results* ("the card
  ability or game rule that initiated the test … plus some other card abilities
  may contribute additional consequences … at this time"); ST.8 *Skill test
  ends* (discard committed cards, return tokens). "After applying all results"
  = after the whole of ST.7, before ST.8.

## 1. Prey — unified `Ranked { direction, measure }`

> **Revised after first review.** Originally shipped as a specific
> `Prey::LowestRemainingHealth` variant (deferring generalization). On a second
> look we unified instead: the "wait for 2+ consumers" rule was *already* met —
> `Prey::HighestStat(Combat)` and lowest-remaining-health are two consumers of
> the same ranked-measure pattern. So both collapse into one `{ direction,
> measure }` shape now, while we have full information about the axis.

- Replace `Prey::HighestStat(Stat)` **and** the would-be `LowestRemainingHealth`
  with one variant on the `#[non_exhaustive]` `Prey` enum
  (`crates/card-dsl/src/card_data.rs`):
  `Prey::Ranked { direction: PreyDirection, measure: PreyMeasure }`.
  - `PreyDirection { Highest, Lowest }` — exhaustive.
  - `PreyMeasure { Skill(SkillKind), RemainingHealth }` — **exhaustive** (not
    `#[non_exhaustive]`): adding a measure must force `resolve_prey` to wire it
    at compile time. Uses `SkillKind` (the four skills), *not* `Stat` — `Stat`
    also has `MaxHealth`/`MaxSanity`, nonsensical as a prey measure alongside a
    dedicated `RemainingHealth`; `SkillKind` makes illegal states
    unrepresentable and lets `Skills::value` be called directly (dropping the
    `cursor::stat_to_skill_kind` hop, which this change removes as dead code).
  - Ghoul Priest's "Highest [combat]" = `Highest` + `Skill(Combat)`; Ravenous
    Ghoul's "Lowest remaining health" = `Lowest` + `RemainingHealth`.
  - Future printed measures (`RemainingSanity`, `Clues`, `CardsInHand`, …) add a
    `PreyMeasure` variant + one `measure_value` arm. `Prey` stays
    `#[non_exhaustive]` for genuinely non-comparative future shapes ("Bearer
    only").
- `resolve_prey` (`crates/game-core/src/engine/dispatch/hunters.rs`) gets one
  `Ranked` branch: score each candidate via a `measure_value(measure, inv) ->
  i32` helper (`i32` so `base_health − damage` can't underflow and skills/health
  share a comparable type), take `max`/`min` per `direction`, keep the matchers.
  Returns `One` / `Tie` / `None` via the existing post-narrowing match.
- Tests in `resolve_prey_tests`: highest-skill single → `One` and tie → `Tie`;
  lowest-remaining-health single → `One` and tie → `Tie`; serde round-trips for
  both `Ranked` forms in `card_data.rs`.

## 2. Retaliate

### State

Add `retaliate: bool` to `Enemy` (`crates/game-core/src/state/enemy.rs`),
beside `hunter`, with a doc-comment citing RR p.18. Additive updates:
`test_enemy` fixture (`test_support/fixtures.rs`, `retaliate: false`) and the
`spawn_enemy` struct literal (`retaliate: false`).

### Firing — Option B (faithful placement)

Retaliate fires *after all of ST.7*, before ST.8 teardown. In the engine, ST.7
spans `apply_skill_test_follow_up` (the Fight consequence) **and**
`fire_on_skill_test_resolution` (OnSkillTestResolution triggers). So the
retaliate attack lands at the boundary between `fire_on_skill_test_resolution`
and the `PostOnResolution` teardown.

- Add `FinishContinuation::PostRetaliate { succeeded }`
  (`crates/game-core/src/state/game_state.rs`, `#[non_exhaustive]` Copy serde
  enum) between `PostFollowUp` and `PostOnResolution`.
- In `drive_skill_test` (`skill_test.rs`): the `PostFollowUp` arm, after running
  `fire_on_skill_test_resolution`, advances to `PostRetaliate { succeeded }`
  (instead of jumping to `PostOnResolution`).
- New `PostRetaliate` arm: fire the retaliate attack iff **all** hold —
  `!succeeded`; the in-flight record's `follow_up` is `Fight { enemy }`; the
  enemy is still in `state.enemies`; the enemy is ready (`!exhausted`); the
  enemy has `retaliate`. If so, `combat::enemy_attack(cx, enemy, investigator)`
  (places damage + horror, handles investigator defeat) **without exhausting the
  enemy** (RR p.18). Then advance to `PostOnResolution`. The `follow_up` field
  lives on the in-flight record until teardown, so the step re-reads it.

### Edge cases

- Non-Fight follow-up (Investigate/Evade/None), success, exhausted enemy, or
  non-retaliate enemy → no retaliate (rule is "fails a skill test **while
  attacking** a **ready** enemy").
- Enemy absent from `state.enemies` → graceful skip. (Can't happen on a *failed*
  fight — no damage was dealt — but the step degrades quietly rather than
  panicking, since retaliate is a bonus attack, not a load-bearing mutation.)
- Retaliate attack defeats the investigator → `enemy_attack` →
  `apply_investigator_defeat` handles elimination; ST.8 teardown (commit-card
  discard, `SkillTestEnded`) still runs in the following `PostOnResolution` step.
- The new step is also the natural future home for the "after an enemy attacks"
  reaction window (Guard Dog C5b, Roland's reaction) — forward-compatible, not
  built now.

### Tests (engine unit, `skill_test.rs`)

- Failed Fight vs. ready retaliate enemy → investigator takes the enemy's
  `attack_damage` + `attack_horror`; enemy stays ready (`!exhausted`);
  `SkillTestEnded` still emitted.
- Successful Fight vs. retaliate enemy → no retaliate attack (enemy takes the
  fight damage as usual).
- Failed Fight vs. **exhausted** retaliate enemy → no retaliate.
- Failed Fight vs. **non-retaliate** enemy → no retaliate.
- Failed **Evade**/**Investigate** test → no retaliate (only "while attacking").

## Files touched

| File | Change |
|---|---|
| `crates/card-dsl/src/card_data.rs` | `Prey::LowestRemainingHealth` variant + generalize-later note |
| `crates/game-core/src/engine/dispatch/hunters.rs` | `resolve_prey` branch + tests |
| `crates/game-core/src/state/enemy.rs` | `retaliate: bool` field + fixture-shape test update |
| `crates/game-core/src/test_support/fixtures.rs` | `test_enemy` → `retaliate: false` |
| `crates/game-core/src/engine/dispatch/encounter.rs` | `spawn_enemy` literal → `retaliate: false` |
| `crates/game-core/src/state/game_state.rs` | `FinishContinuation::PostRetaliate` variant + doc |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | driver wiring + `PostRetaliate` logic + tests |

## Decisions for the phase doc (on merge)

- Prey comparative instructions are one unified `Prey::Ranked { direction,
  measure }` shape (`PreyDirection` × `PreyMeasure`), replacing the old
  `HighestStat(Stat)`. `PreyMeasure` is exhaustive + uses `SkillKind`, so a new
  measure is compile-forced to wire `resolve_prey`. New measures
  (`RemainingSanity`, `Clues`, `CardsInHand`) land with their first consumer.
- Retaliate fires in a dedicated `FinishContinuation::PostRetaliate` step after
  ST.7's OnSkillTestResolution triggers and before ST.8 teardown (RR p.26
  "after applying all results") — also the future home of after-enemy-attacks
  reaction windows.
