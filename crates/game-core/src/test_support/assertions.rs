//! Event assertion macros.
//!
//! The order-insensitive macros — [`assert_event!`](crate::assert_event),
//! [`assert_no_event!`](crate::assert_no_event), and
//! [`assert_event_count!`](crate::assert_event_count) — take a slice
//! or `Vec<Event>` as the first argument and a Rust pattern (with
//! optional `if` guard) as the second. Most tests assert on what
//! happened, not on the order events fired in; reach for these first.
//!
//! **When event order matters** between a few specific events with
//! others interleaved (e.g. "`CardPlayed` precedes `CluePlaced`
//! precedes `CardDiscarded`, with arbitrary events in between"), use
//! [`assert_event_sequence!`](crate::assert_event_sequence). When you
//! need an exact sequence with no events allowed between or around,
//! fall back to plain `assert_eq!` on the slice.
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
        let matched = events.iter().any(|__event| matches!(__event, $pat $(if $guard)?));
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
            .filter(|__event| matches!(__event, $pat $(if $guard)?))
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
            .filter(|__event| matches!(__event, $pat $(if $guard)?))
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

/// Assert that the listed patterns each match at least one event in
/// the slice **and** that a matching event for the first pattern
/// precedes a matching event for the second, the second precedes the
/// third, and so on.
///
/// Other events may appear before, between, or after the matched
/// ones — this is a *subsequence* check, not a contiguous-sequence
/// check. Use plain `assert_eq!` on the slice if you need an exact
/// contiguous order with no events between or around.
///
/// On failure, panics with the pattern source plus a debug-printed
/// list of all events.
///
/// # Example
///
/// ```ignore
/// assert_event_sequence!(
///     result.events,
///     Event::CardPlayed { .. },
///     Event::CluePlaced { .. },
///     Event::CardDiscarded { .. },
/// );
/// ```
#[macro_export]
macro_rules! assert_event_sequence {
    ($events:expr, $($pat:pat $(if $guard:expr)?),+ $(,)?) => {{
        let events = &$events;
        // The cursor advances past each match; the last iteration's
        // assignment is naturally unused (there are no more patterns
        // after it), so allow the dead-store warning that fires on
        // that last assignment.
        #[allow(unused_assignments)]
        {
            let mut cursor: usize = 0;
            $(
                let found = events
                    .iter()
                    .enumerate()
                    .skip(cursor)
                    .find(|(_, __event)| matches!(__event, $pat $(if $guard)?));
                match found {
                    Some((idx, _)) => {
                        cursor = idx + 1;
                    }
                    None => {
                        panic!(
                            "assert_event_sequence!: no event matching `{}` found at or after \
                             position {} in:\n{:#?}",
                            stringify!($pat $(if $guard)?),
                            cursor,
                            events,
                        );
                    }
                }
            )+
        }
    }};
}

#[cfg(test)]
mod tests {
    use crate::event::Event;
    use crate::state::{InvestigatorId, LocationId};

    fn investigator_id() -> InvestigatorId {
        InvestigatorId(1)
    }

    fn sample_events() -> Vec<Event> {
        let id = investigator_id();
        vec![
            Event::ResourcesGained {
                investigator: id,
                amount: 1,
            },
            Event::CluePlaced {
                investigator: id,
                count: 1,
            },
            Event::LocationCluesChanged {
                location: LocationId(101),
                new_count: 0,
            },
            Event::TurnEnded { investigator: id },
        ]
    }

    #[test]
    fn assert_event_sequence_passes_when_subsequence_in_order() {
        let events = sample_events();
        crate::assert_event_sequence!(
            events,
            Event::ResourcesGained { .. },
            Event::CluePlaced { .. },
            Event::TurnEnded { .. },
        );
    }

    #[test]
    fn assert_event_sequence_passes_with_single_pattern() {
        let events = sample_events();
        crate::assert_event_sequence!(events, Event::CluePlaced { .. });
    }

    #[test]
    #[should_panic(expected = "no event matching")]
    fn assert_event_sequence_panics_when_pattern_missing() {
        let events = sample_events();
        crate::assert_event_sequence!(events, Event::ScenarioStarted);
    }

    #[test]
    #[should_panic(expected = "no event matching")]
    fn assert_event_sequence_panics_when_order_is_wrong() {
        let events = sample_events();
        // TurnEnded fires after CluePlaced in `sample_events`, so this
        // order is impossible to satisfy as a subsequence.
        crate::assert_event_sequence!(events, Event::TurnEnded { .. }, Event::CluePlaced { .. },);
    }

    #[test]
    fn assert_event_sequence_respects_guards() {
        let events = sample_events();
        let id = investigator_id();
        crate::assert_event_sequence!(
            events,
            Event::ResourcesGained { investigator, amount: 1 } if *investigator == id,
            Event::CluePlaced { investigator, .. } if *investigator == id,
        );
    }
}
