//! Zero-action abilities on the newly reachable sources are available in
//! player windows.
//!
//! The rules bullets governing which sources an investigator can reach are
//! written **once**, for all three kinds of triggered ability together.
//! `glossary/Triggered_Abilities.md` opens:
//!
//! > An investigator is permitted to use triggered abilities ([free],
//! > [reaction], and [action] abilities) from the following sources:
//!
//! So a zero-action ability on a location, on an enemy at your location, on a
//! co-located threat-area card, on the act or on the agenda is reachable on
//! exactly the same terms as an action-costed one — and is offered in a
//! **player window** rather than through the Activate action, per
//! `glossary/Ability.md`, verbatim:
//!
//! > Free triggered abilities ([free]) – A [free] triggered ability may be
//! > triggered as a player ability during any player window.
//!
//! Which is also why the same entry's [action] clause is the one thing these
//! sources do *not* open in a window:
//!
//! > Action triggered abilities ([action]) – An [action] triggered ability may
//! > be triggered during a player's turn in the investigation phase through the
//! > use of the activate action […]
//!
//! Terminology: what `ArkhamDB`'s card text writes as the `[fast]` token is the
//! **free triggered ability** icon, not the `Fast` keyword (`GLOSSARY.md`,
//! *Fast*). This file says "zero-action ability" throughout.
//!
//! **This is confirmation, not construction.** `enumerate_fast_plays` and
//! `check_activate_ability` already ask `engine::ability_source` the same
//! question the turn-menu enumerator asks it, so every source the turn menu
//! reaches is reached by player windows by construction. These tests are what makes
//! that inheritance a checked property rather than a reading of the call graph.
//!
//! Corpus consumers in Dunwich, none of them implementable yet: Ten-Acre
//! Meadow 02246 (*"\[fast\]: You lure the monster into the rain. Place 1 clue
//! from the token bank on an [[Abomination]] enemy in Ten-Acre Meadow. At the
//! end of the round, remove 1 clue from that enemy. (Group limit once per
//! game)."*) and Dunwich Village 02242 (*"\[fast\]: You borrow some hounds to
//! track the creatures by scent. An investigator in Dunwich Village may place 1
//! of his or her clues on any [[Abomination]] enemy in play. (Group limit once
//! per game)."*) both print one on a **location**; Peter Clover 02079
//! (*"\[fast\] Exhaust Peter Clover: Automatically evade a [[Criminal]] enemy in
//! your location."*) prints one on an asset, which the control bullet already
//! covers. Both locations carry a group usage limit, which a source
//! with no card instance cannot record — so neither could be shipped here even
//! if the DSL had the effect. TODO(#699): record usage limits on a location,
//! act or agenda. Purpose-built abilities prove
//! reachability directly; prior art: `ability_source_colocation.rs`,
//! `ability_source_board.rs`.
//!
//! The **real-corpus** peer — that the zero-action abilities already on cards
//! in play are offered and resolve unchanged — is `fast_play.rs`, which
//! runs against `cards::TEST_REGISTRIES`.
//!
//! Own integration-test binary so it can build its own `MockRegistry`.
//!
//! # The player window these tests use
//!
//! A skill test's ST.1 window, opened by the engine rather than seeded onto the
//! builder's stack, so the prompt the client would see is the thing asserted
//! on. Two seams, both existing public engine entry points:
//!
//! - **The window's own option list** — whether an ability is *offered*. An
//!   option carries an [`OptionTarget`], which is what names the source; it
//!   does **not** name the investigator, and the window enumerates every
//!   investigator's plays. So a source another investigator reaches under a
//!   bullet of their own is offered here legitimately, and the per-investigator
//!   question ("can *this* investigator reach it") is asked at the second seam.
//! - **The apply entry point** — whether an activation submitted by a named
//!   investigator is accepted or rejected, with which reason.
//!
//! The board sits in the **Mythos** phase, at the step-1.4 encounter draw the
//! phase rests at, so the window is the only thing that
//! can permit a zero-action activation: `check_activate_ability`'s gate is
//! *"active during Investigation **or** an open permissive window"*, and an
//! Investigation-phase board would satisfy the first disjunct and mask the one
//! under test.

