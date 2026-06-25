# 2b — Open-turn gameplay via `ResolveInput(OptionId)` (#447)

**Date:** 2026-06-25
**Issue:** #447 (`[engine/ui] 2b: eliminate typed PlayerAction`)
**Follow-ups filed:** #458 (deterministic resume-token, §F), #459 (StartScenario → game-creation, picker-paired)
**Spec refs:** `2026-06-20-unified-control-flow-model-design.md` §E/§F

## Summary

The #393 §E end-state: at the open turn, the engine emits its **legal-action
enumeration** as `OptionId`s and the only gameplay input is
`ResolveInput(PickSingle(OptionId))`. The eleven typed gameplay `PlayerAction`
variants are deleted from the wire; an internal `TurnAction` enum becomes the
id→action map. `PerformSkillTest` (a test-only synthetic) is deleted too, replaced
by a `test_support` helper. The web client's bespoke open-turn controls are removed
in favour of rendering the engine-offered options as a flat list.

**End-state of this PR:** `PlayerAction` has **two** variants — `StartScenario`
(session-setup data) and `ResolveInput` (all in-game input). Collapsing
`StartScenario` into game-creation (single-variant end-state) is #459, landing with
the scenario/investigator picker.

Behaviour-preserving: rules are unchanged; only the action-submission surface moves.

## Motivation

The client should be a **thin renderer of exactly the engine-offered options** — one
input mechanism (`ResolveInput(OptionId)`) instead of "render options *and* know how
to construct typed actions." This is the engine half of the browser capstone and the
foundation #205 (rich client rendering) builds on.

## A. The internal `TurnAction` enum

