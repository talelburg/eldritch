//! Dealing harm is **one** outer triggering condition, `DamageDealt`,
//! whose resolve step is the whole `DamagePlaced` sequence (ADR 0009).
//!
//! `data/rules-reference/rules/glossary/When.md`, verbatim: *"immediately after
//! the specified timing point or triggering condition initiates, but before its
//! impact upon the game state resolves"*. `glossary/After.md`: *"the moment
//! immediately after the specified timing point or triggering condition has
//! fully resolved"*. Dealing harm has fully resolved only once it is placed, so
//! the placement's own cells sit inside the deal's:
//!
//! ```text
//! DamageDealt.when → DamagePlaced.when → placement → DamagePlaced.after → DamageDealt.after
//! ```
//!
//! The corpus halves live elsewhere and must not move: Guard Dog 01021's
//! `DamageDealt` `when` cell in `guard_dog_soak.rs`, and Agnes Baker 01004's
//! `DamagePlaced` `after` cell in `agnes_baker.rs`. This file mirrors
//! `enemy_attack_cells.rs`: each synthetic ability gains a distinct number of
//! resources, and `HorrorTaken` marks the placement.
//!
//! The `DamageDealt` probes declare [`EventPattern::EnemyAttackDamagedSelf`] —
//! Guard Dog's — on a soaking asset, and the attack assigns its damage there.
//! `HarmTaken` (Psychosis 01099's *"After you take"*) listens to the same
//! condition; `harm_taken.rs` covers it.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{
    self, Ability, Effect, EventPattern, EventTiming, HarmKind, HarmPlacedScope, InvestigatorTarget,
};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{CardCode, EnemyId, GameStateBuilder, InvestigatorId, LocationId, Phase};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};

/// A soaking asset (health 2, no sanity) with a `DamageDealt` reaction in the
/// `when` cell (+1) and one in the `after` cell (+8).
const SOAKER: &str = "_dd_soaker";
/// The same asset, but its `when`-cell reaction cancels the deal — Perseverance
/// 04111's shape (*"Cancel up to 4 of that damage and/or horror"*) at its full
/// extent. It keeps the +8 `after`-cell reaction, which must not fire.
const CANCELLING_SOAKER: &str = "_dd_cancelling_soaker";
/// A threat-area card with `DamagePlaced` reactions: *"1 or more horror is
/// placed on"* the controller, in the `when` cell (+2) and the `after` cell
/// (+4).
const PLACED: &str = "_dd_placed";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ATTACKER: EnemyId = EnemyId(7);

/// A reaction on [`EventPattern::EnemyAttackDamagedSelf`] — the one pattern on
/// the `DamageDealt` condition today.
fn on_dealt(timing: EventTiming, effect: Effect) -> Ability {
    dsl::reaction_on_event(EventPattern::EnemyAttackDamagedSelf, timing, effect)
}

/// A reaction on 1 or more horror placed on the controller's investigator.
fn on_placed(timing: EventTiming, amount: u8) -> Ability {
    dsl::reaction_on_event(
        EventPattern::HarmPlaced {
            kind: HarmKind::Horror,
            scope: HarmPlacedScope::YourInvestigator,
        },
        timing,
        dsl::gain_resources(InvestigatorTarget::You, amount),
    )
}

