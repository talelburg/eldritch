//! The **grant sweep** (#772): a card's abilities are what it prints *plus*
//! what the board grants it, and a granted ability is addressed by where it is
//! printed.
//!
//! `data/rules-reference/rules/glossary/Gains.md`, verbatim:
//!
//! > If a card gains a characteristic (such as an icon, a trait, a keyword, or
//! > ability text), the card functions as if it possesses the gained
//! > characteristic.
//!
//! > "Gained" characteristics are not considered to be "printed" on the card.
//! > If an ability refers to the printed characteristics of a card, it does not
//! > refer to gained characteristics.
//!
//! Both halves are load-bearing here. The first is why the recipient is the
//! source an activation names — the ability *is* hers. The second is why the
//! sweep reads printed abilities only, and why an
//! [`AbilityAddress::Granted`](game_core::state::AbilityAddress) names the
//! *granter's* clause rather than a position in the recipient's merged list.
//! `docs/adr/0014-a-granted-ability-is-a-constant-effect-swept-off-the-board.md`
//! has the argument.
//!
//! Own integration-test binary so it can install a hand-rolled `CardRegistry`.
//! The corpus peer — the Parlor 01115 granting Lita Chantler 01117 her Parley,
//! end to end — is `lita_parley.rs`; these cases are the ones no printed card
//! reaches (a granted grant, a grant conditioned on a "you" the recipient has
//! not got, a granter that leaves play mid-window).

use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{
    activated, constant, control_status, gain_resources, grant, Ability, CmpOp, Condition,
    ControlStatus, GrantTarget, InvestigatorTarget, Quantity,
};
use game_core::engine::{legal_actions, EngineOutcome, TurnAction};
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, GameState, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, metadata_for_test_inv, test_investigator, test_location,
    GameStateBuilder,
};

/// The **granter**: a location whose printed text grants an ability to
/// [`RECIPIENT`] while nobody controls it — the Parlor 01115's shape.
const GRANTER: &str = "GRANTLOC";
/// The **recipient**: an asset put into play at [`HERE`] under nobody's
/// control, printing nothing of its own — Lita Chantler 01117's shape before
/// #773 gives her a card module.
const RECIPIENT: &str = "GRANTREC";
/// A second recipient, used only to prove that a **granted grant** does not
/// apply.
const SECOND_HAND: &str = "GRANTSND";
/// A granter whose grant is gated on a condition that needs a "you" — the case
/// the sweep answers `false` for an uncontrolled recipient.
const NEEDS_YOU: &str = "GRANTYOU";

const MINE: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(1);
const THERE: LocationId = LocationId(2);
const RECIPIENT_INST: CardInstanceId = CardInstanceId(50);
const SECOND_INST: CardInstanceId = CardInstanceId(51);

/// The one ability every grant in this file hands over: a plain one-action
/// activation that always changes state, so nothing but the grant itself can
/// decide whether it is offered.
fn granted_activation() -> Ability {
    activated(1, vec![], gain_resources(InvestigatorTarget::Active, 1))
}

fn probe_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        // "While RECIPIENT is not controlled by a player, it gains: <activation>."
        GRANTER => Some(vec![
            constant(grant(
                GrantTarget::Card(RECIPIENT.to_owned()),
                Some(control_status(RECIPIENT, ControlStatus::ByNoPlayer)),
                vec![
                    granted_activation(),
                    // A **granted grant**. The sweep reads printed abilities only,
                    // so this one never reaches SECOND_HAND.
                    constant(grant(
                        GrantTarget::Card(SECOND_HAND.to_owned()),
                        None,
                        vec![granted_activation()],
                    )),
                ],
            )),
            // Ability 1: a `Grant` sitting under an *activated* trigger, so the
            // "inspected, never executed" contract can be driven from the menu.
            activated(
                1,
                vec![],
                grant(GrantTarget::SelfCard, None, vec![granted_activation()]),
            ),
        ]),
        // "While you have a clue at your location, RECIPIENT gains: …" — a
        // condition that needs a "you", which an uncontrolled recipient has not
        // got.
        NEEDS_YOU => Some(vec![constant(grant(
            GrantTarget::Card(RECIPIENT.to_owned()),
            Some(Condition::Compare {
                quantity: Quantity::CluesAtControllerLocation,
                op: CmpOp::Gt,
                value: 0,
            }),
            vec![granted_activation()],
        ))]),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install_probe_registry() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: metadata_for_test_inv,
        abilities_for: probe_abilities,
        ..CardRegistry::EMPTY
    });
}

