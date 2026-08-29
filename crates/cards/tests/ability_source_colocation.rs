//! The co-location bullet: scenario cards at your location are reachable —
//! the location itself, the encounter cards placed on it, and the threat areas
//! of *any* investigator standing there (#708).
//!
//! `glossary/Triggered_Abilities.md`, second bullet, verbatim:
//!
//! > A scenario card that is in play and at the same location as the
//! > investigator. This includes the location itself, encounter cards placed at
//! > that location, and all encounter cards in the threat area of any
//! > investigator at that location.
//!
//! The threat-area half is **not** controller-scoped, which Haunted 01098's
//! ruling (<https://arkhamdb.com/card/01098>) states directly, verbatim:
//!
//! > Any investigator at the same location as the investigator with Haunted in
//! > their threat area may trigger the [action][action] to discard Haunted, as
//! > per the FAQ [V1.0, section 2.1].
//!
//! Both halves are asserted at both seams: an ability must be **offered** by
//! the turn-menu enumerator *and* accepted by the apply entry point. The
//! Parlor's Resign missing from the menu is as much the defect as its being
//! refused on submission.
//!
//! The engine-side peer of these cases — the reachability predicate's own
//! answers, without a registry — is `engine::ability_source`'s unit tests,
//! which build the same board shape one crate down.
//!
//! Own integration-test binary so it can install a hand-rolled `CardRegistry`.
//! The abilities are **purpose-built** rather than borrowed from the corpus, so
//! that "reachable here" and "unreachable there" differ only in the source: the
//! same three probes sit on the location, on the enemy, on the threat-area card
//! and on the investigator card, and only one of them (`gain_resources`) is
//! observable. The Parlor 01115's Resign is a real implementation since #644,
//! but it is the wrong instrument here — it eliminates its user, so every case
//! after the first would be reasoning about a board that no longer has an
//! investigator on it. Prior art: `ability_source_control.rs`.

use std::sync::OnceLock;

use game_core::assert_event;
use game_core::card_data::{CardKind, CardMetadata};
use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{
    activated, gain_resources, heal_damage, Ability, InvestigatorTarget, UsageLimit, UsagePeriod,
};
use game_core::engine::{legal_actions, EngineOutcome, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, EnemyId, GameState, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, metadata_for_test_inv, test_enemy, test_investigator,
    test_location, GameStateBuilder, TEST_INV,
};

/// Synthetic **location** card, standing in for the Parlor 01115. Both
/// locations on the board print it, so "reachable here" and "unreachable
/// there" differ only in where the investigator stands.
const HALL: &str = "SRCLOC01";
/// Synthetic **enemy**, standing in for a Parley cultist — Herman Collins
/// 01138's *"[action] Choose and discard 4 cards from your hand: **Parley.**"*
/// is exactly an *"encounter card placed at that location"*.
const CULTIST: &str = "SRCENE01";
/// Synthetic **treachery** in an investigator's threat area, standing in for
/// Haunted 01098.
const WARD: &str = "SRCTRE01";

const MINE: InvestigatorId = InvestigatorId(1);
const NEIGHBOUR: InvestigatorId = InvestigatorId(2);
const STRANGER: InvestigatorId = InvestigatorId(3);

const HERE: LocationId = LocationId(1);
const THERE: LocationId = LocationId(2);
const CULTIST_HERE: EnemyId = EnemyId(1);
const CULTIST_THERE: EnemyId = EnemyId(2);
const NEIGHBOURS_WARD: CardInstanceId = CardInstanceId(21);
const STRANGERS_WARD: CardInstanceId = CardInstanceId(31);

/// Ability index of the one activation that is legal here.
const LIVE: u8 = 0;
/// Ability index of an activation whose effect cannot change the game state.
const INERT: u8 = 1;
/// Ability index of an activation carrying a *"Limit once per round"* — which
/// a source with no card instance cannot record (#699).
const LIMITED: u8 = 2;

