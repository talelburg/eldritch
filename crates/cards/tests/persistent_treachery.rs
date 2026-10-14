//! Integration: The Gathering's three persistent treacheries (C4c, #235)
//! resolved through the real `cards` registry — they stay in play, enforce
//! their constant restriction, and discard at a forced timing point. Own
//! process so it can install the process-global registry against the real
//! corpus.

use cards::REGISTRY;
use game_core::assert_event;
use game_core::assert_no_event;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::modified_value::{self, ModifiedQuantity, ReadContext};
use game_core::engine::{InputKind, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag,
    ChaosToken, Continuation, DiscardPile, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    InvestigatorTurnFrame, Location, LocationId, ModifierTarget, MythosPhaseFrame, MythosResume,
    Owner, Phase, SkillKind, TokenModifiers, UseKind, Zone,
};
use game_core::test_support::{self, seat, uses, Seat, SeatLookup as _, TestSession};

/// Obscuring Fog 01168.
const OBSCURING_FOG: &str = "01168";
/// Dissonant Voices 01165.
const DISSONANT_VOICES: &str = "01165";
/// Frozen in Fear 01164.
const FROZEN_IN_FEAR: &str = "01164";
/// Holy Rosary 01059 — a playable asset.
const HOLY_ROSARY: &str = "01059";
/// .45 Automatic 01016.
const AUTOMATIC_45: &str = "01016";
/// Working a Hunch 01037 — a Fast event.
const WORKING_A_HUNCH: &str = "01037";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
/// The investigator's location (printed shroud 2).
const HERE: LocationId = LocationId(20);
/// A second location, connected to `HERE`.
const ELSEWHERE: LocationId = LocationId(21);
/// The Obscuring Fog staged on `HERE` by the investigate test. A seat cannot
/// place a location attachment, so its id is named once here; seated cards
/// are minted past it.
const FOG_ATTACHMENT: CardInstanceId = CardInstanceId(1);

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// The investigators `seats` at `HERE` at Mythos step 1.4, each about to draw
/// in seat order, with `treacheries` on top of the encounter deck in draw
/// order.
fn mythos_draw(seats: Vec<Seat>, treacheries: &[&str]) -> TestSession {
    let ids: Vec<InvestigatorId> = (1..=seats.len())
        .map(|n| InvestigatorId(u32::try_from(n).expect("a handful of seats")))
        .collect();
    let mut state = seats
        .into_iter()
        .fold(GameStateBuilder::new(), GameStateBuilder::with_seat)
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_turn_order(ids.clone())
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining(ids)
        .build();
    for code in treacheries {
        state.encounter_deck.push_back(CardCode::new(*code));
    }
    TestSession::new(state)
}

/// Investigator 1 at `HERE`, with a filler deck for Upkeep 4.4's draw.
fn at_here() -> Seat {
    seat(1).at(HERE).deck(["01088"])
}

/// Investigator 1 draws `treachery` at Mythos step 1.4 and reveals it,
/// committing no cards at any skill-test commit window that opens. The
/// Mythos phase then ends into investigator 1's turn.
fn reveal(treachery: &str) -> TestSession {
    let session = mythos_draw(vec![at_here()], &[treachery]).confirm();
    session.assert_at_turn_menu(INV);
    session
}

/// **A card revealed from the encounter deck is the encounter deck's**
/// (`glossary/Discard_Piles.md`: *"Encounter cards are owned by the encounter
/// deck."*), whichever zone its Revelation puts it in: Obscuring Fog 01168
/// attaches to a location, Frozen in Fear 01164 enters the threat area.
#[test]
fn a_revealed_persistent_treachery_is_owned_by_the_encounter_deck() {
    let fog = reveal(OBSCURING_FOG);
    assert_eq!(
        fog.state().locations[&HERE].attachments[0].owner,
        Owner::EncounterDeck,
    );

    let fear = reveal(FROZEN_IN_FEAR);
    assert_eq!(
        fear.state().investigators[&INV].threat_area[0].owner,
        Owner::EncounterDeck,
    );
}