/// The board: the investigator standing at [`HERE`], whose location card is
/// `granter_code`, with the recipient and a second card put into play there
/// under nobody's control.
fn board(granter_code: &str) -> GameState {
    let mut here = test_location(1, "Granting Place");
    here.code = CardCode::new(granter_code);
    here.clues = 0;
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(test_investigator(1), HERE)
        .with_location(here)
        .with_location(test_location(2, "Elsewhere"))
        .with_active_investigator(MINE)
        .with_turn_order([MINE])
        .with_investigator_turn(MINE)
        .build();
    let at_location = &mut state
        .locations
        .get_mut(&HERE)
        .expect("HERE is on the board")
        .cards_at_location;
    at_location.push(CardInPlay::enter_play(
        CardCode::new(RECIPIENT),
        RECIPIENT_INST,
    ));
    at_location.push(CardInPlay::enter_play(
        CardCode::new(SECOND_HAND),
        SECOND_INST,
    ));
    state
}

/// The address the grant lands at: the granter's ability 0, sub-ability 0.
fn granted_address() -> AbilityAddress {
    AbilityAddress::Granted {
        granter: CardCode::new(GRANTER),
        ability: 0,
        sub: 0,
    }
}

fn activation(instance: CardInstanceId, address: AbilityAddress) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: MINE,
        source: AbilitySource::InPlay(instance),
        address,
    }
}

// ---- the sweep -------------------------------------------------------

/// The recipient prints nothing at all, and is offered the granted activation
/// anyway. *"the card functions as if it possesses the gained characteristic"*
/// — so the ability is hers, and the source the menu names is her instance.
#[test]
fn a_granted_ability_is_offered_on_its_recipient() {
    let state = board(GRANTER);
    assert!(
        legal_actions(&state).contains(&activation(RECIPIENT_INST, granted_address())),
        "the granted activation is offered on the recipient, got {:?}",
        legal_actions(&state),
    );
}

/// The same board with the condition flipped: a player controls the recipient,
/// so *"while … not controlled by a player"* no longer holds and the grant
/// lapses. The recipient is still in play and still reachable — only the grant
/// is gone.
#[test]
fn a_grant_vanishes_when_its_condition_stops_holding() {
    let mut state = board(GRANTER);
    // Move the recipient into the investigator's play area: now a player
    // controls her, and the grant's condition is false.
    let card = state
        .locations
        .get_mut(&HERE)
        .expect("HERE is on the board")
        .cards_at_location
        .remove(0);
    state
        .investigators
        .get_mut(&MINE)
        .expect("investigator present")
        .cards_in_play
        .push(card);

    assert!(
        !legal_actions(&state).contains(&activation(RECIPIENT_INST, granted_address())),
        "the grant lapses once a player controls the recipient, got {:?}",
        legal_actions(&state),
    );
}

/// **A grant cannot be granted.** The sweep calls the *printed* lookup, not the
/// merged one, so the `Effect::Grant` sitting inside the granter's own granted
/// list never reaches the board. That is what leaves the addressing with a
/// vector that is a pure function of `(code, side)` — and the whole reason
/// there is no fixed point to iterate to.
#[test]
fn a_granted_grant_does_not_apply() {
    let state = board(GRANTER);
    let offered = legal_actions(&state);
    assert!(
        !offered.iter().any(|action| matches!(
            action,
            TurnAction::ActivateAbility {
                source: AbilitySource::InPlay(inst),
                ..
            } if *inst == SECOND_INST
        )),
        "the second-hand recipient is granted nothing, got {offered:?}",
    );
}

