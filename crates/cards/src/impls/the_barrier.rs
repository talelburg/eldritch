//! The Barrier (The Gathering Act 2, 01109).
//!
//! ```text
//! Act 2 — The Barrier. Clues: 3.
//! Objective - When the round ends, investigators in the hallway may, as
//!   a group, spend the requisite number of clues to advance.
//! (reverse) The barrier blocking passage into the parlor has vanished.
//!   Reveal the Parlor.
//!   Put the set-aside Lita Chantler into play in the Parlor.
//!   Spawn the set-aside Ghoul Priest in the Hallway.
//! ```
//!
//! This module implements **both** of 01109's abilities. The **front
//! objective** (the round-end clue-spend advance) is a `When`-`RoundEnded`
//! reaction whose native does the group clue-spend + `advance_act`. The
//! `RoundEnded` `EmitEvent` coordinator surfaces it as the round-end `when`-cell
//! reaction candidate; picking it fires this native, `Skip` declines.
//! The contributor location (the Hallway, `01112`) is printed-in-card — passed
//! to the native directly — not a framework data field.
//!
//! The **reverse** is a Forced on-advance ability that fires via
//! the act's own `ActAdvanced` condition when the act advances
//! (`glossary/Act_Deck_and_Agenda_Deck.md`: flip the card, follow the
//! reverse). It reveals
//! the Parlor (01115), puts the set-aside Lita Chantler (01117) into play there,
//! and spawns the set-aside Ghoul Priest (01116) in the Hallway (01112), making
//! act 3's "If the Ghoul Priest is Defeated, advance" objective reachable in
//! real play.
//!
//! All three lines are modelled. Lita arrives in the Parlor's
//! `cards_at_location` zone under nobody's control, where the Parlor's grant
//! gives her the Parley and her own card's grant waits for a controller.
//!
//! Like 01108's board build, the reverse is board-dependent, single-use
//! scenario logic, so it lives card-locally as a [`card_dsl::dsl::Effect::Native`]
//! handler rather than as shared `Effect` variants. The spawn reuses
//! the engine's [`NativeCx::put_set_aside_card_into_play`], which reads
//! the Priest's cardtype back from the corpus metadata and mints his
//! combat stats / keywords / per-investigator health from it.
//!
//! **Cell: the `when` cell of the `RoundEnded` condition**, for the front
//! objective. The printed word is *"When"* — *"**Objective** - When the round
//! ends, investigators in the hallway may, as a group, spend the requisite
//! number of clues to advance."* — and `glossary/When.md` puts that
//! *"immediately after the specified timing point or triggering condition
//! initiates, but before its impact upon the game state resolves."* The round's
//! ending is a **bare milestone** — "until the end of the round" lasting
//! effects have expired just before it, and the Upkeep → Mythos handoff runs
//! after the whole sequence, on the Upkeep anchor's resume (ADR 0008) — so this
//! is the first cell of a sequence that changes
//! nothing as it resolves, and the clue-spend happens with the board exactly as
//! the round's end found it. Agenda 01107's `at`-cell round-end doom follows it,
//! in that card's own ruling: *""At the end of the round" effects trigger after
//! "When the round ends" effects (e.g. The Barrier)"*
//! (<https://arkhamdb.com/card/01107>).
//!
//! **Cell: the `after` cell of the `ActAdvanced` condition**, for the reverse.
//! The reverse prints no trigger word, because it is not a triggered ability: it
//! is step 2 of the advance procedure — *"Flip the advancing card over and
//! follow the instructions on the reverse ("b") side."*
//! (`glossary/Act_Deck_and_Agenda_Deck.md`). Declaring it in the `after` cell of
//! the flip is what puts it where the procedure puts it: after step 1's token
//! removal and the flip itself, and before step 3's *"the next card in the deck
//! becomes the current act/agenda"* — the `AdvanceReverse` frame holds the deck
//! cursor until its `Finalize` step, which it reaches only once the Parlor
//! reveal and the Priest spawn have drained. With no printed word to read
//! against, nothing contests the cell. This act has no rulings (recorded in
//! `data/arkhamdb-faq/no-rulings.txt`).
//!
//! # Module gap
//!
//! With one investigator in the Hallway the Priest's `Prey - Highest
//! [combat]` resolves to that lone investigator and the spawn engages inline
//! (`Done`). Two investigators in the Hallway tied on combat make the spawn
//! suspend (`AwaitingInput`) after the reverse has written, so the native is
//! rejected and the act does not advance. TODO(#1139): advance, and ask whom
//! the Priest engages.

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::{self, EvalContext};
use game_core::engine::{self, EngineOutcome, NativeCx};
use game_core::state::{GameState, Owner};

use crate::impls::{lita_chantler, parlor, CardRecord};

/// `ArkhamDB` code for Act 2, "The Barrier".
pub const CODE: &str = "01109";

/// This card's registration, listed in [`ALL`](super::ALL).
///
/// The round-end advance is offered only when the Hallway (01112) group can
/// afford the act's clue threshold, which is what its eligibility condition
/// checks.
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[
        (REVERSE, reverse),
        (ROUND_END_ADVANCE, advance_via_clue_spend),
    ])
    .conditions(&[(CAN_ADVANCE, can_advance)]);

/// Native-effect tag for this act's reverse.
const REVERSE: &str = "01109:reverse";

/// Native-effect tag for the front objective's round-end group clue-spend
/// advance ("When the round ends, investigators in the hallway may, as a group,
/// spend the requisite number of clues to advance").
const ROUND_END_ADVANCE: &str = "01109:round-end-advance";

