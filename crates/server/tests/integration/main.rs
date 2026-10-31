//! The server's integration tests, built as one binary.
//!
//! Each file in this directory is a module of it, so `common`'s helpers are
//! checked for dead code once, against every test that could use them. A new
//! test file goes here, with its `mod` line below.

mod common;

mod accounts;
mod auth;
mod closing_demo;
mod config;
mod connect_pool;
mod game_session;
mod github;
mod health;
mod lifecycle;
mod migrations;
mod provider_sign_in;
mod registries;
mod resume;
mod setup_events;
mod static_files;
mod stored_game;
mod ws;
