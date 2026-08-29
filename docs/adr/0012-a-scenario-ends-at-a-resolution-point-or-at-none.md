# A scenario ends at a resolution point, or at none

`Resolution` was `Won { id } | Lost { reason }`. The rules do not model a scenario ending that way. `glossary/Winning_and_Losing.md` describes **three** endings and one axis: the act deck and the agenda deck both invoke resolution points in the same notation — *"Some instructions in the act deck (as well as on other encounter cardtypes) contain resolution points, in the format of: '**(→R#)**.'"*, and the same sentence again for the agenda deck — and the third ending is the absence of one, *"Should the scenario end with no resolution being reached (for example, if all investigators have been eliminated or have resigned), instructions for resolving the scenario can be found in the 'do not read until end of game' section of the campaign guide."*

Whether that is a *win* is a question the rules ask only in standalone play. In campaign play *"players will proceed to the next scenario in the campaign regardless of the outcome of the scenario. Even if players 'lose' a scenario, they still continue their campaign (although with some negative consequences from their failure)."* The scare quotes around "lose", and around *"they may even have 'won!'"* earlier in the same entry, are the source's. Only the standalone bullet collapses the axis: *"They win if they complete a resolution on an act card. Any other resolution is considered a loss."* So **a scenario ends at a resolution point, or at none; win and loss are a standalone-mode projection over that fact, computed where the ending is displayed and stored nowhere.**

```rust
pub struct ResolutionId(u8);
pub enum ScenarioEnding { Resolution(ResolutionId), NoResolution }
```

The engine stored the projection and discarded the fact, and two defects followed in opposite directions. **The id was thrown away:** `the_gathering.rs` recorded terminal agenda 01107 as `Lost { reason: "The ghouls break free" }` while the card's own reverse prints `(→R3)`; `scenario.rs` flagged that `reason` field as *"Not semantically load-bearing today"*, and #766's campaign log has to look an ending up by id. **The absence was labelled a loss:** `check_all_defeated` latched `Lost { reason: "no resolution was reached" }` for `glossary/Elimination.md` step 6. #644 then makes that same door reachable by **resignation**, where `glossary/Resign.md` — *"An investigator who resigns is not considered to have been defeated"* — makes the mislabel starkest: an investigator who walks out of the Parlor alive with their clues has not lost the scenario.

**The ending does not record which deck invoked it.** This was the open design call, and the deck loses on two counts. It is a proxy rather than a fact — the same sentence that names the act deck adds *"as well as on other encounter cardtypes"*, so a treachery invoking `(→R2)` has no deck of origin and standalone mode's *"a resolution on an act card"* has no answer for it. And the question it is a proxy *for* — is this resolution favorable — is scenario-local knowledge the campaign guide owns, not something a latch site can infer: The Gathering's R1 and R2 both leave the investigators alive with Lita Chantler, while R3 kills everyone. Nothing in the workspace models standalone versus campaign play, so there is no consumer for the projection today; if one arrives, the scenario module is where it belongs.

**`ResolutionId` is a `u8`, and that is what makes the ending `Copy`.** `(→R#)` is a number and the campaign guide titles its entries "Resolution 1", "Resolution 2", "Resolution 3". Under the old `String` the dispatch sites each cloned — `agenda.resolution.clone()`, twice for acts — purely to get an owned value out from under the `&mut GameState` they were about to hand to the latch helper. All three clones are gone. A later cycle printing a non-numeric id widens the newtype, which is the point of having one.

**The three fields that were `Option<Resolution>` split by the question each asks.** `Act.resolution` and `Agenda.resolution` were `Option<ResolutionId>`: a card reverse either prints a resolution point or it does not, and `NoResolution` is never a printed value. (Both fields are since gone — a resolution point is a printed *effect* on the reverse, per [ADR 0013](0013-a-resolution-point-is-a-printed-effect.md). `ResolutionId` and the split below are what survived, and they are what this decision was about.) The latch is `GameState.ending: Option<ScenarioEnding>`, whose `None` means *the scenario has not ended* — a different question from `NoResolution`, which means *it ended without reaching a point*. Collapsing the latch to `Option<ResolutionId>` as well would have made it `Option<Option<ResolutionId>>` and left `end_scenario`'s first-writer-wins `is_none()` guard unreadable, which is the guard [ADR 0004](0004-a-latched-resolution-cancels-opportunities-not-resolutions.md) rests on. The field and its helper were renamed with the type (`resolution` → `ending`, `request_resolution` → `end_scenario`) so the latch is named for what it holds.

## Considered options

**Keep `Won { id } | Lost { reason }`.** What every other implementation of this game does, and what a reader expects — which is most of why this needed writing down. It has no representation for the third ending, forces an agenda's numbered resolution through a diagnostic string, and hardcodes a standalone-mode reading into campaign state.

**Add `NoResolution` as a third variant beside `Won` / `Lost`.** The minimal fix for #644's immediate need. It corrects the ending that was missing while leaving the two that were misclassified, and phase 9 would still have no id for an agenda-invoked ending.

**Carry the invoking deck on the ending.** Rejected for the reasons above; it was the shape this ADR started with, and the "other encounter cardtypes" clause is what killed it.

## Consequences

Nothing in `GameState` answers "did we win". The display boundary names the ending instead: `crates/web/src/board.rs` renders "Scenario ended — Resolution 3" or "— no resolution reached", which is what the player carries to the campaign guide's "do not read until end of game" section, and it stays a pure function over `GameState` with no registry access.

**`protocol` is untouched, but `web` was not** — the client renders `Event::ScenarioResolved` generically, yet `board.rs` and `web/tests/board.rs` both named `Resolution` to build the banner. The blast radius is `game-core`, `scenarios`, and `web`. ADR 0004's latch shape and the `ScenarioEnd` continuation keep theirs; only the payload changed.

The reshape exposed a fidelity gap it deliberately did not close. Agenda 01107's reverse is **conditional** — `(→R3)` only *"If the investigators are at Act 1 or 2"*, while the act-3 branch prints no resolution point at all and instead defeats everyone who has not resigned. A `resolution` *field* could not read `act_index` to choose, so the act-1/2 branch shipped alone under a `# Module gap` note. Under `Lost { reason }` that approximation was merely vague; typed, it was a wrong id — which is the cost of making the id load-bearing, and the reason the gap was written down rather than left implicit. [ADR 0013](0013-a-resolution-point-is-a-printed-effect.md) is what it forced: a resolution point is an effect the reverse runs, not a datum the card carries — the mechanism the branch needs, with the branch itself (#809) following behind it.

#775 (act 3's printed R1/R2 choice) and #766 (the phase-9 campaign log) both key off the id this preserves.

---

*Folded: #808 (ADR 0013) — the card `resolution` fields this ADR introduced are gone; the ending shape, the `u8`, and the omission of the invoking deck stand.*
