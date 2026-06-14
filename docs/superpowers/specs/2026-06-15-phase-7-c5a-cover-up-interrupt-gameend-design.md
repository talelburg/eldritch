# C5a — Cover Up before-timing interrupt window + GameEnd forced point

Phase 7, Slice 1, Group C, sub-slice **C5a** ([#236](https://github.com/talelburg/eldritch/issues/236)).
Engine machinery only; the Cover Up card content lands in C5c ([#238](https://github.com/talelburg/eldritch/issues/238)).

## Goal

Build the two engine mechanisms Cover Up 01007 is the first (and, in Slice 1,
only) card to need, **without** referencing card code `01007` anywhere in the
engine — the machinery is data-driven so the real card (C5c) plugs in via the
registry. C5a is verified against a synthetic Cover-Up-shaped fixture.

## Card text (verified against `data/arkhamdb-snapshot/pack/core/core.json:212`)

> **Revelation** - Put Cover Up into play in your threat area, with 3 clues on it.
> [reaction] When you would discover 1 or more clues at your location: Discard that many clues from Cover Up instead.
> **Forced** - When the game ends, if there are any clues on Cover Up: You suffer 1 mental trauma.

Split across the slice:
- The **Revelation** (threat-area placement + 3 clues) is card content → **C5c**.
- The **[reaction]** before-timing replacement interrupt → **C5a** (machinery) + C5c (the ability wiring).
- The **Forced** game-end trauma → **C5a** (the `GameEnd` point) + C5c (the ability wiring).

## Rules grounding (RR, verified)

- **The reaction is optional.** "Triggered abilities on a card a player controls
  are optionally triggered (or not) by that player at the appropriate timing
  moment" (RR p.2). Only a bold "Forced –" command is mandatory. So the engine
  must *offer a choice*, never auto-apply the replacement.
- **"When … would" is a before-timing replacement.** "A reaction ability with a
  triggering condition beginning with the word 'when…' may be used after the
  specified triggering condition initiates, but before its impact upon the game
  state resolves" (RR p.2). Combined with "would discover" + "instead", the
  reaction pre-empts the discovery — it does NOT fit the forced-point dispatcher
  or the existing After-timing reaction window.
- **Eligibility needs game-state potential.** "A triggered ability can only be
  initiated if its effect has the potential to change the game state" (RR p.2):
  with 0 clues on the source card, "discard that many clues from Cover Up" does
  nothing, so the interrupt is not offered.

## Approach (decided)

**Fork 1 — interrupt mechanism: minimal card-local seam, not a general
before-timing reaction-window subsystem.** Cover Up is the only before-timing
replacement interrupt in all of Slice 1, and general trigger-dispatch
unification is explicitly parked in #212 (after Group C). A targeted seam at the
`discover_clue` chokepoint is forward-compatible with #212 and matches the
"don't generalize until 2+ consumers" convention.

**Fork 2 — game-end trauma: emit a trauma event only.** Trauma is a
cross-scenario campaign concept owned by Phase 9 (`state/investigator.rs:16`,
`state/game_state.rs:27` both flag it as not-yet-modeled). In a single Slice-1
scenario it has no persistent home, so C5a emits an observable
`Event::TraumaSuffered` and leaves persistence to Phase 9. No speculative state
field.

**Bespoke effects stay `Effect::Native` (card-local), integration-tested.** The
replacement ("discard that many from self") and the trauma ("suffer 1 mental
trauma, if any clues") are single-consumer card logic, so they live as
`Effect::Native` dispatched by tag — not new typed `Effect`/`Condition`
variants. C5a is verified through an integration test that installs a registry
exposing those natives (the existing `synth_cards::TEST_REGISTRY`,
extended), rather than game-core unit tests (which can't install a registry).
This keeps the DSL surface to trigger *taxonomy* only; no bespoke effect
primitives for one-offs.

## Engine machinery (what C5a builds)

### 1. Clue storage on a card instance
`CardInPlay` (`crates/game-core/src/state/card.rs`) gains `clues: u8`
(`#[serde(default)]`) plus a small accessor. This is the substrate the interrupt
discards from and the forced ability inspects. Distinct from `uses`
(charges/ammo) and from the investigator/location clue pools. C5a only adds the
field + accessor; *placing* 3 clues on entry is Cover Up's Revelation (C5c) /
the fixture.

### 2. Trigger taxonomy (data-driven matching)
Two new `EventPattern` variants in `crates/card-dsl/src/dsl.rs`, each matched
**only** by its dedicated dispatch site (never the general reaction-window
pipeline — `trigger_matches` returns `false` for them, mirroring how
`EndOfTurn`/`AfterLocationInvestigated` are forced-only):
- `WouldDiscoverClues` — paired with `EventTiming::Before`. Cover Up's reaction
  compiles to `OnEvent { pattern: WouldDiscoverClues, timing: Before }`.
- `GameEnd` — Cover Up's forced compiles to `OnEvent { pattern: GameEnd, … }`
  (timing carried for symmetry; the forced point fires it directly).

### 3. The interrupt seam — `discover_clue` (`engine/evaluator.rs:466`)
`discover_clue` is the single chokepoint every discovery routes through (base
Investigate follow-up *and* Deduction's `OnSkillTestResolution` extra clue). The
seam runs **before** the existing mutate step:
1. Scan the discovering investigator's `controlled_card_instances()` (threat
   area + in play) for an `OnEvent { WouldDiscoverClues, Before }` ability whose
   source instance has `clues >= 1` and whose discovery location is the
   controller's location.
2. If eligible: latch `clue_interrupt_pending { location, count, controller,
   source_instance }` on `GameState` and return `EngineOutcome::AwaitingInput`
   with a yes/no prompt (`InputResponse::Confirm` = use the replacement,
   `InputResponse::Skip` = discover normally).
3. If not eligible: fall through to the normal discovery unchanged.

`resolve_input` (`engine/dispatch/mod.rs:335`) routes `clue_interrupt_pending`
as a new mutually-exclusive suspension mode, **before** the skill-test path
(mirroring `pending_end_turn` / `spawn_engage_pending` / `act_round_end_pending`
and their `dispatch/mod.rs` blocking guards):
- `Confirm`: evaluate the interrupt ability's effect (the card-local Native
  "discard that many from self", reading the replaced count from
  `EvalContext.clue_discovery_count`), discovering nothing. Discard is capped at
  `min(count, clues_on_source)`.
- `Skip`: perform the original discovery (the deferred `discover_clue` body).
- Then re-enter `drive_skill_test` to finish the stranded skill-test teardown.

**Reentrancy.** The base-Investigate discovery moves into a resumable
`FinishContinuation` driver step (`engine/dispatch/skill_test.rs`) so the
suspend/resume threads through the existing `in_flight_skill_test.continuation`
state machine rather than stranding mid-`finish_skill_test`.

**Bounded caveat (documented in code).** The interrupt may suspend only where
`discover_clue` is the *terminal* effect of its evaluation context — true for
every Slice-1 discovery source (base Investigate + Deduction's lone extra
clue). A `discover_clue` nested mid-`Seq`/`ForEach` that suspends would strand
the rest of the tree; no such card exists in scope, and full resumable-evaluator
reentrancy is #212.

### 4. `EvalContext.clue_discovery_count`
New field carrying "the count being replaced" so the Native replacement effect
discards "that many." Set by the seam before evaluating the interrupt ability;
mirrors how `EvalContext.failed_by` carries the skill-test margin (C4b).

### 5. `ForcedTriggerPoint::GameEnd`
Fired once from `fire_scenario_resolution` (`engine/mod.rs:184`) on the
`None→Some` resolution latch, after the existing victory-display scan. It scans
**all** investigators' `controlled_card_instances()` for `OnEvent { GameEnd }`
forced abilities. Cover Up's fires the card-local Native "if any clues on self,
emit `Event::TraumaSuffered { investigator, kind: Mental, amount: 1 }`" (the
"if any clues" gate lives inside the Native effect — no `Condition` variant
needed). Non-interactive (no suspension), consistent with the bounded model and
with the existing deterministic simultaneous-forced ordering (C4c).

### 6. `Event::TraumaSuffered`
New event `{ investigator, kind: TraumaKind, amount: u8 }` with
`TraumaKind { Physical, Mental }`. Observable + replay-visible now; persistence
(campaign log, sanity reduction) is Phase 9. No mutation of investigator state.

## Test plan (integration, `crates/scenarios/tests/`)

Extend `synth_cards` with a Cover-Up-shaped fixture card declaring both
abilities, and register its Native effects on `TEST_REGISTRY.native_effect_for`.
Drive through `apply` with the registry installed:

- **Interrupt — Confirm**: investigator with the fixture (N clues on it) at a
  location with clues; investigate succeeds → `AwaitingInput`; `Confirm` →
  location clue count unchanged, investigator gains 0, fixture clues drop by the
  discovered count.
- **Interrupt — Skip**: same setup; `Skip` → normal discovery (location −1,
  investigator +1, fixture clues unchanged).
- **Eligibility**: fixture with 0 clues → no `AwaitingInput`; discovery resolves
  normally.
- **Count coupling**: a 2-clue discovery (base + Deduction-style extra, or a
  2-count discover) → `Confirm` discards 2 from the fixture; cap honored when
  fixture holds fewer.
- **GameEnd — with clues**: latch a resolution with fixture clues remaining →
  `Event::TraumaSuffered { kind: Mental, amount: 1 }` emitted once.
- **GameEnd — no clues**: fixture with 0 clues → no `TraumaSuffered`.
- **Game-core unit tests** for the structural pieces that don't need the
  registry: `CardInPlay.clues` (de)serialization default; the seam's eligibility
  predicate and `Skip`-path fall-through; the new `FinishContinuation` step's
  resume; `GameEnd` firing exactly once on the resolution latch.

Full strict gauntlet (`fmt`, `clippy --all-targets --all-features -D warnings`,
`RUSTFLAGS=-D warnings test`, `doc`, wasm build/clippy) green before push.

## Out of scope (deferred)

- Cover Up's Revelation placement + 3-clue stamping, the real `01007` impl, and
  deck integration — **C5c** (#238).
- Trauma persistence / campaign-log recording — **Phase 9**.
- General before-timing reaction windows, resumable-evaluator reentrancy for
  nested discovery, and player-chosen simultaneous-trigger ordering — **#212 /
  #213**.

## Dependencies

- **C4a** (#233) — threat-area zone + `controlled_card_instances()`: Cover Up's
  home and the scan source the seam and the `GameEnd` point both reuse.
- Slice-1 engine spine (A1/A2): forced-trigger dispatch, `fire_forced_triggers`.

## Decisions made (for the phase doc when the PR lands)

- Before-timing clue-discovery interrupt is a **card-local seam at the
  `discover_clue` chokepoint** with a `clue_interrupt_pending` suspension mode +
  yes/no `AwaitingInput`, not a general before-timing reaction-window subsystem
  (that's #212). Bounded to terminal-position discovery; resume re-enters
  `drive_skill_test`.
- **Bespoke card effects stay `Effect::Native`, integration-tested via
  `synth_cards::TEST_REGISTRY`** rather than promoted to typed `Effect` /
  `Condition` variants — single-consumer one-offs don't earn DSL surface; the
  engine adds only trigger *taxonomy* (`WouldDiscoverClues`, `GameEnd`).
- **Game-end trauma emits `Event::TraumaSuffered` only**; persistence is Phase 9.
