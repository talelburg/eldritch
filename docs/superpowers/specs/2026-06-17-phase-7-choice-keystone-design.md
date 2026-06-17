# PR-1 — the unified `Choose` keystone (#349)

**Status:** design approved. The keystone of the
[choice-cluster completion](2026-06-17-phase-7-choice-cluster-completion-decomposition-design.md)
(PR-1 of 8) and the resolution of
[#349](https://github.com/talelburg/eldritch/issues/349) (uniform DSL choice
interface). Built on the shipped
[Axis A](2026-06-17-trigger-dispatch-axis-a-interactive-choice-design.md)
(PR #350) interactive-choice machinery.

## Why this exists

Axis A shipped the choice *mechanism* — the `Continuation::Choice` frame, the
`0⇒reject·1⇒auto·2+⇒suspend` resolve convention (`resolve_choice_count`), the
`DecisionCursor` replay, `resume_choice`, and the `PickSingle(OptionId)` /
structured-`InputRequest` contract — but left `InvestigatorTarget::ChosenByController`
and `LocationTarget::ChosenByController` offering **all** candidates with a
`TODO`. #349 was filed to unify the entity-target surface and add the constrained
forms ("an enemy/investigator *at your location*", "your or a connecting
location") the Axis-E cards want, designed against those real cards rather than
speculatively.

PR-1 builds that surface. It is the keystone the six-card cluster depends on
(PRs 2, 6, 8); it ships **no new shipped card** (synthetic-card validation only,
the Axis-A precedent for the target stubs).

## §1 — The surface (V7)

#349's sketch proposed a monolithic `Choose { variety, constraint, chooser }`.
Working it against the real cards showed that unifies the **wrong axis**: a flat
`constraint` shared across varieties admits nonsensical pairs ("a location *at
your location*") needing a runtime guard, and per-variety scope enums duplicate
"at your location" across investigator and enemy. The axis with real reuse is the
**spatial vocabulary** — the set of locations a choice is relative to — which is
shared between *location-picks* (which locations may I pick?) and
*entity-position-filters* (where must the entity be?).

```rust
/// The interactive-choice wrapper. `chooser` is deferred (see §5); the wrapper
/// reserves its home and keeps the unified `Choose` name #349 asked for.
struct Choose<S> { scope: S }

/// The chooser-relative spatial vocabulary — defined ONCE, reused as a
/// location-pick scope and (via `EntityScope::At`) an entity-position filter.
enum LocationSet {
    Here,        // the chooser's location ("your location")
    Anywhere,    // any location in play (the old bare `ChosenByController`)
    // YourOrConnecting → added by PR-8 (#306), its only consumer, with the
    // location-adjacency model.
}

/// An entity-choice filter. Locational today; non-spatial arms (`Engaged`,
/// `WithTrait`, …) accrete on this enum when a card needs them — additively,
/// touching neither `LocationSet` nor location-picks. (Minimal-enum-with-a-
/// documented-growth-path, the `UsagePeriod::Round` idiom.)
enum EntityScope { At(LocationSet) }

enum InvestigatorTarget { You, Active,                  Chosen(Choose<EntityScope>) }
enum EnemyTarget        { Engaged,                      Chosen(Choose<EntityScope>) }  // PR-2 (#301)
enum LocationTarget     { YourLocation, TestedLocation, Chosen(Choose<LocationSet>) }
```

Properties this buys:

- **Type-safe — no illegal combos, no runtime guard.** Variety is the target
  type; you cannot write "a location at your location."
- **Spatial vocabulary defined once.** `LocationSet::Here` ("your location")
  serves both an entity filter (`At(Here)`) and a location pick. As the vocabulary
  grows (`YourOrConnecting`, `Connecting`), each term is added to `LocationSet`
  once and is instantly available in both roles — no re-duplication.
- **`AtYourLocation` deduped** across investigator and enemy (both
  `Chosen(Choose<EntityScope>)`).
- **Both growth directions are additive in disjoint places:** more spatial terms
  → `LocationSet`; non-spatial entity filters → `EntityScope`. Location-picks
  never see entity filters; the entity targets are already pointed at
  `EntityScope`, so a future "choose a Ghoul enemy" is a new `EntityScope` arm
  with zero call-site churn.
- **Unified `Choose` preserved** with a home for the deferred `chooser`.

Migration: `ChosenByController` → `Chosen(Choose { scope: …Anywhere })`
(investigator: `EntityScope::At(LocationSet::Anywhere)`; location:
`LocationSet::Anywhere`). Builder sugar keeps call sites readable:
`chosen_anywhere()`, `chosen_at_your_location()`.

(Full variant comparison — V1 flat-constraint, V2 per-enum, V3 per-variety
scopes, V4/V5 `EntityScope`+`LocationScope`, V6 `Choose<LocationSet>`-everywhere,
V7 — lives in the conversation that produced this doc; V7 won on
spatial-vocabulary DRY *over time* while staying type-safe and additive.)

## §2 — The resolver

Generalize `ground_chosen_targets` (evaluator.rs) to match the `Chosen(_)`
variant (not the bare `ChosenByController`) on the effects carrying each target,
then dispatch to a per-variety candidate enumerator before the **unchanged**
`resolve_choice_count` → bind/suspend/replay path:

| Target | Scope | Candidates |
|---|---|---|
| `LocationTarget::Chosen` | `LocationSet::Here` | the chooser's location (singleton → auto-binds) |
| | `LocationSet::Anywhere` | all locations (today's behavior) |
| `InvestigatorTarget::Chosen` | `EntityScope::At(Here)` | investigators whose location == chooser's |
| | `EntityScope::At(Anywhere)` | all investigators (today's behavior) |

`Here` and `Anywhere` are computed against the chooser (the `EvalContext`
controller, until §5's `chooser` lands). Enumeration is inherently per-variety
(investigators vs. locations are different `BTreeMap`s); the *convention*
(`0/1/2+`), the frame, and replay are the shared parts and are reused verbatim.
Binding still writes `EvalContext.chosen_investigator` / `chosen_location`; the
existing target resolvers read them unchanged.

## §3 — Scope boundary (what ships where)

PR-1 ships the **investigator + location** varieties + `LocationSet{Here,Anywhere}`
+ `EntityScope{At}`. Deferred to their consuming PRs:

- **Enemy variety → PR-2 (#301).** `EnemyTarget::Chosen(Choose<EntityScope>)`, the
  `chosen_enemy: Option<EnemyId>` `EvalContext` binding, and the enemy enumerator
  ride with their first effect consumer (`Effect::DealDamageToEnemy`). `EntityScope`
  is reused verbatim — PR-2 adds only the enumerator + binding.
- **`LocationSet::YourOrConnecting` → PR-8 (#306).** Added with the
  location-adjacency model and its only consumer (Dynamite Blast).
- **`chooser` → deferred** (§5).

## §4 — Reused from Axis A, untouched

`ChoiceFrame`, `Continuation::Choice`, `resolve_choice_count` / `ChoiceResolution`,
`suspend_for_choice`, `DecisionCursor` (+ `recorded_so_far` / `root` replay),
`resume_choice`, `PickSingle(OptionId)` / `ChoiceOption` / `InputRequest::choice`.
PR-1 adds **no** frame, **no** input-contract change, **no** new suspension mode —
it widens only the DSL *surface* and the *candidate enumeration*.

## §5 — `chooser` deferral

`chooser` is real but **implicitly carried by the controller binding** today, so
it needs no DSL field yet. Agenda 01105 ("the lead investigator must decide")
already ships a lead choice: `ForcedTriggerPoint::AgendaAdvanced` binds
`controller = turn_order.first()` (the lead), and both the `ChooseOne` and the
`You` resolve to it. An explicit `chooser` only becomes load-bearing when
`controller ≠ chooser` — multiplayer, or a card played by a non-lead that says
"the lead chooses." None of the cluster's six cards need it. When it lands it is
a field on `Choose<S>`, no churn to the three target enums (the wrapper's reason
for existing now).

## §6 — Testing

Synthetic `TEST_REGISTRY` cards (no shipped consumer):

- Migrate the existing `ChosenByController` synthetics → `Chosen(…Anywhere)`;
  assert behavior-preserving (auto-resolve 0/1, suspend on 2+).
- A `Chosen(Choose { scope: EntityScope::At(Here) })` investigator card: offers
  only co-located investigators — auto-binds with one present, suspends with two
  co-located, rejects with none.
- A `LocationSet::Here` location-pick card: auto-binds (singleton).

Plus the unit coverage on the enumerators (candidate sets per scope).

## §7 — Coarse sequencing (writing-plans turns this into TDD tasks)

1. **DSL surface** — `Choose<S>`, `LocationSet{Here,Anywhere}`, `EntityScope{At}`,
   the three target enums' `Chosen` variants, builders; serde round-trips. Pure
   types.
2. **Migrate `ChosenByController`** call sites + inline tests + synthetic cards to
   `Chosen(…Anywhere)`; prove behavior-preserving.
3. **`EntityScope::At(Here)` resolution** — the co-located-investigator enumerator
   + `LocationSet::Here` location enumerator; synthetic constrained cards.
4. **Phase-7 doc** update (the keystone row + a Decisions entry) as the final
   commit, after CI is green.

## Out of scope (deferred, each with its consumer)

- Enemy variety (PR-2), `YourOrConnecting` + adjacency (PR-8), `chooser`
  (multiplayer / non-lead chooser), non-spatial `EntityScope` arms (first
  trait/attribute/engagement *choice* card).
- `PickMultiple` / multi-target — every pick here is "choose **an** X" (Axis-A
  spec §"One selection family").

## Dependencies

- **Axis A** (PR #350) — the frame, convention, replay, and input contract PR-1
  widens (does not change).
- **Axis B** (PRs #338–#343) — the continuation stack the `Choice` frame lives on.

## What "done" looks like

`InvestigatorTarget` / `LocationTarget` carry `Chosen(Choose<…>)`;
`LocationSet::Anywhere` reproduces today's all-candidates behavior and
`EntityScope::At(Here)` filters to the chooser's location (auto-resolving 0/1,
suspending on 2+); `ChosenByController` is gone; the Axis-A frame/contract is
unchanged; synthetic cards cover the migration + the constrained pick; #349 is
closed. First Aid / Medical Texts / Beat Cop / Dynamite are then unblocked on the
choice surface, pending their own primitives.
