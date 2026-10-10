//! **Taking an action** (see `GLOSSARY.md`): which actions provoke an attack of
//! opportunity, and which are refused before anything is paid. Driven through
//! the public [`apply`](game_core::engine::apply) API, with the real card corpus
//! installed so the play and activation rows can seat printed cards.
//!
//! `glossary/Attack_of_Opportunity.md`, verbatim:
//!
//! > Each time an investigator is engaged with one or more ready enemies and
//! > takes an action other than to **fight**, to **evade**, or to activate a
//! > **parley** or **resign** ability, each of those enemies makes an attack of
//! > opportunity against the investigator, in the order of the investigator's
//! > choosing.
//! >
//! > - An attack of opportunity is made immediately after all costs of
//! >   initiating the action that provoked the attack have been paid, but before
//! >   the application of that action's effect upon the game state.
//!
//! The table holds one row per way of taking an action. Each row takes the
//! action with one ready enemy engaged and asserts whether that enemy attacks,
//! and that it attacks after the action is paid for and before the action's
//! effect.
//!
//! The registry is `cards::REGISTRY` with one probe card composed in,
//! [`TWO_ACTION_PROBE`]. No printed card in the corpus has a two-action
//! activation, and the rule's *"An ability that costs more than one action only
//! provokes one attack of opportunity from each engaged enemy"* needs one. The
//! probe models that primitive and impersonates no printed card (ADR 0016).

use card_dsl::dsl::{self, Ability, InvestigatorTarget};
use cards::REGISTRY;
use game_core::card_registry::CardRegistry;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    LocationId, Status,
};
use game_core::test_support::{self, TestSession};

/// A probe whose only property is an activated ability costing two actions:
/// *"\[action\]\[action\]: gain 1 resource."* No row seats it yet; #995 adds the
/// two-action activation row that does.
const TWO_ACTION_PROBE: &str = "_aoo_two_action";

/// First Aid (01019), a Guardian asset with no Revelation. It is the one card in
/// the deck, so the Draw row draws something real without the empty-deck
/// horror.
const FIRST_AID: &str = "01019";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(1);
const THERE: LocationId = LocationId(2);
/// Ready and engaged with [`INV`]: the enemy that attacks, or doesn't.
const ATTACKER: EnemyId = EnemyId(7);
/// Ready, at [`HERE`], engaged with nobody: the Engage row's target.
const BYSTANDER: EnemyId = EnemyId(8);

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == TWO_ACTION_PROBE {
        return Some(vec![dsl::activated(
            2,
            Vec::new(),
            dsl::gain_resources(InvestigatorTarget::You, 1),
        )]);
    }
    (REGISTRY.abilities_for)(code)
}

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(CardRegistry {
        abilities_for,
        ..REGISTRY
    });
}

/// [`INV`] at [`HERE`] (connected to [`THERE`]) with 3 actions and First Aid
/// in deck, [`ATTACKER`] engaged with them, and [`BYSTANDER`] beside them, and
/// a chaos bag for the tested actions to start their test against. The
/// attacker deals 1 damage and has the health to survive any fight.
fn board() -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.deck = vec![CardCode::new(FIRST_AID)];
    let mut attacker = test_support::test_enemy(ATTACKER.0, "Attacker");
    attacker.attack_damage = 1;
    attacker.attack_horror = 0;
    attacker.max_health = 5;
    let mut bystander = test_support::test_enemy(BYSTANDER.0, "Bystander");
    bystander.current_location = Some(HERE);
    let mut state = GameStateBuilder::new()
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_location(test_support::test_location(THERE.0, "There"))
        .with_investigator_at(inv, HERE)
        .with_enemy(bystander)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .with_enemy_engaged(attacker, INV)
        .build();
    state.connect(HERE, THERE);
    state
}

/// One way of taking an action: what to take, the event that marks its effect
/// landing, and whether it provokes.
struct Row {
    name: &'static str,
    action: TurnAction,
    effect: fn(&Event) -> bool,
    provokes: bool,
}

