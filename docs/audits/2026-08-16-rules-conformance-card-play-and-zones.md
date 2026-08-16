# Rules-conformance pass — card play, costs, zones, and the cardtype framework — 2026-08-16

The full official rules reference is now vendored verbatim at
`data/rules-reference/rules/` (6 section files plus 188 glossary entries,
indexed by `data/rules-reference/rules/README.md`). This pass reads the rules
that govern **one surface** — playing a card from hand, what it costs, which
zone it ends up in, and the framework obligations each cardtype carries — and
checks the engine against them, quoting the load-bearing clause verbatim in
every case.

**Scope.** Everything from a card being declared out of hand to it being placed
(RR Appendix I, `Play`, `Play_Action`, `Fast`, `Costs`, `Limbo`), the zone
model (`In_Play_and_Out_of_Play`, `Enters_Play`, `Leaves_Play`, `Threat_Area`,
`Attach_To`, `Discard_Piles`, `Set_Aside`, `Removed_from_Game`), the cardtype
entries and their framework obligations (`Asset_Cards`, `Event_Cards`,
`Skill_Cards`, `Treachery_Cards`, `Enemy_Cards`, `Location_Cards`, `Weakness`,
`Signature_Cards`), and the card-property vocabulary the play path reads
(`Slots`, `Uses`, `Unique`, `Copy`, `Per_Investigator`, `Ownership_and_Control`,
`Resources`, `Clues`, `Doom`, `Drawing_Cards`, `Hand_Size`,
`Act_Deck_and_Agenda_Deck`, `Victory_Display_Victory_Points`).

**Out of scope, deliberately.** Skill-test resolution, combat, enemy movement
and engagement, the phase framework, encounter-draw sequencing, elimination,
and deckbuilding — each is its own surface. Mechanics belonging to cycles
outside Chapter 1 were skipped unless the engine actively does something
incompatible with them; none does, on this surface.

## Method

1. **Rules first, verbatim.** Read in full, from `data/rules-reference/rules/`:
   `Appendix_I_Initiation_Sequence.md`, `Appendix_IV_Card_Anatomy.md`, and the
   ~55 glossary entries named under Scope above, plus everything they
   cross-reference into (`Surge`, `Revelation`, `Blank`, `Permanent`, `Cannot`,
   `Immune`, `Exhaust`, `Ready`, `Limits_and_Maximums`, `Search`, `Tokens`,
   `Encounter_Cards_Vs_Scenario_Cards`). No network fetches; no rule is
   paraphrased from memory anywhere below.
2. **Code, read rather than grepped.** `engine/dispatch/cards.rs` (1,169 lines)
   in full; `engine/dispatch/slots.rs`, `engine/dispatch/threat_area.rs`,
   `engine/dispatch/act_agenda.rs`, `state/investigator.rs`,
   `state/card.rs` and `card-dsl/src/card_data.rs` in full;
   `check_play_card` and its helpers in `engine/dispatch/reaction_windows.rs`;
   the `GameState` field set and the zone-bearing helpers in
   `state/game_state.rs`; the play/draw/attach/discard arms of
   `engine/evaluator.rs`; the upkeep hand-size block and the commit-cap block in
   `engine/dispatch/phases.rs` / `skill_test.rs`; `event.rs`'s full variant list;
   and `render_kind` in `card-data-pipeline/src/main.rs`.
3. **Card text and rulings from the vendored sources only.** Every card quoted
   below was read from `data/arkhamdb-snapshot/pack/`; every ruling cited from
   `data/arkhamdb-faq/<pack>/<code>.md`. The corpus was cross-checked by script
   against `PACK_FILES` so "reachable in the shipping corpus" is a fact about
   what the build compiles, not a guess.
