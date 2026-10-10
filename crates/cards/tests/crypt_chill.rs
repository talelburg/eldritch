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
use game_core::action::{Action, EngineRecord};
use game_core::assert_event;
use game_core::engine::{ApplyResult, EngineOutcome, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    ContinuationStack, GameState, GameStateBuilder, InvestigatorId, LocationId, Owner, Phase, Zone,
};
use game_core::test_support::{self, ScriptedResolver, TestSession};

/// Crypt Chill.
const CRYPT_CHILL: &str = "01167";
/// Lita Chantler.
const LITA: &str = "01117";
/// The Parlor — where Lita is put into play, and what grants the Parley.
const PARLOR: &str = "01115";

const INV: InvestigatorId = InvestigatorId(1);
const PARLOR_ID: LocationId = LocationId(1);
const LITA_INST: CardInstanceId = CardInstanceId(50);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// The investigator in the revealed Parlor with Lita put into play there,
/// scenario-owned — act 01109b's *"Put the set-aside Lita Chantler into play
/// in the Parlor"*. A `0` token makes the Parley's intellect 5 vs 4 succeed.
fn parlor_with_lita() -> GameState {
    let mut parlor = test_support::test_location(1, "Parlor");
    parlor.code = CardCode::new(PARLOR);
    parlor.revealed = true;
    let mut inv = test_support::test_investigator(1);
    inv.skills.intellect = 5;
    let mut state = GameStateBuilder::new()
        .with_investigator_at(inv, PARLOR_ID)
        .with_location(parlor)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    state
        .locations
        .get_mut(&PARLOR_ID)
        .expect("the Parlor is on the board")
        .cards_at_location
        .push(CardInPlay::enter_play(
            CardCode::new(LITA),
            LITA_INST,
            Owner::Scenario,
        ));
    state
}

/// Win the Parley, so the investigator controls Lita.
fn parley(state: GameState) -> GameState {
    let parley = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(LITA_INST),
        address: AbilityAddress::Granted {
            granter: CardCode::new(PARLOR),
            ability: 1,
            sub: 0,
        },
    };
    TestSession::new(state)
        .resolve_choices(|c: &mut ScriptedResolver| {
            c.commit_cards(&[]);
        })
        .take(&parley)
        .finish()
        .state
}

/// Reveal Crypt Chill against the investigator, failing its test.
fn reveal_crypt_chill(mut state: GameState) -> ApplyResult {
    // An encounter card is revealed at rest, never beneath an open-turn prompt.
    state.continuations = ContinuationStack::new();
    state.phase = Phase::Mythos;
    state.chaos_bag.tokens = vec![ChaosToken::AutoFail];
    state.encounter_deck.push_back(CardCode::new(CRYPT_CHILL));
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    test_support::drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: INV }),
        resolver,
    )
}

/// **Crypt Chill removes a Parleyed Lita from the game** (#918). She is the
/// only asset the investigator controls, so the failed test discards her; she
/// leaves play, and because the scenario owns her she goes to no discard pile.
#[test]
fn crypt_chill_removes_a_parleyed_lita_from_the_game() {
    let controlled = parley(parlor_with_lita());
    assert!(
        controlled.investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == LITA_INST),
        "the Parley put Lita under the investigator's control",
    );

    let result = reveal_crypt_chill(controlled);
    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&INV];
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
        !result
            .state
            .encounter_discard
            .contains(&CardCode::new(LITA)),
        "nor into the encounter discard, got {:?}",
        result.state.encounter_discard,
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
