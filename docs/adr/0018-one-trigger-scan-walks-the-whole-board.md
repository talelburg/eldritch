# One trigger scan walks the whole board; a condition narrows, it never picks zones

"Which triggered abilities does this timing event reach?" was answered once per ability kind, and the answers had drifted apart. The forced side was a private `ForcedTriggerPoint` enum with one hand-written zone table per variant: `EnteredLocation` looked at the entered location's card, `RoundEnded` at the act, the agenda and every Active investigator's controlled cards, `EnemyDefeated` at the current act only. So *"Forced - After an enemy is defeated"* printed on an agenda, an asset or a location was never collected, never rejected and never logged (#698). Each table was a guess about where a card listening to that condition could sit, and every guess was right only until the next card arrived.

**So the scan walks the whole board at every condition, and a condition never chooses where to look.** The walk is `engine::board::walk`, which visits, in one fixed order, each card with its `Placement`:

1. each investigator — the active one first, then the rest of `turn_order`, then anyone else by id — their investigator card, their play area, then their threat area;
2. each location by `LocationId`, its attachments, and the cards put into play at it;
3. each enemy by id, then its attachments;
4. the current act, then the current agenda.

The walk itself is unfiltered, and the scan (`trigger_scan::board_walk`) narrows it three ways. It appends the Fast events in each investigator's hand, in the walk's investigator order, after the act and the agenda: a hand is not on the board, so it comes after the whole of it. It skips eliminated investigators, since Rules Reference p.10 removes their cards and only this filter keeps their investigator card out (#567). The one exception is the investigator `EliminationGameEnd` names, who is already off `Active` when step 0 fires for their weaknesses. And at an advance, the act or agenda slot holds the card the event names: during its reverse it is still the current one, and the event's code is the authority for which card that is.

## What narrows a condition is on the card

The zone tables carried narrowings that no pattern stated. Walk the whole board without them and the Attic's horror fires when you enter the Cellar. The narrowings now live in the matcher, `trigger_scan::pattern_matches`, in one of two forms:

- **A matcher arm, when the narrowing is the pattern's own definition.** `EnteredLocation` is heard only on the entered location's card. The Attic 01113's ruling (<https://arkhamdb.com/card/01113>) is the shape: *"The **Forced** ability triggers each time an investigator enters this location."* Likewise `LeftLocation` on an attachment of the left location (Barricade 01038's *"attached location"*), `EndOfTurn` only for the ending investigator's controller (Frozen in Fear 01164's *"your turn"*), an advance only on the advancing act or agenda, and elimination's game end only on a weakness the eliminated investigator controls (Rules Reference p.10 step 0: *"Trigger any “when the game ends” abilities on each weakness the eliminated investigator owns that is in play."*).
- **A field on the pattern, when one pattern has both scoped and unscoped consumers.** `EnemyAttacks { attacker, target }`: Silver Twilight Acolyte 01102 (*"After Silver Twilight Acolyte attacks"*) is `{ This, Any }` and Dodge 01023 (*"when an enemy attacks an investigator at your location"*) is `{ Any, AtYourLocation }`. These are two fields rather than variants because the corpus varies the attacker and the target independently. `SkillTestResolved { tested_location }` is `Attached` for Obscuring Fog 01168 (*"After attached location is successfully investigated"*) and `Any` for Dr. Milan 01033 and Lita Chantler 01117. Only the values a corpus card prints exist; *"attacks you"* or a trait filter arrives with the card that needs it.

The matcher is split by what it reads. `trigger_matches` reads only the event, the pattern and the controller. `scope_matches` reads the card's source and the board around it. **Both are exhaustive on the event, and the pattern ↔ condition pairing before them is exhaustive in both directions.** `EventPattern::condition` (in `card-dsl`) and `TimingEvent::condition` (in `game-core`) meet at `TriggeringCondition`, so a new pattern or a new event cannot compile until it names its condition, its narrowing and its scope. This is ADR 0008's classification discipline, extended from who resolves a condition to which patterns it matches, and `card-dsl` still sits below `game-core`. The `_ => false` arm the reaction pairing used to end with is gone, and with it the two ways a pairing went silently missing.

## The forced-binding rule

A forced hit is one candidate, and it is not filtered by reachability. ADR 0010: *"a forced ability is not restricted to the sources its controller could legally use"*. Its controller is:

- a **controlled** card → its controller;
- the current **act** or **agenda** → the lead proxy, the first Active investigator in `turn_order` (GLOSSARY "Lead investigator");
- any other **uncontrolled** card — a location, an enemy, an attachment on either → the condition's **subject**, falling back to the lead proxy when there is none.

