//! The board stagers — [`enemy`] and [`location`] beside [`seat`], and the
//! act, agenda and encounter setters on the builder — observed only through
//! the built state.
//!
//! Synthetic probes only (ADR 0016), local to this binary. `_stage_enemy`
//! models "an enemy whose health is printed per investigator"; `_stage_loc`
//! models "a location with a printed shroud and a per-investigator clue
//! value"; `_stage_spawn` models "an enemy encounter card that spawns at the
//! drawer's location"; `_stage_inv` models "an investigator card";
//! `_stage_act`, `_stage_objective_act`, `_stage_agenda` and
//! `_stage_late_agenda` model "an act / agenda with a printed threshold" (the
//! objective act prints none). None stands in for a printed card.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, ClueValue, HealthValue, Prey};
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    CardCode, EnemyId, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Owner, Phase, SkillKind,
};
use game_core::test_support::{
    self, act, agenda, clues, enemy, location, seat, AnyKind, BoardLookup as _, MockRegistry, Seat,
    StagedAgenda, TestSession,
};

/// An enemy printed 4 fight, 3 evade, 3 health per investigator, 2 damage and
/// 1 horror, Hunter.
const PROBE_ENEMY: &str = "_stage_enemy";
/// A location printed shroud 4 and 2 clues per investigator.
const PROBE_LOC: &str = "_stage_loc";
/// An investigator printed with stock stats.
const PROBE_INV: &str = "_stage_inv";
/// An enemy encounter card with no spawn instruction and no Revelation.
const SPAWNER: &str = "_stage_spawn";
/// An act printed with a clue threshold of 2.
const PROBE_ACT: &str = "_stage_act";
/// An act that advances on an objective: it prints no clue threshold.
const PROBE_OBJECTIVE_ACT: &str = "_stage_objective_act";
/// An agenda printed with a doom threshold of 4.
const PROBE_AGENDA: &str = "_stage_agenda";
/// An agenda printed with a doom threshold of 7.
const PROBE_LATE_AGENDA: &str = "_stage_late_agenda";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn enemy_kind(fight: u8, health: HealthValue, hunter: bool) -> CardKind {
    CardKind::Enemy {
        fight,
        evade: 3,
        damage: 2,
        horror: 1,
        health: Some(health),
        victory: None,
        spawn: None,
        surge: false,
        peril: false,
        hunter,
        retaliate: false,
        prey: Prey::Default,
        quantity: 1,
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(test_support::test_card_metadata(
                PROBE_ENEMY,
                "Stage Probe",
                enemy_kind(4, HealthValue::PerInvestigator(3), true),
            ))
            .with_card(test_support::test_card_metadata(
                SPAWNER,
                "Stage Spawner",
                enemy_kind(1, HealthValue::Fixed(1), false),
            ))
            .with_card(test_support::test_card_metadata(
                PROBE_LOC,
                "Stage Room",
                CardKind::Location {
                    shroud: 4,
                    printed_clues: ClueValue::PerInvestigator(2),
                    victory: None,
                },
            ))
            .with_card(test_support::test_investigator_card(PROBE_INV))
            .with_card(test_support::test_card_metadata(
                PROBE_ACT,
                "Stage Act",
                CardKind::Act {
                    clue_threshold: Some(2),
                    victory: None,
                },
            ))
            .with_card(test_support::test_card_metadata(
                PROBE_OBJECTIVE_ACT,
                "Stage Objective",
                CardKind::Act {
                    clue_threshold: None,
                    victory: None,
                },
            ))
            .with_card(test_support::test_card_metadata(
                PROBE_AGENDA,
                "Stage Agenda",
                CardKind::Agenda { doom_threshold: 4 },
            ))
            .with_card(test_support::test_card_metadata(
                PROBE_LATE_AGENDA,
                "Stage Late Agenda",
                CardKind::Agenda { doom_threshold: 7 },
            ))
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

#[test]
fn a_printed_enemy_scales_its_health_to_one_seat() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1))
        .with_enemy(enemy(7).as_card(PROBE_ENEMY))
        .build();
    assert_eq!(state.enemies[&EnemyId(7)].max_health, 3);
}

