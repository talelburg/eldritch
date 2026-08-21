# Coding standards

What this repo expects of the code itself. The `code-review` skill's **Standards** axis reads this file; so should anything else asking "how is code written here?"

Standards live in exactly one place each. A few have a home elsewhere — this file points at those rather than restating them, because a standard copied twice is a standard that drifts. Everything else is defined below.

## Documented elsewhere

| Standard | Where |
|---|---|
| Validate-first / mutate-second handler contract, and the `apply_via` rollback that backstops it | [`architecture.md`](architecture.md) → Event-sourced state |
| Card text and rules citation policy (read the vendored text locally, always read the FAQ, never fetch) | `CLAUDE.md` → Cite card text and rules from the vendored sources |
| Running local checks with CI's exact strict flags | `CLAUDE.md` → Commands |
| Domain vocabulary — use the glossary's words in names and test titles | `CONTEXT.md` |
| How the docs are written, not the code — the file to read before adding a rule to `CLAUDE.md` or writing an ADR, since both have a bar this file does not state | [`docs/agents/writing.md`](writing.md) |

## Defined here

### Match a card's declared `EventTiming` to its quoted trigger word

Every card module opens with the printed text verbatim. That block is the evidence: **read the declared `EventTiming` against the trigger word the module itself quotes**, and which cell each word names is **Timing cell** in `CONTEXT.md`. A mismatch in the corpus is a bug.

**Say in prose which cell the ability resolves in, and why.** Write *"the `at` cell of the `EnemyDefeated` condition"* in the paragraph under the quoted text — not a description of the engine window the ability happens to ride. The form is a **bold inline lead-in opening *"Cell: …"***, not a `# Cell` rustdoc heading, so the paragraph sits in the header's flow next to the quoted text rather than opening a section a reader can scroll past. A module declaring two cells writes two such paragraphs, each naming which ability it is for (`the_barrier`, `cover_up`).

A card declaring `EventTiming::When` on a triggering condition whose resolve step has not been migrated to coordinator-owned resolution is **rejected**; migrating that condition is the fix, not retagging the card. See [ADR 0008](../adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md) and `ConditionResolution::Caller` (`crates/game-core/src/engine/dispatch/emit.rs`) for the per-condition migration and its cost. **No card is licensed to declare one cell and resolve in another**, and none can be.

**Why, and why there is no automated check:** the trigger word is not mechanically derivable from the printed text — *"if … would …"* is when-tier while a bare *"if"* on a settled state is at-tier, a tiering `CONTEXT.md` argues from the rules rather than quoting. A parser would encode one reading and then be trusted as though it had checked. The six mis-tags #694 audited were not a gap in the evidence: every one of those modules quoted its own trigger word directly above the wrong enum. The failure was an unassigned reading, and this section is where it is assigned.

### Don't add DSL primitives speculatively

A new `Effect` (or `EventPattern`) variant waits until **two or more hand-written cards want the same pattern**. Until then the card gets a Rust impl or a card-local native tag. *"Place N doom on the current agenda"* is the worked case: Ancient Evils 01166 and Silver Twilight Acolyte 01102 each carried a byte-identical `<code>:place-doom` tag until #716 made the second consumer graduate it to `Effect::PlaceDoomOnCurrentAgenda`.

**Why:** a variant added for one card fixes the DSL's shape around a sample of one, and the kernel then has to keep resolving it. One consumer is a card-local detail; two is a pattern.

### Never hand-edit `crates/cards/src/generated/cards.rs`

It is pipeline output and carries a header comment saying so. Change the impl, or the snapshot and `PACK_FILES`, then re-run `cargo run -p card-data-pipeline` — see [`architecture.md`](architecture.md) → Card-data pipeline.

**Why:** the next pipeline run silently reverts the edit, and nothing between now and then tells you.

### Test layering

In order of importance:

1. **Card tests** — per-card in `crates/cards/src/impls/<name>.rs`; **each card needs at least one.**
2. **Engine unit tests** — `crates/game-core/src/engine/mod.rs` + per-module `#[cfg(test)]`. Use the `TestGame` builder (`.with_phase(…).with_investigator(…).with_active_investigator(…).build()`, with `test_investigator(id)` / `test_location(id, name)` / `test_enemy(id, name)` fixtures) and the **event-assertion macros** `assert_event!` / `assert_no_event!` / `assert_event_count!` / `assert_event_sequence!` (order-insensitive by default; `_sequence` for in-order subsequence). Use `assert_eq!` on the events slice only when you need exact contiguous order.
3. **Integration tests** — `crates/cards/tests/`; each file is its own cargo binary/process, so it can `install(cards::REGISTRY)` without colliding. The right home for anything needing real card metadata + abilities, which `game-core` can't reach by crate direction. Pattern: `crates/cards/tests/play_card.rs`.

`game-core::test_support` is unconditionally `pub` (no feature flag).

### Stub deferred functionality with a TODO that names the issue

When a variant, handler, or effect can't be implemented yet because the supporting infrastructure doesn't exist, return `EngineOutcome::Rejected` (or the analogous rejection) with a message in the form `TODO(#NN): <variant> needs <thing> (lands with #MM)`. Where several variants share a blocker, share a small helper rather than copy-pasting the prose.

