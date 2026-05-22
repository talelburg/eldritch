# Phase 4 — Scenario plumbing

## Status

🟡 In progress. Design pass complete 2026-05-21. First four PRs merged: `#103` unified window stack as PR #129, `#74` ScenarioModule skeleton as PR #130, `#72` encounter deck state as PR #132, and `#126` Revelation DSL + on-draw resolution as PR #133. Remaining: `#127`, `#69`, `#70`, `#71`, `#128`, `#73`.

## Goal

A synthetic toy scenario plays setup → resolution in tests, demonstrating that the engine drives all four phases through real scenario data — encounter deck draws, treachery + enemy resolution, hunter movement, doom progression, and act/agenda transitions.

## Issues (10 — 7 originals retained or rescoped, 3 new; `#75` migrated to Phase 9)

| # | Title | Notes |
|---|---|---|
| `#127` | enemy spawn rules (`Spawn { location: SpawnLocation }`, engagement-on-spawn, `EventPattern::EnemySpawned`) | Split out of `#69`. First consumer is a synthetic spawn-bearing enemy. |
| `#69` | Mythos phase content (draw + resolve + Surge) | Composes `#72` + `#126` + `#127`: each investigator draws 1 from the encounter deck, resolves it as treachery or enemy spawn, handles Surge by drawing another. |
| `#70` | Upkeep phase content | Ready cards, draw 1, gain 1 resource. Folds in `GameState.round: u32` incremented at Mythos start; becomes the load-bearing counter for any future round-end hook. |
| `#71` | Enemy phase: engagement attacks | **Rescoped** to engagement-attacks only (small PR). Iterates engaged enemies, fires each one's `enemy_attack`. |
| `#128` | Hunter movement | Split out of `#71`. `Prey` enum on `Enemy`; BFS over location-connection graph; move + engage-on-arrival. Ambiguous shortest paths prompt the active investigator via `AwaitingInput` + `InputResponse::PickLocation`. |
| `#73` | act + agenda + doom + threshold-advance | Kept whole. Doom +1 at Mythos start, threshold-driven agenda advance, act-advance condition emits `ActAdvanced`, end-of-deck → `ScenarioWon` / `ScenarioLost`. |

### Closed

| # | Title | PR | Notes |
|---|---|---|---|
| `#103` | unified window stack (player + reaction) | #129 | Foundational refactor of #52 machinery; ships unified `open_windows` stack. |
| `#74` | scenario module skeleton: `ScenarioModule` + `ScenarioRegistry` | #130 | `ScenarioId` / `Resolution` / `ScenarioModule` / `ScenarioRegistry` in `game-core`; synthetic fixture in `scenarios`; engine post-apply hook with parameterized `apply_with_scenario_registry` helper for test mocking. |
| `#72` | encounter deck state | #132 | `GameState.encounter_deck: VecDeque<CardCode>` + `encounter_discard: Vec<CardCode>`. Helpers `shuffle_encounter_deck` / `reshuffle_encounter_discard` / `draw_encounter_top` mirror the existing player-deck pattern in `engine/dispatch.rs`. `EngineRecord::EncounterDeckShuffled` + `Event::EncounterDeckShuffled` are additive siblings to the existing `DeckShuffled` (which stays player-deck-only). |
| `#126` | DSL `Trigger::Revelation` + `EventPattern::CardRevealed` + on-draw resolution | #133 | `Trigger::Revelation` and `EventPattern::CardRevealed { card_type: Option<CardType> }` land in `card-dsl`. Engine: `Event::CardRevealed` + `EngineRecord::EncounterCardRevealed` + `encounter_card_revealed` dispatch handler. Documented exception to validate-first / mutate-second contract (early `Event::CardRevealed` emission is the rules-correct interposition point for Before-timing reactions). Enemy arm stubbed for #127 to flip. First consumer: synthetic treachery in `crates/scenarios/src/test_fixtures/synth_cards.rs` with effect "gain 1 resource" (chose existing `Effect::GainResources` over a new lose-resources primitive — corpus has no in-scope two-consumer case). |

