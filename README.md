# Eldritch

Digital simulator for the Arkham Horror Living Card Game. Lets a small group play through campaigns and scenarios in the browser, with a full rules engine that knows the cards.

> **Unofficial fan tool. Not affiliated with Fantasy Flight Games or Asmodee. Arkham Horror is a trademark of Fantasy Flight Games.**

## Status

Early development. Not yet playable. Tracking work via issues and milestones; see the project board.

## Quick start

Requires Rust stable (pinned via `rust-toolchain.toml`) and the `wasm32-unknown-unknown` target. The `rust-toolchain.toml` will set both up automatically on `cargo` invocation.

Install the dev-loop tool (CI pins `trunk@0.21.14`; match it to avoid behavior drift):

```sh
cargo install --locked trunk --version 0.21.14
```

Run the server:

```sh
cargo run -p server
# → Axum on http://localhost:8000
```

Run the web dev server (in another terminal):

```sh
cd crates/web
trunk serve
# → http://localhost:3000 (REST + WS proxy config lives in crates/web/Trunk.toml —
#   don't pass --proxy-backend: a root proxy panics on trunk 0.21.x)
```

Run the test suite:

```sh
cargo test --all
```

## Repo layout

```
crates/
├── card-dsl/            # effect DSL + static card metadata types (pure data)
├── game-core/           # rules engine, no I/O
├── cards/               # card definitions (DSL + Rust)
├── scenarios/           # scenario modules + campaign orchestrators
├── protocol/            # client↔server wire messages
├── server/              # Axum binary, websocket hub, persistence
├── web/                 # Leptos WASM client
└── card-data-pipeline/  # CLI for ingesting ArkhamDB metadata
data/
└── arkhamdb-snapshot/   # pinned card data, manually updated
```

## License

MIT. See `LICENSE`.
