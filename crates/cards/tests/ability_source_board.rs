//! The act/agenda bullet: the current act and the current agenda are reachable
//! ability sources from anywhere on the board.
//!
//! `glossary/Triggered_Abilities.md`, third bullet, verbatim:
//!
//! > The current act or current agenda card.
//!
//! Unlike the second bullet, this one names no location — an investigator
//! activates an act or agenda ability from wherever they stand, and from
//! nowhere at all. And unlike the first, it names no controller. Disrupting the
//! Ritual 01148's ruling says the same about the printed card
//! (<https://arkhamdb.com/card/01148>), verbatim:
//!
//! > Your investigator doesn't need to be at the Ritual Site in order to
//! > activate the ability of this act card.
//!
//! Both halves are asserted at both seams: an ability must be **offered** by
//! the turn-menu enumerator *and* accepted by the apply entry point.
//!
//! The engine-side peer of these cases — the reachability predicate's own
//! answers, without a registry — is `engine::ability_source`'s unit tests,
//! which build the same board shape one crate down. The **real-corpus** peer,
//! checking that the widening did not turn the forced act and agenda abilities
//! The Gathering ships into activations, is `enumerate_actions.rs`.
//!
//! Own integration-test binary so it can build its own `MockRegistry`.
//! **No corpus card can exercise this**: the act and agenda abilities the
//! corpus prints — the resigns on Predator or Prey? 01121a and Time Is Running
//! Short 01122 (*"\[action\]: **Resign.** You don't want to risk taking too long,
//! so you head to safety with the information you've gathered."*), and the act
//! abilities on Uncovering the Conspiracy 01123 (*"\[action\] The investigators
//! spend 2 clues per investigator, as a group: Draw the top card of the Cultist
//! deck."*) and Disrupting the Ritual 01148 — have no ability implementations.
//! Purpose-built abilities prove reachability directly. Prior art:
//! `ability_source_colocation.rs`.

use card_dsl::dsl::{self, Ability, InvestigatorTarget, UsageLimit, UsagePeriod};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, GameState, GameStateBuilder, InvestigatorId, LocationId,
};
use game_core::test_support::{self, act, agenda, seat, MockRegistry, Seat, TestSession};
use game_core::{assert_event, assert_no_event};
use std::sync::LazyLock;

/// Synthetic **act**, standing in for Uncovering the Conspiracy 01123.
const ACT_ONE: &str = "SRCACT01";
/// The act that supersedes [`ACT_ONE`] when the cursor moves on. It carries the
/// same probe abilities, so "reachable" and "no longer reachable" differ only
/// in which act is current.
const ACT_TWO: &str = "SRCACT02";
/// Synthetic **agenda**, standing in for Predator or Prey? 01121a.
const AGENDA: &str = "SRCAGD01";

const MINE: InvestigatorId = InvestigatorId(1);
const NEIGHBOUR: InvestigatorId = InvestigatorId(2);

const HERE: LocationId = LocationId(1);
const THERE: LocationId = LocationId(2);

/// Ability index of the one activation that is legal here.
const LIVE: u8 = 0;
/// Ability index of an activation whose effect cannot change the game state.
const INERT: u8 = 1;
/// Ability index of an activation carrying a *"Limit once per round"* — which
/// a source with no card instance cannot record.
/// TODO(#699): track usage limits on a board source.
const LIMITED: u8 = 2;

/// The same three abilities on every synthetic board card, so "offered from
/// this source" and "not offered from that one" differ only in the source.
fn probe_abilities() -> Vec<Ability> {
    vec![
        dsl::activated(
            1,
            vec![],
            dsl::gain_resources(InvestigatorTarget::Active, 1),
        ),
        // Nobody is damaged on this board, so healing damage is provably
        // inert (`effect_can_change_state`).
        dsl::activated(1, vec![], dsl::heal_damage(InvestigatorTarget::Active, 1)),
        dsl::activated(
            1,
            vec![],
            dsl::gain_resources(InvestigatorTarget::Active, 1),
        )
        .with_usage_limit(UsageLimit {
            count: 1,
            period: UsagePeriod::Round,
        }),
    ]
}

