# A native resolves in program order

Dynamite Blast 01024 prints *"Choose either your location or a connecting location. Deal 3 damage to each enemy and to each investigator at the chosen location."* Its native damages each enemy, then calls `deal_harm` once for each investigator. `deal_harm` doesn't place anything: it pushes a deal frame, with its soak prompt, and returns. The stack is last in, first out, so two deals pushed one after the other resolve in reverse. The card worked only because its author knew that and folded the deals in reverse id order.

A native is ordinary Rust, read top to bottom, but its exits ran on two clocks. A direct write (placing doom, revealing a location) landed when called. A push (`deal_harm`, `move_investigator`, `resolve_encounter_card`, a spawned enemy's engagement tie) left its work for after the native returned. So "take harm, then X" wrote X before the harm was assigned. #1189 refused a direct write made while a pushed prompt was pending. That caught the bug, but it left every author tracking both clocks and every new exit waiting for someone to decide which clock it ran on.

**So a native resolves in the order its author wrote it.** Its exits run at once until one leaves work pending. From then on, every exit it calls is held as an op on a `NativeTail` frame directly beneath the frame that started the deferral, and the drive loop runs the ops in call order once that frame has resolved. An op that leaves work pending again re-parks the rest of the tail beneath that work. Dynamite Blast's investigators are now dealt in a forward loop.

## What starts the deferral

**Every frame the native's exits push starts it, except the coordinator (`EmitEvent`) frame of a caller-owned condition** ([ADR 0008](0008-a-triggering-condition-resolves-inside-its-own-sequence.md)'s `ConditionResolution::Caller`). It doesn't matter whether the frame is a prompt yet: a deal, a move's arrival, an encounter card's frames, a spawn's engagement tie and an `Elimination` all start it.

A caller-owned condition's frame holds only its `when`/`at`/`after` cells, because the caller has already made the change. That is Dynamite Blast's enemy loop: a lethal `deal_damage_to_enemy` removes the enemy and queues `EnemyDefeated`, and the damage to the next enemy must not sink beneath that window. `glossary/Effects.md`: *"All aspects of an effect have timing priority over all "after..." triggering conditions that might arise as a consequence of that effect."* Roland Banks 01001's *"After you defeat an enemy"* reaction is offered once the blast is finished.

The rule survives ADR 0008's migration without revisiting. Once `EnemyDefeated` becomes coordinator-owned, its frame's resolve step performs the defeat, which is effect work, so it will start the deferral correctly.

**The pushed frames are the ones above the stack's length when the native was entered**, lifted by one if an exit latched the scenario's ending. `end_scenario` inserts the ending frame at the bottom of the stack ([ADR 0004](0004-a-latched-resolution-cancels-opportunities-not-resolutions.md)). Without the offset, the native's own parent frame would count as pushed. The tail driver marks the stack the same way before each run of ops.

**The tail sits above any caller-owned frames queued before the deferral.** Once Dynamite Blast defers, the stack from the top is: the first investigator's deal, then the tail holding the other deals, then the enemies' `EnemyDefeated` frames. Windows queued before the deferral keep the order they had. The `after` cell a deal opens inside its own frame still resolves before the tail's next op. Reaching that cell is the second place `Effects.md`'s rule applies, and it is #640's question.

## Reads after the deferral are refused

After the deferral, the board the native can see is not the board the pending frames will leave. **A `state()` read after the deferral makes the native `Rejected`**, and the apply's snapshot rollback undoes everything. So does an exit that returns a value (`take_encounter_top`, `discard_from_play`, `spend_uses`, `move_horror`, and the rest): a placeholder would lie to the caller, and a continuation closure can't ride a frame that serializes. `cards()` stays allowed, because the registry doesn't change during an apply.

**A deferred exit returns `Done`, meaning queued**, as [ADR 0003](0003-emitting-a-timing-point-queues-abilities.md) defines it for an emit. Op preconditions are not checked up front. An op that turns out `Rejected` when the tail runs it rejects the whole apply, which rolls back cleanly. Choose-before-write is unchanged: a suspend after the deferral is a suspend after a write.

The tail frame is Driven and completes under a latched ending, because it is mandatory resolution already under way. It also counts as a queued ability for the phase-anchor assertion. It holds the native's tag, its ops and a cursor, and serializes like every other frame.

## What it changes in the corpus

**Dynamite Blast** is the only native that called an exit after one that starts a deferral. **They're Getting Out 01107**'s act-3 bullet, *"Each investigator that has not resigned is defeated and suffers 1 physical trauma."*, changes as intended. If Roland holds Cover Up 01007, his defeat pushes an `Elimination` frame for its game-end Forced. The physical trauma now lands after that elimination, steps 1–6 included, where it used to land before Cover Up's mental trauma.

**The Barrier 01109** reads its own writes and is unchanged. Its reverse is *"Reveal the Parlor. Put the set-aside Lita Chantler into play in the Parlor. Spawn the set-aside Ghoul Priest in the Hallway."* The reveal pushes nothing. Putting Lita, an asset, into play at a location logs `CardPutIntoPlayAtLocation` and pushes no frame at all. Only the Ghoul Priest's spawn can push one, an engagement tie, and it is the last call.

**The Necronomicon 01009 is a known limit.** Its native moves horror to Daisy, then reads the board and may discard the card. If the move defeated her while she owned an in-play weakness with a game-end ability, the elimination would push an `Elimination` frame, and the read after it would reject the action. That can't happen in the corpus. Cover Up is the only game-end weakness, and it is Roland's.

## Considered options

**(B) Natives return a plan instead of calling exits.** It is purer, but it forbids every read of the native's own writes (The Barrier reveals the Parlor and then puts Lita into play in it), and it rewrites every native.

**(C) Keep #1189's refusal.** It catches the bug, but it leaves two clocks. The next card that prints "take harm, then X" would have to be written in reverse order or split into two natives.
