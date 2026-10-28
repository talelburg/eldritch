//! Every prompt is a function of its frame: a prompt frame staged on top of a
//! state settles into a session showing its prompt.
//!
//! The six kinds a step used to build only at the moment it pushed them —
//! a hunter's tie, a spawn's tie, a slot make-room pick, an additional-action
//! choice, an attack-order pick, and the substitution offer (in
//! `test_session.rs`) — are staged here first, then the kinds that do work
//! before they rest. Each test stages the frame with
//! [`GameStateBuilder::with_frame`] and reads only the prompt and the board.
//!
//! Synthetic probes only (ADR 0016): the `_sp_*` codes model one primitive
//! each and stand in for no printed card. They live here, the one binary that
//! reads them.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot};
use card_dsl::dsl::{self, EventPattern, EventTiming, InvestigatorTarget, SkillTestKind};
use game_core::assert_event_count;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{InputKind, OptionTarget, PromptNature, Registries, TimingEvent};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, AcknowledgeForcedFrame, Act, AdditionalActionChoiceFrame,
    AdvanceDeck, AdvanceReverseFrame, AdvanceStep, AdvanceTrigger, AssetEntry, Assignment,
    AttackLoopFrame, AttackLoopStage, CandidateSource, CardCode, CardInPlay, CardInstanceId,
    Continuation, DamageSource, DealDamageFrame, DealDamageStep, EffectFrame, EffectPrompt,
    EnemyAttackSource, EnemyId, GameStateBuilder, HandDiscardCostFrame, HunterChoice,
    InvestigatorId, LocationId, Owner, ResolutionCandidate, SkillKind, SkillTestId,
    SlotDiscardFrame, SpawnEngagePending, TimingMode, TimingPointWindowFrame,
};
use game_core::test_support::{self, seat, MockRegistry, Seat, SeatLookup as _, TestSession};

/// A single-hand asset in play, holding the investigator's one hand slot
/// beside [`GRIP`].
const TORCH: &str = "_sp_torch";
/// A second single-hand asset in play.
const GRIP: &str = "_sp_grip";
/// A single-hand asset entering play into full hand slots.
const LAMP: &str = "_sp_lamp";
/// An ally with 2 health, which can soak damage.
const SOAKER: &str = "_sp_soaker";
/// `When the round ends: Gain 1 resource.` — a reaction a round-end window
/// offers.
const ROUND_END: &str = "_sp_round_end";
/// A plain card to hold in hand.
const FILLER: &str = "_sp_filler";

const INV: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const NORTH: LocationId = LocationId(11);
const SOUTH: LocationId = LocationId(12);
const GHOUL: EnemyId = EnemyId(7);
const RAT: EnemyId = EnemyId(8);

