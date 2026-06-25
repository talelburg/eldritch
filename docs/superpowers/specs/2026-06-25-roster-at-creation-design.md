# Roster at game-creation — design (#459 + #224)

**Date:** 2026-06-25
**Issues:** #459 (roster at game-creation; `StartScenario` → `CreateGameRequest`; action
log becomes `ResolveInput`-only) and #224 (migrate `StartScenario` tests to roster
seating; require a non-empty roster), folded into one PR.
**Phase:** 7 — The Gathering (browser capstone — the picker).

## Problem

The web client creates a game by auto-`POST`ing `/games` with a hardcoded scenario and
then submitting `PlayerAction::StartScenario { roster: vec![] }`. But the scenario's
`setup()` seats **nobody**, and `start_scenario` rejects a roster that would leave zero
investigators seated — so the "Start scenario" button always rejects with *"a scenario
requires at least one investigator"*. The UI is unplayable.

The root cause is that seating is a **logged player action** applied on top of a
roster-less seed, rather than part of game creation. #459 moves seating into game
creation so the persisted seed is already seated; #224 then removes the now-unnecessary
"pre-seeded state + empty roster" test scaffolding and tightens seating to a single
strict path.

## Goals

- A solo human picks an investigator + scenario in the browser, creates a game, and
  lands directly on the setup mulligan prompt — rules-correct, no reject.
- `PlayerAction` collapses to a single `ResolveInput` variant. The action log is
  `ResolveInput`-only; seating is no longer a logged action.
- The persisted seed bakes in seating + setup shuffle; replay re-draws deterministically
  from the frozen seed (no setup RNG re-run).