#[test]
fn obscuring_fog_attaches_raises_shroud_and_discards_on_investigate() {
    // Reveal: attaches to the investigator's location, not discarded.
    let revealed = reveal(OBSCURING_FOG);
    let state = revealed.state();
    let loc = &state.locations[&HERE];
    assert_eq!(loc.attachments.len(), 1, "Obscuring Fog attached");
    assert_eq!(loc.attachments[0].code.as_str(), OBSCURING_FOG);
    assert!(
        !state
            .encounter_discard
            .contains(&CardCode::new(OBSCURING_FOG)),
        "a persistent treachery is not auto-discarded after its Revelation",
    );

    // +2 shroud: printed 2 → modified 4.
    assert_eq!(
        modified_value::modified_value(
            state,
            Some(&REGISTRY),
            ModifierTarget::Location(loc.id),
            ModifiedQuantity::Shroud,
            ReadContext::from_state(state),
        )
        .total(),
        4,
        "attached Obscuring Fog grants +2 shroud (printed 2)",
    );

    // Forced — after the attached location is successfully investigated,
    // discard Obscuring Fog. Drive a real (passing) Investigate so the
    // in-flight SkillTest frame is live when SkillTestResolved fires: the
    // trigger scan reads `tested_location` off that frame to match Obscuring
    // Fog's *"attached location"* scope (the lean, location-free timing event
    // derives the location from the stack rather than carrying it).
    let mut loc = test_support::test_location(HERE.0, "Here");
    loc.shroud = 0; // effective 0 + 2 (Obscuring Fog) = 2; intellect 3 clears it
    loc.clues = 1;
    loc.attachments.push(CardInPlay::enter_play(
        CardCode::new(OBSCURING_FOG),
        FOG_ATTACHMENT,
        Owner::EncounterDeck,
    ));
    let session = GameStateBuilder::new()
        .with_location(loc)
        .with_seat(seat(1).at(HERE))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    let result = session.finish();
    assert!(
        result.state.locations[&HERE].attachments.is_empty(),
        "Obscuring Fog discards after its location is successfully investigated",
    );
    // Through the leave-play exit, filed by its owner: the encounter deck.
    assert_eq!(
        result.state.encounter_discard,
        vec![CardCode::new(OBSCURING_FOG)]
    );
    assert!(result.state.investigators[&INV].discard.is_empty());
    assert_event!(
        result.events,
        Event::CardDiscarded {
            code,
            from: Zone::LocationAttachment,
            to: DiscardPile::Encounter,
        } if code.as_str() == OBSCURING_FOG
    );
}

// ---- Dissonant Voices (01165) --------------------------------------

#[test]
fn dissonant_voices_enters_threat_area_and_discards_on_round_end() {
    let revealed = reveal(DISSONANT_VOICES);
    let inv = &revealed.state().investigators[&INV];
    assert_eq!(inv.threat_area.len(), 1, "Dissonant Voices in threat area");
    assert_eq!(inv.threat_area[0].code.as_str(), DISSONANT_VOICES);
    assert!(!revealed
        .state()
        .encounter_discard
        .contains(&CardCode::new(DISSONANT_VOICES)));

    // Forced — at the end of the round, discard Dissonant Voices. The solo
    // investigator ends the turn, and the round runs on through the Enemy and
    // Upkeep phases to its end, and into the next round's Mythos encounter draw.
    let session = revealed.take(&TurnAction::EndTurn);
    // The round ended and the next Mythos draw is up.
    session.assert_at_encounter_draw();
    let state = session.state();
    assert!(
        state.investigators[&INV].threat_area.is_empty(),
        "Dissonant Voices discards at end of round",
    );
    assert!(state
        .encounter_discard
        .contains(&CardCode::new(DISSONANT_VOICES)));
}

