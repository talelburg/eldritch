//! Activation costs resolve their source by **identity**, not by its position
//! in the controller's `cards_in_play` (#706).
//!
//! The hazard this pins down: a cost that removes the source mid-payment
//! (`SpendUses` depleting a `discard_when_empty` asset) shifts every later
//! entry in `cards_in_play` down one. A cached position would then address
//! whatever card slid into the slot — silently exhausting a *different* card.
//! Resolving by `CardInstanceId` at payment time cannot: the source is gone, so
//! the later cost rejects and the apply boundary rolls the whole activation
//! back.
//!
//! Own integration-test binary so it can install a hand-rolled `CardRegistry`
//! (no corpus card pairs a depleting `SpendUses` with a later `Exhaust`)
//! without colliding with the real-corpus binaries. Prior art:
//! `reject_rollback.rs`.

use game_core::state::AbilityAddress;
use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons, Uses};
use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{activated, gain_resources, Ability, Cost, InvestigatorTarget};
use game_core::engine::{EngineOutcome, TurnAction};
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, InvestigatorId, LocationId, Phase, UseKind,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};

/// Synthetic asset: `Uses (1 supply)`, discards itself when they deplete, and
/// its ability spends that last supply *and then* exhausts. Not in the corpus.
const DEPLETER: &str = "SRCID001";
/// A second synthetic asset, in play behind the depleter, so a stale position
/// index has something wrong to point at.
const BYSTANDER: &str = "SRCID002";

const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const DEPLETER_INST: CardInstanceId = CardInstanceId(0);
const BYSTANDER_INST: CardInstanceId = CardInstanceId(1);

fn probe_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() != DEPLETER {
        return None;
    }
    Some(vec![activated(
        0,
        vec![
            Cost::SpendUses {
                kind: UseKind::Supplies,
                count: 1,
            },
            Cost::Exhaust,
        ],
        gain_resources(InvestigatorTarget::Active, 1),
    )])
}

fn asset_metadata(code: &'static str, name: &'static str, uses: Option<Uses>) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: name.to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: Some(0),
            slots: vec![],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses,
            play_only_during_turn: false,
        },
    }
}

fn probe_metadata(code: &CardCode) -> Option<&'static CardMetadata> {
    static DEPLETER_META: OnceLock<CardMetadata> = OnceLock::new();
    static BYSTANDER_META: OnceLock<CardMetadata> = OnceLock::new();
    match code.as_str() {
        DEPLETER => Some(DEPLETER_META.get_or_init(|| {
            asset_metadata(
                DEPLETER,
                "Depleter",
                Some(Uses {
                    kind: UseKind::Supplies,
                    count: 1,
                    discard_when_empty: true,
                }),
            )
        })),
        BYSTANDER => {
            Some(BYSTANDER_META.get_or_init(|| asset_metadata(BYSTANDER, "Bystander", None)))
        }
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install_probe_registry() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: probe_metadata,
        abilities_for: probe_abilities,
        ..CardRegistry::EMPTY
    });
}

/// Depleter at position 0 with its last supply, bystander behind it at 1.
fn board() -> game_core::GameState {
    let mut inv = test_investigator(1);
    let mut depleter = CardInPlay::enter_play(CardCode::new(DEPLETER), DEPLETER_INST);
    depleter.uses.insert(UseKind::Supplies, 1);
    inv.cards_in_play.push(depleter);
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(BYSTANDER),
        BYSTANDER_INST,
    ));

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build()
}

#[test]
fn cost_after_the_source_leaves_play_rejects_rather_than_hitting_another_card() {
    let state = board();
    let before = state.clone();

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(DEPLETER_INST),
            address: AbilityAddress::Printed(0),
        },
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Exhaust after the source self-discarded must reject, got {:?}",
        result.outcome,
    );
    assert_eq!(
        result.state, before,
        "a rejected activation must leave state byte-identical \
         (pre-#706: the stale position exhausted the bystander)",
    );
    assert!(
        result.events.is_empty(),
        "a rejected activation must emit no events, got {:?}",
        result.events,
    );
}

/// The other half of identity addressing: with the source sitting *behind*
/// another card in play, both source-referencing costs must still land on the
/// source. A validation-time position happens to be right here, so this is the
/// regression guard for the rewrite rather than a pre-existing bug.
#[test]
fn costs_land_on_the_source_when_it_is_not_first_in_play() {
    let mut inv = test_investigator(1);
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(BYSTANDER),
        BYSTANDER_INST,
    ));
    let mut depleter = CardInPlay::enter_play(CardCode::new(DEPLETER), DEPLETER_INST);
    // Two supplies: spending one does not deplete, so the source stays in play
    // and the following Exhaust cost has something to find.
    depleter.uses.insert(UseKind::Supplies, 2);
    inv.cards_in_play.push(depleter);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build();

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(DEPLETER_INST),
            address: AbilityAddress::Printed(0),
        },
    );

    // The drive loop carries on to the turn menu once the effect resolves, so
    // the success signal here is "not rejected" rather than `Done`.
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "the activation should resolve, got {:?}",
        result.outcome,
    );

    let in_play = &result.state.investigators[&INV].cards_in_play;
    let bystander = in_play
        .iter()
        .find(|c| c.instance_id == BYSTANDER_INST)
        .expect("bystander stays in play");
    let source = in_play
        .iter()
        .find(|c| c.instance_id == DEPLETER_INST)
        .expect("source stays in play with a supply left");

    assert!(source.exhausted, "the Exhaust cost must hit the source");
    assert!(
        !bystander.exhausted,
        "the Exhaust cost must not hit the card in front of the source",
    );
    assert_eq!(
        source.uses.get(&UseKind::Supplies).copied(),
        Some(1),
        "the SpendUses cost must drain the source's pool",
    );
    assert!(
        bystander.uses.is_empty(),
        "the SpendUses cost must not drain another card",
    );
}
