//! #918 integration: Crypt Chill 01167 discarding a Parleyed Lita Chantler
//! 01117 — against the real `cards::REGISTRY`.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-10-09)
//!
//! **Crypt Chill (01167)**, `text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > **Revelation** - Test \[willpower\] (4). If you fail, choose and discard 1
//! > asset you control (if you cannot, take 2 damage instead).
//!
//! **Lita Chantler (01117)**: an `Ally` asset the Parlor 01115's Parley lets an
//! investigator take control of.
//!
//! ## Rulings
//!
//! Lita (`data/arkhamdb-faq/core/01117.md`,
//! <https://arkhamdb.com/card/01117>):
//!
//! > If Lita leaves play while a player controls her temporarily during "The
//! > Gathering" scenario (i.e. while she is technically not a part of that
//! > player's deck), remove her from the game (do not place her into any
//! > discard pile). This does not affect possible scenario resolutions.
//!
//! Crypt Chill (`data/arkhamdb-faq/core/01167.md`,
//! <https://arkhamdb.com/card/01167>): *"You must discard as asset from
//! play."* A controlled Lita is an asset in play, so she is a legal choice.
//!
//! Crypt Chill used to route the discarded asset to its **controller's**
//! discard pile, so a Parleyed Lita landed in the investigator's discard. It
//! now leaves play through the engine's discard-from-play exit, which files a
//! card by its **owner** — the scenario, for Lita.
//!
//! Own process → installs `cards::REGISTRY`.

use cards::REGISTRY;
use game_core::assert_event;
use game_core::engine::{OptionTarget, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    GameState, GameStateBuilder, InvestigatorId, LocationId, Owner, SkillKind, Zone,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

/// Crypt Chill.
const CRYPT_CHILL: &str = "01167";
/// Lita Chantler.
const LITA: &str = "01117";
/// The Parlor — where Lita is put into play, and what grants the Parley.
const PARLOR: &str = "01115";

const INV: InvestigatorId = InvestigatorId(1);
const PARLOR_ID: LocationId = LocationId(1);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// The investigator's turn in the revealed Parlor with Lita put into play
/// there, scenario-owned — act 01109b's *"Put the set-aside Lita Chantler into
/// play in the Parlor"*. A `0` token makes the Parley's intellect 5 vs 4
/// succeed. Returns Lita's instance id with the state.
fn parlor_with_lita() -> (GameState, CardInstanceId) {
    let mut parlor = test_support::test_location(PARLOR_ID.0, "Parlor");
    parlor.code = CardCode::new(PARLOR);
    parlor.revealed = true;
    let mut state = GameStateBuilder::new()
        // A filler deck card so the round's Upkeep step-4.4 draw doesn't deal
        // empty-deck horror.
        .with_seat(
            seat(1)
                .at(PARLOR_ID)
                .skill(SkillKind::Intellect, 5)
                .deck(["01088"]),
        )
        .with_location(parlor)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    // A seat cannot place a card at a location, so Lita is put there by a
    // state edit, with an id minted from the state's counter.
    let lita = state.card_instance_ids.mint();
    state
        .locations
        .get_mut(&PARLOR_ID)
        .expect("the Parlor is on the board")
        .cards_at_location
        .push(CardInPlay::enter_play(
            CardCode::new(LITA),
            lita,
            Owner::Scenario,
        ));
    (state, lita)
}

/// Win the Parley, so the investigator controls Lita.
fn parley(state: GameState, lita: CardInstanceId) -> TestSession {
    let parley = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(lita),
        address: AbilityAddress::Granted {
            granter: CardCode::new(PARLOR),
            ability: 1,
            sub: 0,
        },
    };
    let session = TestSession::new(state).take(&parley).commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

/// Reveal Crypt Chill against the investigator, failing its test: with Crypt
/// Chill on top of the encounter deck and only auto-fail in the bag, end the
/// turn so the next round's Mythos step 1.4 draws it, and commit nothing.
fn reveal_crypt_chill(controlled: &TestSession) -> TestSession {
    let mut state = controlled.state().clone();
    state.chaos_bag.tokens = vec![ChaosToken::AutoFail];
    state.encounter_deck.push_back(CardCode::new(CRYPT_CHILL));
    let drawing = TestSession::new(state).take(&TurnAction::EndTurn);
    assert_eq!(drawing.prompt().target, Some(OptionTarget::EncounterDeck));
    drawing.confirm().commit(&[])
}

/// **Crypt Chill removes a Parleyed Lita from the game** (#918). She is the
/// only asset the investigator controls, so the failed test discards her; she
/// leaves play, and because the scenario owns her she goes to no discard pile.
#[test]
fn crypt_chill_removes_a_parleyed_lita_from_the_game() {
    let (state, lita) = parlor_with_lita();
    let controlled = parley(state, lita);
    assert_eq!(
        controlled.state().in_play(INV, LITA),
        lita,
        "the Parley put Lita under the investigator's control",
    );

    let session = reveal_crypt_chill(&controlled);
    // The revelation resolves and play runs on out of the Mythos phase to the
    // investigator's next turn.
    session.assert_at_turn_menu(INV);
    let state = session.state();
    let inv = &state.investigators[&INV];
    assert!(
        inv.cards_in_play.is_empty(),
        "she left play, got {:?}",
        inv.cards_in_play,
    );
    assert_eq!(inv.damage(), 0, "an asset was discarded, so no damage");
    assert!(
        !inv.discard.contains(&CardCode::new(LITA)),
        "she is not placed into the investigator's discard, got {:?}",
        inv.discard,
    );
    assert!(
        !state.encounter_discard.contains(&CardCode::new(LITA)),
        "nor into the encounter discard, got {:?}",
        state.encounter_discard,
    );
    assert!(
        state.removed_from_game.contains(&CardCode::new(LITA)),
        "she is removed from the game, got {:?}",
        state.removed_from_game,
    );
    assert_event!(
        session.events(),
        Event::CardRemovedFromGame {
            code,
            from: Zone::InPlay,
        } if code.as_str() == LITA
    );
}