/// Two locations, two investigators — mine `HERE`, a neighbour `THERE` — and a
/// two-act deck plus a one-agenda deck, both cursors at the front.
fn board() -> GameState {
    board_with(seat(1).at(HERE))
}

/// [`board`] with my investigator seated as `mine`.
fn board_with(mine: Seat) -> GameState {
    board_on_act(mine, 0)
}

/// [`board_with`], with the act cursor at `act_index`.
fn board_on_act(mine: Seat, act_index: usize) -> GameState {
    table(mine)
        .with_act_deck([act(ACT_ONE, 99), act(ACT_TWO, 99)])
        .with_current_act(act_index)
        .with_agenda_deck([agenda(AGENDA, 99)])
        .build()
}

/// The locations and investigators of [`board_with`], with no act or agenda
/// deck.
fn table(mine: Seat) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(mine)
        .with_seat(seat(2).at(THERE))
        .with_location(test_support::location(HERE))
        .with_location(test_support::location(THERE))
        .with_seating([MINE, NEIGHBOUR])
        .open_turn(MINE)
}

fn activation(source: AbilitySource, ability_index: u8) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: MINE,
        source,
        address: AbilityAddress::Printed(ability_index),
    }
}

/// Offered by the enumerator *and* accepted by the apply entry point, with the
/// effect actually run.
fn assert_offered_and_activatable(state: GameState, source: AbilitySource, why: &str) {
    let action = activation(source, LIVE);
    assert!(
        enumerate::legal_actions(&state, registries().cards).contains(&action),
        "{why}, so its ability belongs in the turn menu; menu was {:?}",
        enumerate::legal_actions(&state, registries().cards),
    );

    let before = state.investigators[&MINE].resources;
    let session = TestSession::new(state, registries()).take(&action);
    session.assert_at_turn_menu(MINE);
    assert_eq!(
        session.state().investigators[&MINE].resources,
        before + 1,
        "{why}, so the activation should resolve: the ability's effect (gain 1 \
         resource) should have run",
    );
}

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // `TEST_INV`'s metadata rides `build`'s composed `metadata_for_test_inv`.
        cards: MockRegistry::new()
            .with_abilities(ACT_ONE, probe_abilities)
            .with_abilities(ACT_TWO, probe_abilities)
            .with_abilities(AGENDA, probe_abilities)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// *"The current act …"* — Uncovering the Conspiracy 01123's group clue-spend,
/// in the shape the engine can currently prove.
#[test]
fn the_current_act_offers_its_ability() {
    assert_offered_and_activatable(
        board(),
        AbilitySource::Act,
        "this is the current act, and the bullet naming it is gated on nothing",
    );
}

/// *"… or current agenda card."* — the Core resigns on 01121a and 01122.
#[test]
fn the_current_agenda_offers_its_ability() {
    assert_offered_and_activatable(
        board(),
        AbilitySource::Agenda,
        "this is the current agenda, and the bullet naming it is gated on nothing",
    );
}

/// The distinguishing property of this bullet: no location gate. Standing in
/// the Hallway rather than the Study changes nothing, and neither does standing
/// nowhere at all.
#[test]
fn both_are_reachable_from_anywhere_on_the_board() {
    for (mover, why) in [
        (
            Some(THERE),
            "an investigator at another location reaches the board cards",
        ),
        (
            None,
            "an investigator who is at no location at all still reaches them",
        ),
    ] {
        for source in [AbilitySource::Act, AbilitySource::Agenda] {
            let mine = match mover {
                Some(location) => seat(1).at(location),
                None => seat(1),
            };
            assert_offered_and_activatable(board_with(mine), source, why);
        }
    }
}

