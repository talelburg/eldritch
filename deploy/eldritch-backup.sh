#!/usr/bin/env bash
#
# eldritch-backup — copy the database off the box, nightly.
#
# Run by deploy/eldritch-backup.service, which deploy/eldritch-backup.timer
# starts at 04:00 Israel time. Installed to /usr/local/bin/eldritch-backup by
# hand (docs/deploy.md, "Backups"). Runs as `eldritch` while the server runs.
#
# In order: take a consistent copy of the live database to
# backups/nightly-<YYYY-MM-DD>.db → check it → upload every nightly and
# pre-deploy backup to R2 → on success, touch backups/last-upload, whose mtime
# is what scripts/deploy.sh's staleness warning reads → keep the newest 7
# nightlies on the box. The pre-<sha>.db files are deploy.sh's to prune.
#
set -euo pipefail

ROOT=/srv/eldritch
# The path in deploy/eldritch.env.example's DATABASE_URL.
DB="$ROOT/data/eldritch.db"
BACKUPS="$ROOT/backups"
RCLONE_CONFIG=/etc/eldritch/rclone.conf
REMOTE=r2:eldritch-backups
KEEP=7
# The bare sqlite3 CLI has no busy timeout, so a write in flight at 04:00
# would fail the copy. The server's pool waits 5 s on a lock, so this outlasts
# any write it makes.
BUSY_MS=10000

say() { printf 'eldritch-backup: %s\n' "$*" >&2; }

# The UTC date. At 04:00 Israel time it is also Israel's date; a missed run
# caught up at boot in the first hours after midnight Israel time takes the
# previous day's name.
out="$BACKUPS/nightly-$(date -u +%F).db"
# The copy is made and checked under a name the upload doesn't match, so a
# torn or corrupt copy never reaches R2, and a failed same-day rerun leaves
# the day's good copy alone. VACUUM INTO refuses to overwrite.
tmp="$out.tmp"

rm -f "$tmp"
sqlite3 -bail -cmd ".timeout $BUSY_MS" "$DB" "VACUUM INTO '$tmp'"

check=$(sqlite3 -bail "$tmp" "PRAGMA integrity_check")
if [ "$check" != ok ]; then
  say "integrity check failed on $tmp, not uploading:"
  printf '%s\n' "$check" >&2
  rm -f "$tmp"
  exit 1
fi
mv -f "$tmp" "$out"
say "copied the database to $out"

# `copy`, never `sync`: pruning here must never delete anything in R2.
# --ignore-existing because the bucket lock refuses to overwrite an object in
# its first 30 days, so a re-made file with an uploaded name (a second run on
# the same day, a redeployed sha's backup) would fail the whole upload. The
# first upload of that name stays.
rclone copy --config "$RCLONE_CONFIG" --ignore-existing --verbose \
  --include 'nightly-*.db' --include 'pre-*.db' \
  "$BACKUPS" "$REMOTE"
touch "$BACKUPS/last-upload"
say "uploaded $BACKUPS to $REMOTE"

# A glob expands sorted, and the names sort by date.
shopt -s nullglob
nightlies=("$BACKUPS"/nightly-*.db)
for ((i = 0; i < ${#nightlies[@]} - KEEP; i++)); do
  rm -f "${nightlies[i]}"
  say "pruned ${nightlies[i]}"
done
