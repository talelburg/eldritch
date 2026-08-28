# Phase 7 — The Gathering

## Status

**The engine foundation for the solo gate is complete.** Slice 1 (solo Roland
playing The Gathering end-to-end to Won + Lost, kickoff #216 / gate #245 /
PR #326) shipped, and so did every architectural arc the gate needed (see
**What shipped** below). What remains is a **rules-correctness cluster**, the
**browser capstone**, and the scenario's **optional content** — ordered into four
waves under **Remaining gate work**.

**Phase 7 is the 1-player solo rules-correctness gate.** Scope is deliberately
narrow — **1 player, 1 investigator, Standard**. Difficulty selection and solo-2
are **Future slices**; investigator breadth is its own milestone,
[phase 7.5](phase-7.5-investigator-breadth.md).

**Rechartered 2026-08-23** (`/grill-with-docs`), which changed three things this
doc previously said. The scenario's **optional content is now in the gate** —
Lita Chantler, the Parlor barrier and Resign were filed under *Future slices* and
are now #258's six children. The **playthrough is now an issue** (#769) and runs
twice, once as reconnaissance before the fixes and once as the run of record.
**Investigator breadth left**, to `phase-7.5-investigator-breadth`.

## Goal

A solo human, in the browser, picks an investigator, sets up The Gathering,
and plays it to a resolution — **rules-correct for 1-player Standard**.

## What shipped (retrospective)

The blow-by-blow lives in the closed issues, git history, and the
`docs/superpowers/specs/2026-06-*` design docs; only the load-bearing residue is
in **Architecture to build on**. In dependency order, the arcs that landed:

1. **Continuation-stack cleanup** (#345/#348/#380) — normalized the
   `InputResponse` channel (`PickSingle`/`PickMultiple`/`Confirm`/`Skip`), folded
   every `*_pending` side-channel onto continuation frames.
2. **#393 unified control-flow model** (C checkpoint) — every suspending/looping
   step is a continuation frame; the main loop's one rule is **handle the top
   frame**. `InvestigatorTurn` re-emits legal actions as `OptionId`s, so the stack
   is non-empty during play. (`AttackLoop` cursor-lift, PR #412.)
3. **Keystone — mid-action park/resume** (K1–K5b, #293/#379/#361/#378/#143/#44) —
   AoO, retaliate, activated-ability & non-fast card-play AoO, player attack-order,
   and interactive damage/horror soak distribution all park their triggering action
   on an `ActionResolution`/`AttackLoop`/`DealDamage` frame and resume under a
   re-validation gate. PR #424 reified the **effect evaluator as continuation
   frames** (retiring suspend-and-replay + `DecisionCursor` + `Continuation::Choice`).
4. **Skill-test player windows** — #374 (ST.1/ST.2 fast-play windows; Hyperawareness,
   Magnifying Glass) and #64 (after-resolution reaction window; Dr. Milan 01033).
5. **EmitEvent-frame arc A→D** (#435 umbrella, #433/#434/#431/#423) — event
   emission, windows, and the `when/at/after × forced/reaction` matrix are all
   `drive`-loop-dispatched frames. Final slice PR #446 deleted `apply_effect` /
   `drive_effect_to_base`; every effect site is now top-frame dispatched.
   The arc's two follow-on corrections shipped as **#569** (an emit queues its
   abilities rather than resolving them — [ADR 0003](../adr/0003-emitting-a-timing-point-queues-abilities.md))
   and **#566** (a latched resolution ends the scenario —
   [ADR 0004](../adr/0004-a-latched-resolution-cancels-opportunities-not-resolutions.md)).

## Remaining gate work

Four waves. Within a wave the issues are independent unless an arrow says
otherwise; the waves themselves are ordered.

### Wave 0 — reconnaissance

**#769, first half.** A rough browser playthrough *before* any of the remaining
fixes. The 2026-08-22 sweep found six solo-reachable defects by reading code; a
real run is a different instrument and finds a different class of thing — stalls,
prompts with no usable control, states a player cannot read. Whatever it turns up
gets an issue and joins wave 1. Doing this once at the end would mean every
discovery lands after the gate was thought closed.

**The run happened, and it paid.** It turned up **#786** (a Forced ability with an
unmet condition initiates anyway, so Cover Up 01007 prompts for its game-end trauma
with 0 clues on it) and **#787** (the skill-test result panel renders the chaos token
as `—` whenever a symbol token's ST.4 effect suspends and splits the event batch).
They join by kind rather than wholesale into wave 1: #786 is a rules defect and sits
in wave 1, #787 is a read-the-state defect and sits in wave 2. Both are the class the
2026-08-22 sweep's code-reading could not reach — one needed a *game* to end with the
clues already spent, the other needed a Ghoul standing next to the investigator when
the Tablet came out.

### Wave 1 — rules correctness

| Issue | Defect |
|---|---|
| #763 ✅ PR #789 | A zero-icon commit is accepted — commit is a free discard outlet |
| #764 ✅ PR #790 | A defeated active investigator's turn does not end |
| #786 ✅ PR #791 | The forced-trigger scan ignores eligibility, so a Forced ability with an unmet condition initiates and prompts |
| #697 ✅ PR #793 | Two of four phase-ends never emit, and there is no phase-start point at all |
| #664 ✅ PR #794 | A `ChooseOne` mode with no eligible target is still offered |
| #651 ✅ PR #796 | Hunter pathfinding routes *around* a movement block instead of being stopped by it |
| #797 ✅ PR #798 | Agenda 01107's Ghoul move reroutes around a movement block instead of being stopped by it |
| #682 ✅ PR #799 | An attack whose target leaves play mid-test resolves against difficulty 0 |
| #562 ❌ PR #800 | 01110's forced act-advance double-prompts (advance-flip slice 4) — not a defect; 01110 is terminal, so it prompts once |

### Wave 2 — the input surface

| Issue | Defect |
|---|---|
| #541 ✅ PR #801 | End turn, Gain resource, Draw and the Mythos draw live in a sticky `.action-bar`, so the board is not the input surface and every anchored `PickSingle` renders twice — closes **#206** |
| #787 ✅ PR #802 | The skill-test result panel renders the chaos token as `—` whenever a symbol token's ST.4 effect suspends and splits the event batch |
| #770 ✅ PR #803 | A terminal version mismatch surfaces as one status line under a live-looking board, indistinguishable from an engine stall |

### Wave 3 — optional content (#258's children)

Ordered: **#644 → #772 → #771 → #773 → #774 → #775**.

- **#644 — Resign semantics.** Unblocked: #695 shipped as #707/#708/#709/#735, and
  #696 already gave the engine `ActionDesignator::Resign` and its AoO exemption. What
  is left is elimination **by resignation** — `Status::Resigned` / `DefeatCause::
  Resigned` exist, are wired through elimination, and have never been constructed —
  plus `glossary/Winning_and_Losing.md`'s *"no resolution being reached"* ending. #696's
  reviewer note stands: tagging the Parlor 01115 is this issue's job.
- **#772 — an uncontrolled asset is an unreachable ability source, and *"she gains"*
  has no DSL.** Two gaps on one card pair. `glossary/Triggered_Abilities.md` bullet 2
  reaches Lita 01117; #708's implementation of that bullet walks the location, its
  attachments, co-located enemies and *other investigators'* threat areas, and she is
  none of those. She is also the **first source with a `CardInstanceId` that nobody
  controls**, so #708's `instance().is_none()` rejections do not cover her. Separately,
  neither 01115's Parley nor 01117's buffs are *printed* on the card that has them —
  each is granted under a mutually exclusive `While` predicate, and `card-dsl` has no
  conditional granting. Taken as a native hook on #368's precedent, with one caveat
  worth watching: #368's natives return `bool`, and a grant returns `Vec<Ability>` and
  must be consulted wherever abilities are enumerated. If that spreads further than a
  hook threads cleanly it is an **ADR**, not a `TODO`.
- **#771 — set-aside assets.** Set-aside is enemies-only (`set_aside_enemies` +
  `spawn_set_aside_enemy`), so act 01109b's *"Put the set-aside Lita Chantler into play
  in the Parlor"* has nowhere to go while its neighbour *"Spawn the set-aside Ghoul
  Priest in the Hallway"* works (#231).
- **#773 — Lita 01117's controlled-side grants.** A location-scoped `+1 [combat]` for
  *each* investigator there and a `+1` damage reaction on another investigator's
  successful `[[Monster]]` attack. Both collapse to one investigator in 1p; implement
  the printed predicate, not its degenerate case, or the test asserts nothing.
