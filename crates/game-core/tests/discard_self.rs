//! `Cost::DiscardSelf`: an activated ability discards its own in-play asset
//! as a cost. Mock registry in its own integration binary (own process +
//! `OnceLock<CardRegistry>`), mirroring `weapon_fight.rs`.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot};
use game_core::dsl::{activated, gain_resources, Ability, Cost, InvestigatorTarget};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, Phase};
use game_core::test_support::{apply_no_commits, test_investigator, GameStateBuilder};
use game_core::{assert_event, Action, PlayerAction};

const TRINKET: &str = "TRNK1";

fn trinket_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| CardMetadata {
        code: TRINKET.to_owned(),
        name: "Mock Trinket".to_owned(),
        traits: vec!["Item".to_owned()],
        text: Some("[fast] Discard Mock Trinket: gain 1 resource.".to_owned()),
        pack_code: "_mock".to_owned(),
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
        },
    })
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    (code.as_str() == TRINKET).then(trinket_static)
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        // [fast] Discard Mock Trinket: gain 1 resource.
        TRINKET => Some(vec![activated(
            0,
            vec![Cost::DiscardSelf],
            gain_resources(InvestigatorTarget::You, 1),
        )]),
        _ => None,
    }
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(game_core::card_registry::CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: |_| None,
        });
    });
}

#[test]
fn discard_self_removes_source_from_play_and_runs_the_effect() {
    install_mock_registry();
    let id = InvestigatorId(1);
    let inst = CardInstanceId(0);
    let mut inv = test_investigator(1);
    let before = inv.resources;
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(TRINKET), inst));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator(inv)
        .build();

    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id: inst,
            ability_index: 0,
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv_after = &result.state.investigators[&id];
    assert!(inv_after.cards_in_play.is_empty(), "source asset left play");
    assert_eq!(inv_after.discard, vec![CardCode::new(TRINKET)]);
    assert_eq!(inv_after.resources, before + 1, "the effect still ran");
    assert_event!(
        result.events,
        Event::CardDiscarded {
            from: game_core::state::Zone::InPlay,
            ..
        }
    );
}
