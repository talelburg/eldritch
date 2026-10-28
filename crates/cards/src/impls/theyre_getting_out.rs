//! They're Getting Out! (The Gathering Agenda 3, 01107).
//!
//! ```text
//! Forced - At the end of the enemy phase: Each unengaged [[Ghoul]]
//!   enemy moves 1 location towards the Parlor.
//! Forced - At the end of the round: Place 1 doom on this agenda for
//!   each [[Ghoul]] enemy in the Hallway or Parlor.
//! ```
//!
//! Both are board-dependent, single-use scenario logic, so they live
//! card-locally as `Effect::Native` handlers rather than shared
//! `Effect` variants. The enemy-phase-end move keys off
//! `EventPattern::PhaseEnded { Enemy }`; the round-end doom off
//! `EventPattern::RoundEnded`.
//!
//! **Cell: the `at` cell, for both fronts.** Each prints *"At"* — *"At the end of
//! the enemy phase"* and *"At the end of the round"* — and `glossary/At.md`
//! puts such abilities *"in between any "when..." abilities and any
//! "after..." abilities with the same triggering condition."* The ruling on
//! the round-end half says it in this scenario's own terms: *""At the end of
//! the round" effects trigger after "When the round ends" effects (e.g. The
//! Barrier)"* (<https://arkhamdb.com/card/01107>) — act 01109's *when* half
//! resolves first, then this doom placement, then any `after` ability.
//!
//! **The reverse**, `back_text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > - If the investigators are at Act 1 or 2, they are trapped inside the
//! >   house as the ghouls tear them apart. **(→R3)**
//! > - If the investigators are at Act 3, they barely escape with their lives,
//! >   allowing the ghouls to run rampant. Each investigator that has not
//! >   resigned is defeated and suffers 1 physical trauma.
//!
//! **The reverse branches on the act deck**, and only the first branch carries a
//! resolution point. At act 1 or 2 it reaches the printed `(→R3)` by *running an
//! effect* (ADR 0013); at act 3 it prints **no** `(→R#)` at all — it defeats the
//! table instead, and the scenario ends where the campaign guide's untitled
//! *"If no resolution was reached (each investigator resigned or was defeated)"*
//! entry says it does. 01107 is terminal in the ordinary way: last card in the
//! agenda deck, so dooming out flips it and its reverse is what ends the
//! scenario, on either branch.
//!
//! The branch is an `Effect::If` over a card-local
//! [`Condition::Native`](card_dsl::dsl::Condition::Native) reading
//! `act_index`. One consumer, and a plain scenario-state read rather than a
//! *compound or target-referencing* predicate, the kind due for promotion to
//! declarative vocab, so it stays card-local. **The card says "Act 3"; the engine holds a
//! zero-based act cursor**, so the predicate is `act_index == 2`.
//!
//! The act-3 arm's fan-out rides `Effect::ForEach` over every investigator;
//! its body is a card-local native effect calling
//! [`defeat_investigator`](NativeCx::defeat_investigator) and announcing the
//! trauma for the one investigator it runs for. The defeat-and-trauma body has
//! one consumer, so it fails the DSL-primitive threshold and stays native (ADR
//! 0013). The walk runs in **turn order**, and the order is observable on a
//! defeat body: the lead-investigator proxy moves to the next Active seat as
//! each one falls, and step 6's no-remaining-players check fires on whoever
//! falls last. Each investigator's Elimination — including one a *"when the game
//! ends"* weakness such as Cover Up 01007 routes onto a
//! `Continuation::Elimination` frame — drains before the walk reaches
//! the next investigator.
//!
//! **The ending is reached by the rules' route, not latched.** Each defeat runs
//! Rules Reference p.10 Elimination, and step 6 — *"If there are no remaining
//! players, the scenario ends. Refer to "no resolution was reached" entry for
//! that scenario in the campaign guide."* — is what produces
//! [`ScenarioEnding::NoResolution`](game_core::scenario::ScenarioEnding::NoResolution).
//! The card names no ending on this branch, because the card prints none.
//!
//! *"That has not resigned"* needs no filter of the card's own: the walk only
//! reaches investigators still `Active`, and a resigned one is not.
//! [`defeat_investigator`](NativeCx::defeat_investigator) no-ops on a
//! non-`Active` investigator as well, and the body reads the status anyway, so
//! that the trauma announcement fires only for someone this card actually
//! defeated.
//!
//! **A defeat by a card ability is neither killed nor driven insane.**
//! `glossary/Defeat.md`: *"An investigator might also be defeated by a card
//! ability."* The same entry makes killed and insane consequences of **trauma**
//! — *"Taking trauma may cause an investigator to be killed or driven insane"* —
//! and these investigators take one physical trauma, the first step on that
//! track rather than its end. So the defeat carries
//! [`game_core::state::EliminationCause::CardAbility`] and leaves
//! [`game_core::state::Status::Defeated`], both distinct from the damage
//! and horror values.
//!
//! **The physical trauma is announced, not recorded.** `Event::TraumaSuffered`
//! is emitted per defeated investigator, following Cover Up 01007's
//! mental-trauma precedent.
//!
//! **Cell: the `after` cell of the `AgendaAdvanced` condition**, for the
//! reverse. The reverse prints no trigger word, because it is not a triggered
//! ability: it is step 2 of the advance procedure — *"Flip the advancing card
//! over and follow the instructions on the reverse ("b") side."*
//! (`glossary/Act_Deck_and_Agenda_Deck.md`). Declaring it in the `after` cell of
//! the flip is what puts it where the procedure puts it: after step 1's token
//! removal and the flip itself, and before step 3's *"the next card in the deck
//! becomes the current act/agenda"* — which for a terminal agenda never comes,
//! since the `AdvanceReverse` frame holds the cursor and there is no next card.
//! The same cell 01105 and 01106 declare. With no printed word to read against,
//! nothing contests it; the card's rulings
//! (<https://arkhamdb.com/card/01107>) reach only the two Forced fronts.
//!
//! Map note: on The Gathering's star map (Hallway hub ↔ Attic/Cellar/
//! Parlor), every location has a unique shortest first step toward the
//! Parlor, so the lowest-`LocationId` tie-break below is unreachable in this
//! scenario. The move goes through
//! [`relocate_enemy`](NativeCx::relocate_enemy), so a Ghoul arriving at an
//! investigator's location engages on arrival per the general engagement rule
//! — the card text is positional only, but the framework rule applies
//! regardless.
//!
//! # Module gap
//!
//! TODO(#1026): the walk is sequential, and this body's intermediate state is
//! visible. The rules resolve *"each investigator … is defeated"*
//! simultaneously (`glossary/Priority_of_Simultaneous_Resolution.md`: *"Once all
//! necessary choices have been made, the effect resolves simultaneously upon
//! all affected entities."*), while the walk defeats one investigator at a time,
//! so the lead-investigator proxy and the step-6 check above see a table that
//! is part defeated.
//!
//! TODO(#766): the physical trauma is announced but not recorded in the
//! campaign log.
//!
//! A tie between first steps toward the Parlor is broken by the lowest
//! `LocationId`, where the rules have the controlling player choose (RR p.12).
//! A player's choice on a tie is not modelled; The Gathering's map has no tie.

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming, InvestigatorTargetSet, Phase};
use game_core::engine::evaluator::{self, EvalContext};
use game_core::engine::{self, EngineOutcome, NativeCx};
use game_core::event::TraumaKind;
use game_core::state::{Enemy, EnemyId, GameState, LocationId, Status};