use card_dsl::card_data::{CardKind, CardMetadata};
use card_dsl::dsl::{self, Ability, InvestigatorTarget};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::engine::{InputKind, OptionTarget};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, Phase, SkillKind,
};
use game_core::test_support::{
    self, act, agenda, seat, MockRegistry, Seat, SeatLookup as _, TestSession,
};
use game_core::{assert_event, assert_no_event};
use std::sync::LazyLock;

/// Synthetic **location** card, standing in for Ten-Acre Meadow 02246. Two
/// locations on the board print it, so "offered here" and "not offered there"
/// differ only in whether an investigator is standing on it.
const HALL: &str = "ZEROLOC01";
/// A location card with no abilities at all, so the investigator parked out of
/// the way contributes nothing to the window's option list.
const EMPTY_HALL: &str = "ZEROLOC02";
/// Synthetic **enemy**, standing in for an *"encounter card placed at that
/// location"*.
const CULTIST: &str = "ZEROENE01";
/// Synthetic **treachery** in an investigator's threat area, standing in for
/// Haunted 01098.
const WARD: &str = "ZEROTRE01";
/// Synthetic **act**.
const ACT: &str = "ZEROACT01";
/// Synthetic **agenda**.
const AGENDA: &str = "ZEROAGD01";

const MINE: InvestigatorId = InvestigatorId(1);
const NEIGHBOUR: InvestigatorId = InvestigatorId(2);
const STRANGER: InvestigatorId = InvestigatorId(3);

const HERE: LocationId = LocationId(1);
const THERE: LocationId = LocationId(2);
const FAR: LocationId = LocationId(3);

const CULTIST_HERE: EnemyId = EnemyId(1);
const CULTIST_THERE: EnemyId = EnemyId(2);

/// Ability index of the zero-action ability — `Trigger::Activated
/// { action_cost: 0 }`, the `[free]` icon.
const ZERO_ACTION_ABILITY: u8 = 0;
/// Ability index of the action-costed ability on the same card — the `[action]`
/// icon, which a player window does **not** open.
const ACTION_COSTED_ABILITY: u8 = 1;

/// The starting action allowance, asserted unchanged after every zero-action
/// activation: *"a free triggered ability that does not cost an action"*.
const ACTIONS: u8 = 3;

/// The same two abilities on every synthetic card, so "offered from this
/// source" and "not offered from that one" differ only in the source, and
/// "offered in a window" and "not offered in a window" differ only in the
/// action cost.
fn probe_abilities() -> Vec<Ability> {
    vec![
        dsl::activated(
            0,
            vec![],
            dsl::gain_resources(InvestigatorTarget::Active, 1),
        ),
        dsl::activated(
            1,
            vec![],
            dsl::gain_resources(InvestigatorTarget::Active, 1),
        ),
    ]
}

