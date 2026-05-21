# Phase 4 — Scenario plumbing

## Status

🟡 In progress. Design pass complete 2026-05-21. First PR (`#103` unified window stack) merged 2026-05-21 as PR #129. Remaining: `#74`, `#72`, `#126`, `#127`, `#69`, `#70`, `#71`, `#128`, `#73`.

## Goal

A synthetic toy scenario plays setup → resolution in tests, demonstrating that the engine drives all four phases through real scenario data — encounter deck draws, treachery + enemy resolution, hunter movement, doom progression, and act/agenda transitions.

## Issues (10 — 7 originals retained or rescoped, 3 new; `#75` migrated to Phase 9)

| # | Title | Notes |
|---|---|---|
| `#103` | unified window stack (player + reaction) | **Rescoped** to subsume `#52`'s `in_flight_reaction_window` Option into a `Vec<OpenWindow>` stack carrying queued reaction triggers AND the Fast-action gate. First step in Phase 4; every later phase-content PR plugs into the new shape. |
| `#74` | scenario module skeleton: `ScenarioModule` + `ScenarioRegistry` | `ScenarioModule` is a static struct of `fn` pointers mirroring `CardRegistry`; `ScenarioRegistry` looks up modules by `ScenarioId`. Engine calls `detect_resolution` after each `apply`. Ships with a synthetic test fixture under `crates/scenarios/src/test_fixtures/`. |
| `#72` | encounter deck state | Shuffled deck of treacheries + enemies; deterministic via the deck-shuffle RNG path. Empty → shuffle discard back. |
| `#126` | DSL `Trigger::Revelation` + `EventPattern::CardRevealed` + on-draw resolution path | Split out of `#69`. First consumer is a synthetic treachery in the test fixture (e.g. "lose 1 resource"). |
| `#127` | enemy spawn rules (`Spawn { location: SpawnLocation }`, engagement-on-spawn, `EventPattern::EnemySpawned`) | Split out of `#69`. First consumer is a synthetic spawn-bearing enemy. |
| `#69` | Mythos phase content (draw + resolve + Surge) | Composes `#72` + `#126` + `#127`: each investigator draws 1 from the encounter deck, resolves it as treachery or enemy spawn, handles Surge by drawing another. |
| `#70` | Upkeep phase content | Ready cards, draw 1, gain 1 resource. Folds in `GameState.round: u32` incremented at Mythos start; becomes the load-bearing counter for any future round-end hook. |
| `#71` | Enemy phase: engagement attacks | **Rescoped** to engagement-attacks only (small PR). Iterates engaged enemies, fires each one's `enemy_attack`. |
| `#128` | Hunter movement | Split out of `#71`. `Prey` enum on `Enemy`; BFS over location-connection graph; move + engage-on-arrival. Ambiguous shortest paths prompt the active investigator via `AwaitingInput` + `InputResponse::PickLocation`. |
| `#73` | act + agenda + doom + threshold-advance | Kept whole. Doom +1 at Mythos start, threshold-driven agenda advance, act-advance condition emits `ActAdvanced`, end-of-deck → `ScenarioWon` / `ScenarioLost`. |

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
| 2 | `#74` `ScenarioModule` + registry + synthetic fixture stub | Defines the shape every later issue conforms to. Fixture starts as `setup() = empty state with 1 location`, `detect_resolution = None`. Engine learns to call `detect_resolution` post-`apply`. |
| 3 | `#72` encounter deck state | Independent of `#74`'s API beyond GameState. Sets up the data Mythos will draw from. |
| 4 | `#126` DSL `Trigger::Revelation` + on-draw path | Lands the DSL primitive in isolation. First consumer is a synthetic treachery in the fixture. |
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
- **`detect_resolution` polling frequency.** Currently "after each `apply`." Correct but potentially expensive at scale. Defer perf concern until a real scenario is observable; flag inline like `#52`'s trigger-indexing deferral.
- **`Resolution` value shape.** Enum like `Resolution::{Won { resolution_id }, Lost { reason }}`. XP/trauma application reads off the resolution; the typed `Fact` log is Phase 9's job. How `apply_resolution` hands surviving-investigator state to Phase 9 is Phase-9's design (probably a `ScenarioOutcome` return value).
- **Mid-scenario serialization.** Action-log replay must reproduce final state. Function pointers don't serialize; serializable `ScenarioId` + registry lookup keeps replay deterministic, mirroring `CardCode` / `CardRegistry`. Document explicitly in `#74`.
- **Synthetic fixture as a teaching example.** Worth a small comment budget explaining "this is the minimum a scenario needs to exist." Helps the Phase-7 Gathering implementer see the shape without grokking The Gathering's content first.

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
