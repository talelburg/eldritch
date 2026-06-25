# 2b — Open-turn gameplay via `ResolveInput(OptionId)` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route all open-turn gameplay through `ResolveInput(PickSingle(OptionId))` against an internal `TurnAction` enumeration; delete the eleven typed gameplay `PlayerAction` variants and the test-only `PerformSkillTest`, leaving `StartScenario` + `ResolveInput`.

**Architecture:** Additive-then-delete to keep the build green at every commit. First add the new path (`TurnAction`, `dispatch_turn_action`, `legal_actions → Vec<TurnAction>`, a `ResolveInput` arm for the open turn) *alongside* the still-working typed arms. Then migrate every test and the web client to the new path. Only in the final "flip" task does the open turn emit `AwaitingInput`, `awaits_input()` flip to `true`, and the typed variants get deleted.

**Tech Stack:** Rust (workspace: `game-core`, `cards`, `scenarios`, `server`, `web`/wasm+Leptos), serde, sqlx (server).

## Global Constraints

- **CI gauntlet (all must pass, warnings-as-errors):** `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Validate-first / mutate-second:** every handler checks all preconditions and returns `Rejected { reason }` with state+events unchanged before any mutation.
- **`apply()` and the action log stay token-free** — replay must reproduce state bit-for-bit. (Deterministic resume-token is #458, out of scope.)
- **No silent approximation / no speculative DSL.** Behaviour-preserving: rules unchanged.
- **`game_core::test_support` is unconditionally `pub`** (callable from every test crate).
- Commit scope prefix `engine:` (or `engine/ui:` where web is touched); end commit bodies with the `Co-Authored-By` / `Claude-Session` trailers.

**Spec:** `docs/superpowers/specs/2026-06-25-2b-optionid-routing-design.md`. **Issue:** #447. **Follow-ups (out of scope):** #458, #459.

---

## File structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/game-core/src/engine/enumerate.rs` | Define `pub enum TurnAction` + `label()`; `legal_actions(state) -> Vec<TurnAction>` | 1 |
| `crates/game-core/src/engine/dispatch/mod.rs` | `dispatch_turn_action`; `resolve_input` open-turn arm; (flip) delete typed arms | 1, 7 |
| `crates/game-core/src/engine/dispatch/phases.rs` | (flip) `drive` open-turn `AwaitingInput` emission + `turn_menu` | 7 |
| `crates/game-core/src/state/game_state.rs` | (flip) `Continuation::awaits_input` for `InvestigatorTurn` | 7 |
| `crates/game-core/src/action.rs` | (flip) delete 11 gameplay variants + `PerformSkillTest` | 6, 7 |
| `crates/game-core/src/test_support/resolver.rs` | `take_turn_action` free fn + `TestSession::take`; `perform_skill_test` helper | 2, 6 |
| `crates/web/src/input.rs` | `PickSingle` option-list renderer in `AwaitingInputView` | 3 |
| `crates/web/src/controls.rs` | (flip) remove bespoke open-turn controls; keep `StartScenario` | 7 |
| `crates/web/src/legality.rs` | (flip) simplify `ActionControl` set | 7 |
| test files (game-core + 4 test crates) | migrate construction → helper; stop asserting open-turn `Done` | 4, 5, 6 |

---

## Task 1: Internal `TurnAction` enumeration + dispatch + open-turn `ResolveInput` arm (additive)

Adds the new path without changing observable behaviour except that `ResolveInput(PickSingle(OptionId))` at the open turn now dispatches (was: rejected). Typed `PlayerAction` gameplay variants still work unchanged.

**Files:**
- Modify: `crates/game-core/src/engine/enumerate.rs` (define `TurnAction`; retype `legal_actions` + `push_*` helpers; update tests)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (add `dispatch_turn_action`; rewrite the `InvestigatorTurn` arm of `resolve_input`)
- Modify: `crates/game-core/src/engine/mod.rs` (re-export `TurnAction` if `PlayerAction` is re-exported there)

**Interfaces:**
- Produces:
  - `pub enum TurnAction { EndTurn, Move{investigator,destination}, Investigate{investigator}, Resource{investigator}, Draw{investigator}, Fight{investigator,enemy}, Evade{investigator,enemy}, Engage{investigator,enemy}, PlayCard{investigator,hand_index}, ActivateAbility{investigator,instance_id,ability_index}, AdvanceAct{investigator} }` (derives `Debug, Clone, PartialEq, Eq`; **no serde**)
  - `impl TurnAction { pub fn label(&self, state: &GameState) -> String }`
  - `pub fn legal_actions(state: &GameState) -> Vec<TurnAction>`
  - `pub(crate) fn dispatch_turn_action(cx: &mut Cx, action: &TurnAction) -> EngineOutcome`
- Consumes (existing handlers, signatures unchanged): `actions::{move_action, investigate, resource_action, engage, fight, evade}`, `cards::{draw, play_card}`, `abilities::activate_ability`, `act_agenda::advance_act_action`, `phases::end_turn`.