/// [`WARD`]'s printed cardtype: co-location reaches a threat-area card only
/// as an encounter card, so the registry has to know it is a treachery.
fn ward_metadata() -> CardMetadata {
    CardMetadata {
        code: WARD.to_owned(),
        name: WARD.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness: None,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

/// Three locations and three investigators.
///
/// `HERE` and `THERE` both print [`HALL`]; mine and a neighbour stand `HERE`
/// and nobody stands `THERE`, so a source at `THERE` is reachable by no
/// investigator at all and its absence from the window is unambiguous. A
/// stranger stands `FAR`, on an abilityless location, carrying a threat-area
/// card of their own: the co-location bullet does not reach it from `HERE`,
/// which the apply seam asserts.
///
/// An enemy stands on `HERE` and another on `THERE`; the one `HERE` is engaged
/// with mine and ready, which is what makes the attack-of-opportunity case
/// meaningful.
fn board() -> GameState {
    board_seating(seat(1).at(HERE))
}

/// The [`board`], with mine seated as `mine` (always [`ACTIONS`] actions).
///
/// The Mythos phase, resting at `MINE`'s step-1.4 encounter draw: not
/// Investigation, so the window must be the only thing that permits the
/// activation (see the module header).
fn board_seating(mine: Seat) -> GameState {
    let here = test_support::location(HERE).code(HALL);
    let there = test_support::location(THERE).code(HALL);
    let far = test_support::location(FAR).code(EMPTY_HALL);

    let nearby = test_support::enemy(CULTIST_HERE).code(CULTIST);
    let distant = test_support::enemy(CULTIST_THERE).code(CULTIST).at(THERE);

    GameStateBuilder::new()
        .with_registries(registries())
        .with_phase(Phase::Mythos)
        .with_mythos_draw_remaining([MINE])
        .with_seat(mine.actions(ACTIONS))
        .with_seat(seat(2).at(HERE).threat_area([WARD]))
        .with_seat(seat(3).at(FAR).threat_area([WARD]))
        .with_location(here)
        .with_location(there)
        .with_location(far)
        .with_enemy_engaged(nearby, MINE)
        .with_enemy(distant)
        .with_active_investigator(MINE)
        .with_seating([MINE, NEIGHBOUR, STRANGER])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_act_deck([act(ACT, 99)])
        .with_agenda_deck([agenda(AGENDA, 99)])
        .build()
}

/// The [`WARD`] in `bearer`'s threat area.
fn ward_of(state: &GameState, bearer: InvestigatorId) -> AbilitySource {
    AbilitySource::InPlay(state.in_play(bearer, WARD))
}

/// Drive `state` to a real, engine-opened player window: the ST.1 window of a
/// plain skill test, started where the Mythos phase rests. Returns the session
/// parked at the window and the window's option anchors.
///
/// The prompt is asserted, not assumed — a board with nothing eligible would
/// auto-skip the window and land on the commit prompt instead, and a test that
/// mistook one for the other would pass for the wrong reason.
fn at_player_window(state: GameState) -> (TestSession, Vec<OptionTarget>) {
    let session = TestSession::new(state, registries()).test_skill(MINE, SkillKind::Willpower, 4);
    assert_eq!(
        session.state().phase,
        Phase::Mythos,
        "the window opens off-turn"
    );
    let request = session.prompt();
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "a player window is skippable — nobody is obliged to use a zero-action ability: \
         {request:?}",
    );
    let targets = request
        .options
        .iter()
        .map(|o| o.target.clone().expect("every option here is anchored"))
        .collect();
    (session, targets)
}

fn activation(source: AbilitySource, ability_index: u8) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: MINE,
        source,
        address: AbilityAddress::Printed(ability_index),
    }
}

/// Submit `source`'s zero-action ability as `MINE` at the open player window.
///
/// The apply seam, not a pick off the window: an option names its source but
/// not the investigator it was enumerated for (see the module header), so the
/// activation is submitted naming `MINE`, as a client would.
fn activate_at_window(window: TestSession, source: AbilitySource) -> TestSession {
    window.submit_off_menu(&activation(source, ZERO_ACTION_ABILITY))
}

/// The window offers this source's zero-action ability, and `MINE` can take it:
/// the effect runs and no action is spent.
fn assert_offered_and_usable(
    source: impl FnOnce(&GameState) -> AbilitySource,
    target: impl FnOnce(&GameState) -> OptionTarget,
    why: &str,
) {
    let (window, targets) = at_player_window(board());
    let (source, target) = (source(window.state()), target(window.state()));
    assert!(
        targets.contains(&target),
        "{why}, so its zero-action ability belongs in the player window; window offered {targets:?}",
    );

    let before = window.state().investigators[&MINE].resources;
    let session = activate_at_window(window, source);
    // Not rejected: the activation resolved and the window is offered again.
    assert!(
        session.prompt().skippable,
        "{why}, so the activation should resolve back to the window; got {:?}",
        session.prompt(),
    );
    assert_event!(
        session.events(),
        Event::AbilityActivated { investigator, source: s, address: AbilityAddress::Printed(ZERO_ACTION_ABILITY), .. }
            if *investigator == MINE && *s == source
    );
    assert_eq!(
        session.state().investigators[&MINE].resources,
        before + 1,
        "the ability's effect (gain 1 resource) should have run",
    );
    assert_eq!(
        session.state().investigators[&MINE].actions_remaining,
        ACTIONS,
        "*\"a free triggered ability that does not cost an action\"* — the allowance is untouched",
    );
}

/// `MINE` cannot reach this source, and the rejection leaves the board
/// untouched.
///
/// Asserted at the apply seam rather than against the window's option list,
/// because an option names its source and not the investigator it was
/// enumerated for; see the module header.
fn assert_out_of_reach(source: impl FnOnce(&GameState) -> AbilitySource, why: &str) {
    let (window, _) = at_player_window(board());
    let source = source(window.state());

    let reason = window.assert_rejects(|window| activate_at_window(window, source));
    assert!(
        reason.contains("cannot reach"),
        "{why}, so activating it must reject naming reachability, got: {reason}",
    );
}