#[test]
fn dissonant_voices_forbids_playing_an_asset() {
    // Investigator mid-investigation with a playable asset (Holy Rosary,
    // 01059) in hand and Dissonant Voices in their threat area. The menu does
    // not offer the play, so it is sent off the menu to prove the handler
    // itself refuses it.
    let play = TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    };
    let session = GameStateBuilder::new()
        .with_seat(
            seat(1)
                .at(HERE)
                .hand([HOLY_ROSARY])
                .threat_area([DISSONANT_VOICES]),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session();
    assert!(
        !enumerate::legal_actions(session.state()).contains(&play),
        "the menu does not offer a play Dissonant Voices forbids",
    );
    let result = session.submit_off_menu(&play);
    let reason = result.expect_rejected();
    assert!(
        !reason.is_empty(),
        "Dissonant Voices forbids playing assets: {reason}",
    );
    // Validate-first: the asset stays in hand, nothing entered play.
    let inv = &result.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(HOLY_ROSARY)]);
    assert!(inv.cards_in_play.is_empty());
}

#[test]
fn dissonant_voices_round_end_coexists_with_agenda_01107_doom() {
    // Both Dissonant Voices (threat area) and agenda 01107 carry an
    // `At`-`RoundEnded` forced ability. Two simultaneous forced at one timing
    // point let the lead order them (#213), so the real round-end coordinator
    // path opens the ordered forced-run: the agenda places doom per ghoul in the
    // Hallway/Parlor and Dissonant Voices discards itself, both resolving in the
    // lead's chosen order rather than rejecting. Driven through the end of the
    // Upkeep phase, the production route into the round end.
    let loc = |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
    // Ghoul Minion 01160 (Humanoid. Monster. Ghoul.).
    let mut ghoul = test_support::test_enemy(1, "Ghoul");
    ghoul.code = CardCode::new("01160");
    ghoul.current_location = Some(LocationId(2)); // Hallway
    let mut state = GameStateBuilder::new()
        .ending_upkeep_phase()
        .with_seat(seat(1).threat_area([DISSONANT_VOICES]))
        .with_turn_order([INV])
        .with_location(loc(2, "01112", "Hallway"))
        .with_location(loc(5, "01115", "Parlor"))
        .with_enemy(ghoul)
        .build();
    // The builder stages no agenda.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;

    // Settling ends the phase into the round end: 2+ At-forced → the lead
    // orders them.
    let opened = TestSession::new(state);
    let dissonant_voices = opened.state().card(INV, DISSONANT_VOICES);
    let offered: Vec<_> = opened
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect();
    assert_eq!(
        offered,
        vec![Some(dissonant_voices.clone()), Some(OptionTarget::Agenda)],
        "two simultaneous RoundEnded forced present the lead an ordering choice",
    );
    // Resolve the forced run in the lead's chosen order: the agenda first, then
    // Dissonant Voices — both fire, neither rejects.
    let after_first = opened.pick(OptionTarget::Agenda);
    assert_eq!(
        after_first.prompt().options.len(),
        1,
        "the second forced is still pending after the first resolves",
    );
    let done = after_first.pick(dissonant_voices);
    // Both forced resolved and the round ended cleanly: play advanced into the
    // next Mythos phase, not a rejection.
    assert_eq!(done.state().phase, Phase::Mythos);
    assert!(
        done.state().agenda_doom >= 1,
        "agenda 01107 placed doom for the Ghoul in the Hallway; agenda_doom = {}",
        done.state().agenda_doom,
    );
    assert!(
        done.state().investigators[&INV].threat_area.is_empty(),
        "Dissonant Voices also discarded in the same round-end resolution",
    );
}

// ---- Frozen in Fear (01164) ----------------------------------------

/// `HERE` and `ELSEWHERE`, connected.
fn two_locations(builder: GameStateBuilder) -> GameStateBuilder {
    let mut here = test_support::test_location(HERE.0, "A");
    here.connections = vec![ELSEWHERE];
    let mut elsewhere = test_support::test_location(ELSEWHERE.0, "B");
    elsewhere.connections = vec![HERE];
    builder.with_location(here).with_location(elsewhere)
}

