# Rules-conformance pass — enemies, encounters, damage/horror, defeat and elimination — 2026-08-16

The full official Rules Reference was vendored verbatim at `data/rules-reference/rules/`
on 2026-08-15 (six section files plus 188 glossary entries, indexed by
[`rules/README.md`](../../data/rules-reference/rules/README.md)). Until that landed, the
only offline rules source was the 2016 Core-set PDF, and the previous audit closed with
the observation that it *"structurally cannot serve that role"* for Chapter 1
([2026-08-14 register](2026-08-14-chapter-1-forward-compatibility.md), "Uncertain"). This
pass is the first read of the engine against the real text. It covers **one** surface:
enemies and their keywords, encounter-card resolution, dealing and taking damage/horror,
and defeat/elimination.

**Scope.** In: enemy state and engagement, the Engage/Fight/Evade actions, attacks of
opportunity, the Enemy phase, Hunter/Prey/Retaliate/Aloof/Massive/Surge/Peril, encounter
draw and spawn, damage/horror assignment and soak, enemy and investigator defeat, and the
p.10 elimination sequence. Out: skill-test timing internals (ST.1–ST.8 beyond the
follow-up step), act/agenda machinery, the reaction-window and frame model as such (ADRs
0002–0004), card-play and cost payment, and every mechanic belonging to a cycle outside
Chapter 1. Findings are against the **compiled corpus** (Core + Dunwich), not the whole
snapshot — where a gap only bites for a snapshot-only card it is said so explicitly.

## Method

1. **Rules read in full**, all under `data/rules-reference/rules/`:
   `Appendix_II_Timing_and_Gameplay.md`, and the glossary entries `Enemy_Cards`,
   `Enemy_Engagement`, `Engage_Action`, `Fight_Action`, `Evade_Action`,
   `Attack_of_Opportunity`, `Attacker_Attacked`, `Enemy_Phase`, `Threat_Area`, `Hunter`,
   `Prey`, `Retaliate`, `Aloof`, `Massive`, `Nearest`, `Farthest_From_All_Investigators`,
   `Spawn`, `Keywords`, `Surge`, `Peril`, `Encounter_Deck`, `Encounter_Set`,
   `Encounter_Cards_Vs_Scenario_Cards`, `Revelation`, `Treachery_Cards`, `Mythos_Phase`,
   `Dealing_Damage_Horror`, `Taking_Damage_Horror`, `Direct_Damage_Direct_Horror`,
   `Health_and_Damage`, `Sanity_and_Horror`, `Heal`, `Defeat`, `Elimination`,
   `Killed_Insane_Investigators`, `Trauma`, `Resign`, `Winning_and_Losing`, `Immune`,
   `Cannot`, `Priority_of_Simultaneous_Resolution`, `Move`, `Location_Cards`,
   `Empty_Location`. Cross-references followed into `Ready`, `Exhaust`, `Upkeep_Phase`,
   `Investigation_Phase`, `In_Player_Order`, `Lead_Investigator`, `Parley`,
   `Drawing_Cards`, `Weakness`, `Enters_Play`, `Leaves_Play`, `Discard_Piles`,
   `In_Play_and_Out_of_Play`, `Per_Investigator`, `Automatic_Failure_Success`,
   `Skill_Tests`. No network fetch was made; every quote below is copied from the
   vendored file named beside it.
2. **Code read in full**: `engine/dispatch/combat.rs`, `engine/dispatch/hunters.rs`,
   `engine/dispatch/encounter.rs` (handler half; the test modules were skimmed for
   pinned behaviour), `engine/dispatch/elimination.rs`, `engine/pathfinding.rs`,
   `state/enemy.rs`, `state/investigator.rs`, and the enemy/damage-relevant parts of
   `event.rs`. Targeted reads into `engine/dispatch/actions.rs` (Engage / Move / Fight /
   Evade / auto-engage-on-enter), `engine/dispatch/skill_test.rs` (follow-up application,
   retaliate, `peril_check`), `engine/dispatch/phases.rs` (Upkeep 4.3, Enemy-phase
   cursor), `engine/dispatch/reaction_windows.rs` (Fight target eligibility),
   `engine/evaluator.rs` (`Heal`), and `crates/cards/src/impls/theyre_getting_out.rs`.
3. **Card text and rulings** from `data/arkhamdb-snapshot/pack/` and
   `data/arkhamdb-faq/`, both read locally. Corpus membership was confirmed by grepping
   the generated corpus `crates/cards/src/generated/cards.rs`, not assumed from the pack
   name.
