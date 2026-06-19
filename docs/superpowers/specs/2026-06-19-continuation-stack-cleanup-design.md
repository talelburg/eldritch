# §1 continuation-stack cleanup — design

**Date:** 2026-06-19
**Issues:** #348 (umbrella) · #345 (serializable `EvalContext`) · #347 (token-routed resume) · #380 (revelation disposal)
**Phase:** 7 — "the solo correctness gate", ordering step 2 (DO-FIRST, before the keystone)

## Why this pass exists

The trigger-dispatch umbrella (§1) decided on **one `Vec<Continuation>` suspend/resume
stack**, migrated incrementally. Axis B put `Resolution` + `SkillTest` on it; Axis A added
`Choice`. The remaining suspension modes still live as their own `Option<…>` fields on
`GameState`, each wired three times (the field, a reject-guard in `apply_player_action`, a
route in the `resolve_input` cascade). The cascade carries fragile hand-ordered priority
reasoning ("X is an inner suspension of the test, route before Y") that grows per mode.

The **keystone** (mid-action suspend/resume for attacks of opportunity, retaliate, soak, and
attack-order — Phase 7 ordering step 3) *adds* suspension modes to the attack loop. Rather
than build the Nth ad-hoc route on top of the mixed model, this pass migrates the existing
modes onto the one stack first, so the keystone rides one clean stack.

These four issues share the same seam and are designed together:

- **#348** migrates the remaining suspension modes onto `Continuation` frames and collapses
  both the `resolve_input` cascade and the `apply_player_action` guard ladder into a single
  dispatch on the top frame.
- **#345** makes `EvalContext` serializable with grouped bindings, so migrated frames
  snapshot context instead of re-storing ad-hoc ingredient tuples.
- **#347** makes resume **token-routed**: deterministic counter, stamped on the awaiting
  frame, echoed at resume → stale-submit rejection + direct frame lookup.
- **#380** removes the `pending_revelation_discard` side-channel by making encounter-card
  resolution a continuation frame whose framework teardown disposes of the card.

