# Running `/implement-spec` here

`/implement-spec` builds a whole spec on one **integration branch**. Implementer subagents work the ticket graph's frontier in parallel worktrees, and each finished ticket is folded in, one at a time. This file is the repo's adaptation of that flow: where the upstream skill and `CLAUDE.md`'s PR procedure meet, and the failures from the #925, #938 and #962 runs that shaped it. Read it when `/implement-spec` starts, before the first dispatch.

## The PR

The integration branch is the PR's branch. Open it as `CLAUDE.md` step 1 says: an empty `<scope>: start #NN` commit, a push, then `gh pr create --draft`, with a `Closes #NN` line for the spec and for every ticket. That replaces upstream's "open the draft after the first merge". Every push to the integration branch then runs the full CI on the draft, which is how the CI-only `wasm-build` job gets covered during the run.

## Explore first, then put every open question to the user in one batch

Run exploration to completion **before** dispatching any implementer. Exploration checks every claim the spec and tickets make about the current code against the code itself: the claims [`to-spec.md`](to-spec.md) lists, which should already have been checked once before publishing. The usual suspects are claims to check, not promises to protect:
- "existing suites pass without edits": grep the construction sites of any variant that gains a field, and the tests that pin the behaviour being changed;
- "order unchanged": find the test that pins today's order;
- a user story that no ticket covers.

A test that pins incidental behaviour, prompt option order included, is edited by the change that moves it (`standards.md` → A test that pins incidental behaviour is not a contract). So a false "unchanged" makes a gate about the ticket's size, never one about keeping the test.

Every contradiction goes to the user as **one batch of gates**, each with options and a recommendation. Only tickets with no open question are dispatched.

Use a general-purpose dispatch for exploration and tell it to stay read-only in the repo. The `Explore` type can't write files, and its notes have to land on disk for implementers to read. Notes go in a directory outside the repo, for example `/tmp/eldritch-<spec>-notes/`. Approved decisions go there too, and as a comment on the ticket they affect.

**Why:** #962 surfaced five decisions about 20 minutes into the run, after the user had left, and the run then sat for two days. Three of those five were visible from the code while the spec was being written: a promised "option order unchanged" that was false, two "passes without edits" claims a grep would have refuted, and an uncovered user story. Exploration also ran alongside the implementers, and one implementer spent 165k tokens before stopping at a gate the exploration later answered. #938 lost about 30 minutes to two smaller shape errors of the same kind.

## Dispatching implementers

**Before dispatching a ticket, confirm each of its blockers is merged**: after `git fetch origin`, `git merge-base --is-ancestor origin/<blocker-branch> origin/<integration-branch>` exits 0 for every blocker. In PR #1023, #1022 started on a branch missing 1017b's files and had to be restarted.

Each implementer's prompt carries:
- the "does all the work itself and delegates to no subagent of its own" line, and the worktree shell rules, both from `CLAUDE.md` → Agent skills;
- its setup command, spelled out: `git fetch origin`, then `git switch --detach origin/<integration-branch>`. Worktree isolation bases on `main`, and 10 of 10 #938 implementers started there. Its pushes go to `git push origin HEAD:refs/heads/<ticket-branch>`. Named in prose, "reset onto the integration branch" became a `git reset --hard` that the auto-mode classifier refused in four PR #1057 implementers, and a named `git checkout -b` collided on resume;
- context pointers to the spec, its ticket, and the notes directory, rather than restated content;
- the gate rule. An implementer stops and reports at a gate rather than deciding it, with one exception: a test that pins exactly the behaviour an approved decision changes is pre-approved for editing. In #962 an implementer had to choose between stopping on such a test and editing it unapproved;
- the check rule. Before handing back, it merges the integration branch's current tip into its work (`git fetch origin`, then `git merge origin/<integration-branch>`) and runs `scripts/ci-local.sh` in the foreground with the Bash `timeout` at `600000`, reading the exit status as the verdict (`CLAUDE.md` → Commands). It doesn't pass `--all`: the pushed CI on the draft covers the wasm jobs;
- the handoff. It lists the test helpers it added, with their files. Before the review, the orchestrator folds duplicates across tickets into one: PR #1023's review found `assert_handler_rejects` in four files and `card(session, code)` in five, and the fix-up took 253 tool calls.

## Merging

**One merge is in flight at a time.** The implementer has already merged the integration tip and run `ci-local.sh` (Dispatching, above), so its ticket branch usually contains the tip. The orchestrator then merges it inline, as a fast-forward of its own checkout:

```sh
git fetch origin
git merge --ff-only origin/<ticket-branch>
git push origin <integration-branch>
```

A ticket that merged meanwhile makes the fast-forward fail. **Only then is a merger subagent dispatched.** It works on a **detached HEAD** and pushes by refspec:

```sh
git fetch origin
git switch --detach origin/<integration-branch>
git merge --no-ff origin/<ticket-branch>
git push origin HEAD:<integration-branch>
```

It runs `ci-local.sh` (in the foreground, as above) only after resolving a conflict. A clean merge goes straight to the push, and the pushed CI tests it. The merger never checks out the integration branch by name. The orchestrator's own checkout is the only one holding that ref, and it only fast-forwards, here with `git merge --ff-only origin/<integration-branch>` once the merger has pushed.

**Why:** mergers running in parallel, each re-testing between its merge and its push, had 28 pushes rejected across four runs. PR #1114's 12 parallel mergers took 84 minutes where PR #1129's 7 serial ones took 4.4, and one PR #1080 merger gave up and pushed an untested merge. The runs that merged conflict-free tickets inline (PRs #1030 and #1039) had no rejections. And in #925 the mergers ran `git checkout -B <integration-branch> origin/...` inside their worktrees. That moved the ref the main checkout was on, so the orchestrator's `--ff-only` failed against what looked like local changes, and three stale worktrees needed a forced removal.

## Review, and closing out

After the last merge the review runs on the integration branch, and its findings go to the user before `gh pr ready`, exactly as `CLAUDE.md` step 3 says for any PR. A single fix subagent may apply the fixes first.

Once the PR merges, run `scripts/prune-agent-worktrees.sh --merged-pr <PR#>`, read its dry run, then run it again with `--apply`. After that, delete the ticket branches on the remote (`git push origin --delete <branch>`). Upstream's cleanup removes worktrees only, and the #925 ticket branches were still on the remote two runs later.

**The flag is what clears this run's leftovers.** Without it the script keeps a worktree whose lock names a live pid, and here that pid is the orchestrating session itself. It also keeps a branch whose tip is on no remote, which is what a squash-merged ticket tip becomes once its remote branch is deleted. Every run before #1148 force-removed its worktrees by hand, and in PR #1096 that removed 14 where the user had approved 7.
