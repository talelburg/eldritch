# Rules-conformance pass — abilities, triggers, reaction windows, and effect resolution — 2026-08-16

The full official Rules Reference is now vendored verbatim at
`data/rules-reference/rules/` (six section files plus 188 glossary entries).
This document reads the rules that govern **one surface** — when an ability
initiates, in what order simultaneous abilities resolve, what a reaction window
is, and what "the effect resolves" means — and checks the engine against them.
It reports only places where engine behaviour **contradicts a verbatim rule**,
each classified as **WRONG** (the concept is modelled and the model is wrong) or
**ABSENT** (there is no representation for the concept at all).

**Scope.** `crates/game-core/src/engine/dispatch/{emit,forced_triggers,
reaction_windows,coordinator,abilities}.rs`, `engine/evaluator.rs`,
`engine/outcome.rs`, the `Continuation` machinery in
`crates/game-core/src/state/game_state.rs`, and `crates/card-dsl/src/dsl.rs`.
Skill-test *arithmetic* and *combat* are in scope only where they intersect
ability timing (ST.4/ST.5 ordering, attack ordering). Deckbuilding, campaign
persistence, the protocol layer, and scenario structure are out of scope.
Mechanics belonging to cycles outside Chapter 1 are out of scope and are not
reported. This is **not a build list**: seven contradictions in a surface this
dense is a good result, and three of the seven are latent rather than reachable
by any card the build compiles today. Where that is so, the finding says so.

## Method

1. **Read the rules first, in full.** `Appendix_II_Timing_and_Gameplay.md` and
   `Appendix_I_Initiation_Sequence.md` end to end, plus these glossary entries
   read whole: `Ability`, `Triggered_Abilities`, `Triggering_Condition`,
   `Forced_Abilities`, `Constant_Abilities`, `Reaction_Opportunities`,
   `Nested_Sequences`, `Nested_Skill_Tests`, `When`, `After`, `At`, `If`,
   `Then`, `Instead`, `May`, `Must`, `Cannot`, `As_If`, `Gains`, `Blank`,
   `Cancel`, `Priority_of_Simultaneous_Resolution`, `Delayed_Effects`,
   `Lasting_Effects`, `Limits_and_Maximums`, `Choices_and_the_Grim_Rule`,
   `Effects`, `Modifiers`, `Target`, `Costs`, `Additional_Costs`,
   `Self_Referential_Text`, `Limbo`, `Exhaust`, `Ready`, `Keywords`,
   `Revelation`, `Fast`. Every quote below was copied from the vendored file,
   not from memory; no network fetch was made.
2. **Read the code in full.** `reaction_windows.rs` (2,746 lines),
   `forced_triggers.rs` (808), `coordinator.rs` (138), `emit.rs` (296),
   `abilities.rs` (416), `dsl.rs`'s vocabulary half (triggers, effects, costs,
   conditions, targets), `outcome.rs`, and the `Continuation` enum. Targeted
   reads into `evaluator.rs` (the frame stepper, the initiation gate, the
   constant-modifier queries, target grounding), `skill_test.rs` (the ST.3–ST.7
   driver), and `combat.rs` (attack ordering).
3. **Confirmed every finding against a card.** Printed text is quoted from
   `data/arkhamdb-snapshot/pack/`; rulings from `data/arkhamdb-faq/<pack>/`.
   Where a card impl is the evidence, it was read
   (`crates/cards/src/impls/*.rs`).
