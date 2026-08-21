//! Attacks of opportunity are exempted by the ability's **bold action
//! designator**, not by the shape of its effect (#696).
//!
//! `glossary/Attack_of_Opportunity.md`, verbatim:
//!
//! > Each time an investigator is engaged with one or more ready enemies and
//! > takes an action other than to **fight**, to **evade**, or to activate a
//! > **parley** or **resign** ability, each of those enemies makes an attack of
//! > opportunity against the investigator, in the order of the investigator's
//! > choosing. Each attack deals that enemy's damage and horror to the
//! > investigator.
//!
//! The exemption is four designators. The engine used to infer it from the
//! effect root — exempting `Effect::Fight` because every corpus weapon happens
//! to be rooted in one — which got two shapes wrong: a `Seq`-wrapped Fight
//! provoked, and **Parley** / **Resign** have no effect of their own to match,
//! so the Parlor 01115's *"[action] **Resign.**"* would have provoked once #708
//! made a location's abilities reachable.
//!
//! Own integration-test binary so it can install a hand-rolled `CardRegistry`.
//! **No corpus card can exercise any of these three shapes:** no shipped weapon
//! wraps its Fight in a `Seq`, and the Parley and Resign cards (the Parlor
//! 01115, the Midnight Masks cultists 01138-01140, Mob Enforcer 01101) have no
//! ability implementations — Resign's effect has nowhere to resolve until #644.
//! The synthetic cards stand in for them, printing the designator each does and
//! an effect chosen only so the *initiation* gate passes; what is asserted is
//! the attack of opportunity, never the effect. Prior art:
//! `ability_source_colocation.rs`, which stands in for the same cards.
//!
//! The real-corpus half — Machete 01020 (**Fight**, exempt), Flashlight 01087
//! (**Investigate**, not exempt), First Aid 01019 (no designator, not exempt) —
//! is `crates/cards/tests/activate_ability_aoo.rs`. The predicate's own
//! exhaustive table over the six designators is `provokes_aoo`'s unit test.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{
    activated, activated_as, fight, gain_resources, seq, Ability, ActionDesignator, Effect,
    InvestigatorTarget,
};
use game_core::engine::EngineOutcome;
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, GameState,
    InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, metadata_for_test_inv, test_enemy, test_investigator,
    test_location, GameStateBuilder,
};
use game_core::TurnAction;

/// Synthetic **asset**, standing in for a weapon that wraps its Fight in a
/// `Seq` — a shape no corpus weapon prints, and the one the retired effect-root
/// match got wrong.
const SATCHEL: &str = "DESIGN_ASSET";
/// Synthetic **location**, standing in for the Parlor 01115: *"[action]
/// **Resign.** 'This is too much for me!' You run out the front door, fleeing
/// in panic."*
const PARLOR: &str = "DESIGN_LOC";
/// Synthetic **enemy**, standing in for Mob Enforcer 01101: *"[action] Spend 3
/// resources: **Parley.** Mob Enforcer's engagement cost is 0 for the remainder
/// of the round."*
const ENFORCER: &str = "DESIGN_ENEMY";

const MINE: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(1);
const ATTACKER: EnemyId = EnemyId(1);
const BAG: CardInstanceId = CardInstanceId(10);

/// The designated Fight, wrapped in a `Seq` so its effect **root** is not a
/// `Effect::Fight`.
const SEQ_FIGHT_DESIGNATED: u8 = 0;
/// The same effect tree with **no** designator — the control that fixes what is
/// doing the work.
const SEQ_FIGHT_UNDESIGNATED: u8 = 1;
/// The sole ability on the location / on the enemy.
const ONLY: u8 = 0;

/// A `Seq` whose root is not a Fight but which fights inside it. The
/// `gain_resources` leg is there only to make the root a `Seq`.
fn seq_wrapped_fight() -> Effect {
    seq(vec![
        gain_resources(InvestigatorTarget::Active, 1),
        fight(0u8, 0u8),
    ])
}

fn probe_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SATCHEL => Some(vec![
            activated_as(ActionDesignator::Fight, 1, vec![], seq_wrapped_fight()),
            activated(1, vec![], seq_wrapped_fight()),
        ]),
        // The Parlor's Resign. `gain_resources` stands in for the effect:
        // resignation's own semantics are #644, and an ability whose effect
        // cannot change the game state is refused at initiation before the
        // attack-of-opportunity question is ever reached.
        PARLOR => Some(vec![activated_as(
            ActionDesignator::Resign,
            1,
            vec![],
            gain_resources(InvestigatorTarget::Active, 1),
        )]),
        // Mob Enforcer's Parley, with the same stand-in effect (engagement-cost
        // modifiers are not modelled).
        ENFORCER => Some(vec![activated_as(
            ActionDesignator::Parley,
            1,
            vec![],
            gain_resources(InvestigatorTarget::Active, 1),
        )]),
        _ => None,
    }
}

