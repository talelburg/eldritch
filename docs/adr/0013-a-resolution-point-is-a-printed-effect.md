# A resolution point is a printed effect, not a card datum

[ADR 0012](0012-a-scenario-ends-at-a-resolution-point-or-at-none.md) made a scenario's ending a resolution point or none, and stored the point a terminal card invokes as a flat field: `Act.resolution` and `Agenda.resolution`, both `Option<ResolutionId>`. That works for a reverse whose entire printed content is `(→R#)`. It cannot express a reverse that decides.

The Gathering has one of each, and they are the corpus's only two terminal cards. Agenda 3, **They're Getting Out!** (01107), `back_text` verbatim (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):

> - If the investigators are at Act 1 or 2, they are trapped inside the house as the ghouls tear them apart. **(→R3)**
> - If the investigators are at Act 3, they barely escape with their lives, allowing the ghouls to run rampant. Each investigator that has not resigned is defeated and suffers 1 physical trauma.

Two branches, and **only the first carries a resolution point.** Act 3, **What Have You Done?** (01110), asks instead:

> The lead investigator must decide (choose one):
> - It was never much of a home. Burn it down! **(→R1)**
> - This hell-pit is my home! No way are we burning it! **(→R2)**

One branches on board state, the other on a player's choice. A field can hold neither, so the engine held `Some(ResolutionId::new(3))` and `Some(ResolutionId::new(1))` — latching R3 on a doom-out the card does not print it for, and making R2 unreachable.

**So a resolution point is a printed effect on the card's reverse, like every other thing printed on a reverse.** `Effect::ReachResolution(u8)` is a `card-dsl` primitive; `Act.resolution` and `Agenda.resolution` are deleted; and a card is **terminal because it is the last card in its deck**, which is a structural fact the deck already carries rather than a flag a card could carry wrongly.

```rust
// card-dsl — a bare u8, because card-dsl has no workspace dependencies
// and ResolutionId lives in game-core. Converted at the evaluator.
Effect::ReachResolution(u8)
```

