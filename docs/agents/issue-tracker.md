# Issue tracker: GitHub

Issues and specs for this repo live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: write the body to a file with the Write tool, then `gh issue create --title "..." --body-file <file>`. The same goes for `gh issue comment`, `gh issue edit`, `gh pr create` and `gh pr edit`. Use the file even for a short body: a worktree-isolated subagent's shell guard refuses a heredoc or `$(...)` body as "too complex to verify", and the #938 and #962 implementers lost turns to that refusal.
- **Read an issue**: `gh issue view <number> --json title,body,labels,milestone,comments`, narrowing with `--jq` as needed. `--comments` alone prints only the comments when output isn't a terminal, so an issue with none comes back empty.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on an issue**: `gh issue comment <number> --body-file <file>`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number>`, after any closing note goes in with `gh issue comment --body-file`

Infer the repo from `git remote -v` — `gh` does this automatically when run inside a clone.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag.)_

When set to `yes`, PRs run through the same labels and states as issues, using the `gh pr` equivalents:

- **Read a PR**: `gh pr view <number> --json title,body,labels,comments,reviews` and `gh pr diff <number>` for the diff.
- **List external PRs for triage**: `gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments` then keep only `authorAssociation` of `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, or `NONE` (drop `OWNER`/`MEMBER`/`COLLABORATOR`).
- **Comment / label / close**: `gh pr comment`, `gh pr edit --add-label`/`--remove-label`, `gh pr close`.

GitHub shares one number space across issues and PRs, so a bare `#42` may be either — resolve with `gh pr view 42` and fall back to `gh issue view 42`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --json title,body,labels,milestone,comments`.

## HTML reports

The HTML reports from `/improve-codebase-architecture` and similar surveys don't go in the repo or in an issue body; GitHub can't attach a file to an issue. They go on the orphan branch `architecture-reviews`, which is never merged and is served by GitHub Pages at <https://talelburg.github.io/eldritch/>. Add the file to that branch, link it from `index.html`, and link the report and its tracking issue to each other, as #921 does.

**Read a report verbatim:** `git fetch origin architecture-reviews && git show origin/architecture-reviews:<file>.html`, or `curl -s <url>`. `WebFetch` returns a summary, which drops the line references and counts a report is read for.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single issue with **child** issues as tickets.

- **Map**: a single issue labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body. `gh issue create --label wayfinder:map`.
- **Child ticket**: an issue linked to the map as a GitHub sub-issue (`gh api` on the sub-issues endpoint). Where sub-issues aren't enabled, add the child to a task list in the map body and put `Part of #<map>` at the top of the child body. Labels: `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed, the ticket is assigned to the driving dev.
- **Blocking**: GitHub's **native issue dependencies** — the canonical, UI-visible representation. Add an edge with `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`, where `<blocker-db-id>` is the blocker's numeric **database id** (`gh api repos/<owner>/<repo>/issues/<n> --jq .id`, _not_ the `#number` or `node_id`). GitHub reports `issue_dependencies_summary.blocked_by` (open blockers only — the live gate). Where dependencies aren't available, fall back to a `Blocked by: #<n>, #<n>` line at the top of the child body. A ticket is unblocked when every blocker is closed.
- **Frontier query**: list the map's open children (`gh issue list --state open`, scoped to the map's sub-issues / task list), drop any with an open blocker (`issue_dependencies_summary.blocked_by > 0`, or an open issue in the `Blocked by` line) or an assignee; first in map order wins.
- **Claim**: `gh issue edit <n> --add-assignee @me` — the session's first write.
- **Resolve**: `gh issue comment <n> --body "<answer>"`, then `gh issue close <n>`, then append a context pointer (gist + link) to the map's Decisions-so-far.

## Repo-specific conventions

Eldritch already has an issue workflow; these skills slot into it rather than replacing it.

- **Milestones** track the phase arc: `phase-0-foundations` → `phase-10-dunwich-and-iteration`.
  Each has a plan doc at `docs/phases/phase-N-<slug>.md`; `docs/phases/README.md` indexes the arc.
- **Priority labels**: `p0-blocker` / `p1-next` / `p2-later`.
- **Category labels**: `engine` / `card` / `scenario` / `infra` / `test` (also in use: `ui`,
  `docs`, `feature`, `core-set`, `dunwich-legacy`).
- **State labels** predating this setup: `ready`, `in-progress`, `blocked`, `needs-design`.
  How they relate to the triage roles is in `docs/agents/triage-labels.md`.
- **PRs** squash-merge; subjects follow `scope: description`; the body ends with `Closes #NN.`
  Merging is gated on explicit user approval — see the PR procedure in `CLAUDE.md`.
