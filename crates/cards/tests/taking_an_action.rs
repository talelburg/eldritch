//! **Taking an action** (see `GLOSSARY.md`): which actions provoke an attack of
//! opportunity, and which are refused before anything is paid. The attack rows
//! take each action from the turn menu through
//! [`TestSession`](game_core::test_support::TestSession); the refusal test
//! submits it past the menu with
//! [`dispatch_turn_action_unchecked`](game_core::test_support::dispatch_turn_action_unchecked),
//! as a client that never read the menu would. The real card corpus is
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
//!
//! The Parley exemption, which no implemented card prints, is
//! `crates/game-core/tests/action_designator_aoo.rs`. A designated Evade or
//! Move ability is refused before it is taken (`engine::designator`), so it has
//! no row yet.

use card_dsl::dsl::{self, Ability, InvestigatorTarget};
use cards::REGISTRY;
use game_core::card_registry::CardRegistry;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId, Owner, Status, UseKind,
};
use game_core::test_support::{self, TestSession};

/// A probe whose only property is an activated ability costing two actions:
/// *"\[action\]\[action\]: gain 1 resource."*
const TWO_ACTION_PROBE: &str = "_aoo_two_action";

/// First Aid (01019), a Guardian asset with no Revelation. It is the one card in
/// the deck, so the Draw row draws something real without the empty-deck
/// horror, and its action ability is the activation with no designator.
const FIRST_AID: &str = "01019";
/// Emergency Cache (01088), a non-fast event costing 0.
const EMERGENCY_CACHE: &str = "01088";
/// Magnifying Glass (01030), a fast asset.
const MAGNIFYING_GLASS: &str = "01030";
/// Machete (01020), whose action ability is a designated Fight.
const MACHETE: &str = "01020";
/// Flashlight (01087), whose action ability is a designated Investigate.
const FLASHLIGHT: &str = "01087";
/// Parlor (01115), whose action ability is a Resign.
const PARLOR: &str = "01115";
/// Beat Cop (01018), whose ability 1 is a fast ability.
const BEAT_COP: &str = "01018";

/// The in-play instances the activation rows seat, numbered clear of any the
/// engine mints.
const KIT: CardInstanceId = CardInstanceId(901);
const BLADE: CardInstanceId = CardInstanceId(902);
const TORCH: CardInstanceId = CardInstanceId(903);
const PROBE: CardInstanceId = CardInstanceId(904);
const COP: CardInstanceId = CardInstanceId(905);

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

/// One way of taking an action: the board it needs beyond [`board`], what to
/// take, the event that marks its effect landing, how many actions it spends,
/// and whether it provokes.
struct Row {
    name: &'static str,
    setup: fn(&mut GameState),
    action: TurnAction,
    effect: fn(&Event) -> bool,
    actions_spent: u8,
    provokes: bool,
}

impl Row {
    /// A basic action: one action, on the plain [`board`].
    fn basic(
        name: &'static str,
        action: TurnAction,
        effect: fn(&Event) -> bool,
        provokes: bool,
    ) -> Self {
        Self {
            name,
            setup: |_| {},
            action,
            effect,
            actions_spent: 1,
            provokes,
        }
    }
}

fn basic_action_rows() -> Vec<Row> {
    vec![
        Row::basic(
            "Draw",
            TurnAction::Draw { investigator: INV },
            |e| matches!(e, Event::CardsDrawn { .. }),
            true,
        ),
        Row::basic(
            "Resource",
            TurnAction::Resource { investigator: INV },
            |e| matches!(e, Event::ResourcesGained { .. }),
            true,
        ),
        Row::basic(
            "Move",
            TurnAction::Move {
                investigator: INV,
                destination: THERE,
            },
            |e| matches!(e, Event::InvestigatorMoved { .. }),
            true,
        ),
        Row::basic(
            "Investigate",
            TurnAction::Investigate { investigator: INV },
            |e| matches!(e, Event::SkillTestStarted { .. }),
            true,
        ),
        Row::basic(
            "Engage",
            TurnAction::Engage {
                investigator: INV,
                enemy: BYSTANDER,
            },
            |e| matches!(e, Event::EnemyEngaged { enemy, .. } if *enemy == BYSTANDER),
            true,
        ),
        Row::basic(
            "Fight",
            TurnAction::Fight {
                investigator: INV,
                enemy: ATTACKER,
            },
            |e| matches!(e, Event::SkillTestStarted { .. }),
            false,
        ),
        Row::basic(
            "Evade",
            TurnAction::Evade {
                investigator: INV,
                enemy: ATTACKER,
            },
            |e| matches!(e, Event::SkillTestStarted { .. }),
            false,
        ),
    ]
}

