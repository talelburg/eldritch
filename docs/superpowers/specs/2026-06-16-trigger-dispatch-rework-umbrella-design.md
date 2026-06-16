# Trigger-dispatch rework (#212 / #213 / #117) — umbrella architecture

**Status:** design approved (umbrella). Foundation (Axis B) deep-dive is the
next spec; Axes A/C/D and the orthogonal card prereqs (Axis E) follow.

**Scope of this doc:** the *shared abstractions* across the four trigger/choice
subsystems, so they stay coherent as each ships in its own spec. It is not the
Axis-B implementation plan (that is a separate spec + plan).

## Why this exists

Phase-7 Slice 1 shipped every Gathering card implementable on today's engine and
carved the rest to follow-ups. A cluster of those follow-ups is blocked on the
same missing machinery — the work issues #212 (`emit_event` dispatch
unification), #213 (iterative simultaneous-trigger ordering), and #117 (trigger
index) describe. The phase doc files this under "Engine north-star
(cross-slice, may be its own slice)."

The deferred cards, with verbatim text (verified against
`data/arkhamdb-snapshot/pack/core/core.json`):

| Code | Card | Text |
|---|---|---|
| 01022 | Evidence! | Fast. Play after you defeat an enemy. / Discover 1 clue at your location. |
| 01023 | Dodge | Fast. Play when an enemy attacks an investigator at your location. / Cancel that attack. |
| 01024 | Dynamite Blast | Choose either your location or a connecting location. Deal 3 damage to each enemy and to each investigator at the chosen location. |
| 01018 | Beat Cop | You get +1 [combat]. / [fast] Discard Beat Cop: Deal 1 damage to an enemy at your location. |
| 01019 | First Aid | Uses (3 supplies). If First Aid has no supplies, discard it. / [action] Spend 1 supply: Heal 1 damage or horror from an investigator at your location. |
| 01035 | Medical Texts | [action] Choose an investigator at your location and test [intellect] (2). If you succeed, heal 1 damage from that investigator. If you fail, deal 1 damage to that investigator. |
| 01031 | Old Book of Lore | [action] Exhaust Old Book of Lore: Choose an investigator at your location. That investigator searches the top 3 cards of his or her deck for a card, draws it, and shuffles the remaining cards into his or her deck. |
| 01032 | Research Librarian | [reaction] After Research Librarian enters play: Search your deck for a [[Tome]] asset and add it to your hand. Shuffle your deck. |
| 01036 | Mind over Matter | Fast. Play only during your turn. / Until the end of the round, you may use your [intellect] in place of your [combat] and [agility]. |
| 01038 | Barricade | Attach to your location. / Non-[[Elite]] enemies cannot move into attached location. / **Forced** - When an investigator leaves attached location: Discard Barricade. |
| 01086 | Knife | [action]: Fight. You get +1 [combat] for this attack. / [action] Discard Knife: Fight. You get +2 [combat] for this attack. This attack deals +1 damage. |
| 01087 | Flashlight | Uses (3 supplies). / [action] Spend 1 supply: Investigate. Your location gets -2 shroud for this investigation. |

## The five axes the cards need

- **Axis A — interactive choice.** `Effect::ChooseOne`,
  `LocationTarget::ChosenByController`, `InvestigatorTarget::ChosenByController`
  are `AwaitingInput` stubs in `evaluator.rs`. Dynamite Blast, Beat Cop (2+
  targets), First Aid, Old Book of Lore, Medical Texts.
- **Axis B — trigger-dispatch spine.** #212 + #117 + #213. No new cards but
  correctness-critical; the foundation everything else builds on.
- **Axis C — reaction-event-play.** A Fast event played from hand inside a
  reaction window, gated by a play-timing predicate. Evidence!, Dodge, Mind over
  Matter (play half).
- **Axis D — attack cancellation.** A new cancel/replacement subsystem. Dodge.
- **Axis E — orthogonal per-card primitives.** Not trigger work; separate
  prereqs in the #295/#276 mold. `Effect::Heal`, deck-search, `Effect::Investigate`,
  discard-self cost, stat-substitution, location attachment, AoE damage.

## Current architecture (what the rework changes)