- [ ] **Step 1: Write the failing test** — open-turn `ResolveInput(OptionId)` dispatches the enumerated action. Add to `enumerate.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn resolve_input_optionid_dispatches_enumerated_turn_action() {
    // EndTurn is always OptionId of its position in legal_actions; submitting it
    // via ResolveInput must dispatch (not reject) even while the open turn still
    // idles Done (pre-flip).
    let state = open_turn_state();
    let actions = legal_actions(&state);
    let idx = actions.iter().position(|a| *a == TurnAction::EndTurn).expect("EndTurn offered");
    let result = crate::apply(
        state,
        crate::Action::Player(crate::action::PlayerAction::ResolveInput {
            response: crate::action::InputResponse::PickSingle(crate::engine::OptionId(idx as u32)),
        }),
    );
    assert!(
        !matches!(result.outcome, crate::EngineOutcome::Rejected { .. }),
        "open-turn OptionId dispatch rejected: {:?}", result.outcome
    );
}
```

- [ ] **Step 2: Run it, verify it fails**

Run: `cargo test -p game-core resolve_input_optionid_dispatches_enumerated_turn_action`
Expected: FAIL — `TurnAction` undefined (compile error), or (once defined) the `InvestigatorTurn` arm still rejects.

- [ ] **Step 3: Define `TurnAction` + `label` + retype `legal_actions`.** In `enumerate.rs`, add the enum (fields exactly as the current `PlayerAction` gameplay variants — copy field names/types from `action.rs`). Change `legal_actions` and every `push_*` helper to push `TurnAction::X` instead of `PlayerAction::X`. Add:

```rust
impl TurnAction {
    /// Plain human-readable menu label. Rich/structured rendering is #205.
    #[must_use]
    pub fn label(&self, state: &GameState) -> String {
        use TurnAction::*;
        let loc_name = |id: LocationId| state.locations.get(&id).map_or_else(|| format!("loc {}", id.0), |l| l.name.clone());
        let enemy_name = |id: EnemyId| state.enemies.get(&id).map_or_else(|| format!("enemy {}", id.0), |e| e.name.clone());
        match self {
            EndTurn => "End turn".into(),
            Move { destination, .. } => format!("Move to {}", loc_name(*destination)),
            Investigate { .. } => "Investigate".into(),
            Resource { .. } => "Gain resource".into(),
            Draw { .. } => "Draw".into(),
            Fight { enemy, .. } => format!("Fight {}", enemy_name(*enemy)),
            Evade { enemy, .. } => format!("Evade {}", enemy_name(*enemy)),
            Engage { enemy, .. } => format!("Engage {}", enemy_name(*enemy)),
            PlayCard { investigator, hand_index } => {
                let code = state.investigators.get(investigator)
                    .and_then(|inv| inv.hand.get(*hand_index as usize))
                    .map_or_else(|| format!("card {hand_index}"), std::string::ToString::to_string);
                format!("Play {code}")
            }
            ActivateAbility { ability_index, .. } => format!("Activate ability {ability_index}"),
            AdvanceAct { .. } => "Advance act".into(),
        }
    }
}
```

- [ ] **Step 4: Add `dispatch_turn_action` in `dispatch/mod.rs`** — the gameplay match lifted from `apply_player_action`, dispatching to the same handlers (do NOT remove the typed arms from `apply_player_action` yet):

```rust
/// Dispatch one enumerated open-turn action (the internal id→action map target).
/// The same handlers `apply_player_action`'s typed arms call; behaviour-identical.
pub(crate) fn dispatch_turn_action(cx: &mut Cx, action: &crate::engine::enumerate::TurnAction) -> EngineOutcome {
    use crate::engine::enumerate::TurnAction;
    match action {
        TurnAction::EndTurn => phases::end_turn(cx),
        TurnAction::Move { investigator, destination } => actions::move_action(cx, *investigator, *destination),
        TurnAction::Investigate { investigator } => actions::investigate(cx, *investigator),
        TurnAction::Resource { investigator } => actions::resource_action(cx, *investigator),
        TurnAction::Draw { investigator } => cards::draw(cx, *investigator),
        TurnAction::Fight { investigator, enemy } => actions::fight(cx, *investigator, *enemy),
        TurnAction::Evade { investigator, enemy } => actions::evade(cx, *investigator, *enemy),
        TurnAction::Engage { investigator, enemy } => actions::engage(cx, *investigator, *enemy),
        TurnAction::PlayCard { investigator, hand_index } => cards::play_card(cx, *investigator, *hand_index),
        TurnAction::ActivateAbility { investigator, instance_id, ability_index } =>
            abilities::activate_ability(cx, *investigator, *instance_id, *ability_index),
        TurnAction::AdvanceAct { investigator } => act_agenda::advance_act_action(cx, *investigator),
    }
}
```

- [ ] **Step 5: Rewrite the `InvestigatorTurn` arm of `resolve_input`** (`dispatch/mod.rs:569`). Replace the reject body with OptionId resolution. Note: this arm is reached because `ResolveInput` is always allowed past the pending-prompt gate, and routing is on the top frame; pre-flip the frame is `InvestigatorTurn { ending: false }`.