#[test]
fn a_printed_enemy_scales_its_health_to_seats_staged_after_it() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_enemy(enemy(7).as_card(PROBE_ENEMY))
        .with_seat(seat(1))
        .with_seat(seat(2))
        .build();
    assert_eq!(state.enemies[&EnemyId(7)].max_health, 6);
}

#[test]
fn a_printed_enemy_scales_its_health_to_seats_staged_before_it() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1))
        .with_seat(seat(2))
        .with_enemy(enemy(7).as_card(PROBE_ENEMY))
        .build();
    assert_eq!(state.enemies[&EnemyId(7)].max_health, 6);
}

#[test]
fn a_printed_enemy_carries_the_printed_card_and_its_staged_board_state() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE))
        .with_enemy(
            enemy(7)
                .as_card(PROBE_ENEMY)
                .engaged_with(INV)
                .damage(1)
                .exhausted()
                .owner(Owner::Investigator(INV)),
        )
        .build();
    let staged = &state.enemies[&EnemyId(7)];
    assert_eq!(staged.name, "Stage Probe");
    assert_eq!(staged.code.as_str(), PROBE_ENEMY);
    assert_eq!((staged.fight, staged.evade), (4, 3));
    assert_eq!((staged.attack_damage, staged.attack_horror), (2, 1));
    assert!(staged.hunter);
    assert_eq!(staged.damage, 1);
    assert!(staged.exhausted);
    assert_eq!(staged.engaged_with, Some(INV));
    assert_eq!(staged.current_location, Some(HERE));
    assert_eq!(staged.owner, Owner::Investigator(INV));
}

#[test]
#[should_panic(expected = "as_card(\"01160\")")]
fn a_stock_enemy_rejects_a_printed_code() {
    let _ = enemy(7).code("01160");
}

#[test]
#[should_panic(expected = "as_card(\"01111b\")")]
fn a_stock_location_rejects_a_printed_code() {
    let _ = location(HERE).code("01111b");
}

#[test]
fn a_stock_enemy_takes_a_synthetic_code() {
    let state = GameStateBuilder::new()
        .with_enemy(enemy(7).code("_probe_01160"))
        .build();
    assert_eq!(state.enemies[&EnemyId(7)].code.as_str(), "_probe_01160");
}

#[test]
#[should_panic(expected = "the registry has no card _no_such_card")]
fn build_names_an_unknown_printed_code() {
    let _ = GameStateBuilder::new()
        .with_registries(registries())
        .with_enemy(enemy(7).as_card("_no_such_card"))
        .build();
}

#[test]
#[should_panic(expected = "as_card(_stage_enemy): name the registries")]
fn build_names_a_printed_code_when_no_registries_were_named() {
    let _ = GameStateBuilder::new()
        .with_enemy(enemy(7).as_card(PROBE_ENEMY))
        .build();
}

#[test]
fn a_staged_location_is_revealed_with_no_clues() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1))
        .with_location(location(HERE))
        .with_location(location(11).as_card(PROBE_LOC))
        .build();
    for id in [HERE, LocationId(11)] {
        let staged = &state.locations[&id];
        assert!(staged.revealed, "{id:?} is staged revealed");
        assert_eq!(staged.clues, 0, "{id:?} is staged with no clues");
    }
}

#[test]
fn a_printed_location_carries_the_printed_card_and_its_staged_board_state() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1))
        .with_location(
            location(HERE)
                .as_card(PROBE_LOC)
                .unrevealed()
                .clues(3)
                .connections([LocationId(11)]),
        )
        .build();
    let staged = &state.locations[&HERE];
    assert_eq!(staged.name, "Stage Room");
    assert_eq!(staged.code.as_str(), PROBE_LOC);
    assert_eq!(staged.shroud, 4);
    assert_eq!(staged.printed_clues, ClueValue::PerInvestigator(2));
    assert!(!staged.revealed);
    assert_eq!(staged.clues, 3);
    assert_eq!(staged.connections, vec![LocationId(11)]);
}

