# Rules-conformance audit: skill-test resolution and timing — 2026-08-16

The official Rules Reference was vendored into `data/rules-reference/rules/` on
2026-08-15, verbatim and readable offline, which makes it possible for the first
time to check the engine against the *printed* skill-test sequence rather than
against memory of it. This audit does exactly that for one surface: the skill
test, from ST.1 to ST.8, and the framework rules that hang off it. Every claim
below was read out of the vendored rules text and then verified against the code
at the cited line before being written down.

**Scope.** RR Appendix II's Skill Test Timing chart (ST.1–ST.8) and its framework
event details, Appendix I's initiation sequence, and the glossary entries that
govern skill tests — difficulty, base value, modifiers, automatic
failure/success, chaos tokens, nested skill tests, wild icons, skill cards,
cancel, target, printed, gains, and X. Out of scope: the phase sequence itself
(mythos/investigation/enemy/upkeep framework events), enemy attacks except where
retaliate re-enters the skill test, and the Chapter-2-and-later token families
(bless/curse/frost) except where the engine actively contradicts them — it does
not; it simply has no representation for them, which is registered already.

Seven findings: **five WRONG**, **one ABSENT**, and **one correction to the
2026-08-14 forward-compatibility register** (no ADR conflicts). One of the five
is a live corruption/panic path reachable from legal inputs with shipped cards.

## Method

1. **Read the rules first, in full.** `Appendix_II_Timing_and_Gameplay.md`,
   `Appendix_I_Initiation_Sequence.md`, and the glossary entries
   `Skill_Tests`, `Difficulty_skill_tests`, `Base_Value`, `Modifiers`,
   `Automatic_Failure_Success`, `Chaos_Tokens`, `Nested_Skill_Tests`,
   `Wild_Skill_Icons`, `Skill_Cards`, `Cancel`, `Difficulty_level`, `Target`,
   `Qualifiers`, `Printed`, `Gains`, `The_letter_X`. Every quote below is
   copied from those files; nothing was fetched.
