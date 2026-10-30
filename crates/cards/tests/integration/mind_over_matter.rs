//! Mind over Matter 01036 — substituting Intellect for a
//! Combat/Agility test, the intellect-icon commit rule, the play-timing gate,
//! and the weapon-bonus interaction, end-to-end against the real
//! `cards::REGISTRY`.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::card_data::UseKind;
use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{InputKind, OptionTarget, PromptNature};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, EnemyId, FastWindowKind,
    GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame, MythosResume, Phase, PhaseStep,
    SkillKind,
};
use game_core::test_support::{enemy, location, seat, uses, Seat, SeatLookup as _, TestSession};

const MOM: &str = "01036";
const OVERPOWER: &str = "01091"; // combat skill icons
const SPECIAL: &str = "01006"; // .38 Special — Fight weapon
const MAGNIFYING_GLASS: &str = "01030"; // a Fast asset
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

/// Investigator 1 at `HERE` with `combat` / `intellect`, holding Mind over
/// Matter first in hand.
fn fighter(combat: i8, intellect: i8) -> Seat {
    seat(1)
        .at(HERE)
        .skill(SkillKind::Intellect, intellect)
        .skill(SkillKind::Combat, combat)
        .hand([MOM])
}

/// `investigator`'s turn at `HERE`, engaged with a fight-3 enemy. `combat` /
/// `intellect` are set so a `Numeric(0)` draw makes substitution flip the
/// outcome (combat fails the fight-3 test, intellect passes).
fn board(investigator: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(investigator)
        // Co-located (copied from the seat): Fight is location-gated. Health
        // 5 survives, so we can read `damage`.
        .with_enemy(enemy(ENEMY).fight(3).health(5).engaged_with(INV))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

fn play_card(session: TestSession, hand_index: u8) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(hand_index),
    })
}

fn fight(session: TestSession) -> TestSession {
    session.take(&TurnAction::Fight {
        investigator: INV,
        enemy: ENEMY,
    })
}

/// Answer the substitution prompt, a Decision (ADR 0015): printed option 0
/// uses the substitute skill, 1 keeps the printed one.
fn substitute(session: TestSession, use_substitute: bool) -> TestSession {
    session.pick_nth(usize::from(!use_substitute))
}

#[test]
fn play_then_fight_substituting_succeeds_via_intellect() {
    // combat 1 fails fight-3; intellect 4 passes.
    let s = play_card(board(fighter(1, 4)), 0);
    // MoM plays (Fast, your turn) and returns to the open-turn menu.
    s.assert_at_turn_menu(INV);
    assert!(
        s.state().investigators[&INV]
            .discard
            .contains(&CardCode::new(MOM)),
        "event discarded after play",
    );

    let s = fight(s);
    assert_eq!(
        s.prompt().nature,
        PromptNature::Decision,
        "substitution prompt"
    );
    let s = substitute(s, true); // use Intellect
    assert!(
        matches!(s.prompt().kind, InputKind::PickMultiple(_)),
        "commit window"
    );
    let s = s.commit(&[]);
    s.assert_at_turn_menu(INV);
    assert_event!(s.events(), Event::SkillTestSucceeded { .. });
    assert!(
        s.state().enemies[&ENEMY].damage >= 1,
        "Fight dealt damage on success"
    );
}

#[test]
fn fight_declining_substitution_fails_on_combat() {
    let s = play_card(board(fighter(1, 4)), 0);
    let s = substitute(fight(s), false).commit(&[]); // keep Combat
    s.assert_at_turn_menu(INV);
    assert_event!(s.events(), Event::SkillTestFailed { .. });
    assert_eq!(
        s.state().enemies[&ENEMY].damage,
        0,
        "combat 1 < fight 3 → no damage"
    );
}