#[test]
fn a_spawn_after_staging_enemy_zero_mints_a_fresh_id() {
    // Mythos step 1.4: `INV` is to draw the top encounter card, the spawner.
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE))
        .with_location(location(HERE))
        .with_enemy(enemy(0).at(HERE))
        .with_seating([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .with_encounter_deck([SPAWNER])
        .build();
    let session = TestSession::new(state, registries()).confirm();
    let enemies = &session.state().enemies;
    assert_eq!(enemies.len(), 2, "the spawn joined the staged enemy");
    assert_eq!(
        enemies[&EnemyId(0)].code,
        enemy(0).mint(registries(), 1).code
    );
    assert!(enemies.values().any(|e| e.code.as_str() == SPAWNER));
}

#[test]
fn add_location_after_staging_mints_a_fresh_id() {
    let mut state = GameStateBuilder::new()
        .with_location(location(LocationId(0)))
        .build();
    let added = state.add_location(&test_support::test_card_metadata(
        PROBE_LOC,
        "Stage Room",
        CardKind::Location {
            shroud: 4,
            printed_clues: ClueValue::Fixed(0),
            victory: None,
        },
    ));
    assert_ne!(added, LocationId(0));
    assert_eq!(state.locations.len(), 2);
}

#[test]
fn place_resolves_against_the_built_board_and_advances_the_counter() {
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE))
        .with_seat(seat(2).at(HERE))
        .with_location(location(HERE))
        .build();
    state.place(
        enemy(5).as_card(PROBE_ENEMY).engaged_with(INV),
        registries(),
    );
    let placed = state.enemy(EnemyId(5));
    assert_eq!(placed.max_health, 6, "3 per investigator, two seated");
    assert_eq!(placed.current_location, Some(HERE));
    assert_eq!(state.enemy_ids.mint(), EnemyId(6));
}

#[test]
fn place_puts_a_location_on_the_built_board_and_advances_the_counter() {
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1))
        .build();
    state.place(location(4).as_card(PROBE_LOC), registries());
    assert_eq!(state.location(LocationId(4)).shroud, 4);
    assert_eq!(state.location_ids.mint(), LocationId(5));
}

#[test]
#[should_panic(expected = "place: EnemyId(5) is already on the board")]
fn place_refuses_an_id_already_on_the_board() {
    let mut state = GameStateBuilder::new().with_enemy(enemy(5)).build();
    state.place(enemy(5), registries());
}

#[test]
fn mint_gives_the_bare_printed_enemy() {
    let minted = enemy(7).as_card(PROBE_ENEMY).mint(registries(), 3);
    assert_eq!((minted.id, minted.max_health), (EnemyId(7), 9));
}

#[test]
fn mint_gives_the_bare_printed_location() {
    let minted = location(HERE).as_card(PROBE_LOC).mint(registries());
    assert_eq!((minted.id, minted.shroud), (HERE, 4));
}

#[test]
#[should_panic(expected = "enemy: no EnemyId(9) on the board; there: [EnemyId(1), EnemyId(4)]")]
fn the_enemy_reader_lists_the_enemies_there() {
    let state = GameStateBuilder::new()
        .with_enemy(enemy(1))
        .with_enemy(enemy(4))
        .build();
    let _ = state.enemy(EnemyId(9));
}

#[test]
#[should_panic(expected = "location: no LocationId(9) on the board; there: [LocationId(10)]")]
fn the_location_reader_lists_the_locations_there() {
    let state = GameStateBuilder::new()
        .with_location(location(HERE))
        .build();
    let _ = state.location(LocationId(9));
}

#[test]
fn card_in_play_finds_the_seated_card() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).in_play([("_stage_asset", clues(2))]))
        .build();
    assert_eq!(state.card_in_play(INV, "_stage_asset").clues, 2);
}

#[test]
#[should_panic(
    expected = "has 0 cards _missing in play or in their threat area, not exactly one; there: [_stage_asset"
)]
fn card_in_play_lists_the_cards_there() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).in_play(["_stage_asset"]))
        .build();
    let _ = state.card_in_play(INV, "_missing");
}

#[test]
fn printed_and_stock_seats_share_one_collection() {
    let seats: Vec<Seat<AnyKind>> = vec![
        seat(1).skill(SkillKind::Combat, 5).into(),
        seat(2).as_card(PROBE_INV).into(),
    ];
    let state = seats
        .into_iter()
        .fold(
            GameStateBuilder::new().with_registries(registries()),
            GameStateBuilder::with_seat,
        )
        .build();
    assert_eq!(state.investigators[&INV].skills.combat, 5);
    assert_eq!(state.investigators[&InvestigatorId(2)].name, PROBE_INV);
}

