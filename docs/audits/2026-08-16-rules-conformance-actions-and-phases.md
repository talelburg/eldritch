# Rules-conformance pass — action structure, turn and phase sequence — 2026-08-16

The full Rules Reference is now vendored verbatim at `data/rules-reference/rules/`
(six section files plus 188 glossary entries, indexed by
[`README.md`](../../data/rules-reference/rules/README.md)). This pass reads the
rules that govern **one** surface — what an action is, what the round is made
of, and in what order the two interleave — and checks the engine against them.
Every claim below is anchored to a verbatim quote from the vendored text and to
a `file:line` in the engine, and every one was re-read in the code before it was
written down.

**Scope.** Appendices I–III; the ten action-glossary entries plus **Attack of
Opportunity**, **Fast**, **Parley**, **Explore**; the four phase entries; and
the turn/round supporting entries (**Active Player**, **In Player Order**,
**Lead Investigator**, **Hand Size**, **Ready**, **Exhaust**, **Resign**,
**Winning and Losing**, **Game**, **Mulligan**, **Drawing Cards**, **Limits and
Maximums**), plus everything those cross-referenced into (**Elimination**,
**Enemy Engagement**, **Hunter**, **Move**, **Play**, **Threat Area**,
**Triggered Abilities**, **Lasting Effects**, **Delayed Effects**, **Nested
Sequences**, **Reaction Opportunities**, **Priority of Simultaneous
Resolution**, **Act Deck and Agenda Deck**, **Clues**, **Killed/Insane
Investigators**, **Replacing an Opening Hand**, **Empty Location**).

