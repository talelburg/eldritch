# Chapter 1 forward-compatibility gap register — 2026-08-14

The corpus the build compiles is Core + Dunwich (48 hand-written impls). The
snapshot vendored under `data/arkhamdb-snapshot/pack/` is all of Chapter 1 —
roughly 4,500 distinct cards across twelve pack directories — and it is
planning input, nothing more (`CONTEXT.md`, **Snapshot** / **Corpus**). This
document measures that planning input against the DSL and engine we have, and
sorts what it finds into four buckets: expressible today, wants a new DSL
primitive, wants a new engine capability, or **contradicts an architectural
assumption we have already committed to**.

**This is knowledge, not a build list.** CLAUDE.md's rule stands unchanged:
*"Don't add DSL primitives speculatively — wait until two or more hand-written
cards want the same pattern."* Bucket 2 in particular is a map of where the
vocabulary would grow *if* those cards ever land, and reading it as a backlog
would invert the rule it is written under. Bucket 3 is the same for engine
verbs. Only bucket 4 asks for a decision, and even there the decision is
"decide before it costs a rewrite", not "build now" — each entry is shaped as a
self-contained conflict for a later `/wayfinder` pass to route, and deliberately
recommends no solution.

Scope note: `parallel/`, `return/`, `promo/`, and `side/` were excluded from the
survey by explicit decision and are deferred to a later pass. Everything else in
the snapshot was in scope.

## Method

1. **Read the yardstick first.** `crates/card-dsl/src/dsl.rs` (2,491 lines) and
   `crates/card-dsl/src/card_data.rs` in full; the engine's shape from
   `crates/game-core/src/state/game_state.rs` (`GameState`, the whole
   `Continuation` enum), `crates/game-core/src/action.rs`,
   `crates/game-core/src/card_registry.rs`, `crates/protocol/src/lib.rs`, and
   targeted reads into `engine/evaluator.rs` and `engine/dispatch/skill_test.rs`
   at the sites the findings cite. ADRs 0002/0003/0004 read in full. The 48 card
   impls were used as worked examples via the DSL's own doc-comments (each
   primitive names its motivating card), not read one by one.
2. **Clustered the cards by script, not by reading.** All twelve in-scope pack
   directories were merged with `jq` into one array (4,732 entries; 4,543
   distinct named cards after dropping reprint stubs with no `type_code` and
   deduplicating by `code`), then swept with two Python regex passes over
   `text` + `back_text` — roughly 160 patterns covering keywords, ability
   markers, and mechanical templates. Counts in this document come from those
   sweeps and are approximate by construction: a regex over card text
   over-counts flavour uses of a word and under-counts a mechanic phrased
   unusually. They are used to size clusters, never to make a classification.
3. **Read representatives in full** for each cluster where the classification
   was uncertain — every card quoted below was pulled verbatim from the
   snapshot JSON, with its markup intact. Depth went to bucket 4; buckets 1–3
   were sampled only until the classification stopped being in doubt.

Card text is quoted verbatim from `data/arkhamdb-snapshot/pack/`, which is
authoritative for what the build would compile from. The survey pass itself made
no web fetches; where an ArkhamDB **ruling** would change a classification, that
was flagged rather than guessed at. Those flags were resolved in a **second
pass** against the FFG FAQ, which reversed one classification and narrowed
another — see [Rulings checked](#rulings-checked-after-first-issue). Sections
revised by that pass say so inline.

## Verdict

**The architecture survives Chapter 1.** The overwhelming majority of what the
snapshot contains is either expressible today or lands as purely additive
growth along axes the DSL was explicitly designed to grow along — new
`EventPattern` variants, new `Quantity` variants, new `Cost` variants, new
engine verbs behind `Effect`. The event-sourced core, the validate-first
handler contract, the continuation-frame model, and the three ADRs' commitments
are not threatened by card content: I looked specifically for a card that
breaks validate-first/mutate-second and **did not find one** — the `apply_via`
rollback backstop makes the whole class structurally unreachable
(`crates/game-core/src/engine/mod.rs`).

Five conflicts are real, and four of the five attack the same two places: **the
`CardRegistry`'s code-keyed, `'static`, immutable shape**, and **the assumption
that a game is one scenario played by players who can all see everything**.
Neither is a bug; both are shapes chosen when the corpus was Core + Dunwich,
where nothing contradicted them. All five sit in packs — The Circle Undone, The
Innsmouth Conspiracy, The Scarlet Keys, The Feast of Hemlock Vale — that are
years of work away, which is exactly the window in which deciding is cheap.

Bucket counts (mechanics, not cards): **11 expressible today**, **11 wanting a
DSL primitive**, **16 wanting an engine capability**, **5 architectural
conflicts**.

> **Revised after a rulings check.** As first written this section reported six
> conflicts, the cheapest being nested skill tests. FFG's FAQ 1.17 says a skill
> test *cannot* initiate during another, which makes the engine's non-nesting
> assumption correct and demotes that entry to bucket 3 (item 16) as a missing
> *deferral* mechanism. Two other classifications were narrowed by the same
> pass; see [Rulings checked](#rulings-checked-after-first-issue).

> **A second lens was run afterwards.** The four buckets only ask what the card
> content *demands that we lack*; none of them can express "two things we
> already have are one thing". A consolidation pass over the same yardstick
> found seven candidates, two of which are not independently decidable because
> they sit downstream of bucket-4 conflicts. See
> [What this register did not ask](#what-this-register-did-not-ask).

## The yardstick — what the DSL and engine express today

Recorded here because everything below is measured against it, and because two
repo docs have drifted from it (see [Doc drift](#doc-drift-noticed-in-passing)).

- **Triggers** (`crates/card-dsl/src/dsl.rs:78`): `Constant`, `OnPlay`,
  `OnCommit`, `Revelation`, `Activated { action_cost }`,
  `OnSkillTestResolution { outcome }`, `OnEvent { pattern, timing, kind }`,
  `ElderSign { modifier }`.
- **Event patterns** (`dsl.rs:250`) — 15 variants, each added with a named
  first consumer: `EnemyDefeated`, `CardRevealed`, `EnemySpawned`,
  `EnteredLocation`, `PhaseEnded`, `ActAdvanced`, `AgendaAdvanced`,
  `RoundEnded`, `EndOfTurn`, `SkillTestResolved`, `WouldDiscoverClues`,
  `GameEnd`, `EnemyAttackDamagedSelf`, `EnemyAttacks`, `EnteredPlay`,
  `LeftLocation`. Timing is `When | At | After` (`dsl.rs:442`); forced vs
  reaction is `TriggerKind` (`dsl.rs:233`).
- **Effects** (`dsl.rs:606`): `GainResources`, `DiscoverClue`, `Deal`,
  `DealDamageToEnemy`, `Heal`, `Modify`, `Seq`, `If`, `ForEach`, `ChooseOne`,
  `AdvanceCurrentAct`, `Native`, `SkillTest`, `DiscardSelf`, `Cancel`,
  `PutIntoThreatArea`, `Fight`, `DrawCards`, `BoostAttackDamage`,
  `DiscoverAdditionalClues`, `Restrict`, `Investigate`, `SearchDeck`,
  `AttachSelfToLocation`.
- **Costs** (`dsl.rs:462`): `Resources`, `Exhaust`, `DiscardCardFromHand`,
  `SpendUses`, `DiscardSelf`.
- **Conditions and values**: `Condition` (`dsl.rs:1136`) is `SkillTest`,
  `SkillTestKind`, `Compare { Quantity, CmpOp, i8 }`, `Native { tag }`; the
  entire `Quantity` vocabulary (`dsl.rs:1184`) is three variants —
  `CluesAtControllerLocation`, `EngagedEnemies`, `SkillTestFailedBy`.
  `IntExpr` (`dsl.rs:1210`) is a literal, a two-branch conditional, or a
  `Quantity` count. There is no arithmetic and no nesting.
- **Modifiers are a pull query, not a layer.** `constant_skill_modifier`
  (`crates/game-core/src/engine/evaluator.rs:2051`) and
  `unconditional_constant_stat_modifier` (`:2118`) sum `Effect::Modify` over
  **the controller's own `cards_in_play` only**;
  `effective_shroud` (`:2138`) does the same over one location's
  attachments. `Stat` (`dsl.rs:887`) is the four skills plus `MaxHealth`,
  `MaxSanity`, `Shroud`. Nothing else in the game has a modifiable value.
- **The registry** (`crates/game-core/src/card_registry.rs:76`) is five
  function pointers, all keyed by `&CardCode` or `&str`:
  `metadata_for: fn(&CardCode) -> Option<&'static CardMetadata>`,
  `abilities_for: fn(&CardCode) -> Option<Vec<Ability>>`, plus three
  tag-keyed native escape hatches (`native_effect_for`,
  `native_eligibility_for`, `native_condition_for`).
- **The stack** is `GameState.continuations: Vec<Continuation>`
  (`crates/game-core/src/state/game_state.rs:140`), a 25-variant frame enum
  (`:437`) covering phase anchors, timing-point windows, effect walks, the
  skill test, encounter draws, plays-from-hand, and the scenario ending.
- **The world** is one `GameState`: `investigators` (`:40`), `locations`
  (`:42`), `enemies` (`:65`) as three `BTreeMap`s keyed by three distinct
  phantom-typed ids, plus `chaos_bag` (`:67`) and `rng` (`:88`).
- **The log** is `Vec<Action>` where `Action = Player(PlayerAction) |
  Engine(EngineRecord)` (`crates/game-core/src/action.rs:26`), and
  `PlayerAction` has exactly one variant, `ResolveInput`. Session setup is
  explicitly *not* logged: *"Session setup (seating investigators, starting the
  scenario) is handled by the non-logged `seat_and_open` entry point and never
  appears here"* (`action.rs:37-40`).

