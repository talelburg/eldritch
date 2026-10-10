//! Integration: The Gathering's four one-shot Revelation treacheries
//! (C4b, #234) resolved through the real `cards` registry and the full
//! `apply` → commit-window → resolve → discard pipeline. Own process so
//! it can install the process-global registry against the real corpus.
//!
//! The skill-test margin math and the suspended-revelation discard
//! mechanism are unit-tested in game-core (#286); here we prove the
//! *card effects* resolve end-to-end against real corpus metadata.

use cards::REGISTRY;
use game_core::action::{Action, EngineRecord};
use game_core::engine::{ApplyResult, EngineOutcome, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    Agenda, CardCode, CardInPlay, CardInstanceId, ChaosToken, DiscardPile, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Owner, Zone,
};
use game_core::test_support::{self, ScriptedResolver, TestSession};
use game_core::{assert_event, assert_event_count};

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Reveal the top encounter card for investigator 1, auto-committing no
/// cards at the skill-test commit window (if one opens).
fn reveal_top(state: GameState) -> ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    test_support::drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    )
}

/// Common board: one investigator at a location, the named treachery on
/// top of the encounter deck, and a single rigged chaos token.
fn board_with(treachery: &str, token: ChaosToken) -> GameState {
    let inv = test_support::test_investigator(1);
    let mut state = GameStateBuilder::new()
        .with_investigator_at(inv, LocationId(20))
        .with_location(test_support::test_location(20, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.chaos_bag.tokens = vec![token];
    state.encounter_deck.push_back(CardCode::new(treachery));
    state
}

#[test]
fn grasping_hands_deals_one_damage_per_point_failed_then_discards() {
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage.
    let result = reveal_top(board_with("01162", ChaosToken::Numeric(-2)));
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].damage(),
        2,
        "1 damage per point failed (failed by 2)",
    );
    assert!(
        result
            .state
            .encounter_discard
            .contains(&CardCode::new("01162")),
        "the treachery discards after its suspended Revelation resolves",
    );
    assert!(!result.state.has_skill_test_in_flight());
}

#[test]
fn grasping_hands_fail_by_two_is_single_deal_of_two_damage() {
    // #426: Count(SkillTestFailedBy) deals all N damage in one Deal evaluation,
    // emitting a single DamageTaken { amount: 2 } event — not two separate
    // DamageTaken { amount: 1 } events.
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2.
    let result = reveal_top(board_with("01162", ChaosToken::Numeric(-2)));
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.investigators[&InvestigatorId(1)].damage(), 2);
    // Exactly one DamageTaken event, with amount == 2.
    assert_event_count!(result.events, 1, Event::DamageTaken { .. });
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 2 }
            if *investigator == InvestigatorId(1)
    );
}

#[test]
fn rotting_remains_fail_by_two_is_single_deal_of_two_horror() {
    // #426: Count(SkillTestFailedBy) deals all N horror in one Deal evaluation,
    // emitting a single HorrorTaken { amount: 2 } event — not two separate
    // HorrorTaken { amount: 1 } events.
    // Willpower 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2.
    let result = reveal_top(board_with("01163", ChaosToken::Numeric(-2)));
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.investigators[&InvestigatorId(1)].horror(), 2);
    // Exactly one HorrorTaken event, with amount == 2.
    assert_event_count!(result.events, 1, Event::HorrorTaken { .. });
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 2 }
            if *investigator == InvestigatorId(1)
    );
}

#[test]
fn crypt_chill_with_no_asset_takes_two_damage_then_discards() {
    // Willpower 3 + Numeric(0) = 3 vs difficulty 4 → fail; no asset in
    // play → take 2 damage instead.
    let result = reveal_top(board_with("01167", ChaosToken::Numeric(0)));
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].damage(),
        2,
        "no asset controlled → 2 damage fallback",
    );
    assert!(result
        .state
        .encounter_discard
        .contains(&CardCode::new("01167")));
}

