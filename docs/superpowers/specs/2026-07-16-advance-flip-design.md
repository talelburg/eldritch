# Act/agenda advance as an on-card flip — design

**Date:** 2026-07-16
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration) · **Issue:** #558 · **Umbrella:** #206
**Related:** #555 (effect-internal choice anchoring), #541 / S6 (flat-bar retirement), #553/#557 (the anchoring arc this builds on)

## Goal

Model an act/agenda **advance** as a physical, on-card **flip → resolve**: click the
card to flip it to its reverse (front image/text → reverse), then click the reverse
to resolve its effect and advance to the next card. This replaces the current
two-step "flat-bar `Confirm` (the advance) + on-card `Resolve` (the forced reverse)"
that reads as a redundant double-acknowledge.

## Guiding principle: the interactive-resolve convention fires **once, on the card**

The load-bearing insight from the brainstorm. `interactive_acknowledge` (server sets
it `true` for every real game — `session.rs`) turns on a single **convention**: *a
step with one forced outcome surfaces as a click instead of resolving silently.*
That convention (#466) is implemented three times today:

- **Forced effects** — `Continuation::AcknowledgeForced` (`forced_triggers.rs`): the
  Attic horror, agenda/act forced reverses. A one-option "Resolve" pick.
- **Single-option choices** — `resolve_choice_count`'s `1 if interactive ⇒ Suspend`
  arm (`choice.rs`): a `ChooseOne`/`ForEach`/native with exactly one legal option
  suspends for a click instead of auto-binding. Used across the evaluator and card
  natives (Crypt Chill, Dynamite Blast).
- **Advances** — `AwaitAck` (`advance_reverse.rs`): the acknowledge this design turns
  into the flip.

These are **one concept**, not three. #466 is not a separate mechanism that becomes
obsolete once "click-to-resolve" is universal — **#466 *is* that universal model.**
The anchoring arc (#553/#557) re-homes its clicks from the flat bar onto the source
cards; S6 (#541) retires the flat bar once nothing un-anchored is left.

What is genuinely wrong today is **stacking** — firing the convention *twice* for one
logical forced step:

| Case | Double-fire | Fix |
|---|---|---|
| Forced effect **with a choice** (01105: "Resolve" → "discard vs horror") | wrapper ack + inner choice | **#555** |
| Forced advance **via a card ability** (01110: ability-ack + `AwaitAck`) | two advance acks | this design, slice 4 |
| **Chosen** advance (act action → `AwaitAck`) | the action already acknowledged | this design, slice 2 |
| No-choice forced effect (Attic horror) | *none* — the one click **is** the interaction | keep as-is |

**This design's principle: the flip's clicks are instances of the fire-once
convention, not new bespoke frames.** We do not refactor `resolve_choice_count`'s
call sites here (that is #555 + S6); we make the *advance* fire the convention
exactly once, on the card.

## The flip, mechanically

The `AdvanceReverse` continuation frame (`{ deck, from, leaving_code, step }`, already
serialized in `GameState`) drives the advance through `AwaitAck → FireReverse →
Finalize`. The client derives **which face to show** from that frame's `step` — no
client-only flip state, so it stays replay-consistent:

- **no `AdvanceReverse` frame for this deck** → the **current front** (`deck[index]`),
  glowing with a chosen-advance option if one is legal (S5's "Advance act");
- **frame present, `step == AwaitAck`** → **front**, glowing "click to flip" (a forced
  advance awaiting its acknowledge);
- **frame present, `step >= FireReverse`** → the **reverse** (`back_name`/`back_text`),
  glowing "Resolve".

So the flip *is* the `AwaitAck → FireReverse` transition rendered.

## Forced vs. chosen: whether the flip-click prompts

The acknowledge (flip-click) only *prompts* when the advance was **forced**
(unchosen). When the advance was **chosen**, the player's own action already is the
flip, so we **skip** the acknowledge (skip — not auto-resolve; auto-resolving is a
phantom step). Every case is still "click the card to flip → click the reverse to
resolve"; only the first click's origin differs.

The split is **forced vs. chosen**, *not* act vs. agenda — verified against the code:

| Advance | Trigger | Flip-click (click 1) |
|---|---|---|
| **Agenda** (01105–07) | Forced — doom threshold (`advance_agenda`, direct) | the `AwaitAck` acknowledge, on-card |
| **Act, chosen** (01109 round-end objective; `AdvanceAct` clue-spend action) | Deliberate — player spends clues | the advance action itself (`AwaitAck` **skipped**) |
| **Act 01110** ("What Have You Done?") | Forced — `forced_on_event(EnemyDefeated 01116) → AdvanceCurrentAct` | on-card forced acknowledge (see wrinkle) |

Click 2 is always the reverse's "Resolve" — already anchored to `Act`/`Agenda` by
#557 (the `FireReverse` forced fires before `Finalize` bumps the index, so
`candidate_anchor`'s `code == current_{act,agenda}` still holds).

### The 01110 wrinkle

01110 is structurally unlike the agenda. The agenda advances by a **direct engine
doom-check** (`advance_agenda` → one `AwaitAck`), but 01110 advances via a **forced
card ability** whose *effect* is `AdvanceCurrentAct`. So today it stacks **two**
acknowledges: the `#466 AcknowledgeForced` for the 01110 ability, then the advance's
own `AwaitAck` — both "the act is advancing." **Resolution (fire-once):** a forced
ability whose effect is *only* an act/agenda advance suppresses its `#466` confirm —
the advance's `AwaitAck` is the single flip-click, unifying 01110 with the agenda.
01110's reverse is the scenario *resolution* latch (deferred to Phase 9), so click-2
there is "win," not a reverse effect — but the flip model is identical. This is the
only place the design changes forced-effect *semantics* (suppression) rather than
re-homing a click, so it is isolated to **slice 4**, behind the common cases.

## Engine design

`advance_act` / `advance_agenda` already know the advance's origin, so the
`AdvanceReverse` frame carries a `trigger: AdvanceTrigger { Forced, Deliberate }`:

- `advance_agenda` → `Forced`
- `advance_act` from the `AdvanceAct` action / round-end objective → `Deliberate`
- `advance_act` from 01110's effect (`apply_advance_current_act`, evaluator) → `Forced`

`AwaitAck` then: push the `…Advanced` event always; **pause with the on-card flip-pick
iff `interactive_acknowledge && trigger == Forced`**; otherwise fall straight through
to `FireReverse` (no phantom step). Non-interactive play is unchanged (never pauses).

The pause becomes a one-option `PickSingle` whose `ChoiceOption` is anchored to
`OptionTarget::Act` / `OptionTarget::Agenda` (was `InputRequest::confirm(…)`, a bare
`Confirm` with no anchor). `advance_reverse::resume` accepts `PickSingle(OptionId(0))`
where it accepted `Confirm`. Display-only anchor: resume validates only the echoed
option id.

## Data (pipeline)

The snapshot already carries the reverse side for every act/agenda — `back_name`,
`back_text`, `back_flavor` — but `CardMetadata.text` today is **only the front**
(`text`). Ingest `back_name` / `back_text` into `CardMetadata` (add fields; the
`card-data-pipeline` maps them) and regenerate the corpus. The reverse face renders
from these.

## Web

`ActCard` / `AgendaCard` render front vs. reverse by the `AdvanceReverse` frame's
`step` (per "The flip, mechanically"): reverse shows `back_name` + `back_text`. Card
**images and the flip animation** are art-gated (the card-art pipeline does not exist
yet) and are **out of scope** — the first cut is a *content* flip (front text →
reverse name+text), which needs no art.

## Slicing

1. **Pipeline** — `CardMetadata` gains `back_name`/`back_text`; pipeline maps them;
   regenerate corpus. Not user-visible alone (slice 3 consumes it).
2. **Engine** — `AdvanceReverse` gains `trigger`; `AwaitAck` becomes the on-card
   forced-flip `PickSingle` / skips for chosen advances; `resume` accepts the pick.
   Covers agenda + chosen-act.
3. **Web** — render front/reverse by `step`; reverse shows the ingested back text.
4. **Later** — fold 01110's forced-ability advance into the flip (suppress its
   redundant `#466`). Terminal / once-per-scenario; rides behind the common cases.

Slices 1–3 deliver the full flip for the agenda and the chosen act. Each is an
independently reviewable PR; the flip is user-visible once slice 3 lands.

## What "done" looks like

Advancing an act or agenda in interactive play is: **click the card → it flips to its
reverse (showing the reverse's name + effect text) → click the reverse → the effect
resolves and the next card comes up.** No flat-bar advance `Confirm`. Forced advances
prompt the flip-click; chosen advances flip from the player's own advance action. The
interactive-resolve convention fires exactly once per advance, on the card.

## Out of scope

- **Effect-internal choice anchoring** (01105's discard-vs-horror on the reverse) —
  the choice instance of fire-once, tracked in **#555**.
- **Flat-bar retirement** — the broader fire-once/re-home endgame, **S6 (#541)**.
- **Card art + flip animation** — art-gated; the first cut is a content flip.
- **Refactoring `resolve_choice_count`'s call sites** — left to #555 / S6.
- **01110's Phase-9 R1/R2 resolution branch** — its reverse stays the single Won/R1
  latch.
