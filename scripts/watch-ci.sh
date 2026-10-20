#!/usr/bin/env bash
#
# watch-ci.sh — wait for the `ci` workflow on a PR's head commit and report it.
#
# `gh pr checks <PR#> --watch` started straight after a push can return before
# the `ci` run has registered: the only check it sees is the Dependabot
# `auto-merge` job (skipped on every non-Dependabot PR), so it reports done
# against a run that hasn't started. This script resolves the PR's head SHA,
# waits for the `ci` run on exactly that SHA, then polls its jobs quietly.
#
# Straight after a push, `gh pr view` can still report the previous head, and
# the script would then watch the old commit's run and report it. So when the
# current branch is the PR's head branch and the PR's head is behind the local
# `HEAD`, it first waits for the PR to catch up (#1148: it once reported green
# for the previous commit, with "merge once green" approved). A PR head the
# local `HEAD` doesn't contain, such as the merge `gh pr update-branch` makes,
# is watched as it is, and so is the PR's head when run from anywhere else.
#
# It exits as soon as any job fails, without waiting for the rest, and prints
# one `name conclusion` line per job. A run cancelled because a newer commit
# was pushed is followed to the new head rather than reported as a failure.
# After the verdict it prints the PR's `mergeStateStatus`, and on `BEHIND`
# names `gh pr update-branch`: a Dependabot merge to main can leave a green PR
# that the merge then refuses.
# Run it in the background and wait for the completion notification.
#
# Usage:
#   scripts/watch-ci.sh <PR#>
#
# Exit status: 0 all jobs green, 1 a job failed or was cancelled, 2 usage error,
# the PR's head never caught up with the local HEAD, or no `ci` run appeared
# for the head SHA within the wait. The merge state never changes the exit
# status.
#
set -uo pipefail

usage() { sed -n '2,/^set -/p' "$0" | sed 's/^#\{0,1\} \{0,1\}//;$d'; }

PR="${1:-}"
case "$PR" in
  -h|--help) usage; exit 0 ;;
  ''|*[!0-9]*) usage >&2; exit 2 ;;
esac

POLL=15            # seconds between polls
PUSH_WAIT=120      # seconds to wait for a push to reach the PR's head
REGISTER_WAIT=300  # seconds to wait for the run to appear

head_sha() { gh pr view "$PR" --json headRefOid --jq .headRefOid; }

merge_state() {
  local state
  state=$(gh pr view "$PR" --json mergeStateStatus --jq .mergeStateStatus 2>/dev/null) || return 0
  if [ "$state" = "BEHIND" ]; then
    echo "merge state: BEHIND its base — bring it up to date with: gh pr update-branch $PR"
  else
    echo "merge state: $state"
  fi
}

SHA=$(head_sha) || exit 2

# Wait for the push to show up on the PR before choosing a run to watch.
HEAD_REF=$(gh pr view "$PR" --json headRefName --jq .headRefName) || exit 2
if [ "$(git branch --show-current 2>/dev/null)" = "$HEAD_REF" ]; then
  LOCAL=$(git rev-parse HEAD) || exit 2
  waited=0
  while [ "$SHA" != "$LOCAL" ] && git merge-base --is-ancestor "$SHA" "$LOCAL" 2>/dev/null; do
    if [ "$waited" -ge "$PUSH_WAIT" ]; then
      echo "PR #$PR head is still ${SHA:0:8}, behind local HEAD ${LOCAL:0:8}, after ${PUSH_WAIT}s — was HEAD pushed?" >&2
      exit 2
    fi
    [ "$waited" -eq 0 ] && echo "PR #$PR head ${SHA:0:8}: waiting for it to reach local HEAD ${LOCAL:0:8}"
    sleep "$POLL"; waited=$(( waited + POLL ))
    SHA=$(head_sha) || exit 2
  done
fi

while :; do
  echo "PR #$PR head ${SHA:0:8}: waiting for the ci run"
  RUN=""
  waited=0
  while [ -z "$RUN" ]; do
    RUN=$(gh run list --workflow ci.yml --commit "$SHA" --event pull_request \
            --limit 1 --json databaseId --jq '.[0].databaseId // empty')
    [ -n "$RUN" ] && break
    if [ "$waited" -ge "$REGISTER_WAIT" ]; then
      echo "no ci run for ${SHA:0:8} after ${REGISTER_WAIT}s — was the commit pushed?" >&2
      exit 2
    fi
    sleep "$POLL"; waited=$(( waited + POLL ))
  done
  echo "run $RUN: watching"

  while :; do
    state=$(gh run view "$RUN" --json status,jobs \
      --jq '[.status, ([.jobs[] | select(.conclusion == "failure")] | length)] | @tsv')
    status=${state%%$'\t'*}
    failed=${state##*$'\t'}
    if [ "$failed" -gt 0 ] || [ "$status" = "completed" ]; then
      break
    fi
    sleep "$POLL"
  done

  # ci.yml's concurrency group cancels a run when a newer commit is pushed to
  # the PR. That is not a failure: follow the PR to its new head.
  conclusion=$(gh run view "$RUN" --json conclusion --jq .conclusion)
  if [ "$failed" -eq 0 ] && [ "$conclusion" = "cancelled" ]; then
    NEW=$(head_sha) || exit 2
    if [ "$NEW" != "$SHA" ]; then
      echo "run $RUN superseded by a push (${NEW:0:8}); following it"
      SHA=$NEW
      continue
    fi
  fi
  break
done

gh run view "$RUN" --json jobs \
  --jq '.jobs[] | "\(.name) \(if .status == "completed" then .conclusion else .status end)"' |
  column -t
if [ "$failed" -gt 0 ] || [ "$conclusion" != "success" ]; then
  echo "ci FAILED — logs: gh run view $RUN --log-failed"
  merge_state
  exit 1
fi
echo "ci passed"
merge_state