```rust
Some(Continuation::InvestigatorTurn { ending: false, .. }) => {
    let crate::action::InputResponse::PickSingle(opt) = response else {
        return EngineOutcome::Rejected {
            reason: "ResolveInput: the open turn expects PickSingle(OptionId)".into(),
        };
    };
    let actions = crate::engine::enumerate::legal_actions(&cx.state);
    let Some(action) = actions.get(opt.0 as usize).cloned() else {
        return EngineOutcome::Rejected {
            reason: format!("ResolveInput: open-turn OptionId({}) out of range (0..{})", opt.0, actions.len()).into(),
        };
    };
    dispatch_turn_action(cx, &action)
}
Some(Continuation::InvestigatorTurn { .. }) => EngineOutcome::Rejected {
    reason: "ResolveInput: no input prompt is outstanding (transient rotation frame)".into(),
},
```

- [ ] **Step 6: Update `enumerate.rs` tests to `TurnAction`.** All existing assertions `legal_actions(&state).contains(&PlayerAction::X{..})` → `TurnAction::X{..}`. In `every_enumerated_action_is_accepted_by_its_handler`, replace the `apply(state, Action::Player(action))` loop with the OptionId round-trip (the truest cross-check):

```rust
let actions = legal_actions(&state);
for (i, action) in actions.iter().enumerate() {
    let result = crate::apply(
        state.clone(),
        crate::Action::Player(crate::action::PlayerAction::ResolveInput {
            response: crate::action::InputResponse::PickSingle(crate::engine::OptionId(i as u32)),
        }),
    );
    assert!(
        !matches!(result.outcome, crate::EngineOutcome::Rejected { .. }),
        "enumerated {action:?} (OptionId {i}) rejected: {:?}", result.outcome
    );
}
```

- [ ] **Step 7: Run the new + updated tests, verify pass**

Run: `cargo test -p game-core --lib enumerate`
Expected: PASS (including `resolve_input_optionid_dispatches_enumerated_turn_action`, the retyped order tests, and the cross-check).

- [ ] **Step 8: Full game-core lib gauntlet (typed path still works)**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core` then `cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: PASS — typed `PlayerAction` gameplay actions still dispatch via `apply_player_action`'s untouched arms; only addition is the open-turn OptionId path.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/enumerate.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/mod.rs
git commit -m "engine: internal TurnAction enumeration + open-turn OptionId dispatch (additive, #447)"
```

---

## Task 2: `test_support` semantic helper (`take_turn_action`)

A free function + fluent method letting tests express "take this action" and have the OptionId computed from the live enumeration. Works pre-flip (open turn idles `Done`) and post-flip (open turn emits `AwaitingInput`) — both accept `ResolveInput` at the open turn.

**Files:**
- Modify: `crates/game-core/src/test_support/resolver.rs`

**Interfaces:**
- Consumes: `enumerate::{legal_actions, TurnAction}` (Task 1); `apply`.
- Produces:
  - `pub fn take_turn_action(state: GameState, action: TurnAction) -> ApplyResult`
  - `impl TestSession { pub fn take(self, action: TurnAction) -> Self }`

- [ ] **Step 1: Write the failing test** (in `resolver.rs` tests):

```rust
#[test]
fn take_turn_action_resolves_end_turn_via_optionid() {
    use crate::engine::enumerate::TurnAction;
    let state = /* an open-turn single-investigator state; reuse a builder mirroring enumerate::tests::open_turn_state */
        crate::test_support::GameStateBuilder::default()
            .with_investigator(crate::test_support::test_investigator(1))
            .with_phase(crate::state::Phase::Investigation)
            .with_active_investigator(crate::state::InvestigatorId(1))
            .with_turn_order([crate::state::InvestigatorId(1)])
            .with_chaos_bag(crate::state::ChaosBag::new([crate::state::ChaosToken::Numeric(0)]))
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(crate::state::InvestigatorId(1))
            .build();
    let result = take_turn_action(state, TurnAction::EndTurn);
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }), "{:?}", result.outcome);
}
```

- [ ] **Step 2: Run it, verify it fails**

Run: `cargo test -p game-core take_turn_action_resolves_end_turn_via_optionid`
Expected: FAIL — `take_turn_action` undefined.

- [ ] **Step 3: Implement the helper** (in `resolver.rs`):

```rust
/// Drive one open-turn action by enumerating the legal actions, finding the
/// `OptionId` whose `TurnAction` equals `action`, and submitting it as
/// `ResolveInput(PickSingle(..))`. Panics if `action` is not currently legal
/// (a test-authoring bug). Returns the raw `ApplyResult` — assert on the
/// resulting **state/events**, not on `outcome == Done` (post-flip the outcome
/// is the next open-turn menu's `AwaitingInput`).
#[must_use]
pub fn take_turn_action(state: GameState, action: crate::engine::enumerate::TurnAction) -> ApplyResult {
    let actions = crate::engine::enumerate::legal_actions(&state);
    let idx = actions.iter().position(|a| *a == action).unwrap_or_else(|| {
        panic!("take_turn_action: {action:?} is not legal; offered: {actions:?}")
    });
    apply(state, Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(crate::engine::OptionId(idx as u32)),
    }))
}
```

- [ ] **Step 4: Add the fluent `TestSession::take`** (next to `TestSession::apply`):

```rust
/// Fluent open-turn action: see [`take_turn_action`]. Threads the resulting
/// state; drains any `AwaitingInput` the action itself opens via the session's
/// resolver script, exactly like [`TestSession::apply`].
#[must_use]
pub fn take(self, action: crate::engine::enumerate::TurnAction) -> Self {
    let idx = crate::engine::enumerate::legal_actions(&self.state)
        .iter().position(|a| *a == action)
        .unwrap_or_else(|| panic!("TestSession::take: {action:?} not legal"));
    self.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(crate::engine::OptionId(idx as u32)),
    }))
}
```

(Match `TestSession`'s field access pattern — read how `apply` reads `self.state` at `resolver.rs:453`.)

- [ ] **Step 5: Run the helper tests, verify pass**

Run: `cargo test -p game-core take_turn_action`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/test_support/resolver.rs
git commit -m "engine: test_support::take_turn_action open-turn OptionId helper (#447)"
```

