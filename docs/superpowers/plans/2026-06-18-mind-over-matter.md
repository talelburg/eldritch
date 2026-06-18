# Mind over Matter 01036 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Mind over Matter 01036 — a Fast event that, until end of round, lets the controller make a chosen Combat/Agility test as an Intellect test instead.

**Architecture:** Substituting *makes the test an Intellect test*, so the core trick is to rewrite the in-flight test's `skill` to `Intellect` (keeping `kind`) at test initiation, after a yes/no prompt — every skill-keyed path (base, icons, bonuses) then does the right thing for free. Round-scoped `skill_substitutions` state (cleared at the round boundary) drives the prompt; a `CardMetadata::play_only_during_turn()` accessor enforces the play-timing.

**Tech Stack:** Rust workspace — `card-dsl` (metadata), `game-core` (state/skill-test/play gate), `cards` (content). Tests: `cargo test`, per-card `#[cfg(test)]`, integration tests in `crates/cards/tests/`.

## Global Constraints

- CI runs `fmt`, `clippy`, `test`, `doc`, `wasm-build`, `wasm-test`, `wasm-clippy`, all warnings-as-errors. Match strict flags locally before pushing:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown` + `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- `card-dsl` sits below `game-core`. Escape card text like `\[reaction\]`/`` `[fast]` `` in doc comments (rustdoc parses `[x]` as an intra-doc link; the `doc` CI job fails otherwise).
- Handler contract: validate-first / mutate-second.
- Card text + rulings from the card's ArkhamDB page (`https://arkhamdb.com/card/01036`); FAQ is load-bearing here (substitution → Intellect *test*; intellect/wild icons + intellect bonuses; combat bonuses including a weapon's dropped; choice before the test begins; a genuine "may").
- Branch `engine/mind-over-matter` (already created, rebased on `main`). One PR. Commit subjects `scope: description`; bodies end with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Deferred (do not implement): skill-test PLAYER WINDOWs [#374](https://github.com/talelburg/eldritch/issues/374) (MoM unaffected — played before the test).

---

### Task 1: `CardMetadata::play_only_during_turn()` accessor

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs` (next to `is_fast()`, ~line 509)
- Test: `crates/card-dsl/src/card_data.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `CardMetadata::play_only_during_turn(&self) -> bool`.

- [ ] **Step 1: Write the failing test**

In a `#[cfg(test)]` module of `crates/card-dsl/src/card_data.rs` (reuse the existing test module that builds a `CardMetadata`, or add one mirroring `is_fast_tests`), add:

```rust
#[test]
fn play_only_during_turn_reads_the_text_clause() {
    use crate::card_data::{CardKind, CardMetadata, Class, SkillIcons};
    let mut m = CardMetadata {
        code: "01036".into(),
        name: "Mind over Matter".into(),
        traits: vec!["Insight".into()],
        text: Some("Fast. Play only during your turn.\nUntil the end of the round, …".into()),
        pack_code: "core".into(),
        kind: CardKind::Event {
            class: Class::Seeker,
            cost: Some(1),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
        },
    };
    assert!(m.play_only_during_turn());
    m.text = Some("Fast. Discover 1 clue.".into());
    assert!(!m.play_only_during_turn());
    m.text = None;
    assert!(!m.play_only_during_turn());
}
```

(If `SkillIcons::default()` / `CardMetadata` field set differs, copy the construction from the existing `is_fast_tests` module in the same file.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p card-dsl play_only_during_turn_reads_the_text_clause`
Expected: FAIL — no method `play_only_during_turn`.

- [ ] **Step 3: Add the accessor**

In `crates/card-dsl/src/card_data.rs`, immediately after the `is_fast()` method:

```rust
    /// Whether the card's printed text restricts it to "Play only during your
    /// turn" (Mind over Matter 01036, Working a Hunch 01037, …). Parse-on-read
    /// from the already-stored `text` — no separate pipeline field. Read at the
    /// play-timing gate (rare), so deriving it on read is fine.
    #[must_use]
    pub fn play_only_during_turn(&self) -> bool {
        self.text
            .as_deref()
            .is_some_and(|t| t.contains("Play only during your turn"))
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p card-dsl play_only_during_turn_reads_the_text_clause`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/card-dsl/src/card_data.rs
git commit -m "card-dsl: CardMetadata::play_only_during_turn() (parse-on-read)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `SkillSubstitution` state + round-boundary expiry

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (new struct + two fields)
- Modify: `crates/game-core/src/state/builder.rs` (defaults for the new fields)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (clear at round bump, ~line 367)
- Test: `crates/game-core/src/engine/dispatch/phases.rs` (`#[cfg(test)]`) — round-clear

**Interfaces:**
- Produces:
  - `pub struct SkillSubstitution { pub investigator: InvestigatorId, pub use_skill: SkillKind, pub for_skills: Vec<SkillKind> }`
  - `GameState.skill_substitutions: Vec<SkillSubstitution>`
  - `GameState.pending_substitution_prompt: Option<InvestigatorId>` (used by Task 3)

- [ ] **Step 1: Add the struct + fields**

In `crates/game-core/src/state/game_state.rs`, add the struct (near the other small state structs, e.g. by `PendingSkillModifier`):

```rust
/// An active "use X in place of Y" skill substitution (Mind over Matter
/// 01036). Round-scoped: cleared at the round boundary. While present, the
/// owning investigator may make a `for_skills` test as a `use_skill` test
/// instead (the choice is offered at test initiation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSubstitution {
    /// Whose tests this substitution applies to.
    pub investigator: InvestigatorId,
    /// The skill used in place of `for_skills` (Mind over Matter: Intellect).
    pub use_skill: SkillKind,
    /// The skills that may be replaced (Mind over Matter: Combat, Agility).
    pub for_skills: Vec<SkillKind>,
}
```

Add two fields to `pub struct GameState` (next to `pending_played_event`):

```rust
    /// Active round-scoped skill substitutions (Mind over Matter 01036).
    /// Cleared at the round boundary.
    pub skill_substitutions: Vec<SkillSubstitution>,
    /// Set while a skill test is paused on its "use X in place of Y?" prompt
    /// at initiation (Mind over Matter). Routes the next `ResolveInput` to
    /// `resume_substitution_choice`. Holds the test's investigator.
    pub pending_substitution_prompt: Option<InvestigatorId>,
```

Ensure `SkillKind` is in scope in `game_state.rs` (it's re-exported in state; add `use` if needed — it's `crate::card_data::SkillKind`, already used by `PendingSkillModifier`/`InFlightSkillTest`).

- [ ] **Step 2: Default the fields in the builder**

In `crates/game-core/src/state/builder.rs`, find where `GameState` is constructed (the `pending_played_event: None,` line, ~line 292) and add:

```rust
            skill_substitutions: Vec::new(),
            pending_substitution_prompt: None,
```

- [ ] **Step 3: Write the failing round-clear test**

In `crates/game-core/src/engine/dispatch/phases.rs` `#[cfg(test)]`, add a test that the round-bump clears `skill_substitutions`. Mirror an existing phases test that advances a round (e.g. one that checks `action_surcharge_spent_this_round` clearing, or drives `EndTurn`/phase stepping into a new round). Seed a substitution, advance to a new round, assert it's gone:

```rust
#[test]
fn round_bump_clears_skill_substitutions() {
    use crate::card_data::SkillKind;
    use crate::state::{InvestigatorId, SkillSubstitution};
    // Build a state at the round boundary the same way the neighbouring
    // round-bump test does, with one investigator. Seed a substitution.
    let mut state = /* GameStateBuilder … round-1 Investigation, one investigator */;
    state.skill_substitutions.push(SkillSubstitution {
        investigator: InvestigatorId(1),
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat, SkillKind::Agility],
    });
    // Drive the round bump (the same call the existing per-round-clear test
    // uses — e.g. stepping Upkeep→Mythos / the new-round transition).
    /* … advance one round … */
    assert!(state.skill_substitutions.is_empty());
}
```

(Fill the build/advance lines by copying the nearest existing round-transition test in this module; the assertion is the contract.)

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test -p game-core round_bump_clears_skill_substitutions`
Expected: FAIL (substitution still present — not yet cleared).

- [ ] **Step 5: Clear at the round bump**

In `crates/game-core/src/engine/dispatch/phases.rs`, at the round-counter bump (right after the `action_surcharge_spent_this_round` clear loop, ~line 368):

```rust
    // New round: round-scoped skill substitutions (Mind over Matter 01036)
    // expire "until the end of the round".
    cx.state.skill_substitutions.clear();
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p game-core round_bump_clears_skill_substitutions && cargo build -p game-core`
Expected: PASS / clean build.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/state/builder.rs crates/game-core/src/engine/dispatch/phases.rs
git commit -m "engine: SkillSubstitution state + round-boundary expiry

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: substitution prompt at test initiation + resume

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` (`start_skill_test` tail; new `open_commit_window` + `resume_substitution_choice`; a coverage helper)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resolve_input` routing)
- Test: `crates/game-core/src/engine/dispatch/skill_test.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `GameState.skill_substitutions`, `GameState.pending_substitution_prompt` (Task 2); `InFlightSkillTest.{skill, test_modifier}`; `InputRequest::choice` / `ChoiceOption` / `OptionId` / `PickSingle`.
- Produces: `resume_substitution_choice(cx, &InputResponse) -> EngineOutcome`; the prompt suspend in `start_skill_test`.

- [ ] **Step 1: Write the failing tests**

In `crates/game-core/src/engine/dispatch/skill_test.rs` `#[cfg(test)]`, add (these don't need a registry — substitution state and the skill rewrite are registry-free). Mirror the module's existing `start_skill_test`-driving tests for setup:

```rust
#[test]
fn combat_test_with_active_substitution_prompts_then_becomes_intellect_on_yes() {
    use crate::card_data::SkillKind;
    use crate::state::SkillSubstitution;
    // One Active investigator; non-empty chaos bag; build via the module's
    // usual helper.
    let mut state = /* … one investigator, Investigation, ChaosBag with a token … */;
    state.skill_substitutions.push(SkillSubstitution {
        investigator: InvestigatorId(1),
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat, SkillKind::Agility],
    });
    let mut events = Vec::new();
    let out = {
        let mut cx = Cx { state: &mut state, events: &mut events };
        start_skill_test(&mut cx, InvestigatorId(1), SkillKind::Combat,
            SkillTestKind::Fight, 3, SkillTestFollowUp::None, None, None, None, 2)
    };
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "substitution prompt");
    assert_eq!(state.pending_substitution_prompt, Some(InvestigatorId(1)));
    // "yes" (option 0) → the in-flight test is now an Intellect test, weapon
    // combat modifier dropped, and the commit window opens.
    let out = {
        let mut cx = Cx { state: &mut state, events: &mut events };
        super::resume_substitution_choice(&mut cx,
            &InputResponse::PickSingle(crate::engine::OptionId(0)))
    };
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "commit window");
    let ift = state.in_flight_skill_test.as_ref().unwrap();
    assert_eq!(ift.skill, SkillKind::Intellect);
    assert_eq!(ift.kind, SkillTestKind::Fight, "still a Fight (damage)");
    assert_eq!(ift.test_modifier, 0, "weapon combat bonus dropped");
    assert!(state.pending_substitution_prompt.is_none());
}

#[test]
fn combat_test_no_active_substitution_opens_commit_window_directly() {
    let mut state = /* … one investigator, Investigation, ChaosBag … (no substitution) … */;
    let mut events = Vec::new();
    let out = {
        let mut cx = Cx { state: &mut state, events: &mut events };
        start_skill_test(&mut cx, InvestigatorId(1), SkillKind::Combat,
            SkillTestKind::Fight, 3, SkillTestFollowUp::None, None, None, None, 0)
    };
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
    assert!(state.pending_substitution_prompt.is_none(), "no prompt");
    assert_eq!(state.in_flight_skill_test.as_ref().unwrap().skill, SkillKind::Combat);
}

#[test]
fn substitution_choice_no_keeps_the_printed_skill() {
    use crate::card_data::SkillKind;
    use crate::state::SkillSubstitution;
    let mut state = /* … one investigator, Investigation, ChaosBag … */;
    state.skill_substitutions.push(SkillSubstitution {
        investigator: InvestigatorId(1),
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat, SkillKind::Agility],
    });
    let mut events = Vec::new();
    {
        let mut cx = Cx { state: &mut state, events: &mut events };
        let _ = start_skill_test(&mut cx, InvestigatorId(1), SkillKind::Agility,
            SkillTestKind::Evade, 3, SkillTestFollowUp::None, None, None, None, 0);
    }
    let out = {
        let mut cx = Cx { state: &mut state, events: &mut events };
        super::resume_substitution_choice(&mut cx,
            &InputResponse::PickSingle(crate::engine::OptionId(1)))
    };
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "commit window");
    assert_eq!(state.in_flight_skill_test.as_ref().unwrap().skill, SkillKind::Agility);
}
```

(Fill the state-build lines from the module's existing `start_skill_test` tests — they construct a one-investigator Investigation state with a `ChaosBag`. Keep the assertions.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p game-core substitution`
Expected: FAIL (`resume_substitution_choice` missing; no prompt suspend).

- [ ] **Step 3: Factor out the commit-window open**

In `start_skill_test`, the tail currently pushes `Continuation::SkillTest`, emits `SkillTestStarted`, and returns the commit `AwaitingInput`. Keep the `SkillTestStarted` emit at record-creation, and extract the *commit-window open* (push frame + return prompt) into a helper. Replace the tail:

```rust
    cx.events.push(Event::SkillTestStarted {
        investigator,
        skill,
        difficulty,
    });

    // "Use X in place of Y?" (Mind over Matter 01036): if this is a Combat or
    // Agility test and the investigator has a covering round-scoped
    // substitution active, offer the choice BEFORE the commit window — the
    // test type is fixed here, per the card's FAQ. The in-flight record (just
    // created) is the parking; resume rewrites its skill on "yes".
    if substitution_covers(cx.state, investigator, skill) {
        cx.state.pending_substitution_prompt = Some(investigator);
        let use_skill = SkillKind::Intellect; // sole substitution in scope
        return EngineOutcome::AwaitingInput {
            request: InputRequest::choice(
                format!(
                    "{investigator:?}: use {use_skill:?} in place of {skill:?} for this test? \
                     (PickSingle(0) = use {use_skill:?}, PickSingle(1) = keep {skill:?})",
                ),
                vec![
                    ChoiceOption { id: OptionId(0), label: format!("Use {use_skill:?}") },
                    ChoiceOption { id: OptionId(1), label: format!("Keep {skill:?}") },
                ],
            ),
            resume_token: ResumeToken(0),
        };
    }
    open_commit_window(cx)
}

/// Whether `investigator` has an active round-scoped substitution covering a
/// `skill` test (Mind over Matter 01036: Intellect for Combat/Agility).
fn substitution_covers(
    state: &crate::state::GameState,
    investigator: InvestigatorId,
    skill: SkillKind,
) -> bool {
    state.skill_substitutions.iter().any(|s| {
        s.investigator == investigator && s.for_skills.contains(&skill)
    })
}

/// Push the skill-test resume frame and return the commit-window `AwaitingInput`
/// for the in-flight test. Shared by `start_skill_test` (no-substitution path)
/// and `resume_substitution_choice`.
fn open_commit_window(cx: &mut Cx) -> EngineOutcome {
    let (investigator, skill, difficulty) = {
        let t = cx
            .state
            .in_flight_skill_test
            .as_ref()
            .expect("open_commit_window: in-flight test must exist");
        (t.investigator, t.skill, t.difficulty)
    };
    cx.state
        .continuations
        .push(crate::state::Continuation::SkillTest);
    EngineOutcome::AwaitingInput {
        request: InputRequest::prompt(format!(
            "Commit cards from hand for {investigator:?}'s {skill:?} skill test \
             (difficulty {difficulty}). Empty indices commits no cards.",
        )),
        resume_token: ResumeToken(0),
    }
}
```

Add the imports `start_skill_test` needs at the top of `skill_test.rs`: `ChoiceOption`, `OptionId` (from `crate::engine::outcome` / wherever `InputRequest` comes from — match the existing `InputRequest`/`ResumeToken` import).

- [ ] **Step 4: Add `resume_substitution_choice`**

In `skill_test.rs`:

```rust
/// Resume the Mind over Matter substitution prompt: `PickSingle(0)` rewrites
/// the in-flight test to an Intellect test (dropping any weapon combat bonus
/// per the FAQ); `PickSingle(1)` keeps the printed skill. Either way, opens the
/// commit window. (#322)
pub(in crate::engine) fn resume_substitution_choice(
    cx: &mut Cx,
    response: &InputResponse,
) -> EngineOutcome {
    let InputResponse::PickSingle(OptionId(opt)) = response else {
        return EngineOutcome::Rejected {
            reason: "substitution prompt expects PickSingle(0|1)".into(),
        };
    };
    if *opt > 1 {
        return EngineOutcome::Rejected {
            reason: format!("substitution prompt: PickSingle({opt}) out of range (0|1)").into(),
        };
    }
    cx.state.pending_substitution_prompt = None;
    if *opt == 0 {
        // Use Intellect: the test becomes an Intellect test (icons/bonuses
        // follow), and a weapon's combat bonus (test_modifier) is dropped —
        // FAQ "ignore any bonuses to Combat or Agility". Bonus damage is
        // separate (extra_damage / bonus_attack_damage) and untouched.
        let t = cx
            .state
            .in_flight_skill_test
            .as_mut()
            .expect("resume_substitution_choice: in-flight test must exist");
        t.skill = SkillKind::Intellect;
        t.test_modifier = 0;
    }
    open_commit_window(cx)
}
```

- [ ] **Step 5: Route it in `resolve_input`**

In `crates/game-core/src/engine/dispatch/mod.rs`, at the **top** of `resolve_input` (before the `Continuation::Resolution` check — the substitution prompt is the most-recent outstanding input even when a forced run / window frame sits below it, e.g. a treachery agility test):

```rust
    // Mind over Matter (#322): a skill test paused on its "use X in place of
    // Y?" prompt at initiation. Set only between the prompt and the commit
    // window, so it unambiguously owns the next input — route it first.
    if cx.state.pending_substitution_prompt.is_some() {
        return skill_test::resume_substitution_choice(cx, response);
    }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p game-core substitution && cargo test -p game-core --lib skill_test && cargo build -p game-core`
Expected: PASS; existing skill-test tests still green (no substitution active ⇒ unchanged path).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch/skill_test.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: skill substitution prompt at test initiation (Mind over Matter)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `check_play_card` "play only during your turn" gate

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`check_play_card`, the Fast-event gate ~line 1374)
- Test: covered by Task 6's integration tests (needs real card text via the registry). A game-core unit test is impractical (the gate reads `metadata_for(code)`); rely on Task 6.

**Interfaces:**
- Consumes: `CardMetadata::play_only_during_turn()` (Task 1); `card_registry::current()`.
- Produces: the tightened gate.

- [ ] **Step 1: Read the flag in `check_play_card`**

In `check_play_card`, after `card_type` is resolved and before the `allowed` computation (~line 1374), add:

```rust
    // "Play only during your turn" (Mind over Matter 01036, Working a Hunch
    // 01037, …): tighten the Fast gate to the active investigator's
    // Investigation turn — never an out-of-turn permissive Fast window (the
    // Mythos `MythosAfterDraws` window). Read parse-on-read from metadata.
    let only_during_turn = card_registry::current()
        .and_then(|reg| (reg.metadata_for)(&code))
        .is_some_and(CardMetadata::play_only_during_turn);
```

(`CardMetadata` is already imported in this file via `crate::card_data`; if not, add `use crate::card_data::CardMetadata;`.)

- [ ] **Step 2: Apply it in the gate**

Change the Fast-event arm so a `only_during_turn` card requires
`active_during_investigation` (no permissive-window allowance):

```rust
    let allowed = if is_fast {
        match card_type {
            CardType::Event => {
                if only_during_turn {
                    active_during_investigation
                } else {
                    active_during_investigation || permissive_window
                }
            }
            CardType::Asset => {
                if only_during_turn {
                    active_during_investigation
                } else {
                    active_during_investigation || (owner_is_active && permissive_window)
                }
            }
            _ => active_during_investigation,
        }
    } else {
        active_during_investigation
    };
```

- [ ] **Step 3: Build + run existing play-card tests**

Run: `cargo build -p game-core && cargo test -p game-core --lib && cargo test -p cards --test fast_play`
Expected: clean; existing tests pass (cards without the clause are unaffected — `only_during_turn` is `false`).

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: 'play only during your turn' tightens the Fast play gate

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Mind over Matter 01036 card + register

**Files:**
- Create: `crates/cards/src/impls/mind_over_matter.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`pub mod`, `abilities_for` arm, `native_effect_for` arm)
- Test: `crates/cards/src/impls/mind_over_matter.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `card_dsl::dsl::{on_play, native, Ability}`; `game_core::{Cx, EngineOutcome, EvalContext}`; `game_core::card_registry::NativeEffectFn`; `game_core::state::SkillSubstitution`; `card_dsl::card_data::SkillKind`.
- Produces: `mind_over_matter::{CODE, abilities, native_effect_for}`.

- [ ] **Step 1: Create the card module + unit test**

Create `crates/cards/src/impls/mind_over_matter.rs`:

```rust
//! Mind over Matter (Seeker insight event, 01036).
//!
//! ```text
//! Fast. Play only during your turn.
//! Until the end of the round, you may use your [intellect] in place of your
//!   [combat] and [agility].
//! ```
//!
//! One `OnPlay` native: push a round-scoped [`SkillSubstitution`] letting the
//! controller make a Combat/Agility test as an Intellect test instead (offered
//! at test initiation; intellect/wild icons + intellect bonuses apply, a
//! weapon's combat bonus is dropped — see the engine's substitution prompt).
//! "Fast" + "Play only during your turn" come from the corpus metadata (`Fast.`
//! ⇒ `is_fast`; the clause ⇒ `CardMetadata::play_only_during_turn()`), enforced
//! by the play-card gate — no per-card play-timing code here.

use card_dsl::dsl::{native, on_play, Ability};
use game_core::card_data::SkillKind;
use game_core::card_registry::NativeEffectFn;
use game_core::state::SkillSubstitution;
use game_core::{Cx, EngineOutcome, EvalContext};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01036";

const SUBSTITUTE: &str = "01036:intellect-substitution";

/// `OnPlay`: activate the round-scoped Intellect-for-Combat/Agility substitution.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_play(native(SUBSTITUTE))]
}

/// Resolve this card's native-effect tag. Wired into the crate registry's
/// `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == SUBSTITUTE).then_some(substitute as NativeEffectFn)
}

/// Push the round-scoped substitution for the controller.
fn substitute(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    cx.state.skill_substitutions.push(SkillSubstitution {
        investigator: ctx.controller,
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat, SkillKind::Agility],
    });
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn ability_is_on_play_native() {
        let a = super::abilities();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].trigger, Trigger::OnPlay);
        assert!(matches!(&a[0].effect, Effect::Native { tag } if tag == super::SUBSTITUTE));
    }

    #[test]
    fn native_resolves_only_its_tag() {
        assert!(super::native_effect_for(super::SUBSTITUTE).is_some());
        assert!(super::native_effect_for("01036:other").is_none());
    }
}
```

(Confirm `SkillSubstitution` is re-exported at `game_core::state::SkillSubstitution` — Task 2 defines it in `state::game_state`; add it to the `state` module's re-exports if not already public there.)

- [ ] **Step 2: Register the module**

In `crates/cards/src/impls/mod.rs`: add `pub mod mind_over_matter;` in name order (after `medical_texts` / before `old_book_of_lore`); add the `abilities_for` arm `mind_over_matter::CODE => Some(mind_over_matter::abilities()),`; and add to the `native_effect_for` chain `.or_else(|| mind_over_matter::native_effect_for(tag))`.

- [ ] **Step 3: Run the unit tests**

Run: `cargo test -p cards mind_over_matter`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/cards/src/impls/mind_over_matter.rs crates/cards/src/impls/mod.rs
git commit -m "cards: Mind over Matter 01036 (intellect substitution push)

Closes #322.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Integration tests (end-to-end against the real registry)

**Files:**
- Create: `crates/cards/tests/mind_over_matter.rs`

**Interfaces:**
- Consumes: `cards::REGISTRY`; `game_core::{apply, Action, PlayerAction, InputResponse, OptionId}`; the public Fight/Evade actions; test_support builders.

- [ ] **Step 1: Write the integration tests**

Create `crates/cards/tests/mind_over_matter.rs`. Mirror the seating/harness of an existing combat integration test (`crates/cards/tests/weapon_38_special.rs` for a weapon Fight; `crates/cards/tests/medical_texts.rs` for the `apply_no_commits` commit-window helper and a `Numeric(0)` chaos bag). Cover:

```rust
//! #322 integration: Mind over Matter 01036 — substituting Intellect for a
//! Combat/Agility test, the play-timing gate, and the weapon-bonus drop.
//! Own process → installs `cards::REGISTRY`.