use crate::generated::traits::GHOUL;
use crate::impls::{parlor, CardRecord};

/// `ArkhamDB` code for Agenda 3, "They're Getting Out!".
pub const CODE: &str = "01107";

/// This card's registration, listed in [`ALL`](super::ALL). Its act-three
/// branch is one of the two [`CardRecord::conditions`] registrants.
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[
        (MOVE_GHOULS, move_ghouls_toward_parlor),
        (ROUND_END_DOOM, place_round_end_doom),
        (DEFEATED_WITH_TRAUMA, defeated_with_trauma),
    ])
    .conditions(&[(AT_ACT_THREE, at_act_three)]);

const MOVE_GHOULS: &str = "01107:move-ghouls";
const ROUND_END_DOOM: &str = "01107:round-end-doom";
const AT_ACT_THREE: &str = "01107:at-act-three";
const DEFEATED_WITH_TRAUMA: &str = "01107:defeated-with-trauma";

/// Zero-based cursor for the card's *"Act 3"*. `act_index` counts from 0, so
/// act 3 is index 2.
const ACT_THREE_INDEX: usize = 2;

/// The Hallway's printed code. It and the Parlor are the doom-counting
/// locations; the Parlor is also the movement target.
const HALLWAY: &str = "01112";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::forced_on_event(
            EventPattern::PhaseEnded {
                phase: Phase::Enemy,
            },
            EventTiming::At,
            dsl::native(MOVE_GHOULS),
        ),
        dsl::forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::At,
            dsl::native(ROUND_END_DOOM),
        ),
        dsl::forced_on_event(
            EventPattern::AgendaAdvanced,
            EventTiming::After,
            dsl::if_else(
                dsl::native_condition(AT_ACT_THREE),
                dsl::for_each(
                    InvestigatorTargetSet::All,
                    dsl::native(DEFEATED_WITH_TRAUMA),
                ),
                dsl::reach_resolution(3),
            ),
        ),
    ]
}

/// *"If the investigators are at Act 3"* — the reverse's branch predicate.
///
/// The card counts acts from 1 and the engine holds a zero-based cursor, so
/// this is [`ACT_THREE_INDEX`]. Everything else — The Gathering's acts 1 and 2 —
/// takes the printed `(→R3)` arm, which is the other half of the card's own
/// dichotomy.
fn at_act_three(state: &GameState, _ctx: &EvalContext) -> bool {
    state.act_index == ACT_THREE_INDEX
}