A new enum in `game-core` (not on the wire — **no `Serialize`/`Deserialize`**):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAction {
    EndTurn,
    Move { investigator: InvestigatorId, destination: LocationId },
    Investigate { investigator: InvestigatorId },
    Resource { investigator: InvestigatorId },
    Draw { investigator: InvestigatorId },
    Fight { investigator: InvestigatorId, enemy: EnemyId },
    Evade { investigator: InvestigatorId, enemy: EnemyId },
    Engage { investigator: InvestigatorId, enemy: EnemyId },
    PlayCard { investigator: InvestigatorId, hand_index: u8 },
    ActivateAbility { investigator: InvestigatorId, instance_id: CardInstanceId, ability_index: u8 },
    AdvanceAct { investigator: InvestigatorId },
}
```

These are exactly the variants lifted off `PlayerAction`, same fields. `TurnAction`
is `pub` so `test_support` (and tests in other crates) can construct it for the
semantic helper, but it never crosses the transport boundary.

A `fn label(&self, state: &GameState) -> String` produces the human-readable menu
label (e.g. `"Move to Attic"`, `"Play Knife"`, `"Fight Ghoul"`). Labels are
deliberately plain for 2b; **rich/structured rendering metadata is #205's concern.**

`TurnAction` is irreducible: `legal_actions` must return *something* naming each
option and carrying the params the handlers need (`destination`, `enemy`,
`hand_index`, …). This enum exists regardless of how thin `PlayerAction` becomes.

## B. The `PlayerAction` wire surface

Delete the eleven gameplay variants **and** `PerformSkillTest`. After this PR:

```rust
#[non_exhaustive]
pub enum PlayerAction {
    StartScenario { roster: Vec<RosterEntry> },  // session-setup data (free-form; → #459)
    ResolveInput { response: InputResponse },    // all in-game menu input
}
```

- **`StartScenario` stays** (this PR): it carries the roster — investigator codes +
  chosen decks — which is free-form *data submission*, not menu *selection*. The
  engine cannot enumerate "all possible rosters" as `OptionId`s. Tunnelling it
  through a new `InputResponse::Roster(..)` would merely **relocate** the typed
  payload, not remove it. #459 moves the roster to game-creation (the server already
  separates a persisted `seed_state` from the action log), which is the real fix and
  pairs with the picker.
- **`PerformSkillTest` goes** (§D): zero production usages; a pure test scaffold.

`dispatch::apply_player_action` shrinks to three arms: `StartScenario`,
`ResolveInput`, and (transitional) nothing else — the gameplay match arms move into
`dispatch_turn_action` (§C).

## C. Open-turn emission + resolution

### Emission (in `drive`)

When `Continuation::InvestigatorTurn { ending: false }` is the top frame, `drive`
currently idles with `Done`. It now returns:

```rust
EngineOutcome::AwaitingInput {
    request: turn_menu(&cx.state),     // InputRequest::choice over legal_actions
    resume_token: ResumeToken(0),      // placeholder; deterministic token is #458
}
```

`turn_menu(state)` builds `InputRequest::choice(prompt, options)` where
`options[i] = ChoiceOption { id: OptionId(i), label: legal_actions(state)[i].label(state) }`.
The menu is **never empty** — `EndTurn` is always legal at the open turn.

`Continuation::awaits_input()` flips to `true` for `InvestigatorTurn { ending: false }`
(it was `false`). `ending: true` is a transient internal rotation frame and stays
`false`. This makes the dispatch-mod pending-prompt gate reject any stray non-
`ResolveInput` action at the open turn, consistent with every other suspension.

Only the `InvestigatorTurn { ending: false }` idle case becomes `AwaitingInput`; the
other `drive` idle cases (terminal empty stack, parked phase, empty Fast-gate) still
return `Done`.

### Resolution (in `resolve_input`)

The `InvestigatorTurn { ending: false }` arm (today: rejects) becomes:

```rust
fn resume_turn_action(cx, response) -> EngineOutcome {
    let InputResponse::PickSingle(opt) = response else { return Rejected(..) };
    let actions = enumerate::legal_actions(&cx.state);
    let Some(action) = actions.get(opt.0 as usize).cloned() else { return Rejected("OptionId out of range") };
    dispatch_turn_action(cx, &action)
}
```

`dispatch_turn_action(cx, &TurnAction)` is the match lifted out of
`apply_player_action`, dispatching to the same handlers (`actions::move_action`,
`cards::play_card`, `phases::end_turn`, …) with unchanged signatures. The outcome
flows back through `drive`, which then emits the next open-turn menu, propagates a
mid-action suspension (skill test, AoO), or carries a phase transition forward.

### OptionId stability — re-enumerate, don't cache

`legal_actions` is re-run at resolve time and indexed by `OptionId`. This is correct
**and** the cleaner model:

- `enemies` / `locations` / `investigators` are `BTreeMap`s; `hand` / `cards_in_play`
  are `Vec`s — so enumeration order is deterministic. `apply` is atomic: nothing
  mutates state between the `AwaitingInput` return and the next `apply(ResolveInput)`,
  and replay re-derives the identical list. So the index the client echoes always
  maps to the same `TurnAction`.
- **Not** storing the enumeration on the frame is deliberate: the frame lives in
  `GameState`, which is serialized (persistence + replay). Caching `Vec<TurnAction>`
  there would force `TurnAction` to serialize, bake derived data into the
  event-sourced core, and add a consistency burden — for zero correctness gain on a
  ~30-element list that re-derives trivially. (Reaction windows store candidates only
  because those are transient snapshots that are *not* cheaply re-derivable; the open
  turn is the opposite.)

Out-of-range `OptionId`, wrong `InputResponse` variant → clean `Rejected`, state and
events unchanged (validate-first/mutate-second).

## D. `PerformSkillTest` removal → `test_support` helper

`PerformSkillTest` is a synthetic "start a skill test in isolation" action with
**zero production callers** — all ~51 usages are tests across `game-core`, `cards`,
`scenarios`, and `server`. It lets tests probe skill-test resolution with an
arbitrary skill + difficulty without a real triggering action.

`#[cfg(test)]`-gating the variant **does not work**: external integration crates
(`crates/cards/tests/`, `crates/server/tests/`, …) compile `game-core` without its
test cfg, so they would not see a cfg-gated variant.

