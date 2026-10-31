# Deploying the server

How a release gets from CI onto the box, and how the box is brought up once. Decided in [#1223](https://github.com/talelburg/eldritch/issues/1223); built in [#1226](https://github.com/talelburg/eldritch/issues/1226). Nightly backups to R2 are in [Backups](#backups).

**A normal week:** you merge, CI goes green on `main`, and its `release` job uploads `eldritch-<sha>`: the release `server` binary, the release client bundle, and the server's migrations, all stamped with the sha. Some time before the session, from WSL in this checkout:

```sh
scripts/deploy.sh --dry-run   # what it would do to the box
scripts/deploy.sh             # deploy GitHub's main head
```

If the session goes wrong, `scripts/deploy.sh --rollback` puts the previous release back. Run `scripts/deploy.sh --help` for every option. The rest of this page is the one-time setup.

## The box

| | |
|---|---|
| Domain | [eldritch.gripe](https://eldritch.gripe), from Cloudflare Registrar |
| Box | `78.46.203.30` and `2a01:4f8:c016:2b28::1`, a Hetzner CX23 on Ubuntu 24.04 LTS |

`/srv/eldritch/`, owned by the `eldritch` system user:

```
releases/<sha>/   server, dist/, migrations/   (the last 5 are kept)
data/eldritch.db  the database
backups/pre-<sha>.db   the backup taken just before <sha> went on
backups/nightly-<date>.db   the nightly backups (the last 7 are kept)
backups/last-upload    touched after each successful upload to R2
current -> releases/<sha>    what the service runs
previous -> releases/<sha>   what --rollback goes back to
```

The service runs as `eldritch`, listens on `127.0.0.1:8000`, and Caddy serves it at the domain over `https://` and `wss://`. Your login user can stop, start and restart the service and act as `eldritch` without a password; anything else asks for your password. That way a bug in `deploy.sh` can reach no further than `/srv/eldritch`.

## One-time setup

### 1. Buy the domain

A domain from Cloudflare Registrar, with no "Arkham Horror" in the name (`docs/product-decisions.md`). The R2 [backups](#backups) use the same Cloudflare account. Any ending works for the setup. Before picking a cheaper ending, check its renewal price: Cloudflare sells at cost and renews at the price it shows. Don't add DNS records yet.

### 2. Create the box

A Hetzner CX23 in Falkenstein or Nuremberg, the nearest CX locations to the group in Israel. Check at purchase that the CX line is offered there; the fallback is DigitalOcean's 1 GiB droplet in Frankfurt.

- Image: Ubuntu 24.04.
- SSH key: your WSL key, `~/.ssh/id_ed25519.pub`. With a key, Hetzner sets no root password.
- Networking: keep both IPv4 and IPv6.
- Firewall: inbound TCP 22, 80 and 443. UDP 443 is optional and lets Caddy serve HTTP/3.
- Volumes, backups, placement groups, labels and cloud config: none. Backups come from `deploy.sh` and the nightly job (see [Backups](#backups)), not Hetzner's.

### 3. Your login user, and ssh locked down

As root (`ssh root@<ip>`):

```sh
adduser talel && usermod -aG sudo talel
rsync --archive --chown=talel:talel ~/.ssh /home/talel
```

Check that `ssh talel@<ip>` works and that `sudo` asks for your password. Then, as `talel`, turn off root login and password login. Ubuntu reads `/etc/ssh/sshd_config.d/*.conf` in name order and the first value for a setting wins, so the `00-` keeps a cloud image's `50-cloud-init.conf` from overriding it:

```sh
sudo tee /etc/ssh/sshd_config.d/00-hardening.conf <<'EOF'
PermitRootLogin no
PasswordAuthentication no
EOF
sudo sshd -t && sudo systemctl reload ssh
sudo sshd -T | grep -E '^(permitrootlogin|passwordauthentication) '   # both "no"
```

Keep that session open while a second terminal checks that `ssh talel@<ip>` gets in and `ssh root@<ip>` is refused.

### 4. The ssh alias, in WSL

`deploy.sh` connects to the host `eldritch` (override it with `ELDRITCH_HOST`). In `~/.ssh/config`:

```
Host eldritch
    HostName <box ip>
    User talel
```

`ssh eldritch` should land on the box as `talel`.

### 5. The service user and its tree

```sh
sudo adduser --system --group --home /srv/eldritch eldritch
sudo -u eldritch mkdir -p /srv/eldritch/{releases,data,backups}
sudo ls -la /srv/eldritch   # all three owned by eldritch
```

### 6. The sudoers file

```sh
sudo visudo -f /etc/sudoers.d/eldritch
```

```
talel ALL=(root) NOPASSWD: /usr/bin/systemctl stop eldritch, /usr/bin/systemctl start eldritch, /usr/bin/systemctl restart eldritch
talel ALL=(eldritch) NOPASSWD: ALL
```

Check it without a remembered password:

```sh
sudo -k
sudo -n -u eldritch whoami        # eldritch
sudo -n systemctl stop eldritch   # gets past sudo; systemctl may complain the unit isn't there yet
sudo -n apt update                # refused: a password is required
```

### 7. Packages

On the box: `sudo apt install sqlite3`, which `deploy.sh` uses for its backups and the live-game check. Install Caddy from Caddy's own apt repository, using the Debian/Ubuntu commands at <https://caddyserver.com/docs/install>. `http://<box ip>` should then show Caddy's placeholder page.

In WSL: `sudo apt install shellcheck`, to lint `deploy.sh`.

### 8. The checked-in config

From this checkout, copy `deploy/` to the box (`scp deploy/* eldritch:`), then on the box:

```sh
sudo install -m 644 eldritch.service /etc/systemd/system/eldritch.service
sudo install -d /etc/eldritch
sudo install -m 600 eldritch.env.example /etc/eldritch/eldritch.env
sudo install -m 644 Caddyfile /etc/caddy/Caddyfile
sudo systemctl daemon-reload
sudo systemctl enable eldritch   # enable only: there is no release to start yet
```

Don't reload Caddy yet: it would ask for a certificate before DNS points at the box.

### 9. DNS

In Cloudflare, add an `A` record pointing the domain at the box's IPv4 and an `AAAA` record pointing it at the IPv6, both **DNS only (grey cloud)**. A proxied record would put Cloudflare's TLS in front of Caddy's and its timeouts on the WebSocket.

- **The IPv6 address is `::1` in the block Hetzner shows.** The console lists the server's network, `2a01:…::/64`; the server itself is on `2a01:…::1`, which `ip -6 addr show scope global` on the box confirms.
- **The name in the records is the name in the Caddyfile.** The records are for the bare domain, so the Caddyfile serves `eldritch.gripe`. Serving `www.` too needs its own record.

Once the box's `dig +short <domain>` and `dig +short AAAA <domain>` return its addresses (WSL has no `dig`; a resolver may hold a stale answer for its TTL):

```sh
sudo systemctl reload caddy
```

### 10. The first deploy

`scripts/deploy.sh` on an empty box copies the release, points `current` at it and starts the server. There is no database to check or back up and no release to stop. Open `https://<domain>` and check that a game loads and its WebSocket connects (`wss://`).

OAuth callback registration isn't here yet: there's no callback until Accounts, and #1236 adds the step.

## Deploying

`scripts/deploy.sh [<ref>]` deploys GitHub's `main` head by default, or the commit `<ref>` names. It takes a release only from CI's artifacts, so it fails with a message when that commit has no green run yet. Two quick merges cancel the earlier run, so only the later one is deployable. To deploy a branch before merging, build it first: `gh workflow run ci --ref <branch>`.

- **The guard.** If a game was played or created in the last 30 minutes, the deploy refuses, because it would restart the server under the group. `--force` overrides it.
- **Order.** Copy the release → stop → back the database up to `backups/pre-<sha>.db` → `previous` takes the old `current` → `current` takes the new release → start → prune to the last 5 releases (never `current` or `previous`).
- **Already on the box.** A release still under `releases/` isn't downloaded again.
- **Going back across a migration.** A deploy refuses to put a release on a database that holds a migration the release doesn't know, because sqlx would refuse to start it. `--rollback` is the way back.

## Rolling back

`scripts/deploy.sh --rollback` points `current` back at `previous` and clears `previous`, so a second rollback refuses. The guard doesn't apply: rolling back mid-session is the point.

- **No new migration since the previous release:** the database is kept. Games played since the deploy either replay on the old release or are kept as unresumable (see *unresumable game* in `GLOSSARY.md`).
- **A migration the previous release lacks:** the previous release can't start on this database, so the rollback restores `backups/pre-<current sha>.db`, the backup taken just before the current release went on. It first says how many actions and games that discards, and asks `y/N`.

## Backups

Every night at 04:00 Israel time, the `eldritch-backup` timer copies the live database to `backups/nightly-<YYYY-MM-DD>.db` (the UTC date), checks it, and uploads it with the `pre-<sha>.db` files to the Cloudflare R2 bucket `eldritch-backups`. The server keeps running. The copy is SQLite's `VACUUM INTO`, which holds a read lock for the few milliseconds it takes. On a successful upload, the job touches `backups/last-upload`. It then keeps the newest 7 nightlies on the box. The `pre-<sha>.db` files are pruned with their releases, as before.

- **Nothing in R2 is deleted by the box.** The upload is `rclone copy`, never `sync`, and it skips any name already in the bucket. R2 expires objects after 90 days (lifecycle rule `expire-90d`). A bucket lock (`keep-30d`) refuses every delete *and every overwrite* in an object's first 30 days, so a re-made file whose name was already uploaded keeps its first upload.
- **A missed night fires at the next boot** (`Persistent=true`).
- **If the job stops working,** `deploy.sh` warns when `backups/last-upload` is more than 36 hours old, or missing. It still deploys. The job's own log: `sudo journalctl -u eldritch-backup -n 50`.
- **The R2 token** is in `/etc/eldritch/rclone.conf`, mode 600, owned by `eldritch`. It is an Account API token with Object Read & Write on `eldritch-backups` only. Cloudflare shows the secret once, so a lost box means minting a new token, not recovering this one.

### Setting up R2

In the Cloudflare dashboard, under R2:

1. Create the bucket `eldritch-backups`: automatic location, no jurisdiction, Standard storage class (the free tier covers Standard only).
2. In the bucket's settings, add the object lifecycle rule `expire-90d`: the whole bucket, delete objects after 90 days.
3. Under *Manage API tokens*, create an **Account API token** with **Object Read & Write**, scoped to `eldritch-backups`, with no expiry. Keep the Access Key ID, the Secret Access Key and the Account ID until the next step.

The bucket lock comes later, in step 4 below, because the smoke test deletes its test object.

### Setting up the box

1. Install rclone and check the version. Cloudflare needs 1.59 or later; older ones get `HTTP 401`.

   ```sh
   sudo apt install rclone
   rclone version
   ```

2. Write the rclone config without the secret touching the shell or its history:

   ```sh
   sudo install -m 600 -o eldritch -g eldritch /dev/null /etc/eldritch/rclone.conf
   sudoedit /etc/eldritch/rclone.conf
   ```

   ```ini
   [r2]
   type = s3
   provider = Cloudflare
   access_key_id = <ACCESS_KEY_ID>
   secret_access_key = <SECRET_ACCESS_KEY>
   endpoint = https://<ACCOUNT_ID>.r2.cloudflarestorage.com
   acl = private
   no_check_bucket = true
   no_head = true
   ```

   A token scoped to one bucket can't check that the bucket exists, hence `no_check_bucket`. `no_head` stops rclone checking each upload with `HEAD …?versionId=…`, which R2 answers with `501 NotImplemented`. Without it, each new upload logs an error, then passes on rclone's retry. The upload is still verified: R2 checks the `PUT`'s `Content-Md5` before storing it.

3. Smoke-test the token as `eldritch`. All three must succeed, with no `ERROR` lines:

   ```sh
   RCLONE="sudo -u eldritch rclone --config /etc/eldritch/rclone.conf"
   echo ok > /tmp/smoke.txt
   $RCLONE copyto /tmp/smoke.txt r2:eldritch-backups/smoke.txt
   $RCLONE ls r2:eldritch-backups
   $RCLONE deletefile r2:eldritch-backups/smoke.txt
   ```

4. **Only now**, in the dashboard: open the bucket, then Settings → Bucket lock rules → Add rule. Name `keep-30d`, no prefix, 30 days.

5. Optionally, check the lock. The delete must fail with `ObjectLockedByBucketPolicy`; the file then expires under `expire-90d`:

   ```sh
   $RCLONE copyto /tmp/smoke.txt r2:eldritch-backups/lock-test.txt
   $RCLONE deletefile r2:eldritch-backups/lock-test.txt
   ```

6. Copy `deploy/` to the box (`scp deploy/* eldritch:`), then on the box:

   ```sh
   sudo install -m 755 eldritch-backup.sh /usr/local/bin/eldritch-backup
   sudo install -m 644 eldritch-backup.service eldritch-backup.timer /etc/systemd/system/
   sudo systemctl daemon-reload
   sudo systemctl enable --now eldritch-backup.timer
   systemctl list-timers eldritch-backup   # next run: 04:00 Israel time
   ```

7. Run it once, and check the upload:

   ```sh
   sudo systemctl start eldritch-backup
   sudo journalctl -u eldritch-backup -n 20
   $RCLONE ls r2:eldritch-backups        # nightly-<today>.db and the pre-*.db files
   sudo -u eldritch ls -l /srv/eldritch/backups/   # last-upload, the nightly, no .tmp
   ```

### Restoring

From *the box is gone*. Restoring onto a box that still exists skips to step 4.

1. Bring a new box up through steps 2–8 of the one-time setup, and step 1 of *Setting up the box* above.
2. In the dashboard, create a new token as in *Setting up R2* step 3. The old secret can't be shown again. Delete the old token.
3. Write `/etc/eldritch/rclone.conf` with it, as in *Setting up the box* step 2.
4. Fetch the backup you want, newest first:

   ```sh
   RCLONE="sudo -u eldritch rclone --config /etc/eldritch/rclone.conf"
   $RCLONE ls r2:eldritch-backups
   $RCLONE copyto r2:eldritch-backups/nightly-<date>.db /srv/eldritch/backups/restore.db
   sudo -u eldritch sqlite3 /srv/eldritch/backups/restore.db "PRAGMA integrity_check"   # ok
   ```

5. Put it in place with the server stopped. A leftover `-journal` belongs to the database being replaced, and SQLite would apply it to the restored one:

   ```sh
   sudo systemctl stop eldritch
   sudo -u eldritch rm -f /srv/eldritch/data/eldritch.db-journal
   sudo -u eldritch mv -T /srv/eldritch/backups/restore.db /srv/eldritch/data/eldritch.db
   ```

   On a new box, deploy a release (step 10 of the setup, or `scripts/deploy.sh`), which starts the server. On the old box, `sudo systemctl start eldritch`. A backup holds each game's creation inputs and action log, so any release at or past the backup's migrations resumes it. Alternatively, leave the file where it is and point `DATABASE_URL` in `/etc/eldritch/eldritch.env` at it.

### The restore drill

Run on setup day, and as the acceptance of #1227. It starts a second server on port 8001 against a copy from R2, and leaves the live one alone:

- [ ] As `eldritch`, fetch last night's backup into a scratch path:

  ```sh
  sudo -u eldritch rclone --config /etc/eldritch/rclone.conf copyto r2:eldritch-backups/nightly-<date>.db /srv/eldritch/backups/drill.db
  ```

- [ ] Start the drill server:

  ```sh
  sudo -u eldritch env ELDRITCH_BIND=127.0.0.1:8001 ELDRITCH_DIST=/srv/eldritch/current/dist \
    DATABASE_URL=sqlite:///srv/eldritch/backups/drill.db /srv/eldritch/current/server
  ```

- [ ] From WSL, `ssh -L 8001:127.0.0.1:8001 eldritch`, open `http://localhost:8001`, and resume a game that existed before the backup. The client resumes the game whose id its browser saved, and `localhost:8001` is a different origin from the domain, so it opens on the picker. Take an id from `sudo -u eldritch sqlite3 /srv/eldritch/backups/drill.db "SELECT game_id FROM games"`, then in the browser console run `localStorage.setItem("eldritch_game_id", "<game_id>"); location.reload()`. After a real restore at the domain, players' browsers still hold their ids.
- [ ] Stop the drill server (Ctrl-C) and `sudo -u eldritch rm -f /srv/eldritch/backups/drill.db*`.
- [ ] The next morning, check that the timer fired on its own: `systemctl list-timers eldritch-backup`, `sudo journalctl -u eldritch-backup`, and a new nightly in R2.

## Drills

Run on setup day, and as the acceptance of #1226:

- [ ] A first deploy onto the empty box serves the game at the domain over `https://` and `wss://`.
- [ ] Make a move; `deploy.sh` again refuses.
- [ ] Deploy a second sha with `--force`, then `--rollback`: the server comes back on the first sha with the database kept.

A rollback across a release that adds a migration is drilled by the first issue whose release adds one.
