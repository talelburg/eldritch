# Skill-test outcome timing point (Slice D follow-on, #423) — Design

> **Status:** design, awaiting implementation plan.
> **Branch:** `engine/effect-callsite-migration` (continues the Slice D / #423 work,
> after the frame-driven skill-test driver of commit `58cf482`).
> **Relates to:** the #212/#213 trigger-dispatch unification arc — `EventPattern`'s
> doc-comments call the `SuccessfullyInvestigated` / `AfterLocationInvestigated`
> pattern *split* a temporary measure "until then." This change collapses that
> split for the skill-test-outcome moment.

## Problem

When a skill test resolves, the engine emits the **logged** `Event::SkillTestSucceeded`
/ `Event::SkillTestFailed` (the event-sourcing record) — but it does **not** fire a
general timing point that forced/reaction abilities can listen for. The only
outcome-driven timing point today is `TimingEvent::SuccessfullyInvestigated`, a
special case fired **only** for a *successful Investigate*, at the dedicated
`SkillTestStep::EmitSuccessReactions` step.

That special case is the wrong shape. Arkham has a general timing point — "after you
**succeed**/**fail** a skill test" — of which "after you successfully **investigate**"
is one narrowing. The current code models the narrowing as if it were the whole thing.

Two further issues sit in the same code path:

1. **Chaos-symbol side-effects resolve at the wrong step, synchronously.** Today
   `apply_symbol_outcome` runs **all** symbol side-effects at the `Resolving` step,
   *after* the ST.6 determination, via the auto-assigning `take_damage`/`take_horror`
   shortcut. Two problems: (a) unconditional effects belong at **ST.4** (*before* the
   determination) and result-conditional ones at **ST.7** — not bundled post-ST.6;
   (b) applying them synchronously denies the player the soak distribution/window the
   effect is entitled to (a symbol that deals damage to an investigator with a soak
   asset should be interactive). The Tablet token (`immediate: [Damage(1)]` when Ghouls
   are present) deals its damage at the wrong time *and* never offers soak.

2. **The determination and timing-point emission can fold.** The success/fail
   determination (ST.6) and the timing-point emission sit in two adjacent steps
   (`Resolving` → `EmitSuccessReactions`) with nothing suspendable between them — once
   the result-conditional `on_fail` symbol effect is correctly moved to ST.7, they
   collapse into one step that decides, emits, and pre-advances to `FireOnCommit`.

## Rules grounding (verbatim)

Rules Reference p.26–27, *Skill Test Timing* (vendored PDF
`data/rules-reference/ahc01_rules_reference_web.pdf`, pp.26–27):

- **ST.3 Reveal chaos token.** "The investigator performing the skill test reveals one
  chaos token at random from the chaos bag."
- **ST.4 Apply chaos symbol effect(s).** "Apply any effects initiated by the symbol on
  the revealed chaos token. Each of the following symbols indicates that an ability on
  the scenario reference card must initiate: [Skull], [Cultist], [Tablet], or
  [Elder Thing]. … If none of the above symbols are revealed, or if the icon has no
  corresponding ability, this step completes with no effect."
- **ST.5 Determine investigator's modified skill value.** "Start with the base skill …
  and apply all active modifiers, including the appropriate icons that have been
  committed to this test, effects of the chaos token(s) revealed, and all active card
  abilities that are modifying the investigator's skill value."
- **ST.6 Determine success/failure of skill test.** "Compare the investigator's
  modified skill value to the difficulty of the skill test. If the investigator's
  skill value equals or exceeds the difficulty … the investigator succeeds at the
  test. … If the investigator's skill value is less than the difficulty … the
  investigator fails at the test."
- **ST.7 Apply skill test results.** "Resolve the appropriate consequences (based on
  the success or failure established during step ST.6) at this time."

So the canonical order is **ST.4 symbol effect → ST.5 total → ST.6 decide**, exactly
as the user described, and the general "succeeded/failed skill test {kind}" timing
point belongs at the **ST.6→ST.7 boundary** — right after success is established,
before any ST.7 consequence.

The numeric chaos-token *modifier* is applied at ST.5 ("effects of the chaos token(s)
revealed"); the symbol's *initiated non-modifier effects* (damage/horror) are applied
at ST.4. The current code already applies the modifier at ST.5 (correct); only the
non-modifier `immediate` side-effects are mis-ordered.

Card text (ArkhamDB, verified):

- **Dr. Milan Christopher (01033):** "After you successfully investigate: Gain 1
  resource." → reaction, narrowed to `{ outcome: Success, kind: Investigate }`.
- **Obscuring Fog (01168):** "Revelation – Attach to your location. … **Forced** –
  After attached location is successfully investigated: Discard Obscuring Fog." →
  forced, same narrowing; its forced scan must cover the **investigated location's
  attachment zone**.

In-scope symbol tokens (The Gathering, `crates/scenarios/src/the_gathering.rs`):

- **Skull:** `modifier = -(ghoul count)`. No side-effects.
- **Cultist:** `modifier = -1`, `on_fail: [Horror(1)]`.
- **Tablet:** `modifier = -2`, `immediate: [Damage(1)]` when Ghouls are in play.

## Design

### 1. Subsume the special case into one general timing point

Remove the investigate-only pattern/point/event and replace with one general,
kind-and-outcome-parameterized triple. **Unified** shape (single `outcome` field, not
two `Succeeded`/`Failed` variants) — one set of dispatch match arms; `outcome` is just
a field both the forced and reaction paths read.

- **`card_dsl::dsl::EventPattern`** — remove `SuccessfullyInvestigated` and
  `AfterLocationInvestigated`; add:

  ```rust
  /// A skill test resolved with the given outcome. The card-facing narrowing
  /// of the engine's ST.6→ST.7 timing point. `kind: None` matches any test
  /// type; `Some(k)` narrows to that type. Forced (Obscuring Fog 01168) vs
  /// reaction (Dr. Milan 01033) is the `OnEvent { kind }` distinction, not a
  /// pattern distinction — both share this pattern.
  SkillTestResolved {
      outcome: TestOutcome,
      kind: Option<SkillTestKind>,
  },
  ```

  `outcome` is **required**: "after you succeed" and "after you fail" are distinct
  printed triggers; no card reacts to "either." `kind: Option` because the user's
  framing is general ("{skill test type}") and a future "after you fail a Fight test"
  card narrows without an engine change; the two in-scope consumers narrow to
  `Some(Investigate)`.

- **`game_core::engine::dispatch::emit::TimingEvent`** — remove
  `SuccessfullyInvestigated { investigator, location }`; add:

  ```rust
  /// A skill test resolved (ST.6). **Dual** (forced + reaction). The general
  /// timing point of which "after you successfully investigate" is the
  /// {Investigate, Success} narrowing. Carries no location: the forced scan
  /// derives the investigated location from the still-live in-flight SkillTest
  /// frame (`current_skill_test().tested_location`).
  SkillTestResolved {
      investigator: InvestigatorId,
      kind: SkillTestKind,
      outcome: TestOutcome,
  },
  ```

  - `forced_point()` → `Some(ForcedTriggerPoint::SkillTestResolved { investigator, kind, outcome })`.
  - `reaction_bucket()` → `After`.
  - `opens_reaction_window()` → `true`.

- **`game_core::engine::dispatch::forced_triggers::ForcedTriggerPoint`** — remove
  `AfterLocationInvestigated { investigator, location }`; add
  `SkillTestResolved { investigator, kind, outcome }`. In `collect_forced_hits`, the
  new arm scans (matching `EventPattern::SkillTestResolved { outcome: o, kind: k }`
  where `o == outcome && k.is_none_or(|k| k == kind)`):
  1. the investigator's controlled instances (in-play + threat-area), as the
     `AfterLocationInvestigated` arm does today; **and**
  2. the **investigated location's attachment zone** — derived from
     `state.current_skill_test().map(|t| t.tested_location)` (the in-flight `SkillTest`
     frame is still on the stack at ST.6→ST.7; teardown is at `PostOnResolution`).
     This preserves Obscuring Fog's attachment-zone scan with no location field on the
     event. A non-Investigate test at a fog-attached location still scans the
     attachment, but the `kind: Some(Investigate)` pattern won't match — no false
     discard.

### 2. Reaction-scan reroute

In `reaction_windows.rs`, replace the
`(TimingEvent::SuccessfullyInvestigated, EventPattern::SuccessfullyInvestigated) =>
*investigator == controller` arm of `trigger_matches` with:

```rust
(
    TimingEvent::SkillTestResolved { investigator, kind, outcome },
    EventPattern::SkillTestResolved { outcome: p_out, kind: p_kind },
) => *investigator == controller
    && outcome == p_out
    && p_kind.is_none_or(|k| k == *kind),
```

Update the `run_reaction_continuation` window-list match arm (the
`SuccessfullyInvestigated` entry) and any other `SuccessfullyInvestigated` reference to
`SkillTestResolved`.

Both scans already filter by `TriggerKind` (forced: `push_matching` requires
`*kind == TriggerKind::Forced`; reaction: `scan_pending_triggers` skips
`*kind != TriggerKind::Reaction`), so the shared pattern routes Obscuring Fog (Forced)
and Dr. Milan (Reaction) to their correct phases — no cross-firing.

### 3. Frame rework — suspendable symbol effects, folded emit, ST.7 on_fail

**Suspendability is the load-bearing constraint.** Chaos-symbol side-effects must be
applied via pushed `Effect::Deal` (target `You`), not the synchronous
`elimination::take_damage` / `take_horror` shortcut. `Effect::Deal`'s evaluator routes
through `combat::soak_and_distribute` (the *interactive* soak path: a player
distribution choice + the soak reaction window, Guard Dog 01021 / Holy Rosary 01028) —
"the *interactivity* added vs. the K5a `take_damage`/`take_horror`" (evaluator.rs). So a
symbol effect **can suspend** when the tester has a soak asset, and the driver must
cope. The old shortcut (`soak_and_place`, auto-assign, no window, never suspends) is
replaced; routing damage through the interactive path is also RR-correct (the player
assigns damage to soak assets).

**RR step placement (verified):** unconditional symbol effects resolve at **ST.4**;
result-conditional symbol effects ("if this test is failed…") resolve at **ST.7**
(verified against the FFG rulings / ArkhamDB rules reference). This is exactly the
`SymbolOutcome` `immediate` (ST.4) vs `on_fail` (ST.7) split. So:

- **`immediate`** symbol effects (Tablet's `Damage(1)`, conditional only on board state)
  → pushed at **ST.4**, *before* the determination.
- **`on_fail`** symbol effects (Cultist's `Horror(1)`, conditional on the result) →
  pushed at **ST.7**, *after* the timing point, among the test-result effects.

**The token is drawn once and the determination is carried across the yield.** ST.4's
pushed effect may suspend; a naive re-entry would re-draw the chaos token (re-advancing
the recorded RNG → a different token). So `Resolving` draws once, computes the outcome
(pure), and threads it forward; no step re-draws.

Delete `SkillTestStep::EmitSuccessReactions` + `emit_success_reactions_step`. Step
sequence (the two new/changed steps in **bold**):

1. **`Resolving` (ST.3 + ST.4):** draw the chaos token (records RNG); resolve any symbol
   outcome; emit `Event::ChaosTokenRevealed`; compute `succeeded` / `failed_by` /
   `margin` / fail-reason (pure — ST.5 modified value vs difficulty, ST.6 comparison);
   **push the `immediate` symbol effects as `Effect::Deal`**. Pre-advance the cursor to
   the new **`DetermineOutcome`** step *carrying the computed determination*. Return
   `Done` (the pushed `Deal` is the new top frame → the loop drives it, suspending on a
   soak window if present, then re-dispatches this `SkillTest` at `DetermineOutcome`).
   If no `immediate` effect exists, nothing is pushed and the loop falls straight into
   `DetermineOutcome`.
2. **`DetermineOutcome` (ST.6 emit + ST.6→ST.7 timing point):** emit the logged
   `Event::SkillTestSucceeded { margin }` / `SkillTestFailed { reason, by }` — now
   *after* the immediate `DamageTaken` (the ST.4 ordering fix); then **fold in**
   `emit_event(cx, SkillTestResolved { investigator, kind, outcome })`. Pre-advance to
   `FireOnCommit { succeeded, failed_by }` (before emitting — suspend/resume invariant)
   and return the `emit_event` outcome. *(Nothing suspendable sits between the two
   emits, so they fold into one step — the user's simplification, restored now that
   `on_fail` has moved to ST.7.)*
3. **ST.7 sequence (mostly unchanged):** `FireOnCommit → ApplyFollowUp →
   ApplyResultEffect` (card `on_success`/`on_fail`) **+ a new `ApplySymbolOnFail` step
   that pushes the `on_fail` symbol effects as `Effect::Deal`** when the test failed
   `→ FireOnResolution → PostRetaliate → PostOnResolution`. The `ApplySymbolOnFail` slot
   sits among the ST.7 result effects (RR: the test-performer chooses the order of
   multiple results; the engine sequences deterministically — order vs the card
   `on_fail` is not load-bearing in scope, the only pairing being Cultist's lone
   `Horror`). It pushes via `Effect::Deal` so a sanity-soak (Holy Rosary 01028) suspends
   cleanly.

The determination computed at `Resolving` (succeeded / failed_by / margin / fail-reason)
is threaded to `DetermineOutcome` — carried in the step's cursor payload or stashed on
the in-flight `SkillTest` frame (mechanism pinned in the plan; the existing step
variants already thread `succeeded` / `failed_by`).

`apply_symbol_outcome` is replaced by two helpers that build `Effect::Deal`(s) from the
`SymbolOutcome` bundle and `push_effect` them: `push_symbol_immediate` (ST.4) and
`push_symbol_on_fail` (ST.7). `TokenEffect::Damage(n)` → `Effect::Deal { kind: Damage,
target: You, amount: n }`; `TokenEffect::Horror(n)` → `Effect::Deal { kind: Horror, … }`;
multiple effects combine into one `Effect::Seq`.

### 4. Card reroute (content)

- **`dr_milan_christopher.rs`** — `OnEvent { pattern: SkillTestResolved { outcome:
  Success, kind: Some(Investigate) }, timing: After, kind: Reaction }`.
- **`obscuring_fog.rs`** — `OnEvent { pattern: SkillTestResolved { outcome: Success,
  kind: Some(Investigate) }, timing: After, kind: Forced }`.

## Why it's safe (behaviour preservation)

- Firing the general timing point for **every** test is free where no card listens:
  `queue_reaction_window` early-returns on an empty candidate set (no window, no
  prompt), and `collect_forced_hits` returns empty (no forced run). A plain test with
  no listeners is unchanged.
- The logged `SkillTestSucceeded` / `SkillTestFailed` events are still emitted at the
  determination (now from `DetermineOutcome` rather than inline), so every test
  asserting them stays green — *except* for the symbol-effect re-ordering below.
- The only **intended** behaviour changes:
  1. **ST.4 immediate effects** (Tablet's `Damage(1)`) now precede
     `SkillTestSucceeded/Failed` in the event log (was post-ST.6). Tests asserting the
     old order are updated to the RR-correct order.
  2. **ST.7 on_fail effects** (Cultist's `Horror(1)`) now resolve *after* the outcome
     timing point, among the ST.7 results (was immediately post-ST.6, before any ST.7
     step). Tests asserting the old order are updated.
  3. **Interactive soak.** Symbol damage/horror now route through `Effect::Deal` →
     `soak_and_distribute` (player distribution choice + soak window) instead of the
     auto-assigning `take_damage`/`take_horror`. A symbol effect with a soak asset in
     play now opens the distribution/soak interaction (RR-correct) rather than
     silently auto-placing.
  4. Dr. Milan / Obscuring Fog fire via the general pattern instead of the removed
     special-case pattern — their own card tests are the regression net and must stay
     green **without changing the assertions** (same observable effect).

## Regression net (must stay green)

- `crates/cards/src/impls/dr_milan_christopher.rs` (reaction, gain resource).
- `crates/cards/src/impls/obscuring_fog.rs` (forced, location attachment discard).
- `cargo test -p cards --test revelation_treacheries` (Crypt Chill / Grasping Hands
  on_fail suspension through the skill-test driver).
- `crates/game-core` skill-test engine tests (`engine::dispatch::skill_test`). The
  `apply_symbol_outcome_runs_immediate_always_and_on_fail_only_on_failure` test is
  rewritten: `apply_symbol_outcome` is gone, replaced by the `push_symbol_immediate`
  (ST.4) / `push_symbol_on_fail` (ST.7) helpers driven through the real loop.
- The Gathering scenario tests exercising Tablet/Cultist symbol tokens
  (`crates/scenarios/src/the_gathering.rs`).

## New tests

- **ST.4 ordering:** a Tablet draw with a Ghoul in play emits `DamageTaken` *before*
  `SkillTestSucceeded`/`SkillTestFailed` (assert event subsequence).
- **ST.7 on_fail ordering:** a Cultist draw on a *failed* test emits its `Horror`
  *after* the determination and the `SkillTestResolved` timing point; on a *passed*
  test, no `Horror`.
- **Symbol effect suspends on soak:** a Tablet draw (with a Ghoul in play) while the
  tester controls a health-bearing soak asset opens the soak distribution/window
  (`AwaitingInput`); resuming completes the test from `DetermineOutcome` **without
  re-drawing** the chaos token (assert a single `ChaosTokenRevealed`).
- **General timing point fires for a non-Investigate test:** a synthetic in-play card
  with `OnEvent { pattern: SkillTestResolved { outcome: Success, kind: None }, …,
  Reaction }` reacts to a passed *Fight* or *Plain* test (proves the point is no longer
  Investigate-gated). Engine-level test with a fixture registry.
- **No spurious window:** a plain test with no listener opens no reaction window and
  emits no extra prompt.

## Out of scope (deferred)

- Modelling the test-performer's *choice of order* among multiple ST.7 results (RR
  allows it; the engine sequences deterministically). Not load-bearing in scope — the
  only symbol `on_fail` is Cultist's lone `Horror`.
- Any `kind`-narrowing beyond `Investigate` for real cards (none in Core/Dunwich scope
  yet); the `Option<SkillTestKind>` surface is ready for them.

## File map

| File | Change |
|---|---|
| `crates/card-dsl/src/dsl.rs` | `EventPattern`: drop `SuccessfullyInvestigated` + `AfterLocationInvestigated`, add `SkillTestResolved { outcome, kind }` |
| `crates/game-core/src/engine/dispatch/emit.rs` | `TimingEvent`: drop `SuccessfullyInvestigated`, add `SkillTestResolved`; update `forced_point`/`reaction_bucket`/`opens_reaction_window` |
| `crates/game-core/src/engine/dispatch/forced_triggers.rs` | `ForcedTriggerPoint`: drop `AfterLocationInvestigated`, add `SkillTestResolved`; `collect_forced_hits` arm scans controlled instances + `tested_location` attachments |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | `trigger_matches` + `run_reaction_continuation` arms → `SkillTestResolved` |
| `crates/game-core/src/state/game_state.rs` | delete `SkillTestStep::EmitSuccessReactions`; add `DetermineOutcome` (carries the determination) + `ApplySymbolOnFail`; update `SkillTestStep` docs |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | delete `emit_success_reactions_step`; restructure `Resolving` (draw once, compute, push `immediate` symbol `Effect::Deal`, carry determination); add `DetermineOutcome` (emit logged event + folded timing point) and `ApplySymbolOnFail` (ST.7) arms; replace `apply_symbol_outcome` with `push_symbol_immediate`/`push_symbol_on_fail` building `Effect::Deal` |
| `crates/cards/src/impls/dr_milan_christopher.rs` | pattern → `SkillTestResolved` (Reaction) |
| `crates/cards/src/impls/obscuring_fog.rs` | pattern → `SkillTestResolved` (Forced) |
