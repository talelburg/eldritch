# #368 — Trigger-level eligibility predicate (native hook)

**Date:** 2026-06-26
**Issues:** #368 (eligibility generalization) — and **closes #470** (The Barrier offered when ineligible).
**Out of scope / split off:** #368 item 2 ("capped count") moved to **#471** (Deduction).

## Problem

A reaction ability must be **suppressed at scan time** when its effect can't change game state (RR p.2: *"An ability cannot initiate … if the resolution of its effect will not change the game state."*). The engine has no generic mechanism — it carries hardcoded, per-card stand-ins, and one is actually **missing**, which is a live bug:

1. **Cover Up `01007`** — `scan_pending_triggers` hardcodes `if matches!(event, WouldDiscoverClues) && card.clues == 0 { continue }` (`reaction_windows.rs:245`). An emptied Cover Up would otherwise prompt a useless interrupt on every discovery.
2. **The Barrier `01109`** (Act 2) — its round-end "investigators in the hallway may spend the requisite clues to advance" reaction has **no** affordability gate in the offer scan (`scan_act_agenda_reactions`). The resolve handler `round_end_advance` rejects on insufficient clues, but the candidate is offered regardless — **bug #470** (offered at 0/3 clues, investigator not in the Hallway). The doc comments in `the_barrier.rs` / `round_end_advance` claim "offered only when affordable" — stale; the gate was never implemented.

## Solution

A per-ability **native eligibility predicate**, evaluated at reaction-scan time. Chosen over a declarative `Condition` because both live consumers are single-consumer and heterogeneous (source-instance clues vs. location-group clues ≥ a dynamic act threshold), so declarative DSL vocabulary would be speculative (CLAUDE.md: no DSL primitives until 2+ cards share a pattern). A predicate is promoted to a declarative `Condition` later, when one recurs.

### 1. DSL (`card-dsl`)

