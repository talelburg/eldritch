# Slice B-ii — Coordinators + Round-End Remodel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the round-end `when → at` ordering structural via a bucket-iterating coordinator, remodel act 01109's advancement from the `Act.round_end_advance` framework field into a registry `When`-reaction ability (deleting the bespoke `ActRoundEnd` machinery), and add per-cell eligibility re-scan with a regression test.

**Architecture:** The `EventTiming` bucket (B-i) is now first-class. The forced/reaction scans become bucket-aware. Round-end (the only multi-bucket moment) gets a `Continuation::EmitEvent` cursor that iterates `When` (act-advance reaction) → `At` (agenda doom forced) → `After` (empty), re-scanning each cell, then runs teardown. Per-bucket forced→reaction reuses the **existing** forced-run + reaction-window frames (a separate drive-dispatched `TimingPoint` frame is Slice C). Single-bucket events stay on the unchanged `emit_event` path. Behaviour-preserving except the §G re-scan.

**Tech Stack:** Rust workspace; `game-core` engine, `cards` registry, `scenarios` content. `Effect::Native { tag }` for the group clue-spend; existing reaction-window `PickSingle`/`Skip` for the player choice.

**Parent spec:** [`2026-06-23-emitevent-frame-slice-b-coordinators-design.md`](../specs/2026-06-23-emitevent-frame-slice-b-coordinators-design.md). Issue: [#434](https://github.com/talelburg/eldritch/issues/434) (Slice B-ii).

## Global Constraints

- **Behaviour-preserving except the §G re-scan.** Single-bucket events: byte-identical event log. Round-end: same group spend, same `when→at` order, same teardown. The only intended new behaviour is per-cell re-scan (a `when`-cell that changes an `at`-cell's eligibility), exercised solely by the synthetic §G fixture.
- **CI gauntlet before push** (warnings-as-errors), from repo root: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Never hand-edit `crates/cards/src/generated/`.**
- **Look up card text before quoting it** — act 01109 (The Barrier) and agenda 01107 (They're Getting Out) via ArkhamDB `https://arkhamdb.com/card/01109` / `01107` (+ FAQ), cross-checked against `data/arkhamdb-snapshot/`.
- Branch: `engine/timing-coordinators` (current).

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/game-core/src/engine/dispatch/forced_triggers.rs` | forced scan | `collect_forced_hits` / `push_matching` / `fire_forced_triggers` gain a `bucket: EventTiming` param |
| `crates/cards/src/impls/the_barrier.rs` | act 01109 | add `abilities()` (a `When` RoundEnded reaction → native) + native handler + tests |
| `crates/cards/src/impls/mod.rs` | registry wiring | wire 01109 into `abilities_for` + `native_effect_for` |
| `crates/cards/src/impls/theyre_getting_out.rs` | agenda 01107 doom | re-tag RoundEnded forced `After → At` |
| `crates/cards/src/impls/dissonant_voices.rs` | 01165 discard | re-tag RoundEnded forced `After → At` |
| `crates/game-core/src/state/game_state.rs` | frames/structs | add `Continuation::EmitEvent`; delete `ActRoundEnd`, `ActRoundEndPending`, `Act.round_end_advance`, `RoundEndAdvance` |
| `crates/game-core/src/engine/dispatch/emit.rs` | round-end emit | `reaction_window(RoundEnded, When)` mapping; round-end pushes `EmitEvent` |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | scan/affordability | `When`-RoundEnded act-advance candidate + affordability gate |
| `crates/game-core/src/engine/dispatch/phases.rs` | round-end flow | rewrite `upkeep_phase_end` → push `EmitEvent`; delete `round_end_advance_window` / `resume_act_round_end_advance` / `upkeep_round_end_at_and_after` |
| `crates/game-core/src/engine/dispatch/mod.rs` | input routing | delete the `ActRoundEnd` resume arm |
| `crates/scenarios/src/the_gathering.rs` | setup | drop `round_end_advance: Some(...)` |
| `crates/scenarios/src/test_fixtures/synthetic.rs` | §G fixture | synthetic act/agenda exercising `when`-changes-`at` |

---

### Task 1: Parameterize the forced scan by bucket

Today `push_matching` hardcodes `timing == EventTiming::After`. Generalize to a passed bucket so the round-end `At` cell can scan `At`-timed forced abilities. All current callers pass `After` — behaviour-preserving.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/forced_triggers.rs`
- Modify callers: `crates/game-core/src/engine/dispatch/emit.rs` (the `fire_forced_triggers` / `collect_forced_hits` call sites)

**Interfaces:**
- Produces: `collect_forced_hits(state, point, bucket: EventTiming) -> Vec<ResolutionCandidate>`; `fire_forced_triggers(cx, point, bucket: EventTiming) -> EngineOutcome`; `push_matching(.., want, bucket: EventTiming)`.

- [ ] **Step 1: Add the `bucket` parameter to `push_matching`** and filter on it. In `forced_triggers.rs`, change the signature and the filter:

```rust
fn push_matching(
    reg: &card_registry::CardRegistry,
    code: &CardCode,
    controller: InvestigatorId,
    source: Option<CardInstanceId>,
    out: &mut Vec<ResolutionCandidate>,
    want: impl Fn(&EventPattern) -> bool,
    bucket: EventTiming,
) {
    let Some(abilities) = (reg.abilities_for)(code) else {
        return;
    };
    for (idx, ability) in abilities.iter().enumerate() {
        if let Trigger::OnEvent { pattern, timing, .. } = &ability.trigger {
            if *timing == bucket && want(pattern) {
                out.push(ResolutionCandidate {
                    code: code.clone(),
                    controller,
                    ability_index: u8::try_from(idx)
                        .expect("ability_index fits u8 — abilities vecs are tiny"),
                    source: match source {
                        Some(id) => CandidateSource::InPlay(id),
                        None => CandidateSource::Board,
                    },
                });
            }
        }
    }
}
```

- [ ] **Step 2: Thread `bucket` through `collect_forced_hits` and `fire_forced_triggers`.** Add `bucket: EventTiming` to both signatures; pass it to every `push_matching` call in `collect_forced_hits`; pass it from `fire_forced_triggers` to `collect_forced_hits`.

- [ ] **Step 3: Update callers to pass `EventTiming::After`** (behaviour-preserving). In `emit.rs`, `collect_forced_hits(cx.state, &point)` → `collect_forced_hits(cx.state, &point, EventTiming::After)`, and `fire_forced_triggers(cx, &point)` → `fire_forced_triggers(cx, &point, EventTiming::After)`. Grep `collect_forced_hits\|fire_forced_triggers` across `crates/game-core` for any other caller and pass `After`.

- [ ] **Step 4: Build + run forced-trigger tests.**

Run: `cargo build --all --all-features` → clean.
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --test forced_triggers` → PASS (unchanged: all current forced are `After`).

- [ ] **Step 5: Commit.**

```bash
git add -A && git commit -m "$(cat <<'EOF'
engine: parameterize the forced scan by EventTiming bucket (Slice B-ii task 1)

push_matching / collect_forced_hits / fire_forced_triggers take a bucket and
filter timing == bucket; all callers pass After (behaviour-preserving). Lets
the round-end At cell scan At-timed forced abilities in a later task.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ
EOF
)"
```

---

### Task 2: Act 01109 dormant `When`-reaction ability + native handler

Add 01109's advancement as a registry ability + native group-spend handler, **dormant** — the round-end flow still uses the framework `round_end_advance` window, and nothing scans `When`-RoundEnded reactions yet, so this ability does nothing until Task 3 wires it in. This keeps Task 2 independently green.

**Files:**
- Modify: `crates/cards/src/impls/the_barrier.rs` (01109)
- Modify: `crates/cards/src/impls/mod.rs` (registry wiring)

**Interfaces:**
- Produces: `the_barrier::abilities()` returning a single `reaction_on_event(EventPattern::RoundEnded, EventTiming::When, native(ACT_ROUND_END_ADVANCE))`; `the_barrier::native_effect_for(tag)` exposing the handler; both wired into `impls::abilities_for` / `impls::native_effect_for`.

- [ ] **Step 1: Confirm the card text** with WebFetch `https://arkhamdb.com/card/01109` (+ FAQ), cross-checked against `data/arkhamdb-snapshot/`. Quote the round-end advance clause verbatim in the impl's module doc.

- [ ] **Step 2: Add `abilities()` + the native tag + handler to `the_barrier.rs`.** The handler reuses the group-spend mechanics. (The Hallway contributor location `01112` is printed on the card — carried as a const here.)

```rust
use card_dsl::dsl::{native, reaction_on_event, Ability, EventPattern, EventTiming};

pub(crate) const ACT_ROUND_END_ADVANCE: &str = "act_round_end_advance";
const CONTRIBUTOR_LOCATION: &str = "01112"; // the Hallway (printed on 01109)

/// "When the round ends, investigators in the Hallway may, as a group, spend
/// [clue_threshold] clues to advance." Modeled as a When-timed RoundEnded
/// reaction: the round-end When window offers this as a single Board candidate
/// (PickSingle = advance, Skip = decline). Affordability gates the candidate in
/// the scan (reaction_windows.rs), so firing always succeeds.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::RoundEnded,
        EventTiming::When,
        native(ACT_ROUND_END_ADVANCE),
    )]
}

/// Native: spend the act's clue_threshold from contributor-location
/// investigators, then advance the current act. Synchronous — the player choice
/// was the window PickSingle. Delegates to the engine's group-spend entry
/// (Step 0), passing the printed contributor location.
fn advance_via_clue_spend(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    game_core::engine::round_end_advance(cx, CONTRIBUTOR_LOCATION)
}

pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        ACT_ROUND_END_ADVANCE => Some(advance_via_clue_spend as NativeEffectFn),
        _ => None,
    }
}
```

**Glue (decided): expose a `pub` engine entry, the `cards` handler delegates.** The
`act_agenda::{investigators_at, clues_held, spend_clues_from, advance_act}` +
`location_id_by_code` helpers are `pub(crate)` to `game-core`; the `cards` native handler
already takes `&mut Cx`. So add **Step 0** below — a `pub fn` wrapping the generic mechanics,
parameterized by the contributor location (the only card-specific datum). This keeps 01109's
*ability declaration* in `cards` (the asymmetry fix) while the *mechanics* stay engine-side,
matching how the existing `theyre_getting_out` natives manipulate `cx.state` via engine APIs.
The chosen alternative (handling the tag inside the engine evaluator) was rejected: native
dispatch routes through `cards`' `native_effect_for`, so the registration must live in `cards`.

- [ ] **Step 0 (engine entry): add `pub fn round_end_advance`.** In
  `crates/game-core/src/engine/dispatch/act_agenda.rs`, add (re-exported as
  `game_core::engine::round_end_advance`):

```rust
/// Group clue-spend round-end act advance (generic; 01109's mechanics). Resolves
/// `contributor_location_code` to its in-play location, sums clues held by
/// investigators there, and — if they can afford the current act's
/// `clue_threshold` — spends it and advances the act. Affordability is gated in
/// the reaction scan, so the unaffordable path is a defensive no-op reject.
pub fn round_end_advance(cx: &mut Cx, contributor_location_code: &str) -> EngineOutcome {
    let Some(act) = cx.state.act_deck.get(cx.state.act_index) else {
        return EngineOutcome::Rejected { reason: "round_end_advance: no current act".into() };
    };
    let threshold = act.clue_threshold;
    let Some(loc) = crate::engine::location_id_by_code(cx.state, contributor_location_code) else {
        return EngineOutcome::Rejected {
            reason: "round_end_advance: contributor location not in play".into(),
        };
    };
    let contributors = investigators_at(cx.state, loc);
    if clues_held(cx.state, &contributors) < u32::from(threshold) {
        return EngineOutcome::Rejected {
            reason: "round_end_advance: contributors no longer hold enough clues".into(),
        };
    }
    spend_clues_from(cx.state, &contributors, threshold);
    advance_act(cx);
    EngineOutcome::Done
}
```

  Add the re-export to `crates/game-core/src/engine/mod.rs` (alongside `location_id_by_code`).

- [ ] **Step 3: Wire 01109 into the registry.** In `crates/cards/src/impls/mod.rs`, add `the_barrier::CODE => Some(the_barrier::abilities())` to `abilities_for`, and add `the_barrier::native_effect_for(tag)` into the `native_effect_for` chain (follow the existing `theyre_getting_out` pattern).

- [ ] **Step 4: Card test (dormant ability is well-formed).** In `the_barrier.rs` tests, assert `abilities()` is one `OnEvent { RoundEnded, When, Reaction }` whose effect is `native(ACT_ROUND_END_ADVANCE)`, and that `native_effect_for(ACT_ROUND_END_ADVANCE)` is `Some`.

- [ ] **Step 5: Build + test + commit.**

Run: `cargo build --all --all-features` (after resolving the Step-2 glue) → clean. `cargo test -p cards the_barrier` → PASS.

```bash
git add -A && git commit -m "engine: act 01109 When-reaction advance ability (dormant) (Slice B-ii task 2)

[body: dormant until Task 3 wires the round-end When window; native reuses the
group clue-spend; framework round_end_advance still active.]

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011oqjdoNhbcH4kay8J3FEoJ"
```

---

### Task 3: The round-end switch — `EmitEvent` coordinator + When window + doom→At

The atomic migration: round-end stops using the framework act-window and the imperative `at_and_after` chain, and instead drives a bucket-iterating `EmitEvent` coordinator (When act-reaction → At doom → After → teardown). This task must land as one green commit (the framework window and the ability can't both advance the act).

**Files:** `game_state.rs` (add `Continuation::EmitEvent`), `emit.rs` (`reaction_window(RoundEnded, When)`, round-end emit), `reaction_windows.rs` (act-advance candidate + affordability), `phases.rs` (rewrite `upkeep_phase_end`), `theyre_getting_out.rs` + `dissonant_voices.rs` (doom→At), `the_gathering.rs` (drop `round_end_advance`).

**Interfaces:**
- Produces: `Continuation::EmitEvent { event: TimingEvent, bucket: EventTiming, continuation: ForcedContinuation }`; `emit::drive_emit_event(cx) -> EngineOutcome`; `WindowKind::RoundEndAdvance { … }` (or reuse) for the When act window.

- [ ] **Step 1: Add the `EmitEvent` coordinator frame.** In `game_state.rs`, add:

```rust
/// Round-end bucket cursor (Slice B-ii): iterates When → At → After for one
/// event, re-scanning eligibility at each cell, then runs `continuation`.
/// Driven imperatively this slice (drive-loop arm + the per-bucket TimingPoint
/// frame are Slice C). Only RoundEnded uses it — single-bucket events stay on
/// the plain `emit_event` path.
EmitEvent {
    event: crate::engine::TimingEvent,
    bucket: EventTiming,
    continuation: ForcedContinuation,
},
```

- [ ] **Step 2: Map the When round-end reaction window.** In `emit.rs`, make `reaction_window` bucket-aware for RoundEnded: `reaction_window(RoundEnded, When) → Some(WindowKind::RoundEndAdvance { … })`; `reaction_window(RoundEnded, At/After) → None`. Add the `WindowKind::RoundEndAdvance` variant (carries what `WindowOpened` needs — the contributor location / threshold for observability). (This variant is reworked by B-iii's WindowKind deletion; minimal here.)

- [ ] **Step 3: Scan the act-advance candidate with affordability.** In `reaction_windows.rs`, the `When`-RoundEnded scan offers 01109's reaction as a `Board` candidate **only if** the contributor-location group can afford `clue_threshold` (port `round_end_advance_window`'s affordability check into the scan eligibility). Unaffordable ⇒ no candidate ⇒ window auto-skips.

- [ ] **Step 4: Implement `drive_emit_event`.** The imperative coordinator: for `bucket`, re-scan + run the cell (When: open the act-advance reaction window via the existing reaction-window machinery; At: `fire_forced_triggers(.., EventTiming::At)` or the 2+ forced run; After: scan, empty). On a cell suspend, return `AwaitingInput`. On a cell complete, advance `bucket` (When→At→After), re-scanning. After `After`, pop the frame and run `continuation` (`UpkeepAfterRoundEnded` → teardown). Wire the reaction-window close (`close_reaction_window_at`) to re-enter `drive_emit_event` when an `EmitEvent` frame is beneath (mirrors the existing skill-test re-entry).

- [ ] **Step 5: Rewrite `upkeep_phase_end`** to push `EmitEvent { RoundEnded, When, UpkeepAfterRoundEnded }` and drive it, replacing the `round_end_advance_window` branch and the `upkeep_round_end_at_and_after` call. Keep the `PhaseEnded { Upkeep }` emit. `upkeep_round_end_teardown` stays (run by the coordinator's continuation).

- [ ] **Step 6: Re-tag the doom abilities `After → At`.** In `theyre_getting_out.rs` (01107 RoundEnded forced) and `dissonant_voices.rs` (01165), change `EventTiming::After` → `EventTiming::At`. Now the `At` cell's `fire_forced_triggers(.., At)` finds them.

- [ ] **Step 7: Drop the framework field.** In `the_gathering.rs`, remove `round_end_advance: Some(RoundEndAdvance { … })` from act 01109's `Act` (leave the field default until Task 4 deletes it, or delete inline if Task 4 is folded).

- [ ] **Step 8: Build + round-end regression suite.**

Run: `cargo build --all --all-features` → clean.
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --test act_round_end && cargo test -p cards act_advancement the_barrier && cargo test -p scenarios the_gathering` → PASS (same advance/decline behaviour, same `when→at` order).

- [ ] **Step 9: Commit** (`engine: drive round-end through the When→At EmitEvent coordinator (Slice B-ii task 3)`).

---

### Task 4: Delete the dead `ActRoundEnd` machinery

After Task 3 nothing uses the framework act-window path. Delete it.

**Files:** `game_state.rs`, `phases.rs`, `mod.rs`, `the_gathering.rs`.

- [ ] **Step 1: Delete the types.** Remove `Act.round_end_advance` field, `RoundEndAdvance` struct, `ActRoundEndPending` struct, `Continuation::ActRoundEnd` variant.
- [ ] **Step 2: Delete the functions.** Remove `round_end_advance_window`, `resume_act_round_end_advance`, `upkeep_round_end_at_and_after` from `phases.rs`.
- [ ] **Step 3: Delete the input-routing arm** `Some(Continuation::ActRoundEnd(_)) => …` in `mod.rs`.
- [ ] **Step 4: Fix all `Act { … }` constructors** (the_gathering.rs, synthetic fixtures, tests) to drop the removed field.
- [ ] **Step 5: Build (compiler confirms nothing references the deleted items) + full suite + commit.**

Run: `cargo build --all --all-features` → clean. `RUSTFLAGS="-D warnings" cargo test --all --all-features` → PASS.

---

### Task 5: §G per-cell re-scan regression test

The one new behaviour: a `when`-cell that changes whether an `at`-cell forced fires.

**Files:** `crates/scenarios/src/test_fixtures/synthetic.rs` (fixture) + a new engine/integration test.

- [ ] **Step 1: Build a synthetic act/agenda fixture** where the `When`-RoundEnded act advance, when taken, removes/satisfies the precondition an `At`-RoundEnded agenda forced keys on (e.g. the `At` forced is eligible only while the original act is current; advancing in the `When` cell makes it ineligible).
- [ ] **Step 2: Write the failing test** asserting: take the `When` advance (PickSingle) → the `At` forced does **not** fire (its eligibility was re-scanned after the `When` cell). Contrast with a control where skipping the `When` cell leaves the `At` forced firing.
- [ ] **Step 3: Run → confirm it passes** (the coordinator re-scans each cell). If it fails (stale eligibility), fix `drive_emit_event` to re-scan entering each cell.
- [ ] **Step 4: Commit.**

---

### Task 6: CI gauntlet + PR

- [ ] **Step 1: Full gauntlet** (six jobs). Fix any finding.
- [ ] **Step 2: Push** `git push -u origin engine/timing-coordinators`.
- [ ] **Step 3: Open PR** — title `engine: round-end When→At coordinator + act 01109 remodel (Slice B-ii)`. Body: design-decisions paragraph (bucket axis used; 01109 → When reaction ability reusing window Pick/Skip; affordability in scan; doom→At; ActRoundEnd deleted; per-cell re-scan; TimingPoint-frame + drive-loop dispatch deferred to Slice C). Reference the spec; "Part of #434 (Slice B-ii)."
- [ ] **Step 4: Watch CI**; fix failures on the branch.
- [ ] **Step 5: Phase-doc update deferred** to when all of Slice B lands (do NOT edit it here).

## Self-Review notes

- **Spec coverage:** bucket-aware scan (T1) ✓; 01109 → When reaction ability + native (T2) ✓; coordinator + doom→At + switch (T3) ✓; delete ActRoundEnd (T4) ✓; §G re-scan (T5) ✓. The general drive-dispatched `TimingPoint` frame is explicitly deferred to Slice C (noted in spec + T3).
- **Two open glue decisions flagged for review** (not placeholders — bounded choices): (a) T2 native↔engine glue (recommend a `pub` engine entry); (b) T3 `WindowKind::RoundEndAdvance` shape (minimal, reworked by B-iii). Everything else is concrete.
- **Atomicity:** T3 is one commit (framework window ↔ ability can't coexist). T1/T2 are dormant/behaviour-preserving and land first; T4 is pure deletion after T3; T5 adds the only new behaviour.
