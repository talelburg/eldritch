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
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{ApplyResult, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, ChaosBag, ChaosToken, DiscardPile, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, Owner, SkillKind, TokenModifiers, Zone,
};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event, assert_event_sequence};

const GHOUL_MINION: &str = "01160";
const MOB_ENFORCER: &str = "01101";
const GHOUL_PRIEST: &str = "01116";
/// A card attached to the enemy under test. No core card attaches to an enemy,
/// so this models the primitive — an attachment with an owner — rather than a
/// printed card (ADR 0016).
const ATTACHMENT: &str = "_enemy_attachment";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// A solo investigator engaged with `code`, one point of damage short of
/// defeat, with combat high enough that the Fight test cannot fail against the
/// bag's single `Numeric(0)` token. The unarmed Fight deals 1 damage — enough
/// to defeat. A spare deck card lets a test end the turn and play on through
/// Upkeep step 4.4's draw.
fn solo_investigator_facing(
    code: &str,
    owner: Owner,
    health: u8,
    fight: i8,
    victory: Option<u8>,
) -> GameState {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Enemy under test");
    enemy.code = CardCode::new(code);
    enemy.fight = fight;
    enemy.max_health = health;
    enemy.damage = health - 1;
    enemy.victory = victory;
    enemy.owner = owner;

    GameStateBuilder::new()
        .with_round(0)
        .with_seat(seat(1).at(HERE).skill(SkillKind::Combat, 8).deck(["01088"]))
        // Fight is location-gated (#401); an engaged enemy is at its
        // investigator's location.
        .with_enemy_engaged(enemy, INV)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .build()
}

/// Fight the enemy to death: the Fight suspends on the commit window, and
/// committing nothing resolves the test, the damage, and the defeat.
fn fight_to_defeat(state: GameState) -> TestSession {
    let session = TestSession::new(state)
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: ENEMY,
        })
        .commit(&[]);
    assert_event!(session.events(), Event::EnemyDefeated { enemy: e, .. } if *e == ENEMY);
    assert!(
        !session.state().enemies.contains_key(&ENEMY),
        "the defeated enemy leaves play"
    );
    session
}

/// [`fight_to_defeat`], ended: the commit step's state and events.
fn defeated(state: GameState) -> ApplyResult {
    fight_to_defeat(state).finish()
}

#[test]
fn defeated_ghoul_minion_is_drawn_again_once_the_encounter_deck_runs_out() {
    let after = fight_to_defeat(solo_investigator_facing(
        GHOUL_MINION,
        Owner::EncounterDeck,
        2,
        2,
        None,
    ));

    assert_eq!(
        after.state().encounter_discard,
        vec![CardCode::new(GHOUL_MINION)],
        "a defeated non-weakness enemy is placed on the encounter discard pile"
    );

    // Now run the divergence scenario end to end: the encounter deck is empty
    // and an encounter card is drawn. `draw_encounter_top` reshuffles the
    // discard back in first ("If the encounter deck is empty, shuffle the
    // encounter discard pile back into the encounter deck") — so the card drawn
    // must be the Ghoul the investigator just killed, which then spawns again.
    // Before this fix the discard was empty and the draw had nothing to find.
    assert!(after.state().encounter_deck.is_empty());
    // Ending the turn plays on through the round to the next Mythos phase's
    // encounter draw.
    let drawing = after.take(&TurnAction::EndTurn);
    assert_eq!(drawing.prompt().target, Some(OptionTarget::EncounterDeck));
    let seen = drawing.events().len();

    let redraw = drawing.confirm();
    let events = &redraw.events()[seen..];
    assert_event!(
        events,
        Event::CardRevealed { code, .. } if code.as_str() == GHOUL_MINION
    );
    assert!(
        redraw
            .state()
            .enemies
            .values()
            .any(|e| e.code.as_str() == GHOUL_MINION),
        "the reshuffled Ghoul Minion spawns again; enemies = {:?}",
        redraw.state().enemies
    );
    assert!(redraw.state().encounter_discard.is_empty());
}

#[test]
fn defeated_enemy_weakness_lands_in_its_owners_discard_pile() {
    let after = defeated(solo_investigator_facing(
        MOB_ENFORCER,
        Owner::Investigator(INV),
        3,
        4,
        None,
    ))
    .state;

    let inv = &after.investigators[&INV];
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
    let after = defeated(solo_investigator_facing(
        GHOUL_PRIEST,
        Owner::EncounterDeck,
        5,
        4,
        Some(2),
    ))
    .state;

    assert_eq!(after.victory_display, vec![CardCode::new(GHOUL_PRIEST)]);
    assert!(
        after.encounter_discard.is_empty(),
        "the victory display is instead of the discard pile"
    );
    assert!(after.investigators[&INV].discard.is_empty());
}

/// Attach an investigator-owned card to the enemy under test. A seat places
/// no enemy attachment, so it goes on the built state, with an id minted from
/// its counter.
fn attach_investigators_card(state: &mut GameState) {
    let id = state.card_instance_ids.mint();
    state
        .enemies
        .get_mut(&ENEMY)
        .expect("the enemy under test")
        .attachments
        .push(CardInPlay::enter_play(
            CardCode::new(ATTACHMENT),
            id,
            Owner::Investigator(INV),
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
    let mut state = solo_investigator_facing(GHOUL_MINION, Owner::EncounterDeck, 2, 2, None);
    attach_investigators_card(&mut state);
    let result = defeated(state);

    assert_eq!(
        result.state.encounter_discard,
        vec![CardCode::new(GHOUL_MINION)]
    );
    assert_eq!(
        result.state.investigators[&INV].discard,
        vec![CardCode::new(ATTACHMENT)],
        "the attachment goes to its owner's discard, not the encounter discard"
    );
    assert_event_sequence!(
        result.events,
        Event::EnemyDefeated { enemy, .. } if *enemy == ENEMY,
        Event::CardDiscarded {
            code,
            from: Zone::Enemy,
            to: DiscardPile::Encounter,
        } if code.as_str() == GHOUL_MINION,
        Event::CardDiscarded {
            code,
            from: Zone::EnemyAttachment,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ATTACHMENT && *to == INV,
    );
}

/// **A Victory enemy's attachment is discarded too.** The enemy goes to the
/// victory display instead of a discard pile, but it still leaves play, so
/// Leaves Play's *"All attachments on the card are discarded"* applies to it as
/// to any other.
#[test]
fn a_defeated_victory_enemys_attachment_is_discarded_by_its_owner() {
    let mut state = solo_investigator_facing(GHOUL_PRIEST, Owner::EncounterDeck, 5, 4, Some(2));
    attach_investigators_card(&mut state);
    let result = defeated(state);

    assert_eq!(
        result.state.victory_display,
        vec![CardCode::new(GHOUL_PRIEST)]
    );
    assert!(result.state.encounter_discard.is_empty());
    assert_eq!(
        result.state.investigators[&INV].discard,
        vec![CardCode::new(ATTACHMENT)]
    );
    assert_event_sequence!(
        result.events,
        Event::EnemyDefeated { enemy, .. } if *enemy == ENEMY,
        Event::EnteredVictoryDisplay { code, victory: 2 } if code.as_str() == GHOUL_PRIEST,
        Event::CardDiscarded {
            code,
            from: Zone::EnemyAttachment,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ATTACHMENT && *to == INV,
    );
}