- `seat_and_open` has **one** seating path: a non-empty roster of registry-resolvable
  investigators. No pre-seeded tolerance (#224).
- Every `StartScenario` call site (~37) is migrated to the new function once.

## Non-goals

- Decklist import / deck validation — the roster `deck` stays a free input (Phase 9).
- Multi-scenario / multi-investigator breadth — only The Gathering and Roland (`01001`)
  exist; the picker is structured to grow but ships with one of each.
- `#458` (deterministic resume-token) and `#205` (structured input rendering) — separate
  capstone items.

## Design

### 1. Engine: seating becomes a function, not an action

Extract the body of `engine::dispatch::phases::start_scenario` into a public engine entry
point:

```rust
// game_core (re-exported at crate root)
pub fn seat_and_open(setup_state: GameState, roster: &[RosterEntry]) -> ApplyResult;
```

It constructs a `Cx` over `setup_state`, runs the existing seating logic (resolve stats →
seat → reveal start location → round/phase → deal hands → shuffle encounter deck → reset
actions → open the mulligan), runs `drive()` to the first `AwaitingInput`, and returns the
same `ApplyResult { state, events, outcome }` that `apply` produces. All current machinery
is reused; it simply isn't reached through `apply`.

`PlayerAction::StartScenario` and its dispatch arm are deleted. `PlayerAction` collapses to:

```rust
#[non_exhaustive]
pub enum PlayerAction {
    ResolveInput { response: InputResponse },
}
```

Kept as a single-variant `#[non_exhaustive]` enum (not flattened to a struct) to preserve
the externally-tagged `{"resolve_input": …}` wire form and leave room to grow — lower
churn than changing the wire shape. `RosterEntry` stays in `game_core::action` (now
consumed by `seat_and_open` + the protocol).

**Validation tightening (#224).** `seat_and_open` rejects:
- an **empty roster** outright (new — replaces the old "zero investigators after seating"
  check and drops the pre-seeded tolerance);
- a roster entry whose code is absent from the registry or is not a
  `CardKind::Investigator`;
- application to a state already in progress (`round != 0`), as today.

There is now exactly one seating path. The
`start_scenario_empty_roster_passes_through_with_preseated_investigator` test inverts to
assert rejection.

### 2. Persistence + protocol: the seed bakes in seating

`CreateGameRequest` gains the roster:

```rust
pub struct CreateGameRequest {
    pub scenario_id: String,
    pub roster: Vec<RosterEntry>,
}
```

`GameSession::create` runs `(module.setup)()` → `seat_and_open(state, &roster)` and
persists `result.state` (already seated + shuffled + mulligan-pending) as `seed_state`;
it stores `result.outcome` as the session's outcome (an `AwaitingInput` mulligan for a
normal game). Because the seed's `RngState` is frozen post-shuffle and shuffles draw from
that seed (no `EngineRecord` for setup), **replay no longer re-runs setup RNG** — `load`
replays the `ResolveInput`-only log over the already-seated seed and reproduces state
bit-for-bit.

A rejected seating maps to a new error and **persists nothing** (no orphan `games` row):

```rust
pub enum SessionError {
    // …existing…
    #[error("seating rejected: {0}")]
    Seating(String),
}
```

`create_game` (HTTP) maps `SessionError::Seating(_)` → **422 Unprocessable Entity**
(distinct from `UnknownScenario` → 400). The handler currently returns a bare status code
for errors (no body), matching the existing `create_game` shape; the reject reason is
logged server-side. Surfacing it to the client is out of scope (the picker only offers
valid investigators, so a 422 is a programming error, not a user-facing one).

### 3. Web: minimal picker + gated creation

**`picker.rs`** (new component): a pre-game screen with a scenario `<select>` (The
Gathering) and an investigator radio list (Roland Banks `01001`), structured as lists so
Slice-2 investigators drop in. "Create game" builds `CreateGameRequest { scenario_id,
roster }` where the roster is the chosen investigator paired with a **default deck**
(below) and submits it.

**Default Roland deck** — a named const in the web crate (a placeholder until Phase 9
decklist import), composed only of already-implemented cards so the hand is playable:

| Code | Card | Kind |
|------|------|------|
| 01006 | .38 Special | Guardian asset (signature) |
| 01020 | Machete | Guardian asset |
| 01018 | Beat Cop | Guardian asset |
| 01021 | Guard Dog | Guardian asset |
| 01019 | First Aid | Guardian asset |
| 01024 | Dynamite Blast | Guardian event |
| 01022 | Evidence! | Guardian event |
| 01023 | Dodge | Guardian event |
| 01025 | Vicious Blow | Guardian skill |
| 01030 | Magnifying Glass | Seeker asset |
| 01039 | Deduction | Seeker skill |
| 01037 | Working a Hunch | Seeker event |
| 01089–01093 | Guts / Perception / Overpower / Manual Dexterity / Unexpected Courage | Neutral skills |
| 01007 | Cover Up | Roland's signature weakness |

The list is a deliberate placeholder, **not** a legal 30+1 deck — it exists so the mulligan
and a first turn have real cards. Codes are verified against the implemented impls in
`crates/cards/src/impls/`.

**`transport.rs`** stops auto-creating. The flow becomes:
- A saved game id → reconnect (unchanged).
- No saved id → render the picker; its submit calls `create_game(roster)`.
- `StaleId` (saved id unknown to the server) → clear the id and **drop back to the
  picker** rather than silently recreating — the client never persisted the roster, so it
  cannot recreate the same game; the user re-picks.

`ActionControls`' "Start scenario" button is removed (the picker replaces it). The
mulligan and all gameplay already render through `AwaitingInputView` — unchanged.

### 4. Test migration (#459 mechanics + #224 tightening)

~37 `StartScenario` call sites, migrated once:

- **game-core in-crate unit tests + `crates/game-core/tests/`** (~15 sites): call
  `install_test_registry()`, drop `.with_investigator(test_investigator(N))` pre-seeding,
  and seat `roster: vec![RosterEntry { investigator: CardCode(TEST_INV), deck: vec![] }; N]`
  through `seat_and_open`. `TEST_INV` already resolves (via the test registry) to a
  seatable investigator with the same 3/3/3/3 stats as `test_investigator`, so this stays
  within crate layering (game-core never reaches `cards`). Multi-investigator tests seat a
  length-N roster (ids mint `1..=N`).
- **`scenarios/tests` + `server/tests` + `cards/tests`** (~10 sites): same call shape;
  these install `cards::REGISTRY` and may seat real Roland (`01001`) where corpus stats
  matter. The fold-style tests (`closing_demo`, `the_gathering*`) seat first via
  `seat_and_open`, then fold the subsequent `ResolveInput`s.
- A `test_support` helper (e.g. `seat_test(state, codes)`) is added if it materially cuts
  churn.

**Assertion consequences (low, mechanical):** a roster-seated synthetic investigator is
named `"Test Investigator"` (no `N` suffix) and starts at `starting_location` (often
`None`) instead of wherever a fixture placed it. A handful of name/location assertions get
minor tweaks. Skills, actions, resources, clues are identical — no numeric re-baselining.

### 5. Server creation path

`crates/server/src/session.rs` `create` now needs a roster argument; `lifecycle.rs`
`create_game` threads `request.roster` in and maps `SessionError::Seating` → 422. The
server already installs `cards::REGISTRY` and `scenario_registry` at startup, so
`seat_and_open` resolves real investigator stats during creation.

## Testing strategy

- **Engine:** `seat_and_open` opens the mulligan for a valid roster; rejects an empty
  roster, an unknown code, a non-investigator code, and an already-started state (ported
  and inverted from the existing `start_scenario` tests).
- **Server:** `create` with a roster persists a seated seed and returns an `AwaitingInput`
  outcome; `load` replays a `ResolveInput`-only log and reproduces the live state
  bit-for-bit; a bad roster → 422 and **no** `games` row.
- **Protocol:** `CreateGameRequest { scenario_id, roster }` round-trips through JSON.
- **Web:** a wasm test in `crates/web/tests/` asserts the picker renders Roland and that
  submit emits `CreateGameRequest` carrying a non-empty roster.

## Risks / open considerations

- **Large diff.** The 37-site sweep dominates. Mitigated by the mechanical shape and the
  pre-existing `TEST_INV` registry; reviewed as a focused pass.
- **`StaleId` UX.** Dropping to the picker (vs. silent recreate) is a deliberate v0
  simplification — the client doesn't persist the roster. Acceptable for solo v0.
- **Single-variant `PlayerAction`.** Kept as an enum for wire stability; revisit only if a
  future variant never materializes (it will — engine-side typed actions are gone, but the
  enum is the wire seam).

## Outcome

`PlayerAction` = `{ ResolveInput }`; the action log is input-only; the persisted seed is
seated + shuffled + mulligan-pending; the browser picker creates a playable Roland game in
The Gathering; #459 and #224 both close. The phase-7 doc's "Browser capstone — picker"
item advances and the open-turn UI is testable again.
