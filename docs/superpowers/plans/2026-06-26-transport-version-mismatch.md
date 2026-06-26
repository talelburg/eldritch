# Transport Version-Mismatch Surfacing (#463) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the browser client receives a server frame it can't deserialize (a client/server wire-format skew), show an actionable status instead of silently hanging on `<no game>`, and stop the reconnect loop.

**Architecture:** Add a terminal `ConnStatus::VersionMismatch` state rendered on `BoardView`'s existing status line; in `connect_once`, handle the deserialization `Err` (currently silently dropped) by setting that status and returning a new `ConnectOutcome::VersionMismatch`; the reconnect loop returns on that outcome, leaving the status visible.

**Tech Stack:** Rust, Leptos (`crates/web`, wasm32), `wasm-bindgen-test` (headless Firefox).

## Global Constraints

- **YAGNI:** no protocol version tag / handshake (detect from the deserialization `Err`); no dedicated banner UI (reuse the existing `.status` line).
- **CI gauntlet before push** (all seven jobs, warnings-as-errors); `wasm-build`/`wasm-test`/`wasm-clippy` matter since this is `crates/web`:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `wasm-pack test --headless --firefox crates/web`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Branch:** `web/transport-version-mismatch` (already created; spec committed). One branch, follow-up commits, no force-push.
- Spec of record: `docs/superpowers/specs/2026-06-26-transport-version-mismatch-design.md`.

---

### Task 1: `ConnStatus::VersionMismatch` + board rendering

The variant and its rendering ship together: `BoardView`'s `status` closure is the only exhaustive `match` on `ConnStatus` (`board.rs:18`), so adding the variant requires adding the arm in the same change. This is the user-visible contract and is TDD'd.

**Files:**
- Modify: `crates/web/src/store.rs` (add the `ConnStatus::VersionMismatch` variant)
- Modify: `crates/web/src/board.rs:18-24` (add the status-string arm)
- Test: `crates/web/tests/board.rs` (new wasm test asserting the status text)

**Interfaces:**
- Produces (consumed by Task 2): `ConnStatus::VersionMismatch` (in `web::store`).

- [ ] **Step 1: Write the failing wasm test**

In `crates/web/tests/board.rs`, add this test at the end of the file. It drives the store's `status` directly (the field `reduce` never sets) and asserts the actionable text renders on the `.status` line:

```rust
#[wasm_bindgen_test]
async fn version_mismatch_status_renders_actionable_message() {
    use web::store::ConnStatus;
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| s.status = ConnStatus::VersionMismatch);
    leptos::task::tick().await;

    // Scope to the last mounted .status line (DOM accumulates across tests).
    let lines = leptos::prelude::document()
        .query_selector_all(".status")
        .expect("query_selector_all");
    let html = lines
        .item(lines.length() - 1)
        .expect("at least one .status line")
        .dyn_ref::<web_sys::Element>()
        .expect("Element")
        .inner_html();

    assert!(
        html.contains("version mismatch"),
        "status line must name the version mismatch: {html}"
    );
    assert!(
        html.contains("restart the server"),
        "status line must tell the user what to do: {html}"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `wasm-pack test --headless --firefox crates/web --test board`
Expected: FAIL to compile — `ConnStatus::VersionMismatch` does not exist yet.

- [ ] **Step 3: Add the `VersionMismatch` variant**

In `crates/web/src/store.rs`, add the variant to the `ConnStatus` enum (after `AwaitingRoster`):

```rust
    /// No saved game and no roster chosen yet — render the picker.
    AwaitingRoster,
    /// A server frame failed to deserialize — the client and server binaries
    /// disagree on the wire format. Terminal: restart the server and reload.
    VersionMismatch,
```

- [ ] **Step 4: Add the board status arm**

In `crates/web/src/board.rs`, add the arm to the `status` closure's match (after the `AwaitingRoster` arm):

```rust
        ConnStatus::AwaitingRoster => "awaiting-roster",
        ConnStatus::VersionMismatch => "version mismatch — restart the server and reload",
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `wasm-pack test --headless --firefox crates/web --test board`
Expected: PASS (the new test plus the existing board tests).

- [ ] **Step 6: Commit**

