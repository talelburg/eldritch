//! #773 integration: Lita Chantler 01117's two granted abilities, end-to-end
//! against the real `cards::REGISTRY`.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-08-30)
//!
//! **Lita Chantler (01117)**, `text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`) — an `Ally` asset,
//! health 3 / sanity 3, cost 0:
//!
//! > While you control Lita Chantler, she gains:
//! > "Each investigator at your location gets +1 \[combat\].
//! > \[reaction\] When an investigator at your location successfully attacks a
//! > \[\[Monster\]\] enemy: That investigator deals +1 damage."
//!
//! **Silver Twilight Acolyte (01102)**, `traits` verbatim: `Humanoid. Cultist.
//! Silver Twilight.` — the negative case. It has to come from outside The
//! Gathering: every enemy in that scenario (01116, 01118, 01119, 01160, 01161)
//! is `Humanoid. Monster. Ghoul.`
//!
//! **Machete (01020)**, `text` verbatim: *"\[action\]: **Fight.** You get +1
//! \[combat\] for this attack. If the attacked enemy is the only enemy engaged
//! with you, this attack deals +1 damage."*
//!
//! **Dynamite Blast (01024)**, `text` verbatim: *"Choose either your location or
//! a connecting location. Deal 3 damage to each enemy and to each investigator
//! at the chosen location."*
//!
//! ## Ruling (`data/arkhamdb-faq/core/01117.md`, <https://arkhamdb.com/card/01117>)
//!
//! > Lita's +1 damage bonus only applies to **Fight** actions, not to any other
//! > effects that deal damage ([Sneak Attack](https://arkhamdb.com/card/01052),
//! > etc.).
//!
//! The two ends of that ruling are the last two tests here: a designated
//! **Fight** (Machete) gets the bonus because a designator *performs* a Fight
//! action, and Dynamite Blast does not because it constructs no
//! `SkillTestFollowUp::Fight` for the bonus to ride on.
//!
//! ## The `when` cell
//!
//! `a_co_located_attack_on_a_monster_deals_one_more_damage` is the regression
//! cover for #773's engine half: `SkillTestResolved` migrated from caller-owned
//! to a coordinator-owned **bare milestone** so its `when` cell could be walked
//! at all. The `2` it asserts is the boost landing between RR ST.6's
//! determination and ST.7's application — *"before its impact upon the game
//! state resolves"* (`glossary/Ability.md`) — which is observable here and
//! nowhere in `timing_cells.rs`, whose tests drive a bare plain test with no
//! ST.7 of its own.
//!
//! ## One asymmetry worth naming
//!
//! *"Moving away from Lita"* is something only a **second** investigator can do.
//! She is an ally in her controller's play area, so her location is her
//! controller's by construction — he carries her with him. The bonus dropping
//! on a move is therefore tested on the bystander, which is also the case that
//! discriminates the printed predicate (*"each investigator at your location"*)
//! from its 1p reading.
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::card_registry;
use game_core::engine::modified_value::{ModifiedQuantity, ReadContext};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    EnemyId, GameState, InvestigatorId, LocationId, ModifierTarget, Phase, SkillKind,
    TokenModifiers,
};
use game_core::test_support::{
    test_enemy, test_investigator, test_location, GameStateBuilder, ScriptedResolver, TestSession,
};
use game_core::{assert_event, OptionId, TurnAction};

/// Lita Chantler.
const LITA: &str = "01117";
/// Machete — a designated **Fight** ability, for the ruling's inclusion half.
const MACHETE: &str = "01020";
/// Dynamite Blast — damage that is not an attack, for its exclusion half.
const DYNAMITE: &str = "01024";
/// Silver Twilight Acolyte — `Humanoid. Cultist. Silver Twilight.`
const ACOLYTE: &str = "01102";
/// "Skids" O'Toole — 8 health / 6 sanity, so Dynamite Blast's 3 to each
/// investigator at the chosen location neither defeats nor needs soaking away.
const SKIDS: &str = "01003";

/// Lita's controller.
const KEEPER: InvestigatorId = InvestigatorId(1);
/// A second investigator, who does the attacking in most cases below — so the
/// tests read *"an investigator"* rather than *"you"*.
const OTHER: InvestigatorId = InvestigatorId(2);

