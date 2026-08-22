# Phase plans

Eldritch is broken into 11 phases, milestone-tracked on GitHub. Each one's plan, status, decisions, and open questions lives in its own file in this directory. Closed phases get short retrospectives; the active phase has full detail; future phases capture what's been decided and what's still open.

## Why this exists

The plan-of-record for the project is GitHub (milestones, issues, labels). These docs sit on top: they capture **cross-issue context** that doesn't fit naturally in any single issue body — the *ordering* between issues in a phase, the *design decisions* made along the way that shape later work, the *open questions* the phase will need to settle.

When starting work on a new issue, read the relevant phase doc first. It's faster than re-deriving the context from chat history or git log.

## The 11 phases

| Phase | Title | Status | Doc |
|---|---|---|---|
| 0 | Foundations | ✅ closed | [phase-0-foundations.md](phase-0-foundations.md) |
| 1 | Engine bones | ✅ closed | [phase-1-engine-bones.md](phase-1-engine-bones.md) |
| 2 | Card data + DSL | ✅ closed | [phase-2-card-data-and-dsl.md](phase-2-card-data-and-dsl.md) |
| 3 | Skill-test end-to-end | ✅ closed | [phase-3-skill-test-end-to-end.md](phase-3-skill-test-end-to-end.md) |
| 4 | Scenario plumbing | ✅ closed | [phase-4-scenario-plumbing.md](phase-4-scenario-plumbing.md) |
| 5 | Server + persistence | ✅ closed | [phase-5-server-and-persistence.md](phase-5-server-and-persistence.md) |
| 6 | Web client v0 | ✅ closed | [phase-6-web-client-v0.md](phase-6-web-client-v0.md) |
| 7 | The Gathering | 🟡 in progress | [phase-7-the-gathering.md](phase-7-the-gathering.md) |
| 8 | Multiplayer + auth | ⏳ planned | [phase-8-multiplayer-and-auth.md](phase-8-multiplayer-and-auth.md) |
| 9 | Campaign + Night of the Zealot | ⏳ planned | [phase-9-campaign-and-night-of-the-zealot.md](phase-9-campaign-and-night-of-the-zealot.md) |
| 10 | Dunwich + iteration | ⏳ planned | [phase-10-dunwich-and-iteration.md](phase-10-dunwich-and-iteration.md) |

**Status legend:**
- ✅ **closed** — milestone closed; docs are retrospective.
- 🟡 **in progress** — issues filed and being worked; doc has live status.
- ⏳ **planned** — issues filed but work not started; doc has issue list + dependency notes; ordering may be TBD.
- 📐 **architecture only** — no issues filed yet; doc captures the strategy-phase decisions and explicit scoping TBDs.

## Cross-cutting / unmilestoned work

Some issues don't belong to a single phase. They live unmilestoned (mostly `p2-later`) and get picked up when convenient — the authoritative list is the [open unmilestoned issues query](https://github.com/talelburg/eldritch/issues?q=is%3Aissue+is%3Aopen+no%3Amilestone). Examples of the standing kind: `#31` (empty-`turn_order` guard), `#117` (event-keyed trigger index), `#119` (damage/horror dispatcher consolidation), `#174` (replay snapshots — build only when profiling demands it).

Three **standing cross-cutting records** live under [`docs/audits/`](../audits/). Each is a point-in-time reading that still constrains work today; each section below says what it covers and what it asks of you. Per-finding detail lives in the issues each record filed, and the reasoning behind a fix lives in its PR — don't look for either here.

### The 2026-07-17 repository audit

[Record](../audits/2026-07-17-audit.md). A full-repo pass over code quality, correctness, security, infrastructure, tech debt, and tracker hygiene, by nine parallel domain auditors over the whole workspace. It filed #564–#593, spanning engine, pipeline, server, web, and infra, and those issues carry the verified `file:line` evidence; the doc keeps what they don't — method, the overall verdict, and what was checked and found *sound*.

Read it before concluding that a seam is untested territory: the verdict names four clusters (suspension/elimination interleavings, pipeline text-parsing heuristics, the server's missing identity/versioning/limits, and the web transport's error classification), and work landing in one of them is landing on known ground.

### The Chapter 1 forward-compatibility gap register

[Record](../audits/2026-08-14-chapter-1-forward-compatibility.md). It measures the DSL and engine against the whole Chapter 1 snapshot — not the corpus — and sorts what it finds into four buckets: expressible today, wants a new DSL primitive, wants a new engine capability, or contradicts a current architectural assumption. Unlike the other records it filed **no** issues, deliberately: only the five bucket-4 entries ask for a decision, and each is written to become a decision ticket when its phase arrives rather than to be actioned now.

**Read it before any work that would widen the `CardRegistry`, the `GameState` partition, the home for hidden information, or the scenario-scoped action log** — those are the assumptions bucket 4 contradicts, and a widening that ignores the register buys a shape the rest of Chapter 1 will not fit. One correction is outstanding: #657 records that the register's nested-skill-tests entry describes an engine that stacks a frame, where it in fact rejects, so read that entry against the code.

### The 2026-08-16 rules-conformance audits

