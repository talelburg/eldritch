//! [`NativeOp`]: a [`NativeCx`](crate::engine::NativeCx) exit that returns
//! nothing a native reads, held as data and run against the engine.
//!
//! An op's arguments are owned and serializable, so an op can wait on a
//! continuation frame and survive an `apply` boundary. Running it calls the
//! same engine function the exit names, which stays the one implementation
//! of its rule. An exit that returns a value (`spend_uses`,
//! `take_encounter_top`, …) has no op: the native reads what it returns.

use card_dsl::dsl::Restriction;
use serde::{Deserialize, Serialize};

use crate::engine::dispatch::actions::{r#move, take};
use crate::engine::dispatch::{
    act_agenda, cards, combat, elimination, encounter, engagement, hunters, placement, reveal,
    set_aside, skill_test,
};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{board, Cx};
use crate::event::TraumaKind;
use crate::state::{
    CardCode, CardInstanceId, DamageSource, EnemyId, InvestigatorId, LocationId, Owner,
    SkillSubstitution, Zone,
};

/// One call of a `NativeCx` exit that returns `()` or an [`EngineOutcome`],
/// one variant per exit, holding that call's arguments.
///
/// Each variant is documented on the exit of the same name, which builds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeOp {
    /// [`NativeCx::deal_harm`](crate::engine::NativeCx::deal_harm).
    DealHarm {
        investigator: InvestigatorId,
        damage: u8,
        horror: u8,
    },
    /// [`NativeCx::defeat_investigator`](crate::engine::NativeCx::defeat_investigator).
    DefeatInvestigator { investigator: InvestigatorId },
    /// [`NativeCx::suffer_trauma`](crate::engine::NativeCx::suffer_trauma).
    SufferTrauma {
        investigator: InvestigatorId,
        kind: TraumaKind,
        amount: u8,
    },
    /// [`NativeCx::deal_damage_to_enemy`](crate::engine::NativeCx::deal_damage_to_enemy).
    DealDamageToEnemy {
        enemy: EnemyId,
        amount: u8,
        by: Option<InvestigatorId>,
    },
    /// [`NativeCx::put_set_aside_card_into_play`](crate::engine::NativeCx::put_set_aside_card_into_play).
    PutSetAsideCardIntoPlay {
        code: CardCode,
        at: Option<CardCode>,
        owner: Owner,
    },
    /// [`NativeCx::reveal_location`](crate::engine::NativeCx::reveal_location).
    RevealLocation { location: LocationId },
    /// [`NativeCx::relocate_enemy`](crate::engine::NativeCx::relocate_enemy).
    RelocateEnemy { enemy: EnemyId, to: LocationId },
    /// [`NativeCx::place_investigator`](crate::engine::NativeCx::place_investigator).
    PlaceInvestigator {
        investigator: InvestigatorId,
        location: LocationId,
    },
    /// [`NativeCx::move_investigator`](crate::engine::NativeCx::move_investigator).
    MoveInvestigator {
        investigator: InvestigatorId,
        destination: LocationId,
    },
    /// [`NativeCx::engage_ready_enemies`](crate::engine::NativeCx::engage_ready_enemies).
    EngageReadyEnemies,
    /// [`NativeCx::place_doom_on_agenda`](crate::engine::NativeCx::place_doom_on_agenda).
    PlaceDoomOnAgenda { count: u8 },
    /// [`NativeCx::place_doom_on_card`](crate::engine::NativeCx::place_doom_on_card).
    PlaceDoomOnCard { instance: CardInstanceId, count: u8 },
    /// [`NativeCx::remove_clues`](crate::engine::NativeCx::remove_clues).
    RemoveClues { instance: CardInstanceId, count: u8 },
    /// [`NativeCx::discard_all_resources`](crate::engine::NativeCx::discard_all_resources).
    DiscardAllResources { investigator: InvestigatorId },
    /// [`NativeCx::discard_encounter_card`](crate::engine::NativeCx::discard_encounter_card).
    DiscardEncounterCard { code: CardCode, from: Zone },
    /// [`NativeCx::discard_from_hand`](crate::engine::NativeCx::discard_from_hand).
    DiscardFromHand {
        investigator: InvestigatorId,
        hand_index: usize,
    },
    /// [`NativeCx::remove_discard_pile_from_game`](crate::engine::NativeCx::remove_discard_pile_from_game).
    RemoveDiscardPileFromGame { investigator: InvestigatorId },
    /// [`NativeCx::reshuffle_encounter_discard`](crate::engine::NativeCx::reshuffle_encounter_discard).
    ReshuffleEncounterDiscard,
    /// [`NativeCx::resolve_encounter_card`](crate::engine::NativeCx::resolve_encounter_card).
    /// Holds the code; the card's metadata is looked up when the op runs.
    ResolveEncounterCard {
        investigator: InvestigatorId,
        code: CardCode,
    },
    /// [`NativeCx::lose_actions`](crate::engine::NativeCx::lose_actions).
    LoseActions {
        investigator: InvestigatorId,
        count: u8,
    },
    /// [`NativeCx::round_end_advance`](crate::engine::NativeCx::round_end_advance).
    RoundEndAdvance { contributor_location_code: CardCode },
    /// [`NativeCx::substitute_skill_until_end_of_round`](crate::engine::NativeCx::substitute_skill_until_end_of_round).
    SubstituteSkillUntilEndOfRound { substitution: SkillSubstitution },
    /// [`NativeCx::restrict_until_end_of_round`](crate::engine::NativeCx::restrict_until_end_of_round).
    RestrictUntilEndOfRound {
        investigator: InvestigatorId,
        restriction: Restriction,
    },
    /// [`NativeCx::return_committed_card_to_hand`](crate::engine::NativeCx::return_committed_card_to_hand).
    ReturnCommittedCardToHand { index: u8 },
    /// [`NativeCx::discard_after_this_test`](crate::engine::NativeCx::discard_after_this_test).
    DiscardAfterThisTest { card: CardInstanceId },
}

