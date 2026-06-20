# InvestigatorTurn Frame (slice 2a-i) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reify the active investigator's open turn (Rules Reference step 2.2.1) as a `Continuation::InvestigatorTurn` frame sitting above the `InvestigationPhase` anchor, absorbing the `pending_end_turn` cursor — a behaviour-preserving structural relocation.

**Architecture:** Today the open turn is *not* a frame: the `InvestigationPhase` anchor idles at `InvestigationResume::TurnBegins` (`is_open_turn()` → `drive` breaks with `Done`) and `state.pending_end_turn` flags an `end_turn` that stranded before rotation. This slice pushes an `InvestigatorTurn { investigator }` frame above the anchor when the `InvestigatorTurnBegins` window closes; `resume_end_turn` pops it; the idle is now "an `InvestigatorTurn` frame is on top" instead of "the anchor is at `TurnBegins`". `pending_end_turn` becomes an `ending: bool` field on the frame. No new anchor-resume variant is needed: `resume_end_turn` is always called *directly* (from `end_turn`, the stranded-skill-test resume, and the forced-run continuation), never re-entered through the anchor's `on_child_pop`, so the anchor stays at `TurnBegins` beneath the frame and is popped/rotated as before.

**Tech Stack:** Rust, `game-core` kernel crate. No new deps.

## Global Constraints

