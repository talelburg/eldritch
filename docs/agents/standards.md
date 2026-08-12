# Coding standards

What this repo expects of the code itself. The `code-review` skill's **Standards** axis reads this file; so should anything else asking "how is code written here?"

Standards live in exactly one place each. Most already have a home in `CLAUDE.md` — this file points at those rather than restating them, because a standard copied twice is a standard that drifts. Only the rules with no other home are defined below.

## Documented elsewhere

| Standard | Where |
|---|---|
| Validate-first / mutate-second handler contract, and the `apply_via` rollback that backstops it | `CLAUDE.md` → Architecture → Event-sourced state |
| Test layering, the `TestGame` builder, the event-assertion macros | `CLAUDE.md` → Architecture → Test layering |
| Never hand-edit `crates/cards/src/generated/cards.rs` | `CLAUDE.md` → Architecture → Card-data pipeline |
| Don't add DSL primitives speculatively — wait for two hand-written cards wanting the same pattern | `CLAUDE.md` → Architecture → Hybrid card-effect DSL |
| Card text and rules citation policy (ArkhamDB first, always read the FAQ, snapshot as fallback) | `CLAUDE.md` → Architecture → Domain knowledge |
| Running local checks with CI's exact strict flags | `CLAUDE.md` → Commands |
| Domain vocabulary — use the glossary's words in names and test titles | `CONTEXT.md` |

## Defined here

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

### Let an absent derive speak for itself

When a type deliberately omits a derive — `PartialEq` on `GameState`, say, because comparing large trees is expensive — don't add a comment explaining the omission. If the reason matters, it belongs in the commit message or PR description, where archaeology will find it.

**Why:** comments about code that isn't there go stale, can't be checked, and imply a positive assertion where there is only a default.
