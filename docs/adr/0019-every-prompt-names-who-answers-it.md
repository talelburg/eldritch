# Every prompt names who answers it

Take a two-seat Gathering in which Daisy Walker 01002 and Agnes Baker 01004 are both in the Hallway when The Barrier 01109 advances. Its reverse ends *"Spawn the set-aside Ghoul Priest in the Hallway."*, and Ghoul Priest 01116 prints *"**Prey** - Highest [combat]."* Both investigators print combat 2. `glossary/Prey.md` settles the tie: *"if multiple investigators are tied in meeting these instructions, the lead investigator may decide among them"*. Neither card has rulings (`data/arkhamdb-faq/no-rulings.txt`).

So the engine has to ask someone, and it had no answer to *whom*. Every prompt frame's payload knew who must answer it, and every `InputRequest` dropped that. The engine also did not model the lead: the first Active investigator in `turn_order` stood in for them, and that one proxy was read both as "the lead" and as "first in player order".

**So every prompt names its responder, and the lead investigator is game state.**

## Who answers

Each prompt kind names one of three responders, read from its frame:

| Responder | Prompt kinds |
|---|---|
| **One named investigator** | the open turn; a substitution; a slot make-room discard; an additional-action choice; a damage distribution; an attack-order pick; a hand-discard cost; an effect's choice (its controller); a forced acknowledge (its candidate's controller); the skill test's commit window and result pause (the tester); the head of the mulligan, hand-size and encounter-draw queues |
| **The lead** | hunter-move ties; spawn-engage ties; the advance acknowledge; the order of a 2+ forced run |
| **Several answerers** | reaction windows and framework Fast windows. Each option names the investigator it is offered to. |

**The lead answers every group decision.** `glossary/Lead_Investigator.md`: *"If there are ever multiple valid options for a choice or decision that must be made (for example, a hunter enemy that could move in two different directions), the lead investigator is the final arbiter in choosing among those options."* `glossary/Priority_of_Simultaneous_Resolution.md` gives the forced run's order to the same seat: *"If two or more forced abilities (including delayed effects) would resolve at the same time, the lead investigator determines the order in which the abilities resolve."* The lead is resolved to an investigator wherever the engine knows one, so a host never re-derives it, and the wire still says that the lead is the one deciding.

**A reaction belongs to the investigator it is offered to**, the triggerer under [ADR 0018](0018-one-trigger-scan-walks-the-whole-board.md)'s reachability. `glossary/You_Your.md`: *"When resolving a triggered ability ([fast], [reaction], or [action] ability), "you/your" refers to the investigator triggering the ability."* **A commit belongs to the committer.** Today only the tester commits; commits by others are #65.

**In a several-answerer window, each investigator owns their options and passes separately**, and the window closes once every answerer has passed or nothing remains. Appendix II gives the window no single owner: *"Players may use [free] triggered abilities in these windows."* Building this needs a pass that says who passed, which #581's seat check supplies, so it is deferred to phase 8 as #1194. Until then one pass closes the window for every answerer.