Established by reading the engine, not the issues:

- **`apply(state, action) -> ApplyResult`** is the only mutator. `Cx { state,
  events }` is threaded through handlers; `EvalContext` carries the card-text
  "you"/"source". Outcome is `Done | AwaitingInput { request, resume_token } |
  Rejected`.
- **Two parallel trigger-dispatch paths.** Forced abilities fire immediately via
  `fire_forced_triggers(cx, ForcedTriggerPoint)` (collect-then-resolve, fixed
  deterministic order, **abandons later hits on suspend** — its own doc-comment
  flags this as the #212 reentrancy gap). Optional reactions open a window via
  `queue_reaction_window(cx, WindowKind)` (player-driven pick/skip). Today
  forced-vs-reaction is routed **by `EventPattern`** (the C6a workaround for "the
  engine has no `Trigger::Forced`"), which is why `AfterLocationInvestigated`
  (forced) and `SuccessfullyInvestigated` (reaction) had to be invented as twin
  patterns for the same game moment.
- **Event emission is `cx.events.push(...)` at ~many sites**, with no dispatch
  hook — the manual-wiring smell #212 names.
- **~9 hand-wired suspension modes.** Each is a `pending_*` field on `GameState`
  (`in_flight_skill_test`, `clue_interrupt_pending`, `hunter_move_pending`,
  `spawn_engage_pending`, `hand_size_discard_pending`, `act_round_end_pending`,
  `open_windows`, `pending_end_turn`, `pending_enemy_attack`) wired three times:
  the field, a reject-guard in `apply_player_action`, and a route in
  `resolve_input`. The evaluator's `Seq` loop documents it **cannot resume
  mid-`Seq`** once `ChooseOne` starts returning `AwaitingInput`.

The single missing primitive under all of it: **suspend and resume from the
*middle* of a computation, with nesting.** Both the forced path's
abandon-on-suspend and the evaluator's can't-resume-mid-`Seq` are the same gap.

## Rules grounding (verified against `data/rules-reference/ahc01_rules_reference_web.pdf`)

- **Forced before reaction** (p.2, "Forced Abilities"): *"For any given timing
  point, all forced abilities initiated in reference to that timing point must
  resolve before any [reaction] abilities referencing the same timing point in
  the same manner may be initiated."*
- **Simultaneous forced ordering** (p.17, "Priority of Simultaneous
  Resolution"): *"If two or more forced abilities (including delayed effects)
  would resolve at the same time, the lead investigator determines the order in
  which the abilities resolve."* And: *"If an effect affects multiple players
  simultaneously, but the players must individually make choices to resolve the
  effect, these choices are made in player order."*
- **When vs after** (p.2): a forced/reaction ability with a "when…" timing point
  initiates *before* the triggering condition's impact resolves; an "after…"
  ability *immediately after* the impact has resolved. → maps to
  `EventTiming::{Before, After}`.
- **Fast** (p.11): a fast event *"may be played from a player's hand any time
  its play instructions specify. If the instructions specify when/after a timing
  point, the card may be played **as if the described timing point were a
  triggering condition** … If the instructions specify a duration or period …
  during any player window within that period."* Free 󲅺 abilities: *"may be used
  during any player window."* Reaction 󲆍 abilities: *"may be used any time its
  triggering condition is met."*

**Correction to issue #213:** the issue proposes "uniform across forced and
optional triggers, with skip available only when every remaining trigger is
optional" — one mixed pool. RR p.2 forbids interleaving an optional reaction
ahead of an unresolved forced at the same point. The correct model is **two-phase
per (timing point, Before/After): forced-first, then reaction.** This satisfies
the issue's acceptance criteria (forced can't be skipped; skip offered only when
all remaining are optional — trivially true once forced have all resolved) while
being rules-accurate, and it matches the engine's *existing* split, so it is
*less* of a rewrite than the issue's "one chokepoint" prose implies. Update #213
to reflect two-phase.

## §1 — Continuation model (decision: unified stack, incremental migration)

A single `Vec<Continuation>` on `GameState` is the one suspend/resume mechanism.
The migration is **incremental**, not big-bang:

- **On the stack now:** trigger-dispatch frames (the #213 ordering loop),
  `ChooseOne` / target-selection choice frames, and the skill-test +
  reaction-window *resumptions* that interleave with them.
- **Stay on their `pending_*` fields (cleanup later, tracked follow-up):**
  mulligan, hand-size discard, hunter-move, act-round-end, enemy-attack-loop,
  end-turn. These never nest under trigger dispatch (different phases), so they
  don't need to be on the stack yet.

**Key reframing that makes this clean:** a `Continuation` is a **resume-handle**
— a typed frame (tag + minimal payload naming which resume fn to call), *not* a
relocation of every feature's working state. A frame can say "resume the
skill-test driver" while `in_flight_skill_test` stays where it is as that
feature's storage. So we migrate *control flow* (one router in `resolve_input`,
the suspend/resume discipline) now; deleting `pending_*` fields is optional later
cleanup, not a prerequisite.

Rejected alternatives:

- **Full unified stack (big-bang).** Migrate all ~9 modes now. Cleaner end state
  (one router, no legacy paths) but rewrites intricate, well-tested
  mulligan/upkeep/hunter/enemy-phase resume code unrelated to triggers, for zero
  functional gain, and locks the `Continuation` shape before the new machinery
  proves it. Against the surgical principle.
- **Per-feature `pending_*` (status quo extended).** Fails the requirement by
  construction: `Option<PendingChoice>` + `Option<PendingTriggerDispatch>` can't
  represent a choice arising while a trigger dispatch is already suspended
  (depth > 1). Deepens the exact smell #212 was filed to kill.

The decision criterion was **"can it represent depth > 1?"** — proven necessary
by the #213 ordering loop (resolving one trigger can emit events that open a
window/choice which must fully resolve before re-presenting siblings) and by
Dynamite Blast (choice → AoE → defeat → after-defeat window → that reaction's
effect is itself a choice; depth 4).

## §2 — `emit_event` chokepoint + two-phase dispatch

1. **`emit_event(cx, event)` is the chokepoint for events that have a matching
   `EventPattern`.** Pure notifications (`CardsDrawn`, `ResourcesGained`,
   `ActionsRemainingChanged`, …) keep `cx.events.push`. Rationale: `emit_event`
   *can suspend* (fire a forced trigger / open a window / hit a choice), so every
   `emit_event` site is a pause point — and not every emission site is a safe
   place to pause (cf. the `unreachable!("window closed while a skill test is in
   flight")` guards in `run_window_continuation`). Keeping the pause surface
   equal to the *listenable* surface (= events with an `EventPattern`) is the
   honest scope. A card can only listen to an `EventPattern` that exists, so
   adding a reaction to a today-notification event requires adding the pattern
   *and* converting that one emit site, coupled — they can't drift apart. This
   satisfies #212's "never forget the window" goal for every currently-listenable
   event without making the whole emission surface suspendable.

2. **Per window-bearing event, dispatch is two-phase, per (Before/After)
   timing** (RR p.2):
   - **Phase 1 — forced:** collect forced hits; if 2+, the lead investigator
     orders them *iteratively* (resolve one → re-collect → repeat, honoring
     delayed effects + state changes). Mandatory, no skip. **This is #213's real
     ordering loop**, replacing `fire_forced_triggers`' fixed order *and* its
     abandon-on-suspend.
   - **Phase 2 — reaction:** open the reaction window; optional reactions + Fast
     plays (Axis C) resolve in player order, repeatedly, skip = pass. Today's
     `queue_reaction_window`, generalized.

3. **Backing store = the #117 event-keyed index** (`TriggerKind → Vec<entry>`,
   maintained at `CardInPlay` enter/leave-play, seeded at registry install),
   replacing the full board walk in both phases. #117's defensive test
   (index survives a card leaving play mid-window) carries over.

