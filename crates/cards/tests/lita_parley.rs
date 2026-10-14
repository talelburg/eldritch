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

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    GameStateBuilder, InvestigatorId, LocationId, Owner, SkillKind, Status, Zone,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

/// Lita Chantler — the `Ally` the Parlor grants to.
const LITA: &str = "01117";
/// The Parlor — where she is put into play, and what grants the Parley.
const PARLOR: &str = "01115";
/// Beat Cop 01018 — a second `Ally`, for the slot cases. Costs 4.
const BEAT_COP: &str = "01018";
/// Grasping Hands 01162 — *"**Revelation** - Test \[agility\] (3). For each
/// point you fail by, take 1 damage."* The soak-defeat exit's driver.
const GRASPING_HANDS: &str = "01162";

const INV: InvestigatorId = InvestigatorId(1);
const PARLOR_ID: LocationId = LocationId(1);
const HALLWAY_ID: LocationId = LocationId(2);
/// Lita's instance id. A seat cannot place a card at a location, so the
/// builder does not mint hers; it is fixed here, and seated cards are minted
/// past it.
const LITA_INST: CardInstanceId = CardInstanceId(50);

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

/// The investigator: intellect 5, so a `[intellect]` (4) test turns on the
/// chaos token rather than on the investigator.
fn sharp() -> Seat {
    seat(1).skill(SkillKind::Intellect, 5)
}

/// Lita as she is put into play at the Parlor: the scenario's, under nobody's
/// control.
fn lita_at_the_parlor() -> CardInPlay {
    CardInPlay::enter_play(CardCode::new(LITA), LITA_INST, Owner::Scenario)
}

/// The board: [`sharp`] in the revealed Parlor, Lita in play *at* the Parlor
/// under nobody's control, and a Hallway to stand in instead. `token` is the
/// only chaos token in the bag, so the test's outcome is fixed.
fn board(token: ChaosToken) -> GameStateBuilder {
    board_with(sharp().at(PARLOR_ID), lita_at_the_parlor(), token)
}

/// [`board`] with the investigator seated as `investigator` and Lita as
/// `lita`.
fn board_with(investigator: Seat, lita: CardInPlay, token: ChaosToken) -> GameStateBuilder {
    let mut parlor = test_support::test_location(PARLOR_ID.0, "Parlor");
    parlor.code = CardCode::new(PARLOR);
    parlor.revealed = true;
    parlor.cards_at_location.push(lita);
    let mut hallway = test_support::test_location(HALLWAY_ID.0, "Hallway");
    hallway.code = CardCode::new("01112");

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_location(parlor)
        .with_location(hallway)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([token]))
}

/// Take the Parley and play it to resolution: the activation opens a commit
/// window (nothing to commit), then the test resolves and the turn goes on.
fn parley_on(board: GameStateBuilder) -> TestSession {
    let session = board.session().take(&parley()).commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

// ---- the offer -------------------------------------------------------

/// An investigator in the Parlor is offered Lita's Parley. It is offered **on
/// Lita** — `glossary/Gains.md`: *"the card functions as if it possesses the
/// gained characteristic"* — so the source the menu names is her instance, not
/// the Parlor's location card.
#[test]
fn an_investigator_in_the_parlor_is_offered_litas_parley() {
    let state = board(ChaosToken::Numeric(0)).build();
    assert!(
        enumerate::legal_actions(&state, TEST_REGISTRIES.cards).contains(&parley()),
        "the Parley is offered on Lita, got {:?}",
        enumerate::legal_actions(&state, TEST_REGISTRIES.cards),
    );
}

/// An investigator elsewhere is not: the grant rides in on the co-location
/// bullet, and Lita is at the Parlor.
#[test]
fn an_investigator_elsewhere_is_not_offered_the_parley() {
    let state = board_with(
        sharp().at(HALLWAY_ID),
        lita_at_the_parlor(),
        ChaosToken::Numeric(0),
    )
    .build();
    assert!(
        !enumerate::legal_actions(&state, TEST_REGISTRIES.cards).contains(&parley()),
        "the Parley is out of reach from the Hallway, got {:?}",
        enumerate::legal_actions(&state, TEST_REGISTRIES.cards),
    );
}

/// **Once controlled, the Parley is no longer offered** — the grant's condition
/// (*"While Lita Chantler is not controlled by a player"*) has flipped. Nothing
/// removed the ability; the sweep simply stops producing it.
#[test]
fn the_parley_is_no_longer_offered_once_she_is_controlled() {
    let result = parley_on(board(ChaosToken::Numeric(0)));
    assert!(
        !enumerate::legal_actions(result.state(), TEST_REGISTRIES.cards).contains(&parley()),
        "the grant lapses the instant a player controls her, got {:?}",
        enumerate::legal_actions(result.state(), TEST_REGISTRIES.cards),
    );
}

// ---- the test and its consequence ------------------------------------

/// *"Test \[intellect\] (4). If you succeed, take control of Lita Chantler."*
/// Intellect 5 against difficulty 4 with a `0` token succeeds, and she moves out
/// of the Parlor's `cards_at_location` and into the investigator's play area — *"it enters
/// your play area (not your hand)"*.
#[test]
fn a_successful_parley_takes_control_of_lita() {
    let result = parley_on(board(ChaosToken::Numeric(0)));

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded {
            skill: SkillKind::Intellect,
            ..
        }
    );
    assert_event!(
        result.events(),
        Event::ControlTaken { investigator, code, .. }
            if *investigator == INV && code.as_str() == LITA
    );
    assert!(
        result.state().locations[&PARLOR_ID]
            .cards_at_location
            .is_empty(),
        "she left the location's zone",
    );
    let in_play = &result.state().investigators[&INV].cards_in_play;
    assert_eq!(
        in_play.len(),
        1,
        "she is the one card in the investigator's play area"
    );
    assert_eq!(in_play[0].code.as_str(), LITA);
}

