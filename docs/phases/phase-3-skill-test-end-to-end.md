# Phase 3 — Skill-test end-to-end

## Status

🟡 In progress. 17 closed / 6 open as of 2026-05-17.

## Goal

Full skill test sequence runs through the engine in tests — with real-card metadata + abilities end-to-end.

## Issues

### Closed (17)

| # | Title | Notes |
|---|---|---|
| `#19` | deterministic `ChoiceResolver` | Test infra; ships ahead of first engine consumer (`#63`). `commit_cards` stubbed pending commit-window response shape. |
| `#63` | skill-test card commits from hand | First engine consumer of `#19`. Splits `resolve_skill_test` → `start_skill_test` + `finish_skill_test`; adds `state.in_flight_skill_test`, `SkillTestFollowUp`, `InputResponse::CommitCards`. |
| `#37` | Magnifying Glass (01030) | First new card after the Phase-3 demo; composes `#87` + `#45`. |
| `#38` | Hyperawareness (01034) | Exercises `Trigger::Activated` + `ThisSkillTest` push end-to-end. |
| `#45` | per-skill-test-kind modifier scope | Adds `ModifierScope::WhileInPlayDuring(SkillTestKind)`. |
| `#47` | DSL evaluator (Effect → state mutation) | Initial scaffold with stubs for non-leaf effects. |
| `#48` | chaos bag draw + token modifier resolution | `resolve_token` + `TokenResolution`. |
| `#49` | skill-test resolution flow | `resolve_skill_test`. |
| `#50` | Move action + action-point spending | First non-trivial player action. |
| `#51` | Investigate action | First skill-test-initiating action. |
| `#53` | DSL Activated trigger + cost primitives | `Trigger::Activated`, `Cost` enum, `Ability.costs`. |
| `#87` | per-instance card state | `CardInPlay` struct + `CardInstanceId` + counter. |
| `#88` | cards-registry binding | Cross-crate bridge. Option 3 (static `OnceLock` + `fn`-pointer struct). |
| `#89` | PlayCard action | Asset → in-play; event → discard. |
| `#92` | constant-modifier query during skill-test resolution | Closes the Phase-3 demo: Holy Rosary `+1 willpower` actually applies. |
| `#102` | `ThisSkillTest` modifier accumulator | Pushed + drained on `SkillTestEnded`. |
| `#112` | DSL `Trigger::OnSkillTestResolution` + tested-location plumbing | Split out of `#39` so the trigger variant lands on its own; `#39` consumes it. |

### Open (6)

| # | Title | Notes |
|---|---|---|
| `#39` | Deduction (01039) | Consumes `#112`; lands as the next PR. |
| `#52` | reaction windows + trigger ordering | Needs `#54` + `#19`. The largest remaining engine piece. |
| `#54` | DSL `OnEvent` trigger | Small extension; unblocks `#52` and `#55` Roland Banks. |
| `#55` | Roland Banks investigator (01001) | Needs `#54` + `#52`. |
| `#56` | Study location (01111) | Needs a location-state shape decision; thinnest issue body. |
| `#64` | skill-test after-resolution trigger window | Needs `#54` (OnEvent) + `#19`. Distinct semantic from `#112` — `#64` is a player-window-driven reactive trigger after the test ends; `#112` is a resolution-machinery trigger on committed cards. |

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
| 10 | `#39` Deduction | consumes `#112` |
| 11 | `#54` OnEvent trigger | small |
| 12 | `#52` reaction windows | needs `#54` + `#19`; largest remaining piece |
| 13 | `#55` Roland Banks | needs `#54` + `#52` |
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
- **`Trigger::OnSkillTestResolution` is resolution-machinery, not a reaction window (`#112`, PR #113).** Deduction-shaped text ("if this skill test is successful while investigating, discover 1 additional clue at that location") doesn't fit `OnCommit` (outcome unknown at commit) and doesn't fit `#64`'s reactive window (no player decision). New variant fires inside `finish_skill_test` for committed cards, gated on actual outcome. Settles `#39` Deduction's routing question; keeps `#64` strictly for player-window-driven "after a test succeeds, you may …" cards. Event order pinned by `investigate_canonical_event_order_with_on_resolution`: action follow-up before OnResolution before discards, load-bearing for `#64` listeners that key off `SkillTestEnded` as "all sub-effects applied."

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