- **#774 — the Parlor movement barrier**, split out of #258 because it is **mandatory
  printed behaviour** that was inheriting optional content's deferral. Lands after
  #651 and inherits its reading of where a block applies rather than inventing a second.
  #651 has shipped, so that reading is now fixed: **the block is checked against the
  compelled step, never baked into the graph** — distances and shortest paths run on
  the full connection graph (`glossary/Nearest.md`: *"even if one or more of those
  connections are blocked by another card ability"*) and a blocked step is a non-move
  (`glossary/Hunter.md`). Two notes from that PR for whoever picks this up: the same
  clause appears on a **fixed**-destination mover in `glossary/Patrol.md`, so it does
  not depend on the target being a "nearest" one — which is why agenda 01107's forced
  Ghoul move became **#797**, shipped in PR #798. So there is now exactly one
  application site to inherit, not two: the enemy-side predicate is
  `enemy_can_enter_location`, and both `hunter_destinations` and
  `move_ghouls_toward_parlor` apply it to the compelled step. The graph-pruning
  pathfinding variants (`bfs_distance_with`, `shortest_first_steps_with`) are gone
  with the reading they served, so a new mover cannot reach for them by accident.
- **#775 — act 3's R1/R2 choice.** 01110b asks the lead investigator to choose the
  ending, and `the_gathering.rs:235` hardcodes `Resolution::Won { id: "R1" }`, so R2
  is unreachable. Not a new finding: the 2026-08-22 sweep had already split it into
  **#766** as phase-9 campaign work, and it moves here on the reading #766 itself
  offered — *"the 01110 choice can land earlier as a plain prompt if #75 is not yet
  there."* The **prompt** is in the gate because it is what makes both printed endings
  reachable; the **consequences** (trauma, campaign log, earning the Lita Chantler
  card) stay in #766, so `apply_resolution` grows a match arm per id and nothing more.

`[action]: **Parley.**` needs no new action type — #696 shipped the designator.
#231 and #257, named in #258 as neighbours, are both closed.

### Wave 4 — the gate-closer

**#769, second half.** The run of record: **Won** (R1 or R2), **Lost**, and
**Resigned**. Then the phase-doc commit and the milestone.

### The arcs behind it (retrospective)

The four numbered items below are the record of how the remaining work got its
shape — the shipped arcs, their rejected alternatives, and the `TODO(#NNN)`
promotion triggers they left behind. They are not a queue; the queue is the waves
above.

**1. `IntExpr` correctness cluster.** **DSL core + #300 + #426 ✅ shipped (PR #450).**
A shared `Quantity` vocabulary (`CluesAtControllerLocation`, `EngagedEnemies`,
`SkillTestFailedBy`) backs both `IntExpr::Count` (value) and `Condition::Compare`/
`CmpOp` (predicate, retiring `LocationHasClues`); `Effect::Deal.amount` +
`Effect::Fight.extra_damage` widened to `IntExpr` with `From`/`Into` builders
(literals untouched). **#426** — Grasping Hands 01162 / Rotting Remains 01163 deal
one `Count(SkillTestFailedBy)` instance (`ForEachPointFailed` deleted). **#300** —
Machete gets `+1` damage only against the sole engaged enemy.
**#449 ✅ shipped** — `Effect::Fight` now picks among the engaged enemies
(auto-binds 1, suspends for a `PickSingle` on 2+; `single_engaged_enemy` retired),
so an investigator swarmed by 2+ enemies can activate a weapon and Machete's `+0`
branch is reachable. **#451 ✅ shipped (PR #455)** — widened that candidate scope
engaged → any co-located enemy (`combat::fight_target_scope()` = `At(Here)`, shared
by the pre-cost gate and the target grounding so they can't drift), matching #401's
basic-action fix and Machete's FAQ (you *can* attack an Aloof / other-player-engaged
enemy; you just forfeit the sole-engaged damage bonus).
**#592 ✅ shipped (PR #610)** — that forfeit was aspirational until now: Machete's
condition was `Compare(EngagedEnemies, Eq, 1)`, which counts *your* engaged enemies
and never reads the target, so once #451 widened the scope the two questions came
apart and an unengaged co-located target still got `+1`. The clause is a conjunction
over the *chosen* enemy, and neither half is expressible declaratively (no `Condition`
combinator, no target-referencing `Condition`/`Quantity`, `IntExpr::Cond`'s branches
are `i8` so conditions can't nest) — so it rides a new `Condition::Native { tag }`,
the read-only mirror of `Effect::Native` dispatched via
`CardRegistry::native_condition_for`, exactly the single-consumer-native call item 2
below made for eligibility predicates. **#609** carries the promotion trigger (the
second card wanting a compound or target-referencing condition adds `Condition::All`
plus a target-referencing variant and re-expresses Machete through them), with
`TODO(#609)` markers at all four sites. The card predicate and the kernel's
`Quantity::EngagedEnemies` now share one `GameState::enemies_engaged_with` reading of
"engaged with you", since two copies drifting apart is what this bug was.
- **#118 — Roland's elder-sign ✅ shipped (PR #454).** `Trigger::ElderSign { modifier:
  IntExpr }` + an ST.4 firing path: the bonus rides the chaos-token `Modifier` total
  (sourced from the investigator card via `elder_sign_modifier` — **not** `Effect::Modify`).
  Folded in the **investigator-card bridge** (`Investigator.card_code` at seating +
  `ability_usage` + a `scan_investigator_card_reactions` source / `CandidateSource::
  Investigator`), which also fixes Roland's **reaction** firing from a *seated*
  investigator (previously only via test card-injection). **Bridge retired by #448 ✅
  shipped (PR #457)** — the investigator card is now a real `CardInPlay`
  (`Investigator.investigator_card`) holding health/sanity + harm + identity + usage, so
  `card_code` / `ability_usage` / the bespoke `scan_investigator_card_reactions` source all
  collapse into the uniform `controlled_card_instances()` scan; this also fully resolves
  #453's `card_code`-sentinel question (no field left to default) and made the web client a
  registry host (`cards::REGISTRY` installed at startup, since capacity now reads from
  metadata). **#453 ✅ shipped (PR #456)** removed the `#[serde(default)]` convention the new fields
  follow — the non-`Option` fields are now required on the wire (a stale payload errors
  rather than silently degrading `card_code` to the empty sentinel); the two `Option`
  fields (`pending_played_event`, `usage_limit`) stay implicitly optional because serde
  defaults a missing `Option` to `None` regardless, so #453's concern #2 for
  `pending_played_event` is only partially met (forcing it needs a custom deserializer,
  deferred). His signature is in the "done" criteria. *(`pending_played_event` has since
  been retired entirely — PR #606 moved the in-progress play onto its continuation frame;
  see [ADR-0002](../adr/0002-in-progress-play-lives-on-its-frame.md). #453's concern #2 now
  applies to `usage_limit` alone.)*