### Moved out of Phase 4

| # | Title | New home |
|---|---|---|
| `#75` | campaign log + `Fact` enum + scenario sequencing | Phase 9. Phase 4's `apply_resolution` returns a typed `Resolution` and applies XP/trauma directly; the `Fact` log and `next_scenario` orchestration land alongside Night of the Zealot. |

### Still unmilestoned (concrete-consumer-first)

- `#56` Study (01111) — waits on a "location abilities DSL + reveal effects" issue (also unfiled). Picked up together when location-bearing content is in scope.
- Trigger indexing (perf) — `#52` deferral, resurfaces when boards grow.

## Ordering (Shape B)

| # | PR / planned step | Why this slot |
|---|---|---|
| 1 | `#103` unified window stack | ✅ PR #129. Foundational refactor of `#52` machinery. Every subsequent phase-content PR opens windows; doing this first means each plugs into a stable shape rather than retrofitting twice. |
| 2 | `#74` `ScenarioModule` + registry + synthetic fixture stub | ✅ PR #130. Defines the shape every later issue conforms to. Synthetic fixture: 1 location, 1 investigator, one-line resolution predicate (`phase == Investigation && round >= 1`). Engine learns to call `detect_resolution` post-`apply`. |
| 3 | `#72` encounter deck state | ✅ PR #132. Sets up the data Mythos will draw from. Helpers in `crates/game-core/src/engine/dispatch.rs` mirror the existing player-deck shape. |
| 4 | `#126` DSL `Trigger::Revelation` + on-draw path | ✅ PR #133. Lands the DSL primitive in isolation. First consumer is a synthetic treachery in the fixture. |
| 5 | `#127` enemy spawn rules | First consumer is a synthetic spawn-bearing card. |
| 6 | `#69` Mythos phase content | Composes 3 + 4 + 5. |
| 7 | `#70` Upkeep phase content | Ready / draw / gain 1 / round counter bump. |
| 8 | `#71` Enemy phase: engagement attacks | Small PR; reuses `enemy_attack`. |
| 9 | `#128` Hunter movement | Larger PR with its own design (Prey enum, BFS, PickLocation ambiguity). |
| 10 | `#73` act + agenda + doom + threshold-advance | Wires into Mythos start (doom +1), engine state conditions (act-advance), end-of-deck win/lose events. |
| 11 | Phase-4 closing demo | Synthetic scenario plays setup → resolution end-to-end as an integration test in `crates/scenarios/tests/`. May fold into the previous PR. |

`#72`, `#126`, `#127` are independent of each other and could land in parallel. `#73` is independent of phase content and could slip earlier. `#103` and `#74` could swap, but `#103`-first means `#74`'s engine integration uses the unified window stack directly.

## Decisions made (design pass 2026-05-21)

