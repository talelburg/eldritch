//! Integration: Crypt Chill 01167 discarding a Parleyed Lita Chantler
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
//! The discarded asset leaves play through the engine's discard-from-play exit,
//! which files a card by its **owner** — the scenario, for Lita — not by its
//! controller.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::TurnAction;
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
/// Lita, put into play in the Parlor.
const LITA_ID: CardInstanceId = CardInstanceId(900);

/// The investigator's turn in the revealed Parlor with Lita put into play
/// there, scenario-owned — act 01109b's *"Put the set-aside Lita Chantler into
/// play in the Parlor"*. A `0` token makes the Parley's intellect 5 vs 4
/// succeed. Returns Lita's instance id with the state.
fn parlor_with_lita() -> (GameState, CardInstanceId) {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        // A filler deck card so the round's Upkeep step-4.4 draw doesn't deal
        // empty-deck horror.
        .with_seat(
            seat(1)
                .at(PARLOR_ID)
                .skill(SkillKind::Intellect, 5)
                .deck(["01088"]),
        )
        // A seat cannot place a card at a location, so Lita's id is named
        // here.
        .with_location(
            test_support::location(PARLOR_ID)
                .as_card(PARLOR)
                .cards_at_location([CardInPlay::enter_play(
                    CardCode::new(LITA),
                    LITA_ID,
                    Owner::Scenario,
                )]),
        )
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        // Crypt Chill waits on top of the encounter deck for the next Mythos
        // phase; nothing draws from it before then.
        .with_encounter_deck([CRYPT_CHILL])
        .build();
    (state, LITA_ID)
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
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&parley)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

/// Reveal Crypt Chill against the investigator, failing its test: with Crypt
/// Chill on top of the encounter deck ([`parlor_with_lita`] staged it) and
/// only auto-fail in the bag, end the turn so the next round's Mythos step
/// 1.4 draws it, and commit nothing.
fn reveal_crypt_chill(controlled: &TestSession) -> TestSession {
    let mut state = controlled.state().clone();
    state.chaos_bag.tokens = vec![ChaosToken::AutoFail];
    let drawing = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::EndTurn);
    drawing.assert_at_encounter_draw();
    drawing.confirm().commit(&[])
}

/// Regression for #918: **Crypt Chill removes a Parleyed Lita from the game**,
/// rather than discarding her to the investigator's pile. She is the
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