- **Behaviour-preserving:** every existing test must stay green by the end of each task (the C-checkpoint changes *structure*, not rules — spec §"Testing strategy"). The open-turn idle outcome stays `EngineOutcome::Done`; do **not** flip it to `AwaitingInput` (that is 2b/#205). (Decision, this slice.)
- **Validate-first / mutate-second** handler contract (CLAUDE.md): preconditions first, mutate + push events only after.
- **CI gauntlet, warnings-as-errors.** Before declaring done, run all host jobs: `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`. (The wasm jobs don't touch this engine-only change but stay green by construction.)
- **Design of record:** `docs/superpowers/specs/2026-06-20-unified-control-flow-model-design.md` §C (the per-phase CPS table — row "2.2.1 the active investigator's actions → frame `InvestigatorTurn` net-new") and §E (the `InvestigatorTurn` frame, 2a sub-checkpoint: "typed `PlayerAction` survive, accepted iff they match an offered option … existing tests keep working"). The **legal-action enumerator** named in §E is **slice 2a-ii**, NOT this slice — this slice ships only the frame + cursor absorption.
- **Commit-message footer** (every commit), copied verbatim:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
  ```
- **Branch:** `engine/investigator-turn-frame`. One branch for the slice; one commit per task.

---

### Task 1: Introduce the `InvestigatorTurn` continuation variant + builder helper

Pure type plumbing: add the variant, satisfy every exhaustive `match` on `Continuation`, classify it (not a phase anchor; does **not** await input so typed actions run at the open turn), and add a test-only builder helper. **The frame is never pushed yet**, so behaviour is unchanged and all tests stay green.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` — add the `Continuation::InvestigatorTurn` variant (~after the `*Phase` anchor variants, near line 522); extend `awaits_input` (~line 599), `as_resolution`/`as_resolution_mut` (~lines 608/629).
- Modify: `crates/game-core/src/state/builder.rs` — add `with_investigator_turn` (~near `with_phase_anchor`, line 275) and stage it in `build()` (~line 303).
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` — add the defensive `InvestigatorTurn` arm to `resolve_input`'s top-frame match (~line 353).
- Test: inline `#[cfg(test)]` in `crates/game-core/src/state/game_state.rs`.

**Interfaces:**
- Produces: `Continuation::InvestigatorTurn { investigator: InvestigatorId }` (a struct-like variant). `GameStateBuilder::with_investigator_turn(self, investigator: InvestigatorId) -> Self` — stages an `InvestigatorTurn` frame on top of whatever `with_phase_anchor` staged.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/game-core/src/state/game_state.rs`:

```rust
#[test]
fn investigator_turn_frame_classification() {
    let frame = Continuation::InvestigatorTurn {
        investigator: InvestigatorId(1),
    };
    // The open turn is not a framework anchor...
    assert!(!frame.is_phase_anchor());
    // ...and it does NOT await ResolveInput — typed actions (Move, Fight, …)
    // run against it, exactly as they ran against the TurnBegins anchor.
    assert!(!frame.awaits_input());
    // It carries no resolution payload.
    assert!(frame.as_resolution().is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core investigator_turn_frame_classification`
Expected: FAIL to compile — `no variant named InvestigatorTurn`.

- [ ] **Step 3: Add the variant and exhaustive-match arms**

In `crates/game-core/src/state/game_state.rs`, add the variant to `enum Continuation` (place it after the four `*Phase` anchor variants, before the closing `}` near line 522):

```rust
    /// The active investigator's open turn — Rules Reference step 2.2.1
    /// (slice 2a-i, #393). Pushed *above* the [`Continuation::InvestigationPhase`]
    /// anchor once the `InvestigatorTurnBegins` window closes; the anchor spans the
    /// whole phase beneath it. The player takes basic actions (each a typed
    /// `PlayerAction` today; a sub-resolution frame above this one tomorrow) while
    /// it is on top; `EndTurn` pops it via
    /// [`resume_end_turn`](crate::engine). Does **not** await `ResolveInput` — like
    /// the `TurnBegins` anchor it replaced, typed actions run against it (the idle
    /// outcome stays `Done`; surfacing the legal-action enumeration as
    /// `AwaitingInput` is slice 2b/#205).
    InvestigatorTurn {
        /// Whose turn this is. Mirrors [`GameState::active_investigator`] while on
        /// top; the durable source for the end-of-turn rotation.
        investigator: InvestigatorId,
    },
```

Extend `awaits_input` (the `match self` near line 599) — add an explicit arm so the open turn accepts typed actions (the default `!is_phase_anchor()` would wrongly return `true`):

```rust
    pub fn awaits_input(&self) -> bool {
        match self {
            Continuation::Resolution(f) => !f.pending_triggers.is_empty(),
            // The open turn: typed actions (Move/Investigate/Fight/…) run, so it
            // is NOT a mandatory ResolveInput prompt (slice 2a-i, #393).
            Continuation::InvestigatorTurn { .. } => false,
            other => !other.is_phase_anchor(),
        }
    }
```

Add `Continuation::InvestigatorTurn { .. }` to the `None`-returning lists in both `as_resolution` and `as_resolution_mut` (the long `| ...` chains near lines 611–624 and 632–645), e.g.:

```rust
            Continuation::SkillTest(_)
            | Continuation::Choice(_)
            | Continuation::HunterMove(_)
            | Continuation::SpawnEngage(_)
            | Continuation::HandSizeDiscard(_)
            | Continuation::ActRoundEnd(_)
            | Continuation::SubstitutionPrompt { .. }
            | Continuation::Mulligan { .. }
            | Continuation::EncounterDraw { .. }
            | Continuation::EncounterCard { .. }
            | Continuation::InvestigatorTurn { .. }
            | Continuation::MythosPhase { .. }
            | Continuation::InvestigationPhase { .. }
            | Continuation::EnemyPhase { .. }
            | Continuation::UpkeepPhase { .. } => None,
```

- [ ] **Step 4: Add the defensive `resolve_input` arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, in the `resolve_input` top-frame `match` (near line 353), add an arm before the `None =>` arm:

```rust
        // The open turn does not emit an AwaitingInput prompt in 2a (typed
        // actions drive it; the enumeration is 2a-ii / surfacing is 2b). A
        // ResolveInput arriving here is spurious — reject defensively, mirroring
        // the phase-anchor arm (slice 2a-i, #393).
        Some(Continuation::InvestigatorTurn { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (the open turn \
                     takes typed actions, not ResolveInput)"
                .into(),
        },
```

- [ ] **Step 5: Add the builder helper**

In `crates/game-core/src/state/builder.rs`, add after `with_phase_anchor` (near line 288):

```rust
    /// Stage an [`InvestigatorTurn`](Continuation::InvestigatorTurn) frame
    /// (slice 2a-i, #393) on top of the staged `*Phase` anchor — the realistic
    /// invariant for a state constructed mid-turn (the real driver pushes it once
    /// the `InvestigatorTurnBegins` window closes). Pair with
    /// `with_phase_anchor(InvestigationPhase { resume: TurnBegins })`.
    pub fn with_investigator_turn(mut self, investigator: InvestigatorId) -> Self {
        self.investigator_turn = Some(investigator);
        self
    }
```

Add the field to the builder struct (find the struct definition — it holds `phase_anchor: Option<Continuation>`; add alongside it):

```rust
    investigator_turn: Option<InvestigatorId>,
```

Initialize it in the builder's `Default`/`new` (wherever `phase_anchor` is initialized to `None`):

```rust
            investigator_turn: None,
```

In `build()` (near line 303, right after the `phase_anchor` is pushed onto `continuations`), push the frame above it:

```rust
        if let Some(investigator) = self.investigator_turn {
            continuations.push(Continuation::InvestigatorTurn { investigator });
        }
```

Ensure `InvestigatorId` is in scope in `builder.rs` (it already imports state types; add to the `use` if the compiler flags it).

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p game-core investigator_turn_frame_classification`
Expected: PASS.

- [ ] **Step 7: Run the full host gauntlet**

Run:
```
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS. (Behaviour is unchanged — the frame is never pushed by production code yet.)

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "engine: introduce InvestigatorTurn continuation variant (slice 2a-i of #393)

Type plumbing only: the open turn becomes a Continuation::InvestigatorTurn
variant classified as a non-anchor frame that accepts typed actions (does not
await ResolveInput). Adds the with_investigator_turn test builder. Production
never pushes the frame yet, so behaviour is unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

### Task 2: Push the frame at the open turn; route `end_turn` through it

The behaviour-preserving switchover. The `InvestigatorTurnBegins`-window-close (`anchor_on_child_pop`'s `TurnBegins` arm) now **pushes** the `InvestigatorTurn` frame instead of idling; `drive` idles on the frame (so `is_open_turn` is removed); `resume_end_turn` **pops** the frame before rotating. `pending_end_turn` is left exactly as-is this task (Task 3 absorbs it).

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` — the `TurnBegins` arm of `anchor_on_child_pop` (~line 834); `resume_end_turn` (~line 259).
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` — `drive` (~line 169).
- Modify: `crates/game-core/src/state/game_state.rs` — remove `is_open_turn` (~line 573) and its inline test if any.
- Modify (tests): every `#[cfg(test)]` site that constructs `InvestigationPhase { resume: TurnBegins }` and then drives `end_turn`/`EndTurn` — in `crates/game-core/src/engine/dispatch/phases.rs` (the push sites near lines 1551, 1619, 2617, 2904, 2964) — must also stage the `InvestigatorTurn` frame.

**Interfaces:**
- Consumes: `Continuation::InvestigatorTurn { investigator }`, `GameStateBuilder::with_investigator_turn` (Task 1).
- Produces: production code that leaves an `InvestigatorTurn` frame on top during the open turn; `resume_end_turn(cx, active_id)` now pops that frame as its first act.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/game-core/src/engine/dispatch/phases.rs`:

```rust
#[test]
fn open_turn_leaves_investigator_turn_frame_on_top() {
    use crate::state::{Continuation, InvestigationResume};
    // Reach the open turn the way production does: enter the Investigation
    // phase for a single investigator (no Fast cards → windows auto-skip).
    let mut state = GameStateBuilder::default()
        .with_investigator(test_investigator(1))
        .with_phase(Phase::Investigation)
        .build();
    state.turn_order = vec![InvestigatorId(1)];

    let mut events = Vec::new();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        // investigation_phase pushes the anchor + opens (auto-skips) both
        // windows, landing the first investigator's open turn.
        investigation_phase(&mut cx);
        super::super::drive(&mut cx, EngineOutcome::Done)
    };

    // The open turn idles as Done (NOT AwaitingInput) — behaviour-preserving.
    assert_eq!(outcome, EngineOutcome::Done);
    // Top frame is the InvestigatorTurn for investigator 1...
    assert_eq!(
        state.continuations.last(),
        Some(&Continuation::InvestigatorTurn {
            investigator: InvestigatorId(1),
        }),
    );
    // ...sitting above the still-present InvestigationPhase anchor.
    assert!(state.continuations.iter().any(|c| matches!(
        c,
        Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins
        }
    )));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core open_turn_leaves_investigator_turn_frame_on_top`
Expected: FAIL — `continuations.last()` is the `InvestigationPhase` anchor (no frame pushed), assertion on `InvestigatorTurn` fails.

- [ ] **Step 3: Push the frame in the `TurnBegins` arm**

In `crates/game-core/src/engine/dispatch/phases.rs`, replace the `TurnBegins` arm of `anchor_on_child_pop` (currently near line 834):

```rust
        Some(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        }) => {
            // 2.2.1 — the active investigator now acts as player-driven input;
            // no continuation work (slice 2 makes this an InvestigatorTurn frame).
            EngineOutcome::Done
        }
```

with:

```rust
        Some(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        }) => {
            // 2.2.1 — push the InvestigatorTurn frame above the anchor (slice
            // 2a-i, #393). The anchor stays at TurnBegins beneath it; the frame
            // is the open-turn idle point (drive breaks here, returning Done).
            // `active_investigator` was set by rotate_to_active in
            // begin_investigator_turn; it is the frame's investigator.
            let investigator = cx.state.active_investigator.unwrap_or_else(|| {
                unreachable!(
                    "TurnBegins reached with no active_investigator; \
                     begin_investigator_turn always sets it"
                )
            });
            cx.state
                .continuations
                .push(Continuation::InvestigatorTurn { investigator });
            EngineOutcome::Done
        }
```

- [ ] **Step 4: Idle `drive` on the frame; drop `is_open_turn`**

In `crates/game-core/src/engine/dispatch/mod.rs`, change the `drive` loop's anchor-advance guard (near line 169) from:

```rust
            Some(ref c) if c.is_phase_anchor() && !c.is_open_turn() => {
```

to:

```rust
            Some(ref c) if c.is_phase_anchor() => {
```

The `_ => return EngineOutcome::Done` arm below now also catches the `InvestigatorTurn` frame on top (the open-turn idle) — exactly the old `is_open_turn` break, now keyed off the frame's presence rather than the anchor's resume. Update the doc-comment on `drive` (the bullet "and with `Done` at the open turn (`InvestigationPhase{TurnBegins}`)") to say "and with `Done` when an `InvestigatorTurn` frame is on top (the open turn)".

Then delete the now-unused `is_open_turn` method from `crates/game-core/src/state/game_state.rs` (near line 573) and any inline test that referenced it.

- [ ] **Step 5: Pop the frame in `resume_end_turn`**

In `crates/game-core/src/engine/dispatch/phases.rs`, at the top of `resume_end_turn` (near line 259), pop the `InvestigatorTurn` frame before the existing rotate-or-end logic:

```rust
pub(super) fn resume_end_turn(cx: &mut Cx, active_id: InvestigatorId) -> EngineOutcome {
    // The turn is over: pop the InvestigatorTurn frame this turn ran on (slice
    // 2a-i, #393). It is always on top here — end_turn reaches this after the
    // EndOfTurn forced run resolves, the stranded-skill-test resume after the
    // SkillTest pops, and the forced-run continuation after its Resolution pops.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(crate::state::Continuation::InvestigatorTurn { investigator })
                if *investigator == active_id
        ),
        "resume_end_turn: expected InvestigatorTurn({active_id:?}) on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();

    // 2.2.2 decision: "return to 2.2" for the next investigator, or
    // proceed to 2.3. next_active_investigator_after skips eliminated
    // investigators (Rules Reference p.10) — the same shared helper the
    // Enemy phase uses.
    if let Some(next_id) = super::cursor::next_active_investigator_after(cx.state, active_id) {
        begin_investigator_turn(cx, next_id);
        EngineOutcome::Done
    } else {
        cx.state.active_investigator = None;
        // 2.3 → Enemy. The cascade may suspend on a hunter-movement tie
        // (Enemy 3.2); propagate its outcome rather than swallowing it.
        investigation_phase_end(cx)
    }
}
```

- [ ] **Step 6: Update the test push-sites**

For each `#[cfg(test)]` setup in `crates/game-core/src/engine/dispatch/phases.rs` that pushes `InvestigationPhase { resume: TurnBegins }` and then exercises `end_turn`/`EndTurn` (near lines 1551, 1619, 2617, 2904, 2964), push the `InvestigatorTurn` frame on top of the anchor so the constructed state matches the real open-turn invariant. Replace each block of the form:

```rust
        state
            .continuations
            .push(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            });
```

with:

```rust
        state
            .continuations
            .push(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            });
        // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame the
        // driver leaves on top once the InvestigatorTurnBegins window closes.
        state
            .continuations
            .push(crate::state::Continuation::InvestigatorTurn {
                investigator: <ACTIVE_ID>,
            });
```

where `<ACTIVE_ID>` is the `with_active_investigator(...)` id used in that test (e.g. `InvestigatorId(1)` at lines 1551/2617; check each site). For the two-investigator rotation test (near line 1619, active `InvestigatorId(1)`), use `InvestigatorId(1)` — the test then asserts rotation to `InvestigatorId(2)` after `end_turn` pops `1`'s frame and pushes `2`'s.

> NOTE for the implementer: run the failing tests after this edit and read each panic — the `resume_end_turn` `debug_assert` will point to any site whose active id you mismatched. Fix the id to match that test's `with_active_investigator`.

- [ ] **Step 7: Run the targeted + full test suite**

Run: `cargo test -p game-core open_turn_leaves_investigator_turn_frame_on_top`
Expected: PASS.

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: all PASS (this is the behaviour-preservation gate — every existing `end_turn`/rotation/phase-cascade test must still pass).

- [ ] **Step 8: Run the rest of the gauntlet**

```
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "engine: open turn becomes an InvestigatorTurn frame (slice 2a-i of #393)

The InvestigatorTurnBegins-window close now pushes a Continuation::InvestigatorTurn
frame above the InvestigationPhase anchor instead of idling at the TurnBegins
resume; drive idles on the frame (is_open_turn removed); resume_end_turn pops it
before rotating. Behaviour-preserving — the idle outcome stays Done and every
end_turn/rotation/phase-cascade test is unchanged. pending_end_turn is untouched
this commit (absorbed next).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

### Task 3: Absorb `pending_end_turn` into the frame's `ending` flag

`pending_end_turn` is the flag meaning "an `end_turn` stranded before rotation, so the resolving skill test must trigger rotation" — the discriminator `resume_skill_test_commit` uses to tell a stranded `EndOfTurn`-forced test from an ordinary mid-turn test. Move it onto the `InvestigatorTurn` frame as `ending: bool` and delete the `GameState` field.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` — add `ending: bool` to the `InvestigatorTurn` variant; remove the `pub pending_end_turn: Option<InvestigatorId>` field (~line 190).
- Modify: `crates/game-core/src/state/builder.rs` — drop the `pending_end_turn: None` initializer (~line 346); the `with_investigator_turn` push now sets `ending: false`.
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` — `end_turn`'s stranded branch (~line 243) sets the frame's `ending` instead of `pending_end_turn`.
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` — `resume_skill_test_commit`'s stranded branch (~line 303) reads the frame's `ending`.
- Modify (tests): `crates/cards/tests/persistent_treachery.rs` — the three `pending_end_turn.is_none()` asserts (lines 319, 336, 405).
- Modify: any site that constructs `Continuation::InvestigatorTurn { investigator }` now needs `ending: false` (Task 1/2 builder + test push-sites + the Task 2 `TurnBegins` arm). The compiler lists them all.

**Interfaces:**
- Consumes: the `InvestigatorTurn` frame (Tasks 1–2), `resume_end_turn` (Task 2, pops the frame).
- Produces: `Continuation::InvestigatorTurn { investigator, ending }`; no `GameState::pending_end_turn`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/game-core/src/engine/dispatch/phases.rs` (it exercises the field absorption directly — the full stranded-`EndOfTurn`-forced-into-skill-test flow is already covered by the existing Frozen-in-Fear integration test, which stays green as the behaviour gate):

```rust
#[test]
fn investigator_turn_defaults_to_not_ending() {
    use crate::state::Continuation;
    // The builder-staged open-turn frame is not mid-end-turn.
    let state = GameStateBuilder::default()
        .with_investigator(test_investigator(1))
        .with_phase(Phase::Investigation)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: crate::state::InvestigationResume::TurnBegins,
        })
        .with_active_investigator(InvestigatorId(1))
        .with_investigator_turn(InvestigatorId(1))
        .build();
    assert_eq!(
        state.continuations.last(),
        Some(&Continuation::InvestigatorTurn {
            investigator: InvestigatorId(1),
            ending: false,
        }),
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core investigator_turn_defaults_to_not_ending`
Expected: FAIL to compile — `InvestigatorTurn` has no field `ending`.

- [ ] **Step 3: Add the `ending` field**

In `crates/game-core/src/state/game_state.rs`, extend the `InvestigatorTurn` variant:

```rust
    InvestigatorTurn {
        /// Whose turn this is. Mirrors [`GameState::active_investigator`] while on
        /// top; the durable source for the end-of-turn rotation.
        investigator: InvestigatorId,
        /// `true` once `end_turn`'s `EndOfTurn` forced effect suspended into a
        /// skill test before rotation (a single Frozen in Fear 01164), stranding
        /// the turn (slice 2a-i, #393 — absorbs the former
        /// `GameState::pending_end_turn`). The skill-test commit resume reads this
        /// to decide the resolved test triggers rotation; an ordinary mid-turn
        /// test leaves it `false`.
        ending: bool,
    },
```

Update the Task 1/2 construction sites to `ending: false`: the `with_investigator_turn` push in `builder.rs`, the `TurnBegins` arm push in `phases.rs`, the test builder/push-sites, and the Task 1/2 unit tests' literal matches. The compiler enumerates every one — fix each to `{ investigator, ending: false }` (and the assertion-literals in `open_turn_leaves_investigator_turn_frame_on_top` and `investigator_turn_frame_classification`).

- [ ] **Step 4: Remove the `pending_end_turn` field**

In `crates/game-core/src/state/game_state.rs`, delete the field (near line 190):

```rust
    pub pending_end_turn: Option<InvestigatorId>,
```

(Keep `pending_enemy_attack` — it is a different cursor, lifted in slice 3 not here. Leave its doc-comment's "Mirror of `pending_end_turn`" reference; reword it to "Mirror of the former `pending_end_turn` (now the InvestigatorTurn frame's `ending`)" so the doc link doesn't dangle.)

In `crates/game-core/src/state/builder.rs`, delete the `pending_end_turn: None,` initializer (near line 346).

- [ ] **Step 5: Set `ending` in `end_turn`'s stranded branch**

In `crates/game-core/src/engine/dispatch/phases.rs`, in `end_turn`'s `AwaitingInput` arm (near line 238), replace the `pending_end_turn` write:

```rust
        EngineOutcome::AwaitingInput { .. } => {
            let forced_run_open = matches!(
                cx.state.continuations.last(),
                Some(crate::state::Continuation::Resolution(f)) if f.is_forced()
            );
            if !forced_run_open {
                cx.state.pending_end_turn = Some(active_id);
            }
            end_of_turn
        }
```

with:

```rust
        EngineOutcome::AwaitingInput { .. } => {
            let forced_run_open = matches!(
                cx.state.continuations.last(),
                Some(crate::state::Continuation::Resolution(f)) if f.is_forced()
            );
            if !forced_run_open {
                // Single suspending EndOfTurn forced (one Frozen in Fear): flag
                // the InvestigatorTurn frame (below the skill test) as ending, so
                // the commit-resume triggers rotation (slice 2a-i, #393 — was
                // pending_end_turn). A forced run owns its own EndOfTurnAfterForced
                // continuation, so it must NOT be flagged.
                let frame = cx
                    .state
                    .continuations
                    .iter_mut()
                    .rev()
                    .find_map(|c| match c {
                        crate::state::Continuation::InvestigatorTurn {
                            investigator,
                            ending,
                        } if *investigator == active_id => Some(ending),
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                        unreachable!(
                            "end_turn stranded with no InvestigatorTurn({active_id:?}) on the stack"
                        )
                    });
                *frame = true;
            }
            end_of_turn
        }
```

Update the doc-comment block above (lines ~218–228) that describes the two cases to reference the frame's `ending` flag instead of `pending_end_turn`.

- [ ] **Step 6: Read `ending` in `resume_skill_test_commit`**

In `crates/game-core/src/engine/dispatch/mod.rs` (near line 298–306), replace the stranded-resume branch:

```rust
                // Otherwise: a single suspending `EndOfTurn` forced effect
                // (one Frozen in Fear) stranded `end_turn` before rotation;
                // resume it now that the test is fully done (C4c, #235). An
                // `AwaitingInput` mid-teardown leaves `pending_end_turn` set
                // for the next resume.
                if let Some(active_id) = cx.state.pending_end_turn.take() {
                    return phases::resume_end_turn(cx, active_id);
                }
```

with:

```rust
                // Otherwise: a single suspending `EndOfTurn` forced effect
                // (one Frozen in Fear) stranded `end_turn` before rotation. The
                // SkillTest has popped, so the InvestigatorTurn frame is back on
                // top; if its `ending` flag is set, resume rotation now that the
                // test is fully done (C4c, #235; slice 2a-i absorbs
                // pending_end_turn). resume_end_turn pops the frame.
                if let Some(crate::state::Continuation::InvestigatorTurn {
                    investigator,
                    ending: true,
                }) = cx.state.continuations.last()
                {
                    let active_id = *investigator;
                    return phases::resume_end_turn(cx, active_id);
                }
```

- [ ] **Step 7: Update the integration-test asserts**

In `crates/cards/tests/persistent_treachery.rs`, the three `assert!(r.state.pending_end_turn.is_none());` lines (319, 336, 405) reference a field that no longer exists. These assert the turn is not mid-strand. Replace each with an assertion on the frame, e.g.:

```rust
    // No InvestigatorTurn frame is flagged as ending (turn not stranded mid-end).
    assert!(!r.state.continuations.iter().any(|c| matches!(
        c,
        game_core::state::Continuation::InvestigatorTurn { ending: true, .. }
    )));
```

(Match the crate's actual import path for `Continuation` — check the file's existing `use` lines; it may already alias `game_core::state::...`.)

- [ ] **Step 8: Run the targeted test + the Frozen-in-Fear gate**

Run: `cargo test -p game-core investigator_turn_defaults_to_not_ending`
Expected: PASS.

Find and run the existing stranded-`EndOfTurn` test (the single-Frozen-in-Fear / `EndOfTurn`-forced-into-skill-test case; grep `pending_end_turn` history or `EndOfTurn` in `crates/game-core/src/engine` and `crates/cards/tests` to locate it):

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: all PASS — particularly the Frozen-in-Fear stranded-rotation test and `persistent_treachery`.

- [ ] **Step 9: Run the rest of the gauntlet**

```
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all PASS.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "engine: absorb pending_end_turn into the InvestigatorTurn frame (slice 2a-i of #393)

The stranded-end-turn flag moves from GameState::pending_end_turn onto the
InvestigatorTurn frame as `ending: bool`; the field is removed. end_turn flags the
frame when a single EndOfTurn forced suspends before rotation; the skill-test
commit resume reads the flag to trigger rotation. Behaviour-preserving — the
Frozen in Fear stranded-rotation path is unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

## After the tasks

- **PR:** open against `main` with the repo template; design-decisions paragraph: the frame relocation + `pending_end_turn` absorption, the "no `AfterTurn` resume needed because `resume_end_turn` is always called directly" simplification, and that the legal-action enumerator is the *next* slice (2a-ii), not this one. `Closes` — there's no issue filed for the sub-slice; reference #393 and the spec. (Confirm with the user whether to file a tracking issue first, per the issue-first-PR norm.)
- **Phase/spec doc update (final commit, only once CI is green):** add a "2a-i shipped" note to the spec's Sequencing §2 line and/or `docs/phases/phase-7-the-gathering.md` ordering step 3, mirroring how 1a/1b were annotated. Record the one load-bearing decision (open turn is a frame above the anchor; `pending_end_turn` → `InvestigatorTurn.ending`; no `AfterTurn` resume).
- **Next slice (2a-ii):** the legal-action enumerator + validate-typed-against-offered (spec §E). Sets up `AttackLoop` (slice 3) and the keystone (slice 4).

## Self-review notes

- **Spec coverage:** §C row "2.2.1 → `InvestigatorTurn` net-new" ✅ (Tasks 1–2). §E "frame + pending_end_turn absorbed, typed actions survive, tests green" ✅ (Tasks 1–3; idle stays `Done`). §E enumerator ✅ explicitly deferred to 2a-ii (out of scope, stated). No other slice-2 spec clause is in this sub-slice.
- **Type consistency:** `InvestigatorTurn { investigator, ending }` — `investigator: InvestigatorId`, `ending: bool`. `resume_end_turn(cx, active_id: InvestigatorId)` signature unchanged (pops the frame). `with_investigator_turn(investigator: InvestigatorId)`. All construction sites carry both fields after Task 3.
- **Behaviour-preservation gate:** `RUSTFLAGS="-D warnings" cargo test --all --all-features` green at the end of every task; the open-turn idle stays `EngineOutcome::Done`; the Frozen-in-Fear stranded-rotation and `persistent_treachery` integration tests are the load-bearing guards.
