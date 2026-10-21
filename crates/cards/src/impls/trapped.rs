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
//! the act's own `ActAdvanced` condition when the act advances, before the
//! next act becomes current. "Discard each enemy in the Study" is not
//! modelled: an enemy in the Study when the act advances is left at the
//! removed location. The four rooms are
//! set aside by the scenario's `setup()` as bare card codes; this ability
//! puts each into play through
//! [`put_set_aside_card_into_play`](NativeCx::put_set_aside_card_into_play),
//! which mints its `LocationId` and wires its printed connections from the
//! scenario's layout table, then relocates investigators to the Hallway
//! (01112) and removes the Study (01111) through
//! [`remove_location_from_game`](NativeCx::remove_location_from_game), which
//! discards whatever is attached to it by owner.
//!
//! The board build is board-dependent, single-use scenario logic, so it
//! lives card-locally as a [`card_dsl::dsl::Effect::Native`] handler
//! (the `board_build` fn) rather than as shared `Effect` variants.
//!
//! **Cell: the `after` cell of the `ActAdvanced` condition.** The reverse prints
//! no trigger word, because it is not a triggered ability: it is step 2 of the
//! Rules Reference's advance procedure — *"Flip the advancing card over and
//! follow the instructions on the reverse ("b") side."*
//! (`glossary/Act_Deck_and_Agenda_Deck.md`). Declaring it in the `after` cell of
//! the flip is what puts it where the procedure puts it: after step 1's token
//! removal and the flip itself, and before step 3's *"the next card in the deck
//! becomes the current act/agenda"* — the `AdvanceReverse` frame holds the deck
//! cursor until its `Finalize` step, which it reaches only once the board build
//! has drained — the "before the next act becomes current" noted above. With no
//! printed word to read against, nothing contests the cell. The card has no
//! rulings (recorded in `data/arkhamdb-faq/no-rulings.txt`).

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::{self, EvalContext};
use game_core::engine::{EngineOutcome, NativeCx};
use game_core::state::{Owner, Status};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Act 1, "Trapped".
pub const CODE: &str = "01108";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(BOARD_BUILD, board_build)]);

/// The Study (01111), removed from the game by this reverse.
const STUDY: &str = "01111";

/// The Hallway (01112), where this reverse places each investigator.
const HALLWAY: &str = "01112";

/// The set-aside rooms this reverse puts into play, in printed order:
/// *"Put into play the set-aside Hallway, Cellar, Attic, and Parlor."*
/// The order is cosmetic — each room's connections are wired from the
/// scenario's layout as it enters, and a connection is made by whichever
/// of its two endpoints enters second.
const ROOMS: [&str; 4] = [HALLWAY, "01114", "01113", "01115"];

/// Native-effect tag for this act's reverse board build.
const BOARD_BUILD: &str = "01108:board-build";

/// 01108's Forced on-advance reverse: build the Act-1 board.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::ActAdvanced,
        EventTiming::After,
        dsl::native(BOARD_BUILD),
    )]
}

/// Put the set-aside Hallway/Cellar/Attic/Parlor into play, relocate
/// every investigator to the Hallway (01112), and remove the Study
/// (01111). Validates the whole set of rooms and the Study up front
/// and rejects before any mutation; a rejection is additionally rolled
/// back wholesale by `apply_via`'s snapshot-restore, so validate-first
/// here is about precise reasons, not state safety.
fn board_build(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    // Validate-first: all four rooms set aside, and 01111 in play.
    for code in ROOMS {
        if !cx
            .state()
            .set_aside_cards
            .iter()
            .any(|c| c.as_str() == code)
        {
            return EngineOutcome::Rejected {
                reason: format!("01108 board-build: {code} is not set aside").into(),
            };
        }
    }
    if evaluator::location_id_by_code(cx.state(), STUDY).is_none() {
        return EngineOutcome::Rejected {
            reason: format!("01108 board-build: no in-play Study ({STUDY})").into(),
        };
    }
    // Put the set-aside rooms into play; each mints its id and wires its
    // printed connections to the rooms already there.
    for code in ROOMS {
        // A location records no owner, so the owner argument is unused here.
        match cx.put_set_aside_card_into_play(code, None, Owner::Scenario) {
            EngineOutcome::Done => {}
            other => return other,
        }
    }
    // Relocate all investigators to the Hallway (01112).
    let Some(dest) = evaluator::location_id_by_code(cx.state(), HALLWAY) else {
        unreachable!("01108 board-build: the Hallway just entered play");
    };
    // A placement, not a move: no engaged enemy follows and no move trigger
    // fires. "Each investigator" is each one still in the game: a resigned or
    // defeated investigator has left the board and is not placed back onto it.
    let ids: Vec<_> = cx
        .state()
        .investigators
        .iter()
        .filter(|(_, i)| i.status == Status::Active)
        .map(|(&id, _)| id)
        .collect();
    for id in ids {
        cx.place_investigator(id, dest);
    }
    cx.reveal_location(dest);
    // Remove the Study (01111) from the game. Its attachments go with it, each
    // discarded by its owner (`glossary/Leaves_Play.md`).
    let Some(study) = evaluator::location_id_by_code(cx.state(), STUDY) else {
        unreachable!("01108 board-build: the Study was validated in play above");
    };
    let removed = cx.remove_location_from_game(study);
    debug_assert!(
        removed,
        "01108 board-build: the Study was just found in play"
    );
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger, TriggerKind};

    #[test]
    fn abilities_are_one_forced_on_advance_native_board_build() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After,
                kind: TriggerKind::Forced,
            }
        );
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == "01108:board-build"),
            "board build is a card-local native effect, got {:?}",
            abilities[0].effect
        );
    }
}
