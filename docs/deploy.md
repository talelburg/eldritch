# Deploying the server

How a release gets from CI onto the box, and how the box is brought up once. Decided in [#1223](https://github.com/talelburg/eldritch/issues/1223); built in [#1226](https://github.com/talelburg/eldritch/issues/1226).

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
current -> releases/<sha>    what the service runs
previous -> releases/<sha>   what --rollback goes back to
```

The service runs as `eldritch`, listens on `127.0.0.1:8000`, and Caddy serves it at the domain over `https://` and `wss://`. Your login user can stop, start and restart the service and act as `eldritch` without a password; anything else asks for your password. That way a bug in `deploy.sh` can reach no further than `/srv/eldritch`.

## One-time setup

### 1. Buy the domain

A domain from Cloudflare Registrar, with no "Arkham Horror" in the name (`docs/product-decisions.md`). The R2 backups in #1227 use the same Cloudflare account. Any ending works for the setup. Before picking a cheaper ending, check its renewal price: Cloudflare sells at cost and renews at the price it shows. Don't add DNS records yet.

### 2. Create the box

A Hetzner CX23 in Falkenstein or Nuremberg, the nearest CX locations to the group in Israel. Check at purchase that the CX line is offered there; the fallback is DigitalOcean's 1 GiB droplet in Frankfurt.

- Image: Ubuntu 24.04.
- SSH key: your WSL key, `~/.ssh/id_ed25519.pub`. With a key, Hetzner sets no root password.
- Networking: keep both IPv4 and IPv6.
- Firewall: inbound TCP 22, 80 and 443. UDP 443 is optional and lets Caddy serve HTTP/3.
- Volumes, backups, placement groups, labels and cloud config: none. Backups come from `deploy.sh`'s pre-deploy backups and #1227, not Hetzner's.

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

### 11. Sign-in providers

Friends sign in through GitHub. Each provider needs an app registered under your account, whose callback URL is the server's public origin followed by `/auth/<provider>/callback`. The server builds that URL from `ELDRITCH_PUBLIC_ORIGIN` rather than from the request, because the dev proxy rewrites `Host`. A provider is on once both its client id and its secret are set. With one of the two missing, or a provider on and no origin, the server refuses to start and the journal names the missing variable.

**GitHub.** Register two OAuth apps at <https://github.com/settings/developers> → *OAuth Apps* → *New OAuth App*. Each OAuth app takes one callback URL:

| App | Homepage URL | Authorization callback URL |
|---|---|---|
| production | `https://eldritch.gripe` | `https://eldritch.gripe/auth/github/callback` |
| dev | `http://localhost:3000` | `http://localhost:3000/auth/github/callback` |

Leave *Enable Device Flow* off. On each app's page, *Generate a new client secret*, and copy it at once: GitHub shows it only once.

**On the box**, set the production app's values in the environment file, which is readable only by root, then restart:

```sh
sudoedit /etc/eldritch/eldritch.env
#   ELDRITCH_PUBLIC_ORIGIN=https://eldritch.gripe
#   ELDRITCH_GITHUB_CLIENT_ID=<the production app's client id>
#   ELDRITCH_GITHUB_CLIENT_SECRET=<its secret>
sudo systemctl restart eldritch
journalctl -u eldritch -n 20      # "sign-in with GitHub is on"
```

Then add yourself to the allowlist (see [The allowlist](#the-allowlist)) and sign in at `https://eldritch.gripe`.

**In dev**, put the dev app's values in `.env` at the root of the checkout. Git ignores it, and the server doesn't read it, so source it into the shell that runs the server:

```sh
# .env
ELDRITCH_PUBLIC_ORIGIN=http://localhost:3000
ELDRITCH_GITHUB_CLIENT_ID=<the dev app's client id>
ELDRITCH_GITHUB_CLIENT_SECRET=<its secret>
```

```sh
set -a; . ./.env; set +a
cargo run -p server -- --dev-auth          # the dev picker stays beside GitHub
cd crates/web && trunk serve               # in a second terminal
```

Open `http://localhost:3000`, and not `127.0.0.1:3000`. The in-flight sign-in cookie is set for the host the browser is on, and GitHub sends the browser back to `localhost`, so a sign-in started on `127.0.0.1` comes back without it and gets the "try again" page.

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

A rollback that restores the backup also discards any allowlist edit made since the deploy.

## The allowlist

Sam can play once Sam's email is on the allowlist. You keep it on the box, over ssh, with the server binary's `accounts` subcommand. It runs as `eldritch`, as the service does. The environment file is readable only by root, so the command names its one variable itself: `DATABASE_URL`, the same path as in `deploy/eldritch.env.example`. On the box:

```sh
alias accounts='sudo -u eldritch env DATABASE_URL=sqlite:///srv/eldritch/data/eldritch.db /srv/eldritch/current/server accounts'

accounts add --name Sam sam@gmail.com sam@work.com   # one entry, every email Sam signs in with
accounts list                                        # id, name, emails, enabled or disabled
accounts remove sam@gmail.com                        # or the entry's id
```

- **One entry per person.** Each entry is an account ([ADR 0023](adr/0023-an-account-is-an-allowlist-entry-matched-by-any-verified-email.md)), and a sign-in through any of its emails is that account. `add` refuses an email another entry already holds, naming that entry.
- **`remove` disables the entry and ends its sessions.** The entry stays in `list`, marked disabled, so the games that refer to it stay intact. Its holder's next request or connect finds them signed out. A game socket already open stays open until it next reconnects.
- **It runs the migrations first,** as the server does at startup, so run it through `current`, never an older release.

## Drills

Run on setup day, and as the acceptance of #1226:

- [ ] A first deploy onto the empty box serves the game at the domain over `https://` and `wss://`.
- [ ] Make a move; `deploy.sh` again refuses.
- [ ] Deploy a second sha with `--force`, then `--rollback`: the server comes back on the first sha with the database kept.

A rollback across a release that adds a migration is drilled by the first issue whose release adds one.