// 1. play_during_turn_then_fight_substitutes_to_intellect:
//    Seat Roland (combat C, intellect I, with I != C and chosen so the test
//    outcome differs) at a location with one engaged enemy; Mind over Matter in
//    hand. Numeric(0) chaos bag. Play MoM (Fast, Investigation, active) → Done,
//    MoM in discard, substitution active. Take the Fight action → substitution
//    prompt (AwaitingInput). PickSingle(0) → commit window; resolve commits;
//    assert the test resolved on the INTELLECT value (set I high enough to
//    pass where C would fail, or assert SkillTestSucceeded/Failed accordingly)
//    and that the enemy took the attack's damage on success (kind == Fight).
//
// 2. fight_declining_substitution_uses_combat:
//    Same board; at the prompt PickSingle(1) → the test resolves on COMBAT
//    (assert the opposite outcome from #1), proving the genuine "may".
//
// 3. weapon_fight_substitution_drops_combat_bonus_keeps_damage:
//    Roland with .38 Special in play + an engaged enemy; MoM active. Activate
//    the .38 Special fight ability → substitution prompt → PickSingle(0):
//    assert the test value uses intellect with NO weapon +combat bonus, and on
//    success the enemy still takes the weapon's bonus damage.
//
// 4. only_intellect_icons_committable_when_substituted:
//    With substitution chosen (Intellect test), an [intellect]/wild skill card
//    in hand is committable and a [combat]-only one is rejected by the commit
//    validator.
//
// 5. mind_over_matter_rejected_outside_your_turn:
//    Drive to the Mythos `MythosAfterDraws` fast window; playing MoM there is
//    Rejected (play-only-during-your-turn gate).
//
// 6. working_a_hunch_rejected_outside_your_turn (regression):
//    Working a Hunch 01037 likewise rejected in the Mythos window.
```

Implement each as a `#[test]` with the assertions above, copying setup from the named existing tests. For the chaos bag use `ChaosBag::new([ChaosToken::Numeric(0)])` so outcomes are deterministic; pick combat/intellect values that straddle the difficulty so substitution changes pass/fail observably.

