# Eldritch

A simulator for *Arkham Horror: The Card Game*. This glossary pins the terms where the rulebook, ArkhamDB, and our own project vocabulary disagree. Every entry below has already caused a real mistake in review — that is the bar for adding one.

Use these words in code, test names, issue titles, commit messages, and chat. If a concept you need isn't here, either you're inventing language the project doesn't use, or there's a genuine gap worth filling.

## Game vocabulary

**Class**:
A card's allegiance — Guardian, Seeker, Rogue, Mystic, Survivor, Neutral, or Mythos.
_Avoid_: Faction (ArkhamDB's word for the same field; the rulebook says class, and so do we)

**Phase**:
One of the four segments of a game round: Mythos, Investigation, Enemy, Upkeep.
_Avoid_: Round, step, stage. See **Project phase** below — the word is overloaded across the repo.

**Fast**:
A keyword ability. Per the Rules Reference: *"A fast card does not cost an action to be played and is not played using the 'Play' action."* A property of how a card is played, and the entry says nothing about abilities.

A second mechanic shares the word. ArkhamDB's card text writes the **free triggered ability** icon as the markup token `[fast]`, and the Rules Reference defines that icon separately — it *"indicates a free triggered ability that does not cost an action and may be used during any player window."* Ours is `Trigger::Activated { action_cost: 0 }`. Magnifying Glass (01040) prints both at once — the keyword line `Fast.`, and then `[fast] If there are no clues on your location: Return Magnifying Glass to your hand.`

_Avoid_: Reading a `[fast]` token in ArkhamDB card text as the keyword — it is the ability icon, and only a bare `Fast.` line is the keyword. Say "zero-action ability" (or the Rules Reference's "free triggered ability") for the second sense, never "fast ability".

**Horror soak**:
Our informal term for assigning horror to an asset that has sanity, rather than to the investigator. The word does not appear in the Rules Reference, which says only that an asset "must have sanity in order to be assigned horror."
_Avoid_: Max sanity, sanity boost, sanity modifier. An asset's sanity is a capacity to absorb horror, never a change to the investigator's own sanity.

**Commit**:
What a player does with a skill card to add it to an in-progress skill test. Skill cards are committed, never played. Committing at ST.2 takes the card **out of hand** into **limbo** (below), where it stays until ST.8 discards it.
_Avoid_: Play, use (for skill cards specifically). Also avoid describing a committed card as still in hand — that was #631.

**Play**:
What a player does with an Asset or Event from hand. Those are the only two card types that can be played; everything else is either the investigator card or scenario-bag content.
_Avoid_: Using "play" for committing a skill, or for revealing encounter cards

**Limbo**:
The Rules Reference's name (glossary, added in FAQ 1.23) for a card that is in **no zone at all** — "neither in play, in the discard pile, nor … in an investigator's hand". Two states in the engine are limbo, and both ride the continuation frame driving them rather than any zone: an **in-progress play** (below) and a **committed card**, which "enters limbo as it is committed to a skill test" at ST.2 and leaves it when ST.8 discards it. Limbo is the umbrella; name the specific state when you mean one of them. A limbo card "is technically not in play, and does not count as being in play for the purposes of other card effects" — so it is invisible to in-play queries while its effects still alter the game state.
_Avoid_: Calling it a zone, or a "pending" pile — it is "not a physical game area". Also avoid reaching for it where the specific term is clearer: a reader who sees "limbo" has to work out which of the two you meant.

**In-progress play**:
A card partway through being played — the play-side flavour of **limbo** (above). Per the Rules Reference (Appendix I, Initiation Sequence) it "commences being played" at step 3, leaving hand, and is "regarded as played (and placed in play, or in its owner's discard pile if it's an event)" only at the completion of step 4. Between those points it is in **no zone at all** — not in hand, not in play, not in a discard pile. Plays nest, so several cards can be in this state at once: a Fast event played to cancel the attack of opportunity a non-fast play provoked is a second play running inside the first. Each rides the continuation frame driving it; see `docs/adr/0002-in-progress-play-lives-on-its-frame.md`.
_Avoid_: Saying the card is "still in hand" or "already in the discard" while its effect resolves — it is in neither, and treating it as either is what erased a card from the game in #604. "Pending play" names the same thing less precisely; prefer in-progress play.

**Discovery**:
A single instance of discovering clues, carrying a count — not a count of separate discoveries. Its count is capped at the clues actually present at the location, so a discovery is what you *do* take, never what you requested. "Discover 1 additional clue" (Deduction 01039) raises an existing discovery's count; it does not create a second discovery. The distinction is invisible in final clue totals and decisive for anything keying off the would-be discovery — Cover Up 01007 replaces one discovery of 2, not two of 1.
_Avoid_: Treating "discover N clues" as N discoveries, or as a request the engine might not fill. Deduction shipped as a second discovery, with a test asserting two `CluePlaced` events; Cover Up over-discarded as a result, and #471 fixed both.

**Timing cell**:
One of the three slots in the sequence the Rules Reference runs around every triggering condition — `glossary/Nested_Sequences.md`: *"Each time a triggering condition occurs, the following sequence is followed: 1) execute "when..." effects that interrupt that triggering condition, (2) resolve the triggering condition, and then, (3) execute "after..." effects in response to that triggering condition."* `glossary/At.md` (identical in `glossary/If.md`) names the middle one: abilities using *"at"* or *"if"* *"trigger in between any "when..." abilities and any "after..." abilities with the same triggering condition."* So the card's own printed trigger word names its cell: *when* interrupts the condition, *at* and *if* land between, *after* waits until it has fully resolved. Within a cell, forced abilities resolve before reactions.

The word **"would"** overrides the leading word. `glossary/Instead.md` gives it a higher priority than the same triggering condition without it: *"(For instance, "When X would occur" resolves before "When X occurs.")"* Reading that against `glossary/At.md` is what puts *"if … would …"* in the when cell, while *"if"* on a settled state — act 01110's *"**Objective** - If the Ghoul Priest is Defeated, advance."* — stays in the at cell. Neither entry says so outright; the conclusion is ours.
_Avoid_: Treating the trigger word as flavour, and confusing a cell with the triggering condition it hangs off — one condition has all three cells, every time it occurs. A timing declaration written from a guess rather than from the card is the recurring mistake; check the tag against the trigger word the module already quotes.

**Queued ability**:
An ability whose continuation frame is on the stack but whose effect has not run. Emitting a timing point *queues* its forced and reaction abilities; it does not resolve them. A caller's own work after the emit therefore runs **before** those abilities unless it rides a resumption frame; see `docs/adr/0003-emitting-a-timing-point-queues-abilities.md`.
_Avoid_: saying an emit "fires" or "resolves" abilities, and reading `EngineOutcome::Done` from one as "nothing happened" — that reading orphaned agenda 01107's forced Ghoul movement for an entire scenario.

**Base value**:
A quantity before any modifier applies. Per the Rules Reference: *"Base value is the value of an element before any modifiers are applied. Unless otherwise specified, the base value of an element derived from a card is the value printed on that card."* Usually the printed number, but a card can **replace** it — Duke 02014's *"You attack with a base [combat] skill of 4"* — and modifiers still stack on top of the replacement.
_Avoid_: Using it for the number a test is actually resolved against (that is the **modified value**), and confusing a base replacement with an automatic success or failure. They look alike and sit at opposite ends of the calculation: a base replacement is the bottom, a substitution is the top. Collapsing them was a real error during #628's design.

**Modified value**:
A quantity after the whole calculation: base, then modifiers, then clamping and any substitution. Never stored — it is recalculated at every read, because the Rules Reference defines it that way: *"Any time a new modifier is applied (or removed), the entire quantity is recalculated from the start."* An investigator's skill value, a test's difficulty, an enemy's fight and evade, and a location's shroud are all modified values. See `docs/adr/0005-a-modified-quantity-is-recalculated-at-every-read.md`.
_Avoid_: "Total", "effective value", or "final value" for the general concept — the Rules Reference reserves *total* for specific composites ("total skill value", "total difficulty"), and "effective" reads as a one-off computation rather than a live one. Also avoid speaking of a modified value being *set*: nothing sets it, things modify its inputs.

**Automatic failure / success**:
A **determination** that a skill test fails or succeeds regardless of the numbers, substituting the whole quantity at ST.6 — *"If a skill test automatically fails, the investigator's total skill value for that test is considered 0. If a skill test automatically succeeds, the total difficulty of that test is considered 0."* The determination belongs to the **test**, not to either quantity: it is resolved once across both, because automatic failure takes precedence over automatic success and neither quantity can see the other's substitution. Distinct from the `[auto_fail]` **token symbol being revealed**, which is what almost every card that mentions `[auto_fail]` actually keys off (Shrivelling 01060, Baseball Bat 01074, Jewel of Aureolus 02269 …) — those fire on the reveal and are indifferent to how the test resolves.

A determination known **before** the chaos token is revealed skips ST.3 and ST.4 — no token is drawn at all. One caused **by** a token does not, since the reveal has already happened. Both are the same determination; only the timing differs.
_Avoid_: Reading "if a `[skull]` or `[auto_fail]` symbol is revealed" as a clause about failing; it is a clause about the token. No Core + Dunwich card keys off the *determination* being an automatic failure rather than an ordinary one, so `FailureReason::AutoFail` is display attribution, not a rules distinction.

## Project vocabulary

**Project phase**:
One of the 11 milestones the build is broken into, tracked in `docs/phases/` and on GitHub.
_Avoid_: Bare "phase" anywhere a game phase could be meant — which is most of the engine. Say "project phase" or "phase 7" and reserve the bare word for the game concept.

**Snapshot**:
Everything vendored under `data/arkhamdb-snapshot/` — all of Chapter 1's **card data**, pinned at one upstream commit. Most of it is **planning input**: it exists so decisions about the DSL and the engine can be made against the full set of cards we will eventually support, not just the ones we build against today. It is much larger than the corpus.
_Avoid_: Calling it the corpus. Since #618 those are different sets, and conflating them makes "how many cards do we have?" unanswerable. Also avoid the bare word for the other two things we vendor from ArkhamDB — see **Rules text** and **Card FAQ** below. The snapshot is cards.

**Rules text**:
The Rules Reference ingested verbatim into `data/rules-reference/rules/`, one file per section and per glossary entry, filenames equal to ArkhamDB's anchor ids. It is the canonical source for how the game runs, and it covers all of Chapter 1 — the printed Rules Reference plus the rules deluxe expansions and the official FAQ added on top.
_Avoid_: Reading the vendored PDF instead. That is the 2016 Core Set edition, retained only as the pinned publisher original; it predates `Bonded`, `Concealed X`, Bless/Curse and every FAQ amendment. Also avoid calling this "the snapshot" — it has no upstream commit to pin, so its provenance is a URL, a date and a hash.

**Card FAQ**:
The official per-card rulings ingested into `data/arkhamdb-faq/<pack>/<code>.md`, covering the whole snapshot. A card with no file has no rulings; `no-rulings.txt` says so explicitly, which is what makes absence an answer rather than a gap.
_Avoid_: Treating a card's printed text as the whole story. Rulings routinely carry mechanics the text does not, which is why they are vendored alongside it.

**Corpus**:
The subset of the snapshot that `PACK_FILES` ingests and the build actually compiles — Core + Dunwich, emitted as `crates/cards/src/generated/cards.rs`. A pack becomes part of the corpus by being moved into `PACK_FILES`; that promotion is deliberate, never automatic.
_Avoid_: Assuming a card in the snapshot has metadata at runtime. `cards::metadata_for` only answers for the corpus.
