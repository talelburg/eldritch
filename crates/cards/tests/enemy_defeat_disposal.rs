//! Where a defeated enemy's card goes (#632), against the real
//! `cards::REGISTRY`. A defeated enemy leaves play through the leave-play exit,
//! filed by its owner (#982): the encounter deck's enemy to the encounter
//! discard, a weakness enemy to its bearer's discard.
//!
//! `data/rules-reference/rules/glossary/Defeat.md`:
//!
//! > If an enemy has as much or more damage on it as it has health, that enemy
//! > is defeated and placed on the encounter discard pile (or on its owner's
//! > discard pile if it is a weakness).
//!
//! `data/rules-reference/rules/glossary/Encounter_Deck.md`:
//!
//! > If the encounter deck is empty, shuffle the encounter discard pile back
//! > into the encounter deck.
//!
//! `data/rules-reference/rules/glossary/Victory_Display_Victory_Points.md`:
//!
//! > As a victory point enemy is defeated, place the card in the victory
//! > display instead of in the discard pile.
//!
//! The three enemies under test, verbatim from
//! `data/arkhamdb-snapshot/pack/core/`:
//!
//! - **Ghoul Minion 01160** (`core_encounter.json`) — no card text; health 2,
//!   fight 2, no Victory value.
//! - **Mob Enforcer 01101** (`core.json`, `subtype_code: "basicweakness"`) —
//!   "**Prey** - Bearer only.\nHunter.\n\\[action\\] Spend 4 resources:
//!   **Parley.** Discard Mob Enforcer."; health 3, fight 4.
//!   (Its Parley *discards* rather than defeats — <https://arkhamdb.com/card/01101>:
//!   "Discarding an enemy is not the same as defeating it" — so it does not
//!   reach the path under test here.)
//! - **Ghoul Priest 01116** (`core_encounter.json`) — "**Prey** - Highest
//!   \\[combat\\].\nHunter. Retaliate."; printed health 5 *per investigator*
//!   (`"health_per_investigator": true`), fight 4, Victory 2. These tests are
//!   solo, so the spawned health the engine would compute is 5 — the number the
//!   fixture uses.
//!
//! The enemies are built as fixtures rather than spawned from the corpus, each
//! with the owner it would enter play with: the encounter deck for Ghoul Minion
//! and Ghoul Priest, and its bearer for Mob Enforcer (`glossary/Weakness.md`). No
//! engine path spawns a weakness enemy from a draw yet (#514).

use cards::REGISTRY;
use game_core::action::{Action, EngineRecord, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, ApplyResult};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, ContinuationStack, DiscardPile,
    EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId, Owner, Phase, TokenModifiers,
    Zone,
};
use game_core::{assert_event, assert_event_sequence, test_support};

const GHOUL_MINION: &str = "01160";
const MOB_ENFORCER: &str = "01101";
const GHOUL_PRIEST: &str = "01116";
/// A card attached to the enemy under test. No core card attaches to an enemy,
/// so this models the primitive — an attachment with an owner — rather than a
/// printed card (ADR 0016).
const ATTACHMENT: &str = "_enemy_attachment";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// A solo investigator engaged with `code`, one point of damage short of
/// defeat, with combat high enough that the Fight test cannot fail against the
/// bag's single `Numeric(0)` token. The unarmed Fight deals 1 damage — enough
/// to defeat.
fn solo_investigator_facing(
    code: &str,
    owner: Owner,
    health: u8,
    fight: i8,
    victory: Option<u8>,
) -> (InvestigatorId, EnemyId, GameState) {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 8;

    let mut enemy = test_support::test_enemy(100, "Enemy under test");
    enemy.code = CardCode::new(code);
    enemy.fight = fight;
    enemy.max_health = health;
    enemy.damage = health - 1;
    enemy.victory = victory;
    enemy.owner = owner;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // Fight is location-gated (#401)

    let state = GameStateBuilder::new()
        .with_round(0)
        .open_turn(inv_id)
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(test_support::test_location(10, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, state)
}

/// Fight the enemy to death: the Fight suspends on the commit window, and
/// committing nothing resolves the test, the damage, and the defeat.
fn fight_to_defeat(state: GameState, inv_id: InvestigatorId, enemy_id: EnemyId) -> ApplyResult {
    let after_fight = test_support::take_turn_action(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );
    let result = engine::apply(
        after_fight.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert_event!(result.events, Event::EnemyDefeated { enemy: e, .. } if *e == enemy_id);
    assert!(
        !result.state.enemies.contains_key(&enemy_id),
        "the defeated enemy leaves play"
    );
    result
}

#[test]
fn defeated_ghoul_minion_is_drawn_again_once_the_encounter_deck_runs_out() {
    let (inv_id, enemy_id, state) =
        solo_investigator_facing(GHOUL_MINION, Owner::EncounterDeck, 2, 2, None);
    let after = fight_to_defeat(state, inv_id, enemy_id).state;

    assert_eq!(
        after.encounter_discard,
        vec![CardCode::new(GHOUL_MINION)],
        "a defeated non-weakness enemy is placed on the encounter discard pile"
    );

    // Now run the divergence scenario end to end: the encounter deck is empty
    // and an encounter card is drawn. `draw_encounter_top` reshuffles the
    // discard back in first ("If the encounter deck is empty, shuffle the
    // encounter discard pile back into the encounter deck") — so the card drawn
    // must be the Ghoul the investigator just killed, which then spawns again.
    // Before this fix the discard was empty and the draw had nothing to find.
    let mut state = after;
    assert!(state.encounter_deck.is_empty());
    state.continuations = ContinuationStack::new(); // drop the open-turn prompt; draw straight
    state.phase = Phase::Mythos;

    let redraw = engine::apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: inv_id,
        }),
    );
    assert_event!(
        redraw.events,
        Event::CardRevealed { code, .. } if code.as_str() == GHOUL_MINION
    );
    assert!(
        redraw
            .state
            .enemies
            .values()
            .any(|e| e.code.as_str() == GHOUL_MINION),
        "the reshuffled Ghoul Minion spawns again; enemies = {:?}",
        redraw.state.enemies
    );
    assert!(redraw.state.encounter_discard.is_empty());
}

