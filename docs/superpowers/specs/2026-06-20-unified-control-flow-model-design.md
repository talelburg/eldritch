# Unified continuation-stack control-flow model (#393) — design

Tracking issue: **#393** (`engine`, `needs-design`, `p1-next`). Successor to the
**§1 continuation-stack cleanup** (`2026-06-19-continuation-stack-cleanup-design.md`,
#345 + #348 + #380, shipped via PRs #385–#392). Folds in #347 (token-routing,
relayered) and #384's engine half; subsumes the keystone substrate.

## Why this pass exists

Today the continuation stack holds **suspensions** (`SkillTest`, `Choice`,
reaction `Resolution` windows, `EncounterDraw`, …) while **phase/turn structure**
lives on the native Rust call stack (`step_phase → enemy_phase → …`) plus a
handful of bespoke cursors, and the **open-turn action choice** isn't modelled at
all — the engine opens a Fast window and passively waits for the player to send a
typed `PlayerAction` variant, dispatched through a `match`.

This produces three hand-wired control-flow idioms that the §1 cleanup left
standing:

1. **The guard ladder** — `apply_player_action` (dispatch/mod.rs:61–189) opens
   with ~130 lines of `if matches!(top frame, Pending::X) && !ResolveInput { reject }`
   blocks, one per suspension mode.
2. **The action-variant `match`** (mod.rs:191–231) — passive dispatch on whichever
   typed `PlayerAction` arrives, with no notion of *which* actions are currently
   legal.
3. **`run_window_continuation`** (reaction_windows.rs:966) — a `match kind { PhaseStep::X => run the next chunk of the phase cascade }`. This is the engine's de-facto
   continuation table: "what runs after this window closes" is encoded as
   `WindowKind`-keyed arms threading the synchronous `step_phase` cascade, with
   three loop bookmarks — `enemy_attack_pending`, `pending_end_turn`,
   `pending_enemy_attack` — standing in for resume state.

The §1 spec's own argument — *"the keystone adds suspension modes, so migrate onto
the one stack first rather than building the Nth ad-hoc route on top"* — applies
one level up. The engine's suspension surface is now mature enough to finish the
trajectory: **reify every step of control flow as a frame**, so the main loop
becomes a single rule — *handle the top frame*.

## The model

`Continuation` stays a serializable **enum** (not trait objects). Each frame
answers three questions, expressed as `match`-on-variant dispatch, not trait
methods:

- **`drive`** — run my next chunk: emit straight-line effects, then push a child
  frame / await input / pop. (New: the play-loop half.)
- **`on_child_pop`** — a child just completed; continue from where I parked. (This
  *is* `run_window_continuation`'s per-`WindowKind` logic, relocated onto the
  parent frame.)