fn move_to(destination: LocationId) -> TurnAction {
    TurnAction::Move {
        investigator: INV,
        destination,
    }
}

#[test]
fn frozen_in_fear_surcharges_first_move_each_round_only() {
    let session = two_locations(GameStateBuilder::new())
        .with_seat(seat(1).at(HERE).threat_area([FROZEN_IN_FEAR]))
        .open_turn(INV)
        .session();
    assert_eq!(session.state().investigators[&INV].actions_remaining, 3);

    // First move this round costs 2 (base 1 + surcharge 1): 3 → 1.
    let session = session.take(&move_to(ELSEWHERE));
    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].actions_remaining,
        1,
        "first move/fight/evade each round costs +1 action",
    );

    // Second move this round costs 1 (surcharge already spent): 1 → 0.
    let session = session.take(&move_to(HERE));
    assert_eq!(
        session.state().investigators[&INV].actions_remaining,
        0,
        "subsequent actions that round cost the normal 1",
    );
}

/// The turn menu prices each basic action the way taking it does. Frozen in
/// Fear 01164 surcharges only *"one of the following actions (move, fight, or
/// evade)"*, so with a single action left the menu drops Move, Fight and Evade
/// (each costs 2) and keeps Investigate, Resource, Draw and Engage (each costs
/// 1). Once a surcharged move has spent the surcharge for the round, a move at
/// 1 action is offered again.
#[test]
fn the_action_menu_offers_only_what_the_frozen_in_fear_surcharge_leaves_affordable() {
    let me = INV;
    // Frozen in Fear, `actions` actions left, an engaged enemy and an
    // unengaged bystander at `HERE`.
    let board = |actions: u8| {
        let mut bystander = test_support::test_enemy(8, "Bystander");
        bystander.current_location = Some(HERE);
        two_locations(GameStateBuilder::new())
            .with_seat(
                seat(1)
                    .at(HERE)
                    .deck(["01019"])
                    .threat_area([FROZEN_IN_FEAR])
                    .actions(actions),
            )
            .with_enemy(bystander)
            .open_turn(me)
            .with_enemy_engaged(test_support::test_enemy(7, "Engaged"), me)
    };
    let fight = TurnAction::Fight {
        investigator: me,
        enemy: EnemyId(7),
    };
    let evade = TurnAction::Evade {
        investigator: me,
        enemy: EnemyId(7),
    };
    let never_surcharged = [
        TurnAction::Investigate { investigator: me },
        TurnAction::Resource { investigator: me },
        TurnAction::Draw { investigator: me },
        TurnAction::Engage {
            investigator: me,
            enemy: EnemyId(8),
        },
    ];

    let menu = enumerate::legal_actions(&board(1).build());
    for surcharged in [&move_to(ELSEWHERE), &fight, &evade] {
        assert!(
            !menu.contains(surcharged),
            "{surcharged:?} costs 2 under Frozen in Fear, so 1 action can't pay for it",
        );
    }
    for plain in &never_surcharged {
        assert!(menu.contains(plain), "{plain:?} is never surcharged");
    }

    assert!(
        enumerate::legal_actions(&board(2).build()).contains(&move_to(ELSEWHERE)),
        "2 actions pay for the surcharged move",
    );

    // The surcharged move spends 2 of 3 actions and the surcharge for the round.
    let session = board(3).session().take(&move_to(ELSEWHERE));
    assert_eq!(session.state().investigators[&me].actions_remaining, 1);
    assert!(
        enumerate::legal_actions(session.state()).contains(&move_to(HERE)),
        "with the surcharge spent this round, a move costs 1",
    );
}

/// A two-investigator Investigation-phase board, investigator 1's turn, with
/// `copies` Frozen in Fear in investigator 1's threat area and the chaos bag
/// rigged to `tokens`.
fn frozen_in_fear_board(copies: usize, tokens: Vec<ChaosToken>) -> TestSession {
    GameStateBuilder::new()
        .with_seat(seat(1).threat_area(vec![FROZEN_IN_FEAR; copies]))
        .with_seat(seat(2))
        .with_turn_order([INV, INV2])
        .with_chaos_bag(ChaosBag::new(tokens))
        .open_turn(INV)
        .session()
}

