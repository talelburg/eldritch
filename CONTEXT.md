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
What a player does with a skill card to add it to an in-progress skill test. Skill cards are committed, never played.
_Avoid_: Play, use (for skill cards specifically)

**Play**:
What a player does with an Asset or Event from hand. Those are the only two card types that can be played; everything else is either the investigator card or scenario-bag content.
_Avoid_: Using "play" for committing a skill, or for revealing encounter cards

**In-progress play**:
A card partway through being played. Per the Rules Reference (Appendix I, Initiation Sequence) it "commences being played" at step 3, leaving hand, and is "regarded as played (and placed in play, or in its owner's discard pile if it's an event)" only at the completion of step 4. Between those points it is in **no zone at all** — not in hand, not in play, not in a discard pile. Plays nest, so several cards can be in this state at once: a Fast event played to cancel the attack of opportunity a non-fast play provoked is a second play running inside the first. Each rides the continuation frame driving it; see `docs/adr/0002-in-progress-play-lives-on-its-frame.md`.
_Avoid_: Saying the card is "still in hand" or "already in the discard" while its effect resolves — it is in neither, and treating it as either is what erased a card from the game in #604. "Pending play" names the same thing less precisely; prefer in-progress play.

**Discovery**:
A single instance of discovering clues, carrying a count — not a count of separate discoveries. Its count is capped at the clues actually present at the location, so a discovery is what you *do* take, never what you requested. "Discover 1 additional clue" (Deduction 01039) raises an existing discovery's count; it does not create a second discovery. The distinction is invisible in final clue totals and decisive for anything keying off the would-be discovery — Cover Up 01007 replaces one discovery of 2, not two of 1.
_Avoid_: Treating "discover N clues" as N discoveries, or as a request the engine might not fill. Deduction shipped as a second discovery, with a test asserting two `CluePlaced` events; Cover Up over-discarded as a result, and #471 fixed both.

**Queued ability**:
An ability whose continuation frame is on the stack but whose effect has not run. Emitting a timing point *queues* its forced and reaction abilities; it does not resolve them. A caller's own work after the emit therefore runs **before** those abilities unless it rides a resumption frame; see `docs/adr/0003-emitting-a-timing-point-queues-abilities.md`.
_Avoid_: saying an emit "fires" or "resolves" abilities, and reading `EngineOutcome::Done` from one as "nothing happened" — that reading orphaned agenda 01107's forced Ghoul movement for an entire scenario.

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