**2. #368 — trigger-level eligibility ✅ shipped (PR #472, also closes #470).**
The hardcoded scan-suppression stand-ins (Cover Up 01007 `card.clues == 0`; act
01109 round-end clue-threshold — the latter actually *missing* from the offer
scan post-#434, i.e. bug #470) are lifted into a per-ability **native eligibility
predicate** evaluated at reaction-scan time (RR p.2: an ability can't initiate if
its effect won't change game state). Resolved as a native hook (`Ability.
eligibility` tag → `CardRegistry::native_eligibility_for` →
`fn(&GameState, &EvalContext) -> bool`), **not** a declarative `Condition`: both
live consumers are single-consumer + heterogeneous, so declarative DSL vocab
would be speculative — promote to a `Condition` when a predicate recurs (Lone
Wolf 02188, Burned Ruins 02205). The Barrier's offer + resolve share one
`round_end_advance_affordable` helper so they can't drift. **Item 2 (capped
discovery count) moved to #471 ✅ shipped (PR #617)** — it became live once
Deduction 01039 was fixed to modify a single discovery's count (FAQ-confirmed)
rather than spawn a second discovery. Deduction now rides an `OnCommit`
accumulator (`Effect::DiscoverAdditionalClues` → `InFlightSkillTest.
bonus_clues_discovered`, the clue-side sibling of Vicious Blow 01025's
`bonus_attack_damage`), and the Investigate follow-up makes **one** discovery of
`1 + bonus` at `LocationTarget::TestedLocation` — a base-Investigate change, since
the PR deletes Deduction's own `TestedLocation` anchoring. `discover_clue` caps at
`min(count, location.clues)` **before** emitting the discovery condition (then named
`WouldDiscoverClues`; one condition, `DiscoverClues`, since #703), so Cover Up
01007's "discard that many" reads the real quantity; `perform_discovery` keeps its
own `min` as the shrinkage backstop, fixing the count at the moment of the would-be
discovery. The shape distinction is now glossary vocabulary (**Discovery** in
`CONTEXT.md`) — it is invisible in clue totals and cost a shipped bug. *(The
predicate is no longer evaluated at reaction-scan time
**only** — PR #608 re-runs the whole reaction scan at both prompt sites and once
more at initiation, so a sibling option that resolves first can withdraw a
candidate the scan had cleared. Scan-time filtering is now the optimisation;
initiation is the binding check. The equivalent gap on the **forced** side, where
`collect_forced_hits` applies the same gate at collect time, is #607.)*

**3. #695 — an investigator can only activate abilities on cards they control.**
`glossary/Triggered_Abilities.md` lists four sources an investigator may use a
triggered ability from, verbatim:

> - A card in play and under his or her control. This includes his or her investigator card.
> - A scenario card that is in play and at the same location as the investigator. This includes the location itself, encounter cards placed at that location, and all encounter cards in the threat area of any investigator at that location.
> - The current act or current agenda card.
> - Any card that explicitly allows the investigator to activate its ability.

The engine honoured the first only in part until **#707** (below). Activation resolved
its source against the acting investigator's own `cards_in_play`, so a location's, an enemy's, the act's and the
agenda's abilities cannot be initiated and never reach the turn menu — the Parlor
01115's Resign among them, though that card is separately deferred as optional content
(#258) and has nowhere to resolve to until #644 gives Resign its semantics. The
basic weaknesses Haunted 01098 / Psychosis 01099 / Hypochondria 01100 are the sharper
case: each prints an `[action] [action]` discard as its *only* exit, so drawn into any
scenario they are permanent. The forced and reaction scans already
walk a wider source set, so the disagreement is between two paths inside the engine,
not a missing capability. The fix promotes the source descriptor those paths use into
the activation action and puts the four bullets into one reachability predicate that
both the validator and the turn-menu enumerator consult; #695's spec comment is the
definition of record, and it is a deliberate wire break with no migration.

**#706 ✅ shipped (PR #732)** is the prefactor that arc needs first. Cost payment
cached the source card's *position* in the controller's `cards_in_play` at validation
and indexed back into the collection to exhaust it or spend its uses. The code
documented the hazard in place: a cost that removes its own source mid-payment
invalidates the index the next source-referencing cost would use, safe only because no
shipped card pairs those two in that order and because `reject_incompatible_costs`
doesn't know about uses-depletion. Costs now re-resolve the source by its
`CardInstanceId` through a single `require_source_in_play` gate, and a source that has
left play produces a rejection rather than silently addressing whichever card slid into
the vacated slot — verified by a hand-rolled-registry test that, before the fix,
exhausted the *neighbour*. Ordered first because every remaining ticket under #695 adds
a source with no position in any collection (a location, an enemy, the act, the
agenda), each of which would otherwise have to work around the index separately.
The rejection lands after the earlier costs have already mutated, which the handler
contract licenses: `apply_via` snapshot-restores state, events and the RNG position on
`Rejected` (#161). No ADR — the two readings a future author would want (why the guard
on `Cost::DiscardSelf` is unreachable today yet kept, and why no source-descriptor type
was minted early) are both `TODO`-adjacent comments at the sites that hold them.

**#707 ✅ shipped (PR #733)** is the first of the four bullets and the vocabulary
change the other three attach to. The activation action stops naming a bare
`CardInstanceId` and names an **ability source** (`CONTEXT.md`), and
`engine::ability_source` answers reachability once for the validator, the turn-menu
enumerator and the fast-window enumerator alike — `resolve` is a lookup in
`reachable_sources` rather than a second reading of the rules, so the menu and
handler-acceptance cannot drift apart about which sources exist. The control bullet is
now honoured in full: an ability on your own investigator card or on a card in your own
threat area is offered and activatable, and another investigator's card is not.
Widening what is *addressable* widened nothing else — the initiation sequence's checks
run unchanged behind it. It is the deliberate wire break: persisted games are discarded,
no version field is added (#581), `GameSession::load` fails loudly on a log it cannot
replay, and `get_or_load_room` no longer reports an unreplayable game as a nonexistent
one. Design and the three rejected alternatives:
[ADR 0010](../adr/0010-an-activation-names-an-ability-source.md). #709 (act and
agenda) attached to the same predicate.

**#708 ✅ shipped (PR #734)** is the second bullet — *"a scenario card that is in
play and at the same location as the investigator"* — and the one that unblocks the
shipping scenario. `reachable_sources` walks co-location after control: the location
itself, its attachments, each enemy standing on it (with its attachments), then the
threat areas of the *other* investigators there. That last is not controller-scoped —
the bullet says *"any investigator at that location"*, and Haunted 01098's ruling
(<https://arkhamdb.com/card/01098>) says the same outright — so #707's own-threat-area
half and this one are different rules, not the same rule twice. `AbilitySource` gains
`Location(LocationId)` and `Enemy(EnemyId)`, the latter forced by the corpus: the
Midnight Masks Parley cultists (Herman Collins 01138, Peter Warren 01139, Victoria
Devereux 01140) and Mob Enforcer 01101 all print their `[action]` on an enemy. The
Parlor 01115's Resign is now reachable in the engine's sense; it still has nowhere to
resolve to until #644.

Two rejections ship with it, both because addressing a source with no `CardInstanceId`
is new. A **usage limit** on one has nowhere to record a use — usage state is
`CardInPlay::ability_usage` — so it rejects naming **#699**, which builds the
capability Base of the Hill 02282 and Ten-Acre Meadow 02246 will need; this is the
change that first put `bump_usage_counter`'s `unreachable!` behind player input, and a
panic reachable from player input must not ship. A **source-referencing cost**
(`Exhaust` / `SpendUses` / `DiscardSelf`) on one rejects at validation for the same
reason, so the menu never offers it; no corpus card prints one. `Event::AbilityActivated`
now names the source rather than an instance — recorded as an amendment on
[ADR 0010](../adr/0010-an-activation-names-an-ability-source.md), whose prediction that
#709 would force the field to become optional was overtaken. Zero-action abilities on
the new sources work through the shared predicate; **#710 ✅ shipped (PR #737)** proved
it.

**#709 ✅ shipped (PR #736)** is the third bullet — *"The current act or current
agenda card."* — and the one gated on **nothing**: not control, not co-location.
Disrupting the Ritual 01148's ruling (<https://arkhamdb.com/card/01148>) says so
about a printed card, verbatim: *"Your investigator doesn't need to be at the
Ritual Site in order to activate the ability of this act card."* `AbilitySource`
gains `Act` and `Agenda`, and neither carries an id — there is exactly one
current act, and it is `act_deck[act_index]`, so the descriptor names the
**cursor** rather than a position. That is what makes "an act that is no longer
current is unreachable" true by construction rather than by check, and what makes
an empty deck or a cursor past its end simply reach nothing. `reachable_sources`
appends both *after* the co-location pass, which moved into its own
`colocated_sources`: #708's early return for an investigator at no location would
otherwise have swallowed a bullet that does not depend on standing anywhere.
Everything downstream was already written against the descriptor — both #708
rejections key off `source.instance()` being `None`, so they cover the board
cards unchanged, and `TurnAction::target`'s deliberately-exhaustive match made
the two new anchors a compile error rather than a silent `Global`, which is the
property ADR 0010 wrote that match to hold open.

**#735 ✅ shipped (PR #739)** is the convergence #709 deferred: the forced and
reaction scans now say where an ability comes from in the *same* vocabulary an
activation does. `CandidateSource` was carrying a single `Board` kind for the act,
the agenda **and** an attacking enemy's own ability, so `candidate_anchor` worked
out which of the three a candidate was by comparing its code against the current
act and agenda at read time — and fell through to `OptionTarget::Global` when
neither matched, which is exactly the enemy case, leaving Silver Twilight Acolyte
01102's forced-doom prompt anchored to nothing. The enum became
`{ Ability(AbilitySource), Hand }`: a **wrapper, not a merge**, because a Fast
event played from hand is not an ability source in the rules' sense — none of
`glossary/Triggered_Abilities.md`'s four bullets names a card in hand — and
folding it in would have put a permanently-unreachable kind into the enum
`reachable_sources` enumerates. The two source→anchor maps the turn menu and the
candidate path each carried collapsed into one `From<AbilitySource> for
OptionTarget`, and `Global` stopped being any candidate's anchor. The
`SourceGone` probe became `ability_source::source_card`, placed beside
`reachable_sources` and documented as the question it deliberately is **not**:
existence, not reachability, because a forced ability is not restricted to the
sources its controller could legally *use*. Two lapse *attributions* changed with
it (an attacking enemy's ability, and one minted from an attachment) and no
resolution did — `withdraw_lapsed_candidates` decides what survives a re-scan,
and it is untouched. Recorded as an amendment to
[ADR 0010](../adr/0010-an-activation-names-an-ability-source.md).

**#710 ✅ shipped (PR #737)** closes the cluster, and it is the one ticket that was
**confirmation rather than construction** — no production behaviour changed. The
rules bullets are written once for `[free]`, `[reaction]` and `[action]` together, so
the widening #707/#708/#709 shipped reaches player windows by construction:
`enumerate_fast_plays` already consults `engine::ability_source`, the same predicate
`check_activate_ability` and the turn-menu enumerator consult. A zero-action ability
— the `[free]` icon, *"a free triggered ability that does not cost an action and may
be used during any player window"*, never *"fast ability"* (`CONTEXT.md`) — on a
location, an enemy at your location, a co-located threat area, the act or the agenda
is therefore offered in a player window on exactly the terms an action-costed one is
offered in the turn menu.

What shipped is the coverage that makes that inheritance a **checked property**
rather than a reading of the call graph, at the two seams #695 named: a real
engine-opened player window's own option list (whether an ability is *offered*) and
the apply entry point (whether a named investigator's submission is accepted). It is
verified by mutation rather than by passing — dropping the co-location bullet from
`reachable_sources` fails five of the new tests, and narrowing the player-window
enumerator to in-play instances alone fails five. The real-corpus half covers all
three cards that print a zero-action ability today (Beat Cop 01018, Hyperawareness
01034, Physical Training 01017), and the boards sit in the **Mythos** phase
deliberately: `check_activate_ability`'s gate is *"active during Investigation **or**
an open permissive window"*, so an Investigation-phase board would satisfy the first
disjunct and mask the rule under test.

**One negative stays at the apply seam, and that is a property of the option shape.**
An option carries an `OptionTarget`, which names the *source* and not the investigator
it was enumerated for, while `enumerate_fast_plays` walks every investigator — so
"offered" is directly assertable at the option list and "not offered *to this
investigator*" is not. Proving the per-investigator negatives there would mean making
`enumerate_fast_plays` public for a test; they are asserted at the apply seam instead,
where the submission names its actor. The day `OptionTarget` learns who an option is
for — #381's eligibility half is the likely occasion — that split can close.

**#696 ✅ shipped (PR #738)** is the bug the widening exposed, and the one place the
arc changed a *rule* rather than what is addressable. The attack-of-opportunity
exemption is written against the **bold action designator** a card prints — *"an
action other than to **fight**, to **evade**, or to activate a **parley** or
**resign** ability"* — and `provokes_aoo` inferred it from the effect root instead,
exempting `Effect::Fight` because every corpus weapon happens to be rooted in one.
Its own doc comment conceded the premise (*"There is no `Effect::Evade` and no
activated parley/resign card in scope"*), which the Parlor 01115's `[action]
**Resign.**` falsifies in the shipping scenario the moment #708 makes a location's
abilities reachable. `Trigger::Activated` now carries an `ActionDesignator`
(`CONTEXT.md`), and the corpus declares what it prints: the four weapons are
**Fight**, Flashlight 01087 is **Investigate** — a designator the exempt list does
*not* name, so it provokes exactly as the basic investigate action does. Declared
rather than derived because the rules quote the designator and Frozen in Fear
01164's ruling quotes it independently; **Parley** and **Resign** have no effect
shape for an effect-root match to find, and a `Seq`-wrapped Fight has the wrong one.
The same declaration retires the second effect-root sniff — the pre-cost *"a Fight
ability needs a co-located enemy"* check — which widens it to a `Seq`-wrapped weapon
and narrows it away from an undesignated one, a shape no corpus card is in and which
now rejects in the evaluator with a message that says so. No ADR: the choice is
reversible and `ActionDesignator`'s own rustdoc carries the reasoning. **Scope was
the designator, not the semantics** — Resign's elimination-by-resignation is still
#644, so the Parlor stays unimplemented (an ability whose effect cannot change the
game state is refused at initiation, so there is nothing to give it yet) and the
acceptance case rides a synthetic Resign-designated location through the real
`AbilitySource::Location` path. Nothing forces #644 to tag 01115 when it lands;
that guard is its reviewer's.

**4. Browser capstone — the gate-closer.** Positioned last so it designs against
the now-stable set of input shapes:
- **#447 — 2b: typed `PlayerAction` elimination ✅ shipped (PR #460).** Open-turn
  gameplay now flows through `ResolveInput(PickSingle(OptionId))` against an
  open-turn `AwaitingInput` action menu (the engine surfaces `legal_actions` as
  the menu; `InvestigatorTurn::awaits_input` → true). The 11 typed gameplay
  variants are gone — the wire surface is `StartScenario` + `ResolveInput`; an
  internal `TurnAction` id→action map (`dispatch_turn_action`) is the sole
  gameplay path, re-enumerated at resolve (not cached). The test-only
  `PerformSkillTest` was removed too (→ `test_support::perform_skill_test*`). The
  web client lost its bespoke open-turn controls — gameplay renders through
  `AwaitingInputView`'s `PickSingle` option list (flat labels; richer per-option
  metadata beyond `label` was explicitly out of #205's scope — a future enrichment).
  **Split out:** **#458** (deterministic resume-token, §F — `ResumeToken(0)` stays
  for now) and **#459 ✅ shipped (PR #461)** — see the picker bullet below.
- **#205 — structured input rendering ✅ shipped (PR #462).** `InputRequest` gained an
  `InputKind { PickSingle, PickMultiple, Confirm }` discriminator (variant names mirror
  `InputResponse` 1:1) plus an orthogonal `skippable: bool`; the ambiguous
  `prompt`/`choice` constructors were replaced by `pick_single`/`pick_multiple`/`confirm`
  + a chainable `.skippable()`, and every engine prompt site declares its kind.
  `AwaitingInputView` switches on `kind` (Confirm → a Confirm button — the gate fix) and
  renders a Skip button whenever `skippable` (the reaction/fast-window decline path, which
  previously had no control). Richer per-option metadata beyond `label` is **not** in
  scope — this PR is the discriminator only.
- **Investigator/scenario picker ✅ shipped (PR #461 — #459 + #224).** Seating moved
  out of `PlayerAction::StartScenario` into the non-logged engine fn `seat_and_open`;
  the server seats at game-creation and persists the seated, mulligan-pending state as
  the seed (`CreateGameRequest` carries the roster; a bad roster → **422**, no orphan
  row). `PlayerAction` collapsed to a single `ResolveInput` variant — the **action log
  is `ResolveInput`-only**, and the setup shuffle is baked into the frozen seed
  `RngState` (replay no longer re-runs setup RNG). Migration `0002` persists the seed's
  `EngineOutcome` so `load` restores an `AwaitingInput` seed from an empty log. The
  browser **picker** (`picker.rs`) collects an investigator + scenario and drives
  creation (`ConnStatus::AwaitingRoster`; Roland seats with a placeholder default deck
  of implemented cards); the old Start-scenario button (`controls.rs`/`legality.rs`) is
  deleted. **#224** folded in: a non-empty roster is mandatory (single seating path),
  and the ~37 `StartScenario` test sites migrated to `seat_and_open` (game-core's own
  tests seat synthetic `TEST_INV` via the test registry, preserving crate layering).
- **End-to-end browser playthrough** of The Gathering to a resolution — now **#769**,
  and now bracketing the rest of the gate rather than trailing it (waves 0 and 4).
  The Mythos-encounter-draw stall is **resolved** by #205's
  `InputKind` discriminator (PR #462): the draw now renders a Confirm button and resolves,
  and skippable windows render a Skip control. The picker → seating → mulligan →
  investigation → Mythos flow all works in-browser (PR #461). What remains is *exercising*
  the full playthrough to a resolution end-to-end — no known engine/client blocker.

  > **Dev-loop note (not a gate blocker):** the wire-format change in #205 means a stale
  > server binary + freshly-rebuilt client silently hangs at `<no game>` — the client drops
  > the un-parseable old-shape `Hello` (`transport.rs` `if let Ok(msg)`), leaving `game:
  > None`. Restart both processes after a wire change. Surfacing a visible
  > version-mismatch status instead of the silent drop is a possible future hardening
  > (out of #205 scope).
- **Visual card rendering (#519, PR #520 — display-only).** Hand cards now render as
  faithful mini-card rectangles (cost / name / traits / translated text / slots / skill
  icons, class-coloured) via a reusable `Card` component (`crates/web/src/card.rs`),
  replacing the flat name list. First slice of a **zone-by-zone** rework — in-play, threat,
  locations, enemies, and act/agenda stay text until their own slices. ArkhamDB text markup
  (`[symbol]` / `[[trait]]` / `<b>`) is translated to text **chips** by a pure
  `parse_card_text` (split from rendering so it's native-testable); unknown tokens render
  verbatim *with brackets* to surface unmapped markup. The ArkhamDB **icon font is deferred**
  on provenance grounds (vendored-asset discipline, cf. the P6.4 leptos-use deferral), with
  the chip→glyph seam built so it drops in without restructuring — revisit near a future
  merge. Spec/plan: `docs/superpowers/specs/2026-06-29-web-card-rendering-design.md`,
  `docs/superpowers/plans/2026-06-29-web-card-rendering-hand.md`.
  - **Slice 2 — in-play assets (#521, PR #522).** `Card` gained an optional
    `in_play: Option<CardInPlay>` prop (extend, don't fork): the printed face minus the
    cost corner, plus live per-instance state — exhausted (`card--exhausted` dim + badge),
    uses chips, and soak chips (`dmg`/`hor` vs the asset's health/sanity) built by a pure
    `live_state_chips`. The board's in-play list is now a `.card-row` of `Card`s. Still
    display-only; threat area, the spatial map (locations/enemies), and act/agenda remain
    later slices. Spec/plan: `docs/superpowers/specs/2026-06-30-in-play-card-rendering-design.md`,
    `docs/superpowers/plans/2026-06-30-in-play-card-rendering.md`.
  - **Slice 3 — engaged enemies (#523, PR #524).** A **dedicated `EnemyCard`** component
    (fork, not `Card`): enemies are a different data source — the `Enemy` *state struct*
    carries stats + live state, vs `Card`'s `code`→registry + `CardInPlay`. Renders combat
    stats (fight/evade/health/attack), keyword chips (Hunter/Retaliate/Victory), traits,
    ability text (looked up by code via the registry, reusing `parse_card_text` +
    the now-`pub(crate)` `render_segments`), and the `card--exhausted` dim + badge; red
    `card--enemy` border. Engaged enemies render as a `.card-row` in the threat area; the
    map's enemy tokens and threat-area treacheries stay later slices, and `prey` display is
    deferred (moot in 1p). Spec/plan:
    `docs/superpowers/specs/2026-06-30-enemy-card-rendering-design.md`,
    `docs/superpowers/plans/2026-06-30-enemy-card-rendering.md`.
  - **Slice 4 — location cards / the map (#527, PR #528).** The spatial map's nodes now
    render as location cards (name, `shroud` chip, `clues`, traits, ability text, `Victory`
    chip — traits/text/victory from the corpus by `loc.code`); unrevealed nodes withhold
    that info *structurally* (`loc.revealed.then(...)`). The grid is **normalized to the
    origin** (`layout_positions` subtracts min col/row) so a departed location (the Study,
    post-Act-1) leaves no dead column; connection lines + `map_extent` derive from the same
    positions. Unengaged-enemy tokens (deferred from slice 3) render in the nodes. Two
    layout fixes shipped alongside (PRs #526): `.board-main` stacks the map above the
    investigators panel (the map's absolutely-positioned nodes overflowed a shrunk flex
    row), and a sticky `.action-bar` keeps the controls reachable on the now-tall board.
    Registry-discipline note for future map tests: metadata-dependent rendering is tested
    in its own binary (`tests/location_card.rs`, real `cards::REGISTRY`, mounts
    `location_map` directly) since registry install is first-wins per process — `tests/map.rs`
    keeps the synthetic registry. Spec/plan:
    `docs/superpowers/specs/2026-06-30-location-card-rendering-design.md`,
    `docs/superpowers/plans/2026-06-30-location-card-rendering.md`. Interior-gap collapse,
    full cards inside nodes, and clickable locations stay out of scope.
  - **Slice 5 — threat-area treacheries (#529, PR #530).** Threat-area treacheries (Cover
    Up, Frozen in Fear) render via the **existing `Card` generic (`None`) arm** (no new
    component — a treachery is a `CardInPlay`, exactly `Card`'s model): name/traits/text/
    weakness + a clues-on-card chip (Cover Up's 3 clues). `live_state_chips` gained a
    `clues N` chip and the generic arm gained a `card-live` footer (exhausted dim/badge
    stays Asset-only — no in-scope non-asset exhausts). Treacheries render in the threat
    `.card-row` alongside the engaged-enemy cards; dead `.threat ul` removed. **This
    completes the display-only card coverage** for every zone (hand, in-play, enemies,
    locations, act/agenda terse-only, threat area). Remaining web work: act/agenda cards
    (terse phase-bar today) and the **interactivity pass** (cards/locations/enemies grow
    their own action buttons; retire the sticky `.action-bar`). Spec/plan:
    `docs/superpowers/specs/2026-06-30-treachery-card-rendering-design.md`,
    `docs/superpowers/plans/2026-06-30-treachery-card-rendering.md`.
  - **Slice 6 — act/agenda cards + turn tracker + collapsible log (#532, PR #533).** A
    three-column layout. **Act/Agenda render as cards** atop the board (`act_agenda.rs`, a
    `location_map`-style pure fn in `BoardView` — act shows `clues to advance: N` since the
    act has no running clue counter; agenda shows real `doom d/N`). A **right-hand
    `TurnTrackerView`** outlines the round's four phases with their RR sub-steps + structural
    player windows (the `ROUND` const is transcribed from RR Appendix II pp. 23-25 — step
    labels, loop tails elided — and cited in the module doc; reviewer-verified against the
    pinned PDF), highlighting the current phase. The **left event log is collapsible**. The
    `phase_bar` is **retired** (phase/round → tracker, act/agenda → cards). This finishes
    the display-only card/layout pass for every zone. The remaining web work is the
    **interactivity pass** (next bullet). Spec/plan:
    `docs/superpowers/specs/2026-06-30-act-agenda-and-sidebars-design.md`,
    `docs/superpowers/plans/2026-06-30-act-agenda-and-sidebars.md`.
  - **Interactivity pass (#206 umbrella; slices S0–S6 = #535–#541) — complete, closed by S6/PR #801.**
    Retires the flat `.action-bar`: actionable board entities glow and open a **context menu** of their legal
    actions; multi-select (mulligan/commit/discard) is click-to-select on the hand; windows /
    soak / effect choices resolve on their source cards; a slim prompt banner carries prompt
    text + Confirm/Pass. Engine-authoritative — each option the board offers *is* an option the
    engine enumerated as legal, so the board can never surface an action the server rejects (no
    client-side legality re-computation; the drift #206 warned of is structurally impossible).
    Design (whole-model umbrella): `docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md`.
    - **S0 — `OptionTarget` anchor on `ChoiceOption` (#535, PR #542).** Each wire `ChoiceOption`
      gains a structured `OptionTarget` (`Global` / `Location` / `Enemy` / `HandCard` /
      `CardInstance` / `Act`); `turn_menu` derives real anchors from a new `TurnAction::target`,
      every other option-builder emits `Global` for now. (**`Global` was removed by S6** — see
      that entry: un-anchored became `None`, and the field became `Option<OptionTarget>`.) `label` stays the full engine-authored
      string. Required wire field (#453 precedent). Engine + protocol only — no web behavior
      change (the bar still reads `label`). Plan:
      `docs/superpowers/plans/2026-07-01-interactivity-s0-optiontarget.md`.
    - **S1 — web plumbing + location context menus (#536, PR #543).** The shared routing seam:
      a `web::interaction` module (pure `pending_options` / `options_for` + a `PendingOptions`
      context signal, native-tested) and a wasm-only `ContextMenu` component (backdrop + a
      button per option; a click submits `ResolveInput(PickSingle)` and closes). Map nodes glow
      (`.map-location.actionable`) and open their menu; the flat action bar is untouched (bar
      keeps everything until S6), so S1 is purely additive. Per-entity placement, shared
      component; `ContextMenu` is wasm-gated (submits via the wasm-only `OutboundTx`) while the
      glow/open/`on:click` stay non-gated so the node compiles on host. Plan:
      `docs/superpowers/plans/2026-07-01-interactivity-s1-location-menus.md`.
    - **S2 — enemy menus + fixed-at-cursor (#537, PR #544).** `EnemyCard` glows and opens a
      Fight/Evade context menu (`options_for(Enemy(id))`). The shared `ContextMenu` moved to
      **`position: fixed` at the cursor** — `open` is now `RwSignal<Option<(i32,i32)>>` and a
      wasm-only `interaction::menu_layer` (a `.menu-hit` coord-capture layer + the menu) DRYs
      the trigger; S1's map node migrated to it. This **resolves S1's `overflow` clipping**
      (fixed escapes overflow) — but not `z-index` *stacking*: `.map-location` sets `z-index:1`,
      so `.map-location.actionable` gets `z-index:20` to float its menu above the sticky
      `.action-bar` (cards set no `z-index`, so theirs escape to root). **Deferred:** map-token
      (co-located/unengaged) enemy menus — rare in 1p (enemies auto-engage). Plan:
      `docs/superpowers/plans/2026-07-01-interactivity-s2-enemy-menus.md`.
    - **S3 — hand Play menu + multi-select + prompt banner (#538, PR #545).** A `HandCardView`
      wrapper (keeps `Card` display-only) gives a playable hand card a "Play" `menu_layer`, and —
      when a `PickMultiple` is live — turns hand cards into click-to-select (`.hand-slot.selected`
      ring). Introduces the deferred **prompt banner** (`prompt_banner.rs`, bottom-fixed): for a
      `PickMultiple` it renders prompt text + Confirm (submits the selection) + Pass (Skip). New
      `MultiSelect` context ({`active` derived from the outcome, `selected` set}) + a pure
      `is_multi_select`. **`input.rs`'s `PickMultiple` arm is removed** (the board hand + banner
      replace it; bar keeps `PickSingle`/`Confirm`/`Skip`; `tests/input.rs` deleted) — the agreed
      deviation from "bar keeps everything", since two selection UIs would collide. Selection
      click is non-gated (no coords). Plan:
      `docs/superpowers/plans/2026-07-01-interactivity-s3-hand-and-multiselect.md`.
    - **S4 — in-play/investigator card menus + reaction-window triggers (#539, PR #546).** The
      first engine change since S0: `build_resolution_options` anchors reaction candidates by
      `CandidateSource` (`InPlay`→`CardInstance`, `Hand`→a new `OptionTarget::HandCardByCode` — every
      copy of a Fast reaction event, so `OptionTarget` drops `Copy`, `Board`→`Global`); `drive_fast_window`
      reuses `TurnAction::target`. The anchor is **display-only** (the resolve path indexes
      `candidates[i]` by the echoed `OptionId`, never the anchor). Web: `InPlayCardView` wraps in-play,
      threat, **and the investigator card** (so Roland's signature reaction glows — a review catch);
      `HandCardView` dual-matches via `options_for_hand_card`. `PromptBanner` extended to skippable
      windows (prompt + Pass); `input.rs`'s Skip removed (the bar keeps window *options* so `Board`/
      `Global` stays reachable until S6). Plan:
      `docs/superpowers/plans/2026-07-02-interactivity-s4-in-play-and-window-triggers.md`.
    - **Round-end-advance banner fix (#549, PR #550).** An S4 skippable-window artifact: the bottom
      `PromptBanner` (`z-index: 25`, fixed) covers the sticky `.action-bar` (`z-index: 10`), hiding a
      `Board`/`Global` window option that lived *only* in the bar — the **round-end act-advance
      reaction**. The banner now renders a skippable window's `PickSingle` *options* as buttons (not
      just its Pass), so such options have a visible home. A load-bearing early piece of S6 (bar
      retirement then relies on the banner as the catch-all for un-anchored options); ordering is
      #550 → S5 → S6.
    - **S5 — act-advance menu + interactive soak + effect choices (#540, PR #552).** Re-anchors the
      last three `Global` prompt families onto their board entities. Engine: soak options →
      `CardInstance` (a soak-local `soak_options` builder in `combat.rs`; the shared
      `hunters::candidate_options` and its five other callers are untouched); effect `ChooseOne` →
      `Enemy`/`Location` (a `target` closure threaded through `resolve_grounded_choice` into a new
      `choice::awaiting_choice_anchored`); the round-end act-advance reaction → `OptionTarget::Act`
      (`build_resolution_options` maps a `CandidateSource::Board` candidate whose code is the current
      act, via `current_act_code`), so open-turn *and* round-end advance share the act-card home under
      one matcher. Web: the act renders as a glow-capable `ActCard` (mirrors `EnemyCard`, the only new
      component); soak/choice glow for free via the existing `InPlayCardView` (`CardInstance`) and
      `EnemyCard`/`location_map` (`Enemy`/`Location`) matchers. The `PromptBanner` now filters its
      option buttons to `Global` only (anchored options have card homes) — a load-bearing early piece
      of S6's bar retirement. Anchors stay **display-only** (resolve indexes `candidates[i]` by the
      echoed `OptionId`, never the anchor). **Deferred (still `Global`):** #492 (surfacing
      single-option soak/attack-order auto-binds — a surfacing-gate change, its own PR),
      investigator-choice (`ground_investigator_choice`), agenda `Board` reactions (no
      `OptionTarget::Agenda`), and `step_choose_one` effect-branch choices (no board entity). Plan:
      `docs/superpowers/plans/2026-07-15-interactivity-s5-act-soak-effect-choices.md`.
    - **Forced-effect card anchor (#553, PR #554) — S5 follow-up.** The forced paths still emitting
      `Global`: a single interactive forced-acknowledge (`drive_acknowledge_forced`) now anchors its
      "Resolve" to the source card (the 2+ ordered run already anchored via `build_resolution_options`).
      `Continuation::AcknowledgeForced` carries the `ResolutionCandidate`; the `CandidateSource →
      OptionTarget` mapping is now a shared `candidate_anchor` helper (extracted from
      `build_resolution_options`, no drift). Engine-only — in-play sources glow via the existing
      `InPlayCardView` matcher. The same PR then closes the **location** gap: a location's own forced
      ability (the Attic 01113's on-enter horror) had no `CardInstanceId`, so the `EnteredLocation` scan
      collapsed it to `Board` → `Global`. Added `CandidateSource::Location(LocationId)` and widened
      `push_matching`'s source param from `Option<CardInstanceId>` to `CandidateSource` (each of 14 call
      sites states its origin); `candidate_anchor` maps `Location → OptionTarget::Location`, so both the
      ack path and the ordered run glow the map node (S1's existing matcher). Plan:
      `docs/superpowers/plans/2026-07-16-forced-effect-card-anchor.md`.
    - **Agenda forced anchor (#556, PR #557) — act↔agenda parity.** The agenda was the last board
      card rendered display-only while the act (`ActCard`) glowed beside it. Added `OptionTarget::Agenda`
      + `current_agenda_code` + a `candidate_anchor` agenda arm (`Board` code == current agenda →
      `Agenda`), so an agenda-sourced forced ack (What's Going On?! 01105's on-advance reverse) glows the
      agenda and offers "Resolve" there — single-hit ack and the ordered run both anchor. Web: an
      interactive `AgendaCard` mirrors `ActCard`. Timing verified: the `AgendaAdvanced` forced fires at
      `FireReverse`, before `Finalize` bumps `agenda_index`, so the equality anchor holds for the advance.
      The effect-internal `ChooseOne` on such agendas (01105's discard-vs-horror) stays `Global` — the
      general effect-source-anchor machinery is deferred to #555. Plan:
      `docs/superpowers/plans/2026-07-16-agenda-forced-anchor.md`.
    - **Advance-flip slice 1 — reverse-side ingestion (#558, PR #559).** First of three slices turning
      an act/agenda advance into an on-card flip → resolve. This slice is pure data: `CardMetadata` gains
      `back_name`/`back_text` (verbatim ArkhamDB reverse side, generic across double-sided cards), the
      pipeline maps them, and the corpus regenerates (agenda 01105 → "A Lapse in Time"). Not user-visible
      alone. Design/plan: `docs/superpowers/specs/2026-07-16-advance-flip-design.md`,
      `docs/superpowers/plans/2026-07-16-advance-flip.md`.
    - **Advance-flip slice 2 — the on-card flip pick (#558, PR #560).** `Continuation::AdvanceReverse`
      gains an `AdvanceTrigger { Forced, Deliberate }`: a **forced** advance (agenda doom; act 01110 on
      Ghoul-Priest defeat) in interactive mode suspends its `AwaitAck` step with a one-option on-card
      `PickSingle` anchored to `OptionTarget::Act`/`Agenda` (the flip pick, replacing the anchorless
      `Confirm`); a **deliberate** advance (the `AdvanceAct` action / round-end objective) skips the ack —
      the action *was* the flip. `resume` accepts `PickSingle(0)`. The split key is forced-vs-chosen, not
      act-vs-agenda. Display-only anchor (resolve validates only the echoed `OptionId`). Engine-only —
      the client renders whatever it does for an anchored `PickSingle` until slice 3 draws the actual
      face flip.
    - **Advance-flip slice 3 — render the flip (#558, PR #561), closes #558.** The `AdvanceReverse`
      frame's `step` drives which face `ActCard`/`AgendaCard` render: front while `AwaitAck`, the reverse
      (`back_name`/`back_text`, tagged `card--reverse`) from `FireReverse`/`Finalize` on — via a
      `deck_face(game, deck)` pure fn. The glow/menu still come from `options_for(OptionTarget::Act/Agenda)`
      unchanged. **Advance-flip is shipped (slices 1–3):** a forced advance flips the card on-screen to its
      1b face showing the effect, then resolves there. Slice 4 (01110 `#466` suppression — a forced
      ability whose sole effect is an advance stacks a redundant `#466` ack over the flip's `AwaitAck`)
      was deferred to #562 and **closed unfixed** — 01110 is terminal, so its advance latches the
      resolution instead of building an `AdvanceReverse` frame, and there is no second prompt to
      suppress. The advance pick double-rendered on-card *and* in the flat input bar (every anchored
      `PickSingle` did), which S6/#541 reconciled by deleting the bar.
    - **S6 — globals + bar retirement (#541, PR #801), the closer; closes #206.** Homes for the
      three open-turn actions + an encounter-deck element for the draw `Confirm`, then
      **delete `.action-bar`** (folding picker + skill-test-result into their own surfaces).
      Scope grew to `engine` + `ui` during grilling: the three actions were not global but
      **unnamed**, and the draw `Confirm` was byte-identical to the #478 acknowledge pause, so
      neither could be homed client-side without label-sniffing. `OptionTarget::Global` is
      **removed** (un-anchored is `None`) and `InputRequest` carries an anchor of its own.
      [ADR 0011](../adr/0011-the-engine-names-the-surface-a-prompt-renders-on.md).
    - **Investigator panel rework (#547, PR #548) — display-only.** The investigator card is the home
      for the character's live state: skills (W/I/C/A) + hp/san folded onto the card, actions (pips)/
      resources/clues/status beside it, next to the hand; the loose stats line + the map-redundant
      location display are gone. Built on S4's `InPlayCardView` (reused untouched).

**Deferred past the gate:** #353 (uses-depletion — no Gathering card; gated on
Forbidden Knowledge / Grotesque Statue, which arrive with
[phase 7.5](phase-7.5-investigator-breadth.md)'s Mystic pool), #427/#429
(native-loop soak residue; both wait on #728), #119/#26 (behaviour-preserving
cleanups — fold in opportunistically). #294 (multi-soak-window drain) was dissolved
by PR #717 — the attack loop now parks unconditionally, so the strand it described
is unreachable and its `debug_assert` is gone.

**#427/#429's deferral was justified here as "rare in 1p", and that was wrong.**
Dynamite Blast 01024 is in `ROLAND_DEFAULT_DECK` (`crates/web/src/picker.rs`), so
#769 will play it. The deferral holds for a different reason: in 1 player,
`for inv in investigators` is a loop of **one**, so #728's frame-shaped restructure
buys nothing the gate can observe — the soak is a single investigator's, which
already works. #728's real driver is multiplayer.

**Pulled into the gate by the 2026-08-22 triage sweep** — each a rules defect
reachable in The Gathering today: #651 ✅ (hunter pathfinding routes
around a Barricade instead of being stopped by it), #664 (First Aid 01019 offers
a mode with no eligible target and burns a supply), #682 ✅ (an
attack whose target leaves play mid-test resolves against difficulty 0), #763
(zero-icon commits accepted) and #764 (a defeated active investigator's turn
does not end). The same sweep moved #353, #366, #367 and #555 *out* of the
milestone: each one's own body or ADR (0008/0009) defers it until a card wants it.