/// The turn was not left stranded: no `InvestigatorTurn` frame is mid-end
/// (slice 2a-i absorbed the former `pending_end_turn` into `ending`). A stack
/// invariant — no stranded frames — so it reads the stack.
fn assert_no_turn_mid_end(state: &GameState) {
    assert!(
        !state.continuations.iter().any(|c| matches!(
            c,
            Continuation::InvestigatorTurn(InvestigatorTurnFrame { ending: true, .. })
        )),
        "a turn was left mid-end on the stack: {:?}",
        state.continuations,
    );
}

/// End investigator 1's turn, committing nothing to Frozen in Fear's
/// end-of-turn willpower test.
fn end_turn_committing_nothing(session: TestSession) -> TestSession {
    session.take(&TurnAction::EndTurn).commit(&[])
}

#[test]
fn frozen_in_fear_end_of_turn_success_discards_and_turn_resumes() {
    // Willpower 3 + Numeric(0) = 3 vs difficulty 3 → success.
    let s = end_turn_committing_nothing(frozen_in_fear_board(1, vec![ChaosToken::Numeric(0)]));
    // The suspending end-of-turn test did not strand the turn: rotation ran.
    s.assert_at_turn_menu(INV2);
    let state = s.state();
    assert!(
        state.investigators[&INV].threat_area.is_empty(),
        "succeeded willpower(3) test discards Frozen in Fear",
    );
    assert!(state
        .encounter_discard
        .contains(&CardCode::new(FROZEN_IN_FEAR)));
    assert_eq!(
        state.active_investigator,
        Some(INV2),
        "end_turn resumed after the test and rotated to investigator 2",
    );
    assert_no_turn_mid_end(state);
}

#[test]
fn frozen_in_fear_end_of_turn_failure_keeps_card_but_turn_still_resumes() {
    // Willpower 3 + Numeric(-1) = 2 vs difficulty 3 → fail.
    let s = end_turn_committing_nothing(frozen_in_fear_board(1, vec![ChaosToken::Numeric(-1)]));
    // Turn still progresses regardless of the test outcome.
    s.assert_at_turn_menu(INV2);
    let state = s.state();
    assert_eq!(
        state.investigators[&INV].threat_area.len(),
        1,
        "failed test leaves Frozen in Fear in the threat area",
    );
    assert!(!state
        .encounter_discard
        .contains(&CardCode::new(FROZEN_IN_FEAR)));
    assert_eq!(state.active_investigator, Some(INV2));
    assert_no_turn_mid_end(state);
}

#[test]
fn two_frozen_in_fear_end_of_turn_tests_both_resolve_then_turn_resumes() {
    // #213 reentrancy: two Frozen in Fear copies on one investigator fire two
    // simultaneous `EndOfTurn` forced abilities, each a *suspending* willpower
    // test. The lead orders them; firing the first suspends on its commit
    // window, and once it resolves the forced run resumes the second sibling —
    // rather than abandoning it. After both resolve, the end-of-turn tail runs
    // (rotation to the next investigator).

    // Two Numeric(0) draws → willpower 3 vs difficulty 3 → both succeed.
    let session = frozen_in_fear_board(2, vec![ChaosToken::Numeric(0), ChaosToken::Numeric(0)]);
    // Two copies, so neither is found by code: read their ids off the board.
    let copies: Vec<OptionTarget> = session.state().investigators[&INV]
        .threat_area
        .iter()
        .map(|card| OptionTarget::CardInstance(card.instance_id))
        .collect();
    let [first, second] = &copies[..] else {
        panic!("two Frozen in Fear seated: {copies:?}");
    };

    // Order the first forced, commit nothing to its test; order the second,
    // commit nothing to its test.
    let s = session
        .take(&TurnAction::EndTurn)
        .pick(first.clone())
        .commit(&[])
        .pick(second.clone())
        .commit(&[]);

    s.assert_at_turn_menu(INV2);
    let state = s.state();
    assert!(
        state.investigators[&INV].threat_area.is_empty(),
        "both succeeded willpower tests discard both Frozen in Fear copies",
    );
    assert_eq!(
        state
            .encounter_discard
            .iter()
            .filter(|c| **c == CardCode::new(FROZEN_IN_FEAR))
            .count(),
        2,
        "both copies land in the encounter discard",
    );
    // Neither sibling was abandoned, and the end-of-turn tail still ran:
    assert_eq!(
        state.active_investigator,
        Some(INV2),
        "end_turn resumed after both tests and rotated to investigator 2",
    );
    assert_no_turn_mid_end(state);
}