/// `Event::AbilityActivated` names the **source**, not a card instance: the act
/// has none, so the event has to say what actually carried the ability.
#[test]
fn the_activation_event_names_the_board_card_it_came_from() {
    let result = TestSession::new(board(), registries())
        .take(&activation(AbilitySource::Act, LIVE))
        .finish();
    assert_event!(
        result.events,
        Event::AbilityActivated {
            investigator,
            source: AbilitySource::Act,
            code,
            address: AbilityAddress::Printed(LIVE),
        } if *investigator == MINE && code.as_str() == ACT_ONE
    );
}

/// The descriptor names *the current* act, not an act by position. Once the
/// cursor moves on, the superseded act's ability is gone from the menu and the
/// same activation resolves against the act that is current now.
#[test]
fn an_act_that_is_no_longer_current_is_not_reachable() {
    let state = board_on_act(seat(1).at(HERE), 1);

    let result = TestSession::new(state, registries())
        .take(&activation(AbilitySource::Act, LIVE))
        .finish();
    assert_event!(
        result.events,
        Event::AbilityActivated { code, .. } if code.as_str() == ACT_TWO
    );
    // The superseded act must not be what the activation reached.
    assert_no_event!(
        result.events,
        Event::AbilityActivated { code, .. } if code.as_str() == ACT_ONE
    );
}

/// A fixture with no acts or agendas offers no board source, and naming one
/// rejects on reachability rather than reaching for an empty deck.
#[test]
fn an_absent_act_or_agenda_is_out_of_reach() {
    let state = table(seat(1).at(HERE)).build();
    let menu = enumerate::legal_actions(&state, registries().cards);

    for source in [AbilitySource::Act, AbilitySource::Agenda] {
        assert!(
            !menu.contains(&activation(source, LIVE)),
            "{source:?} must stay out of the turn menu with no deck loaded; menu was {menu:?}",
        );
        // Activating it with no deck loaded must reject.
        let reason = TestSession::new(state.clone(), registries())
            .assert_rejects(|s| s.submit_off_menu(&activation(source, LIVE)));
        assert!(
            reason.contains("cannot reach"),
            "the reason should name reachability, got: {reason}",
        );
    }
}

/// Reachability answers only *which sources are addressable*. Legality is
/// unchanged: a provably inert effect is refused from the board cards too
/// (`Appendix_I_Initiation_Sequence.md`'s *"verifying that the resolution of
/// the effect has the potential to change the game state"*).
#[test]
fn an_inert_ability_stays_unoffered_from_the_board_cards() {
    let state = board();
    let menu = enumerate::legal_actions(&state, registries().cards);
    for source in [AbilitySource::Act, AbilitySource::Agenda] {
        assert!(
            !menu.contains(&activation(source, INERT)),
            "the inert ability on {source:?} must stay unoffered; menu was {menu:?}",
        );
    }
}

/// Usage state is per-instance (`CardInPlay::ability_usage`), and neither board
/// card has an instance — so a limited act or agenda ability reaches an
/// unreachable-branch panic, and making them activatable is what puts that
/// branch behind player input. It must **reject**, with a reason that names
/// the open issue building the capability.
/// TODO(#699): track usage limits on a board source.
#[test]
fn a_usage_limited_board_ability_rejects_naming_699_rather_than_panicking() {
    let state = board();
    for source in [AbilitySource::Act, AbilitySource::Agenda] {
        let action = activation(source, LIMITED);

        assert!(
            !enumerate::legal_actions(&state, registries().cards).contains(&action),
            "an ability the engine cannot cap must not be offered; menu was {:?}",
            enumerate::legal_actions(&state, registries().cards),
        );

        // A usage-limited ability on a board card must reject.
        let reason = TestSession::new(state.clone(), registries())
            .assert_rejects(|s| s.submit_off_menu(&action));
        assert!(
            reason.contains("usage limit") && reason.contains("#699"),
            "the reason should name the limit and the issue that builds it, got: {reason}",
        );
    }
}