4. **forced-vs-optional becomes an explicit per-ability property** on the DSL
   trigger: `Trigger::OnEvent { pattern, timing, kind: TriggerKind::{Forced,
   Reaction} }` (field on the existing variant, not a separate `Trigger::Forced`
   — they share pattern/timing and differ only in mandatory/ordering-phase).
   Retires the C6a route-by-pattern wart; lets one game moment carry both a
   forced and a reaction listener without inventing twin patterns.

5. **Both phases suspend/resume on the continuation stack** (§1). A forced hit
   that suspends (Frozen in Fear's test; a `ChooseOne`) parks a frame and resumes
   its siblings — **#294 (two-Guard-Dog multi-soak) and the `fire_forced_triggers`
   reentrancy caveat both dissolve out of this.**

6. **Before-timing dispatch** (fire around the event, before its impact) is part
   of the `emit_event` *signature* (it carries timing), but the Before-*firing*
   wiring lands with its first consumer (Axis D) — no speculative code. After-
   timing is today's path.

## §3 — the choice / input contract (shared)

1. **`Continuation` frames are typed, not closures** — matching the engine's
   established pattern (`InFlightSkillTest.FinishContinuation`,
   `ClueInterruptPending`). `Continuation` is an enum; each sub-project adds the
   variant(s) it needs; the stack gives them one ordering + one router.

2. **`InputRequest` gains structure.** Today `{ prompt: String }` can't render
   "pick one of these 3 locations." It gains a typed options payload — a
   `Vec<ChoiceOption>` carrying an opaque id + a render label (+ enough
   discriminant for the host to show location/investigator/branch). The frame
   stores the **offered set**, so resume validates "is this id in the set I
   offered" — tighter than today's `PickLocation(any id)` + re-derive.

3. **`InputResponse` taxonomy converges to two selection families** plus a
   pass/binary signal:
   - **`PickSingle`** — one `OptionId` from the offered set. Consolidates today's
     `PickLocation` / `PickInvestigator` / `PickIndex` and the new `ChooseOne`
     branch pick (all are "echo back one id"; the distinction bought only log
     readability, recovered by the structured request + the resolved binding in
     the event log). This makes the reaction window and the choice evaluator
     speak one protocol.
   - **`PickMultiple`** — a `Vec<OptionId>` *subset* of the offered set.
     Constraints (`min`/`max`/`exact`) live in the request/frame, not the
     variant. Adopted for *new* multi-selects now; the legacy
     `CommitCards { indices }` / `DiscardCards { indices }` fold into it
     **when their suspension modes migrate to the stack** (the §1 cleanup pass),
     not as a now-rewrite of working legacy paths.
   - **`Skip` / `Confirm`** stay distinct — "decline / I'm done" and "yes" are
     different intents from "selected zero items"; overloading is a footgun.

4. **One uniform resolve convention for all selections** (`ChooseOne`,
   `*::ChosenByController`): enumerate legal options → **0 ⇒ reject** (or no-op
   under "may") → **1 ⇒ auto-bind** (the Fight / Beat-Cop "single engaged enemy"
   precedent in `check_activate_ability`) → **2+ ⇒ suspend** with a choice frame.
   Solo play auto-resolves the common case; only genuinely-ambiguous choices
   round-trip.

## §4 — how each axis attaches (each its own spec)

- **Axis A (choice)** — evaluator. Strategy: resolve an effect tree in **two
  passes — a pure *planning* pass that grounds all choices/targets (can suspend
  to gather them; re-entrant because it mutates nothing), then an *execution*
  pass over the ground plan (mutates once).** This dissolves the "can't resume
  mid-`Seq`" problem without reifying the whole tree walk, and holds for every
  deferred card (choices are all resolvable before the dependent mutation;
  Medical Texts' post-test heal is already ground once the investigator is
  picked, so existing skill-test suspension handles the middle). Out of scope
  (no deferred card needs it): a choice whose legal options depend on a mutation
  *earlier in the same tree*.

- **Axis C (reaction-event-play)** — the reaction phase of `emit_event` admits a
  **Fast event play from hand** as an offered option alongside "fire a pending
  trigger," and a **play-timing predicate** gates it. Crucially the predicate is
  **the same `EventPattern` match from §2** (RR p.11: a fast event with a
  when/after instruction plays "as if the described timing point were a
  triggering condition"). So a Fast event in hand is offered in a window iff its
  play-instruction pattern matches that window's event → becomes another
  `PickSingle` option. **Correction to carry in:** the current
  `fast_actors: FastActorScope::Any` + its comment ("Fast may be played at any
  player window") conflates reaction windows with player windows; Axis C tightens
  it to the pattern-matched predicate rather than inheriting the blanket.

- **Axis D (cancellation)** — §2's **Before-timing dispatch** plus a
  **cancel/replacement signal**: a fired Before-reaction returns a "cancel"
  result the emitting site honors (skips the impact). The umbrella's only
  obligation is that `emit_event`'s Before-phase can carry a cancel result back
  to the call site; where the cancel window opens and how the parked attack is
  discarded is the Axis-D spec.

## §5 — decomposition, sequencing, card readiness

Five sub-projects, each its own spec → plan → issues → PRs. Axis B is built
first; it blocks the rest. Axes A and C are independent of each other (both need
only B); D needs B (Dodge also needs C). Axis E is mostly independent of all.

**Sub-project 1 — Axis B foundation** (next spec; no new cards,
correctness-complete): continuation stack + single resume router; `emit_event`
two-phase dispatch; #117 index; #213 ordering loop; the `kind: Forced|Reaction`
DSL field; migrate skill-test + reaction-window resumptions onto the stack.
Closes #212/#213/#117; **dissolves #294** and the `fire_forced_triggers`
2+-reject. Testable today against Guard Dog, agenda+Dissonant-Voices
`RoundEnded`, Frozen in Fear.

**Sub-project 2 — Axis A** (choice): per §4.
**Sub-project 3 — Axis C** (reaction-event-play): per §4.
**Sub-project 4 — Axis D** (cancellation): per §4.
**Axis E** — orthogonal card prereqs filed separately (#301/#302/#306/#312/
#313/#319/#320/#322/#323), #295/#276 mold, parallelizable.

**Card readiness matrix** (✓ = required):

| Card | B | A | C | D | Axis-E prereq |
|---|---|---|---|---|---|
| Evidence! 01022 | ✓ | | ✓ | | — (DiscoverClue exists) → earliest shippable |
| Dynamite Blast 01024 | ✓ | ✓ | | | AoE-at-location |
| Beat Cop 01018 | ✓ | ✓¹ | | | discard-self cost |
| First Aid 01019 | ✓ | ✓ | | | Heal + uses-discard |
| Medical Texts 01035 | ✓ | ✓ | | | Heal |
| Old Book of Lore 01031 | ✓ | ✓ | | | deck-search top-N |
| Dodge 01023 | ✓ | | ✓ | ✓ | — |
| Barricade 01038 | ✓² | | | | attachment + movement-block |
| Research Librarian 01032 | ✓³ | ✓¹ | | | deck-search by-trait |
| Mind over Matter 01036 | | | ⁴ | | stat-sub + until-end-of-round |
| Knife 01086 | | | | | discard-self cost — pure Axis E |
| Flashlight 01087 | | | | | Investigate-effect + shroud — pure Axis E |

¹ auto-resolves single target on B alone; A only for the 2+ case. ² B for the
forced-on-leave trigger (new pattern + emit site). ³ B for the after-enters-play
reaction window. ⁴ "during your turn" may already pass the existing play gate —
verify; main blocker is Axis E.

**Takeaway:** the trigger rework (B+A+C+D) directly unblocks Evidence!, Dynamite
Blast, Beat Cop, First Aid, Medical Texts, Old Book of Lore, Dodge, and
Barricade's trigger half. **Knife, Flashlight, and Mind over Matter are really
Axis-E work** that can proceed independently of this effort.

## Open questions (for the per-axis specs, not blocking the umbrella)

- Exact `Continuation` enum variants and the `OptionId` representation — settle
  in the Axis-B spec against real consumers, then legacy migration is mechanical.
- Whether a condition-less Fast asset / free 󲅺 ability is legal inside a
  reaction (vs. framework player) window — not needed by the Axis-C cards
  (Evidence!/Dodge are condition-matched; Mind over Matter is "during your
  turn"); make precise when a card forces it.
- Research Librarian's "find a [[Tome]]" when 2+ Tomes are in deck — does the
  deck-search primitive own the selection, or does it route through Axis A? Decide
  in the deck-search prereq spec.
