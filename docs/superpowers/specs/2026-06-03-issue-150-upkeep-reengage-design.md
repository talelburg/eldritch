# #150 — Re-engage readied enemies at Upkeep 4.3

## Problem

Upkeep step 4.3 (`ready_exhausted_cards` in `crates/game-core/src/engine/dispatch.rs`)
flips every exhausted card to ready but does not run the engagement check. A
ready-able, unengaged, co-located enemy therefore never re-engages at Upkeep.

This is reachable single-player today: a successful Evade exhausts and disengages
an enemy; if it survives to Upkeep it readies but stays disengaged, contradicting
the rules.

## Rules (verified verbatim against `data/rules-reference/ahc01_rules_reference_web.pdf`)

- **RR p.25, step 4.3 — Ready exhausted cards:** "Simultaneously ready each
  exhausted card."
- **RR p.10, Enemy Engagement:** "An exhausted unengaged enemy does not engage,
  but if an exhausted enemy at the same location as an investigator becomes
  ready, it engages as soon as it is readied."

These resolve the timing question cleanly: 4.3 readies *simultaneously*, then the
engagement check fires for enemies that *became* ready. Implementation: ready all
exhausted cards first (one pass), then run engagement for the just-readied
enemies (second pass). For a single enemy the two readings are identical; the
second-pass shape makes "simultaneously ready" literal across multiple enemies.

## Change

**Site:** `ready_exhausted_cards` (dispatch.rs:~4696). One function. No signature
change — it stays a synchronous `fn`, because `reengage_at_location` auto-picks
the lead investigator on a prey tie rather than suspending via `AwaitingInput`
(established by `#144`, PR #152). No `EngineOutcome` threading.

**Logic:**

1. Existing loops run unchanged — flip every exhausted investigator in-play card
   and every exhausted enemy to ready, emitting `CardReadied` / `EnemyReadied`.
2. While flipping enemies, collect the ids just readied into a `Vec<EnemyId>`.
3. After all readying, iterate that vec (ascending `EnemyId` order, from the
   `BTreeMap`) and call `reengage_at_location(state, events, eid)` for each enemy
   currently unengaged (`engaged_with.is_none()`).

**Guard conditions, each rules-grounded:**

- **Only newly-readied enemies.** RR p.10 says "*becomes* ready" — an enemy
  already ready in a prior step is not re-checked.
- **Only if unengaged.** An enemy that was exhausted but still engaged (attacked
  last Enemy phase, kept its threat-area placement) readies but keeps its
  engagement. `reengage_at_location`'s documented precondition is
  `engaged_with == None`, so this guard is mandatory.
- Investigator in-play cards never engage — only the enemy loop feeds the second
  pass.

`reengage_at_location` already handles the remainder: early-returns if exhausted
(won't be, post-ready), resolves prey over co-located active investigators, emits
`EnemyEngaged`, and leaves the enemy unengaged when no investigator is co-located.

## Out of scope

- **Aloof keyword.** RR p.10 carve-out ("An enemy with the Aloof keyword does not
  engage in the manner described above"). Not modeled anywhere yet;
  `reengage_at_location` already ignores it, so the gap is pre-existing and shared
  with the `#144` elimination path. Not touched here.

## Tests

In `dispatch.rs` `#[cfg(test)]` (the `ready_exhausted_cards` home):

1. **Engages:** exhausted + unengaged enemy co-located with an active
   investigator → after 4.3, `EnemyReadied` + `EnemyEngaged` fire,
   `engaged_with == Some(inv)`, enemy not exhausted.
2. **No co-located investigator:** exhausted + unengaged enemy alone at a
   location → readies, stays unengaged, no `EnemyEngaged`.
3. **Already engaged:** exhausted + engaged enemy → readies, engagement
   unchanged, no duplicate `EnemyEngaged`.

## Scope estimate

~8 lines of production change plus the three tests.
