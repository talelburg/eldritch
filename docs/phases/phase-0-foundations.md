# Phase 0 — Foundations

## Status

✅ Closed.

## Goal

Empty repo with green CI and a working dev loop.

## Issues (all closed)

- `#1` — license + replace stub README with disclaimer + quick-start.
- `#2` — `.gitignore` + `rust-toolchain.toml`.
- `#3` — cargo workspace skeleton with six crates (`game-core`, `cards`, `scenarios`, `server`, `web`, `card-data-pipeline`).
- `#4` — hello-world Axum endpoint on `server` crate.
- `#5` — hello-world Leptos page on `web` crate.
- `#6` — choose Leptos toolchain (Trunk vs. cargo-leptos), document dev loop.
- `#7` — CI workflow (`fmt`, `clippy`, `test`, `doc`, `wasm-build`).
- `#8` — issue + PR templates.
- `#9` — Dependabot config.

## Decisions made

- **Workspace layout** of six crates, organized by work-type seams: engine churn (`game-core`) vs. content (`cards`, `scenarios`) vs. UI (`web`) vs. server (`server`) vs. tooling (`card-data-pipeline`). Each crate boundary lets that kind of work iterate without forcing the others to recompile.
- **Toolchain pinned via `rust-toolchain.toml`** so contributors and CI use the same Rust version.
- **Dev loop** is two terminals (server on `:8000`, Trunk serving the web dep on `:3000` with proxy back to the server) — pure CSR, no SSR. The web crate is behind auth in production; SEO isn't a concern.
- **CI matrix** is five jobs (`fmt`, `clippy`, `test`, `doc`, `wasm-build`), all with warnings-as-errors. `--all-features` is on for `test` so feature-gated test-support modules build.

## Dependencies

None. This is the bottom of the stack.

## What "done" looked like

`cargo test --all --all-features` green; `cargo build -p web --target wasm32-unknown-unknown` green; CI on every PR green.
