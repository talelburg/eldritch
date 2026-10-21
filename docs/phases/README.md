# Phase plans

Eldritch is broken into 12 phases, milestone-tracked on GitHub. Each one's plan, status, decisions, and open questions lives in its own file in this directory. The active phase has full detail; future phases capture what's been decided and what's still open; a closed phase's live items have been moved out to their new homes by the phase-close checklist below.

## Why this exists

The plan-of-record for the project is GitHub (milestones, issues, labels). These docs sit on top: they capture **cross-issue context** that doesn't fit naturally in any single issue body — the *ordering* between issues in a phase, the *design decisions* made along the way that shape later work, the *open questions* the phase will need to settle.

When starting work on a new issue, read the relevant phase doc first. It's faster than re-deriving the context from chat history or git log.

## The 12 phases

| Phase | Title | Status | Doc |
|---|---|---|---|
| 0 | Foundations | ✅ closed | [phase-0-foundations.md](phase-0-foundations.md) |
| 1 | Engine bones | ✅ closed | [phase-1-engine-bones.md](phase-1-engine-bones.md) |
| 2 | Card data + DSL | ✅ closed | [phase-2-card-data-and-dsl.md](phase-2-card-data-and-dsl.md) |
| 3 | Skill-test end-to-end | ✅ closed | [phase-3-skill-test-end-to-end.md](phase-3-skill-test-end-to-end.md) |
| 4 | Scenario plumbing | ✅ closed | [phase-4-scenario-plumbing.md](phase-4-scenario-plumbing.md) |
| 5 | Server + persistence | ✅ closed | [phase-5-server-and-persistence.md](phase-5-server-and-persistence.md) |
| 6 | Web client v0 | ✅ closed | [phase-6-web-client-v0.md](phase-6-web-client-v0.md) |
| 7 | The Gathering | ✅ closed | [phase-7-the-gathering.md](phase-7-the-gathering.md) |
| 7.5 | Investigator breadth | 🟡 in progress | [phase-7.5-investigator-breadth.md](phase-7.5-investigator-breadth.md) |
| 8 | Multiplayer + auth | ⏳ planned | [phase-8-multiplayer-and-auth.md](phase-8-multiplayer-and-auth.md) |
| 9 | Campaign + Night of the Zealot | ⏳ planned | [phase-9-campaign-and-night-of-the-zealot.md](phase-9-campaign-and-night-of-the-zealot.md) |
| 10 | Dunwich + iteration | ⏳ planned | [phase-10-dunwich-and-iteration.md](phase-10-dunwich-and-iteration.md) |

**7.5 is a decimal on purpose.** Renumbering 8, 9 and 10 — plus every
cross-reference in this directory and the milestone names on GitHub — costs more
than the decimal does.

**Status legend:**
- ✅ **closed** — milestone closed; the phase-close checklist has moved the doc's live items out.
- 🟡 **in progress** — issues filed and being worked; doc has live status.
- ⏳ **planned** — issues filed but work not started; doc has issue list + dependency notes; ordering may be TBD.
- 📐 **architecture only** — no issues filed yet; doc captures the strategy-phase decisions and explicit scoping TBDs.

## Cross-cutting / unmilestoned work

Some issues don't belong to a single phase. They live unmilestoned (mostly `p2-later`) and get picked up when convenient — the authoritative list is the [open unmilestoned issues query](https://github.com/talelburg/eldritch/issues?q=is%3Aissue+is%3Aopen+no%3Amilestone). Examples of the standing kind: `#117` (event-keyed trigger index), `#174` (replay snapshots — build only when profiling demands it).

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

This section is the authoritative spec for the phase-doc edit in the PR procedure (CLAUDE.md, step 2).