- [ ] **Step 2: Run the integration tests**

Run: `cargo test -p cards --test mind_over_matter`
Expected: PASS (all cases).

- [ ] **Step 3: Commit**

```bash
git add crates/cards/tests/mind_over_matter.rs
git commit -m "test: Mind over Matter 01036 end-to-end (substitution, weapon, timing)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Full gauntlet + PR + phase-doc

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green (`cargo fmt` first if `--check` complains).

- [ ] **Step 2: Push + open the PR**

```bash
git push -u origin engine/mind-over-matter
gh pr create --fill
```

PR body: describe the substitution-becomes-an-Intellect-test model, the prompt at initiation (genuine "may"), the round-scoped state, the weapon-bonus drop, and the `play_only_during_turn` accessor (also fixing Working a Hunch). Cite the FAQ. "Closes #322". Watch CI: `gh pr checks <PR#> --watch`.

- [ ] **Step 3: Update the phase doc once CI is green**

In `docs/phases/phase-7-the-gathering.md`: flip Mind over Matter (#322) to `✅ PR #<n>` in the C6b row and the Axis-E note (**this closes the last Axis-E carved card**). Add one Decisions entry:

> **Mind over Matter 01036 — skill substitution as an Intellect-test rewrite (#322, PR #<n>).** Per the FAQ, "use intellect in place of combat/agility" *makes the test an Intellect test* — modeled by rewriting `InFlightSkillTest.skill = Intellect` (keeping `kind`) at initiation, so intellect/wild icons + intellect stat-bonuses follow for free and combat/agility stat-bonuses drop; a weapon's `test_modifier` combat bonus is zeroed too (bonus damage kept). It's a genuine "may" (a player can want to fail), so a yes/no prompt fires in `start_skill_test` before the commit window (routed via `pending_substitution_prompt`); round-scoped `skill_substitutions` (cleared at the round bump) drives it. "Play only during your turn" is a parse-on-read `CardMetadata::play_only_during_turn()` accessor tightening the Fast gate to `active_during_investigation` (also fixes Working a Hunch 01037). Skill-test PLAYER WINDOWs remain unmodeled (#374).

- [ ] **Step 4: Commit + push the doc**

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — close #322 (Mind over Matter), last Axis-E card

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push
```

- [ ] **Step 5: Confirm CI green; await user approval to merge** (`gh pr merge <PR#> --squash --delete-branch`). Do not merge without explicit approval.

---

## Self-Review

**Spec coverage:** Component 1 (state + expiry) → Task 2 ✓; Component 2 (prompt at initiation → Intellect-test rewrite, weapon `test_modifier` drop, genuine "may") → Task 3 ✓; Component 3 (`play_only_during_turn` accessor + gate) → Tasks 1 + 4 ✓; Component 4 (card + Native push) → Task 5 ✓; testing (intellect test, weapon drop, icons, play-timing, Working a Hunch regression) → Task 6 ✓; deferred #374 noted (untouched). No gaps.

**Placeholder scan:** Tasks 2/3/6 leave state-construction setup as "copy the nearest existing test's builder lines" — deliberate (the `GameStateBuilder`/`ChaosBag` seating is mechanical and version-specific; the assertions are the contract). No `TODO`/`TBD` in implementation code.

**Type consistency:** `SkillSubstitution { investigator, use_skill, for_skills }`, `GameState.skill_substitutions` / `pending_substitution_prompt`, `resume_substitution_choice(cx, &InputResponse)`, `substitution_covers(state, investigator, skill)`, `open_commit_window(cx)`, `CardMetadata::play_only_during_turn()`, `mind_over_matter::{CODE, abilities, native_effect_for, SUBSTITUTE}` — consistent across tasks. The substitution rewrites `InFlightSkillTest.{skill, test_modifier}` (both existing fields). `SkillKind::Intellect` is the sole `use_skill`.
