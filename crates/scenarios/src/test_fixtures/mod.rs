//! Synthetic / minimal scenario fixtures.
//!
//! These exist only to exercise the engine's scenario-module wiring;
//! they are *not* part of any shipped campaign. Gated behind
//! `cfg(any(test, feature = "test_fixtures"))` at the crate root —
//! **note the feature is in this crate's `default` set and the server
//! enables it explicitly** (deliberate while the demo scenario doubles
//! as production content), so the fixtures DO ship today; revisit
//! dropping the default when the phase-7 registry swap fully retires
//! the synthetic scenario.

pub mod synth_cards;
pub mod synthetic;