/// Native-condition tag, the round-end advance's eligibility: it may be offered
/// only when the Hallway group can afford the act's clue threshold.
/// `glossary/Ability.md`: *"A triggered ability can only be initiated if its
/// effect has the potential to change the game state"*.
///
/// A **stand-in** for the change-state check: that check reads an ability's
/// effect, and [`ROUND_END_ADVANCE`] is an opaque native whose group clue spend
/// it can't see into, so the card supplies the affordability question itself.
const CAN_ADVANCE: &str = "01109:can-advance";

/// Printed codes the reverse touches.
const GHOUL_PRIEST: &str = "01116";
const HALLWAY: &str = "01112";

/// 01109's two abilities:
/// - the **front objective** — "When the round ends, investigators in the
///   Hallway may, as a group, spend the requisite number of clues to advance":
///   a `When`-timed `RoundEnded` reaction. The round-end `When` window offers
///   this as a single candidate (`PickSingle` = advance, Skip = decline); the
///   native spends + advances. Affordability is gated by the `01109:can-advance`
///   eligibility condition (shared with the resolve-side
///   [`round_end_advance_affordable`](engine::round_end_advance_affordable)),
///   so the candidate isn't offered when the
///   Hallway group can't afford the clue threshold.
/// - the **reverse** — a Forced on-advance ability (reveal the Parlor + spawn
///   the Priest) that fires when the act advances.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::forced_on_event(
            EventPattern::ActAdvanced,
            EventTiming::After,
            dsl::native(REVERSE),
        ),
        dsl::reaction_on_event(
            EventPattern::RoundEnded,
            EventTiming::When,
            dsl::native(ROUND_END_ADVANCE),
        )
        .with_eligibility(dsl::native_condition(CAN_ADVANCE)),
    ]
}

/// True when the Hallway group can afford the current act's clue threshold —
/// the offer-side gate shared with the resolve-side `round_end_advance`.
fn can_advance(state: &GameState, _ctx: &EvalContext) -> bool {
    engine::round_end_advance_affordable(state, HALLWAY)
}

/// Front-objective native: spend the act's `clue_threshold` from Hallway
/// (01112) investigators, then advance the act. Synchronous — the player's
/// choice was the round-end `When` window's `PickSingle`. Delegates to the
/// engine's generic group-spend entry, passing the printed contributor location.
fn advance_via_clue_spend(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.round_end_advance(HALLWAY)
}

/// Resolve the reverse **in printed order**: reveal the Parlor (01115), put the
/// set-aside Lita Chantler (01117) into play there, then spawn the set-aside
/// Ghoul Priest (01116) in the Hallway (01112).
///
/// Validate-first is an explicit **up-front check of every precondition**,
/// not a consequence of the ordering: the Parlor must be in play, the Priest
/// must still be set aside, and the Hallway must be in play, all confirmed
/// before the reveal mutates anything. The Parlor's `revealed` flag is the
/// barrier's gate, so with the reveal first, a spawn that rejected halfway
/// would leave the barrier lifted on a board the reverse never finished.
/// `put_set_aside_card_into_play`
/// re-validates the same two conditions internally, and it is that check —
/// not the ordering — that keeps the board safe. Lita's own precondition joins
/// the up-front block for the same reason.
///
/// She lands in the Parlor **under nobody's control**, which is what the
/// Parlor's own printed grant is written against: *"While Lita Chantler is not
/// controlled by a player, she gains: '\[action\]: **Parley.** …'"*.
fn reverse(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    let Some(parlor_id) = evaluator::location_id_by_code(cx.state(), parlor::CODE) else {
        return EngineOutcome::Rejected {
            reason: "01109 reverse: Parlor (01115) not in play".into(),
        };
    };
    if !cx
        .state()
        .set_aside_cards
        .iter()
        .any(|c| c.as_str() == GHOUL_PRIEST)
    {
        return EngineOutcome::Rejected {
            reason: "01109 reverse: Ghoul Priest (01116) is not set aside".into(),
        };
    }
    if !cx
        .state()
        .set_aside_cards
        .iter()
        .any(|c| c.as_str() == lita_chantler::CODE)
    {
        return EngineOutcome::Rejected {
            reason: "01109 reverse: Lita Chantler (01117) is not set aside".into(),
        };
    }
    if evaluator::location_id_by_code(cx.state(), HALLWAY).is_none() {
        return EngineOutcome::Rejected {
            reason: "01109 reverse: Hallway (01112) not in play".into(),
        };
    }
    // All checks passed — mutate, in the order the card prints.
    cx.reveal_location(parlor_id);
    // Lita has no pile to return to, so she is the scenario's; the Ghoul
    // Priest is an encounter-set enemy, so he is the encounter deck's
    // (`GLOSSARY.md`, **Owner / Controller**).
    let lita =
        cx.put_set_aside_card_into_play(lita_chantler::CODE, Some(parlor::CODE), Owner::Scenario);
    if !matches!(lita, EngineOutcome::Done) {
        return lita;
    }
    cx.put_set_aside_card_into_play(GHOUL_PRIEST, Some(HALLWAY), Owner::EncounterDeck)
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger, TriggerKind};

    #[test]
    fn abilities_are_forced_reverse_then_when_round_end_advance() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        // [0] the reverse: Forced on-advance native.
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After,
                kind: TriggerKind::Forced,
            }
        );
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == "01109:reverse"),
            "the reverse is a card-local native effect, got {:?}",
            abilities[0].effect
        );
        // [1] the front objective: a When-timed RoundEnded reaction → native.
        assert_eq!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::RoundEnded,
                timing: EventTiming::When,
                kind: TriggerKind::Reaction,
            }
        );
        assert!(
            matches!(&abilities[1].effect, Effect::Native { tag } if tag == "01109:round-end-advance"),
            "the round-end advance is a card-local native effect, got {:?}",
            abilities[1].effect
        );
    }
}