Fix: delete the variant; add a `pub` `test_support` entry point that invokes the
skill-test start path directly (the existing `skill_test::perform_skill_test`
handler logic), building the `Cx` + running `drive` the way `apply` does, returning
an `ApplyResult`. `game_core::test_support` is unconditionally `pub`, so it is
callable from every test crate. Migrate the ~51 sites to it. (Pattern precedent:
`ScriptedResolver::commit_cards` wraps the commit window for tests.)

## E. Web client (forced by the deletion)

`crates/web/src/controls.rs` constructs eight gameplay variants; deleting them breaks
compilation. The 2b web change:

- **Add a `PickSingle` option-list renderer** to `AwaitingInputView` (`input.rs`),
  which today only renders `PickMultiple` (commit/mulligan): one button per
  `ChoiceOption`, click → `ResolveInput(PickSingle(id))`. The open-turn menu then
  flows through the same prompt path as every other `AwaitingInput`.
- **Remove the bespoke open-turn controls** in `controls.rs` (`move_picker`,
  `play_picker`, `enemy_picker`, and the investigate/draw/advance/end-turn
  `submit_button`s). Keep the `StartScenario` button (→ migrates in #459).
  `legality::enabled_controls` already returns empty under any `AwaitingInput`, so the
  now-dead controls disappear cleanly with no behaviour change; the `ActionControl`
  legality helper simplifies accordingly.
- **Richness deferred to #205.** 2b ships the honest flat list of engine-offered
  options; #205 re-enriches rendering from structured metadata.

## F. Test strategy (decision: "both")

- **Semantic helper** — `test_support::take_turn_action(state, TurnAction) -> ApplyResult`
  (plus a fluent `TestSession` form): assert the open-turn menu is live, find the
  `OptionId` whose enumerated `TurnAction` equals the argument, submit
  `ResolveInput(PickSingle(..))`, return the result. Keeps tests intent-revealing
  ("move to Attic") without hand-coding indices. Mirrors `commit_cards`.
- **Raw `OptionId`** — retained where the test is *about* the enumeration: the
  `enumerate.rs` order/label tests and the `every_enumerated_action_is_accepted_by_its_handler`
  cross-check (preserved, now dispatching each enumerated `TurnAction` via
  `dispatch_turn_action` and asserting not `Rejected`).
- **Migration** — open-turn-driving tests across `game-core` / `cards` / `scenarios`
  / `server` / `web` move from constructing typed actions to the semantic helper.
  Tests that hand-build a bare phase state and applied a typed action now must seat an
  `InvestigatorTurn` frame (the `.with_investigator_turn()` builder exists) so the
  menu is present; handler-level unit tests may call the handler directly instead.

## Out of scope (filed)

- **#458 — deterministic resume-token.** This PR keeps `ResumeToken(0)` at the open-
  turn site like every existing `AwaitingInput` site. §F's state-derived token
  (server-side stale-submit rejection, #347) is cross-cutting across all sites and
  orthogonal to OptionId routing.
- **#459 — StartScenario → game-creation.** Collapses `PlayerAction` to a single
  `ResolveInput` variant by moving the roster to game-creation (`CreateGameRequest`
  already exists; server already persists a `seed_state` separate from the action
  log). **Lands together with the investigator/scenario picker** capstone item — the
  picker is what collects the roster and creates the game.

## Acceptance (from #447)

- [x] All open-turn gameplay driven by `ResolveInput(PickSingle(OptionId))`; no typed
  gameplay `PlayerAction` variant remains as an input path.
- [x] The id→action map (`TurnAction` + `legal_actions`) is fully internal to the engine.
- [x] Existing rules tests stay green (behaviour-preserving).
</content>
</invoke>
