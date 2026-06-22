//! PR-6 (#321) integration: Medical Texts 01035's `[action] Choose an
//! investigator at your location and test [intellect] (2). If you succeed,
//! heal 1 damage from that investigator. If you fail, deal 1 damage to that
//! investigator.` end-to-end against the real `cards::REGISTRY`.
//!
//! Exercises the first `Effect::SkillTest` initiated from an *activated*
//! ability: the test suspends at the commit window (driven by
//! `apply_no_commits`) and resumes through `advance`, then the
//! outcome branch heals or deals 1 damage to the chosen co-located
//! investigator. In solo the choice auto-binds to the controller (one
//! candidate at the location), so no choice suspend occurs.
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::dsl::HarmKind;
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase,
    TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event, Action, PlayerAction};

const MEDICAL_TEXTS: &str = "01035";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const BOOK_INST: CardInstanceId = CardInstanceId(0);

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Board: Medical Texts in play; the active investigator with `intellect`
/// intellect and `damage` damage, alone at `LOC`. A `Numeric(0)` chaos bag
/// makes the intellect(2) test deterministic — intellect 3 succeeds, 1 fails,
/// both off the difficulty boundary.
fn board(intellect: i8, damage: u8) -> game_core::GameState {
    install();
    let mut inv = test_investigator(1);
    inv.skills.intellect = intellect;
    inv.damage = damage;
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(MEDICAL_TEXTS),
        BOOK_INST,
    ));

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn activate(state: game_core::GameState) -> game_core::engine::ApplyResult {
    apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: INV,
            instance_id: BOOK_INST,
            ability_index: 0,
        }),
    )
}

#[test]
fn success_heals_one_damage_from_the_chosen_investigator() {
    // intellect 3 + 0 = 3 >= difficulty 2 → success → heal 1 damage.
    let r = activate(board(3, 2));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::SkillTestSucceeded { .. });
    assert_event!(
        r.events,
        Event::Healed {
            kind: HarmKind::Damage,
            amount: 1,
            ..
        }
    );
    assert_eq!(r.state.investigators[&INV].damage, 1, "1 damage healed");
}

#[test]
fn failure_deals_one_damage_to_the_chosen_investigator() {
    // intellect 1 + 0 = 1 < difficulty 2 → failure → deal 1 damage.
    let r = activate(board(1, 0));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::SkillTestFailed { .. });
    assert_event!(
        r.events,
        Event::DamageTaken {
            investigator: INV,
            amount: 1,
        }
    );
    assert_eq!(r.state.investigators[&INV].damage, 1, "1 damage dealt");
}