2. **Read the code the surface lives in.**
   `crates/game-core/src/engine/dispatch/skill_test.rs` in full (2,456 lines),
   the skill-test-relevant parts of `engine/evaluator.rs` (the modifier queries
   at `:1203-1221`, `:2051`, `:2299`, the elder-sign query at `:2067`),
   `engine/dispatch/reaction_windows.rs` (the fast-window open/drive/enumerate
   path and the play/activate validators), `engine/dispatch/cards.rs`
   (`play_card` / `commence_play`), `engine/dispatch/actions.rs` (where each
   action's difficulty comes from), `state/game_state.rs`
   (`InFlightSkillTest`, `ResolvedTest`, `SkillTestStep`),
   `state/chaos_bag.rs`, and `crates/card-dsl/src/dsl.rs` for how commits and
   modifiers are expressed.
3. **Checked card text and rulings locally.** Card text from
   `data/arkhamdb-snapshot/pack/core/core.json`; rulings from
   `data/arkhamdb-faq/core/` — 01034 (Hyperawareness), 01036 (Mind over
   Matter), 01039 (Deduction), 01025 (Vicious Blow), 01087 (Flashlight).
   Two of those rulings settle findings below and two confirm the engine is
   right.
4. **Cross-referenced before filing.** `docs/audits/2026-08-14-chapter-1-
   forward-compatibility.md` (bucket 3 items 1, 4, 5, 15, 16 all touch this
   surface), `docs/audits/2026-07-17-audit.md`, all four ADRs, and the tracker
   (`gh issue view` on #65, #139, #565, #572). Anything already registered or
   tracked is named as such in [Already registered or tracked](#already-registered-or-tracked)
   rather than re-reported.

## Findings

### 1. Committed cards are stored as hand *positions*, and the engine's own ST.2→ST.3 player window can move them — WRONG

RR Appendix II, `data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`:

> ST.2 Commit cards from hand to skill test.
>
> The investigator performing the skill test may commit any number of cards with
> an appropriate skill icon from his or her hand to this test.

and, at the end of the sequence:

> ST.8 Skill test ends.
>
> This step formalizes the end of this skill test. Discard all cards that were
> committed to this skill test, and return all revealed chaos tokens to the
> chaos bag.

A committed card is committed *to the test*: it is the same physical card at
ST.2, at ST.5 when its icons are counted, and at ST.8 when it is discarded.

The engine identifies committed cards by their index in the hand at commit time
and never re-binds them:

- `crates/game-core/src/engine/dispatch/skill_test.rs:250-251` — the commit hop
  stores `t.committed_by_active = indices_u8` and advances the cursor to
  `PreTokenWindow`.
- `crates/game-core/src/engine/dispatch/skill_test.rs:1066` —
  `sum_committed_icons` reads `let code = &hand[usize::from(idx)];` at ST.5,
  indexing the hand **as it is then**.
- `crates/game-core/src/engine/dispatch/skill_test.rs:1087-1126` —
  `discard_committed_cards` removes by the same stale indices at ST.8.

Between those two points the engine deliberately opens a player window that lets
cards leave the hand. `skill_test.rs:770-773` opens the RR p.26 ST.2→ST.3 window;
`reaction_windows.rs:2195-2213` (`enumerate_fast_plays`) offers, as options,
`TurnAction::PlayCard { hand_index }` for **every** Fast card in every hand that
passes `check_play_card` — including the cards just committed, which are still
sitting in the hand; and `dispatch/mod.rs:90-93` routes the pick to
`cards::play_card`, whose `commence_play` (`cards.rs:1042-1048`) does
`hand.remove(hand_index)`.

**Divergence scenario** (Core cards only, investigator's own turn, so
`active_during_investigation` is true and the Fast gate passes):

- Hand is `[Working a Hunch 01037, Deduction 01039]`. The investigator takes an
  Investigate action at a location with clues and commits index 1 (Deduction).
- The engine opens the ST.2→ST.3 window; Working a Hunch is Fast and playable
  (its target check passes — there are clues here), so the window stays open and
  offers it. The player plays it.
- Hand is now `[Deduction]`. At ST.5 `sum_committed_icons` evaluates
  `hand[1]` on a one-element vector → **index-out-of-bounds panic**, which
  escapes `apply_via`'s Rejected-only rollback.
- With one more card in hand the panic becomes silent corruption instead: the
  icons of an *uncommitted* card are added to the total, and at ST.8 that card
  is discarded while the committed one stays in hand.

Separately, the same window offering a *committed* card as a legal play is
itself wrong: at ST.2 the card has left the hand for the test, so it is not
available to be played, and ST.8 will try to discard it a second time.

This is the same defect class as **#565** ("Non-fast asset play parks a raw
`hand_index` across the AoO suspension"), which was filed p0 and closed — the fix
re-bound the *asset play* site by card code. The skill-test commit site was not
covered by it and still stores raw positions.

**WRONG.**

### 2. ST.5 is computed before the ST.4 chaos-symbol effects resolve — WRONG

RR Appendix II is explicit about the order and about what ST.5 must include:

> ST.4 Apply chaos symbol effect(s).
>
> Apply any effects initiated by the symbol on the revealed chaos token.

> ST.5 Determine investigator's modified skill value.
>
> Start with the base skill (of the skill that matches the type of test that is
> resolving) of the investigator performing this test, and apply all active
> modifiers, including the appropriate icons that have been committed to this
> test, effects of the chaos token(s) revealed, and all active card abilities
> that are modifying the investigator's skill value.

and `glossary/Modifiers.md`:

> Any time a new modifier is applied (or removed), the entire quantity is
> recalculated from the start, considering the unmodified base value and all
> active modifiers.

The engine computes the ST.5 total at
`crates/game-core/src/engine/dispatch/skill_test.rs:328`
(`let skill_value = sum_skill_value(...)`) and only *then*, at `:386-388`,
pushes the ST.4 symbol effects (`push_symbol_effects`). The in-code comment at
`:325-327` acknowledges the inversion and justifies it — *"The in-scope
`immediate` effects (damage/horror) don't change the skill value, so computing
the total before they resolve is correct"* — but that is not true of the shipped
corpus.

**Divergence scenario** (The Gathering, Core cards only):

- The scenario's `[tablet]` effect is `−2`, plus *1 damage if a Ghoul is at your
  location* (`crates/scenarios/src/the_gathering.rs:152-159`), and that damage
  is an ST.4 `immediate`.
- The investigator controls Beat Cop 01018 — *"You get +1 [combat]."*, printed
  `health: 2` — already carrying 1 damage, and is making a Fight test.
- ST.4's 1 damage is assigned to Beat Cop via the interactive soak path
  (`combat.rs:460-482`), which fills it and discards it. Its `+1 [combat]`
  is no longer an active modifier.
- Rules: ST.5 then computes the modified skill value **without** the +1.
  Engine: the total was already fixed at `:328` **with** the +1, so a test that
  should fail by 1 succeeds.

The inversion also means the reverse case — an ST.4 effect that *adds* a
modifier — would be dropped.

**WRONG.**

### 3. A "for this skill test" modifier has no test identity and outlives the test that never existed — WRONG

Hyperawareness 01034, verbatim from `data/arkhamdb-snapshot/pack/core/core.json`:

> Talent.
> [fast] Spend 1 resource: You get +1 [intellect] for this skill test.
> [fast] Spend 1 resource: You get +1 [agility] for this skill test.

RR Appendix I, `data/rules-reference/rules/Appendix_I_Initiation_Sequence.md`:

> - Check play restrictions: determine if the card can be played, or if the
>   ability can be initiated, at this time. (This includes verifying that the
>   resolution of the effect has the potential to change the game state.) If the
>   play restrictions are not met, abort this process.

The engine's `ModifierScope::ThisSkillTest` arm pushes unconditionally, with no
check that a test is in flight and no test identity on the record
(`crates/game-core/src/engine/evaluator.rs:1210-1221`); the consumer matches on
investigator and stat only (`evaluator.rs:2299-2314`); and the only drain is the
skill-test teardown for that investigator
(`skill_test.rs:883-885`, mirrored in `abandon_test` at `:933-935`). Nothing
clears it at end of round — contrast `skill_substitutions`, which *is* cleared at
round end (`dispatch/phases.rs:1123`). The activation gate
(`reaction_windows.rs:2023-2100` plus `check_effect_target_available` at
`:1952-1994`) has no in-flight-test condition, and the open-turn menu enumerates
0-action activated abilities directly (`engine/enumerate.rs:244-253`).

**Divergence scenario:** Hyperawareness is in play; the investigator is at the
open-turn menu with no test in flight. They activate ability 0, paying 1
resource. Rules: the ability cannot be initiated — its resolution has no
potential to change the game state, because there is no "this skill test".
Engine: it resolves, a `PendingSkillModifier { stat: Intellect, delta: 1 }` is
queued, and it is applied to the investigator's **next** intellect test —
which may be several actions or several rounds later — and only then drained.

The card's own ruling (`data/arkhamdb-faq/core/01034.md`) confirms there is no
per-round limit to lean on: *"You can use [fast]fast actions as many times as
you want, as long as you can pay the cost; there is no limit."* — so the leak can
be stacked arbitrarily high before a test is ever started.

**WRONG.**

### 4. Automatic success and card-ability automatic failure have no representation — ABSENT

`data/rules-reference/rules/glossary/Automatic_Failure_Success.md`:

> Some card or token abilities may cause a skill test to automatically fail or to
> automatically succeed. If a skill test automatically fails or automatically
> succeeds, it does so during step "ST.6" [...]
>
> - If a skill test automatically fails, the investigator's total skill value for
>   that test is considered 0.
> - If a skill test automatically succeeds, the total difficulty of that test is
>   considered 0.

and the expanded rules in the same file:

> - If it is known that an investigator automatically succeeds or fails at a
>   skill test before Step 3 ("Reveal chaos token") occurs, that step is skipped,
>   along with Step 4. No chaos token(s) are revealed from the chaos bag, and the
>   investigator immediately moves to Step 5. All other steps of the skill test
>   resolve as normal.

> However, the skill test still takes place. Cards may still be committed to the
> test, and the investigator’s total modified skill value is still determined, as
> it may have some bearing on other card abilities.

The engine's only route to either outcome is the `[auto_fail]` chaos token:
`TokenResolution` is `Modifier | ElderSign | AutoFail`
(`crates/game-core/src/state/chaos_bag.rs:88-110`), consumed at
`skill_test.rs:344` as `(0, FailureReason::AutoFail)`. There is no
`Effect`/`Ability` shape for "this test automatically succeeds/fails", no way to
mark a test auto-resolved before `Resolving`, and therefore no path that skips
ST.3/ST.4 — `run_resolution` (`skill_test.rs:299-389`) always draws a token from
the bag as its first act. Nor is the modified skill value retained on an
auto-fail: `:344` discards the computed `skill_value` entirely, so the "still
determined... may have some bearing on other card abilities" clause has nothing
to read even if a consumer existed.

Supporting this is not a patch to a step; it needs a determination that can be
latched before ST.3, a token-draw that can be skipped (which also has to keep the
recorded-RNG replay contract intact — `EngineRecord` currently assumes a draw),
and a difficulty that can be overridden to 0. Distinct from the register's
bucket-3 item 4 (chaos-bag manipulation), which is about the bag's *contents*.

**ABSENT.**

### 5. ST.6 compares against a difficulty frozen at ST.1 — WRONG

RR Appendix II:

> ST.6 Determine success/failure of skill test.
>
> Compare the investigator's modified skill value to the difficulty of the skill
> test.

and `glossary/Skill_Tests.md`, "Variable Difficulty Skill Tests":

> While performing a skill test whose difficulty is modified based on another
> aspect of the game, that difficulty changes whenever the corresponding aspect's
> status does.
>
> If an ability or game effect checks a variable difficulty, most commonly during
> a skill test's ST.6 to determine whether that test succeeded or failed, the
> value of that difficulty is set for the effect in question and is not
> re-evaluated if the variable difficulty continues to change.

The reading is that the difficulty stays live *until* ST.6 reads it, and is
pinned only from that read onward. The engine pins it two steps earlier, at
initiation: `investigate` snapshots `effective_shroud` into an `i8`
(`dispatch/actions.rs:112-127`), `fight` snapshots the enemy's fight value
(`:795-830`), `evade` its evade value (`:847-872`), and the value is carried on
`InFlightSkillTest.difficulty` (`state/game_state.rs:1338-1339`) and read
unchanged at `skill_test.rs:347` (`let margin = total.saturating_sub(difficulty)`).
Nothing re-reads the location or the enemy between ST.1 and ST.6, and there is
no difficulty-modifier concept at all — Flashlight 01087's `−2` shroud is applied
at *initiation* (`Effect::Investigate { shroud_modifier }`, `dsl.rs:808-812`),
producing a smaller snapshot rather than a live modifier.

**Divergence scenario:** an investigation begins against a shroud-3 location.
During the ST.1→ST.2 or ST.2→ST.3 player window (both of which the engine opens),
anything that changes that location's shroud — an attachment entering or leaving,
a `Stat::Shroud` modifier — leaves the difficulty at 3. Rules: ST.6 compares
against the shroud as it stands at ST.6.

Latent in the shipping corpus: the only shroud modifier in Core + Dunwich is
Obscuring Fog 01168, which attaches on Revelation, and a mid-test encounter draw
is itself unbuilt (register bucket 3, item 15). Recorded because the pinning is
structural, not because a shipped card reaches it — and because the register's
bucket 3 item 1 ("a real modifier layer") is about *whose* stats can be modified,
not about *when* the difficulty is read.

**WRONG.**

### 6. ST.7's multiple results resolve in a fixed engine order, not the performer's chosen order — WRONG

RR Appendix II:

> ST.7 Apply skill test results.
>
> [...] If there are multiple results to be applied during this step, the
> investigator performing the test applies those results in the order of his or
> her choice.

The engine sequences ST.7 deterministically through five cursor steps —
`FireOnCommit` → `ApplyFollowUp` → `ApplyResultEffect` → `ApplySymbolOnFail` →
`FireOnResolution` (`skill_test.rs:814-856`; the ordering is spelled out in the
`SkillTestStep` docs at `state/game_state.rs:1575-1628`). The choice is
acknowledged and declined in `ApplySymbolOnFail`'s own doc comment
(`game_state.rs:1606-1609`): *"RR lets the test-performer order multiple results,
the engine sequences deterministically."* That is a doc comment, not an ADR — I
checked all four in `docs/adr/`, and none covers it.

**Divergence scenario:** a failed willpower test on Rotting Remains 01163 that
also drew the `[cultist]` token. Two results are pending — the card's margin
horror and the symbol's 1 horror. The engine always applies the card effect
first (`ApplyResultEffect`) and the symbol's second (`ApplySymbolOnFail`). The
performer cannot choose the other order, which matters as soon as a soak asset
would be filled by the first packet, or a card triggers off a specific horror
source landing first.

Low severity today (the in-corpus results are commutative), but it is a
divergence from a printed clause and the fixed order is invisible to clients.

**WRONG.**

### 7. Correction to the 2026-08-14 register: a second skill test is *rejected*, not nested

The forward-compatibility register (bucket 3, item 16) records the deferral gap
correctly and quotes FAQ 1.17 in full; `glossary/Nested_Skill_Tests.md` confirms
it verbatim:

> A skill test cannot initiate during another skill test. If during the
> resolution of a skill test another skill test would initiate, instead the
> second skill test does not initiate until the first skill test has finished
> resolving.

But its description of today's behaviour is wrong. It says: *"Today that pushes a
`SkillTest` frame above a live one; correct behaviour holds it until the whole
investigate action has finished resolving."* The engine does not push a second
frame — `start_skill_test` rejects up front
(`crates/game-core/src/engine/dispatch/skill_test.rs:70-76`):

```rust
if cx.state.has_skill_test_in_flight() {
    return EngineOutcome::Rejected { reason: "skill test: another skill test is already in flight; ..." };
}
```

with `has_skill_test_in_flight` scanning the whole continuation stack
(`state/game_state.rs:2107-2111`). This matters because of what a `Rejected`
does at the apply boundary: `apply_via` restores state, events, and the RNG
position wholesale, so the rejection unwinds **the entire enclosing apply** — the
player's `ResolveInput` that was driving the outer test — rather than just the
inner test. Re-submitting the same response reproduces it deterministically: an
unwedgeable prompt, which is exactly the failure mode #572's last-but-two bullet
("Deterministic-rejection soft-lock backstop") describes for stubbed DSL variants.

The gap itself stays classified as the register has it (additive: a deferred-test
queue drained at an action boundary). Only the description of the current
behaviour needs correcting — and the severity is higher than "no deferral
mechanism" suggests, because the current behaviour is a rollback, not a no-op.

## Already registered or tracked

Checked, confirmed against the rules, and **not** re-reported:

- **Other investigators cannot commit.** RR ST.2: *"Each other investigator at
  the same location as the investigator performing the skill test may commit one
  card with an appropriate skill icon to this test."* The engine has only
  `committed_by_active` (`state/game_state.rs:1340-1352`, whose doc says so).
  Tracked as **#65**, milestoned `phase-8-multiplayer-and-auth`.
- **Commits are not checked for an appropriate icon.** RR ST.2: *"Cards that lack
  an appropriate skill icon may not be committed to a skill test."*
  `validate_commit_indices` (`skill_test.rs:947-1016`) checks bounds, duplicates,
  and the printed `commit_limit` — never icons. Tracked as the first bullet of
  **#572**. Mind over Matter's ruling
  (`data/arkhamdb-faq/core/01036.md`) sharpens it for substituted tests: *"You can
  only commit cards with an Intellect or Wild icons to this test."* — which the
  engine would also need, and which follows for free once the icon check reads
  the (already rewritten) `t.skill`.
- **`u8::try_from(i).expect(...)` in `validate_commit_indices`** panics rather
  than rejecting for hands ≥ 256 — also **#572**.
- **Peril does not block cross-investigator interaction.** RR 1.4 step 2:
  *"Those other players cannot play cards, trigger abilities, or commit cards to
  that investigator's skill test(s) while the peril encounter is resolving."*
  `peril_check` is a documented no-op (`skill_test.rs:1449-1460`). Tracked as
  **#139**. Note the TODO there is tagged `TODO(future-peril-PR)`, not
  `TODO(#139)`, so the anchor convention doesn't find it.
- **Multiple revealed tokens / "reveal another token" / sealing** — register
  bucket 3, item 4. The engine draws exactly one token per test
  (`skill_test.rs:309-310`) and `Chaos_Tokens.md`'s "Resolving Multiple Revealed
  Chaos Tokens" has no representation.
- **Elder signs that run an effect** (rather than a pure modifier) — register
  bucket 3, item 4; scoped deliberately at `dsl.rs:213-218` and
  `evaluator.rs:2077-2082`.
- **Board-wide modifier layer** (modifying another investigator's skills, an
  enemy's fight/evade) — register bucket 3, item 1.
- **A skill test initiated inside a resolution needs deferring** — register
  bucket 3, item 16, corrected in finding 7 above.

## Checked and found sound

Each of these was a live suspicion that the code answered correctly.

- **ST.6's comparison, and the zero-clamp, match the glossary example.**
  `glossary/Modifiers.md`: *"Negative modifiers in excess of a value's current
  quantity can be applied, but, after all active modifiers have been applied, any
  resultant value below zero is treated as zero."* — with the worked Danny /
  "Lucky!" example (base 4, −8, +2 → −2 → treated as 0). The engine sums base +
  constant + pending + icons + `test_modifier` **unclamped**
  (`skill_test.rs:1049-1052`), adds the token modifier, and clamps **once** at the
  end (`:330-343`). Clamping per-modifier — the obvious wrong implementation —
  would give a different answer for that example; the engine does not do it.
- **Difficulty 0 plus a −8 token succeeds; the auto-fail token still fails.**
  Flashlight's ruling (`data/arkhamdb-faq/core/01087.md`): *"If you reduce shroud
  to 0, investigating this location will be successful even if you reveal a −8
  token, because negative values are treated as 0 [...] You can still fail if you
  reveal an [auto_fail] auto-fail token though."* Engine: `total = 0`,
  `margin = 0 − 0 = 0`, `succeeded = margin >= 0 && !auto_fail`
  (`skill_test.rs:347-348`) — success; and the explicit `&& !auto_fail` is what
  keeps the auto-fail token failing a difficulty-0 test, which a bare
  `total >= difficulty` would not.
- **Auto-fail sets the total to 0 (not "skill value ignored"), so the failure
  margin is the full difficulty.** RR ST.6: *"If an investigator automatically
  fails at a test via a card ability or revealing the [auto_fail] symbol, his or
  her total skill value for that test is considered 0."* `skill_test.rs:344`
  plus `failed_by = difficulty - total` at `:349-353` — which is what Rotting
  Remains 01163 and Grasping Hands 01162 scale their horror/damage off.
  `FailureReason` distinguishes `AutoFail` from `Total` at the same numeric
  outcome, as CLAUDE.md requires.
- **Both RR p.26 player windows exist and bracket the commit.** ST.1→ST.2 at
  `skill_test.rs:761-764`, ST.2→ST.3 at `:770-773`, each pre-advancing the cursor
  before opening and auto-skipping when nobody can act
  (`reaction_windows.rs:1587-1623`). (Finding 1 is about what a play *inside* the
  second window does to the commit record, not about the window's existence.)
- **Success/failure reactions fire at ST.6, before the ST.7 consequences.**
  `glossary/Skill_Tests.md`: *"[reaction] or **Forced** abilities with a
  triggering condition dependent upon the skill test being successful or
  unsuccessful (such as 'After you successfully investigate,' [...]) do not
  trigger at this time. These abilities are triggered during Step 6, 'Determine
  success/failure of skill test.'"* The engine emits `SkillTestResolved` from
  `determine_outcome_step` (`skill_test.rs:486-536`), pre-advancing to
  `AcknowledgeOutcome`/`FireOnCommit`, so Dr. Milan Christopher 01033's
  after-you-successfully-investigate reaction is offered *before* the clue is
  discovered at ST.7 — which is what the rule asks for, and is the non-obvious
  ordering.
- **Wild icons count as matching.** `glossary/Wild_Skill_Icons.md`: *"Wild icons
  committed to a skill test are considered 'matching' icons for the purposes of
  card abilities."* `sum_committed_icons` adds `matching + wild`
  (`skill_test.rs:1068-1076`).
- **Committing costs nothing.** RR ST.2: *"Do not pay a card's resource cost when
  committing it."* No cost is charged anywhere on the commit path.
- **Committed cards are discarded at ST.8 and nowhere else** — and *not* when the
  tester has been eliminated, because RR p.10 elimination already removed them
  from the game (`abandon_test`, `skill_test.rs:915-942`, #564). Discard precedes
  `SkillTestEnded` (`:877-878`).
- **The base value is the printed skill.** `glossary/Base_Value.md`: *"the base
  value of an element derived from a card is the value printed on that card."*
  `sum_skill_value` starts from `inv.skills.value(skill)`
  (`skill_test.rs:1040`).
- **Each action's base difficulty comes from the right place.**
  `glossary/Difficulty_skill_tests.md`: fight value for attacking, shroud for
  investigating, evade value for evading, parenthetical for card-initiated tests.
  Engine: `actions.rs:112-127` (shroud, via `effective_shroud` so attachments
  count), `:795-830` (fight), `:847-872` (evade), and `Effect::SkillTest
  { difficulty }` for card tests (`dsl.rs:686-688`).
- **A revealed token's numeric modifier goes to the skill value, not to the
  difficulty.** `glossary/Chaos_Tokens.md`: *"If a revealed chaos token (or the
  effect referenced by a chaos token) has a numerical modifier, that modifier is
  applied to the investigator's skill value for this test."* `skill_test.rs:330-343`.
- **Mind over Matter's substitution is offered before the ST.1 player window.**
  Its ruling (`data/arkhamdb-faq/core/01036.md`): *"You need to play this card
  before the skill test begins. The type of skill test is determined before you
  get the opportunity to play Fast cards"*. The engine prompts at
  `start_skill_test` (`skill_test.rs:123-141`), *before* the ST.1 window opens,
  and drops the weapon's combat bonus on "yes" (`:179-191`) per the same
  ruling's *"ignore any bonuses to Combat or Agility"*. Playing Mind over Matter
  *inside* the ST.1 window therefore does not retro-apply to the live test —
  correct — and the round-scoped substitution it creates matches the card's own
  *"Until the end of the round"*, cleared at round end
  (`dispatch/phases.rs:1123`).
- **Retaliate lands after all ST.7 results, and does not require engagement.**
  `glossary/Retaliate.md`: *"Each time an investigator fails a skill test while
  attacking a ready enemy with the retaliate keyword, after applying all results
  for that skill test, that enemy performs an attack against the attacking
  investigator. An enemy does not exhaust after performing a retaliate attack."*
  plus *"This attack occurs whether the enemy is engaged with the attacking
  investigator or not."* The `PostRetaliate` step sits after `FireOnResolution`
  and before the ST.8 teardown (`skill_test.rs:857-870`);
  `fire_retaliate_if_any` (`:1257-1281`) gates on failed + Fight + still in play
  + `!exhausted` + `retaliate` and deliberately does **not** test engagement; the
  non-exhausting part is carried by `EnemyAttackSource::Retaliate`.
- **A skill test cannot begin during another one** — the invariant is
  rules-correct (`glossary/Nested_Skill_Tests.md`); only the *shape* of the
  refusal is wrong (finding 7).
- **`Trigger::OnCommit` effects are ungated on the outcome, and that is safe.**
  Vicious Blow 01025 and Deduction 01039 both say "if this skill test is
  successful"; the engine runs them at `FireOnCommit` regardless
  (`skill_test.rs:425-435`) but their only readers are success-only follow-ups,
  so the qualifier is honoured downstream. Deduction's ruling confirms the shape:
  *"it modifies the number of clues that you would find, it does not add an extra
  effect on top of any other effects"* — and the follow-up makes one discovery of
  `1 + bonus` (`:1182-1187`), not two discoveries.

## Uncertain

- **When a symbol token's variable modifier is fixed.** The Gathering's
  `[skull]` is *"−X (X = Ghouls at your location)"*; the engine evaluates the
  scenario hook once at reveal (ST.3/ST.4 —
  `crates/game-core/src/scenario.rs:125-129`: *"The hook is evaluated once at
  token reveal, so board-gated branches are decided up front"*), whereas
  `glossary/Modifiers.md` says a modified quantity is recalculated whenever a
  modifier is applied or removed, which would keep X live through ST.5. The
  vendored rules do not say whether a scenario reference card's X behaves as a
  live modifier or as a value fixed on reveal; the `[tablet]`'s board-gated
  damage has the same question. Today the two readings can only differ if
  something removes a Ghoul between ST.3 and ST.5, which needs the ST.4 ordering
  of finding 2 to be fixed first. **Settled by:** a ruling on variable chaos-token
  modifiers, or an FFG FAQ entry on the scenario reference card; neither is in
  the vendored set.
- **Whether retaliate falls before or after ST.8.** `glossary/Retaliate.md` (read
  in full) says only *"after applying all results for that skill test"*.
  "Applying results" is ST.7's own title, so the engine's placement — after ST.7,
  before the ST.8 teardown — is the better reading and I did not file it. The
  residual doubt is whether the committed-card discard and the token's return to
  the bag should precede the retaliate attack; the vendored text does not say,
  and it becomes observable only once a card keys off a committed card being in
  the discard pile. **Settled by:** a timing ruling on retaliate versus ST.8, or
  the first card that reads the discard pile mid-attack.
- **Whether the ST.2→ST.3 window should offer plays at all while a peril
  encounter is resolving**, and whether a *committed* card's Fast play should be
  filtered by the commit or by the peril rule. Finding 1 removes the corruption
  either way, but which rule does the filtering changes where the check lives.
  **Settled by:** #139's peril design, once it exists.
