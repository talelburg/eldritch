# Phase 7.5 — Investigator breadth

## Status

⏳ **planned** (opened 2026-08-23). No issues started; every one is filed.

This milestone was **Slice 2** in `phase-7-the-gathering.md`'s Future slices, an
unfiled intent. It became a milestone on 2026-08-23 (`/grill-with-docs`) when
measuring the gap turned it into a body of work rather than a bullet.

## Goal

All five core investigators playable, each with their signature pair and a legal
deck, chosen from a real picker.

The stated reason for doing this is **discovery** — writing forty-odd card effects
and four elder signs is a stress test of the DSL and the effect evaluator that no
amount of reading the corpus substitutes for. That is also why it runs *after* the
phase-7 gate rather than alongside it: with the gate open, a new failure has two
candidate causes, and the point of the exercise is that a broken elder sign is
unambiguously the elder sign.

## The gap, measured

Level-0 core player cards, against `crates/cards/src/impls/`:

| Class | Level-0 titles | Unimplemented |
|---|---|---|
| Guardian | 10 | **0** |
| Seeker | 10 | **0** |
| Mystic | 10 | 9 |
| Rogue | 10 | 10 |
| Survivor | 10 | 10 |
| Neutral (signatures + weaknesses) | 26 | 15 |
| Investigator cards | 5 | 4 |

Counting real titles: `01000 "Random Basic Weakness"` is a deckbuilder placeholder
meaning *"draw one of the seven"*, not a card, so it is excluded from the neutral row
and lives in #781.

Roland's two classes are already complete, which is exactly why breadth costs what
it costs: every card the gate needed was a Guardian, a Seeker or a neutral skill.

**Level 0 only.** The 22 XP cards in the core pool have no consumer until phase 9
builds the campaign upgrade flow — no deck can legally contain one, so they would
ship untested by anything but a unit test.

## Issues

| Issue | Work |
|---|---|
| #776 | `Trigger::ElderSign` grows an on-success effect — the prefactor |
| #366 | Replacement effects beyond cancel — Wendy's blocker, re-milestoned here |
| #777 | Slice 1 — Agnes Baker 01004 + Heirloom / Dark Memory + Mystic level 0 |
| #778 | Slice 2 — "Skids" O'Toole 01003 + On the Lam / Hospital Debts + Rogue level 0 |
| #779 | Slice 3 — Daisy Walker 01002 + Tote Bag / Necronomicon + the Tome-restricted extra action |
| #780 | Slice 4 — Wendy Adams 01005 + Amulet / Abandoned and Alone + Survivor level 0 |
| #781 | The seven basic weaknesses + the random-basic-weakness draw |
| #782 | Five legal starter decks + a real investigator picker |

#783 (the deckbuilding validator) was filed alongside #782 but milestoned to
**phase 9**, where its real consumer is.

## Ordering

**#776 → #777 → #778 → (#366) → #779 → #780 → #781 → #782.**

**Vertical slices, one investigator at a time** — investigator card, signature
pair, and that investigator's class pool, each slice ending in someone playable.
Horizontal batches (all elder signs, then all assets) were rejected: a batch of
half-wired assets is not a stress test, and a playable investigator is.

**#776 is a standalone prefactor**, on the pattern this repo used for #706 before
#707: three of the four remaining investigators need it, so folding it into a slice
would make one investigator's PR the home for everyone's foundation.

**Ascending difficulty**, which also puts each new capability under the cheapest
card that needs it. Agnes' elder sign fits the existing modifier-only shape once
#776 adds a horror `Quantity`, and her ability is a reaction on herself — Roland's
shape. Skids is the simplest consumer of the *on-success effect*, so that field
gets exercised before Daisy's trait-counting leans on it. Wendy is last because she
is blocked on a `needs-design` issue.

## What the elder signs force

This is where the stress test bites. Verbatim from
`data/arkhamdb-snapshot/pack/core/core.json`:

| Card | `[elder_sign]` effect | What it needs |
|---|---|---|
| Roland 01001 | `+1 for each clue on your location.` | ✅ shipped (#118) |
| Agnes 01004 | `+1 for each horror on Agnes Baker.` | a horror `Quantity` |
| "Skids" 01003 | `+2. If you succeed, gain 2 resources.` | a success-conditional **effect** |
| Daisy 01002 | `+0. If you succeed, draw 1 card for each [[Tome]] you control.` | that, plus trait-counting |
| Wendy 01005 | `+0. If Wendy's Amulet is in play, you automatically succeed instead.` | an outcome override |

`Trigger::ElderSign { modifier: IntExpr }` models exactly Roland's shape — a number
added to the chaos-token `Modifier` total at ST.4 — and has no field for an effect.

**The one question worth arguing about is Wendy's**, and it is deliberately isolated
in #776 rather than buried in her slice: is *"you automatically succeed instead"* an
elder-sign effect at all, or a `SkillTestOutcome` override the elder sign merely
triggers? Whatever shape wins must preserve what
`data/official-faq/Rulings_and_Clarifications.md` establishes with its Hope/Patrice
worked example — an automatically-successful test **still takes place**: cards commit,
the modified skill value is still determined, and only ST.3 and ST.4 are skipped. The
same document distinguishes that from Stray Cat 01076's *"Automatically evade"*, where
*"no skill test is made whatsoever."* Auto-success is not "skip the test", and the two
must not be conflated — 01076 is in Wendy's own slice.

## Decks are constructed, not transcribed

The core box's suggested decklists are **not vendored** — not in
`data/arkhamdb-snapshot/`, not in `data/official-faq/` — and per CLAUDE.md they must
not be fetched. What *is* vendored is each investigator's deckbuilding requirements,
printed on the card back and ingested as `back_text` (#558/PR #559). Agnes 01004,
verbatim:

> **Deck Size**: 30. **Deckbuilding Options**: Mystic cards ([mystic]) level 0-5, Survivor cards ([survivor]) level 0-2, Neutral cards level 0-5. **Deckbuilding Requirements** (do not count toward deck size): Heirloom of Hyperborea, Dark Memory, 1 random basic weakness.

A legal 30-card deck is constructible from the level-0 pool alone: each class has
exactly 10 level-0 titles and deckbuilding permits 2 copies per title, so **10 class
titles × 2 = 20, plus 5 neutral skills × 2 = 10 → 30**, with the signature pair and
the basic weakness outside the count. That recipe works for all five.

#782 hardcodes those five, each with a comment naming the recipe it was built from,
and retires `ROLAND_DEFAULT_DECK` — documented in place today as *"NOT a legal 30+1
deck — a scaffold for UI testing."* The **validator** (#783) is separate on purpose:
#782's decks are legal by construction, so a validator adds no correctness there, and
its real consumer is phase 9's decklist import, where decks arrive from outside the
repo and cannot be trusted.

## Open questions

- **Does the trait-counting `Quantity` (`[[Tome]]`s you control) belong to #776 or to
  Daisy's slice (#779)?** Both are defensible; #776 is where it gets decided.
- **Does Forbidden Knowledge 01058 force #353?** It is one of the two cards
  uses-depletion auto-discard has always been gated on, and #777 is where it acquires
  a consumer. If the card cannot ship correctly without #353, that issue comes in
  rather than the card being approximated.
- **How is a *restricted* additional action represented?** Daisy's is not an action
  count — it is an action of a distinguished kind, spendable only on `[[Tome]]`
  `[action]` abilities, and the turn-menu enumerator has to know which options it can
  pay for. #778 settles the unrestricted shape (Skids, Leo De Luca 01048) in a way
  #779 can extend.

## Dependencies

Phase 7's gate, closed. Nothing here should start while a rules defect in the gate is
open — that is the whole reason for the ordering.

#366 (replacement effects beyond cancel) is `needs-design` and blocks #780 alone.

## What "done" looks like

Five radios in the picker; picking any of the five seats that investigator with a
legal deck and a random basic weakness, and plays The Gathering to a resolution.
Difficulty stays Standard and the player count stays 1 — those are #862 (phase 9)
and #863 (phase 8) respectively, both spun out of phase-7's Future slices when that
phase closed.