const PARLOR: LocationId = LocationId(1);
const HALLWAY: LocationId = LocationId(2);

const GHOUL: EnemyId = EnemyId(100);
const NON_MONSTER: EnemyId = EnemyId(101);

const LITA_INST: CardInstanceId = CardInstanceId(50);
const MACHETE_INST: CardInstanceId = CardInstanceId(51);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(cards::REGISTRY);
}

/// How the board is arranged for one case.
struct Board {
    /// Whether Lita is under `KEEPER`'s control (her play area) or in play at
    /// the Parlor under nobody's — the #772 transition, both sides of it.
    controlled: bool,
    /// Where the second investigator stands.
    other_at: LocationId,
    /// Where the two enemies stand, and whom they are engaged with.
    enemies_at: LocationId,
    /// Whose turn it is, and who therefore takes the Fight.
    actor: InvestigatorId,
    /// Machete in the actor's play area.
    machete: bool,
    /// Dynamite Blast in the actor's hand.
    dynamite: bool,
}

impl Board {
    fn new() -> Self {
        Self {
            controlled: true,
            other_at: PARLOR,
            enemies_at: PARLOR,
            actor: OTHER,
            machete: false,
            dynamite: false,
        }
    }

    fn build(self) -> GameState {
        // Skids O'Toole 01003 (8 health / 6 sanity), for both: the fixture's
        // own code is not in the real registry, and Dynamite Blast's damage
        // reads an investigator's printed health off it.
        let mut keeper = test_investigator(1);
        keeper.investigator_card.code = CardCode::new(SKIDS);
        keeper.current_location = Some(PARLOR);
        keeper.skills.combat = 3;

        let mut other = test_investigator(2);
        other.investigator_card.code = CardCode::new(SKIDS);
        other.current_location = Some(self.other_at);
        other.skills.combat = 3;

        if self.controlled {
            keeper
                .cards_in_play
                .push(CardInPlay::enter_play(CardCode::new(LITA), LITA_INST));
        }
        {
            let actor = if self.actor == KEEPER {
                &mut keeper
            } else {
                &mut other
            };
            if self.machete {
                actor
                    .cards_in_play
                    .push(CardInPlay::enter_play(CardCode::new(MACHETE), MACHETE_INST));
            }
            if self.dynamite {
                actor.hand.push(CardCode::new(DYNAMITE));
            }
        }

        // Health well clear of anything dealt below, so every case is read off
        // `damage` rather than off a defeat.
        let mut ghoul = test_enemy(100, "Ghoul");
        ghoul.traits = vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()];
        ghoul.fight = 3;
        ghoul.max_health = 9;
        ghoul.engaged_with = Some(self.actor);
        ghoul.current_location = Some(self.enemies_at);

        let mut acolyte = test_enemy(101, "Silver Twilight Acolyte");
        acolyte.code = CardCode::new(ACOLYTE);
        acolyte.traits = vec![
            "Humanoid".into(),
            "Cultist".into(),
            "Silver Twilight".into(),
        ];
        acolyte.fight = 3;
        acolyte.max_health = 9;
        acolyte.current_location = Some(self.enemies_at);

        let builder = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(keeper)
            .with_investigator(other)
            .with_location(test_location(1, "Parlor"))
            .with_location(test_location(2, "Hallway"))
            .with_enemy(ghoul)
            .with_enemy(acolyte)
            .with_active_investigator(self.actor)
            .with_turn_order([KEEPER, OTHER])
            .with_investigator_turn(self.actor)
            .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
            .with_token_modifiers(TokenModifiers::default());
        let uncontrolled = !self.controlled;
        let mut state = builder.build();
        if uncontrolled {
            state
                .locations
                .get_mut(&PARLOR)
                .expect("the Parlor is on the board")
                .cards_at_location
                .push(CardInPlay::enter_play(CardCode::new(LITA), LITA_INST));
        }
        state
    }
}

/// An investigator's modified `[combat]`, read the way the engine reads it.
fn combat(state: &GameState, who: InvestigatorId) -> i32 {
    game_core::modified_value(
        state,
        card_registry::current(),
        ModifierTarget::Investigator(who),
        ModifiedQuantity::Skill(SkillKind::Combat),
        ReadContext::OutsideTest,
    )
    .total()
}