4. **Cross-referenced before reporting.** `docs/audits/2026-08-14-chapter-1-
   forward-compatibility.md` read in full, ADRs 0002/0003/0004 read in full,
   `git show 3a9d5fa` (PR #116, the origin of the reaction pipeline) read for
   what was deliberately deferred, and `gh issue view` run on every `TODO(#NNN)`
   anchor a candidate finding touched (#366, #495, #607, #212, #435, #581).
   Anything already registered or tracked is named as such rather than
   re-reported.

Two candidate findings did not survive verification and are recorded under
[Checked and found sound](#checked-and-found-sound) instead, because the
reasoning that killed them is the useful part.

---

## Findings

### 1 — The chaos symbol's ST.4 effect resolves after the skill value has already been computed

**The rule.** `data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`,
the Skill Test timing chart, in order:

> ST.4 Reveal chaos token.
> ST.4 Resolve chaos symbol effect(s).
> ST.5 Determine investigator's modified skill value.

and the body of ST.5, verbatim:

> Start with the base skill (of the skill that matches the type of test that is
> resolving) of the investigator performing this test, and apply all active
> modifiers, including the appropriate icons that have been committed to this
> test, effects of the chaos token(s) revealed, and all active card abilities
> that are modifying the investigator's skill value.

"All **active** card abilities" is read at ST.5 — after ST.4 has resolved.

**The code.** `crates/game-core/src/engine/dispatch/skill_test.rs:328-345`
computes `sum_skill_value(...)`, the total, the margin and the pass/fail verdict;
`:368-381` stashes that verdict onto the in-flight test as `resolved`; and only
then, at `:385-388`, does it push the symbol's immediate effects:

```
// ST.4 apply the symbol's immediate side-effects (Tablet damage when Ghouls
// are present), pushed as Effect::Deal (interactive soak → may suspend).
```

The comment at `:326-327` states the assumption the ordering rests on: *"The
in-scope `immediate` effects (damage/horror) don't change the skill value, so
computing the total before they resolve is correct."* That assumption is false
in the shipping corpus.

**Divergence scenario.** The Gathering, Standard bag. An investigator with Beat
Cop 01018 in play — *"You get +1 [combat]."* (`data/arkhamdb-snapshot/pack/core/
core.json`; declared as `constant(modify(Stat::Combat, 1, WhileInPlay))` at
`crates/cards/src/impls/beat_cop.rs:33`) — already carrying 1 damage, Fights a
Ghoul and reveals `[tablet]`. The Gathering's tablet outcome
(`crates/scenarios/src/the_gathering.rs:152-160`) is `-2` **and** 1 damage while
a Ghoul is at the location. The player soaks that damage onto Beat Cop, whose
printed health is 2, so Beat Cop is defeated and discarded.

- **Engine:** the total was fixed before the damage was dealt, so Beat Cop's
  `+1 [combat]` is still in it. The test is resolved at `base + 1 - 2 + …`.
- **Rules:** at ST.5 Beat Cop is no longer in play, so its ability is not
  "active" and does not contribute. The total is one lower, and the test can
  flip from success to failure.

**Classification: WRONG.** The engine models modifier summation correctly (see
"Checked and found sound" — the clamp arithmetic is exact); it evaluates it one
step too early. The fix is ordering, not architecture: `sum_skill_value` is a
pure query and `DetermineOutcome` (`state/game_state.rs:1551-1562`) is already a
separate cursor step that runs after the ST.4 effects.

---

### 2 — Activated abilities are never checked against the initiation rule, and a target that cannot change state is still eligible

**The rules.** `glossary/Ability.md`, under "Triggered Abilities", verbatim:

> A triggered ability can only be initiated if its effect has the potential to
> change the game state, and its cost (if any) has the potential to be paid in
> full, taking active cost modifiers into account.

`glossary/Costs.md`, verbatim:

> An ability cannot initiate – and therefore its costs cannot be paid – if the
> resolution of its effect will not change the game state.

`glossary/Target.md`, verbatim:

> A card is not an eligible target for an ability if the resolution of that
> ability's effect could not change the target's state. *(For example, an
> exhausted enemy could not be chosen as the target of an effect that reads,
> "choose and exhaust an enemy.")*

**The code.** The engine *has* this gate —
`crates/game-core/src/engine/evaluator.rs:1928` `effect_can_change_state` — and
applies it in three places: the reaction scan (`reaction_windows.rs:199`), the
hand-Fast-event scan (`:504`), and the event-play path (`:1650`). It is **not**
applied to `Trigger::Activated`. `check_activate_ability`
(`reaction_windows.rs:2023-2135`) runs, in order: instance lookup, timing gate,
action economy, `check_cost_payable` per cost (`:2116-2122`),
`reject_incompatible_costs`, and `check_effect_target_available` (`:2125`) —
which covers only `Fight`, `DealDamageToEnemy` and `Investigate`
(`:1952-1994`). Nothing asks whether the effect can change state.

The target half is independent of that: `ground_investigator_choice`
(`evaluator.rs:1720`) enumerates every investigator in scope via
`investigator_candidates` with no state-change filter, and
`effect_can_change_state` has no `Effect::Heal` arm — it falls through to the
conservative default `_ => true` at `evaluator.rs:1961`.

**Divergence scenario.** First Aid 01019, printed
(`data/arkhamdb-snapshot/pack/core/core.json`):

> Uses (3 supplies). If First Aid has no supplies, discard it.
> [action] Spend 1 supply: Heal 1 damage or horror from an investigator at your
> location.

declared as `activated(1, [SpendUses{Supplies,1}], choose_one([heal_damage(…),
heal_horror(…)]))` (`crates/cards/src/impls/first_aid.rs:30-40`). Solo
investigator, undamaged and unhorrored, activates First Aid.

- **Engine:** the activation passes every check. One action point and one supply
  are spent, `Event::AbilityActivated` is emitted, the `ChooseOne` resolves, and
  `Effect::Heal` no-ops (its own doc, `dsl.rs:630-637`: *"a target with nothing
  to heal, is a no-op"*). If that was the third supply, the asset is discarded
  by the depletion rule for nothing.
- **Rules:** the ability cannot initiate at all — the effect has no potential to
  change the game state, and the only candidate is not an eligible target — so
  no action and no supply are spent.

Old Book of Lore 01031 is the same shape with an empty deck. Printed:

> [action] Exhaust Old Book of Lore: Choose an investigator at your location.
> That investigator searches the top 3 cards of his or her deck for a card,
> draws it, and shuffles the remaining cards into his or her deck.

With the chosen deck empty the exhaust and the action are paid for a search that
can find nothing.

**Classification: WRONG.** The gate exists and is already the right shape; it is
simply not wired to the activation path, and `effect_can_change_state` is
missing the `Heal` arm the corpus needs. Note that #495 — which introduced this
gate — is **closed** and covered the play and reaction paths only; the
activation path is not tracked by any open issue.

---

### 3 — Only one timing bucket is dispatched per triggering condition, and a `When`- or `At`-tagged trigger on any other event silently never fires

**The rules.** `glossary/Nested_Sequences.md`, verbatim:

> Each time a triggering condition occurs, the following sequence is followed:
> 1) execute "when..." effects that interrupt that triggering condition,
> (2) resolve the triggering condition, and then, (3) execute "after..." effects
> in response to that triggering condition.

`glossary/At.md` (identical text in `glossary/If.md`), verbatim:

> Some abilities have triggering conditions that use the words "at" or "if"
> instead of specifying "when" or "after," such as "at the end of the round," or
> "if the Ghoul Priest is defeated." These abilities trigger in between any
> "when..." abilities and any "after..." abilities with the same triggering
> condition.

So every triggering condition has a three-cell sequence: `when → at/if →
after`.

**The code.** The engine built exactly that machinery — `Continuation::EmitEvent`
walks `When → At → After` and `Continuation::TimingPoint` runs
forced-then-reaction inside each cell (`dispatch/coordinator.rs:32-104`) — and
then routes **one** event through it. `dispatch/emit.rs:261-269`:

```rust
if matches!(event, TimingEvent::RoundEnded) { … return EngineOutcome::Done; }
```

Every other `TimingEvent` is dispatched single-bucket immediately below: the
reaction window opens at `event.reaction_bucket()` (`emit.rs:198-206` — `When`
for `EnemyAttacks` / `WouldDiscoverClues`, `After` for everything else), and the
forced phase is hard-coded to the `After` cell at `emit.rs:279` and again at
`:290` and `:294`.

The scans filter on that cell exactly. `forced_triggers.rs:524`:

```rust
if *kind == TriggerKind::Forced && *timing == bucket && want(pattern) {
```

and `reaction_windows.rs:324` (mirrored at `:405` and `:494`):

```rust
if *kind != TriggerKind::Reaction || *timing != bucket { continue; }
```

**Divergence scenario.** Declare an ability as
`forced_on_event(EventPattern::EnemyDefeated{…}, EventTiming::When, …)` — the
literal encoding of a printed *"Forced – When an enemy is defeated: …"*. The
`EnemyDefeated` timing event only ever collects the `After` cell, so
`push_matching` never matches it. The ability is **never collected, never
resolved, never rejected, and never logged** — a silent no-op, which CLAUDE.md's
own rule for unimplemented cards ("never silently no-op") forbids elsewhere.

This is not hypothetical pressure on a hypothetical card: two shipped impls are
already mis-tagged *because* of it.

- Act 3, What Have You Done? 01110 is printed
  *"**Objective** - If the Ghoul Priest is Defeated, advance."*
  (`pack/core/core.json`) — an **"if"**, i.e. the `At` cell by
  `glossary/If.md`. It is declared `EventTiming::After`
  (`crates/cards/src/impls/what_have_you_done.rs:33`).
- Frozen in Fear 01164 is printed *"**Forced** - At the end of your turn: Test
  [willpower] (3)…"* — an **"at"**. It is declared `EventTiming::After`
  (`crates/cards/src/impls/frozen_in_fear.rs:40`).

Both are correct *today* only because their bucket is uncontested. Act 01110's
ruling (`data/arkhamdb-faq/core/01110.md`) — *"The **Objective** ability is
mandatory, it will trigger as soon as you defeat the Ghoul Priest, before any
'After you defeat an enemy' reactions can be used."* — is currently satisfied by
a different mechanism (forced-before-reaction, ADR 0004), so the mis-tag is
invisible. Add one *forced* "after an enemy is defeated" ability and the two land
in the same cell as ties, and the 2+ lead-ordered run
(`reaction_windows.rs:152-167`) lets the player put the "after" ability first —
which `glossary/At.md` forbids.

**Classification: WRONG.** The coordinator generalises to any `TimingEvent`
already; `queue_event`'s single-bucket short-circuit is a wiring decision
(its own comment: *"Every other event is single-bucket"*), not an architectural
limit. It is recorded as a finding rather than a scoping note because it fails
**closed and silently**: the mis-tag has no compile-time or runtime signal, and
the two shipped cards above show the pressure is real rather than anticipated.

---

### 4 — An `[action]` ability can only be activated from `cards_in_play`; the threat area, the act, the agenda, and locations are unreachable

**The rules.** `Appendix_II_Timing_and_Gameplay.md`, step 2.2.1, verbatim:

> **Activate** an [action]-costed ability on an in-play card you control, an
> in-play encounter card at your location, a card in your threat area, the
> current act card, or the current agenda card.

restated in `glossary/Triggered_Abilities.md` (from FAQ 1.2), verbatim:

> An investigator is permitted to use triggered abilities ([free], [reaction],
> and [action] abilities) from the following sources:
> - A card in play and under his or her control. This includes his or her
>   investigator card.
> - A scenario card that is in play and at the same location as the
>   investigator. This includes the location itself, encounter cards placed at
>   that location, and all encounter cards in the threat area of any
>   investigator at that location.
> - The current act or current agenda card.
> - Any card that explicitly allows the investigator to activate its ability.

**The code.** `check_activate_ability`
(`crates/game-core/src/engine/dispatch/reaction_windows.rs:2041-2050`) resolves
the ability's source with a single lookup:

```rust
let Some(in_play_pos) = inv.cards_in_play.iter().position(|c| c.instance_id == instance_id)
```

and rejects otherwise. `cards_in_play` excludes the threat area and the
investigator card, both of which the *reaction* path reaches through
`Investigator::controlled_card_instances()` (used at `reaction_windows.rs:288`
and by every constant-modifier query, `evaluator.rs:2267`) — so the two paths
disagree about what "a card you control" is. Locations, acts and agendas have no
`CardInstanceId` at all (`crates/game-core/src/state/location.rs:21-52`;
`Act`/`Agenda` are code-plus-cursor records), so they cannot be named by
`PlayerAction::ActivateAbility`, whose payload is an instance id
(`dispatch/abilities.rs:63-68`). `bump_usage_counter`
(`reaction_windows.rs:1377-1384`) makes the assumption explicit: a
`Board`/`Location`-sourced candidate with a usage limit is `unreachable!()`.

**Divergence scenario.** Haunted 01098, printed verbatim:

> **Revelation** - Add Haunted to your threat area.
> You get -1 to each of your skills.
> [action] [action]: Discard Haunted.

The two-action self-discard is the card's only exit. Under the engine there is no code path that can activate
it: the treachery lives in `threat_area`, which `check_activate_ability` does not
search. The same holds for Psychosis 01099, Hypochondria 01100, Dreams of R'lyeh
01182, and for every location `[action]` in the corpus (Your House 01124,
Southside 01126/01127, St. Mary's Hospital 01128, Miskatonic University 01129,
Downtown 01130/01131, Northside 01134, Arkham Woods 01155, …), every act
`[action]` (Uncovering the Conspiracy 01123, Disrupting the Ritual 01148), and
every agenda `[action]` (the resign abilities on 01121a/01122).

None of these cards has an `abilities()` impl yet, so nothing mis-resolves today
— but the ceiling is structural, not a missing impl, and it is what a Phase-7+
card would hit.

**Classification: mixed, and the split is the point.**
- **Threat-area cards: WRONG.** The engine has the instance, resolves its
  constant abilities and its forced triggers from it, and simply does not look
  there when activating. A one-zone widening of the lookup.
- **The act, the agenda, locations, and encounter cards at your location:
  ABSENT.** There is no addressable ability source for them — no instance
  identity for a location or an act, and `ActivateAbility` has no other way to
  name one. Supporting them changes what an ability source *is*, and interacts
  with the forward-compatibility register's 4.4 (the typed partition of the
  world).

---

### 5 — `Effect::Seq` lets an after-window resolve between two aspects of one effect

**The rule.** `glossary/Effects.md`, verbatim:

> All aspects of an effect have timing priority over all "after..." triggering
> conditions that might arise as a consequence of that effect. *(For example, if
> an effect reads "Gain 3 resources and draw 3 cards," resolve both aspects of
> the effect (gaining resources and drawing cards) before initiating an ability
> that reads "After drawing a card...")*

and `glossary/Then.md`, verbatim:

> The post-then aspect of an effect has timing priority over all other indirect
> consequences of the resolution of the pre-then aspect.

**The code.** `step_effect_frame` (`crates/game-core/src/engine/evaluator.rs:
344-357`) steps a `Seq` by pushing the *advanced* `Seq` frame and then the child
frame on top of it:

```rust
cx.state.continuations.push(Continuation::Effect(EffectFrame::Seq { effects, next: next + 1, ctx }));
cx.state.continuations.push(Continuation::Effect(child));
```

Any window the child queues (`queue_event` → `queue_reaction_window`,
`reaction_windows.rs:72-79`) is pushed above the parked `Seq` frame, so the LIFO
`drive` loop resolves the reaction opportunity **before** the `Seq`'s remaining
steps.

**Divergence scenario.** An ability declared
`Seq([DealDamageToEnemy(chosen, 1), GainResources(You, 2)])` whose damage
defeats the enemy:

- **Engine:** the defeat queues the after-defeat window above the `Seq`; Roland
  Banks 01001's *"[reaction] After you defeat an enemy: Discover 1 clue at your
  location"* is offered and resolved, and only afterwards are the 2 resources
  gained.
- **Rules:** both aspects of the effect resolve first; only then may the
  after-defeat reaction be initiated.

**Reachability, honestly.** No shipped card reaches this. `Effect::Seq` has one
in-corpus consumer, Cover Up 01007's `Seq[discard-from-self, Cancel]`
(`crates/cards/src/impls/cover_up.rs:51`), and neither step opens a window. The
engine's multi-defeat effect, Dynamite Blast 01024, is a card-local `Native`
that loops synchronously (`crates/cards/src/impls/dynamite_blast.rs:115-140`) and
therefore *does* satisfy the rule — by construction, not by design. The finding
is that the DSL's own sequencing primitive does not.

**Classification: WRONG.** The framework has the concept (frames, LIFO,
tail-position emission per ADR 0003); `Seq`'s frame layout puts the window in the
wrong place. Note this is adjacent to but distinct from ADR 0003, which is about
a *caller* doing synchronous work after an emit — here the caller is correct and
the parent frame is the thing that gets jumped.

---

### 6 — The engine never says *who* decides, and one Skip ends every investigator's reaction opportunity

**The rules.** Four framework points name a decider, verbatim.

`glossary/Priority_of_Simultaneous_Resolution.md`:

> If two or more forced abilities (including delayed effects) would resolve at
> the same time, the lead investigator determines the order in which the
> abilities resolve.

`glossary/Choices_and_the_Grim_Rule.md`:

> When investigators are forced to make a choice and there are multiple valid
> options, the lead investigator decides between those options.

`Appendix_II_Timing_and_Gameplay.md`, ST.7:

> If there are multiple results to be applied during this step, the investigator
> performing the test applies those results in the order of his or her choice.

`glossary/Reaction_Opportunities.md`:

> When a triggering condition resolves, investigators are granted the
> opportunity to resolve [reaction] abilities in response to that triggering
> condition. It is only after **all** investigators have passed their reaction
> opportunity that the game moves forward.

**The code.** `InputRequest` (`crates/game-core/src/engine/outcome.rs:149-164`)
carries `prompt`, `options`, `kind` and `skippable` — and no responder. Nothing
in `EngineOutcome::AwaitingInput` identifies whose decision it is. The reaction
window compounds it: `resume_reaction_window`
(`reaction_windows.rs:994-1012`) treats a single `InputResponse::Skip` as
closing the whole window, popping the frame with every remaining candidate on it
(`close_reaction_window`, `:1395-1403`) regardless of which investigator
controls them.

**Divergence scenario.** Two investigators at the same location, an enemy is
defeated, and both have a reaction available. The engine emits one prompt naming
both options with no indication of who answers; whoever answers first may
`Skip`, and the other investigator's reaction opportunity is gone without being
offered.

**Classification: ABSENT**, and **partly registered**. There is no
representation of a per-decision responder anywhere in the engine or protocol.
The protocol half is tracked by **#581** (*"No seat/identity concept in the
protocol, WS handshake, or schema"*) and the forward-compatibility register's
4.3 frames the same shape as a hidden-information question. What neither records
is the **rules** dimension: these four points are framework rules, not card
content, and they are wrong today under any client that is not a single shared
screen. Recorded here so #581 is not resolved as a pure access-control ticket.

---

### 7 — A conditional constant ability is silently ignored

**The rule.** `glossary/Ability.md`, "Constant Abilities", verbatim:

> Constant abilities are always interacting with the game state as long as the
> card is in play. (Some constant abilities continuously seek a specific
> condition, denoted by words such as "during" or "while." The effects of such
> abilities are active any time the specified condition is met.) Constant
> abilities have no point of initiation.

**The code.** `constant_skill_modifier`'s own doc-comment
(`crates/game-core/src/engine/evaluator.rs:2036-2037`) states the gap:

> - **Conditional constants** (`Effect::If` under a `Trigger::Constant`):
>   not yet wired; this helper ignores them.

`sum_constant_modify` (`:2256-2284`) matches only
`Effect::Modify { stat, delta, scope }` directly under a `Trigger::Constant`;
any other effect shape — including an `Effect::If` wrapping a `Modify` — falls
through the `let … else { continue; }` and contributes zero. The DSL has one
conditional-constant idiom (`ModifierScope::WhileInPlayDuring(kind)`,
`dsl.rs:912-915`), which covers exactly the "during a test of this kind"
qualifier and nothing else.

**Divergence scenario.** Declare `constant(Effect::If { condition: Compare {
EngagedEnemies, Gt, 0 }, then: modify(Combat, 1, WhileInPlay), else_: None })`
— the shape of *"While you are engaged with an enemy, you get +1 [combat]"*.
The ability compiles, round-trips through serde, and contributes nothing. No
rejection, no event, no log line. (Resolving it as an *effect* would reject —
`Effect::Restrict` has that guard at `evaluator.rs:500-504` — but a constant is
never resolved, only queried, so no guard fires.)

**Classification: ABSENT.** Constant abilities are modelled as a pattern-match
over one effect shape, not as a query over an effect tree; a conditional
constant has no representation. Low severity — no shipped card wants one, and by
CLAUDE.md's rule none should be added speculatively — but it is reported because
the failure mode is a **silent** no-op rather than a loud rejection, which is the
property that makes a missing primitive expensive to find later.

---

## Checked and found sound

Each of these was checked against the verbatim rule and the engine honours it.
Several were candidate findings that did not survive.

- **Forced resolves before reaction, structurally.** `glossary/Ability.md`:
  *"For any given timing point, all forced abilities initiated in reference to
  that timing point must resolve before any [reaction] abilities (see below)
  referencing the same timing point in the same manner may be initiated."*
  `queue_event` (`emit.rs:270-295`) queues the reaction window *first* so it sits
  **beneath** the forced frames on the LIFO stack, which makes the ordering a
  property of the stack rather than of hand-sequenced calls. The coordinator's
  `TimingSub::Forced → Reaction` cursor (`coordinator.rs:71-98`) does the same
  per cell. ADR 0004's Ghoul Priest trace depends on this and it holds.
- **Nested sequences are LIFO, with no depth limit.** `glossary/
  Nested_Sequences.md`: *"There is no limit to the number of nested sequences
  that may occur, but each nested sequence must complete before returning to the
  sequence that spawned it. In effect, these sequences are resolved in a Last In,
  First Out (LIFO) manner."* `GameState.continuations` is a `Vec<Continuation>`
  driven by uniform top-frame dispatch, so nesting is the data structure. The
  rule's own worked example (a Guard Dog reaction inside an attack of opportunity
  inside a play, spawning a Forced defeat trigger inside *that*) is precisely the
  case ADR 0002 rebuilt the play-in-progress slot for (#604), and it is what
  `play_fast_event` documents at `reaction_windows.rs:1192-1195`. The
  forward-compatibility register flagged this rule as "not classified — recorded
  so it is not mistaken for covered ground"; it is now checked, and it holds.
- **One reaction does not consume the others'.** `glossary/
  Reaction_Opportunities.md`: *"Using a [reaction] ability in response to a
  triggering condition does not prevent other [reaction] abilities from being
  used in response to that same triggering condition."* `fire_pending_trigger`
  removes only the fired candidate (`reaction_windows.rs:1161-1166`) and
  `advance_resolution` (`:1280-1324`) re-prompts with the remainder. The rule's
  Roland-then-Evidence! example is the shipped behaviour.
- **Each eligible ability fires once per occurrence, per instance.**
  `glossary/Triggering_Condition.md`: *"Each eligible ability that triggers in
  reference to a specified timing point may be used once each time that timing
  point occurs. If multiple instances of the same ability are eligible to
  initiate, each instance may be used once."* The scan is per card instance
  (`reaction_windows.rs:288-359`), producing one candidate per (instance,
  ability), and each is removed on firing.
- **Modifier arithmetic and the single clamp at zero.** `glossary/Modifiers.md`
  gives the Danny worked example: base 4, −8 token, +2 from "Lucky!" ⇒ −2 ⇒
  treated as zero, *not* 0 + 2. `sum_skill_value`
  (`skill_test.rs:1027-1053`) sums base + constants + pending + committed icons +
  the initiating effect's modifier with no intermediate clamp, and the single
  `.max(0)` is applied once after the token modifier
  (`:331`, `:340`). Exactly the rule.
- **A forced ability with no potential to change state does not initiate.**
  `glossary/Ability.md`: *"If a forced ability does not have the potential to
  change the game state, the ability does not initiate."* `collect_forced_hits`
  applies `effect_can_change_state` as a `retain` over the collected hits at a
  single chokepoint feeding both the lone-hit path and the 2+ ordered run
  (`forced_triggers.rs:470-484`), so a no-op forced neither resolves nor prompts.
- **Forced runs are mandatory and cannot be passed.** *"The initiation of a
  forced ability that has the potential to change the game state is mandatory
  each time its specified timing point is met."* `resume_reaction_window`
  rejects `Skip` on a `mode: Forced` run (`reaction_windows.rs:994-1010`), and
  the interactive one-option `AcknowledgeForced` prompt
  (`forced_triggers.rs:577-598`) is a confirmation, not a choice.
- **A cancelled ability still counts against its limit.**
  `glossary/Limits_and_Maximums.md`: *"If the effects of a card or ability with a
  limit or maximum are canceled, it is still counted against the limit/maximum,
  because the ability has been initiated."* `fire_pending_trigger` bumps the
  usage counter *before* pushing the effect (`reaction_windows.rs:1175-1178`), so
  initiation is what counts. (`bump_usage_counter`'s own doc-comment at
  `:1338-1345` still says *"today we only bump on successful resolution"* and
  describes the fix as future work — that is stale by one refactor; the behaviour
  is correct and the comment is not. Doc drift, not a finding.)
- **Per-instance limits reset when a card leaves play.** *"If a card leaves play
  and re-enters play during the same period, the card is considered to be
  bringing a new instance of the ability to the game."* The counter lives on
  `CardInPlay.ability_usage`, which is dropped with the instance
  (`dsl.rs:509-513` records the reasoning).
- **Attack order is the attacked investigator's.**
  `Appendix_II_Timing_and_Gameplay.md` 3.3: *"If an investigator is engaged with
  multiple enemies, resolve their attacks in the order of the attacked
  investigator's choosing."* `resolve_attacks_for_investigator`
  (`combat.rs:583-597`) snapshots the attacker set and `drive_attack_loop`
  suspends on a `PickSingle` between attacks with 2+ attackers.
- **Fast timing.** `glossary/Fast.md`: *"A fast event card may be played from a
  player's hand any time its play instructions specify … A fast asset may be
  played by an investigator during any player window on his or her turn …
  Because fast cards do not cost actions to play, they do not provoke attacks of
  opportunity."* All three clauses are implemented distinctly:
  `check_play_card`'s gate splits Fast events (any permissive window) from Fast
  assets (owner-is-active + permissive window) at
  `reaction_windows.rs:1793-1817`, and `provokes_aoo`
  (`abilities.rs:140-142`) exempts `action_cost == 0`.
- **A reaction event is not a free-timing play.** The same `Fast` rule — *"the
  card may be played as if the described timing point were a triggering
  condition for playing the card"* — is enforced by rejecting a
  `TriggerKind::Reaction` event from the standalone `PlayCard` action
  (`reaction_windows.rs:1746-1763`), so Dodge 01023 and Evidence! 01022 are
  playable only in their windows.
- **Constant abilities read from every zone the card can be in play in.**
  `glossary/Ability.md`: *"Card abilities only interact with the game if the card
  bearing the ability is in play."* `sum_constant_modify`
  (`evaluator.rs:2256-2284`) walks `controlled_card_instances()` — investigator
  card, cards in play, and threat area — so a threat-area treachery's constant
  contributes, and `effective_shroud` (`:2138`) does the same for location
  attachments.
- **Cancel is initiation-preserving.** `glossary/Cancel.md`: *"Any time the
  effects of an ability are canceled, the ability (apart from its effects) is
  still regarded as initiated, and any costs have still been paid."*
  `Effect::Cancel` sets a `pending_cancellation` signal the emit site honours
  after the window closes, skipping only the prevented impact
  (`dsl.rs:707-718`, `reaction_windows.rs:1517-1531`); the cancelling event card
  itself still discards through its `PlayFromHand` frame.
- **Limbo.** `glossary/Limbo.md`: *"An event card enters limbo during step 3 of
  the Initiation Sequence, after costs are paid and attacks of opportunity are
  made … It is no longer considered to be in any investigator's hand, but it has
  not yet been placed in any discard pile."* This is exactly the state ADR 0002
  moved onto the `Continuation::PlayFromHand` frame. The vendored `Limbo` entry
  did not exist in the repo when that ADR was written and it corroborates it
  precisely, including the nesting the ADR was motivated by.
- **Skill tests do not nest, and that is right.** `glossary/
  Nested_Skill_Tests.md` confirms what the forward-compatibility register already
  established from FAQ 1.17: *"A skill test cannot initiate during another skill
  test."* The engine's non-nesting assumption (`game_state.rs`, the `SkillTest`
  frame's *"At most one is ever on the stack"*) is rules-correct. The missing
  **deferral** the same rule mandates is **already registered** as bucket 3
  item 16 of `docs/audits/2026-08-14-chapter-1-forward-compatibility.md`, and the
  vendored text does not contradict what that entry assumed.
- **A stale reaction option cannot be fired.** `withdraw_lapsed_candidates` and
  `candidate_still_offerable` (`reaction_windows.rs:772-909`) re-run the scan at
  both prompt sites and at fire time, quoting the initiation rule in the
  doc-comment. The same gap on the **forced** run is acknowledged in that
  doc-comment and **tracked as #607** — reported there, not here.
- **Replacement effects beyond cancel** (`glossary/Instead.md`) are **tracked as
  #366** and registered as bucket 3 item 2 of the forward-compatibility audit.
- **Limit periods other than "per round"** (`glossary/Limits_and_Maximums.md`'s
  "per game", "per turn", "group limit", "Max X per period") are **already
  registered** as bucket 2 item 5.

---

## Uncertain

Things the vendored sources do not settle, and what would settle them.

- ~~**When exactly does "after you successfully investigate" trigger?**~~
  **Settled (#750, #756).** `data/official-faq/Rulings_and_Clarifications.md`,
  section 1.7 ("Skill Test Results and Advanced Timing"):

  > [reaction] or Forced abilities with a triggering condition dependent upon
  > the skill test being successful or unsuccessful (such as "After you
  > successfully investigate," or "After you fail a skill test by 2 or more") do
  > not trigger at this time. These abilities are triggered during Step 6,
  > "Determine success/failure of skill test."

  "At this time" is Step 7. So the trigger is Step 6, before the clue moves —
  which is exactly what the engine does, and Obscuring Fog 01168 and Dr. Milan
  Christopher 01033 resolving before the discovery are correct and now citable.

  What the entry asked, and why it was open: the engine fires
  `TimingEvent::SkillTestResolved` at the ST.6→ST.7 boundary, explicitly *"before
  any ST.7 consequence"* (`state/game_state.rs:1551-1562`). The rules give ST.6
  (determine success/failure) and ST.7 (apply results) as separate steps, which
  supported the engine; but `glossary/After.md` defines "after" as *"immediately
  after the specified timing point or triggering condition has fully resolved"*,
  and it was arguable that "successfully investigating" is not fully resolved
  until the clue moves. Neither card's ruling file addresses it
  (`data/arkhamdb-faq/core/01033.md`, `.../01168.md` — both carry unrelated
  rulings only), which is why it took the FAQ's own section 1.7 to close.
- **ST.7's "order of his or her choice" over multiple results.** The rule is
  quoted in finding 6; the engine applies ST.7 in a fixed cursor order
  (`FireOnCommit → ApplyFollowUp → ApplyResultEffect → …`,
  `game_state.rs:1575+`). Whether the corpus can ever produce two *independent*
  results at ST.7 whose order is observable — as opposed to the current chain,
  where each step feeds the next — I could not establish without enumerating
  every ST.7 contributor across Chapter 1, which is beyond this pass. It is
  reported as part of finding 6 (no responder) rather than as an ordering
  finding of its own, because with a single decider the fixed order is only
  wrong if the outcomes differ.
- **Whether `EnemyAttacks` needs an `After` cell.** The engine opens that window
  in the `When` cell only (`emit.rs:198-206`), which is right for Dodge 01023.
  RR 3.3's *"Upon completion of dealing the attack (and all abilities triggered
  by the attack), exhaust the enemy"* implies an after-attack timing point
  exists; no in-corpus card listens for one, and whether it should be a distinct
  `TimingEvent` or the `After` cell of the existing one is a design question
  finding 3 would answer as a side effect. Not classified.
- **The "never adds" rule in window re-validation.** `withdraw_lapsed_candidates`
  deliberately never *adds* a candidate that entered play during an open window,
  justified in-tree as *"a card that entered play during the window was not in
  play when the triggering condition occurred"*. `glossary/Ability.md`'s *"Card
  abilities only interact with the game if the card bearing the ability is in
  play"* is consistent with that reading but does not state it, and
  `glossary/Lasting_Effects.md`'s parallel clause (*"Cards that enter play … after
  its establishment are not affected"*) is about lasting effects, not reaction
  windows. **What would settle it:** a ruling on a card put into play by a
  reaction resolving in the same window as its own trigger.