4. **Cross-referenced before filing.** The 2026-08-14 forward-compatibility register and
   the 2026-07-17 repo audit were read; every candidate finding was checked against the
   tracker with `gh issue view` / `gh issue list`. Six candidates turned out to be
   already tracked and are recorded as such in [Already
   registered](#already-registered-not-re-reported) rather than restated as findings.
   ADRs 0001–0004 were checked; no finding here conflicts with one.
5. **Every finding was re-verified against the code after drafting.** Three drafted
   claims did not survive that re-read and were dropped: that Retaliate fires without a
   readiness check (it checks), that the Enemy phase resolves attacks outside player
   order (it uses `turn_order`), and that per-investigator enemy health omits eliminated
   investigators (it counts them, which is what the rule asks for).

## Findings

Ordered most-severe first.

### 1. A defeated enemy is removed from the game instead of placed in the encounter discard pile — WRONG

`data/rules-reference/rules/glossary/Defeat.md`:

> If an enemy has as much or more damage on it as it has health, that enemy is defeated
> and placed on the encounter discard pile (or on its owner's discard pile if it is a
> weakness).

`data/rules-reference/rules/glossary/Encounter_Deck.md`:

> If the encounter deck is empty, shuffle the encounter discard pile back into the
> encounter deck.

`crates/game-core/src/engine/dispatch/combat.rs:88-105` is the whole defeat body: it
emits `EnemyDefeated`, calls `cx.state.enemies.remove(&enemy_id)`, and pushes the code to
`state.victory_display` only when the enemy has a printed Victory value. Nothing pushes
the code to `state.encounter_discard`, and nothing routes a weakness enemy to its
bearer's discard. A grep of every `encounter_discard` writer in `game-core` confirms it:
the treachery disposal (`encounter.rs:877`), the unspawnable-`Specific`-location discard
(`encounter.rs:324`), the threat-area discard (`threat_area.rs:140`), and
`Effect::DiscardSelf` (`evaluator.rs:1012`, `:1058`) — no enemy path. The event
documentation asserts the divergence as if it were the rule
(`crates/game-core/src/event.rs:272-273`): *"Per the Rules Reference, defeat takes the
enemy out of play entirely"*. `In_Play_and_Out_of_Play.md` does place the discard pile
out of play, but out-of-play is not removed-from-game, and the two differ exactly where
`Encounter_Deck.md` reshuffles.

**Divergence scenario.** The Gathering, a Ghoul Minion (01160) is drawn and defeated;
later the encounter deck runs out mid-Mythos. Engine: `draw_encounter_top`
(`encounter.rs:577-585`) reshuffles a discard pile that never contained the Ghoul, so the
rebuilt deck is one card short and that Ghoul can never be drawn again. Rules: the Ghoul
was in the encounter discard and is shuffled back in.

**Second head, worse.** Mob Enforcer (01101), Silver Twilight Acolyte (01102) and
Stubborn Detective (01103) are enemy **weaknesses**, all three in the compiled corpus
(`crates/cards/src/generated/cards.rs:1130`, `:1141`, `:1152`, each with
`weakness: true`). `Defeat.md` sends them to their *owner's* discard, and
`glossary/Weakness.md` — *"that weakness remains a part of that investigator's deck for
the rest of the campaign"* — is what makes that load-bearing. The engine drops them
entirely, so a defeated basic weakness leaves the bearer's deck permanently.

Not tracked by any open issue (checked #572, #575, #579, #611 and the full tracker).

### 2. An enemy moved by a card effect never engages on arrival — WRONG

`data/rules-reference/rules/glossary/Enemy_Engagement.md`:

> Any time a ready unengaged enemy is at the same location as an investigator, it engages
> that investigator, and is placed in that investigator's threat area. […]
>
> *For example, a ready unengaged enemy immediately engages if:*
>
> - It spawns at the same location as an investigator,
> - **It moves into the same location as an investigator,**
> - An investigator moves into the same location as it.

The engine honours the rule at four triggers — spawn (`encounter.rs:428-478`), Hunter
arrival (`hunters.rs:272-291`), investigator entry (`actions.rs:564-577`), and Upkeep-4.3
readying (`phases.rs:1178-1197`) — and misses the second bullet. Agenda 01107 *They're
Getting Out!*, printed *"**Forced** - At the end of the enemy phase: Each unengaged
[[Ghoul]] enemy moves 1 location towards the Parlor."*
(`data/arkhamdb-snapshot/pack/core/core_encounter.json`), is implemented at
`crates/cards/src/impls/theyre_getting_out.rs:75-108`. Its mutation loop
(`:101-107`) sets `current_location` and pushes `Event::EnemyMoved` — it never calls
`hunters::reengage_at_location`, the shared primitive that exists for exactly this
(`hunters.rs:314-329`).

**Divergence scenario.** Solo. The investigator sits in the Hallway (01112). A Ghoul
Minion (01160 — no Hunter keyword) is one location away at the end of the Enemy phase.
Agenda 01107 moves it into the Hallway. Engine: it arrives ready and unengaged and stays
that way. Upkeep 4.3 only re-engages the enemies it *newly readied*
(`phases.rs:1178-1195`), and this one was never exhausted, so it is not in that set; the
investigator standing still triggers nothing else. The Ghoul is co-located with the
investigator indefinitely — it never attacks in step 3.3 (which filters on
`engaged_with`, `combat.rs:590-596`) and never makes an attack of opportunity
(`combat.rs:533-547`). Rules: it engages the moment it arrives and attacks the next Enemy
phase. Hunter Ghouls self-correct the following Enemy phase via `process_one_hunter`; the
non-Hunter Ghouls in the scenario (01160, 01161, 01118, 01119) do not.

The rule holds for exhausted movers too, and correctly so:
`data/arkhamdb-faq/core/01107.md` says *"This agenda can move exhausted (evaded)
enemies"*, and `Enemy_Engagement.md` says *"An exhausted unengaged enemy does not
engage"* — `reengage_at_location` already early-returns on `exhausted`
(`hunters.rs:315-318`), so the fix is one call, not a new predicate.

### 3. An enemy with no Spawn instruction prey-resolves its engagement instead of engaging the drawer — WRONG

`data/rules-reference/rules/glossary/Spawn.md`, the FAQ "Spawning an Enemy" quick
reference:

> - If the enemy does not have a "Spawn –" instruction, the investigator drawing the
>   enemy spawns it engaged with him/her, unless it is aloof.
>
> **"Prey –" instructions have no direct impact on which location an enemy will spawn
> at.** The only time "Prey –" instructions will impact this process is when an enemy
> spawns unengaged at a location with multiple investigators […]

`Enemy_Cards.md` agrees: *"If the encountered enemy has no spawn direction, the enemy
spawns engaged with the investigator encountering the card."*

`crates/game-core/src/engine/dispatch/encounter.rs:328-345` resolves the no-instruction
case to the *drawing investigator's location* and then hands off to `spawn_enemy_at`,
which builds its engagement candidates from the location rather than from the drawer
(`:399`) and runs them through `resolve_prey` (`:428`).

**Divergence scenario.** Two investigators, A and B, both in the Study. A draws a Ghoul
Minion (01160 — no Spawn line, `Prey::Default`). Engine: candidates are `[A, B]`,
`Prey::Default` ties, so it pushes a `SpawnEngage` frame and prompts the lead investigator
to pick whom the Ghoul engages (`:448-477`) — B is a legal answer. Rules: no choice
exists; the Ghoul spawns engaged with A, who drew it. With a `Ranked` prey the engine is
worse still: it would silently engage the higher-combat investigator with no prompt at
all. Solo play is unaffected (the singleton set resolves to the drawer either way), so
this is latent until multiplayer.

The `Specific`-spawn path is correct — an enemy spawning at a named location *does*
prey-resolve among the investigators there, exactly as the quoted FAQ paragraph says.

### 4. Barricade does not stop an engaged enemy being dragged into the barricaded location — WRONG

Barricade 01038, `data/arkhamdb-snapshot/pack/core/core.json`:

> Attach to your location.
> Non-[[Elite]] enemies cannot move into attached location.

`data/arkhamdb-faq/core/01038.md`:

> If an investigator that is engaged with an enemy moves to a Barricaded location, the
> engaged enemy will disengage and remain in the investigator's previous location (after
> making an attack of opportunity).

And `data/rules-reference/rules/glossary/Cannot.md`: *"The word 'cannot' is absolute, and
cannot be countermanded by other abilities."*

`crates/game-core/src/engine/dispatch/actions.rs:465-481` drags every engaged enemy to the
destination unconditionally:

```rust
for enemy_id in engaged {
    if let Some(enemy) = cx.state.enemies.get_mut(&enemy_id) {
        enemy.current_location = Some(destination);
    }
}
```

The engine already owns the predicate this needs —
`hunters::enemy_can_enter_location` (`hunters.rs:151-153`), used by Hunter movement and
by the 01107 native — and the move path simply doesn't consult it. Nor is there a
disengage: the ruling requires the engagement to break, which would emit
`Event::EnemyDisengaged`.

**Divergence scenario.** Barricade is attached to the Hallway. The investigator, engaged
with a Ghoul Minion in the Study, moves to the Hallway. Engine: the Ghoul makes its attack
of opportunity, then rides along into the Hallway still engaged — the card it just paid
for did nothing. Rules: the attack of opportunity resolves, then the Ghoul disengages and
stays in the Study.

Barricade 01038 is implemented (`crates/cards/src/impls/barricade.rs`, issue #323) and in
the corpus, so this is live, not latent.

### 5. Hunter pathfinding routes *around* a movement block instead of being stopped by it — WRONG

`data/rules-reference/rules/glossary/Nearest.md`:

> Nearest refers to the entity of the specified kind at a location that can be reached in
> the fewest number of connections, **even if one or more of those connections are
> blocked by another card ability**. The path to the nearest entity is the "shortest" path
> to that entity.

`data/rules-reference/rules/glossary/Hunter.md`:

> If a hunter enemy would be compelled to a location to which the move is blocked by a
> card ability, the enemy does not move.

Together: a block never changes *who is nearest* or *what the shortest path is*; it only
turns the compelled step into a non-move.

`crates/game-core/src/engine/dispatch/hunters.rs:183-240` does the opposite. It builds a
passability predicate from `enemy_can_enter_location` (`:191`) and threads it through
`bfs_distance_with` (`:204`) *and* `shortest_first_steps_with` (`:232`), so a barricaded
location is deleted from the graph before distances are measured. The comment at `:189-190`
states the intent plainly — *"A barricaded location is impassable to a non-Elite enemy —
graph-level, so it shifts which investigator is nearest, not just the final step"* — which
is precisely what `Nearest.md` forbids.

**Divergence scenario.** Hunter at the Hallway (01112). Investigator A is in the Attic
(01114), one connection away, and the Attic is barricaded. Investigator B is in the Cellar
(01113), two connections away, unblocked. Engine: the Attic is unreachable, so B is the
nearest investigator and the hunter moves one step toward the Cellar. Rules: A is nearest
(one connection, block notwithstanding), the hunter is compelled toward the Attic, the
move is blocked, and the hunter does not move at all.

The same graph-level treatment is copied into agenda 01107's Ghoul movement
(`crates/cards/src/impls/theyre_getting_out.rs:93-95`). That case is less clear-cut —
"moves 1 location towards the Parlor" names a fixed destination rather than a "nearest"
target — and is flagged under [Uncertain](#uncertain).

### 6. Massive cannot be represented by the engagement model — ABSENT (already registered as an ingestion gap; the state-shape conflict is new)

`data/rules-reference/rules/glossary/Massive.md`:

> A ready enemy with the massive keyword is considered to be engaged with each
> investigator at the same location as it.
>
> - An enemy with the massive keyword cannot be placed in an investigator's threat area.
> - When an enemy with the massive keyword attacks during the enemy phase, resolve its
>   (full) attack against each investigator it is engaged with, one investigator at a
>   time. […] The massive enemy does not exhaust until its final attack of the phase
>   resolves.
> - A massive enemy does not move with an engaged investigator who moves away from the
>   massive enemy's location.
> - If an investigator fails a combat test against a massive enemy, no damage is dealt to
>   the engaged investigators.

Engagement in the engine is `Enemy.engaged_with: Option<InvestigatorId>`
(`crates/game-core/src/state/enemy.rs:70`) — a single investigator, by construction, with
the field doc explaining that the whole design leans on it. Every consumer reads it that
way: the Enemy-phase attacker scan (`combat.rs:590-596`), the attack-of-opportunity scan
(`combat.rs:536-540`), the move drag-along (`actions.rs:465-481`), and the elimination
disengage (`elimination.rs:151-167`). Nothing carries a Massive bit — `CardKind::Enemy`
has `hunter`, `retaliate` and `prey` and no `massive`
(`crates/card-dsl/src/card_data.rs`), and the pipeline neither parses nor warns on it.

Five corpus enemies print Massive, all confirmed present in
`crates/cards/src/generated/cards.rs`: Umôrdhoth 01157 (`:1724`), The Experiment 02058,
Silas Bishop 02216 (`:4320`), Brood of Yog-Sothoth 02255 (`:4738`), Yog-Sothoth 02323.
Three of them are scenario bosses.

**Already registered** as an ingestion gap: #579 ("Aloof & Massive unparsed … Both
fundamentally change engagement") and the 2026-08-14 register, bucket 3 entry 10
("Unmodelled keywords — Massive (41), Aloof (68) …"). What the rules text adds is the
classification: this is not a missing metadata field with a keyword check hung off it.
`Option<InvestigatorId>` cannot express "engaged with each investigator here", so
supporting Massive means changing the engagement representation and every consumer above
— **ABSENT**, architectural, not a fix. That register also flagged the gap itself:
*"Bucket 3's keyword list assumes each keyword is a self-contained behaviour; Massive and
Aloof in particular interact with engagement in ways not checked against the rules."* They
have now been checked, and the assumption does not hold for Massive.

### 7. Aloof is unmodelled, and the engine's Fight-eligibility comment asserts the opposite of the rule — ABSENT + WRONG comment

`data/rules-reference/rules/glossary/Aloof.md`:

> Aloof is a keyword ability. An enemy with the aloof keyword does not automatically
> engage investigators at its location.
>
> - When an aloof enemy spawns, it spawns unengaged.
> - An investigator may use the engage action or a card ability to engage an aloof enemy.
> - **An investigator cannot attack an aloof enemy while that enemy is not engaged with an
>   investigator.**

With `Cannot.md` — *"absolute, and cannot be countermanded"* — the third bullet is a hard
prohibition, not a default.

The keyword is unmodelled, and the engine says so honestly in one place
(`crates/game-core/src/engine/dispatch/actions.rs:554-557`: *"The **Aloof** carve-out … is
not yet modeled … No Aloof enemy appears in a currently-implemented scenario"*) and
incorrectly in another. `crates/game-core/src/engine/dispatch/reaction_windows.rs:1941-1943`
documents the Fight target scope as:

> Scope is co-located, not engaged-only: per RR you choose an enemy at your location to
> attack and need not already be engaged (matches the basic Fight action — **an Aloof
> enemy**, or one engaged with another investigator in MP, **is a legal weapon target**).

That parenthetical is contradicted by the Aloof bullet above. The widening itself (#451,
`Fight_Action.md`: *"An investigator may fight any enemy at his or her location"*) is
correct — it is the Aloof example that is wrong, and the behaviour follows the comment:
both `fight` (`actions.rs:782-840`) and `fight_target_scope` (`combat.rs:20-22`) gate on
co-location alone.

Whippoorwill 02090 (*"Aloof. Hunter."*) is in the compiled corpus
(`crates/cards/src/generated/cards.rs:2967`), so the auto-engagement half is live the day
a Dunwich scenario ships it: `spawn_enemy_at`, `engage_on_arrival`,
`engage_ready_enemies_on_enter` and `reengage_at_location` would all engage it.
**Already registered** as an ingestion gap (#579); the *comment* asserting the opposite of
`Aloof.md` is new, and is the kind of thing that gets copied forward.

### 8. "Prey – Bearer only" ingests as `Prey::Default` — ABSENT

`data/rules-reference/rules/glossary/Prey.md`:

> If an enemy's prey instructions contain the word "only," that enemy only moves towards
> and engages that investigator (as if it were the only investigator in play), and ignores
> all other investigators while moving and engaging. Other investigators may use the
> engage action or card abilities to engage the enemy.

`parse_prey` (`crates/card-data-pipeline/src/main.rs:1108-1123`) matches five literal
clauses and falls through to `PreyParse::Unrecognized`; `prey_lit` (`:1126-1137`) emits
`Prey::Default` for it (with a warning at `:496`). The three Core enemy weaknesses that
print *"**Prey** - Bearer only."* therefore ship as `Prey::Default` —
`crates/cards/src/generated/cards.rs:1138` (Mob Enforcer 01101), `:1149` (Silver Twilight
Acolyte 01102), `:1160` (Stubborn Detective 01103), all three Hunters.

**Divergence scenario.** Two investigators, A and B, at the same location; A's Stubborn
Detective is one location away. Engine: `hunter_destinations` (`hunters.rs:183-240`)
treats A and B as equally valid nearest investigators, and `engage_on_arrival` ties, so
the lead investigator picks — the Detective can engage B. Rules: it moves toward and
engages A and nothing else, "as if it were the only investigator in play". (Stubborn
Detective's own text then blanks the *engaged* investigator's ability box, so the wrong
engagement mis-applies a second effect.)

This is ABSENT rather than WRONG on two counts: `Prey` has no shape for a non-comparative
instruction (it is `#[non_exhaustive]` precisely for this, and `resolve_prey` panics on
any variant it doesn't know, `hunters.rs:117-120`), and nothing in the engine models a
weakness's **bearer** — `Enemy` has no bearer field
(`crates/game-core/src/state/enemy.rs:30-89`) and `Weakness.md` defines the bearer as
*"the investigator who started the game with the weakness in his or her deck or play
area"*, which is not derivable from current state. Solo play is unaffected. Not covered by
#575 (spawn clauses) or #579 (Aloof/Massive).

### 9. Non-`Specific` spawn instructions silently fall back to "engaged with the drawer" — ABSENT

`data/rules-reference/rules/glossary/Spawn.md`:

> - If an enemy has no legal location to spawn at (for example, if its spawn instruction
>   directs it to a specific location that is not in play, or if no location in play
>   satisfies its "spawn" instruction), it does not spawn, and is discarded instead.
> - If an enemy's spawn instruction has multiple valid locations, the investigator
>   spawning that enemy decides among those locations.

`SpawnLocation` has exactly one variant, `Specific(String)`
(`crates/card-dsl/src/card_data.rs:141-144`), and `parse_spawn_name`
(`crates/card-data-pipeline/src/main.rs:1140-1144`) resolves the clause against printed
location codes; anything unresolvable warns and emits `spawn: None`
(`main.rs:476-491`). `spawn: None` is not "unmodelled" to the engine — it is the
*positive* rule "no spawn instruction", which `spawn_enemy` (`encounter.rs:328-345`)
implements as "spawn at the drawing investigator's location".

**Divergence scenario.** Acolyte 01169, printed *"**Spawn** - Any empty location."*
(`core_encounter.json`), with `Empty_Location.md` defining that as *"a location with no
enemies or investigators at it"*. Engine: no location code resolves, so the corpus carries
`spawn: None` and the Acolyte spawns at the drawing investigator's location — the one
location guaranteed *not* to be empty — and immediately engages them. Rules: the drawing
investigator chooses among the empty locations, and the Acolyte spawns there unengaged.
Wizard of the Order 01170 (same clause) and Servant of Many Mouths 02224 are the other
corpus instances; the snapshot adds "Farthest location from you", "Nearest [[Altered]]
location", "Location with the most clues" and "Engaged with Prey" (01121b, The Masked
Hunter).

The classification is ABSENT — a new `SpawnLocation` shape plus a spawning-investigator
choice point. The *sharp* part is the fallback: everywhere else the engine refuses loudly
on unmodelled content (`PlayCard` on an unimplemented card rejects; `resolve_prey` panics
on an unknown variant), but here an unparsed spawn clause degrades into a different, valid
rule and plays on silently. #575 registers the adjacent parser bug (Ruth Turner 01141's
clause cut at the first period) and says *"genuinely-unmodeled forms still warn"*; the
fallback's *semantics* are not registered anywhere.

### 10. The Engage action steals an enemy without emitting a disengage — WRONG

`data/rules-reference/rules/glossary/Engage_Action.md`:

> An investigator may perform the engage action to engage an enemy that is engaged with a
> different investigator at the same location. The enemy **simultaneously disengages from
> the previous investigator** and engages the investigator performing the action.

`engage_primary_effect` (`crates/game-core/src/engine/dispatch/actions.rs:296-304`)
overwrites `engaged_with` and pushes only `Event::EnemyEngaged`. `Event::EnemyDisengaged`
exists and is emitted on evade (`skill_test.rs:1221`) and on elimination
(`elimination.rs:163`) — the steal path is the one place that mutates an existing
engagement without announcing it. `hunters::engage_enemy_with` documents the contract
correctly (`hunters.rs:305-313`: *"callers are responsible for clearing (and announcing)
any existing engagement first"*); this caller doesn't.

**Divergence scenario.** Multiplayer. A Ghoul is engaged with A; B spends an action to
engage it. Engine: state ends up right, but the event stream shows only `EnemyEngaged{B}`.
Any consumer reconstructing engagement from events — the web client, and any future
"after an enemy disengages from you" reaction — sees the Ghoul engaged with two
investigators. Low severity today (single-player, and no card in the corpus reacts to
disengagement); it is an event-fidelity bug that a reaction trigger would promote.

### 11. Evading an already-exhausted enemy emits a spurious exhaust — WRONG

`data/rules-reference/rules/glossary/Evade_Action.md`:

> Any time an enemy is evaded (whether by an evade action, or by card ability), the enemy
> is exhausted **(if it was ready)** and the engagement is broken.

`data/rules-reference/rules/glossary/Exhaust.md`: *"An exhausted card cannot exhaust again
until it is ready."*

The Evade follow-up (`crates/game-core/src/engine/dispatch/skill_test.rs:1211-1225`) sets
`e.exhausted = true` and pushes `Event::EnemyExhausted` unconditionally. The state is
idempotent, so only the event is wrong — but the situation is reachable: the Evade action
is gated on engagement, not readiness (`validate_engaged_action`,
`actions.rs:651-673`), and an enemy can be engaged *and* exhausted (it exhausts after
attacking in step 3.3, and a card that engages an exhausted enemy leaves it exhausted).
A player who evades such an enemy to break the engagement gets an `EnemyExhausted` for a
card that never exhausted. Low severity; same class as finding 10.

### 12. The attacker exhausts before the attack's triggered abilities resolve — WRONG (known, undocumented as a gap)

`data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`, framework step 3.3:

> When an enemy attacks, deal its attack (both its damage and its horror, simultaneously)
> to the engaged investigator. **Upon completion of dealing the attack (and all abilities
> triggered by the attack), exhaust the enemy.**

`place_queue_exhaust` (`crates/game-core/src/engine/dispatch/combat.rs:670-694`) queues
the per-soaked-asset reaction window first and then exhausts, so `EnemyExhausted` is
pushed before the window is ever opened — the code says so at `:672-675` and the
`drive_attack_loop` doc calls the ordering out explicitly (`:634-636`: *"the
deferred-exhaust-until-after-reactions RR nuance is out of scope"*).

**Divergence scenario.** An enemy attacks an investigator controlling Guard Dog (01021).
Engine: `EnemyExhausted` fires, then Guard Dog's *"Deal 1 damage to the attacking enemy"*
reaction opens. Rules: the reaction is an ability triggered by the attack, so it resolves
first and the enemy exhausts afterwards. No corpus card currently reads the attacker's
ready state inside that window, so this is presently cosmetic in the event stream — but it
is a genuine ordering divergence and it is not on the tracker, only in a code comment.

## Checked and found sound

Each of these was checked against the quoted rule and the code, and holds.

- **Simultaneous placement before defeat checks** (`Dealing_Damage_Horror.md` step 2:
  *"Any assigned damage/horror that has not been prevented is now placed on each card to
  which it has been assigned, simultaneously"*). `place_assignment`
  (`combat.rs:299-343`) accumulates on assets, then places *both* the investigator's
  damage and horror via the numeric helpers before testing either threshold
  (`:317-326`), then sweeps defeated assets. The split of `apply_damage_numeric` /
  `apply_horror_numeric` from the defeat step exists for exactly this reason
  (`:345-361`).
- **Elimination pre-empts a co-overflowing asset's discard.** `Elimination.md` step 1
  removes controlled cards *from the game*; `defeat_overflowed_assets` runs after
  investigator defeat, so a co-overflowing asset is already gone and emits no discard
  (`combat.rs:230-241`). The ordering is deliberate and documented.
- **Soak eligibility and capacity.** *"To be eligible, an asset card must have health in
  order to be assigned damage, and it must have sanity in order to be assigned horror"*
  and *"An asset cannot be assigned damage beyond the amount … to defeat the card"* —
  `build_soakers` (`combat.rs:496-522`) derives per-type remaining capacity from printed
  health/sanity, independently for damage and horror, and `eligible_targets`
  (`:786-817`) never offers a soaker past capacity. The investigator is the uncapped
  mandatory remainder, matching *"All damage/horror that cannot be assigned to an asset
  must be assigned to the investigator."*
- **The defending player assigns each point.** `soak_and_distribute` /
  `advance_distribution` (`combat.rs:460-482`, `:826-851`) prompt whenever a point has
  more than one eligible target and auto-assign only when the investigator is the sole
  option — the player-choice reading of `Dealing_Damage_Horror.md` step 1.
- **Attack-of-opportunity trigger set.** `Attack_of_Opportunity.md` — *"takes an action
  other than to **fight**, to **evade**, or to activate a **parley** or **resign**
  ability"*. `drive_aoo` is called from Investigate, Resource, Engage, Move, Play, and
  activated abilities, and not from Fight or Evade (`actions.rs:76`, `:163`, `:261`,
  `:417`; `cards.rs:514`, `:873`; `abilities.rs:119`). Engage provoking is correct — it
  is not on the exempt list.
- **AoO ordering and cost timing.** *"immediately after all costs of initiating the action
  have been paid, but before the application of that action's effect"* and *"An ability
  that costs more than one action only provokes one attack of opportunity from each
  engaged enemy."* Every call site charges the action, pushes an `ActionResolution` frame,
  then drives one AoO loop; the primary effect runs on resume (`actions.rs:404-418` is the
  canonical shape). *"in the order of the investigator's choosing"* is honoured by the
  order pick at `combat.rs:1179-1181`.
- **AoO attackers do not exhaust; retaliate attackers do not exhaust.**
  `Attack_of_Opportunity.md` (*"An enemy does not exhaust while making an attack of
  opportunity"*) and `Retaliate.md` (*"An enemy does not exhaust after performing a
  retaliate attack"*) — `place_queue_exhaust` gates exhaustion on
  `EnemyAttackSource::EnemyPhase` (`combat.rs:685-694`).
- **Retaliate conditions.** *"Each time an investigator fails a skill test while attacking
  a ready enemy with the retaliate keyword, after applying all results for that skill
  test"* and *"This attack occurs whether the enemy is engaged with the attacking
  investigator or not."* `fire_retaliate_if_any` (`skill_test.rs:1257-1281`) requires
  failure, a `Fight` follow-up, and `!exhausted`; it does **not** require engagement; and
  it runs at the `PostRetaliate` step, after the rest of ST.7.
- **Enemy-phase step 3.3 order.** *"Resolve engaged enemy attacks in player order, with
  each player resolving all of his or her engaged enemies before advancing to the next
  player"* and *"resolve their attacks in the order of the attacked investigator's
  choosing."* The per-investigator cursor walks `turn_order`
  (`phases.rs:521-553`), and within an investigator the attacker list is player-ordered
  via `suspend_order_pick` (`combat.rs:1133-1155`). Only ready engaged enemies attack
  (`:590-596`).
- **Hunter eligibility and the no-move case.** *"each ready, unengaged enemy with the
  hunter keyword … Enemies at a location with one or more investigators do not move."*
  `is_eligible_hunter` (`hunters.rs:131-136`) and `process_one_hunter`
  (`hunters.rs:333-355`), which skips movement when an active investigator shares the
  location but still runs engage-on-arrival.
- **Prey resolution uses modified values and ties fall to the lead.** `Prey.md` and
  `Lead_Investigator.md`. `measure_value` (`hunters.rs:42-61`) folds unconditional
  constant modifiers into the base stat and floors at zero; `resolve_prey` returns
  `Tie` for the lead to break, and hunter/spawn ties genuinely suspend for a pick.
- **Upkeep 4.3 readying then engaging.** *"Simultaneously ready each exhausted card"*
  plus `Enemy_Engagement.md`'s *"if an exhausted enemy at the same location as an
  investigator becomes ready, it engages as soon as it is readied."* `phases.rs:1178-1197`
  readies first, then re-engages only the newly-readied set.
- **Elimination steps 1–6.** `Elimination.md` read against
  `run_elimination_steps` (`elimination.rs:57-218`): controlled and owned cards removed
  from the game including a card mid-play (step 1), clues to the last location and
  resources returned (step 2), engaged enemies disengaged *then* offered to co-located
  survivors per prey (step 3), remaining threat-area cards to the encounter discard
  (step 4), lead recomputed rather than stored (step 5), and the no-remaining-players loss
  latched via `check_all_defeated` (step 6, `:288-311`). The owned-weakness /
  scenario-owned partition at `:90-119` is the right axis and cites the right clause.
- **Enemy defeat feeds the victory display.** `damage_enemy` (`combat.rs:99-105`) captures
  the code and victory value before removal. (This is the one thing the defeat path does
  do with the card — see finding 1 for what it doesn't.)
- **"Per investigator" counts eliminated investigators.** `Per_Investigator.md`: *"If
  investigators have been eliminated from the scenario, they still count toward 'per
  investigator' values."* `spawn_enemy_at` scales `HealthValue::PerInvestigator` by
  `cx.state.investigators.len()` (`encounter.rs:386-393`), and eliminated investigators
  remain in that map (`event.rs:293-295`).
- **Encounter draw order and the eliminated-drawer skip.** Framework step 1.4 draws *"In
  player order"*; `Elimination.md` limits an eliminated investigator's interaction to per
  investigator values. `advance_encounter_draw` (`encounter.rs:787-831`) walks the queue in
  order and skips non-`Active` drawers.
- **Revelation resolves before the card enters play / is discarded.** `Revelation.md` and
  framework step 1.4's ordered steps 3→4. `resolve_encounter_card`
  (`encounter.rs:196-215`) pushes the disposition frame *under* the Revelation effects, so
  an enemy's Revelation resolves before the spawn and a treachery's before the discard.
- **An enemy with no legal `Specific` spawn location is discarded, not spawned.**
  `Spawn.md`: *"it does not spawn, and is discarded instead."* `encounter.rs:307-326`
  (#517).
- **Deck reshuffle on empty.** `Encounter_Deck.md`. `draw_encounter_top`
  (`encounter.rs:577-585`).
- **Heal removes at most what is there.** `Heal.md`: *"If a card is healed for more damage
  or horror than it currently has on it, remove as much of the indicated amount as
  possible."* `heal_effect` (`evaluator.rs:1538-1574`) takes `min(current, count)` and
  emits only when something was healed.
- **The eliminated investigator's location is cleared after the clue deposit**, so step 2
  uses the location they were at "when eliminated" (`elimination.rs:59-64`, `:208-217`).

## Already registered (not re-reported)

Checked against the tracker and the two prior audits; each is a real divergence that
someone has already written down.

- **Failed Fight against an enemy engaged with another single investigator does not
  redirect the damage** (`Fight_Action.md`: *"the damage of the attack is dealt to the
  investigator engaged with that enemy"*). There is no failure branch in
  `apply_skill_test_follow_up` at all (`skill_test.rs:1149-1226`). Tracked as **#409**.
- **Surge and Peril are hardcoded `false` in the generated corpus** — the pipeline emits
  `surge: false, peril: false` for every enemy and treachery
  (`card-data-pipeline/src/main.rs:949`, `:964`), so `metadata.surge()` is never true and
  the surge chain (`Continuation::PlayerDraw.surge_pending`, `MAX_SURGE_CHAIN`) is
  unreachable, and Hunting Shadow 01135 / Offer of Power 01178 lose Peril. Tracked as
  **#138** (parsing) and **#139** (Peril enforcement); `peril_check` is an explicit no-op
  stub (`skill_test.rs:1449-1460`). Worth noting that the machinery on the engine side is
  built and correct — `Surge.md`'s "draw another card" chain is faithfully modelled — and
  waiting only on ingestion.
- **Aloof and Massive unparsed** — **#579**. See findings 6 and 7 for what the rules text
  adds beyond the ingestion framing.
- **Spawn clause cut at the first period** (Ruth Turner 01141) — **#575**. Finding 9 is
  about the fallback's semantics, not the parser.
- **Enemy `fight`/`evade` printed "—" ingested as 0** — **#574** (Yog-Sothoth 02323
  becomes auto-evadable, the inverse of *"Yog-Sothoth cannot be evaded"*).
- **Hunter move order is fixed ascending `EnemyId` with no lead-investigator choice** —
  **#572**, third checkbox. `Lead_Investigator.md` makes the lead *"the final arbiter"*
  among multiple valid options, and `drive_hunter_moves` (`hunters.rs:373-382`) fixes the
  order instead.
- **Multiplayer prey ties on re-engagement auto-pick the lead instead of prompting** —
  **#151**, and documented in place (`hunters.rs:296-304`).
- **Suspension / elimination interleavings** were named a weak seam by the 2026-07-17
  audit; the pieces it filed there (#564, #565, #566, #567, #568, #569) are closed or
  tracked. Nothing in this pass contradicts that record — finding 2 is an engagement
  trigger, not a suspension interleaving.

## Uncertain

- **A card-effect enemy move toward a *fixed* destination, when the compelled step is
  blocked.** `Nearest.md` and `Hunter.md` together settle the Hunter case (finding 5), but
  agenda 01107 says *"moves 1 location towards the Parlor"* — a named destination, not a
  "nearest" target. Whether a blocked shortest step means "do not move" (by analogy with
  the Hunter clause) or "take a different step that still reduces the distance" is not
  settled by any entry I read; `data/arkhamdb-faq/core/01107.md` and
  `data/arkhamdb-faq/core/01038.md` are silent on it. The engine currently reroutes
  (`theyre_getting_out.rs:93-95`). **What would settle it:** an ArkhamDB ruling on 01107
  or 01038 that names the interaction, or an FFG FAQ entry on blocked movement toward a
  named location — neither is in the vendored data today.
- **Simultaneous lethal damage *and* lethal horror.** `place_assignment`
  (`combat.rs:319-326`) picks `DefeatCause::Damage` when both cross in the same placement.
  `Defeat.md` assigns physical trauma to a damage defeat and mental trauma to a horror
  defeat but does not say which applies when both thresholds are crossed simultaneously,
  and `Priority_of_Simultaneous_Resolution.md` covers abilities and lasting effects rather
  than this. Moot while campaign trauma is unmodelled (the 2026-08-14 register, bucket 4
  entry 6), but the `DefeatCause` on the event is already observable. **What would settle
  it:** an FFG ruling on simultaneous defeat causes.
- **"If it is that investigator's turn, the turn ends."** Issue #572's second checkbox
  attributes this clause to "RR p.10", but it does not appear in the vendored
  `Elimination.md`, whose steps run 0–6 with no turn-end step, nor in
  `Appendix_II_Timing_and_Gameplay.md`'s 2.2.2. The underlying behaviour the issue
  describes (a defeated active investigator keeps being prompted) is a real bug either
  way; the *citation* is unverifiable against the vendored text. **What would settle it:**
  locating the clause in the FFG FAQ (the vendored reference folds in FAQ additions, so
  its absence is evidence, not proof), or restating #572's item from `Elimination.md` step
  1 instead.
- **`Status::Killed` / `Status::Insane` on a scenario defeat.**
  `Killed_Insane_Investigators.md` reserves those words for campaign play — *"An
  investigator with physical trauma equal to or higher than his or her printed health is
  killed"* — and adds *"When playing a standalone scenario, there is no practical
  difference between being killed, driven insane, or defeated."* So the engine's naming
  (`state/investigator.rs:194-205`) is behaviourally harmless today and misleading the day
  campaign play lands, when "killed" becomes a distinct, stickier state than "defeated".
  Not filed as a finding because it is a naming question with no current behavioural
  divergence; `CONTEXT.md` has no entry for either term. **What would settle it:** a
  domain-glossary decision, not a rules lookup.