---

## Bucket 1 — expressible today

Eleven mechanics the current vocabulary covers. Counts are of Chapter 1 player
cards (asset/event/skill; 1,634 distinct) unless stated.

1. **Constant stat modifiers while in play** — `Effect::Modify` +
   `ModifierScope::WhileInPlay` / `WhileInPlayDuring(kind)`. 628 player cards
   carry no ability marker at all (no `[action]`/`[reaction]`/`[fast]`/Forced/
   Revelation) and are mostly this shape or pure skill icons; 65 of them
   contain an explicit `+N [skill]` phrase. Holy Rosary 01033 and Magnifying
   Glass 01040 are the shipped examples.
2. **Activated abilities with the shipped cost vocabulary** — action cost on
   the trigger, resources / exhaust / named uses / discard-self on
   `Ability::costs`. 1,063 snapshot cards print `[action]`, 467 print the
   `[fast]` free-triggered icon, and 301 player cards print `Uses (…)`.
   Hyperawareness 01034, `.38 Special` 01006, Beat Cop 01018.
3. **Forced and reaction abilities at the shipped timing points** — the
   open/scan/fire/close pipeline is real
   (`crates/game-core/src/engine/dispatch/reaction_windows.rs`,
   `forced_triggers.rs`), with `TriggerKind` routing forced-before-reaction per
   RR p.2. The *machinery* generalises; the *patterns* mostly do not — see
   bucket 2, item 1.
4. **Skill-card commits, including outcome-keyed ones** — `Trigger::OnCommit`,
   `Trigger::OnSkillTestResolution`, `BoostAttackDamage`,
   `DiscoverAdditionalClues`, and `commit_limit` on `CardKind::Skill`.
   Chapter 1 has 154 skill cards; the plain-icon majority needs no ability at
   all.
5. **Weapons and tools that initiate their own test** — `Effect::Fight` and
   `Effect::Investigate` carry an `IntExpr` modifier and reuse the action's
   follow-up. 200 player cards reference **Fight**, 88 **Investigate**.
6. **Deck search** — `Effect::SearchDeck` with `SearchScope::Top(n) |
   EntireDeck` and a trait/type `CardFilter`. 85 player cards search.
7. **Draw, heal, harm, resource gain** — `DrawCards` (134 player cards draw),
   `Heal` (95 heal), `Deal` with `HarmKind`, `GainResources`,
   `DealDamageToEnemy`.
8. **Treachery Revelations** — `Trigger::Revelation` with `Effect::SkillTest`
   (`on_success`/`on_fail`), `PutIntoThreatArea` for persistent threat-area
   treacheries, and `DiscardSelf` for their exit. 617 snapshot cards print
   **Revelation**; Rotting Remains 01163, Frozen in Fear 01164, Dissonant
   Voices 01165 are shipped.
9. **Constant restrictions** — `Effect::Restrict` (`dsl.rs:927`) covers
   `CannotPlay(card_type)`, `ExtraActionCost { actions, first_each_round }`,
   and `EnemyMovementBlocked`. This is exactly the shape of the Whispers in
   Your Head texts in bucket 4 — the restriction half of those cards is
   already expressible; it is the *hidden* half that is not.
10. **Cancel as a degenerate replacement** — `Effect::Cancel` plus the
    `EventTiming::When` interrupt seam (Dodge 01023, Cover Up 01007). Covers
    the "cancel that attack / that discovery" family cleanly.
