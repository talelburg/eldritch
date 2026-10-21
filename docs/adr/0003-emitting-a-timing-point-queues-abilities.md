# Emitting a timing point queues its abilities; it does not resolve them

`queue_event` is the single dispatch chokepoint for the forced and reaction abilities at a framework timing point, and *firing* an ability means *pushing a frame*: `initiate` pushes the ability's effect frame and returns that effect, `push_reaction_window` pushes a `TimingPointWindow`, and 2+ simultaneous forced hits push a lead-ordered run. Nothing is evaluated synchronously, so `EngineOutcome::Done` from an emit means **queued**, not **resolved**. The decision recorded here is the contract that follows: a caller with work to do after the emit must put that work on a resumption frame and emit in tail position. Checking the returned outcome is not a substitute and never was.

This needs recording because four call sites read `Done` as "nothing happened" and ran their tails synchronously, pushing them *above* the abilities they had just queued. The worst of them, `enemy_phase_end`, pushed the Upkeep phase anchor over agenda 01107's *"Forced – At the end of the enemy phase: Each unengaged [[Ghoul]] enemy moves 1 location towards the Parlor"* — and because phase anchors pop-and-push rather than drain, that frame was orphaned at the bottom of the stack for the remainder of the scenario. The Gathering's agenda-3 Ghoul movement never happened in real play, and the round-end doom counted pre-move positions. Every one of those sites carried a comment asserting a loud guard against exactly this; no such guard has existed under the frame model, and the comments were what persuaded each successive author that the site was safe.

## Considered options

**Restoring synchronous resolution for the 0/1-hit cases** — evaluate the effect inline and only push a frame when it suspends — was rejected because it reintroduces two resolution models with different ordering semantics, and the frame model is what makes suspension across an `apply()` boundary work at all. The bug is not that abilities are frames; it is that four sites believed they weren't.

**A required tail parameter** (`queue_event(cx, event, Tail::None | Tail::ResumesOnFrame)`) would make the question unskippable at every call site, and was rejected on proportionality: twelve of the seventeen emit sites already pre-advance a cursor, park beneath the window, or emit in tail position, and would pay a permanent readability cost for the four that didn't.

**Comment-only** was rejected on the evidence: the comments *were* the failed mechanism.

## Consequences

`emit_event` becomes `queue_event` and `fire_forced_triggers` becomes `queue_forced_triggers`, both `#[must_use]`. "Fire" is the more subtle liar of the two — it reads as "resolve now" and appears in the doc-comments the broken sites were written against — so a half-rename would leave the trap in place.

A debug assertion on `ContinuationStack::push` backstops the class: pushing a phase anchor while any queued ability frame is on the stack panics. It names the offending frame, costs nothing in release, and fires at the push that makes the mistake — reporting the caller's location — rather than several phases later when the symptom appears.

A site that pre-advances its cursor is correct by construction, and asserting on its emit's outcome asserts the wrong invariant. `damage_enemy`'s `EnemyDefeated` emit is the case: `apply_follow_up_step` pre-advances the `SkillTest` cursor before the follow-up pushes anything, and `advance` yields whenever the `SkillTest` is no longer the top frame, so a `debug_assert!(matches!(forced, Done))` there would panic in debug on a legitimate 2+ ordering run. The site discards the outcome instead.

*Folded: #928, #962.*