#[test]
fn obscuring_fog_limit_one_per_location_discards_the_second_copy() {
    // Two investigators at `HERE` draw an Obscuring Fog each at Mythos 1.4.
    let first = mythos_draw(vec![at_here(), seat(2).at(HERE)], &[OBSCURING_FOG; 2]);

    // First copy attaches.
    let second = first.confirm();
    assert_eq!(second.state().locations[&HERE].attachments.len(), 1);

    // Second copy revealed at the same location: limit 1 → discarded, not
    // attached.
    let done = second.confirm();
    done.assert_at_turn_menu(INV);
    assert_eq!(
        done.state().locations[&HERE].attachments.len(),
        1,
        "limit 1 per location: the second copy does not attach",
    );
    assert!(
        done.state()
            .encounter_discard
            .contains(&CardCode::new(OBSCURING_FOG)),
        "the over-limit copy is discarded",
    );
    // An encounter card's discard names the encounter pile, not a stand-in
    // investigator.
    assert_event!(
        done.finish().events,
        Event::CardDiscarded {
            code,
            from: Zone::LocationAttachment,
            to: DiscardPile::Encounter,
        } if code.as_str() == OBSCURING_FOG
    );
}

// ---- Frozen in Fear (01164) × a bold action designator (#754) -------

/// Board: investigator 1 at `HERE`, Frozen in Fear 01164 in the threat area, a
/// .45 Automatic 01016 (4 ammo) in play, and a co-located enemy to shoot, with
/// `actions` actions left. Rigged `Numeric(0)` bag so the attack resolves
/// deterministically.
///
/// The weapon's `[action] Spend 1 ammo: **Fight**.` prints a bold **Fight**
/// designator, so per the Official FAQ it *counts as a Fight action* — and
/// Frozen in Fear surcharges *"the first time you perform one of the following
/// actions (move, fight, or evade) each round"*, one budget across the three.
fn frozen_in_fear_with_weapon_board(actions: u8) -> TestSession {
    let mut enemy = test_support::test_enemy(100, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 9;

    GameStateBuilder::new()
        .with_seat(
            seat(1)
                .at(HERE)
                .threat_area([FROZEN_IN_FEAR])
                .in_play([(AUTOMATIC_45, uses(UseKind::Ammo, 4))])
                .actions(actions),
        )
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_enemy_engaged(enemy, INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
}

/// The .45 Automatic's Fight activation.
fn weapon_fight(session: &TestSession) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, AUTOMATIC_45)),
        address: AbilityAddress::Printed(0),
    }
}

/// Fire the .45 Automatic, committing nothing to the attack's skill test.
fn fire_weapon(session: TestSession) -> TestSession {
    let fight = weapon_fight(&session);
    session.take(&fight).commit(&[])
}

