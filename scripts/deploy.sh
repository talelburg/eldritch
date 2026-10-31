#!/usr/bin/env bash
# Every remote command is built locally on purpose, so SC2029's "expands on the
# client side" is the intent: the box's shell gets only literal paths and shas.
# shellcheck disable=SC2029
#
# deploy.sh — put a CI release onto the box, or roll back to the previous one.
#
# Run from WSL, in this checkout. The release is the `eldritch-<sha>` artifact
# CI's `release` job uploads (binary, client bundle, migrations); this script
# never builds anything. The box layout, the service and the one-time setup are
# in docs/deploy.md.
#
# A deploy, in order: refuse if a game was played in the last 30 minutes →
# copy the release to /srv/eldritch/releases/<sha> → stop the server →
# snapshot the database to backups/pre-<sha>.db → `previous` takes the old
# `current`, `current` takes the new release → start the server → keep the
# last 5 releases and their snapshots. On an empty box, each step that needs a
# current release or a database is skipped.
#
# A rollback points `current` back at `previous` and clears `previous`, so a
# second rollback refuses. When the database holds a migration the previous
# release doesn't know, that release would refuse to start (sqlx's
# `VersionMissing`), so the rollback restores the snapshot taken before the
# current release went on. It says how much play that discards and asks first.
# Otherwise the database is kept. The live-game guard doesn't apply.
#
# Usage:
#   scripts/deploy.sh [<ref>] [--force] [--dry-run]
#   scripts/deploy.sh --rollback [--dry-run]
#
#   <ref>      the commit to deploy (sha, short sha or branch), resolved on
#              GitHub; default: GitHub's `main` head, never the local HEAD.
#   --force    deploy even if a game was played in the last 30 minutes.
#   --dry-run  read the box's state, but only print the commands that would
#              change it.
#
# Environment:
#   ELDRITCH_HOST  the ssh host to deploy to (default: the `eldritch` alias).
#
# Exit status: 0 deployed, rolled back, or nothing to do; 1 refused or failed;
# 2 usage error.
#
set -euo pipefail

usage() { sed -n '5,/^set -/p' "$0" | sed 's/^#\{0,1\} \{0,1\}//;$d'; }

HOST="${ELDRITCH_HOST:-eldritch}"
ROOT=/srv/eldritch
DB="$ROOT/data/eldritch.db"
KEEP=5
GUARD_MINUTES=30
# Every file on the box belongs to the service user.
AS="sudo -n -u eldritch"

REF=""
FORCE=0
DRY_RUN=0
ROLLBACK=0
for arg in "$@"; do
  case "$arg" in
    -h|--help) usage; exit 0 ;;
    --force) FORCE=1 ;;
    --dry-run) DRY_RUN=1 ;;
    --rollback) ROLLBACK=1 ;;
    -*) usage >&2; exit 2 ;;
    *) if [ -n "$REF" ]; then usage >&2; exit 2; fi; REF="$arg" ;;
  esac
done
if [ "$ROLLBACK" = 1 ] && { [ -n "$REF" ] || [ "$FORCE" = 1 ]; }; then
  echo "deploy: --rollback takes no <ref> and no --force" >&2
  exit 2
fi

say() { printf '%s\n' "$*" >&2; }
die() { say "deploy: $*"; exit 1; }

# gh resolves `{owner}/{repo}` from the checkout's remote.
cd "$(dirname "$0")/.."

