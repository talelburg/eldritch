//! Activation costs resolve their source by **identity**, not by its position
//! in the controller's `cards_in_play`.
//!
//! The hazard this pins down: a cost that removes the source mid-payment
//! (`SpendUses` depleting a `discard_when_empty` asset) shifts every later
//! entry in `cards_in_play` down one. A cached position would then address
//! whatever card slid into the slot — silently exhausting a *different* card.
//! Resolving by `CardInstanceId` at payment time cannot: the source is gone, so
//! the later cost rejects and the apply boundary rolls the whole activation
//! back.
//!
//! Own integration-test binary so it can build its own `MockRegistry`
//! (no corpus card pairs a depleting `SpendUses` with a later `Exhaust`)
//! without colliding with the real-corpus binaries. Prior art:
//! `reject_rollback.rs`.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Uses};
use card_dsl::dsl::{self, Ability, Cost, InvestigatorTarget};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, GameStateBuilder, InvestigatorId, LocationId, UseKind,
};
use game_core::test_support::{
    self, seat, uses, MockRegistry, SeatCard, SeatLookup as _, TestSession,
};
use std::sync::LazyLock;

/// Synthetic asset: `Uses (1 supply)`, discards itself when they deplete, and
/// its ability spends that last supply *and then* exhausts. Not in the corpus.
const DEPLETER: &str = "SRCID001";
/// A second synthetic asset, in play behind the depleter, so a stale position
/// index has something wrong to point at.
const BYSTANDER: &str = "SRCID002";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn probe_abilities() -> Vec<Ability> {
    vec![dsl::activated(
        0,
        vec![
            Cost::SpendUses {
                kind: UseKind::Supplies,
                count: 1,
            },
            Cost::Exhaust,
        ],
        dsl::gain_resources(InvestigatorTarget::Active, 1),
    )]
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
        weakness: None,
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

/// The investigator's turn at the Study with `in_play` in play, in order.
fn board(in_play: [SeatCard; 2]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).in_play(in_play))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
}

/// The depleter's ability, activated from `session`'s board.
fn activate_depleter(session: &TestSession) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, DEPLETER)),
        address: AbilityAddress::Printed(0),
    }
}

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(asset_metadata(
                DEPLETER,
                "Depleter",
                Some(Uses {
                    kind: UseKind::Supplies,
                    count: 1,
                    discard_when_empty: true,
                }),
            ))
            .with_card(asset_metadata(BYSTANDER, "Bystander", None))
            .with_abilities(DEPLETER, probe_abilities)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

#[test]
fn cost_after_the_source_leaves_play_rejects_rather_than_hitting_another_card() {
    // Depleter at position 0 with its last supply, bystander behind it at 1.
    let session = board([
        (DEPLETER, uses(UseKind::Supplies, 1)).into(),
        BYSTANDER.into(),
    ]);
    let before = session.state().clone();

    let activate = activate_depleter(&session);
    let session = session.take(&activate);

    // Exhaust after the source self-discarded must reject.
    session.expect_rejected();
    assert_eq!(
        *session.state(),
        before,
        "a rejected activation must leave state byte-identical \
         (pre-#706: the stale position exhausted the bystander)",
    );
    let events = session.finish().events;
    assert!(
        events.is_empty(),
        "a rejected activation must emit no events, got {events:?}",
    );
}

/// The other half of identity addressing: with the source sitting *behind*
/// another card in play, both source-referencing costs must still land on the
/// source. A validation-time position happens to be right here, so this is the
/// regression guard for the rewrite rather than a pre-existing bug.
#[test]
fn costs_land_on_the_source_when_it_is_not_first_in_play() {
    // Two supplies: spending one does not deplete, so the source stays in play
    // and the following Exhaust cost has something to find.
    let session = board([
        BYSTANDER.into(),
        (DEPLETER, uses(UseKind::Supplies, 2)).into(),
    ]);
    let bystander_id = session.state().in_play(INV, BYSTANDER);
    let depleter_id = session.state().in_play(INV, DEPLETER);

    let activate = activate_depleter(&session);
    let session = session.take(&activate);

    // The activation resolves and play is back at the turn menu.
    session.assert_at_turn_menu(INV);

    let in_play = &session.state().investigators[&INV].cards_in_play;
    let bystander = in_play
        .iter()
        .find(|c| c.instance_id == bystander_id)
        .expect("bystander stays in play");
    let source = in_play
        .iter()
        .find(|c| c.instance_id == depleter_id)
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
