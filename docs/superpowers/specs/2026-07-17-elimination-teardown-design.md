# Elimination teardown — design

**Date:** 2026-07-17
**Status:** approved (brainstorm), pending implementation plan
**Phase:** unmilestoned (audit fallout) · **Issues:** #564 (p0/Critical), #567 (p1/High)
**Related:** #144 (the elimination flow this extends), #371 (location-attachment ownership), #566 (the inverse GameEnd case), #572 (defeated-active-investigator's turn)

## Goal

An eliminated investigator is out of the scenario, so nothing of theirs should still
be driving the engine. Today two things survive elimination:

1. A **live `SkillTest` frame** whose tester was just eliminated indexes the hand that
   elimination drained → index-out-of-bounds **panic**, deterministically replayed
   from the persisted log (#564).
2. The **threat area** is never drained, and the `RoundEnded`/`GameEnd` forced scans
   never filter by `Status` — so a dead investigator's cards keep firing (#567).

Both are post-#144 gaps: #144 shipped RR p.10 steps 1–6 but predates the threat-area
zone and never covered mid-test interleavings.

## The rules reading (load-bearing — read this before changing the drain)

This design's destinations are **not** the ones #567's body suggests. The reasoning
is subtle and was re-derived twice during the brainstorm, so it is recorded here in
full rather than left to a future author to rediscover.

### Verbatim sources

All quotes are from the pinned `data/rules-reference/ahc01_rules_reference_web.pdf`.
**Do not use ArkhamDB's `/rules` page for the Elimination entry** — a WebFetch of it
during this brainstorm returned a *fabricated* six-step list that contradicted both
the PDF and our shipped code. The card pages (`/card/<code>`) fetched faithfully.

**p.10, "Elimination":**

> 1. The cards he or she controls in play and all of the cards in his or her
>    out-of-play areas (such as hand, deck, discard pile) are removed from the game.
>    - Any card that player owns but does not control that is in play remains in
>      play, but if that card leaves play it is removed from the game.
> 2. All clue tokens that player possesses are placed at the location the
>    investigator was at when he or she was eliminated, and all of that player's
>    resource tokens are returned to the token pool.
> 3. All enemies engaged with that player are placed at the location the investigator
>    was at when he or she was eliminated, unengaged but otherwise maintaining their
>    current game state.
> 4. All other cards in the eliminated investigator's threat area are placed in the
>    appropriate discard pile.
> 5. If the lead investigator is eliminated, the remaining players (if any) choose a
>    new lead investigator.
> 6. If there are no remaining players, the scenario ends. […]

**p.21, "Weakness":**

> When an investigator draws a weakness with an encounter cardtype (for example, an
> enemy or a treachery weakness), resolve that card as if it were just drawn from the
> encounter deck.
>
> Weaknesses with an encounter cardtype are, like other encounter cards, **not
> controlled by any player**. Weaknesses with a player cardtype are controlled by
> their bearer.

**p.7, "Dealing Damage/Horror"** — the RR's own gloss on "the appropriate discard pile":

> …if an enemy has damage equal to or higher than its health, it is defeated and
> placed in the encounter discard pile (**or in its owner's discard pile if it is a
> weakness**).

**p.16, "Ownership and Control":**

> A card's owner is the player whose deck (or game area) held the card at the start of
> the game. […] Cards by default enter play under their owner's control. Some
> abilities may cause cards to change control during a game.

### Where each threat-area card goes, and why

| Card | Cardtype | Controlled by | Owner | Destination |
|---|---|---|---|---|
| Cover Up 01007 | treachery **weakness** | nobody (p.21) | Roland | **`removed_from_game`** (step 1) |
| Frozen in Fear 01164 | treachery (encounter) | nobody | the scenario | **`encounter_discard`** (step 4) |
| Dissonant Voices 01165 | treachery (encounter) | nobody | the scenario | **`encounter_discard`** (step 4) |

**Encounter treacheries are the easy half.** They are owned by the scenario, not by
the investigator, so nothing about his elimination should remove them from the game.
Step 4 sends them to the encounter discard. This is the half #567's suggested fix
("drain `threat_area` into `removed_from_game`") gets **wrong**.

**Cover Up is where it gets subtle.** The literal chain is:

- It is a *treachery* weakness = an **encounter cardtype** ⇒ p.21 says it is "not
  controlled by any player" ⇒ step 1's clause ("the cards he or she **controls** in
  play") does **not** take it.
- So step 4 takes it, to "the appropriate discard pile" ⇒ per the p.7 parenthetical,
  a weakness goes to **its owner's discard pile** ⇒ Roland's discard.
- But step 1 **already removed Roland's discard pile from the game**, one step
  earlier.

The literal reading therefore prescribes placing a card into a pile that no longer
exists. That is incoherent text, not a subtlety we should faithfully model.

**Resolution:** for a card whose owner has left the game, "the appropriate discard
pile" does not exist — so the card is removed. We model Cover Up as
`removed_from_game` at step 1. This is **observationally identical** to the literal
reading (nothing in the engine reads a non-`Active` investigator's `discard`, and
Phase 9 rebuilds an eliminated investigator's deck from their decklist regardless),
and it avoids carrying a lone card in a dead player's pile.

So #567's suggested fix is **half-right**: right for Cover Up, wrong for the two
encounter treacheries.

### Discriminator: `CardMetadata::weakness`

`weakness: true` → owner is a player → `removed_from_game`.
`weakness: false` → owner is the scenario → `encounter_discard`.

This is the axis the RR itself names in the p.7 parenthetical. It is already ingested
(no pipeline change), and it is exact across the whole in-scope corpus — every
threat-area-capable card in Core+Dunwich is either a `neutral`/no-`encounter_code`
weakness or a `mythos`/`encounter_code` treachery.

Rejected alternatives:

- **Ingest `encounter_code`** (a slice of #579): more literally "is this an encounter
  card", but it is the *wrong* predicate — a weakness distributed in an encounter set
  (12137 Mark of Elokoss, out of scope) is still player-owned. It would also pull
  pipeline + corpus regen into this PR.
- **`CardKind` class**: `CardKind::Treachery` carries no `class`, so it cannot
  distinguish Cover Up from Frozen in Fear at all.

**No registry installed** ⇒ `metadata_for` returns `None` ⇒ treat as non-weakness
(encounter discard). Only reachable from engine-only tests that place synthetic
threat-area cards; documented at the call site.

### Location attachments (Barricade 01038): out of scope, and correct as-is

**Decision: a location attachment is owned but *not controlled* by the player who
played it, so the step-1 sub-bullet governs — it remains in play, and is removed from
the game only if it later leaves play.** A dead investigator's Barricade continuing
to block enemy movement is therefore **correct**, not the bug #567's body claims.

Supporting: RR p.13 "In Play and Out of Play" scopes a player's controlled in-play
cards to "the cards that a player controls **in his or her play area**" — an
attachment sits under a *location*, not in the player's play area. "Attach To" (p.4)
describes only physical placement ("placed beneath and slightly overlapped by") and
transfers nothing. "In play, controlled by nobody" is a
category the RR blesses explicitly (p.21, encounter cards). And Barricade's own Forced
reads "When **an investigator** leaves attached location" — not "you" — so the card
keys on the location, not on its player.

**The counter-argument, recorded so a future author need not re-derive it:** "Cards by
default enter play under their owner's control. Some abilities may cause cards to
change control during a game" — attaching is not such an ability, so there is arguably
no moment at which control is lost. The "In Play and Out of Play" line is defining
*in play*, not defining *control*. There is **no FAQ or ruling** on `/card/01038`
resolving this (checked); the RR is simply incomplete for player-card
location-attachments. This design takes the first reading.

Either way **this PR touches no attachment code** — under the other reading Barricade
would need the controller field from #371 and would be deferred anyway.

Related wrinkle, not addressed here: eliminating an investigator *at* a barricaded
location sets `current_location = None`, which plausibly should trip Barricade's own
Forced. Whether elimination emits a leave-location event is a separate question; not
in scope.

## Part A — abandon the in-flight test (#564)

### The gate

`skill_test::advance`'s loop head already has a top-frame check. Immediately after it
(and after the existing `(continuation, investigator, indices_u8)` read):

```rust
// RR p.10 step 1 removed this investigator's cards from the game — including the
// hand the committed indices point into. Abandon the test rather than resolve it
// on behalf of someone who has left the scenario.
if tester_status != Status::Active {
    return abandon_test(cx, investigator);
}
```

`advance` is the driver's **only** entry point — both the `drive` loop's `SkillTest`
arm and the `finish_skill_test` commit hop go through it — so one gate covers every
path. This mirrors the documented `AttackLoop` early-break (`combat.rs:601`), which
breaks on `Status != Active` for the same reason.

### `abandon_test`

Mirrors the existing `PostOnResolution` teardown **minus the discard**:

- emit `Event::SkillTestEnded { investigator }` — the test *is* over, and it is the
  documented "test is fully over" signal listeners key on;
- drain this investigator's `pending_skill_modifiers` (as the normal teardown does —
  `ModifierScope::ThisSkillTest` contributions must not leak);
- `cx.state.take_skill_test()` to pop the frame (removes by `rposition`, so a player
  window legitimately above it is unaffected);
- return `EngineOutcome::Done`.

**No discard**, because step 1 already removed the committed cards from the game —
they were still in hand (the driver discards only at teardown). Discarding would
resurrect them into a pile.

### Rejected: tear down at elimination time

Having `run_elimination_steps` call `take_skill_test()` would put all elimination
cleanup in one place, but it mutates the continuation stack from *beneath a live
effect* — the `on_fail` Deal that caused the defeat is still executing above it — and
anything reading `current_skill_test()` mid-effect (`ModifierScope::ThisSkillTest`
matching) would then see `None`.

### Defensive totality

The three hand-indexing helpers become total: `.get()`-based, with a `debug_assert!`
tripwire on miss.

- `collect_on_commit` (`skill_test.rs:1296`)
- `collect_on_skill_test_resolution` (`skill_test.rs:1236`)
- `discard_committed_cards` (`skill_test.rs:1053`)

There is a **fourth** `inv.hand[usize::from(i)]` at `skill_test.rs:940`, deliberately
left alone: it is in the commit-time validation path, immediately behind an explicit
`if (i as usize) >= hand_len` bounds check, and runs while the tester is necessarily
`Active`. Not a panic site.

Unreachable once the gate lands — this is a structural backstop so no future path can
re-introduce a panic in a production `apply` (a panic is *not* covered by `apply_via`'s
Rejected-only rollback: it kills the apply as a wasm trap / server task crash). The
tripwire (rather than a silent skip) keeps this inside the repo's
no-silent-approximation rule, matching the existing debug-only guards (#294, #571).

## Part B — the threat-area drain (#567 half 1)

In `run_elimination_steps`:

- **Step 1** — after the existing `cards_in_play` / `hand` / `deck` / `discard`
  drains, also drain threat-area cards with `weakness: true` into
  `removed_from_game`. In-scope: Cover Up 01007. (The other treachery weaknesses —
  Hospital Debts 01011, Haunted 01098, Psychosis 01099, Hypochondria 01100, Rex's
  Curse 02009, Wracked by Nightmares 02015, Internal Injury 02038, Chronophobia
  02039 — are in the corpus but unimplemented; they route correctly for free.)
- **Step 4** — where the stale "KNOWN GAP (#567)" comment currently sits, drain what
  remains (`weakness: false`) to the encounter discard by calling the **existing**
  `threat_area::discard_from_threat_area` per instance. It already does exactly this
  (remove by `instance_id` → `encounter_discard` → `CardDiscarded { from:
  Zone::ThreatArea }`) and needs **no change**.

`discard_from_threat_area` finally gains a production caller, so its
`#[cfg_attr(not(test), allow(dead_code))]` and its stale "C4c (#235) is the first
production caller" comment (that never happened — `Effect::DiscardSelf` took over)
both go.

The stale step-4 comment's claim that threat-area cards are "not modeled yet" is
deleted.

**Explicitly not done:** routing `evaluator::discard_self`'s threat-area arm by
ownership. It hardcodes `encounter_discard`, which is correct for every card that
actually self-discards in scope (01164, 01165 — both non-weakness). Rerouting a
hypothetical self-discarding weakness is speculative; no in-scope card does it.

## Part C — Status-filter the forced scans (#567 half 2)

`collect_forced_hits`' `RoundEnded` and `GameEnd` arms each iterate **all**
investigators (`forced_triggers.rs:322` and `:415` — #567's body cites 320/413, which
are a couple of lines off). Filter both to `Status::Active`.

**This is not belt-and-suspenders, contra #567's body.**
`controlled_card_instances()` yields `investigator_card` **+** `cards_in_play` **+**
`threat_area`. Step 1 drains `cards_in_play`; Part B drains `threat_area`. But
`investigator_card` is a non-`Option` field carrying identity/harm/usage since #448
and **structurally cannot be drained** — `max_health()`/`max_sanity()` read it. The
Status filter is the *only* guard for it. Latent today (Roland's investigator card
carries a reaction, not a `RoundEnded`/`GameEnd` forced), but it is the real fix.

Both maps are `BTreeMap`s, so filtering preserves the frozen deterministic
enumeration order (#570's contract).

## Testing

**Integration** (`crates/cards/tests/` — needs real metadata + abilities, which
`game-core` cannot reach by crate direction):

- Commit a card, fail Grasping Hands 01162 at lethal range → **no panic**; committed
  cards land in `removed_from_game` (not `discard`); the frame is popped; exactly one
  `SkillTestEnded`.
- Same interleaving, **replayed** from the action log → bit-for-bit identical state
  (#564's acceptance criterion; a panic today reproduces on every replay).
- Roland eliminated with clues on Cover Up 01007, run to resolution → **no** mental
  trauma; Cover Up in `removed_from_game`.
- Eliminated investigator with Dissonant Voices 01165 in the threat area → no further
  round-end forceds; the card is in `encounter_discard`.

**Engine unit** (`elimination.rs` `#[cfg(test)]`, `test_support` + test registry):

- Step-1 drain: a `weakness: true` threat-area card → `removed_from_game`.
- Step-4 drain: a `weakness: false` threat-area card → `encounter_discard`, with
  `CardDiscarded { from: Zone::ThreatArea }`.
- `threat_area` empty after elimination in both cases.
- No registry installed → treated as non-weakness (encounter discard), no panic.

Event assertions use the `assert_event!` / `assert_no_event!` macros.

## What this deliberately leaves open

- **#371** — location-attachment ownership/controller field. Unblocked by this PR;
  see the Barricade section for the reading it should implement against.
- **#566** — the inverse case (a *live* investigator's `GameEnd` forced being
  swallowed by `fire_scenario_resolution`).
- **#572** — "defeated active investigator's turn doesn't end", the same
  elimination-interleaving family. Part A's gate is test-local by design; a general
  "tear down frames belonging to an eliminated investigator" pass belongs there.
</content>
