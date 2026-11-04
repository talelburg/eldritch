# A lobby is separate rows until Start, and the assignment sits beside the stored game

Tal creates a game of The Gathering and sends Sam the link. For ten minutes the game has a scenario, a creator and two members, and Sam holds Daisy Walker, but there is no roster, no lead and no RNG seed yet. Then Tal presses Start, and from then on Sam answers Daisy's prompts and Tal answers Roland's. The server restarts at some point on either side of Start, and both stages have to come back.

**A lobby is stored in its own rows: the lobby, its picks, and its members. Start writes the `games` row under the lobby's game id, together with the investigator → account assignment, and deletes the lobby and pick rows in the same transaction. The assignment is stored beside the `games` row, not among its creation inputs.** Decided in #1239.

- **A `games` row still means "started and loadable".** ADR 0022's stored game is unchanged: its creation inputs are the scenario, roster, lead, seed and `interactive_acknowledge` flag, all present from the row's first write. No load path learns about half-filled games.
- **The game id is shared across Start**, so the link Tal sent stays valid. Membership outlives Start in its own rows, because "your games" (#1240) lists lobbies and started games alike.
- **The assignment is not a creation input, so `FORMAT_VERSION` doesn't move.** It controls who may answer, not what the engine does, and it isn't in the action log. The engine never sees an account (ADR 0019 names investigators, not accounts). Reassigning an investigator mid-game (#1241) rewrites the assignment rows and leaves the inputs and the log alone.
- **A stored game with no assignment is unresumable** (`NoAssignment`), with no fallback to the creator holding everyone. Every game stored before #1239 was stored that way, and ADR 0022 already says *"no converter is written until real games exist to convert."*

## Considered options

**One `games` row with nullable creation inputs and a status column** was rejected. There would be a single row to find and no move at Start, but every reader of a `games` row would have to handle a game with no roster. ADR 0022's guarantee that a row is enough to re-run setup would gain an exception, and so would its checks on load.

**Storing the assignment as a creation input** (in the `games` row, bumping `FORMAT_VERSION`) was rejected. A reassignment would then be an edit to a game's inputs, which ADR 0022 treats as fixed at creation.