#[test]
fn substituted_intellect_test_does_not_offer_a_combat_icon() {
    // FAQ: a substituted test *is* an Intellect test, so ST.2's icon
    // eligibility is judged against Intellect — Overpower's two [combat] icons
    // are not appropriate and the commit window does not offer it. This is
    // the substitution's sharpest observable consequence at the commit window:
    // the same card is offered to the un-substituted Combat test.
    // Hand: [MoM, Overpower]. Play MoM (idx 0) → hand becomes [Overpower].
    let overpower = OptionTarget::HandCard {
        investigator: INV,
        hand_index: 0,
    };
    let combat = substitute(
        fight(play_card(board(fighter(4, 2).hand([MOM, OVERPOWER])), 0)),
        false,
    );
    assert!(
        matches!(combat.prompt().kind, InputKind::PickMultiple(_)),
        "commit window"
    );
    assert_eq!(combat.offered_at(&overpower), 1, "offered to a Combat test");

    let s = play_card(board(fighter(4, 2).hand([MOM, OVERPOWER])), 0);
    let s = substitute(fight(s), true); // use Intellect → it's now an Intellect test
    assert!(
        matches!(s.prompt().kind, InputKind::PickMultiple(_)),
        "commit window"
    );
    assert_eq!(
        s.offered_at(&overpower),
        0,
        "not offered to an Intellect test"
    );

    // Committing nothing resolves the test: intellect 2 < fight 3.
    let s = s.commit(&[]);
    s.assert_at_turn_menu(INV);
    assert_event!(s.events(), Event::SkillTestFailed { .. });
    assert_eq!(
        s.state().enemies[&ENEMY].damage,
        0,
        "intellect 2 < fight 3, and no icons were admitted",
    );
}

#[test]
fn mind_over_matter_rejected_outside_your_turn() {
    // The Mythos window rests on Magnifying Glass, a Fast asset it may offer;
    // it does not offer MoM. The play is then sent off the menu to prove the
    // handler itself refuses it (`submit_off_menu`'s second use).
    let window = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Mythos)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE).hand([MOM, MAGNIFYING_GLASS]))
        .with_active_investigator(INV)
        .with_seating([INV])
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_open_window(FastWindowKind::Phase(PhaseStep::MythosAfterDraws))
        .session();
    assert_eq!(
        window.offered_at(&OptionTarget::HandCard {
            investigator: INV,
            hand_index: 0,
        }),
        0,
        "the Mythos window does not offer MoM: {:?}",
        window.prompt(),
    );

    let s = window.submit_off_menu(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    let reason = s.expect_rejected();
    assert!(
        !reason.is_empty(),
        "'Play only during your turn' rejects MoM in the Mythos window: {reason}",
    );
}

#[test]
fn weapon_fight_substituting_uses_intellect_and_keeps_weapon_damage() {
    // .38 Special: "You get +1 [combat] for this attack (if there are 1 or more
    // clues on your location, you get +3 [combat], instead). This attack deals
    // +1 damage." No clue here, so +1.
    // combat 1 + weapon +1 = 2 < fight 3 (would fail); substituting drops the
    // weapon's combat bonus and tests intellect 4 ≥ 3 → success, and the
    // weapon's +1 damage is still dealt (base 1 + 1 = 2).
    let session = board(fighter(1, 4).in_play([(SPECIAL, uses(UseKind::Ammo, 4))]));
    let weapon = session.state().in_play(INV, SPECIAL);

    let s = play_card(session, 0) // MoM
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(weapon),
            address: AbilityAddress::Printed(0),
        });
    assert_eq!(
        s.prompt().nature,
        PromptNature::Decision,
        "substitution prompt"
    );
    let s = substitute(s, true).commit(&[]); // use Intellect (drops the +combat weapon bonus)
    s.assert_at_turn_menu(INV);
    assert_event!(s.events(), Event::SkillTestSucceeded { .. });
    assert_eq!(
        s.state().enemies[&ENEMY].damage,
        2,
        ".38 Special's bonus damage is kept (1 + 1) even when substituting",
    );
}
