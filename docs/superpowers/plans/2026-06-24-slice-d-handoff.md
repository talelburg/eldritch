# Slice D (#423) — session handoff

Picking this up in a fresh session. Read this first, then the spec/plan:
- Spec: `docs/superpowers/specs/2026-06-23-effect-frame-callsite-migration-design.md`
- Plan: `docs/superpowers/plans/2026-06-23-effect-frame-callsite-migration.md`

## Where we are

**Branch:** `engine/effect-callsite-migration` (off `main`). Working tree clean. Nothing pushed; #423 still open. Full strict gauntlet green at HEAD.

**Commits so far (Slice D):**
```
58cf482  engine: fully frame-driven skill-test driver (Slice D, #423)        ← Task 3
aca248d  engine: frame-driven encounter disposition + Mythos draw chain      ← Task 2
4ead786  docs: record frame-driven forced-run decision (Slice D, #423)
57b57ff  engine: push_effect helper + frame-driven forced run (Slice D, #423) ← Task 1
```

**Done (Tasks 1–3):**
- **Task 1** — `push_effect` helper added (`evaluator.rs`); push-and-return sites migrated (activate/resume ability, `resolve_one`, `fire_pending_trigger` in-play branch). **Forced run made fully frame-driven**: `fire_forced_triggers` pushes (single-hit; 2+ routes through `open_forced_resolution`); `end_turn` arms the `InvestigatorTurn{ending}` resumption; the terminal `GameEnd` hook + the 8 forced test helpers drive. `drive_effect_to_base`'s last forced-path use removed.
- **Task 2** — `EncounterCard { card, disposition }` (Discard/Spawn) unifies treachery+enemy revelation; Revelation effects pushed for the loop. **Mythos surge chain frame-driven** via a new `PlayerDraw { investigator, chain_count, surge_pending }` frame (the loop frame owns chain state; `EncounterCard` is transient; disposal is pure). Spawn-engage ties simplified. `apply_engine_record` now drives. Removed dead `SpawnEngagePending.{chain_count,surge,investigator_to_draw}` + the `spawn_enemy_at`/`spawn_set_aside_enemy` drawer params.
- **Task 3** — **skill-test driver `advance` fully frame-driven**, cursor-sequenced per RR ST.1–ST.8. `SkillTestStep` now: `Resolving` (ST.3–6 compute) → `EmitSuccessReactions` (ST.6: emit `SuccessfullyInvestigated` after success established, before ST.7) → `FireOnCommit` (ST.7, Vicious Blow 01025) → `ApplyFollowUp` (ST.7 discover_clue) → `ApplyResultEffect` (ST.7 on_success/on_fail) → `FireOnResolution` (ST.7 OnSkillTestResolution) → `PostRetaliate` → `PostOnResolution`. `advance` yields whenever `last()` is not its `SkillTest`. `skill_test.rs` is now `apply_effect`-free (all 5 cluster sites migrated).

**Remaining:**
- **Task 4 — `PlayFromHand`** ✅ DONE (commits `1d07b8a`/`88de537`/`dae286d`; design `docs/superpowers/specs/2026-06-24-play-from-hand-frame-design.md`, plan `docs/superpowers/plans/2026-06-24-play-from-hand-frame.md`). Single-shot `Continuation::PlayFromHand { investigator, code, hand_index }` disposed by `cards::dispose_play_from_hand` (pops, then event→`flush_pending_played_event` / asset→enter-play + `emit_event(EnteredPlay)` — the drive loop opens the after-enters-play window). `complete_play` + `play_fast_event` now push the frame + `push_effect`; the apply-loop flush was removed (the defeat-mid-play suppress path in `resume_action_resolution` flushes the abort case — mutually exclusive with `PlayFromHand`). `apply_effect` is now **test-only** (`#[allow(dead_code)]` in `evaluator.rs`, pending Task 5).
- **Task 5 — delete the wrappers** (the SOLE remaining Slice D task): migrate `resume_effect_walk` (`choice.rs`, uses `drive_effect_to_base`) to return Done + let the loop drive; delete `apply_effect` AND `drive_effect_to_base`; rework the ~30 `apply_effect` test calls in `evaluator.rs` + the one in `choice.rs` onto the real `drive` (and drop the interim `#[allow(dead_code)]`).
- Remaining `apply_effect` / `drive_effect_to_base` callers (grep to confirm): all production sites done EXCEPT `choice.rs::resume_effect_walk` via `drive_effect_to_base` (Task 5); plus the `#[cfg(test)]` `apply_effect` calls in `evaluator.rs` + `choice.rs` (Task 5).

