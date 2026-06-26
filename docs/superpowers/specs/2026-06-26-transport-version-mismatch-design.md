# #463 — Surface client/server version mismatch instead of hanging silently

**Date:** 2026-06-26
**Issue:** #463 (web: transport silently drops un-parseable server messages)
**Follow-up from:** #205 / PR #462 (the wire-format change that exposed this)

## Problem

The browser transport silently swallows any `ServerMessage` it can't deserialize,
turning a client/server **version skew** into an invisible hang: the board renders
`<no game>` forever with no error.

In `crates/web/src/transport.rs`, `connect_once` does:

```rust
Some(Ok(Message::Text(txt))) => {
    if let Ok(msg) = serde_json::from_str::<ServerMessage>(&txt) {
        saw_hello |= matches!(msg, ServerMessage::Hello { .. });
        store.update(|s| reduce(s, msg));
    }
    // else: dropped silently
}
```

When a freshly-rebuilt client connects to a **stale server binary** (e.g. forgetting
to restart `cargo run -p server` after a wire-format change), every `Hello`/`Applied`
the old server sends is in the old shape. `from_str::<ServerMessage>` returns `Err`,
the frame is dropped, `game` stays `None`, `saw_hello` stays false — but the socket
is still open, so the connection never breaks and never reaches the `StaleId`
self-heal. The user sits on a blank `<no game>` page indefinitely.

This was discovered while shipping #205: a running pre-change server + a hot-reloaded
client reproduced the silent hang, and the invisible failure mode cost real debugging
time.

## Solution

Detect the un-parseable frame, surface it as an actionable status, and stop retrying
(a retry just hits the same stale server).

### 1. `ConnStatus::VersionMismatch` (`crates/web/src/store.rs`)

A new terminal variant on the connection-lifecycle enum:

```rust
pub enum ConnStatus {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
    Failed,
    AwaitingRoster,
    /// A server frame failed to deserialize — the client and server binaries
    /// disagree on the wire format. Terminal: restart the server and reload.
    VersionMismatch,
}
```

### 2. `ConnectOutcome::VersionMismatch` (`crates/web/src/transport.rs`)

A new outcome variant so `connect_once` can tell the reconnect loop to stop:

```rust
enum ConnectOutcome {
    Unreachable,
    StaleId,
    Disconnected,
    /// A frame failed to deserialize (wire-format skew). Terminal — do not retry.
    VersionMismatch,
}
```

### 3. Detection (`connect_once`)

Replace the silent `if let Ok(msg)` with explicit `Err` handling. On the first
un-parseable text frame, set the status, break the read loop, and return
`VersionMismatch`:

```rust
Some(Ok(Message::Text(txt))) => match serde_json::from_str::<ServerMessage>(&txt) {
    Ok(msg) => {
        saw_hello |= matches!(msg, ServerMessage::Hello { .. });
        store.update(|s| reduce(s, msg));
    }
    Err(_) => {
        store.update(|s| s.status = ConnStatus::VersionMismatch);
        version_mismatch = true;
        break;
    }
},
```

with a `let mut version_mismatch = false;` alongside `saw_hello`, and the post-loop
classification ordered so the mismatch wins:

```rust
if version_mismatch {
    ConnectOutcome::VersionMismatch
} else if saw_hello {
    ConnectOutcome::Disconnected
} else {
    ConnectOutcome::StaleId
}
```

Rationale: a frame we can't parse from a server that *is* talking to us is a
wire-format skew, not a transient glitch — there's no value in continuing to read.

### 4. Loop behavior (`start`)

Add a match arm that exits the reconnect loop entirely, *before* the
`status = Reconnecting` line, leaving the `VersionMismatch` status visible:

```rust
match connect_once(&store, &game_id, &mut rx).await {
    ConnectOutcome::VersionMismatch => return, // terminal; leave the status set
    ConnectOutcome::StaleId => { /* unchanged self-heal */ }
    ConnectOutcome::Unreachable | ConnectOutcome::Disconnected => {}
}
```

### 5. Surface (`crates/web/src/board.rs`)

`BoardView` already renders an always-present `.status` line that maps each
`ConnStatus` to a string. Add the arm with an actionable message:

```rust
ConnStatus::VersionMismatch => "version mismatch — restart the server and reload",
```

No new DOM element — reuse the existing `.status` line. This is the minimal,
in-keeping win the issue asks for ("don't hang silently — show *something*").

## Out of scope (YAGNI)

- **No protocol version tag / handshake.** Inferring the skew from the
  deserialization `Err` is sufficient and needs no protocol change. #463 lists the
  version tag as optional; skip it.
- **No dedicated banner UI / styling.** The existing status line carries the message.

## Testing

- **wasm test** (extends `crates/web/tests/board.rs`): drive the store to
  `status = ConnStatus::VersionMismatch` and assert `BoardView`'s `.status` line
  renders the actionable text. This guards the user-visible contract.
- The `connect_once` detection + loop-exit is wasm-only async control flow over a
  live `WebSocket` with no existing test harness (the rest of `transport.rs` is
  likewise not unit-tested). Covered by implementation review, not a fabricated
  socket harness — called out honestly rather than papered over.
- Full CI gauntlet (all seven jobs, strict flags) before push; `wasm-test` /
  `wasm-clippy` matter since this is `crates/web`.

## Done criteria

- A client receiving an un-parseable server frame shows
  `status: version mismatch — restart the server and reload` instead of a silent
  `<no game>`, and stops reconnect-looping.
- All seven CI jobs green.
