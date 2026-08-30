//! Lita Chantler (The Gathering ally asset, 01117).
//!
//! ```text
//! Ally.
//! While you control Lita Chantler, she gains:
//! "Each investigator at your location gets +1 [combat].
//! [reaction] When an investigator at your location successfully attacks a
//! [[Monster]] enemy: That investigator deals +1 damage."
//! ```
//!
//! # The card prints a grant, not two abilities
//!
//! *"While you control Lita Chantler, **she gains**"* — so what 01117 prints is
//! a `Trigger::Constant` [`Effect::Grant`](card_dsl::dsl::Effect::Grant) addressed at itself, conditioned on
//! being controlled by a player, and the two abilities inside it are what she
//! *has* while that holds. `glossary/Gains.md`: *"If a card gains a
//! characteristic (such as an icon, a trait, a keyword, or ability text), the
//! card functions as if it possesses the gained characteristic."*
//!
//! The condition is a **field on the grant**, never an `Effect::If` around it:
//! the grant sweep matches a bare `Effect::Grant` and skips a wrapped one
//! silently (ADR 0014). [`Condition::ControlStatus`](card_dsl::dsl::Condition::ControlStatus) is board-global, which is
//! what lets it be asked at all — the sweep evaluates a grant's condition
//! against the *recipient's* controller, and the whole question here is whether
//! she has one.
//!
//! This is the exact complement of the Parlor 01115's grant, which gives her a
//! **Parley** while `ByNoPlayer` (#772). Exactly one of the two applies at a
//! time; that is the two cards agreeing, not something the mechanism enforces.
//!
//! # The `+1 [combat]` half
//!
//! *"Each investigator at your location"* is
//! [`ModifierAudience::EachInvestigatorAtSourceLocation`], resolved by the
//! modifier sweep against the source's own location. While controlled she sits
//! in her controller's `cards_in_play`, so that location is her controller's —
//! and the audience reaches **every** investigator standing there, not just the
//! controller. The 1p case is the degenerate one, not the rule.
//!
//! She is the audience's first corpus consumer, and she is also why
//! `modified_value::sweep` now sources abilities through
//! `abilities_in_effect::for_source` (#773): a *granted* `Effect::Modify` used
//! to reach the ability paths and nothing else, so this modifier would have been
//! merged onto her ability list and contributed to no stat.
//!
//! # The reaction half
//!
//! *"\[reaction\] **When** an investigator at your location successfully
//! attacks a \[\[Monster\]\] enemy: That investigator deals +1 damage."*
//!
//! **Cell: the `when` cell of the `SkillTestResolved` condition.** The printed
//! word is *"When"*, and `glossary/Ability.md` says what that buys:
//!
//! > A \[reaction\] ability with a triggering condition beginning with the word
//! > "when..." may be used after the specified triggering condition initiates,
//! > but before its impact upon the game state resolves.
//!
//! **She is why that cell exists on this condition.** `SkillTestResolved` was
//! caller-owned until #773, so its `when` cell was not walked and an ability
//! declaring interrupt timing on it was *rejected* — and `standards.md` is
//! explicit that migrating the condition is the fix rather than retagging the
//! card. So the condition migrated, as a **bare milestone**: RR ST.6 is
//! *"Determine success/failure of skill test"*, and a determination mutates
//! nothing — the verdict is stashed on the in-flight test and logged, and no
//! card, enemy, location or investigator moves. The impact is ST.7, *"Apply
//! skill test results"*, which the `SkillTest` frame runs on its own resume
//! after the whole sequence has drained.
//!
//! That is exactly the ordering this ability needs: the boost is written before
//! `ApplyFollowUp` deals the attack's damage, which is *"before its impact upon
//! the game state resolves"*. ADR 0008 carries the classification and the four
//! migrations before this one.
//!
//! **"That investigator" needs no targeting.** [`Effect::BoostAttackDamage`](card_dsl::dsl::Effect::BoostAttackDamage)
//! writes the *in-flight* test's accumulator, and the in-flight test **is** the
//! attacker's, whatever `EvalContext` fires the reaction. The Fight follow-up
//! reads it back at the one site that consumes it.
//!
//! ## Why the pattern grew a `by_controller` flag
//!
//! *"an investigator"*, not *"you"*. The reaction matcher hard-requires *test
//! investigator == controller* **before** a candidate is minted, so no
//! eligibility predicate — which runs afterwards — could have widened it back
//! out. Hence `EventPattern::SkillTestResolved { by_controller: false }`, with
//! every existing consumer (Dr. Milan 01033, Obscuring Fog 01168) taking `true`.
//!
//! ## Why `[[Monster]]` and "at your location" are one card-local native
//!
//! `standards.md`: *"A new `Effect` (or `EventPattern`) variant waits until two
//! or more hand-written cards want the same pattern. Until then the card gets a
//! Rust impl or a card-local native tag."* A corpus survey put the near-term
//! count at **one** — this card — for both a "successful attack" trigger and a
//! trait filter on an attacked enemy. The trait dimension that *does* scale is
//! trait-filtered enemies in triggers generally, whose big consumers are
//! **defeat** and **damage-dealt** rather than attack, so coupling a trait
//! filter to an attack pattern buys generality where it would not compound.
//!
//! Running it as an eligibility predicate rather than inside the effect also
//! honours RR p.2 — *"A triggered ability can only be initiated if its effect
//! has the potential to change the game state"* — so against a non-`[[Monster]]`
//! the reaction is never **offered**, rather than offered and inert.
//!
//! # Module gap
//!
//! Promote the predicate to DSL vocabulary when a second card wants the same
//! shape. The graduation triggers are **Grievous Wound 09027** (*"Play after you
//! successfully attack a non-\[\[Elite\]\] enemy using a \[\[Melee\]\] asset."*)
//! and **Queen of Ash 12168**; the second of them arriving is the signal to
//! turn "successful attack against a trait-filtered enemy" into a real pattern.
//!
//! # Rulings (`data/arkhamdb-faq/core/01117.md`, <https://arkhamdb.com/card/01117>)
//!
//! > Lita's +1 damage bonus only applies to **Fight** actions, not to any other
//! > effects that deal damage ([Sneak Attack](https://arkhamdb.com/card/01052),
//! > etc.).
//!
//! This falls out of the plumbing rather than needing a check: the bonus lives
//! on `SkillTestFollowUp::Fight`, which only a Fight *ability* constructs. A
//! designated **Fight** (Machete 01020, .45 Automatic 01016, Roland's .38
//! Special 01006) runs a `SkillTestKind::Fight` test and qualifies — the ruling
//! scopes to the Fight action, and a designator *performs* one. Dynamite Blast
//! 01024 and Sneak Attack 01052 never construct that follow-up, so they get
//! nothing. (The issue named Dynamite Blast 01023; that code is **Dodge**.)
//!
//! Her three other rulings — that taking control puts her in the play area, that
//! the control is temporary, and that she is removed from the game if she leaves
//! play — belong to the take-control transition and shipped with #772.

