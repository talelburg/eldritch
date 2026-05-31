# Investigator elimination flow (#144) — design

**Status:** Approved (design pass complete), ready for planning
**Issue:** #144 — `[engine] Formalize investigator elimination flow (Rules Reference p.10 steps 1–5)`
**Phase:** 4 — Scenario plumbing (slot 11, immediately before #73)
**Date:** 2026-05-31

## Goal

Extend `apply_investigator_defeat` — the single chokepoint for every defeat (Damage,
Horror, future Resign) — to execute the full Rules Reference p.10 Elimination flow, not
just the `Status` flip + `InvestigatorDefeated` emit it does today. The load-bearing
fix is **step 3**: a defeated investigator's engaged enemies currently keep
`engaged_with == Some(defeated)` as stale state (which #71's attack-loop early-break
works around); after this change they disengage and (when ready) re-engage a surviving
co-located investigator via #128's prey resolver.

`apply_investigator_defeat` stays a **synchronous `fn -> ()`**. No engine-wide
`EngineOutcome` threading is introduced (see "Re-engagement tie" below for why this is
correct and sufficient).

## Rules grounding (Rules Reference, verbatim)

**Elimination (p.10):** "A player is eliminated from a scenario any time his or her
investigator is defeated, or if he or she resigns. … Any time a player is eliminated:
1. The cards he or she controls in play and all of the cards in his or her out-of-play
areas (such as hand, deck, discard pile) are removed from the game.
2. All clue tokens that player possesses are placed at the location the investigator
was at when he or she was eliminated, and all of that player's resource tokens are
returned to the token pool.
3. All enemies engaged with that player are placed at the location the investigator was
at when he or she was eliminated, unengaged but otherwise maintaining their current game
state.
4. All other cards in the eliminated investigator's threat area are placed in the
appropriate discard pile.
5. If the lead investigator is eliminated, the remaining players (if any) choose a new
lead investigator.
6. If there are no remaining players, the scenario ends. Refer to 'no resolution was
reached' entry for that scenario in the campaign guide."

**Enemy Engagement (p.10):** "Any time a ready unengaged enemy is at the same location
as an investigator, it engages that investigator, and is placed in that investigator's
threat area. If there are multiple investigators at the same location as a ready
unengaged enemy, follow the enemy's prey instructions to determine which investigator is
engaged. … An exhausted unengaged enemy does not engage, but if an exhausted enemy at
the same location as an investigator becomes ready, it engages as soon as it is readied."

Two load-bearing nuances:

1. **Re-engagement is `ready`-only.** Step 3 places enemies *unengaged*; the *separate*
   general engagement rule re-engages them — but only the **ready** ones. An exhausted
   enemy stays unengaged until it readies. So step 3 = disengage all, then re-engage only
   the non-exhausted ones.
2. **The eliminated investigator is already non-`Active` before re-engagement runs**
   (status is flipped first), so it is never itself a re-engagement candidate.

## Design decisions

### `apply_investigator_defeat` stays synchronous; the re-engagement tie auto-picks the lead

The only sub-case that could require a player choice is a step-3 re-engagement **prey
tie** — 2+ co-located `Active` investigators where prey doesn't single one out. That is
reachable **only in multiplayer (Phase 8+)**; the Phase-4 demo is single-investigator,
where defeat leaves zero `Active` investigators co-located, so re-engagement always
resolves to `None`.

Making the tie *suspend* (the #128 `PickInvestigator` pattern) would require threading
`EngineOutcome` back out of `enemy_attack`, `fire_attacks_of_opportunity` (mid-action),
and `take_horror` — and solving the mid-loop suspension that
`resolve_attacks_for_investigator`'s own doc-comment flags as unsolved (persist the
remaining-attackers list) plus making attacks-of-opportunity resumable mid-action. Deep,
risky, no consumer until Phase 8.

So on a tie we **deterministically engage the lead** (`tied[0]`, which is `turn_order`-first
because `active_investigators_at` returns turn-order-ordered candidates) and file a Phase-8
follow-up for the interactive choice. This matches the issue's own step-5 deferral
("UX for 'remaining players choose' deferred to a future Lead-pick action") and #137's
precedent (deterministic lead-first now, interactive `ChooseFirstActor` to Phase 8).

### Step-by-step

The five steps run inside `apply_investigator_defeat`, **between** the existing
`InvestigatorDefeated` emit and the `check_all_defeated` call, so the defeat event fires
first and consequence events follow in causal order. The eliminated investigator's
location is read **once** at the top (`inv.current_location`); after steps 2 & 3 consume
it, `current_location` is set to `None` to match the documented "defeated ⇒ between
locations" invariant on `Investigator`.

**Step 1 — Remove controlled cards → removed-from-game pile.**
- New field `Investigator.removed_from_game: Vec<CardCode>` (`#[serde(default)]`; the
  struct is `#[non_exhaustive]`, constructed in only two places — `test_investigator`
  and one doctest — so low-friction).
- Drain `hand`, `deck`, `discard`, and `cards_in_play` (codes) into `removed_from_game`;
  clear the source vecs.
- **No new event** — state inspection (`removed_from_game` populated, sources empty) is
  the assertion surface; add an event later if UI needs it (YAGNI). Side benefit:
  emptying `cards_in_play` correctly stops the eliminated investigator's `Trigger::Constant`
  modifiers from contributing via the registry.

**Step 2 — Clues → location; resources → pool.**
- `loc.clues += inv.clues; inv.clues = 0`; emit existing `Event::LocationCluesChanged`
  with the new count. (Load-bearing for #73's act-advance math.)
- `inv.resources = 0` (no finite token pool is modeled, so "return to pool" = zero out;
  no event).
- If `current_location` is `None` (defeated between locations), skip clue placement
  gracefully (clues simply leave play with the investigator).

**Step 3 — Disengage + re-engage.**
- For each enemy with `engaged_with == Some(defeated)`, in `EnemyId` order: set
  `engaged_with = None` and emit existing `Event::EnemyDisengaged { enemy, investigator:
  defeated }`. (No `current_location` write: an engaged enemy is already at its
  investigator's location — the Move handler drags engaged enemies along and every engage
  site sets the location first — so RR step 3's "placed at the location the investigator
  was at" is already satisfied. The operative change is the disengagement.)
- Then re-engage via a new reusable helper `reengage_at_location(state, events, enemy_id)`:
  guarded on `!enemy.exhausted` (exhausted enemies don't re-engage); runs `resolve_prey`
  over `active_investigators_at(loc)`; `One` → `engage_enemy_with`; `None` → leave
  unengaged; `Tie` → `engage_enemy_with(tied[0])` (lead) with a `TODO(#<phase-8-issue>)`
  for the interactive pick. The helper is the general "an enemy is now ready+unengaged at
  a location → engage co-located investigator per prey" primitive — deliberately *not*
  elimination-specific (see Follow-ups).

**Step 4 — Other threat-area cards → discard. Documented no-op.**
No treachery/asset-in-threat-area state exists yet (enemies are the only threat-area
occupants). Inline `TODO` pointing at the Phase-7+ PR that models threat-area cards.

**Step 5 — Lead transfer. No-op by construction.**
There is no stored `lead_investigator` field — "lead" is computed on demand as
`first_active_investigator` (first `Status::Active` in `turn_order`). When the lead is
defeated, the next `Active` investigator is automatically lead. A doc-comment records
this.

**Step 6 — No remaining players → scenario ends.**
`check_all_defeated` already emits `AllInvestigatorsDefeated` (unchanged). The
`Resolution::Lost` emission + making #137's no-active-investigator park branch
unreachable is the **resolution layer that #73 (slot 12, the next PR) owns** — the engine
can't unilaterally emit a `Resolution` (it's scenario-module-driven via
`detect_resolution`). #144 leaves the clean signal + park; #73 turns it into a loss. This
narrows the issue's literal "step 6 lands here," intentionally and with the user's
sign-off.

### Cleanups folded into this PR

- Update `resolve_attacks_for_investigator`'s early-break doc-comment (dispatch.rs ~3046):
  with the disengage flow now landing, the early-break is "redundant but harmless" exactly
  as the issue predicted — keep the check, update the prose.
- Resolve/repoint the `TODO(#144)` markers (dispatch.rs:3047 and :4629 — the latter stays,
  repointed at #73 for the scenario-end consequence).

## Out of scope / explicitly narrowed (user-approved)

- Interactive re-engagement-tie `PickInvestigator` (multiplayer) → Phase-8 follow-up.
- Step 4 threat-area-card discard → no state to act on; Phase-7+.
- Step 6 `Resolution::Lost` emission + park-branch removal → #73.
- Resign action (DefeatCause::Resigned production) — separate issue when needed.

## Follow-ups to file

1. **Phase-8:** interactive re-engagement-tie `PickInvestigator` UX (the deferred
   "remaining players choose" / lead-decides choice; replaces the deterministic `tied[0]`
   auto-pick in `reengage_at_location`).
2. **Phase-4/Upkeep:** "engage on ready" gap — `ready_exhausted_cards` (Upkeep 4.3,
   dispatch.rs:4351) readies enemies without running the engagement check, so a
   ready+unengaged co-located enemy (reachable today via a successful Evade, which
   exhausts + disengages, then surviving to Upkeep) never re-engages per p.10 ("if an
   exhausted enemy … becomes ready, it engages as soon as it is readied"). Same rule and
   same helper as step 3 — the new issue reuses `reengage_at_location`.

## Testing strategy

Engine unit tests in `dispatch.rs` (`#[cfg(test)]`), using `TestGame` +
`test_investigator` / `test_location` / `test_enemy` fixtures and the event-assertion
macros:

- **Defeat-by-damage** and **defeat-by-horror** each run the full flow (parameterized or
  paired tests): clues land on the location (`LocationCluesChanged`), resources zeroed,
  `removed_from_game` populated and sources emptied, engaged enemy disengaged
  (`EnemyDisengaged`), `current_location` cleared.
- **Re-engagement (single survivor):** a ready enemy engaged with the defeated
  investigator re-engages a surviving co-located investigator (`EnemyEngaged`,
  prey-resolved).
- **Re-engagement tie (multiplayer):** 2+ co-located survivors → enemy engages the lead
  (`turn_order`-first), no suspension.
- **Exhausted enemy does NOT re-engage:** an *exhausted* enemy engaged with the defeated
  investigator disengages (step 3 first half) but is skipped by the re-engage guard, so it
  stays unengaged at the location even with a co-located survivor present (no `EnemyEngaged`).
- **`None` case:** single-investigator defeat → enemy left unengaged at the location, no
  `EnemyEngaged`.
- **#71 early-break still holds:** existing
  `resolve_attacks_for_investigator_early_breaks_when_target_defeated_mid_loop` continues
  to pass (now backed by real disengage rather than just the early-break).
- **Lead transfer:** assert `first_active_investigator` returns the survivor after the
  lead is defeated (existing-pattern assertion; no new mechanism).

## Files touched

- `crates/game-core/src/state/investigator.rs` — add `removed_from_game` field (+ update
  the in-file doctest constructor).
- `crates/game-core/src/test_support/fixtures.rs` — add the field to `test_investigator`.
- `crates/game-core/src/engine/dispatch.rs` — extend `apply_investigator_defeat`; add
  `reengage_at_location`; update early-break + TODO comments; tests.
- (No changes to event.rs — reuses `LocationCluesChanged` / `EnemyDisengaged` /
  `EnemyEngaged` / `AllInvestigatorsDefeated`.)
