# Anchor single-hit forced effects to their source card — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Anchor the single-hit interactive forced-acknowledge option to its source card, so a forced ability on an in-play card glows it (the one forced path still emitting `OptionTarget::Global`).

**Architecture:** Extract the `CandidateSource → OptionTarget` mapping (inline in `build_resolution_options`) into a shared `candidate_anchor` helper; carry the `ResolutionCandidate` on the `AcknowledgeForced` continuation frame; have `drive_acknowledge_forced` anchor its one option via the helper. Anchor is display-only. Engine-only; no web change (in-play sources ride the existing `InPlayCardView` matcher).

**Tech Stack:** Rust (`game-core`).

**Design spec:** `docs/superpowers/specs/2026-07-16-forced-effect-card-anchor-design.md`

## Global Constraints

- **Anchors are display-only.** No resolve path may read the anchor; `resume_acknowledge_forced` still validates only `OptionId(0)`.
- **Serde:** the new field is required (no `#[serde(default)]`) — the seed never holds this frame and the action log replays with current code (#453 error-on-skew convention).
- **CI is 7 warnings-as-errors jobs.** Match locally before pushing:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `wasm-pack test --headless --firefox crates/web`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
  - (The web jobs are unaffected by this engine-only change but are part of the gauntlet.)
- Commit subjects: `scope: description`. Branch `engine/forced-effect-anchor` (already created).

## File structure

- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — Task 1 (extract `candidate_anchor`, `current_act_code` → `pub(super)`, `build_resolution_options` delegates).
- `crates/game-core/src/state/game_state.rs` — Task 2 (`AcknowledgeForced` field).
- `crates/game-core/src/engine/dispatch/forced_triggers.rs` — Task 2 (construction, `drive_acknowledge_forced`, tests).

Task 2 depends on Task 1's `candidate_anchor`/`current_act_code` visibility. Task 1 is a behavior-preserving refactor reviewable on its own.

---

### Task 1: Extract the shared `candidate_anchor` helper

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`current_act_code` → `pub(super)`; add `candidate_anchor`; `build_resolution_options` delegates; add a unit test)

**Interfaces:**
- Produces: `pub(super) fn candidate_anchor(cand: &ResolutionCandidate, current_act: Option<&CardCode>) -> crate::engine::OptionTarget` — `Hand → HandCardByCode`, `InPlay(id) → CardInstance(id)`, `Board → Act` (iff code is the current act) else `Global`.
- Produces: `pub(super) fn current_act_code(state: &GameState) -> Option<CardCode>` (visibility widened).

- [ ] **Step 1: Write the failing test**

In `reaction_windows.rs`, module `resolution_option_anchor_tests` (has `use super::*;`), add:

```rust
    #[test]
    fn candidate_anchor_maps_each_source() {
        use crate::engine::OptionTarget;
        use crate::state::{CardCode, CardInstanceId, InvestigatorId, ResolutionCandidate};
        let act = CardCode::new("01109");
        let inplay = ResolutionCandidate::new(
            CardCode::new("01020"),
            InvestigatorId(1),
            0,
            CandidateSource::InPlay(CardInstanceId(5)),
        );
        let hand = ResolutionCandidate::new(
            CardCode::new("01022"),
            InvestigatorId(2),
            0,
            CandidateSource::Hand,
        );
        let board_act =
            ResolutionCandidate::new(act.clone(), InvestigatorId(1), 0, CandidateSource::Board);
        let board_other = ResolutionCandidate::new(
            CardCode::new("_other"),
            InvestigatorId(1),
            0,
            CandidateSource::Board,
        );
        assert_eq!(
            candidate_anchor(&inplay, Some(&act)),
            OptionTarget::CardInstance(CardInstanceId(5))
        );
        assert_eq!(
            candidate_anchor(&hand, Some(&act)),
            OptionTarget::HandCardByCode {
                investigator: InvestigatorId(2),
                code: CardCode::new("01022"),
            }
        );
        assert_eq!(candidate_anchor(&board_act, Some(&act)), OptionTarget::Act);
        assert_eq!(candidate_anchor(&board_other, Some(&act)), OptionTarget::Global);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core --lib candidate_anchor_maps_each_source`
Expected: FAIL — `cannot find function 'candidate_anchor'`.

- [ ] **Step 3: Add the helper, widen `current_act_code`, delegate**

In `reaction_windows.rs`, change `current_act_code`'s signature to `pub(super)`:

