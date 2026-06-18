# Trigger-dispatch rework — Axis D: cancellation / replacement (Before-timing)

**Status:** design approved (deep-dive). Implements umbrella §4 D
([umbrella](2026-06-16-trigger-dispatch-rework-umbrella-design.md)).
Tracker [#336](https://github.com/talelburg/eldritch/issues/336); unblocks
Dodge 01023 ([#305](https://github.com/talelburg/eldritch/issues/305)).

**Scope:** the general **Before-timing dispatch** wiring through `emit_event`
plus a **cancel/replacement signal** the emitting site honors. Two consumers:
the enemy-attack loop (Dodge 01023 cancels an attack) and the `discover_clue`
chokepoint (Cover Up 01007's clue-discovery replacement, **migrated** off its
bespoke `clue_interrupt` seam). Depends on Axis B (the `emit_event` chokepoint
+ `ResolutionFrame`) and Axis C (hand Fast-event play riding the candidate
list — Dodge is a hand event).

---

## 1. Context — what already exists

- **`emit_event`** (`crates/game-core/src/engine/dispatch/emit.rs`) is the
  unified forced-then-reaction chokepoint (Axis-B T5a). A `TimingEvent` maps to
  an optional forced point and an optional reaction window. For a reaction-only
  event (e.g. `EnemyAttackDamagedSelf`) it just *queues* the window and returns
  `Done`; the **caller** drives the suspend (the soak-window pattern in
  `drive_attack_loop`).
- **`trigger_matches`** (`dispatch/reaction_windows.rs:259`) hard-returns
  `false` for any `EventTiming::Before` (line 265) — **no Before-timing window
  fires anywhere yet.** This is the gap Axis D fills.
- **The enemy-attack loop** (`dispatch/combat.rs`): `drive_attack_loop` iterates
  attackers; per attacker it calls `enemy_attack` (which places damage/horror
  *immediately*, soak-first), then queues the soak reaction window *after*
  placement, then exhausts, then suspends if a soak window opened. Suspend/resume
  is via `GameState.pending_enemy_attack` (+ `remaining_attackers`) and
  `resume_enemy_attack`, re-entered from `run_window_continuation`'s
  `AfterEnemyAttackDamagedAsset` arm.
- **Axis C** (`dispatch/reaction_windows.rs`) already admits a hand Fast event as
  a `CandidateSource::Hand` option in a reaction window, played via
  `play_fast_event` → `cards::begin_event_play`, which runs the matched
  `OnEvent` ability's effect. A reaction event is **window-only** (a standalone
  `PlayCard` of a reaction event rejects). Dodge rides this path unchanged.
- **The `clue_interrupt` seam** (C5a #236): Cover Up 01007's "discover → discard
  instead" is today a bespoke card-local interrupt at the `discover_clue`
  chokepoint (`GameState.clue_interrupt_pending` + a `Confirm`/`Skip` prompt +
  `dispatch/clue_interrupt.rs::resume_clue_interrupt`). Axis D **absorbs** this
  as its second consumer, validating the general design (umbrella §2 ¶6, §4 D).

## 2. The cards (verbatim) and load-bearing rules

Card text, copied from `data/arkhamdb-snapshot/pack/core/core.json`:

- **Dodge 01023** (Tactic event): *"Fast. Play when an enemy attacks an
  investigator at your location. / Cancel that attack."*
- **Cover Up 01007** (Task story asset), the relevant ability: *"[reaction]
  When you would discover 1 or more clues at your location: Discard that many
  clues from Cover Up instead."*

Rules Reference (`data/rules-reference/ahc01_rules_reference_web.pdf`):

- **Cancel (p.6):** *"Cancel abilities interrupt the initiation of an effect,
  and prevent the effect from initiating. … Any time the effects of an ability
  are canceled, the ability (apart from its effects) is still regarded as
  initiated, and any costs have still been paid. The effects of the ability,
  however, are prevented from initiating and do not resolve."*
- **Enemy attack (p.25, step 3.3):** *"Each ready, engaged enemy makes an attack
  against the investigator to which it is engaged. When an enemy attacks, deal
  its attack (both its damage and its horror, simultaneously) to the engaged
  investigator. Upon completion of dealing the attack (and all abilities
  triggered by the attack), exhaust the enemy."*
- **Attack of opportunity (p.7):** *"An enemy does not exhaust while making an
  attack of opportunity."*

**Exhaustion of a cancelled attack.** The RR does not have an explicit sentence
about a cancelled attack and exhaustion. The reading: the enemy *made* an attack
(that is the trigger Dodge keys off); Dodge cancels its *effect* (the
damage/horror); but per the Cancel rule the attack "apart from its effects is
still regarded as initiated", so it reaches "completion" and **the enemy still
exhausts.** The "always exhaust" therefore stays scoped to the **enemy-phase
loop** and is *not* shared into the attack-of-opportunity path, which by p.7
never exhausts (the AoO path is deferred anyway — see §7 / #293).

## 3. DSL surface + Before-timing dispatch

Three new types, all minimal (no speculative fields — matching the bare
`EventPattern::EnemySpawned` precedent):

- **`EventPattern::EnemyAttacks`** (`card-dsl`) — bare. The "an investigator **at
  your location**" spatial scoping lives in the *scan* (which has `state`), not
  in `trigger_matches` (which doesn't), exactly as the soaked-asset instance
  filter already does for `EnemyAttackDamagedSelf`.
- **`Effect::Cancel`** (`card-dsl`) — a leaf that cancels the current cancellable
  impact (§4).
- **`WindowKind::BeforeEnemyAttack { enemy, investigator }`** and
  **`WindowKind::BeforeDiscoverClues { investigator, location, count }`** — the
  two Before-windows (`investigator` is the attacked / discovering investigator).

Dispatch wiring:

- New **`TimingEvent::EnemyAttacks { enemy, investigator }`** and
  **`TimingEvent::WouldDiscoverClues { investigator, location, count }`**. Both
  are reaction-only Before: `forced_point() → None`, `forced_continuation() →
  None`, `reaction_window() → Some(the Before-window)`. Like
  `EnemyAttackDamagedSelf`, `emit_event` just queues the window; the caller
  drives the suspend.
- **`trigger_matches`** is restructured so `EventTiming::Before` is legal for
  exactly two `(window, pattern)` pairs — `(BeforeEnemyAttack, EnemyAttacks)`
  and `(BeforeDiscoverClues, WouldDiscoverClues)` — while `After` keeps its
  current behavior for everything else. (Today the function short-circuits
  `false` on any `Before`.)
- **The scans gain the spatial / eligibility filters** (`state` available there,
  per the `EnemyAttackDamagedSelf` precedent):
  - `BeforeEnemyAttack`: a candidate is eligible only if its controller is
    co-located with the attacked `investigator` (Dodge's "at your location";
    correct for solo-with-2, trivially true for the solo-1 playable target).
    Applies to both in-play reactions (`scan_pending_triggers`) and hand Fast
    events (`scan_hand_fast_events`).
  - `BeforeDiscoverClues`: keeps Cover Up's existing `card.clues > 0`
    potential-gate stand-in and the controller-co-location check.

## 4. The cancel signal

`Effect::Cancel` sets a single **`GameState.pending_cancellation: bool`**,
evaluated while the Before-window resolves (inside `fire_pending_trigger` /
`play_fast_event`). Each emit site, immediately after its Before-window closes,
`std::mem::take`s the flag and honors it:

- enemy-attack loop → **skip `enemy_attack`** for that attacker (no damage/horror
  placed), but **still exhaust** it (§2; RR p.25). No soak window can open (no
  damage ⇒ no surviving damaged soaker).
- `discover_clue` → **skip `perform_discovery`**.

**Why a `bool`, not a typed `Option<Cancellable>`:** Before-windows do not nest
in Slice-1 scope (the attack loop suspends, *or* `discover_clue` suspends — never
both), so exactly one cancellable impact is in flight and the consuming site is
unambiguous. A loud `debug_assert` fires if `Effect::Cancel` is evaluated with no
resolution frame open (a malformed card — `Cancel` outside a window), so the flag
can't be silently stranded. Graduating to a typed marker when Before-windows can
nest is deferred to **#367**, with a `TODO(#367)` on the field and the
`Effect::Cancel` apply site.

**Cancel as degenerate replacement** falls out: Cover Up's reaction effect is
`Seq[Native("01007:discard-that-many-from-self"), Cancel]` — it runs its own
effect *and* cancels the discovery. Dodge's effect is just `Cancel`. A true
replacement (substitute a *different* impact) is **not** built — deferred to
**#366** (`TODO(#366)` on `Effect::Cancel`).

## 5. Enemy-attack loop integration (Dodge path)

`drive_attack_loop` becomes an explicit per-attacker state machine. A
**Before-window opens at the top of the iteration, before any damage**:

Per attacker, in order:
1. early-break on defeat (unchanged).
2. **`emit_event(EnemyAttacks { enemy, investigator })`** → opens
   `BeforeEnemyAttack` iff an eligible cancel reaction exists (Dodge in a
   co-located hand / an in-play reaction). If `open_windows` is non-empty,
   **park and suspend before dealing damage.**
3. `enemy_attack` (place damage) — **skipped if cancelled** (see resume).
4. queue the soak window (`emit_event(EnemyAttackDamagedSelf)`) — unreachable
   when cancelled (no damage).
5. exhaust the attacker — **always** (even on cancel; §2).
6. suspend-if-soak (unchanged).

**Suspend/resume.** `PendingEnemyAttack` gains a `phase: AttackLoopPhase
{ BeforeAttack, AfterSoak }` marker. Suspending on the Before-window keeps the
*current* attacker at the head of `remaining_attackers`. `run_window_continuation`
gets a `BeforeEnemyAttack` arm that re-enters `resume_enemy_attack`; on
`BeforeAttack` it `take`s `pending_cancellation`, then for the head attacker
either skips or runs `enemy_attack`, exhausts it, and continues the loop;
`AfterSoak` is today's behavior. The single-soak-window-per-attack `debug_assert`
invariant is preserved.

**Scope:** enemy-phase attacks only. AoO-cancel is deferred to **#293** (same
root cause as the Guard Dog AoO soak gap: `fire_attacks_of_opportunity` opens no
enemy-attack reaction window, and a before-window there needs the mid-action
park/resume #293 already tracks). #293 has been updated with this scope and the
RR p.7 "AoO does not exhaust" caveat.

## 6. Cover Up migration

`discover_clue` drops its bespoke seam:

- `discover_clue` calls `emit_event(WouldDiscoverClues { investigator, location,
  count })`. If a `BeforeDiscoverClues` window opened, suspend via
  `open_queued_reaction_window`. The window offers Cover Up as a `PickSingle`
  candidate (in-play reaction); the player plays it or `Skip`s.
- The window threads `count` into `EvalContext.clue_discovery_count` when firing
  the candidate — the same window-kind-specific `EvalContext` threading the soak
  window uses for `attacking_enemy`. Cover Up's `Seq[discard-that-many, Cancel]`
  runs during `fire_pending_trigger`.
- `run_window_continuation` gains a `BeforeDiscoverClues` arm: `take`
  `pending_cancellation` → if not cancelled, `perform_discovery`; then if a skill
  test is in flight, re-enter `drive_skill_test`. This **preserves the existing
  reentrancy contract** — `finish_skill_test` pre-advances its continuation to
  `PostFollowUp` before the follow-up suspends. This is the highest-risk piece of
  the migration and gets dedicated tests.

**Deleted:** the `ClueInterruptPending` struct, the `clue_interrupt_pending`
field, `dispatch/clue_interrupt.rs::resume_clue_interrupt`, and its routing in
`dispatch/mod.rs`. The `card.clues > 0` potential-gate and the requested-vs-
capped `count` caveat carry over verbatim into the new scan/window, with their
stale `TODO(#212)` refs (#212 closed) repointed to **#368**.

## 7. Dodge card + testing

**Dodge 01023** — `crates/cards/src/impls/dodge.rs`, `CODE = "01023"`, one
ability: `Ability { trigger: OnEvent { pattern: EnemyAttacks, timing: Before,
.. }, effect: Effect::Cancel }`. Window-only by Axis C's rule. Card text
re-verified against the snapshot at write time.

Testing, per the project's test-layering:

1. **Card test** (`dodge.rs`) — ability shape / serialization.
2. **Engine unit tests** — `trigger_matches` Before pairs; `drive_attack_loop`
   before-window suspend → cancel-resume (no damage, attacker exhausted) and
   skip-resume (damage lands); `Effect::Cancel` set/consume + the loud
   `debug_assert` for `Cancel` outside a window.
3. **Integration tests** (real registry, `crates/cards/tests/`) — a new
   `dodge.rs`: seat an investigator with Dodge in hand, drive an enemy-phase
   attack, pick Dodge → assert no damage/horror events, `EnemyExhausted` present,
   card in discard; a `Skip` path → damage lands. Plus a **migration-regression**
   pass over the Cover Up integration tests (must stay green through the seam
   swap), including the before-discover window opening **mid-Investigate-follow-up**
   and resuming the skill-test driver correctly.

## 8. Deferrals (each with an issue + a code `TODO`)

| Deferral | Issue | `TODO` site |
|---|---|---|
| AoO-cancel (Dodge against attacks of opportunity) + AoO non-exhaust | [#293](https://github.com/talelburg/eldritch/issues/293) | `fire_attacks_of_opportunity` |
| Replacement with a *different* impact (beyond cancel) | [#366](https://github.com/talelburg/eldritch/issues/366) | `Effect::Cancel` doc |
| Typed cancellation marker for nested Before-windows | [#367](https://github.com/talelburg/eldritch/issues/367) | `pending_cancellation` field + `Effect::Cancel` apply |
| Before-discover eligibility (RR p.2 potential) + capped count | [#368](https://github.com/talelburg/eldritch/issues/368) | migrated discover-window scan (was `TODO(#212)`) |

Out of scope and **not** Axis D's deferral: the pre-existing `TODO(#212)` at
`state/game_state.rs:243` (C4b `pending_revelation_discard` generalization) — a
separate concern, left untouched.

## 9. Rejected alternatives

- **Generalize the bespoke `clue_interrupt` yes/no seam to cover attacks too.**
  Bypasses `emit_event` and the candidate list, so it can't play Dodge from hand
  (the Axis-C path), and perpetuates the single-purpose-seam smell the umbrella
  wants Axis D to dissolve.
- **First-class replacement-effect queue (MtG-style).** Over-engineered for two
  consumers, both "prevent the impact, optionally run my own effect"; speculative
  primitive-building. Deferred shape captured in #366.

## 10. Decisions made (for the phase doc when this lands)

- **Before-timing dispatch is a reaction-only `emit_event` Before-window** the
  caller suspends on (the soak-window pattern), and **cancel is `Effect::Cancel`
  setting a single `pending_cancellation: bool` honored by the emit site after
  the window closes.** Cancel = degenerate replacement; Cover Up becomes
  `Seq[discard-from-self, Cancel]`. The bool suffices because Before-windows
  don't nest in scope (typed marker → #367); a cancelled enemy-phase attack still
  exhausts (RR p.6 + p.25), and that "always exhaust" stays out of the AoO path
  (RR p.7; AoO deferred to #293). The C5a `clue_interrupt` seam is **deleted** and
  re-expressed on this mechanism, making it a 2-consumer general design.