The subject is `TimingEvent::subject`, another exhaustive match: the investigator who entered, left, was attacked, tested, discovered, was dealt harm, ended their turn or was eliminated, and `by` for an enemy defeat. Phase boundaries, advances and the round's and game's end have none. The rule reproduces every binding the per-point arms made. It also reaches cards they never did: a location's *"after an enemy is defeated"* binds the investigator who defeated it.

## Reactions are filtered by reachability; forced abilities are not

The two kinds share the walk and the matcher, and differ only in how a hit becomes candidates.

**A reaction is a player choosing to use an ability, so it is offered only to an investigator who may use its source.** ADR 0010's `reachable_sources` is the one predicate that answers that, and activation already asks it. `trigger_scan::collect_reactions` asks it too, and turns a hit into one candidate per investigator whose reachable sources include the hit's source, in the walk's investigator order:

- a controlled card → its controller. An encounter card in a threat area also goes to each other investigator at that location, the clause Haunted 01098's ruling settles (ADR 0010);
- a location, a card attached to it or put into play at it, an enemy and its attachments → each investigator at that location;
- the current act or agenda → **one** candidate, bound to the lead proxy. Every investigator reaches it, and the corpus reaction printed there is a group's single offer: The Barrier 01109's *"When the round ends, investigators in the hallway may, as a group, spend the requisite number of clues to advance."*;
- a Fast event in hand → the investigator holding it, gated as a play.

Before this, the reaction scans reached only the investigators' controlled cards and the act and agenda, so a reaction on a location or an encounter card at your location was never offered. That contradicted ADR 0010's list of sources.

**A forced ability is one candidate whatever reaches it.** ADR 0010: *"a forced ability is not restricted to the sources its controller could legally use"*. Silver Twilight Acolyte 01102 places its doom whether or not the investigator it attacked could use the enemy. Filtering forced hits by reachability would make whether a card's forced text happens depend on where the investigators stand.

## The sweeps, reachability and the instance lookup share the walk

The modifier sweep, the grant sweep, the instance lookup and reachability ask the scan's question too: *"which cards are on the board"*. A zone list of their own is a guess that drifts, the way the forced tables did — one that never skips an eliminated investigator, or that reaches a player card in a co-located threat area where the co-location bullet reaches only *"encounter cards in the threat area of any investigator at that location"* (#975). **All of them read `board::walk`**, and none keeps a zone list of its own, so a new kind of source is added once and every reader sees it.

Each caller filters the walk to what it needs:

- **The modifier sweep and the grant sweep** skip an eliminated investigator's cards, as the trigger scan does. In elimination's step-0 window those cards are still on the board, and they no longer project modifiers or grant abilities.
- **The instance lookup** (`find_instance` and its mutable twin) is unfiltered. Cover Up 01007's game-end trauma resolves at step 0, after its holder has left `Active`, and must still find its card.
- **Reachability** filters by ADR 0010's bullets, and reads another investigator's threat area through the registry's cardtype. A card with no metadata counts as an encounter card, matching the other metadata fallbacks.

**Every reader takes the walk's order.** No modifier-sweep consumer reads its order, since a breakdown's total is a commutative fold. Reachability's order is observable as the order of turn-menu and player-window options: another investigator's threat-area card is listed in that investigator's block, and a reacher who is not first in the walk sees an earlier investigator's cards before their own. The client routes options per board card, so the order across cards is presentation only, and the order of one card's abilities is unchanged.

## Considered options

**One shared per-condition zone table**, used by both kinds: `ForcedTriggerPoint`'s tables, lifted out so reactions read them too. It fixes the forced/reaction drift and keeps today's scan cost. It was rejected because it keeps the failure that produced #698. The table is still a prediction of where a listening card can sit, made before the card exists, so it stays wrong until someone hand-edits the arm. A missing zone is also silent, because a card the table never visits is neither collected nor rejected. With the whole-board walk, a new kind of source is added once, in the walk, and both kinds see it. A wrong narrowing becomes a failing assertion about a card's printed word. The cost is a walk over every card at every cell, and the board is small. #117's event-keyed index is the remedy if it stops being small, and the walk is the shape such an index would index.

## Consequences

**Forced order follows the walk.** A 2+ forced run is ordered by the lead, so the order its options are listed in is presentation only. That order moved: at round end, Dissonant Voices 01165 in a threat area is now listed before agenda 01107, where the old `RoundEnded` arm listed the act and agenda first.

---

*Folded #983 (the sweeps, reachability and the instance lookup moved onto the walk, which moved from `trigger_scan` to `engine::board`, and the Fast events in hand to after the whole board), and #975 (the cardtype filter on co-located threat areas).*