**Process notes for the new session:** the user wants per-task review pauses; take non-trivial engine reworks via subagents to save context, but ALWAYS independently re-verify the gauntlet (subagents have repeatedly claimed green while IDE diagnostics showed stale errors — the real build was clean, but verify every time). Fold review-driven fixes into the relevant task's commit (amend). RR rules/cards must be verified (ArkhamDB / the vendored PDF `data/rules-reference/ahc01_rules_reference_web.pdf`), not recalled.

## NEXT IMPROVEMENT — ✅ DONE (the general skill-test-outcome timing point)

> right now, when the chaos token is revealed we just push a success / fail event - that's wrong, what we should do is resolve the token effect, then resolve the total, then decide if it's success or fail - and right then and there we should emit a timing point of "succeeded/failed skill test {skill test type}", which is what those reactions should listen for.

Shipped as a 4-commit sub-effort (design `docs/superpowers/specs/2026-06-24-skill-test-outcome-timing-point-design.md`, plan `docs/superpowers/plans/2026-06-24-skill-test-outcome-timing-point.md`):

- `1fd83c1` — subsumed `SuccessfullyInvestigated` + `AfterLocationInvestigated` into one general `SkillTestResolved { kind, outcome }` triple (`EventPattern` / `TimingEvent` / `ForcedTriggerPoint`); forced collector derives the investigated location from the in-flight frame's `tested_location` (lean, location-free event). Dr. Milan 01033 / Obscuring Fog 01168 rerouted to the `{ Success, Some(Investigate) }` narrowing. Behaviour-preserving.
- `a3cb506` — generalized the emission to fire for **every** test/outcome (renamed `EmitSuccessReactions` → step then folded; empty candidate sets open no window).
- `c76ddef` — moved the test outcome onto the frame: `InFlightSkillTest.resolved: Option<ResolvedTest>`, set once at ST.6, read by every post-ST.6 step; the `SkillTestStep` variants dropped their `succeeded`/`failed_by` payloads (only `FireOnResolution { next }` keeps state). Invariant: `resolved.is_some()` ⇔ past the commit window.
- `272d9ee` — chaos-symbol side-effects now apply via pushed `Effect::Deal` (interactive `soak_and_distribute`, can suspend): `immediate` at **ST.4** (before the determination), result-conditional `on_fail` at **ST.7** (after the outcome timing point). New steps `DetermineOutcome` (logged events + `SkillTestResolved`, folded) and `ApplySymbolOnFail`; `EmitSuccessReactions`/`emit_success_reactions_step`/`resolve_chaos_token_and_emit`/`apply_symbol_outcome` deleted. Token drawn once at `Resolving`; a soak suspend resumes at `DetermineOutcome` without re-drawing.

Tests: `crates/game-core/tests/skill_test_outcome_timing.rs` (fires for a Plain test; no spurious window) + `crates/scenarios/tests/the_gathering_symbols.rs` (ST.4 ordering, ST.7 ordering, real Guard-Dog soak-suspend proving a single `ChaosTokenRevealed`).

**Branch is NOT done — original Slice D Tasks 4 + 5 (above) still remain** (`PlayFromHand`, then delete `apply_effect`/`drive_effect_to_base`). Resume there. Note: the Task-3 step list above is now historical — `EmitSuccessReactions` → `DetermineOutcome`, and `ApplySymbolOnFail` was inserted before `FireOnResolution`.