11. **Location attachments with a constant modifier** —
    `AttachSelfToLocation` (Barricade 01038) and shroud-modifying attachments
    (Obscuring Fog 01168), read through `effective_shroud`.

## Bucket 2 — wants a new DSL primitive, no engine change

Eleven axes. Every one of these is a place the DSL was *designed* to grow: the
enums carry doc-comments naming the growth path, several name the exact card
that will trigger it. **None of this is scheduled work.**

1. **More `EventPattern` variants.** The single largest gap by card count and
   the least interesting architecturally. 1,224 snapshot cards print
   **Forced** and 558 print `[reaction]`; the 15 shipped patterns were each
   added with a named first consumer, and the enum is exhaustively matched
   precisely so a new one is a deliberate change (`dsl.rs:242-248`).
2. **More `Quantity` variants.** Three exist. Cards count cards in hand,
   resources, doom in play, clues on other locations, enemies in play, allies
   controlled, cards in a discard pile. 315 snapshot cards use an `X` value.
3. **Compound and target-referencing `Condition`s** — `All`/`Any`, and
   predicates about the *chosen* target rather than the controller. Already
   tracked: `TODO(#609)` at `dsl.rs:1171-1177` names Oops! 02113, Esoteric
   Formula 02254, and Springfield M1903 02226 (all Dunwich) as the cards that
   will force it, with Machete 01020's `Condition::Native` as the placeholder.
4. **Margin-keyed outcomes** — `TestOutcome::SuccessBy(u8)` /
   `FailureBy(u8)`, which `dsl.rs:1250-1254` already names as the growth path.
   127 snapshot cards say "succeed(s/ed) by", 102 "fail(s/ed) by".
5. **More `UsagePeriod` variants** — only `Round` exists (`dsl.rs:528`). 414
   cards print a "Limit … per …" and 143 say "per game".
6. **Non-spatial `EntityScope` arms** — `Engaged`, `WithTrait`, by class, by
   card type. `dsl.rs:1020-1024` reserves the growth explicitly.
7. **More `Cost` variants** — spend clues, take damage or horror as a cost,
   exhaust *another* asset, discard a card with a named icon, spend a key.
8. **More `Stat` variants** — enemy fight/evade/health, maximum hand size (6
   player cards), an asset's own health/sanity.
9. **A non-controller chooser.** `Choose<S>` (`dsl.rs:998-1006`) documents
   `chooser` as deliberately deferred: *"every choice is the controller's
   today"*. Cards that make another investigator choose want it.
10. **Exhaust / ready as an effect** rather than only as a cost. 12 player
    cards ready something; 448 mention exhaust, nearly all as a cost.
11. **Wider search scopes and filters** — search a discard pile, search the
    collection, "the top 9 cards" (On the Hunt 03263), filter by cost parity
    or icon count.

## Bucket 3 — wants a new engine capability, still additive

Fifteen. Expensive in engine work, but each slots into the existing shapes: a
new `Effect` variant with a handler, a new field on `GameState`, a new frame
variant. Nothing here contradicts an invariant.

1. **A real modifier layer.** Today modifiers are pulled from the controller's
   own in-play cards (`evaluator.rs:2051`). Nothing can modify an *enemy's*
   stats, *another investigator's* skills, or a *distant* location — e.g. Deep
   One Nursemaid 07254: *"While Deep One Nursemaid is unengaged, each other
   [[Deep One]] enemy at its location or connecting locations gets +1 fight
   and +1 evade."* This is the highest-volume item in the bucket (766 snapshot
   cards match a `+N`/`-N` modifier phrase).
2. **General replacement effects** — 227 player cards say "instead". Only
   cancel exists; `Effect::Cancel`'s doc already carries
   `TODO(#366): a true replace-with-a-different-impact effect` (`dsl.rs:717`).
   Heroic Rescue 03106 is the shape: *"Play when a non-[[Elite]] enemy would
   attack another investigator at your location."*
3. **Attachment lifecycle beyond locations.** 97 player cards attach.
   `AttachSelfToLocation` (`dsl.rs:844`) is the only attach verb and carries
   `TODO(#373)` to generalise. Enemies and `CardInPlay` have no attachment
   zone, so Hunter's Mark 11026 (*"Attach to an enemy at your location"*) and
   the Raven Quill family have nowhere to live.
4. **Chaos-bag manipulation at runtime** — sealing (29 player cards),
   adding/removing tokens, "reveal another token", replacing a token's effect.
   79 player cards reference chaos tokens. `ChaosBag` is ordinary state, so
   this is additive; token *resolution* today handles scenario symbols and
   pure-modifier elder signs only (`dsl.rs:213-218` scopes that deliberately).
5. **Decisions by an investigator who is not the actor, and group payments.**
   184 snapshot cards say "as a group"; five say *"Any investigator may
   trigger this ability"*. `AwaitingInput` prompts one responder.
6. **Action economy beyond the fixed three.** 31 player cards grant an
   additional action; Guidance 03265 grants one *to someone else*:
   *"Choose another investigator at your location who has yet to take his or
   her turn this round."*
7. **Parley and Engage as skill-test kinds.** 59 player cards parley;
   `SkillTestKind` (`dsl.rs:982-994`) explicitly says *"Add a variant when a
   new test-initiating action lands (Parley / Engage will need their own …)"*.
8. **Movement effects** — moving investigators outside the Move action (47
   player cards) and moving enemies (110 snapshot cards).
9. **Tokens on arbitrary cards** — doom on any card (21 player cards place
   doom), resources placed "as a secret", charges moved between assets.
   `CardInPlay` carries uses and an ability-usage map, not a general token bag.
10. **Unmodelled keywords** — Massive (41), Aloof (68), Alert (46), Peril
    (68), Retaliate (38, partially modelled), Permanent (66 player cards, which
    start in play rather than being drawn), Bonded (25), Myriad (22),
    Exceptional (25), Researched (27).
11. **Reading campaign facts inside a scenario.** Hall of Idolatry 04223:
    *"Check Campaign Log. If there are 5 or more tally marks under 'Yig's
    Fury', draw the top card of the encounter deck (top 2 cards instead if
    there are 10 or more tally marks)."* Additive as an *input* — a facts
    struct set at setup. (Persisting facts *across* scenarios is bucket 4,
    entry 6.)
12. **X-costs.** 75 player cards use an `X`; `CardKind::Asset`/`Event` already
    model `cost: Option<i8>` with `None` meaning X
    (`crates/card-dsl/src/card_data.rs:359`, `:387`). The play handler needs a
    player-chosen value before the cost check, which is a suspension inside a
    cost step — new, but the frame model already supports mid-validation
    suspension.
