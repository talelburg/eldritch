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
A keyword ability. Per the Rules Reference: *"A fast card does not cost an action to be played and is not played using the 'Play' action."* A property of how a card is played.
_Avoid_: Using "fast" for a zero-action **activated** ability. That is a different concept that happens to share the word — say "zero-action ability" instead.

**Horror soak**:
Our informal term for assigning horror to an asset that has sanity, rather than to the investigator. The word does not appear in the Rules Reference, which says only that an asset "must have sanity in order to be assigned horror."
_Avoid_: Max sanity, sanity boost, sanity modifier. An asset's sanity is a capacity to absorb horror, never a change to the investigator's own sanity.

**Commit**:
What a player does with a skill card to add it to an in-progress skill test. Skill cards are committed, never played.
_Avoid_: Play, use (for skill cards specifically)

**Play**:
What a player does with an Asset or Event from hand. Those are the only two card types that can be played; everything else is either the investigator card or scenario-bag content.
_Avoid_: Using "play" for committing a skill, or for revealing encounter cards

## Project vocabulary

**Project phase**:
One of the 11 milestones the build is broken into, tracked in `docs/phases/` and on GitHub.
_Avoid_: Bare "phase" anywhere a game phase could be meant — which is most of the engine. Say "project phase" or "phase 7" and reserve the bare word for the game concept.