**#797 joined wave 1 on 2026-08-24**, out of #651's PR rather than a sweep. #651 had
scoped agenda 01107's forced Ghoul move out because it names a *fixed* destination, so
`glossary/Nearest.md`'s "distances ignore blocks" clause never engages and the vendored
text looked silent. `glossary/Patrol.md` is the source that answers it: patrol is also a
fixed-destination shortest-path mover and carries the same *"would be compelled to move
to a location which is blocked by a card ability, the enemy does not move"* clause. So
the audit's Uncertain is resolved and 01107 is a rules defect like its siblings, not a
deferred judgement call. **PR #798 shipped it** on that reading.

The sweep also pulled in **#670**, wrongly: it read *"reachable today"* off the
cards' presence in the core **pack** rather than in this scenario's encounter
sets. Every instance is Pentagram, Arkham or Dunwich, and The Gathering's only two
enemies printing a Spawn clause print `Specific` ones (Flesh-Eater 01118's Attic,
Icy Ghoul 01119's Cellar). #670 → phase 9, and #792 with it.

**#764 was not solo-reachable, and the sweep called it solo.** It shipped inside
the gate anyway because PR #790 was already written and green — not because the
gate needed it. What the sweep's code-reading missed is an interaction one file
away: `apply_investigator_defeat` visibly fails to rotate the turn frame, and
nothing at that call site mentions that `InvestigatorTurn` is also in
`cancelled_by_scenario_end`. With one investigator, defeat *is* all-defeated, so
`check_all_defeated` latches `Resolution::Lost` and ADR 0004 pops the frame
before `drive` can re-prompt it. The stale prompt needs a **surviving**
investigator, so the defect bites at 2+ players — which also made the issue's own
acceptance criterion ("Solo: the game proceeds to the Enemy phase") unsatisfiable
as written. Carry forward to the next sweep: a *solo-reachable* claim about a
defeat path has to be checked against the all-defeated latch, not just the
handler that looks wrong.

**The same issue's body quoted a rule that does not exist.** It cited Elimination
as *"if it is that investigator's turn, the turn ends"*; that clause is in no
vendored source — not `rules/glossary/Elimination.md`, `Defeat.md` or
`Resign.md`, not `Appendix_II_Timing_and_Gameplay.md`, and nowhere in
`data/official-faq/`. The real basis is Appendix II step 2.2.1, *"If the
investigator does not or cannot take an action, proceed to 2.2.2."* Both this and
the solo claim are the sweep's AI-generated triage text, and both survived
untouched to the point of implementation — so a triage-authored citation is worth
re-deriving from the vendored files before building on it, exactly as CLAUDE.md's
citation mandate already requires of a card's text.

**Pulled into the gate by #769's wave-0 reconnaissance run (2026-08-24):** #786 and
#787, placed in waves 1 and 2 above. **Filed and left out:** #788 — the web commit
control offers the whole hand at the commit window, so now that #763 rejects an
ineligible commit a player can be offered a move the rules forbid and get a rejection
back. It is a wart, not a stall: the engine is the authority and is correct, the run
can complete around it, and doing it properly needs a design call (client-side filter
vs. eligible indices on the `InputRequest`) that has to leave room for the
conditional-eligibility cards `skill_icons` cannot express — Opportunist 01053's
*"Commit only to a skill test you are performing"*, Take Heart 04201's *"You may
commit Take Heart to any type of test"*. None of those are implemented yet.

**Pulled into the gate by the 2026-08-23 recharter:** the whole of #258 — the
optional content this doc had filed under *Future slices* — plus #769 (the
playthrough, which had never had an issue), #770 (split from #586), and #775
(found while mapping #258). **Moved out:** #458 and #588 (the resume-token chain →
phase 8; a single player double-clicking Confirm in one tab is a real bug but not
a rules one), and investigator breadth in its entirety →
[phase 7.5](phase-7.5-investigator-breadth.md), which also took #366, since
Wendy Adams 01005 is its first real consumer.

