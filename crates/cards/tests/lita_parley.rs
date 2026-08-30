//! #772 integration: the Parlor 01115 grants Lita Chantler 01117 a **Parley**,
//! and winning it takes control of her — against the real `cards::REGISTRY`.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-08-30)
//!
//! **Parlor (01115)**, `text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > \[action\] **Resign.** "This is too much for me!" You run out the front
//! > door, fleeing in panic.
//! > While Lita Chantler is not controlled by a player, she gains: "\[action\]:
//! > **Parley.** Test \[intellect\] (4). If you succeed, take control of Lita
//! > Chantler."
//!
//! **Lita Chantler (01117)**: an `Ally` asset, health 3 / sanity 3, cost 0.
//!
//! ## Rulings (`data/arkhamdb-faq/core/01117.md`)
//!
//! > When you 'take control' of a card, it enters your play area (not your
//! > hand).
//!
//! > You take control of Lita only temporarily, until the end of the scenario.
//! > Taking control of her doesn't make her a part of your deck.
//!
//! > If Lita leaves play while a player controls her temporarily during "The
//! > Gathering" scenario (i.e. while she is technically not a part of that
//! > player's deck), remove her from the game (do not place her into any
//! > discard pile). This does not affect possible scenario resolutions.
//!
//! The parenthetical is the derivation the engine encodes: her **owner** is the
//! scenario, and where a card goes when it leaves play is a question about its
//! owner rather than about its controller.
//!
//! ## Rules
//!
//! `glossary/Slots.md`: *"If playing **or gaining control** of an asset would
//! put an investigator above his or her slot limit for that type of asset, the
//! investigator must choose and discard other assets under his or her control
//! simultaneously with the new asset entering the slot."* — so the ally slot
//! is contested by the take-control exactly as it is by a play from hand.
//!
//! `glossary/Attack_of_Opportunity.md` exempts *"an action other than to
//! **fight**, to **evade**, or to activate a **parley** or **resign**
//! ability"*, which is why the Parley provokes nothing.
//!
//! The mechanism's own cases — a granted grant, a grant conditioned on a "you"
//! the recipient has not got, an address that goes stale — are in
//! `granted_ability.rs`, on a synthetic registry.
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::assert_event;
use game_core::card_registry;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    GameState, InvestigatorId, LocationId, Phase, Zone,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
    ScriptedResolver, TestSession,
};
use game_core::{legal_actions, TurnAction};

/// Lita Chantler — the `Ally` the Parlor grants to.
const LITA: &str = "01117";
/// The Parlor — where she is put into play, and what grants the Parley.
const PARLOR: &str = "01115";
/// Beat Cop 01018 — a second `Ally`, for the slot cases. Costs 4.
const BEAT_COP: &str = "01018";
/// Daisy Walker 01002 — intellect 5, so a `[intellect]` (4) test turns on the
/// chaos token rather than on the investigator.
const DAISY: &str = "01002";

const INV: InvestigatorId = InvestigatorId(1);
const PARLOR_ID: LocationId = LocationId(1);
const HALLWAY_ID: LocationId = LocationId(2);
const LITA_INST: CardInstanceId = CardInstanceId(50);
const COP_INST: CardInstanceId = CardInstanceId(51);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(cards::REGISTRY);
}

/// The address the Parley lands at: the Parlor's ability 1 (its grant), first
/// granted ability. **Named by where it is printed** — on the Parlor — not by
/// where it appears, which is on Lita.
fn parley_address() -> AbilityAddress {
    AbilityAddress::Granted {
        granter: CardCode::new(PARLOR),
        ability: 1,
        sub: 0,
    }
}

fn parley() -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(LITA_INST),
        address: parley_address(),
    }
}

/// The board: Daisy in the revealed Parlor, Lita in play *at* the Parlor under
/// nobody's control, and a Hallway to stand in instead. `token` is the only
/// chaos token in the bag, so the test's outcome is fixed.
fn board(token: ChaosToken) -> GameState {
    let mut parlor = test_location(1, "Parlor");
    parlor.code = CardCode::new(PARLOR);
    parlor.revealed = true;
    let mut hallway = test_location(2, "Hallway");
    hallway.code = CardCode::new("01112");

    let mut inv = test_investigator(1);
    inv.investigator_card.code = CardCode::new(DAISY);
    inv.skills.intellect = 5;

    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, PARLOR_ID)
        .with_location(parlor)
        .with_location(hallway)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_chaos_bag(ChaosBag::new([token]))
        .build();
    state
        .locations
        .get_mut(&PARLOR_ID)
        .expect("the Parlor is on the board")
        .cards_at_location
        .push(CardInPlay::enter_play(CardCode::new(LITA), LITA_INST));
    state
}