/// *"If the investigators are at Act 3, they barely escape with their lives,
/// allowing the ghouls to run rampant. Each investigator that has not resigned
/// is defeated and suffers 1 physical trauma."* — the body the reverse's
/// `ForEach` runs for each investigator, in turn order.
///
/// **No resolution point.** This branch prints no `(→R#)`, and this function
/// latches nothing: defeating the last active investigator drains Rules
/// Reference p.10 Elimination step 6 — *"If there are no remaining players, the
/// scenario ends"* — which is what produces `ScenarioEnding::NoResolution`.
///
/// **The `Active` gate is *"that has not resigned"*.** The walk already skips a
/// non-`Active` investigator, and [`defeat_investigator`] no-ops on one too;
/// reading the status here is what keeps the trauma announcement from firing
/// for someone who was never defeated by this card, whoever runs the body.
fn defeated_with_trauma(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(investigator) = ctx.each_investigator() else {
        return EngineOutcome::Rejected {
            reason: "01107 defeated-with-trauma: run outside its ForEach over investigators".into(),
        };
    };
    if cx
        .state()
        .investigators
        .get(&investigator)
        .map(|i| i.status)
        != Some(Status::Active)
    {
        return EngineOutcome::Done;
    }
    cx.defeat_investigator(investigator);
    // Announced, not recorded: nothing persists trauma (TODO(#766): record it
    // in the campaign log). Cover Up 01007's mental trauma is the precedent.
    cx.suffer_trauma(investigator, TraumaKind::Physical, 1);
    EngineOutcome::Done
}

/// Whether `enemy`'s printed card has the `Ghoul` trait.
fn is_ghoul(enemy: &Enemy) -> bool {
    crate::has_trait(enemy.code.as_str(), GHOUL)
}

/// Each unengaged Ghoul moves one location toward the Parlor (01115).
///
/// **No Parlor in play is a no-op, not a rejection.** The Parlor enters play
/// only via act 2 (01109)'s reverse, while this agenda becomes current at
/// agenda 2's doom threshold — which does not read the act deck — so a group
/// still at act 1 or 2 meets this Forced with no destination on the board,
/// every enemy phase. `glossary/Ability.md`, Forced Abilities: *"If a forced
/// ability does not have the potential to change the game state, the ability
/// does not initiate."* Rejecting here would report malformed data for a board
/// state the scenario reaches legitimately — and since a rejection rolls the
/// whole apply back, it would land on the *player's* action and make the
/// scenario unplayable. Same reading as the barricaded first step
/// below: no move available means no move, never an error.
fn move_ghouls_toward_parlor(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    let Some(parlor_id) = evaluator::location_id_by_code(cx.state(), parlor::CODE) else {
        return EngineOutcome::Done;
    };
    // Scan first (shared borrows), then mutate. Deterministic lowest-
    // LocationId tie-break among shortest first steps.
    let mut movers: Vec<(EnemyId, LocationId)> = Vec::new();
    for (id, e) in &cx.state().enemies {
        if e.engaged_with.is_some() || !is_ghoul(e) {
            continue;
        }
        let Some(from) = e.current_location else {
            continue;
        };
        // A non-Elite Ghoul cannot move into a barricaded location (Barricade
        // 01038), and the block bites at the *compelled step*, not on the
        // graph the path is measured over — as Hunter movement does.
        // 01107 names a fixed destination rather than a "nearest"
        // target, so `glossary/Nearest.md` does not reach it, but
        // `glossary/Patrol.md` carries the same rule on a fixed-destination,
        // shortest-path mover, which is structurally what 01107 is: *"If an
        // enemy with patrol would be compelled to move to a location which is
        // blocked by a card ability, the enemy does not move."* So a
        // barricaded first step is a non-move, never a detour.
        let mut steps: Vec<LocationId> = engine::shortest_first_steps(cx.state(), from, parlor_id)
            .into_iter()
            .filter(|&loc| engine::enemy_can_enter_location(cx.state(), cx.cards(), e, loc))
            .collect();
        steps.sort_unstable();
        if let Some(&to) = steps.first() {
            movers.push((*id, to));
        }
    }
    for (id, to) in movers {
        // The shared relocation funnel: the board write, the
        // `EnemyMoved` emit, and the engage-on-arrival check the general
        // engagement rule requires of *any* enemy movement — an exhausted
        // (evaded) Ghoul moved by this agenda still arrives unengaged.
        cx.relocate_enemy(id, to);
    }
    EngineOutcome::Done
}

/// Place 1 doom on the agenda per Ghoul in the Hallway (01112) or Parlor
/// (01115). Not filtered by engagement (per card text). No threshold
/// check — RR p.24 checks doom in Mythos step 1.3. Placed through the
/// engine's `place_doom_on_agenda`, so an empty agenda deck takes none.
fn place_round_end_doom(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    let counted: Vec<LocationId> = [HALLWAY, parlor::CODE]
        .iter()
        .filter_map(|c| evaluator::location_id_by_code(cx.state(), c))
        .collect();
    let count = cx
        .state()
        .enemies
        .values()
        .filter(|e| is_ghoul(e))
        .filter(|e| e.current_location.is_some_and(|l| counted.contains(&l)))
        .count();
    let count = u8::try_from(count).unwrap_or(u8::MAX);
    cx.place_doom_on_agenda(count);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests;
