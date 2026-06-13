//! Trapped (The Gathering Act 1, 01108).
//!
//! ```text
//! Act 1 — Trapped. Clues: 2.
//! (reverse) Put into play the set-aside Hallway, Cellar, Attic, and
//! Parlor. Discard each enemy in the Study. Place each investigator in
//! the Hallway. Remove the Study from the game.
//! ```
//!
//! The reverse side is a Forced on-advance ability: it fires via
//! `ForcedTriggerPoint::ActAdvanced` when the act advances, before the
//! next act becomes current. "Discard each enemy in the Study" is a
//! faithful **no-op** — nothing can spawn into the isolated Act-1 Study
//! in Slice-1 scope (location reveal-on-entry is TODO(#257); no encounter
//! path targets the Study). The set-aside locations + their connections
//! are built by the scenario's `setup()`; this ability just moves them
//! into play, relocates investigators to the Hallway (01112), and removes
//! the Study (01111).
//!
//! The board build is board-dependent, single-use scenario logic, so it
//! lives card-locally as a [`card_dsl::dsl::Effect::Native`] handler
//! (the `board_build` fn) rather than as shared `Effect` variants (#276).

use card_dsl::dsl::{native, on_event, Ability, EventPattern, EventTiming};
use game_core::card_registry::NativeEffectFn;
use game_core::{location_id_by_code, reveal_location, Cx, EngineOutcome, EvalContext, Event};

/// `ArkhamDB` code for Act 1, "Trapped".
pub const CODE: &str = "01108";

/// Native-effect tag for this act's reverse board build.
const BOARD_BUILD: &str = "01108:board-build";

/// 01108's Forced on-advance reverse: build the Act-1 board.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::ActAdvanced,
        EventTiming::After,
        native(BOARD_BUILD),
    )]
}

/// Resolve [`BOARD_BUILD`] if `tag` matches. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == BOARD_BUILD).then_some(board_build as NativeEffectFn)
}

/// Put the set-aside Hallway/Cellar/Attic/Parlor into play, relocate
/// every investigator to the Hallway (01112), and remove the Study
/// (01111). Ports the three former `Effect` arms verbatim, now
/// card-local. Rejects (leaving the board partially built — matching the
/// former `Seq` short-circuit) if 01112 or 01111 are not in play.
fn board_build(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    // Put set-aside locations into play.
    let drained = std::mem::take(&mut cx.state.set_aside_locations);
    for loc in drained {
        cx.state.locations.insert(loc.id, loc);
    }
    // Relocate all investigators to the Hallway (01112).
    let Some(dest) = location_id_by_code(cx.state, "01112") else {
        return EngineOutcome::Rejected {
            reason: "01108 board-build: no in-play Hallway (01112)".into(),
        };
    };
    let ids: Vec<_> = cx.state.investigators.keys().copied().collect();
    for id in ids {
        let inv = cx
            .state
            .investigators
            .get_mut(&id)
            .expect("id sourced from keys()");
        let from = inv.current_location;
        inv.current_location = Some(dest);
        if let Some(from_id) = from {
            if from_id != dest {
                cx.events.push(Event::InvestigatorMoved {
                    investigator: id,
                    from: from_id,
                    to: dest,
                });
            }
        }
    }
    reveal_location(cx, dest);
    // Remove the Study (01111) from the game.
    let Some(study) = location_id_by_code(cx.state, "01111") else {
        return EngineOutcome::Rejected {
            reason: "01108 board-build: no in-play Study (01111)".into(),
        };
    };
    cx.state.locations.remove(&study);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger};

    #[test]
    fn abilities_are_one_forced_on_advance_native_board_build() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After
            }
        );
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == "01108:board-build"),
            "board build is a card-local native effect, got {:?}",
            abilities[0].effect
        );
    }

    #[test]
    fn native_effect_for_resolves_only_the_board_build_tag() {
        assert!(super::native_effect_for("01108:board-build").is_some());
        assert!(super::native_effect_for("01108:other").is_none());
    }
}