// ---- the sources a window reaches ---------------------------------------

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // `TEST_INV`'s metadata rides `build`'s composed `metadata_for_test_inv`.
        cards: MockRegistry::new()
            .with_abilities(HALL, probe_abilities)
            .with_abilities(CULTIST, probe_abilities)
            .with_abilities(WARD, probe_abilities)
            .with_card(ward_metadata())
            .with_abilities(ACT, probe_abilities)
            .with_abilities(AGENDA, probe_abilities)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// *"This includes the location itself"* — Ten-Acre Meadow 02246 and Dunwich
/// Village 02242 both print a zero-action ability on a location.
#[test]
fn a_location_you_stand_at_offers_its_zero_action_ability_in_a_window() {
    assert_offered_and_usable(
        |_| AbilitySource::Location(HERE),
        |_| OptionTarget::Location(HERE),
        "you are standing at this location",
    );
}

/// The other half of the same bullet, at the offering seam: nobody stands at
/// `THERE`, so the identical ability printed on it reaches nobody.
#[test]
fn a_location_nobody_stands_at_offers_nothing_to_the_window() {
    let (_, targets) = at_player_window(board());
    assert!(
        !targets.contains(&OptionTarget::Location(THERE)),
        "no investigator is at this location, so its zero-action ability reaches nobody; \
         window offered {targets:?}",
    );
}

/// And at the per-investigator seam: the same location, submitted by an
/// investigator standing somewhere else.
#[test]
fn a_location_you_are_not_at_is_out_of_reach_even_in_a_window() {
    assert_out_of_reach(
        |_| AbilitySource::Location(THERE),
        "you are not at this location",
    );
}

/// *"encounter cards placed at that location"* — an enemy standing on your
/// location is one.
#[test]
fn an_enemy_at_your_location_offers_its_zero_action_ability_in_a_window() {
    assert_offered_and_usable(
        |_| AbilitySource::Enemy(CULTIST_HERE),
        |_| OptionTarget::Enemy(CULTIST_HERE),
        "this enemy is placed at your location",
    );
}

/// The offering seam's half of the same pair, split out to match the location
/// pair above: nobody stands where this enemy is, so it reaches nobody.
#[test]
fn an_enemy_nobody_stands_with_offers_nothing_to_the_window() {
    let (_, targets) = at_player_window(board());
    assert!(
        !targets.contains(&OptionTarget::Enemy(CULTIST_THERE)),
        "no investigator stands where this enemy is; window offered {targets:?}",
    );
}

#[test]
fn an_enemy_at_another_location_is_out_of_reach_even_in_a_window() {
    assert_out_of_reach(
        |_| AbilitySource::Enemy(CULTIST_THERE),
        "this enemy is at another location",
    );
}

/// *"all encounter cards in the threat area of **any** investigator at that
/// location"* — Haunted 01098's ruling (<https://arkhamdb.com/card/01098>),
/// in a player window rather than through the Activate action.
#[test]
fn a_colocated_investigators_threat_area_card_offers_its_zero_action_ability_in_a_window() {
    assert_offered_and_usable(
        |state| ward_of(state, NEIGHBOUR),
        |state| state.card(NEIGHBOUR, WARD),
        "its bearer is standing at your location",
    );
}

#[test]
fn a_threat_area_card_at_another_location_is_out_of_reach_even_in_a_window() {
    assert_out_of_reach(
        |state| ward_of(state, STRANGER),
        "its bearer is standing somewhere else",
    );
}

/// *"The current act or current agenda card."* — the bullet gated on nothing.
#[test]
fn the_current_act_offers_its_zero_action_ability_in_a_window() {
    assert_offered_and_usable(
        |_| AbilitySource::Act,
        |_| OptionTarget::Act,
        "this is the current act, and the bullet naming it is gated on nothing",
    );
}

#[test]
fn the_current_agenda_offers_its_zero_action_ability_in_a_window() {
    assert_offered_and_usable(
        |_| AbilitySource::Agenda,
        |_| OptionTarget::Agenda,
        "this is the current agenda, and the bullet naming it is gated on nothing",
    );
}