```rust
pub(super) fn current_act_code(state: &GameState) -> Option<CardCode> {
```

Add `candidate_anchor` immediately above `build_resolution_options`:

```rust
/// The board anchor for a resolution candidate's source: an in-play instance to
/// its card (#539); a Fast hand event by code — every copy (#539); a board-wide
/// effect to the act card when its code is the current act, else no card home
/// (#540/#553). Shared by [`build_resolution_options`] and the forced-ack path.
pub(super) fn candidate_anchor(
    cand: &ResolutionCandidate,
    current_act: Option<&CardCode>,
) -> crate::engine::OptionTarget {
    use crate::engine::OptionTarget;
    match cand.source {
        CandidateSource::Hand => OptionTarget::HandCardByCode {
            investigator: cand.controller,
            code: cand.code.clone(),
        },
        CandidateSource::InPlay(instance_id) => OptionTarget::CardInstance(instance_id),
        CandidateSource::Board => {
            if current_act == Some(&cand.code) {
                OptionTarget::Act
            } else {
                OptionTarget::Global
            }
        }
    }
}
```

Replace the body of `build_resolution_options`'s `.map(...)` closure — keep the per-source **label** inline, delegate the **target** to the helper:

```rust
        .map(|(i, cand)| {
            let id = OptionId(u32::try_from(i).expect("option count fits in u32"));
            // Label distinguishes a hand Fast-event play from an in-play/board
            // reaction; the board anchor is the shared `candidate_anchor` (#553).
            let label = match cand.source {
                CandidateSource::Hand => format!("Play {} from hand", cand.code),
                CandidateSource::InPlay(_) | CandidateSource::Board => {
                    format!("Resolve reaction: {}", cand.code)
                }
            };
            ChoiceOption::new(id, label, candidate_anchor(cand, current_act))
        })
```

(This replaces the previous `let (label, target) = match cand.source { … }; ChoiceOption::new(id, label, target)` block. The output is byte-identical: `Hand → "Play {code} from hand"` + `HandCardByCode`; `InPlay → "Resolve reaction: {code}"` + `CardInstance`; `Board → "Resolve reaction: {code}"` + `Act`/`Global`.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p game-core --lib -- resolution_option_anchor_tests`
Expected: PASS — the new `candidate_anchor_maps_each_source` plus the unchanged `resolution_options_anchor_by_candidate_source` and `board_candidate_matching_current_act_anchors_to_act` (behavior-identical refactor).

- [ ] **Step 5: Clippy + commit**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean.

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: extract shared candidate_anchor helper (S5 follow-up)"
```

---

### Task 2: Forced-ack frame carries its candidate + anchors the option

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`AcknowledgeForced` field + doc)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (construction, `drive_acknowledge_forced`, migrate 2 tests, add anchor test)

**Interfaces:**
- Consumes: `reaction_windows::candidate_anchor`, `reaction_windows::current_act_code` (Task 1).
- Changes: `Continuation::AcknowledgeForced { source: CardCode }` → `{ candidate: ResolutionCandidate }`.

- [ ] **Step 1: Write the failing test**

In `forced_triggers.rs`'s `#[cfg(test)] mod tests` (has `use super::*;`), add:

```rust
    #[test]
    fn acknowledge_forced_anchors_the_option_to_its_source_card() {
        use crate::engine::OptionTarget;
        use crate::state::Continuation;
        use crate::test_support::GameStateBuilder;

        // A forced ability on an in-play instance surfaces a one-option pick
        // anchored to that card (#553), not Global.
        let mut state = GameStateBuilder::default().build();
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01020"),
                InvestigatorId(1),
                0,
                CandidateSource::InPlay(CardInstanceId(5)),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match super::drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
                assert_eq!(
                    request.options[0].target,
                    OptionTarget::CardInstance(CardInstanceId(5)),
                );
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p game-core --lib acknowledge_forced_anchors_the_option_to_its_source_card`
Expected: FAIL — compile error: `Continuation::AcknowledgeForced` has no field `candidate` (still `source: CardCode`); the two existing `acknowledge_forced_*` tests also stop compiling once the field changes, so they migrate in Step 3.

- [ ] **Step 3: Change the frame, construction, drive, and migrate the existing tests**

**3a.** In `state/game_state.rs`, change the variant + doc. Replace:

```rust
    /// `fire_forced_triggers` (the single-hit path) *above* the forced effect's
    /// root frame; the `drive` loop suspends here, and on resume pops, letting the
    /// effect frame beneath resolve. `source` is the card the forced ability is
    /// printed on (for the prompt's display name).
    AcknowledgeForced { source: CardCode },
```

