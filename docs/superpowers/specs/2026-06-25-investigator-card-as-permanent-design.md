# Investigator-card-as-permanent: unify health/sanity/soak onto the investigator card

**Issue:** [#448](https://github.com/talelburg/eldritch/issues/448) · **Date:** 2026-06-25 · **Status:** design approved, pre-implementation

A foundational, **behaviour-preserving** state-model migration: represent each seated
investigator's **investigator card** as a real `CardInPlay` that holds the investigator's
health/sanity and is the default damage/horror soaker. The four bespoke fields on
`Investigator` (`damage`, `horror`, `max_health`, `max_sanity`) plus the two #118-bridge
fields (`card_code`, `ability_usage`) collapse onto that card.

## Motivation — what it dissolves

The investigator's physical/mental state being four ad-hoc fields forces several special
cases that all disappear once the investigator card is "just a permanent in play":

- **Investigator-card abilities fire for free.** The reaction scan and the constant-modifier
  sum already walk in-play instances; the investigator card simply joins that walk, so
  Roland's reaction + elder-sign stop needing test-only card injection.
- **Usage tracking is free.** A `CardInPlay` already carries `ability_usage`; the bespoke
  `Investigator.ability_usage` home goes away.
- **Soak stops being special-cased.** The investigator card becomes a normal soaker with
  real capacity, so the distribution pipeline treats it uniformly with asset soakers
  instead of dumping the "remainder" on a field — no phantom-soaker hazard, because it is
  the real soaker.

This also **retires the #118 bridge** the engine deliberately shipped as scaffolding (the
`card_code` + `ability_usage` fields, the `scan_investigator_card_reactions` source, and the
direct `elder_sign_modifier` lookup).

## Rules grounding

Rules Reference, "Dealing Damage/Horror" and defeat (quoted verbatim):

> "When an investigator is dealt damage or horror, that investigator may assign it to
> eligible asset cards he or she controls." … "All damage/horror that cannot be assigned to
> an asset must be assigned to the investigator."

> "After applying damage/horror, if an investigator has as much or more damage on it as it
> has health (or as much or more horror on it as it has sanity), that investigator is
> defeated." … "if an asset has damage equal to or higher than its health or horror equal to
> or higher than its sanity, it is defeated and placed in its owner's discard pile."

The investigator's own health/sanity capacity *is* the investigator card. The "must be
assigned to the investigator" clause makes that card the **always-eligible, mandatory-
remainder soaker**, and investigator defeat is the same `accumulated ≥ printed` shape as
asset defeat — only the consequence differs (eliminate vs discard). This is what makes the
unification behaviour-preserving.

## Keystone decisions (settled)

1. **Placement: a dedicated `Investigator.investigator_card: CardInPlay` field (not in
   `cards_in_play`, not `Option`).** Keeps `cards_in_play` meaning "cards the player played,"
   so elimination/defeat/asset loops over it never accidentally touch the investigator card.
   The trade is unifying the currently-inconsistent scan iterators onto one
   `controlled_card_instances()` that prepends the investigator card (see §2).

2. **Capacity read from metadata (uniform with assets).** `max_health()`/`max_sanity()`
   read `CardKind::Investigator { health, sanity }` from the registry, exactly as
   `build_soakers` reads asset capacity. Defeat collapses to `accumulated ≥ printed`
   identically for the investigator card and assets; no capacity is stored on the card. The
   accepted cost: capacity reads require an installed registry, so capacity/defeat tests
   register a synthetic investigator card (see §6).

## §1 — Target state model

```rust
pub struct Investigator {
    pub investigator_card: CardInPlay,   // NEW — the player-character permanent
    // DELETED: damage, horror, max_health, max_sanity, card_code, ability_usage
    …
}
```

`CardInPlay` is **not** modified — it already carries everything the investigator card needs:
`accumulated_damage` / `accumulated_horror` (harm), `ability_usage` (usage), `code`
(identity). The six deleted fields become accessors:

| Old field | New accessor | Source | Registry? |
|---|---|---|---|
| `inv.damage` | `inv.damage()` | `investigator_card.accumulated_damage` | no |
| `inv.horror` | `inv.horror()` | `investigator_card.accumulated_horror` | no |
| `inv.max_health` | `inv.max_health()` | `metadata_for(code)` → `Investigator { health }` | yes |
| `inv.max_sanity` | `inv.max_sanity()` | same → `{ sanity }` | yes |
| `inv.card_code` | `inv.investigator_card.code` | direct | no |
| `inv.ability_usage` | `investigator_card.ability_usage` + its methods | direct | no |

