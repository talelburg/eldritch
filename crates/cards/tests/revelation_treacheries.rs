//! Integration: The Gathering's four one-shot Revelation treacheries
//! (C4b, #234) resolved through the real `cards` registry and the full
//! `apply` → commit-window → resolve → discard pipeline. Own process so
//! it can install the process-global registry against the real corpus.
//!
//! The skill-test margin math and the suspended-revelation discard
//! mechanism are unit-tested in game-core (#286); here we prove the
//! *card effects* resolve end-to-end against real corpus metadata.

use cards::REGISTRY;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    Agenda, CardCode, ChaosToken, DiscardPile, GameStateBuilder, InvestigatorId, LocationId,
    MythosPhaseFrame, MythosResume, Phase, Zone,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_count};

/// Holy Rosary 01059 — a real corpus asset.
const HOLY_ROSARY: &str = "01059";
/// Magnifying Glass 01030 — a second real corpus asset.
const MAGNIFYING_GLASS: &str = "01030";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Common board: Mythos step 1.4, with `investigator` at a location about to
/// draw, the named treachery on top of the encounter deck, a single rigged
/// chaos token, and `agenda` as the agenda deck.
fn board_seating(
    investigator: Seat,
    treachery: &str,
    token: ChaosToken,
    agenda: Vec<Agenda>,
) -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_seat(investigator)
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_turn_order([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .build();
    state.chaos_bag.tokens = vec![token];
    state.encounter_deck.push_back(CardCode::new(treachery));
    // The builder stages no agenda deck.
    state.agenda_deck = agenda;
    TestSession::new(state)
}

/// [`board_seating`] with a plain investigator at `HERE` and no agenda.
fn board_with(treachery: &str, token: ChaosToken) -> TestSession {
    board_seating(seat(1).at(HERE), treachery, token, Vec::new())
}

/// Draw the encounter card for investigator 1, committing no cards at the
/// skill-test commit window, and drain to the investigator's turn.
fn reveal_committing_nothing(session: TestSession) -> TestSession {
    let session = session.confirm().commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn grasping_hands_deals_one_damage_per_point_failed_then_discards() {
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage.
    let session = reveal_committing_nothing(board_with("01162", ChaosToken::Numeric(-2)));
    let state = session.state();
    assert_eq!(
        state.investigators[&INV].damage(),
        2,
        "1 damage per point failed (failed by 2)",
    );
    assert!(
        state.encounter_discard.contains(&CardCode::new("01162")),
        "the treachery discards after its suspended Revelation resolves",
    );
    assert!(!state.has_skill_test_in_flight());
}

#[test]
fn grasping_hands_fail_by_two_is_single_deal_of_two_damage() {
    // #426: Count(SkillTestFailedBy) deals all N damage in one Deal evaluation,
    // emitting a single DamageTaken { amount: 2 } event — not two separate
    // DamageTaken { amount: 1 } events.
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2.
    let session = reveal_committing_nothing(board_with("01162", ChaosToken::Numeric(-2)));
    assert_eq!(session.state().investigators[&INV].damage(), 2);
    // Exactly one DamageTaken event, with amount == 2.
    assert_event_count!(session.events(), 1, Event::DamageTaken { .. });
    assert_event!(
        session.events(),
        Event::DamageTaken { investigator, amount: 2 } if *investigator == INV
    );
}

#[test]
fn rotting_remains_fail_by_two_is_single_deal_of_two_horror() {
    // #426: Count(SkillTestFailedBy) deals all N horror in one Deal evaluation,
    // emitting a single HorrorTaken { amount: 2 } event — not two separate
    // HorrorTaken { amount: 1 } events.
    // Willpower 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2.
    let session = reveal_committing_nothing(board_with("01163", ChaosToken::Numeric(-2)));
    assert_eq!(session.state().investigators[&INV].horror(), 2);
    // Exactly one HorrorTaken event, with amount == 2.
    assert_event_count!(session.events(), 1, Event::HorrorTaken { .. });
    assert_event!(
        session.events(),
        Event::HorrorTaken { investigator, amount: 2 } if *investigator == INV
    );
}

#[test]
fn crypt_chill_with_no_asset_takes_two_damage_then_discards() {
    // Willpower 3 + Numeric(0) = 3 vs difficulty 4 → fail; no asset in
    // play → take 2 damage instead.
    let session = reveal_committing_nothing(board_with("01167", ChaosToken::Numeric(0)));
    assert_eq!(
        session.state().investigators[&INV].damage(),
        2,
        "no asset controlled → 2 damage fallback",
    );
    assert!(session
        .state()
        .encounter_discard
        .contains(&CardCode::new("01167")));
}

#[test]
fn crypt_chill_with_an_asset_discards_the_asset_not_damage() {
    // AutoFail forces the test to fail regardless of Holy Rosary's
    // +1-willpower buff, so the failure branch reliably runs. A real corpus
    // asset (Holy Rosary 01059) is in play.
    let session = reveal_committing_nothing(board_seating(
        seat(1).at(HERE).in_play([HOLY_ROSARY]),
        "01167",
        ChaosToken::AutoFail,
        Vec::new(),
    ));
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 0, "controlled an asset → no damage");
    assert!(
        inv.cards_in_play.is_empty(),
        "the asset was discarded out of play",
    );
    assert!(inv.discard.contains(&CardCode::new(HOLY_ROSARY)));
    assert_event!(
        session.events(),
        Event::CardDiscarded { code, from, to: DiscardPile::Investigator(investigator) }
            if *investigator == INV
                && *code == CardCode::new(HOLY_ROSARY)
                && *from == Zone::InPlay
    );
}

/// Two controlled assets — Holy Rosary 01059 and Magnifying Glass 01030 — with
/// Crypt Chill to draw and an `AutoFail` token forcing the failure.
fn crypt_chill_with_two_assets() -> TestSession {
    board_seating(
        seat(1).at(HERE).in_play([HOLY_ROSARY, MAGNIFYING_GLASS]),
        "01167",
        ChaosToken::AutoFail,
        Vec::new(),
    )
}

/// The suspended discard pick names each asset by its card (#989), not by the
/// `Debug` form of its instance id.
#[test]
fn crypt_chill_discard_options_are_labelled_with_card_names() {
    let s = crypt_chill_with_two_assets().confirm().commit(&[]);
    let rosary = s.state().in_play(INV, HOLY_ROSARY);
    let glass = s.state().in_play(INV, MAGNIFYING_GLASS);
    let labels: Vec<_> = s
        .prompt()
        .options
        .iter()
        .map(|o| (o.target.clone(), o.label.as_str()))
        .collect();
    assert_eq!(
        labels,
        vec![
            (Some(OptionTarget::CardInstance(rosary)), "Holy Rosary"),
            (Some(OptionTarget::CardInstance(glass)), "Magnifying Glass"),
        ],
    );
}

#[test]
fn crypt_chill_with_two_assets_suspends_and_discards_the_chosen_one() {
    // Two controlled assets ⇒ the fail branch suspends for the controller's
    // choice (Axis A #334). AutoFail forces the failure.
    let session = crypt_chill_with_two_assets();
    let glass = OptionTarget::CardInstance(session.state().in_play(INV, MAGNIFYING_GLASS));

    // Commit nothing at the test window, then pick the second asset
    // (Magnifying Glass) at the discard choice, by the instance it anchors to.
    let session = session.confirm().commit(&[]).pick(glass);

    // The choice was consumed: play went on into the investigator's turn.
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(
        inv.damage(),
        0,
        "an asset was discarded → no damage fallback"
    );
    // The chosen asset (Magnifying Glass) is gone; the other stays in play.
    assert!(
        inv.discard.contains(&CardCode::new(MAGNIFYING_GLASS)),
        "the chosen asset was discarded",
    );
    assert_eq!(
        inv.cards_in_play.len(),
        1,
        "only the unchosen asset remains in play",
    );
    assert_eq!(
        inv.cards_in_play[0].code,
        CardCode::new(HOLY_ROSARY),
        "the unchosen Holy Rosary stays",
    );
}

#[test]
fn ancient_evils_places_doom_on_the_current_agenda_then_discards() {
    // No skill test — Numeric token is unused by this revelation. The draw is
    // staged after Mythos step 1.2's doom, so the Revelation's is the only one.
    let drawing = board_seating(
        seat(1).at(HERE),
        "01166",
        ChaosToken::Numeric(0),
        vec![Agenda {
            code: CardCode::new("01105"),
            doom_threshold: 3,
        }],
    );
    let doom_before = drawing.state().agenda_doom;

    let session = drawing.confirm();
    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().agenda_doom,
        doom_before + 1,
        "Ancient Evils places 1 doom on the current agenda",
    );
    assert!(session
        .state()
        .encounter_discard
        .contains(&CardCode::new("01166")));
}