## Frame-model end-states (#393)

For a future author who sees the partial state and wonders what's "missing":
- **C checkpoint** ✅ and **EmitEvent-frame** (3rd checkpoint) ✅ — both shipped.
- **2b** (typed `PlayerAction` → `OptionId`-only) ✅ — shipped (PR #460). The open
  turn is an `AwaitingInput` menu; gameplay is `ResolveInput(PickSingle(OptionId))`
  dispatched via the internal `TurnAction` map. `PlayerAction` = `StartScenario` +
  `ResolveInput` (single-variant end-state deferred to #459).
- **B** (every straight-line step a frame) — **intentionally dormant**, reached
  *content-driven* (a card making a step a decision). No Core+Dunwich card forces
  it; B's marginal frames "earn nothing operationally." The visible remnant is the
  intra-skill-test `SkillTestStep` cursor — **not a gap**, leave it until a card
  puts a decision mid-test.

## Architecture to build on

Only the durable facts a future PR-author needs that aren't obvious from the code.

**Attack loop (keystone for damage/soak work).** `enemy_attack` does `assign →
place → defeat`; window-queuing lives in `drive_attack_loop`, which parks remaining
attackers as a `Continuation::AttackLoop` frame *beneath* the window (#411) and
resumes via `resume_enemy_attack`. With 2+ engaged enemies the player picks attack
order first (`AttackLoopStage::PickOrder`), so the frame spans the whole enemy-phase
step 3.3; single-enemy stays Shape A. The five basic actions + activated abilities +
non-fast card plays park on a `Continuation::ActionResolution` frame and fire AoO via
`drive_aoo` → `drive_attack_loop`; retaliate routes via `drive_retaliate`. Exhaust
differs by source: enemy-phase always (even cancelled, RR p.6/p.25); AoO never
(RR p.7); Retaliate never (RR p.18). `provokes_aoo` exempts `Effect::Fight` weapons;
fast plays/abilities provoke nothing (gate on `!is_fast`). Soak-first by
`CardInstanceId` order is the interactive-distribution entry point.

**Trigger spine.** `queue_event` is the one dispatch chokepoint (two-phase
forced-then-reaction, RR p.2; simultaneous triggers lead-ordered via a
`TimingPointWindow { Forced }` run, RR p.17). It **queues** — a caller with work
after the emit resumes on a frame and emits in tail position (ADR 0003, #569).
Reentrancy resolves by **top-frame
dispatch** (C-plumbing, PR #443): the loop dispatches whatever is on top — a mid-test
window above the `SkillTest`, then the `SkillTest`, then a forced run beneath — so no
driver distinguishes "above" from "below". Reaction/forced windows resume via
`PickSingle(OptionId)`. The `when → at → after` axis is a `Continuation::EmitEvent`/
`TimingPoint` coordinator that re-scans each cell fresh (the per-cell re-scan,
`tests/round_end_rescan.rs`). **Every** triggering condition walks that sequence
(#702 ✅ shipped, PR #712), not just the round end — a cell is populated iff the
per-cell scan finds something in it, so there is no per-condition table of which
cells a condition supports. The condition's own resolution is step 2 of the walk
([ADR-0008](../adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md),
#701 ✅ shipped, PR #711); who performs it is an exhaustive classification, and a
condition still owned by its emitting caller does not walk its `when` cell —
declaring an interrupt on one is **rejected**, never dropped. Ownership migrates one
condition at a time: **#703 ✅ shipped (PR #713)** made clue discovery the first
coordinator-owned migration — `WouldDiscoverClues` collapsed into `DiscoverClues`,
one condition in all three cells, with `discover_clue` reduced to cap-and-emit and
the clues moving at the coordinator's resolve step. Cancellation moved there with it:
a `when` replacement's signal is read between the `when` cell and step 2.
**#714 ✅ shipped (PR #715)** then made that read suppress the *whole* rest of the
sequence rather than step 2 alone — the `at` and `after` cells, and the remainder of
the `when` cell, per Dodge 01023's ruling — so a prevented condition abandons its
coordinator frame and an open `when`-cell window is emptied
(`LapseReason::ConditionPrevented`). Scoped to coordinator-owned conditions, so the
enemy attack inherited it on migration rather than half-gaining it early.
**#704 ✅ shipped (PR #717)** was that migration, and the last one the arc needed:
the enemy attack is one coordinator-owned condition in all three cells, with the
damage and horror placed at the resolve step. Its obstruction was never ownership
but drive shape — the attack loop emitted and then read `open_windows()`
synchronously, which ADR 0003 forbids — so the loop now parks on its `AttackLoop`
frame around *every* attack (head left at the list's front), emits in tail position,
and the `drive` loop walks the sequence above it; `BeforeAttack`/`AfterSoak`,
`resume_enemy_attack` and the single-soak-window guard went with it, and the soak
window rode into the coordinator alongside (caller-owned until #694 retags Guard Dog
01021 to the `when` it prints). **No condition bypasses the coordinator now**, and
the last two per-condition tables are gone with them: the single-cell bucket lookup
and the `when`-timing whitelist in `trigger_matches`, which no longer takes a timing
at all. Forced-before-reaction has exactly one mechanism, the `TimingPoint` cursor.
**#705 ✅ shipped (PR #718)** made the migration discoverable to the card author
rather than only to the ADR's reader: `EventTiming`'s own doc comment (which still
called the `at` cell dormant) now describes the three cells as they work and names
the reject, `CONTEXT.md` defines **timing cell** including the `"if … would …"`
when-tier distinction, and `CLAUDE.md` and `docs/agents/standards.md` carry a
sentence and an index row apiece.
**#719 ✅ shipped (PR #723)** started spending that vocabulary on the corpus: #694's
audit found six of the nineteen shipped `EventTiming` declarations naming the wrong
cell, and the three printing *"at"* or *"if"* — act 01110, Frozen in Fear 01164,
agenda 01107's enemy-phase-end move — retag to `At` with no engine change, since the
`at` cell is walked whoever owns the resolve step. The other three print *"when"* on
caller-owned conditions and ride their migrations (#720/#721/#722), which is why the
retag ticket split from them — **all three have since shipped**, and the audit is
closed. The conventions that stop a seventh mis-tag landed
alongside: `CLAUDE.md` now requires the declared cell to match the trigger word in
the module's own verbatim card-text block *and* the module's prose to name the cell
and why, with the licensed exception spelled out (a *"when"* card on an unmigrated
condition stays `After` until it migrates). The automated check #694 originally
specified is withdrawn with its reasoning recorded, because the trigger word is not
mechanically derivable — a parser would encode one reading of the rules and then be
trusted as though it had checked them. The evidence was never missing; every
mis-tagged module quoted its own trigger word three lines above the wrong enum, so
what the standards row adds is an assignee for the reading. The retrofit onto the
modules the PR didn't touch is **#724 ✅ shipped (PR #730)**, below.
**#720 ✅ shipped (PR #725)** rode the first of those three: Cover Up 01007's
*"**Forced** - When the game ends…"* trauma retags to `When`, and `GameEnd` migrates
ahead of it as the second **bare milestone** — the class `RoundEnded` was the sole
member of. Nothing about game state changes as the ending resolves: the `ScenarioEnd`
frame advances its cursor to `Finalize` and emits in tail position, so the
victory-display scan and `apply_resolution` land at the apply boundary after every
queued ability has drained, which is why the trauma's interactive acknowledge already
spanned `apply` calls. `EliminationGameEnd` migrated with it — RR p.10 Elimination
step 0 scans the same declaration, since a weakness prints one *"when the game ends"*
trigger rather than two, and Cover Up's ruling says it fires there — so migrating one
without the other would have made the retag reject the elimination it was meant to
serve. The dear part was neither classification but a **fork predicate**:
`has_weakness_game_end_ability` decides whether elimination needs its frame at all,
and asked the per-cell scan about `After` only. A per-cell scan answering a
whole-sequence question does not reject, it drops the ability silently — the fork
takes its inline path and step 1 removes the weakness unfired. The list is now
*derived* from the coordinator's own cursor (`EmitStep::cells()`) rather than written
out beside it, so a fourth cell cannot be forgotten from it.
**#721 ✅ shipped (PR #726)** rode the second: Barricade 01038's *"**Forced** - When
an investigator leaves attached location: Discard Barricade."* retags to `When`, and
`LeftLocation` migrates ahead of it — the first migration whose condition was **not**
a bare milestone, so the departure genuinely had to move. Its mutation was the inline
kind #704 made expensive rather than #703's already-named function, so the prefactor
came first as its own commit: the engaged-enemy drag (and the disengage of one that
cannot follow), the location assignment and the `InvestigatorMoved` event became
`resolve_departure`, with the existing move coverage green across it, and only then
did the arm flip. `move_primary_effect` now validates and emits, nothing else.
**Two things deliberately stayed put**, and one of them moved the *other* way. The
entered-location half keeps riding the `MoveEnter` frame #569 parked beneath the emit
— that frame is the whole reason entering does not resolve before leaving, and a
migration that quietly undid it would have re-opened the bug ADR 0003 exists for. The
destination **reveal** moved off `move_primary_effect` and *onto* that frame: it is
the arrival's business, and after the migration the arrival happens further down the
stack than the point the reveal used to sit at, so leaving it where it was would have
revealed a location before the investigator had left the one they were standing on.
Nothing in the corpus was watching it — every test fixture location is born
`revealed: true` — which is why the relocation needed a test built to see it.
The retag itself is **invisible in the corpus**: no Core or Dunwich card observes
whether Barricade discards before or after the departure lands, so unlike #720 this
fixed a wrong tag rather than a live bug. It was done because a permanently-waived row
is the rot a self-clearing list exists to prevent, and because ADR 0008's terminal
condition — every arm flipped, the classification and the reject deleted together —
only stays reachable if the arms actually flip. `CLAUDE.md`'s licensed mismatch then
named Guard Dog 01021 alone — and #727 retired it.
The attacker's **exhaust** could not move to the resolve step — a cancelled attack
still exhausts (Dodge 01023's ruling) and a cancel abandons the sequence before step
2 — so it sits on the parked loop frame, after the `after` cell, where step 3.3 puts
it. Silver Twilight Acolyte 01102 is the corpus proof: the first card to declare the
attack condition, and the first forced point whose scan source is an enemy's own
card. Dodging it places no doom.

**#727 + #722 ✅ shipped (PR #729)** rode the third and last, and it cost a *model*
rather than an arm. Guard Dog 01021's *"[reaction] **When** an enemy attack deals
damage to Guard Dog: Deal 1 damage to the attacking enemy."* could not be served by
flipping `EnemyAttackDamagedSelf` to coordinator-owned, because the condition itself
was wrong: `glossary/Dealing_Damage_Horror.md` deals damage in **two** numbered steps
with a named window between them — assign, *"Abilities that prevent, reduce, or
reassign damage … are resolved between steps 1 and 2"*, then place — and one condition
announced per damaged asset *after* a single collapsed place-and-announce call is
neither of them. So it was replaced by **`DamageAssigned`** (a bare milestone, the
class's fourth member: an assignment is a proposal, tokens sitting *"next to"* the
cards, so nothing is on any card as it resolves) and **`DamagePlaced`**
(coordinator-owned, its impact the unchanged `place_assignment` — simultaneous
placement plus the defeat determination), sequenced by one `Continuation::DealDamage`
frame walking `Distribute → Announce → Place → Finish`. Two emits with a live object
between them is what ADR 0003 forbids doing synchronously, so the cursor is the only
legal way to order them; the frame owns the live assignment and each emit snapshots
it, which is what lets pattern matching keep reading the event instead of reaching
down the stack. **[ADR 0009](../adr/0009-damage-is-assigned-then-placed.md)** records
the model and the sweep of all 159 Chapter 1 packs behind it — Baron Samedi 05019 and
Perseverance 04111 sit on opposite sides of the window and foreclose the
single-condition reading, though neither is in the corpus.
Unlike #721 this fixed a **live bug** as well as a tag: `place_assignment` returned
only the damaged assets that *survived*, and only those were announced, so a Guard Dog
killed by the attack that damaged it never retaliated — against its own ruling
(*"You can use Guard Dog's ability when you assign lethal damage/horror to it."*).
Announcing before anything is placed makes the survivor filter unnecessary and the
ruling true. Two things stayed put: `take_damage` / `take_horror` still place
synchronously and announce nothing, because their callers do synchronous post-call
work (Dynamite Blast 01024's `for inv in investigators` loop wants #704's shape in
miniature) — `TODO(#728)` at each site; and a full cancel in `DamageAssigned`'s `when`
cell would abandon the coordinator's sequence but not the frame beneath it, which is
**#366**'s to fix along with partial replacement, since the one `pending_cancellation`
bool cannot say *whose*. `run_reaction_continuation` was deleted on the way out: with
the last two conditions that had post-window work now walking the coordinator, every
arm had collapsed to `Done`.

**#724 ✅ shipped (PR #730)** closed the arc #719 opened, retrofitting the
cell-naming paragraph onto the thirteen `OnEvent` modules that predated the
convention. Prose only — no declaration retagged, no test changed — and the point
of it is what the reading found: **no seventh mis-tag**. Every one of the fifteen
declared cells matches the trigger word in its own module's verbatim card-text
block, which is the first time that has been checked module by module rather than
inferred from #694's audit. Two readings were worth writing down. The four
**reverse-side** abilities (agendas 01105/01106, acts 01108/01109) print no trigger
word at all, so the rule that the printed word names the cell cannot decide them:
each is step 2 of `glossary/Act_Deck_and_Agenda_Deck.md`'s advance procedure,
*"Flip the advancing card over and follow the instructions on the reverse ("b")
side."*, and declaring them in the flip's `after` cell is what puts them between
step 1's token removal and step 3's *"the next card in the deck becomes the current
act/agenda"* — exactly where the `AdvanceReverse` frame holds the deck cursor until
its `Finalize` step. The modules say outright that nothing contests the cell rather
than implying a reading they didn't make; whether step 2 *is* the condition
resolving, which would make these `at`-tier, is the one question this pass leaves
open and the likeliest ADR to come out of it. Silver Twilight Acolyte 01102, which
had named its `after` cell only as a consequence of the Dodge ruling, now names it
from its own printed *"After"* first. And `CLAUDE.md` gained the paragraph's
**form** — a bold inline `**Cell: …**` lead-in, not a `# Cell` heading — after the
sweep produced both and found neither was written down; all twenty cell-declaring
modules now use one shape, Guard Dog 01021's one-off phrasing included.

**Choice & cancellation.** Interactive choice runs inside the **effect evaluator's
`Continuation::Effect` frames** (#422 / PR #424): `resolve_choice_count` (0 ⇒
reject/auto · 1 ⇒ auto-bind · 2+ ⇒ suspend); a node needing a choice **suspends in
place** and resume **re-steps the same leaf** with `chosen_option` set — no replay,
no `DecisionCursor`. DSL targets bind through `ground_chosen_targets`
(`chosen_investigator`/`location`/`enemy`); native leaves read `chosen_option`.
Spatial targets use `Choose<S> { scope }` (`LocationSet { Here, Anywhere }` /
`EntityScope`). Before-timing cancellation is a Before window the caller suspends on
+ an `Effect::Cancel` leaf setting `pending_cancellation` (a `bool` suffices —
Before-windows don't nest in scope, #367), honored on window close. One bool for
both suppressing arms (cancel, nature-changing replacement); the non-suppressing
third — replace-with-a-different-impact — is #366, unmodelled until a card wants it. A reaction event
(Evidence! 01022) rides the window's candidate list and is *played* when picked
(`TriggerKind::Reaction` `OnEvent`, window-only).

**Skill-test control-flow shape.** Storage is on the stack (`InFlightSkillTest`
folded onto the `Continuation::SkillTest` frame, #348). Dispatch is top-frame
(C-plumbing): the `drive` loop's `SkillTest` arm calls `advance` when the frame is on
top; a mid-test window makes `advance` yield `Done`, and the loop re-dispatches
`SkillTest` on window close. **Intra-test sequencing is still an inline cursor** —
the `SkillTestStep` enum (`PreCommitWindow → AwaitingCommit → PreTokenWindow →
Resolving → …`) is a field advanced by a `loop` in `advance`. That's the remaining
Shape-A compression (= the dormant end-state B); reifying each step is unpaid for
until a card demands it. Two entry points: the commit hop (`finish_skill_test`) and
the loop's `SkillTest` arm.

**`IntExpr` dynamic-expression substrate.** Board-state-dependent values are an
`IntExpr` AST (`card-dsl/src/dsl.rs`: `Lit(i8)` + `Cond { when: Condition, then,
otherwise }`) — **shipped and wired into `Effect::Fight.combat_modifier`** (Roland's
.38 Special 01006: `IntExpr::cond(Condition::LocationHasClues, 3, 1)`). So the
"dynamic skill-test modifier surface" is a settled `IntExpr`, **not** a needs-design
question. The #118/#300/#426 cluster each extend it the same way (add a `Condition`/
term + plumb `IntExpr` into one more `Effect` field).

**Content patterns.** Card stats come from the corpus (`CardKind`; read via
`cards::by_code` / `metadata_for`, never hand-typed) — a future enemy/card lands via
a snapshot bump + regen, no impl. Single-use card logic is `Effect::Native { tag }`
(promote to a shared `Effect` variant only at ≥2 reuses). Scenario chaos-symbol /
reference-card effects live on the `ScenarioModule.resolve_symbol` hook, not card
`abilities()`.

**Asset slots (PR #516, #498).** Slot limits are enforced at the RR "entering the
slot" moment (`dispatch/slots.rs`; `dispose_play_from_hand`'s InPlay branch →
`enter_asset_making_room`), **not** at validation. `default_slot_capacity` holds the
RR p.19 defaults (Ally/Body/Accessory 1, Hand/Arcane 2); a full slot does *not* block
the play (`check_play_card` rejects only `need > cap`, unreachable in corpus) — instead
occupying assets are discarded to make room: forced single-candidate auto-discards,
2+ candidates suspend on a `Continuation::SlotDiscard` `PickSingle` (mirrors the soak
distribution driver on `Continuation::DealDamage`). A slot-modifying card (none in Core/Dunwich) turns
`default_slot_capacity` into a per-investigator query. The in-play-asset discard sequence
is now one helper, `cards::discard_card_from_play` (#119, reused by soak-defeat,
uses-depletion, `Cost::DiscardSelf`, make-room).

**Seating & the seed (PR #461).** Seating is **not** a player action — it's the engine
fn `seat_and_open(setup_state, &roster) -> ApplyResult` (wraps the internal
`start_scenario` + `drive` via `apply_via`). Hosts call it at game-creation and persist
the **seated, mulligan-pending** result as the seed; the action log is `ResolveInput`-only.
Two consequences a future persistence/replay PR must respect: (1) the setup shuffle is
baked into the seed's frozen `RngState`, so replay never re-runs setup RNG — don't
re-seed; (2) the seed can itself be `AwaitingInput`, so the seed's `EngineOutcome` is
persisted alongside `seed_state` (server migration `0002` / `seed_outcome` column) and
`load` initializes the outcome from it before replaying — there is no `state → pending
outcome` reconstruction, so a paused seed with an empty log would otherwise load as
`Done`. A roster is mandatory (`seat_and_open` rejects an empty one); seating always
seats investigators `Active`, so "mulligan excludes an eliminated investigator" is
defensive (covered by a direct `active_investigators_in_turn_order` unit test, not via
seating).

## Future slices (after the gate)

Captured but **unfiled** (no issues yet) — filed when the gate closes.

- **Difficulty selection.** Add Easy / Hard / Expert chaos bags + a picker (Slice 1
  is Standard only).
- **Solo-with-2 UX.** One client driving two investigators (picker, whose-turn, two
  boards vs. tabbed). Open design question; the Tier-2 correctness issues (#65, #381,
  #359, #153, #371) land here.

**Investigator breadth left this section** on 2026-08-23 and is now
[phase 7.5](phase-7.5-investigator-breadth.md), a filed milestone rather than a
captured intent. **The deferred Gathering content left too**, in the other
direction: #258 is in the gate, under **Wave 3** above.

Campaign sequencing (The Midnight Masks, The Devourer Below, campaign log + `Fact`
enum) is **Phase 9** — including the first real Peril/Surge cards (Hunting Shadow
01135 et al.; #138/#139 re-milestoned there).

## Open questions

- **Solo-with-2 UX** — how one client presents two investigators. See Future slices.

*(The Roland elder-sign DSL question was retired on 2026-08-23: #118, #448 and #453
all shipped, so it is fully answered rather than "mostly". Its successor — three of
the five elder signs need an on-success **effect**, which `Trigger::ElderSign` has no
field for — is #776, in [phase 7.5](phase-7.5-investigator-breadth.md).)*

## Dependencies

Phases 4 (scenario module), 5 (server + persistence), 6 (web client) — all closed.
Phase 3's Roland Banks (#55) shipped.

## What "done" looks like

A solo human, in the browser, plays The Gathering to a resolution with **1-player
Standard rules correctness**: every basic action available, attacks of opportunity /
retaliate / soak resolving with proper player agency, skill-test windows open, and
Roland's signature firing.

Since the 2026-08-23 recharter, **"a resolution" means all three** — #769 runs to
**Won** (R1 *or* R2, once #775 offers the choice), to **Lost**, and to **Resigned**
(once #644 gives resignation semantics and Wave 3 makes the Parlor's `[action]`
resolve). Two of those three are currently unreachable, which is why the optional
content came into the gate.

Difficulty and solo-2 are Future slices. Investigator breadth is
[phase 7.5](phase-7.5-investigator-breadth.md).