fn asset(code: &str, slots: Vec<Slot>, health: Option<u8>) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: code.to_owned(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_owned(),
        weakness: None,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots,
            health,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(asset(TORCH, vec![Slot::Hand], None))
            .with_card(asset(GRIP, vec![Slot::Hand], None))
            .with_card(asset(LAMP, vec![Slot::Hand], None))
            .with_card(asset(SOAKER, vec![Slot::Ally], Some(2)))
            .with_card(asset(ROUND_END, vec![], None))
            .with_card(asset(FILLER, vec![], None))
            .with_abilities(ROUND_END, || {
                vec![dsl::reaction_on_event(
                    EventPattern::RoundEnded,
                    EventTiming::When,
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// `INV`'s open turn at [`HERE`], with [`NORTH`] and [`SOUTH`] beside it and
/// `in_play` on the table.
fn table(in_play: &[&str]) -> GameStateBuilder {
    table_with(seat(1).at(HERE).in_play(in_play.iter().copied()))
}

/// [`table`] with `me` seated as `INV`.
fn table_with(me: Seat) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_location(test_support::test_location(NORTH.0, "Attic"))
        .with_location(test_support::test_location(SOUTH.0, "Cellar"))
        .with_seat(me)
        .open_turn(INV)
}

/// The instance id staged cards take, where a frame must name one.
const STAGED: CardInstanceId = CardInstanceId(40);

/// [`table`] with a second investigator at [`HERE`].
fn two_seat_table() -> GameStateBuilder {
    table(&[])
        .with_seat(seat(2).at(HERE))
        .with_turn_order([INV, OTHER])
}

/// The anchors of the prompt's options, in offered order.
fn anchors(session: &TestSession) -> Vec<Option<OptionTarget>> {
    session
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect()
}

// ---- the six kinds built only when pushed ------------------------------

/// A hunter's tie between two destinations asks where it moves, on the map.
#[test]
fn a_staged_hunter_move_asks_for_its_destination() {
    let session = table(&[])
        .with_enemy(test_support::test_enemy(GHOUL.0, "Ghoul"))
        .with_frame(Continuation::HunterMove(HunterChoice::Move {
            enemy: GHOUL,
            candidates: vec![NORTH, SOUTH],
        }))
        .session();
    assert_eq!(
        anchors(&session),
        [
            Some(OptionTarget::Location(NORTH)),
            Some(OptionTarget::Location(SOUTH))
        ],
    );
}

/// A hunter's tie between two investigators asks whom it engages, on their
/// investigator cards.
#[test]
fn a_staged_hunter_engagement_asks_whom_it_engages() {
    let session = two_seat_table()
        .with_enemy(test_support::test_enemy(GHOUL.0, "Ghoul"))
        .with_frame(Continuation::HunterMove(HunterChoice::Engage {
            enemy: GHOUL,
            candidates: vec![INV, OTHER],
        }))
        .session();
    let investigators = session.state().investigators.clone();
    assert_eq!(
        anchors(&session),
        [
            Some(investigators[&INV].card_anchor()),
            Some(investigators[&OTHER].card_anchor())
        ],
    );
}

/// A spawn's tie asks whom the enemy engages, and the answer engages them.
#[test]
fn a_staged_spawn_engagement_asks_and_engages_the_chosen_investigator() {
    let mut ghoul = test_support::test_enemy(GHOUL.0, "Ghoul");
    ghoul.current_location = Some(HERE);
    let session = two_seat_table()
        .with_enemy(ghoul)
        .with_frame(SpawnEngagePending::new(GHOUL, vec![INV, OTHER]))
        .session();
    let other = session.state().investigators[&OTHER].card_anchor();
    assert_eq!(session.offered_at(&other), 1, "{:?}", session.prompt());
    assert_eq!(session.prompt().options.len(), 2);

    let session = session.pick(other);
    assert_eq!(session.state().enemies[&GHOUL].engaged_with, Some(OTHER));
    session.assert_at_turn_menu(INV);
}

/// A slot make-room pick asks which occupier to discard, re-deriving the
/// candidates from the asset it holds; the answer discards it and the asset
/// enters play.
#[test]
fn a_staged_slot_discard_asks_which_occupier_makes_room() {
    let lamp = CardInPlay::enter_play(
        CardCode::new(LAMP),
        CardInstanceId(500),
        Owner::Investigator(INV),
    );
    let session = table(&[TORCH, GRIP])
        .with_frame(SlotDiscardFrame {
            investigator: INV,
            card: Some(lamp),
            entry: AssetEntry::PlayedFromHand,
        })
        .session();
    let torch = OptionTarget::CardInstance(session.state().in_play(INV, TORCH));
    let grip = OptionTarget::CardInstance(session.state().in_play(INV, GRIP));
    assert_eq!(anchors(&session), [Some(torch.clone()), Some(grip)]);

    let session = session.pick(torch);
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.discard, [CardCode::new(TORCH)]);
    assert!(inv.cards_in_play.iter().any(|c| c.code.as_str() == LAMP));
}

/// An additional-action choice asks which offered additional action pays: one
/// granted by a card in play, on that card, and one an effect granted, in the
/// banner.
#[test]
fn a_staged_additional_action_choice_asks_which_pays() {
    let torch = STAGED;
    let session = table_with(seat(1).at(HERE).in_play_as(torch, TORCH))
        .with_frame(AdditionalActionChoiceFrame {
            investigator: INV,
            source: AbilitySource::InPlay(torch),
            address: AbilityAddress::Printed(0),
            offered: vec![Some(torch), None],
        })
        .session();
    let labels: Vec<&str> = session
        .prompt()
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(labels, [TORCH, "Additional action gained this turn"]);
    assert_eq!(
        anchors(&session),
        [Some(OptionTarget::CardInstance(torch)), None]
    );
}

/// An attack-order pick asks which engaged enemy attacks next.
#[test]
fn a_staged_attack_order_pick_asks_which_enemy_attacks_next() {
    let session = table(&[])
        .with_enemy_engaged(test_support::test_enemy(GHOUL.0, "Ghoul"), INV)
        .with_enemy_engaged(test_support::test_enemy(RAT.0, "Rat"), INV)
        .with_frame(AttackLoopFrame {
            investigator: INV,
            remaining_attackers: vec![GHOUL, RAT],
            source: EnemyAttackSource::AttackOfOpportunity,
            stage: AttackLoopStage::PickOrder,
        })
        .session();
    assert_eq!(
        anchors(&session),
        [
            Some(OptionTarget::Enemy(GHOUL)),
            Some(OptionTarget::Enemy(RAT))
        ],
    );
}

// ---- the kinds that do work before they rest ---------------------------

/// An advance staged at its acknowledge shows the flip pick on the act without
/// announcing the advance again: the event was emitted when it was announced.
#[test]
fn a_staged_advance_acknowledge_asks_without_announcing_again() {
    let mut state = table(&[])
        .with_frame(AdvanceReverseFrame {
            deck: AdvanceDeck::Act,
            from: 0,
            leaving_code: CardCode::new("_sp_act_1"),
            step: AdvanceStep::AwaitAck,
            trigger: AdvanceTrigger::Forced,
        })
        .build();
    state.act_deck = vec![
        Act {
            code: CardCode::new("_sp_act_1"),
            clue_threshold: 1,
        },
        Act {
            code: CardCode::new("_sp_act_2"),
            clue_threshold: 1,
        },
    ];
    state.interactive_acknowledge = true;
    let session = TestSession::new(state, registries());
    assert_eq!(anchors(&session), [Some(OptionTarget::Act)]);
    assert_event_count!(session.events(), 0, Event::ActAdvanced { .. });
}

/// A lone forced ability's acknowledge names its card.
#[test]
fn a_staged_forced_acknowledge_asks_to_resolve_on_its_card() {
    let torch = STAGED;
    let session = table_with(seat(1).at(HERE).in_play_as(torch, TORCH))
        .with_frame(AcknowledgeForcedFrame {
            candidate: ResolutionCandidate::new(
                CardCode::new(TORCH),
                INV,
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::InPlay(torch)),
            ),
        })
        .session();
    assert_eq!(session.prompt().prompt, format!("Forced — {TORCH}"));
    assert_eq!(anchors(&session), [Some(OptionTarget::CardInstance(torch))]);
}

/// A deal of damage staged at its distribution asks where the contested point
/// goes: the soaking ally or the investigator.
#[test]
fn a_staged_damage_distribution_asks_where_the_point_goes() {
    let session = table(&[SOAKER])
        .with_frame(DealDamageFrame {
            investigator: INV,
            source: DamageSource::Effect,
            direct: false,
            assignment: Assignment::default(),
            step: DealDamageStep::Distribute {
                remaining_damage: 1,
                remaining_horror: 0,
            },
        })
        .session();
    let soaker = OptionTarget::CardInstance(session.state().in_play(INV, SOAKER));
    let me = session.state().investigators[&INV].card_anchor();
    assert_eq!(session.offered_at(&soaker), 1, "{:?}", session.prompt());
    assert_eq!(session.offered_at(&me), 1, "{:?}", session.prompt());
}

/// A hand-discard cost asks which hand card to discard, on the cards in hand.
#[test]
fn a_staged_hand_discard_cost_asks_which_hand_card() {
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).hand([FILLER, FILLER]))
        .open_turn(INV)
        .with_frame(HandDiscardCostFrame {
            investigator: INV,
            code: CardCode::new(TORCH),
        })
        .session();
    assert_eq!(
        anchors(&session),
        [
            Some(OptionTarget::HandCard {
                investigator: INV,
                hand_index: 0
            }),
            Some(OptionTarget::HandCard {
                investigator: INV,
                hand_index: 1
            }),
        ],
    );
}