/// Official FAQ, `Frequently_Asked_Questions.md`:
///
/// > Abilities with a bold action designator (like Fight, Evade or
/// > Investigate) **count as an action of that type**.
///
/// and Frozen in Fear's own ruling (<https://arkhamdb.com/card/01164>):
///
/// > Also applies to \[action\] card abilities with action designators
/// > (**Move**, **Fight**, **Evade**).
///
/// So shooting the .45 Automatic *is* a Fight action, and the first one each
/// round costs 2 — the same as punching. Before #754 it cost 1, making the
/// weapon a way to duck the treachery entirely.
#[test]
fn frozen_in_fear_surcharges_a_fight_designated_ability() {
    let s = fire_weapon(frozen_in_fear_with_weapon_board(3));
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].actions_remaining,
        1,
        "first Fight each round costs base 1 + surcharge 1, whether basic or designated",
    );
}

/// The surcharge's `first_each_round` budget is one *shared* budget: a
/// designated Fight consumes it, so the basic Fight that follows pays the
/// plain 1. Before #754 the activation path never marked the source spent,
/// so the treachery could surcharge twice in a round.
#[test]
fn a_designated_fight_consumes_the_once_each_round_surcharge() {
    let s = fire_weapon(frozen_in_fear_with_weapon_board(3));
    let fear = s.state().in_play(INV, FROZEN_IN_FEAR);
    assert_eq!(s.state().investigators[&INV].actions_remaining, 1);
    assert!(
        s.state().investigators[&INV]
            .action_surcharge_spent_this_round
            .contains(&fear),
        "the designated Fight marks Frozen in Fear's once-each-round surcharge spent",
    );

    // The follow-up basic Fight pays the plain 1: 1 → 0.
    let s = s
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: EnemyId(100),
        })
        .commit(&[]);
    assert_eq!(
        s.state().investigators[&INV].actions_remaining,
        0,
        "surcharge already spent this round, so the basic Fight costs the normal 1",
    );
}

/// The action menu and the validator agree about the surcharged cost. With a
/// single action left the .45 Automatic's Fight costs 2, so it must not be
/// offered — an enumerator reading the printed cost would list an activation
/// the handler then rejects.
///
/// `legal_actions` gets this for free by delegating to
/// `check_activate_ability`; the test pins that delegation, since the
/// alternative (a parallel affordability check in the enumerator) is exactly
/// how the basic-action and ability paths drifted apart in the first place.
#[test]
fn the_action_menu_hides_a_designated_fight_the_surcharge_makes_unaffordable() {
    let three = frozen_in_fear_with_weapon_board(3);
    assert!(
        enumerate::legal_actions(three.state()).contains(&weapon_fight(&three)),
        "with 3 actions the surcharged Fight (cost 2) is affordable",
    );

    let one = frozen_in_fear_with_weapon_board(1);
    assert!(
        !enumerate::legal_actions(one.state()).contains(&weapon_fight(&one)),
        "1 action left cannot pay the surcharged cost of 2, so the menu omits it",
    );
}

#[test]
fn dissonant_voices_keeps_a_fast_event_out_of_a_player_window() {
    // Same restriction, asked of a *fast window's option list* rather than of
    // the `PlayCard` handler: Working a Hunch 01037 ("Fast. Play only during
    // your turn. / Discover 1 clue at your location.") is fast-eligible on this
    // board, so an engine-opened ST.1 player window enumerates it — but
    // Dissonant Voices forbids playing events, so it must not be offered.
    let mut loc = test_support::test_location(HERE.0, "Study");
    loc.clues = 2;
    let session = GameStateBuilder::new()
        .with_seat(
            seat(1)
                .at(HERE)
                .resources(5)
                .hand([WORKING_A_HUNCH])
                .threat_area([DISSONANT_VOICES]),
        )
        .with_location(loc)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Willpower, 4);

    // With nothing eligible the ST.1 window auto-skips: the test rests at its
    // commit window rather than at a Fast window offering Working a Hunch.
    assert_eq!(
        session.prompt().kind,
        InputKind::PickMultiple,
        "Dissonant Voices forbids events, so the window must not offer Working a \
         Hunch; it stayed open: {:?}",
        session.prompt(),
    );
    let session = session.commit(&[]);
    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::CardPlayed { .. });
}