---

## Task 3: Web — additive `PickSingle` option-list renderer

Add rendering for `AwaitingInput` requests carrying `options` (a `PickSingle` menu) to `AwaitingInputView`. Additive — `controls.rs` is untouched here; the open-turn menu has no options to render until the flip (Task 7), but the renderer is correct for any `PickSingle`-options prompt.

**Files:**
- Modify: `crates/web/src/input.rs`

**Interfaces:**
- Consumes: `EngineOutcome::AwaitingInput { request: InputRequest { options, prompt } }`, `OptionId`, `InputResponse::PickSingle`, `protocol::ClientMessage::Submit`.

- [ ] **Step 1: Read the current `AwaitingInputView`** (`crates/web/src/input.rs`) to match its store/`OutboundTx`/view idioms (it currently renders only `PickMultiple`).

- [ ] **Step 2: Write the failing wasm test** (in `crates/web/tests/controls.rs` or a new `crates/web/tests/awaiting_input.rs`, following the existing `controls.rs` test harness): construct a store state whose `outcome` is `AwaitingInput` with `request.options = [ChoiceOption{id:OptionId(0),label:"End turn"}, ChoiceOption{id:OptionId(1),label:"Investigate"}]`, render `AwaitingInputView`, assert two option buttons render with those labels. (Mirror the existing render-assertion pattern in `web/tests/controls.rs`.)

- [ ] **Step 3: Run it, verify it fails**

Run: `wasm-pack test --headless --firefox crates/web -- awaiting_input`
Expected: FAIL — no option buttons rendered.

- [ ] **Step 4: Implement the option-list branch.** When `request.options` is non-empty, render one button per `ChoiceOption`; on click send `ClientMessage::Submit { action: PlayerAction::ResolveInput { response: InputResponse::PickSingle(opt.id) } }`. Keep the existing `PickMultiple` (commit/mulligan) branch. Sketch:

```rust
// inside AwaitingInputView, when an AwaitingInput is live:
if !request.options.is_empty() {
    let buttons = request.options.iter().cloned().map(|opt| {
        let tx = tx.clone();
        view! {
            <button
                class="option"
                on:click=move |_| {
                    if let Some(tx) = tx.clone() {
                        let _ = tx.unbounded_send(ClientMessage::Submit {
                            action: PlayerAction::ResolveInput {
                                response: InputResponse::PickSingle(opt.id),
                            },
                        });
                    }
                }
            >{opt.label.clone()}</button>
        }
    }).collect::<Vec<_>>();
    return view! { <div class="option-list"><p class="prompt">{request.prompt.clone()}</p>{buttons}</div> }.into_any();
}
// else: existing PickMultiple/commit rendering
```

- [ ] **Step 5: Run the wasm test, verify pass**

Run: `wasm-pack test --headless --firefox crates/web -- awaiting_input`
Expected: PASS.

- [ ] **Step 6: Web build + wasm clippy**

Run: `cargo build -p web --target wasm32-unknown-unknown` then `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/input.rs crates/web/tests/
git commit -m "engine/ui: web AwaitingInput PickSingle option-list renderer (additive, #447)"
```

---

## Task 4: Migrate game-core internal tests to the helper

Mechanical. Convert every open-turn typed-action submission in game-core's own tests to `take_turn_action` / `TestSession::take`, and **stop asserting `outcome == Done` after a turn action** (assert the action's effect instead). Both paths still work, so this is green before the flip.

**Files (game-core internal tests):**
`crates/game-core/src/engine/mod.rs`, `crates/game-core/src/engine/dispatch/actions.rs`, `crates/game-core/src/engine/dispatch/cards.rs`, `crates/game-core/src/engine/dispatch/abilities.rs`, `crates/game-core/src/engine/dispatch/act_agenda.rs`, `crates/game-core/src/engine/dispatch/phases.rs`, `crates/game-core/src/engine/dispatch/mod.rs`, `crates/game-core/src/state/builder.rs`, `crates/game-core/src/state/game_state.rs`, `crates/game-core/src/test_support/assertions.rs`, `crates/game-core/src/test_support/resolver.rs` (doc-examples).

**Transformation pattern (apply throughout):**