**What a prompt asks, and who answers it, is one exhaustive function of its frame**, with no wildcard arm, so a new prompt frame kind does not compile until it is classified. Push sites do not assemble requests, and the host-facing outcome is built from the top frame. That is what lets every prompt be shown again, and what lets a native that reaches a framework prompt simply return, with the prompt surfacing after it (#1139).

It also closes the `PickMultiple` hand-index convention, under which the request carried no options and a host read `OptionId(i)` as index `i` of a hand the request never named. With two hands on the table that was every hand at once. A subset pick carries its candidates as anchored options plus how many may be picked, as [ADR 0011](0011-the-engine-names-the-surface-a-prompt-renders-on.md) already requires of a single pick.

## The lead and player order

**The lead is a creation input, defaulting to the first seat.** Appendix III step 3: *"Choose one of those investigators to be the lead investigator for this game."* It is not a setup prompt, because the players have made the choice before the game exists, and it is not random. `glossary/Lead_Investigator.md`'s fallback, *"If they cannot agree on a choice, a lead investigator is chosen at random"*, never arises: the creation input is the choice the players agreed on.

**Player order is the seating rotated to start at the lead**, Active investigators only. `glossary/In_Player_Order.md`: *"the lead investigator performs his or her part of the sequence first, followed by the other players in clockwise order."* Mythos 1.4's draws, Enemy 3.3's attacks, the Upkeep draws and resources, mulligan, hand size, an "each investigator" effect and the Prey tie's candidate list all walk it. **Investigation 2.2's turn choice walks it too, as a stand-in.** The rule is a group choice, Appendix II 2.2: *"The investigators choose among themselves who (among the investigators) will take this turn"*. Turn choice is #1185.

**The stored lead never changes after creation, and "the lead" is derived from it**: the first Active investigator in player order, so an eliminated lead is replaced by the next investigator after them. That is a flagged stand-in for `glossary/Lead_Investigator.md`'s *"If the lead investigator is eliminated, the remaining players (if any) choose a new lead investigator."* At two seats there is nobody else to choose, so the stand-in is exact there. Revisit it when three or more seats exist.

Deriving the replacement rather than rewriting the field is what keeps a player-order walk stable. Seat 2 is the lead, so the order is `[2, 1]`, and seat 2 is defeated by an attack during Enemy 3.3. Rewrite the stored lead to seat 1 and the rotation becomes `[1, 2]`, where seat 2 has no successor, so seat 1's enemies never attack. With the field fixed the rotation stays `[2, 1]`, seat 1 attacks next, and seat 1 is also the lead.

## A group the ability names is not the lead

**An ability that names who may act is offered to that group, not to the lead.** The Barrier 01109's objective: *"When the round ends, investigators in the hallway may, as a group, spend the requisite number of clues to advance."* `glossary/Costs.md` scopes a group cost the same way: *"each investigator (or each investigator in the group defined by the ability) may contribute to paying the cost."* The lead's *"final arbiter in choosing among those options"* breaks ties among options. It does not let a lead in the Parlor act for a group in the Hallway that they are not part of.

The mechanism is an act or agenda reaction's eligibility, with no new DSL primitive, since `docs/agents/standards.md`'s threshold wants two consumers and 01109 is the only act or agenda reaction in the corpus. The trigger scan tries the investigators in player order and binds the first whose candidate passes the matcher and the initiation gate. 01109's eligibility holds only for the first investigator in the Hallway in player order. An ability that names no group passes on the first try, so it goes to the lead as before.

**The cost is a fall-through.** An act or agenda reaction whose eligibility fails for the lead for any other reason is offered to the next investigator in player order rather than to nobody. 01109's other condition, that the Hallway can afford the clues, fails for every investigator alike, so no corpus card pays this cost. A card whose eligibility depends on who is asked, for a reason other than naming a group, is where this mechanism is reconsidered.

## Who "you" is for a constant with no controller

**"You" is the controller of the recipient**: the card being granted to, or the investigator whose value is being modified. An investigator controls themselves. A modifier on an enemy binds no "you", and a condition that needs one does not hold. `glossary/You_Your.md`: *"If an ability contains a clause identifying whom it is targeting, "you/your" in that ability refers to those investigators."*

This is [ADR 0014](0014-a-granted-ability-is-a-constant-effect-swept-off-the-board.md)'s answer for grants, extended to the modifier sweep so that the two sweeps answer *"does this constant effect apply"* the same way. #679 applies it, with the `condition` field on `Effect::Modify` that 0014 already settled.

## Considered options

**Rotate `turn_order` at seating so the lead is its first entry**, and keep the first-active proxy. It is the smallest change, but it makes the lead field redundant with `turn_order[0]`: two sources of truth for one fact.

**A declarative `offered_to` field on the ability** for The Barrier's group. It has one consumer, which falls under `standards.md`'s threshold, and a card-local eligibility already expresses it.

**Collapse the lead into a plain named investigator on the wire.** Equally cheap, but the host could no longer show that the lead is deciding, which is the one thing that tells a player why a prompt arrived at their seat.
