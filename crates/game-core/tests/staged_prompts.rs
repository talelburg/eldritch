//! Every prompt is a function of its frame: a prompt frame staged on top of a
//! state settles into a session showing its prompt.
//!
//! The kinds a step pushes and then returns from — a hunter's tie, a spawn's
//! tie, a slot make-room pick, an additional-action choice, an attack-order
//! pick, and the substitution offer (in `test_session.rs`) — are staged here
//! first, then the kinds that do work before they rest. Each test stages the frame with
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
use game_core::engine::{
    InputKind, OptionTarget, PromptNature, Registries, Responder, TimingEvent,
};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, AcknowledgeForcedFrame, AdditionalActionChoiceFrame,
    AdvanceDeck, AdvanceReverseFrame, AdvanceStep, AdvanceTrigger, AssetEntry, Assignment,
    AttackLoopFrame, AttackLoopStage, CandidateSource, CardCode, CardInPlay, CardInstanceId,
    Continuation, DamageSource, DealDamageFrame, DealDamageStep, EffectFrame, EffectPrompt,
    EncounterDrawFrame, EnemyAttackSource, EnemyId, GameStateBuilder, HandDiscardCostFrame,
    HunterChoice, InvestigatorId, LocationId, MulliganFrame, Owner, ResolutionCandidate, SkillKind,
    SkillTestId, SlotDiscardFrame, SpawnEngagePending, Status, SubstitutionPromptFrame, TimingMode,
    TimingPointWindowFrame,
};
use game_core::test_support::{
    self, act, enemy, location, seat, BoardLookup as _, MockRegistry, Seat, TestSession,
};

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
/// `Forced - When the round ends: Gain 1 resource.` — a forced ability a
/// round-end run resolves.
const FORCED_ROUND_END: &str = "_sp_forced_round_end";

const INV: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const NORTH: LocationId = LocationId(11);
const SOUTH: LocationId = LocationId(12);
const GHOUL: EnemyId = EnemyId(7);
const RAT: EnemyId = EnemyId(8);

