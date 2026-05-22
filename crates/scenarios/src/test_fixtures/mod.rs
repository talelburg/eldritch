//! Synthetic / minimal scenario fixtures.
//!
//! These exist only to exercise the engine's scenario-module wiring;
//! they are *not* part of any shipped campaign. Gated behind
//! `cfg(any(test, feature = "test_fixtures"))` at the crate root so
//! they never ship in a release build.

pub mod synthetic;