Motivated partly by Phase 8 (multiplayer) robustness and the Phase 7 browser surface
(#205, token-routing gives clean stale-submit rejection); no single-player *behavior*
depends on it today — this is a correctness-preserving structural pass.

## The unifying model

**The continuation stack is the single record of every suspended process.** Each frame
declares how it resumes. Both ladders die: `apply_player_action`'s reject-guards and
`resolve_input`'s priority cascade collapse into **one dispatch on the top frame**.

The player-facing resume signal **unifies onto `ResolveInput`**, and the `InputResponse`
channel is **normalized** as part of the pass: `CommitCards`/`DiscardCards` collapse into
`PickMultiple` and `PickLocation`/`PickInvestigator` into `PickSingle` (the structured-options
consolidation — these land here, in #348 part 2c-i/2c-ii; only the *human labels / client
rendering* defer to #205, not the variant consolidation). On that normalized channel,
`PlayerAction::Mulligan` and `PlayerAction::DrawEncounterCard` then fold into
`PickMultiple` / `Confirm` and are **removed** as standalone actions (part 2c-iii). Setup
emits `AwaitingInput { mulligan for X }` per investigator; Mythos step 1.4 emits
`AwaitingInput { encounter draw for X }` per drawer.

Two precision points:

- "Everything is `ResolveInput`" governs the **resume/advance** signal. Fast-permitting
  windows (reaction / player windows) still *additionally* admit `PlayCard` /
  `ActivateAbility` for Fast cards — that dual intake stays.
- Pure **internal-sequencing** frames (the Enemy-phase attack-loop *cursor* that advances
  across investigators, driven to completion by its child windows closing) do **not** emit
  `AwaitingInput` — they are not player-facing.

The through-line for the whole pass: **co-locate state with the process it belongs to;
delete global side-channels.** Folding `in_flight_skill_test` onto its frame, snapshotting
bindings onto frames (#345), and making encounter-card disposal a frame teardown (#380) are
the same move.

## A. `Continuation` variants after migration (#348)

Existing: `Resolution(ResolutionFrame)`, `Choice(ChoiceFrame)`, and `SkillTest`. Changes:

- **Fold `in_flight_skill_test` onto its frame**: `SkillTest(InFlightSkillTest)` carries the
  payload; the separate `GameState::in_flight_skill_test: Option<…>` field is removed. The
  many call sites that read "the current test" use an accessor
  `cx.state.current_skill_test()` that finds the (unique — no nesting today) `SkillTest`
  frame *wherever it is on the stack* (a reaction window can sit above it mid-resolution, so
  it is **not** `continuations.last()`). Innermost-wins if same-kind nesting ever appears.
  *(This goes beyond #348's literal "`pending_*` fields" wording, but is the same
  one-source-of-truth principle and removes a push-frame/set-field desync pair.)*

- **Migrate the suspension modes** from `Option<…>` fields to typed frames carrying their
  loop / payload state:

  | Removed field | New `Continuation` variant |
  |---|---|
  | `mulligan_pending: Option<InvestigatorId>` | `Mulligan { remaining: Vec<InvestigatorId> }` |
  | `mythos_draw_pending: Option<InvestigatorId>` | `EncounterDraw { remaining: Vec<InvestigatorId> }` |
  | `hunter_move_pending: Option<HunterChoice>` | `HunterMove(HunterChoice)` |
  | `spawn_engage_pending: Option<SpawnEngagePending>` | `SpawnEngage(SpawnEngagePending)` |
  | `hand_size_discard_pending: Option<HandSizeDiscard>` | `HandSizeDiscard(HandSizeDiscard)` |
  | `act_round_end_pending: Option<ActRoundEndPending>` | `ActRoundEnd(ActRoundEndPending)` |
  | `pending_substitution_prompt: Option<InvestigatorId>` | `SubstitutionPrompt { investigator }` |
  | `pending_enemy_attack: Option<PendingEnemyAttack>` | `EnemyAttack(PendingEnemyAttack)` |
  | `pending_end_turn: Option<InvestigatorId>` | `EndTurn { investigator }` |

  Loop modes (`Mulligan`, `EncounterDraw`) hold their `remaining` actor queue on the frame —
  the head is the current actor; resume advances it; the frame pops when the queue drains.

- **Stays a non-frame cursor:** `enemy_attack_pending` (the Enemy-phase 3.3 cursor across
  investigators) is internal sequencing, not a player-facing input frame. It may move onto
  the stack as a non-`AwaitingInput` driver frame when the keystone wants it; **out of scope
  here**.

- **Already retired:** `clue_interrupt_pending` (named in #348) is already a `Resolution`
  window (Axis D) — nothing to do.

## B. Router collapse (#348)

Every frame variant gets a resume handler. The two ladders become one check each:

```rust
// apply_player_action: one guard
if let Some(top) = cx.state.continuations.last() {
    if !top.accepts(action) {            // ResolveInput always; + Fast plays for permissive windows
        return Rejected { reason: top.expected_input_msg() };
    }
}

// resolve_input: one dispatch (after token validation, §C)
match top_frame_after_token_check(cx, token)? {
    Continuation::Mulligan(_)       => resume_mulligan(cx, response),
    Continuation::EncounterDraw(_)  => resume_encounter_draw(cx, response),
    Continuation::HunterMove(_)     => resume_hunter_choice(cx, response),
    // … one arm per variant …
    Continuation::SkillTest(_)      => resume_skill_test_commit(cx, response),
}
```

Routing order becomes **structural** (top of stack), retiring the hand-ordered
`if pending_X.is_some()` priority cascade and its priority-reasoning comments. The
`mutually-exclusive suspension modes` `debug_assert` is subsumed (frames stack rather than
race for a shared slot).

## C. Token-routed resume (#347, end-state **(b)**)

- `GameState` gains `next_resume_token: u64` — a **deterministic counter** (NOT a nonce), so
  replay from the action log reproduces tokens bit-for-bit.
- Each suspend mints the next token and **stamps it on the awaiting frame**
  (`ResolutionFrame` / `ChoiceFrame` / `SkillTest` / the new variants gain a `token`), and
  returns it in `AwaitingInput { resume_token }`.
- `PlayerAction::ResolveInput` gains `token: ResumeToken`; the host echoes the one it
  received. **(Wire + replay-contract change.)**
- `resolve_input` validates `submitted == top frame's token`; a mismatch rejects as
  **stale** before dispatching on the frame variant.

The token does double duty: it is the recorded "this frame is awaiting input" pointer
**and** a staleness check. This replaces the **resume-path** re-derivation that today scans
the stack top-down (`top_reaction_window` / `_index`) for the topmost frame with non-empty
`pending_triggers`. Other uses of the window helpers (the open-window guard; locating where
to enqueue a newly-triggered reaction) are separate concerns and may keep a window lookup —
token-routing does **not** retire `top_reaction_window` wholesale.

`ResumeToken` stops being vestigial (`pub(crate) u64`, currently always `ResumeToken(0)`).

## D. Serializable `EvalContext` with grouped bindings (#345)

`EvalContext` today is `controller` + `source` followed by a flat set of `Option<_>`
window-bound fields, derives `Copy` only (not `Serialize`). Two problems: (A) illegal states
are representable (e.g. `failed_by` + `attacking_enemy` coexisting, when they belong to
disjoint windows — the invariant lives only in doc-comments); (B) not serializable, so
suspended frames store ingredient tuples (`controller` + `source`) and rebuild — each new
frame type re-implements the rebuild, and the rebuild **silently drops window-bound
bindings** (a latent cross-suspend bug).

**Decision: grouped optional bindings, snapshotted per-frame.**

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EvalContext {
    // durable identity
    pub controller: InvestigatorId,
    pub source: Option<CardInstanceId>,
    // window bindings — each cohesive, each optional
    pub skill_test:   Option<SkillTestBinding>,   // { failed_by }
    pub discovery:    Option<DiscoveryBinding>,    // { clue_discovery_count }
    pub enemy_attack: Option<EnemyAttackBinding>,  // { attacking_enemy }
    pub choice:       Option<ChoiceBinding>,       // { investigator, location, enemy, option }
}
```

Frames **snapshot the whole `EvalContext`** instead of storing `controller` + `source`
tuples. This fixes (B) — zero per-frame rebuild code — *and* the latent cross-suspend
binding-loss (bindings survive the suspend because the whole context is captured). The four
scattered `chosen_*` fields collapse into one cohesive `ChoiceBinding`.

### Why grouped-Options, not a `Vec<Binding>`, a per-frame-type enum, or a global stack

These were evaluated and rejected; recorded so a future author does not re-litigate:

- **Per-frame-type enum** (`enum Binding { SkillTestFail{…}, Choice{…}, … }` tailored to the
  frame): cannot express a binding *inherited* from an outer window the frame suspended
  inside (an `on_fail` effect whose choice needs both `failed_by` and the pick). This is the
  "layered, not one-of-N" footgun #345 warns about.
- **`Vec<Binding>` inside `EvalContext`**: a *sibling* of grouped-Options (also per-frame,
  also snapshotted, also desync-safe), but reads are scan-by-kind from scattered evaluator
  sites (not LIFO), and it can hold two same-kind entries. Grouped-Options dominates it for
  by-kind access: O(1) direct field read, cohesive, same nesting expressiveness.
- **Global mutable binding-stack** (peek/pop across resume): breaks under the suspend-and-
  replay re-run. The driver pushes `SkillTestFail` but the `ChoiceFrame` re-runs only the
  effect subtree, not the driver scaffolding — so push/pop balance is split across the
  suspend boundary and across two code paths, and bindings must persist on global state
  invisibly to the replay mechanism. Per-frame snapshots cannot desync (captured once,
  read-only) and are the existing grain of the code (`ChoiceFrame` already snapshots
  `controller`+`source`).

### Same-kind nesting: corpus-verified moot

Grouped-Options holds the **innermost active** binding of each kind; same-kind *nesting* (a
test inside a test) is handled by the per-frame snapshot stack (each level's binding frozen
on its own frame), so a single context only ever needs the innermost value. The only thing
it cannot do is **one effect reading two same-kind bindings simultaneously** — which would
*also* need a DSL surface to name the non-innermost one (none exists).

All 829 Core + Dunwich cards (the project's entire pinned card scope) were scanned: every
`for each point` margin reference is a single innermost test; the multi-`test` cards are all
commit/modifier cards or one test referenced twice (Liquid Courage 02024, Double or Nothing
02026) — **none nests a test inside a test**. Discovery interrupts don't nest (#367);
the attack loop is one-attacker-at-a-time; no card does nested same-target-kind choices.

So this is moot for the corpus. **No `TODO`, no tracking issue** — a plain doc-comment states
the innermost-only semantics as a fact. The grouped-Options → richer-binding change is
localized and self-evident if a future cycle is ever pinned.

## E. Revelation disposal: encounter-card resolution as a frame (#380)

Discarding a one-shot treachery to the encounter discard after its Revelation resolves is
**framework behavior, not card behavior** (RR default; persistent treacheries are the
attaching exception, #235). Card authors must not opt into it. Today the framework handles it
via `pending_revelation_discard: Option<CardCode>` — a global one-slot stash flushed by the
**skill-test driver's** terminal teardown, which only understands "the Revelation suspended
*into a skill test*." Since Axis A a Revelation can suspend on a **choice**, which this
channel does not cover.

**Decision: make encounter-card resolution a continuation frame whose framework teardown
disposes of the card.**

```rust
Continuation::EncounterCard { card: CardCode }
```

(If teardown ever needs more than one resume point, add a small phase enum analogous to
`FinishContinuation` — a single post-Revelation disposal step suffices for the four in-scope
one-shot treacheries, so the frame starts payload-minimal.)

- `resolve_encounter_card` pushes this frame, then runs the Revelation.
- If the Revelation suspends — into a skill test, a choice, a nested effect, *anything* — the
  `EncounterCard` frame sits on the stack **beneath** that suspension.
- When the Revelation's whole sub-resolution completes and control pops back to the
  `EncounterCard` frame, the **framework** runs the standard teardown: one-shot → discard to
  `encounter_discard`; persistent → attach (#235). Then it pops.

Properties: card authors write nothing (just the Revelation effect); suspension-reason-
agnostic (rides the continuation mechanism, not a skill-test-specific flush); removes the
global `pending_revelation_discard` slot **and** the skill-test driver's special teardown for
it (the driver no longer knows treacheries exist). `pending_played_event` (the event-play
analogue) is a sibling side-channel — *not* in scope here, but the same frame-teardown shape
applies and is the natural follow-up.

## Out of scope (explicitly)

- The Enemy-phase `enemy_attack_pending` cursor (internal sequencing; keystone may fold it).
- `pending_played_event` (event-play disposal analogue of #380 — same shape, separate issue).
- The composable/Vec binding model and any same-kind-nesting support (corpus-moot).
- `pending_cancellation`, `pending_skill_modifiers`, `skill_substitutions` (signals /
  accumulators, not input-awaiting suspensions).
- A pure enumerated-legal-actions `InputRequest` model (filed #384; needs its own brainstorm).
- The keystone itself (mid-action attack suspend/resume — Phase 7 ordering step 3).

## Consequences / migration notes

- **Wire + action-log format break.** Removing `PlayerAction::Mulligan` /
  `DrawEncounterCard` and adding `ResolveInput.token` means old serialized action logs do not
  replay. Acceptable pre-1.0; flagged for the persistence layer (Phase 5).
- **Test churn.** Every test that calls `Mulligan` / `DrawEncounterCard`, reads
  `in_flight_skill_test`, or constructs `ResolveInput` without a token migrates. The
  `test_support` resolver and fixtures update once; per-test call sites follow.
- **No intended behavior change** for any in-scope card or scenario. The
  revelation-treachery tests (`crates/cards/tests/revelation_treacheries.rs`) and Crypt
  Chill's suspend path must hold unchanged; add a revelation-suspends-into-a-**choice** test.

## Sequencing (PR decomposition)

One spec, landed as reviewable PRs, each green on the full CI gauntlet:

1. **#345** — serializable `EvalContext` + grouped bindings + per-frame snapshot. Foundational;
   establishes the storage shape the migrated frames use. *(Shipped: PR #385.)*
2. **#348** — migrate the suspension modes onto frames (incl. folding `in_flight_skill_test`);
   collapse both ladders into top-frame dispatch; **normalize the `InputResponse` channel** —
   `CommitCards`/`DiscardCards` → `PickMultiple`, `PickLocation`/`PickInvestigator` →
   `PickSingle`, then fold `Mulligan` → `PickMultiple` and `DrawEncounterCard` → `Confirm`. The
   bulk of the pass. *(Landed incrementally: parts 2a–2c, PRs #386–#391.)*
3. **#347** — token plumbing: `next_resume_token` counter, frame stamps, `ResolveInput.token`,
   stale-reject. Wire change; now trivial on the unified `ResolveInput` channel #348 leaves.
4. **#380** — encounter-card-as-frame + framework disposal teardown; remove
   `pending_revelation_discard`. Small; rides the clean stack.

Rationale (corrected from the original `#347`-before-`#348` ordering): bindings first
(storage), then the migration #348 (consumes the storage and collapses every suspension onto
one `ResolveInput` channel), then token-routing #347 (trivial once that channel is unified),
then the side-channel cleanup #380.

## What "done" looks like

- `continuations: Vec<Continuation>` is the only suspension record; the listed `Option<…>`
  `pending_*` / cursor fields and `in_flight_skill_test` are gone.
- `apply_player_action` and `resolve_input` each dispatch on the top frame; no `pending_X`
  cascade.
- `ResolveInput` is token-validated; stale submits reject cleanly.
- `EvalContext` is `Serialize`; frames snapshot it; bindings are grouped.
- `pending_revelation_discard` is removed; revelations dispose via their `EncounterCard`
  frame, for skill-test *and* choice suspensions.
- Full CI gauntlet green; no behavior change for in-scope content.
