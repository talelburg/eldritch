# Phase 7 Slice 1 — Group C decomposition

Decomposition of **Group C** (the Gathering content) of Phase 7 Slice 1 into
mergeable sub-slices and concrete issues, and the placement of the `emit_event`
dispatch unification (#212) relative to that content.

Companion to the Slice 1 design: `2026-06-10-phase-7-slice-1-gathering-design.md`.
That doc scoped C as a single "spec'd" group; this doc breaks it down.

## Why C needs decomposing

C as originally framed ("Gathering cards + setup + Roland + signature/weakness +
starter deck") is **~47 distinct cards** — 26 encounter + Roland's full suggested
30-card deck (~21 new) — plus the scenario setup module, Standard chaos bag,
symbol-token logic, victory points, and act/agenda advancement. Only ~5 of those
cards are already implemented. That is far too large for one plan; it is really
**seven sub-slices** (C1–C7) feeding a final playable gate, followed by a
dispatch-cleanup refactor.

Decision input: **the full suggested 30-card Roland deck is in scope** (not a
minimal playable subset).

## Where #212 (emit_event unification) factors in

**Decision: build C on the existing rails, extend the existing
`ForcedTriggerPoint` enum-dispatcher with new variants as content demands them,
and land the full #212 chokepoint as a single post-C refactor** validated against
all of C's real content. #213 (player-choice simultaneous-trigger ordering) is
deferred further still; until then, simultaneous triggers resolve in a fixed
**deterministic order**.

Rationale (the count that drove it): tracing C's actual dispatch needs, the
"thread forced/optional triggers in millions of places" fear does not
materialize, because the bulk shares hooks:

- The **7 treacheries** fire through the **one existing Revelation hook** in
  `encounter.rs` — adding a treachery is writing an effect, not wiring dispatch.
- **2 forced locations** reuse the existing `EnteredLocation` path; **Beat Cop**
  reuses the existing `AfterEnemyDefeated` window.
- The genuinely-new dispatch surface is a **handful of framework timing points**
  (`RoundEnded`, `EndOfTurn`, `AfterLocationInvestigated`, `GameEnd`, a
  damage-from-enemy window, an after-investigate window) plus **Cover Up's
  before-timing interrupt** — each a small, locally-correct addition through the
  existing `ForcedTriggerPoint` dispatcher / reaction-window pipeline.

Front-loading #212 would mean designing its event taxonomy **before** the cards
that define its requirements exist (the full 30-deck makes that worse), and its
hard machinery (reentrancy, mid-emit suspension) still is not exercised by C's
content. The existing `resume_reaction_window` loop already does the iterative
multi-optional-trigger resolution, so C's many optional reactions are already
covered. A1/A2 were built forward-compatible specifically so `emit_event` can
absorb `fire_forced_triggers` later without rework.

**In-C consolidation seam:** C4a unifies the forced-scan and reaction-scan onto a
**single shared scan source** (spanning `cards_in_play` + the new threat-area
zone). #212 later absorbs that one clean seam instead of several scattered ones.

## Engine capabilities confirmed present (reused, not rebuilt)

- Act/agenda advancement: `check_doom_threshold`, `advance_agenda`,
  `advance_act_action` (`act_agenda.rs`). C1b adds only new objective *types*.
- Revelation resolution + spawn-at-location + Hunter movement (`encounter.rs`,
  `hunters.rs`). C reuses these.
- Reaction-window pipeline with iterative multi-trigger resolution
  (`reaction_windows.rs`).
- `ForcedTriggerPoint` enum-dispatcher (`forced_triggers.rs`) — extended, not
  replaced.

Confirmed **absent** (real new work): a **threat-area zone** (C4a), **Prey
variants + Retaliate** (C3a), **before-timing interrupt windows** (C5a).

## Sub-slices and issues

Split along the repo's engine-machinery / card-content seam. Issue numbers under
milestone `phase-7-the-gathering`.

### C1 — Scenario skeleton
- **#227** [scenario] C1a — `setup()` world-build (locations + connections,
  act/agenda decks + thresholds, Standard chaos bag) + forced location effects
  (Attic/Cellar) on existing `EnteredLocation` rails.
- **#228** [engine] C1b — new act-advancement objective types: 01109 round-end
  group clue-spend gate; 01110 advance-on-Ghoul-Priest-defeated.

### C2 — Reference card + victory
- **#229** [card] C2 — 01104 symbol-token effects (skull/cultist/tablet,
  board-count Rust impl) via B1 plumbing; Attic/Cellar victory points.

