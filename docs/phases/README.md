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

Some issues don't belong to a single phase. They live unmilestoned (mostly `p2-later`) and get picked up when convenient — the authoritative list is the [open unmilestoned issues query](https://github.com/talelburg/eldritch/issues?q=is%3Aissue+is%3Aopen+no%3Amilestone). Examples of the standing kind: `#31` (empty-`turn_order` guard), `#117` (event-keyed trigger index), `#119` (damage/horror dispatcher consolidation), `#174` (replay snapshots — build only when profiling demands it). The 2026-07-17 audit ([record](../audits/2026-07-17-audit.md)) filed a batch more (#564–#593, spanning engine/pipeline/server/web/infra).

The [Chapter 1 forward-compatibility gap register](../audits/2026-08-14-chapter-1-forward-compatibility.md) (#620, PR #621) is the other standing cross-cutting record, and unlike the audit it filed **no** issues — deliberately. It measures the DSL and engine against the whole Chapter 1 snapshot and sorts what it finds into four buckets; only the five bucket-4 entries ask for a decision, and each is written to become a decision ticket later rather than to be actioned now. Read it before any work that would widen the `CardRegistry`, the `GameState` partition, or the scenario-scoped action log. Its loose ends were three doc-drift items and a Rules Reference too old to answer Chapter 1 rules questions. The stale Rules Reference is **closed** (#624, PR #625): `data/rules-reference/rules/` now carries ArkhamDB's replica, which covers all of Chapter 1. The doc-drift items still have no issue.

The **2026-08-16 rules-conformance audits** (#626, PR #627) are the third standing cross-cutting record, and the first pass over the engine that could check it against the *printed* rules rather than against memory of them — five documents under `docs/audits/`, one per surface (skill tests; abilities and triggers; actions and phases; card play and zones; enemies and damage), 41 findings, each anchored to a verbatim quote from a named file under `data/rules-reference/rules/` and to a `file:line`. Unlike the register they **did** file issues: #628–#657. Three are decision tickets, each one missing abstraction wearing several hats — **#628** (no live modifier layer), **#629** (trigger dispatch and ability addressing), **#630** (automatic success/failure) — and each wants its own grilling session and an ADR before code moves. **#631** was the one `p0-blocker`: committed cards were stored as hand positions and the engine's own ST.2→ST.3 window could move them, a reachable panic with shipped Core cards — **closed** (PR #658), by giving committed cards the Rules Reference's own **limbo** state (`CONTEXT.md`) so they leave the hand at ST.2 and no hand index survives the commit. **#632** (enemies-and-damage, finding 1) is the first `p1-next` off the pile — **closed** (PR #659): a defeated enemy now goes to the encounter discard, or to its owner's discard pile if it is a weakness, so a defeated Ghoul Minion returns on the encounter-deck reshuffle and a defeated basic weakness stays in its bearer's campaign deck. Ownership is answered only for the unambiguous solo case; the bearer model itself stays with #654. **#633** (the same audit's finding 2) followed it — **closed** (PR #660): an enemy relocated by a card effect never ran the engage-on-arrival check, so agenda 01107 could walk a non-Hunter Ghoul onto the lone investigator and it would stand there ready and unengaged forever. Fixed structurally, with a `relocate_enemy` funnel every mover goes through rather than the one missing call. Each audit also carries a **Checked and found sound** section (read it before re-deriving that the engine is right about something) and an **Uncertain** section recording what the vendored sources genuinely do not settle, with a note on what would settle each — those are deliberately unticketed. One finding was a correction to the 2026-08-14 register rather than a divergence (#657).

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
  - Existing **Decisions made** sections in closed phase docs stay where they are; they are not retro-migrated.
- **Never put phase-doc edits in earlier commits** of the same branch (churn + drift), and don't batch them into unrelated PRs.
- **When a phase milestone closes:** flip the phase's Status to ✅ (here and in the doc), trim Open Questions to closed-out items only, and the doc becomes a retrospective.
- **When the next phase starts:** flip its Status from ⏳/📐 to 🟡, add the ordered plan if not already there.