**Out of scope by construction:** skill-test internals (ST.1–ST.8) beyond the
action-follow-up boundary; card-effect semantics; encounter-card content;
mechanics from cycles outside Chapter 1. Gaps already recorded in
[`2026-08-14-chapter-1-forward-compatibility.md`](2026-08-14-chapter-1-forward-compatibility.md),
already batched in the open [#572](https://github.com/talelburg/eldritch/issues/572)
(the 2026-07-17 audit's rules-gaps list), or anchored by a live `TODO(#NNN)`
are named as **already registered** rather than re-reported.

**Nine findings: six WRONG, three ABSENT.** One of the six —
[Finding 1](#1-an-eliminated-investigators-weakness-never-fires-its-when-the-game-ends-ability) —
contradicts a decision the repo deliberately shipped, and is a live correctness
bug for an implemented Core card in **solo** play. No finding contradicts an
ADR: ADRs 0001–0004 were read in full and none of the four touches action
structure or the phase sequence.

## Method

1. **Read the rules first, in full.** Every file listed under Scope, read
   whole. Nothing was quoted from memory and no network fetch was made. Where a
   card's behaviour was load-bearing, its printed text came from
   `data/arkhamdb-snapshot/pack/` and its rulings from
   `data/arkhamdb-faq/<pack>/<code>.md`.
2. **Read the code, in full where the prompt named it.** The production halves
   of `crates/game-core/src/engine/dispatch/phases.rs` (lines 1–1392; the
   remaining 2,700 lines are five `#[cfg(test)]` modules) and
   `.../dispatch/actions.rs` (lines 1–880, likewise), plus
   `.../dispatch/act_agenda.rs`, `.../enumerate.rs`, `crates/game-core/src/action.rs`,
   `crates/game-core/src/state/phase.rs`, the phase/turn portions of
   `.../state/game_state.rs` and `.../engine/mod.rs`, and — pulled in by the
   findings — `.../dispatch/mod.rs`, `.../dispatch/cards.rs`,
   `.../dispatch/abilities.rs`, `.../dispatch/combat.rs`,
   `.../dispatch/hunters.rs`, `.../dispatch/elimination.rs`,
   `.../dispatch/emit.rs`, `.../dispatch/forced_triggers.rs`,
   `.../dispatch/reaction_windows.rs`, `.../dispatch/skill_test.rs`,
   `.../state/investigator.rs`, and `crates/card-dsl/src/dsl.rs`'s
   `EventPattern` / `Restriction` / `ActionClass`.
3. **Grounded every finding in the shipping corpus.** Where a rule has a
   consumer, the consumer was found by `jq` over the Core and Dunwich pack
   files (the corpus the build compiles, per `CONTEXT.md`) and its text quoted
   verbatim. A rule with no corpus consumer is still reported when the engine
   actively models the concept and gets it wrong, and is labelled latent.
4. **Cross-referenced the tracker.** Every `TODO(#NNN)` and issue reference
   near a candidate finding was checked with `gh issue view`. Both prior audits
   were read in full first.

## Findings

### 1. An eliminated investigator's weakness never fires its "when the game ends" ability

**Rules.** `data/rules-reference/rules/glossary/Elimination.md`, the step the
list opens with:

> 0. For the purpose of resolving weakness cards, the game has ended for the
>    eliminated investigator. Trigger any “when the game ends” abilities on each
>    weakness the eliminated investigator owns that is in play. Then, remove
>    those weaknesses from the game.

Step 1 — the removal the engine implements — is the *next* step:

> 1. The cards he or she controls in play and all of the cards in his or her
>    out-of-play areas (such as hand, deck, discard pile) are removed from the
>    game.

The corpus consumer is Roland Banks's signature weakness, Cover Up 01007
(`data/arkhamdb-snapshot/pack/core/core.json`), verbatim:

> **Revelation** - Put Cover Up into play in your threat area, with 3 clues on it.
> [reaction] When you would discover 1 or more clues at your location: Discard that many clues from Cover Up instead.
> **Forced** - When the game ends, if there are any clues on Cover Up: You suffer 1 mental trauma.

and its ruling, `data/arkhamdb-faq/core/01007.md`, verbatim:

> If Roland is eliminated (by being defeated or taking a **resign** action)
> while Cover Up is in play, Cover Up's **Forced** effect triggers, as per the
> FAQ [V1.0, section 'Rulebook errata', topic "Elimination"].

(<https://arkhamdb.com/card/01007>.)

**Code.** `crates/game-core/src/engine/dispatch/elimination.rs:48-50` runs the
elimination steps and only then checks for a total party defeat:

```rust
run_elimination_steps(cx, investigator);

check_all_defeated(cx);
```

`run_elimination_steps` partitions the threat area at
`crates/game-core/src/engine/dispatch/elimination.rs:90` and `:117`, moving
owned weaknesses into `removed_from_game` with no trigger point emitted. The
`GameEnd` forced scan then filters them out a second time —
`crates/game-core/src/engine/dispatch/forced_triggers.rs:434`:

```rust
.filter(|(_, inv)| inv.status == Status::Active)
```

**Divergence scenario (solo, shipping corpus).** Roland Banks (01001) plays The
Gathering with Cover Up in his threat area holding 3 clues, and is defeated by
damage. `apply_investigator_defeat` flips his status, `run_elimination_steps`
drains Cover Up out of `threat_area` into `removed_from_game`, then
`check_all_defeated` latches the loss, which emits `TimingEvent::GameEnd`. The
scan finds no Cover Up (it is out of the threat area) *and* skips Roland
anyway (he is not `Active`). The engine deals **no** mental trauma. The rules —
and the card's own ruling — say the Forced fires at the moment of elimination,
before the removal, and Roland suffers 1 mental trauma. The same hole applies in
multiplayer to an investigator eliminated while the scenario continues.

**Classification: WRONG.** The concept is fully modelled — `EventPattern::GameEnd`,
`ForcedTriggerPoint::GameEnd`, and Cover Up's `forced_on_event(GameEnd, After, …)`
in `crates/cards/src/impls/cover_up.rs` all exist and work on the normal
scenario-end path. What is missing is Elimination step 0: a `GameEnd` emit
scoped to the eliminated investigator's owned weaknesses, fired *before* step
1's removal.

> **This contradicts a shipped decision, not an oversight.** Issue
> [#567](https://github.com/talelburg/eldritch/issues/567) (closed via PR #601)
> is titled *"Elimination leaves threat_area … RoundEnded/GameEnd forced scans
> ignore Status"*, and its acceptance criteria include, verbatim:
> *"Eliminated investigator's Cover Up does **not** fire GameEnd trauma (test:
> eliminate mid-scenario, run to resolution)."* Its stated rationale — *"RR
> p.10 step 1 removes all his cards from the game, so this is a campaign-affecting
> rules error once Phase 9 wires the log"* — reads step 1 without step 0, which
> exists precisely to carve weaknesses out of it. The `RoundEnded` half of #567
> (`forced_triggers.rs:329`) is **correct** and should stay; only the `GameEnd`
> half is inverted. Because #567 is closed and not an ADR, the decision has no
> other home, so re-deciding it is a tracker action, not an ADR amendment.

### 2. An "until the end of the round" lasting effect expires after the round-end abilities, not before

**Rules.** `data/rules-reference/rules/glossary/Lasting_Effects.md`:

> A lasting effect expires as soon as the timing point specified by its duration
> is reached. This means that an "until the end of the phase" lasting effect
> expires **before** an "at the end of the phase" ability or delayed effect may
> initiate.

and `Appendix_II_Timing_and_Gameplay.md`, step 4.6, in full:

> As the upkeep phase is the final phase in the round, this step also formalizes
> the end of the round. Any active "until the end of the round" lasting effects
> expire at this time.

**Code.** `crates/game-core/src/engine/dispatch/phases.rs:1092-1095`
(`upkeep_round_end`) emits `TimingEvent::RoundEnded` and cedes to the drive
loop; the lasting-effect expiry runs only when that coordinator has popped, at
`crates/game-core/src/engine/dispatch/phases.rs:1123`:

```rust
cx.state.skill_substitutions.clear();
```

**Divergence scenario.** An investigator plays Mind over Matter 01036 —
*"Until the end of the round, you may use your [intellect] in place of your
[combat] and [agility]."* — during their turn. The round reaches Upkeep 4.6.
The engine fires every "at the end of the round" ability (agenda 01107's
*"Forced - At the end of the round: Place 1 doom on this agenda for each [[Ghoul]]
enemy in the Hallway or Parlor."*, act 01109's round-end advance window,
Dissonant Voices 01165's self-discard) **with the substitution still active**,
and clears it afterwards. Per the rule the substitution expires first, so any
round-end ability that initiated a combat or agility test would see the
investigator's printed stat, not their intellect.

**Classification: WRONG** (ordering). Latent in the shipping corpus — no
round-end ability in Core or Dunwich initiates a skill test, so nothing observes
the inversion today. Reported anyway because the order will be wrong for the
first card that does, and because of the citation below.

> **The doc-comment cites a rule that does not exist.**
> `crates/game-core/src/engine/dispatch/phases.rs:1120` justifies the ordering
> as *"RR p.24, \"after the round-end forced abilities have resolved\""*. That
> string appears **nowhere** in `data/rules-reference/`; `grep -r` over the whole
> vendored reference returns nothing. The actual 4.6 text (quoted above) says
> only "at this time", and `Lasting_Effects.md` resolves the ambiguity in the
> opposite direction. Worth flagging separately from the ordering bug: a
> quoted-looking citation that no source contains is exactly the failure mode
> the vendoring exists to prevent, and the 2026-08-14 audit's method note
> records a fabricated rules quote from a summarising fetch as a prior instance.

### 3. Player-triggered abilities can only be initiated from cards in the investigator's own play area

**Rules.** `data/rules-reference/rules/glossary/Triggered_Abilities.md`, which
governs `[action]`, `[free]`, and `[reaction]` alike:

> An investigator is permitted to use triggered abilities ([free], [reaction],
> and [action] abilities) from the following sources:
>
> - A card in play and under his or her control. This includes his or her investigator card.
> - A scenario card that is in play and at the same location as the investigator. This includes the location itself, encounter cards placed at that location, and all encounter cards in the threat area of any investigator at that location.
> - The current act or current agenda card.
> - Any card that explicitly allows the investigator to activate its ability.

`glossary/Activate_Action.md` repeats the first three bullets for the Activate
action specifically.

**Code.** `crates/game-core/src/engine/dispatch/reaction_windows.rs:2041-2050`
— the single validator behind both the `ActivateAbility` handler and the
enumerator — resolves the source by searching one vector:

```rust
let Some(in_play_pos) = inv
    .cards_in_play
    .iter()
    .position(|c| c.instance_id == instance_id)
```

The enumerator mirrors it (`crates/game-core/src/engine/enumerate.rs`,
`push_card_actions`: `for card in &inv.cards_in_play`). The asymmetry is sharp,
because the **forced/reaction** scans use a wider set:
`crates/game-core/src/state/investigator.rs:137-141` yields
`investigator_card` → `cards_in_play` → `threat_area`, and the board scans
additionally cover locations, the act, and the agenda (`crates/cards/src/impls/attic.rs`
and `cellar.rs` are shipped location abilities). So a location's *Forced* ability
fires and its `[action]` ability is unreachable.

**Divergence scenario (shipping scenario).** The Gathering puts the Parlor
(01115) into play. Its printed text
(`data/arkhamdb-snapshot/pack/core/core_encounter.json`) begins:

> [action] **Resign.** "This is too much for me!" You run out the front door, fleeing in panic.

An investigator standing in the Parlor with an action remaining asks for the
turn menu. `legal_actions` enumerates `cards_in_play` only, so no option for the
Parlor's ability is offered, and a hand-built `ActivateAbility` naming it would
be rejected with *"has no in-play instance"*. Per the rules the ability is a
legal Activate action from the location the investigator is at.

The same narrowing blocks Dreams of R'lyeh 01182 and Haunted 01098 (threat-area
treacheries whose only exit is an `[action]`), the Core Midnight Masks Parley
enemies 01138–01140, act 01123's and act 01148's `[action]` abilities, and the
Core Downtown/Southside/St. Mary's location actions.

**Classification: WRONG.** The Activate action is modelled end to end —
`Trigger::Activated`, action-cost payment, cost validation, the AoO fork — and
`abilities_for` is code-keyed, so a location or agenda already resolves
abilities. The defect is the permitted-source condition, which is one of the
four bullets instead of four.

*Two riders on the same finding.* First,
`crates/game-core/src/engine/dispatch/abilities.rs:140` exempts only
`Effect::Fight` from the attack of opportunity:

```rust
action_cost > 0 && !matches!(effect, crate::dsl::Effect::Fight { .. })
```

`glossary/Attack_of_Opportunity.md` exempts *"to **fight**, to **evade**, or to
activate a **parley** or **resign** ability"*, so widening the source set
without widening this predicate would make the Parlor's Resign provoke attacks
it must not. The code documents the deferral (*"no activated parley/resign card
in scope"*) — but the Parlor is in the shipped scenario, so the premise is
already false. Second, `ready_exhausted_cards`
(`crates/game-core/src/engine/dispatch/phases.rs:1162`) readies `cards_in_play`
and `enemies` but not `investigator_card` or `threat_area`; that is consistent
with today's narrow exhaust surface and becomes a second bug the moment the
first bullet ("his or her investigator card") is honoured.

### 4. Resign has no producer at all

**Rules.** `data/rules-reference/rules/glossary/Resign.md`, in full:

> Some abilities are identified with a **Resign** action designator. Such
> abilities are initiated using the "Activate" action (see "Activate Action" on
> page 4).
>
> - When an investigator resigns, the investigator is eliminated by resignation
>   (see "Elimination" on page 10.) An investigator who resigns is not
>   considered to have been defeated.

**Code.** `Status::Resigned` and `DefeatCause::Resigned` exist and are wired
through elimination (`crates/game-core/src/engine/dispatch/elimination.rs:38`,
`crates/game-core/src/state/investigator.rs:204`), and the doc-comments say so
explicitly — `investigator.rs:202-204`: *"Investigator chose to resign from the
scenario. Not yet produced by the engine; the Resign action is downstream."*
Nothing in the engine ever constructs `DefeatCause::Resigned`: a repo-wide grep
finds the variant only in the two type definitions, the one match arm, and
cursor/skip predicates.

**Divergence scenario.** A player in The Gathering wants to leave the scenario
alive via the Parlor. There is no input that produces it, at any point in the
round, so `Status::Resigned` is unreachable and the "no resolution reached"
ending that `glossary/Winning_and_Losing.md` describes for a resigning party
cannot occur.

**Classification: ABSENT.** Distinct from Finding 3: even with the Activate
source set widened, resigning needs the designator's own semantics — AoO
exemption, elimination-by-resignation rather than defeat, and the campaign-side
"not defeated" distinction. There is no `Effect` for it and no tracked issue
naming it.

### 5. Two of the four phase-end trigger points are not emitted, and no phase-start trigger point exists

**Rules.** `Appendix_II_Timing_and_Gameplay.md`, step 1.1:

> The beginning of a phase is an important game milestone that may be referenced
> in card text, either as a point at which an ability may or must resolve, or as
> a point at which a delayed effect resolves or a lasting effect expires.

and step 1.5, the mirror for phase ends:

> The end of a phase is an important game milestone that may be referenced in
> card text, either as a point at which an ability may or must resolve, or as a
> point at which a delayed effect resolves or a lasting effect expires.

**Code.** `crates/game-core/src/engine/dispatch/emit.rs:56-136` defines
`TimingEvent` with a `PhaseEnded { phase }` variant and **no** phase-start
variant. Only two of the four phase-ends route through it:
`enemy_phase_end` (`phases.rs:664`) and `upkeep_phase_end` (`phases.rs:373`)
call `queue_event(… PhaseEnded …)`, while `investigation_phase_end`
(`phases.rs:393`) and `mythos_phase_end` (`phases.rs:727`) push a bare
`Event::PhaseEnded` and comment that a forced ability *"would NOT fire until
#212's queue_event restructure"* (`phases.rs:391`, `phases.rs:724`). All four
`PhaseStarted` sites (`phases.rs:430`, `:305`, `:591`, `:989`) push a bare event
with no emit. `crates/card-dsl/src/dsl.rs:317` records the same limitation on
the pattern itself: *"Currently wired for Enemy and Upkeep phase-ends only;
Mythos and Investigation are not wired (see #212)."*

**Divergence scenario.** Wizard of the Order 01170, a Core encounter enemy
(`data/arkhamdb-snapshot/pack/core/core_encounter.json`), verbatim:

> **Spawn** - Any empty location.
> Retaliate.
> **Forced** - At the end of the mythos phase: Place 1 doom on Wizard of the Order.

It spawns; the Mythos phase ends every round thereafter; the engine emits
`Event::PhaseEnded { Mythos }` as a log entry only, no forced scan runs, and the
enemy accrues no doom for the rest of the scenario. The Dunwich half of the
corpus has three phase-*start* consumers with nowhere at all to attach —
Hunting Horror 02141 and Peter Clover 02079 (*"**Forced** - At the start of the
enemy phase: …"*) and agenda 02065 (*"**Forced** - At the start of the enemy
phase: Discard each [[Criminal]] enemy in the same location as an [[Abomination]]
enemy."*). The Red-Gloved Man 02310 is a second Mythos-end consumer.

**Classification: WRONG for phase ends** (the trigger point exists and two of
four phases are silently unwired), **ABSENT for phase starts** (no
`TimingEvent`, no `EventPattern`, no emit site — a new pattern plus four emit
sites, not a fix).

> **The tracked-gap escape hatch does not apply.** Both code comments and the
> DSL doc-comment defer to
> [#212](https://github.com/talelburg/eldritch/issues/212), which is **CLOSED**
> ("emit_event dispatch unification"). The pointers send a reader to finished
> work while the gap is live — the same species of drift the 2026-08-14 audit
> recorded for #52 and #44.

### 6. The mulligan reshuffles the discarded cards before drawing their replacements

**Rules.** `data/rules-reference/rules/glossary/Mulligan.md`:

> After a player draws a starting hand during setup, that player has a single
> opportunity to declare a mulligan on any number of the drawn cards he or she
> does not wish to keep in his or her starting hand. These cards are set aside,
> and an equivalent number of cards are drawn and added to the player's starting
> hand. The set-aside cards are then shuffled back into the player's deck.

Three ordered steps: set aside, draw replacements, *then* shuffle back.
`Appendix_III_Setting_Up_The_Game.md` step 8 is consistent (it uses the same
set-aside-then-shuffle-back shape for opening-hand weaknesses).

**Code.** `crates/game-core/src/engine/dispatch/cards.rs:618-629`, in
`resume_mulligan`:

```rust
for &i in sorted.iter().rev() {
    let card = inv_mut.hand.remove(i as usize);
    inv_mut.deck.push(card);
}
…
if redrawn_count > 0 {
    shuffle_player_deck(cx, investigator);
    draw_cards(cx, investigator, redrawn_count);
```

The mulliganed cards go into `deck`, the deck is shuffled, and only then are
replacements drawn — so the discarded cards are in the pool the replacements
come from.

**Divergence scenario.** An investigator opens with five cards, four of them
weak, and mulligans all four. The engine puts those four back, shuffles a
30-card deck, and draws four — with a real chance of returning one or more of the
exact cards just rejected. Per the rules the four are set aside and *cannot* be
redrawn; four different cards come off a 25-card deck, and only afterwards do
the four rejects shuffle back in.

**Classification: WRONG** (ordering). The destination is right — the module's
own comment correctly notes the cards go back to the deck rather than the
discard pile — and the weakness-replacement handling around it is faithful; only
the sequencing of shuffle-back versus redraw is inverted.

### 7. A failed Fight against an enemy engaged with another investigator deals no damage

**Rules.** `data/rules-reference/rules/glossary/Fight_Action.md`:

> If the test fails, no damage is dealt to the attacked enemy. However, if an
> investigator fails this test against an enemy that is engaged with another
> single investigator, the damage of the attack is dealt to the investigator
> engaged with that enemy.

The same entry establishes that the case is reachable:

> - An investigator may fight any enemy at his or her location, including: an
>   enemy he or she is engaged with, an unengaged enemy at the same location, or
>   an enemy engaged with another investigator who is at the same location.

**Code.** `crates/game-core/src/engine/dispatch/skill_test.rs:1189-1210` handles
the `SkillTestFollowUp::Fight` arm, and `apply_follow_up_step`
(`skill_test.rs:454-469`) gates the whole follow-up on success:

```rust
if resolved(cx).succeeded {
    apply_skill_test_follow_up(cx, investigator, follow_up);
}
```

The only thing that happens on a failed Fight is the retaliate check
(`fire_retaliate_if_any`, `skill_test.rs:1257`). Nothing anywhere reads the
target enemy's `engaged_with` on failure.

**Divergence scenario.** Roland and Daisy are both at the Study; a Ghoul Minion
is engaged with Daisy. Roland spends an action to Fight it (legal — the
enumerator offers any co-located enemy, `enumerate.rs`, `push_combat_engage_actions`)
and fails the combat test. The engine deals nothing to anyone. The rules deal
the attack's damage to **Daisy**.

**Classification: ABSENT.** There is no representation of "attack damage
redirected to a third party on a failed test": `SkillTestFollowUp::Fight` carries
only `{ enemy, extra_damage }`, the failure path has no branch, and the
follow-up gate is a blanket success check. Multiplayer-only in effect, and
multiplayer is in scope — `docs/product-decisions.md` calls it synchronous-only
but first-class, and `docs/phases/phase-8-multiplayer-and-auth.md` is a
milestone.

### 8. Stealing an enemy from another investigator announces the engagement but not the disengagement

**Rules.** `data/rules-reference/rules/glossary/Engage_Action.md`:

> - An investigator may perform the engage action to engage an enemy that is
>   engaged with a different investigator at the same location. The enemy
>   simultaneously disengages from the previous investigator and engages the
>   investigator performing the action.

**Code.** `crates/game-core/src/engine/dispatch/actions.rs:299-305`:

```rust
let enemy_mut = cx.state.enemies.get_mut(&enemy_id).expect("checked above");
enemy_mut.engaged_with = Some(investigator);
cx.events.push(Event::EnemyEngaged {
    enemy: enemy_id,
    investigator,
});
```

The overwrite is correct as *state* (`engaged_with` is a single `Option`), but
no `Event::EnemyDisengaged` is emitted for the previous investigator. The
neighbouring helper `reengage_at_location`
(`crates/game-core/src/engine/dispatch/hunters.rs:308-313`) documents the
contract this path breaks: *"callers are responsible for clearing (and
announcing) any existing engagement first."*

**Divergence scenario.** A Ghoul is engaged with Daisy; Roland, co-located,
spends an action to Engage it. The event stream shows one `EnemyEngaged { Ghoul,
Roland }` and nothing about Daisy. Any consumer keyed on disengagement — the web
client's board diff, a future "after an enemy disengages from you" reaction, a
replay reader reconstructing threat areas from events rather than state — sees
the Ghoul in two threat areas or in none.

**Classification: WRONG.** The disengagement concept, its event, and the
announce-before-reengage convention all exist; this one call site skips them.
Low severity, multiplayer-only, and state-correct — reported because it is a
one-line divergence from the engine's own documented contract.

### 9. Evading an already-exhausted enemy exhausts it again

**Rules.** `data/rules-reference/rules/glossary/Evade_Action.md`:

> - Any time an enemy is evaded (whether by an evade action, or by card
>   ability), the enemy is exhausted (**if it was ready**) and the engagement is
>   broken.

and `glossary/Exhaust.md`:

> - An exhausted card cannot exhaust again until it is ready (typically by a
>   game step or card ability).

**Code.** `crates/game-core/src/engine/dispatch/skill_test.rs:1212-1226`, the
Evade follow-up, is unconditional:

```rust
e.engaged_with = None;
e.exhausted = true;
cx.events.push(Event::EnemyDisengaged { enemy, investigator });
cx.events.push(Event::EnemyExhausted { enemy });
```

There is no ready check anywhere on the path: `validate_engaged_action`
(`actions.rs:651-673`) requires only that the enemy exists and is engaged with
the actor, which is correct — evading an exhausted enemy is legal — but the
follow-up then re-exhausts it.

**Divergence scenario.** An enemy exhausts after attacking during the Enemy
phase and stays engaged. Next round the investigator evades it and succeeds. The
engine emits `EnemyExhausted` for a card that was already exhausted. The
disengagement is right; the exhaust is a no-op on state but a spurious event —
so an "after an enemy exhausts" listener fires twice for one exhaustion.

**Classification: WRONG** (a missing condition — the rule's parenthetical "if it
was ready"). Low severity today: no corpus card listens for `EnemyExhausted`.

## Checked and found sound

Recorded so the next reader does not re-derive them. Each was checked against
the verbatim rule, not assumed.

- **The round is the right shape, and round 1 skips Mythos.** `Phase::next`
  (`state/phase.rs:31-38`) cycles Mythos → Investigation → Enemy → Upkeep, and
  `start_scenario` (`phases.rs:120-126`) opens directly in Investigation with no
  `PhaseStarted(Mythos)` emitted, honouring Appendix II's *"**During the first
  round of the game, skip the mythos phase.**"* The round counter increments at
  1.1 inside `mythos_phase` (`phases.rs:424`), which is where Appendix II puts
  the round boundary.
- **Every player window in the Appendix II chart exists, at the charted
  position.** Post-1.4 (`MythosAfterDraws`), post-2.1 (`InvestigationBegins`),
  post-2.2 (`InvestigatorTurnBegins`), the 3.2/3.3 window
  (`BeforeInvestigatorAttacked`), the post-3.3 window
  (`AfterAllInvestigatorsAttacked`), and post-4.1 (`UpkeepBegins`). No window is
  opened where the chart has none (1.1–1.3, 3.1, 4.2–4.6). The
  `investigation_phase` → window → `begin_investigator_turn` structure
  (`phases.rs:303-369`) preserves the printed 2.1 → window → 2.2 order rather
  than collapsing it.
- **The Mythos sub-steps run in printed order, including the agenda's reverse
  before the draws.** `mythos_phase` (`phases.rs:414-453`) parks the anchor at
  `Draws`, runs 1.2 (`place_doom_on_agenda`) then 1.3 (`check_doom_threshold`),
  and lets 1.4 run from the anchor's resume only once any `AdvanceReverse` frame
  has popped — so *"When the agenda deck advances … follow any advancement
  instructions"* resolves before *"each investigator draws 1 encounter card"*.
- **The 1.4 encounter sub-sequence follows the five printed steps.**
  `resolve_encounter_card` (`dispatch/encounter.rs:121-220`) emits
  `CardRevealed`, resolves the Revelation, and only then disposes of the card —
  treachery to the encounter discard, enemy spawned — with the reasoning quoted
  from Appendix II in the doc-comment. Surge restarts the chain. Peril is not
  modelled (already registered: forward-compatibility bucket 3, item 10).
- **Upkeep runs 4.2 → 4.3 → 4.4 → 4.5 → 4.6 with 4.4's two passes separated.**
  `upkeep_resume` (`phases.rs:316-341`); `upkeep_draw_and_resource`
  (`phases.rs:1382-1391`) draws for every investigator first and grants
  resources in a second pass, matching *"In player order, each investigator
  draws 1 card. Once those cards have been drawn, each investigator gains 1
  resource."*
- **4.3 readies every exhausted card in play, not just the actor's, and
  re-engages what readies.** `ready_exhausted_cards` (`phases.rs:1162-1208`)
  covers investigator in-play cards and enemies, then applies
  `Enemy_Engagement.md`'s *"if an exhausted enemy at the same location as an
  investigator becomes ready, it engages as soon as it is readied"* to the
  newly-readied set only. (The `investigator_card` / `threat_area` omission is
  Finding 3's rider, not a live bug.)
- **Hand size is 8, checked in player order, exact-count enforced.**
  `HAND_SIZE_LIMIT` (`phases.rs:506`), `over_cap_investigators`
  (`phases.rs:1209-1215`), and `resume_hand_size_discard`'s
  `indices.len() != target` rejection (`phases.rs:1284`) implement *"each
  investigator with more than 8 cards in hand chooses and discards cards from
  his or her hand until he or she has 8 cards remaining in hand."*
- **Three actions per turn, granted at 4.2, forfeited at 2.2.2.**
  `ACTIONS_PER_TURN = 3` (`phases.rs:21`); `reset_actions` (`phases.rs:1360-1380`)
  is the sole refresh site and runs at 4.2; `end_turn` (`phases.rs:192-198`)
  zeroes the remainder. That matches FAQ 1.31 in `glossary/Action.md`:
  *"Investigators gain actions during step 4.2 of the upkeep phase. Any unused
  actions are forfeited at the end of an investigator's turn (during step 2.2.2
  of the investigation phase)."* Round 1's seed in `start_scenario`
  (`phases.rs:148`) covers *"Investigators **always** begin the game with 3
  actions."*
- **The attack of opportunity fires at the right instant, from the right
  enemies, once each, on the right actions.** `drive_aoo`
  (`dispatch/combat.rs:533-547`) filters to `engaged_with == Some(actor) &&
  !exhausted` — the rule's *"engaged with one or more **ready** enemies"* — and
  attacks once per enemy, satisfying *"An ability that costs more than one
  action only provokes one attack of opportunity from each engaged enemy."*
  Every action-spending handler pushes its `ActionResolution` frame and drives
  the loop *between* paying and resolving: Investigate (`actions.rs:69-77`),
  Resource (`:156-164`), Engage (`:254-262`), Move (`:404-418`), Draw
  (`cards.rs:507-515`), non-fast PlayCard (`cards.rs:846-874`; `play_card`
  itself at `cards.rs:804`, with the cost
  paid first), and action-cost non-Fight abilities (`abilities.rs:105-120`).
  That is *"immediately after all costs of initiating the action that provoked
  the attack have been paid, but before the application of that action's effect
  upon the game state."* Fight and Evade never drive it; Fast plays and
  0-action abilities never drive it (*"Because fast cards do not cost actions to
  play, they do not provoke attacks of opportunity"*). The attacker does not
  exhaust (`EnemyAttackSource::AttackOfOpportunity`, pinned by a test citing the
  rule).
- **The eight action types, and their individual rules.** Draw
  (`cards.rs:497`), Resource (`actions.rs:146`), Move (`:323`), Investigate
  (`:35`), Fight (`:782`), Evade (`:846`), Engage (`:210`), Play
  (`cards.rs:804`), and Activate (`abilities.rs:64`) are all present and
  each enforces its own targeting rule: Move requires a printed connection
  (`actions.rs:396`) and forbids the current location (`:374`) per
  `glossary/Move.md`'s *"it cannot move to its same (current) placement"*;
  Investigate tests intellect against the location's effective shroud
  (`actions.rs:117-133`); Fight is co-location-gated and Evade is
  engagement-only, exactly as `Fight_Action.md` and `Evade_Action.md` split
  them; Engage rejects an enemy already engaged with the actor
  (`actions.rs:234`), per *"An investigator cannot use the engage action to
  engage an enemy he or she is already engaged with."*
- **Enemies engaged with a moving investigator move with them, and the entered
  location auto-engages.** `move_primary_effect` (`actions.rs:465-481`) and
  `engage_ready_enemies_on_enter` (`:564-577`) implement
  `Enemy_Engagement.md`'s *"should the investigator move, the enemy remains
  engaged and moves to the new location simultaneously"* and *"An investigator
  moves into the same location as it."* First entry reveals the location
  (`actions.rs:489`), per `glossary/Clues.md`.
- **Enemy phase 3.2 and 3.3.** `is_eligible_hunter` (`hunters.rs:131-136`)
  filters to *"each ready, unengaged enemy with the hunter keyword"*, and
  `process_one_hunter` (`:333-355`) skips movement when investigators are
  already present — *"Enemies at a location with one or more investigators do
  not move."* Ties go to the lead. 3.3 resolves per investigator in turn order,
  all of one investigator's enemies before the next, with a player-chosen order
  when 2+ are ready (`combat.rs:576-597`), matching *"resolve their attacks in
  the order of the attacked investigator's choosing"*; the enemy exhausts after
  the attack completes, and only for the Enemy-phase source.
- **Setup follows Appendix III's order and opens no windows.**
  `start_scenario` (`phases.rs:27-163`) seats investigators, places them at the
  starting location, grants 5 resources (step 7), deals 5 cards and sets aside
  weaknesses (step 8), shuffles the encounter deck, and runs the mulligan loop
  in player order — with no player window anywhere in it, per *"There are no
  action windows during setup."* The set-aside weaknesses are shuffled back into
  their owners' decks once every mulligan is done (`cards.rs:645-676`), per
  *"Upon completion of this step, shuffle each of these weakness cards back into
  its owner's deck."* (The redraw *within* a mulligan is Finding 6.)
- **Deck-out.** `draw_one_with_deckout` (`cards.rs:445-473`) reshuffles the
  discard, draws, then takes 1 horror — *"that investigator shuffles his or her
  discard pile back into his or her deck, then draws the card, and upon
  completion of the entire draw takes one horror."*
- **Advancing the act is free, not an action.** `advance_act_action`
  (`act_agenda.rs:192-206`) spends clues and never touches `actions_remaining`,
  matching `glossary/Clues.md`'s *"This is normally done as a [free] player
  ability."* It is gated to the Investigation phase, matching *"during any
  investigator's turn"*.
- **The agenda advances only at 1.3, or where a card says otherwise.**
  `check_doom_threshold` is called from `mythos_phase` step 1.3 and from the
  card-facing `place_doom_on_current_agenda` (`act_agenda.rs:81-84`), whose only
  consumer is Ancient Evils 01166 — whose printed text explicitly grants the
  exception the rule's note requires: *"Note: Unless a card otherwise specifies
  that it can advance the agenda, this is the only time at which the agenda can
  advance."*
- **Elimination steps 1–5.** `run_elimination_steps`
  (`elimination.rs:57-218`) removes controlled and owned cards (including a
  card mid-play, taken off its frame), deposits clues at the last location,
  returns resources, unengages enemies there, and disposes of scenario-owned
  threat-area cards — each step annotated with its rule. Eliminated
  investigators are skipped by every cursor (`dispatch/cursor.rs`), so they
  neither draw at 1.4, take turns at 2.2, resolve attacks at 3.3, nor draw at
  4.4. Step 0 is Finding 1; step 6's "no remaining players" is handled by the
  resolution latch.

## Uncertain

- **Whether an investigator with no actions left should have their turn end
  without an explicit input.** Appendix II 2.2.1 ends *"If the investigator does
  not or cannot take an action, proceed to 2.2.2."* The engine always requires
  an explicit `EndTurn` from the menu. This is arguably correct — the investigator
  can still play Fast cards and free abilities from the same menu, which is what
  the between-action player window is for — but the rules text reads as
  automatic. Settling it needs a decision about whether the open-turn menu *is*
  the 2.2.1 player window; the answer is entangled with
  [#146](https://github.com/talelburg/eldritch/issues/146), which is open and
  covers "2.2.1 between-action windows" explicitly. **Already registered** on
  that basis, and not counted as a finding.
- **Whether `PhaseEnded` should be reaction-visible.**
  `crates/card-dsl/src/dsl.rs:313-315` states the pattern is *"Matched only by
  the forced dispatch path … never by player reaction windows"*. Appendix II
  charts no player window at a phase-end step, which supports the restriction,
  but `glossary/Triggered_Abilities.md` says a `[reaction]` *"may be used any
  time its triggering condition is met"* rather than only in windows. No Chapter
  1 card prints a `[reaction]` on a phase end, so nothing forces the question. A
  card that did would settle it.
- **Who is prompted for a "lead investigator decides" suspension.**
  `Lead_Investigator.md` makes the lead the arbiter of ties, and the engine
  routes hunter-movement, spawn-engagement, and simultaneous-forced ordering to
  "the lead" — but `GameState` has no `lead_investigator` field; the lead is
  implicitly `turn_order[0]` and `InputRequest` carries no addressee. In solo
  this is indistinguishable from correct. Settling it needs the seat/identity
  work, which the 2026-08-14 audit places at
  [#581](https://github.com/talelburg/eldritch/issues/581). Not counted as a
  finding: no rule is currently contradicted, only unrepresented.
- **Additional actions.** `glossary/Action.md`'s FAQ 1.10 describes additional
  actions in detail, and the corpus's first consumer is a **seatable Core
  investigator**, not a distant player card — Daisy Walker 01002:
  *"You may take an additional action during your turn, which can only be used
  on [[Tome]] [action] abilities."* `ACTIONS_PER_TURN` is a flat `u8` and
  seating Daisy today silently drops her class feature. The 2026-08-14 audit
  registers this as bucket 3, item 6 ("Action economy beyond the fixed three"),
  counted there against 31 *player* cards; the correction worth carrying forward
  is that the first consumer is already installable, which is a different
  urgency than that entry implies. **Already registered**, urgency noted.