Five documents, one per surface: [skill tests](../audits/2026-08-16-rules-conformance-skill-tests.md), [abilities and triggers](../audits/2026-08-16-rules-conformance-abilities-and-triggers.md), [actions and phases](../audits/2026-08-16-rules-conformance-actions-and-phases.md), [card play and zones](../audits/2026-08-16-rules-conformance-card-play-and-zones.md), and [enemies and damage](../audits/2026-08-16-rules-conformance-enemies-and-damage.md). They are the first pass over the engine that could check it against the *printed* rules rather than against memory of them, the Rules Reference having been vendored shortly beforehand. 41 findings, each anchored to a verbatim quote from a named file under `data/rules-reference/rules/` and to a `file:line`, filed as #628–#657.

Three of those were decision tickets rather than fixes — #628 (no live modifier layer), #629 (trigger dispatch and ability addressing), #630 (automatic success/failure) — each one missing abstraction wearing several hats. All three are now resolved, each through a grilling session, an ADR and a sliced migration: [ADR 0005](../adr/0005-a-modified-quantity-is-recalculated-at-every-read.md) and [0006](../adr/0006-a-modifier-declares-who-it-reaches.md) for #628, [ADR 0007](../adr/0007-a-tests-determination-is-resolved-above-the-fold.md) for #630, and [ADR 0008](../adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md) plus the #693 epic for #629. The ADRs, not this page, are where their conclusions are binding.

Each audit also carries two sections worth knowing about. **Checked and found sound** records what was verified as correct — read it before re-deriving that the engine is right about something. **Uncertain** records what the vendored sources genuinely do not settle, with a note on what would settle each; those are deliberately unticketed, and a question that lands in one is a question to put to the maintainer rather than to answer from the code. **Read an Uncertain entry before assuming it is still open** — an entry is struck in place, not deleted, once a source settles it, so the section is a mix of live questions and closed ones with their answers attached. Two were closed by #756 out of the official-FAQ sweep (#750), both in the engine's favour: retaliate's placement relative to ST.8 in the skill-tests audit, and when *"after you successfully investigate"* triggers in the abilities-and-triggers audit.

### Tracker conventions (established 2026-07-17)

- **Milestone = phase deferral.** An issue milestoned to a later phase is deferred to it; an issue milestoned to the *current* phase is part of finishing that phase.
- **Priority labels order the current queue.** `p0-blocker` / `p1-next` / `p2-later` are load-bearing only on unmilestoned and current-phase issues — that's the knock-out order. On later-phase-milestoned issues, priority is merely intra-milestone ordering.
- **Cross-issue dependencies get a "Relationships" comment** on the issue (types: *Sibling* / *Blocked-by* / *Coordinate-with* / *Enables* / *Amplified-by*), so implementers see coupling without spelunking. GitHub's mention backlinks make one-sided comments discoverable from both ends, but prefer noting the edge on whichever issue an implementer is likely to pick up first.

## Template

Each phase doc follows this shape:

1. **Status** — closed / in progress / planned / architecture only, with a date stamp.
2. **Goal** — the milestone's one-liner.
3. **Issues** — every issue in the milestone, linked, with current state.
4. **Ordering** — Shape-B-style ordered plan, or "TBD" with rationale.
5. **Decisions made** — *closed phases only.* New design decisions go to `docs/adr/` instead; see "Maintaining these docs" below.
6. **Open questions** — what's not yet scoped. Also the **fog gauge**: a phase whose open questions are architectural and unresolved is a `/wayfinder` phase; one whose questions are only ordering and scope is a `/grill-with-docs` phase.
7. **Dependencies** — which prior phases this needs.
8. **What "done" looks like** — the concrete demonstration that closes the phase.

## Maintaining these docs

This section is the authoritative spec for the phase-doc update step of the PR procedure (CLAUDE.md step 6).

- **When a PR closing a phase issue is ready to merge — and ONLY then** (as the branch's final commit, after CI is green, so the entry reflects the actually-shipping state with the PR # known and review fixes folded in):
  - Move the closing issue's row to the phase doc's **Closed** table and bump any open/closed counts.
  - Flip the corresponding **Ordering / Arc** row to `✅ PR #N`.
  - Remove any **Open question** the PR settled.
  - **Do not add a "Decisions made" entry.** Design decisions live in `docs/adr/` now. If the PR made a choice that is (a) hard to reverse, (b) surprising without context, and (c) the result of a real trade-off, write it up as an ADR in the same commit. All three must hold — if a future PR-author would discover the same fact by grepping the code or reading a doc-comment / `TODO(#NNN)`, there's no ADR to write. Most PRs need none.
  - Existing **Decisions made** sections in closed phase docs stay where they are; they are not retro-migrated ([ADR 0001](../adr/0001-workflow-runs-on-mattpocock-skills.md) drew that scope line).
- **Never put phase-doc edits in earlier commits** of the same branch (churn + drift), and don't batch them into unrelated PRs.
- **When a phase milestone closes:** flip the phase's Status to ✅ (here and in the doc), trim Open Questions to closed-out items only, and the doc becomes a retrospective. Sweep the agent-facing docs in the same pass, against the checklist in [`docs/agents/writing.md`](../agents/writing.md) — a rule whose transitional state ended during the phase is deleted here or it outlives the thing it described.
- **When the next phase starts:** flip its Status from ⏳/📐 to 🟡, add the ordered plan if not already there.