WORK=$(mktemp -d)
SSH_OPTS=(-o BatchMode=yes -o ControlMaster=auto -o "ControlPath=$WORK/ssh-%C" -o ControlPersist=60)
cleanup() {
  ssh "${SSH_OPTS[@]}" -O exit "$HOST" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# Reads the box. Runs under --dry-run too: the plan depends on what it finds.
inspect() { ssh "${SSH_OPTS[@]}" "$HOST" "$1" </dev/null; }

# Changes the box. Printed always, run unless --dry-run.
change() {
  printf '+ %s\n' "$1"
  [ "$DRY_RUN" = 1 ] || ssh "${SSH_OPTS[@]}" "$HOST" "$1" </dev/null
}

# Like `change`, with a local file on the remote command's stdin.
change_with_input() {
  printf '+ %s < %s\n' "$1" "$(basename "$2")"
  [ "$DRY_RUN" = 1 ] || ssh "${SSH_OPTS[@]}" "$HOST" "$1" <"$2"
}

sql() { inspect "$AS sqlite3 -bail $DB \"$1\""; }

# The sha a `current` or `previous` symlink points at, or nothing.
release_of() { inspect "$AS readlink $ROOT/$1 || true" | sed 's#.*/##'; }

db_exists() { [ -n "$(inspect "$AS test -f $DB && echo yes || true")" ]; }

# Migration versions the database has applied, one per line.
db_versions() { sql "SELECT version FROM _sqlx_migrations WHERE success = 1"; }

# Migration versions from a list of migration filenames on stdin: each
# filename's numeric prefix, as sqlx reads it (0004_stored_game.sql → 4).
versions_of_files() { sed -n 's/^0*\([0-9][0-9]*\)_.*\.sql$/\1/p'; }

# The versions in $1 (lines) missing from $2 (lines), space-separated.
missing() {
  comm -23 <(printf '%s\n' "$1" | sed '/^$/d' | sort -u) \
           <(printf '%s\n' "$2" | sed '/^$/d' | sort -u) | tr '\n' ' ' | sed 's/ $//'
}

# Points symlink $1 at release $2, replacing it in one rename.
point() {
  change "$AS ln -sfn releases/$2 $ROOT/$1.new && $AS mv -T $ROOT/$1.new $ROOT/$1"
}

# Waits for the server to answer on its loopback port.
check_up() {
  [ "$DRY_RUN" = 1 ] && return 0
  local _
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if inspect "curl -fsS -o /dev/null http://127.0.0.1:8000/" 2>/dev/null; then
      say "deploy: server up on $1"
      return 0
    fi
    sleep 1
  done
  die "server on $1 isn't answering on 127.0.0.1:8000; see: ssh $HOST sudo journalctl -u eldritch -n 50"
}

rollback() {
  local cur prev
  cur=$(release_of current)
  prev=$(release_of previous)
  [ -n "$prev" ] || die "no previous release to roll back to (rollback goes back one step; deploy a sha instead)"

  local restore=0 snap="$ROOT/backups/pre-$cur.db"
  if db_exists; then
    local lacks
    lacks=$(missing "$(db_versions)" "$(inspect "$AS ls $ROOT/releases/$prev/migrations" | versions_of_files)")
    if [ -n "$lacks" ]; then
      [ -n "$(inspect "$AS test -f $snap && echo yes || true")" ] \
        || die "the database holds migration(s) $lacks that $prev lacks, and there is no snapshot $snap to restore"
      local counts actions games
      counts=$(sql "ATTACH '$snap' AS snap; SELECT (SELECT count(*) FROM main.actions) - (SELECT count(*) FROM snap.actions), (SELECT count(*) FROM main.games) - (SELECT count(*) FROM snap.games);")
      actions=${counts%%|*}
      games=${counts##*|}
      say "deploy: the database holds migration(s) $lacks that $prev lacks, so $prev can't start on it."
      say "deploy: restoring the snapshot taken before $cur went on discards $actions action(s) and $games game(s) recorded since."
      if [ "$DRY_RUN" = 1 ]; then
        say "deploy: (dry run: would ask before restoring)"
      else
        local answer
        read -r -p "Restore the snapshot? [y/N] " answer || answer=""
        case "$answer" in
          y|Y|yes) ;;
          *) die "nothing changed" ;;
        esac
      fi
      restore=1
    fi
  fi

  change "sudo -n systemctl stop eldritch"
  if [ "$restore" = 1 ]; then
    # The WAL files belong to the database being replaced; left behind, SQLite
    # would apply them to the restored one.
    change "$AS cp $snap $DB.restore && $AS rm -f $DB-wal $DB-shm && $AS mv -T $DB.restore $DB"
  fi
  point current "$prev"
  change "$AS rm $ROOT/previous"
  change "sudo -n systemctl start eldritch"
  check_up "$prev"
  if [ "$restore" = 1 ]; then
    say "deploy: rolled back to $prev, database restored to before $cur"
  else
    say "deploy: rolled back to $prev, database kept"
  fi
}