### C3 — Encounter enemies
- **#230** [engine] C3a — Prey variants (highest-combat, lowest-remaining-health)
  + Retaliate.
- **#231** [card] C3b — Ghoul Priest, Flesh-Eater, Icy Ghoul, Ghoul Minion,
  Ravenous Ghoul, Swarm of Rats.
- **#232** [engine/card] C3c — agenda 01107 forced movement (existing
  `PhaseEnded(Enemy)`) + doom (new `ForcedTriggerPoint::RoundEnded`).

### C4 — Treacheries + threat area
- **#233** [engine] C4a — threat-area zone + shared forced/reaction scan source +
  `ForcedTriggerPoint::{EndOfTurn, AfterLocationInvestigated}`. *(in-C
  consolidation seam.)*
- **#234** [card] C4b — one-shot Revelations: Grasping Hands, Rotting Remains,
  Crypt Chill, Ancient Evils (existing Revelation hook).
- **#235** [card] C4c — persistent threat-area/attachment: Frozen in Fear,
  Dissonant Voices, Obscuring Fog.

### C5 — Roland deck: Guardian + signature + weakness
- **#236** [engine] C5a — Cover Up before-timing interrupt window +
  `ForcedTriggerPoint::GameEnd`.
- **#237** [engine] C5b — Guard Dog reaction window (after enemy deals damage).
- **#238** [card] C5c — .38 Special signature + Cover Up weakness content.
- **#239** [card] C5d — Guardian L0 assets (.45 Automatic, Physical Training,
  Beat Cop, First Aid, Machete, Guard Dog).
- **#240** [card] C5e — Guardian L0 events + skill (Evidence!, Dodge, Dynamite
  Blast, Vicious Blow).

### C6 — Roland deck: Seeker + Neutral
- **#241** [engine] C6a — Dr. Milan after-successful-investigate reaction window.
- **#242** [card] C6b — Seeker cards (Old Book of Lore, Research Librarian, Dr.
  Milan, Medical Texts, Mind over Matter, Barricade).
- **#243** [card] C6c — Neutral cards (Knife, Flashlight, Emergency Cache, Guts,
  Perception, Overpower, Manual Dexterity, Unexpected Courage).

### C7 — Closeout (the "Slice 1 done" gate)
- **#244** [infra] C7a — registry swap (`synth_cards::TEST_REGISTRY` →
  `cards::REGISTRY`) + web `SCENARIO_ID` → `"the-gathering"` (folded-in B3).
- **#245** [test] C7b — end-to-end integration test: solo Roland, full deck,
  Standard, drives to Won and Lost. Pairs with **#224** (roster-seating test
  migration).

### Post-C
- **#212** emit_event chokepoint (deterministic-order variant) — consolidates the
  `ForcedTriggerPoint` dispatcher + reaction-window queue sites + Revelation hook.
- **#213** player-choice simultaneous-trigger ordering — separate, later.

## Ordering & parallelism

`C1a (#227)` is the root dependency. After it:

- C1b, C2, C3*, C4*, C5*, C6* proceed largely in parallel.
- Within C3: C3a → C3b → C3c. Within C4: C4a → {C4b, C4c}. Within C5: {C5a, C5b}
  → {C5c, C5d} → C5e. Within C6: C6a → C6b; C6c independent.
- `C7a (#244)` and `C7b (#245)` are last, after C1–C6.
- `#212` after C7.

## Out of scope (deferred to later Slice-1 / Phase-7 work)

- Lita Chantler's parley/take-control and the Parlor (01115) Resign action
  (per the Gathering design).
- Difficulty selection beyond Standard.
- #213 player-choice trigger ordering.
- Roland's investigator elder-sign + reaction (#118).

## Corrections folded in

- The Gathering design spec named Roland's signature ".45 Automatic"; the real
  signature is **Roland's .38 Special (01006)** (snapshot-verified). .45
  Automatic (01016) is a separate Guardian deck card.
