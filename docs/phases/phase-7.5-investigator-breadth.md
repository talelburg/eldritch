# Phase 7.5 — Investigator breadth

## Status

✅ **Closed 2026-10-29** (opened 2026-08-23). All five core investigators are
playable from the picker, each with a legal default deck; every issue on the
milestone is closed.

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
| Mystic | 10 | **0** |
| Rogue | 10 | **0** |
| Survivor | 10 | **0** |
| Neutral (signatures + weaknesses) | 26 | **0** |
| Investigator cards | 5 | **0** |

Counting real titles: `01000 "Random Basic Weakness"` is a deckbuilder placeholder
meaning *"draw one from the weakness pool"* (10 copies over 8 titles; see GLOSSARY.md),
not a card, so it is excluded from the neutral row, and the draw it stands for is
#1116's. No neutral card is left: Abandoned and Alone 01015 and the basic
weaknesses, Amnesia 01096, Paranoia 01097, Haunted 01098, Psychosis 01099,
Hypochondria 01100, Mob Enforcer 01101, Silver Twilight Acolyte 01102 and
Stubborn Detective 01103, are implemented, all of them #781's.

Roland's two classes are already complete, which is exactly why breadth costs what
it costs: every card the gate needed was a Guardian, a Seeker or a neutral skill.

**Level 0 only.** The 22 XP cards in the core pool have no consumer until phase 9
builds the campaign upgrade flow — no deck can legally contain one, so they would
ship untested by anything but a unit test.

## Issues

**Open:** none.

### Closed

| Issue | Work |
|---|---|
| #776 | Elder signs and scenario symbols share one token outcome — the prefactor: #1040 (one `TokenOutcome`), #1041 (elder-sign effects), #1042 (the horror-count quantity, now `HorrorOnSource`), #1043 (`revealed` / `total`) |
| #777 | Slice 1 — Agnes Baker 01004 + Heirloom / Dark Memory + Mystic level 0: #1045–#1056, closed by #1058 (Agnes plays The Gathering to a resolution with the picker's scaffold deck) |
| #778 | Slice 2 — "Skids" O'Toole 01003 + On the Lam / Hospital Debts + Rogue level 0: #1064–#1078, closed by #1079 ("Skids" plays The Gathering to a resolution with the picker's scaffold deck); closes #576 (`is_fast` after prefix lines) through #1065 |
| #779 | Slice 3 — Daisy Walker 01002 + Tote Bag / Necronomicon + the Tome-restricted extra action, plus the trait-counting `Quantity` (decided in #1000): #1089–#1095, #1097, #1098, landed as PR #1096 |
| #780 | Slice 4 — Wendy Adams 01005 + Wendy's Amulet + Survivor level 0: #1102–#1113, landed as PR #1114 |
| #1116 | The random basic weakness draw (roster count, weakness pool, seeded draw at scenario start) + the playable gate on roster codes + `WeaknessKind` in the corpus: #1117, #1118, #1119, landed as PR #1120 |
| #781 | Abandoned and Alone 01015 + the basic weaknesses (Amnesia 01096, Paranoia 01097, Haunted 01098, Psychosis 01099, Hypochondria 01100, Mob Enforcer 01101, Silver Twilight Acolyte 01102, Stubborn Detective 01103), with what they needed of the engine: drawn encounter-cardtype weaknesses resolve as encounter cards (closes #514), engagement exits and spawning engaged with the drawer (closes #650), `DamageDealt` around `DamagePlaced`, direct harm and `HarmTaken`, bearer-only prey (closes #654), the one-shot weaknesses, the text-box blank, and the pool test with no exclusions: #1121–#1128, landed as PR #1129 |
| #782 | Five default decks in `cards::decks`, each with a fixed basic weakness, and the picker's "Random basic weakness" checkbox; the Learn to Play's starter-deck sidebar vendored in `data/learn-to-play/`; the scaffold decks retired, landed as PR #1134 |
| #1144 | Lint posture: `#[expect(…, reason)]` by default, `allow_attributes` and `unreachable_pub` adopted, landed as PR #1168 |

#783 (the deckbuilding validator) was filed alongside #782 but milestoned to
**phase 9**, where its real consumer is.

## Ordering

**#1025 ✅ PR #1030 → #776 ✅ PR #1044 → #777 ✅ PR #1057 → #778 ✅ PR #1080 → #779 ✅ PR #1096 → #780 ✅ PR #1114 → #1116 ✅ PR #1120 → #781 ✅ PR #1129 → #782 ✅ PR #1134.**

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
gets exercised before Daisy's trait-counting leans on it. Wendy is last because her
reaction and her Amulet need the most engine: a chaos-token reveal that is a timing
moment, and a play from the discard pile.

## What the elder signs force

This is where the stress test bites. Verbatim from
`data/arkhamdb-snapshot/pack/core/core.json`:

| Card | `[elder_sign]` effect | What it needs |
|---|---|---|
| Roland 01001 | `+1 for each clue on your location.` | ✅ shipped (#118) |
| Agnes 01004 | `+1 for each horror on Agnes Baker.` | a horror `Quantity` |
| "Skids" 01003 | `+2. If you succeed, gain 2 resources.` | a success-conditional **effect** |
| Daisy 01002 | `+0. If you succeed, draw 1 card for each [[Tome]] you control.` | that, plus trait-counting |
| Wendy 01005 | `+0. If Wendy's Amulet is in play, you automatically succeed instead.` | an `immediate` automatic success, plus a `ControlStatus` condition on the Amulet (#780) |

`Trigger::ElderSign` carries a `TokenOutcome` — the shape a scenario's symbol table
returns too: a modifier, an `immediate` part, and `on_success` / `on_fail` parts.
The modifier is copied unevaluated into a test-scoped row at ST.3 and read at ST.5.
`immediate` runs at ST.4, where the ability initiates. `on_success` / `on_fail` are
skill test results and run at ST.7, after the action's own result (the clue an
investigation discovers). All three run with the investigator card as source.

**Wendy's automatic success is an elder-sign effect, not an outcome override**:
`immediate: If(ControlStatus(<Amulet>, ByAPlayer), AutoResolve(AutomaticSuccess))`,
a test-scoped
determination that ST.6 reads (ADR 0007). The token is already revealed, so
`data/rules-reference/rules/glossary/Automatic_Failure_Success.md` skips nothing:
*"If a chaos token effect causes an investigator to automatically succeed or fail at
a skill test, continue with Steps 3 and 4, as normal."* The determination
substitutes the difficulty, not the skill value — Wendy's ruling, *"If a skill test
automatically succeeds, the total difficulty of that test is considered 0"*
(https://arkhamdb.com/card/01005) — so ST.5 still determines the modified skill
value, as `data/official-faq/Rulings_and_Clarifications.md`'s Hope/Patrice example
requires of every automatically-successful test. The same document distinguishes
that from Stray Cat 01076's *"Automatically evade"*, where *"no skill test is made
whatsoever."* Auto-success is not "skip the test", and the two must not be
conflated — 01076 is in Wendy's own slice.

## Decks are constructed, not transcribed

Each investigator the picker offers seats a **default deck** (`GLOSSARY.md`), held
in `cards::decks` so the picker, the server's tests and phase 9 read one list. All
five are **inspired by** the 2016 Learn to Play's *"Investigator Starter Decks"*
sidebar (p. 4), vendored with a verbatim transcription in
`data/learn-to-play/starter_decks.md`. The sidebar gives only Roland's and Wendy's
decks; Daisy, Skids and Agnes get the same structure, applied by us:

- one copy of each of the 10 primary-class level-0 titles,
- one copy of each of the 10 secondary-class level-0 titles,
- two each of Knife 01086, Flashlight 01087 and Emergency Cache 01088,
- two each of two neutral skills, covering the investigator's weakest skills,

which is 10 + 10 + 6 + 4 = 30, with the signatures outside the count. Every card is
level 0, as the sidebar's *"do not include these upgraded versions"* requires.

| Investigator | Neutral skills (×2) | Fixed weakness |
|---|---|---|
| Roland 01001 | Guts 01089, Manual Dexterity 01092 | Silver Twilight Acolyte 01102 |
| Daisy 01002 | Overpower 01091, Manual Dexterity 01092 | Paranoia 01097 |
| Skids 01003 | Guts 01089, Perception 01090 | Mob Enforcer 01101 |
| Agnes 01004 | Perception 01090, Overpower 01091 | Haunted 01098 |
| Wendy 01005 | Overpower 01091, Unexpected Courage 01093 | Amnesia 01096 |

Roland's is the book's Roland deck with Silver Twilight Acolyte in place of
Paranoia; Wendy's is the book's Wendy deck unchanged. Each deck's doc comment gives
its source, how it differs, why it takes its skills, and its card back's
requirements verbatim.

**Every deck fixes its basic weakness in the list**, as the Learn to Play decks do,
where the card back says *"1 random basic weakness"*. That is a deliberate
deviation, and the picker's "Random basic weakness" checkbox (unchecked by default)
restores the printed behaviour: `DefaultDeck::roster_entry` leaves the fixed
weakness out and asks seating to draw one from the weakness pool.

The decks are legal by construction, and `cards::decks`' tests check each against
its card back in both modes. The **validator** (#783) is separate on purpose: its
real consumer is phase 9's decklist import, where decks arrive from outside the repo
and cannot be trusted. Import replaces the default decks only as the *default*; they
stay as the picker's fallback.

## Open questions

None.

## Dependencies

Phase 7's gate, closed. Nothing here should start while a rules defect in the gate is
open — that is the whole reason for the ordering.

#366 (replacement effects beyond cancel) blocks nothing here: Wendy's reaction is a
cancel followed by her own effect, and the Amulet's Forced is a routing rule. Its
general machinery is unmilestoned.

## What "done" looks like

Five radios in the picker; picking any of the five seats that investigator with a
legal default deck, with its fixed weakness or, with the "Random basic weakness"
box checked, a random one, and plays The Gathering to a resolution.
Difficulty stays Standard and the player count stays 1 — those are #862 (phase 9)
and #863 (phase 8) respectively, both spun out of phase-7's Future slices when that
phase closed.
