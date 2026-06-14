# Phase 7 — C4b: one-shot Revelation treacheries (design)

**Issues:** infra prerequisite [#286](https://github.com/talelburg/eldritch/issues/286);
content C4b [#234](https://github.com/talelburg/eldritch/issues/234).
**Milestone:** phase-7-the-gathering.

## Goal

Implement the four one-shot Revelation treacheries of The Gathering's
encounter set. Three of them initiate a skill test with a failure-side,
margin-keyed effect; one places doom. The work splits into a shared-engine
prerequisite PR (#286) and a pure-content PR (#234) on top.

## Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core_encounter.json`)

- **Grasping Hands 01162** (Hazard, qty 3) —
  *Revelation - Test [agility] (3). For each point you fail by, take 1 damage.*
- **Rotting Remains 01163** (Terror, qty 3) —
  *Revelation - Test [willpower] (3). For each point you fail by, take 1 horror.*
- **Crypt Chill 01167** (Hazard, qty 2) —
  *Revelation - Test [willpower] (4). If you fail, choose and discard 1 asset
  you control (if you cannot, take 2 damage instead).*
- **Ancient Evils 01166** (Omen, qty 3) —
  *Revelation - Place 1 doom on the current agenda. This effect can cause the
  current agenda to advance.*

Note on Crypt Chill: "take 2 damage" is the fallback for being **unable to
discard** (no asset controlled), not a free pass/fail alternative.

## Why this is not "no new dispatch"

The C4b issue assumed the existing Revelation hook suffices. It does for
Ancient Evils, but the three test treacheries need machinery that does not
exist:

1. **No `Effect` initiates a skill test.** Only player actions call
   `start_skill_test`. A `Trigger::Revelation` effect cannot start one.
2. **`SkillTestFollowUp` is success-side only** (`apply_skill_test_follow_up`
   under `if succeeded`, `skill_test.rs:178`). No failure-side, margin-keyed
   path exists.
3. **A suspending revelation never discards its treachery.**
   `resolve_encounter_card` (`encounter.rs:138-142`) returns early when a
   revelation effect returns non-`Done`, skipping `encounter_discard.push`.
   A test-initiating revelation always suspends at the commit window.

## Architecture

### PR 1 — engine machinery (#286)

**1. `Effect::SkillTest { skill: Stat, difficulty: u8, on_fail: Box<Effect> }`**
(shared; 3 consumers). Evaluator maps `Stat`→`SkillKind`, calls
`start_skill_test(.., kind: SkillTestKind::Plain, ..)`. Always returns
`AwaitingInput` (commit window). `on_fail` runs after resolution **on
failure only**; success is a no-op for these cards.

**2. `Effect::ForEachPointFailed(Box<Effect>)`** (shared; 2 consumers).
Runs `body` once per point the just-resolved test failed by. The failure
margin is threaded to the evaluator via `EvalContext`, set when the driver
runs `on_fail`. Grasping Hands = `ForEachPointFailed(DealDamage{You,1})`.

**3. Failure-side follow-up plumbing.** `finish_skill_test` gains an `else`
arm to its `if succeeded` (`skill_test.rs:178`) that runs the in-flight
test's `on_fail` with the margin in context. `on_fail` + the source treachery
code live on a **new `InFlightSkillTest.revelation: Option<RevelationFollowUp>`
field**, orthogonal to the success-side `follow_up: SkillTestFollowUp` (a
treachery test has no success effect; a Fight has no margin-keyed failure
effect — separate axes). `SkillTestFollowUp` stays `Copy`; the new non-`Copy`
field doesn't touch its copy sites.

**4. Suspendable-revelation discard.** When `resolve_encounter_card`'s
revelation loop suspends (non-`Done`), stash
`cx.state.pending_revelation_discard = Some(code)`. The skill-test driver's
terminal `PostOnResolution` step `take()`s it into `encounter_discard`. For a
normal Investigate/Fight/Evade test the slot is `None` (no-op). Scoped to
skill-test-suspended revelations; broader suspension (ChooseOne / #212)
generalizes later. **This is the seam C4c (#235) extends** for threat-area
placement (a persistent treachery enters the threat area instead of discard).

**5. `pub place_doom_on_current_agenda(cx)`** — wraps the existing
`pub(super)` `place_doom_on_agenda` + `check_doom_threshold` (`act_agenda.rs`)
so Ancient Evils' card-local native fn can place doom and run the advance
check.

### PR 2 — C4b cards (#234)

Each is a module `crates/cards/src/impls/treachery_0116*.rs` exposing
`CODE` + `abilities() -> Vec<Ability>` (a single `Trigger::Revelation`
ability), registered in the corpus.

| Card | Revelation effect |
|---|---|
| Grasping Hands 01162 | `SkillTest{ Agility, 3, on_fail: ForEachPointFailed(DealDamage{You,1}) }` |
| Rotting Remains 01163 | `SkillTest{ Willpower, 3, on_fail: ForEachPointFailed(DealHorror{You,1}) }` |
| Crypt Chill 01167 | `SkillTest{ Willpower, 4, on_fail: Native("01167:crypt-chill-fail") }` |
| Ancient Evils 01166 | `Native("01166:place-doom")` (no test) |

- **Crypt Chill native fn** (`01167:crypt-chill-fail`): deterministically
  discard one controlled asset (lowest-cost / first), else 2 damage if none.
  `TODO(#212)` for the interactive choice — mirrors the 01105 reverse
  precedent (deferred interactive choice + deterministic legal branch).
- **Ancient Evils native fn** (`01166:place-doom`): calls
  `place_doom_on_current_agenda(cx)`.

Single-consumer logic stays card-local `Effect::Native` (#276); only the
genuinely-shared `SkillTest` / `ForEachPointFailed` become `Effect` variants
(CLAUDE.md: two-or-more-consumer rule).

## Data flow (a test treachery)

draw → `resolve_encounter_card` emits `CardRevealed` → runs the
`Trigger::Revelation` `Effect::SkillTest` → `start_skill_test` returns
`AwaitingInput`; loop stashes `pending_revelation_discard = Some(code)` and
returns → player `ResolveInput::CommitCards` → `finish_skill_test` draws the
token, on failure runs `on_fail` (margin in `EvalContext`) → driver teardown
flushes `pending_revelation_discard` to `encounter_discard`, emits
`SkillTestEnded`, clears in-flight → `Done`.

## Error handling

- All new effects follow the validate-first / mutate-second contract; loud
  reject on absent registry / unknown native tag (existing pattern).
- `place_doom_on_current_agenda` no-ops on an empty agenda deck (existing
  helper guards).
- Crypt Chill's native fn rejects loudly only on state-corruption invariants;
  "no asset controlled" is the legitimate 2-damage branch, not an error.

## Testing

- **Card tests** (per impl): Grasping Hands fail-by-2 → 2 damage; Rotting
  Remains fail-by-N → N horror; Crypt Chill fail with an asset (discard) and
  without (2 damage); Ancient Evils → +1 doom, and advance when at threshold.
  Seeded chaos bag for deterministic margins.
- **Engine unit tests** (skill_test / encounter modules): `Effect::SkillTest`
  suspends then resumes; `ForEachPointFailed` scales by margin;
  `pending_revelation_discard` flushes after a treachery test and stays `None`
  for a plain Investigate (regression); `Active`-status check on a
  Mythos-phase draw.
- **Integration** (`crates/cards/tests/`, `cards::REGISTRY` installed): full
  draw→reveal→commit→resolve→discard for one test treachery.

## Risk to verify during implementation

`start_skill_test` requires `investigator.status == Active`. Encounter draws
happen in Mythos. Confirm `Active` means "in play" (not turn ownership) so a
Mythos-phase treachery test is legal; relax for treachery-initiated tests if
not.

## Out of scope

- Crypt Chill's interactive asset choice / mid-revelation `ChooseOne`
  suspension (#212) — deterministic legal branch ships now.
- Generalizing `pending_revelation_discard` beyond skill-test-suspended
  revelations (#212).
- C4c persistent threat-area treacheries (#235) — this design only lays the
  `pending_revelation_discard` seam they extend.