```bash
git add crates/web/src/store.rs crates/web/src/board.rs crates/web/tests/board.rs
git commit -m "web: ConnStatus::VersionMismatch + actionable status-line rendering

A terminal connection state for a client/server wire-format skew, rendered on
BoardView's existing status line so the mismatch is visible instead of a silent
<no game>. Wired into the transport in the next commit.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Detect the skew in `connect_once` and stop the reconnect loop

Wires Task 1's variant into the transport: handle the deserialization `Err` (today silently dropped), classify the connection as `VersionMismatch`, and exit the reconnect loop. This is wasm-only async control flow over a live `WebSocket` with no test harness (the rest of `transport.rs` is likewise not unit-tested), so it is verified by build + wasm-clippy + review, not a fabricated socket test.

**Files:**
- Modify: `crates/web/src/transport.rs` — `ConnectOutcome` enum (~76-86), `connect_once` (~145-170), the `start` reconnect loop (~47-67)

**Interfaces:**
- Consumes (from Task 1): `ConnStatus::VersionMismatch`.
- Produces: `ConnectOutcome::VersionMismatch` (module-private).

- [ ] **Step 1: Add the `ConnectOutcome::VersionMismatch` variant**

In `crates/web/src/transport.rs`, add the variant to the `ConnectOutcome` enum. Replace:

```rust
enum ConnectOutcome {
    /// `WebSocket::open` failed: the server is unreachable (down or
    /// restarting). Keep the id and retry.
    Unreachable,
    /// Opened, but the server closed before sending any `Hello` — the id
    /// is stale (unknown to the server). Discard it and recreate.
    StaleId,
    /// Connected, saw `Hello`, then the socket closed: a normal
    /// disconnect. Keep the id and reconnect.
    Disconnected,
}
```

with:

```rust
enum ConnectOutcome {
    /// `WebSocket::open` failed: the server is unreachable (down or
    /// restarting). Keep the id and retry.
    Unreachable,
    /// Opened, but the server closed before sending any `Hello` — the id
    /// is stale (unknown to the server). Discard it and recreate.
    StaleId,
    /// Connected, saw `Hello`, then the socket closed: a normal
    /// disconnect. Keep the id and reconnect.
    Disconnected,
    /// A server frame failed to deserialize — the client and server binaries
    /// disagree on the wire format. Terminal: do not retry (a retry hits the
    /// same stale server). The status is set to `VersionMismatch` before this
    /// is returned.
    VersionMismatch,
}
```

- [ ] **Step 2: Handle the deserialization `Err` in `connect_once`**

In `connect_once`, add the mismatch flag beside `saw_hello`. Replace:

```rust
    let (mut write, read) = ws.split();
    let mut read = read.fuse();
    let mut saw_hello = false;
```

with:

```rust
    let (mut write, read) = ws.split();
    let mut read = read.fuse();
    let mut saw_hello = false;
    let mut version_mismatch = false;
```

Then replace the silent text-frame branch:

```rust
                Some(Ok(Message::Text(txt))) => {
                    if let Ok(msg) = serde_json::from_str::<ServerMessage>(&txt) {
                        saw_hello |= matches!(msg, ServerMessage::Hello { .. });
                        store.update(|s| reduce(s, msg));
                    }
                }
```

with explicit `Err` handling that sets the status and breaks:

```rust
                Some(Ok(Message::Text(txt))) => {
                    match serde_json::from_str::<ServerMessage>(&txt) {
                        Ok(msg) => {
                            saw_hello |= matches!(msg, ServerMessage::Hello { .. });
                            store.update(|s| reduce(s, msg));
                        }
                        // A frame we can't parse from a server that IS talking
                        // to us is a wire-format skew, not a transient glitch.
                        Err(_) => {
                            store.update(|s| s.status = ConnStatus::VersionMismatch);
                            version_mismatch = true;
                            break;
                        }
                    }
                }
```

- [ ] **Step 3: Classify the outcome (mismatch wins)**

Replace the post-loop classification at the end of `connect_once`:

```rust
    if saw_hello {
        ConnectOutcome::Disconnected
    } else {
        ConnectOutcome::StaleId
    }
```

with:

```rust
    if version_mismatch {
        ConnectOutcome::VersionMismatch
    } else if saw_hello {
        ConnectOutcome::Disconnected
    } else {
        ConnectOutcome::StaleId
    }