This is the mechanism both branches needed, built once. On it, 01107's reverse becomes an `Effect::If` over a card-local `Condition::Native` reading `act_index` (#809), and 01110's becomes an `Effect::ChooseOne` of two `ReachResolution`s (#775). Neither needs a second theory of what a reverse is — which is the whole argument for building the mechanism before either card fix.

**It also un-skips a step the engine was skipping.** `check_doom_threshold` (`crates/game-core/src/engine/dispatch/act_agenda.rs`) read the field and latched the ending *instead of* advancing, so a terminal agenda never emitted `AgendaAdvanced`, never paused on #558's advance-flip acknowledge, and never fired its reverse as an ability — while agendas 01105 and 01106 did all three. A terminal agenda now takes the same `AdvanceReverse` path as every other one, and the ending is latched by the reverse it fires. At the table you flip the agenda and read its back; the player has to see *which* branch fired, because one kills them at R3 and the other kills them at no resolution at all.

**Why the DSL gets a new variant here, when `standards.md` says to wait for two consumers.** It has two on day one — 01107 and 01110 — and every future scenario's terminal cards behind them. More to the point, this is not a variant *added* for a card; it is where a deleted `GameState` field went. The threshold exists to stop the DSL's shape being fixed around a sample of one, and a primitive that replaces engine state for every scenario in the game is the opposite case.

**What stays native, and why.** 01107's act-3 branch fans out — *"Each investigator that has not resigned is defeated"* — and that stays a card-local native loop. `Effect::ForEach` exists in the DSL but its evaluator arm is `awaiting_input_stub("ForEach")` with the `InvestigatorTargetSet` resolver unwired, and its general design is deferred with open questions (#363). Independently, a fan-out needs a body to fan out *to*, and a defeat effect has exactly one consumer. The loop and the body fail the same threshold separately, so neither graduates. 01107 is nonetheless a *second* investigator-all consumer alongside agenda 01105, which retires #363's "the two current consumers are different shapes" defence; that is recorded on #363 rather than acted on here.

**A defeat by card ability will be neither killed nor insane.** When 01107's branch lands, `DefeatCause` gains `CardAbility` and `Status` gains `Defeated`, on the strength of `glossary/Defeat.md`: *"An investigator might also be defeated by a card ability."* The existing `Killed` / `Insane` are not available to reuse, because the same entry makes those the consequences of *trauma*, not of defeat — *"Taking trauma may cause an investigator to be **killed** or driven **insane**"* — and 01107's investigators take one physical trauma, which is the first step on that track rather than the end of it. That entry is also why the card prints its trauma clause at all: trauma follows automatically only *"In campaign play, an investigator that is defeated by taking damage equal to his or her health suffers 1 physical trauma."*, and this defeat is not by damage.

The act-3 branch then reaches its ending by the rules' own route rather than by a shortcut that happens to agree with them: defeating the last active investigator drains `check_all_defeated`, which latches `NoResolution` for `glossary/Elimination.md` step 6, *"If there are no remaining players, the scenario ends."* And *"that has not resigned"* needs no filter — `apply_investigator_defeat` already early-returns on any non-`Active` status.

## Considered options

**Keep the field and add a scenario-module hook** — `ScenarioModule::resolution_for(deck, index, &GameState) -> Option<ResolutionId>`, a function pointer the latch site consults. Expresses 01107's board-state branch and nothing else: a hook consulted at the latch site cannot ask the lead investigator a question, so 01110 would still need a second mechanism. Widening the field to `ResolutionPoint::Fixed(id) | Conditional(tag)` fails the same way.

**Keep the field as the unconditional shorthand, and let a conditional card carry an effect instead.** Terminality still derived from deck position, every terminal card still flips and fires its reverse; the ending latched by the field *or* by a `ReachResolution` on the reverse. This was the cheap option and it was close. It leaves **two ways a resolution point can be latched**, with nothing preventing a card from carrying both and the field silently losing the first-writer-wins race — and it makes "is this card's ending in the data or in the code?" a question every future scenario author has to ask per card. Rejected for one source of truth, knowing the price.

**Only 01107 changes; every other terminal card keeps latching immediately.** Cheapest by far — one card, no sweep. Rejected because it hands the advance-flip fidelity to exactly the card being fixed and denies it to 01110 and to every scenario after, which is an inconsistency the next author inherits without being told why.

## Consequences

**The sweep is the cost, and it was known before the decision.** 104 construction sites across 38 files carried `resolution:` — 83 `None` (mechanical deletion) and 21 `Some`, against only 3 read sites. The 21 are the real work: each is a fixture whose point is *"this card is terminal, so reaching it ends the scenario"*, and under this ADR a card reaches a resolution point only via an ability from the card registry. Nine of them are **in-crate `game-core` unit tests**, where `OnceLock<CardRegistry>` is process-global and per-test installs collide — the constraint that already pushed `fire_forced_on_enter` into `test_support`. They are served by one synthetic terminal card there, composed into each mock's `abilities_for` the way `metadata_for_test_inv` is documented to be composed today, rather than by relocating tests or by pointing fixtures at real corpus codes that a snapshot bump could move.

**Terminal cards now emit `AgendaAdvanced` / `ActAdvanced` before the ending latches**, so any test asserting an exact event sequence across a terminal advance changed. `advance_reverse::finalize`'s past-the-end assertion is replaced by its inverse: a terminal card does not bump the cursor, and the ending must be latched by the time it finishes — so a terminal card that reaches neither a `ReachResolution` nor a defeat fails loudly instead of silently running off the end of the deck.

**The mechanism and the card fix ship as two PRs**, #808 then #809. The first is a pure refactor whose entire claim is *nothing behaves differently except the advance-flip*; the second is 01107's branch, and every consequence named above that touches `DefeatCause`, `Status`, or trauma belongs to it. Every one of the 38 files is in the first, so the second is small enough to read as a card fix. Between the two, 01107's reverse reaches R3 unconditionally and 01110's reaches R1 unconditionally — behaviour-identical to the fields they replace, and recorded as a `# Module gap` on each card.

**Physical trauma will be announced, not recorded.** The act-3 branch emits `Event::TraumaSuffered { kind: Physical, count: 1 }` per investigator, following Cover Up 01007's mental-trauma precedent; nothing persists it until the phase-9 campaign log (#766). An ending that announced nothing would leave #766 nothing to log.

ADR 0012's substance is untouched — the ending shape, the `u8`, and the deliberate omission of the invoking deck all stand. Only its description of the two card fields was stale, so it is **folded** rather than superseded: the fields are named in the past tense there, with a pointer here and a one-line changelog footer.
