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
# Every other branch is listed with its reason for being kept. Ticket branches
# from an /implement-spec run whose remote copies were never deleted show up
# there; delete those on the remote by hand once the integration PR merges.
#
# Usage:
#   scripts/prune-agent-worktrees.sh          # dry run: print what would go
#                                             # (it still runs `git fetch --prune`,
#                                             # which only syncs remote-tracking refs)
#   scripts/prune-agent-worktrees.sh --apply  # remove it
#
set -uo pipefail

APPLY=0
case "${1:-}" in
  --apply) APPLY=1 ;;
  '') ;;
  -h|--help) sed -n '2,/^set -/p' "$0" | sed 's/^#\{0,1\} \{0,1\}//;$d'; exit 0 ;;
  *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

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

# ---------------------------------------------------------------- worktrees

echo "agent worktrees:"
found=0
while IFS= read -r line; do
  case "$line" in
    "worktree "*) wt=${line#worktree }; lock="" ; locked=0 ;;
    "locked"*)    locked=1; lock=${line#locked} ;;
    "")
      [[ "$wt" == "$ROOT/.claude/worktrees/agent-"* ]] || continue
      found=1
      name=${wt#"$ROOT"/}
      if [ -n "$(git -C "$wt" status --porcelain 2>/dev/null)" ]; then
        echo "  keep    $name — uncommitted changes"; continue
      fi
      if [ -z "$(git for-each-ref --contains "$(git -C "$wt" rev-parse HEAD)" refs/heads refs/remotes)" ]; then
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
        act git worktree remove -f -f "$wt"
      else
        echo "  remove  $name — clean, unlocked"
        act git worktree remove "$wt"
      fi ;;
  esac
done < <(git worktree list --porcelain; echo)
[ "$found" -eq 0 ] && echo "  (none)"
[ "$APPLY" -eq 1 ] && git worktree prune

# ----------------------------------------------------------------- branches

CURRENT=$(git branch --show-current)
# Branches still checked out in some worktree (after the removals above).
CHECKED_OUT=$(git worktree list --porcelain | sed -n 's|^branch refs/heads/||p')

echo "local branches:"
while IFS=$'\t' read -r b up track; do
  [ "$b" = main ] && continue
  if grep -qxF "$b" <<<"$CHECKED_OUT" || [ "$b" = "$CURRENT" ]; then
    echo "  keep    $b — checked out"; continue
  fi
  if [ "$track" = "[gone]" ]; then
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
