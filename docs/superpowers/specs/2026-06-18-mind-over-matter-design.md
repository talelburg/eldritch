# Mind over Matter 01036 — skill substitution (Intellect for Combat/Agility)

**Date:** 2026-06-18
**Phase:** 7 (The Gathering) — Slice-1 follow-up, the last Axis-E carved Seeker card
**Issue:** [#322](https://github.com/talelburg/eldritch/issues/322)
**Ships as:** one PR (engine machinery + corpus regen + card).

## Problem

Mind over Matter is the last carved C6b Seeker card. Verbatim text + the
official FAQ (ArkhamDB `https://arkhamdb.com/card/01036`):

> **Mind over Matter** (Insight. Seeker event, cost 1):
> "Fast. Play only during your turn. Until the end of the round, you may
> use your [intellect] in place of your [combat] and [agility]."

FAQ rulings (load-bearing, not derivable from the text):
- *"When making a Combat or Agility skill test, you may make an **Intellect
  test** instead. If you do, apply only bonuses to your Intellect for this
  test, and ignore any bonuses to Combat or Agility."*
- *"You can only commit cards with an **Intellect or Wild** icon to this
  test."*
- *"You need to play this card **before the skill test begins**. The type
  of skill test is determined before you get the opportunity to play Fast
  cards."*
- *"You cannot play this card during the Mythos phase, because 'your turn'
  is within the Investigation phase."*

So substituting **turns the test into an Intellect test** (icons + bonuses
follow intellect), the choice is made **at test initiation** (the type is
fixed before commits/token), and it's a genuine "**may**" — a player may
decline (e.g. to fail on purpose). The engine has no skill-substitution,
no round-duration effect lifetime, and no "play only during your turn"
play-timing restriction.

## Design

### Component 1 — round-scoped skill substitution (state + expiry)

The event discards on play, so the substitution can't live on a card —
it's explicit state:

```rust
// GameState
pub skill_substitutions: Vec<SkillSubstitution>,

pub struct SkillSubstitution {
    pub investigator: InvestigatorId,
    pub use_skill: SkillKind,        // Intellect
    pub for_skills: Vec<SkillKind>,  // [Combat, Agility]
}
```

- **Pushed** by the card's `OnPlay` (Component 4).
- **Expiry — "until the end of the round":** cleared at **step 4.6**
  (`upkeep_after_round_ended`, after the round-end forced abilities resolve) —
  RR p.24: "Upkeep phase ends. Round ends. … Any active 'until the end of the
  round' lasting effects expire at this time." (Not the next round's Mythos
  bump — functionally equivalent in Slice 1, but the rules name step 4.6.) All
  substitutions are round-scoped (the sole consumer), so the whole `Vec`
  clears.

### Component 2 — the substitution choice (becomes an Intellect test)

The clean realization of the FAQ: **substituting just makes the test an
Intellect test**, so all existing skill-keyed machinery does the rest. No
special-casing in `sum_skill_value`.

- **Where:** `start_skill_test`. After building the in-flight record with
  the original skill but **before** opening the commit window, if the test
  is a Combat or Agility test (`skill ∈ {Combat, Agility}`) **and** a
  covering substitution is active for that investigator, suspend with a
  yes/no prompt ("use Intellect in place of <skill>?").
- **On "yes":** rewrite `in_flight.skill = Intellect` (keep `kind` =
  Fight/Evade) **and zero `in_flight.test_modifier`** — see "Weapon attacks"
  below. On "no": leave both. Either way, then open the commit window.
- **Why the skill rewrite is enough for icons/stat-bonuses:**
  `sum_skill_value` keys its base, constant modifiers, and pending modifiers
  off `in_flight.skill`; commit validation (`validate_commit_indices`)
  requires icons matching `in_flight.skill`; the Fight/Evade follow-up keys
  off `kind`. So `skill = Intellect, kind = Fight` yields the FAQ behavior —
  intellect base, intellect/wild icons, intellect stat-bonuses,
  combat/agility *stat-keyed* bonuses ignored, damage still dealt.