```rust
// BEFORE
let result = apply(state, Action::Player(PlayerAction::Move { investigator: inv, destination: dest }));
assert!(matches!(result.outcome, EngineOutcome::Done));
assert_eq!(result.state.investigators[&inv].current_location, Some(dest));

// AFTER
let result = take_turn_action(state, TurnAction::Move { investigator: inv, destination: dest });
assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));  // not `== Done`
assert_eq!(result.state.investigators[&inv].current_location, Some(dest));
```

```rust
// Fluent BEFORE
GameStateBuilder::new().session().apply(Action::Player(PlayerAction::EndTurn)).run()
// Fluent AFTER
GameStateBuilder::new().session().take(TurnAction::EndTurn).run()
```

Rules for this task:
- A test that hand-builds a bare phase state and applied a typed gameplay action must first seat an `InvestigatorTurn` frame so the action is enumerable: add `.with_investigator_turn(<id>)` to the builder (see `state/builder.rs:299`). If it constructed state without the builder, push `Continuation::InvestigatorTurn { investigator, ending: false }`.
- A test whose subject *is* the rejection of a typed action sent at the wrong time: re-express via `take_turn_action` expecting the handler's `Rejected` (the legality is unchanged), or drop it if it only asserted "typed variant exists."
- Do **not** change `PerformSkillTest` sites here (Task 6).
- Leave `StartScenario` sites as-is.

- [ ] **Step 1: Migrate `engine/mod.rs` tests.** Apply the pattern. Run `cargo test -p game-core --lib engine::tests` — Expected: PASS.
- [ ] **Step 2: Commit** — `git commit -am "engine: migrate engine/mod.rs tests to take_turn_action (#447)"`
- [ ] **Step 3: Migrate `dispatch/actions.rs` + `dispatch/cards.rs` tests.** Run `cargo test -p game-core --lib dispatch::actions dispatch::cards` — Expected: PASS.
- [ ] **Step 4: Commit** — `git commit -am "engine: migrate dispatch actions/cards tests (#447)"`
- [ ] **Step 5: Migrate `dispatch/abilities.rs`, `dispatch/act_agenda.rs`, `dispatch/phases.rs`, `dispatch/mod.rs` tests.** Run `cargo test -p game-core --lib dispatch` — Expected: PASS.
- [ ] **Step 6: Commit** — `git commit -am "engine: migrate remaining dispatch-module tests (#447)"`
- [ ] **Step 7: Migrate `state/builder.rs`, `state/game_state.rs`, `test_support/assertions.rs`, `test_support/resolver.rs` doc-examples + tests.** Run `RUSTFLAGS="-D warnings" cargo test -p game-core` and `RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features` — Expected: PASS (doctests too).
- [ ] **Step 8: Commit** — `git commit -am "engine: migrate state/test_support tests + doctests (#447)"`
- [ ] **Step 9: Full game-core gauntlet.** Run `RUSTFLAGS="-D warnings" cargo test -p game-core` + `cargo clippy -p game-core --all-targets --all-features -- -D warnings` — Expected: PASS.

---

## Task 5: Migrate integration test crates to the helper

Same transformation as Task 4, across the external test crates. Each crate already calls `install(cards::REGISTRY)` where needed; `game_core::test_support::{take_turn_action, TurnAction}` is reachable (test_support is `pub`; `TurnAction` is re-exported from `enumerate`). Do NOT touch `PerformSkillTest` (Task 6) or `StartScenario`.

**Files:**
- `crates/cards/tests/*.rs` — all files listed in the spec's migration set (≈38 files: `act_advancement, activate_ability_aoo, barricade, beat_cop, commit_cap, cover_up, deduction, dodge, dodge_aoo, dr_milan, dynamite_blast, enumerate_actions, evidence, fast_play, first_aid, flashlight, guard_dog_soak, holy_rosary, hyperawareness, magnifying_glass, medical_texts, mind_over_matter, neutral_cards, old_book_of_lore, persistent_treachery, play_card, play_card_aoo, reject_rollback, research_librarian, retaliate_windows, roland_banks, roland_banks_seated, roland_elder_sign, skill_test_commits, soak_distribution, vicious_blow, weapon_38_special, weapon_machete`).
- `crates/scenarios/tests/*.rs` (≈11: `closing_demo, cover_up_interrupt, hunter_movement, mythos_phase, revelation_choice, synthetic_resolution, the_gathering, the_gathering_resolutions, the_gathering_symbols, upkeep_hand_size, upkeep_phase`).
- `crates/server/tests/*.rs` (`game_session, resume, ws`; `closing_demo` if it uses gameplay variants — note: `ws.rs` keeps `StartScenario`).
- `crates/web/tests/controls.rs`.

