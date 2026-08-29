//! The act/agenda bullet: the current act and the current agenda are reachable
//! ability sources from anywhere on the board (#709).
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
//! Own integration-test binary so it can install a hand-rolled `CardRegistry`.
//! **No corpus card can exercise this**: the act and agenda abilities the
//! corpus prints — the resigns on Predator or Prey? 01121a and Time Is Running
//! Short 01122 (*"[action]: **Resign.** You don't want to risk taking too long,
//! so you head to safety with the information you've gathered."*), and the act
//! abilities on Uncovering the Conspiracy 01123 (*"[action] The investigators
//! spend 2 clues per investigator, as a group: Draw the top card of the Cultist
//! deck."*) and Disrupting the Ritual 01148 — have no ability implementations,
//! and shipping one is not available either: Resign's effect has nowhere to go
//! until #644. Purpose-built abilities prove reachability directly. Prior art:
//! `ability_source_colocation.rs`.

use game_core::assert_event;
use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{
    activated, gain_resources, heal_damage, Ability, InvestigatorTarget, UsageLimit, UsagePeriod,
};
use game_core::engine::{legal_actions, EngineOutcome, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilitySource, Act, Agenda, CardCode, GameState, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, metadata_for_test_inv, test_investigator, test_location,
    GameStateBuilder,
};

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
/// a source with no card instance cannot record (#699).
const LIMITED: u8 = 2;

/// The same three abilities on every synthetic board card, so "offered from
/// this source" and "not offered from that one" differ only in the source.
fn probe_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        ACT_ONE | ACT_TWO | AGENDA => Some(vec![
            activated(1, vec![], gain_resources(InvestigatorTarget::Active, 1)),
            // Nobody is damaged on this board, so healing damage is provably
            // inert (`effect_can_change_state`).
            activated(1, vec![], heal_damage(InvestigatorTarget::Active, 1)),
            activated(1, vec![], gain_resources(InvestigatorTarget::Active, 1)).with_usage_limit(
                UsageLimit {
                    count: 1,
                    period: UsagePeriod::Round,
                },
            ),
        ]),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install_probe_registry() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: metadata_for_test_inv,
        abilities_for: probe_abilities,
        back_abilities_for: |_| None,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
        native_condition_for: |_| None,
    });
}

/// Two locations, two investigators — mine `HERE`, a neighbour `THERE` — and a
/// two-act deck plus a one-agenda deck, both cursors at the front.
fn board() -> GameState {
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(test_investigator(1), HERE)
        .with_investigator_at(test_investigator(2), THERE)
        .with_location(test_location(1, "Study"))
        .with_location(test_location(2, "Hallway"))
        .with_active_investigator(MINE)
        .with_turn_order([MINE, NEIGHBOUR])
        .with_investigator_turn(MINE)
        .build();
    state.act_deck = vec![
        Act {
            code: CardCode::new(ACT_ONE),
            clue_threshold: 99,
        },
        Act {
            code: CardCode::new(ACT_TWO),
            clue_threshold: 99,
        },
    ];
    state.act_index = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(AGENDA),
        doom_threshold: 99,
    }];
    state.agenda_index = 0;
    state
}

fn activation(source: AbilitySource, ability_index: u8) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: MINE,
        source,
        ability_index,
    }
}

/// Offered by the enumerator *and* accepted by the apply entry point, with the
/// effect actually run.
fn assert_offered_and_activatable(state: GameState, source: AbilitySource, why: &str) {
    let action = activation(source, LIVE);
    assert!(
        legal_actions(&state).contains(&action),
        "{why}, so its ability belongs in the turn menu; menu was {:?}",
        legal_actions(&state),
    );

    let before = state.investigators[&MINE].resources;
    let result = dispatch_turn_action_unchecked(state, &action);
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{why}, so the activation should resolve; got {:?}",
        result.outcome,
    );
    assert_eq!(
        result.state.investigators[&MINE].resources,
        before + 1,
        "the ability's effect (gain 1 resource) should have run",
    );
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
            let mut state = board();
            state
                .investigators
                .get_mut(&MINE)
                .expect("on the board")
                .current_location = mover;
            assert_offered_and_activatable(state, source, why);
        }
    }
}

/// `Event::AbilityActivated` names the **source**, not a card instance: the act
/// has none, so the event has to say what actually carried the ability.
#[test]
fn the_activation_event_names_the_board_card_it_came_from() {
    let result = dispatch_turn_action_unchecked(board(), &activation(AbilitySource::Act, LIVE));
    assert_event!(
        result.events,
        Event::AbilityActivated {
            investigator,
            source: AbilitySource::Act,
            code,
            ability_index: LIVE,
        } if *investigator == MINE && code.as_str() == ACT_ONE
    );
}

/// The descriptor names *the current* act, not an act by position. Once the
/// cursor moves on, the superseded act's ability is gone from the menu and the
/// same activation resolves against the act that is current now.
#[test]
fn an_act_that_is_no_longer_current_is_not_reachable() {
    let mut state = board();
    state.act_index = 1;

    let result = dispatch_turn_action_unchecked(state, &activation(AbilitySource::Act, LIVE));
    assert_event!(
        result.events,
        Event::AbilityActivated { code, .. } if code.as_str() == ACT_TWO
    );
    assert!(
        !result.events.iter().any(|e| matches!(
            e,
            Event::AbilityActivated { code, .. } if code.as_str() == ACT_ONE
        )),
        "the superseded act must not be what the activation reached; events were {:?}",
        result.events,
    );
}

/// A fixture with no acts or agendas offers no board source, and naming one
/// rejects on reachability rather than reaching for an empty deck.
#[test]
fn an_absent_act_or_agenda_is_out_of_reach() {
    let mut state = board();
    state.act_deck.clear();
    state.agenda_deck.clear();
    let menu = legal_actions(&state);

    for source in [AbilitySource::Act, AbilitySource::Agenda] {
        assert!(
            !menu.contains(&activation(source, LIVE)),
            "{source:?} must stay out of the turn menu with no deck loaded; menu was {menu:?}",
        );
        let before = state.clone();
        let result = dispatch_turn_action_unchecked(state.clone(), &activation(source, LIVE));
        let EngineOutcome::Rejected { reason } = &result.outcome else {
            panic!("activating {source:?} with no deck loaded must reject; got {result:?}");
        };
        assert!(
            reason.contains("cannot reach"),
            "the reason should name reachability, got: {reason}",
        );
        assert_eq!(
            result.state, before,
            "a rejected activation must leave state byte-identical",
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
    let menu = legal_actions(&state);
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
/// branch behind player input. It must **reject**, naming #699, which builds
/// the capability.
#[test]
fn a_usage_limited_board_ability_rejects_naming_699_rather_than_panicking() {
    let state = board();
    for source in [AbilitySource::Act, AbilitySource::Agenda] {
        let action = activation(source, LIMITED);
        let before = state.clone();

        assert!(
            !legal_actions(&state).contains(&action),
            "an ability the engine cannot cap must not be offered; menu was {:?}",
            legal_actions(&state),
        );

        let result = dispatch_turn_action_unchecked(state.clone(), &action);
        let EngineOutcome::Rejected { reason } = &result.outcome else {
            panic!("a usage-limited ability on {source:?} must reject, got {result:?}");
        };
        assert!(
            reason.contains("usage limit") && reason.contains("#699"),
            "the reason should name the limit and the issue that builds it, got: {reason}",
        );
        assert_eq!(
            result.state, before,
            "a rejected activation must leave state byte-identical",
        );
    }
}