/// An asset whose printed name differs from its code, so a prompt that shows
/// the code where it should show the name is caught.
fn asset(code: &str, name: &str, slots: Vec<Slot>, health: Option<u8>) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: name.to_owned(),
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
            .with_card(asset(TORCH, "Torch", vec![Slot::Hand], None))
            .with_card(asset(GRIP, "Grip", vec![Slot::Hand], None))
            .with_card(asset(LAMP, "Lamp", vec![Slot::Hand], None))
            .with_card(asset(SOAKER, "Soaker", vec![Slot::Ally], Some(2)))
            .with_card(asset(ROUND_END, "Round End", vec![], None))
            .with_card(asset(FILLER, "Filler", vec![], None))
            .with_card(asset(FORCED_ROUND_END, "Forced Round End", vec![], None))
            .with_abilities(ROUND_END, || {
                vec![dsl::reaction_on_event(
                    EventPattern::RoundEnded,
                    EventTiming::When,
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .with_abilities(FORCED_ROUND_END, || {
                vec![dsl::forced_on_event(
                    EventPattern::RoundEnded,
                    EventTiming::When,
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            // Implemented with no abilities, so the turn menu offers its play.
            .with_abilities(FILLER, Vec::new)
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
    board().with_seat(me).open_turn(INV)
}

/// [`HERE`], with [`NORTH`] and [`SOUTH`] beside it, and nobody seated.
fn board() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_location(location(HERE).name("Study"))
        .with_location(location(NORTH).name("Attic"))
        .with_location(location(SOUTH).name("Cellar"))
}

/// `me` seated as `INV` and `other` as `OTHER`, with `OTHER` the lead, so
/// player order is `[OTHER, INV]` and the first seat is not the lead. No turn
/// is open.
fn led_by_other(me: Seat, other: Seat) -> GameStateBuilder {
    board().with_two_seats_led_by_the_second(me, other)
}

/// The instance id staged cards take, where a frame must name one.
const STAGED: CardInstanceId = CardInstanceId(40);

/// The anchors of the prompt's options, in offered order.
fn anchors(session: &TestSession) -> Vec<Option<OptionTarget>> {
    session
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect()
}

/// What a player must never read in a prompt or an option label: an id's
/// `Debug` form (`InvestigatorId(1)`, `EnemyId(7)`, `[LocationId(11), …]`), a
/// struct's (`SlotCounts { … }`), a raw card code where the card has a name
/// (every `_sp_*` probe here is named differently from its code), or a
/// developer's hint about the wire (`Submit InputResponse::PickSingle(OptionId)`,
/// a rules-reference page cite).
const UNREADABLE: [&str; 9] = [
    "Id(",
    "{",
    "[",
    "InputResponse",
    "OptionId",
    "PickSingle(",
    "_sp_",
    "RR p.",
    "turn order",
];

/// The prompt reads as a game: its text and every option label are free of
/// [`UNREADABLE`].
fn assert_reads_as_a_game(session: &TestSession) {
    let prompt = session.prompt();
    let texts = std::iter::once(&prompt.prompt).chain(prompt.options.iter().map(|o| &o.label));
    for text in texts {
        for forbidden in UNREADABLE {
            assert!(
                !text.contains(forbidden),
                "prompt must read as a game, found {forbidden:?} in {text:?}: {prompt:?}"
            );
        }
    }
}

// ---- the kinds a step pushes and returns from --------------------------

/// A hunter's tie between two destinations asks where it moves, on the map.
#[test]
fn a_staged_hunter_move_asks_for_its_destination() {
    let session = table(&[])
        .with_enemy(enemy(GHOUL).name("Ghoul"))
        .with_frame(Continuation::HunterMove(HunterChoice::Move {
            enemy: GHOUL,
            candidates: vec![NORTH, SOUTH],
        }))
        .session();
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        "The lead investigator chooses where Ghoul moves."
    );
    assert_eq!(
        anchors(&session),
        [
            Some(OptionTarget::Location(NORTH)),
            Some(OptionTarget::Location(SOUTH))
        ],
    );
    assert_eq!(session.prompt().responder, Responder::Lead(INV));
}

/// A hunter's tie between two investigators asks whom it engages, on their
/// investigator cards, and the lead answers it: `glossary/Lead_Investigator.md`,
/// *"the lead investigator is the final arbiter in choosing among those
/// options"*. The lead sits in the second seat, so the first seat is not it.
#[test]
fn a_staged_hunter_engagement_asks_the_lead_whom_it_engages() {
    let session = led_by_other(seat(1).at(HERE), seat(2).at(HERE))
        .open_turn(INV)
        .with_enemy(enemy(GHOUL).name("Ghoul"))
        .with_frame(Continuation::HunterMove(HunterChoice::Engage {
            enemy: GHOUL,
            candidates: vec![INV, OTHER],
        }))
        .session();
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        "The lead investigator chooses whom Ghoul engages."
    );
    let investigators = session.state().investigators.clone();
    assert_eq!(
        anchors(&session),
        [
            Some(investigators[&INV].card_anchor()),
            Some(investigators[&OTHER].card_anchor())
        ],
    );
    assert_eq!(session.prompt().responder, Responder::Lead(OTHER));
}

/// A spawn's tie asks the lead whom the enemy engages, and the answer engages
/// them.
#[test]
fn a_staged_spawn_engagement_asks_the_lead_and_engages_the_chosen_investigator() {
    let session = led_by_other(seat(1).at(HERE), seat(2).at(HERE))
        .open_turn(INV)
        .with_enemy(enemy(GHOUL).name("Ghoul").at(HERE))
        .with_frame(SpawnEngagePending::new(GHOUL, vec![INV, OTHER]))
        .session();
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        "The lead investigator chooses whom Ghoul engages."
    );
    let other = session.state().investigators[&OTHER].card_anchor();
    assert_eq!(session.offered_at(&other), 1, "{:?}", session.prompt());
    assert_eq!(session.prompt().options.len(), 2);
    assert_eq!(session.prompt().responder, Responder::Lead(OTHER));

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
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        format!(
            "{}: choose an asset to discard to make room for Lamp.",
            session.state().name_of(INV)
        )
    );
    let torch = OptionTarget::CardInstance(session.state().in_play(INV, TORCH));
    let grip = OptionTarget::CardInstance(session.state().in_play(INV, GRIP));
    assert_eq!(anchors(&session), [Some(torch.clone()), Some(grip)]);
    assert_eq!(session.prompt().responder, Responder::Investigator(INV));

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
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        format!(
            "{}: choose which additional action to use for this action.",
            session.state().name_of(INV)
        )
    );
    let labels: Vec<&str> = session
        .prompt()
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(labels, ["Torch", "Additional action gained this turn"]);
    assert_eq!(
        anchors(&session),
        [Some(OptionTarget::CardInstance(torch)), None]
    );
    assert_eq!(session.prompt().responder, Responder::Investigator(INV));
}

/// An attack-order pick asks which engaged enemy attacks next.
#[test]
fn a_staged_attack_order_pick_asks_which_enemy_attacks_next() {
    let session = table(&[])
        .with_enemy(enemy(GHOUL).name("Ghoul").engaged_with(INV))
        .with_enemy(enemy(RAT).name("Rat").engaged_with(INV))
        .with_frame(AttackLoopFrame {
            investigator: INV,
            remaining_attackers: vec![GHOUL, RAT],
            source: EnemyAttackSource::AttackOfOpportunity,
            stage: AttackLoopStage::PickOrder,
        })
        .session();
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        format!(
            "{} is engaged with 2 enemies: choose which attacks next.",
            session.state().name_of(INV)
        )
    );
    assert_eq!(
        anchors(&session),
        [
            Some(OptionTarget::Enemy(GHOUL)),
            Some(OptionTarget::Enemy(RAT))
        ],
    );
    assert_eq!(session.prompt().responder, Responder::Investigator(INV));
}

