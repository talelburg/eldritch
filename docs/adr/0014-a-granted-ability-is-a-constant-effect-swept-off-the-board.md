# A granted ability is a constant effect swept off the board

`CardRegistry::abilities_for` is `fn(&CardCode) -> Option<Vec<Ability>>`. It takes a card code and nothing else, and #774 refined that to *"static per side"* — a location shows its back's abilities while unrevealed and its front's while revealed, one choice made in one place by `engine::abilities_in_effect`. Either way, what a card can do is a pure function of the card.

The Gathering's Lita Chantler pair prints abilities that no card has. Parlor 01115, verbatim (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):

> While Lita Chantler is not controlled by a player, she gains: "\[action\]: **Parley.** Test \[intellect\] (4). If you succeed, take control of Lita Chantler."

Lita Chantler 01117:

> While you control Lita Chantler, she gains:
> "Each investigator at your location gets +1 \[combat\].
> \[reaction\] When an investigator at your location successfully attacks a \[\[Monster\]\] enemy: That investigator deals +1 damage."

`glossary/Gains.md` says what that means: *"If a card gains a characteristic (such as an icon, a trait, a keyword, or ability text), the card functions as if it possesses the gained characteristic."* Lita **has** the Parley — she is the source an activation names, and it is her ability that resolves. But it is not printed on her, it is printed on the Parlor; and which of her two ability sets she has depends on whether anybody controls her, which is game state.

**So a grant is a `Trigger::Constant` effect, found by a board sweep, and merged into the recipient's abilities one layer above the registry.**

```rust
// card-dsl
Effect::Grant {
    to: GrantTarget,              // SelfCard | Card(code)
    condition: Option<Condition>, // evaluated by the sweep, not by the evaluator
    abilities: Vec<Ability>,
}
```

`abilities_in_effect::for_source` — the funnel #774 already built, through which every enumerator reads a card's abilities — returns the side-in-effect printed abilities plus whatever the sweep grants. The sweep is the walk `modified_value::sweep` already performs over all seven in-play collections, matching `Effect::Grant` where that one matches `Effect::Modify`.

**The condition is a field on the variant, not an `Effect::If` around it.** A `Trigger::Constant` effect is *inspected* by a sweep, never *executed* by the evaluator, so it cannot borrow the evaluator's control flow: there is no resolution in progress, none of `EvalContext`'s `skill_test` / `discovery` / `choice` bindings exist, and the "else" of *"this ability applies"* is silence. A constant effect that wants a condition carries it as data. The alternative is teaching a sweep to see through `Effect::If`, and this codebase already knows how that ends — `modified_value::sweep` matches only a **bare** `Effect::Modify`, a gated one is skipped, and that gap is **#679**. Two sweeps with different traversal power would give two answers to *"does this constant effect apply"*, which is worse than either. This also tells #679 where to go: the fix for a gated modifier is a `condition` field on `Modify`, not a smarter walk.

**The sweep reads printed abilities only.** It calls the private `printed_in_effect` — today's `for_source`, side choice intact — rather than the merged public one. Two things follow. A grant cannot itself be granted, so there is no fixed point to iterate to and no termination argument to make; and the address below indexes a vector that is a pure function of `(code, side)`, which is what makes it sound. A granted grant does not misfire — it does not apply, as a limitation with a `TODO` naming its promotion.

**An ability is addressed by where it is printed.** `ResolutionCandidate.ability_index: u8` becomes:

```rust
enum AbilityAddress {
    Printed(u8),
    Granted { granter: CardCode, ability: u8, sub: u8 },
}
```

An ability has no id; a candidate minted by a scan is re-resolved by address when it fires, across a suspension. With a state-dependent ability list, a position in the merged vector is not an identity — and Lita is the card that proves it, because taking control removes the Parley and adds two buffs in the same instant, inside the Parley's own resolution. Before this, the merged index 1 on 01117 would mean *"\[action\] Parley"* at scan time and *"`Modify(Combat,+1)`"* at resolve time. `granter` is a `CardCode` rather than an `AbilitySource` because a grant is declared by printed text, and the Parlor's clause means the same thing whichever `LocationId` it landed on. Resolution re-derives the grant, so a granter that left play or whose condition flipped resolves to `None` and the candidate lapses through machinery that already exists (`lapse_reason`).

