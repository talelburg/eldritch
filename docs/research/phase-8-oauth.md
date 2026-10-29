# Phase 8 research: OAuth on our stack

Answers #1212 (part of the phase 8 wayfinder map, #1210): how an axum server signs people in with GitHub and Google, checks them against a hand-kept email allowlist, and knows which account a WebSocket belongs to. Researched 2026-10-29 against the tree at `d22758d9` (axum `0.8.9`, sqlx `0.9.0`, hyper `1.9.0` in `Cargo.lock`; toolchain 1.95).

Throwaway research note. Nothing here is decided: the recommendation at the end is input to [Accounts and the allowlist](https://github.com/talelburg/eldritch/issues/1213) and to the plan composition (#1222), not a ruling.

## The scenario

A friend opens `https://<host>/`. The SPA loads and asks the server who they are. Nobody yet, so it shows "Sign in with GitHub" and "Sign in with Google". They click GitHub. The browser goes to `/auth/github/login`. The server makes a random `state` and a PKCE verifier, puts both in a short-lived cookie, and redirects to `github.com/login/oauth/authorize`. The friend approves. GitHub redirects the browser to `/auth/github/callback?code=…&state=…`. The server checks `state` against the cookie and swaps the code for an access token. It then asks GitHub for the friend's **verified** email addresses and looks them up in the allowlist. On a match it writes a session row, sets a session cookie and redirects to `/`. From then on, the browser sends that cookie with every request it makes to our origin, **including the WebSocket upgrade**. The `/ws/{game_id}` handler reads the cookie before it upgrades, and so knows the account.

## 1. Crates

All version and dependency facts below come from the crates.io API (`/api/v1/crates/<name>` and `/<version>/dependencies`), read 2026-10-29.

| Crate | Latest | Released | Depends on | Fit with our tree |
|---|---|---|---|---|
| `oauth2` | 5.0.0 | 2025-01-21 | `http ^1`, optional `reqwest ^0.12` | Fits. Brings its own `reqwest 0.12` (latest is 0.13.5), `rand 0.8`, `getrandom 0.2`. Those are duplicates of newer versions we already have, which costs compile time but nothing else. |
| `openidconnect` | 4.0.1 | 2025-07-06 | `oauth2 ^5.0.0`, `rsa`, `p256`, `p384`, `ed25519-dalek` | Fits. Same author and same HTTP-client story as `oauth2`. It is heavier because it verifies JWT signatures. |
| `axum-login` | 0.18.0 | 2025-07-20 | `axum ^0.8.1`, **`tower-sessions ^0.14.0`** | The published release is one `tower-sessions` minor behind. |
| `tower-sessions` | 0.15.0 | 2026-02-01 | `tower-cookies ^0.11`, `time`, `tower-sessions-core =0.15.0` | Fits on its own. |
| `tower-sessions-sqlx-store` | 0.15.0 | 2025-01-01 | **`sqlx ^0.8.0`**, **`tower-sessions-core ^0.14.0`** | **Doesn't fit.** It is built on sqlx 0.8, but our pool is sqlx 0.9 (`crates/server/Cargo.toml`), so it can't take our `SqlitePool`. It also implements the 0.14 store trait, not 0.15. |
| `axum-extra` | 0.12.6 | 2026-04-14 | `axum ^0.8.9` (opt.), `cookie ^0.18` (opt.) | Fits exactly. Features: `cookie`, `cookie-signed`, `cookie-private`, `cookie-key-expansion`. MSRV 1.80. |
| `tower-cookies` | 0.11.0 | 2025-01-01 | `cookie ^0.18`, `axum-core ^0.5` (opt.) | Fits. We'd only pull it in through `tower-sessions`. |

**Maintenance.**
- **`axum-login` and the sqlx store.** Upstream has caught up on `main` but hasn't released. `axum-login`'s `main` has *"Upgrade tower-sessions to 0.15.0 (#315)"* (2026-05-07), and `tower-sessions-stores`' `sqlx-store/Cargo.toml` on `main` pins `sqlx = { version = "0.9.0", … }` and `tower-sessions-core = "0.15.0"`. Neither is on crates.io: the latest releases are still `axum-login 0.18.0` (2025-07) and `tower-sessions-sqlx-store 0.15.0` (2025-01). So today the coherent *published* set is axum-login 0.18 + tower-sessions 0.14 + sqlx-store on sqlx 0.8. That means a second sqlx version and a second pool.
- **`oauth2`.** The repo is active (last commits 2026-02-22). `main` is mid-restructure into a workspace with an `oauth2-reqwest 0.1.0-alpha.3` crate, so a 6.x with a breaking HTTP-client change is probably coming. 5.0 is the stable line.

**What each crate does for us.**
- `oauth2` handles the authorization-code exchange, PKCE, and the CSRF `state` token. It doesn't help with sessions or identity. Its docs: *"To prevent SSRF vulnerabilities, be sure to configure the HTTP client **not to follow redirects**."* For reqwest that is `redirect::Policy::none()` ([docs.rs/oauth2/5.0.0](https://docs.rs/oauth2/5.0.0/oauth2/)). `openidconnect` carries the same warning.
- `openidconnect` adds discovery (`CoreProviderMetadata::discover_async`) and a `Nonce`. Its `id_token.claims(&verifier, &nonce)` verifies the ID token's signature, issuer, audience, expiry and nonce, and `claims.email()` reads the email ([docs.rs/openidconnect/4.0.1](https://docs.rs/openidconnect/4.0.1/openidconnect/)). GitHub isn't an OIDC provider for user sign-in, so GitHub always goes through plain `oauth2`.
- `tower-sessions` is the session middleware: a `Session` extractor holding a key/value bag, and a `SessionStore` trait with `create` (default-provided), `save`, `load` and `delete` (source: `tower-sessions-core/src/session_store.rs` at `v0.15.0`). Its cookie defaults (`src/service.rs` at `v0.15.0`) are `name: "id"`, `http_only: true`, **`same_site: SameSite::Strict`**, `secure: true` and `path: "/"`. Default expiry is two weeks. The docs warn that it is *"**not safe** to access and mutate session state concurrently."*
- `axum-login` adds `AuthUser` and `AuthnBackend` traits plus `login_required` route guards on top of `tower-sessions`. Its own OAuth example (`examples/oauth2/src/web/app.rs`) has to override the Strict default: `.with_same_site(SameSite::Lax) // Ensure we send the cookie from the OAuth redirect.` That override is the trap in §3.

## 2. The verified email, per provider

The allowlist keys on email, so each provider has to give us an email *it has verified*.

**GitHub** ([REST: emails](https://docs.github.com/en/rest/users/emails?apiVersion=2022-11-28)):
- `GET /user/emails` *"Lists all of your email addresses, and specifies which one is visible to the public."* *"OAuth app tokens and personal access tokens (classic) need the `user:email` scope to use this endpoint."*
- Each entry has `email`, `primary`, `verified` and `visibility`, all required. Filter to `verified == true`.
- Don't use `GET /user`'s `email` field. It is the *public* email and may be null.
- So the authorize request asks for `scope=user:email`. GitHub supports PKCE: the `code_challenge` *"Must be a 43 character SHA-256 hash of a random string generated by the client,"* and *"the plain code challenge method is not supported"* ([authorizing OAuth apps](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps)).
- About `state`: *"If the states don't match, then a third party created the request, and you should abort the process."*

**Google** ([OpenID Connect](https://developers.google.com/identity/openid-connect/openid-connect)):
- The scope *"in a basic request should be `openid email`."*
- `email`: *"Provided only if you included the `email` scope in your request."* `email_verified`: *"True if the user's email address has been verified; otherwise false."*
- `hd` is the Workspace domain. It doesn't matter for an email allowlist.
- On validating the ID token: Google says that when the token comes straight from its token endpoint, *"since you are communicating directly with Google over an intermediary-free HTTPS channel"* and authenticating with the client secret, *"you can be confident that the token you receive really comes from Google and is valid."* That makes signature verification optional for our server-side flow. `openidconnect` does it anyway, and it also checks the nonce.
- Discovery document: `https://accounts.google.com/.well-known/openid-configuration`.

**Consequences for the allowlist** (inputs for #1213, not decisions):
- **Which GitHub address must match.** GitHub may return several verified emails, so we need a rule. Either *any verified email on the allowlist admits* (simple: a friend whose primary is a work address still gets in), or only `primary` counts.
- **Case.** Compare case-insensitively, or normalise to lowercase at both ends.
- **Gate versus identity.** Email is the gate, not necessarily the identity. GitHub's numeric user id and Google's `sub` are stable even when an email changes. Whether an **Account** keys on (provider, subject) or on email is #1213's question.

## 3. The session carrier, and the WebSocket upgrade

**A browser WebSocket can't take custom headers.** The constructor is `new WebSocket(url)` or `new WebSocket(url, protocols)`, with nothing else ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/WebSocket)). The only header script can influence is `Sec-WebSocket-Protocol`. So `Authorization: Bearer …` is impossible.

**It does carry cookies.** The WHATWG WebSockets spec builds the handshake as a Fetch request whose *"mode is "websocket", credentials mode is "include", cache mode is "no-store", and redirect mode is "error"."* ([§2.2](https://websockets.spec.whatwg.org/)). Fetch's HTTP-network-or-cache fetch then sets *includeCredentials* true when the *"request's credentials mode is "include""*, and *"If includeCredentials is true, then: Append a request `Cookie` header for httpRequest."* ([Fetch](https://fetch.spec.whatwg.org/)). Fetch also always sends `Origin` on it: *"If request's response tainting is "cors" or request's mode is either "websocket" or "webtransport", then append (`Origin`, serializedOrigin) to request's header list."*

So a **same-origin cookie authenticates the upgrade with no client change to how the URL is built.** `crates/web/src/url.rs` already derives a same-origin `ws(s)://{host}/ws/{id}`, and in production the SPA and the WS come from the same axum server (`crates/server/src/lib.rs`: static fallback plus `/ws/{game_id}`).

On the server, `game_ws` already takes extractors before `WebSocketUpgrade` (`State`, `Path`). Adding a cookie or session extractor and returning a non-101 response before `on_upgrade` is ordinary axum. The upgrade then never happens.

**SameSite, concretely.** Cookie attributes behave like this ([MDN Set-Cookie](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie); [RFC 6265bis draft, 29 Oct 2026](https://httpwg.org/http-extensions/draft-ietf-httpbis-rfc6265bis.html)):
- **Strict:** *"Send the cookie only for requests originating from the same site that set the cookie."*
- **Lax:** Strict's rule, plus cross-site requests that are a *"top-level navigation"* using a *"safe"* method.
- **The callback is a cross-site top-level GET** (github.com → us). A cookie holding `state` and the PKCE verifier must therefore be **Lax**. A Strict one isn't sent, `state` can't be checked, and every login fails. That is why `axum-login`'s example overrides `tower-sessions`' Strict default.
- **Setting a cookie on the callback works at any SameSite value.** 6265bis §5.7 step 18.3: *"Top-level navigations can create a cookie with any SameSite value, even if the new cookie wouldn't have been sent along with the request had it already existed prior to the navigation."*
- **Cross-site WebSockets don't get the cookie.** A cross-site page opening `wss://<our host>/ws/…` makes a non-navigation cross-site request, so a Lax or Strict session cookie is excluded (6265bis §5.8.3: excluded *"unless … The retrieval's type is "HTTP" … The target browsing context … is … a top-level traversable"*). This blunts the cross-site WebSocket hijacking that #584 describes.
- **#584's Origin check is still worth doing.** SameSite is about *site* (registrable domain), not origin, so a sibling subdomain counts as same-site. RFC 6455 §10.2 also asks for the check: servers *"SHOULD verify the |Origin| field is an origin they expect. If the origin indicated is unacceptable to the server, then it SHOULD respond to the WebSocket handshake with a reply containing HTTP 403 Forbidden status code."*

**Origin check under the dev proxy.** Compare against a **configured allowed origin**, not one derived from `Host`. Trunk's proxy (`src/proxy.rs`, v0.21.14) replaces `Host` with the backend's host and drops the port: `request.header(HOST, outbound_host)` where `outbound_host = authority.host()`. It forwards the original in `X-Forwarded-Host` and forwards every other inbound header, `Origin` and `Cookie` included, on both HTTP and WS. In dev the WS arrives at :8000 with `Origin: http://localhost:3000` and `Host: localhost`. In prod it's `Origin: https://<host>`. A `PUBLIC_ORIGIN`-style setting covers both. The same setting builds the OAuth `redirect_uri`.

**What this means for `crates/web`'s connect path.** This is a real hazard.
- A rejected upgrade is invisible to script. The WHATWG spec: in the handshake-failure cases *"the WebSocket connection close code would be 1006"*, deliberately, because *"Allowing a script to distinguish these cases would allow a script to probe the user's local network."*
- `gloo-net`'s `WebSocket::open` fails synchronously only for a bad URL or a blocked port (`crates/net/src/websocket/futures.rs`). A 401 or 403 upgrade surfaces later as a close event.
- Today `connect_once` turns "closed before any `Hello`" into `ConnectOutcome::StaleId`, and `run` then **calls `clear_saved_id()` and sends the player back to the roster picker** (`crates/web/src/transport.rs`). So a signed-out player whose upgrade is refused would silently lose their saved game.
- Two remedies, which combine:
  - The client checks sign-in over plain HTTP before it opens the socket, e.g. `GET /auth/me` returning 200 or 401. Fetch *can* see the status.
  - The server accepts the upgrade and then closes with an application code from the private range. RFC 6455 §7.4.2: *"Status codes in the range 4000-4999 are reserved for private use."* gloo hands the code through as `WebSocketError::ConnectionClose(CloseEvent { code, .. })`, which `connect_once` currently discards (`Some(Err(_)) | None => break`).
- **Side observation (not verified by test).** For the same reason, a server that is *down* probably also lands in `StaleId` rather than `Unreachable` today, since `WebSocket::open` doesn't fail synchronously for an unreachable host. Worth a look under #1218 (Disconnection).

## 4. Local dev: trunk on :3000 → server on :8000

The dev loop in `CLAUDE.md` survives with **one new `[[proxy]]` entry and one flag**. The flag is the non-obvious part.

- **Trunk follows redirects by default.** `ProxyClients::create_client` (`src/serve/proxy.rs`, v0.21.14) builds reqwest with `.redirect(if opts.redirect { Policy::default() } else { Policy::none() })`. The config doc reads: *"Automatically redirect proxy requests? `no_redirect` defaults to `false`, i.e. yes, follow redirects automatically."* (`src/config/models/proxy.rs`.) Left alone, trunk would *itself* follow the server's 302 to `github.com/login/oauth/authorize` and hand the browser GitHub's HTML at `localhost:3000/auth/github/login`. The `/auth` entry needs `no_redirect = true`:

  ```toml
  [[proxy]]
  backend = "http://localhost:8000/auth"
  no_redirect = true
  ```

  Putting every auth route (`/auth/{provider}/login`, `/auth/{provider}/callback`, `/auth/me`, `/auth/logout`) under one `/auth` prefix keeps this to a single entry. It also respects the existing Trunk.toml constraints: distinct prefixes, and nothing at `/`.
- **Response headers pass through verbatim.** Trunk copies every backend response header (`for (key, val) in backend_res.headers() { res = res.header(key, val) }`), so `Set-Cookie` and `Location` reach the browser unchanged. The server's post-login redirect should therefore be **relative** (`Location: /`) or built from `PUBLIC_ORIGIN`. An absolute `http://localhost:8000/` would drop the dev session onto the no-hot-reload port.
- **Ports don't separate cookies.** RFC 6265 §8.5: *"Cookies do not provide isolation by port."* A cookie set through :3000 is also sent to :8000, and the reverse, so the dev loop and the single-port `trunk build && cargo run -p server` alternative share one session. **Hostnames do separate cookies:** `localhost` and `127.0.0.1` are different hosts. Trunk binds `127.0.0.1`, but `CLAUDE.md` opens `http://localhost:3000`. Pick `localhost` everywhere, including the registered callback URL, or the state cookie and the callback land in different jars.
- **`Secure` on plain-http localhost.** MDN: *"The `https:` requirements are ignored when the `Secure` attribute is set by localhost."* A `Secure` session cookie therefore works in dev. The `__Host-` prefix needs the cookie *"set with the `Secure` attribute by a secure page (HTTPS)"*. Whether browsers count `http://localhost` as that wasn't checked, so either don't use the prefix or make the cookie name configurable.

## 5. Provider setup

**GitHub OAuth App** ([creating an OAuth app](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/creating-an-oauth-app)):
- Fields: "Application name", "Homepage URL", "Authorization callback URL", and an optional description.
- *"You can enter up to 10 callback URLs."* *"A user or organization can own up to 100 OAuth apps."* The page mentions no cost.
- `redirect_uri` matching: *"When wildcard matching is disabled, the redirect URL must exactly match the callback URL."* For loopback, *"The redirect_uri does not need to match the port specified in the callback URL for the app,"* and GitHub *"recommends not to use localhost"* in favour of `127.0.0.1`. That advice is aimed at native apps. It conflicts with the cookie-host point in §4, so register the exact `http://localhost:3000/auth/github/callback` alongside the prod URL, or use a separate dev app.
- Tokens: with token expiry on, *"The access token expires after eight hours."* That doesn't matter to us: we use the token once, to read `/user/emails`, and throw it away.

**Google OAuth client** (type "Web application"; [web-server flow](https://developers.google.com/identity/protocols/oauth2/web-server); [publishing status](https://support.google.com/cloud/answer/15549945)):
- Redirect URIs *"must use the HTTPS scheme, not plain HTTP."* Localhost URIs, including localhost IP addresses, are exempt. Hosts can't be raw IPs, again except localhost. TLDs must be on the public suffix list. No wildcards or fragments.
- Matching: the `redirect_uri` *"must exactly match one of the authorized redirect URIs for the OAuth 2.0 client."* `http://localhost:3000/auth/google/callback` is registrable next to the prod URL.
- **Publishing status.** "Testing" apps *"are limited to up to 100 test users listed in the OAuth consent screen,"* and *"Authorizations by a test user will expire seven days from the time of consent."* But there's an exception: if the app requests only a subset of name, email address and user profile, *"your users do not need to be in the trusted user list"*, *"they will not see a warning message"*, and *"their authorizations will not expire after 7 days."* With `openid email` we fall inside it, so a Testing-status app needs no verification and no test-user list. Our own allowlist does the gating.
- A Google Cloud project is required to create the client. Its cost wasn't checked against a primary source.

**Secrets.** Each provider gives a client id and a client secret. `docs/product-decisions.md` says "No embedded secrets", so both come from the environment, next to `PUBLIC_ORIGIN` and the allowlist's location. Separate dev and prod apps keep the prod secret off dev machines, and both providers allow it at no stated cost.

## Recommendation

**Crates:**
- `oauth2 = "5"` for GitHub and `openidconnect = "4"` for Google. `openidconnect` depends on `oauth2 ^5`, so it's one crate family and one HTTP client.
- Both use their `reqwest` feature (reqwest 0.12, rustls TLS). The client must be built with `redirect::Policy::none()`, as both crates' docs require.
- `axum-extra` with `cookie-private` (or `cookie-signed`) for the cookies.
- **No `axum-login`, no `tower-sessions`.**

**Session carrier:**
- **OAuth transient state** (`state`, PKCE verifier, Google nonce, provider) goes in one encrypted cookie: `PrivateCookieJar`, `HttpOnly`, **`SameSite=Lax`**, `Path=/auth`, about 10-minute `Max-Age`. It is cleared on callback. No server table.
- **The session** is a random 256-bit id in an `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/` cookie. It keys a `sessions` table in our existing SQLite (sqlx 0.9): id → account, created, expires.
- The `/ws/{game_id}` handler and the HTTP routes resolve that cookie to an account *before* upgrading. They re-check the allowlist, so removing a friend takes effect on their next connect.
- The upgrade also gets #584's Origin check against `PUBLIC_ORIGIN`.
- The client calls `GET /auth/me` before opening the socket. The server closes an unauthenticated socket with a 4xxx code instead of failing the handshake, so `crates/web` never takes "signed out" for `StaleId`.
- **Dev:** add the `/auth` proxy entry with `no_redirect = true`, use `localhost` consistently, and register `http://localhost:3000/auth/{github,google}/callback`, preferably on separate dev apps.

**Why this shape.**
- **It fits the tree.** The two moving parts are an exchange (the `oauth2` family) and a lookup (one table). Both fit sqlx 0.9 and axum 0.8.9 today with only released crates.
- **It survives restarts.** A server-side session in the SQLite file outlives a redeploy, which feeds the exit criterion that "anyone can drop and reconnect mid-game". Logout and allowlist removal are a row delete.
- **It's small.** The whole session layer is roughly a cookie extractor and four queries.

### Alternatives considered and rejected

- **`axum-login` + `tower-sessions` + `tower-sessions-sqlx-store`.** This is the idiomatic stack, and it's fine on upstream `main`. Released, it can't take our sqlx 0.9 pool (§1): adopting it now means a second sqlx and a second pool, or git dependencies. Its value is password and permission plumbing we've ruled out (`product-decisions.md`: no signup, no password reset). We'd also have to override its Strict default for the OAuth round-trip. **Revisit** once `axum-login` and the sqlx store publish their 0.15/sqlx-0.9 releases, if the hand-rolled layer starts growing.
- **`tower-sessions` 0.15 alone, with our own `SessionStore` on sqlx 0.9.** Viable. The trait is four methods. But it trades our one typed `sessions` table for a serialized key/value bag in a store trait, and the docs warn about concurrent mutation. Its `MemoryStore` loses every session on redeploy. It's the nearest runner-up. Pick it if the session grows more state than an account id.
- **A stateless session.** A signed or private cookie, or a JWT, carrying account id and expiry: no table at all. It can't be revoked before expiry except through the allowlist re-check, which we do anyway. Logout only clears the browser's copy. It's an acceptable fallback, but the table costs little and makes logout real.
- **A token in the WS URL** (`/ws/{id}?token=…`, or a one-time ticket fetched over HTTP). This exists to work around cross-origin deployments. We're same-origin in prod (one axum server) and in dev (the trunk proxy), so the cookie already arrives. Tokens in query strings end up in access logs, and the token has to be readable by script.
- **The token in `Sec-WebSocket-Protocol`, or an auth message as the first frame.** Both make the token readable by script, the second keeps unauthenticated sockets open, and neither buys anything over a same-origin cookie.
- **A GitHub App instead of an OAuth App.** More configuration (installations, permissions) for a sign-in-only use. An OAuth App with `user:email` is the documented path for reading verified emails.
- **Plain `oauth2` for Google**, via the userinfo endpoint or by reading the ID token unverified, as Google allows over a direct TLS channel. It drops the `openidconnect` dependency weight, but we'd lose the checked nonce, issuer and audience for no real saving. Worth reconsidering only if build time from `rsa`, `p256` and `p384` becomes a problem.

## Open points for the grilling tickets

- **#1213 (Accounts and the allowlist).** Whether an Account keys on email or on (provider, subject). Which of a GitHub user's verified emails must match. Where the allowlist lives: a file, an env var, or a table. Session lifetime and sliding expiry. Whether logout exists in the UI.
- **#1222 (compose the plan).** The auth slice's shape: server routes under `/auth`, the `sessions` migration, the Trunk.toml entry, and the `crates/web` sign-in check. Whether that slice lands before or with #584's Origin check.
- **#584.** The Origin check reads a configured `PUBLIC_ORIGIN`. Under the trunk proxy, `Host` is rewritten (§3).
- **#1218.** Whether `crates/web` maps a server outage to `StaleId` today (§3 side observation).