use card_dsl::dsl::{
    boost_attack_damage, constant, control_status, grant, modify_for, reaction_on_event, Ability,
    ControlStatus, EventPattern, EventTiming, GrantTarget, ModifierAudience, ModifierScope,
    SkillTestKind, Stat, TestOutcome,
};
use game_core::card_registry::EligibilityFn;
use game_core::state::{GameState, LocationId, SkillTestFollowUp};
use game_core::EvalContext;

/// `ArkhamDB` code for Lita Chantler.
pub const CODE: &str = "01117";

/// The trait the reaction filters the attacked enemy on.
const MONSTER: &str = "Monster";

/// Eligibility tag carrying the whole printed predicate of the reaction —
/// *"an investigator **at your location** successfully attacks a
/// **\[\[Monster\]\]** enemy"*. Both halves ride one tag because both are
/// answered off the same in-flight Fight frame.
const MONSTER_ATTACKED_HERE_TAG: &str = "01117:monster_attacked_here";

/// What 01117 prints: one `Trigger::Constant` [`Effect::Grant`](card_dsl::dsl::Effect::Grant)
/// to herself, holding the two abilities she gains while a player controls her.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![constant(grant(
        GrantTarget::SelfCard,
        Some(control_status(CODE, ControlStatus::ByAPlayer)),
        vec![
            // "Each investigator at your location gets +1 [combat]."
            constant(modify_for(
                ModifierAudience::EachInvestigatorAtSourceLocation,
                Stat::Combat,
                1,
                ModifierScope::WhileInPlay,
            )),
            // "[reaction] When an investigator at your location successfully
            // attacks a [[Monster]] enemy: That investigator deals +1 damage."
            reaction_on_event(
                EventPattern::SkillTestResolved {
                    outcome: TestOutcome::Success,
                    kind: Some(SkillTestKind::Fight),
                    by_controller: false,
                },
                EventTiming::When,
                boost_attack_damage(1),
            )
            .with_eligibility(MONSTER_ATTACKED_HERE_TAG),
        ],
    ))]
}