13. **"Cannot be canceled" / immunity flags** — Terrible Secret 05015 ends
    *"Cannot be canceled."*
14. **Victory display, resign, and elimination edges at Chapter-1 volume** —
    186 cards reference the victory display, 128 resign. The machinery exists;
    the coverage does not.
15. **Nested encounter draws inside a resolution.** Ruins of Eztli 04053 draws
    an encounter card from inside a skill test's Forced window. The
    `EncounterCard` frame model handles the *draw* fine — it is the drawn card's
    *test* that has nowhere to go (item 16).
16. **A second skill test needs deferring, and there is no queue to defer it
    into.** *(Moved here from bucket 4 after a rulings check.)* FFG's FAQ,
    section 'Game Play', point 1.17 ("Nested Skill Tests"), verbatim:

    > A skill test cannot initiate during another skill test. If during the
    > resolution of a skill test another skill test would initiate, instead the
    > second skill test does not initiate until the first skill test has finished
    > resolving. If the first skill test was part of an action, the second skill
    > test does not initiate until that action has finished resolving.

    So the engine's non-nesting assumption — *"At most one is ever on the stack
    (no nesting today)"* (`game_state.rs:513-515`), repeated at
    `skill_test.rs:892-894` — is **rules-correct**, and the global
    `pending_skill_modifiers` (`game_state.rs:112`) with its per-investigator
    drain is correct with it: only one test is ever live, so there is no outer
    test's state to strip.

    What is missing is the *delay*. The rule defers past **two** boundaries —
    the test, and then the enclosing action — and no deferral concept exists in
    `crates/game-core/src/engine/dispatch/skill_test.rs` or on `GameState`. The
    composition is reachable in the current corpus: Ruins of Eztli 04053's
    Forced fires at `SkillTestResolved`, inside the outer test, and the drawn
    encounter card's Revelation may be a testing treachery (Rotting Remains
    01163 is already ingested). Today that pushes a `SkillTest` frame above a
    live one; correct behaviour holds it until the whole investigate action has
    finished resolving.

    Additive — a deferred-test queue drained at an action boundary — and the
    target behaviour is specified rather than open, which is why this is bucket
    3 and not a design question. It is nonetheless the only item in any bucket
    that is a **live correctness gap in the shipping corpus** rather than
    forward-compatibility work.

---

## Bucket 4 — contradicts a current architectural assumption

Five. Each is written to stand alone as a decision ticket: the conflict, the
evidence, and the open question. **No recommendation is offered** — routing
these is `/wayfinder`'s job, and pre-empting it here would smuggle a design
decision into an audit.

*(A sixth entry, "Skill tests nest, and the engine states that they do not",
was demoted to bucket 3 item 16 after FAQ 1.17 was checked. The numbering below
closes the gap; what was 4.6 is now 4.5.)*

### 4.1 — A customizable card's abilities are a property of the copy, not the code

**The conflict.** `CardRegistry.abilities_for` is
`fn(&CardCode) -> Option<Vec<Ability>>`
(`crates/game-core/src/card_registry.rs:81`): a card's ability set is a pure
function of its printed code, and every copy of a code in every deck in every
game has the same one. The Scarlet Keys' Customizable cards break that
directly — the ability set is chosen per copy at deckbuilding time and recorded
on an upgrade sheet that belongs to the deck, not to the card. Two players in
the same game can hold the same code with different abilities; so can one
player across two scenarios. `cards::is_playable(code)` and the whole
"a card is playable iff it has an `abilities()` impl" rule (CLAUDE.md,
*Hybrid card-effect DSL*) key off the same function.

**The evidence.** 16 Chapter 1 cards print `Customizable.` (all `tskp`).
Runic Axe 09022, verbatim:

> Customizable. Uses (4 charges). Replenish 1 of these charges at the start of each round.
> [action]: <b>Fight.</b> You get +1 [combat] for this attack. Before this attack, you may spend any number of charges to imbue the axe with that many different inscriptions -
> - <i>Accuracy</i> - You get an additional +2 [combat] for this attack.
> - <i>Power</i> - This attack deals +1 damage.

and its `customization_text`, verbatim in part:

> □ <b>Heirloom.</b> This asset gets -1 cost and gains the [[Relic]] trait.
> □ <b>Inscription of Glory.</b> Add this inscription: "- <i>Glory</i> - If this attack defeats an enemy, choose one: draw 1 card, heal 1 damage, or heal 1 horror."
> □□□ <b>Ancient Power.</b> You may imbue the same inscription up to three times.

The Raven Quill 09042 goes further and parameterises the copy with a *name*:

> Customizable. When you purchase The Raven Quill, name a [[Tome]] or [[Spell]] asset and record that name on its upgrade sheet.
> Attach to a named asset you control.