- **Unified window stack shape (`#103`, PR #129).** `GameState.open_windows: Vec<OpenWindow>` replaces the old `in_flight_reaction_window: Option<ReactionWindow>` shape. Multi-window nesting is structural. Phase-content PRs (`#69` / `#70` / `#71`) plug into this stack via `queue_reaction_window` (push) and `close_reaction_window_at` (remove by index); the `WindowKind` enum is `#[non_exhaustive]`, so adding new variants (`BetweenInvestigatorTurns`, `BeforeEnemyAttack`, etc.) at consumer-PR time is non-breaking.
- **`close_reaction_window_at` takes an explicit window index (`#103`, PR #129).** Phase-content PRs that push pure-Fast-gating windows (`BetweenPhases`, etc.) on top of a draining reaction window MUST resolve the close target via `GameState::top_reaction_window_index()`, not via `last()` / `pop()`. The stack would otherwise corrupt: `top_reaction_window_mut()` skips empty-`pending_triggers` windows but `pop` doesn't, so a `BetweenPhases-on-top-of-ReactionWindow` stack would close the wrong window.
- **Card-level Fast restrictions are NOT engine-enforced (`#103`, PR #129).** The engine's PlayCard gate enforces the type-level Fast rules from Rules Reference page 11 (events: any window where `fast_actors` permits; assets: owner-only). Card-level restrictions like Working a Hunch's "Play only during your turn" are out of scope — a future DSL primitive will enforce them. Don't assume the engine prevents a Fast event from being played by a non-owner when a window permits.

- **Engine resolution hook is a parameterized helper (`#74`, PR #130).** `apply()` is a one-liner over `apply_with_scenario_registry(state, action, scenario_registry::current())`. Engine unit tests pass a locally-constructed mock `ScenarioRegistry` to the parameterized variant; the process-global `OnceLock` is only touched by one test (the idempotent-install test in `scenario_registry`) and by the `scenarios::tests::synthetic_resolution` integration test. The parameterized helper is intentionally **not** re-exported at `game_core`'s crate root — it lives at `game_core::engine::apply_with_scenario_registry`, signalling it's a test-mocking escape hatch rather than a peer to `apply()`. Pattern is the recommended shape for future engine ↔ registry interactions where unit tests would otherwise contend on `OnceLock`.
- **`Resolution` is `Won { id: String } / Lost { reason: String }` (`#74`, PR #130).** String payloads stand in for Phase-9's typed campaign-log `Fact` enum. Both variants kept `#[non_exhaustive]` so Phase 9 can extend without breaking Phase-4 consumers. `id` was chosen over `branch` / `resolution_id` because ArkhamDB's resolution identifiers (`R1` / `R2` / `R3` / `R4`) are conventionally called "resolution IDs" in the source data, and `id` reads cleanly in pattern-match positions (`Won { id }` vs `Won { branch }`). The `#[non_exhaustive]` annotation protects variant shape but not field names — a future rename would be breaking. Worth re-examining when the first real scenario (Phase 7 Gathering) lands enough resolution variants to confirm the name in context.
- **`apply_resolution` is called by the engine right after `ScenarioResolved` (`#74`, PR #130).** Same `apply()` call, same events buffer. Action-log replay reproduces XP / trauma changes deterministically. `apply_resolution` is a `fn` (not `Fn`/`FnMut`), so by signature it cannot reject — Phase 9 inherits the constraint that resolution effects must be infallible at the type level. If Phase 9 needs to surface "couldn't apply trauma because X," it'll need either a degraded `Event::ScenarioResolutionFailed` and continue-anyway, or engine-side pre-validation before the call. Idempotency latch is deferred — see `#131`.
- **`scenarios::test_fixtures` defaults on, including in `server` (`#74`, PR #130).** Empirically necessary: cargo compiles `scenarios` as a normal dependency of `scenarios/tests/*.rs` integration binaries (not with `cfg(test)`), so the `#[cfg(any(test, feature = "test_fixtures"))]` gate would otherwise be inactive there. As a side effect, `crates/server/Cargo.toml` (which has `scenarios = { path = "../scenarios" }` without `default-features = false`) compiles the synthetic fixture into the production binary. Today it's harmless dead code; the cleanup window is **when Phase 7 ships the first real scenario** — at that point `server` should add `default-features = false` and the fixture stays test-only. File-grep anchor: `default = ["test_fixtures"]` in `crates/scenarios/Cargo.toml`.

- **Additive sibling for `DeckShuffled` (`#72`, PR #132).** Encounter deck shuffles ride a new `EngineRecord::EncounterDeckShuffled` / `Event::EncounterDeckShuffled` rather than renaming or tagging the existing `DeckShuffled` (which stays player-deck-only). Trade-off: one variant says "player" implicitly, the other says "encounter" explicitly. Worth re-examining if act / agenda decks join the family — at that point a tagged `DeckKind` refactor becomes load-bearing. Companion convention: the mid-handler reshuffle path (`reshuffle_encounter_discard` called from `draw_encounter_top` on empty deck) does NOT push the `EngineRecord` — mirrors the player-deck pattern where replay determinism comes from the seeded RNG, not log entries. The `EngineRecord` variant is reserved for explicit "shuffle X into the encounter deck" effects.

- **`EventPattern::CardRevealed { card_type: Option<CardType> }` (`#126`, PR #133).** Chose `card_type` narrowing over the `EnemyDefeated`-mirror `by_controller: bool`. Encounter draws are engine-driven, not card-controlled, so `by_controller` doesn't fit the semantics; treachery-vs-enemy narrowing is the load-bearing distinction for hypothetical Forewarned-style listeners. The first real listener (Phase-7+) gets to confirm or extend.
- **`Event::CardRevealed` emits BEFORE Revelation resolves (`#126`, PR #133).** Intentional ordering: Before-timing reaction listeners (#52's machinery; not yet wired) need the event to fire first so they can interpose / cancel. Documented exception to validate-first / mutate-second — the only state-changing pre-emit op is the encounter-deck draw, which is the load-bearing 'reveal' moment per the Rules Reference. Precedent: `play_card`'s mid-resolution caveat in CLAUDE.md. `#127`'s spawn-handler PR retains the same shape (the enemy arm replaces the stub reject, but the reveal-before-spawn ordering stays).


- **Toy scenario = synthetic fixture, not The Gathering.** A 1-location, 1-enemy, 1-act, 1-agenda fixture lives under `crates/scenarios/src/test_fixtures/` (gated `#[cfg(any(test, feature = "test_fixtures"))]`) and serves only as Phase-4's demo. The Gathering stays the Phase-7 content goal. Rationale: keeps Phase 4 infra-focused — synthetic content needs only the primitives, not Study / Hallway / Attic / Cellar / Parlor / Ghoul Priest / specific NotZ-I treacheries. Mirrors Phase 3's "build minimal infra each card needs, ship the card" pattern flipped to infra-side.
- **Unified window stack (`#103` × `#52`).** `Vec<OpenWindow>` on `GameState` replaces the single `in_flight_reaction_window: Option<...>`. Each `OpenWindow` carries (a) kind/timing, (b) queue of pending reaction triggers, (c) the set of investigators who may submit Fast actions during it. Rules Reference treats "player window" as the umbrella; the engine should too. `#52`'s `FinishContinuation` machinery survives — driver pushes/pops instead of manipulating an Option. Worth the refactor cost up-front because every Phase-4 phase-content PR opens windows; retrofitting twice is worse than once.
- **`#75` campaign log migrates to Phase 9.** Phase 4 demonstrates a single scenario plays setup → resolution; the typed `Fact` log + `next_scenario` orchestration only have a real consumer when Night of the Zealot lands. `apply_resolution` in Phase 4 returns a typed `Resolution` value and applies XP/trauma directly to investigators on existing state.
- **`#69` splits into three.** `#126` (Revelation DSL), `#127` (spawn rules), `#69` rescoped to "Mythos phase loop on top of those primitives." Mirrors the Phase-3 `#53` split (DSL primitive + accumulator + card consumer in three PRs).
- **`#71` splits into two.** Engagement attacks (small, reuses `enemy_attack`) separated from `#128` Hunter movement (Prey enum + BFS + PickLocation). Independent acceptance per PR.
- **`#73` stays whole.** Act + agenda + doom + threshold-advance share a state shape; splitting would mean 4 tiny PRs with duplicated context.
- **Location abilities + `#56` stay unmilestoned.** Phase-4 toy is synthetic and needs no location abilities. File "location abilities DSL surface + reveal effects" only when a location-bearing scenario forces it (likely Phase 7 Gathering); pick it up with `#56` together. Matches `#52` trigger-indexing deferral, `#63` max-1-commit deferral, `#55` elder-sign split — concrete consumer first.
- **`ScenarioModule` is `fn`-pointers + a static registry**, mirroring `CardRegistry`. No `dyn`, no `Box`. Hosts call `scenarios::install(scenarios::REGISTRY)` once at startup. Tests install only what they need. Function pointers don't serialize, so `GameState` carries a serializable `ScenarioId` and the engine looks the module up via the registry — same pattern `CardRegistry` uses with `CardCode`. Survives action-log replay.
- **Round counter folded into `#70`.** `GameState.round: u32` incremented at the start of Mythos. Lazy `usage_limit` reset from `#55` stays correct under this; the counter is load-bearing for any future round-end hook. The exact Rules Reference clause governing round boundaries should be cited verbatim in the `#70` PR.

## Open questions (settled enough to start; concrete answers come with the relevant PR)

- **Window-stack invariants.** Exact push/pop points beyond phase boundaries and the reaction points `#52` already opens. Acceptable to start with that minimal set; expand when a card forces it.
- **Hunter target-selection details (`#128`).** Default `Prey { Lowest(Stat), Highest(Stat), LeadInvestigator, Bearer, Custom(fn) }`. BFS shortest path; ambiguous-path resolution via `InputResponse::PickLocation` from the active investigator (cite the exact Rules Reference clause in the PR description before locking the shape).
- **Resolution-fired idempotency latch.** `apply()` re-calls `detect_resolution` on every `Done` outcome with no engine-side guard (tracked as `#131`). Acceptable for Phase 4 (synthetic fixture's `apply_resolution` is a no-op) but the first real `apply_resolution` (Phase 9 XP / trauma) will stack effects unless the latch lands first. Likely shape: `GameState.resolution: Option<Resolution>` checked at the top of `fire_scenario_resolution`. Defer until Phase 9.
- **`AwaitingInput`-skip contract isn't positively tested.** The hook fires on `Done` only; `AwaitingInput` and `Rejected` both skip. `Rejected` has a positive test, `AwaitingInput` does not (the cleanest `AwaitingInput`-producer in dispatch is `PerformSkillTest` at the commit window, which needs a fuller `TestGame` setup than the existing scenario-resolution tests use). Worth adding when a future PR is already touching that area; not blocking.

## Dependencies

Phase 3 — needs the skill-test machinery, action handlers, DSL evaluator, reaction-window machinery (which `#103` then refactors into the unified stack). Specifically Phase-3 issues that gate Phase-4 work:

- `#52` reaction windows — Phase-4 phase content emits events that triggered abilities react to.
- `#54` `OnEvent` trigger — DSL extension for the above.
- `#62` player decks (already shipped) — `#70` Upkeep draws from them.
- `#67` enemy state (already shipped) — `#71` engagement attacks and `NEW-C` hunter movement rely on it.

## What "done" looks like

A custom toy scenario (1 location, 1 enemy, 1 act, 1 agenda, 1 synthetic treachery, 1 spawn-bearing enemy card) plays setup through a resolution in `crates/scenarios/tests/`:

- Engine cycles Mythos → Investigation → Enemy → Upkeep → Mythos.
- Mythos draws the encounter deck and resolves treacheries / spawns enemies; Surge handled.
- Investigation lets the investigator move, investigate, fight, evade — all reusing Phase-3 actions.
- Enemy phase: engaged enemies attack; hunters move toward Prey.
- Upkeep: ready, draw, gain resource, round counter bumps.
- `#73`: doom advances on the agenda each Mythos start; threshold met → `AgendaAdvanced`. Act advances when its condition is met; end-of-deck on either side emits `ScenarioWon` / `ScenarioLost`.
- `detect_resolution` fires at the right moment; `apply_resolution` returns a typed `Resolution` and applies XP/trauma.
- Mid-scenario state serializes + replays identically via the action log (verified by a replay test).
