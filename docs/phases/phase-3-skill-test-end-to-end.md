# Phase 3 — Skill-test end-to-end

## Status

🟡 In progress. 21 closed / 2 open as of 2026-05-20.

## Goal

Full skill test sequence runs through the engine in tests — with real-card metadata + abilities end-to-end.

## Issues

### Closed (21)

| # | Title | Notes |
|---|---|---|
| `#19` | deterministic `ChoiceResolver` | Test infra; ships ahead of first engine consumer (`#63`). `commit_cards` stubbed pending commit-window response shape. |
| `#55` | Roland Banks reaction (01001) | Reaction half only. Elder-sign + dynamic skill-test modifier DSL spun off to `#118` (Phase 7). First real-card consumer of `#52` reaction-window machinery; introduced `Ability.usage_limit` primitive. |
| `#63` | skill-test card commits from hand | First engine consumer of `#19`. Splits `resolve_skill_test` → `start_skill_test` + `finish_skill_test`; adds `state.in_flight_skill_test`, `SkillTestFollowUp`, `InputResponse::CommitCards`. |
| `#37` | Magnifying Glass (01030) | First new card after the Phase-3 demo; composes `#87` + `#45`. |
| `#38` | Hyperawareness (01034) | Exercises `Trigger::Activated` + `ThisSkillTest` push end-to-end. |
| `#39` | Deduction (01039) | First consumer of `Trigger::OnSkillTestResolution`; also wires `Effect::If` evaluator + adds `Condition::SkillTestKind` for the kind narrowing. |
| `#45` | per-skill-test-kind modifier scope | Adds `ModifierScope::WhileInPlayDuring(SkillTestKind)`. |
| `#47` | DSL evaluator (Effect → state mutation) | Initial scaffold with stubs for non-leaf effects. |
| `#48` | chaos bag draw + token modifier resolution | `resolve_token` + `TokenResolution`. |
| `#49` | skill-test resolution flow | `resolve_skill_test`. |
| `#50` | Move action + action-point spending | First non-trivial player action. |
| `#51` | Investigate action | First skill-test-initiating action. |
| `#52` | reaction windows + trigger ordering | Engine machinery for `Trigger::OnEvent` reactions, mid-action firing per Rules Reference, state-machine refactor of `finish_skill_test` via `FinishContinuation`. First consumer is `#55`. |
| `#53` | DSL Activated trigger + cost primitives | `Trigger::Activated`, `Cost` enum, `Ability.costs`. |
| `#87` | per-instance card state | `CardInPlay` struct + `CardInstanceId` + counter. |
| `#88` | cards-registry binding | Cross-crate bridge. Option 3 (static `OnceLock` + `fn`-pointer struct). |
| `#89` | PlayCard action | Asset → in-play; event → discard. |
| `#92` | constant-modifier query during skill-test resolution | Closes the Phase-3 demo: Holy Rosary `+1 willpower` actually applies. |
| `#102` | `ThisSkillTest` modifier accumulator | Pushed + drained on `SkillTestEnded`. |
| `#112` | DSL `Trigger::OnSkillTestResolution` + tested-location plumbing | Split out of `#39` so the trigger variant lands on its own; `#39` consumes it. |
| `#54` | DSL `OnEvent` trigger | DSL surface only — `Trigger::OnEvent` + `EventPattern::EnemyDefeated` + `EventTiming::{Before, After}` + `on_event` builder. Firing (engine reaction-window machinery) lands in `#52`. |

### Open (2)

| # | Title | Notes |
|---|---|---|
| `#56` | Study location (01111) | Needs a location-state shape decision; thinnest issue body. |
| `#64` | skill-test after-resolution trigger window | Needs `#54` (OnEvent) + `#19`. Distinct semantic from `#112` — `#64` is a player-window-driven reactive trigger after the test ends; `#112` is a resolution-machinery trigger on committed cards. The `FinishContinuation::PostOnResolution` seat (added in `#52`) is the natural step boundary. |

