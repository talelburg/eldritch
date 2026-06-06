# Collapse marker `WindowKind` variants into `PlayerWindow(PhaseStep)` (#140)

## Problem

`WindowKind` (`crates/game-core/src/state/game_state.rs`) currently mixes
three kinds of variant:

| Variant | Payload | Read by `trigger_matches`? | Continuation | Opened in production? |
|---|---|---|---|---|
| `AfterEnemyDefeated { enemy, by }` | yes | **yes** — `by` → `EnemyDefeated.by_controller` | none (`Done`) | yes |
| `BetweenPhases { from, to }` | yes | no | none (`Done`) | **no — dead** |
| `MythosAfterDraws` | none | no | `mythos_phase_end` | yes |
| `UpkeepBegins` | none | no | `upkeep_resume` | yes |
| `BeforeInvestigatorAttacked` | none | no | cursor + resolve attacks | yes |
| `AfterAllInvestigatorsAttacked` | none | no | `enemy_phase_end` | yes |
| `InvestigationBegins` | none | no | `begin_investigator_turn` | yes |
| `InvestigatorTurnBegins` | none | no | none (`Done`) | yes |

Two problems:

1. **`BetweenPhases` is dead.** It is never opened by any production path —
   the "phase machine opens this at every transition" machinery described in
   its doc-comment was never built; each phase got its own specific marker
   instead. It survives only as a test fixture (a generic "Fast-allowed window"
   stand-in) and exhaustiveness/serde boilerplate.
2. **Six payload-less markers sit as siblings of the one genuinely
   event-carrying variant**, obscuring that they are all the same shape: a
   printed player window at a Rules-Reference timing step, distinguished only
   by *which* step.

The issue set a bar of "≥3 phase-content PRs landed before refactoring"; six
markers now exist, so the data is in.

## Decision

Collapse the six markers into a single `WindowKind::PlayerWindow(PhaseStep)`,
keep the event-carrying `AfterEnemyDefeated` distinct, and delete the dead
`BetweenPhases`.

`PlayerWindow` carries **only** `PhaseStep` — not the issue's originally
suggested `{ phase, step }`. Each `PhaseStep` already uniquely determines its
phase, nothing reads a window's phase (the engine reads `cx.state.phase`), so a
`phase` field would be redundant state that can desync.

## Type shape

`crates/game-core/src/state/game_state.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WindowKind {
    AfterEnemyDefeated { enemy: EnemyId, by: Option<InvestigatorId> },
    PlayerWindow(PhaseStep),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PhaseStep {
    MythosAfterDraws,
    UpkeepBegins,
    BeforeInvestigatorAttacked,
    AfterAllInvestigatorsAttacked,
    InvestigationBegins,
    InvestigatorTurnBegins,
}
```

`PhaseStep` takes the same derives + `#[non_exhaustive]` as `WindowKind`. Each
marker's existing Rules-Reference doc-comment (timing-step citation,
continuation description) moves onto the corresponding `PhaseStep` variant.

## Changes

1. **Definition** — replace the six markers + `BetweenPhases` with the two
   enums above; migrate doc-comments.

2. **Construction sites** — rewrite each `WindowKind::Marker` to
   `WindowKind::PlayerWindow(PhaseStep::Marker)`. Production sites:
   `engine/dispatch/encounter.rs`, `engine/dispatch/phases.rs` (several),
   `engine/dispatch/reaction_windows.rs`.

3. **`trigger_matches`** (`reaction_windows.rs`) — the six-name false-arm
   collapses to one `WindowKind::PlayerWindow(_)` arm (still `false` for all
   patterns — none of these are event-reaction windows). The
   `AfterEnemyDefeated` arm is unchanged.

4. **`run_window_continuation`** (`reaction_windows.rs`) — restructure to:

   ```rust
   match kind {
       WindowKind::PlayerWindow(step) => match step {
           PhaseStep::MythosAfterDraws => { /* unchanged body */ }
           PhaseStep::UpkeepBegins => { /* unchanged body */ }
           PhaseStep::BeforeInvestigatorAttacked => { /* unchanged body */ }
           PhaseStep::AfterAllInvestigatorsAttacked => { /* unchanged body */ }
           PhaseStep::InvestigationBegins => { /* unchanged body */ }
           PhaseStep::InvestigatorTurnBegins => EngineOutcome::Done,
       },
       WindowKind::AfterEnemyDefeated { .. } => EngineOutcome::Done,
   }
   ```

   Bodies (including the skill-test-in-flight `unreachable!` guards) are
   preserved verbatim. `BetweenPhases`'s old `Done` arm is dropped — its only
   sibling in that arm, `InvestigatorTurnBegins`, keeps its existing `Done`.

5. **Dead-fixture migration** — the test sites that injected `BetweenPhases` as
   a generic "Fast-allowed window whose continuation is a no-op" migrate to
   `PhaseStep::InvestigatorTurnBegins`: the payload-less marker with the same
   `Done`-continuation shape, so behavior is preserved. Sites:
   - `crates/game-core/src/test_support/builder.rs` — two `with_open_window` tests.
   - `crates/game-core/tests/reaction_windows.rs` — the empty-window-on-stack test.
   - `crates/cards/tests/fast_play.rs` — six uses.

6. **Serde tests** — the six per-marker round-trip tests in `game_state.rs`
   collapse to one representative `PlayerWindow(PhaseStep::…)` round-trip
   (the `AfterEnemyDefeated` round-trip stays). The `event.rs` serde test that
   used `BetweenPhases` migrates to a `PlayerWindow` shape.

## Serde / replay safety

`WindowKind` appears only in `Event` and `GameState.open_windows`, never in any
`Action`. Replay is from the flat `Vec<Action>` log, so the changed JSON shape
does not break determinism. `GameState` is serialized to the web client, but
client and server ship together — there is no persisted save format to migrate.

## Non-goals

- No change to `AfterEnemyDefeated` or its `by_controller` routing.
- No change to `OpenWindow`, `FastActorScope`, or Fast-eligibility logic.
- No new timing points — the refactor only restructures the existing eight.

## Success criteria

Behavior-preserving refactor. Success = the full CI gauntlet stays green
(`test` under `RUSTFLAGS="-D warnings"`, `clippy --all-targets --all-features
-D warnings`, `fmt --check`, `doc -D warnings`, `wasm-build`), with the only
intended behavior delta being that `WindowKind::BetweenPhases` no longer exists.