**Open question.** Where does a per-copy ability set live, given that decks
arrive by import (`docs/product-decisions.md`) and the registry is the only
sanctioned bridge from engine to content? Related but separable: does a
"card" acquire an identity distinct from its code — which ADR 0002 explicitly
declined to mint (*"Minting a per-card hand-slot identity was rejected as a new
concept for no gain … nothing in the in-scope corpus distinguishes two copies
of a card in hand"*)? Chapter 1 is the corpus that distinguishes them.

### 4.2 — Cards mutate their own printed properties at runtime

**The conflict.** `CardRegistry.metadata_for` is
`fn(&CardCode) -> Option<&'static CardMetadata>`
(`crates/game-core/src/card_registry.rs:78`) — a shared immutable `'static`
reference, keyed by code. Every consumer of printed data (cost, traits, card
type, `surge`, skill icons, slots) reads through it. A card that *changes*
another card's printed properties has nowhere to write. This is distinct from
4.1: 4.1 is about ability sets fixed before play, this is about printed values
changing during play.

**The evidence.** Keyword grants and removals, verbatim:

- Overzealous 03040 — *"<b>Revelation</b> - Draw the top card of the encounter deck. That card gains surge."*
- Deep One Nursemaid 07254 — *"<b>Forced</b> - After Deep One Nursemaid engages you: Draw the top card of the encounter deck. That card loses surge."*

`surge` is a `bool` field on `CardKind::Treachery` and `CardKind::Enemy`
(`crates/card-dsl/src/card_data.rs:449` (treachery), `:433` (enemy)), read off the `'static`
metadata by the surge chain (`Continuation::PlayerDraw.surge_pending`,
`game_state.rs`). Trait and cost grants, from Runic Axe 09022's customization
sheet: *"This asset gets -1 cost and gains the [[Relic]] trait."* And, at the
far end, Counterespionage 08049 rewrites its own text:

> [reaction] When you play Counterespionage, increase its cost by 2: Change "the encounter deck" to "your deck".
> [reaction] When you play Counterespionage, increase its cost by 2: Change "you" to "any investigator".

**Open question.** Does printed metadata stay `'static` with an override layer
consulted at every read site, or does metadata become per-instance state? The
first keeps the registry's shape and pays at every call site; the second
changes what a `CardCode` *is*. Both interact with 4.1, which wants the same
answer for abilities. Note that Counterespionage's second reaction changes the
*binding of "you"* inside its own effect, which is an `EvalContext` question
rather than a metadata one — **flagged for an ArkhamDB ruling check** before it
is classified alongside the others.

### 4.3 — Hidden information has no home in a single public `GameState`

**The conflict.** There is one `GameState` and every client receives all of it.
`ServerMessage::Applied { state: Box<GameState>, … }`
(`crates/protocol/src/lib.rs:50`) is *"Broadcast to every connection of a game
after an accepted action"* and carries *"The authoritative game state after the
action resolved"*; `Hello` (`:33`) does the same on connect. Hands are
`Investigator.hand: Vec<CardCode>`
(`crates/game-core/src/state/investigator.rs:62`), stored plainly for every
investigator. There is no per-seat view, no redaction, and no concept of a fact
one player knows and another does not. Replay determinism compounds it: the
flat `Vec<Action>` log reproduces state bit-for-bit, so anyone holding the log
holds every secret in it.

**The evidence.** 25 Chapter 1 cards print the `Hidden.` keyword, whose whole
function is secrecy. Possession 03340, verbatim:

> Peril. Hidden.
> <b>Revelation</b> - Secretly add Possession (Traitorous) to your hand.
> If you have horror on you greater than twice your sanity, you are immediately eliminated and <b>killed</b>.
> You may commit this card to a skill test at your location. That test automatically fails.

Whispers in Your Head 03084d, verbatim:

> Peril. Hidden.
> <b>Revelation</b> - Secretly add Whispers in Your Head (Doubt) to your hand.
> You cannot play events.
> [action] [action]: Discard Whispers in Your Head (Doubt) from your hand.

Note that the *restriction* half ("You cannot play events") is already
expressible — `Restriction::CannotPlay(CardType::Event)` is exactly Dissonant
Voices 01165's shape (`dsl.rs:930`). It is the secrecy that has no
representation: the point of the card is that the other players do not know
which restriction is in force.

The Scarlet Keys builds an entire subsystem on it. The Red-Gloved Man 09518
prints only *"Concealed 1 [per_investigator]. Retaliate."*, and Big Ben 09511
reads:

> [fast]: Choose a concealed mini-card at Big Ben or a connecting location and test [agility] (2). If you succeed, look at the revealed side of that concealed mini-card <i>(without exposing it)</i>. If you fail, take 1 horror. (Limit once per turn.)

Gift of Madness 03186 combines secrecy with randomness: *"[action]: Randomly
choose 1 enemy from among the set-aside [[Monster]] enemies and place it
beneath the act deck without looking at it."*

**Open question.** Is hidden information a transport concern (per-seat
redaction of a fully-known server state) or an engine concern (state the engine
itself partitions, so the log cannot leak it)? The two answers have different
blast radii, and the seat/identity work already tracked in #581 is the natural
place the first would attach.

### 4.4 — Entities that do not fit the typed partition of the world

**The conflict.** `GameState` partitions the board into three disjoint,
distinctly-typed maps — `investigators: BTreeMap<InvestigatorId, Investigator>`
(`game_state.rs:40`), `locations: BTreeMap<LocationId, Location>` (`:42`),
`enemies: BTreeMap<EnemyId, Enemy>` (`:65`) — and an investigator's position is
`current_location: Option<LocationId>`
(`crates/game-core/src/state/investigator.rs:44`). The partition is load-bearing
everywhere: pathfinding, engagement, attacks, and clue discovery all take a
`LocationId` or an `EnemyId` and cannot take "whichever this is". Chapter 1
prints three separate kinds of entity that refuse the partition.

**The evidence.**

*Enemy-locations* — a distinct `type_code` in the snapshot (`enemy_location`,
13 entries, all Feast of Hemlock Vale). Shapeless Cellar 10547, verbatim:

> Massive. Retaliate. Cannot make attacks of opportunity.
> Shapeless Cellar cannot be sealed or flipped.
> <b>Forced</b> - After you fail a skill test while investigating Shapeless Cellar: Shapeless Cellar attacks you.
> <b>Forced</b> - If there are no clues on Shapeless Cellar: Add it to the victory display.

A single entity that is investigated (needs a shroud and a clue pool), attacked
and attacks (needs health and a fight value), and occupies a board position.
Living Bedroom 10532b even names the category in its own text: *"<b>Forced</b> -
When this enemy-location is revealed: Place 1 doom on it."*

*Vehicles* — assets that contain investigators and move between locations
(5 entries, The Innsmouth Conspiracy). Thomas Dawson's Car 07211a, verbatim:

> Vehicle. Limit 2 investigators in this vehicle.
> This vehicle is running. Investigators cannot enter or leave it.
> [action] If you are this vehicle's driver: Draw the top card of the encounter deck. Then, move this vehicle to a connecting [[Road]] location. (Max once per round.)
> [action] If you are this vehicle's driver: You stop the car. Flip this vehicle over.

An investigator's position becomes "inside a `CardInPlay` that is itself at a
`LocationId`" — a two-level position that `current_location: Option<LocationId>`
cannot express.

*Enemies in hand* — Watcher from Another Dimension 06017, verbatim:

> Peril. Hidden. Hunter.
> <b>Revelation</b> - Secretly add this enemy to your hand.
> You may fight or evade this enemy while it is in your hand (as if it were at your location). If you succeed, discard it from your hand. If you fail, spawn it engaged with you.
> <b>Forced</b> - When your deck runs out of cards, if this enemy is in your hand: It attacks you <i>(from your hand)</i>.

An enemy with no `EnemyId` — it is a `CardCode` in a `Vec<CardCode>` hand — that
is nonetheless a legal Fight and Evade target and can make an attack.

*Narrowed by a rulings check.* The fight/evade half is smaller than it looks.
FFG's FAQ, section 'Card Ability Interpretation', point 2.10 ("As if…", amended
October 2020), verbatim in the load-bearing part:

> The indicated ability or action is resolved with the altered game state in
> mind, but the actual game state remains unchanged. […] The game state is not
> physically altered in any way. […] Unless otherwise stated, an investigator's
> threat area is not inherently altered during the resolution of the indicated
> ability or action.

So "as if it were at your location" is **scoped to the one action** and moves
nothing: the Watcher is never engaged, never occupies a `LocationId`, and never
enters `enemies`. That asks for `Fight`/`Evade` to accept a hand-card subject
under a scoped as-if — narrow, and not a challenge to the partition.

What survives is the other half. *"**Forced** - When your deck runs out of
cards, if this enemy is in your hand: It attacks you (from your hand)"* carries
**no** as-if clause. That is an enemy with no `EnemyId` and no location making a
real attack, and it is the part of this case that still refuses the partition.

**Open question.** Does the world stay a typed partition with adapters for the
exceptions, or does it become one entity table with capability facets? The
question is worth asking now because the partition is assumed at hundreds of
call sites, and the three cases above are not variations on one theme — they
break position, identity, and zone respectively.

### 4.5 — The replayable unit is a scenario; a campaign is the unit of play

**The conflict.** Everything durable is scoped to one scenario.
`GameState.scenario_id: Option<ScenarioId>`, the `Vec<Action>` log replays one
scenario's actions, `CardInstanceId`s are *"unique within a scenario"*
(`crates/game-core/src/state/card.rs`), and trauma exists only as an emitted
`Event::TraumaSuffered` whose doc says *"persistence (campaign log, max-stat
reduction) is Phase 9 — no state mutation"*
(`crates/game-core/src/event.rs:115-117`). The log is also already not
self-contained: *"Session setup (seating investigators, starting the scenario)
is handled by the non-logged `seat_and_open` entry point and never appears
here"* (`crates/game-core/src/action.rs:37-40`). So "replaying the action log
reproduces state bit-for-bit" is true *given* a setup that lives outside the
log — and in campaign play that setup is an output of the previous scenario.

**The evidence.** Cards that write to campaign state from inside a scenario,
and cards that mutate the deck itself. Dark Pact 04038, verbatim:

> Campaign Mode only.
> Deal 2 damage to an investigator at your location.
> <b>Forced</b> - When the game ends or you are eliminated, if Dark Pact is still in your hand: Remove Dark Pact from your deck. Search the collection for The Price of Failure and add it to your deck.

That effect reaches outside the game entirely — "the collection" is the
player's card pool, not any zone in `GameState`. The Raven Quill 09042 writes
to an upgrade sheet: *"[reaction] When you resign or the game ends: Either mark
a checkbox on The Raven Quill's upgrade sheet, or reduce the experience cost to
upgrade the attached asset before the next scenario by 1."* Archaic Glyphs
03025 writes a fact: *"Record in your Campaign Log that 'you have translated
the glyphs.'"* 66 snapshot cards reference trauma, 26 experience, and 5 the
campaign log by name.

Note this entry is deliberately narrow. *Reading* campaign facts inside a
scenario is bucket 3, item 11 — additive, a facts struct set at setup. What is
in conflict is the claim that the action log is the game: under campaign play,
a scenario's log plus its setup is one node in a chain, and the chain has state
of its own that no `apply()` produced.