## Ordering (Shape B)

The Phase-3 work was split into two arcs:

### Arc 1: the demonstration (closed 2026-05-15 to 2026-05-16)

Three PRs to demonstrate "skill test runs through the engine on real cards":

1. `#88` cards-registry binding (PR #94)
2. `#89` PlayCard + OnPlay (PR #95)
3. `#92` constant-modifier query (PR #98)

End state: Holy Rosary's `+1 willpower` applies during a real willpower skill test; Working a Hunch resolves on-play end-to-end.

### Arc 2: completing the milestone (in progress)

Cards rather than infra-first. Build the minimal infra each card needs, ship the card, repeat. Gives steady "new card works" wins instead of ~8 infra PRs before any new card lands.

| # | PR / planned step | Status |
|---|---|---|
| 1 | `#87` per-instance card state | ✅ PR #99 |
| 2 | `#45` per-skill-test-kind scope | ✅ PR #100 |
| 3 | `#37` Magnifying Glass | ✅ PR #101 |
| 4 | `#53` Activated trigger + cost primitives | ✅ PR #104 |
| 5 | `#102` `ThisSkillTest` accumulator | ✅ PR #105 |
| 6 | `#38` Hyperawareness | ✅ PR #106 |
| 7 | `#19` ChoiceResolver | ✅ PR #109 |
| 8 | `#63` skill-test commits | ✅ PR #110 |
| 9 | `#112` `Trigger::OnSkillTestResolution` + tested-location plumbing | ✅ PR #113 |
| 10 | `#39` Deduction | ✅ PR #114 |
| 11 | `#54` OnEvent trigger | ✅ PR #115 |
| 12 | `#52` reaction windows | ✅ PR #116 |
| 13 | `#55` Roland Banks reaction | ✅ PR #120 |
| 14 | `#56` Study | needs location-state design; defer or build alongside Phase 4 |
| 15 | `#64` after-resolution trigger window | distinct from `#112`; reactive trigger window for OnEvent-style "after this test succeeds, …" cards |

## Decisions made

- **CardRegistry pattern (`#88`).** Engine cannot import the `cards` crate (would cycle); instead, `game_core::card_registry` holds a `OnceLock<CardRegistry>` with two `fn` pointers. `cards::REGISTRY` is the static value the host installs once at startup. Tests that don't touch card data don't need to install one. Considered four options; option 3 (static + `fn` pointer struct) won on test ergonomics. Tracked design alt-history: `#93` (three-crate `card-dsl` split) is the cleaner long-term layering; not blocked on by current work.
- **`#[non_exhaustive]` lifted on `CardMetadata`.** The only construction site is the pipeline-generated corpus in a different crate. Other types (enums) keep their `#[non_exhaustive]` because their guard is different.
- **`PlayCard` routing (`#89`).** Only `Asset` and `Event` are playable from hand. Every other `CardType` rejects with a single generic message — no special-case explanations. (User's call: "encoding future functionality in error messages.")
- **Per-instance card state (`#87`).** `Investigator.cards_in_play: Vec<CardInPlay>` replaces `Vec<CardCode>`. `CardInPlay { code, instance_id, exhausted, uses: BTreeMap<UseKind, u8>, accumulated_damage, accumulated_horror }`. `GameState.next_card_instance_id: u32` counter.
- **DSL `SkillTestKind` lives in `dsl.rs`**, not `state`. Keeps dsl's pure-types isolation (mirrors `Stat`'s position as a DSL classifier). The engine imports it.
- **`Trigger::Activated { action_cost: u8 }` + `Cost` enum (`#53`).** Action cost separated from arbitrary payment costs so validation and event emission stay clean. `Cost::DiscardCardFromHand` exists as a variant but the handler stubs it pending `#19`.
- **`#53` acceptance was split into two PRs.** The literal issue acceptance included "Hyperawareness lands using the new primitives." Splitting into `#53` (DSL + dispatch) + `#102` (`ThisSkillTest` accumulator) + `#38` (Hyperawareness card) keeps each PR focused, mirroring the `#88`/`#89` and `#45`/`#37` patterns.
- **Investigation-phase + active-investigator gate on `PlayCard`/`ActivateAbility` is overly strict for Fast abilities.** No-op for Phase-3 scope (only Investigation phase exists); `#103` lifts the gate when phase content lands.
- **`test-support` Cargo feature dropped (PR #104).** `pub mod test_support` is now unconditional. Surfaced during `#53` review when the integration-test binary failed to compile without `--all-features`. The feature gated nothing in practice (only consumer was `cards`, which always enabled it).
- **Event-sequence macro `assert_event_sequence!` added (PR #95).** The file's own "don't add preemptively" note had been waiting for a concrete first user; Working a Hunch's ordering test was that user.
- **`ChoiceResolver` shipped ahead of consumer (`#19`, PR #109).** Test seam (trait + `ScriptedResolver` + `drive` + `TestSession` + `TestGame::session()`) lands now; no engine path emits `AwaitingInput` yet. `ScriptedResolver::commit_cards` is a recorded stub that panics on resolve until `#63` finalizes the commit-window response variant(s). Diverged from the issue's `pick_target(id)` to concrete `pick_investigator` / `pick_location` matching the existing `InputResponse` variants. Pre-existing `#19` forward references in `dispatch.rs` / `dsl.rs` / `evaluator.rs` were rephrased to "no engine consumer landed yet" so they don't dangle after merge.
- **Always-suspend at the commit window (`#63`, PR #110).** Every skill test routes through `AwaitingInput` even when the active investigator's hand is empty. Engine uniformity over a "suspend only when hand non-empty" shortcut — the alternative would force every client and test to branch on whether the apply needs a resume. Test side absorbs the cost via `test_support::apply_no_commits`; every new skill-test-initiating action follows this pattern.
- **`SkillTestFollowUp` is an enum, not an `Effect` (`#63`, PR #110).** The action-specific success path (discover clue / damage enemy / disengage+exhaust / none) is captured as an enum on the in-flight record rather than a DSL `Effect`. The DSL doesn't yet have primitives for `damage_enemy` or "disengage + exhaust"; introducing them just to route Fight/Evade follow-ups would be premature DSL expansion. Revisit if a third primitive use case emerges or if `#39` Deduction's OnCommit logic wants a DSL effect.
- **Event ordering: action-specific follow-up fires BEFORE `SkillTestEnded` (`#63`, PR #110).** `SkillTestSucceeded → follow-up events → CardDiscarded* → SkillTestEnded`. Matches the `SkillTestEnded` event-doc text ("Cleanup events … precede this"); load-bearing for `#64`'s after-resolution trigger window and any future listener keying off the end marker as "all sub-effects already applied." Pinned by `investigate_canonical_event_sequence_pins_followup_before_test_ended`.
- **"Max 1 committed per skill test" not enforced (`#63`, PR #110).** Perception, Overpower, and Unexpected Courage all carry the constraint in their printed text but the engine accepts duplicate commits today. The general "card-level commit-limit constraint" needs the DSL to grow a primitive — wait until the second card with a non-trivial commit restriction lands before designing it. No separate tracking issue; resurfaces naturally when needed.
- **Upkeep hand-size limit tracked as `#111` (`#63`, PR #110).** `InputResponse::CommitCards` carries `u32` indices for wire-format symmetry with `PickIndex`. The engine assumes hand size stays below 256 (a `u8::try_from(...).expect(...)` justified by the preceding bound check); `#111` makes that structural by implementing the Arkham upkeep discard-to-max-hand-size step.
- **`Trigger::OnEvent` ships with both `Before` and `After` timings (`#54`, PR #115).** Reaction-window machinery in `#52` pre-committed to the natural "scan Before triggers, fire event, scan After triggers" shape. Shipping only `After` would force `#52` to grow the variant before it can do the canonical loop. `EventPattern` starts at one variant (`EnemyDefeated { by_controller: bool }`) since only Roland Banks (`#55`) consumes it in Phase 3; `by_controller: bool` instead of a `By` enum because a single-variant enum is noise — swap to enum the day a second "by" qualifier lands.
- **`Trigger::OnSkillTestResolution` is resolution-machinery, not a reaction window (`#112`, PR #113).** Deduction-shaped text ("if this skill test is successful while investigating, discover 1 additional clue at that location") doesn't fit `OnCommit` (outcome unknown at commit) and doesn't fit `#64`'s reactive window (no player decision). New variant fires inside `finish_skill_test` for committed cards, gated on actual outcome. Settles `#39` Deduction's routing question; keeps `#64` strictly for player-window-driven "after a test succeeds, you may …" cards. Event order pinned by `investigate_canonical_event_order_with_on_resolution`: action follow-up before OnResolution before discards, load-bearing for `#64` listeners that key off `SkillTestEnded` as "all sub-effects applied."
- **Reaction windows fire mid-action, not post-action (`#52`, PR #116).** Per the Rules Reference's "after… may be used immediately after that triggering condition's impact upon the game state has resolved," windows open between the impact and the surrounding action's next step. A first pass deferred windows to the action's outer apply boundary; this was rules-incorrect and got fixed mid-PR after the user asked for a rules-citation pass. `#64` (after-resolution reactive window) and any future card whose timing interacts with sibling effects depend on this shape — don't revert to the deferred design without re-litigating the rules clause.
- **`finish_skill_test` is a resumable state machine (`#52`, PR #116).** Mid-action window suspension needs the skill-test resolver to be re-entrant. The shape: `finish_skill_test` is now a commit-stage entry that runs validate → chaos token → action follow-up, advances `InFlightSkillTest.continuation`, and delegates to a `drive_skill_test` loop. The driver checks for queued reaction windows before each step and suspends/resumes via `close_reaction_window`. `FinishContinuation::{AwaitingCommit, PostFollowUp { succeeded }, PostOnResolution { succeeded }}` carries the outcome as variant payload so "outcome known iff past commit" is structural. The `PostOnResolution` step is the natural seat `#64` will hang its reactive window off.
- **All triggers route through the player; forced vs. optional is a flag, not auto-resolve (`#52`, PR #116).** Reaction-window resolution presents every pending trigger through `AwaitingInput`; `InputResponse::PickIndex(i)` fires the i-th, `Skip` closes the window. `PendingTrigger.forced` is enforced by `close_reaction_window` (Skip rejects while forced remain). Phase-3 has no Forced cards; the engine constructs `forced: false` everywhere and the DSL has no surface for setting it. The first Forced card adds the DSL primitive without engine churn — the scaffold is intentionally ready.
- **Rules Reference vendored at `data/rules-reference/` (`#52`, PR #116).** The PDF is in-repo with a `SOURCE.md` naming the upstream URL — FFG's `filer_public` CDN has restructured several times so a stable local path beats a link. The CLAUDE.md directive on consulting it for procedural-rules claims points at the local path. Quote-handling clause says "load-bearing clause verbatim; elision OK on decorative surrounding clauses; never substitute words" — that's the standard for engine doc-comments citing the Rules Reference.
- **Trigger indexing is a follow-up, not Phase 3 (`#52`, PR #116).** `scan_pending_triggers` walks every investigator's `cards_in_play` per window emission. For Phase-3 board sizes (a handful of cards per investigator) this is negligible. An index keyed by trigger kind becomes worth the invariant-maintenance cost when boards grow in Phase 4+. Tracked as a separate issue.
- **`#55` split into reaction (Phase 3) + elder-sign (Phase 7, `#118`) (PR #120).** Roland's `[reaction]` is the load-bearing Phase-3 piece (first real-card consumer of `#52` reaction windows). His `[elder_sign]` needs a `Trigger::ElderSign` variant plus a dynamic skill-test modifier DSL surface (numeric expressions over state — clues at controller's location), whose shape benefits from a second consumer (likely `.45 Automatic` / Cover Up in Phase 7). Single-PR Roland would have lumped two unrelated DSL additions; the split keeps each PR focused and avoids locking the dynamic-modifier shape on one consumer. The broader unification of damage/horror/clues onto `CardInPlay` is `#119` (unmilestoned cross-cutting).
- **Investigator card is a `CardInPlay` placed at scenario setup (`#55`, PR #120).** Per Rules Reference pages 4 ("Attach To") and 6 ("Clues"), the investigator card is a card in play under its owner's control. Putting it in `cards_in_play` lets the existing reaction-window scan, constant-modifier query, and any future ability walk pick up investigator abilities uniformly with assets — no bespoke "investigator-abilities" path. An earlier draft added `Investigator.card_code` as a back-pointer; removed during review because nothing reads it (the `CardInPlay.code` is the canonical identifier). When `#119` lands and the investigator's damage/horror/clues migrate to that `CardInPlay`, a `CardInstanceId` pointer may be wanted then — concrete consumer first.
- **`UsageLimit` primitive lives on `Ability`, counter lives on `CardInPlay` (`#55`, PR #120).** `Ability.usage_limit: Option<UsageLimit { count, period }>` with `UsagePeriod::Round` for "Limit once per round" (Rules Reference page 14). Per-instance storage on `CardInPlay::ability_usage` (keyed by ability index) matches the page-14 rule that a card leaving and re-entering play brings a new ability instance — the counter drops with the `CardInPlay` automatically. Lazy round-keyed reset (mismatched-round records read as 0) avoids needing a round-end hook, which matters because Phase 3 doesn't cycle rounds yet. `Skip` doesn't bump the counter — page 14 ties the count to *initiation*. Cancellation-counts-against-limit (also page 14) is flagged with a TODO near `bump_usage_counter` for when a cancellation primitive lands.

## Open questions

- **`#56` Study (location).** Locations have a state shape (shroud, clues, connections), but printed locations also have abilities — Reveal effects, on-enter triggers. The shape for location abilities isn't yet decided. Could land alongside Phase-4 scenario plumbing (`#74` scenario module skeleton) where the location-handling shape gets settled.

## Dependencies

Phases 0–2.

## What "done" looks like

Skill test runs through the engine in tests against real-card metadata + abilities, with the full Phase-3-tagged card set implemented:
- Magnifying Glass `+1 intellect while investigating` ✓
- Hyperawareness `[fast] Spend 1 resource: +1 stat for this skill test` ✓
- Deduction commit-from-hand triggered effect
- Roland Banks investigator with his reaction ability
- Study location with whatever its printed abilities require

End-to-end integration tests in `crates/cards/tests/` cover each card.

## Side work shipped during the Phase-3 timeline

Engine work that happened in parallel but isn't milestoned to Phase 3. Captured for narrative completeness; the milestone closure check doesn't gate on these.

| # | Title | Status | Why during Phase 3 |
|---|---|---|---|
| `#62` | player decks + hands + cards-in-play state | ✅ | Foundational for `#87` + `#89`. |
| `#67` | enemy state + Fight / Evade actions | ✅ | Natural continuation of `#50` Move. |
| `#78` | attack of opportunity + engaged-enemy follow | ✅ | Combat completion. |
| `#80` | investigator defeat: detection, state, action gating | ✅ | Combat side effect. |
| `#84` | Draw action | ✅ | Player-deck consumer. |
| `#85` | Mulligan | ✅ | Scenario-setup hand redraw. |
