# A stored game is its inputs and a checked log

Thursday's game is half-played. On Friday a PR adds a field to `GameState`. On Saturday the group reloads, and the load breaks: the `games` row held the seated state, its outcome and its setup events as serialized engine types (migrations 0001–0003), so every change to an engine type's shape stranded every saved game. A log that still deserialized could also replay to a silently wrong board. `PickSingle(OptionId)` is a position in the option list, so a PR that adds or reorders an option makes an old answer pick a different one. `Confirm` and `Skip` name nothing, so an old `Skip` passes whatever skippable prompt a PR inserts before the one it answered. Chaos-token pulls and shuffles are recomputed from the seed rather than logged, so a PR that adds one RNG draw changes every later pull while every logged choice still matches.

**A saved game survives an engine change where it can, and fails loudly where it can't. It never resumes on a silently wrong board.** That is the promise, decided in #1215.

## What is stored

**A stored game is its creation inputs plus its action log, never its state.** The `games` row holds the scenario id, the roster, the lead index, the RNG seed and the `interactive_acknowledge` flag. A load re-runs `seat_and_open` from them and replays the log, and setup's events come out of the re-run. The row also carries `format_version`, bumped when the shape of the inputs or of a log entry changes, and `created_build`, which is diagnostic only: a build mismatch alone never refuses a load, because the checks below catch drift action by action. Each action carries `at`, when it was applied, for #1226's live-game guard. The roster is stored as the engine's `RosterEntry` JSON, so renaming one of its fields fails a load as a deserialization error, which is loud.

**The setup fingerprint records what setup decided**, as card codes: every investigator's hand, every investigator's deck in order, the encounter deck in order, and the chaos bag (a numeric token as its signed modifier, any other by its variant name). At load, the re-run setup's fingerprint is compared with the stored one. It is built from card codes only, never from a serialized engine type, so a shape change can't trip it, and it is stored as plain JSON rather than a hash so that a mismatch names the zone and position that differ (`server::fingerprint`). A frozen seed survives only the engine changes this promise doesn't need, and breaks on the ones it does.

## The replay checks

Each log entry is a server-owned type that records what it answered: the response, each picked option's `id`, `label`, `target` and `investigator`, the answered prompt's `kind`, `target` and `responder` (not its text, since a copy-edit would break every saved game), and `RngState.draws` after the action applied. Replay checks each entry in order:

1. **The prompt is the one it answered.** The current prompt's identity must equal the recorded one. This is what catches an inserted prompt absorbing an old `Skip`. #1230's `prompt: seq` doesn't replace it: it guards a live submit from a stale tab, but during replay entry N always answers whatever sits at position N.
2. **Each pick finds its option by (label, target, investigator)**, and replay answers with the id the option has now. Zero matches or several fails the load. Additions and reorderings survive, which matters because every new card adds options; a removed or renamed option fails. The investigator is in the key because a several-answerer window can offer two investigators the same label on the same anchor ([ADR 0019](0019-every-prompt-names-who-answers-it.md); `ChoiceOption::investigator`: *"Two options on one anchor offered to two investigators differ here."*).
3. **The engine accepts it.** A rejection fails the load.
4. **The RNG checkpoint matches.** `draws` must equal the recorded value, so the first action whose randomness differs fails rather than every later pull drifting silently.

Recording and checks 1 and 2 are one pure, I/O-free module, `server::logged_answer`, so the session core can lift it unchanged; `answered_options` is the one lookup from a response to the options it picked. An `actions` row holds the logged answer as JSON in `answer` and the checkpoint in its own `draws` column. An action the engine accepts but the log can't record is refused at `apply` like a rejection, so a game never holds a move it couldn't replay.

## An unresumable game is kept

A load that fails a check returns `SessionError::Unresumable` with the game id and a reason: the version found, the fingerprint zone that differs, setup no longer running from the inputs, or the zero-based seq of the action that failed and why. `Ok(None)` stays "no such game". A partial replay is never served. The rows are left as they are, so rolling the deploy back recovers the game, which deleting it or falling back to the last good prefix would not. Only the current `format_version` loads, and no converter is written until real games exist to convert.

## Considered options

**Pinning a game to the build that created it** was rejected: it would strand a game on almost any engine PR, and 24 commits touched `game-core/src/state/` between September and #1215.

**Hand-written migrations per engine change** were rejected as too costly at friends scale. Both rejections rest on `docs/product-decisions.md`: *"where player experience and implementation cost trade off, player experience wins."*

**A label check that only compares**, keeping the logged position and failing when the label there differs, was rejected because it strands a game on every addition to a menu.

**Logging every random outcome** instead of checkpointing `draws` was rejected as an engine rewrite: production code never constructs an `EngineRecord`, and every draw comes from the seeded `RngState`.

**Snapshots** (#174) stay deferred. If one is ever built, it is a cache tagged with the build that wrote it and discarded on a mismatch, never the source of truth.
