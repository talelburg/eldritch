# A modifier declares who it reaches

ADR 0005 settled that every modified quantity is a query over live state, keyed on a `ModifierTarget` and fed by a sweep over every place a modifying card can sit. It did not settle the question that sweep immediately raises: having *found* a `Modify` on some card, how does the engine know whether that modifier reaches the entity being asked about?

The engine had no answer, because it had never needed one. A `Modify`'s audience was implicit — the controller of the card carrying it — and that implicitness is precisely what made the old scan controller-keyed. We decided that **a modifier declares its audience in the DSL, as the card's printed text does**, and that the engine never derives the audience from where the source card happens to sit. `Effect::Modify` carries a `ModifierAudience` alongside its `stat`, `delta` and `scope`.

Lita Chantler 01117 is why derivation cannot work. Her text — *"Each investigator at your location gets +1 [combat]."* — sits on an asset **one investigator controls**, and reaches every investigator standing with them. Beat Cop 01018's *"You get +1 [combat]."* sits in exactly the same place and reaches exactly one. The two are indistinguishable by placement, so any rule that reads the audience off the board gets one of them wrong. Every other non-controller case in the corpus *is* derivable — Whateley Ruins 02250 is a location modifying investigators in it, Whippoorwill 02090 an enemy doing the same, Obscuring Fog 01168 an attachment modifying what it is attached to, The Ritual Begins 01144 an agenda modifying every enemy — which is what makes the derivation tempting, and what makes it a trap: it covers thirteen of fourteen corpus cards and is silently wrong about the fourteenth.

The five variants each name a card that needs it:

- `Controller` — *"You get …"*. Beat Cop 01018, Holy Rosary 01035, Magnifying Glass 01030. The overwhelming majority, and the `modify` builder's default, so no existing card declaration changed shape.
- `EachInvestigatorAtSourceLocation` — Lita Chantler 01117, Whippoorwill 02090 (*"Each investigator at Whippoorwill's location gets -1 [willpower], -1 [intellect], -1 [combat], and -1 [agility]."*), Whateley Ruins 02250 (*"Each investigator in Whateley Ruins gets -1 [willpower]."*). One audience serving an asset, an enemy and a location is the evidence that audience and placement are different axes.
- `EachEnemyAtSourceLocation` — Cold Spring Glen 02244, *"Each enemy in Cold Spring Glen gets -1 evade."*
- `EachEnemy` — The Ritual Begins 01144, *"Each enemy gets +1 fight and +1 evade."*
- `AttachedCard` — Obscuring Fog 01168 (*"Attached location gets +2 shroud."*) and Towering Beasts 02256 (*"Attached enemy gets +1 fight and +1 health."*).

## Considered options

**Deriving the audience from the source's placement** — a controlled card reaches its controller, a location or its attachments reach whoever stands there, an enemy reaches investigators at its location, the act and agenda reach everyone — was attractive because it needs no DSL change at all and answers thirteen of the fourteen. It was rejected on Lita, and on a second count that outlives her: the rule would be invisible. "A `Stat::Shroud` modifier found on a location's attachment applies to that location" was already an unwritten convention inside `effective_shroud`, and generalising a convention that no card text can contradict makes the next card that *does* contradict it a silent wrong answer rather than a compile error.

**A `ModifierScope` variant per audience** — `WhileInPlayForEachInvestigatorHere` and friends — was rejected for conflating two independent axes. Scope is *how long*, audience is *who*; the product of the two is a combinatorial enum in which most cells never occur, and ADR 0005 already commits to `ModifierScope` staying the card author's duration vocabulary while the engine records a resolved `Lifetime`.

**Keying the query on a controller rather than a target**, and letting the audience widen the controller set, was the shape the old code implied. It cannot express Cold Spring Glen or The Ritual Begins at all: an enemy's evade has no controller to key on.

## Consequences

The audience is checked against the target, so the sweep visiting a card never means the card contributes — `audience_reaches` is the gate, and a `Controller`-audienced card on another investigator's board reproduces exactly the old controller-keyed scan. That is what makes the widening a no-behaviour-change slice despite the population growing from one collection to six.

Obscuring Fog 01168 is the one corpus card whose declaration changed, from a bare `Modify(Shroud, +2)` read only by a scan that knew where to look, to `AttachedCard`. It reads identically; the rule that used to live in the scanner now lives on the card.

A `Modify` whose audience is not `Controller` **rejects** under a non-constant scope. `PendingSkillModifier` records an investigator and nothing else, so a wider audience has nowhere to be written down; rejecting says so rather than silently narrowing the modifier to its controller. Recorded rows that can name an arbitrary target arrive with #676.

`Stat` gained `Fight` and `Evade` in the same change, not speculatively but because `ModifierTarget::Enemy` is otherwise a target with no quantity — the audience vocabulary and the quantities it addresses are one decision, and half of it would not be testable.