- **Weapon attacks (the load-bearing case for a Seeker):** a weapon's
  "+N [combat]" is snapshotted onto `in_flight.test_modifier` (skill-agnostic
  state), separate from the stat-keyed modifiers above. Per the FAQ ("ignore
  any bonuses to Combat or Agility"), that combat bonus **must be dropped**
  when substituting — so the "yes" path zeroes `test_modifier`. The weapon's
  bonus **damage** (`extra_damage` / `bonus_attack_damage`) is a damage
  bonus, not a skill bonus, so it is **kept**. Net: a Seeker fires .38
  Special, substitutes Intellect for the test (no weapon combat bonus, uses
  intellect value), and still deals the weapon's bonus damage on success —
  exactly the intended play. (`test_modifier` on a Combat/Agility test is
  only ever a weapon combat/agility bonus in scope; if a future
  skill-agnostic `test_modifier` source that should *survive* substitution
  lands, gate the zeroing then.)
- **Genuine "may":** declining keeps the combat/agility test, so a player
  can choose the lower value to fail intentionally. Auto-max would be
  wrong; this is a real prompt.
- **Timing:** the prompt fires at test *initiation* (before commits and the
  chaos token), matching the FAQ ("the type is determined before you play
  Fast cards"). `start_skill_test` is the single chokepoint for Fight,
  Evade, weapon Fights, and forced Combat/Agility tests, so all route
  through it uniformly.

**Suspend/resume mechanism:** a `GameState.pending_substitution_prompt:
Option<InvestigatorId>` (set when suspending) tells `resolve_input` to
route the next `PickSingle(OptionId)` to `resume_substitution_choice`
(option 0 = use Intellect, 1 = keep printed skill), which rewrites
`in_flight.skill` and then opens the commit window. Routed *before* the
existing commit/reaction routes (it's the innermost pending; no reaction
window is open for this test yet). The in-flight record is the parking —
no need to stash `start_skill_test`'s arguments.

### Component 3 — "Play only during your turn" (a pipeline-parsed metadata flag)

A new pipeline-parsed metadata flag, mirroring `is_fast` — parsed at
ingestion and **stored** on the card kind, *not* matched against card text
at runtime (runtime text-parsing in the engine would be inconsistent with
how every other printed property is handled, and a bad precedent):

- `CardKind::{Asset,Event}.play_only_during_turn: bool`, parsed in
  `card-data-pipeline` from text containing `"Play only during your turn"`,
  with a `CardMetadata::play_only_during_turn()` accessor that reads the
  stored field (exactly like `is_fast()`). Costs a **corpus regen**
  (`cargo run -p card-data-pipeline`) and adding the field to the ~22
  hand-written `CardKind::{Asset,Event}` literals (mechanical, one-time).
- `check_play_card` reads it: when set, the Fast gate requires
  `active_during_investigation` (drops the `permissive_window` disjunct),
  so the card can't be played in an out-of-turn permissive Fast window (the
  `MythosAfterDraws` window) — matching the FAQ "'your turn' is within the
  Investigation phase."
- **Bonus:** 10 corpus cards carry this clause, including **already-shipped
  Working a Hunch 01037** (which currently bypasses the timing via the
  default Fast gate) — so the accessor retroactively fixes it too.

### Component 4 — the card

- **Mind over Matter 01036** — Fast event; `OnPlay` is a card-local
  `Effect::Native("01036:mind-over-matter")` that pushes
  `SkillSubstitution{controller, Intellect, [Combat, Agility]}` onto
  `skill_substitutions`. The "play only during your turn" gate comes from
  the corpus flag (regen). Single consumer of the substitution push, so
  `Native` (not a typed effect), per the "shared variant only at ≥2
  consumers" rule.

## Testing

**Engine (game-core):**
- `start_skill_test` for a Combat/Agility test with a covering substitution
  suspends with the choice; "yes" → `in_flight.skill == Intellect`; "no" →
  unchanged. No substitution active ⇒ no prompt.
- Round-boundary clear empties `skill_substitutions`.
- `check_play_card`: a `play_only_during_turn` Fast event is rejected in
  the `MythosAfterDraws` permissive window but allowed during the active
  investigator's Investigation turn.

**Integration (`crates/cards/tests/`, real registry):**
- Play Mind over Matter (Fast, during your turn) → substitution active;
  the event is discarded. Then a Fight: choosing "yes" runs an **Intellect
  test** — assert the total uses intellect (set intellect ≠ combat so it's
  observable), an [intellect] skill card is committable while a [combat]
  one is not, and damage is still dealt on success (kind = Fight). Choosing
  "no" runs the combat test.
- **Weapon attack while substituting:** with Mind over Matter active, fire
  a weapon whose `Effect::Fight` carries a `+combat` modifier (e.g. .38
  Special) and choose "yes": assert the test total uses **intellect with no
  weapon combat bonus** (`test_modifier` zeroed), and the weapon's bonus
  **damage** is still dealt on success.
- Playing Mind over Matter outside your turn (Mythos window) is rejected.
- Working a Hunch 01037 (regression): now rejected outside your turn.

## Decisions

- **Substituting makes the test an Intellect test** (FAQ), so it's modeled
  by rewriting `in_flight.skill = Intellect` at initiation — every
  skill-keyed path (base, icons, bonuses) then does the right thing with no
  `sum_skill_value` special-casing. Combat/agility bonuses are ignored
  precisely because the test's skill is now Intellect.
- **The choice is a real prompt at test initiation, not auto-max** — "may"
  is genuine (a player can want to fail); the type is fixed before
  commits/token (FAQ), so the prompt fires in `start_skill_test` before the
  commit window.
- **`play_only_during_turn` is a pipeline-parsed `CardKind` flag** (parsed
  at ingestion, stored, read via `CardMetadata::play_only_during_turn()`) —
  consistent with `is_fast`, not a runtime text scan (which would be a bad
  precedent for the engine). The one-time `CardKind`-literal churn + regen
  is accepted. 10 corpus consumers; it retroactively fixes Working a Hunch
  01037.
- **The substitution push is `Effect::Native`** (single consumer); the
  reusable machinery is the `skill_substitutions` state + round expiry +
  the initiation prompt.

## Out of scope / deferred

- **Skill-test PLAYER WINDOWs** (ST.1/ST.2 Fast/reaction windows) are not
  modeled — [#374](https://github.com/talelburg/eldritch/issues/374). MoM
  is unaffected (it's played *before* the test, not in a skill-test
  window).
- The modifier/icon interaction is fully handled (no deferral): intellect
  test ⇒ intellect/wild icons + intellect stat-keyed bonuses; combat/agility
  stat-keyed bonuses (Beat Cop, Physical Training) drop because the test's
  skill is now Intellect; and a **weapon's combat bonus** (`test_modifier`)
  is dropped while its bonus damage is kept (Component 2, "Weapon attacks") —
  the intended Seeker-with-a-firearm play.