impl NativeOp {
    /// Run the op against `cx` through its exit's engine function. An op
    /// whose exit returns `()` comes back [`EngineOutcome::Done`].
    #[expect(
        clippy::too_many_lines,
        reason = "one exhaustive match arm per `NativeOp` variant"
    )]
    pub(crate) fn run(self, cx: &mut Cx) -> EngineOutcome {
        match self {
            Self::DealHarm {
                investigator,
                damage,
                horror,
            } => {
                return combat::deal_harm(
                    cx,
                    investigator,
                    damage,
                    horror,
                    DamageSource::Effect,
                    false,
                );
            }
            Self::DefeatInvestigator { investigator } => {
                elimination::defeat_investigator(cx, investigator);
            }
            Self::SufferTrauma {
                investigator,
                kind,
                amount,
            } => elimination::suffer_trauma(cx, investigator, kind, amount),
            Self::DealDamageToEnemy { enemy, amount, by } => {
                combat::deal_damage_to_enemy(cx, enemy, amount, by);
            }
            Self::PutSetAsideCardIntoPlay { code, at, owner } => {
                return set_aside::put_set_aside_card_into_play(
                    cx,
                    code.as_str(),
                    at.as_ref().map(CardCode::as_str),
                    owner,
                );
            }
            Self::RevealLocation { location } => reveal::reveal_location(cx, location),
            Self::RelocateEnemy { enemy, to } => hunters::relocate_enemy(cx, enemy, to),
            Self::PlaceInvestigator {
                investigator,
                location,
            } => placement::place_investigator(cx, investigator, location),
            Self::MoveInvestigator {
                investigator,
                destination,
            } => return r#move::move_by_effect(cx, investigator, destination),
            Self::EngageReadyEnemies => engagement::engage_ready_enemies(cx),
            Self::PlaceDoomOnAgenda { count } => act_agenda::place_doom_on_agenda(cx, count),
            Self::PlaceDoomOnCard { instance, count } => board::place_doom(cx, instance, count),
            Self::RemoveClues { instance, count } => board::remove_clues(cx, instance, count),
            Self::DiscardAllResources { investigator } => {
                cards::discard_all_resources(cx, investigator);
            }
            Self::DiscardEncounterCard { code, from } => {
                board::discard_drawn_card(cx, code, Owner::EncounterDeck, from);
            }
            Self::DiscardFromHand {
                investigator,
                hand_index,
            } => {
                // `cards::discard_from_hand` panics on an index past the
                // hand's end; the exit promises a no-op there.
                let in_hand = cx
                    .state
                    .investigators
                    .get(&investigator)
                    .is_some_and(|inv| hand_index < inv.hand.len());
                if in_hand {
                    cards::discard_from_hand(cx, investigator, hand_index);
                }
            }
            Self::RemoveDiscardPileFromGame { investigator } => {
                cards::remove_discard_pile_from_game(cx, investigator);
            }
            Self::ReshuffleEncounterDiscard => encounter::reshuffle_encounter_discard(cx),
            Self::ResolveEncounterCard { investigator, code } => {
                let Some(metadata) = (cx.cards.metadata_for)(&code) else {
                    return EngineOutcome::Rejected {
                        reason: format!("resolve_encounter_card: unknown card {code:?}").into(),
                    };
                };
                return encounter::resolve_encounter_card(
                    cx,
                    investigator,
                    code,
                    metadata,
                    Owner::EncounterDeck,
                );
            }
            Self::LoseActions {
                investigator,
                count,
            } => take::lose_actions(cx, investigator, count),
            Self::RoundEndAdvance {
                contributor_location_code,
            } => {
                return act_agenda::round_end_advance(cx, contributor_location_code.as_str());
            }
            Self::SubstituteSkillUntilEndOfRound { substitution } => {
                skill_test::substitute_skill_until_end_of_round(cx, substitution);
            }
            Self::RestrictUntilEndOfRound {
                investigator,
                restriction,
            } => combat::restrict_until_end_of_round(cx, investigator, restriction),
            Self::ReturnCommittedCardToHand { index } => {
                skill_test::return_committed_card_to_hand(cx, index);
            }
            Self::DiscardAfterThisTest { card } => skill_test::discard_after_this_test(cx, card),
        }
        EngineOutcome::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_encounter_card_op_round_trips_through_json_holding_its_code() {
        let op = NativeOp::ResolveEncounterCard {
            investigator: InvestigatorId(1),
            code: CardCode::new("_synth_treachery"),
        };
        let json = serde_json::to_string(&op).expect("serialize");
        let back: NativeOp = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, op);
    }
}
