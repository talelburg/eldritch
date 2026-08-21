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

**Assign / Place**:
The two steps every deal of damage or horror walks, and never one word for both. `glossary/Dealing_Damage_Horror.md` **assigns** first — *"Determine the amount of damage and/or horror being dealt. Place damage and/or horror tokens equal to the amount of damage and horror being dealt **next to** the cards that will be taking the damage/horror"* — and **places** second, *"on each card to which it has been assigned, simultaneously"*. Between them sits a window the rules name outright: *"Abilities that prevent, reduce, or reassign damage and/or horror that is being dealt are resolved between steps 1 and 2."* An assignment is therefore a proposal that cards can still edit, and a placement is the single simultaneous moment that settles it — which is why each is its own triggering condition, with its own three cells. Guard Dog 01021 triggers on the assignment (*"You can use Guard Dog's ability when you assign lethal damage/horror to it"*); Mark Harrigan 03001's *"After damage is placed on a card you control"* triggers on the placement.
_Avoid_: "Deal" for either step alone — dealing is the whole procedure. Also avoid "apply", the Rules Reference's own heading for step 2, which reads in our code like `apply(state, action)`; say **place**, the word the card text uses. Treating a card's assigned share as damage it has taken is the error the split exists to prevent: nothing is on any card until the placement.

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

**Ability source**:
The thing whose ability is being used — what an activation *names*, and what a forced or reaction ability *fires from*, addressed independently of the collection it happens to sit in. `glossary/Triggered_Abilities.md` lists four kinds of them and calls them sources: *"An investigator is permitted to use triggered abilities (\[free\], \[reaction\], and \[action\] abilities) from the following sources: A card in play and under his or her control. This includes his or her investigator card. / A scenario card that is in play and at the same location as the investigator … / The current act or current agenda card. / Any card that explicitly allows the investigator to activate its ability."* Ours is `AbilitySource`, and **reachability** — whether a given investigator may use an ability from a given source — is a separate question the engine answers in one predicate, for `[free]`, `[reaction]` and `[action]` alike. See `docs/adr/0010-an-activation-names-an-ability-source.md`.
_Avoid_: Reading a card **played from hand** as an ability source — a Fast event is played, not fired from a source, and none of the four bullets names a card in hand; that is the one origin the engine's candidate descriptor keeps outside this vocabulary. Also avoid saying "the source card", or reading a source as a zone. A source is not "a card in `cards_in_play`": your investigator card and your threat area are cards in play under your control too, and the activation and reaction paths disagreeing about exactly that is what #707 was. The **source of an ability** is also not the **source of damage** (`DamageSource`) or the **source instance** an `EvalContext` binds for `DiscardSelf` — related, deliberately not the same word doing three jobs.

**Action designator**:
The **bold word** an ability prints above its effect — *"[action]: **Fight.**"*, *"[action] Spend 1 supply: **Investigate.**"*, *"[action] **Resign.**"*. `glossary/Ability.md` names them: *"Some abilities have bold action designators (such as **Fight**, **Evade**, **Investigate**, or **Move**). Activating such an ability performs the designated action as described in the rules, but modified in the manner described by the ability."* **Parley** and **Resign** have their own glossary entries and are designators too. Ours is `ActionDesignator`, declared on `Trigger::Activated` — never inferred from the effect, because the rules quote the designator: the attack-of-opportunity exemption is *"an action other than to **fight**, to **evade**, or to activate a **parley** or **resign** ability"*, and Frozen in Fear 01164's ruling is *"Also applies to \[action\] card abilities with action designators (**Move**, **Fight**, **Evade**)."*
_Avoid_: Reading a designator off the ability's effect. **Parley** and **Resign** have no effect shape of their own, and an effect shape is not a designator: a `Seq` that fights inside it is still a **Fight** ability if it prints the word, and an ability with no bold word is not one however it resolves. Also avoid treating a designator as an exemption in itself — **Investigate** and **Move** are designators that provoke attacks of opportunity exactly as their basic actions do.

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
_Avoid_: Calling it the corpus. Since #618 those are different sets, and conflating them makes "how many cards do we have?" unanswerable. Also avoid the bare word for the other things we vendor — see **Rules text**, **Card FAQ** and **Official FAQ** below. The snapshot is cards.

**Rules text**:
The Rules Reference ingested verbatim into `data/rules-reference/rules/`, one file per section and per glossary entry, filenames equal to ArkhamDB's anchor ids. It is the canonical source for how the game runs, and it covers all of Chapter 1 — the printed Rules Reference plus the rules deluxe expansions and the official FAQ added on top.
_Avoid_: Reading the vendored PDF instead. That is the 2016 Core Set edition, retained only as the pinned publisher original; it predates `Bonded`, `Concealed X`, Bless/Curse and every FAQ amendment. Also avoid calling this "the snapshot" — it has no upstream commit to pin, so its provenance is a URL, a date and a hash.

**Card FAQ**:
ArkhamDB's community-collated per-card rulings, ingested into `data/arkhamdb-faq/<pack>/<code>.md`, covering the whole snapshot. A card with no file has no rulings; `no-rulings.txt` says so explicitly, which is what makes absence an answer rather than a gap.
_Avoid_: Treating a card's printed text as the whole story. Rulings routinely carry mechanics the text does not, which is why they are vendored alongside it. Also avoid confusing it with the **Official FAQ** below — that is the publisher's own document, and it outranks this one.

**Official FAQ**:
Fantasy Flight's *Notes, Errata, and Frequently Asked Questions*, converted from the pinned PDF into `data/official-faq/`, one file per section. Its unique content is the **Q&A section** — 145 pairs that appear in no other vendored source, and the reason #672 existed. Its numbered rules sections (Game Play 1.x, Card Ability Interpretation 2.x) are already in **Rules text**, folded in there by ArkhamDB; the duplication is deliberate, so that "not here" cannot mean "we didn't ingest that page".
_Avoid_: Reaching for it first on a procedural rules question. **Rules text** is indexed per entry and answers most of them; the Official FAQ is a flat document you go to when it doesn't, and it wins on conflict — *"the text of this document takes precedence"*.

**Corpus**:
The subset of the snapshot that `PACK_FILES` ingests and the build actually compiles — Core + Dunwich, emitted as `crates/cards/src/generated/cards.rs`. A pack becomes part of the corpus by being moved into `PACK_FILES`; that promotion is deliberate, never automatic.
_Avoid_: Assuming a card in the snapshot has metadata at runtime. `cards::metadata_for` only answers for the corpus.