**Note on `cards/tests/enumerate_actions.rs`:** this asserts the enumeration directly — migrate its assertions from `PlayerAction::X` to `TurnAction::X` (raw-OptionId/enumeration test per the spec's "both" decision), not to the semantic helper.

- [ ] **Step 1: Migrate `crates/cards/tests/` (batch of files).** Apply the pattern file-by-file. Run `cargo test -p cards` — Expected: PASS.
- [ ] **Step 2: Commit** — `git commit -am "cards: migrate integration tests to take_turn_action (#447)"`
- [ ] **Step 3: Migrate `crates/scenarios/tests/`.** Run `cargo test -p scenarios` — Expected: PASS.
- [ ] **Step 4: Commit** — `git commit -am "scenarios: migrate integration tests to take_turn_action (#447)"`
- [ ] **Step 5: Migrate `crates/server/tests/` + `crates/web/tests/controls.rs`.** Run `cargo test -p server` and `wasm-pack test --headless --firefox crates/web` — Expected: PASS.
- [ ] **Step 6: Commit** — `git commit -am "server,web: migrate integration tests to take_turn_action (#447)"`

---

## Task 6: Remove `PerformSkillTest` from the wire → `test_support` helper

Replace the synthetic skill-test entry point with a `test_support` function, migrate the ~51 sites, then delete the variant. Additive-then-delete keeps it green.

**Files:**
- Modify: `crates/game-core/src/test_support/resolver.rs` (add `perform_skill_test` helper)
- Modify (migrate sites): `crates/cards/tests/{commit_cap,deduction,holy_rosary,hyperawareness,magnifying_glass,neutral_cards,roland_elder_sign,skill_test_commits}.rs`; `crates/game-core/tests/{activate_ability,on_skill_test_resolution,skill_test_outcome_timing}.rs`; `crates/scenarios/tests/the_gathering_symbols.rs`; `crates/server/tests/{closing_demo,common/mod.rs,resume}.rs`; game-core internal tests in `engine/mod.rs`, `engine/evaluator.rs`, `engine/dispatch/{skill_test,reaction_windows}.rs`, `state/game_state.rs`.
- Modify (delete variant + handler dispatch arm): `crates/game-core/src/action.rs`, `crates/game-core/src/engine/dispatch/mod.rs`. Keep the underlying `skill_test::perform_skill_test` handler function (the helper calls it).
- Modify: `crates/card-dsl/src/dsl.rs` (only if it references the variant in a doc-comment — update prose, no logic).

**Interfaces:**
- Produces: `pub fn perform_skill_test(state: GameState, investigator: InvestigatorId, skill: SkillKind, difficulty: i8) -> ApplyResult` (drives via the engine the way `apply` does, returning the first outcome — typically the commit-window `AwaitingInput`).

- [ ] **Step 1: Write the failing test** for the helper (in `resolver.rs` tests): a state with a seated investigator + non-empty chaos bag; `perform_skill_test(state, inv, SkillKind::Intellect, 2)` returns an `AwaitingInput` (commit window) — `assert!(matches!(result.outcome, EngineOutcome::AwaitingInput { .. }))`.

- [ ] **Step 2: Run it, verify it fails** — `cargo test -p game-core perform_skill_test_helper` → FAIL (undefined).

- [ ] **Step 3: Implement the helper.** Build the `Cx` and call the existing handler + `drive` exactly as `apply` does for a player action. Read `crate::apply` and `engine::dispatch::skill_test::perform_skill_test`'s signature first; the helper is:

```rust
#[must_use]
pub fn perform_skill_test(state: GameState, investigator: InvestigatorId, skill: crate::state::SkillKind, difficulty: i8) -> ApplyResult {
    // Mirror `apply`'s Cx construction; dispatch the skill-test start path directly,
    // then run the main loop. (Exact Cx/drive wiring copied from `engine::apply`.)
    crate::engine::apply_via(state, |cx| crate::engine::dispatch::skill_test::perform_skill_test(cx, investigator, skill, difficulty))
}
```

If no `apply_via`-style seam exists, add a small `pub(crate) fn apply_via(state, f: impl FnOnce(&mut Cx) -> EngineOutcome) -> ApplyResult` in `engine/mod.rs` that factors the `Cx` build + `drive` + `ApplyResult` assembly out of `apply` (refactor `apply` to call it too — behaviour-identical). This is the clean way to expose the skill-test start without a wire variant.

- [ ] **Step 4: Run the helper test, verify pass** — `cargo test -p game-core perform_skill_test_helper` → PASS.

- [ ] **Step 5: Commit the helper (variant still present)** — `git commit -am "engine: test_support::perform_skill_test helper (#447)"`

- [ ] **Step 6: Migrate all `PerformSkillTest` sites** to the helper. Pattern:

```rust
// BEFORE
let result = apply(state, Action::Player(PlayerAction::PerformSkillTest { investigator: inv, skill: SkillKind::Intellect, difficulty: 2 }));
// AFTER
let result = perform_skill_test(state, inv, SkillKind::Intellect, 2);
```

Run after each crate: `cargo test -p game-core`, `cargo test -p cards`, `cargo test -p scenarios`, `cargo test -p server`. Expected: PASS.

- [ ] **Step 7: Delete the variant.** Remove `PerformSkillTest` from `PlayerAction` (`action.rs`) and its arm from `apply_player_action` (`dispatch/mod.rs:91`). Update any doc-comment in `card-dsl/src/dsl.rs` that names it.

- [ ] **Step 8: Full workspace gauntlet** — `RUSTFLAGS="-D warnings" cargo test --all --all-features` + `cargo clippy --all-targets --all-features -- -D warnings`. Expected: PASS (no remaining `PerformSkillTest` references).

- [ ] **Step 9: Commit** — `git commit -am "engine: delete PerformSkillTest from the wire (#447)"`

---

## Task 7: The flip — open turn emits `AwaitingInput`; delete the typed gameplay variants

The keystone. Every test + the web client are now on the OptionId path, so flipping behaviour and deleting the variants is green.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (or wherever `drive`'s idle arm lives — `dispatch/mod.rs:290`): emit `AwaitingInput` at the open turn.
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`drive` idle arm; delete `apply_player_action`'s typed gameplay arms).
- Modify: `crates/game-core/src/state/game_state.rs` (`Continuation::awaits_input`).
- Modify: `crates/game-core/src/action.rs` (delete the 11 gameplay variants).
- Modify: `crates/web/src/controls.rs` (remove bespoke open-turn controls), `crates/web/src/legality.rs` (simplify `ActionControl`).

**Interfaces:**
- Consumes: `enumerate::{legal_actions, TurnAction}`, `InputRequest::choice`, `ChoiceOption`, `OptionId`.
- Produces: open-turn `EngineOutcome::AwaitingInput { request, resume_token: ResumeToken(0) }`.

- [ ] **Step 1: Write the failing test** (game-core lib): at the open turn, `apply` of any prior action that lands on the open turn yields `AwaitingInput` whose `request.options` matches `legal_actions`:

```rust
#[test]
fn open_turn_emits_awaiting_input_menu() {
    let state = /* open_turn_state() shape */;
    let expected = crate::engine::enumerate::legal_actions(&state).len();
    // Re-enter drive by submitting a no-op-ish resolve that returns to the open turn,
    // or assert directly on a freshly-driven state. Simplest: drive a fresh state to
    // the open turn and inspect the outcome the session last produced.
    let result = crate::test_support::take_turn_action(state, crate::engine::enumerate::TurnAction::Resource { investigator: crate::state::InvestigatorId(1) });
    let crate::EngineOutcome::AwaitingInput { request, .. } = result.outcome else {
        panic!("open turn must emit AwaitingInput after an action resolves, got {:?}", result.outcome)
    };
    assert_eq!(request.options.len(), /* legal_actions count at the post-action open turn */ );
    assert!(request.options.iter().any(|o| o.label == "End turn"));
}
```

- [ ] **Step 2: Run it, verify it fails** — `cargo test -p game-core open_turn_emits_awaiting_input_menu` → FAIL (outcome is `Done`).

- [ ] **Step 3: Add `turn_menu` + emit in `drive`.** Add a helper (next to the enumerator or in `dispatch/mod.rs`):

```rust
fn turn_menu(state: &GameState) -> crate::engine::InputRequest {
    let options = crate::engine::enumerate::legal_actions(state).iter().enumerate()
        .map(|(i, a)| crate::engine::ChoiceOption { id: crate::engine::OptionId(i as u32), label: a.label(state) })
        .collect();
    crate::engine::InputRequest::choice("Choose an action", options)
}
```

In `drive`, replace the idle return for the open turn. The current catch-all `_ => return EngineOutcome::Done` (`dispatch/mod.rs:301`) must special-case the open turn *before* it:

```rust
Some(Continuation::InvestigatorTurn { ending: false, .. }) => {
    return EngineOutcome::AwaitingInput {
        request: turn_menu(&cx.state),
        resume_token: crate::engine::ResumeToken(0), // deterministic token is #458
    };
}
// ... existing InvestigatorTurn { ending: true } arm stays ...
_ => return EngineOutcome::Done,
```

- [ ] **Step 4: Flip `awaits_input`** in `game_state.rs:819`: move `InvestigatorTurn { ending: false }` to return `true`; keep `ending: true` (transient) returning `false`. Update the arm:

```rust
Continuation::InvestigatorTurn { ending: false, .. } => true,
Continuation::InvestigatorTurn { .. } | Continuation::AttackLoop { .. } | Continuation::ActionResolution { .. } => false,
```

Update the doc-comment above it (it currently says "The open turn takes typed actions … not ResolveInput") to reflect the OptionId reality, and the `awaits_input_gates_suspensions_but_not_anchors_or_fast_windows` test at `game_state.rs:1916` if it asserts the old open-turn value.

- [ ] **Step 5: Run the flip test + FULL WORKSPACE suite, then exhaustively fix open-turn `Done` assertions.** Run `RUSTFLAGS="-D warnings" cargo test --all --all-features` (not just game-core). The flip is the **exhaustive detector**: every test whose action sequence now ends back at the open turn returns `AwaitingInput` (the menu) instead of `Done`. Tasks 4/5 deliberately did NOT try to pre-find these (static inspection is unreliable — the suite mixes legit `Done`-after-`ResolveInput`/mythos/skill-resolution that STAY `Done` with `Done`-after-action-returning-to-open-turn that flip). Fix EACH failure thus:
  - **Default:** the `Done` was incidental — assert on the action's effect (state/events) instead, or `assert!(!matches!(outcome, Rejected{..}))`. Many already have the effect assertion right after; just drop the `Done` line.
  - **ANTI-VACUOUSNESS TRAP (critical):** for any test where the `Done` assertion was itself the proof of an ABSENCE (no AoO window opened, no choice/reaction prompt fired, no suspension) — do NOT swap `Done`→`AwaitingInput` or `!AwaitingInput`. Post-flip the open-turn menu is *also* `AwaitingInput`, indistinguishable from the window whose absence is under test, so either swap makes the test vacuous. Re-express on concrete state/events (e.g. "no AoO damage taken", "heal not applied", "no `EnemyAttack` event"), outcome-independent. (The 2 known AoO cases in `activate_ability_aoo.rs` were already fixed in Task 5; apply the same principle to any others the flip surfaces — `fast_play.rs`, reaction/window-absence tests, etc.)
  - Tests that legitimately still end mid-resolution (`ResolveInput`/`commit_cards` completing a skill test that does NOT return to the open turn, mythos/encounter `Confirm`, phase-transition tails) keep their `Done` — only change the ones the test run actually reports failing.
  Expected after fixes: PASS.

- [ ] **Step 6: Delete the typed gameplay arms + variants.** Remove the 11 gameplay arms from `apply_player_action` (`dispatch/mod.rs:90-127`, leaving `StartScenario` + `ResolveInput`). Delete the 11 variants from `PlayerAction` (`action.rs`). `dispatch_turn_action` is now the sole dispatch path.

- [ ] **Step 7: Migrate the web open-turn controls.** In `controls.rs`, delete `move_picker`, `play_picker`, `enemy_picker`, `fight_action`, `evade_action`, and the `investigate`/`advance_act`/`draw`/`end-turn` `submit_button`s + their wiring in `ActionControls`. Keep the `StartScenario` button. The open-turn menu now renders through `AwaitingInputView` (Task 3). In `legality.rs`, drop the `ActionControl` variants for the removed controls (keep `StartScenario`); `enabled_controls` already returns empty under `AwaitingInput`, so no behaviour gate is lost. Update `web/tests/controls.rs` assertions accordingly.

- [ ] **Step 8: Full CI gauntlet (all seven jobs).**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all PASS. `PlayerAction` is now `StartScenario` + `ResolveInput` only.

- [ ] **Step 9: Commit** — `git commit -am "engine/ui: open turn emits AwaitingInput menu; delete typed gameplay variants (#447)"`

---

## Task 8: PR + phase-doc update

- [ ] **Step 1: Push the branch + open the PR.**

```bash
git push -u origin engine/optionid-routing
gh pr create --fill --title "engine/ui: 2b — open-turn gameplay via ResolveInput(OptionId) (#447)"
```

PR body: design-decisions paragraph (TurnAction internal enum; re-enumerate not cache — derived-state-out-of-GameState rationale; StartScenario kept as free-form session data; PerformSkillTest → helper; web flat-list now, richness #205) + `Closes #447.` + note the two follow-ups #458/#459.

- [ ] **Step 2: Watch CI** — `gh pr checks <PR#> --watch`. Fix failures with follow-up commits (no force-push).

- [ ] **Step 3: Update `docs/phases/phase-7-the-gathering.md`** as the final commit once CI is green (per `docs/phases/README.md`): under "Browser capstone", flip the **#447** row to `✅ PR #N`; add a **Decisions made** entry only if load-bearing for a future PR (e.g. "open turn is an `AwaitingInput` menu; `legal_actions` re-enumerated at resolve, not cached on the frame"); note #458/#459 as the split-out follow-ups (token + StartScenario→creation-with-picker). Move #447 to the Closed table, bump counts.

- [ ] **Step 4: Merge only after explicit user approval** — `gh pr merge <PR#> --squash --delete-branch`; confirm #447 auto-closed; `git pull` on `main`.

---

## Self-review

**Spec coverage:** §A TurnAction → Task 1. §B PlayerAction shrink → Tasks 6 (PerformSkillTest) + 7 (gameplay variants). §C emission/resolution/stability → Tasks 1 (resolution + re-enumerate) + 7 (emission + awaits_input). §D PerformSkillTest → Task 6. §E web → Tasks 3 + 7. §F test strategy (both) → Tasks 2 (helper) + 1/5 (raw OptionId in enumerate tests). Out-of-scope #458/#459 → not implemented, referenced in Tasks 7/8. ✅ all covered.

**Placeholder scan:** Mechanical-migration tasks (4, 5, 6-step-6) intentionally give a transformation pattern + exhaustive file list + per-chunk verification rather than 550 explicit diffs — this is the right granularity for a mechanical sweep, not a placeholder. All novel code (Tasks 1, 2, 3, 6-helper, 7) has concrete code. The one genuine unknown — whether an `apply_via` seam exists — is handled with an explicit "if not, add it by factoring `apply`" instruction (Task 6 Step 3).

**Type consistency:** `TurnAction` field names/types copied from `PlayerAction` (Task 1); `take_turn_action(GameState, TurnAction) -> ApplyResult` and `TestSession::take(TurnAction)` consistent across Tasks 2/4/5/7; `perform_skill_test(GameState, InvestigatorId, SkillKind, i8) -> ApplyResult` consistent Task 6; `turn_menu`/`label`/`legal_actions` signatures consistent Tasks 1/7. ✅
</content>
</invoke>