**Open question.** Is a campaign a sequence of independently-replayable
scenario logs with a separately-persisted carry (and if so, what makes the carry
trustworthy?), or is the campaign itself the event-sourced unit? Interacts with
schema versioning (#583) and with 4.1, since a customizable card's upgrade
sheet is exactly campaign carry.

---

## Close calls — bucket 3 or bucket 4

Recorded because the boundary is the most valuable judgment in this document
and because a later reader deserves to see which way each went and why.

- **Validate-first / mutate-second.** I went looking for a card mechanic that
  breaks it — partial payments, costs whose legality is unknowable before
  resolution, mid-cost randomness — and found none in Chapter 1. Even X-costs
  (bucket 3, item 12) only need a suspension inside the cost step, which the
  frame model supports. `apply_via`'s snapshot-and-restore
  (`crates/game-core/src/engine/mod.rs`) makes the failure mode structurally
  unreachable regardless. **No bucket-4 entry.**
- **Replacement effects** ("instead", 227 player cards). Tempting for bucket 4
  because Heroic Rescue 03106 redirects an enemy attack to a *different
  investigator*, which sounds like it fights ADR 0004's opportunity/resolution
  classification. It does not: replacement is already the shape `Effect::Cancel`
  generalises to, `TODO(#366)` names it, and the `EventTiming::When` seam is
  where it attaches. **Bucket 3.**
- **The board-wide modifier layer.** High volume and genuinely large, but
  `constant_skill_modifier` is a query function, not an invariant. Widening
  what it sums breaks no commitment. **Bucket 3.**
- **Group decisions and non-actor prompts** (184 "as a group"). `AwaitingInput`
  prompting one responder is a shape, not a rule; the frame model already
  carries `FastActorScope` for multi-actor windows. **Bucket 3.**
- **Reading the campaign log mid-scenario.** Split from 4.5 deliberately:
  reading is an input, writing is a lifecycle. **Read → bucket 3; write →
  bucket 4.**
- **Chaos-bag sealing and token replacement.** `ChaosBag` is ordinary
  serialisable state and the RNG is already recorded; sealing is a state edit.
  **Bucket 3.**
- **Nested skill tests.** The only close call I resolved *toward* bucket 4 —
  and **the rulings check reversed it.** The reasoning was that the frames
  tolerate nesting while `pending_skill_modifiers` does not, and that the
  non-nesting doc-comments were ADR 0003's failure mode recurring: comments
  asserting a safety no mechanism enforced. FAQ 1.17 says a skill test cannot
  initiate during another at all, so the comments assert a *rule*, not an
  unenforced safety, and the shared modifier list is correct. The residue — that
  the deferral the rule mandates has nowhere to live — is real but additive.
  **Bucket 3, item 16.** Recorded here rather than deleted because the
  reasoning that got it wrong is the useful part: an unenforced comment is
  evidence of a missing mechanism only once you know the comment is wrong, and
  that took a primary source, not a re-reading of the code.

## Rulings checked after first issue

The first issue of this document flagged three classifications as depending on
ArkhamDB rulings it had not fetched. All three were checked afterwards against
the FFG FAQ. Two moved a classification; one resolved to "no ruling exists",
which is itself a durable answer.

- **Nested skill tests → FAQ 'Game Play' 1.17.** *"A skill test cannot initiate
  during another skill test."* This **reversed** the classification: what was
  bucket 4 entry 4.5 is now bucket 3 item 16, a missing deferral queue rather
  than a contradicted assumption. Full quote and consequences at item 16.
- **Watcher from Another Dimension 06017 → FAQ 'Card Ability Interpretation'
  2.10 ("As if…").** The card page itself carries no ruling on engagement,
  targeting, or attacks of opportunity; it defers to the general "as if" entry,
  which scopes the alteration to the action and leaves the game state and threat
  area physically unchanged. **Narrowed** 4.4's enemy-in-hand case to the Forced
  attack, which has no as-if clause. Quoted in 4.4.
- **Counterespionage 08049 → no ruling.** ArkhamDB reports *"No faqs yet for
  this card."* Material circulating about what the *"Change 'you' to 'any
  investigator'"* kicker rebinds comes from the site's community **Reviews**
  section, which is not a ruling and is not treated as one here. 4.2's
  classification therefore rests on the card text alone, and the open question
  stands unchanged — now as a known gap in the sources rather than an unchecked
  one.

**A rule this pass surfaced that the survey never examined.** FAQ 'Game Play'
1.4 ("Nested Sequences") is a real and separate rule: a `[reaction]` or Forced
ability that creates a new triggering condition starts a nested sequence, *"and
each nested sequence must complete before returning to the sequence that spawned
it. In effect, these sequences are resolved in a Last In, First Out (LIFO)
manner."* There is no limit to the depth. This is ADR 0003's territory, and the
engine's frame stack is LIFO by construction, so it may already be correct — but
this survey chased the skill-test question and never checked it. Not classified
in any bucket; recorded so it is not mistaken for covered ground.

**Method note.** These rulings were supplied by the user from the ArkhamDB rules
page. An attempt to fetch them programmatically produced a **fabricated** rules
entry — a plausible "Nested Skill Tests" text asserting that nesting *is* legal
and resolves inside-out, which is the opposite of what FAQ 1.17 says. It was
caught only because a prompt offering an explicit "not present" escape hatch got
a different answer than the others. Summarised web fetches are not a usable
primary source for rules text.

## Doc drift noticed in passing

Not findings, but they made the yardstick harder to establish and would mislead
the next reader:

- **CLAUDE.md** describes the `CardRegistry` as *"two function pointers"*. It
  has held five since the native escape hatches landed
  (`crates/game-core/src/card_registry.rs:76-101`). The bucket-4 entries above
  cite the real shape.
- **`crates/card-dsl/src/dsl.rs:42-54`** ("Has DSL surface but not yet engine
  support") still says `Trigger::OnEvent` *"compiles and round-trips through
  serde but otherwise does nothing at runtime"* and that per-round limit
  tracking *"still needs a primitive"*. Both shipped: the reaction pipeline is
  `crates/game-core/src/engine/dispatch/reaction_windows.rs` and `UsageLimit`
  is `dsl.rs:515`. The comment also points at
  [#52](https://github.com/talelburg/eldritch/issues/52) as where the plumbing
  "lands" — **#52 is closed**, so the pointer sends the next reader to finished
  work.
- **CLAUDE.md** lists *"Horror soak isn't modeled by the DSL yet — tracked in
  #44"* under *Domain knowledge that's load-bearing but not visible in the
  code*. **#44 is closed** ("Damage/horror soak: interactive distribution +
  non-attack sources"). Not found by this survey — the claim reached it through
  its own dispatch prompt, sourced from CLAUDE.md, and was caught only when the
  referenced issues were checked against the tracker afterwards.

## What this register did not ask

Every bucket here answers one question: *what does the card content demand that
we do not have?* Bucket 1 is "nothing needed"; 2, 3, and 4 are escalating
degrees of "something needed". There is no bucket for **"two things we already
have are one thing"**, or for **"this abstraction is the wrong shape and Chapter
1 shows why"**. A consolidation finding had nowhere to land, so it would not
have been recorded even if the survey noticed one.

That is a lens, not an oversight. A second pass was run with it afterwards,
against the same yardstick; its findings are **C1–C7** below.

**This does not conflict with the speculative-primitives rule.** CLAUDE.md
guards against *adding vocabulary for hypothetical cards*. Noticing that two
**shipped** concepts with **real consumers** are one concept is not speculative;
both already exist and already have cards depending on them. The genuine risk is
different — designing a generalisation against cards that are years away is a
reliable way to build the wrong abstraction — so the output of such a pass
should be *which of our current distinctions are load-bearing and which are
accidental*, never *design the general form*. Knowing `Quantity` is heading
toward an expression language is useful **now** because it changes how the next
two variants get added; building it now is premature.

### The consolidation findings

Ordered by confidence. These are **observations, not proposals** — none is a
design, and C3 and C7 are not independently decidable at all.

**C1 — `BoostAttackDamage` and `DiscoverAdditionalClues` are one effect.**
The strongest of the seven, and visible in the doc-comments alone. `dsl.rs:770`:

> Add `N` to the in-flight skill test's **bonus attack damage** […] Accumulated
> at commit time (under [`Trigger::OnCommit`]) onto the in-flight record;
> **only a Fight skill test's follow-up reads it**, so the "during an attack"
> qualifier is intrinsic […] A no-op when there is no in-flight test.

and `dsl.rs:779`:

> Add `N` to the in-flight skill test's **bonus clue count** […] Accumulated at
> commit time (under [`Trigger::OnCommit`]) onto the in-flight record; **only an
> Investigate skill test's follow-up reads it**, so the "while investigating"
> qualifier is intrinsic […] A no-op when there is no in-flight test.

Same mechanism, same timing, same no-op condition, same intrinsic-qualifier
argument. Two variants exist because two cards wanted two fields (Vicious Blow
01025, Deduction 01039). Chapter 1 carries many more "+N to that test's X"
cards; under this shape each is a new `Effect` variant carrying a fresh copy of
that paragraph. Caveat: `DiscoverAdditionalClues` also carries the
raises-one-discovery-rather-than-adding-a-second semantic (#617, **Discovery**
in `CONTEXT.md`) — that governs how the follow-up *reads* the accumulated value,
not the accumulation, so it does not block a merge.

**C2 — the three native escape hatches are one concept.** The registry holds
`native_effect_for`, `native_eligibility_for`, and `native_condition_for`
(`crates/game-core/src/card_registry.rs:85-101`): three string-keyed tag
namespaces with three fn types, all doing *card-local Rust callback by tag*.
`TODO(#609)` already expects the condition slot to be deleted. Chapter 1
guarantees more escape hatches rather than fewer, so this is the axis most
likely to keep growing sideways.

**C3 — `Deal` / `DealDamageToEnemy` / `Heal`, which is downstream of 4.4.**
Two observations. The small one: the amount types are inconsistent by accident —
`Deal.amount` is `IntExpr` (`dsl.rs:623`) while `DealDamageToEnemy.amount`
(`:629`) and `Heal.count` (`:637`) are `u8`. Chapter 1 wants "heal damage equal
to…", so the asymmetry is not load-bearing.

The larger one: `Deal` and `DealDamageToEnemy` are split **because
`InvestigatorTarget` and `EnemyTarget` are different types**, not because
dealing harm differs — `Heal` is even documented as *"the inverse of
[`Effect::Deal`]"* (`dsl.rs:630-631`). That is 4.4's typed partition surfacing
inside the DSL, which makes this **not independently decidable**: if the world
becomes one entity table with capability facets these merge, and if the
partition holds they do not.

**C4 — `Fight` and `Investigate` are one shape.** `Investigate`'s own doc says
*"The mirror of [`Fight`](Self::Fight)"* (`dsl.rs:801`). Both initiate an
action-shaped skill test from a card effect, both take an `IntExpr` modifier,
and both are documented as inspectable-not-`Native` so the activation check can
reject before any cost is paid. The real difference is which side of the test
the modifier lands on — the investigator's total (`Fight`) or the location's
difficulty (`Investigate`). Chapter 1 adds card-initiated Evade, Parley, Engage,
and Move, so this has the same unbounded-growth shape as C5.

**C5 — `Quantity` / `IntExpr` want to be an expression language.** Three
`Quantity` variants (`dsl.rs:1184`) and *"There is no arithmetic and no
nesting"*. Chapter 1 wants many more. Under this register that surfaces as many
independent bucket-2 entries; what the register cannot express is whether the
enum should stop growing and become a small expression language instead. **No
single card asks for that**, which is precisely why a card-driven survey cannot
see it. `Stat` (`dsl.rs:887`) has the same shape — *"the four skills plus
`MaxHealth`, `MaxSanity`, `Shroud`. Nothing else in the game has a modifiable
value"* — and Chapter 1 makes more things modifiable.

**C6 — `Cancel` is a special case of replacement, not a neighbour of it.**
Already named in-tree: `Cancel` describes itself as *"the degenerate replacement
('replace with nothing')"* with `TODO(#366)` for the general form
(`dsl.rs:714-717`). Bucket 3 files this as "we will need replacement". The
consolidation reading is that the general form should **absorb** the special
case, and the ~227 Chapter 1 cards using "instead" are the evidence for which
way that subsumption runs.

**C7 — 4.1 and 4.2 may be one decision.** The Verdict observes that four of
five conflicts *"attack the same two places"*. That is most of the way to a
consolidation finding, but framed as "these problems cluster" rather than
"therefore one redesign may resolve several". 4.1 (abilities are per-copy) and
4.2 (printed properties change in play) may be a single question — *what is a
card instance?* — split in two by this document's structure rather than by the
domain. Like C3, not independently decidable.

### The pattern underneath

Five of the seven are the same failure mode: **a variant per consumer where the
mechanism is shared**. The DSL grew card-by-card, correctly, under CLAUDE.md's
two-consumers rule — but that rule says when to *add* a primitive and never says
when two already-added primitives have converged. Nothing in the process looks
back. That is a gap beside the rule, not a fault in it.

**Sequencing.** This pass belongs **before** `/wayfinder`. If 4.1 and 4.2 are
one decision, wayfinder should be handed one ticket rather than two — otherwise
it grills the same underlying question twice and can reach locally-sensible
answers that do not compose. Unpicking that afterwards is far more expensive
than merging the tickets beforehand. But note the asymmetry C3 and C7 introduce:
those two are *downstream* of bucket-4 decisions, so they belong **inside**
wayfinder as context for a conflict rather than being settled ahead of it. C1,
C2, and C4 are independent of every bucket-4 entry and could be acted on at any
time.

## Limitations

- **Sampled, not read exhaustively.** ~4,500 cards were clustered by regex over
  `text` and `back_text`; only representatives were read in full. Every card
  quoted here was read whole and quoted verbatim from the snapshot JSON, but a
  mechanic that appears on few cards *and* is phrased unusually could have been
  missed entirely. The counts are cluster sizes from regex matches and should be
  treated as order-of-magnitude, not exact — several patterns knowingly
  over-count flavour text ("doom", "instead", "cannot").
- **Encounter-side depth is shallower than player-side.** 2,825 of the distinct
  cards are encounter/scenario content (locations, acts, agendas, enemies,
  treacheries, story and scenario-reference cards). Scenario *structures* —
  The Forgotten Age's exploration deck, The Circle Undone's dual scenario,
  The Dream-Eaters' paired campaigns, The Innsmouth Conspiracy's flashbacks,
  The Scarlet Keys' web of movement — were not surveyed at all. They are
  `scenarios`-crate concerns rather than DSL/engine expressiveness, but any of
  them could surface an engine conflict this document does not name.
- **Provisional classifications.** Entry 4.2's Counterespionage 08049 clause
  (*"Change 'you' to 'any investigator'"*) may be an `EvalContext`-binding
  question rather than a metadata question; it is grouped with 4.2 provisionally.
  Bucket 3's keyword list assumes each keyword is a self-contained behaviour;
  Massive and Aloof in particular interact with engagement in ways not checked
  against the rules.
- **Rulings — now checked, with one gap.** The three classifications this
  document originally flagged as ruling-dependent have all been resolved; see
  [Rulings checked](#rulings-checked-after-first-issue). Two moved. The
  remaining gap is Counterespionage 08049, which has **no** official ruling, so
  4.2 rests on card text alone and would move if FFG ever publishes one. No
  other classification in this document was checked against a ruling — the
  buckets were assigned from card text and code, and a ruling could in
  principle move any of them.
- **The vendored Rules Reference cannot answer Chapter 1 rules questions.**
  `data/rules-reference/ahc01_rules_reference_web.pdf` is the Core-set-era
  document; `pdftotext` over it returns **zero** occurrences of "nested", and it
  predates the "As if…" entry (FAQ v1.8, October 2020) entirely. CLAUDE.md
  positions it as the offline fallback for exactly the questions this survey
  raised, and for Chapter 1 content it structurally cannot serve that role.
  Both rulings that moved a classification here came from outside the repo.
- **Excluded by decision, not by judgment.** `parallel/`, `return/`, `promo/`,
  and `side/` were not surveyed. Parallel investigators in particular are
  likely to intersect entry 4.1 — they replace an investigator's front or back
  face, which is another per-copy divergence from a printed code.
