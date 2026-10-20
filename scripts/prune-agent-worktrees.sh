#!/usr/bin/env bash
#
# prune-agent-worktrees.sh — clear out what worktree-isolated subagents and
# merged PRs leave behind.
#
# A worktree-isolated subagent that exits without cleaning up leaves its
# worktree under .claude/worktrees/agent-*, often still locked by a process
# that no longer exists (a locked worktree needs `git worktree remove -f -f`),
# plus a `worktree-agent-*` branch. Squash-merged PRs leave local branches
# that `git branch --merged` never reports, because the squash commit is not
# their tip.
#
# What is removed, and why each is safe:
#
#   * an agent worktree that is clean, whose HEAD is on some branch or remote,
#     and that is either unlocked or locked by a dead pid. A dirty one, or one whose lock names a live process or no pid at
#     all, is reported and left alone.
#   * a `worktree-agent-*` branch that no worktree has checked out and whose
#     tip is already on a remote, so nothing unpushed is lost.
#   * a branch whose upstream is gone after `git fetch --prune` and whose PR
#     merged. That is what `gh pr merge --delete-branch` leaves locally. A
#     branch with a gone upstream and no merged PR (a PR closed unmerged, or a
#     remote deleted by hand) may hold the only copy of its work, so it is
#     reported and kept.
#
# Every other branch is listed with its reason for being kept.
#
# After an /implement-spec run, pass `--merged-pr <N>`, the integration PR,
# once it has merged. The rules above keep that run's leftovers: each
# worktree's lock names a live pid, the orchestrating session itself, and once
# the ticket branches are deleted on the remote, a squash-merged tip is on no
# branch or remote. So with `--merged-pr`, one more thing is safe:
#
#   * a clean worktree, or a branch nothing has checked out, whose tip is an
#     ancestor of the PR's head (`refs/pull/<N>/head`, which GitHub keeps after
#     the merge) and not already on origin/main. Every commit on it is in the
#     PR, so a worktree's lock is released even when the lock holder is alive.
#     A tip already on origin/main carries nothing of the PR's; the rules above
#     decide it, because it may belong to another session's live subagent.
#
# Usage:
#   scripts/prune-agent-worktrees.sh                  # dry run: print what would go
#                                                     # (it still runs `git fetch --prune`,
#                                                     # which only syncs remote-tracking refs)
#   scripts/prune-agent-worktrees.sh --apply          # remove it
#   scripts/prune-agent-worktrees.sh --merged-pr <N>  # also what merged PR #N holds (dry run)
#   scripts/prune-agent-worktrees.sh --merged-pr <N> --apply
#
set -uo pipefail

APPLY=0
MERGED_PR=""
while [ $# -gt 0 ]; do
  case "$1" in
    --apply) APPLY=1; shift ;;
    --merged-pr)
      MERGED_PR="${2:-}"
      [[ "$MERGED_PR" =~ ^[0-9]+$ ]] || { echo "--merged-pr needs a PR number" >&2; exit 2; }
      shift 2 ;;
    -h|--help) sed -n '2,/^set -/p' "$0" | sed 's/^#\{0,1\} \{0,1\}//;$d'; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# The main checkout, even when run from inside a worktree: the common git dir
# is the main checkout's .git.
COMMON=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || {
  echo "not inside a git repository" >&2
  exit 2
}
ROOT=$(dirname "$COMMON")
[ -d "$ROOT/.git" ] || {
  echo "cannot locate the main checkout from $COMMON" >&2
  exit 2
}
cd "$ROOT" || exit 2

act() {
  if [ "$APPLY" -eq 1 ]; then "$@"; else echo "    would run: $*"; fi
}

git fetch --quiet --prune origin || echo "!! fetch failed; upstream state may be stale" >&2

PR_HEAD=""
if [ -n "$MERGED_PR" ]; then
  state=$(gh pr view "$MERGED_PR" --json state --jq .state) || exit 2
  if [ "$state" != MERGED ]; then
    echo "PR #$MERGED_PR is $state, not MERGED" >&2
    exit 2
  fi
  git fetch --quiet origin "refs/pull/$MERGED_PR/head" || {
    echo "cannot fetch refs/pull/$MERGED_PR/head" >&2
    exit 2
  }
  PR_HEAD=$(git rev-parse FETCH_HEAD) || exit 2
  echo "merged PR #$MERGED_PR head: ${PR_HEAD:0:8}"
fi

# Is commit $1 in the merged PR's head, and not already on origin/main?
in_merged_pr() {
  [ -n "$PR_HEAD" ] &&
    git merge-base --is-ancestor "$1" "$PR_HEAD" &&
    ! git merge-base --is-ancestor "$1" origin/main
}