4. **Cross-referenced before writing.** ADRs 0001–0004 read in full;
   `docs/audits/2026-08-14-chapter-1-forward-compatibility.md` read in full;
   every candidate finding checked against the issue tracker via `gh` before it
   was written down. Four candidates turned out to be tracked already and were
   demoted to [Already tracked](#already-tracked); they are listed rather than
   dropped, because the rules quote that pins each is new information even where
   the gap is not.

## Verdict

**The play path is in good shape.** The Initiation Sequence's shape is real in
the code — restrictions checked, cost established payable, action spent, cost
paid, card carried in a zone-less limbo on its own frame, placed at step 4 —
and RR's most easily-missed clauses on this surface (Fast skips the *action*
cost but not the resource cost; Fast provokes no attack of opportunity; an
event may not be played if its effect cannot change the game state; a full slot
does not block a play but forces a discard; a played event's costs are paid
before the AoO) are each enforced at the right site with the right citation.

Six divergences. **One is live in the shipping corpus** (finding 1 — four Core
skill cards draw a card and the engine skips the empty-deck rule); the rest are
narrower, and two of the six have no reachable consumer in Core + Dunwich
today. Four further gaps this pass would otherwise have reported are already
tracked, including the two with the largest gameplay impact (acts not scaling
per-investigator, #573; `is_unique` not ingested, #579).

Counts: **4 WRONG**, **2 ABSENT**, **0 ADR CONFLICT**.

---

## Findings

### 1 — A card-effect draw skips the empty-deck rule entirely

**WRONG.** Live in the shipping corpus.

`data/rules-reference/rules/glossary/Drawing_Cards.md`:

> - When a player draws two or more cards as the result of a single ability or game step, those cards are drawn simultaneously. If a deck empties middraw, reset the deck and complete the draw.
> - There is no limit to the number of cards a player may draw each round.
> - If an investigator with an empty investigator deck needs to draw a card, that investigator shuffles his or her discard pile back into his or her deck, then draws the card, and upon completion of the entire draw takes one horror.

The engine has exactly this logic — in `draw_one_with_deckout`
(`crates/game-core/src/engine/dispatch/cards.rs:445`), which reshuffles the
discard, draws, and applies the horror. The **Draw action** and **Upkeep step
4.4** call it. `Effect::DrawCards` does not:

- `crates/game-core/src/engine/evaluator.rs:633` —
  `crate::engine::dispatch::cards::draw_cards(cx, target_id, count);`
- `crates/game-core/src/engine/dispatch/cards.rs:259` — `draw_cards` is
  documented as *"just the structural move; reshuffle / horror penalty logic for
  an empty deck lives in [`draw`]"*. It takes `min(count, deck.len())`, emits
  `CardsDrawn { count: drawn }` — possibly zero — and returns.

**Divergence scenario.** Guts 01089's printed text, verbatim from
`data/arkhamdb-snapshot/pack/core/core.json`:

> Max 1 committed per skill test.
> If this test is successful, draw 1 card.

An investigator with an empty deck and a non-empty discard commits Guts to a
Willpower test and succeeds. *Engine:* `CardsDrawn { count: 0 }`, no reshuffle,
no card, no horror. *Rules:* shuffle the discard back into the deck, draw the
card, take 1 horror. The same applies to Perception 01090, Overpower 01091 and
Manual Dexterity 01092 — all four print the identical clause, all four are
implemented (`crates/cards/src/impls/{guts,perception,overpower,manual_dexterity}.rs`),
and all four route through `draw_cards(InvestigatorTarget::You, 1)`. A skill
committed late in a scenario is exactly when the deck is empty, so this is not a
corner.

The multi-card half of the rule (*"If a deck empties middraw, reset the deck and
complete the draw"*) has no consumer in the corpus — every implemented draw is
`count: 1` — but the same call site would under-draw silently if one landed.

No ruling exists for any of the four cards on this point; their FAQ files
(`data/arkhamdb-faq/core/0108{9,…}.md`) cover only the "Max 1 per skill test"
scope and who draws when committing to another player's test.

Not tracked: `gh issue list` surfaces #429 (interactive soak *for* the
deck-out horror) and #310 (closed — the `Effect::DrawCards` primitive itself),
neither of which is this.

### 2 — The card leaves hand and is announced *before* its attack of opportunity; the rules put both after it

**WRONG.** Ordering only; no divergent outcome in the current corpus.

`data/rules-reference/rules/Appendix_I_Initiation_Sequence.md`, steps 2–3:

> 2. Pay the cost(s). If this step is reached and the cost(s) cannot be paid, abort this process without paying any costs.
>    - Upon completion of this step, attacks of opportunity, if applicable, resolve.
> 3. The card commences being played, or the effects of the ability attempt to initiate.

`data/rules-reference/rules/glossary/Limbo.md` states where the card is during
that gap, and dates the transition precisely:

> An event card enters limbo during step 3 of the Initiation Sequence, after costs are paid and attacks of opportunity are made *(see "[Appendix I: Initiation Sequence](../Appendix_I_Initiation_Sequence.md)")*.

So the ordering the rules give is: pay → **AoO** → card leaves hand into limbo.
The engine's is: pay → **card leaves hand into limbo** → AoO.

- `crates/game-core/src/engine/dispatch/cards.rs:857` — `let card =
  commence_play(cx, investigator, idx);` which emits `Event::CardPlayed` and
  `hand.remove(hand_index)` (`:1037`).
- `crates/game-core/src/engine/dispatch/cards.rs:873` — `return
  super::combat::drive_aoo(cx, investigator);`, only afterwards.

**Divergence scenario.** An investigator engaged with a ready Ghoul, holding
Dynamite Blast 01024 at hand index 2 and Dodge 01023 at index 3, takes the Play
action on Dynamite Blast. *Engine:* `CardPlayed { code: "01024" }` is emitted
and the hand collapses to 4 cards before the Ghoul's attack of opportunity
resolves — so during the AoO's Dodge window, Dodge is at index 2 and any
"cards in your hand" reading is one lower. *Rules:* Dynamite Blast is still in
hand throughout the AoO and enters limbo only once the attack has resolved, and
the card is not yet "regarded as played" (that is step 4).

Nothing in the corpus reads hand size mid-play and no `EventPattern` keys off a
card being played, so this changes no outcome today; the observable divergence
is confined to the event stream's order and the hand indices offered inside the
AoO window. Recorded because the ordering is cheap to correct now and expensive
once a card counts hand size.

The in-code justification (`cards.rs:844-845`, *"RR p.5 / the Dynamite Blast FAQ
('spend an action and pay the cost, then … attack of opportunity')"*) is
accurate as far as it goes — `data/arkhamdb-faq/core/01024.md` says exactly
that — but the FAQ is silent on where the card is while that happens, and
`Limbo` is not. **This is not an ADR conflict:** ADR 0002 decides *where* an
in-progress play lives (its frame, not a global slot), not *when* it gets
there, and its own framing quotes the same Appendix I steps.

### 3 — "Enters play" is a framework timing point, but only one of the three enter-play paths emits it

**WRONG.** No reachable consumer in Core + Dunwich.

`data/rules-reference/rules/glossary/Enters_Play.md`:

> The phrase "enters play" refers to any time a card makes a transition from an out-of-play area into a play area (see "[In Play and Out of Play](In_Play_and_Out_of_Play.md)" on page 13).

and `In_Play_and_Out_of_Play.md` says which areas those are:

> The cards that a player controls in his or her play area are considered in play.
>
> The current act, the current agenda, each location in the play area, and each encounter card in a investigator's threat area or at a location, are all considered in play.

The engine has three enter-play paths and emits the `EnteredPlay` timing event
from one:

- `crates/game-core/src/engine/dispatch/cards.rs:887` `enter_asset_into_play` —
  emits `TimingEvent::EnteredPlay { instance, controller }`. ✅
- `crates/game-core/src/engine/dispatch/threat_area.rs:51` `place_in_threat_area`
  — emits only `Event::CardEnteredThreatArea`. ❌
- `crates/game-core/src/engine/dispatch/threat_area.rs:89` `attach_to_location`
  — emits only `Event::CardAttachedToLocation`. ❌

Both of the latter two put a card into an area the rules call in play, and both
mint it through the very same `new_in_play_instance` helper (`:23`).

**Divergence scenario.** Barricade 01038 is played and resolves
`Effect::AttachSelfToLocation`; Cover Up 01007 and Frozen in Fear 01164 resolve
`PutIntoThreatArea`. In all three cases the card transitions from out of play to
in play, and no `EnteredPlay` timing point fires — so a card whose ability reads
"after X enters play" would never see them.

`EventPattern::EnteredPlay` is documented as self-referential and narrow
(`crates/card-dsl/src/dsl.rs:401-407`: *"Bare and **self-referential** — the
engine fires it only for the just-entered instance … A general 'after any card
enters play' reaction is out of scope"*), and its only consumer is Research
Librarian 01032, an asset. So nothing in the corpus is *currently* misfired.
The finding is that the emission set is asymmetric with the rules' definition,
not that a shipped card is broken: the first threat-area or attachment card with
an on-enter-play reaction gets a silent no-op rather than a compile error.

### 4 — Nothing stops a weakness being chosen for the upkeep hand-size discard

**ABSENT.** The restriction has no representation anywhere in the engine.

`data/rules-reference/rules/glossary/Weakness.md`:

> - A player may not optionally choose to discard a weakness card from hand, unless a card explicitly specifies otherwise.

The upkeep hand-size discard is a player choice over raw hand indices, with no
card-identity filter at all:
`crates/game-core/src/engine/dispatch/phases.rs:1259` `resume_hand_size_discard`
validates only that the indices are unique, in bounds, and exactly
`hand.len() - HAND_SIZE_LIMIT` in count, then removes them
(`phases.rs:1324`). The engine knows how to ask the question — `is_weakness_code`
already exists at `crates/game-core/src/engine/dispatch/cards.rs:22` and is used
by the opening-hand set-aside — but no discard path consults it.

**Divergence scenario.** An investigator draws a basic weakness treachery
(Amnesia 01096 or similar), which today stays in hand: `resolve_drawn_weaknesses`
handles only *persistent* treachery weaknesses and leaves the rest in hand by
design (`cards.rs:126`, deferred to #514). At upkeep with 9 cards in hand, the
player is prompted to discard 1. *Engine:* discarding the weakness is accepted.
*Rules:* the weakness may not be optionally discarded; a non-weakness card must
go instead.

Reachability rides on #514 — once drawn weaknesses resolve properly, a treachery
weakness stops lingering in hand — but the restriction is broader than that one
path (weakness *assets* and *events* legitimately sit in hand, per the same
glossary entry: *"When an investigator draws a weakness with a player cardtype …
add it to that investigator's hand"*), so closing #514 would not close this.

### 5 — Threat-area assets do not occupy slots

**WRONG.** No reachable consumer in Core + Dunwich.

`data/rules-reference/rules/glossary/Slots.md`:

> Each investigator has a number of specific slots that can be filled at any given moment. Each asset in an investigator's play area **or threat area** with a slot symbol is held in a slot of that type. Slots limit the number of asset cards the investigator is permitted to have in play simultaneously.

(Emphasis in the source is absent; the "or threat area" clause is quoted
verbatim.)

`crates/game-core/src/engine/dispatch/slots.rs:95` `occupied_slots` walks
`inv.cards_in_play` only. `Investigator.threat_area` is a peer `Vec<CardInPlay>`
(`crates/game-core/src/state/investigator.rs:85`) and is never consulted — nor
is it consulted by `make_room_candidates` (`slots.rs:131`), so a threat-area
asset would neither consume a slot nor be discardable to free one.

**Divergence scenario.** A slot-bearing encounter asset placed in an
investigator's threat area alongside two hand-slot assets in play: *engine:* a
third hand-slot asset plays with no make-room prompt. *Rules:* the investigator
is over their 2 hand slots and must discard.

No corpus card reaches it — the threat area only ever holds treacheries today
(`place_in_threat_area`'s callers) — so this is a latent asymmetry rather than a
live bug. Recorded because the fix is one iterator chain (`controlled_card_instances`
already exists at `investigator.rs:137` and walks exactly the right set minus the
investigator card) and the omission is invisible from `occupied_slots`'s own
doc-comment, which explains why the *investigator card* is excluded and says
nothing about the threat area.

### 6 — A victory-point card that is neither an enemy nor a location has nowhere to go

**ABSENT.** No implemented consumer.

`data/rules-reference/rules/glossary/Victory_Display_Victory_Points.md`:

> - As a victory point enemy is defeated, place the card in the victory display instead of in the discard pile.
> - At the end of a scenario, place each victory point location that is in play, revealed, and with no clues on it in the victory display.
> - As a victory point treachery card completes its resolution, place it in the victory display instead of in the discard pile.

The engine implements the first two: `CardKind::Enemy` and `CardKind::Location`
each carry `victory: Option<u8>` (`crates/card-dsl/src/card_data.rs:428`,
`:462`), a defeated victory enemy is pushed to `state.victory_display`
(`crates/game-core/src/engine/dispatch/combat.rs:99-103`), and victory locations
are placed at resolution. `CardKind::Treachery`, `CardKind::Event` and
`CardKind::Asset` carry **no** `victory` field, and `dispose_play_from_hand`
routes every played event to the owner's discard unconditionally
(`crates/game-core/src/engine/dispatch/cards.rs:943-951`).

**Divergence scenario.** Delve Too Deep 02111 is in the corpus
(`data/arkhamdb-snapshot/pack/dwl/tmm.json`, ingested via `PACK_FILES`). Its
text, verbatim:

> In player order, each investigator draws 1 card from the top of the encounter deck. Then, add Delve Too Deep to the victory display.

*Engine, were it implemented under the shipped `PlayDestination` set:* it lands
in the player discard. *Rules and its own text:* the victory display. The card
has no `abilities()` impl today, so `PlayCard` rejects it and no wrong outcome
occurs — this is filed as ABSENT rather than as an unimplemented card because
the missing piece is the metadata field and the disposal route, not the card
script. (The escape hatch exists: `Continuation::take_play_in_progress` lets an
effect take the card off its frame, which is how Barricade re-homes itself, so a
native could place it. The point is that nothing in the *framework* knows a
played card can end anywhere but the discard.) No corpus treachery carries a
victory value, so the third clause has no consumer at all.

---

## Already tracked

Each of these is a genuine contradiction with a verbatim rule, and each is
already on the tracker. Listed with the rule that pins it, because the citation
is new even where the gap is not — nothing below re-reports as a finding.

- **Acts never scale per-investigator — #573.** `Act_Deck_and_Agenda_Deck.md`:
  *"An act card may indicate a flat value (such as '4') or a per investigator
  value (as indicated by the [per_investigator] icon)."* `Per_Investigator.md`:
  *"that value is multiplied by the number of investigators who started the
  scenario."* The pipeline emits `CardKind::Act { clue_threshold: opt_u8(c.clues) }`
  and drops `clues_fixed` entirely (`crates/card-data-pipeline/src/main.rs:973`)
  — while the adjacent `Location` arm (`:967`) does pass it through
  `clue_value_lit`. `GameState`'s `Act.clue_threshold` is a bare `u8`
  (`state/game_state.rs:291`) read straight into the affordability check
  (`act_agenda.rs:155`). A 2-investigator The Gathering advances Act 1 for 2
  clues where the card prints 2 [per_investigator] = 4. The issue title states
  the consequence exactly ("multiplayer act advancement ~halved"); no
  correction needed.
- **`is_unique` is not ingested — #579.** `Unique.md`: *"A player cannot bring
  into play a unique card if a copy of that card (by title) is already in
  play."* `CardMetadata` has no unique field, and `check_play_card` has no such
  gate. Reachable in the shipping corpus via Dr. Milan Christopher 01033
  (`is_unique: true`, `deck_limit: 2`) — two investigators can each have one in
  play. #579 records the item and the consequence verbatim ("duplicate uniques
  can coexist in play").
- **`surge` and `peril` are hardcoded `false` for every enemy and treachery —
  #138 (and #139 for Peril enforcement, #579 for the Asset/Location variants).**
  `Surge.md`: *"After drawing and resolving an encounter with the surge keyword,
  an investigator must draw another card from the encounter deck."* The pipeline
  emits `surge: false, peril: false` literally
  (`card-data-pipeline/src/main.rs:949`, `:964`); the engine's surge chain
  (`MAX_SURGE_CHAIN`, `Continuation::PlayerDraw.surge_pending`) is built and
  correct but is fed a constant `false`, so no corpus card ever surges. Four
  Dunwich corpus treacheries print Surge (02081, 02084, 02221, 02258).
- **Doom is counted only on the agenda — #572** (`TODO(#572)` anchored at
  `act_agenda.rs:53`). `Doom.md`: *"If there are no '**Objective** – '
  requirements for advancing the current agenda and the requisite amount of doom
  is in play (among the agenda and all cards in play), the agenda advances"*,
  and `Act_Deck_and_Agenda_Deck.md` step 1: *"If the agenda deck is advancing,
  remove all doom from each card in play."* The in-code note is accurate — no
  corpus card carries doom, so the sum is presently the same number.
- **Clue contribution is a fixed order rather than a player choice — #153.**
  `Clues.md`: *"Any or all investigators may contribute any number of clues
  towards the total number of clues required to advance the act."*
  `spend_clues` (`act_agenda.rs:216`) drains the acting investigator first, then
  turn order. Outcome-equivalent solo; a real choice in multiplayer.
- **Non-persistent drawn weaknesses stay in hand — #514.** `Weakness.md`: *"When
  an investigator draws a weakness with an encounter cardtype … resolve that
  card as if it were just drawn from the encounter deck."* `resolve_drawn_weaknesses`
  (`cards.rs:132`) handles persistent treachery weaknesses only, by design and
  with the deferral named in its doc-comment. Finding 4 above rides on this but
  is not subsumed by it.
- **Attachment ownership is assumed to be the firing controller — `TODO(#371)`
  at `evaluator.rs:1051`.** `Attach_To.md`'s "Control of Attachments" block and
  `Ownership_and_Control.md`'s *"If a card would enter an out-of-play area that
  does not belong to the card's owner, the card is physically placed in its
  owner's equivalent out-of-play area instead"* are the governing text; solo is
  unaffected.
- **Attach-to generalisation — #373**, and **other-investigator commits — #65**
  (which is why `Limits_and_Maximums`' *"'Max X per [period]' imposes a maximum
  across all copies of a card (by title) for all players"* is currently moot;
  see below).

Nothing in this pass contradicts an assumption made by
`docs/audits/2026-08-14-chapter-1-forward-compatibility.md`. Its bucket-3 item
10 lists Permanent among the unmodelled keywords, which is the same gap #579
tracks from the ingestion side; its bucket-4 entries are about Chapter-1
content years away and are untouched by anything here.

---

## Checked and found sound

Specific, and each verified against the quoted rule rather than assumed.

**The Initiation Sequence.** `check_play_card`
(`engine/dispatch/reaction_windows.rs:1684`) runs the two preliminary
confirmations in RR's order and for RR's reasons — play restrictions
(including the state-change gate) before cost, then *"Determine the cost … If it
is established that the cost … can be paid, proceed"* via
`check_play_resource_cost_payable`. `play_card` then spends the action, pays the
cost, and only afterwards resolves effects. An X-cost card (`play_cost() ==
None`) is **rejected loudly** rather than silently played for free — the right
call under *"Determine the cost"*, and correctly flagged as currently
unreachable.

**Fast.** `Fast.md`'s three bullets are each enforced separately and correctly:
a Fast play costs no action but still pays the resource cost (`cards.rs:846-853`,
citing #501); a Fast event is admitted *"any time its play instructions
specify"* — modelled as its reaction window, with a standalone `PlayCard` on a
reaction event rejected explicitly; a Fast **asset** is restricted to the owner
(`owner_is_active && permissive_window`), matching *"A fast asset may be played
by an investigator during any player window on his or her turn"*; and Fast plays
skip `drive_aoo` entirely, matching *"Because fast cards do not cost actions to
play, they do not provoke attacks of opportunity."* "Play only during your turn"
is ingested as metadata and gates the Fast arms.

**The state-change gate.** `Event_Cards.md`: *"An event card cannot be played
unless the resolution of its effect has the potential to change the game
state."* `check_event_play_changes_state` applies it to events only — correct,
since an asset always changes state by entering play — and is conservative
(only provable no-ops blocked). Working a Hunch 01037 at a 0-clue location is the
shipped case.

**Limbo, as a concept.** RR's *"it is neither in play, in the discard pile, nor
is it in an investigator's hand … Limbo is not a physical game area"* is modelled
exactly: the card rides `Continuation::PlayFromHand` in no zone at all, and
`Limbo`'s closing clause — *"If its effects cause it to enter play (such as
attaching to another game element …), it leaves limbo and enters play at that
point in time"* — is precisely what `AttachSelfToLocation` does by taking the
card off its frame (Barricade 01038). Finding 2 concerns only *when* limbo
begins, not this.

**Event disposal.** *"Any time a player plays an event card, its costs are paid,
its effects are resolved (or canceled), and the card is placed in its owner's
discard pile after those effects resolve."* `dispose_play_from_hand` places it
after the `OnPlay` effect pops, exactly once, with `from: Zone::Hand` (the last
zone it was actually in). Assets land in `cards_in_play` and stay, per
`Asset_Cards.md`.

**Slots.** The capacity table matches `Slots.md`'s list exactly (1 accessory / 1
body / 1 ally / 2 hand / 2 arcane), pinned by `capacity_matches_rr_defaults`.
Critically, the engine gets the *shape* of the rule right: a full slot does not
reject the play — *"the investigator must choose and discard other assets under
his or her control simultaneously with the new asset entering the slot"* — and
only a need exceeding total capacity is a hard reject. The make-room candidate
set is correctly narrowed to assets occupying a slot type actually in deficit.
(The discard is sequenced before the entry rather than simultaneous with it; no
event or ability interleaves, so nothing observes the difference.)

**Uses.** *"When a card bearing this keyword enters play, place a number of
resource tokens equal to the value (X)"* — seeded in `new_in_play_instance`
(`threat_area.rs:23`) from printed metadata, on the single shared
construction point for all three enter-play paths. *"Some cards with this
keyword bear text that causes the card to be discarded if it has no uses
remaining. If the card contains no such text, it remains in play even if out of
uses"* — modelled as `Uses.discard_when_empty`, parsed per card rather than
assumed.

**Discard routing by ownership.** `Discard_Piles.md`: *"Any time a card is
discarded, it is placed faceup on top of its owner's discard pile. Encounter
cards are owned by the encounter deck."* `Effect::DiscardSelf`
(`evaluator.rs:988`) routes a threat-area card to `encounter_discard`, and a
location attachment by *card type* — player card (Barricade 01038) to the
player's discard, encounter card (Obscuring Fog 01168) to the encounter discard,
defaulting to encounter when no registry is installed. That distinction is easy
to miss and is made explicitly, with both cards named.

**Attach-to legality.** *"The 'attach to' phrase is checked for legality each
time a card would be attached"* — Barricade rejects when the controller is
between locations; Obscuring Fog's printed *"Limit 1 per location"* is enforced
in its Revelation native, and a second copy is discarded rather than attached,
matching *"If such a card cannot remain in its prior state or game area, discard
it."*

**Put into play vs played.** `Put_into_Play.md`: *"The resource cost of a card
being put into play **is not paid**"* and *"A card that has been put into play
is not considered to have been played or drawn."* `PutIntoThreatArea` pays
nothing and emits `CardEnteredThreatArea`, never `CardPlayed`.

**Set aside / removed from game.** `set_aside_locations` and
`set_aside_enemies` on `GameState` model *"Set-aside cards have no interaction
with the game until they are referenced"*; `Investigator.removed_from_game`
models `Removed_from_Game.md` for elimination. Both are real zones, not
approximations.

**Threat area.** *"An investigator's threat area is a play area in which
encounter cards currently engaged with and/or affecting an investigator are
placed"* and *"The cards in an investigator's threat area are at the same
location as the investigator"* — modelled structurally by hanging
`threat_area` off the investigator, so co-location is not a fact to maintain.

**Opening-hand weaknesses.** RR setup step 8's set-aside-and-redraw loop is
implemented for both the initial draw and the mulligan redraw, with the
set-aside pile flushed back into the deck and reshuffled once every mulligan
completes — and deliberately *not* wired to the revelation path
(`replace_opening_hand_weaknesses`'s doc-comment says so), which is the
distinction `Revelation.md` draws: *"Revelation abilities do not resolve during
setup."*

**Per-investigator scaling where it is modelled.** `reveal_location`
(`engine/dispatch/reveal.rs:22`) multiplies by `investigators.len()` and
documents why that is faithful to *"the number of investigators who started the
scenario"* — eliminated investigators stay in the map, which is what
`Per_Investigator.md`'s *"If investigators have been eliminated from the
scenario, they still count toward 'per investigator' values"* requires. Enemy
`HealthValue::PerInvestigator` scales the same way. (Acts are the exception —
see #573 above.)

**Commit caps.** `Limits_and_Maximums.md`'s "Max X … committed" is enforced from
printed metadata at the commit window (`skill_test.rs:991-1012`), per card code,
against the actual committed multiset. Its *"for all players"* scope is
unexercised only because a single investigator commits today (#65).

**Constant restrictions on playing.** `Play_Restrictions_Permissions_and_Instructions.md`'s
*"In order to use such an ability or to play such a card, its play restrictions
must be observed"* — `play_is_prohibited` gates `PlayCard` on
`Restriction::CannotPlay(card_type)` (Dissonant Voices 01165) as a validate-first
check. Every implemented card's printed play restriction was checked against the
snapshot: only Mind over Matter 01036 and Working a Hunch 01037 carry one, and
both are handled.

---

## Uncertain

- **Whether the step-3/AoO ordering (finding 2) is worth correcting on its own.**
  The rule is unambiguous and the fix is a small reordering, but nothing in the
  corpus observes it, and moving `commence_play` after `drive_aoo` means the
  card is still in hand while a Fast play may reshuffle or re-index that hand —
  which is precisely the hazard #565/#604 and ADR 0002 were about. Settling it
  wants a `/wayfinder`-style look at whether hand indices survive a mid-AoO
  play, not a one-line move.
- **Whether a "Uses" asset placed in a threat area exists anywhere in Chapter
  1**, which is what would make finding 5 reachable. The check I ran covers
  Core + Dunwich only; a snapshot-wide sweep for slot-bearing assets that enter
  a threat area would settle it.
- **Whether `Limits_and_Maximums`' "Max X per [period]" on *playing* a card has
  any Core/Dunwich consumer.** The scan for `Max ` in implemented card text
  turned up only the five neutral skills' commit caps, so nothing is unenforced
  today; a scan over the *whole* corpus (not just implemented cards) would say
  whether the first Dunwich card to need it already exists as a stub.
- **Whether the "simultaneously" in `Slots`' make-room clause and in `Unique`'s
  *"discard the player card simultaneously as the encounter card enters play"*
  is ever observable** given the engine's sequential emission. No reaction
  window opens between the two steps today, so the question is unforced; the
  first replacement or interrupt effect (#366) makes it real.
- **Whether `Costs`' *"If multiple costs for a single card or ability require
  payment, those costs must be paid simultaneously"* has a Chapter-1 consumer
  whose outcome differs from the engine's sequential action-then-resources
  payment.** No corpus card distinguishes the two; a card whose cost payment
  itself changes affordability would.