#[test]
fn crypt_chill_with_an_asset_discards_the_asset_not_damage() {
    // AutoFail forces the test to fail regardless of Holy Rosary's
    // +1-willpower buff, so the failure branch reliably runs.
    let mut state = board_with("01167", ChaosToken::AutoFail);
    // Put a real corpus asset (Holy Rosary 01059) into play.
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .cards_in_play
        .push(CardInPlay::enter_play(
            CardCode::new("01059"),
            CardInstanceId(1),
            Owner::Investigator(InvestigatorId(1)),
        ));

    let result = reveal_top(state);
    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.damage(), 0, "controlled an asset → no damage");
    assert!(
        inv.cards_in_play.is_empty(),
        "the asset was discarded out of play",
    );
    assert!(inv.discard.contains(&CardCode::new("01059")));
    assert_event!(
        result.events,
        Event::CardDiscarded { code, from, to: DiscardPile::Investigator(investigator) }
            if *investigator == InvestigatorId(1)
                && *code == CardCode::new("01059")
                && *from == Zone::InPlay
    );
}

/// The suspended discard pick names each asset by its card (#989), not by the
/// `Debug` form of its instance id.
#[test]
fn crypt_chill_discard_options_are_labelled_with_card_names() {
    let mut state = board_with("01167", ChaosToken::AutoFail);
    {
        let in_play = &mut state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .cards_in_play;
        in_play.push(CardInPlay::enter_play(
            CardCode::new("01059"),
            CardInstanceId(1),
            Owner::Investigator(InvestigatorId(1)),
        ));
        in_play.push(CardInPlay::enter_play(
            CardCode::new("01030"),
            CardInstanceId(2),
            Owner::Investigator(InvestigatorId(1)),
        ));
    }

    let s = TestSession::new(state)
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .apply(Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }));
    let labels: Vec<_> = s
        .prompt()
        .options
        .iter()
        .map(|o| (o.target.clone(), o.label.as_str()))
        .collect();
    assert_eq!(
        labels,
        vec![
            (
                Some(OptionTarget::CardInstance(CardInstanceId(1))),
                "Holy Rosary"
            ),
            (
                Some(OptionTarget::CardInstance(CardInstanceId(2))),
                "Magnifying Glass"
            ),
        ],
    );
}

#[test]
fn crypt_chill_with_two_assets_suspends_and_discards_the_chosen_one() {
    // Two controlled assets ⇒ the fail branch suspends for the controller's
    // choice (Axis A #334). AutoFail forces the failure.
    let mut state = board_with("01167", ChaosToken::AutoFail);
    {
        let in_play = &mut state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .cards_in_play;
        // Two real corpus assets: Holy Rosary 01059, Magnifying Glass 01030.
        in_play.push(CardInPlay::enter_play(
            CardCode::new("01059"),
            CardInstanceId(1),
            Owner::Investigator(InvestigatorId(1)),
        ));
        in_play.push(CardInPlay::enter_play(
            CardCode::new("01030"),
            CardInstanceId(2),
            Owner::Investigator(InvestigatorId(1)),
        ));
    }

    // Commit nothing at the test window, then pick the second asset
    // (Magnifying Glass) at the discard choice, by the instance it anchors to.
    let mut resolver = ScriptedResolver::new();
    resolver
        .commit_cards(&[])
        .pick(OptionTarget::CardInstance(CardInstanceId(2)));
    let result = test_support::drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(
        inv.damage(),
        0,
        "an asset was discarded → no damage fallback"
    );
    // The chosen asset (Magnifying Glass) is gone; the other stays in play.
    assert!(
        inv.discard.contains(&CardCode::new("01030")),
        "the chosen asset was discarded",
    );
    assert_eq!(
        inv.cards_in_play.len(),
        1,
        "only the unchosen asset remains in play",
    );
    assert_eq!(
        inv.cards_in_play[0].code,
        CardCode::new("01059"),
        "the unchosen Holy Rosary stays",
    );
    assert!(
        result.state.continuations.is_empty(),
        "choice frame consumed"
    );
}

#[test]
fn ancient_evils_places_doom_on_the_current_agenda_then_discards() {
    // No skill test — Numeric token is unused by this revelation.
    let mut state = board_with("01166", ChaosToken::Numeric(0));
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    let doom_before = state.agenda_doom;

    let result = reveal_top(state);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.agenda_doom,
        doom_before + 1,
        "Ancient Evils places 1 doom on the current agenda",
    );
    assert!(result
        .state
        .encounter_discard
        .contains(&CardCode::new("01166")));
}