// ---- the kinds that do work before they rest ---------------------------

/// An advance staged at its acknowledge shows the flip pick on the act without
/// announcing the advance again: the event was emitted when it was announced.
/// The lead acknowledges it, since the flip names no one.
#[test]
fn a_staged_advance_acknowledge_asks_the_lead_without_announcing_again() {
    let mut state = led_by_other(seat(1).at(HERE), seat(2).at(HERE))
        .open_turn(INV)
        .with_frame(AdvanceReverseFrame {
            deck: AdvanceDeck::Act,
            from: 0,
            leaving_code: CardCode::new("_sp_act_1"),
            step: AdvanceStep::AwaitAck,
            trigger: AdvanceTrigger::Forced,
        })
        .with_act_deck([act("_sp_act_1", 1), act("_sp_act_2", 1)])
        .build();
    state.interactive_acknowledge = true;
    let session = TestSession::new(state, registries());
    assert_reads_as_a_game(&session);
    assert_eq!(anchors(&session), [Some(OptionTarget::Act)]);
    assert_eq!(session.prompt().responder, Responder::Lead(OTHER));
    assert_event_count!(session.events(), 0, Event::ActAdvanced { .. });
}

/// An advance acknowledge still runs after the scenario ends (it is part of
/// the ending's resolution), so it can surface with every investigator
/// eliminated. Nobody is Active to lead then, and the stored lead answers.
#[test]
fn an_advance_acknowledge_with_everyone_eliminated_asks_the_stored_lead() {
    let mut state = led_by_other(
        seat(1).at(HERE).status(Status::Defeated),
        seat(2).at(HERE).status(Status::Resigned),
    )
    .with_frame(AdvanceReverseFrame {
        deck: AdvanceDeck::Act,
        from: 0,
        leaving_code: CardCode::new("_sp_act_1"),
        step: AdvanceStep::AwaitAck,
        trigger: AdvanceTrigger::Forced,
    })
    .with_act_deck([act("_sp_act_1", 1), act("_sp_act_2", 1)])
    .build();
    state.interactive_acknowledge = true;
    let session = TestSession::new(state, registries());
    assert_eq!(anchors(&session), [Some(OptionTarget::Act)]);
    assert_eq!(session.prompt().responder, Responder::Lead(OTHER));
}