#[test]
fn an_act_deck_carries_each_cards_printed_threshold_in_order() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_act_deck([PROBE_ACT, PROBE_OBJECTIVE_ACT])
        .build();
    let deck: Vec<(&str, u8)> = state
        .act_deck
        .iter()
        .map(|act| (act.code.as_str(), act.clue_threshold))
        .collect();
    assert_eq!(deck, [(PROBE_ACT, 2), (PROBE_OBJECTIVE_ACT, 0)]);
}

#[test]
fn an_agenda_deck_carries_each_cards_printed_threshold_in_order() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_agenda_deck([PROBE_AGENDA, PROBE_LATE_AGENDA])
        .build();
    let deck: Vec<(&str, u8)> = state
        .agenda_deck
        .iter()
        .map(|agenda| (agenda.code.as_str(), agenda.doom_threshold))
        .collect();
    assert_eq!(deck, [(PROBE_AGENDA, 4), (PROBE_LATE_AGENDA, 7)]);
}

#[test]
fn a_deck_mixes_stock_and_printed_cards_in_the_order_written() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_agenda_deck([agenda("_stock_agenda", 2), StagedAgenda::from(PROBE_AGENDA)])
        .build();
    let deck: Vec<(&str, u8)> = state
        .agenda_deck
        .iter()
        .map(|agenda| (agenda.code.as_str(), agenda.doom_threshold))
        .collect();
    assert_eq!(deck, [("_stock_agenda", 2), (PROBE_AGENDA, 4)]);
}

#[test]
fn a_stock_act_keeps_its_stated_threshold_with_no_registries() {
    let state = GameStateBuilder::new()
        .with_act_deck([act("_act_1", 3), act("_act_2", 1)])
        .build();
    let deck: Vec<(&str, u8)> = state
        .act_deck
        .iter()
        .map(|act| (act.code.as_str(), act.clue_threshold))
        .collect();
    assert_eq!(deck, [("_act_1", 3), ("_act_2", 1)]);
}

#[test]
fn the_current_act_and_agenda_and_the_doom_land() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_act_deck([PROBE_ACT, PROBE_OBJECTIVE_ACT])
        .with_agenda_deck([PROBE_AGENDA, PROBE_LATE_AGENDA])
        .with_current_act(1)
        .with_current_agenda(1)
        .with_agenda_doom(5)
        .build();
    assert_eq!(
        state.act_deck[state.act_index].code.as_str(),
        PROBE_OBJECTIVE_ACT
    );
    assert_eq!(
        state.agenda_deck[state.agenda_index].code.as_str(),
        PROBE_LATE_AGENDA
    );
    assert_eq!(state.agenda_doom, 5);
}

#[test]
#[should_panic(expected = "with_act_deck([\"01108\"])")]
fn a_stock_act_rejects_a_printed_code() {
    let _ = act("01108", 2);
}

#[test]
#[should_panic(expected = "with_agenda_deck([\"01105\"])")]
fn a_stock_agenda_rejects_a_printed_code() {
    let _ = agenda("01105", 3);
}

#[test]
#[should_panic(expected = "with_act_deck(_stage_agenda): _stage_agenda is not an act card")]
fn build_names_a_deck_card_of_the_wrong_type() {
    let _ = GameStateBuilder::new()
        .with_registries(registries())
        .with_act_deck([PROBE_AGENDA])
        .build();
}

#[test]
#[should_panic(expected = "with_act_deck(_stage_act): name the registries")]
fn build_names_a_printed_deck_card_when_no_registries_were_named() {
    let _ = GameStateBuilder::new().with_act_deck([PROBE_ACT]).build();
}

#[test]
fn the_encounter_deck_and_discard_land_in_order() {
    let state = GameStateBuilder::new()
        .with_encounter_deck(["_top", "_second", "_bottom"])
        .with_encounter_discard(["_first_discarded", "_last_discarded"])
        .build();
    assert_eq!(
        Vec::from(state.encounter_deck),
        [
            CardCode::new("_top"),
            CardCode::new("_second"),
            CardCode::new("_bottom")
        ]
    );
    assert_eq!(
        state.encounter_discard,
        [
            CardCode::new("_first_discarded"),
            CardCode::new("_last_discarded")
        ]
    );
}