/// Put `code` into [`INV`]'s play area as `instance`, with `supplies` supply
/// uses if it has any.
fn seat_in_play(state: &mut GameState, code: &str, instance: CardInstanceId, supplies: u8) {
    let mut card = CardInPlay::enter_play(CardCode::new(code), instance, Owner::Investigator(INV));
    if supplies > 0 {
        card.uses.insert(UseKind::Supplies, supplies);
    }
    state
        .investigators
        .get_mut(&INV)
        .unwrap()
        .cards_in_play
        .push(card);
}

/// Put `code` alone into [`INV`]'s hand, at index 0.
fn seat_in_hand(state: &mut GameState, code: &str) {
    state.investigators.get_mut(&INV).unwrap().hand = vec![CardCode::new(code)];
}

/// The activation for ability `index` of the card `instance` in [`INV`]'s play area.
fn activate(instance: CardInstanceId, index: u8) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(instance),
        address: AbilityAddress::Printed(index),
    }
}

/// Playing a card and activating an ability, each taken through the same step
/// as the basic actions. A fast play and a fast ability spend no action, so
/// they take no action at all and provoke nothing.
fn play_and_activate_rows() -> Vec<Row> {
    vec![
        // Emergency Cache 01088: *"Gain 3 resources."* A non-fast event.
        Row {
            name: "Play (non-fast event)",
            setup: |state| seat_in_hand(state, EMERGENCY_CACHE),
            action: TurnAction::PlayCard {
                investigator: INV,
                hand_index: 0,
            },
            effect: |e| matches!(e, Event::ResourcesGained { amount: 3, .. }),
            actions_spent: 1,
            provokes: true,
        },
        // Magnifying Glass 01030: *"Fast."* Played without the Play action.
        Row {
            name: "Play (fast asset)",
            setup: |state| seat_in_hand(state, MAGNIFYING_GLASS),
            action: TurnAction::PlayCard {
                investigator: INV,
                hand_index: 0,
            },
            effect: |e| matches!(e, Event::CardPlayed { .. }),
            actions_spent: 0,
            provokes: false,
        },
        // First Aid 01019: *"[action] Spend 1 supply: Heal 1 damage or horror
        // from an investigator at your location."* No designator. The
        // investigator carries 1 damage so the heal can change the game state.
        Row {
            name: "Activate (no designator)",
            setup: |state| {
                seat_in_play(state, FIRST_AID, KIT, 3);
                state
                    .investigators
                    .get_mut(&INV)
                    .unwrap()
                    .investigator_card
                    .accumulated_damage = 1;
            },
            action: activate(KIT, 0),
            effect: |e| matches!(e, Event::Healed { .. }),
            actions_spent: 1,
            provokes: true,
        },
        // Machete 01020: *"[action]: <b>Fight.</b> You get +1 [combat] for this
        // attack."* A designated Fight is exempt. The bystander is removed so
        // the attacker is the one enemy here to fight.
        Row {
            name: "Activate (designated Fight)",
            setup: |state| {
                seat_in_play(state, MACHETE, BLADE, 0);
                state.enemies.remove(&BYSTANDER);
            },
            action: activate(BLADE, 0),
            effect: |e| matches!(e, Event::SkillTestStarted { .. }),
            actions_spent: 1,
            provokes: false,
        },
        // Flashlight 01087: *"[action] Spend 1 supply: <b>Investigate.</b>"*
        // Investigate is not on the exempt list, so it provokes as the basic
        // action does.
        Row {
            name: "Activate (designated Investigate)",
            setup: |state| seat_in_play(state, FLASHLIGHT, TORCH, 3),
            action: activate(TORCH, 0),
            effect: |e| matches!(e, Event::SkillTestStarted { .. }),
            actions_spent: 1,
            provokes: true,
        },
        // Parlor 01115: *"[action] <b>Resign.</b>"* A location's ability,
        // reached by standing in it. Resign is exempt.
        Row {
            name: "Activate (Resign)",
            setup: |state| {
                let here = state.locations.get_mut(&HERE).unwrap();
                here.code = CardCode::new(PARLOR);
                here.revealed = true;
            },
            action: TurnAction::ActivateAbility {
                investigator: INV,
                source: AbilitySource::Location(HERE),
                address: AbilityAddress::Printed(0),
            },
            effect: |e| matches!(e, Event::InvestigatorEliminated { .. }),
            actions_spent: 1,
            provokes: false,
        },
        // The probe's *"[action][action]: gain 1 resource."* Two actions, and
        // still one attack from the one engaged enemy.
        Row {
            name: "Activate (two actions)",
            setup: |state| seat_in_play(state, TWO_ACTION_PROBE, PROBE, 0),
            action: activate(PROBE, 0),
            effect: |e| matches!(e, Event::ResourcesGained { amount: 1, .. }),
            actions_spent: 2,
            provokes: true,
        },
        // Beat Cop 01018: *"[fast] Discard Beat Cop: Deal 1 damage to an enemy
        // at your location."* The bystander is removed so the attacker is the
        // one enemy here to choose.
        Row {
            name: "Activate (fast)",
            setup: |state| {
                seat_in_play(state, BEAT_COP, COP, 0);
                state.enemies.remove(&BYSTANDER);
            },
            action: activate(COP, 1),
            effect: |e| matches!(e, Event::EnemyDamaged { enemy, .. } if *enemy == ATTACKER),
            actions_spent: 0,
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

/// Every action but Fight, Evade, a designated Fight and a Resign is attacked
/// by a ready engaged enemy, once, after its actions are paid and before its
/// effect lands. The exempt actions are paid for and take effect with no attack
/// at all, and a fast play or fast ability spends nothing and is not attacked.
#[test]
fn a_ready_engaged_enemy_attacks_every_action_but_the_exempt_ones() {
    for row in basic_action_rows()
        .into_iter()
        .chain(play_and_activate_rows())
    {
        let mut state = board();
        (row.setup)(&mut state);
        let session = TestSession::new(state).take(&row.action);
        let events = session.events();

        let effect = position(events, row.name, "effect", row.effect);
        let attacks: Vec<usize> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == INV))
            .map(|(i, _)| i)
            .collect();
        let payment = |e: &Event| matches!(e, Event::ActionsRemainingChanged { investigator, .. } if *investigator == INV);

        if row.actions_spent == 0 {
            assert!(
                !events.iter().any(payment),
                "{}: spends no action, but paid one in {events:#?}",
                row.name,
            );
            assert_eq!(
                attacks,
                Vec::<usize>::new(),
                "{}: fast, but was attacked",
                row.name
            );
            continue;
        }

        let paid = position(events, row.name, "payment", payment);
        assert!(
            matches!(
                events[paid],
                Event::ActionsRemainingChanged { new_count, .. } if new_count == 3 - row.actions_spent
            ),
            "{}: should spend {} action(s), paid {:?}",
            row.name,
            row.actions_spent,
            events[paid],
        );
        assert!(paid < effect, "{}: paid after the effect landed", row.name);

        if row.provokes {
            let [attack] = attacks[..] else {
                panic!(
                    "{}: provokes one attack, got {attacks:?} in {events:#?}",
                    row.name
                );
            };
            assert!(
                paid < attack && attack < effect,
                "{}: the attack must fall between payment ({paid}) and effect ({effect}), \
                 was {attack}",
                row.name,
            );
        } else {
            assert_eq!(
                attacks,
                Vec::<usize>::new(),
                "{}: exempt, but was attacked",
                row.name
            );
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
