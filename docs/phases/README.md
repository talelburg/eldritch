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

The **2026-08-16 rules-conformance audits** (#626, PR #627) are the third standing cross-cutting record, and the first pass over the engine that could check it against the *printed* rules rather than against memory of them — five documents under `docs/audits/`, one per surface (skill tests; abilities and triggers; actions and phases; card play and zones; enemies and damage), 41 findings, each anchored to a verbatim quote from a named file under `data/rules-reference/rules/` and to a `file:line`. Unlike the register they **did** file issues: #628–#657. Three are decision tickets, each one missing abstraction wearing several hats — **#628** (no live modifier layer), **#629** (trigger dispatch and ability addressing), **#630** (automatic success/failure) — and each wants its own grilling session and an ADR before code moves. **#631** was the one `p0-blocker`: committed cards were stored as hand positions and the engine's own ST.2→ST.3 window could move them, a reachable panic with shipped Core cards — **closed** (PR #658), by giving committed cards the Rules Reference's own **limbo** state (`CONTEXT.md`) so they leave the hand at ST.2 and no hand index survives the commit. **#632** (enemies-and-damage, finding 1) is the first `p1-next` off the pile — **closed** (PR #659): a defeated enemy now goes to the encounter discard, or to its owner's discard pile if it is a weakness, so a defeated Ghoul Minion returns on the encounter-deck reshuffle and a defeated basic weakness stays in its bearer's campaign deck. Ownership is answered only for the unambiguous solo case; the bearer model itself stays with #654. **#633** (the same audit's finding 2) followed it — **closed** (PR #660): an enemy relocated by a card effect never ran the engage-on-arrival check, so agenda 01107 could walk a non-Hunter Ghoul onto the lone investigator and it would stand there ready and unengaged forever. Fixed structurally, with a `relocate_enemy` funnel every mover goes through rather than the one missing call. **#639** (the abilities-and-triggers audit, finding 2) is the next off the pile — **closed** (PR #666): the generic initiation gate ran on the play, reaction, and forced paths but never on `Trigger::Activated`, so First Aid 01019 spent an action and a supply healing an investigator with nothing to heal, and Old Book of Lore 01031 paid its exhaust to search an empty deck. The gate now sits in `check_activate_ability`, which the turn menu and fast-window enumerator both filter on, so the menu cannot offer an activation the validator would reject; `effect_can_change_state` gained the two arms whose no-op case is a property of the *target*, and target grounding filters ineligible investigators by the same predicate. A search is proven inert only against an empty deck — a fruitless *filter* still shuffles — and a filtered-to-empty candidate list at resolution **skips** its sub-effect rather than rejecting, so Medical Texts 01035 passing its test with nobody damaged does not unwind the chaos draw. The same rule one level down — a modal ability still offering a dead mode — is **#664**. **#638** (the actions-and-phases audit, finding 1) followed — **closed** (PR #667): Rules Reference p.10 Elimination *opens* with a step 0 the engine never implemented — *"For the purpose of resolving weakness cards, the game has ended for the eliminated investigator. Trigger any 'when the game ends' abilities on each weakness the eliminated investigator owns that is in play."* — so an eliminated Roland's Cover Up 01007 never dealt its mental trauma, in solo or in multiplayer. Step 0 needs a scan of its own rather than a narrowing of the existing `GameEnd` one, which skips non-`Active` investigators and fires every controlled card; and because a timing-point emit only *queues* (ADR 0003), the remaining steps ride a `Continuation::Elimination` frame so the abilities resolve while their cards are still in play. Elimination stays synchronous when no such weakness is in play, which is what keeps every other defeat's post-elimination bookkeeping unchanged. This **reverses** #567's acceptance criterion that a dead investigator's Cover Up must *not* fire, whose rationale read step 1 without the step 0 that exists to carve weaknesses out of it; #567's `RoundEnded` half stands. Resign will inherit step 0 for free once that action lands. **#637** (the same audit's finding 6) followed — **closed** (PR #668): the mulligan shuffled the rejected cards back into the deck *before* drawing their replacements, inverting the glossary's own order — *"These cards are set aside, and an equivalent number of cards are drawn and added to the player's starting hand. The set-aside cards are then shuffled back into the player's deck."* — so a card could be dealt straight back as its own replacement, which the rules forbid. The judgement call was where the step-8 weakness sweep over the redraw sits: **after** the set-aside cards return, which is both the glossary's ordering and the only one that leaves the sweep a deck to draw from — run while the mulliganed cards were still held out, a one-card deck emptied the hand outright. Each audit also carries a **Checked and found sound** section (read it before re-deriving that the engine is right about something) and an **Uncertain** section recording what the vendored sources genuinely do not settle, with a note on what would settle each — those are deliberately unticketed. One finding was a correction to the 2026-08-14 register rather than a divergence (#657).

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
