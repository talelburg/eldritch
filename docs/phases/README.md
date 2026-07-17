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
| 8 | Multiplayer + auth | 📐 architecture only | [phase-8-multiplayer-and-auth.md](phase-8-multiplayer-and-auth.md) |
| 9 | Campaign + Night of the Zealot | 📐 architecture only | [phase-9-campaign-and-night-of-the-zealot.md](phase-9-campaign-and-night-of-the-zealot.md) |
| 10 | Dunwich + iteration | 📐 architecture only | [phase-10-dunwich-and-iteration.md](phase-10-dunwich-and-iteration.md) |

**Status legend:**
- ✅ **closed** — milestone closed; docs are retrospective.
- 🟡 **in progress** — issues filed and being worked; doc has live status.
- ⏳ **planned** — issues filed but work not started; doc has issue list + dependency notes; ordering may be TBD.
- 📐 **architecture only** — no issues filed yet; doc captures the strategy-phase decisions and explicit scoping TBDs.

## Cross-cutting / unmilestoned work

Some issues don't belong to a single phase. They live unmilestoned (mostly `p2-later`) and get picked up when convenient — the authoritative list is the [open unmilestoned issues query](https://github.com/talelburg/eldritch/issues?q=is%3Aissue+is%3Aopen+no%3Amilestone). Examples of the standing kind: `#31` (empty-`turn_order` guard), `#117` (event-keyed trigger index), `#119` (damage/horror dispatcher consolidation), `#174` (replay snapshots — build only when profiling demands it). The 2026-07-17 audit filed a batch more (#564–#593, spanning engine/pipeline/server/web/infra).

## Template

Each phase doc follows this shape:

1. **Status** — closed / in progress / planned / architecture only, with a date stamp.
2. **Goal** — the milestone's one-liner.
3. **Issues** — every issue in the milestone, linked, with current state.
4. **Ordering** — Shape-B-style ordered plan, or "TBD" with rationale.
5. **Decisions made** — settled architecture or design choices that shape later work.
6. **Open questions** — what's not yet scoped.
7. **Dependencies** — which prior phases this needs.
8. **What "done" looks like** — the concrete demonstration that closes the phase.

## Maintaining these docs

This section is the authoritative spec for the phase-doc update step of the PR procedure (CLAUDE.md step 6).

- **When a PR closing a phase issue is ready to merge — and ONLY then** (as the branch's final commit, after CI is green, so the entry reflects the actually-shipping state with the PR # known and review fixes folded in):
  - Move the closing issue's row to the phase doc's **Closed** table and bump any open/closed counts.
  - Flip the corresponding **Ordering / Arc** row to `✅ PR #N`.
  - Remove any **Open question** the PR settled.
  - Add a **Decisions made** entry *only* for choices load-bearing for future PRs. The test: *would a future PR-author choose differently without this entry?* If they'd discover the same fact by grepping the code or reading a doc-comment / `TODO(#NNN)`, leave it out. Lean toward skipping — 3–4 well-chosen entries beat a comprehensive list.
- **Never put phase-doc edits in earlier commits** of the same branch (churn + drift), and don't batch them into unrelated PRs.
- **When a phase milestone closes:** flip the phase's Status to ✅ (here and in the doc), trim Open Questions to closed-out items only, and the doc becomes a retrospective.
- **When the next phase starts:** flip its Status from ⏳/📐 to 🟡, add the ordered plan if not already there.
