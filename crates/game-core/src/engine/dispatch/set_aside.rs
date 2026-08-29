//! The set-aside zone: bringing a set-aside card into play.
//!
//! Rules Reference, `Set Aside`: *"Cards that are set aside are placed
//! outside the play area, and are considered out of play. Such cards are
//! generally brought into play at a specific point in a scenario."*
//!
//! The zone is [`GameState::set_aside_cards`](crate::state::GameState::set_aside_cards)
//! — printed codes, one collection for every cardtype. Nothing is built at
//! `setup()`, because nothing *can* be: an enemy's per-investigator health
//! needs the live investigator count, and a location's [`LocationId`] is
//! minted on entry. [`put_set_aside_card_into_play`] is the single door
//! back in, and it dispatches on the metadata's
//! [`CardKind`](crate::card_data::CardKind).
//!
//! A location's connections come with it: they are printed as connection
//! symbols on the cards, which the pinned snapshot has no field for, so
//! they live in the active scenario's
//! [`LocationLayout`](crate::scenario::LocationLayout) and are wired at
//! entry — the only moment both endpoints of a connection have ids.

use crate::card_data::CardKind;
use crate::card_registry;
use crate::engine::dispatch::encounter::spawn_enemy_at;
use crate::engine::dispatch::threat_area::put_into_play_at_location;
use crate::engine::{location_id_by_code, Cx, EngineOutcome};
use crate::scenario::scenario_layout;
use crate::state::{CardCode, GameState, LocationId};

/// Bring the set-aside card `code` into play, dispatching on its printed
/// cardtype:
///
/// - **Location** — minted into play (`at` must be `None`; a location
///   brings its own place), then wired to every in-play neighbour the
///   active scenario's layout pairs it with.
/// - **Enemy** — spawned at the location named by `at`, minting its stats
///   from the corpus so per-investigator health scales by the live
///   investigator count.
/// - **Asset** — put into play **at** the location named by `at`, in that
///   location's `cards_at_location` zone and under **no investigator's
///   control**. Act 01109b's *"Put the set-aside Lita Chantler into play in
///   the Parlor"* is the shape; `glossary/In_Play_and_Out_of_Play.md`
///   counts *"each encounter card in a investigator's threat area **or at a
///   location**"* as in play, so she is in play with no controller until
///   somebody takes control of her.
///
/// Validate-first: rejects, mutating nothing, if `code` isn't in the
/// set-aside zone, no card registry is installed, the code has no
/// metadata, the cardtype's target argument is wrong, or the named spawn
/// location isn't in play. A rejection is additionally rolled back
/// wholesale by `apply_via`'s snapshot-restore, so validate-first here is
/// about precise reasons, not state safety.
pub fn put_set_aside_card_into_play(cx: &mut Cx, code: &str, at: Option<&str>) -> EngineOutcome {
    let Some(pos) = cx
        .state
        .set_aside_cards
        .iter()
        .position(|c| c.as_str() == code)
    else {
        return EngineOutcome::Rejected {
            reason: format!("put_set_aside_card_into_play: {code} is not set aside").into(),
        };
    };
    let Some(registry) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "put_set_aside_card_into_play: no card registry installed".into(),
        };
    };
    let Some(metadata) = (registry.metadata_for)(&CardCode::new(code)) else {
        return EngineOutcome::Rejected {
            reason: format!("put_set_aside_card_into_play: no metadata for {code}").into(),
        };
    };
    match metadata.kind {
        CardKind::Location { .. } => {
            if at.is_some() {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "put_set_aside_card_into_play: location {code} enters play as its own \
                         place, so it takes no target location"
                    )
                    .into(),
                };
            }
            // All checks passed — mutate.
            cx.state.set_aside_cards.remove(pos);
            let id = cx.state.add_location(metadata);
            wire_layout_connections(cx.state, id);
            EngineOutcome::Done
        }
        CardKind::Enemy { .. } => {
            let Some(location_code) = at else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "put_set_aside_card_into_play: enemy {code} needs a spawn location"
                    )
                    .into(),
                };
            };
            let Some(location_id) = location_id_by_code(cx.state, location_code) else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "put_set_aside_card_into_play: location {location_code} not in play"
                    )
                    .into(),
                };
            };
            // All checks passed — mutate.
            cx.state.set_aside_cards.remove(pos);
            spawn_enemy_at(cx, CardCode::new(code), metadata, location_id)
        }
        CardKind::Asset { .. } => {
            let Some(location_code) = at else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "put_set_aside_card_into_play: asset {code} needs a location to be put \
                         into play at"
                    )
                    .into(),
                };
            };
            let Some(location_id) = location_id_by_code(cx.state, location_code) else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "put_set_aside_card_into_play: location {location_code} not in play"
                    )
                    .into(),
                };
            };
            // All checks passed — mutate.
            cx.state.set_aside_cards.remove(pos);
            put_into_play_at_location(cx, location_id, CardCode::new(code));
            EngineOutcome::Done
        }
        ref kind => EngineOutcome::Rejected {
            reason: format!(
                "TODO(#824): a set-aside {kind:?} ({code}) needs a put-into-play path \
                 (lands with #824)"
            )
            .into(),
        },
    }
}

