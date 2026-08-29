//! The control bullet: an activation names an **ability source**, and every
//! card in play under your control carries reachable abilities — including your
//! investigator card and your own threat area (#707).
//!
//! `glossary/Triggered_Abilities.md`, first bullet, verbatim:
//!
//! > A card in play and under his or her control. This includes his or her
//! > investigator card.
//!
//! Both halves are asserted at both seams: an ability must be **offered** by
//! the turn-menu enumerator *and* accepted by the apply entry point. The
//! Parlor's Resign missing from the menu is as much the defect as its being
//! refused on submission.
//!
//! Own integration-test binary so it can install a hand-rolled `CardRegistry`:
//! no corpus card carries an activated ability on an investigator card or on a
//! threat-area card, and none can be shipped to fake one (Haunted 01098's
//! discard has nowhere to go until #708/#644). Prior art:
//! `activation_cost_source.rs`.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{activated, gain_resources, heal_damage, Ability, Cost, InvestigatorTarget};
use game_core::engine::{legal_actions, EngineOutcome, TurnAction};
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, GameState, InvestigatorId, LocationId,
    Phase,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, metadata_for_test_inv, test_investigator, test_location,
    GameStateBuilder, TEST_INV,
};

/// Synthetic **treachery** sitting in an investigator's threat area — the card
/// type the zone actually holds (Rules Reference p.20: *"a play area in which
/// encounter cards currently engaged with and/or affecting an investigator are
/// placed"*), so the source under test has the corpus's shape.
const WARD: &str = "SRCCTL01";
/// Synthetic asset, in play under the *other* investigator's control.
const THEIRS: &str = "SRCCTL02";

const MINE: InvestigatorId = InvestigatorId(1);
const THEM: InvestigatorId = InvestigatorId(2);
const LOC: LocationId = LocationId(10);
const WARD_INST: CardInstanceId = CardInstanceId(1);
const THEIRS_INST: CardInstanceId = CardInstanceId(2);

/// Ability index of the one activation that is legal here.
const LIVE: u8 = 0;
/// Ability index of an activation whose cost cannot be paid.
const UNAFFORDABLE: u8 = 1;
/// Ability index of an activation whose effect cannot change the game state.
const INERT: u8 = 2;
/// Ability index of an activation that discards its own source as a cost.
const SELF_DISCARDING: u8 = 3;

/// The same three abilities on every synthetic card, so "offered from this
/// source" and "not offered from that one" differ only in the source.
///
/// `UNAFFORDABLE` and `INERT` are the initiation sequence's two preliminary
/// confirmations (`Appendix_I_Initiation_Sequence.md`) — *"verifying that the
/// resolution of the effect has the potential to change the game state"* and
/// that the cost *"can be paid"*. Widening which sources are addressable must
/// not widen what is legal, so they stay unoffered from every source.
fn probe_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        TEST_INV | WARD | THEIRS => Some(vec![
            activated(1, vec![], gain_resources(InvestigatorTarget::Active, 1)),
            activated(
                1,
                vec![Cost::Resources(99)],
                gain_resources(InvestigatorTarget::Active, 1),
            ),
            // Nobody is damaged on this board, so healing damage is provably
            // inert (`effect_can_change_state`).
            activated(1, vec![], heal_damage(InvestigatorTarget::Active, 1)),
            activated(
                1,
                vec![Cost::DiscardSelf],
                gain_resources(InvestigatorTarget::Active, 1),
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

fn asset_kind() -> CardKind {
    CardKind::Asset {
        class: Class::Neutral,
        cost: Some(0),
        xp: Some(0),
        slots: vec![],
        health: None,
        sanity: None,
        skill_icons: SkillIcons::default(),
        is_fast: false,
        deck_limit: 2,
        uses: None,
        play_only_during_turn: false,
    }
}

fn treachery_kind() -> CardKind {
    CardKind::Treachery {
        surge: false,
        peril: false,
        quantity: 1,
    }
}

fn probe_metadata(code: &CardCode) -> Option<&'static CardMetadata> {
    static WARD_META: OnceLock<CardMetadata> = OnceLock::new();
    static THEIRS_META: OnceLock<CardMetadata> = OnceLock::new();
    metadata_for_test_inv(code).or_else(|| match code.as_str() {
        WARD => Some(WARD_META.get_or_init(|| metadata(WARD, "Ward", treachery_kind()))),
        THEIRS => Some(THEIRS_META.get_or_init(|| metadata(THEIRS, "Theirs", asset_kind()))),
        _ => None,
    })
}

#[ctor::ctor(unsafe)]
fn install_probe_registry() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: probe_metadata,
        abilities_for: probe_abilities,
        ..CardRegistry::EMPTY
    });
}

/// Two investigators at the same location: mine holds a threat-area card, theirs
/// an asset in play. Co-location is deliberate — under the control bullet it
/// must not matter, and it is #708 that makes their threat area reachable.
fn board() -> GameState {
    let mut mine = test_investigator(1);
    mine.threat_area
        .push(CardInPlay::enter_play(CardCode::new(WARD), WARD_INST));
    let mut them = test_investigator(2);
    them.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(THEIRS), THEIRS_INST));

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(mine, LOC)
        .with_investigator_at(them, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(MINE)
        .with_turn_order([MINE, THEM])
        .with_investigator_turn(MINE)
        .build()
}