// ---- "Each investigator at your location gets +1 [combat]." -----------

/// The controller's own bonus. She is in his play area, so her location is his
/// and the audience always reaches him — the case the printed text calls *"each
/// investigator at your location"* and 1p calls *"you"*.
#[test]
fn her_controller_gets_the_combat_bonus() {
    let state = Board::new().build();
    assert_eq!(combat(&state, KEEPER), 4, "printed 3 + her granted 1");
}

/// **The printed predicate, not its 1p degenerate case.** A second investigator
/// standing at her location gets it too, from a card in somebody else's play
/// area — which is why the sweep walks every investigator's instances rather
/// than only the target's.
#[test]
fn a_second_investigator_at_her_location_gets_it_too() {
    let state = Board::new().build();
    assert_eq!(combat(&state, OTHER), 4);
}

/// And loses it by walking away. Nothing is invalidated: the audience resolves
/// against her location at every read.
#[test]
fn moving_away_from_her_drops_the_bonus() {
    let state = Board {
        other_at: HALLWAY,
        ..Board::new()
    }
    .build();
    assert_eq!(combat(&state, OTHER), 3, "printed 3, nothing granted");
    assert_eq!(combat(&state, KEEPER), 4, "he still stands with her");
}

/// **While nobody controls her, neither half applies** — the grant's condition
/// is *"While **you control** Lita Chantler"*, and in the Parlor before a Parley
/// nobody does (#772). She is in play, at the very location the audience would
/// resolve to, and still contributes nothing.
#[test]
fn while_uncontrolled_the_combat_bonus_does_not_apply() {
    let state = Board {
        controlled: false,
        ..Board::new()
    }
    .build();
    assert_eq!(combat(&state, KEEPER), 3);
    assert_eq!(combat(&state, OTHER), 3);
}

// ---- "[reaction] When an investigator at your location …" -------------

/// Take `action` and answer the prompts it opens with `script`.
fn drive(
    state: GameState,
    action: &TurnAction,
    script: impl FnOnce(&mut ScriptedResolver),
) -> game_core::engine::ApplyResult {
    TestSession::new(state)
        .take(action)
        .resolve_choices(script)
        .run()
}

/// The script for a Fight that **does** open Lita's reaction window: commit
/// nothing, then either fire the window's only option or decline it.
fn fight_and_answer_window(fire: bool) -> impl FnOnce(&mut ScriptedResolver) {
    move |c: &mut ScriptedResolver| {
        c.commit_cards(&[]);
        if fire {
            c.pick_single(OptionId(0));
        } else {
            c.skip();
        }
    }
}

/// The script for a Fight that must open **no** reaction window at all: the
/// commit prompt and nothing after it.
///
/// This is what distinguishes *"never offered"* from *"offered and declined"*,
/// which is the whole of RR p.2's potential gate. `ScriptedResolver` panics on
/// a prompt it has no scripted answer for, so a window opening here fails the
/// test rather than being silently skipped.
fn fight_expecting_no_window(c: &mut ScriptedResolver) {
    c.commit_cards(&[]);
}

fn fight(actor: InvestigatorId, enemy: EnemyId) -> TurnAction {
    TurnAction::Fight {
        investigator: actor,
        enemy,
    }
}

/// The acceptance case: a **second** investigator, at her location, successfully
/// attacks a `[[Monster]]`. The reaction is offered — *"an investigator"*, not
/// *"you"* — and firing it makes the attack deal `1 + 1 = 2`.
///
/// *"That investigator deals +1 damage"* needs no targeting: the boost is
/// written onto the in-flight test, which is the attacker's, whichever
/// investigator's context fired the reaction.
#[test]
fn a_co_located_attack_on_a_monster_deals_one_more_damage() {
    let result = drive(
        Board::new().build(),
        &fight(OTHER, GHOUL),
        fight_and_answer_window(true),
    );
    assert_event!(
        result.events,
        Event::EnemyDamaged {
            enemy: GHOUL,
            amount: 2,
            ..
        }
    );
    assert_eq!(result.state.enemies[&GHOUL].damage, 2);
}