/// Asset metadata with 2 health and no sanity, so the attack's damage point is
/// offered to it and its horror point is not.
fn soaker_metadata(code: &str) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
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
            health: Some(2),
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(soaker_metadata(SOAKER))
            .with_card(soaker_metadata(CANCELLING_SOAKER))
            .with_abilities(SOAKER, || {
                vec![
                    on_dealt(
                        EventTiming::When,
                        dsl::gain_resources(InvestigatorTarget::You, 1),
                    ),
                    on_dealt(
                        EventTiming::After,
                        dsl::gain_resources(InvestigatorTarget::You, 8),
                    ),
                ]
            })
            .with_abilities(CANCELLING_SOAKER, || {
                vec![
                    on_dealt(EventTiming::When, Effect::Cancel),
                    on_dealt(
                        EventTiming::After,
                        dsl::gain_resources(InvestigatorTarget::You, 8),
                    ),
                ]
            })
            .with_abilities(PLACED, || {
                vec![
                    on_placed(EventTiming::When, 2),
                    on_placed(EventTiming::After, 4),
                ]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// Investigation-phase board: the active investigator at `HERE` with `asset` in
/// play and [`PLACED`] in their threat area, engaged by one ready attacker
/// dealing 1 damage and 1 horror. `EndTurn` cascades into the Enemy phase.
fn board_with(asset: &str) -> GameStateBuilder {
    let mut attacker = test_support::test_enemy(ATTACKER.0, "Attacker");
    attacker.code = CardCode::new("_dd_attacker");
    attacker.attack_damage = 1;
    attacker.attack_horror = 1;
    GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .resources(0)
                // Keeps Upkeep's draw off an empty deck, whose penalty horror
                // would read as a placement.
                .deck(["_dd_filler"])
                .in_play([asset])
                .threat_area([PLACED]),
        )
        .with_enemy_engaged(attacker, INV)
        .open_turn(INV)
}

/// End the turn, assign the attack's damage point to `asset`, then fire the
/// reaction on each of `fired` in order as its cell offers it — picked by the
/// card it is printed on, so a cell that never opens has nothing to pick and
/// panics.
fn end_turn_firing(asset: &str, fired: &[&str]) -> TestSession {
    let session = board_with(asset).session().take(&TurnAction::EndTurn);
    let soaker = session.state().card(INV, asset);
    assert!(
        matches!(soaker, OptionTarget::CardInstance(_)),
        "the asset anchors the distribution prompt"
    );
    fired.iter().fold(session.pick(soaker), |session, code| {
        let card = session.state().card(INV, *code);
        session.pick(card)
    })
}

/// Every marker and every placement, in emission order — `Some(amount)` for a
/// marker's resource gain, `None` for the horror landing on the investigator.
/// Truncated at the end of the Enemy phase, before Upkeep's resource gain.
fn timeline(events: &[Event]) -> Vec<Option<u8>> {
    events
        .iter()
        .take_while(|e| {
            !matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Enemy
                }
            )
        })
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } => Some(Some(*amount)),
            Event::HorrorTaken { .. } => Some(None),
            _ => None,
        })
        .collect()
}

/// The headline claim: the placement's whole sequence nests inside the deal's
/// resolve step, so *"after … deals"* waits for *"after … is placed"*.
#[test]
fn the_placement_sequence_nests_inside_the_deal() {
    let r = end_turn_firing(SOAKER, &[SOAKER, PLACED, PLACED, SOAKER]);
    assert_eq!(
        timeline(r.events()),
        vec![Some(1), Some(2), None, Some(4), Some(8)],
        "expected DamageDealt.when → DamagePlaced.when → placement → \
         DamagePlaced.after → DamageDealt.after; events = {:?}",
        r.events()
    );
    assert_eq!(r.state().investigators[&INV].horror(), 1);
    let soaker = r.state().in_play(INV, SOAKER);
    let accumulated = r.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == soaker)
        .expect("the soaker survives 1 of its 2 health")
        .accumulated_damage;
    assert_eq!(accumulated, 1, "the damage point landed on the soaker");
}

/// A cancel in `DamageDealt`'s `when` cell abandons its resolve step, and with
/// it the placement: nothing lands, and neither `after` cell runs.
#[test]
fn a_cancel_in_the_dealt_when_cell_stops_the_placement() {
    // One reaction prompt only: the cancel. No later cell offers anything.
    let r = end_turn_firing(CANCELLING_SOAKER, &[CANCELLING_SOAKER]);
    r.assert_at_encounter_draw();
    assert_eq!(
        timeline(r.events()),
        Vec::<Option<u8>>::new(),
        "neither the placement nor any later cell may run; events = {:?}",
        r.events()
    );
    let inv = &r.state().investigators[&INV];
    assert_eq!(inv.damage(), 0);
    assert_eq!(inv.horror(), 0);
    let soaker = r.state().in_play(INV, CANCELLING_SOAKER);
    let accumulated = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == soaker)
        .expect("the soaker is still in play")
        .accumulated_damage;
    assert_eq!(accumulated, 0, "nothing placed on the soaker either");
}