/// The same three abilities on every synthetic card — and on the investigator
/// card, so the control bullet #707 shipped can be re-checked against the same
/// probe — so "offered from this source" and "not offered from that one"
/// differ only in the source.
fn probe_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        TEST_INV | HALL | CULTIST | WARD => Some(vec![
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

fn metadata(code: &'static str, name: &'static str, kind: CardKind) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: name.to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness: false,
        kind,
    }
}

fn probe_metadata(code: &CardCode) -> Option<&'static CardMetadata> {
    static WARD_META: OnceLock<CardMetadata> = OnceLock::new();
    metadata_for_test_inv(code).or_else(|| match code.as_str() {
        WARD => Some(WARD_META.get_or_init(|| {
            metadata(
                WARD,
                "Ward",
                CardKind::Treachery {
                    surge: false,
                    peril: false,
                    quantity: 1,
                },
            )
        })),
        _ => None,
    })
}

#[ctor::ctor(unsafe)]
fn install_probe_registry() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: probe_metadata,
        abilities_for: probe_abilities,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
        back_abilities_for: |_| None,
        native_condition_for: |_| None,
    });
}

/// Two locations printing the same card, an enemy on each, and three
/// investigators: mine and a neighbour standing `HERE`, a stranger `THERE`.
/// The neighbour and the stranger each carry a threat-area card, so the only
/// difference between the reachable one and the unreachable one is where its
/// bearer stands.
fn board() -> GameState {
    let mine = test_investigator(1);

    let mut neighbour = test_investigator(2);
    neighbour
        .threat_area
        .push(CardInPlay::enter_play(CardCode::new(WARD), NEIGHBOURS_WARD));

    let mut stranger = test_investigator(3);
    stranger
        .threat_area
        .push(CardInPlay::enter_play(CardCode::new(WARD), STRANGERS_WARD));

    let mut here = test_location(1, "Hall");
    here.code = CardCode::new(HALL);
    let mut there = test_location(2, "Far Hall");
    there.code = CardCode::new(HALL);

    let mut nearby_cultist = test_enemy(1, "Cultist");
    nearby_cultist.code = CardCode::new(CULTIST);
    nearby_cultist.current_location = Some(HERE);
    let mut distant_cultist = test_enemy(2, "Far Cultist");
    distant_cultist.code = CardCode::new(CULTIST);
    distant_cultist.current_location = Some(THERE);

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(mine, HERE)
        .with_investigator_at(neighbour, HERE)
        .with_investigator_at(stranger, THERE)
        .with_location(here)
        .with_location(there)
        .with_enemy(nearby_cultist)
        .with_enemy(distant_cultist)
        .with_active_investigator(MINE)
        .with_turn_order([MINE, NEIGHBOUR, STRANGER])
        .with_investigator_turn(MINE)
        .build()
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

/// Neither offered nor accepted, and the rejection leaves the board untouched.
fn assert_out_of_reach(state: GameState, source: AbilitySource, why: &str) {
    let action = activation(source, LIVE);
    let before = state.clone();

    assert!(
        !legal_actions(&state).contains(&action),
        "{why}, so it must stay out of the turn menu; menu was {:?}",
        legal_actions(&state),
    );

    let result = dispatch_turn_action_unchecked(state, &action);
    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!("{why}, so activating it must reject; got {result:?}");
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

/// *"This includes the location itself"* — the Parlor's Resign, in the shape
/// the engine can currently prove (#258).
#[test]
fn the_location_you_stand_at_offers_its_ability() {
    assert_offered_and_activatable(
        board(),
        AbilitySource::Location(HERE),
        "you are standing at this location",
    );
}

/// `Event::AbilityActivated` names the **source**, not a card instance: a
/// location has none, so the event has to say what actually carried the
/// ability.
#[test]
fn the_activation_event_names_a_source_with_no_card_instance() {
    let result =
        dispatch_turn_action_unchecked(board(), &activation(AbilitySource::Location(HERE), LIVE));
    assert_event!(
        result.events,
        Event::AbilityActivated {
            investigator,
            source: AbilitySource::Location(location),
            code,
            ability_index: LIVE,
        } if *investigator == MINE && *location == HERE && code.as_str() == HALL
    );
}

#[test]
fn a_location_you_are_not_at_does_not() {
    assert_out_of_reach(
        board(),
        AbilitySource::Location(THERE),
        "you are not at this location",
    );
}

/// *"encounter cards placed at that location"* — the Midnight Masks Parley
/// cultists (Herman Collins 01138, Peter Warren 01139, Victoria Devereux
/// 01140) and Mob Enforcer 01101 all print their `[action]` on an enemy.
#[test]
fn an_enemy_at_your_location_offers_its_ability() {
    assert_offered_and_activatable(
        board(),
        AbilitySource::Enemy(CULTIST_HERE),
        "this enemy is placed at your location",
    );
}

#[test]
fn an_enemy_at_another_location_does_not() {
    assert_out_of_reach(
        board(),
        AbilitySource::Enemy(CULTIST_THERE),
        "this enemy is at another location",
    );
}

/// *"all encounter cards in the threat area of **any** investigator at that
/// location"* — Haunted 01098's ruling, at the apply seam.
#[test]
fn a_colocated_investigators_threat_area_card_offers_its_ability() {
    assert_offered_and_activatable(
        board(),
        AbilitySource::InPlay(NEIGHBOURS_WARD),
        "its bearer is standing at your location",
    );
}

#[test]
fn a_threat_area_card_on_an_investigator_elsewhere_does_not() {
    assert_out_of_reach(
        board(),
        AbilitySource::InPlay(STRANGERS_WARD),
        "its bearer is at another location",
    );
}

/// Reachability answers only *which sources are addressable*. Legality is
/// unchanged: a provably inert effect is refused from every newly reachable
/// source alike (`Appendix_I_Initiation_Sequence.md`'s *"verifying that the
/// resolution of the effect has the potential to change the game state"*).
#[test]
fn an_inert_ability_stays_unoffered_from_every_newly_reachable_source() {
    let state = board();
    let menu = legal_actions(&state);
    for source in [
        AbilitySource::Location(HERE),
        AbilitySource::Enemy(CULTIST_HERE),
        AbilitySource::InPlay(NEIGHBOURS_WARD),
    ] {
        assert!(
            !menu.contains(&activation(source, INERT)),
            "the inert ability on {source:?} must stay unoffered; menu was {menu:?}",
        );
    }
}

/// Usage state is per-instance (`CardInPlay::ability_usage`), and a location
/// has no instance — so a limited location ability reaches an
/// unreachable-branch panic, and this is the change that first puts that branch
/// behind player input. It must **reject**, naming #699, which builds the
/// capability Base of the Hill 02282 (*"(Limit once per round.)"*) and Ten-Acre
/// Meadow 02246 (*"(Group limit once per game)"*) will need.
#[test]
fn a_usage_limited_ability_on_a_location_rejects_naming_699_rather_than_panicking() {
    let state = board();
    let action = activation(AbilitySource::Location(HERE), LIMITED);
    let before = state.clone();

    assert!(
        !legal_actions(&state).contains(&action),
        "an ability the engine cannot cap must not be offered; menu was {:?}",
        legal_actions(&state),
    );

    let result = dispatch_turn_action_unchecked(state, &action);
    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!("a usage-limited location ability must reject, got {result:?}");
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

/// The same limit on an in-play instance is *not* refused: the rejection is
/// about the source having nowhere to record a use, not about limits.
#[test]
fn the_same_limit_on_a_card_instance_is_not_refused() {
    let state = board();
    assert!(
        legal_actions(&state)
            .contains(&activation(AbilitySource::InPlay(NEIGHBOURS_WARD), LIMITED)),
        "a threat-area card carries per-instance usage state; menu was {:?}",
        legal_actions(&state),
    );
}

/// #707's control bullet is untouched by the widening.
#[test]
fn abilities_reachable_under_the_control_bullet_are_unaffected() {
    let state = board();
    let own_card = state.investigators[&MINE].investigator_card.instance_id;
    assert_offered_and_activatable(
        state,
        AbilitySource::InPlay(own_card),
        "your investigator card is a card in play under your control",
    );
}