**Who "you" is for a source with no controller, answered here.** The sweep evaluates `Grant.condition` with `Option<InvestigatorId>` — the recipient's controller, `None` when uncontrolled — and a condition needing a "you" against `None` does not hold. All three near cards land: 01115's *"not controlled by a player"* is board-global, 01117's *"while **you** control"* is self-reducing on a self-grant, and Higher Education 02187's *"while **you** have 5 or more cards in your hand"* is supplied by a controller a player asset always has. #679's doc-comment poses this exact question; it is answered here rather than in the modifier sweep, so #773 is not blocked on a phase-9 issue.

**That answer forces one `Condition` variant, and it meets `standards.md`'s threshold on the same pair this ADR is about.** 01115's gate has to be answerable with **no** "you" bound, and the card-local escape hatch cannot be: a `Condition::Native` predicate is `fn(&GameState, &EvalContext) -> bool`, and an `EvalContext` names a controller, so a native cannot be asked anything at all on behalf of an uncontrolled card. (Widening that signature to `Option<&EvalContext>` is cheap — two impls, one of which already ignores its context — but it grows the one slot `card_registry.rs` says outright not to grow: *"This slot is expected to be **deleted** along with `Condition::Native` … don't grow it by registering a second card's tag."*)

So `Condition::ControlStatus { code, status }`, board-global by construction. **The polarity is a named field rather than a wrapping `Condition::Not`**, because the two consumers are complements on the same pair: 01115 asks for `ByNoPlayer`, and 01117's *"While **you control** Lita Chantler"* asks for `ByAPlayer` — self-reducing on a self-grant, since the sweep binds "you" to the recipient's own controller. A negation combinator would have had one consumer and would have had to propagate the *"cannot be asked"* case rather than invert it; a two-valued status has two consumers and cannot get that wrong.

## Considered options

**A card-local native, promoted at the third consumer.** On #368's precedent: a `native_grants_for(tag) -> fn(&GameState, &EvalContext) -> Vec<Ability>` registry slot, with a `TODO` promoting it to a declarative variant later. Rejected on `standards.md`'s stated threshold — *"A new `Effect` (or `EventPattern`) variant waits until **two or more hand-written cards want the same pattern**"* — which 01115 and 01117 meet on day one, inside this cluster, before any promotion trigger could fire. The snapshot puts more behind them: 102 cards read `gains:`, and the near backlog holds "Jazz" Mulligan 02060 (*"While 'Jazz' Mulligan is not controlled by a player, he gains: '\[action\]: **Parley.**…'"* — identical in shape to 01115), All In 02068, Fold 02069, Clover Club Lounge/Bar/Cardroom 02071-73, and Museum Halls 02127, whose unrevealed back grants an ability to a *different location*. The native also has no natural keying: the grant would be registered under the recipient's code while being printed on the granter's card. And it inherits the `EvalContext` problem above — it could not be asked about an uncontrolled recipient at all.

**Widen `abilities_for` to take `&GameState`.** It makes every card's ability lookup a state-dependent call at all of its call sites, including the card-local `abilities()` round-trips inside `crates/cards` that are answering *"what does this card print"* and have no state to pass. It also inverts what the registry is for: `card_registry` is the `OnceLock` bridge that lets `game-core` reach `cards` without depending on it, and its function pointers are `Copy` and context-free by design. A registry that reads game state is a second evaluator.

**Put both ability sets on Lita's own card, gated by eligibility predicates.** Cheapest by a wide margin: two natives on 01117 asking *"is the Parlor in play and is Lita uncontrolled"* and *"is Lita controlled"*, no sweep, no address change. Rejected because it attributes the Parley to the wrong card and leaves 01115's printed clause unimplemented, and because `Gains.md` makes granted-vs-printed a distinction the engine would then be unable to draw — *"'Gained' characteristics are not considered to be 'printed' on the card."* It works only while Lita cannot be in play without the Parlor, which is a fact about The Gathering rather than about either card.