Reserve `unreachable!()` for invariant violations — corruption, not unimplemented work. A `todo!()` panic and a silent no-op are both wrong: the first crashes on a path the engine should reject cleanly, the second pretends the feature works.

**Why:** each new piece of infrastructure depends on later infrastructure, so the gaps are numerous and long-lived. A loud rejection carrying a precise pointer keeps every gap visible and greppable.

### Never silently approximate a card

When a card can't be honestly expressed in the current DSL, there are two acceptable moves: ship the parts the DSL *can* express and document the gap in a `# Module gap` section in the card's module, or leave the card unimplemented and note the dependency. File the missing primitive as a follow-up issue either way.

Approximating is the one thing that isn't allowed. The playability gate would then hand a player a card the simulator resolves incorrectly — a wrong answer presented as a right one.

**Why:** caught twice in Phase 2. Holy Rosary's `sanity: 2` was read as +2 max sanity when it is horror-soak capacity, and Magnifying Glass's "+1 [intellect] while investigating" was flattened to a permanent +1 intellect, which over-applies to every other intellect test. Both were caught by the user, not by tooling.

### Verify card data against the snapshot before implementing

Before writing a card impl — or a card issue's body — confirm the card's code, name, and text against `data/arkhamdb-snapshot/pack/`. When the plan, the issue, or your recollection disagrees with the snapshot, **the snapshot wins**.

**Why:** during Phase-2 issue creation, 4 of 5 planned card codes were wrong — 01054 was Leo De Luca rather than Holy Rosary, 01045 was Burglary rather than Hyperawareness, 01039 was Deduction rather than Working a Hunch. Each would have produced a confidently-implemented wrong card. A single grep catches all of them.

### Prefer no example to a wrong one

When citing a card by name to illustrate a pattern — in a comment, doc, issue, or PR body — verify it first, per the citation policy above. If a quick check doesn't surface a card that genuinely exemplifies the pattern, write the generic description instead. "Card-derived investigate effects" beats naming a card that turns out not to do that.

Treat card citations in existing comments and docs as unverified until checked, particularly ones an agent wrote.

**Why:** a confabulated "Magnifying Glass's *Action: Investigate*" reached both a memory file and a code comment before being caught. Wrong examples are worse than absent ones — they propagate into reviewers' mental models and become facts the project has to unlearn.

### Emit a timing point in tail position; put post-emit work on a frame

`queue_event` (and `queue_forced_triggers` beneath it) **queues** an ability — it pushes a continuation frame — and returns. It does not resolve anything, so a returned `EngineOutcome::Done` means *queued*, not *happened*. Any work a handler does after the emit is pushed **above** the abilities it just queued and therefore runs **first**.

So a call site with post-emit work arms its own resume point *before* emitting, and emits as the last thing it does: re-park a phase anchor at a new resume (`enemy_phase_end`, `upkeep_phase_end`), flag the frame it is already riding (`end_turn`'s `InvestigatorTurn { ending: true }`), or push a dedicated frame for the tail (`move_primary_effect`'s `MoveEnter`). Inspecting the returned outcome is not a substitute for any of that — `if !matches!(out, Done) { … }` and `debug_assert!(matches!(out, Done))` both pass in the ordinary single-ability case, while the ability sits unresolved on the stack.

A `debug_assert` in the `drive` loop backstops the class: no queued ability frame may sit beneath a phase anchor. See `docs/adr/0003-emitting-a-timing-point-queues-abilities.md`, and **Queued ability** in `CONTEXT.md`.

**Why:** four call sites believed the emit resolved, and each carried a comment asserting a loud guard that had not existed since the effect-frame migration. The worst pushed the Upkeep phase anchor over agenda 01107's forced Ghoul movement, stranding it at the bottom of the stack — that ability never fired in a real game of The Gathering (#569).

### Insert a fn above another by matching its doc block, not its signature

Rust `///` comments attach to the *next* item, so an `Edit` whose `old_string` matches only the existing function's signature line drops the new function **between** that function's doc block and its `fn` — silently re-attaching the existing doc to the new function and leaving the existing one undocumented. Either include the whole `///` block in `old_string` and place the new function cleanly before it, or insert after an unambiguous boundary (the prior function's closing `}` plus a blank line) and then check that every `fn` still carries its own doc.

**Why:** nothing is broken, only misattributed, so `RUSTDOCFLAGS="-D warnings" cargo doc` says nothing — this is caught by eye or not at all. Review caught the same mistake twice: `drive_fast_window` inserted above `enumerate_fast_plays` (#476), and `run_mythos_draws` above `anchor_on_child_pop` (#482).

### Let an absent derive speak for itself

When a type deliberately omits a derive — `PartialEq` on `GameState`, say, because comparing large trees is expensive — don't add a comment explaining the omission. If the reason matters, it belongs in the commit message or PR description, where archaeology will find it.

**Why:** comments about code that isn't there go stale, can't be checked, and imply a positive assertion where there is only a default.