/// An effect node staged resting at its pick shows the prompt it stored, not
/// one recomputed by running it, and the answer resolves the node with the
/// pick.
#[test]
fn a_staged_effect_choice_shows_its_stored_prompt_and_resolves_the_pick() {
    let effect = dsl::choose_one([
        (
            "Gain 1 resource",
            dsl::gain_resources(InvestigatorTarget::You, 1),
        ),
        (
            "Gain 2 resources",
            dsl::gain_resources(InvestigatorTarget::You, 2),
        ),
    ]);
    let session = table(&[])
        .with_frame(Continuation::Effect(EffectFrame::Leaf {
            effect: Box::new(effect),
            ctx: EvalContext::for_controller(INV),
            prompt: Some(EffectPrompt {
                text: "The stored question".to_owned(),
                options: vec![
                    ("Gain 1 resource".to_owned(), None),
                    ("Gain 2 resources".to_owned(), None),
                ],
                target: None,
                nature: PromptNature::Decision,
            }),
        }))
        .session();
    let prompt = session.prompt();
    assert_eq!(prompt.prompt, "The stored question");
    assert_eq!(prompt.nature, PromptNature::Decision);
    let before = session.state().investigators[&INV].resources;

    let session = session.pick_nth(1);
    assert_eq!(session.state().investigators[&INV].resources, before + 2);
    session.assert_at_turn_menu(INV);
}

/// A skill test staged at its commit window asks for the commit.
#[test]
fn a_staged_skill_test_asks_for_its_commit() {
    let session = table(&[])
        .with_frame(test_support::test_skill_test(
            SkillTestId(0),
            INV,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
        ))
        .session();
    assert_eq!(session.prompt().kind, InputKind::PickMultiple);
    assert!(session.prompt().prompt.starts_with("Commit cards"));
}

/// A reaction window staged with a candidate offers it, skippable.
#[test]
fn a_staged_reaction_window_offers_its_candidate() {
    let card = STAGED;
    let session = table_with(seat(1).at(HERE).in_play_as(card, ROUND_END))
        .with_frame(TimingPointWindowFrame {
            event: TimingEvent::RoundEnded,
            bucket: EventTiming::When,
            mode: TimingMode::Reaction,
            candidates: vec![ResolutionCandidate::new(
                CardCode::new(ROUND_END),
                INV,
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::InPlay(card)),
            )],
        })
        .session();
    assert!(session.prompt().skippable);
    assert_eq!(anchors(&session), [Some(OptionTarget::CardInstance(card))]);
}