/// **Who "you" is for a source with no controller.** The sweep evaluates a
/// grant's condition against the *recipient's* controller, and a card put into
/// play at a location has none. A condition that needs a "you" therefore does
/// not hold — it is not silently treated as true, and it does not borrow some
/// other investigator's seat.
#[test]
fn a_condition_needing_a_you_does_not_hold_for_an_uncontrolled_recipient() {
    let mut state = board(NEEDS_YOU);
    // Put a clue on the location, so the condition *would* hold for anybody who
    // had a "you" there.
    state
        .locations
        .get_mut(&HERE)
        .expect("HERE is on the board")
        .clues = 3;

    let address = AbilityAddress::Granted {
        granter: CardCode::new(NEEDS_YOU),
        ability: 0,
        sub: 0,
    };
    assert!(
        !legal_actions(&state).contains(&activation(RECIPIENT_INST, address)),
        "a condition needing a controller does not hold on an uncontrolled recipient, got {:?}",
        legal_actions(&state),
    );
}

// ---- addressing across a suspension ----------------------------------

/// A candidate minted while the grant applied and fired after the granter left
/// play resolves to nothing, and the activation is **rejected** rather than
/// firing whatever now sits at that position. The granter here is the location
/// itself, so "left play" is the investigator walking away from it.
#[test]
fn a_granted_address_resolves_to_nothing_once_the_granter_is_gone() {
    let mut state = board(GRANTER);
    let minted = activation(RECIPIENT_INST, granted_address());
    assert!(
        legal_actions(&state).contains(&minted),
        "precondition: the activation is live before the granter goes",
    );

    // The granting location stops granting: its printed text is replaced by a
    // code the probe registry implements nothing for.
    state
        .locations
        .get_mut(&HERE)
        .expect("HERE is on the board")
        .code = CardCode::new("GRANTNIL");

    let result = dispatch_turn_action_unchecked(state, &minted);
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "a stale granted address must reject, got {:?}",
        result.outcome,
    );
}

/// The same shape with the *condition* flipping rather than the granter
/// leaving: a candidate minted against an uncontrolled recipient lapses once
/// somebody controls her, rather than firing the wrong ability. This is the
/// hazard `AbilityAddress` exists for — with a merged index, position 0 on the
/// recipient would name one ability at scan time and another at resolve time.
#[test]
fn a_granted_address_lapses_when_the_condition_flips_mid_window() {
    let mut state = board(GRANTER);
    let minted = activation(RECIPIENT_INST, granted_address());
    assert!(
        legal_actions(&state).contains(&minted),
        "precondition: the activation is live while nobody controls the recipient",
    );

    let card = state
        .locations
        .get_mut(&HERE)
        .expect("HERE is on the board")
        .cards_at_location
        .remove(0);
    state
        .investigators
        .get_mut(&MINE)
        .expect("investigator present")
        .cards_in_play
        .push(card);

    let result = dispatch_turn_action_unchecked(state, &minted);
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "an address whose grant no longer applies must reject, got {:?}",
        result.outcome,
    );
}

/// **A grant reaches only the card it names.** The granter grants to
/// `RECIPIENT` by code, and an investigator standing somewhere else is offered
/// nothing — the recipient is reachable through the co-location bullet, and
/// that bullet is what the grant rides in on.
#[test]
fn a_grant_is_not_offered_to_an_investigator_elsewhere() {
    let mut state = board(GRANTER);
    state
        .investigators
        .get_mut(&MINE)
        .expect("investigator present")
        .current_location = Some(THERE);
    assert!(
        !legal_actions(&state).contains(&activation(RECIPIENT_INST, granted_address())),
        "an investigator elsewhere cannot reach the recipient, got {:?}",
        legal_actions(&state),
    );
}

/// The engine's own `Effect` contract: a `Grant` is **inspected, never
/// executed**. Resolving one as an effect is a misuse and rejects loudly,
/// exactly as `Effect::Restrict` does — so a card that puts one under a
/// triggered trigger fails at resolution rather than silently doing nothing.
#[test]
fn resolving_a_grant_as_an_effect_rejects() {
    let state = board(GRANTER);
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: MINE,
            source: AbilitySource::Location(HERE),
            address: AbilityAddress::Printed(1),
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "a Grant is a constant marker; running it is a misuse, got {:?}",
        result.outcome,
    );
}
