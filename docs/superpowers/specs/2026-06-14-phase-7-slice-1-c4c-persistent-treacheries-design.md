# Phase 7 Slice 1 — C4c: persistent threat-area / attachment treacheries

**Issue:** [#235](https://github.com/talelburg/eldritch/issues/235) (Group C, sub-slice C4c).
**Depends on:** C4a (threat-area zone + forced points #233/PR #285), C3c (`RoundEnded` #232/PR #278), #286 (`Effect::SkillTest` + suspendable-revelation discard, PR #287).
**Date:** 2026-06-14.

## Goal

Implement the three persistent encounter treacheries from The Gathering's
encounter sets — each *stays in play* (threat area or location
attachment), enforces a **constant restriction**, and **discards itself**
at a forced timing point. Card texts (verified against
`data/arkhamdb-snapshot/pack/core/core_encounter.json`):

- **Frozen in Fear (01164)** — `Revelation` - Put into play in your threat
  area. *The first time you perform one of the following actions (move,
  fight, or evade) each round, it costs 1 additional action.* `Forced` -
  At the end of your turn: Test [willpower] (3). If you succeed, discard
  Frozen in Fear.
- **Dissonant Voices (01165)** — `Revelation` - Put into play in your
  threat area. *You cannot play assets or events.* `Forced` - At the end
  of the round: Discard Dissonant Voices.
- **Obscuring Fog (01168)** — `Revelation` - Attach to your location.
  *Limit 1 per location. Attached location gets +2 shroud.* `Forced` -
  After attached location is successfully investigated: Discard Obscuring
  Fog.

## Design overview

The work splits into **shared engine seams** and **three thin card
impls**. The constant restrictions are modeled by *extending the
inspectable DSL* (chosen over card-local registry query hooks) so the
engine reads them from `abilities_for` exactly as it already reads
`Trigger::Constant` `Effect::Modify` in `constant_skill_modifier`.

### Seam 1 — location attachment zone

`Location` gains `attachments: Vec<CardInPlay>` (defaulted empty in
`Location::new` and existing struct literals). New `Zone::LocationAttachment`
variant and `Event::CardAttachedToLocation { location, code, instance_id }`
mirroring `Event::CardEnteredThreatArea`.

Two helpers beside the existing threat-area pair in
`dispatch/threat_area.rs` (or a sibling `attachments.rs`):

- `attach_to_location(cx, location_id, code) -> Option<CardInstanceId>` —
  mints an instance, pushes onto the location's `attachments`, emits
  `CardAttachedToLocation`. **Generic — no limit enforcement** (the
  "Limit 1 per location" clause is printed on Obscuring Fog specifically;
  other attachments need not carry it, so the limit lives in the card, not
  the helper).
- `discard_from_location_attachment` is *not* a separate helper — discard
  is unified in Seam 4 (`Effect::DiscardSelf`).

### Seam 2 — derived persistence (replaces any "suppress-discard" flag)

`resolve_encounter_card`'s treachery arm today always pushes the card to
`encounter_discard` after the `Revelation` resolves. Persistent
treacheries must not be auto-discarded — instead of a side-channel state
flag, the engine **derives** persistence from the card's abilities:

> A treachery is **persistent** iff it has at least one ability whose
> trigger is not `Trigger::Revelation`.

One-shot treacheries (01162/01163/01166/01167) have *only* a `Revelation`,
so they auto-discard as today. All three C4c cards carry `Revelation` +
`Constant`/`OnEvent` ongoing abilities, so the arm skips the auto-discard
and the **card fully owns its disposition** in every path — it places
itself (threat area / attachment) on the normal path, and discards itself
to `encounter_discard` on the limit-1 path (Obscuring Fog).

`TODO`: this derivation assumes every persistent treachery carries an
ongoing ability and every one-shot carries none — true for all
Core+Dunwich treacheries. Revisit with an explicit persistence marker
only if a treachery ever needs to persist with no ongoing ability, or
auto-discard despite carrying one.

### Seam 3 — constant restrictions in the inspectable DSL

`card-dsl` additions:

- `Stat::Shroud` — a location stat an `Effect::Modify` can adjust
  (Obscuring Fog's `+2`).
- `enum Restriction { CannotPlay(CardType), ExtraActionCost { actions, first_each_round: bool } }`
  where `actions` enumerates the gated action kinds (move / fight / evade
  for Frozen in Fear). Represent `actions` as a small explicit set type
  (e.g. a struct of `move`/`fight`/`evade` bools, or a `Vec<ActionClass>`)
  — concrete shape chosen during implementation; it must be `Copy`/`Clone`
  + serde + `PartialEq`.
- `Effect::Restrict(Restriction)` + a `restrict(restriction)` builder,
  used only under `Trigger::Constant`.

`game-core` query helpers, each mirroring `sum_constant_modify`'s
"scan controlled instances, look up abilities, sum/test matching
`Trigger::Constant` effects" shape:

- `effective_shroud(state, reg, location) -> u8` — printed `location.shroud`
  plus every `Stat::Shroud` `Modify(WhileInPlay)` on that location's
  `attachments`. Read by `investigate` in place of the raw
  `location.shroud` at `actions.rs:98`.
- `play_is_prohibited(state, reg, investigator, card_type) -> bool` — true
  if any active `Restriction::CannotPlay(card_type)` on the investigator's
  controlled instances matches. Checked in `play_card` validation
  (reject before mutation).
- `pending_action_surcharge(state, reg, investigator, action_class) -> (u8, Vec<CardInstanceId>)`
  — sums the extra action cost from `Restriction::ExtraActionCost`
  restrictions whose `actions` include `action_class`; for
  `first_each_round` restrictions, *excludes* source instances already in
  the per-round spent-set (Seam 5). Returns the extra cost **and** the
  list of `first_each_round` source instances to mark spent on commit
  (so cost-peek stays read-only for validate-first).

`TODO` at `ExtraActionCost`: the `first_each_round` gate also appears on
non-cost mechanisms (a constant ability that suppresses attacks of
opportunity on the first action each round; a forced trigger that fires
on the first move each turn). Promote `first_each_round` to a shared
"first-applicable each round/turn" scope spanning constant modifiers and
forced triggers once a second mechanism needs the same gate — not while
action-cost is its only consumer.

### Seam 4 — unified self-discard (`Effect::DiscardSelf`)

One typed effect replaces per-card discard logic. `apply_effect` for
`Effect::DiscardSelf` reads `eval_ctx.source` (the firing instance,
threaded by Seam 6), locates it across all investigators' `threat_area`
and all locations' `attachments`, removes it, pushes its code to
`encounter_discard`, and emits `Event::CardDiscarded { from: <the zone it
was found in> }`. Rejects loudly if `source` is `None` or the instance is
not found (a forced self-discard must have a source).

`TODO`: scoped to the two encounter zones (threat area, location
attachment → `encounter_discard`). Extend to player-controlled zones
(`cards_in_play` → the owner's `discard`) when a player card first needs
to discard itself by source instance.

### Seam 5 — per-round surcharge tracking (per source instance)

`Investigator` gains `action_surcharge_spent_this_round: BTreeSet<CardInstanceId>`
(`BTree` for deterministic serde). Reset to empty for every investigator
at the round boundary (the round-increment site reached on Mythos entry —
located during implementation; co-located with the existing round
counter). Keying by **source instance** (not a single bool) means two
copies of Frozen in Fear each charge their own first action (+2 total,
rules-correct) and a future independent surcharge tracks separately.

Move/fight/evade handlers change from "spend 1 action" to:
1. `let (extra, to_mark) = pending_action_surcharge(state, reg, inv, class);`
2. `let cost = 1 + extra;`
3. validate `actions_remaining >= cost` (reject otherwise — validate-first,
   nothing marked);
4. on commit: spend `cost` actions and insert `to_mark` into the
   spent-set.

This needs a `spend_actions(cx, inv, n)` generalizing the existing
`spend_one_action`.

### Seam 6 — forced-trigger source threading + scan extensions

`ForcedHit` gains `source: Option<CardInstanceId>`. `resolve_one` uses
`EvalContext::for_controller_with_source` when `source` is `Some`, else
`for_controller`. In `collect_forced_hits`:

- **`EndOfTurn`** (already scans the investigator's controlled instances)
  and **`AfterLocationInvestigated`** — bind `source = the scanned
  instance`. Additionally extend `AfterLocationInvestigated` to scan the
  **investigated location's `attachments`** (Obscuring Fog attaches to the
  location, not the threat area), binding `source = the attachment
  instance`, `controller = the investigating investigator`.
- **`RoundEnded`** (today scans only the current act + agenda, board
  cards, `source = None`) — additionally scan **each investigator's
  controlled instances** for `RoundEnded` forced abilities, binding
  `source = instance`, `controller = that investigator` (Dissonant Voices
  lives in the threat area).

Board-card hits (act/agenda) keep `source = None`.

### Seam 7 — success-side skill test (`on_success`)

`Effect::SkillTest` today carries only `on_fail` (the failure-side
follow-up from #286). Frozen in Fear discards on **success**, so add a
symmetric `on_success: Option<Box<Effect>>`. Thread it through
`start_skill_test` → `InFlightSkillTest` (which must also remember the
**source instance**, so the `on_success`/`on_fail` eval-contexts carry it
across the suspend/resume) → run the `on_success` effect at skill-test
teardown on success, mirroring where `on_fail` runs on failure. This
closes a real asymmetry; Frozen in Fear is the concrete driver.

## The three card impls

Each is a module `crates/cards/src/impls/treachery_<code>.rs` with `CODE`,
`abilities()`, a card-local placement native (placement keeps the card's
own `CODE`, which the evaluator doesn't carry), and tests. Discard and
restriction enforcement are engine-side (Seams 3/4), so the impls stay
thin. The forced abilities are `Trigger::OnEvent { pattern, timing: After }`
constructed via the existing forced-ability builder (the one C3c/C4a used
for agenda/location forced effects — exact builder name confirmed during
implementation).

- **Obscuring Fog 01168** — `revelation(native(LIMIT1_ATTACH))` (if the
  controller's location already holds an 01168 attachment → push code to
  `encounter_discard` + emit `CardDiscarded`; else `attach_to_location`);
  `constant(modify(Stat::Shroud, 2, WhileInPlay))`;
  `forced_after(AfterLocationInvestigated, DiscardSelf)`.
- **Dissonant Voices 01165** — `revelation(native(TO_THREAT_AREA))`
  (`place_in_threat_area(cx, ctx.controller, CardCode::new(CODE))`);
  `constant(restrict(CannotPlay(Asset)))` + `constant(restrict(CannotPlay(Event)))`;
  `forced_after(RoundEnded, DiscardSelf)`.
- **Frozen in Fear 01164** — `revelation(native(TO_THREAT_AREA))`;
  `constant(restrict(ExtraActionCost { {Move,Fight,Evade}, first_each_round: true }))`;
  `forced_after(EndOfTurn, SkillTest(Willpower, 3, on_success = DiscardSelf, on_fail = none))`.

## Test plan (TDD, simpler cards first to land the seams)

1. **Attachment zone + helpers** — `attach_to_location` mints/pushes/emits;
   `Location::new` defaults attachments empty; serde roundtrip.
2. **Derived persistence** — `resolve_encounter_card` auto-discards an
   all-Revelation treachery (regression) but *not* one with an ongoing
   ability (unit, synthetic abilities).
3. **`Stat::Shroud` + `effective_shroud`** — printed + attachment modifiers.
4. **`Effect::DiscardSelf`** — finds + discards a threat-area instance and
   a location-attachment instance, right `from` zone; rejects with no
   source / unknown instance.
5. **Obscuring Fog** (card test, integration with registry) — attach,
   +2 effective shroud at its location, limit-1 discards the second copy,
   `AfterLocationInvestigated` discards it; investigate uses raised shroud.
6. **`Restriction::CannotPlay` + `play_is_prohibited`** — `play_card`
   rejects an asset/event while the restriction is active; unrelated
   types unaffected.
7. **Dissonant Voices** (card test) — to threat area, blocks
   asset/event play, `RoundEnded` discards it.
8. **`on_success` skill test + per-round surcharge** — success runs the
   `on_success` effect; `pending_action_surcharge` charges the first
   gated action and not the second (same round), resets next round; two
   sources each charge.
9. **Frozen in Fear** (card test) — to threat area, first move/fight/evade
   that round costs 2 actions and subsequent ones cost 1, surcharge resets
   next round, `EndOfTurn` willpower(3): success discards / failure keeps.
10. **Full strict gauntlet** (fmt, clippy host + wasm, test, doc, wasm-build).

## Out of scope / explicit non-goals

- Player-card self-discard zones (Seam 4 `TODO`).
- The general "first-applicable each round/turn" scope (Seam 5 `TODO`).
- Orne Library's location-gated (un-gated) `ExtraActionCost` — Dunwich
  scenario content, Phase 10.
- Multi-investigator interactions beyond what the per-instance keying
  already gives for free.
