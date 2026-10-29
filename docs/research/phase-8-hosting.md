# Phase 8 research: hosting — domain, server, TLS, deploy path, cost

Research for [#1211](https://github.com/talelburg/eldritch/issues/1211), part of the phase-8 wayfinder map [#1210](https://github.com/talelburg/eldritch/issues/1210). Every price below was read on **2026-10-29** and excludes VAT unless stated. Prices move (Hetzner raised its prices twice in 2026), so re-check before buying.

The constraint is `docs/product-decisions.md` → "Hosting and auth (v1)": one small VM ($5–10/month) or home hardware, no Kubernetes, no autoscaling, no CDN, and GitHub or Google OAuth against a hand-kept allowlist.

## The scenario

A friend opens `https://<name>/` on game night and clicks "Sign in with Google". Google sends their browser back to `https://<name>/auth/callback`, and Google will only do that if it accepts `<name>` as a redirect host. The page then opens `wss://<name>/ws/...` and holds that socket for a three-hour session. Each action is written to `eldritch.db` on the server's disk. Everything below follows from four requirements in that path: a hostname Google accepts, a certificate for it, a socket that stays open for hours, and a disk that outlives a redeploy.

## Recommendation

**Run a small VPS with a cheap `.com` (or similar) domain. Put Caddy in front for automatic Let's Encrypt TLS, ship the server as a plain binary plus the trunk `dist/` under `systemd`, and back up with nightly `VACUUM INTO` snapshots copied off the box.**

- **Box:** a Hetzner **CX23** (2 vCPU, 4 GB, 40 GB NVMe, 20 TB traffic) at **€5.99/month including IPv4**, plus backups at **€1.20/month**. Hetzner's own page showed every Cost-Optimized plan as *"currently unavailable"* on 2026-10-29, so the dependable fallback is a **DigitalOcean 1 GiB droplet at $6.00/month** (weekly backups add 20%, so $1.20).
- **Domain:** a non-infringing name such as `eldritch-<something>.com` from Cloudflare Registrar (at cost, about $10.46/year now and about $11.17 after Verisign's increase) or Porkbun ($11.08/year for `.com`, the same price to renew).
- **Estimated cost:** about **€7.20/month (≈ €86/year) for the box, plus ≈ $11/year for the domain, so roughly $95–105/year in total**. On the DigitalOcean fallback it is $7.20/month, or ≈ $86 + $11 ≈ **$97/year**. Both sit inside the $5–10/month posture.

Why this setup:
- **It has the fewest third parties on the request path.** Your registrar points at your box, and that's it. Nothing else touches the socket.
- **Long-lived WebSockets aren't subject to anyone's proxy policy.** Cloudflare and Fly both close sockets on their own schedule (details below).
- **A real domain is the only option that satisfies both OAuth providers forever,** and it keeps the URL stable across a provider move.

### Runner-ups

1. **Home hardware plus Tailscale Funnel.** Costs $0 beyond electricity, and you get a free `https://<machine>.<tailnet>.ts.net` hostname with TLS managed for you. `ts.net` is on the Public Suffix List, so Google should accept it. The catches: Funnel is beta, its bandwidth limits are *"non-configurable"* and unpublished, and the game is only up while your home machine and connection are. That suits a "game night only" posture, and it's the cheapest way to try the real flow before paying for anything.
2. **Fly.io.** shared-cpu-1x 256 MB costs $2.19/month and a 1 GB volume $0.15/month. You get a free `*.fly.dev` hostname (on the PSL) with free TLS, so it comes to about **$2.50/month with no domain needed**, making it the cheapest paid option. The catches: you need a Dockerfile, a volume is *"a slice of an NVMe drive on the same physical server"* that Fly warns can be lost, and you must turn off autostop so it doesn't kill sockets.
3. **Home hardware plus a Cloudflare named Tunnel.** Free, but it needs a domain on Cloudflare DNS, and Cloudflare *"may restart servers, which terminates WebSockets connections"*. The client's reconnect logic would have to cover that from the start.

**Rejected: Oracle Cloud Always Free.** It's free and the A1 shape is generous, but Oracle *"may reclaim"* instances whose 95th-percentile CPU and network stay under 20% over 7 days. A game server idle between game nights fits that description exactly.

## 1. A domain: is one needed?

**Google requires a hostname, not a bare IP, and it must be HTTPS.** From Google's [OAuth 2.0 for web server apps → Redirect URI validation rules](https://developers.google.com/identity/protocols/oauth2/web-server):
- Scheme: redirect URIs must use HTTPS, except for localhost.
- Host: *"Hosts cannot be raw IP addresses."* (Localhost IPs are exempt.)
- Domain: *"Top-level domains must belong to the public suffix list"*, and the host can't be `googleusercontent.com` or a URL shortener.

**GitHub sets no host rule.** [GitHub's OAuth App docs](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) only require that *"the redirect URL must exactly match the callback URL"* (or, with wildcard matching, that *"the redirect URL's host (excluding subdomains) and port must exactly match"*). They state no scheme or host restriction, so a GitHub-only app could in principle call back to a bare IP. Since January 2026 that IP could even get a publicly trusted certificate: Let's Encrypt's [IP address certificates went GA on 2026-01-15](https://letsencrypt.org/2026/01/15/6day-and-ip-general-availability), but only as 6-day short-lived certs validated over http-01 or tls-alpn-01. That's fragile, and it rules out Google, so it isn't worth designing around.

**Free subdomains work for Google if their parent is on the Public Suffix List.** The [PSL](https://publicsuffix.org/list/public_suffix_list.dat), version `2026-10-28_13-15-55_UTC`, contains `duckdns.org`, `ts.net`, `fly.dev`, `trycloudflare.com`, `dynv6.net` and `freemyip.com`. So `eldritch.duckdns.org` is a registrable "top private domain" in its own right, and Google's TLD rule is satisfied. [DuckDNS](https://www.duckdns.org/about.jsp) is *"a free service which will point a DNS (sub domains of duckdns.org) to an IP of your choice."* Google's [branding page](https://support.google.com/cloud/answer/15549049) says *"Google restricts your OAuth 2.0 application to using Authorized Domains"*. Verifying a domain with Google is needed only for brand verification, which an unverified hobby app doesn't need (next paragraph).

**Google's Testing-mode caps don't bite for sign-in-only scopes.** Google's [Manage App Audience](https://support.google.com/cloud/answer/15549945?hl=en) page says Testing-status apps *"are limited to up to 100 test users"*, and that *"Authorizations by a test user will expire seven days from the time of consent"*. The *"only exception to this behavior"* is an app that *"requests a subset of the following: name, email address, and user profile"*, which is exactly what an allowlist login needs. The app can stay unverified indefinitely.

**Verdict:** a domain isn't strictly required (DuckDNS, `ts.net` and `fly.dev` all pass Google's rules). It's still recommended at ≈ $11/year, because it survives a move between providers and doesn't depend on a free service staying up.

**Registrars (yearly, USD):**

| Registrar | `.com` | Renews at | Notes |
|---|---|---|---|
| Cloudflare Registrar | ≈ $10.46 (≈ $11.17 after the Verisign increase) | same, at cost | [*"does not mark up domain prices at all"*](https://www.cloudflare.com/products/registrar/). [*"all domains on Cloudflare Registrar use Cloudflare nameservers"*](https://developers.cloudflare.com/registrar/faq/), which is fine, and required anyway for a Cloudflare Tunnel. The price figures come from aggregators ([CostGoat](https://costgoat.com/compare/domains/com), [domainoffer](https://domainoffer.net/tld/com/cloudflare)), not Cloudflare's own page; confirm in the dashboard. |
| [Porkbun](https://porkbun.com/products/domains) | $11.08 | $11.08 | Free WHOIS privacy. `.dev` $12.87, `.app` $14.93 (both HSTS-preloaded, which is fine behind Caddy). Avoid `.xyz`, `.fun` and `.online`: cheap the first year, $14–31 to renew. |

**Naming:** product-decisions' Legal posture keeps "Arkham Horror" and other Fantasy Flight marks out of the domain. Any `eldritch…` name passes. Check availability at purchase time.

## 2. Where it runs

The server is light. It's an axum binary with SQLite, serving a static WASM bundle, and the build happens elsewhere, so 1 GB of RAM is plenty and 512 MB is probably enough. WebSockets are the hard requirement.

| Option | Monthly | What you get | WebSocket risk | Source |
|---|---|---|---|---|
| **Hetzner CX23** (EU: NBG1/HEL1) | €5.99 incl. IPv4. Backups from €1.198 | 2 vCPU, 4 GB, 40 GB NVMe, 20 TB | none (your own Caddy) | [cost-optimized](https://www.hetzner.com/cloud/cost-optimized/). Shown *"currently unavailable"* on 2026-10-29. Hetzner says the plans run on older hardware, so *"the number of available servers is limited"*. Prices rose for new orders on [15 June 2026](https://docs.hetzner.com/general/infrastructure-and-availability/price-adjustment/) (CX23 went from €3.99 to €5.49 excluding IPv4). |
| Hetzner CPX02 (EU) | €6.49 | 1 vCPU, 1 GB, 20 GB | none | [regular-performance](https://www.hetzner.com/cloud/regular-performance/), also shown unavailable. The US locations cost far more (CPX11 is €17.99). |
| **DigitalOcean Basic** | $4.00 (512 MiB) / **$6.00 (1 GiB)** / $12 (2 GiB) | 1 vCPU, 10/25/50 GiB, 500/1000/2000 GiB transfer | none | [droplet pricing](https://www.digitalocean.com/pricing/droplets). Backups cost 20% (weekly) or 30% (daily) of the droplet price. |
| Fly.io | $2.19 (256 MB) / $3.69 (512 MB) + $0.15/GB volume | shared IPv4 free, first 10 hostname certs free, `*.fly.dev` | autostop has to be off. The proxy sends a close frame before stopping a Machine | [pricing](https://docs.fly.io/about/pricing): *"New organizations don't have a free tier"*. [volumes](https://docs.fly.io/volumes/overview): *"Always provision at least two volumes per app"*. [autostop](https://fly.io/docs/launch/autostop-autostart/) |
| Oracle Always Free | $0 | up to 2× E2.1.Micro (1/8 OCPU, 1 GB) or A1 with 2 OCPU / 12 GB | none, but the instance itself may be reclaimed | [Always Free resources](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm): reclaims idle instances if *"CPU utilization for the 95th percentile is less than 20%"* over 7 days |
| Home + **Tailscale Funnel** | $0 | `<host>.<tailnet>.ts.net`, TLS automatic | beta, unpublished bandwidth limits | [Funnel](https://tailscale.com/kb/1223/funnel): *"Funnel can only listen on ports 443, 8443, and 10000"*, *"can only use DNS names in your tailnet's domain"*, *"subject to non-configurable bandwidth limits"*, *"available for all plans"* |
| Home + **Cloudflare named Tunnel** | $0 (+ domain) | outbound-only connector. Domain must be on Cloudflare | Cloudflare *"may restart servers, which terminates WebSockets connections"* and closes idle ones | [WebSockets](https://developers.cloudflare.com/network/websockets/): *"supported on all Cloudflare plans"* |
| Cloudflare Quick Tunnel | $0 | random `trycloudflare.com` name | — | [Quick Tunnels](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/): *"The hostname changes each time you create a Quick Tunnel"*. That rules it out, because the OAuth callback must be fixed. |

Latency barely matters for a turn-based game, so pick the region nearest the group. Hetzner's EU locations are cheap, and its US ones are not.

## 3. TLS (`wss://`)

**Caddy is the recommended path.** Per [Caddy → Automatic HTTPS](https://caddyserver.com/docs/automatic-https), *"If your domain's A/AAAA records point to your server"*, Caddy obtains and renews certificates from Let's Encrypt, falling back to ZeroSSL. It needs ports 80 and 443 reachable and a persistent data directory. Renewal failures are retried with backoff for up to 30 days. The whole Caddyfile is two lines: `<name> { reverse_proxy 127.0.0.1:8000 }`. Per the [reverse_proxy docs](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy), Caddy handles WebSockets by *"performing the HTTP upgrade request then transitioning the connection to a bidirectional tunnel"*, with no extra config, so `wss://` comes for free.

One thing to know for the "what a redeploy does to a live game" question: on a Caddy *config reload*, *"WebSocket connections are forcibly closed"* unless `stream_close_delay` is set (the docs suggest `5m`). Restarting the eldritch binary drops every socket regardless. Reconnect is therefore needed for redeploys as well as flaky Wi-Fi.

nginx with certbot also works, but it means writing the TLS config, the `Upgrade`/`Connection` header lines and the renewal timer yourself. Caddy does all of that by default, and nothing here needs nginx specifically. With a tunnel (Funnel or Cloudflare), the tunnel terminates TLS, so there's no Caddy and no open ports.

## 4. Getting a build onto the box

**What the code needs today** (`crates/server/src/main.rs`, `lib.rs`, `db.rs`):
- It reads `DATABASE_URL`, defaulting to `sqlite:eldritch.db` (relative to the working directory).
- It **binds `127.0.0.1:8000`, hardcoded.** That's right behind Caddy on the same host. A Docker container can't be reached through it, so a Docker deploy needs a bind-address env var first.
- It serves the frontend from **`crates/web/dist`, a path relative to the working directory**, so a `systemd` unit sets `WorkingDirectory=` to a directory that contains `crates/web/dist/`. Otherwise the dist path should become configurable.
- Migrations are embedded (`sqlx::migrate!`), so the binary brings its own schema.

**Plain binary plus `systemd` (recommended)** is one service unit with `Restart=always`, `WorkingDirectory=/srv/eldritch` and `Environment=DATABASE_URL=sqlite:/srv/eldritch/data/eldritch.db`. A deploy is: copy the binary and the `dist/` tarball over, then `systemctl restart eldritch`. There's no daemon or registry to run.

**Docker** is the portability insurance product-decisions mentions. It's required for Fly.io and optional elsewhere. It needs the bind fix above and a volume for the database.

**Can CI publish it? Yes, for free.** *"GitHub Actions usage is free for standard GitHub-hosted runners in public repositories"* ([Actions billing](https://docs.github.com/en/actions/concepts/billing-and-usage)), and *"GitHub Packages usage is free for public packages"*, with Container registry storage and bandwidth *"currently free"* ([Packages billing](https://docs.github.com/en/billing/concepts/product-billing/github-packages)). The current `ci.yml` already runs `trunk build --release` in the `wasm-build` job. A release workflow could attach `server` plus `dist.tar.gz` to a GitHub Release (or push a GHCR image). Pushing to the box from CI then needs an SSH key held as an Actions secret, which doesn't conflict with "no embedded secrets" since it's never in the repo.

**For the map's "deploying from CI or by hand" question:** two options are cheap.
- **(a) By hand:** a `scripts/deploy.sh` that builds locally (or downloads the CI artifact), copies it over and restarts the service. No secrets live in GitHub.
- **(b) CI-published artifact, pulled by hand:** CI builds on tag, and a one-liner on the box fetches and restarts.

Fully automatic push-on-merge adds a deploy key and drops sockets mid-game on every merge, which is the opposite of what game night wants. I lean towards (b), or (a) to start.

## 5. SQLite on that box

**Persistence:** on a VPS the database is a file on the root disk, which survives reboots and binary redeploys. Put it in its own directory (`/srv/eldritch/data/`) so a redeploy never touches it. On Fly it must live on a volume.

**Backups, cheapest first:**
1. **Provider snapshots.** Hetzner's are €1.198/month for the CX23, with *"Seven backup slots … per server"*. DigitalOcean charges 20% of the droplet for weekly. These are whole-disk and crash-consistent rather than SQLite-aware, and they're stored with the same provider, but they cost no effort.
2. **Nightly `VACUUM INTO` plus an off-box copy.** [SQLite's docs](https://www.sqlite.org/lang_vacuum.html) call `VACUUM INTO` *"an alternative to the backup API for generating backup copies of a live database"*, and *"the generated output database is a consistent snapshot of the original"*. A `systemd` timer runs `sqlite3 eldritch.db "VACUUM INTO '/srv/eldritch/backups/<date>.db'"` and `rclone`s the file to **Cloudflare R2**, whose [free tier](https://developers.cloudflare.com/r2/pricing/) gives 10 GB-month of storage and free egress. A friends-only action log stays far below that, so the cost is $0.
3. **Litestream.** It streams changes to S3-compatible storage (R2 works) for point-in-time recovery. Per [How it works](https://litestream.io/how-it-works/), it *"works by effectively taking over the checkpointing process"* of SQLite's WAL. `db.rs` doesn't set a journal mode, so the server runs in SQLite's default rollback-journal mode, and adopting Litestream means switching to WAL first. It's more machinery than a game-night app needs, so it's a runner-up.

**Recommended:** (2), with (1) on as well if the budget allows. It loses at most a day's actions. The event log already gives undo and resume within a game, so a nightly snapshot is enough between games.

## Open items this leaves for other tickets

- **Redeploy versus live games** (map fog): every binary restart drops all sockets, and a Caddy reload does too unless `stream_close_delay` is set. That feeds [#1215](https://github.com/talelburg/eldritch/issues/1215) and [#1218](https://github.com/talelburg/eldritch/issues/1218).
- **Two small code changes before any deploy:** make the bind address configurable, which Docker and tunnels need, and make the `dist` path independent of the working directory.
- Hetzner stock: check the console for CX23 availability at purchase time. If none, use DigitalOcean's $6 droplet.