/// Resolve Lita's eligibility tag.
#[must_use]
pub(crate) fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    match tag {
        MONSTER_ATTACKED_HERE_TAG => Some(monster_attacked_here as EligibilityFn),
        _ => None,
    }
}

/// *"an investigator at your location successfully attacks a `[[Monster]]`
/// enemy"*, read off the in-flight test.
///
/// The test's own [`SkillTestFollowUp::Fight`] is what makes it an **attack** —
/// the same frame the +1 damage will ride on — so a Fight test that is not
/// resolving an attack, and any non-Fight test, answers `false`.
///
/// *"your location"* is the **controller's**, and that is Lita's: the ability is
/// hers, she is gained only while a player controls her, and a controlled ally
/// asset sits in that player's play area. So the same location the `+1 [combat]`
/// audience resolves against answers this too — the two halves cannot disagree.
fn monster_attacked_here(state: &GameState, ctx: &EvalContext) -> bool {
    let Some(here) = investigator_location(state, ctx.controller) else {
        return false;
    };
    let Some(test) = state.current_skill_test() else {
        return false;
    };
    let SkillTestFollowUp::Fight { enemy, .. } = test.follow_up else {
        return false;
    };
    if investigator_location(state, test.investigator) != Some(here) {
        return false;
    }
    state
        .enemies
        .get(&enemy)
        .is_some_and(|e| e.traits.iter().any(|t| t == MONSTER))
}

