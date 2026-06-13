# Phase 7 Slice 1 C2 — Reference-card symbol tokens + location victory points

**Issue:** [#229](https://github.com/talelburg/eldritch/issues/229) (`[card] Slice 1 C2`).
**Depends on:** C1a (#227, locations + board) and B1 (#223, reference-card plumbing — which this design partly reverses; see below).
**Decomposition:** `docs/superpowers/specs/2026-06-11-phase-7-slice-1-group-c-decomposition-design.md`.

## Goal

Two independent deliverables under one issue:

1. **The Gathering reference card 01104's chaos-symbol effects** resolve during skill
   tests (skull / cultist / tablet).
2. **Attic (01113) and Cellar (01114) award victory points** at scenario end.

Both are off the *strict* win/lose latch (VP is dormant until Phase 9 XP; symbol
effects shift test odds but don't gate a resolution), but C2 makes the scenario
play faithfully rather than with C1a's flat placeholder token values.

### Verified card text (`data/arkhamdb-snapshot/pack/core/core_encounter.json`)

01104 *The Gathering* (Easy / Standard):

> `[skull]` −X. X is the number of `[[Ghoul]]` enemies at your location.
> `[cultist]` −1. If you fail, take 1 horror.
> `[tablet]` −2. If there is a `[[Ghoul]]` enemy at your location, take 1 damage.

The Gathering's verified Standard bag has **no Elder Thing token**, so only
skull/cultist/tablet need values; the mechanism still admits elder-thing for
later scenarios.

### Verified rule (Rules Reference p.21, "Victory Display, Victory Points")

> "As a victory point enemy is defeated, place the card in the victory display
> instead of in the discard pile. **At the end of a scenario, place each victory
> point location that is in play, revealed, and with no clues on it in the
> victory display.**"

So a location's VP is recorded **at scenario end**, gated on *in play + revealed +
no clues* — **not** at the moment its last clue is taken. The issue's "on clear"
phrasing is imprecise; this design follows the RR. The enemy "as defeated" path
is **out of scope** (C3).

---

## Deliverable 1 — Symbol-token effects

### Ownership: the scenario module, not the card

A scenario's chaos-symbol effects are printed on its single reference card, but
they are modelled as **scenario behaviour on `ScenarioModule`**, not as a card's
`abilities()`. Rationale (settled in brainstorming):

- There is exactly **one reference card per scenario**, already named by the
  module — "the card owns it" and "the scenario owns it" describe the same single
  behaviour, with no fidelity lost.
- The reference card is **never a card-object**: never in a hand, never played,
  revealed, or moved by the engine. It doesn't fit the `abilities()` model, which
  exists for cards the engine manipulates (play / commit / reveal triggers).
- The effect is **board-dependent** (skull = −Ghoul-count) so it must run against
  live state. The `cards` registry bridge is static-data-only (`abilities_for`
  returns a fixed `Vec<Ability>`); letting a card own this would force a third,
  context-taking bridge function used *only* by reference cards. `ScenarioModule`
  is already the context-taking home (`setup`, `apply_resolution` take state), so
  the hook reuses an existing pattern and adds no new cross-crate surface.
- No DSL primitive is added: the only things that recur across scenarios are the
  symbol-draw hook (built here) and a board-trait query (promoted to shared infra
  in C3 when it has a second consumer — Prey / agenda movement). C2 writes the
  Ghoul count inline.

### `ScenarioModule` change

Remove the dead B1 field, add the hook:

```rust
pub struct ScenarioModule {
    // removed: pub reference_card: &'static str,
    pub setup: fn() -> GameState,
    pub apply_resolution: fn(&Resolution, &mut GameState, &mut Vec<Event>),
    /// Resolve a drawn chaos **symbol** token (skull/cultist/tablet/elder-thing)
    /// against live board state. `None` = this scenario has no reference-card
    /// symbol effects (test fixtures); the engine then uses the static
    /// `TokenModifiers` path. Never called for Numeric/AutoFail/ElderSign.
    pub resolve_symbol: Option<fn(ChaosToken, &SymbolCtx) -> SymbolOutcome>,
}
```

`ScenarioModule` stays `Copy` (`Option<fn(...)>` is `Copy`, const-constructible).

### Reversing B1's dead plumbing

B1 (#223) added `reference_card` + `active_reference_card()` +
`reference_card_with_registry()` in anticipation of C2 consuming them. The
redesign consumes a `resolve_symbol` hook instead, which never needs the card
*code*. Confirmed dead: `active_reference_card()` has **zero call sites**;
`reference_card` is only written (module literals) and read by the unused lookup
plus one field-assertion test. C2 therefore **deletes**
`active_reference_card`, `reference_card_with_registry`, and their tests, and
drops the `reference_card` field from every module literal
(`the_gathering`, the synthetic fixture, the `scenario_registry` default, and the
server / game-core test fixtures). This is a deliberate, documented partial
reversal of B1 — not churn: B1 made a reasonable forward bet on a shape we only
finalised now.

### New types (in `game-core`, shared with `scenarios`)

```rust
/// Read-only board view handed to a scenario's symbol hook.
pub struct SymbolCtx<'a> {
    state: &'a GameState,
    investigator: InvestigatorId,
}
impl<'a> SymbolCtx<'a> {
    pub fn state(&self) -> &GameState;
    pub fn investigator(&self) -> InvestigatorId;
    /// The testing investigator's current location, if placed.
    pub fn investigator_location(&self) -> Option<LocationId>;
}

/// What a symbol token does this test: a modifier plus deferred effects.
pub struct SymbolOutcome {
    /// Added to the test's skill total (skull −X, cultist −1, tablet −2).
    pub modifier: i8,
    /// Applied regardless of pass/fail (tablet's board-gated damage).
    pub immediate: Vec<TokenEffect>,
    /// Applied only if the test fails (cultist's horror).
    pub on_fail: Vec<TokenEffect>,
}

/// A symbol's side effect on the tested investigator.
pub enum TokenEffect { Damage(u8), Horror(u8) }
```

`SymbolOutcome::default()` (modifier 0, no effects) is the natural "this symbol
does nothing" value. Helper builders may be added if they keep `the_gathering`'s
hook readable; not required.

The split into `modifier` / `immediate` / `on_fail` mirrors the resolution
timing: the modifier is needed *before* the total is computed; side effects run
*after* success/failure is known. The hook is called once at reveal (board state
doesn't change between reveal and outcome), so it computes board-gated branches
(tablet's "is a Ghoul present") up front and files them under `immediate`.

### Engine integration (`engine/dispatch/skill_test.rs`)

In `resolve_chaos_token_and_emit`, replace the unconditional
`resolve_token(token, &token_modifiers)` with:

1. If `token` is a **symbol** (Skull / Cultist / Tablet / ElderThing) **and** the
   active scenario module has a `resolve_symbol` hook (looked up via a small
   helper in `scenario.rs`, e.g. `resolve_symbol_token(state, token, investigator)
   -> Option<SymbolOutcome>`, replacing the deleted `active_reference_card`
   routing):
   - Call the hook → `SymbolOutcome`.
   - `resolution = TokenResolution::Modifier(outcome.modifier)` and push the
     existing `ChaosTokenRevealed { token, resolution }` (unchanged event shape).
   - Compute total / success / fail exactly as today.
   - **After** the `SkillTestSucceeded` / `SkillTestFailed` event: apply
     `outcome.immediate`, then `outcome.on_fail` iff the test failed. Each
     `TokenEffect` is applied to the tested investigator by **routing through the
     same code as `Effect::DealDamage` / `Effect::DealHorror`** (the evaluator's
     `deal_damage_effect` / `deal_horror_effect`, or a shared helper they call),
     so defeat handling and the existing `DamageTaken` / `HorrorTaken` events are
     reused — no new damage/horror event types.
2. Otherwise (numeric/auto-fail/elder-sign, or no hook): today's static
   `resolve_token` path, unchanged.

Symbol tokens never carry AutoFail/ElderSign semantics, so only the `Modifier`
arm is affected; `ElderSign` / `AutoFail` handling is untouched.

`the_gathering::setup()` drops its placeholder `skull/cultist/tablet`
`TokenModifiers` (now superseded by the hook). `token_modifiers` and the static
`resolve_token` path remain for hook-less scenarios (synthetic fixtures).

### 01104's hook (`the_gathering.rs`, plain Rust)

```rust
fn resolve_symbol(token: ChaosToken, cx: &SymbolCtx) -> SymbolOutcome {
    let ghouls = ghoul_count_at_investigator_location(cx); // loop state.enemies
    match token {
        ChaosToken::Skull => SymbolOutcome { modifier: -(ghouls as i8), ..default },
        ChaosToken::Cultist => SymbolOutcome {
            modifier: -1, on_fail: vec![TokenEffect::Horror(1)], ..default },
        ChaosToken::Tablet => SymbolOutcome {
            modifier: -2,
            immediate: if ghouls > 0 { vec![TokenEffect::Damage(1)] } else { vec![] },
            ..default },
        _ => SymbolOutcome::default(), // no Elder Thing in The Gathering's bag
    }
}
```

`ghoul_count_at_investigator_location` = enemies in `state.enemies` whose `traits`
contains `"Ghoul"` and whose `current_location == cx.investigator_location()`.
Inline for C2; C3 promotes the trait-at-location query to shared infra when Prey /
agenda movement become the second/third consumers.

---

## Deliverable 2 — Location victory points

### `GameState` change

```rust
/// The victory display (RR p.21): an out-of-play zone of cards worth
/// experience, scored at scenario end. Phase 9 sums their corpus victory
/// values for XP. Locations enter here at resolution; victory-point enemies
/// (C3) enter as defeated.
pub victory_display: Vec<CardCode>,
```

### Engine scan (`engine/mod.rs`, `fire_scenario_resolution`)

At the existing None→Some resolution chokepoint, after the `ScenarioResolved`
event and **independent of the scenario module** (it reads the *card* registry,
not the scenario registry — same "engine-state property" reasoning that lets
`ScenarioResolved` fire without a module):

For each location in `state.locations` (the in-play map; set-aside locations are
excluded by construction) that is `revealed && clues == 0`, look up
`card_registry::current()?.metadata_for(&loc.code)`; if its `kind` is
`CardKind::Location { victory: Some(v), .. }` with `v > 0`, push `loc.code` into
`state.victory_display` and emit:

```rust
Event::EnteredVictoryDisplay { code: CardCode, victory: u8 }
```

Iterate locations in `LocationId` order for deterministic event/zone ordering. No
registry installed → no metadata → no VP (graceful, matching other registry-gated
paths). Placement happens before `apply_resolution` runs; order is immaterial
today (nothing reads `victory_display` until Phase 9).

---

## Testing

Per the layering in `CLAUDE.md` (card tests → engine unit tests → integration):

**Symbol effects** — integration tests in `crates/cards/tests/` (need the real
card registry for Ghoul metadata + the scenario module installed; game-core can't
reach the corpus). Drive a skill test, force each symbol token via a seeded bag /
`with_token_modifiers`-style fixture, and assert:
- skull: total reduced by exactly the Ghoul count at the location (0 Ghouls,
  1 Ghoul, 2 Ghouls — the board-count case the acceptance criterion calls out).
- cultist: −1 to the total; `HorrorTaken` emitted iff the test **failed**.
- tablet: −2 to the total; `DamageTaken` emitted iff a Ghoul is at the location
  (independent of pass/fail).
- a non-symbol (numeric) token is unaffected.

**Engine unit tests** (`skill_test.rs`, `mod.rs`) using `TestGame` + a stub
`resolve_symbol` hook (no corpus needed):
- a returned `SymbolOutcome` modifier reaches the total; `immediate` always
  applies; `on_fail` applies only on failure.
- hook-less scenario → static `resolve_token` path unchanged (regression guard).

**Victory points** — integration test (needs corpus victory values):
- a resolution with Attic (revealed, 0 clues) in play → Attic's code in
  `victory_display`, `EnteredVictoryDisplay { victory: 1 }` emitted.
- a victory location still holding clues, or unrevealed, or set-aside → **not**
  placed.
- Attic + Cellar both cleared → both placed (deterministic order).

## Out of scope (named, deferred)

- Victory-point **enemies** entering the display as defeated (C3).
- Phase 9 XP/campaign-log consumption of `victory_display`.
- Promoting "enemies with trait T at location L" to a shared query (C3, at its
  second consumer).
- Elder Thing symbol values (not in The Gathering's bag).

## Files touched

- `crates/game-core/src/scenario.rs` — `ScenarioModule` field swap; new
  `SymbolCtx` / `SymbolOutcome` / `TokenEffect`; `resolve_symbol_token` helper;
  delete `active_reference_card` + `reference_card_with_registry` + tests.
- `crates/game-core/src/engine/dispatch/skill_test.rs` — symbol-token routing +
  side-effect application.
- `crates/game-core/src/engine/mod.rs` — VP scan in `fire_scenario_resolution`.
- `crates/game-core/src/state/game_state.rs` — `victory_display` field.
- `crates/game-core/src/event.rs` — `EnteredVictoryDisplay` variant.
- `crates/scenarios/src/the_gathering.rs` — `resolve_symbol` hook + Ghoul count;
  drop placeholder token modifiers.
- `crates/scenarios/src/test_fixtures/synthetic.rs`,
  `crates/game-core/src/scenario_registry.rs`, server / game-core test fixtures —
  `reference_card` → `resolve_symbol: None`.
- `crates/cards/tests/` — new integration test(s).