```

- [ ] **Step 4: Exit the reconnect loop on `VersionMismatch`**

In `start`, add the terminal arm to the `match connect_once(...)`. Replace:

```rust
        match connect_once(&store, &game_id, &mut rx).await {
            // Opened, but the server closed before any Hello: a valid game
            // always sends Hello immediately, so the id is unknown to the
            // server (e.g. a DB reset). Discard it and create a fresh game.
            ConnectOutcome::StaleId => {
```

with:

```rust
        match connect_once(&store, &game_id, &mut rx).await {
            // A wire-format skew: the status is already set to VersionMismatch.
            // Stop the loop entirely — retrying just hits the same stale server.
            ConnectOutcome::VersionMismatch => return,
            // Opened, but the server closed before any Hello: a valid game
            // always sends Hello immediately, so the id is unknown to the
            // server (e.g. a DB reset). Discard it and create a fresh game.
            ConnectOutcome::StaleId => {
```

(The `return` exits before the `status = Reconnecting` line at the loop's foot, leaving the `VersionMismatch` status visible.)

- [ ] **Step 5: Build + wasm-clippy to verify the wiring compiles cleanly**

Run: `cargo build -p web --target wasm32-unknown-unknown`
Expected: clean (the `ConnectOutcome` match in `start` is now exhaustive over four variants).

Run: `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 6: Re-run the wasm board tests (no regression)**

Run: `wasm-pack test --headless --firefox crates/web --test board`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/transport.rs
git commit -m "web: surface wire-format skew + stop reconnect loop on it

connect_once handles the previously-silent deserialization Err: it sets
ConnStatus::VersionMismatch and returns ConnectOutcome::VersionMismatch, on which
the reconnect loop returns (a retry hits the same stale server). Closes the
invisible <no game> hang from a stale server binary + new client.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Full CI gauntlet, push, PR

- [ ] **Step 1: Run the complete local gauntlet**

Run each, expecting all green:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
If `cargo fmt --check` flags anything, run `cargo fmt` and fold into the relevant commit.

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin web/transport-version-mismatch
gh pr create --fill
```
PR body: short design-decisions paragraph (detect from the deserialization `Err`, no protocol version tag; `VersionMismatch` is terminal — stop retrying; reuse the existing status line). Quote the silent-hang failure mode it fixes. Ensure the body has `Closes #463.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`
Fix any failures with follow-up commits to the same branch (no force-push).

- [ ] **Step 4: Phase doc**

No phase-7 doc change required: #463 is a `p2-later` dev-ergonomics fix, not a gate item. The phase-7 doc's dev-loop note (added in PR #462) already mentions this hardening as a possible follow-up; updating it is optional and not required for this PR. Skip unless a quick one-line "(done in #463)" feels worth it.

- [ ] **Step 5: Merge only after explicit user approval**

Do not merge autonomously. Surface CI-green, and wait. On approval:
```bash
gh pr merge <PR#> --squash --delete-branch
```
Then confirm #463 auto-closed and `git pull` on `main`.

## Self-Review

**Spec coverage:**
- `ConnStatus::VersionMismatch` → Task 1, Step 3. ✓
- `ConnectOutcome::VersionMismatch` → Task 2, Step 1. ✓
- Detection (handle the `Err`) → Task 2, Step 2. ✓
- Classification (mismatch wins) → Task 2, Step 3. ✓
- Loop exit / stop retrying → Task 2, Step 4. ✓
- Surface on the existing `.status` line → Task 1, Step 4. ✓
- YAGNI (no version tag, no banner UI) → Global Constraints; nothing in the tasks adds them. ✓
- wasm test on board status rendering → Task 1, Steps 1–5. ✓
- Honest note that `connect_once` control flow isn't unit-tested → Task 2 preamble. ✓

**Placeholder scan:** No "TBD"/"handle errors"/"similar to" — every code step carries full before/after code and exact commands. ✓

**Type consistency:** `ConnStatus::VersionMismatch` (Task 1) is the exact name consumed in Task 2's detection and board arm; `ConnectOutcome::VersionMismatch` (Task 2 Step 1) matches the arm added in Step 4; `version_mismatch` flag named consistently across Steps 2–3. ✓