# Finds the CI artifact for $1 and downloads its tarball into $WORK.
fetch() {
  local run
  run=$(gh api "repos/{owner}/{repo}/actions/artifacts?name=eldritch-$1" \
    --jq '[.artifacts[] | select(.expired | not)][0].workflow_run.id // empty')
  [ -n "$run" ] || die "no release artifact for $1 — CI hasn't finished green for it, or the run was cancelled by a newer push. For a branch: gh workflow run ci --ref <branch>"
  say "deploy: downloading eldritch-$1 from run $run"
  gh run download "$run" -n "eldritch-$1" -D "$WORK"
  [ -f "$WORK/eldritch-$1.tar.gz" ] || die "artifact eldritch-$1 holds no eldritch-$1.tar.gz"
}

deploy() {
  local sha cur
  sha=$(gh api "repos/{owner}/{repo}/commits/${REF:-main}" --jq .sha) \
    || die "GitHub doesn't know ${REF:-main}"
  cur=$(release_of current)
  if [ "$sha" = "$cur" ]; then
    say "deploy: $sha is already current"
    return 0
  fi

  local has_db=0
  if db_exists; then has_db=1; fi

  if [ "$has_db" = 1 ] && [ "$FORCE" = 0 ]; then
    local idle
    idle=$(sql "SELECT (CAST(strftime('%s', 'now') AS INTEGER) * 1000 - max(t)) / 60000 FROM (SELECT max(at) AS t FROM actions UNION ALL SELECT max(created_at) FROM games);")
    if [ -n "$idle" ] && [ "$idle" -lt "$GUARD_MINUTES" ]; then
      die "a game was played $idle minute(s) ago; deploying restarts the server under it. Wait, or pass --force"
    fi
  fi

  local on_box target_files
  on_box=$(inspect "$AS test -d $ROOT/releases/$sha && echo yes || true")
  if [ -n "$on_box" ]; then
    target_files=$(inspect "$AS ls $ROOT/releases/$sha/migrations")
  else
    fetch "$sha"
    target_files=$(tar -tzf "$WORK/eldritch-$sha.tar.gz" | sed -n 's#^\./migrations/##p')
  fi

  # A deploy only goes forward. Going back across a migration needs the
  # snapshot, which is --rollback's job.
  if [ "$has_db" = 1 ]; then
    local lacks
    lacks=$(missing "$(db_versions)" "$(printf '%s\n' "$target_files" | versions_of_files)")
    [ -z "$lacks" ] || die "the database holds migration(s) $lacks that $sha lacks, so it would refuse to start. To go back to the previous release, use --rollback"
  fi

  if [ -n "$on_box" ]; then
    say "deploy: $sha is already on the box; skipping the download"
    # Marks it recent, so pruning keeps it.
    change "$AS touch $ROOT/releases/$sha"
  else
    local dir="$ROOT/releases/$sha"
    change_with_input "$AS rm -rf $dir.tmp && $AS mkdir $dir.tmp && $AS tar -xzf - -C $dir.tmp && $AS mv -T $dir.tmp $dir" \
      "$WORK/eldritch-$sha.tar.gz"
  fi

  [ -z "$cur" ] || change "sudo -n systemctl stop eldritch"
  # VACUUM INTO refuses to overwrite, and a redeployed sha has an old snapshot.
  [ "$has_db" = 0 ] || change "$AS rm -f $ROOT/backups/pre-$sha.db && $AS sqlite3 -bail $DB \"VACUUM INTO '$ROOT/backups/pre-$sha.db'\""
  [ -z "$cur" ] || point previous "$cur"
  point current "$sha"
  change "sudo -n systemctl start eldritch"
  check_up "$sha"

  # Never prunes `current` or `previous`, so a stale `previous` older than the
  # last five survives too.
  local releases name kept=0
  releases=$(inspect "$AS ls -1t $ROOT/releases")
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    if [ "$name" = "$sha" ] || [ "$name" = "$cur" ] || [ "$kept" -lt "$KEEP" ]; then
      kept=$((kept + 1))
      continue
    fi
    change "$AS rm -rf $ROOT/releases/$name $ROOT/backups/pre-$name.db"
  done <<<"$releases"

  if [ -n "$cur" ]; then
    say "deploy: deployed $sha (previous: $cur)"
  else
    say "deploy: deployed $sha (first release on the box)"
  fi
}

# Every read below swallows a failed lookup as "not there", so a sudo grant
# that doesn't work must stop the script here rather than pass for an empty box.
inspect "$AS true && sudo -n -l systemctl stop eldritch >/dev/null" \
  || die "on $HOST, can't run as eldritch or can't stop the service without a password; see the sudoers step in docs/deploy.md"

if [ "$ROLLBACK" = 1 ]; then rollback; else deploy; fi