/// The acting investigator's own investigator-card instance.
fn my_investigator_card(state: &GameState) -> CardInstanceId {
    state.investigators[&MINE].investigator_card.instance_id
}

fn activation(source: AbilitySource, ability_index: u8) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: MINE,
        source,
        ability_index,
    }
}

#[test]
fn an_ability_on_your_own_investigator_card_is_offered_and_activatable() {
    let state = board();
    let action = activation(AbilitySource::InPlay(my_investigator_card(&state)), LIVE);

    assert!(
        legal_actions(&state).contains(&action),
        "the investigator card is a card in play under your control, so its ability \
         belongs in the turn menu; menu was {:?}",
        legal_actions(&state),
    );

    let result = dispatch_turn_action_unchecked(state, &action);
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "the activation should resolve, got {:?}",
        result.outcome,
    );
    assert_eq!(
        result.state.investigators[&MINE].resources, 6,
        "the ability's effect should have run",
    );
}

#[test]
fn an_ability_on_a_card_in_your_own_threat_area_is_offered_and_activatable() {
    let state = board();
    let action = activation(AbilitySource::InPlay(WARD_INST), LIVE);

    assert!(
        legal_actions(&state).contains(&action),
        "a card in your own threat area is in play and under your control; menu was {:?}",
        legal_actions(&state),
    );

    let result = dispatch_turn_action_unchecked(state, &action);
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "the activation should resolve, got {:?}",
        result.outcome,
    );
    assert_eq!(result.state.investigators[&MINE].resources, 6);
}

#[test]
fn an_ability_on_another_investigators_card_is_neither_offered_nor_activatable() {
    let state = board();
    let action = activation(AbilitySource::InPlay(THEIRS_INST), LIVE);
    let before = state.clone();

    assert!(
        !legal_actions(&state).contains(&action),
        "a card another investigator controls is out of reach; menu was {:?}",
        legal_actions(&state),
    );

    let result = dispatch_turn_action_unchecked(state, &action);
    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!("activating another investigator's card must reject, got {result:?}");
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

/// Reachability answers only *which sources are addressable*. Legality is
/// unchanged: an unpayable cost and a provably inert effect are refused from
/// every reachable source alike.
#[test]
fn an_unpayable_or_inert_ability_stays_unoffered_from_every_reachable_source() {
    let state = board();
    let menu = legal_actions(&state);
    for source in [
        AbilitySource::InPlay(my_investigator_card(&state)),
        AbilitySource::InPlay(WARD_INST),
    ] {
        for ability_index in [UNAFFORDABLE, INERT] {
            assert!(
                !menu.contains(&activation(source, ability_index)),
                "ability {ability_index} on {source:?} must stay unoffered; menu was {menu:?}",
            );
        }
    }
}

/// A `Cost::DiscardSelf` on a threat-area source sends the card to the
/// **encounter** discard, not to a player's discard pile: threat-area cards are
/// scenario-owned. Widening the addressable source set is what first made this
/// branch reachable — before it, `Cost::DiscardSelf` could only ever name a card
/// in `cards_in_play`, and the helper it reached panics on anything else.
#[test]
fn discarding_a_threat_area_source_as_a_cost_sends_it_to_the_encounter_discard() {
    let state = board();
    let action = activation(AbilitySource::InPlay(WARD_INST), SELF_DISCARDING);

    assert!(
        legal_actions(&state).contains(&action),
        "menu was {:?}",
        legal_actions(&state),
    );

    let result = dispatch_turn_action_unchecked(state, &action);
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "the activation should resolve, got {:?}",
        result.outcome,
    );
    assert!(
        result.state.investigators[&MINE].threat_area.is_empty(),
        "the source should have left the threat area",
    );
    assert_eq!(
        result.state.encounter_discard,
        vec![CardCode::new(WARD)],
        "a scenario-owned card goes to the encounter discard",
    );
    assert!(
        result.state.investigators[&MINE].discard.is_empty(),
        "and not to the investigator's own discard pile",
    );
}

/// The investigator card is reachable and cannot be discarded. It must reject
/// rather than panic: a **panic reachable from player input must not ship**.
#[test]
fn discarding_your_investigator_card_as_a_cost_rejects_rather_than_panicking() {
    let state = board();
    let action = activation(
        AbilitySource::InPlay(my_investigator_card(&state)),
        SELF_DISCARDING,
    );
    let before = state.clone();

    let result = dispatch_turn_action_unchecked(state, &action);
    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!("discarding the investigator card must reject, got {result:?}");
    };
    assert!(
        reason.contains("cannot be discarded as a cost"),
        "the reason should say why, got: {reason}",
    );
    assert_eq!(
        result.state, before,
        "a rejected activation must leave state byte-identical",
    );
}