**Keep `ability_index: u8` and re-check the trigger shape at resolve.** The cheap version of the addressing fix: the forced and reaction resolvers assert the ability at the index still has the trigger kind the candidate was minted for, as `resolve_activated_ability` already did. It catches every swap except one ability replacing another of the same trigger kind. Rejected because `ResolutionCandidate` rides `GameState` across the wire (`protocol::ServerMessage::Hello` carries `state: Box<GameState>`), so its addressing is in the serialized shape — and this is the one moment when the whole mechanism is already open.

## Consequences

**`Option` on `for_source` changes meaning.** It has meant *"this card implements nothing"*, and callers reject on `None`. It now means *"no printed **and** no granted abilities"*. The rejection those callers want survives, and #772 ships without a `lita_chantler.rs` at all — 01117 prints only her own grant declaration, which is **#773's** scope, and a stub module existing for a type-system reason would pull that issue's card into this one.

**Ownership becomes a property of an in-play card.** `CardInPlay` gains `owner: Option<InvestigatorId>` (`None` = scenario-owned), written at its single construction site (`engine::dispatch::threat_area::new_in_play_instance`) and read by `discard_card_from_play`. Taking control moves the instance without changing the owner, which is the whole content of Lita's ruling (<https://arkhamdb.com/card/01117>): *"You take control of Lita only **temporarily**, until the end of the scenario. Taking control of her doesn't make her a part of your deck."* Her removal then derives rather than being special-cased — *"If Lita leaves play while a player controls her temporarily during 'The Gathering' scenario **(i.e. while she is technically not a part of that player's deck)**, remove her from the game (do not place her into any discard pile)."* The parenthetical is the derivation; encoding only the conclusion would throw away the premise. This is reachable two ways once she is controlled — soak defeat and slot make-room if the controller later plays another Ally.

**A board assembled from parts stamps the owner it implies.** `GameStateBuilder::build` fills a blank `owner` on every card already in an investigator's play area with that investigator. There is no other way for one to be there at construction time — a scenario-owned card enters play mid-scenario, into a location's `cards_at_location`, and reaches a play area only through `Effect::TakeControl`. Without the stamp every fixture-built asset would be scenario-owned, and a discard would remove it from the game instead of filing it.

**Entering a play area is now two things.** `slots::enter_asset_making_room` takes a `CardInPlay` rather than a `CardCode` — the take-control path must preserve `instance_id`, `ability_usage`, `accumulated_damage`/`horror` and `uses`, and the frame's job (holding the card that is in no zone, ADR 0002) is served strictly better by the instance. It also takes an `AssetEntry`, because only one of the two paths is a card *entering play*: a card whose control changed was already in play, and announcing it would fire every after-enters-play reaction a second time.

**A granted ability cannot carry a printed usage limit.** `CardInPlay::ability_usage` is a `BTreeMap<u8, _>` keyed by printed index — a JSON object key is a string, so an enum key does not survive the wire — and a granted ability has no printed index. `reject_untrackable_usage_limit` refuses one before any cost is paid, beside the same refusal for a source with no card instance (#699). No corpus grant prints a limit; the Parlor's Parley is unlimited.

**`Effect::If { then: Grant }` is a trap.** It is silently invisible to the sweep. The variant's doc-comment says so, and each granting card's own test pins the bare shape, as `parlor.rs`'s back-side test already does for its `Restrict`.

**The two grants are complements, and nothing enforces it.** 01115 grants while Lita is uncontrolled, 01117 while she is controlled, so exactly one fires. That is a property of the two cards agreeing, not of the mechanism: a third card granting to Lita would simply append. This is correct — `Gains.md` puts no cap on gained characteristics — but the engine has no notion of a grant *replacing* one, which is the shape to watch for.
