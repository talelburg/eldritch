# Axis B — trigger-dispatch foundation (#212 / #213 / #117)

**Status:** design. First buildable sub-project of the trigger-dispatch rework
(umbrella: `2026-06-16-trigger-dispatch-rework-umbrella-design.md`). Read the
umbrella first — this spec assumes its §1–§5 decisions.

**One-line goal:** replace the engine's two hand-wired trigger-dispatch paths and
its proliferation of suspension modes with **one continuation stack** and **one
`emit_event` chokepoint** doing rules-correct two-phase (forced-then-reaction)
dispatch. No new cards; correctness-complete; the foundation Axes A/C/D build on.

## What's wrong today (recap, from reading the engine)

- **Two parallel dispatch paths.** Forced abilities fire immediately via
  `fire_forced_triggers(cx, ForcedTriggerPoint)` (collect-then-resolve, fixed
  deterministic order, **abandons later hits on suspend** — its own doc-comment
  flags this as the reentrancy gap). Optional reactions open a window via
  `queue_reaction_window` / `open_fast_window` (`open_windows` stack, player
  pick/skip). The same game moment can need both (an enemy defeat → forced act-3
  advance **and** Roland's reaction), wired as two separate calls.
- **forced-vs-reaction routed by `EventPattern`** (C6a workaround for "no
  `Trigger::Forced`"), forcing twin patterns for one moment
  (`AfterLocationInvestigated` forced vs `SuccessfullyInvestigated` reaction).
- **Event emission is `cx.events.push` with no dispatch hook** (the #212 smell).
- **Suspension is hand-rolled per feature.** The skill-test driver is itself a
  continuation state machine (`FinishContinuation`: AwaitingCommit → PostFollowUp
  → PostRetaliate → PostOnResolution; `close_reaction_window_at` re-enters
  `drive_skill_test`). Each suspension mode is a `pending_*` field + a guard in
  `apply_player_action` + a route in `resolve_input`.

## Target architecture

### The one stack

`GameState` gains `continuations: Vec<Continuation>` (serde, replay-safe). It is
the single suspend/resume authority. The single resume router lives at the top of
`resolve_input`: if the stack is non-empty, resume its top frame; else fall
through to the legacy `pending_*` ladder (unchanged — those modes stay on their
fields per the umbrella's incremental boundary).

```rust
enum Continuation {
    /// One iterative trigger-resolution loop — used for BOTH the forced phase
    /// and the reaction window (see "One loop, two phases" below). Absorbs
    /// `open_windows`-the-Vec; the former `OpenWindow` (kind, pending, fast_actors)
    /// plus the loop parameters live in the frame.
    Resolution {
        candidates: Vec<Candidate>,   // forced abilities | reaction abilities + fast plays
        can_skip: bool,               // forced: false; reaction: true
        decider: Decider,             // lead (forced) | player-order (reaction)
        binding: EventBinding,        // controller / source / attacking enemy / location, from the event
        timing: EventTiming,
    },
    /// The skill-test driver is mid-resolution; data is in the singleton
    /// `in_flight_skill_test` field (read by many call sites; no nesting today).
    SkillTest,
    // Axis A adds Choice { .. }; Axis D adds whatever cancellation needs.
}
```

Window data moves *into* the frame — **no marker-pointing-at-a-separate-Vec**
(that dual-bookkeeping is the trap). `in_flight_skill_test` stays a singleton
field referenced by the `SkillTest` frame (widely read; only one in flight). The
orthogonal phase `pending_*` modes (mulligan, hand-size discard, hunter-move,
act-round-end, enemy-attack-loop, end-turn) are untouched.

#### One loop, two phases

The forced phase and the reaction window are **the same iterative loop** —
collect candidates → present (with skip iff `can_skip`) → decider picks one →
resolve (or skip → end) → re-collect → repeat until empty. Today
`fire_forced_triggers` and `resume_reaction_window` are two separate
implementations of that one loop; this collapses them. The two phases are two
*parameterized runs*:

| | forced phase | reaction phase |
|---|---|---|
| `can_skip` | false (mandatory) | true (pass) |
| `candidates` | `Forced` `OnEvent` abilities | `Reaction` abilities **+ Fast plays from hand** (Axis C) |
| `decider` | lead investigator (RR p.17) | player order (RR p.2; = the one player in solo) |

The rules *vocabulary* stays distinct (a forced resolution and a reaction window
are different concepts; prompts/logs say which), but via the parameters — not two
code paths. The simpler forced path thereby adopts the reaction window's existing
machinery (offered set, `PickSingle`/`OptionId`, usage limits) with
`can_skip=false`, instead of maintaining its own.

### `emit_event` two-phase dispatch

`emit_event(cx, event)` is the chokepoint for events that have a matching
`EventPattern`. It pushes the event, then per (Before/After) timing runs the
shared resolution loop twice — phase 1 (forced), then phase 2 (reaction):

1. **Phase 1 — forced.** Collect `kind: Forced` `OnEvent` hits. 0 → skip. 1 →
   resolve. 2+ → push a `Resolution` frame (`can_skip=false`, `decider=lead`) and
   run the loop (resolve one, remove, re-present the rest — RR p.2/p.17). A hit
   whose effect suspends (Frozen in Fear's test; later a choice) parks on the
   stack and resumes the loop — **this dissolves #294 and the abandon-on-suspend
   caveat.**
2. **Phase 2 — reaction.** Run the same loop (`can_skip=true`, `decider=player-order`)
   over `kind: Reaction` abilities + Fast plays; skip = pass.

**`Event` vs `EventPattern` (why two enums).** `Event` (game-core) is the ground
*fact* — concrete ids (`EnemyDefeated { enemy, by, code }`), for the log/replay/
client + as the binding source threaded into `EvalContext`. `EventPattern`
(card-dsl) is the listener's *filter* — a predicate relative to the listener
(`{ by_controller: bool, code: Option<String> }`; `by_controller` and the
`Option` wildcard are meaningless on a concrete fact). The match is
`same_discriminant && predicate(pattern, event, listener)`. They can't merge:
the relative/wildcard qualifiers can't live on a fact, and — decisively —
`card-dsl` is below `game-core`, so `EventPattern` cannot reference engine ids
(`EnemyId`/`InvestigatorId`/`LocationId`). The only shared part is the
discriminant case-list, which is exactly the `TriggerKind` key #117 indexes by.

The `EnemyDefeated` anchor: the defeat site calls `emit_event(cx,
Event::EnemyDefeated { by, code, .. })` once; phase 1 runs the forced act-3
advance, phase 2 opens the Roland window. Framework `PlayerWindow(PhaseStep)`
windows have no `EventPattern` and stay explicit `open_fast_window` calls.

### The DSL change

`Trigger::OnEvent { pattern, timing }` → `{ pattern, timing, kind: TriggerKind }`
where `TriggerKind = Forced | Reaction`. Retires route-by-pattern; lets one
moment carry both a forced and a reaction listener. Existing `OnEvent` cards are
classified at migration (the current forced points → `Forced`; the current
reaction-window cards → `Reaction`).

### The scan interface + #117 (final task)

`emit_event` collects hits through **one scan function**, implemented first with
the existing full board walk (`collect_forced_hits` / `scan_pending_triggers`
logic, unified). The **#117 event-keyed index** (`TriggerKind → Vec<entry>`,
maintained at `CardInPlay` enter/leave-play, seeded at registry install) is
swapped in behind that interface as the **last task** — isolating its new
invariant (every zone transition updates the index or it desyncs) from the
dispatch restructure. Net maintainability win: trigger-discovery registered once
at enter/leave-play, not smeared across per-timing-point scan arms.

## Proposed task breakdown (refined in the plan)

1. **DSL: `TriggerKind` on `Trigger::OnEvent`** + classify all existing `OnEvent`
   cards; serde round-trip. (No behavior change yet — both dispatch paths read
   the field instead of inferring from which path they're in.)
2. **Continuation stack scaffolding:** `continuations: Vec<Continuation>` on
   `GameState` (serde, absent-field-loads test) + the single resume router at the
   top of `resolve_input` (empty stack → falls through to legacy). No frames
   pushed yet.
3. **Window unification (pure refactor):** `open_windows` → `Continuation::Resolution`
   frames (the reaction run: `can_skip=true`). Rewire `top_reaction_window(_index)`,
   `close_reaction_window_at`, `resume_reaction_window`, `open_fast_window`, the
   `apply_player_action` reject-guard, and `check_play_card`'s `permissive_window`
   to operate on the topmost `Resolution` frame. Behavior identical; all existing
   window/skill-test/reaction tests stay green. (This also lands the shared loop
   that task 5's forced run reuses.)
4. **Skill-test frame (pure refactor):** the driver resumes via a
   `Continuation::SkillTest` frame; `in_flight_skill_test` stays as data.
   Behavior identical.
5. **`emit_event` + the forced run + two-phase driving:** introduce `emit_event`;
   add the forced run of the shared loop (`can_skip=false`, `decider=lead`) and
   have `emit_event` drive phase 1 → phase 2; replace the explicit
   `fire_forced_triggers` / event-driven `queue_reaction_window` call sites with
   `emit_event`. **Binding-context audit:** every migrated `Event`
   variant must carry what its old `ForcedTriggerPoint` / `WindowKind` carried;
   enrich the few that don't. Collapse `ForcedTriggerPoint` + the event-driven
   `WindowKind` variants into the event-keyed dispatch (framework `PlayerWindow`
   steps survive). Closes #212/#213; dissolves #294 + the 2+-reject.
6. **#117 index (final):** swap the full-scan for the index behind the scan
   interface; enter/leave-play maintenance + install seed + the defensive
   "index survives a card leaving play mid-window" test. Closes #117.

Tasks 1–4 are behavior-preserving and independently mergeable; 5 is the
semantic change; 6 is the optimization. Each its own work issue + PR.

## Test strategy

Existing content exercises every new path — no new cards needed:

- **Two-phase + simultaneous forced ordering:** agenda 01107's `RoundEnded` doom
  + Dissonant Voices 01165's `RoundEnded` discard (2 simultaneous forced → the
  lead-investigator ordering loop).
- **Reentrancy / suspend-mid-forced:** Frozen in Fear 01164's `EndOfTurn` forced
  effect that suspends on a willpower test — the forced hit suspends, the
  skill-test frame resolves, control returns to (here, finishes) the dispatch.
- **#294 dissolved:** a single attack damaging two `EnemyAttackDamagedSelf`
  reactors (two Guard Dogs) drains both soak windows then resumes — the
  `debug_assert` guard in `drive_attack_loop` is removed/relaxed.
- **Unification anchor:** an enemy defeat fires the forced advance *and* opens
  the reaction window from one `emit_event`.
- **Window unification regression net:** the full existing reaction-window,
  fast-window, mulligan, and skill-test suite stays green across tasks 3–4.

## Closes / corrects

- Closes #212 (emit_event chokepoint), #213 (two-phase iterative ordering; issue
  text already amended to match), #117 (index), #294 (multi-soak resume).
- Removes `fire_forced_triggers`' fixed-order + abandon-on-suspend + 2+-reject;
  the `ForcedTriggerPoint` enum and the event-driven `WindowKind` variants
  collapse into the event-keyed dispatch.

## Out of scope

- Axis A (`ChooseOne` / target-selection choice frames), Axis C
  (reaction-event-play), Axis D (cancellation / Before-firing) — later
  sub-projects. The `Continuation` enum is designed to accept their variants.
- Migrating the orthogonal `pending_*` phase modes onto the stack (later cleanup).
- **Newly-arising forced hits mid-loop** (a forced effect that creates a new
  simultaneous forced trigger at the same point — "delayed effects"): no
  Slice-1+ card does this; the `ForcedOrdering` loop drains a fixed `remaining`
  list, with a `TODO` for merge-in if such a card lands.
- Skill-test nesting (one in flight today; `in_flight_skill_test` stays a
  singleton — `TODO` to move into the frame if nesting ever arrives).

## Risks

- **Window unification (task 3) is the largest single change** — it touches every
  `open_windows` reader. Mitigated by being a pure refactor with a large existing
  regression net; land it on its own PR.
- **Binding-context audit (task 5)** is where a migrated event could silently drop
  context a dispatch needs. Mitigated by per-event tests asserting the fired
  effect sees the right `controller` / `source` / `attacking_enemy`.
- **Index desync (task 6)** — mitigated by landing it last, behind a stable scan
  interface, with the defensive leave-play-mid-window test (and optionally a
  debug-only cross-check against the full scan).