/// Wire `id`'s printed connections from the active scenario's
/// [`LocationLayout`](crate::scenario::LocationLayout): for every pair
/// naming `id`'s code, connect it to the paired location if that one is
/// already in play.
///
/// Connections are bidirectional, so the pass is symmetric — whichever of
/// a pair enters second is the one that wires it, and a pair whose other
/// half never enters play stays unwired.
fn wire_layout_connections(state: &mut GameState, id: LocationId) {
    let code = state.locations[&id].code.as_str().to_owned();
    let layout = scenario_layout(state);
    let neighbours: Vec<LocationId> = layout
        .iter()
        .filter_map(|&(a, b)| {
            let other = if a == code {
                b
            } else if b == code {
                a
            } else {
                return None;
            };
            location_id_by_code(state, other)
        })
        .filter(|&n| n != id)
        .collect();
    for neighbour in neighbours {
        state.connect(id, neighbour);
    }
}

#[cfg(test)]
mod tests {
    use super::put_set_aside_card_into_play;
    use crate::engine::{Cx, EngineOutcome};
    use crate::state::{CardCode, InvestigatorId};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn rejects_a_code_that_is_not_set_aside() {
        // Empty set-aside zone — the call must reject before touching the
        // registry or the board, and mint nothing.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([InvestigatorId(1)])
            .build();
        let mut events = Vec::new();
        let outcome = put_set_aside_card_into_play(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            "01116",
            Some("01112"),
        );
        assert!(
            matches!(outcome, EngineOutcome::Rejected { .. }),
            "a code that isn't set aside must reject, got {outcome:?}",
        );
        assert!(state.enemies.is_empty(), "no enemy minted on reject");
        assert!(state.locations.is_empty(), "no location minted on reject");
    }

    #[test]
    fn keeps_the_code_aside_on_a_failed_entry() {
        // The card is set aside, but a bare unit test installs no card
        // registry — the call must reject without removing the code from the
        // zone (validate-first: no mutation on reject).
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([InvestigatorId(1)])
            .build();
        state.set_aside_cards.push(CardCode::new("01116"));
        let mut events = Vec::new();
        let outcome = put_set_aside_card_into_play(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            "01116",
            Some("01112"), // not in play
        );
        assert!(
            matches!(outcome, EngineOutcome::Rejected { .. }),
            "an entry that cannot complete must reject, got {outcome:?}",
        );
        assert_eq!(
            state.set_aside_cards,
            vec![CardCode::new("01116")],
            "the code stays set aside when the entry rejects",
        );
        assert!(state.enemies.is_empty(), "no enemy minted on reject");
    }
}
