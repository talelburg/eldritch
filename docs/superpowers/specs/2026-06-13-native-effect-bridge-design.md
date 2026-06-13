# `Effect::Native` — card-local Rust effects via registry bridge (design)

**Issue:** [#276](https://github.com/talelburg/eldritch/issues/276) (Phase 7 infra).
**First downstream consumer:** C3c agenda 01107 ([#232](https://github.com/talelburg/eldritch/issues/232)).

## Problem

The card-effect ethos is "use the DSL where convenient, implement directly
in Rust where it isn't." But the registry's only behavior hook is
`abilities_for(code) -> Vec<Ability>`, and an `Ability`'s behavior **is**
an `Effect`. So the only way to give a card Rust behavior today is to add a
variant to the shared `card_dsl::Effect` enum with a hand-written
evaluator arm — there is **no card-local Rust path**. Single-use scenario
logic accretes in the shared enum (C1b added three one-off variants; the
agenda 01107 work would add two more). Variants should exist **only when
the logic is reused**; single-use card logic belongs card-locally in Rust.

## Constraint that fixes the shape

`Effect`/`Ability` live in `card-dsl`, which is *below* `game-core` in the
crate graph, so an `Effect` variant cannot reference `game_core::GameState`
/`Cx`. `Effect` also carries a **tested** serde round-trip contract
(`card-dsl/src/dsl.rs`), so it can't hold a fn pointer. Therefore the
Rust-logic bridge must be the **registry** (in `game-core`, which can name
both sides) — the same mechanism as `abilities_for`/`metadata_for`. The
`Effect` variant carries only a serializable **tag**; the registry maps the
tag to a `cards`-provided fn.

## Design

### `card-dsl`

Add one generic variant:

```rust
Effect::Native { tag: &'static str }
```

plus a `native(tag)` builder. This is the **only** variant ever added for
this purpose; single-use logic never touches the enum again. The existing
serde round-trip test must still pass (a `&'static str` is trivially
serializable).

### `game-core`

- `pub type NativeEffectFn = fn(&mut Cx, &EvalContext) -> EngineOutcome;`
- Promote `Cx` (`{ state: &mut GameState, events: &mut Vec<Event> }`) to
  `pub`, documented as the **effect-resolution context** — the surface an
  effect mutates through. `EvalContext` is already `pub`. Passing `Cx`
  (rather than the two refs separately) keeps migrated card-local fns
  isomorphic to the evaluator arms they replace and lets them call the
  existing `Cx`-taking helpers unchanged once those are `pub`.
- `CardRegistry` gains `native_effect_for: fn(&str) -> Option<NativeEffectFn>`.
- evaluator `Effect::Native { tag }` arm:

  ```rust
  Effect::Native { tag } => {
      let Some(reg) = card_registry::current() else {
          return EngineOutcome::Rejected {
              reason: format!("Native effect {tag:?}: no card registry installed").into(),
          };
      };
      let Some(f) = (reg.native_effect_for)(tag) else {
          return EngineOutcome::Rejected {
              reason: format!("Native effect {tag:?}: no handler registered").into(),
          };
      };
      f(cx, eval_ctx)
  }
  ```

  Loud reject on absent registry / unknown tag — the established
  `card_registry::current()` rejection pattern; no silent no-op.
- Promote the helpers a migrating card calls to `pub`: `location_id_by_code`,
  `reveal_location`. (`shortest_first_steps` stays `pub(crate)` until C3c
  needs it — out of scope here.)

### `cards`

- `fn registry_native_effect_for(tag: &str) -> Option<NativeEffectFn>` — a
  match over tags → card-local fns — wired into `REGISTRY` as the new
  `native_effect_for` field.
- Tag convention: `"<cardcode>:<name>"`, e.g. `"01108:board-build"`.
- The Rust logic lives **card-locally** in the card module.

## Proof + cleanup (this PR)

Migrate `act_01108`'s reverse board build off the three single-use
variants onto a card-local native fn:

```rust
// act_01108.rs
pub fn abilities() -> Vec<Ability> {
    vec![on_event(EventPattern::ActAdvanced, EventTiming::After,
                  native("01108:board-build"))]
}

fn board_build(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    // put set-aside locations into play; relocate all investigators to the
    // Hallway (01112); remove the Study (01111) — the three former arms,
    // verbatim, now card-local.
    ...
}
```

Then **remove** `Effect::{PutSetAsideLocationsIntoPlay, RelocateAllInvestigators,
RemoveLocationFromGame}`, their builders, and their evaluator arms. Keep
`Effect::AdvanceCurrentAct` (a genuinely reusable framework op, used by
`act_01110`).

## Testing

- **Evaluator unit tests:** `Effect::Native` happy path (registered tag
  runs and mutates), unknown-tag reject, no-registry reject.
- **`act_01108` card test:** unchanged assertions on the board-build
  outcome (set-aside drained into play, investigators relocated to 01112,
  Study removed) — now exercising the native path.
- **Integration** (`crates/cards/tests/`): the act-1 advance still builds
  the board end-to-end (existing coverage continues to pass).

## Out of scope

- The agenda 01107 effects (C3c, [#232](https://github.com/talelburg/eldritch/issues/232)) — the next consumer.
- **Suspending** native effects (returning `AwaitingInput` mid-resolution).
  Native effects are fire-to-completion (`Done`/`Rejected`) in this cut,
  matching the forced-trigger path; deferred until a consumer needs it.
