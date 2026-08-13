# An in-progress play lives on its continuation frame

The Rules Reference gives a played card a transitional state. At Appendix I step 3 "the card commences being played", and only at the completion of step 4 is it "regarded as played (**and placed in play, or in its owner's discard pile if it's an event**)". Between those two points the card is in no zone at all — "In Play and Out of Play" lists hand, decks, discard piles, the victory display and set-aside/removed cards as out of play, and a card mid-play is in none of them, nor in any player's play area. The engine modelled that state in a single global slot, `GameState.pending_played_event`. We moved it onto the `Continuation::PlayFromHand` frame instead, because **plays nest**: a non-fast event provokes an attack of opportunity while it is itself mid-play, and a Fast event played to cancel that attack overwrote the slot and erased the first card from the game (#604 — Dynamite Blast 01024 with a mid-AoO Dodge 01023, both Core Set, no panic and no error). The continuation stack already nests correctly — Dodge's frame sits above Dynamite Blast's and pops first — so carrying the card there makes the failure structurally impossible instead of guarded against.

The nesting is the whole reason, and it is invisible at every individual call site. A reader looking at `begin_event_play`, at `dispose_play_from_hand`, or at the asset enter-play path sees one play at a time and will conclude a global slot was fine.

## Considered options

**Parking the card's code and re-locating it at use time** — the fix #565 originally proposed, mirroring `play_fast_event`'s existing `hand.iter().position(|c| *c == code)` — was rejected as a symptom fix. It repairs the asset path's stale `hand_index` while leaving the event path's single slot intact, so #604 survives it; and it leaves the same discipline to be remembered independently at three sites (`ActionResume::PlayCard`, `Continuation::PlayFromHand`, `Continuation::SlotDiscard`), which is the condition that produced three instances of one bug in the first place.

**Making the slot a stack (`Vec<PlayInProgress>`)** was rejected as the smaller version of the same idea. It fixes nesting, but keeps a global side-channel whose entries every consumer must correlate with the frame they belong to ("which entry is mine?"), and that correlation is exactly what the continuation stack already expresses for free. The continuation-stack cleanup folded every other `*_pending` side-channel onto frames; this slot escaped it by an accident of timing, having been introduced by #364 (Dynamite Blast, 2026-06-18) the day before #345/#380 landed. It was never a deliberate exception.

**Minting a per-card hand-slot identity** was rejected as a new concept for no gain. A hand is a `Vec<CardCode>` and copies are fungible; nothing in the in-scope corpus distinguishes two copies of a card in hand.

## Consequences

`pending_played_event` is retired, and with it the `Effect::AttachSelfToLocation` special case: Barricade 01038 reads the nearest enclosing `PlayFromHand` frame, which is what "*this* card attaches itself" actually means.

Elimination gains a sweep it never had. `run_elimination_steps` drained `cards_in_play`, owned threat-area weaknesses, `hand`, `deck` and `discard`, and never looked at the in-progress slot — so a controller defeated by the attack of opportunity its own Dynamite Blast provoked ended with the card in the *drained* discard pile of a `Killed` investigator and an empty `removed_from_game`. Frame-carried makes the sweep a walk of that investigator's continuation frames, which is order-independent and so immune to the flush-versus-teardown ordering that caused it.

The state shape changes, and there is no wire or schema versioning yet (#583). Persisted games from before this change will not load. That is acceptable now and will not be later — it is an argument for #583 landing before real games exist, not against this shape.

This ADR covers only the **stale reference** half of the re-validation arc (#605) — what we store. The arc's other half, **stale verdict** (a scan-time conclusion acted on at fire time, #568), is deliberately not recorded here: moving a check from scan time to fire time is locally reversible, and the Rules Reference clause that governs it — "A triggered ability can only be initiated if its effect has the potential to change the game state, and its cost (if any) has the potential to be paid in full, taking active cost modifiers into account" — belongs in a doc-comment at the site that enforces it.