/// The distinguishing property of that bullet, held in a player window too: an
/// investigator standing nowhere at all still reaches both board cards, while
/// everything the co-location bullet gave them has gone.
#[test]
fn the_act_and_agenda_stay_reachable_from_nowhere_at_all() {
    // Mine is seated at no location at all.
    let (window, targets) = at_player_window(board_seating(seat(1)));

    for target in [OptionTarget::Act, OptionTarget::Agenda] {
        assert!(
            targets.contains(&target),
            "{target:?} is gated on nothing, so it survives leaving the map; window offered \
             {targets:?}",
        );
    }

    let mut window = window;
    for source in [AbilitySource::Act, AbilitySource::Agenda] {
        window = activate_at_window(window, source);
        assert_event!(
            window.events(),
            Event::AbilityActivated { source: s, .. } if *s == source
        );
    }

    let rejected = activate_at_window(window, AbilitySource::Location(HERE));
    assert!(
        rejected.expect_rejected().contains("cannot reach"),
        "the co-location bullet went with the location, so the Meadow is out of reach",
    );
}

// ---- what a player window does *not* open --------------------------------

/// *"An \[action\] triggered ability may be triggered during a player's turn in
/// the investigation phase through the use of the activate action"* — a player
/// window is neither, so the same card's action-costed ability stays shut.
///
/// This is what makes the offering assertions above load-bearing: both
/// abilities sit on the same card, reached through the same source, and only
/// the zero-action one is opened by the window.
#[test]
fn an_action_costed_ability_on_the_same_source_is_not_opened_by_the_window() {
    let (window, _) = at_player_window(board());
    let session = window.submit_off_menu(&activation(
        AbilitySource::Location(HERE),
        ACTION_COSTED_ABILITY,
    ));
    let reason = session.expect_rejected();
    assert!(
        reason.contains("action-cost ability requires Investigation phase"),
        "the reason should name the Activate action's gate, got: {reason}",
    );
}

/// *"a free triggered ability that does not cost an action"* — and RR p.5's
/// attack of opportunity is provoked by **actions**, so a ready enemy engaged
/// with the activating investigator does nothing at all.
/// Asserted for **every** source, not just one: the criterion reads across
/// them, and each kind reaches `activate_ability` by a different arm of the
/// reachability predicate.
#[test]
fn a_zero_action_ability_on_a_new_source_provokes_no_attack_of_opportunity() {
    let sources: [fn(&GameState) -> AbilitySource; 5] = [
        |_| AbilitySource::Location(HERE),
        |_| AbilitySource::Enemy(CULTIST_HERE),
        |state| ward_of(state, NEIGHBOUR),
        |_| AbilitySource::Act,
        |_| AbilitySource::Agenda,
    ];
    for source in sources {
        let (window, _) = at_player_window(board());
        let state = window.state();
        assert!(
            !state.enemies[&CULTIST_HERE].exhausted
                && state.enemies[&CULTIST_HERE].engaged_with == Some(MINE),
            "the AoO precondition — a ready enemy engaged with the activator — must hold",
        );
        let source = source(state);

        let session = activate_at_window(window, source);
        let events = session.events();
        assert_event!(
            events,
            Event::AbilityActivated { source: s, .. } if *s == source
        );
        // An attack of opportunity lands as damage/horror on the activator and
        // exhausts the attacker; none of the three appears.
        assert_no_event!(events, Event::DamagePlacedOnInvestigator { .. });
        assert_no_event!(events, Event::HorrorPlacedOnInvestigator { .. });
        assert_no_event!(events, Event::EnemyExhausted { .. });
        assert_eq!(
            session.state().investigators[&MINE].actions_remaining,
            ACTIONS,
            "no action was spent from {source:?}, so there was no action to provoke from",
        );
    }
}

// ---- the window's own contract -------------------------------------------

/// Nobody is obliged to use one: passing the window leaves every offered
/// ability unused and the skill test carries on.
#[test]
fn passing_the_window_uses_nothing() {
    let (window, _) = at_player_window(board());
    let before = window.state().investigators[&MINE].resources;
    let session = window.skip();
    assert_eq!(
        session.state().investigators[&MINE].resources,
        before,
        "skipping the window activates nothing",
    );
}
