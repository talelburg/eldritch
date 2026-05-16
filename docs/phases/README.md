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
| 3 | Skill-test end-to-end | 🟡 in progress | [phase-3-skill-test-end-to-end.md](phase-3-skill-test-end-to-end.md) |
| 4 | Scenario plumbing | ⏳ planned | [phase-4-scenario-plumbing.md](phase-4-scenario-plumbing.md) |
| 5 | Server + persistence | 📐 architecture only | [phase-5-server-and-persistence.md](phase-5-server-and-persistence.md) |
| 6 | Web client v0 | 📐 architecture only | [phase-6-web-client-v0.md](phase-6-web-client-v0.md) |
| 7 | The Gathering | 📐 architecture only | [phase-7-the-gathering.md](phase-7-the-gathering.md) |
| 8 | Multiplayer + auth | 📐 architecture only | [phase-8-multiplayer-and-auth.md](phase-8-multiplayer-and-auth.md) |
| 9 | Campaign + Night of the Zealot | 📐 architecture only | [phase-9-campaign-and-night-of-the-zealot.md](phase-9-campaign-and-night-of-the-zealot.md) |
| 10 | Dunwich + iteration | 📐 architecture only | [phase-10-dunwich-and-iteration.md](phase-10-dunwich-and-iteration.md) |

**Status legend:**
- ✅ **closed** — milestone closed; docs are retrospective.
- 🟡 **in progress** — issues filed and being worked; doc has live status.
- ⏳ **planned** — issues filed but work not started; doc has issue list + dependency notes; ordering may be TBD.
- 📐 **architecture only** — no issues filed yet; doc captures the strategy-phase decisions and explicit scoping TBDs.

## Cross-cutting / unmilestoned work

Some issues don't belong to a single phase. They live unmilestoned with `p2-later` labels and get picked up when convenient:

- `#26`, `#27`, `#28` — test-harness ergonomics (rename adders, total-event-count macro, with-investigator-at convenience).
- `#31` — turn-order management cleanup (empty `turn_order` / `EndTurn` gracefully).
- `#42` — card-data-pipeline test coverage expansion.
- `#44` — DSL horror-soak / damage-redirect primitive (needed when soak-bearing cards land; tracked from PR-I).
- `#83` — `enemy_attack` cleanup (apply both damage and horror when one defeats).
- `#93` — split DSL types into a shared `card-dsl` crate (architectural improvement; not phase-gated).

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

- **When a phase issue closes:** update its line in the phase doc's Issues section (✅ closed) and add any design decision it cemented to the Decisions section.
- **When a phase milestone closes:** flip the phase's Status to ✅, trim Open Questions to closed-out items only, and the doc becomes a retrospective.
- **When the next phase starts:** flip its Status from ⏳ to 🟡, add Shape B if not already there.
- **PR convention:** doc updates happen in the same PR that touches the underlying issue when natural; standalone "update phase docs" PRs are fine too.