- **`awaiting`** — what input am I blocked on → `(Vec<OptionId>, InputResponse variant)`,
  or `None`. (Already exists: `resolve_input`'s top-frame `match`.)

The uniform main loop:

```text
loop {
    match top frame {
        None                                  => bootstrap / terminal only
        Some(f) if f.awaiting().is_some()      => break;  // emit AwaitingInput + token
        Some(f)                                => f.drive(cx);  // may push / pop / await
    }
}
```

During play the stack is **never empty** — the `InvestigatorTurn` frame
re-emits an action choice while the active investigator has actions. The stack is
empty only at bootstrap (the action that pushes the first phase frame) and at the
terminal resolution.

### Why the enum, not a `trait Frame` with `Box<dyn Frame>` stacks

The three operations are trait-shaped, and a `trait Frame` for *behaviour* is
fine — but the **stored stack must be the enum**, because `GameState` derives
`Debug, Clone, PartialEq, Eq, Serialize, Deserialize` and a `Vec<Box<dyn Frame>>`
field satisfies **none** of `Deserialize`, `PartialEq`, `Eq` automatically:

- `Deserialize` is **not object-safe** (`fn deserialize() -> Self`, no `&self`
  receiver), so `dyn Deserialize` cannot exist; deserializing a polymorphic stack
  needs hand-rolled tagged dispatch (`typetag`/`inventory`) — which is, with a
  dependency, exactly the tagged union an enum already is.
- `dyn Frame` is not `Eq`/`PartialEq` (GameState derives `Eq`) and not `Clone`
  without `dyn-clone`.

A trait also models **open extension** (foreign crates defining frames) — the
precise opposite of the kernel/content layering: the frame set is **closed and
kernel-owned**; `cards`/`scenarios` never define control-flow frames. So
`impl Continuation { fn drive(&self) { match self { … } } }` is both the more
correct model and the one that keeps the derives.

### Why frames must serialize at all

Not (today) because we persist the live stack: the server stores **seed state +
action log** and `load()` **replays from seed** (server/src/session.rs), so the
stack is rebuilt by `apply()` during replay, never deserialized from a persisted
live state. The real reasons:

1. **Whole-struct derives.** The stack is a field of `GameState`, which is
   `Serialize + Deserialize + Eq` as a whole; a type's derives are only as
   available as its least-capable field. Hard, today, compile-time constraint.
2. **Transport.** `GameState` is serialized to the web client (server/src/ws.rs,
   web/src/transport.rs).
3. **Snapshot persistence (future, YAGNI).** `load()` is O(n) in game length
   (full action-log replay); the obvious optimization is periodic live-state
   snapshots, which need the (turn-non-empty) stack to round-trip. Keeping frames
   serializable keeps that door open for free.

(The existing `Continuation` doc-comment overstates "for persistence" — corrected
to the above.)

## Granularity: C checkpoint, B end-state, the promotion rule

"Everything is a frame" is the destination; *how finely* is the lever.

- **C (the checkpoint we build):** a step is a frame **iff it can suspend (await
  input) or loop over actors**. Straight-line, non-suspending steps stay
  synchronous *inside* the frame that owns them.
- **B (the named end-state):** *every* step is a frame, including straight-line
  ones; phase drivers become pure sequencers with zero embedded step logic.

C's frame set is a **strict subset** of B's, so C → B is monotonic: never a
restructure, only *extract a straight-line chunk into its own frame* and change
the parent from "run inline" to "push it." B's marginal frames (steps that never
suspend) earn nothing operationally — they never survive an `apply()` boundary, so
C is already fully serializable *at rest* at every boundary; B's payoff is purely
**code uniformity**.

**The promotion rule (why C is not a coin-flip, and how B is reached):**

> A step is a frame iff it can suspend or loop. A straight-line step stays
> synchronous **until content introduces a decision or trigger window there** — at
> which point promoting it is a local extract-into-frame.

So B is reached **content-driven**, one step at a time, each promotion paid for by
a real card. Worked example: Upkeep step 4.4 (`upkeep_draw_and_resource`) is
straight-line today; a card that lets a player *decline* the resource gain (a
per-turn decision) is exactly the trigger that promotes 4.4 to an `AwaitingInput`
frame. (Pattern only — no such card is in the Core+Dunwich corpus; not cited.)

## A. Frame variant set (C-checkpoint additions)

`Continuation` has 11 variants today, all already frames: `Resolution`,
`SkillTest`, `Choice`, `HunterMove`, `SpawnEngage`, `HandSizeDiscard`,
`ActRoundEnd`, `SubstitutionPrompt`, `Mulligan`, `EncounterDraw`, `EncounterCard`.
The C checkpoint adds **the per-phase anchors (four variants) plus two net-new
frames** (`InvestigatorTurn`, `AttackLoop`):

