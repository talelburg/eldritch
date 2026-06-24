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
- **Task 4 — `PlayFromHand`** (sites 2 + 5b in the plan): unified normal+Fast hand-play frame; disposal-by-type (event→discard, asset→enter-play + EnteredPlay + reaction window); reconcile the played-event flush (single `CardDiscarded`). Deserves its own brainstorm (fresh design surface). `complete_play` (`cards.rs`) and `play_fast_event` (`reaction_windows.rs`) still use `apply_effect`.
- **Task 5 — delete the wrappers**: migrate `resume_effect_walk` (`choice.rs`, uses `drive_effect_to_base`) to return Done + let the loop drive; delete `apply_effect` AND `drive_effect_to_base`; rework the ~30 `apply_effect` test calls in `evaluator.rs` + the one in `choice.rs` onto the real `drive`.
- Remaining production `apply_effect` callers (grep to confirm): `encounter.rs` is done; `cards.rs:complete_play`, `reaction_windows.rs:play_fast_event` (Task 4); `choice.rs:resume_effect_walk` via `drive_effect_to_base` (Task 5); the evaluator test calls (Task 5).

**Process notes for the new session:** the user wants per-task review pauses; take non-trivial engine reworks via subagents to save context, but ALWAYS independently re-verify the gauntlet (subagents have repeatedly claimed green while IDE diagnostics showed stale errors — the real build was clean, but verify every time). Fold review-driven fixes into the relevant task's commit (amend). RR rules/cards must be verified (ArkhamDB / the vendored PDF `data/rules-reference/ahc01_rules_reference_web.pdf`), not recalled.

## NEXT IMPROVEMENT — queued (user's words, captured verbatim, not yet done)

> right now, when the chaos token is revealed we just push a success / fail event - that's wrong, what we should do is resolve the token effect, then resolve the total, then decide if it's success or fail - and right then and there we should emit a timing point of "succeeded/failed skill test {skill test type}", which is what those reactions should listen for.

(This refines the Task-3 skill-test driver: introduce a general `Succeeded/Failed skill test {type}` timing point emitted at ST.6 — the general version of `SuccessfullyInvestigated` — that those success/fail reactions listen for. Do this before/with Task 4–5 as the user directs.)