/// A lone forced ability's acknowledge names its card, and is asked of its
/// controller, not of the lead.
#[test]
fn a_staged_forced_acknowledge_asks_its_controller_to_resolve_on_its_card() {
    let torch = STAGED;
    let session = led_by_other(seat(1).at(HERE).in_play_as(torch, TORCH), seat(2).at(HERE))
        .open_turn(INV)
        .with_frame(AcknowledgeForcedFrame {
            candidate: ResolutionCandidate::new(
                CardCode::new(TORCH),
                INV,
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::InPlay(torch)),
            ),
        })
        .session();
    assert_reads_as_a_game(&session);
    assert_eq!(session.prompt().prompt, "Forced — Torch");
    assert_eq!(anchors(&session), [Some(OptionTarget::CardInstance(torch))]);
    assert_eq!(session.prompt().responder, Responder::Investigator(INV));
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
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        format!(
            "{}: assign 1 damage to which target? (1 damage / 0 horror left)",
            session.state().name_of(INV)
        )
    );
    let soaker = OptionTarget::CardInstance(session.state().in_play(INV, SOAKER));
    let me = session.state().investigators[&INV].card_anchor();
    assert_eq!(session.offered_at(&soaker), 1, "{:?}", session.prompt());
    assert_eq!(session.offered_at(&me), 1, "{:?}", session.prompt());
    assert_eq!(session.prompt().responder, Responder::Investigator(INV));
}

/// A hand-discard cost asks which hand card to discard, on the cards in hand.
#[test]
fn a_staged_hand_discard_cost_asks_which_hand_card() {
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_location(location(HERE).name("Study"))
        .with_seat(seat(1).at(HERE).hand([FILLER, FILLER]))
        .open_turn(INV)
        .with_frame(HandDiscardCostFrame {
            investigator: INV,
            code: CardCode::new(TORCH),
        })
        .session();
    assert_reads_as_a_game(&session);
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
    assert_eq!(session.prompt().responder, Responder::Investigator(INV));
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
    assert_reads_as_a_game(&session);
    let prompt = session.prompt();
    assert_eq!(prompt.prompt, "The stored question");
    assert_eq!(prompt.nature, PromptNature::Decision);
    assert_eq!(prompt.responder, Responder::Investigator(INV));
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
    assert_reads_as_a_game(&session);
    assert!(matches!(session.prompt().kind, InputKind::PickMultiple(_)));
    assert!(session.prompt().prompt.starts_with("Commit cards"));
    assert_eq!(session.prompt().responder, Responder::Investigator(INV));
}

/// A reaction window staged with a candidate offers it, skippable. Several may
/// answer a window, so each option names the investigator it is offered to.
#[test]
fn a_staged_reaction_window_offers_its_candidate_to_its_controller() {
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
    assert_reads_as_a_game(&session);
    assert!(session.prompt().skippable);
    assert_eq!(anchors(&session), [Some(OptionTarget::CardInstance(card))]);
    assert_eq!(session.prompt().responder, Responder::Several);
    assert_eq!(session.prompt().options[0].investigator, Some(INV));
    assert_eq!(
        session.prompt().options[0].label,
        "Resolve reaction: Round End"
    );
}

/// A forced run of two or more is ordered by the lead:
/// `glossary/Priority_of_Simultaneous_Resolution.md`, *"the lead investigator
/// determines the order in which the abilities resolve"*. The lead sits in the
/// second seat, and the options name no investigator of their own.
#[test]
fn a_staged_forced_run_asks_the_lead_to_order_it() {
    let mine = STAGED;
    let theirs = CardInstanceId(41);
    let candidate = |controller, card| {
        ResolutionCandidate::new(
            CardCode::new(FORCED_ROUND_END),
            controller,
            AbilityAddress::Printed(0),
            CandidateSource::Ability(AbilitySource::InPlay(card)),
        )
    };
    let session = led_by_other(
        seat(1).at(HERE).in_play_as(mine, FORCED_ROUND_END),
        seat(2).at(HERE).in_play_as(theirs, FORCED_ROUND_END),
    )
    .open_turn(INV)
    .with_frame(TimingPointWindowFrame {
        event: TimingEvent::RoundEnded,
        bucket: EventTiming::When,
        mode: TimingMode::Forced,
        candidates: vec![candidate(INV, mine), candidate(OTHER, theirs)],
    })
    .session();
    assert_eq!(session.prompt().options.len(), 2, "{:?}", session.prompt());
    assert_eq!(session.prompt().responder, Responder::Lead(OTHER));
    assert!(session
        .prompt()
        .options
        .iter()
        .all(|o| o.investigator.is_none()));
}

