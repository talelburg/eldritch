# Trigger-dispatch rework — Axis A: interactive choice

**Status:** design approved. Sub-project 2 of the trigger-dispatch rework
([umbrella](2026-06-16-trigger-dispatch-rework-umbrella-design.md) §4 A, §5),
following the shipped Axis-B foundation
([spec](2026-06-16-trigger-dispatch-rework-axis-b-foundation-design.md),
PRs #338–#343). Tracker issue [#334](https://github.com/talelburg/eldritch/issues/334).

**Scope of this doc:** the deep-dive for the interactive-choice machinery the
umbrella scoped — un-stubbing `Effect::ChooseOne`,
`LocationTarget::ChosenByController`, `InvestigatorTarget::ChosenByController`,
plus a general "a native leaf may request a controller pick" path. It refines
(and in two places deliberately *narrows*) the umbrella's §3/§4-A sketch against
the real card set.

## Why this exists

`Effect::ChooseOne`, `LocationTarget::ChosenByController`, and
`InvestigatorTarget::ChosenByController` are `AwaitingInput` stubs in
`evaluator.rs` — they `Err("… requires AwaitingInput plumbing")`. Axis B built
the continuation stack (`Continuation::{Resolution, SkillTest}`) and the
reentrant forced-run loop, but deliberately left the choice frame and structured
input un-built (no consumer yet). Axis A is the first real choice consumer; it
adds `Continuation::Choice`, the structured input contract, and the evaluator
resolution for the three stubs.

No shipped card consumes the stubs live today. Two cards ship deterministic
`TODO(#212)` stand-ins *in place of* a real choice — they are Axis A's real-card
demonstrators (below).

## Scope decisions (settled in brainstorming)

### Demonstrators: upgrade both stand-ins, no Axis-E coupling

Axis A is machinery (like Axis B: no *new* cards), validated by synthetic test
cards **and** by upgrading the two existing deterministic stand-ins to real
interactive choices. Neither upgrade needs an Axis-E prereq:

- **Crypt Chill 01167** — Revelation willpower(4) test; on fail, *"choose and
  discard 1 asset you control (if you cannot, take 2 damage instead)."* Currently
  discards the first asset in play order. A textbook auto-resolve: `0 assets ⇒
  take 2 damage · 1 ⇒ auto-discard · 2+ ⇒ suspend & the controller picks`.
- **Agenda 01105 "What's Going On?!"** — forced on-advance reverse; the lead
  *"must decide (choose one): Either each investigator discards 1 card at random
  from his or her hand, or the lead investigator takes 2 horror."* Currently
  ships the 2-horror branch deterministically.

**The randomness concern in 01105's old TODO is moot.** That TODO claimed the
random-discard branch needs a *recorded* `EngineRecord` for replay. It does not:
randomness is `(seed, draws)` on `GameState` (`crates/game-core/src/rng.rs`),
and replay determinism comes from the action log replaying *in order* — each
`rng.next_index` call lands at the same stream position and reproduces bit-for-bit.
`action.rs`'s `EngineRecord` doc-comment says so directly ("RNG determinism
reproduces them from the same triggering action"); `EngineRecord::DeckShuffled`
exists only for *out-of-band* shuffles, and even there for log clarity, not
determinism. The lead's pick is a `ResolveInput` in the log; a chosen
random-discard then runs `rng.next_index` deterministically on replay. So 01105
needs only: the `ChooseOne` machinery (this axis), the existing `Effect::ForEach`
(each investigator), and a ~10-line **`Effect::Native`** "discard 1 at random
from hand" leaf — the single-consumer native pattern Crypt Chill already follows.
No randomness primitive, no `EngineRecord` variant.

### The three choice shapes

| Shape | Surface | Demonstrator | Also validated by |
|---|---|---|---|
| **Branch pick** — choose one of N *effects* | `Effect::ChooseOne(Vec<Effect>)` | agenda 01105 | synthetic |
| **Target pick** — choose one of N *board entities* | `LocationTarget::ChosenByController`, `InvestigatorTarget::ChosenByController` | — (Axis-E cards later) | synthetic ×2 |
| **Instance pick from a native** — choose one of N *card instances* | (stays `Effect::Native`) | Crypt Chill 01167 | — |

The third shape (a *card instance* — an asset you control) is **not** one of the
three named stubs, and **no Axis-E card reuses it** (Dynamite Blast → location;
Beat Cop → enemy; First Aid / Medical Texts / Old Book → investigator). Rather
than add a speculative `CardInstanceTarget::ChosenByController` DSL surface for a
single consumer (against the "no DSL primitive until ≥2 consumers" rule), Crypt
Chill **stays native** and is the first consumer of a general *"a native leaf may
suspend for a controller pick"* path — reusable by any `Effect::Native` (the
project's single-consumer escape hatch), and a verbatim reuse of the C5a
clue-interrupt pattern (native body re-invoked on resume with the chosen value
threaded through `EvalContext`).

### One selection family: `PickSingle` only

Every choice surface in scope, and every card this axis unblocks downstream,
selects exactly **one** thing ("choose **an** X", never "up to N"):
`ChooseOne` → one branch; the two targets → one location / investigator; Crypt
Chill → one asset; Dynamite Blast → one location; Beat Cop → one enemy; First
Aid / Medical Texts / Old Book → one investigator.

So **`PickMultiple` has zero consumers in Axis A.** Per the umbrella §3.3 it is
"adopted for *new* multi-selects," and the legacy multi-selects (`CommitCards` /
`DiscardCards`) fold into it "when their suspension modes migrate to the stack —
*not* as a now-rewrite." Neither trigger fires here. `PickMultiple` is deferred
to its first real consumer (a future "choose up to 2 targets" card, or the
commit/discard cleanup pass).

## §1 — Single-pass suspend-and-replay (narrows the umbrella's two-pass)

The umbrella §4-A proposed a two-pass evaluator: a non-mutating *planning* pass
that grounds all choices, then an *execution* pass that mutates once. The split
exists solely to make re-running safe when a choice sits **after a mutation in a
`Seq`** (re-running `Seq[mutate, choice]` from the top would double-apply the
mutation).

**No card — implemented, demonstrator, or in the blocked Axis-A set — has a
choice inside a `Seq` at all:**

| Card | Choice position |
|---|---|
| 01105 | `ChooseOne` *is* the whole effect |
| 01167 | single native leaf (a SkillTest `on_fail`) |
| Dynamite Blast | one chosen location → a single AoE effect |
| Beat Cop | one chosen enemy → a single DealDamage |
| First Aid | chosen investigator + damage/horror branch, both ground *before* the one heal |
| Medical Texts | chosen investigator, ground *before* the skill test |
| Old Book of Lore | chosen investigator, then the search |

In every case the choice is the whole effect, a target feeding a single effect,
or ground before a skill test — never after a mutation in a `Seq`. So the
planning/execution split is machinery for a card that does not exist yet.

**Axis A ships single-pass suspend-and-replay instead.** The evaluator, on
hitting an un-ground choice, suspends; on resume it appends the pick to a
`decisions` log and **re-runs the effect from the top**, replaying `decisions`
to reach the next un-ground choice. Because no mutation precedes any choice in
the current set, the re-run is safe. The `decisions` log is kept (not collapsed
to a single-pick frame) because **First Aid** needs multi-choice-per-effect
(investigator + branch) — one named card away, so a synthetic 2-choice card
validates it now rather than churning the frame next axis.

Safety is made **loud, not by-convention**, with two guards (standard
"minimal-correct slice + loud reject + `TODO(#NNN)`" pattern — cf. C5a's
terminal-position bound, #294's `debug_assert`):

1. **Seq guard** (`apply_seq`): if a `Seq` element suspends (`AwaitingInput`) and
   any **earlier** element already ran, reject `TODO(#NNN): choice after a Seq
   step not yet supported`. Defers `Seq[mutate, choice]` → the full two-pass,
   filed as a follow-up when a card needs it.
2. **Native-standalone guard**: if a native leaf suspends while `decisions` is
   already non-empty (DSL choices preceded it), reject. Bounds native picks to
   "the native is the whole effect" (= Crypt Chill), deferring native↔DSL-choice
   interleaving.

## §2 — The `Choice` frame + resume

`Continuation` gains a third variant alongside Axis-B's `Resolution` + `SkillTest`:

```rust
enum Continuation {
    Resolution(ResolutionFrame),
    SkillTest,
    Choice(ChoiceFrame),   // ← Axis A
}

struct ChoiceFrame {
    decisions:  Vec<OptionId>,          // picks recorded so far (the replay log)
    offered:    Vec<OptionId>,          // set offered at the CURRENT suspend; resume validates membership
    effect:     Effect,                 // root effect being (re-)resolved — Native is just a leaf here
    controller: InvestigatorId,         // EvalContext ingredients (see below)
    source:     Option<CardInstanceId>,
}
```

`Continuation` (hence `ChoiceFrame`) serializes for replay/persistence, so the
frame either stores a serializable `EvalContext` or stores its ingredients and
rebuilds. **Every `EvalContext` field is serde-derivable** (`InvestigatorId`,
`Option<CardInstanceId>`, `Option<u8>`, `Option<EnemyId>`) — it just doesn't
`#[derive(Serialize)]` today (it is `Copy`-only; nothing has needed to persist
one). So this is a consistency/semantics call, not a capability constraint. We
**store the ingredients** (`controller`, `source`) and rebuild
`EvalContext::for_controller_with_source(controller, source)` on resume, for two
reasons:

- **It matches the one existing precedent.** `InFlightSkillTest` stores
  `investigator` + `source` + the `on_fail`/`on_success` effects and rebuilds the
  context (`skill_test.rs:210`) rather than persisting one.
- **The window-bound fields are transient.** `failed_by` /
  `clue_discovery_count` / `attacking_enemy` are "bound only during a specific
  window"; persisting them in a frame that outlives the window risks a stale
  binding on resume. Setting `chosen_instance` from the pick on resume mirrors
  `skill_test.rs:258` setting `failed_by` from the just-computed margin exactly.

For Axis A's demonstrators those two ingredients suffice; a future choice that
closes over a bound window field adds that ingredient then. (The alternative —
deriving `Serialize` and storing the whole context — is viable and slightly less
code; rejected only for the consistency/transient-field reasons above.)

A principled version of this — making `EvalContext` serializable with its
bindings *modeled* (durable ingredients vs. a composable per-window binding, so
illegal combinations are unrepresentable) — is filed as
[#345](https://github.com/talelburg/eldritch/issues/345), scoped to the umbrella
§1 continuation-stack cleanup pass. Axis A's `controller`/`source` ingredient
split is forward-compatible with it (it becomes the structured `durable` half),
so deferring costs nothing here.

A single resume path (the two `ChoiceResume` variants from the brainstorm
collapse: `Effect::Native` *is* an `Effect`, so re-running the root effect tree
covers both DSL and native leaves; the native reads its pick from `EvalContext`).
`resolve_input` routes `PickSingle(id)` to the top `Choice` frame: validate
`id ∈ offered`, push onto `decisions`, rebuild the `EvalContext`, and re-run the
effect from the top. DSL choice nodes consume the `decisions` cursor in pre-order;
a re-reached native leaf reads `EvalContext.chosen_instance`.

**Reentrancy under Axis B:** 01105's `ChooseOne` fires inside a **Forced**
resolution run (forced agenda effect). The `Choice` frame parks above the
`Resolution(Forced)` frame; Axis B's iterative forced loop already resumes its
siblings after a parked sub-frame completes (same mechanism as a forced effect
suspending into a skill test). No new forced-loop surgery.

## §3 — The input contract

- **`InputResponse`** gains **`PickSingle(OptionId)`** — "echo back one id from
  the offered set." Consolidates the *new* choice picks; legacy `PickIndex` /
  `PickLocation` / `PickInvestigator` on the reaction-window path stay as-is
  (deferred cleanup, not rewritten — the two protocols coexist).
- **`InputRequest`** gains a typed `Vec<ChoiceOption>` — opaque `OptionId` +
  render label + a discriminant (location / investigator / branch) so the host
  can render the right control. This is the umbrella §3.2 structured-options
  upgrade and seeds #205. (The legacy `{ prompt: String }` path stays for the
  reaction window.)
- **`OptionId`** — a `u32` newtype, the index into the frame's offered `Vec`, so
  resume validates "is this id in the set I offered" structurally.
- **`Skip` / `Confirm`** stay distinct from selection, unchanged.

## §4 — `EvalContext` addition

One field, in the established `Option<T>`-bound-during-a-window mold (mirrors
`failed_by` / `clue_discovery_count` / `attacking_enemy`):

```rust
/// The card instance a controller picked, bound only while re-invoking a
/// native leaf that suspended for an instance choice (Crypt Chill 01167).
/// `None` outside that window. Mirrors `clue_discovery_count`.
pub chosen_instance: Option<CardInstanceId>,
```

The DSL choices need no `EvalContext` field — they bind via the frame's
`decisions` cursor; only the opaque native leaf reads its pick from context. The
frame reconstructs the context from its stored `controller` + `source`
ingredients on resume (§2), setting `chosen_instance` from the pick.

## §5 — The uniform resolve convention

Every selection (`ChooseOne`, `*::ChosenByController`, the native instance pick)
enumerates legal options, then:

- **0 options** → reject (or the printed fallback under a "may" / "if you cannot"
  — Crypt Chill's "take 2 damage instead");
- **1 option** → auto-bind, no input (the Fight / Beat-Cop "single engaged enemy"
  precedent in `check_activate_ability`);
- **2+ options** → suspend with a `Choice` frame.

Solo play auto-resolves the common case; only genuinely-ambiguous choices
round-trip.

## §6 — Synthetic validation + sequencing

**Synthetic test cards** (`synth_cards::TEST_REGISTRY`, the C5a integration-test
home) carry the surfaces with no shipped consumer: a `ChooseOne` card
(branch-pick + auto-resolve 0/1/2+); a `LocationTarget::ChosenByController` card
and an `InvestigatorTarget::ChosenByController` card; and a **2-choice** card
(target + branch) for the multi-choice replay path First Aid will need.

**Coarse sequencing** (writing-plans turns this into TDD tasks):

1. **Input contract** — `PickSingle(OptionId)`, structured `InputRequest`
   (`Vec<ChoiceOption>` + `OptionId`), serde round-trips. Pure plumbing, no
   behavior.
2. **`Continuation::Choice` frame** + `resume_choice` router + the
   `0⇒reject·1⇒auto·2+⇒suspend` resolver helper.
3. **`Effect::ChooseOne`** via the evaluator + `decisions` replay + the Seq
   guard; synthetic `ChooseOne` card + **agenda 01105** (`ChooseOne` + `ForEach`
   + native random-discard — exercises the Forced-run reentrancy).
4. **`Location` / `Investigator::ChosenByController`** + synthetic cards (incl.
   the 2-choice synthetic).
5. **Native instance-pick** (`chosen_instance` + native-standalone guard) +
   **Crypt Chill 01167**.
6. **Phase-7 doc** update (Axis-A row + Decisions) as the final commit, after CI
   is green.

## Out of scope (deferred, with the guard that catches each)

- **`Seq[mutate, choice]`** → the full two-pass planning/execution split.
  Caught by the Seq guard. File a follow-up.
- **Native ↔ DSL-choice interleaving** (a native that picks *and* sits among
  other choices). Caught by the native-standalone guard. File a follow-up.
- **`PickMultiple`** / multi-target selection. No Axis-A consumer; first real
  consumer or the commit/discard cleanup pass owns it.
- **A choice whose legal options depend on a mutation earlier in the same tree**
  (umbrella §4-A) — strictly stronger than the Seq guard; no card needs it.

## Dependencies

- **Axis B** (PRs #338–#343) — the continuation stack
  (`Continuation::{Resolution, SkillTest}`) and the reentrant forced run. Axis A
  adds the `Choice` frame to that stack.
- The `EngineRecord` / `rng` model (`crates/game-core/src/rng.rs`) — relied on
  for 01105's replayable random discard.

## What "done" looks like

`Effect::ChooseOne`, `LocationTarget::ChosenByController`, and
`InvestigatorTarget::ChosenByController` resolve interactively (auto-resolving
0/1, suspending on 2+); Crypt Chill 01167 and agenda 01105 ship their real
interactive choices (replacing the deterministic stand-ins); synthetic cards
cover the un-stubbed targets + the multi-choice replay; and the two guards reject
the deferred shapes loudly. The Axis-E cards (Dynamite Blast, Beat Cop, First
Aid, Medical Texts, Old Book of Lore) are then unblocked on the choice axis,
pending only their orthogonal Axis-E primitives.