# ---------------------------------------------------------------- worktrees

echo "agent worktrees:"
found=0
# Worktrees this run removes, or would: their branches stop being checked out,
# so the branch pass below must not count them, in a dry run or not.
declare -a REMOVED=()
wt=""
while IFS= read -r line; do
  case "$line" in
    "worktree "*) wt=${line#worktree }; lock="" ; locked=0 ;;
    "locked"*)    locked=1; lock=${line#locked} ;;
    "")
      # Porcelain ends each record with a blank line, and the `echo` below
      # adds one for a git that omits the last; read each record once.
      [ -n "$wt" ] || continue
      rec=$wt; wt=""
      [[ "$rec" == "$ROOT/.claude/worktrees/agent-"* ]] || continue
      found=1
      name=${rec#"$ROOT"/}
      if [ -n "$(git -C "$rec" status --porcelain 2>/dev/null)" ]; then
        echo "  keep    $name — uncommitted changes"; continue
      fi
      head=$(git -C "$rec" rev-parse HEAD)
      if in_merged_pr "$head"; then
        REMOVED+=("$rec")
        if [ "$locked" -eq 1 ]; then
          echo "  remove  $name — tip is in merged PR #$MERGED_PR; releasing its lock${lock:+:$lock}"
          act git worktree remove -f -f "$rec"
        else
          echo "  remove  $name — clean, tip is in merged PR #$MERGED_PR"
          act git worktree remove "$rec"
        fi
        continue
      fi
      if [ -z "$(git for-each-ref --contains "$head" refs/heads refs/remotes)" ]; then
        echo "  keep    $name — HEAD is on no branch or remote"; continue
      fi
      if [ "$locked" -eq 1 ]; then
        pid=$(grep -oE 'pid[^0-9]*[0-9]+' <<<"$lock" | grep -oE '[0-9]+$')
        if [ -z "$pid" ]; then
          echo "  keep    $name — locked with no pid to check:$lock"; continue
        fi
        if kill -0 "$pid" 2>/dev/null; then
          echo "  keep    $name — locked by live pid $pid"; continue
        fi
        echo "  remove  $name — lock owner pid $pid has exited"
        REMOVED+=("$rec")
        act git worktree remove -f -f "$rec"
      else
        echo "  remove  $name — clean, unlocked"
        REMOVED+=("$rec")
        act git worktree remove "$rec"
      fi ;;
  esac
done < <(git worktree list --porcelain; echo)
[ "$found" -eq 0 ] && echo "  (none)"
[ "$APPLY" -eq 1 ] && git worktree prune

# ----------------------------------------------------------------- branches

CURRENT=$(git branch --show-current)
# Branches still checked out in some worktree, not counting the worktrees
# removed above (or that a dry run would remove).
CHECKED_OUT=$(git worktree list --porcelain | awk -v removed="$(printf '%s\n' "${REMOVED[@]}")" '
  BEGIN { n = split(removed, r, "\n"); for (i = 1; i <= n; i++) gone[r[i]] = 1 }
  /^worktree / { wt = substr($0, 10) }
  /^branch refs\/heads\// && !(wt in gone) { print substr($0, 19) }')

echo "local branches:"
while IFS=$'\t' read -r b up track; do
  [ "$b" = main ] && continue
  if grep -qxF "$b" <<<"$CHECKED_OUT" || [ "$b" = "$CURRENT" ]; then
    echo "  keep    $b — checked out"; continue
  fi
  if in_merged_pr "$b"; then
    echo "  delete  $b — tip is in merged PR #$MERGED_PR"
    act git branch -D "$b"
  elif [ "$track" = "[gone]" ]; then
    if [ -n "$(gh pr list --head "$b" --state merged --limit 1 --json number --jq '.[].number' 2>/dev/null)" ]; then
      echo "  delete  $b — upstream gone, PR merged"
      act git branch -D "$b"
    else
      echo "  keep    $b — upstream gone but no merged PR"
    fi
  elif [[ "$b" == worktree-agent-* ]]; then
    if [ -n "$(git branch -r --contains "$b" 2>/dev/null)" ]; then
      echo "  delete  $b — agent branch, tip already on a remote"
      act git branch -D "$b"
    else
      echo "  keep    $b — agent branch with commits on no remote"
    fi
  else
    if [ -n "$up" ]; then echo "  keep    $b — tracks $up ${track}"; else echo "  keep    $b — local only"; fi
  fi
done < <(git for-each-ref --format='%(refname:short)%09%(upstream:short)%09%(upstream:track)' refs/heads)

[ "$APPLY" -eq 0 ] && echo "dry run — pass --apply to remove the above"
exit 0
