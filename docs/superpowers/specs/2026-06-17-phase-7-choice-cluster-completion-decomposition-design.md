# Phase 7 — choice-cluster completion (decomposition)

**Status:** design approved (framing + decomposition). A **decomposition**
doc in the mold of
[Group C decomposition](2026-06-11-phase-7-slice-1-group-c-decomposition-design.md):
it settles the *shape of the work* (the shared-machinery inventory, the ordered
PR split, what each closes) and **defers the concrete mechanics** of each piece
to its own per-PR design when picked up. Built on the shipped
[Axis A](2026-06-17-trigger-dispatch-axis-a-interactive-choice-design.md) (PR #350)
and [Axis B](2026-06-16-trigger-dispatch-rework-axis-b-foundation-design.md)
(PRs #338–#343) foundations.

## Why this exists

Axis A shipped the interactive-choice *mechanism* (the `Continuation::Choice`
frame, the `0⇒reject·1⇒auto·2+⇒suspend` resolve convention, the `decisions`
replay cursor, `ground_chosen_targets`). It deliberately left
`*::ChosenByController` offering *all* candidates with a `TODO`, because the
**constrained** forms ("an enemy/investigator *at your location*", "your or a
connecting location") have no consumer until the Axis-E cards — and #349 says
to design the constraint vocabulary against *those*, not speculatively.

Those cards are now reachable. Six Axis-E carve-outs are unblocked by Axes A+B
plus a handful of small orthogonal primitives, and they cluster tightly: each
new primitive gets two-or-more consumers, satisfying the repo's "no DSL
primitive until ≥2 cards want it" rule, and they collectively supply the 2–3
concrete constraints #349 wanted to design against.

Completing them closes **C5d** (#239, the last open Group-C sub-slice), three
C6/C5e carve-outs (#312, #313, #321), one C5e carve-out (#306), and the choice
unification (#349).

## The six cards (texts verbatim, `data/arkhamdb-snapshot/pack/core/core.json`)

| Code | Name | Text (load-bearing clause) | Issue |
|---|---|---|---|
| 01018 | Beat Cop | `You get +1 [combat].` / `[fast] Discard Beat Cop: Deal 1 damage to an enemy at your location.` | #301 (prereq) / #239 |
| 01019 | First Aid | `Uses (3 supplies). If First Aid has no supplies, discard it.` / `[action] Spend 1 supply: Heal 1 damage or horror from an investigator at your location.` | #302 (prereq) / #239 |
| 01086 | Knife | `[action]: Fight. You get +1 [combat] for this attack.` / `[action] Discard Knife: Fight. You get +2 [combat] for this attack. This attack deals +1 damage.` | #312 |
| 01035 | Medical Texts | `[action] Choose an investigator at your location and test [intellect] (2). If you succeed, heal 1 damage from that investigator. If you fail, deal 1 damage to that investigator.` | #321 |
| 01087 | Flashlight | `Uses (3 supplies).` / `[action] Spend 1 supply: Investigate. Your location gets -2 shroud for this investigation.` | #313 |
| 01024 | Dynamite Blast | `Choose either your location or a connecting location. Deal 3 damage to each enemy and to each investigator at the chosen location.` | #306 |

All six choice positions are "ground-before-a-single-effect" (a target feeding
one effect, or a choice that *is* the whole effect) — none sits after a mutation
in a `Seq`, so Axis A's single-pass suspend-and-replay covers them and its
`apply_seq` guard never fires (Axis-A spec §1).

## Shared-machinery inventory

What each piece *is* is fixed here; *how* it works is its PR's design.

1. **Unified `Choose` surface (#349) — the keystone.** Resolves #349 by
   unifying on the **spatial vocabulary** rather than the monolithic
   `{variety, constraint, chooser}` sketch (which unified the wrong axis and
   admitted nonsensical pairs like "a location at your location"). The full
   design + the variant comparison that landed it lives in
   [the PR-1 keystone design](2026-06-17-phase-7-choice-keystone-design.md);
   the shape:

   ```rust
   struct Choose<S> { scope: S /* chooser deferred — latent in solo (#349) */ }
   enum LocationSet { Here, Anywhere /* YourOrConnecting → PR-8 */ }   // chooser-relative spatial vocab; `Here` once
   enum EntityScope { At(LocationSet) /* Engaged/WithTrait arms accrete later */ }

   enum InvestigatorTarget { You, Active,            Chosen(Choose<EntityScope>) }
   enum EnemyTarget        { Engaged,                Chosen(Choose<EntityScope>) }   // PR-2
   enum LocationTarget     { YourLocation, TestedLocation, Chosen(Choose<LocationSet>) }
   ```

   `LocationSet` is the chooser-relative spatial vocabulary, used directly for
   location-picks and via `EntityScope::At` for entity-position-filters — so
   "your location" (`Here`) is defined once and reused in both roles. Variety
   stays statically typed at the effect target (no illegal pairs, no runtime
   guard); the resolver extends `ground_chosen_targets` per-variety, reusing
   Axis A's resolve convention + `Choice` frame **unchanged**. `chooser` is
   deferred (real but implicitly carried by the controller binding today —
   agenda 01105's lead choice already works that way); the `Choose<S>` wrapper
   reserves its home. PR-1 ships the **investigator + location** varieties +
   `LocationSet{Here,Anywhere}`; migrates `*::ChosenByController` →
   `Chosen(…Anywhere)`; migrates the synthetic test cards; ships **no new
   shipped card**. **Closes #349.**

2. **`Cost::DiscardSelf`** — discard the *source asset in play* as an activation
   cost (distinct from `Effect::DiscardSelf`, which discards a treachery from a
   threat area / attachment). Validate-first: the source must be in play; emits
   `CardDiscarded { from: Zone::Play }`. Consumers: Beat Cop, Knife.

3. **`Effect::Heal { kind, target, count }`** — the engine's first healing
   primitive (`kind: Damage | Horror`); saturating decrement of the resolved
   investigator's `damage`/`horror`. First Aid's "damage **or** horror" composes
   as `ChooseOne([Heal{Damage,…}, Heal{Horror,…}])` over Axis A's already-shipped
   `Effect::ChooseOne`. Consumers: First Aid, Medical Texts.

4. **Uses-depletion auto-discard** — discard an asset when `Cost::SpendUses`
   empties its pool ("If First Aid has no supplies, discard it"). One in-scope
   consumer (First Aid); the flag-vs-native modeling is its PR's call.

5. **`Effect::DealDamageToEnemy { target: EnemyTarget, amount }`** — direct
   (non-test) damage to a resolved enemy via the existing
   `deal_damage_to_enemy` engine entry. **Typed, not `Native`**, for the same
   reason `Effect::Fight` is (dsl.rs): Beat Cop's discard-self cost requires a
   *pre-cost* "≥1 enemy at your location?" target check, which only an
   inspectable effect supports — a native effect would force a reject *after*
   paying the cost, violating validate-first. The ≥2-consumer rule yields to the
   validate-first contract here, a documented exception exactly like `Fight`.
   Consumer: Beat Cop.

6. **`Effect::Investigate { shroud_modifier: IntExpr }`** — the Investigate
   mirror of `Effect::Fight`: initiates an Investigate test against the
   controller's location, snapshotting a per-investigation *location-difficulty*
   delta (distinct from `InFlightSkillTest.test_modifier`, which adjusts the
   investigator's total). Consumer: Flashlight.

7. **Dynamite Blast's AoE-at-chosen-location** — "deal 3 to each enemy **and**
   each investigator at the chosen location." The location *choice* rides the
   typed `LocationTarget::Chosen(YourOrConnecting)` surface from piece 1; the
   fan-out *application* over the resolved location is the one genuinely-new bit
   (existing fan-out is controller-location-relative, e.g.
   `InvestigatorTargetSet::AtControllerLocation`). Its PR settles typed-fan-out
   vs. `Native`.

## Decomposition (ordered PRs)

**Every PR maps to an existing issue — no new issues are filed.** The issues
already split engine-prereq (#301, #302) from content (#239, #312, #321) the way
the repo carves; the seven content/prereq issues plus the keystone #349 cover the
whole cluster. One branch per issue, per the PR convention.

| PR | Issue | Kind | Delivers | Deps |
|---|---|---|---|---|
| **1** | #349 | infra | Unified `Choose` + `LocationSet`/`EntityScope` + `Investigator`/`Location` varieties + synthetic cards (subsumes `*::ChosenByController`) | Axes A/B |
| **2** | #301 | infra | `Cost::DiscardSelf` (in-play asset) + `EnemyTarget`/`chosen_enemy` (the enemy variety) + `Effect::DealDamageToEnemy` (Beat Cop's engine prereqs) | 1 |
| **3** | #302 | infra | `Effect::Heal` + uses-depletion auto-discard (First Aid's engine prereqs) | — |
| **4** | #239 | content | Beat Cop 01018 + First Aid 01019 → **closes C5d** | 1, 2, 3 |
| **5** | #312 | content | Knife 01086 (reuses #301's `Cost::DiscardSelf` + existing `Effect::Fight`) | 2 |
| **6** | #321 | content | Medical Texts 01035 (reuses #302's `Effect::Heal` + #349's investigator choice + existing `SkillTest`) | 1, 3 |
| **7** | #313 | infra+content | Flashlight 01087 (`Effect::Investigate` + per-investigation shroud) | — |
| **8** | #306 | infra+content | Dynamite Blast 01024 (location choice + AoE-at-chosen-location) | 1 |

- **PR-1 (#349) is the keystone**: PRs 2, 6, 8 depend on it. PRs 3 and 7 are
  independent of the choice axis and may land any time (PR-7 Flashlight touches
  no choice machinery).
- **C5d (#239) closes when Beat Cop *and* First Aid both land** (PR-4) — the two
  cards #301/#302 were carved to unblock. The Guardian L0 constant "+1 [combat]"
  half of Beat Cop is already expressible
  (`constant(modify(Combat, 1, WhileInPlay))`) and ships with PR-4.
- **#349 lacks a milestone** (it predates this plan); pull it onto
  `phase-7-the-gathering` when this kickoff lands.
- The **enemy variety** (`EnemyTarget::Chosen(Choose<EntityScope>)` + the
  `chosen_enemy` binding + the enemy enumerator) ships in **PR-2 (#301)**, not
  PR-1 — an enemy target is meaningless without an enemy-targeting effect, and
  the first one (`Effect::DealDamageToEnemy`) is #301's. `EntityScope::At` is
  reused verbatim from PR-1, so PR-2 adds only the enemy enumerator + binding.
- Each content PR carries its cards' per-card tests; the `Choose` resolver is
  validated by synthetic `TEST_REGISTRY` cards (constrained + enemy picks,
  multi-choice replay) in PR-1.

## Deferred to per-PR design (open mechanics)

Explicitly **not** settled here — each is its PR's first design question:

- The location-connection / adjacency model behind `LocationSet::YourOrConnecting`
  (PR-8, #306 — its only consumer). The `Choose<S>` / `LocationSet` / `EntityScope`
  shape itself is **settled** (V7; see the keystone design doc).
- Where `Effect::DealDamageToEnemy` is declared (PR-1/#349 alongside the `Enemy`
  variety, vs. PR-2/#301 as Beat Cop's consumer-specific effect).
- Uses-depletion modeling: a pipeline-parsed `CardKind::Asset` flag vs. a
  card-local native check (PR-3, #302).
- Dynamite Blast's AoE application: a typed fan-out effect vs. `Effect::Native`
  reading `chosen_location` (PR-8, #306).

## Out of scope (still blocked)

- **#319 Old Book of Lore / #320 Research Librarian** — deck-search primitives
  (search-top-N-and-select; search-by-trait tutor). Substantial new machinery,
  no overlap with this cluster.
- **#322 Mind over Matter** — stat-*substitution* modifier + until-end-of-round
  lifetime + play-timing restriction (the latter shared with Axis C
  reaction-event-play).
- **#323 Barricade** — enemy-movement-into-location prohibition + event→location
  attachment + forced-on-leave.
- **#304 Evidence! / #305 Dodge** — Axis C (reaction-event-play) / Axis D
  (attack cancellation), not this cluster.
- **`PickMultiple` / multi-target selection** — every pick here is "choose **an**
  X" (one); no consumer (Axis-A spec §"One selection family").

## Dependencies

- **Axis A** (PR #350) — the `Continuation::Choice` frame, resolve convention,
  `decisions` replay, `ground_chosen_targets`, the `PickSingle`/structured-
  `InputRequest` contract. PR-1 extends its resolver; it does not change the
  frame.
- **Axis B** (PRs #338–#343) — the continuation stack and reentrant forced run.
- `Effect::Fight` / `IntExpr` (#295) — Knife's abilities; the typed-effect
  precedent for `DealDamageToEnemy` (5) and `Investigate` (6).
- `deal_damage_to_enemy` (#237, C5b) — the engine entry `DealDamageToEnemy` calls.

## What "done" looks like

All six cards implemented per verbatim text with per-card tests; the unified
`Choose` surface resolves `Any` / `AtYourLocation` / `YourOrConnecting` picks
(auto-resolving 0/1, suspending on 2+) across investigator / location / enemy
varieties; `*::ChosenByController` is gone (subsumed); #239 (C5d), #312, #313,
#321, #306, and #349 are closed; the full strict gauntlet is green.
