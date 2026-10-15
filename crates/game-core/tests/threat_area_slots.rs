//! A slot-bearing asset in the threat area is held in a slot, so playing an
//! asset into full slots offers it as a make-room discard (#1092).
//! `glossary/Slots.md`: *"Each asset in an investigator's play area or threat
//! area with a slot symbol is held in a slot of that type."* and *"If playing
//! or gaining control of an asset would put an investigator above his or her
//! slot limit for that type of asset, the investigator must choose and discard
//! other assets under his or her control simultaneously with the new asset
//! entering the slot."*
//!
//! The only corpus card that reaches a threat area with a slot is The
//! Necronomicon 01009, which cannot leave play while it has horror. So the
//! primitive is pinned with synthetic single-hand assets (ADR 0016), one of
//! them seated in the threat area.
//!
//! Its own test binary: the mock registry is built once per process.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};

/// A synthetic single-hand asset, seated in the threat area.
const CHARM: &str = "TEST-THREAT-CHARM";
/// A synthetic single-hand asset, seated in the play area.
const GRIP: &str = "TEST-GRIP";
/// A synthetic single-hand asset, played from hand into full hand slots.
const TORCH: &str = "TEST-TORCH";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

fn one_hand_asset(code: &str) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: code.to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(one_hand_asset(CHARM))
            .with_card(one_hand_asset(GRIP))
            .with_card(one_hand_asset(TORCH))
            .with_abilities(TORCH, Vec::new)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// CHARM in the threat area and GRIP in the play area fill both hand slots;
/// TORCH is in hand. Playing it suspends on the make-room choice.
fn torch_into_full_hands() -> TestSession {
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .in_play([GRIP])
                .threat_area([CHARM])
                .hand([TORCH]),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session();
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    })
}

#[test]
fn a_threat_area_slot_asset_is_offered_as_a_make_room_candidate() {
    let session = torch_into_full_hands();
    let charm = session.state().in_play(INV, CHARM);
    let grip = session.state().in_play(INV, GRIP);
    assert_eq!(
        (
            session.offered_at(&OptionTarget::CardInstance(charm)),
            session.offered_at(&OptionTarget::CardInstance(grip)),
        ),
        (1, 1),
        "both hand-slot holders are offered, the threat-area one included: {:?}",
        session.prompt()
    );

    let session = session.pick(OptionTarget::CardInstance(charm));
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert!(inv.threat_area.is_empty(), "the charm left the threat area");
    assert_eq!(inv.discard, vec![CardCode::new(CHARM)]);
    assert_eq!(
        inv.cards_in_play
            .iter()
            .map(|c| c.code.as_str())
            .collect::<Vec<_>>(),
        vec![GRIP, TORCH]
    );
}