- **A PR closing a phase issue edits the phase doc alongside its code**, in the same commit or any later one on the branch. The draft PR is opened before the work, so the PR # is already known. If a CI or review fix changes what ships, that fix's commit updates the doc too. These are the only edits it makes:
  - Move the closing issue's row to the phase doc's **Closed** table and bump any open/closed counts.
  - Flip the corresponding **Ordering / Arc** row to `✅ PR #N`.
  - Remove any **Open question** the PR settled.
  - Add an **Open question** the PR raised, or a row for a follow-up issue it filed.
- **Anything else is a PR retro, and a PR retro is not written.** How the PR went, why it was designed that way, what review changed, what was tried and dropped: that is history, and git and the PR already keep it. The reasoning goes in the PR description; a decision that passes the three-part test below goes in an ADR; a fact a future author needs at the code goes in a doc-comment. A phase doc is read before picking up an issue, and every PR retro is something that reader has to wade past — phase 7's "The arcs behind it" grew to roughly 650 lines of them.
- **Do not add a "Decisions made" entry.** Design decisions live in `docs/adr/` now. If the PR made a choice that is (a) hard to reverse, (b) surprising without context, and (c) the result of a real trade-off, write it up as an ADR in the same commit. All three must hold — if a future PR-author would discover the same fact by grepping the code or reading a doc-comment / `TODO(#NNN)`, there's no ADR to write. Most PRs need none.
  - Existing **Decisions made** sections in closed phase docs stay where they are; they are not retro-migrated ([ADR 0001](../adr/0001-workflow-runs-on-mattpocock-skills.md) drew that scope line).
- **Don't batch phase-doc edits into unrelated PRs.**
- **When a phase milestone closes:** work the phase's sweep issue through [the phase-close checklist](#the-phase-close-checklist) below.
- **When the next phase starts:** flip its Status from ⏳/📐 to 🟡, add the ordered plan if not already there, and file a `[docs] Phase N close sweep` issue on its milestone. The milestone then can't close until the sweep has been done. Phase 7 closed without its sweep, because nothing but memory asked for one at the busiest moment of the phase.

### The phase-close checklist

The sweep issue's PR works through these six items, and its description lists every edit, tracker edits included. Every item is a judgement, not a lint: an ADR may name a gone symbol because removing it *is* the decision, a live note can sit beside a dead one, and an issue body is overtaken by the code or it isn't. That is why no script runs the list. The evidence under each item is the drift #1143 found at 7.5's close, after phase 7 had closed without a sweep.

1. **The closing phase's doc.** Every live item moves to a new home, and Status goes to ✅, here and in the doc.
   - An open row goes to a milestone, or to standing unmilestoned work (above).
   - A settled open question goes into an ADR if it passes the three-part test above. If the code shows the answer, the code is the record. Otherwise the question is removed: the PR that settled it holds the reasoning.
   - An unsettled open question goes to the Open questions of the phase that will decide it, or failing that into a `needs-design` issue.
   - No PR retro and no phase retrospective is written.

   **Why:** 7.5's doc still listed three open, unmilestoned rows (#1059, #1099, #1100) as the phase closed. Phase 7's "The arcs behind it", 647 of the doc's 1,177 lines, mentioned 11 open issues that appear nowhere else in the doc, and carried live notes such as #427/#429 waiting on #728.
2. **Tracker vs docs.**
   - Each milestone's state matches the table above: `gh api 'repos/talelburg/eldritch/milestones?state=all' --jq '.[] | "\(.title) \(.state) \(.open_issues)"'`. A phase's milestone closes once its last issue does.
   - Each open phase's doc mentions every open issue on its milestone (`gh issue list --milestone <title> --state open`), in whatever format the doc uses. The check runs one way: closed issues aren't required, because ticket children don't belong in a phase doc.

   **Why:** the phase-6 milestone stayed open with 0 open issues while the table marked phase 6 ✅. GLOSSARY counted 11 milestones when there were 12. Phase 9's doc left out nine open issues on its milestone.