with:

```rust
    /// `fire_forced_triggers` (the single-hit path) *above* the forced effect's
    /// root frame; the `drive` loop suspends here, and on resume pops, letting the
    /// effect frame beneath resolve. `candidate` is the forced ability's
    /// [`ResolutionCandidate`] — its `code` names the prompt and its `source`
    /// anchors the option to the board card (#553).
    AcknowledgeForced { candidate: ResolutionCandidate },
```

**3b.** In `forced_triggers.rs`, the `#466` construction (`fire_forced_triggers`) — replace:

```rust
                    .push(crate::state::Continuation::AcknowledgeForced {
                        source: hit.code.clone(),
                    });
```

with:

```rust
                    .push(crate::state::Continuation::AcknowledgeForced {
                        candidate: hit.clone(),
                    });
```

**3c.** Rewrite `drive_acknowledge_forced` to read `candidate` and anchor the option:

```rust
pub(crate) fn drive_acknowledge_forced(cx: &mut Cx) -> EngineOutcome {
    use crate::engine::{ChoiceOption, InputRequest, OptionId, ResumeToken};
    let Some(crate::state::Continuation::AcknowledgeForced { candidate }) =
        cx.state.continuations.last()
    else {
        return EngineOutcome::Rejected {
            reason: "drive_acknowledge_forced: top frame is not AcknowledgeForced".into(),
        };
    };
    let name = forced_source_name(&candidate.code);
    let act = super::reaction_windows::current_act_code(cx.state);
    let anchor = super::reaction_windows::candidate_anchor(candidate, act.as_ref());
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            format!("Forced — {name}"),
            vec![ChoiceOption::new(OptionId(0), "Resolve", anchor)],
        ),
        resume_token: ResumeToken(0),
    }
}
```

(`candidate` and `current_act_code(cx.state)` are both shared borrows of `cx.state`; `act` is owned, so `act.as_ref()` outlives the call. `candidate_anchor` takes `&ResolutionCandidate`.)

**3d.** Migrate the two existing tests' frame construction. In both `acknowledge_forced_suspends_then_pops_on_pick` and `acknowledge_forced_rejects_non_pick_response`, replace:

```rust
        state.continuations.push(Continuation::AcknowledgeForced {
            source: CardCode("01113".into()),
        });
```

with:

```rust
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01113"),
                InvestigatorId(1),
                0,
                CandidateSource::Board,
            ),
        });
```

