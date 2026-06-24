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

1. **Chaos-symbol side-effects resolve at the wrong step.** RR ST.4 (*Apply chaos
   symbol effect(s)*) precedes ST.5 (*modified skill value*) and ST.6
   (*success/failure*). Today `apply_symbol_outcome` runs **all** symbol side-effects
   (both unconditional `immediate` and `on_fail`) at the `Resolving` step, *after* the
   ST.6 determination. The Tablet token in The Gathering (`immediate: [Damage(1)]`
   when Ghouls are present) therefore deals its damage *after* `SkillTestSucceeded/Failed`,
   when RR puts it *before* the determination.

2. **An unnecessary driver step.** The success/fail determination (ST.6) and the
   timing-point emission live in two adjacent steps (`Resolving` →
   `EmitSuccessReactions`) with nothing between them. They can be one step: decide,
   pre-advance the cursor to `FireOnCommit`, emit the timing point from there.

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

### 3. Frame rework — fold emission into `Resolving`, delete the extra step

Delete `SkillTestStep::EmitSuccessReactions` (in `state/game_state.rs`), its `advance`
arm, and `emit_success_reactions_step` (in `skill_test.rs`).

`run_resolution` (the `Resolving` arm's body) is restructured to run ST.3–ST.6 in RR
order, then emit, returning `EngineOutcome` (it previously returned `()`):

1. **ST.3** draw the chaos token; resolve any symbol outcome; emit
   `Event::ChaosTokenRevealed`.
2. **ST.4** apply the symbol's **`immediate`** side-effects (Tablet's `Damage(1)`) —
   *before* the total. (New: split out of `apply_symbol_outcome`.)
3. **ST.5** compute the modified skill value (base + icons + constant + pending +
   test modifier + token numeric modifier), clamped per the resolution.
4. **ST.6** determine `succeeded` / `failed_by`; emit the logged
   `Event::SkillTestSucceeded { margin }` or `Event::SkillTestFailed { reason, by }`.
5. apply the symbol's **`on_fail`** side-effects (Cultist's `Horror(1)`) when the test
   failed — *after* ST.6 (they are conditional on the result).
6. **pre-advance** the cursor to `FireOnCommit { succeeded, failed_by }` (the
   suspend/resume invariant: set the resume target *before* emitting).
7. **emit** `emit_event(cx, SkillTestResolved { investigator, kind, outcome })` for
   **every** test, and return its `EngineOutcome`.

The `Resolving` arm then:

```rust
SkillTestStep::Resolving => {
    let outcome = run_resolution(cx, investigator, &indices_u8);
    // A 2+ simultaneous forced run suspends for the lead's ordering choice.
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        return outcome;
    }
    // Otherwise emit_event either pushed a TimingPointWindow (reaction) — the
    // top-of-loop check yields to `drive`, which resolves it and re-dispatches
    // this SkillTest at FireOnCommit — or found nothing and the loop continues
    // straight into FireOnCommit.
}
```

This is the user's structural simplification: "after we calculate everything and decide
on `succeeded`, set the step to on-commit and emit from there — that goes on the stack,
resolves, and the loop re-enters the skill test at the on-commit step."

`apply_symbol_outcome` splits into two helpers (or one fn taking a phase selector):
`apply_symbol_immediate` (ST.4) and `apply_symbol_on_fail` (post-ST.6). Both keep the
existing routing through `elimination::take_damage` / `take_horror`.

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
- The logged `SkillTestSucceeded` / `SkillTestFailed` events are untouched (still
  emitted at the determination), so every test asserting them stays green.
- The only **intended** behaviour changes:
  1. The Tablet token's `immediate` `Damage(1)` now precedes `SkillTestSucceeded/Failed`
     in the event log (ST.4 vs post-ST.6). Scenario/engine tests asserting the old
     order are updated to the RR-correct order.
  2. Dr. Milan / Obscuring Fog fire via the general pattern instead of the removed
     special-case pattern — their own card tests are the regression net and must stay
     green **without changing the assertions** (same observable effect).

## Regression net (must stay green)

- `crates/cards/src/impls/dr_milan_christopher.rs` (reaction, gain resource).
- `crates/cards/src/impls/obscuring_fog.rs` (forced, location attachment discard).
- `cargo test -p cards --test revelation_treacheries` (Crypt Chill / Grasping Hands
  on_fail suspension through the skill-test driver).
- `crates/game-core` skill-test engine tests (`engine::dispatch::skill_test`),
  including `apply_symbol_outcome_runs_immediate_always_and_on_fail_only_on_failure`
  (updated to the immediate/on_fail split).
- The Gathering scenario tests exercising Tablet/Cultist symbol tokens
  (`crates/scenarios/src/the_gathering.rs`).

## New tests

- **ST.4 ordering:** a Tablet draw with a Ghoul in play emits `DamageTaken` *before*
  `SkillTestSucceeded`/`SkillTestFailed` (assert event subsequence).
- **on_fail ordering:** a Cultist draw on a *failed* test emits its `Horror` *after*
  the determination; on a *passed* test, no `Horror`.
- **General timing point fires for a non-Investigate test:** a synthetic in-play card
  with `OnEvent { pattern: SkillTestResolved { outcome: Success, kind: None }, …,
  Reaction }` reacts to a passed *Fight* or *Plain* test (proves the point is no longer
  Investigate-gated). Engine-level test with a fixture registry.
- **No spurious window:** a plain test with no listener opens no reaction window and
  emits no extra prompt.

## Out of scope (deferred)

- Re-sequencing symbol *immediate* effects that could themselves suspend or open
  reaction windows (current routing is synchronous `take_damage`/`take_horror`); the
  edge case of an `immediate` effect eliminating the tester mid-test is pre-existing
  and unchanged.
- Any `kind`-narrowing beyond `Investigate` for real cards (none in Core/Dunwich scope
  yet); the `Option<SkillTestKind>` surface is ready for them.

## File map

| File | Change |
|---|---|
| `crates/card-dsl/src/dsl.rs` | `EventPattern`: drop `SuccessfullyInvestigated` + `AfterLocationInvestigated`, add `SkillTestResolved { outcome, kind }` |
| `crates/game-core/src/engine/dispatch/emit.rs` | `TimingEvent`: drop `SuccessfullyInvestigated`, add `SkillTestResolved`; update `forced_point`/`reaction_bucket`/`opens_reaction_window` |
| `crates/game-core/src/engine/dispatch/forced_triggers.rs` | `ForcedTriggerPoint`: drop `AfterLocationInvestigated`, add `SkillTestResolved`; `collect_forced_hits` arm scans controlled instances + `tested_location` attachments |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | `trigger_matches` + `run_reaction_continuation` arms → `SkillTestResolved` |
| `crates/game-core/src/state/game_state.rs` | delete `SkillTestStep::EmitSuccessReactions`; update `SkillTestStep` docs |
| `crates/game-core/src/engine/dispatch/skill_test.rs` | delete `emit_success_reactions_step`; restructure `run_resolution` (ST.3–6 order, emit timing point, return outcome); `Resolving` arm; split `apply_symbol_outcome` |
| `crates/cards/src/impls/dr_milan_christopher.rs` | pattern → `SkillTestResolved` (Reaction) |
| `crates/cards/src/impls/obscuring_fog.rs` | pattern → `SkillTestResolved` (Forced) |