1. **Per-phase anchor variants** — `MythosPhase { resume: MythosResume }`,
   `InvestigationPhase { resume: InvestigationResume }`,
   `EnemyPhase { resume: EnemyResume }`, `UpkeepPhase { resume: UpkeepResume }`
   (four `Continuation` arms, **not** one generic `Phase { phase, resume }`). The
   anchor's `on_child_pop` **is** the relocated `run_window_continuation` logic for
   that phase. Per-phase variants make illegal phase/boundary pairings (e.g.
   `Mythos` + `AfterAttackLoop`) **unrepresentable**; the generic variant's "less
   boilerplate" only holds in its *unsafe* flat-`resume` form (a safe generic
   variant needs nested per-phase enums — strictly more machinery than four thin
   variants). Handlers co-locate with each phase's existing module.

2. **`InvestigatorTurn { who, … }`** (net-new behaviour) — step 2.2.1. `awaiting`
   → the legal-action enumeration as `Vec<OptionId>` + `PickSingle`; re-emits
   while `who` has actions; each chosen action runs as a transient sub-resolution
   (§D), and on its pop the turn frame re-emits the next choice; pops at 0 actions
   / `EndTurn` → the `InvestigationPhase` anchor rotates to the next investigator
   or ends the phase. **Absorbs `pending_end_turn`** (end-turn teardown becomes
   this frame's pop sequence; a skill test opening mid-teardown pushes a `SkillTest`
   above it, and `on_child_pop` finishes teardown).

