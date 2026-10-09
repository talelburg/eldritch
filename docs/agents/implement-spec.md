# Running `/implement-spec` here

`/implement-spec` builds a whole spec on one **integration branch**. Implementer subagents work the ticket graph's frontier in parallel worktrees, and merger subagents fold each ticket in. This file is the repo's adaptation of that flow: where the upstream skill and `CLAUDE.md`'s PR procedure meet, and the failures from the #925, #938 and #962 runs that shaped it. Read it when `/implement-spec` starts, before the first dispatch.

## The PR

The integration branch is the PR's branch. Open it as `CLAUDE.md` step 1 says: an empty `<scope>: start #NN` commit, a push, then `gh pr create --draft`, with a `Closes #NN` line for the spec and for every ticket. That replaces upstream's "open the draft after the first merge". Every merger push then runs the full CI on the draft, which is how the CI-only wasm jobs get covered during the run.

## Explore first, then put every open question to the user in one batch

Run exploration to completion **before** dispatching any implementer. Exploration checks every claim the spec and tickets make about the current code against the code itself. The usual suspects:
- "existing suites pass without edits": grep the construction sites of any variant that gains a field, and the tests that pin the behaviour being changed;
- "order unchanged": find the test that pins today's order;
- a user story that no ticket covers.

Every contradiction goes to the user as **one batch of gates**, each with options and a recommendation. Only tickets with no open question are dispatched.

Use a general-purpose dispatch for exploration and tell it to stay read-only in the repo. The `Explore` type can't write files, and its notes have to land on disk for implementers to read. Notes go in a directory outside the repo, for example `/tmp/eldritch-<spec>-notes/`. Approved decisions go there too, and as a comment on the ticket they affect.

**Why:** #962 surfaced five decisions about 20 minutes into the run, after the user had left, and the run then sat for two days. Three of those five were visible from the code while the spec was being written: a promised "option order unchanged" that was false, two "passes without edits" claims a grep would have refuted, and an uncovered user story. Exploration also ran alongside the implementers, and one implementer spent 165k tokens before stopping at a gate the exploration later answered. #938 lost about 30 minutes to two smaller shape errors of the same kind.

## Dispatching implementers

Each implementer's prompt carries:
- the "does all the work itself and delegates to no subagent of its own" line, and the worktree shell rules, both from `CLAUDE.md` → Agent skills;
- an instruction to reset onto the integration branch first. Worktree isolation bases on `main`, and 10 of 10 #938 implementers started there;
- context pointers to the spec, its ticket, and the notes directory, rather than restated content;
- the gate rule. An implementer stops and reports at a gate rather than deciding it, with one exception: a test that pins exactly the behaviour an approved decision changes is pre-approved for editing. In #962 an implementer had to choose between stopping on such a test and editing it unapproved.

## Merging

A merger works on a **detached HEAD** and pushes by refspec:

```sh
git fetch origin
git switch --detach origin/<integration-branch>
git merge --no-ff origin/<ticket-branch>
git push origin HEAD:<integration-branch>
```

It never checks out the integration branch by name. The orchestrator's own checkout is the only one holding that ref, and it only fast-forwards: `git merge --ff-only origin/<integration-branch>`.

**Why:** in #925 the mergers ran `git checkout -B <integration-branch> origin/...` inside their worktrees. That moved the ref the main checkout was on, so the orchestrator's `--ff-only` failed against what looked like local changes, and three stale worktrees needed a forced removal.

## Review, and closing out

After the last merge the review runs on the integration branch, and its findings go to the user before `gh pr ready`, exactly as `CLAUDE.md` step 3 says for any PR. A single fix subagent may apply the fixes first.

Once the PR merges, delete the ticket branches on the remote (`git push origin --delete <branch>`) and run `scripts/prune-agent-worktrees.sh --apply` for the local leftovers. Upstream's cleanup removes worktrees only, and the #925 ticket branches were still on the remote two runs later.
