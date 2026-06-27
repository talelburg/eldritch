# Surface single-option auto-binds as choices (#466)

## Problem

The engine auto-resolves things the player never sees or "performs":

- **No-choice forced effects** — the Attic (01113) "Forced: take 1 horror", the
  Cellar (01114) "Forced: take 1 damage". They apply silently; the only trace is
  a changed counter the player may not notice.
- **Auto-bound single-option choices** — when exactly one option is legal the
  engine auto-binds it without asking (`resolve_choice_count`'s `1 ⇒ Auto`): the
  sole attack target, the only asset to discard, etc. The player never "does" it.

Tabletop Arkham makes the player physically perform all of these, so surfacing
them is also more faithful.

## Decisions from brainstorming

- **UX shape: surface as a choice, not a separate acknowledge frame.** Rather than
  bolt on a bespoke "acknowledge" continuation, we treat the *auto-bind itself* as
  the root cause and make a single legal option surface as a **one-option
  `PickSingle`** — gated behind the interactive flag. The pick happens **before**
  the option resolves (the player "performs" it, then the effect lands).
- **No double-confirm.** When 2+ options exist the player already picks (an
  ordering / target choice) — that *is* the awareness. We only change the n=1 case.
- **Reuse `interactive_acknowledge`.** It already means "human play — make things
  visible" (it gates the skill-test result pause #478 and the act/agenda advance
  pause #482). We broaden its meaning to also gate single-option surfacing; a
  rename is deferred (noted below).
- **Full unification in this PR.** Every single-option auto-bind surfaces under the
  flag — attack target, asset discard, forced effect — not just forced harm. This
  is the whole point: one mechanism, not a parallel acknowledge subsystem. The
  previously-"deferred" single-option surfacing is no longer a follow-up; it *is*
  the solution.

## Goal

With `interactive_acknowledge` on, a solo human in the browser sees and confirms
every auto-resolved single-option step before it resolves: the Attic's forced
horror, the sole attack target, the only asset to discard. With the flag off
(tests, non-interactive consumers) behavior is **unchanged** — single options
auto-bind exactly as today, so determinism and existing tests are preserved.

## Design

Two auto-bind mechanisms exist; both get the same flag-aware n=1 treatment.

### Mechanism A — `resolve_choice_count` (effect-level choices)

`crates/game-core/src/engine/dispatch/choice.rs` defines the shared resolver:

```rust
pub fn resolve_choice_count(n: usize) -> ChoiceResolution {
    match n {
        0 => ChoiceResolution::Empty,   // caller's printed fallback / reject
        1 => ChoiceResolution::Auto(0), // bind the sole option silently
        _ => ChoiceResolution::Suspend, // controller picks
    }
}
```

Make it flag-aware:

```rust
pub fn resolve_choice_count(n: usize, interactive: bool) -> ChoiceResolution {
    match n {
        0 => ChoiceResolution::Empty,
        1 if interactive => ChoiceResolution::Suspend, // surface the sole option
        1 => ChoiceResolution::Auto(0),                // flag off: today's behavior
        _ => ChoiceResolution::Suspend,
    }
}
```

The five call sites pass `cx.state.interactive_acknowledge`:

- `evaluator.rs:544` — `ChooseOne` branches
- `evaluator.rs:688` — deck choice
- `evaluator.rs:1606` — spatial/entity candidate choice (e.g. `Effect::Fight` target)
- `crates/cards/src/impls/crypt_chill.rs:82` — asset to discard
- `crates/cards/src/impls/dynamite_blast.rs:93` — location to blast

Each call site already has a `Suspend` arm that builds one render label per
candidate and suspends (`awaiting_choice` / `suspend_for_native_choice`). With one
candidate that arm naturally produces a **one-option** `PickSingle`; resume indexes
`OptionId(0)` to the sole candidate. So the call sites need no structural change
beyond passing the flag — the existing `Suspend` path handles n=1 correctly because
label-building maps over the candidate list (length 1 → one label). No new resume
logic: `resume_effect_choice` already validates the pick by checked indexing.

**`ChoiceResolution::Auto`** stays for the flag-off path; it is not removed.

### Mechanism B — forced-trigger surfacing (no-choice forced effects)

Attic/Cellar forced abilities are `forced_on_event(..., deal_horror/deal_damage)`
— no `ChooseOne`, so they never reach `resolve_choice_count`. They resolve through
the forced-trigger chokepoint in `emit.rs`:

```rust
// emit_event(): today
let candidates = collect_forced_hits(state, &point, After);
if candidates.len() >= 2 {
    open_forced_resolution(cx, event, candidates) // lead orders them (a choice)
} else {
    fire_forced_triggers(cx, &point, After)       // 0 or 1: fire synchronously
}
```

`open_forced_resolution` (`reaction_windows.rs:123`) builds a `ChoiceOption` per
forced candidate ("forced — cannot skip; the lead orders them") and resolves them
in the picked order. That is exactly a "which forced effect to resolve next"
choice — and for a single candidate it is the acknowledge we want.

Change: route **n ≥ 1** through `open_forced_resolution` **when interactive**, and
let that path present a single candidate as a one-option choice:

```rust
let candidates = collect_forced_hits(state, &point, After);
let interactive = cx.state.interactive_acknowledge;
match candidates.len() {
    0 => fire_forced_triggers(cx, &point, After), // nothing forced (→ reaction)
    1 if !interactive => fire_forced_triggers(cx, &point, After), // today's path
    _ => open_forced_resolution(cx, event, candidates), // ≥2 always; 1 when interactive
}
```

`open_forced_resolution` / its window-resume must present a single-candidate set as
a one-option `PickSingle` rather than auto-firing (the analog of Mechanism A's n=1
change). The pick precedes resolution → "confirm before the effect" holds. With the
flag off, a single forced effect still fires synchronously (today's behavior), so
non-interactive consumers and tests are unaffected.

This covers the original #466 examples (Attic, Cellar) and any future no-choice
forced ability uniformly, with no per-card work.

### Prompt copy

- **Forced effects (Mechanism B):** `"Forced — {name}"`, where `{name}` is the
  source card's display name resolved via `card_registry::current().metadata_for`
  (fallback to the raw code when no registry is installed — i.e. tests). One option
  labelled with the source.
- **Effect choices (Mechanism A):** keep the labels each call site already builds
  (asset name, enemy name, branch label). No copy change.
- **Descriptive effect text** ("…takes 1 horror") is explicitly **out of scope** —
  see follow-ups. The MVP names the source; the player reads the resulting counter
  change.

### Web client

No new view. `AwaitingInputView` already renders `InputKind::PickSingle` as one
button per option and `InputKind::Confirm` as a Confirm button. A one-option
`PickSingle` renders as a single labelled button — the desired "perform it" control.
Display names already resolve client-side (`crate::names`), so `"Forced — The
Attic"` shows the location name.

### The flag

`interactive_acknowledge` (`game_state.rs:248`, default `false`; the server sets it
`true` for human play in `GameSession::create`). Its meaning broadens from "pause
to acknowledge results" to "human play — surface single-option steps". A rename to
something like `interactive` / `surface_single_options` is **deferred** to avoid
churn in this PR; noted as a follow-up.

## Non-goals / deferred (with follow-ups)

- **Descriptive effect/ability text.** Building player-facing prose from an effect
  tree or emitted events ("Roland takes 1 horror") — a general capability that also
  serves an event feed. **Action: file a new follow-up issue.** (Overlaps #469's
  "player-facing copy, not protocol strings".)
- **Pure-framework harm with no card ability.** The draw-from-empty-deck horror
  penalty deals harm via a framework rule, not a forced *ability*, so Mechanism B
  does not reach it. **Action: note on #429** that its interactive soak/harm should
  also surface an acknowledge when that work lands.
- **Event feed / passive log.** A running history pane in the web client — a
  separate UI investment, not pursued here.
- **2+ simultaneous-forced ordering UX** is unchanged (already surfaced).
- **`interactive_acknowledge` rename** — deferred (see The flag).

## Testing

- **`resolve_choice_count` unit (choice.rs):** `1 with interactive ⇒ Suspend`;
  `1 without ⇒ Auto(0)`; `0 ⇒ Empty`; `2+ ⇒ Suspend` regardless of the flag.
- **Mechanism A integration:** a `ChooseOne`/`Effect::Fight` with a single legal
  option suspends a one-option `PickSingle` when `interactive_acknowledge` is on,
  and resolves on resume; with the flag off it auto-binds (today's behavior,
  regression-guarded). Card test: Crypt Chill with one asset surfaces the discard
  as a one-option pick under the flag.
- **Mechanism B integration (cards crate, real registry):** entering the Attic
  (01113) with the flag on suspends a one-option `PickSingle` whose label names the
  source, and applies the 1 horror only **after** the pick; the Cellar (01114)
  likewise for 1 damage. With the flag off both fire synchronously (no suspend),
  guarding today's behavior.
- **No-regression sweep:** the existing engine/server suites run with the flag off
  by default, so they must pass unchanged.

## Open questions

None outstanding — the brainstorm settled the UX shape (surface as choice, before
resolution), the flag (reuse `interactive_acknowledge`), and scope (full
unification; descriptive text and framework-harm deferred to follow-ups).