/// A failed Parley costs the action and nothing else: intellect 5 minus the
/// `[-2]` token is 3 against difficulty 4, and the take-control is on the
/// **success** side alone.
#[test]
fn a_failed_parley_leaves_her_where_she_is() {
    let result = parley_on(board(ChaosToken::Numeric(-2)));

    assert_event!(result.events(), Event::SkillTestFailed { .. });
    assert_no_event!(result.events(), Event::ControlTaken { .. });
    assert_eq!(
        result.state().locations[&PARLOR_ID].cards_at_location.len(),
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
    let mut attacker = test_support::test_enemy(100, "Ghoul");
    attacker.attack_damage = 2;

    let result = parley_on(board(ChaosToken::Numeric(0)).with_enemy_engaged(attacker, INV));
    assert_eq!(
        result.state().investigators[&INV].damage(),
        0,
        "a Parley is attack-of-opportunity exempt, got {:?}",
        result.events(),
    );
}

// ---- the instance, and the slot --------------------------------------

/// **Taking control preserves her instance.** The ruling makes control
/// temporary and separate from ownership, so what moves is the card itself, not
/// a fresh copy: her accumulated damage and horror survive the move, as do her
/// instance id and her usage counters.
#[test]
fn taking_control_preserves_her_instance() {
    let mut lita = lita_at_the_parlor();
    lita.accumulated_damage = 2;
    lita.accumulated_horror = 1;
    lita.bump_ability_usage(0, 0);

    let result = parley_on(board_with(
        sharp().at(PARLOR_ID),
        lita,
        ChaosToken::Numeric(0),
    ));
    let lita = &result.state().investigators[&INV].cards_in_play[0];
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

/// **Taking control leaves her the scenario's.** *"Taking control of her doesn't
/// make her a part of your deck"* (<https://arkhamdb.com/card/01117>): she
/// enters the investigator's play area still owned by the scenario.
#[test]
fn after_a_parley_she_is_still_scenario_owned() {
    let result = parley_on(board(ChaosToken::Numeric(0)));
    let lita = &result.state().investigators[&INV].cards_in_play[0];
    assert_eq!(lita.code.as_str(), LITA);
    assert_eq!(lita.owner, Owner::Scenario);
}

/// **Gaining control contests the ally slot.** With Beat Cop already filling
/// the investigator's one ally slot, the make-room machinery runs on the way in — and with
/// exactly one occupier there is no choice to offer, so it is discarded
/// outright (the RR's *"must choose and discard"* has a single answer). Beat
/// Cop is the investigator's own card, so it lands in their discard pile.
#[test]
fn taking_control_makes_room_in_the_ally_slot() {
    let result = parley_on(board_with(
        sharp().at(PARLOR_ID).in_play([BEAT_COP]),
        lita_at_the_parlor(),
        ChaosToken::Numeric(0),
    ));
    let inv = &result.state().investigators[&INV];
    assert_eq!(
        inv.cards_in_play.len(),
        1,
        "the ally slot holds one ally, got {:?}",
        inv.cards_in_play,
    );
    assert_eq!(inv.cards_in_play[0].code.as_str(), LITA);
    assert!(
        inv.discard.contains(&CardCode::new(BEAT_COP)),
        "Beat Cop is the investigator's own card, so it goes to their discard; got {:?}",
        inv.discard,
    );
}

/// The **other** leaving-play exit: soak defeat. `glossary/Asset_Cards.md`
/// makes an asset an eligible soak target for the investigator who *controls*
/// it, and RR p.7 sends a defeated one to *"its owner's discard pile"* — which
/// for Lita is nowhere, so she is removed from the game by the same derivation
/// the slot exit uses. Driven through Grasping Hands 01162 (*"Test
/// \[agility\] (3). For each point you fail by, take 1 damage."*), soaking its
/// damage onto her.
#[test]
fn lita_defeated_by_soaked_damage_is_removed_from_the_game() {
    // Health 3: one damage already on her, so the two soaked points defeat
    // her. A spare deck card keeps the round's Upkeep draw off an empty deck.
    let mut lita = lita_at_the_parlor();
    lita.accumulated_damage = 1;
    let controlled = parley_on(board_with(
        sharp().at(PARLOR_ID).deck(["01088"]),
        lita,
        ChaosToken::Numeric(0),
    ));
    // The bag that let the Parley succeed now rigs the treachery: Agility 3
    // against Grasping Hands' printed 3, minus 2 — a failure by 2, so it
    // deals 2 damage. The turn's board is re-settled with the new bag and
    // Grasping Hands on top of the encounter deck.
    let mut state = controlled.state().clone();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(-2)];
    state
        .encounter_deck
        .push_back(CardCode::new(GRASPING_HANDS));

    // The turn ends and the round runs on to the next Mythos phase, whose
    // encounter draw reveals Grasping Hands.
    let drawing =
        TestSession::new_with_registries(state, TEST_REGISTRIES).take(&TurnAction::EndTurn);
    drawing.assert_at_encounter_draw();
    // Each point of the failure's damage asks where it goes: the investigator
    // or the one controlled soaker, Lita. Both go to her.
    let lita = OptionTarget::CardInstance(LITA_INST);
    let r = drawing.confirm().commit(&[]).pick(lita.clone()).pick(lita);
    let r = r.state();

    let inv = &r.investigators[&INV];
    assert!(
        inv.cards_in_play.is_empty(),
        "she soaked her third damage and left play, got {:?}",
        inv.cards_in_play,
    );
    assert!(
        !inv.discard.contains(&CardCode::new(LITA)),
        "she is not placed into any discard pile, got {:?}",
        inv.discard,
    );
    assert!(
        r.removed_from_game.contains(&CardCode::new(LITA)),
        "the soak-defeat exit removes her from the game too, got {:?}",
        r.removed_from_game,
    );
}

/// **Elimination removes her, but not into her controller's pile.**
/// `glossary/Elimination.md` step 1: *"The cards he or she **controls** in play
/// and all of the cards in his or her out-of-play areas (such as hand, deck,
/// discard pile) are removed from the game."* — so she leaves play with her
/// controller. Which pile she is removed *to* is the ownership question: the
/// per-investigator `removed_from_game` is that investigator's own deck's
/// cards, which she explicitly is not.
#[test]
fn eliminating_her_controller_removes_her_to_the_scenarios_pile() {
    // The test investigator has 8 health; 8 damage defeats it outright. Lita can
    // soak, so each point asks where it goes: all 8 go on the investigator.
    let mut session = parley_on(board(ChaosToken::Numeric(0))).take_damage(INV, 8);
    let me = session.state().investigators[&INV].card_anchor();
    for _ in 0..8 {
        session = session.pick(me.clone());
    }
    let state = session.finish().state;

    let inv = &state.investigators[&INV];
    assert_eq!(
        inv.status,
        Status::Defeated,
        "the damage eliminated her controller"
    );
    assert!(
        inv.cards_in_play.is_empty(),
        "every card she controlled left play, got {:?}",
        inv.cards_in_play,
    );
    assert!(
        !inv.removed_from_game.contains(&CardCode::new(LITA)),
        "she is not one of the eliminated investigator's own cards, got {:?}",
        inv.removed_from_game,
    );
    assert!(
        state.removed_from_game.contains(&CardCode::new(LITA)),
        "she is removed to the scenario's pile, got {:?}",
        state.removed_from_game,
    );
}

/// **Lita leaving play while controlled is removed from the game**, not
/// discarded: *"remove her from the game (do not place her into any discard
/// pile)"*. The derivation is her **owner** — the scenario — and the exit taken
/// here is the one the slot rule opens, playing a second ally into the one ally
/// slot she now fills.
#[test]
fn lita_leaving_play_while_controlled_is_removed_from_the_game() {
    // Beat Cop waits in hand, with the resources for it, through the Parley;
    // the Parley leaves an action to play it with.
    let controlled = parley_on(board_with(
        sharp().at(PARLOR_ID).hand([BEAT_COP]).resources(5),
        lita_at_the_parlor(),
        ChaosToken::Numeric(0),
    ));

    let hand_index = u8::try_from(
        controlled.state().investigators[&INV]
            .hand
            .iter()
            .position(|c| c.as_str() == BEAT_COP)
            .expect("Beat Cop is in hand"),
    )
    .expect("hand index fits u8");
    let result = controlled
        .take(&TurnAction::PlayCard {
            investigator: INV,
            hand_index,
        })
        .finish();

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
            code,
            from: Zone::InPlay,
        } if code.as_str() == LITA
    );
}