#[test]
fn defeated_enemy_weakness_lands_in_its_owners_discard_pile() {
    let (inv_id, enemy_id, state) = solo_investigator_facing(
        MOB_ENFORCER,
        Owner::Investigator(InvestigatorId(1)),
        3,
        4,
        None,
    );
    let after = fight_to_defeat(state, inv_id, enemy_id).state;

    let inv = &after.investigators[&inv_id];
    assert_eq!(
        inv.discard,
        vec![CardCode::new(MOB_ENFORCER)],
        "a defeated enemy weakness goes to its owner's discard pile, so it stays \
         part of that investigator's deck for the campaign"
    );
    assert!(
        after.encounter_discard.is_empty(),
        "…and NOT to the encounter discard, which would feed a player card into \
         the encounter deck on the next reshuffle"
    );
    assert!(
        !inv.removed_from_game.contains(&CardCode::new(MOB_ENFORCER)),
        "…and NOT out of the game"
    );
    assert!(after.victory_display.is_empty());
}

#[test]
fn defeated_victory_enemy_goes_to_the_victory_display_and_no_discard_pile() {
    let (inv_id, enemy_id, state) =
        solo_investigator_facing(GHOUL_PRIEST, Owner::EncounterDeck, 5, 4, Some(2));
    let after = fight_to_defeat(state, inv_id, enemy_id).state;

    assert_eq!(after.victory_display, vec![CardCode::new(GHOUL_PRIEST)]);
    assert!(
        after.encounter_discard.is_empty(),
        "the victory display is instead of the discard pile"
    );
    assert!(after.investigators[&inv_id].discard.is_empty());
}

/// Attach an investigator-owned card to `enemy_id`.
fn attach_investigators_card(state: &mut GameState, enemy_id: EnemyId, owner: InvestigatorId) {
    state
        .enemies
        .get_mut(&enemy_id)
        .expect("the enemy under test")
        .attachments
        .push(CardInPlay::enter_play(
            CardCode::new(ATTACHMENT),
            CardInstanceId(500),
            Owner::Investigator(owner),
        ));
}

/// **A defeated enemy's attachment is discarded by its own owner.**
/// `glossary/Leaves_Play.md`: *"If a card leaves play, the following
/// consequences occur simultaneously with the card leaving play: … All
/// attachments on the card are discarded."* — and a discarded card goes to its
/// owner's pile (`glossary/Discard_Piles.md`). The Ghoul Minion is the encounter
/// deck's; the card attached to it is the investigator's. The disposal events
/// follow `EnemyDefeated`, the enemy's first.
#[test]
fn a_defeated_enemys_attachment_is_discarded_by_its_owner() {
    let (inv_id, enemy_id, mut state) =
        solo_investigator_facing(GHOUL_MINION, Owner::EncounterDeck, 2, 2, None);
    attach_investigators_card(&mut state, enemy_id, inv_id);
    let result = fight_to_defeat(state, inv_id, enemy_id);

    assert_eq!(
        result.state.encounter_discard,
        vec![CardCode::new(GHOUL_MINION)]
    );
    assert_eq!(
        result.state.investigators[&inv_id].discard,
        vec![CardCode::new(ATTACHMENT)],
        "the attachment goes to its owner's discard, not the encounter discard"
    );
    assert_event_sequence!(
        result.events,
        Event::EnemyDefeated { enemy, .. } if *enemy == enemy_id,
        Event::CardDiscarded {
            code,
            from: Zone::Enemy,
            to: DiscardPile::Encounter,
        } if code.as_str() == GHOUL_MINION,
        Event::CardDiscarded {
            code,
            from: Zone::EnemyAttachment,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ATTACHMENT && *to == inv_id,
    );
}

/// **A Victory enemy's attachment is discarded too.** The enemy goes to the
/// victory display instead of a discard pile, but it still leaves play, so
/// Leaves Play's *"All attachments on the card are discarded"* applies to it as
/// to any other.
#[test]
fn a_defeated_victory_enemys_attachment_is_discarded_by_its_owner() {
    let (inv_id, enemy_id, mut state) =
        solo_investigator_facing(GHOUL_PRIEST, Owner::EncounterDeck, 5, 4, Some(2));
    attach_investigators_card(&mut state, enemy_id, inv_id);
    let result = fight_to_defeat(state, inv_id, enemy_id);

    assert_eq!(
        result.state.victory_display,
        vec![CardCode::new(GHOUL_PRIEST)]
    );
    assert!(result.state.encounter_discard.is_empty());
    assert_eq!(
        result.state.investigators[&inv_id].discard,
        vec![CardCode::new(ATTACHMENT)]
    );
    assert_event_sequence!(
        result.events,
        Event::EnemyDefeated { enemy, .. } if *enemy == enemy_id,
        Event::EnteredVictoryDisplay { code, victory: 2 } if code.as_str() == GHOUL_PRIEST,
        Event::CardDiscarded {
            code,
            from: Zone::EnemyAttachment,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ATTACHMENT && *to == inv_id,
    );
}
