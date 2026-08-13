# A latched resolution cancels opportunities, not resolutions

The Rules Reference is terse about the end of a scenario: *"Some instructions in the act and agenda decks (as well as on other encounter cardtypes) contain resolution points, in the format of: '(→R#).' If a resolution point is reached, the scenario ends."* The engine did not act on that. `state.resolution` latched mid-apply, deep inside whatever ability reached the point, and play simply continued — `fire_scenario_resolution` ran the scenario module's `apply_resolution` at the apply boundary while the continuation stack still held live work, and discarded the drive outcome of the game-end forced pass entirely. We decided that the latch stays a pure flag and that the *drive loop* gates on it, frame by frame: a frame representing an **opportunity to act** (a reaction window, a Fast window) or the **framework sequence** (a phase anchor, the open turn, the encounter-draw loop, a surge chain, the enemy attack loop, the mulligan) is discarded; a frame representing **mandatory resolution already under way** (an effect frame, a skill test, an advance-reverse, a forced ordering run, an acknowledge, a mid-resolution prompt) completes. When no completing frame remains, a `ScenarioEnd` frame emits the `GameEnd` timing point, its forced abilities drain above it, and the apply boundary — the only place that holds the `ScenarioRegistry` — pops the frame and runs `apply_resolution` exactly once.

The Ghoul Priest is why the rule has to be shaped this way. Act 01110's objective is Forced, and its ruling is explicit: *"The **Objective** ability is mandatory, it will trigger as soon as you defeat the Ghoul Priest, before any 'After you defeat an enemy' reactions can be used."* The engine already gets that ordering right — `queue_event` pushes the reaction window first and the forced ability's frame above it, so the forced advance resolves first. But advancing act 3 reaches a resolution point, so by the time that window would open the scenario has ended and Roland Banks' after-defeat reaction must not be offered at all. At the moment of the latch the stack reads, bottom to top: the Investigation anchor, the open turn, the Fight's action-resolution frame, the skill test, the queued reaction window, and the effect frame doing the advancing. The window that must be cancelled sits **below** the ability that ends the scenario and **above** the skill-test teardown that must still run. No rule based on stack position can separate those, which is the whole reason the gate is positional-independent.

## Considered options

**Clearing the stack at latch time** was the first proposal and is wrong: it destroys the in-flight skill test mid-teardown, and for a terminal act it would destroy the lead investigator's own R1/R2 resolution choice printed on the advancing card's back (#593).

**Inserting a `ScenarioEnd` frame above the topmost framework frame** was rejected once the Ghoul Priest trace was written out. A single insertion index assumes the stack partitions into "in-flight above, framework below"; it does not, and every index that preserves the skill-test teardown also lets Roland's window open.

**Pre-empting on the next drive iteration** was rejected because a drive iteration is one *step*, not one ability: it would bisect a `Seq` effect or a multi-step advance-reverse.

## Consequences

The classification is an exhaustive `match` on `Continuation`, so a new frame variant cannot default into either bucket without a decision — the same discipline `is_phase_anchor` and `awaits_input` already use.

The `ScenarioEnd` frame's presence *is* the once-only finalize marker, so no second "already finalized" flag is needed on `GameState`; `state.resolution` answers "did the scenario end", and the frame answers "has the ending finished".

The game-end acknowledge still prompts. Cover Up 01007's *"Forced – When the game ends, if there are any clues on Cover Up: You suffer 1 mental trauma"* is campaign state the player has to watch land, and suppressing the acknowledge to keep the pass synchronous — the fallback #566 originally proposed — buys nothing once the ending has a frame to span the apply boundary on.

`ActionResolution` is classified as completing: an action whose attack of opportunity ended the scenario still finishes. This is a judgement call rather than a rules citation. Half-resolving an action already taken is harder to reason about than finishing it, and the victory-display scan reads final board state, so a completed action is the more predictable input to it.
