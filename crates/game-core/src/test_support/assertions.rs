//! Order-insensitive event assertion macros.
//!
//! Tests assert on what happened, not on the order events fired in.
//! The macros below take a slice or `Vec<Event>` as the first argument
//! and a Rust pattern (with optional `if` guard) as the second; on
//! failure they print the actual event list to make the mismatch
//! obvious.
//!
//! # Examples
//!
//! ```
//! use game_core::{
//!     apply, Action, Event, InvestigatorId, PlayerAction, Phase,
//!     assert_event, assert_no_event,
//!     test_support::{test_investigator, TestGame},
//! };
//!
//! let state = TestGame::new()
//!     .with_phase(Phase::Investigation)
//!     .with_investigator(test_investigator(1))
//!     .with_active_investigator(InvestigatorId(1))
//!     .build();
//! let result = apply(state, Action::Player(PlayerAction::EndTurn));
//!
//! assert_event!(result.events, Event::TurnEnded { .. });
//! assert_no_event!(result.events, Event::ScenarioStarted);
//! ```

/// Assert that at least one [`Event`](crate::Event) in the slice
/// matches the given pattern (with optional guard).
///
/// On failure, panics with the pattern source plus a debug-printed
/// list of actual events.
#[macro_export]
macro_rules! assert_event {
    ($events:expr, $pat:pat $(if $guard:expr)?) => {{
        let events = &$events;
        let matched = events.iter().any(|e| matches!(e, $pat $(if $guard)?));
        if !matched {
            panic!(
                "assert_event!: no event matching `{}` in:\n{:#?}",
                stringify!($pat $(if $guard)?),
                events,
            );
        }
    }};
}

/// Assert that NO [`Event`](crate::Event) in the slice matches the
/// given pattern (with optional guard).
///
/// On failure, panics with the pattern source plus a debug-printed
/// list of actual events.
#[macro_export]
macro_rules! assert_no_event {
    ($events:expr, $pat:pat $(if $guard:expr)?) => {{
        let events = &$events;
        let matched: Vec<&_> = events
            .iter()
            .filter(|e| matches!(e, $pat $(if $guard)?))
            .collect();
        if !matched.is_empty() {
            panic!(
                "assert_no_event!: expected no event matching `{}`, but found:\n{:#?}\nin full list:\n{:#?}",
                stringify!($pat $(if $guard)?),
                matched,
                events,
            );
        }
    }};
}

/// Assert that exactly `$count` [`Event`](crate::Event)s in the slice
/// match the given pattern (with optional guard).
///
/// On failure, panics with the pattern source, the expected count, the
/// actual count, and a debug-printed list of all events.
#[macro_export]
macro_rules! assert_event_count {
    ($events:expr, $count:expr, $pat:pat $(if $guard:expr)?) => {{
        let events = &$events;
        let expected: usize = $count;
        let actual = events
            .iter()
            .filter(|e| matches!(e, $pat $(if $guard)?))
            .count();
        if actual != expected {
            panic!(
                "assert_event_count!: expected {} events matching `{}`, found {}, in:\n{:#?}",
                expected,
                stringify!($pat $(if $guard)?),
                actual,
                events,
            );
        }
    }};
}