fn probe_metadata(code: &CardCode) -> Option<&'static CardMetadata> {
    static SATCHEL_META: OnceLock<CardMetadata> = OnceLock::new();
    metadata_for_test_inv(code).or_else(|| match code.as_str() {
        SATCHEL => Some(SATCHEL_META.get_or_init(|| CardMetadata {
            code: SATCHEL.to_string(),
            name: "Satchel".to_string(),
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
                slots: vec![],
                health: None,
                sanity: None,
                skill_icons: SkillIcons::default(),
                is_fast: false,
                deck_limit: 1,
                uses: None,
                play_only_during_turn: false,
            },
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
        native_condition_for: |_| None,
    });
}

/// One location printing the Parlor's card, the acting investigator standing on
/// it with the satchel in play, and one **ready** enemy engaged with them
/// dealing 1 damage. A single attacker keeps the attack-of-opportunity loop
/// synchronous — no order pick — and no soaker is in play, so the damage lands
/// on the investigator directly and `damage()` is the whole assertion.
///
/// The `Numeric(0)` chaos bag is there only so the Fight buried in the `Seq`
/// has a bag to draw from once the effect runs — the assertion is taken before
/// the test resolves either way.
fn board() -> GameState {
    let mut mine = test_investigator(1);
    mine.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(SATCHEL), BAG));

    let mut parlor = test_location(1, "Parlor");
    parlor.code = CardCode::new(PARLOR);

    let mut attacker = test_enemy(1, "Mob Enforcer");
    attacker.code = CardCode::new(ENFORCER);
    attacker.current_location = Some(HERE);
    attacker.engaged_with = Some(MINE);
    attacker.attack_damage = 1;
    attacker.attack_horror = 0;

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(mine, HERE)
        .with_location(parlor)
        .with_enemy(attacker)
        .with_active_investigator(MINE)
        .with_turn_order([MINE])
        .with_investigator_turn(MINE)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build()
}

fn activation(source: AbilitySource, ability_index: u8) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: MINE,
        source,
        ability_index,
    }
}

/// Activate and assert the activation was not rejected, returning the damage
/// the acting investigator is carrying afterwards. An attack of opportunity is
/// dealt after the costs and before the effect, so a non-zero reading is the
/// attack and nothing else: this board deals the investigator no other damage.
fn damage_after_activating(source: AbilitySource, ability_index: u8) -> u8 {
    let result = dispatch_turn_action_unchecked(board(), &activation(source, ability_index));
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "the activation itself must be legal for the attack-of-opportunity question \
         to arise; got {:?}",
        result.outcome,
    );
    result.state.investigators[&MINE].damage()
}

/// The designator, not the effect root: an ability printing **Fight** is exempt
/// even when its effect is a `Seq` that fights inside it. The retired
/// effect-root match saw a `Seq`, not a `Effect::Fight`, and provoked.
#[test]
fn a_seq_wrapped_fight_is_exempt_because_the_designator_says_fight() {
    assert_eq!(
        damage_after_activating(AbilitySource::InPlay(BAG), SEQ_FIGHT_DESIGNATED),
        0,
        "a **Fight** ability provokes no attack of opportunity, whatever shape \
         its effect tree has",
    );
}

/// The control that fixes what is doing the work: the *same* effect tree with
/// no printed designator is an ordinary activate action and provokes. So the
/// exemption above is the designator's doing, not the `Effect::Fight` buried in
/// the `Seq`.
#[test]
fn the_same_effect_tree_without_the_designator_provokes() {
    assert_eq!(
        damage_after_activating(AbilitySource::InPlay(BAG), SEQ_FIGHT_UNDESIGNATED),
        1,
        "an undesignated action-cost ability provokes, even though it fights \
         inside its effect",
    );
}

/// The Parlor 01115's *"[action] **Resign.**"*, activated while engaged with a
/// ready enemy, provokes nothing. **Resign** has no effect shape of its own, so
/// no effect-root match could ever have exempted it — and #708 made a
/// location's abilities reachable, which is what put the question on the board.
#[test]
fn a_resign_ability_on_a_location_provokes_nothing() {
    assert_eq!(
        damage_after_activating(AbilitySource::Location(HERE), ONLY),
        0,
        "**Resign** is on the exempt list",
    );
}

/// A **Parley** ability, printed where the corpus prints them — on an enemy at
/// your location (Mob Enforcer 01101, the Midnight Masks cultists). Parleying
/// with the very enemy that would make the attack provokes nothing.
#[test]
fn a_parley_ability_on_an_enemy_provokes_nothing() {
    assert_eq!(
        damage_after_activating(AbilitySource::Enemy(ATTACKER), ONLY),
        0,
        "**Parley** is on the exempt list",
    );
}