One optional field on `Ability`:
```rust
pub struct Ability {
    pub trigger: Trigger,
    pub costs: Vec<Cost>,
    pub effect: Effect,
    pub usage_limit: Option<UsageLimit>,
    pub eligibility: Option<String>,   // NEW — native eligibility tag
}
```
`eligibility` is genuinely-optional (most abilities have none), so it stays implicitly-optional on the wire — serde defaults a missing `Option` to `None` (the #453 per-field carve-out, same as `usage_limit`). Plus a chaining builder:
```rust
impl Ability {
    #[must_use]
    pub fn with_eligibility(mut self, tag: impl Into<String>) -> Self {
        self.eligibility = Some(tag.into());
        self
    }
}
```
It lives on `Ability` (not inside the `Trigger::OnEvent` variant) — simpler than threading a field through the `Trigger` enum's many match sites, and it is only ever *consulted* in the reaction/fast scans, so it is inert on a forced ability. All existing `Ability` constructors set `eligibility: None`.

### 2. Registry (`game-core` + `cards`)

Parallel to `native_effect_for`:
```rust
// card_registry.rs
pub type EligibilityFn = fn(&GameState, &EvalContext) -> bool;   // read-only; same EvalContext native effects receive

pub struct CardRegistry {
    // … existing …
    pub native_eligibility_for: fn(&str) -> Option<EligibilityFn>,
}
// default registry: native_eligibility_for: |_| None
```
`cards::REGISTRY` provides a crate-level `native_eligibility_for(tag)` that dispatches to per-card fns (mirroring the existing `native_effect_for` dispatch).

### 3. Scan wiring (`reaction_windows.rs`)

A `CandidateSource::instance()` helper (`InPlay(id) => Some(id)`, `Board | Hand => None`) and one shared predicate evaluator:
```rust
fn ability_eligible(
    state: &GameState,
    ability: &Ability,
    source: CandidateSource,
    controller: InvestigatorId,
) -> bool {
    let Some(tag) = &ability.eligibility else { return true };          // no gate → eligible
    let Some(reg) = card_registry::current() else { return false };     // can't verify → suppress
    let Some(pred) = (reg.native_eligibility_for)(tag) else { return false };
    let ctx = EvalContext::for_controller_with_optional_source(controller, source.instance());
    pred(state, &ctx)
}
```
Applied at **both** reaction-scan sites, after the trigger kind/timing/pattern match:
- **`scan_pending_triggers`** (in-play cards → Cover Up): **delete** the hardcoded `WouldDiscoverClues && card.clues == 0` `continue`; instead `if !ability_eligible(state, ability, CandidateSource::InPlay(card.instance_id), id) { continue }`. Behaviour-preserving for Cover Up (its only reaction is the discover interrupt).
- **`scan_act_agenda_reactions`** (acts/agendas → The Barrier): **add** `if !ability_eligible(state, ability, CandidateSource::Board, lead) { continue }`. This site has no eligibility check today — adding it is the **#470 fix**.

(The fast-window scan `any_fast_play_eligible` is **not** wired now — no in-scope fast consumer; deferred until Burned Ruins 02205 lands.)

### 4. Consumers

**Cover Up `01007`** (`cover_up.rs`):
- Its `reaction_on_event(WouldDiscoverClues, When, …)` ability gains `.with_eligibility("01007:has_clues")`.
- `native_eligibility_for("01007:has_clues")` = a read-only predicate that finds the source instance via `ctx.source` (iterating the controller's `threat_area`/`cards_in_play`, as the discard native already does) and returns `clues > 0`.
- The `card.clues == 0` hardcode leaves the engine.

**The Barrier `01109`** (`the_barrier.rs`):
- Its round-end `reaction_on_event(RoundEnded, When, …)` ability gains `.with_eligibility("01109:can_advance")`.
- `native_eligibility_for("01109:can_advance")` = `|s, _ctx| round_end_advance_affordable(s, HALLWAY)`.
- **New public helper** `round_end_advance_affordable(state: &GameState, contributor_location_code: &str) -> bool` (in `act_agenda.rs`, re-exported): resolves the location, sums `clues_held(investigators_at(loc))`, compares to the current act's `clue_threshold`. **`round_end_advance` (resolve) is refactored to call it**, so offer and resolve share one affordability check and cannot drift. The "investigators in the Hallway" condition is subsumed — no Hallway investigators ⇒ 0 clues ⇒ ineligible.
- Correct the stale "offered only when affordable" doc comments to point at the real gate.

### 5. What closes

PR `Closes #368` (eligibility generalization complete; both stand-ins now declarative-tag-driven) **and `Closes #470`** (the Barrier offer is now suppressed when unaffordable).

## Out of scope

- **#368 item 2** (cap the discovery-count threaded to the interrupt) — moved to **#471**, where fixing Deduction makes it live.
- **Pending snapshot consumers** Lone Wolf 02188 / Burned Ruins 02205 — not in corpus (Dunwich). The hook supports them later (Burned Ruins also needs fast-window wiring).
- **Declarative `Condition` eligibility vocabulary** — deferred until a predicate recurs across 2+ cards.

## Testing

- **DSL/registry units:** `with_eligibility` sets the tag; default registry's `native_eligibility_for` returns `None`; `ability_eligible` returns `true` for a no-tag ability and `false` when the registry or predicate is absent.
- **Cover Up (card/integration):** its discover interrupt is **offered** when Cover Up holds clues and **suppressed** when it holds 0 (replacing the old hardcode's coverage).
- **The Barrier (integration, reproduces #470):** at clues `< threshold` (e.g. 0/3) the round-end advance reaction is **not** offered; at `≥ threshold` with investigators in the Hallway it **is**. A direct `round_end_advance_affordable` unit test for the boundary.
- **Regression:** existing Cover Up and Barrier tests stay green (behaviour-preserving for the in-budget cases).
- **CI gauntlet** (all seven jobs, strict flags) before push — touches `game-core`, `card-dsl`, `cards`.

## Done criteria

- The Barrier's round-end advance is not offered when the Hallway group can't afford the threshold (the #470 screenshot case no longer prompts).
- Cover Up's interrupt is gated by a declared eligibility tag, not a hardcoded scan branch.
- Both reaction-scan sites consult one shared `ability_eligible`; no per-card `clues`/threshold special-casing remains in the scan.
- All seven CI jobs green.