fn basic_action_rows() -> Vec<Row> {
    vec![
        Row {
            name: "Draw",
            action: TurnAction::Draw { investigator: INV },
            effect: |e| matches!(e, Event::CardsDrawn { .. }),
            provokes: true,
        },
        Row {
            name: "Resource",
            action: TurnAction::Resource { investigator: INV },
            effect: |e| matches!(e, Event::ResourcesGained { .. }),
            provokes: true,
        },
        Row {
            name: "Move",
            action: TurnAction::Move {
                investigator: INV,
                destination: THERE,
            },
            effect: |e| matches!(e, Event::InvestigatorMoved { .. }),
            provokes: true,
        },
        Row {
            name: "Investigate",
            action: TurnAction::Investigate { investigator: INV },
            effect: |e| matches!(e, Event::SkillTestStarted { .. }),
            provokes: true,
        },
        Row {
            name: "Engage",
            action: TurnAction::Engage {
                investigator: INV,
                enemy: BYSTANDER,
            },
            effect: |e| matches!(e, Event::EnemyEngaged { enemy, .. } if *enemy == BYSTANDER),
            provokes: true,
        },
        Row {
            name: "Fight",
            action: TurnAction::Fight {
                investigator: INV,
                enemy: ATTACKER,
            },
            effect: |e| matches!(e, Event::SkillTestStarted { .. }),
            provokes: false,
        },
        Row {
            name: "Evade",
            action: TurnAction::Evade {
                investigator: INV,
                enemy: ATTACKER,
            },
            effect: |e| matches!(e, Event::SkillTestStarted { .. }),
            provokes: false,
        },
    ]
}

fn position(events: &[Event], name: &str, what: &str, p: impl Fn(&Event) -> bool) -> usize {
    events
        .iter()
        .position(p)
        .unwrap_or_else(|| panic!("{name}: no {what} event in {events:#?}"))
}

/// Every basic action but Fight and Evade is attacked by a ready engaged enemy,
/// after its one action is paid and before its effect lands. Fight and Evade
/// are paid for and take effect with no attack at all.
#[test]
fn a_ready_engaged_enemy_attacks_every_action_but_fight_and_evade() {
    for row in basic_action_rows() {
        let session = TestSession::new(board()).take(&row.action);
        let events = session.events();

        let paid = position(events, row.name, "payment", |e| {
            matches!(
                e,
                Event::ActionsRemainingChanged { investigator, new_count: 2 } if *investigator == INV
            )
        });
        let effect = position(events, row.name, "effect", row.effect);
        assert!(paid < effect, "{}: paid after the effect landed", row.name);

        let attack = events.iter().position(
            |e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == INV),
        );
        if row.provokes {
            let attack = attack
                .unwrap_or_else(|| panic!("{}: provokes, but no attack in {events:#?}", row.name));
            assert!(
                paid < attack && attack < effect,
                "{}: the attack must fall between payment ({paid}) and effect ({effect}), \
                 was {attack}",
                row.name,
            );
        } else {
            assert_eq!(attack, None, "{}: exempt, but was attacked", row.name);
        }
    }
}

/// The step refuses a basic action, with a reason, for an investigator who may
/// not take one. It does so even when the action reaches the handler without
/// passing the turn menu, and before anything is paid.
#[test]
fn a_basic_action_is_refused_for_an_investigator_who_is_not_active_or_out_of_actions() {
    let mut not_active = board();
    not_active.investigators.get_mut(&INV).unwrap().status = Status::Resigned;
    let mut no_actions = board();
    no_actions
        .investigators
        .get_mut(&INV)
        .unwrap()
        .actions_remaining = 0;
    let mut missing = board();
    missing.investigators.remove(&INV);
    let cases = [
        ("not Active", not_active),
        ("out of actions", no_actions),
        ("missing", missing),
    ];
    for (case, state) in cases {
        for row in basic_action_rows() {
            let result = test_support::dispatch_turn_action_unchecked(state.clone(), &row.action);
            match result.outcome {
                EngineOutcome::Rejected { reason } => assert!(
                    !reason.is_empty(),
                    "{} for an investigator {case}: refused without a reason",
                    row.name,
                ),
                other => panic!(
                    "{} for an investigator {case}: expected a refusal, got {other:?}",
                    row.name,
                ),
            }
        }
    }
}
