# Anchor single-hit forced effects to their source card — design

**Date:** 2026-07-16
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); a follow-up to the board interactivity pass
**Issue:** #553 · **Umbrella:** #206 · **Follows:** S5 (#540, PR #552), which set the anchor pattern

## Goal

A **single forced-trigger hit** surfaced under interactive play reaches the wire
as `OptionTarget::Global`, so its source card doesn't glow. This anchors it, the
same way S5 anchored soak / act / effect choices — so a forced ability on an
in-play card lights that card and offers its "Resolve" on the board.

## The one unanchored path

Forced triggers surface two ways; only one is unanchored:

- **2+ ordered forced hits** route through `open_forced_resolution` → the shared
  resolution window → `build_resolution_options`, which S4/S5 already anchor
  (`InPlay → CardInstance`, `Board`+act → `Act`). **Already glow.**
- **A single forced hit under `interactive_acknowledge`** (the common case) routes
  through `drive_acknowledge_forced` (`forced_triggers.rs`), a one-option
  `PickSingle` "confirm before the effect" pause (#466) that hardcodes
  `ChoiceOption::global(OptionId(0), "Resolve")`. **This is the only unanchored
  forced path.**

`Confirm`-style acknowledges (act/agenda advance via `advance_reverse`, skill-test
result #478) carry **no options** (`InputKind::Confirm`), so they have nothing to
anchor — out of scope.

## The serde non-issue (verified)

`Continuation::AcknowledgeForced` is part of `GameState`, which derives
`Serialize`/`Deserialize` — but changing this variant's fields is **not** a
persistence-compat concern:

- The server persists the **seed state** (`session.rs` — `GameState` right after
  scenario setup) plus the **action log** (`insert_action`); current state is
  reconstructed by replaying the log with current code (`session.rs`
  `from_str(&seed_state)` then replay). A seed is a clean post-setup point that
  **never holds an `AcknowledgeForced` frame**, and the log replays under the
  current struct definition — so no persisted payload ever carries this frame
  across versions.
- The only other serialization is WS transport (server→client, same deploy).

So the new field is **required** on the wire, matching the #453 "a stale payload
errors rather than silently degrading" convention — no `#[serde(default)]` needed.

## Architecture (all in `crates/game-core`)

### Shared anchor mapping

Extract the `CandidateSource → OptionTarget` mapping (currently inline in
`build_resolution_options`) into one helper, so the reaction-window and forced-ack
paths can't drift (brainstorm decision):

```rust
// reaction_windows.rs, pub(super)
/// The board anchor for a resolution candidate's source: an in-play instance to
/// its card (#539); a Fast hand event by code (every copy); a board-wide effect
/// to the act card when its code is the current act, else no home (#540/#553).
pub(super) fn candidate_anchor(
    cand: &ResolutionCandidate,
    current_act: Option<&CardCode>,
) -> OptionTarget {
    match cand.source {
        CandidateSource::Hand => OptionTarget::HandCardByCode {
            investigator: cand.controller,
            code: cand.code.clone(),
        },
        CandidateSource::InPlay(id) => OptionTarget::CardInstance(id),
        CandidateSource::Board => {
            if current_act == Some(&cand.code) {
                OptionTarget::Act
            } else {
                OptionTarget::Global
            }
        }
    }
}
```

`build_resolution_options` keeps its per-source **label** inline (windows read
`"Resolve reaction: {code}"` / `"Play {code} from hand"`) and calls
`candidate_anchor(cand, current_act)` for the **target** — behavior-identical to
today. `current_act_code` becomes `pub(super)` so the forced path can reuse it.

### The forced-ack frame carries its candidate

`Continuation::AcknowledgeForced { source: CardCode }` (`state/game_state.rs`)
carries only the code today. Change it to carry the whole candidate:

```rust
AcknowledgeForced { candidate: ResolutionCandidate },
```

`ResolutionCandidate` already derives `Clone`/`Eq`/`Serialize` and holds `code`,
`controller`, `ability_index`, `source` — everything the anchor + the prompt name
need. Construction (`forced_triggers.rs`, the `#466` push) becomes
`AcknowledgeForced { candidate: hit.clone() }` (`hit: &ResolutionCandidate` is in
scope; its `source` — dropped today — is the anchor).

### The surface point anchors the option

`drive_acknowledge_forced` (`forced_triggers.rs`) reads the frame's `candidate`,
names the prompt from `candidate.code` (unchanged `forced_source_name`), and
anchors the single option:

```rust
let anchor = super::reaction_windows::candidate_anchor(
    candidate,
    super::reaction_windows::current_act_code(cx.state).as_ref(),
);
// …
vec![ChoiceOption::new(OptionId(0), "Resolve", anchor)],
```

`resume_acknowledge_forced` and the two `mod.rs` dispatch arms match
`AcknowledgeForced { .. }` — **unchanged** (they ignore the fields). The anchor is
**display-only**: resume still validates the single `OptionId(0)` (never the
anchor), so the confirm-before-effect contract is untouched.

## Web

**None.** An in-play / threat-area / investigator forced source anchors to
`CardInstance` → glows + opens a "Resolve" menu via the existing `InPlayCardView`
matcher (the free ride S5's soak used). A `Board` forced source that is the
current act → the act card (`ActCard`). An agenda-sourced forced effect → `Global`
→ the flat bar, exactly as today (a forced ack is a non-skippable non-`PickMultiple`
prompt, so the `PromptBanner` doesn't render it) — no regression; in-play forced
acks simply gain a glow they lacked.

## Testing

- **Engine (native):** `candidate_anchor` — `InPlay(id) → CardInstance(id)`;
  `Board` with `code == current act → Act`, else `Global`; `Hand → HandCardByCode`
  (covers the extraction, regression-guarding `build_resolution_options`'s existing
  behavior). `drive_acknowledge_forced` — a frame whose candidate is an `InPlay`
  source yields a `PickSingle` whose single option is `CardInstance`-anchored (not
  `Global`); an act-sourced `Board` candidate → `Act`. Update the existing
  `acknowledge_forced_*` tests' frame construction to the `candidate` field.
- **Regression:** existing `build_resolution_options` anchor tests
  (`resolution_option_anchor_tests`) still pass unchanged (the label/target output
  is identical after the extraction).
- No new web test — the in-play glow rides the existing `InPlayCardView` headless
  coverage (a `CardInstance`-anchored option glows the card).
- Full 7-job CI gauntlet green.

## What "done" looks like

- A single forced ability on an in-play / threat / investigator card, in
  interactive play, glows that card and offers "Resolve" on it (not only the flat
  bar); an act-sourced forced effect anchors to the act card.
- The `CandidateSource → OptionTarget` mapping lives in exactly one place.
- Anchors stay display-only; the confirm-before-effect resume is unchanged.
- Native tests pass; full gauntlet green.

## Out of scope

- `Confirm`-style acknowledges (act/agenda advance, skill-test result) — no options
  to anchor.
- Agenda `Board` anchoring — no `OptionTarget::Agenda` (agenda-forced stays
  `Global`, consistent with S5).
- The 2+ ordered forced run — already anchored via `build_resolution_options`.
- Any change to forced-trigger *timing / eligibility* — this only re-labels the
  already-surfaced option's anchor.

---

## Follow-up: location-sourced forced anchor

**Added:** 2026-07-16 (same PR #554, before merge) · continues #553

### The remaining gap

The initial cut anchored **in-play instances** and the **act**. A forced ability on
a **location** (the Attic 01113 — "Forced – After you enter the Attic: take 1
horror") still reaches the wire as `OptionTarget::Global`: resolvable only from the
flat bar, never by clicking the location on the map.

Cause: a location has a `LocationId`, **not** a `CardInstanceId`. The only scan that
reads a location's *own* code — `collect_forced_hits`'s `EnteredLocation` arm
(`forced_triggers.rs`, `push_matching(reg, &loc.code, …, None, …)`) — passes
`source: None`, which `push_matching` collapses to `CandidateSource::Board`. The
Attic's code is not the current act's code, so `candidate_anchor`'s `Board` arm
falls to `Global`. `CandidateSource` carries no `LocationId`, so there is no path
from a location-sourced forced hit to `OptionTarget::Location(id)` — which the map
node already renders (from S1).

The two *other* location-touching scans are **not** in scope: the skill-test
`tested_location` scan (Obscuring Fog 01168) and the `LeftLocation` scan (Barricade
01038) both walk `loc.attachments` and pass `Some(att.instance_id)` — they already
anchor to the **attachment** instance (`InPlay`), which is correct. Only the
location's *own* forced ability (`EnteredLocation`, `&loc.code`, `None`) is
unanchored.

### Change: `CandidateSource` gains a `Location` variant

```rust
// state/game_state.rs
pub enum CandidateSource {
    InPlay(CardInstanceId),
    /// A location's own ability (the Attic's forced horror) — anchors to the
    /// location on the map. Locations have a `LocationId`, not a `CardInstanceId`.
    Location(LocationId),
    Board,
    Hand,
}
```

`push_matching`'s `source` parameter is `Option<CardInstanceId>` today, which can
express only `InPlay` / `Board` — a location is a third origin it cannot encode.
Widen the parameter to take a `CandidateSource` **directly**; the internal
`match source { Some => InPlay, None => Board }` disappears and each call site
states its origin (`CandidateSource::Board` where it passed `None`,
`CandidateSource::InPlay(id)` where it passed `Some(id)`). The `EnteredLocation` arm
passes `CandidateSource::Location(*location)`.

`candidate_anchor` gains one arm:

```rust
CandidateSource::Location(id) => OptionTarget::Location(id),
```

This flows through the shared helper, so **both** the single-hit
`drive_acknowledge_forced` path (the Attic case) and the 2+ ordered
`build_resolution_options` path anchor a location forced to its map node — for free,
and consistently.

### Web

**None.** `OptionTarget::Location(id)` already glows the map node and opens its
menu (S1). The Attic simply gains the glow it lacked.

### Testing (additions)

- `candidate_anchor`: `Location(id) → OptionTarget::Location(id)`.
- `drive_acknowledge_forced`: a frame whose candidate is a `Location` source yields
  a `PickSingle` whose single option is `Location`-anchored (not `Global`) — the
  Attic-enter case.
- Regression: the widened `push_matching` leaves every existing anchor test
  (`InPlay` / `Board` / act) unchanged.