(`ResolutionCandidate`, `CandidateSource`, `InvestigatorId`, `CardCode` are all in scope via `use super::*`. `Board` with a non-act code → `Global`, which these tests don't assert — they check option count + resume, still valid.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p game-core --lib -- acknowledge_forced`
Expected: PASS — the new anchor test plus the two migrated tests.
Run: `cargo test -p game-core`
Expected: PASS (no resolve-path regression — `resume_acknowledge_forced` and the `mod.rs` `AcknowledgeForced { .. }` arms are untouched).

- [ ] **Step 5: Clippy + commit**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: clean.

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/forced_triggers.rs
git commit -m "engine: anchor single-hit forced-ack option to its source card (#553)"
```

---

### Task 3: Full gauntlet + PR

**Files:** none (verification + PR); `docs/phases/phase-7-the-gathering.md` note is the final commit at PR-ready time.

- [ ] **Step 1: Run the full 7-job gauntlet**

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green.

- [ ] **Step 2: Push + open the PR**

```bash
git push -u origin engine/forced-effect-anchor
gh pr create --fill
```

PR body: note the one unanchored forced path fixed, the shared-helper extraction, the serde non-issue, `Closes #553`.

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`
Fix any failure with a follow-up commit to the same branch.

- [ ] **Step 4: Phase-doc note (only once CI is green + ready to merge)**

Add a one-line entry to the interactivity section of `docs/phases/phase-7-the-gathering.md` recording the forced-effect anchor (#553, PR #N) as a follow-up to S5. Commit as the final commit.

- [ ] **Step 5: Merge only after explicit user approval**

```bash
gh pr merge <PR#> --squash --delete-branch
```

Confirm #553 auto-closed and `git pull` on `main`.

---

### Task 4: Anchor location-sourced forced effects to the map node (#553 follow-up, same PR)

**Design:** the "Follow-up: location-sourced forced anchor" section of the spec.

**Why:** a location's own forced ability (the Attic 01113 — "Forced – After you enter: take 1 horror") reaches the wire as `OptionTarget::Global` (resolvable only from the flat bar). A location has a `LocationId`, not a `CardInstanceId`; the `EnteredLocation` scan passes `push_matching(… None …)` → `CandidateSource::Board` → (code ≠ act) → `Global`. `CandidateSource` carries no `LocationId`, so there is no path to `OptionTarget::Location(id)` (which the map node already renders, S1).

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`CandidateSource::Location` variant + `instance()` arm)
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs` (widen `push_matching`'s `source` param; the `EnteredLocation` site passes `Location`; the other 13 sites pass `Board`/`InPlay`; add a game-core unit test)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`candidate_anchor` + `build_resolution_options` label + `bump_usage_counter` arms; add a unit test)
- Modify: `crates/cards/tests/forced_acknowledge.rs` (extend the Attic test with the anchor assertion — the end-to-end red)

**Interfaces:**
- Changes: `CandidateSource` gains `Location(LocationId)`.
- Changes: `push_matching(… source: Option<CardInstanceId> …)` → `push_matching(… source: CandidateSource …)`.

**Approach note (widen vs. wrapper):** `push_matching`'s `source: Option<CardInstanceId>` is a two-case encoding (`Some`=InPlay, `None`=Board) the feature outgrows — a location is a third origin it cannot represent. Widen the parameter to `CandidateSource` (the internal `match` disappears; each call site states its origin) rather than add a single-use `push_matching`-with-location wrapper (Karpathy: no abstractions for single-use code). The 13 mechanical `None→Board`/`Some(x)→InPlay(x)` edits are trivially reviewable and make every call site self-document its origin.

- [ ] **Step 1: Write the failing end-to-end test (real Attic registry)**

In `crates/cards/tests/forced_acknowledge.rs`, add `OptionTarget` to the `game_core::engine` import and, inside `attic_forced_acknowledges_before_horror_when_interactive`'s `AwaitingInput` arm (right after the `options.len()` assert), add:

```rust
            assert_eq!(
                request.options[0].target,
                OptionTarget::Location(LOC),
                "the forced-on-enter option anchors to the location on the map (#553), not the flat bar"
            );
```

Import line becomes:

```rust
use game_core::engine::{EngineOutcome, OptionTarget};
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cards --test forced_acknowledge attic_forced_acknowledges_before_horror_when_interactive`
Expected: FAIL — `left: Global, right: Location(LocationId(1))` (the Attic currently anchors `Global`).

- [ ] **Step 3: Add the `Location` variant + fix every match site**

**3a.** In `state/game_state.rs`, add the variant (after `InPlay`):

```rust
    /// A location's own ability (the Attic's forced horror). Anchors to the
    /// location on the map. Locations have a [`LocationId`], not a
    /// [`CardInstanceId`], so this can't fold into `InPlay` (#553).
    Location(LocationId),
```

(`LocationId` is already in scope in `game_state.rs`; it is `Copy`, so `CandidateSource` keeps its `Copy` derive.)

**3b.** In `state/game_state.rs`, `CandidateSource::instance()` — a location has no instance id, so it groups with the `None` arm:

```rust
            CandidateSource::Board | CandidateSource::Hand | CandidateSource::Location(_) => None,
```

**3c.** In `reaction_windows.rs`, `candidate_anchor` — add before the `Board` arm:

```rust
        CandidateSource::Location(location_id) => OptionTarget::Location(location_id),
```

**3d.** In `reaction_windows.rs`, `build_resolution_options`'s label match — a location forced reads like any board reaction:

```rust
                CandidateSource::InPlay(_) | CandidateSource::Board | CandidateSource::Location(_) => {
                    format!("Resolve reaction: {}", cand.code)
                }
```

**3e.** In `reaction_windows.rs`, `bump_usage_counter` — a location has no per-instance usage counter (and location forced abilities carry no usage limit), so it joins the unreachable arm; update the message:

```rust
        CandidateSource::Board | CandidateSource::Hand | CandidateSource::Location(_) => unreachable!(
            "bump_usage_counter: a usage-limited candidate must be an in-play instance \
             (board / hand / location candidates carry no per-instance usage limits); candidate {trigger:?}"
        ),
```

**3f.** In `forced_triggers.rs`, widen `push_matching`'s signature and drop the internal `match`:

```rust
fn push_matching(
    reg: &card_registry::CardRegistry,
    code: &CardCode,
    controller: InvestigatorId,
    source: CandidateSource,
    out: &mut Vec<ResolutionCandidate>,
    bucket: EventTiming,
    want: impl Fn(&EventPattern) -> bool,
) {
```

and the `out.push` becomes (drop the `match source { … }`, keep the comment trimmed):

```rust
                out.push(ResolutionCandidate {
                    code: code.clone(),
                    controller,
                    ability_index: u8::try_from(idx)
                        .expect("ability_index fits u8 — abilities vecs are tiny"),
                    // Origin set by the caller: an in-play/threat instance, a
                    // scenario board card, or a location's own forced ability.
                    source,
                });
```

**3g.** In `forced_triggers.rs`, update all 14 `push_matching` call sites' `source` arg:
- The `EnteredLocation` site (`&loc.code`) — **the fix**: `CandidateSource::Location(*location)`.
- The 7 sites currently passing `None` (act/agenda by code: PhaseEnded ×2, ActAdvanced, AgendaAdvanced, EnemyDefeated act/agenda ×3): `CandidateSource::Board`.
- The 6 sites currently passing `Some(card.instance_id)` / `Some(att.instance_id)` (RoundEnded, EndOfTurn, SkillTestResolved, tested_location attachment, GameEnd, LeftLocation attachment): `CandidateSource::InPlay(<the id>)`.

- [ ] **Step 4: Run to verify the end-to-end test passes**

Run: `cargo build -p game-core` — expected: clean (every match site handled).
Run: `cargo test -p cards --test forced_acknowledge` — expected: PASS (all four; the Attic now anchors `Location`).

- [ ] **Step 5: Add game-core unit tests (regression guards)**

In `reaction_windows.rs` `resolution_option_anchor_tests`, extend `candidate_anchor_maps_each_source` with a location case (or add a focused test):

```rust
        use crate::state::LocationId;
        let loc = ResolutionCandidate::new(
            CardCode::new("01113"),
            InvestigatorId(1),
            0,
            CandidateSource::Location(LocationId(7)),
        );
        assert_eq!(
            candidate_anchor(&loc, Some(&act)),
            OptionTarget::Location(LocationId(7))
        );
```

In `forced_triggers.rs` `mod tests`, mirror the shipped in-play anchor test with a location source:

```rust
    #[test]
    fn acknowledge_forced_anchors_a_location_source_to_its_map_node() {
        use crate::engine::OptionTarget;
        use crate::state::{Continuation, LocationId};
        use crate::test_support::GameStateBuilder;

        let mut state = GameStateBuilder::default().build();
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01113"),
                InvestigatorId(1),
                0,
                CandidateSource::Location(LocationId(3)),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match super::drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1);
                assert_eq!(
                    request.options[0].target,
                    OptionTarget::Location(LocationId(3)),
                );
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }
```

Run: `cargo test -p game-core --lib -- candidate_anchor_maps_each_source acknowledge_forced_anchors_a_location`
Expected: PASS.

- [ ] **Step 6: Clippy + commit**

Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings` — expected: clean.

```bash
git add crates/game-core/src/state/game_state.rs \
        crates/game-core/src/engine/dispatch/forced_triggers.rs \
        crates/game-core/src/engine/dispatch/reaction_windows.rs \
        crates/cards/tests/forced_acknowledge.rs
git commit -m "engine: anchor location-sourced forced effects to the map node (#553)"
```

---

## Self-review

**Spec coverage:**
- Shared `candidate_anchor` extraction → **Task 1** ✅
- `current_act_code` `pub(super)` → **Task 1** ✅
- Frame carries `ResolutionCandidate` → **Task 2** ✅
- `drive_acknowledge_forced` anchors the option → **Task 2** ✅
- Display-only (resume untouched) → asserted by the unchanged `acknowledge_forced_suspends_then_pops_on_pick` / `_rejects_non_pick_response` → **Task 2** ✅
- Web: none (in-play glow rides `InPlayCardView`) → no task, per spec ✅
- Serde non-issue → Global Constraints (required field) ✅

**Placeholder scan:** none — every code step carries full code.

**Type consistency:** `candidate_anchor(&ResolutionCandidate, Option<&CardCode>) -> OptionTarget` is defined in Task 1 and called in Task 2 with matching types; `AcknowledgeForced { candidate: ResolutionCandidate }` is defined in Task 2 (3a) and constructed/read consistently (3b/3c/3d).