// ---- who answers the queue frames and the turn -------------------------

/// The open turn is asked of the investigator whose turn it is.
#[test]
fn the_open_turn_is_asked_of_whoever_takes_it() {
    let session = led_by_other(seat(1).at(HERE), seat(2).at(HERE))
        .open_turn(INV)
        .session();
    assert_eq!(session.prompt().responder, Responder::Investigator(INV));
}

/// A mulligan queue asks its head.
#[test]
fn a_staged_mulligan_asks_the_head_of_its_queue() {
    let session = led_by_other(seat(1).at(HERE), seat(2).at(HERE))
        .with_mulligan_remaining([OTHER, INV])
        .session();
    assert_eq!(session.prompt().responder, Responder::Investigator(OTHER));
}

/// A hand-size discard queue asks its head.
#[test]
fn a_staged_hand_size_discard_asks_the_head_of_its_queue() {
    let session = led_by_other(seat(1).at(HERE), seat(2).at(HERE).hand([FILLER; 9]))
        .with_hand_size_discard_pending([OTHER])
        .session();
    assert!(matches!(session.prompt().kind, InputKind::PickMultiple(_)));
    assert_eq!(session.prompt().responder, Responder::Investigator(OTHER));
}

/// The Mythos encounter-draw queue asks its head.
#[test]
fn a_staged_encounter_draw_asks_the_head_of_its_queue() {
    let session = led_by_other(seat(1).at(HERE), seat(2).at(HERE))
        .with_mythos_draw_remaining([OTHER, INV])
        .session();
    session.assert_at_encounter_draw();
    assert_eq!(session.prompt().responder, Responder::Investigator(OTHER));
}

/// A substitution offer staged above its skill test names the investigator it
/// asks and the two skills, with no wire hint.
#[test]
fn a_staged_substitution_names_the_investigator_and_the_skills() {
    let session = table(&[])
        .with_frame(test_support::test_skill_test(
            SkillTestId(0),
            INV,
            SkillKind::Combat,
            SkillTestKind::Plain,
            2,
        ))
        .with_frame(SubstitutionPromptFrame { investigator: INV })
        .session();
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        format!(
            "{}: use Intellect in place of Combat for this test?",
            session.state().name_of(INV)
        )
    );
}

/// The Mythos encounter draw names its drawer.
#[test]
fn a_staged_encounter_draw_names_its_drawer() {
    let session = table(&[])
        .with_frame(Continuation::EncounterDraw(EncounterDrawFrame {
            remaining: vec![INV],
        }))
        .session();
    assert_reads_as_a_game(&session);
    assert_eq!(
        session.prompt().prompt,
        format!("{} draws an encounter card.", session.state().name_of(INV))
    );
}

/// The mulligan and the hand-size discard read as a game.
#[test]
fn a_staged_mulligan_and_hand_size_discard_read_as_a_game() {
    let mulligan = table(&[])
        .with_frame(Continuation::Mulligan(MulliganFrame {
            remaining: vec![INV],
        }))
        .session();
    assert_reads_as_a_game(&mulligan);

    let hand_size = table_with(seat(1).at(HERE).hand([FILLER; 9]))
        .with_hand_size_discard_pending([INV])
        .session();
    assert_reads_as_a_game(&hand_size);
}

/// The turn menu offers a card in hand by its name.
#[test]
fn the_turn_menu_offers_a_hand_card_by_name() {
    let session = table_with(seat(1).at(HERE).hand([FILLER])).session();
    assert_reads_as_a_game(&session);
    assert!(
        session
            .prompt()
            .options
            .iter()
            .any(|o| o.label == "Play Filler"),
        "{:?}",
        session.prompt()
    );
}