/// Drive the Parley to resolution: the activation opens a commit window
/// (nothing to commit), then the test resolves.
fn drive_parley(state: GameState) -> game_core::engine::ApplyResult {
    let mut session = TestSession::new(state).take(&parley());
    session = session.resolve_choices(|c: &mut ScriptedResolver| {
        c.commit_cards(&[]);
    });
    session.run()
}

// ---- the offer -------------------------------------------------------

/// An investigator in the Parlor is offered Lita's Parley. It is offered **on
/// Lita** — `glossary/Gains.md`: *"the card functions as if it possesses the
/// gained characteristic"* — so the source the menu names is her instance, not
/// the Parlor's location card.
#[test]
fn an_investigator_in_the_parlor_is_offered_litas_parley() {
    let state = board(ChaosToken::Numeric(0));
    assert!(
        legal_actions(&state).contains(&parley()),
        "the Parley is offered on Lita, got {:?}",
        legal_actions(&state),
    );
}

/// An investigator elsewhere is not: the grant rides in on the co-location
/// bullet, and Lita is at the Parlor.
#[test]
fn an_investigator_elsewhere_is_not_offered_the_parley() {
    let mut state = board(ChaosToken::Numeric(0));
    state
        .investigators
        .get_mut(&INV)
        .expect("investigator present")
        .current_location = Some(HALLWAY_ID);
    assert!(
        !legal_actions(&state).contains(&parley()),
        "the Parley is out of reach from the Hallway, got {:?}",
        legal_actions(&state),
    );
}

/// **Once controlled, the Parley is no longer offered** — the grant's condition
/// (*"While Lita Chantler is not controlled by a player"*) has flipped. Nothing
/// removed the ability; the sweep simply stops producing it.
#[test]
fn the_parley_is_no_longer_offered_once_she_is_controlled() {
    let result = drive_parley(board(ChaosToken::Numeric(0)));
    assert!(
        !legal_actions(&result.state).contains(&parley()),
        "the grant lapses the instant a player controls her, got {:?}",
        legal_actions(&result.state),
    );
}

// ---- the test and its consequence ------------------------------------

/// *"Test \[intellect\] (4). If you succeed, take control of Lita Chantler."*
/// Daisy's 5 against difficulty 4 with a `0` token succeeds, and she moves out
/// of the Parlor's `cards_at_location` and into Daisy's play area — *"it enters
/// your play area (not your hand)"*.
#[test]
fn a_successful_parley_takes_control_of_lita() {
    let result = drive_parley(board(ChaosToken::Numeric(0)));

    assert_event!(
        result.events,
        Event::SkillTestSucceeded {
            skill: game_core::state::SkillKind::Intellect,
            ..
        }
    );
    assert_event!(
        result.events,
        Event::ControlTaken { investigator, code, .. }
            if *investigator == INV && code.as_str() == LITA
    );
    assert!(
        result.state.locations[&PARLOR_ID]
            .cards_at_location
            .is_empty(),
        "she left the location's zone",
    );
    let in_play = &result.state.investigators[&INV].cards_in_play;
    assert_eq!(in_play.len(), 1, "she is the one card in Daisy's play area");
    assert_eq!(in_play[0].code.as_str(), LITA);
}

/// A failed Parley costs the action and nothing else: Daisy's 5 minus the
/// `[-2]` token is 3 against difficulty 4, and the take-control is on the
/// **success** side alone.
#[test]
fn a_failed_parley_leaves_her_where_she_is() {
    let result = drive_parley(board(ChaosToken::Numeric(-2)));

    assert_event!(result.events, Event::SkillTestFailed { .. });
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::ControlTaken { .. })),
        "nothing took control, got {:?}",
        result.events,
    );
    assert_eq!(
        result.state.locations[&PARLOR_ID].cards_at_location.len(),
        1,
        "she is still in play at the Parlor",
    );
}