Mutating harm goes through `&mut inv.investigator_card.accumulated_*` (or a thin
`apply_damage`/`heal` helper on `Investigator`), preserving the validate-first/mutate-second
contract at the existing call sites.

## §2 — Unified iteration (ability / reaction / constant-mod wiring)

`controlled_card_instances()` becomes the single "triggerable in-play instances" iterator,
prepending the investigator card:

```rust
pub fn controlled_card_instances(&self) -> impl Iterator<Item = &CardInPlay> {
    std::iter::once(&self.investigator_card)
        .chain(self.cards_in_play.iter())
        .chain(self.threat_area.iter())
}
```

Then migrate the two consumers that bypass it today onto it:
- **`sum_constant_modify`** (`evaluator.rs`, currently iterates `inv.cards_in_play` directly).
- **the reaction scan** (`scan_pending_triggers` / `reaction_windows.rs`).

Net effect: the investigator card's `Trigger::Constant` modifiers, `OnEvent` reactions, and
`ElderSign` trigger all participate automatically. The bespoke
`scan_investigator_card_reactions` source and `CandidateSource::Investigator` are **deleted**
(subsumed). `elder_sign_modifier` reads abilities via `investigator_card.code`.

**Care point:** any consumer that means "assets/events the player played" (not "every
triggerable instance") must keep iterating `cards_in_play`, not the unified iterator. Audit
each call site during the migration and classify it.

## §3 — Soak + defeat unification

- **`build_soakers`** gains a `CardKind::Investigator { health, sanity }` arm and adds the
  investigator card as the **always-eligible default soaker**, replacing the `assign_attack`
  "remainder-to-the-investigator-field" special case. Per the RR, the investigator card takes
  whatever can't be assigned to an asset.
- **Defeat** uses the asset rule uniformly: `investigator_card.accumulated_damage ≥
  max_health()` (resp. horror/sanity). The single investigator-specific branch: when the
  *investigator card* overflows, run **investigator elimination** (`Status::Killed` /
  `Insane`, carrying `DefeatCause`) instead of asset discard. `defeat_overflowed_assets` /
  `apply_{damage,horror}_numeric` / `place_assignment` route harm to
  `investigator_card.accumulated_*` and branch on "is this the investigator card?".
- **Healing** (`heal_effect`) reduces `investigator_card.accumulated_*`.

## §4 — Seating + event suppression

At `start_scenario`, mint `investigator_card` as a `CardInPlay` (mint an instance id, set
`.code` from the roster entry). **No `EnteredPlay` / play events are emitted** — this is
scenario setup placement, not a played card (matching how investigators are minted today and
how `ScenarioStarted` is the only timing point at seating). The card is created *before* any
harm/ability path can reference it. Capacity is not stamped (read on demand from metadata).

## §5 — Bridge retirement (#118)

Folded into this epic, as the bridge's documented sunset:
- Delete `Investigator.card_code` → identity is `investigator_card.code`.
- Delete `Investigator.ability_usage` (+ `is_usage_exhausted`/`bump_ability_usage` wrappers)
  → use the card's own `ability_usage` + `CardInPlay` methods.
- Delete `scan_investigator_card_reactions` / `CandidateSource::Investigator` → unified scan.
- `elder_sign_modifier` looks abilities up by `investigator_card.code`.

## §6 — Test / unseated investigators

Every investigator now owns an `investigator_card`. Approach (no `Option`, per decision):
- `test_investigator(id)` mints an `investigator_card` with a fixed synthetic code
  (`"TEST_INV"`), `accumulated_* = 0`.
- `test_support` provides an opt-in **game-core test registry** that registers `TEST_INV`
  with `health/sanity = 8/8` (mirroring today's `max_health: 8`), plus a
  `seated_test_investigator()` / builder helper for tests that read capacity or exercise
  defeat. (One process shares one `OnceLock` registry, as today — the test registry covers
  the synthetic code used across game-core unit tests.)
- `damage()` / `horror()` and harm/soak-routing remain registry-free; only capacity/defeat
  tests install the test registry — consistent with soak tests already needing asset
  metadata.

## §7 — Migration: one branch, four green checkpoints

One feature branch (`engine/investigator-card-permanent`), four sequential commits, each
leaving the full gauntlet green so the branch is reviewable commit-by-commit. Behaviour-
preserving throughout; transient dual-write keeps early checkpoints green.

1. **Scaffold + dual-write.** Add `investigator_card` + accessors; mint the card at seating;
   keep the four old fields and keep them in sync with the card. No reads migrated yet — this
   only makes the card *exist* before anything depends on it. Update `test_support` (§6).
2. **Harm + soak + defeat.** Route damage/horror writes, soak distribution, and defeat onto
   the investigator card (§3). The investigator card becomes a real soaker; defeat reads the
   card. Old fields still dual-written but no longer the source of truth for harm.
3. **Abilities + reactions + constant-mods + bridge retirement.** Unify
   `controlled_card_instances()` and migrate `sum_constant_modify` + the reaction scan onto
   it (§2); retire the bridge (§5).
4. **Delete + finalize.** Remove the six old fields and the dual-write; flip every remaining
   read to the accessors; update the web wire format (`crates/web/src/board.rs`) and all
   fixtures.

Each checkpoint runs the full CI gauntlet (`-D warnings` test, host clippy, fmt, doc, wasm
build, wasm clippy). The PR description maps commit → checkpoint for review.

## Consumer surface (implementation appendix)

File:line anchors gathered during scoping (verify before editing — line numbers drift):

- **Field defs:** `state/investigator.rs:64,66,68,70`. **Seating mint:**
  `engine/dispatch/phases.rs:97–126` (reads `health`/`sanity` from metadata at `:51–56`).
  **Fixture:** `test_support/fixtures.rs:35–64`.
- **Damage writes:** `dispatch/combat.rs:360` (`apply_damage_numeric`), `:296`
  (`place_assignment`), `:413–422` (`soak_and_place`); `dispatch/elimination.rs:204–209`
  (`take_damage`); `dispatch/actions.rs:852,988,1108,1289` (`inv.damage = 0` resets);
  `evaluator.rs:1488` (heal). **Damage reads:** `combat.rs:361` (defeat), `hunters.rs:50–51`
  (prey), `web/board.rs:106`.
- **Horror writes:** `combat.rs:390` (`apply_horror_numeric`), `:297`; `elimination.rs:187–193`
  (`take_horror`); `cards.rs:866` (reset); `evaluator.rs:1489` (heal). **Horror reads:**
  `combat.rs:391` (defeat), `web/board.rs:107`.
- **Capacity reads:** `combat.rs:361,391` (defeat thresholds); `hunters.rs:50–53` (remaining
  health); `web/board.rs:106–107`.
- **Soak pipeline:** `combat.rs:413–422` (`soak_and_place`), `:433–455`
  (`soak_and_distribute`/K5b), `:457–495` (`build_soakers`), `:164–185` (`assign_attack` —
  the investigator remainder special-case to remove), `:211–256`
  (`defeat_overflowed_assets`), `:278–322` (`place_assignment`), `:872–950`
  (`resume_damage_assignment`).
- **Elimination/defeat:** `elimination.rs:18–51` (`apply_investigator_defeat`), `:56–165`
  (steps), `:235–250` (`check_all_defeated`).
- **Bridge:** `state/investigator.rs:41–54` (`card_code`), `:129` (`ability_usage`),
  `:149–167` (usage methods); `reaction_windows.rs:362–424`
  (`scan_investigator_card_reactions`, skips empty code `:387`); `evaluator.rs:1935–1960`
  (`elder_sign_modifier`); folded at `skill_test.rs:314`.
- **Unified-iteration consumers:** `state/investigator.rs:145–147`
  (`controlled_card_instances`); `evaluator.rs:2112–2140` (`sum_constant_modify`, iterates
  `cards_in_play` directly — migrate); `evaluator.rs:2061–2104` (`pending_action_surcharge`,
  already uses `controlled_card_instances`); `reaction_windows.rs:222–288`
  (`scan_pending_triggers`).
- **Asset-soak model to mirror:** `combat.rs:478–481` (reads `CardKind::Asset { health,
  sanity }`), `:228–230` (asset defeat); `state/card.rs:130–136`
  (`accumulated_damage`/`accumulated_horror`).

## Out of scope / non-goals

- No rules-behaviour change — purely a state-model migration. (If any latent bug surfaces, it
  gets its own issue, not a silent fix here.)
- No `Option<CardCode>` redesign of identity beyond what folding onto `investigator_card.code`
  already gives (the #453 sentinel question is fully resolved by this epic: there is no
  separate `card_code` field to default).
- Capacity-modifying effects (cards that raise max health/sanity) remain unmodelled, as today.
- Horror-soak ≠ max-sanity boost (#44) is unaffected.

## Risks

- **Iterator-classification errors (§2):** migrating a `cards_in_play` loop that meant
  "played cards" onto the unified iterator would wrongly include the investigator card.
  Mitigation: audit + classify every call site; the dedicated-field choice means the default
  (leaving a loop on `cards_in_play`) is safe.
- **Registry-coupled capacity in tests (§6):** mitigated by the synthetic-card test registry;
  watch for game-core tests that read capacity without seating.
- **Single large branch:** mitigated by four independently-green checkpoints mapped in the PR.
