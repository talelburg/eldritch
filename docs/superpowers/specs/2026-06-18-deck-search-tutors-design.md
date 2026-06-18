# Deck-search tutors — `Effect::SearchDeck` + Old Book of Lore + Research Librarian

**Date:** 2026-06-18
**Phase:** 7 (The Gathering) — Slice-1 follow-up, Axis-E carved Seeker cards
**Issues:** [#319](https://github.com/talelburg/eldritch/issues/319) (Old Book of Lore 01031), [#320](https://github.com/talelburg/eldritch/issues/320) (Research Librarian 01032)
**Ships as:** one PR (primitive + both cards).

## Problem

Two Core Seeker cards were carved out of C6b (#242) because they need a
deck-search/tutor primitive the engine lacks. Verbatim text
(`data/arkhamdb-snapshot/pack/core/core.json`):

- **Old Book of Lore 01031** (Item. Tome. asset, cost 3, Hand slot):
  "[action] Exhaust Old Book of Lore: Choose an investigator at your
  location. That investigator searches the top 3 cards of his or her deck
  for a card, draws it, and shuffles the remaining cards into his or her
  deck."
- **Research Librarian 01032** (Ally. Miskatonic. asset, cost 2, Ally slot,
  health 1 / sanity 1): "[reaction] After Research Librarian enters play:
  Search your deck for a [[Tome]] asset and add it to your hand. Shuffle
  your deck."

Both want: look at a region of a deck (top N, or the whole deck), select
one matching card, move it to hand, shuffle. The engine has no
deck-inspection/select primitive — only draw-from-top.

## Rules grounding (RR p.18, "Search")

> "When a player is instructed to search for a card, that player is
> permitted to look at all of the cards in the searched area without
> revealing those cards to the other players."
> - "If an effect searches an entire deck, the deck must be shuffled upon
>   completion of the search."
> - "When resolving a search effect, a player is obligated to find the
>   object of the search should one or more eligible options be found
>   within the searched area."

Load-bearing consequence: **there is no decline.** If ≥1 eligible card
exists the searcher *must* take one; only when zero eligible cards exist
does the search "find nothing." This maps cleanly onto the existing choice
convention (`resolve_choice_count`): 0 ⇒ find-nothing, 1 ⇒ auto-take,
2+ ⇒ suspend. Both cards shuffle on completion (Old Book shuffles its
remaining top-3 back; Librarian searches the entire deck → mandatory
shuffle).

## Design

### Component 1 — `Effect::SearchDeck` (the primitive)

A typed, inspectable effect (matching the repo idiom: `Fight`,
`Investigate`, `Heal`, `DrawCards` are all typed data — `Effect::Native`
is reserved for one-offs, and deck-search has 2 consumers now plus many
future Seeker tutors):

```rust
Effect::SearchDeck {
    target: InvestigatorTarget,   // whose deck
    scope:  SearchScope,
    filter: Option<CardFilter>,
}

pub enum SearchScope { Top(u8), EntireDeck }

pub struct CardFilter {
    pub trait_: Option<&'static str>,   // e.g. "Tome"
    pub kind:   Option<CardType>,       // e.g. Asset
}
```

`filter` matches a candidate code by reading `card_registry::metadata_for`
(traits + `CardType`); `None` matches every card. Both predicates, when
present, must hold (trait AND type).

**Resolution** (validate-first / mutate-second; the take + shuffle are the
only mutations, and they run *after* every choice resolves):

1. Resolve `target` to an investigator. Old Book's `Chosen(investigator At
   Here)` grounds through the existing `ground_chosen_targets` path — add
   `Effect::SearchDeck { target, .. }` to that match arm so its `Chosen`
   target binds `chosen_investigator` (and may suspend on 2+ co-located
   investigators) before the handler runs.
2. Enumerate eligible candidates from that investigator's `deck`, in deck
   order (deterministic, so `OptionId` indices replay): `Top(n)` ⇒ the
   first `n` codes; `EntireDeck` ⇒ all codes; then `filter` applied,
   preserving order.
3. Apply the choice convention, **overriding `Empty`**: 0 eligible ⇒
   find-nothing (skip step 4; *not* a reject); 1 ⇒ auto-take; 2+ ⇒ suspend
   for a pick (reuse the Axis-A `ChoiceFrame`/`DecisionCursor` exactly like
   `Effect::ChooseOne` — `cursor.take()` to replay, `suspend_for_choice`
   to suspend, candidate labels = card names).
4. Remove the chosen code from `deck`, push to `hand`, emit a card-moved
   event.
5. Shuffle the investigator's `deck` via the existing RNG-replayable
   shuffle helper (emits `Event::DeckShuffled`). Runs for both scopes
   (Old Book's "shuffle the remaining cards into the deck" = a whole-deck
   shuffle after the take; Librarian's mandatory entire-deck shuffle).
   When step 3 found nothing, still shuffle for `EntireDeck` (RR), and
   harmlessly for `Top(n)` (a top-N search that took nothing left the deck
   unchanged; shuffling is a no-op-equivalent and matches Old Book's
   "shuffle the remaining cards" even in the degenerate empty case).

**Draw-vs-add-to-hand:** Old Book says "draws it", Librarian says "add to
your hand". Both land in hand; the only rules difference is whether
on-draw triggers fire, and **no Core card has an on-draw trigger**. Model
both as a single move-to-hand and treat the distinction as
unobservable-in-scope (deferred until a draw-trigger card lands). The
emitted event is a card-moved-to-hand event (not necessarily reusing
`CardsDrawn`); chosen at implementation time to avoid implying a "draw"
that future on-draw triggers would wrongly key off.

**Why the nested target+select choice is safe:** Old Book has two suspend
points (choose investigator, then choose card). The `DecisionCursor`
replays picks in pre-order; single-pass replay re-runs the whole effect on
each resume. This is safe here because **no state mutates until after both
choices** — grounding binds `chosen_investigator` (no mutation), the
select binds the pick (no mutation), and only step 4/5 mutate. This is
within Axis A's single-pass model; no new choice-frame plumbing. In solo
with one investigator the target auto-binds (1 candidate), so only the
card-select can suspend — the common case.

### Component 2 — `EnteredPlay` reaction trigger (for Research Librarian)

There is currently no enters-play reaction `EventPattern`. Research
Librarian's "[reaction] After ... enters play" needs:

- A new `EventPattern::EnteredPlay` (reaction `TriggerKind`).
- An emit point: after an asset lands in `cards_in_play` in the play-card
  path, open a reaction window scanning the controller's controlled
  instances for matching `OnEvent` reactions — riding the **existing**
  reaction-window pipeline (the same machinery as `BeforeDiscoverClues` /
  `AfterSuccessfulInvestigate` / Axis-C). The play-card path suspends and
  resumes around that window (the reaction's `SearchDeck` may itself
  suspend for the card-select).

Scope guard: the window is controller-scoped ("after *it* enters play");
self-referential ("Research Librarian enters play") is satisfied because
the reaction lives on the just-entered instance and the pattern fires for
that entry. Other cards' enters-play reactions are out of scope but the
pattern is general.

### Component 3 — the two cards (data)

- **Old Book of Lore 01031** —
  `activated([action], [Exhaust], SearchDeck { target: Chosen(investigator
  At Here), scope: Top(3), filter: None })`.
- **Research Librarian 01032** —
  `reaction(EnteredPlay (self), SearchDeck { target: You, scope:
  EntireDeck, filter: Some(CardFilter { trait_: Some("Tome"), kind:
  Some(Asset) }) })`. Plus the Ally slot / health 1 / sanity 1 are corpus
  metadata (no hand-typing).

## Testing

**Infra (synth-card / engine tests):**
- `Top(n)` with 0 eligible ⇒ no take, deck/hand unchanged except shuffle;
  1 eligible ⇒ auto-take to hand + shuffle; 2+ ⇒ suspend then `PickSingle`
  takes the chosen card.
- `EntireDeck` + `CardFilter` (trait + type): only matching cards are
  candidates; non-matching ignored; mandatory shuffle on completion.
- Old Book nested target-then-select: 2 co-located investigators ⇒
  suspend on the investigator pick, resume, then suspend on the card pick,
  resume → correct investigator's deck searched.
- Shuffle is RNG-replayable (replay reproduces the same post-search deck).

**Cards (per-card tests in `crates/cards/src/impls/<name>.rs`):**
- Old Book: `[action]` + exhaust cost; searches the active investigator's
  top 3; drawn card in hand; deck shuffled; card exhausts.
- Research Librarian: entering play opens the reaction; searching pulls a
  Tome asset (seeded in deck) to hand; a deck with no Tome asset ⇒
  find-nothing; deck shuffled.

Integration coverage (real registry + metadata) lives in
`crates/cards/tests/` if the per-card + synth tests don't reach a
registry-dependent path (the `CardFilter` metadata lookup does — Research
Librarian's filter needs `metadata_for`, so at least one test must run
with `cards::REGISTRY` installed).

## Decisions

- **One unified `Effect::SearchDeck`, not two narrow effects nor
  `Effect::Native`.** Two consumers share the inspect→select→shuffle core
  now; the repo rule ("share a variant when ≥2 cards reuse the pattern")
  is met, and selecting from a hidden zone is a genuine reusable kernel
  capability.
- **No decline option** (RR p.18 "obligated to find ... should one or more
  eligible options be found"); `resolve_choice_count`'s `Empty` arm is
  overridden to find-nothing rather than reject.
- **Move-to-hand models both "draws it" and "add to hand"**; the draw/add
  distinction is unobservable in Core (no on-draw triggers) and deferred.
- **`EnteredPlay` rides the existing reaction-window pipeline**, not a new
  subsystem.

## Out of scope / deferred

- On-draw triggers (the draw-vs-add distinction) — no Core consumer.
- General enters-play reactions beyond the self-referential case — the
  pattern is general but only Research Librarian exercises it here.
- Mind over Matter 01036 (#322) and Barricade 01038 (#323) — the other two
  carved Seeker cards, separate sub-slices (different machinery).