fn investigator_location(
    state: &GameState,
    id: game_core::state::InvestigatorId,
) -> Option<LocationId> {
    state.investigators.get(&id)?.current_location
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        Condition, ControlStatus, Effect, EventPattern, EventTiming, GrantTarget, ModifierAudience,
        ModifierScope, SkillTestKind, Stat, TestOutcome, Trigger, TriggerKind,
    };
    use game_core::card_data::SkillKind;
    use game_core::state::{
        CardCode, CardInPlay, CardInstanceId, Continuation, EnemyId, GameState, GameStateBuilder,
        InvestigatorId, LocationId, SkillTestFollowUp, SkillTestId,
    };
    use game_core::test_support::{test_enemy, test_investigator, test_location, test_skill_test};
    use game_core::EvalContext;

    /// The one thing 01117 prints. Destructured rather than `matches!`-ed with
    /// `..`, so an `Effect::If` wrapper — which the grant sweep skips silently —
    /// fails the test rather than passing it.
    fn granted() -> Vec<card_dsl::dsl::Ability> {
        let abilities = super::abilities();
        assert_eq!(
            abilities.len(),
            1,
            "the card prints one grant and nothing else"
        );
        assert_eq!(abilities[0].trigger, Trigger::Constant);
        let Effect::Grant {
            to,
            condition,
            abilities: granted,
        } = &abilities[0].effect
        else {
            panic!(
                "01117 prints a bare Effect::Grant, got {:?}",
                abilities[0].effect
            );
        };
        assert_eq!(
            *to,
            GrantTarget::SelfCard,
            "\"she gains\" — she grants to herself"
        );
        assert_eq!(
            *condition,
            Some(Condition::ControlStatus {
                code: super::CODE.to_owned(),
                status: ControlStatus::ByAPlayer,
            }),
            "\"While you control Lita Chantler\" — and on the grant, never \
             wrapped in an Effect::If",
        );
        granted.clone()
    }

    #[test]
    fn the_grant_holds_the_two_abilities_she_gains() {
        assert_eq!(granted().len(), 2);
    }

    #[test]
    fn the_first_granted_ability_is_the_location_scoped_combat_buff() {
        let granted = granted();
        assert_eq!(granted[0].trigger, Trigger::Constant);
        assert_eq!(
            granted[0].effect,
            Effect::Modify {
                stat: Stat::Combat,
                delta: 1,
                scope: ModifierScope::WhileInPlay,
                audience: ModifierAudience::EachInvestigatorAtSourceLocation,
            },
            "\"Each investigator at your location\" — the printed predicate, \
             not the 1p degenerate case",
        );
    }

    #[test]
    fn the_second_granted_ability_is_the_when_reaction_on_any_investigators_attack() {
        let granted = granted();
        assert_eq!(
            granted[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::SkillTestResolved {
                    outcome: TestOutcome::Success,
                    kind: Some(SkillTestKind::Fight),
                    by_controller: false,
                },
                timing: EventTiming::When,
                kind: TriggerKind::Reaction,
            },
            "\"[reaction] When an investigator ... successfully attacks\": the \
             `when` cell, which #773 opened on this condition by migrating it \
             to coordinator-owned — and unqualified, \"an investigator\", not \
             \"you\"",
        );
        assert_eq!(granted[1].effect, Effect::BoostAttackDamage(1));
        assert_eq!(
            granted[1].eligibility.as_deref(),
            Some(super::MONSTER_ATTACKED_HERE_TAG),
            "the [[Monster]] and at-your-location halves ride the tag",
        );
    }

    /// Board for the predicate: Lita's controller and the attacker each at a
    /// location of their own, one `[[Monster]]` enemy and one that is not.
    ///
    /// `attacker_here` puts the attacker at the controller's location;
    /// `monster` picks which enemy the in-flight Fight is against.
    fn board(attacker_here: bool, monster: bool, with_fight: bool) -> GameState {
        const HERE: LocationId = LocationId(1);
        const ELSEWHERE: LocationId = LocationId(2);
        let attacker = InvestigatorId(2);

        let mut lita_controller = test_investigator(1);
        lita_controller.current_location = Some(HERE);
        lita_controller.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(super::CODE),
            CardInstanceId(9),
        ));

        let mut other = test_investigator(2);
        other.current_location = Some(if attacker_here { HERE } else { ELSEWHERE });

        // A Ghoul from The Gathering (Humanoid. Monster. Ghoul.) and Silver
        // Twilight Acolyte 01102 (Humanoid. Cultist. Silver Twilight.), the
        // corpus's nearest non-[[Monster]] — every enemy in The Gathering itself
        // is a [[Monster]], so the negative case has to come from outside it.
        let mut ghoul = test_enemy(1, "Ghoul Minion");
        ghoul.traits = vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()];
        let mut acolyte = test_enemy(2, "Silver Twilight Acolyte");
        acolyte.code = CardCode::new("01102");
        acolyte.traits = vec![
            "Humanoid".into(),
            "Cultist".into(),
            "Silver Twilight".into(),
        ];

        let mut state = GameStateBuilder::new()
            .with_investigator(lita_controller)
            .with_investigator(other)
            .with_location(test_location(1, "Parlor"))
            .with_location(test_location(2, "Hallway"))
            .with_enemy(ghoul)
            .with_enemy(acolyte)
            .build();

        if with_fight {
            let mut test = test_skill_test(
                SkillTestId(1),
                attacker,
                SkillKind::Combat,
                SkillTestKind::Fight,
                3,
            );
            test.follow_up = SkillTestFollowUp::Fight {
                enemy: if monster { EnemyId(1) } else { EnemyId(2) },
                extra_damage: 0,
            };
            state.continuations.push(Continuation::SkillTest(test));
        }
        state
    }

    /// A `[[Monster]]` attacked by an investigator standing with Lita.
    fn co_located_monster_fight() -> GameState {
        board(true, true, true)
    }

    /// The same attack against Silver Twilight Acolyte 01102.
    fn co_located_non_monster_fight() -> GameState {
        board(true, false, true)
    }

    /// A `[[Monster]]` attacked from the Hallway, away from her.
    fn monster_fight_elsewhere() -> GameState {
        board(false, true, true)
    }

    /// The same board with no test in flight at all.
    fn no_test_in_flight() -> GameState {
        board(true, true, false)
    }

    fn eligible(state: &GameState) -> bool {
        let pred =
            super::native_eligibility_for(super::MONSTER_ATTACKED_HERE_TAG).expect("registered");
        pred(
            state,
            &EvalContext::for_controller_with_source(InvestigatorId(1), CardInstanceId(9)),
        )
    }

    #[test]
    fn a_co_located_attack_on_a_monster_is_eligible() {
        assert!(eligible(&co_located_monster_fight()));
    }

    #[test]
    fn a_co_located_attack_on_a_non_monster_is_not() {
        assert!(
            !eligible(&co_located_non_monster_fight()),
            "Silver Twilight Acolyte 01102 is Humanoid. Cultist. Silver \
             Twilight. — no [[Monster]], so the reaction is never offered",
        );
    }

    #[test]
    fn an_attack_on_a_monster_elsewhere_is_not() {
        assert!(
            !eligible(&monster_fight_elsewhere()),
            "\"an investigator **at your location**\"",
        );
    }

    #[test]
    fn with_no_fight_in_flight_nothing_is_eligible() {
        assert!(!eligible(&no_test_in_flight()));
    }

    #[test]
    fn the_tag_dispatches_and_an_unknown_one_does_not() {
        assert!(super::native_eligibility_for(super::MONSTER_ATTACKED_HERE_TAG).is_some());
        assert!(super::native_eligibility_for("nope").is_none());
        assert!(crate::impls::native_eligibility_for(super::MONSTER_ATTACKED_HERE_TAG).is_some());
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
