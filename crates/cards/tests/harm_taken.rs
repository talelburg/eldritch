//! Direct harm, and the "harm taken by you" pattern on the deal's outer
//! condition, at the cases no implemented card can show on its own.
//!
//! **Direct harm.** `glossary/Direct_Damage_Direct_Horror.md`: *"that damage or
//! horror must be assigned directly to the specified card, and cannot be
//! assigned or re-assigned elsewhere."* A direct deal offers no asset the soak,
//! yet it is still a deal: both of its triggering conditions are announced,
//! `DamagePlaced` nested inside `DamageDealt` (ADR 0009).
//!
//! **Nothing taken, nothing fires.** Psychosis 01099 prints *"**Forced** - After
//! you take 1 or more horror: Take 1 direct damage."* A deal whose horror is
//! prevented in the window between assigning and placing places nothing, and
//! `GLOSSARY.md` ("Assign / Place") counts harm as dealt only once it is placed:
//! *"If no damage/horror is applied in this step, no damage/horror has been
//! successfully dealt"*. No implemented card prevents harm, so a synthetic
//! canceller models the primitive (Perseverance 04111's shape at its full
//! extent) beside the real Psychosis.
//!
//! Own integration-test binary so it can build its own `MockRegistry`.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{
    self, Ability, Effect, EventPattern, EventTiming, HarmKind, HarmPlacedScope, HarmTakenScope,
    IntExpr, InvestigatorTarget,
};
use game_core::assert_no_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, GameStateBuilder, InvestigatorId, LocationId,
};
use game_core::test_support::{self, seat, BoardLookup as _, MockRegistry, TestSession};

/// Psychosis 01099, printed: the real card's abilities and metadata.
const PSYCHOSIS: &str = "01099";
/// A threat-area treachery whose `when`-cell reaction to horror the controller
/// would take cancels the deal.
const PREVENT: &str = "_ht_prevent";
/// A threat-area treachery with `[action]: Take 1 direct horror.` and two
/// markers: +4 resources after horror is placed on the controller's
/// investigator, +8 after the controller takes horror.
const DIRECT: &str = "_ht_direct";
/// An asset with 2 sanity and no abilities, which a non-direct deal would
/// offer the horror to.
const SOAKER: &str = "_ht_soaker";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(1);

fn treachery(code: &str) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: "Probe".to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness: None,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

fn soaker() -> CardMetadata {
    CardMetadata {
        code: SOAKER.to_string(),
        name: "Soaker Probe".to_string(),
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
            sanity: Some(2),
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

/// Horror the controller takes, in `timing`'s cell.
fn on_horror_taken(timing: EventTiming, effect: Effect) -> Ability {
    dsl::reaction_on_event(
        EventPattern::HarmTaken {
            kind: HarmKind::Horror,
            scope: HarmTakenScope::You,
        },
        timing,
        effect,
    )
}

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(
                cards::by_code(PSYCHOSIS)
                    .expect("Psychosis in corpus")
                    .clone(),
            )
            .with_abilities(PSYCHOSIS, cards::impls::psychosis::abilities)
            .with_card(treachery(PREVENT))
            .with_abilities(PREVENT, || {
                vec![on_horror_taken(EventTiming::When, Effect::Cancel)]
            })
            .with_card(treachery(DIRECT))
            .with_abilities(DIRECT, || {
                vec![
                    dsl::activated(
                        1,
                        vec![],
                        dsl::deal_direct_horror(InvestigatorTarget::You, IntExpr::Lit(1)),
                    ),
                    dsl::reaction_on_event(
                        EventPattern::HarmPlaced {
                            kind: HarmKind::Horror,
                            scope: HarmPlacedScope::YourInvestigator,
                        },
                        EventTiming::After,
                        dsl::gain_resources(InvestigatorTarget::You, 4),
                    ),
                    on_horror_taken(
                        EventTiming::After,
                        dsl::gain_resources(InvestigatorTarget::You, 8),
                    ),
                ]
            })
            .with_card(soaker())
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The investigator's open turn at `HERE`, with `threat` in their threat area
/// and the soaker in play.
fn board(threat: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .resources(0)
                .in_play([SOAKER])
                .threat_area(threat.iter().copied()),
        )
        .open_turn(INV)
        .session()
}

/// Every marker and every horror placement, in emission order: `Some(amount)`
/// for a marker's resource gain, `None` for horror landing on the investigator.
fn timeline(events: &[Event]) -> Vec<Option<u8>> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } => Some(Some(*amount)),
            Event::HorrorPlacedOnInvestigator { .. } => Some(None),
            _ => None,
        })
        .collect()
}

/// A direct deal offers no soak prompt, though an asset could take the horror,
/// and still announces both conditions: the placement's `after` (+4), then the
/// whole deal's (+8).
#[test]
fn a_direct_deal_offers_no_soak_and_still_walks_both_conditions() {
    let session = board(&[DIRECT]);
    let source = AbilitySource::InPlay(session.state().in_play(INV, DIRECT));
    let session = session.take(&TurnAction::ActivateAbility {
        investigator: INV,
        source,
        address: AbilityAddress::Printed(0),
    });
    // The first prompt is the placement's reaction, on the probe: no soak
    // prompt came before it.
    let card = session.state().card(INV, DIRECT);
    assert_eq!(session.offered_at(&card), 1, "the HarmPlaced marker");
    let session = session.pick(card.clone());
    assert_eq!(session.offered_at(&card), 1, "the HarmTaken marker");
    let session = session.pick(card);

    session.assert_at_turn_menu(INV);
    assert_eq!(
        timeline(session.events()),
        vec![None, Some(4), Some(8)],
        "events = {:?}",
        session.events()
    );
    let state = session.state();
    let inv = &state.investigators[&INV];
    assert_eq!(inv.horror(), 1);
    let soaker = state.in_play(INV, SOAKER);
    let soaker = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == soaker)
        .expect("the soaker is in play");
    assert_eq!(soaker.accumulated_horror, 0);
}

/// Horror cancelled in the deal's `when` cell is never taken, so Psychosis's
/// *"After you take 1 or more horror"* does not fire.
#[test]
fn prevented_horror_does_not_fire_psychosis() {
    let session = board(&[PSYCHOSIS, PREVENT]).take_horror(INV, 1);
    // Assigned to the investigator card, then cancelled in the window.
    let me = session.state().investigators[&INV].card_anchor();
    let session = session.pick(me);
    let canceller = session.state().card(INV, PREVENT);
    let session = session.pick(canceller);

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 0, "the horror was prevented");
    assert_eq!(inv.damage(), 0, "Psychosis did not fire");
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
    assert_no_event!(session.events(), Event::DamagePlacedOnInvestigator { .. });
}