3. **`AttackLoop { investigator, remaining_attackers, source, phase }`** (net-new,
   the keystone — §D) — step 3.3. A literal lift of `PendingEnemyAttack`. **Absorbs
   both `enemy_attack_pending`** (→ the `EnemyPhase` anchor's per-investigator
   iteration cursor) **and `pending_enemy_attack`** (→ this frame's fields).

All three deleted cursors land as frame state. Nothing else is net-new — every
other framework suspension already is a frame.

## B. Uniform main loop & the deletions

The guard ladder (8 blocks) and the action `match` both die:

- **Guard ladder → one rule.** "Is this action allowed?" is answered by the top
  frame: an `InvestigatorTurn` (or fast window) offering it → allowed; otherwise
  only `ResolveInput` is valid. No `if pending_X { reject }` cascade.
- **`run_window_continuation`'s `match` → per-anchor `on_child_pop`.** Its
  `WindowKind`-keyed arms move onto the relevant `*Phase` anchor.
- **The strand-guards dissolve where they were fragile.** The
  `unreachable!("…would strand the test in the wrong phase")` guards on every
  phase-transitioning window arm (reaction_windows.rs) exist because a synchronous
  phase cascade can't coexist with an in-flight skill test. A `*Phase` anchor sitting
  *beneath* a `SkillTest` frame can't be stranded — it simply resumes when the test
  pops. The guards become cheap asserts (or vanish) exactly at the looping/suspending
  steps; they stay as asserts where already fine.

## C. Per-phase CPS decomposition (under the rule)

| Phase | Step | C verdict |
|---|---|---|
| **Mythos** | 1.1 round+`PhaseStarted`, 1.2 place doom, 1.3 doom threshold | **sync** (anchor; agenda-advance may push a `Choice` child) |
| | 1.4 each investigator draws encounter | **frame** — `EncounterDraw` *(exists)* |
| | post-1.4 `MythosAfterDraws` window | **frame** — `Resolution` *(exists)* |
| | end + → Upkeep | **sync** (anchor `on_child_pop`) |
| **Investigation** | 2.1 `PhaseStarted` + `InvestigationBegins` window | window **frame** *(exists)*; emit **sync** |
| | 2.2 rotate to active investigator | **sync** |
| | `InvestigatorTurnBegins` window | **frame** *(exists)* |
| | **2.2.1 the active investigator's actions** | **frame** — `InvestigatorTurn` **(net-new)** |
| | 2.3 end + → Enemy | **sync** (anchor) |
| **Enemy** | 3.1 `PhaseStarted` | **sync** |
| | 3.2 hunter movement | **frame** — `HunterMove` *(exists)* |
| | **3.3 per-investigator attack loop** | **frame** — `AttackLoop` **(net-new, keystone)** |
| | 3.4 `PhaseEnded` + → Upkeep | **sync** (anchor) |
| **Upkeep** | 4.1 `PhaseStarted` + window | window **frame** *(exists)*; emit **sync** |
| | 4.2 reset actions, 4.3 ready cards | **sync** |
| | 4.4 draw + gain resource | **sync today → frame when content makes it a choice** *(B-promotion example)* |
| | 4.5 hand-size discard | **frame** — `HandSizeDiscard` *(exists)* |
| | act round-end clue spend | **frame** — `ActRoundEnd` *(exists; see §G ordering fix)* |
| | 4.6 `PhaseEnded`/round-end + → Mythos | **sync** (anchor) |

**Net-new surface = `InvestigatorTurn` + `AttackLoop` + the four `*Phase` anchors.**
Everything else that's a frame already exists; the work is relocating
`run_window_continuation` onto the anchors, building the legal-action enumerator,
converting the three cursors, and extending the attack loop to park player actions.

## D. The keystone — `AttackLoop` + mid-action park/resume

**The problem.** AoO fires from 5 sites — cards.rs:258 (play card),
actions.rs:73/121/205/334 (move/investigate/…) — each a *synchronous*
`fire_attacks_of_opportunity(cx, investigator)` returning `()` mid-handler. A
synchronous mid-handler call **can't suspend**, so AoO drops any cancel/soak/
reaction window (combat.rs:559–566 documents this as `TODO(#293)`). Fixing it
requires the action to run as a frame so the AoO has a resume point — *which is why
the keystone is inseparable from `InvestigatorTurn`.*

**Part 1 — `AttackLoop` frame.** `PendingEnemyAttack` becomes the frame's fields
verbatim. `source: EnemyAttackSource` **preserves the RR p.7 non-exhaust rule**
(AoO doesn't exhaust; enemy-phase always does); `phase: AttackLoopPhase`
(`BeforeAttack`/`AfterSoak`) is the sub-cursor; cancel/soak windows are children
pushed above it. `enemy_attack_pending` → the `EnemyPhase` anchor's per-investigator
cursor (the anchor pushes one `AttackLoop` per active investigator).

**Part 2 — mid-action park.** An action runs as a **transient sub-resolution frame**
above `InvestigatorTurn`. At the AoO boundary the action frame **pushes
`AttackLoop { source: AttackOfOpportunity }`** and records its resume point; the
`AttackLoop` runs (suspending on windows, no longer dropping them); on pop, the
action frame's `on_child_pop` runs the **post-AoO chunk** (the primary effect). The
chunk boundary sits exactly where the synchronous call sits today, so AoO-vs-effect
ordering is preserved by construction. Collapses #293 / #379 / #361 / #378 / #143 /
#44 into one attack-loop arc.

**The hardest design constraint — mid-action viability.** Mid-action suspension
means the world can change underneath an action: an AoO can defeat the actor,
discard a needed card, exhaust the source. So the action frame's `on_child_pop`
**re-checks viability before completing** and aborts cleanly if the actor is no
longer `Active` or a primary-effect precondition no longer holds. This is the same
hazard CLAUDE.md notes for `play_card` ("on-play effect that rejects mid-resolution
leaves partial state"), made load-bearing.

The atomicity requirement is **softer than it was**, because `GameState` is
`Clone`/serializable: the apply loop already clears events on `Rejected` and returns
state unchanged, a snapshot-and-rollback safety net that makes per-handler
validate-first a belt-and-suspenders nicety rather than the sole guard against
corruption (this snapshot-ability is also the substrate a future **undo** would ride
— explicitly downstream/out-of-scope). The one subtlety: mid-action abort is **not**
a clean whole-action rollback — the AoO damage and the player's window choices are
real and must persist even if the action's primary effect aborts. So the in-scope
mechanism stays the **re-validation gate** in `on_child_pop`, not a blanket rollback.

Expected the **riskiest slice**; gets its own PR + a test matrix (AoO that defeats
the actor mid-Move; AoO cancelled by Dodge; AoO soaked onto Guard Dog → retaliate).

**Multi-step action parameterization (issue open-Q2, resolved).** Top-level
enumeration = **action + primary target** (move-to-X, fight-enemy-Y, play-card-Z);
everything downstream (the AoO sub-loop, a Fight/Investigate skill test, a
play-card commit/choice window) is a **child frame the action frame pushes**. The
AoO `AttackLoop` is just one such child — structurally identical to how a Fight
already pushes a `SkillTest`. Keeps the open-turn enumeration flat (≈30 options).

## E. Enumerated-action input & the `PlayerAction` surface

The `InvestigatorTurn` frame emits the **legal-action enumeration** as `OptionId`s
and keeps an internal id→action map. Eager enumeration is cheap (board tops out
~30 legal actions; the client needs the list regardless). The enumerator's source
of truth is the existing per-action precondition checks (`check_play_card`,
`check_activate_ability`, move adjacency, fight/evade co-location), which must be
callable in *"is this legal?"* mode, not only *"validate this submitted action."*

Two sequenced sub-checkpoints for the action API:

- **2a (this checkpoint):** typed `PlayerAction::{Move, Investigate, Fight, …}`
  **survive**, accepted iff they match an offered option. The guard ladder still
  dies; the enumeration still ships; existing tests keep working; lands
  incrementally.
- **2b (committed, scheduled — *not* content-triggered):** eliminate the typed
  gameplay variants; all gameplay becomes `ResolveInput(PickSingle(OptionId))`
  against the open-turn frame; id→action map fully internal. Motivated by *player*
  UX (the heart of #205): the client becomes a thin renderer of exactly the
  engine-offered options, one input mechanism instead of "render options *and*
  know how to construct typed actions." Distinct from B's content-driven
  promotions — this one is on the roadmap regardless of any card.

## F. #347 — server-only stale-submit rejection

The engine emits a **deterministic token value** on each `AwaitingInput` (a counter
derived from state — e.g. suspend count / action-log length). The **server** holds
the token from its last broadcast and rejects a client echo that's stale, **at the
network boundary, before `apply()`**. No `token` field on `ResolveInput` or on
frames; `apply()` and the action log stay **token-free** so replay stays bit-for-bit
deterministic (stale-submit rejection is a session/protocol concern, not a replay
concern). The engine's token *emission* is in scope here; the server's *rejection*
is the consuming half (network boundary, outside `game-core`).

## G. Pre-req bug — Upkeep `when → at` round-end ordering

The RR **"At"** entry: *"abilities [using] 'at' … such as 'at the end of the round'
… trigger **in between** any 'when…' abilities and any 'after…' abilities with the
same triggering condition."* So at one timing point: **`when` → `at` → `after`**;
within each, RR **"Forced"** sorts forced-before-reaction *"in the same manner"*
(i.e. within a bucket). For The Gathering:

- Act 01109 "The Barrier" — *"**When** the round ends … may … spend … to advance"* → `when`.
- Agenda 01107 "They're Getting Out!" — *"Forced – **At the end of** the round: Place 1 doom …"* → `at`.

So the act's clue-spend window must open **before** the agenda's doom. Current code
is **inverted**: `upkeep_phase_end` (phases.rs:646) fires `emit_event(RoundEnded)`
(the agenda's `at` forced) and only then, in `upkeep_after_round_ended`, opens the
act's `when` window. Consequential — if the doom advances the agenda (a loss
condition on agenda 3), players per the rules should have gotten their act-advance
window first.

**Action:** fix the ordering directly — **not a 2-line swap but a small rethread**
of two independently-suspendable round-end steps (the `when` act window *and* the
`at` multi-Forced run can each suspend, #213): open the `when` window first; run
the `at` `RoundEnded` Forced + teardown on its resume, or inline when no window
opens. Plus a regression test on agenda-3 + act-2 at round end. Landed as its
**own small bug issue (#395) before** the Upkeep-anchor relocation slice, so the
refactor stays behaviour-preserving. (Shipped: PR for #395; `upkeep_phase_end` →
`upkeep_round_end_at_and_after` → `upkeep_round_end_teardown`.) (Confirmed: no
Gathering-specific FFG ruling exception — the act page has "No faqs yet" and the RR
"At" rule governs.)

## Named end-states (sequenced destinations beyond C)

| Destination | What | Trigger to pursue |
|---|---|---|
| **B** — full frame granularity | every straight-line step a frame | **content-driven** (a card makes a step a decision) |
| **2b** — `PlayerAction` elimination | gameplay → `ResolveInput(OptionId)` only | **committed/scheduled** (UX; §E) |
| **EmitEvent-frame** — when/at/after × forced/reaction | event emission as frames | **committed/scheduled** (3rd checkpoint) |

**EmitEvent-frame detail (3rd checkpoint).** `emit_event` (T5a chokepoint, PR #342,
closing #212) models only the RR p.2 **forced → reaction** axis. The orthogonal
**`when`/`at`/`after`** axis (§G) is still hand-threaded per site — the source of
the Upkeep bug, and the "hand-wiring smell" #212 was filed against. The two RR
rules compose into a **3×2 nested grid**:

```text
when-forced → when-reaction → at-forced → at-reaction → after-forced → after-reaction
```

On the model this is two thin coordinator frames:

- **`EmitEvent { event, bucket }`** — iterates `when → at → after` (the unfinished
  tail of #212).
- **`TimingPoint { event, bucket, sub }`** — for one bucket, runs `forced → reaction`
  (exactly what T5a's `emit_event` does today, made frame-resumable). Its children
  already are/want frames (the `forced` sub-step is #213's iterative lead-ordering
  loop; the `reaction` sub-step is the existing `Resolution` window).

**Correctness caution:** the six cells must be evaluated **in order with eligibility
re-checked at each cell** — a `when` reaction can change whether an `at` forced even
fires — so the grid is *not* pre-computed. The nested frames make "enter each cell
fresh, re-scan" structural. Build on the *proven* model (post-C); `emit_event` is
the highest-blast-radius function in the engine, so it does not ride the C
checkpoint. Re-open #212 (or a successor) scoped to this.

## Bugs surfaced

- **Upkeep `when→at` round-end ordering** (§G) — file + fix before the Upkeep-anchor
  relocation slice.

## Out of scope (explicitly)

- **B promotions, 2b, EmitEvent-frame** — named end-states above; their own slices/
  issues.
- **Undo** — the snapshot-ability discussed in §D is its substrate, but undo itself
  is way downstream.
- **Browser rendering / option metadata (#205, #384 client half)** — the engine
  emits `OptionId`s; tests drive via `OptionId(n)`; what labels/controls/parameters
  the client surfaces is #205 at the capstone.
- **Server-side stale-submit rejection** (§F) — engine emits the token value here;
  the server consumes it at the network boundary.

## Testing strategy

1. **Behaviour-preserving for C.** The whole existing engine + integration suite
   must stay green through 2a — the C checkpoint changes *structure*, not rules
   (except the §G fix, which gets its own regression test). Per-cursor / per-anchor
   conversions are individually green at each PR boundary.
2. **New unit coverage** (`game-core` engine tests, `TestGame` builder +
   event-assertion macros): the uniform main loop (top-frame dispatch; empty-stack
   only at bootstrap/terminal); each `*Phase` anchor's `on_child_pop` chunk
   sequencing; `InvestigatorTurn` re-emission while actions remain and pop at 0 /
   `EndTurn`; the legal-action enumerator (each precondition check in "is-legal"
   mode) against known board states.
3. **Keystone matrix** (§D) — AoO that defeats the actor mid-Move; AoO cancelled by
   Dodge; AoO soaked onto Guard Dog → retaliate; multi-attacker AoO; the
   re-validation-gate abort path.
4. **Integration** (`crates/cards/tests/`) — a real Gathering turn driven entirely
   through the new loop (and, at 2b, entirely through `OptionId`s).

## Sequencing (PR decomposition)

Each step is independently green (mirrors §1's parts 2a–2c cadence):

0. **§G ordering bug** — standalone fix + regression test (pre-req). ✅ shipped
   (PR #396, closes #395).
1. **`*Phase` anchors + uniform main loop** — too large for one PR; decomposes into
   three behaviour-preserving sub-slices (exploration found `apply()` runs one
   action per call with a *synchronous* phase cascade, and `run_window_continuation`
   is a `match WindowKind` whose **6 `PlayerWindow(PhaseStep)` arms** —
   `MythosAfterDraws`, `UpkeepBegins`, `BeforeInvestigatorAttacked`,
   `AfterAllInvestigatorsAttacked`, `InvestigationBegins`, `InvestigatorTurnBegins` —
   are the phase-structure continuations the anchors own; its other arms are
   card/ability reactions that stay put):
   - **1a — anchor frames + relocate the 6 `PhaseStep` arms. ✅ shipped (PR #397).**
     Introduced the four `*Phase` anchor `Continuation` variants + per-phase `resume`
     enums; each phase pushes its anchor at entry (windows/loops push *above* it) and
     pops at its exit (Upkeep at `upkeep_round_end_teardown`, after the round-end
     sequence); the `run_window_continuation` `PlayerWindow` match collapsed to a
     single `PlayerWindow(_) => anchor_on_child_pop` arm — the `PhaseStep` is no
     longer the continuation key, the anchor's `resume` is. Behaviour-preserving
     (review-confirmed faithful). Added `GameStateBuilder::with_phase_anchor` for the
     ~20 tests that construct mid-phase states directly. Guard ladder, action
     `match`, card-reaction arms untouched.
   - **1b — uniform main loop + cascade-fold (merges the former 1b/1c). ✅ shipped
     (PR #398).** On exploration the cascade-fold and the loop proved inseparable — "anchor drive"
     only means something once a loop *invokes* the advance when a child pops, and
     the strand-guard payoff only materializes with loop-driven transitions. So one
     slice: add a `drive(cx)` loop the `apply` entry runs, which **advances the top
     frame** until it (a) hits a suspension awaiting input → `AwaitingInput`, (b)
     reaches the open turn (`InvestigationPhase{TurnBegins}`) → idle `Done`, or (c)
     reaches terminal → `Done`. Each `*Resume` gains an **`Entry`** variant; the
     anchor's resume-keyed `advance` subsumes the phase drivers (`Entry` = today's
     `mythos_phase`/… opening), the boundary chunks (today's `anchor_on_child_pop`),
     and the transitions (pop self + advance `state.phase` + push next anchor
     `{Entry}` — replacing `*_phase_end`'s synchronous `step_phase`). `step_phase`
     and the four `*_phase_end` functions dissolve. The **guard ladder** collapses to
     one rule (top frame is a non-anchor suspension ⇒ only `ResolveInput`); the
     **strand-guards** become genuinely impossible (a skill test sits *above* its
     phase anchor) → `debug_assert!`. Sets up slice 2's `InvestigatorTurn`.
     *Surfaced in review:* the unified guard rule also gates `Choice` /
     `SubstitutionPrompt` frames that the eight-block ladder never covered —
     closing a latent hole where a typed action arriving mid-`Choice` (a
     `ChooseOne` OnPlay) fell through and mutated half-resolved state. A
     `*Phase`-entry follow-up remains (`start_scenario`/`resume_mulligan` still
     call `investigation_phase` directly rather than pushing
     `InvestigationPhase{Entry}`; behaviour-identical, but two conventions).
2. **`InvestigatorTurn` frame (2a) + legal-action enumerator** — open-turn becomes a
   frame; typed `PlayerAction` validated against the offered set; `pending_end_turn`
   absorbed. Splits into two behaviour-preserving sub-slices:
   - **2a-i — the frame + cursor absorption. ✅ shipped (PR #400).** The open turn is a
     `Continuation::InvestigatorTurn { investigator, ending }` pushed above the
     `InvestigationPhase` anchor when the `InvestigatorTurnBegins` window closes; `drive`
     idles on it (`is_open_turn` removed); `resume_end_turn` pops it. `pending_end_turn`
     folded onto the frame's `ending` flag (field removed). Idle outcome stays `Done`
     (typed actions survive; `AwaitingInput` surfacing is 2b). **No `AfterTurn` anchor-
     resume variant needed** — `resume_end_turn` is always called directly (the three
     end-of-turn sites), never re-entered via the anchor's `on_child_pop`, so the anchor
     parks at `TurnBegins` beneath the frame. (Legacy build-then-push test setups for
     other phases tracked in #399.)
   - **2a-ii — the legal-action enumerator** (§E). Read-only `legal_actions(state)
     -> Vec<PlayerAction>` built on shared "is-legal?" predicates, so it matches
     handler-acceptance by construction; nothing routes through it yet (that flip is
     2b). A **cross-check** test (every enumerated action applies without `Rejected`)
     pins the equivalence. Sub-sliced by action group:
     - **2a-ii-1 — scaffold + basic actions. ✅ shipped (PR #402).** EndTurn, Resource,
       Draw, Investigate, Move; extracted a pure `action_cost` out of `charge_action`.
     - **2a-ii-2 — combat/engage. ✅ shipped (PR #405).** Fight per current engaged-only
       handler (#401 widens to co-located later); Evade; Engage incl. enemies engaged
       with others (RR p.11).
     - **2a-ii-3 — play/activate. ✅ shipped (PR #406).** PlayCard, ActivateAbility — by
       delegation to the handlers' `check_play_card`/`check_activate_ability` predicates;
       registry-gated, so tests live in `crates/cards/tests/`.
     - **2a-ii-4 — AdvanceAct + sweep. ✅ shipped (PR #407) — closes 2a-ii.** AdvanceAct via
       an extracted `check_advance_act`; a whole-enumeration sweep pins that every action
       category is enumerated and applies. The enumerator now covers the full open-turn
       surface (EndTurn/Resource/Draw/Investigate/Move/Fight/Evade/Engage/PlayCard/
       ActivateAbility/AdvanceAct); routing typed dispatch through it remains 2b.
3. **`AttackLoop` frame (cursor lift)** — `PendingEnemyAttack` +
   `enemy_attack_pending` → frame/anchor; enemy-phase attacks unchanged in behaviour.
4. **Keystone mid-action park** — actions run as sub-resolution frames; AoO pushes
   `AttackLoop`; re-validation gate; the §D matrix. **Riskiest.**
5. **#347 token emission** (engine half, §F).

Post-checkpoint, separately tracked: **2b**, **EmitEvent-frame** (#212 successor),
**B** promotions (content-driven), **#205/#384 client** (capstone).

## What "done" looks like (C checkpoint)

- The guard ladder, the action-variant `match`, `run_window_continuation`'s
  `WindowKind` table, and all three cursors (`enemy_attack_pending`,
  `pending_end_turn`, `pending_enemy_attack`) are **gone**.
- `apply_player_action` is the uniform main loop; the only "is this allowed?" rule
  is the top frame's offered set.
- AoO opens cancel/soak/reaction windows (Dodge, Guard Dog retaliate work against
  AoO); the keystone matrix passes.
- The engine emits a legal-action enumeration as `OptionId`s; typed `PlayerAction`
  still accepted (2a).
- The §G ordering bug is fixed with a regression test.
- Full CI gauntlet green; the Gathering plays end-to-end through the new loop.