3. **ADRs vs code.**
   - Every backticked symbol in an ADR still exists under `crates/`, unless its removal *is* the ADR's decision (ADR 0016's deleted `test_fixtures/`). Any other mention of a gone symbol is rewritten in present tense, under [`writing.md`](../agents/writing.md)'s folding rule: it is a fold, not a supersession.
   - A plan that has shipped is written in present tense.
   - The footer reads `Folded: #a, #b.`, issue numbers only.

   **Why:** ADRs named nine symbols that no longer existed, among them `begin_event_play` (0002), `request_resolution` (0004 and 0012) and `DefeatCause` (0013, by then `EliminationCause::CardAbility`). ADR 0013 described its plan, "#808 then #809", in the future tense after both had shipped. Footers ran to 234 words (0009).
4. **Agent docs.** `writing.md`'s "Is anything here now history?" is applied to `CLAUDE.md`, `docs/agents/` and `GLOSSARY.md`. A rule whose transitional state ended during the phase is deleted, or it outlives the thing it described. A count that goes stale is replaced by a pointer to whatever holds the list.

   **Why:** `issue-tracker.md` pointed at the closed #921 as its example of a review's tracking issue. `domain.md`'s example ADR conflict was the upstream template's "ADR-0007 (event-sourced orders)", and this repo's ADR 0007 is about something else. `triage-labels.md` named `wontfix` as in use when no issue carried it.
5. **Code prose.** Comments under `crates/` are checked against [`standards.md`](../agents/standards.md)'s "Code prose" rule. Each check below is one command from the repo root. Read every hit rather than bulk-fixing: a pattern can't tell history from a verbatim quote or present-tense game state.
   - An issue number appears only as `TODO(#N):`, or as `Regression for #N:` in test code. The first command lists every other `#N` in a comment. The second lists `Regression for #N:` outside a `tests/` directory, and each hit must sit in a `#[cfg(test)]` module. The third lists a `TODO(#N)` that is backticked or has no colon after it.

     ```sh
     rg -nP '//.*(?<![\w&#])(?<!TODO\()(?<!Regression for )#\d+\b' crates/
     rg -n 'Regression for #' crates/ --glob '!**/tests/**'
     rg -nP '//.*(`TODO\(#\d+\)|TODO\(#\d+\)(?!:))' crates/
     ```
   - No history markers. "since #", "formerly" and past-tense "used to" are violations, while "used to" in its functional sense ("can be used to hold") is a false hit. "previously" and "no longer" are allowed only for present-tense game state or inside a verbatim quote.

     ```sh
     rg -niP '//.*(since #|used to|formerly|previously|no longer)' crates/
     ```
   - Every `TODO(#N)` names an open issue. This prints the numbers that don't:

     ```sh
     comm -23 <(rg -oN --no-filename 'TODO\(#(\d+)\)' -r '$1' crates/ | sort -u) <(gh issue list --state open --limit 1000 --json number --jq '.[].number' | sort -u)
     ```

   **Why:** at main 86332221, comments under `crates/` carried 2,220 issue references in neither form, 87 history markers, and a `TODO` on the closed #921. #1142 fixed them with a one-off scan (PR #1158), and the commands above are its core patterns.
6. **Labels.** No open issue carries a contradictory pair of state labels: `blocked` with `ready-for-agent` or `ready-for-human`, or `needs-design` with `ready-for-human`. None carries `blocked` without naming a blocker that is still live. An issue body the code has overtaken is rewritten to match the code, or confirmed current. Passing `--label` twice lists the issues carrying both:

   ```sh
   gh issue list --state open --label blocked --json number,title,labels
   gh issue list --state open --label needs-design --label ready-for-human
   ```

   **Why:** #588 and #642 were `blocked` and `ready-for-agent` at once, and #670, #679 and #759 were `needs-design` and `ready-for-human`. #65, #609 and #492 were `blocked` on a blocker that had closed or a trigger that had fired. Twelve issue bodies needed checking against the code, among them #290, whose proposed remedy contradicted GLOSSARY's Limbo entry.