/// Declining the offered reaction leaves the base attack alone — so the `2`
/// above is the reaction's doing and not the board's.
#[test]
fn declining_the_reaction_leaves_the_attack_at_one_damage() {
    let result = drive(
        Board::new().build(),
        &fight(OTHER, GHOUL),
        fight_and_answer_window(false),
    );
    assert_eq!(result.state.enemies[&GHOUL].damage, 1);
}

/// Against Silver Twilight Acolyte 01102 the reaction is **never offered** — the
/// eligibility predicate runs before the ability is minted as a candidate, which
/// is RR p.2's *"A triggered ability can only be initiated if its effect has the
/// potential to change the game state"*. Nothing to decline; the attack simply
/// deals 1.
#[test]
fn against_a_non_monster_the_reaction_is_never_offered() {
    let result = drive(
        Board::new().build(),
        &fight(OTHER, NON_MONSTER),
        fight_expecting_no_window,
    );
    assert_eq!(
        result.state.enemies[&NON_MONSTER].damage, 1,
        "and `fight_expecting_no_window` scripts no answer for a reaction \
         prompt, so reaching this line at all is the \"never offered\" half",
    );
}

/// *"at **your** location"*: an investigator fighting the same `[[Monster]]`
/// somewhere else gets nothing, even though Lita is controlled and in play.
#[test]
fn an_attack_away_from_her_gets_no_bonus() {
    let state = Board {
        other_at: HALLWAY,
        enemies_at: HALLWAY,
        ..Board::new()
    }
    .build();
    let result = drive(state, &fight(OTHER, GHOUL), fight_expecting_no_window);
    assert_eq!(result.state.enemies[&GHOUL].damage, 1);
}

/// The reaction half of *"while uncontrolled, neither ability applies"*. She is
/// in play at the Parlor, the `[[Monster]]` is attacked there, and the grant's
/// `ByAPlayer` condition is the only thing standing between the two — so no
/// window opens and the attack deals its base 1.
#[test]
fn while_uncontrolled_the_reaction_does_not_apply() {
    let state = Board {
        controlled: false,
        ..Board::new()
    }
    .build();
    let result = drive(state, &fight(OTHER, GHOUL), fight_expecting_no_window);
    assert_eq!(result.state.enemies[&GHOUL].damage, 1);
}

// ---- her ruling ------------------------------------------------------

/// *"Lita's +1 damage bonus only applies to **Fight** actions"* — and a
/// designated **Fight** ability *is* one. Machete's own attack deals
/// `1 (base) + 1 (sole engaged enemy) = 2`; Lita's reaction takes it to 3.
#[test]
fn a_designated_fight_ability_gets_the_bonus() {
    let state = Board {
        actor: KEEPER,
        machete: true,
        ..Board::new()
    }
    .build();
    let result = drive(
        state,
        &TurnAction::ActivateAbility {
            investigator: KEEPER,
            source: AbilitySource::InPlay(MACHETE_INST),
            address: AbilityAddress::Printed(0),
        },
        |c: &mut ScriptedResolver| {
            // Two enemies stand at the Parlor, so a weapon Fight asks which
            // (#451 widened a Fight's candidates to every co-located enemy);
            // the Ghoul is `EnemyId(100)`, first in ascending order.
            c.pick_single(OptionId(0));
            c.commit_cards(&[]);
            c.pick_single(OptionId(0));
        },
    );
    assert_event!(
        result.events,
        Event::EnemyDamaged {
            enemy: GHOUL,
            amount: 3,
            ..
        }
    );
}

/// *"…not to any other effects that deal damage."* Dynamite Blast 01024 deals
/// its 3 to each enemy at the chosen location and never constructs a
/// `SkillTestFollowUp::Fight`, so there is no attack for the bonus to ride on
/// and no window to open.
#[test]
fn a_non_attack_damage_effect_gets_nothing() {
    let state = Board {
        actor: KEEPER,
        dynamite: true,
        ..Board::new()
    }
    .build();
    let result = drive(
        state,
        &TurnAction::PlayCard {
            investigator: KEEPER,
            hand_index: 0,
        },
        |c: &mut ScriptedResolver| {
            c.pick_single(OptionId(0));
            c.pick_single(OptionId(0));
            c.pick_single(OptionId(0));
        },
    );
    assert_eq!(
        result.state.enemies[&GHOUL].damage, 3,
        "the printed 3 and not a point more",
    );
}