/// **The Parley provokes no attack of opportunity.**
/// `glossary/Attack_of_Opportunity.md` exempts *"an action other than to
/// **fight**, to **evade**, or to activate a **parley** or **resign**
/// ability"*, and the ability prints the designator, so the exemption is the
/// engine's to apply.
#[test]
fn the_parley_provokes_no_attack_of_opportunity() {
    let mut state = board(ChaosToken::Numeric(0));
    let mut attacker = test_enemy(100, "Ghoul");
    attacker.current_location = Some(PARLOR_ID);
    attacker.engaged_with = Some(INV);
    attacker.attack_damage = 2;
    state.enemies.insert(attacker.id, attacker);

    let result = drive_parley(state);
    assert_eq!(
        result.state.investigators[&INV].damage(),
        0,
        "a Parley is attack-of-opportunity exempt, got {:?}",
        result.events,
    );
}

// ---- the instance, and the slot --------------------------------------

/// **Taking control preserves her instance.** The ruling makes control
/// temporary and separate from ownership, so what moves is the card itself, not
/// a fresh copy: her accumulated damage and horror survive the move, as do her
/// instance id and her usage counters.
#[test]
fn taking_control_preserves_her_instance() {
    let mut state = board(ChaosToken::Numeric(0));
    {
        let lita = &mut state
            .locations
            .get_mut(&PARLOR_ID)
            .expect("the Parlor is on the board")
            .cards_at_location[0];
        lita.accumulated_damage = 2;
        lita.accumulated_horror = 1;
        lita.bump_ability_usage(0, 0);
    }

    let result = drive_parley(state);
    let lita = &result.state.investigators[&INV].cards_in_play[0];
    assert_eq!(
        lita.instance_id, LITA_INST,
        "the same instance moved, rather than a fresh one being minted",
    );
    assert_eq!(lita.accumulated_damage, 2, "her damage travelled with her");
    assert_eq!(lita.accumulated_horror, 1, "her horror travelled with her");
    assert!(
        lita.ability_usage.contains_key(&0),
        "her usage counters travelled with her, got {:?}",
        lita.ability_usage,
    );
}

/// **Gaining control contests the ally slot.** With Beat Cop already filling
/// Daisy's one ally slot, the make-room machinery runs on the way in — and with
/// exactly one occupier there is no choice to offer, so it is discarded
/// outright (the RR's *"must choose and discard"* has a single answer). Beat
/// Cop is Daisy's own card, so it lands in her discard pile.
#[test]
fn taking_control_makes_room_in_the_ally_slot() {
    let mut state = board(ChaosToken::Numeric(0));
    state
        .investigators
        .get_mut(&INV)
        .expect("investigator present")
        .cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(BEAT_COP), COP_INST).owned_by(Some(INV)));

    let result = drive_parley(state);
    let inv = &result.state.investigators[&INV];
    assert_eq!(
        inv.cards_in_play.len(),
        1,
        "the ally slot holds one ally, got {:?}",
        inv.cards_in_play,
    );
    assert_eq!(inv.cards_in_play[0].code.as_str(), LITA);
    assert!(
        inv.discard.contains(&CardCode::new(BEAT_COP)),
        "Beat Cop is Daisy's own card, so it goes to her discard; got {:?}",
        inv.discard,
    );
}

/// **Lita leaving play while controlled is removed from the game**, not
/// discarded: *"remove her from the game (do not place her into any discard
/// pile)"*. The derivation is her **owner** — the scenario — and the exit taken
/// here is the one the slot rule opens, playing a second ally into the one ally
/// slot she now fills.
#[test]
fn lita_leaving_play_while_controlled_is_removed_from_the_game() {
    let controlled = drive_parley(board(ChaosToken::Numeric(0))).state;
    let mut state = controlled;
    {
        let inv = state
            .investigators
            .get_mut(&INV)
            .expect("investigator present");
        inv.hand.push(CardCode::new(BEAT_COP));
        inv.resources = 5;
        inv.actions_remaining = 3;
    }

    let hand_index = u8::try_from(
        state.investigators[&INV]
            .hand
            .iter()
            .position(|c| c.as_str() == BEAT_COP)
            .expect("Beat Cop is in hand"),
    )
    .expect("hand index fits u8");
    let result = take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index,
        },
    );

    let inv = &result.state.investigators[&INV];
    assert!(
        !inv.discard.contains(&CardCode::new(LITA)),
        "she is not placed into any discard pile, got {:?}",
        inv.discard,
    );
    assert!(
        result
            .state
            .removed_from_game
            .contains(&CardCode::new(LITA)),
        "she is removed from the game, got {:?}",
        result.state.removed_from_game,
    );
    assert_event!(
        result.events,
        Event::CardRemovedFromGame {
            investigator,
            code,
            from: Zone::InPlay,
        } if *investigator == INV && code.as_str() == LITA
    );
}
