//! Attacks of opportunity provoked by **activated abilities** (#361, K3 of the
//! keystone arc), driven through the public [`apply`] API with the real card
//! corpus installed.
//!
//! Per the Rules Reference (p.5, "Attack of Opportunity"): an investigator
//! engaged with one or more ready enemies who takes an action **other than to
//! fight, to evade, or to activate a parley or resign ability** provokes one
//! `AoO` from each such enemy, after costs are paid and before the action's
//! effect resolves. So a non-fight/evade action-cost activated ability (First
//! Aid 01019, Flashlight 01087, Medical Texts 01035, Old Book of Lore 01031)
//! provokes; a Fight ability (Machete 01020, .45 Automatic) is exempt; and a
//! fast ability (`action_cost == 0`, e.g. Beat Cop 01018) is not an action and
//! never provokes.
//!
//! `game-core`'s unit tests can't install `cards::REGISTRY` (the engine crate
//! can't depend on `cards`), so this lives in `crates/cards/tests/`. The `AoO`
//! window/suspend mechanism itself is covered by `dodge_aoo.rs` /
//! `guard_dog_soak.rs` (K1); these tests prove the **new firing sites** route
//! through it.
//!
//! ## Verified card text (`ArkhamDB`, 2026-06-21)
//!
//! **First Aid (01019):** "[action] Spend 1 supply: Heal 1 damage or 1 horror
//! from an investigator at your location." A non-fight action ability → provokes.
//!
//! **Machete (01020):** "[action]: Fight. You get +1 [combat] for this attack.
//! …" A Fight ability → `AoO`-exempt (RR p.5).
//!
//! **Flashlight (01087):** "Uses (3 supplies). [action] Spend 1 supply:
//! Investigate. Your location gets -2 shroud for this investigation." A bold
//! **Investigate** designator — which the exempt list does *not* name — so it
//! provokes, exactly as the basic investigate action does.
//!
//! Since #696 the exemption reads the ability's declared **action designator**
//! rather than its effect root. The shapes only a designator can distinguish —
//! a `Seq`-wrapped Fight, and the **Parley** / **Resign** abilities that have no
//! effect of their own — have no corpus card to exercise them and are covered by
//! `crates/game-core/tests/action_designator_aoo.rs`.
#![allow(clippy::too_many_lines)]

use cards::REGISTRY;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, PromptNature};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, ChaosBag, ChaosToken, Enemy, EnemyId,
    GameStateBuilder, InvestigatorId, LocationId, Status, UseKind,
};
use game_core::test_support::{self, seat, uses, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

/// First Aid (01019): Guardian Item, `[action] Spend 1 supply: Heal …`. A
/// non-fight action ability → provokes an `AoO`.
const FIRST_AID: &str = "01019";
/// Machete (01020): Guardian weapon, `[action]: Fight …`. `AoO`-exempt.
const MACHETE: &str = "01020";
/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, damage-retaliate soak.
const GUARD_DOG: &str = "01021";
/// Flashlight (01087): neutral Tool, `[action] Spend 1 supply: **Investigate.**`
/// **Investigate** is a bold action designator, but not one on the
/// attack-of-opportunity exempt list — so it provokes (#696).
const FLASHLIGHT: &str = "01087";
/// Beat Cop (01018): Guardian Ally; ability 1 is `[fast] Discard Beat Cop: Deal
/// 1 damage to an enemy at your location` — fast, so never provokes.
const BEAT_COP: &str = "01018";
/// Dodge (01023): Neutral Tactic, Fast, before-attack cancel reaction.
const DODGE: &str = "01023";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);
const ATTACKER: EnemyId = EnemyId(7);

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// A ready enemy dealing `damage` / 0 horror, with 5 health. Engage it with
/// `with_enemy_engaged`, which places it at the investigator's location.
fn ready_attacker(damage: u8) -> Enemy {
    let mut e = test_support::test_enemy(ATTACKER.0, "Attacker");
    e.attack_damage = damage;
    e.attack_horror = 0;
    e.max_health = 5;
    e
}

/// `investigator`'s turn at the Study, engaged by `attacker`, against a
/// single-`0` chaos bag (for a skill test the activation starts).
fn engaged_session(investigator: Seat, attacker: Enemy) -> TestSession {
    GameStateBuilder::new()
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .open_turn(INV)
        .with_enemy_engaged(attacker, INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

/// Activate printed ability `index` of the card `code` the investigator has in
/// play.
fn activate(session: TestSession, code: &str, index: u8) -> TestSession {
    let source = AbilitySource::InPlay(session.state().in_play(INV, code));
    session.take(&TurnAction::ActivateAbility {
        investigator: INV,
        source,
        address: AbilityAddress::Printed(index),
    })
}

/// The card `code` the investigator has in play.
fn card_in_play<'s>(session: &'s TestSession, code: &str) -> &'s CardInPlay {
    let id = session.state().in_play(INV, code);
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == id)
        .unwrap_or_else(|| panic!("{code} still in play"))
}

// -----------------------------------------------------------------------
// A non-fight action ability provokes an AoO (the new #361 firing site).
// -----------------------------------------------------------------------

/// Activating First Aid (a non-fight `[action]` ability) while engaged with a
/// ready enemy provokes an `AoO`. With Guard Dog in play and no Dodge, the `AoO`
/// damage soaks onto Guard Dog and its reaction window opens — the same
/// suspend/resume path Move uses (K1), proving `activate_ability` now routes
/// through `drive_aoo`.
#[test]
fn activating_a_non_fight_ability_while_engaged_provokes_an_aoo() {
    // First Aid in play (3 supplies) + Guard Dog in play (the soaker). The
    // investigator carries damage so First Aid's heal has something to remove
    // — without it the #639 initiation gate refuses the activation outright and
    // the `AoO` under test never fires.
    let session = engaged_session(
        seat(1)
            .at(HERE)
            .in_play([GUARD_DOG])
            .in_play([(FIRST_AID, uses(UseKind::Supplies, 3))])
            .harm(1, 0),
        ready_attacker(2),
    );
    let dog = session.state().card(INV, GUARD_DOG);

    // Activate First Aid (ability 0). Action-cost, non-fight → provokes an AoO
    // after the supply cost is paid and before the heal effect resolves.
    // The AoO provokes a soak distribution prompt (Guard Dog has capacity, #44/
    // K5b): assign both AoO damage points onto Guard Dog to reproduce the soak.
    let session = activate(session, FIRST_AID, 0).pick(dog.clone()).pick(dog);
    let state = session.state();

    assert!(
        session.prompt().skippable,
        "the AoO's damage window must suspend the activation: {:?}",
        session.prompt()
    );
    let dog_in_play = card_in_play(&session, GUARD_DOG);
    // The AoO's damage is *assigned* to Guard Dog and not yet placed: the open
    // window is Guard Dog's own `when` cell, which the Rules Reference puts
    // between assigning and placing (#727). What this test is about is that the
    // AoO suspended the activation at all, and it does so one step earlier now.
    assert_eq!(
        dog_in_play.accumulated_damage, 0,
        "AoO damage from the activation is assigned to Guard Dog, not yet placed"
    );
    assert_eq!(
        state.investigators[&INV].damage(),
        1,
        "investigator took no AoO damage (soaked onto Guard Dog) — the 1 damage \
         is the seeded harm that makes First Aid activatable at all"
    );
    // First Aid spent its supply (the cost paid before the AoO).
    let kit_in_play = card_in_play(&session, FIRST_AID);
    assert_eq!(
        kit_in_play.uses.get(&UseKind::Supplies).copied(),
        Some(2),
        "First Aid spent 1 supply as the activation cost, before the AoO"
    );
    // The heal effect has NOT run yet — it resolves only after the AoO window closes.
}

// -----------------------------------------------------------------------
// A Fight ability is AoO-exempt (RR p.5).
// -----------------------------------------------------------------------

/// Activating Machete (a `[action]: Fight` ability) while engaged with a ready
/// enemy provokes **no** `AoO` — Fight is on the exempt list. The activation goes
/// straight to the Fight skill test (its commit window), and Guard Dog (in play)
/// soaks nothing.
#[test]
fn activating_a_fight_ability_while_engaged_provokes_no_aoo() {
    // The Fight starts a Combat skill test, which draws from the bag.
    let session = activate(
        engaged_session(
            seat(1).at(HERE).in_play([GUARD_DOG, MACHETE]),
            ready_attacker(2),
        ),
        MACHETE,
        0,
    );
    let state = session.state();

    // Guard Dog soaked nothing; the investigator took no AoO damage.
    assert_eq!(
        card_in_play(&session, GUARD_DOG).accumulated_damage,
        0,
        "no AoO soaked onto Guard Dog"
    );
    assert_eq!(
        state.investigators[&INV].damage(),
        0,
        "no AoO damage to the investigator"
    );
    // The activation went straight to the Fight skill test (no AoO first).
    assert_event!(session.events(), Event::SkillTestStarted { .. });
}

// -----------------------------------------------------------------------
// A fast ability is not an action and never provokes (RR p.11).
// -----------------------------------------------------------------------

/// Activating Beat Cop's `[fast]` ability (`action_cost == 0`) while engaged
/// with a ready enemy provokes **no** `AoO` — fast is not an action. The ability
/// resolves (deals 1 damage to the enemy) without the investigator taking any
/// `AoO` damage.
#[test]
fn activating_a_fast_ability_while_engaged_provokes_no_aoo() {
    // Beat Cop ability 1 is the `[fast]` deal-1-damage (ability 0 is its
    // constant +1 combat).
    let session = activate(
        engaged_session(seat(1).at(HERE).in_play([BEAT_COP]), ready_attacker(2)),
        BEAT_COP,
        1,
    );
    session.assert_at_turn_menu(INV);

    // No AoO fired: outcome-independent proof. An AoO would deal the attacker's
    // 2 damage to the investigator, surfacing as a `DamageTaken`/`HorrorTaken`
    // event for `INV` (Beat Cop's own effect damages the *enemy*, not the
    // investigator). None firing ⇒ no attack of opportunity.
    assert_no_event!(
        session.events(),
        Event::DamageTaken { investigator, .. } | Event::HorrorTaken { investigator, .. }
            if *investigator == INV
    );
    assert_eq!(
        session.state().investigators[&INV].damage(),
        0,
        "no AoO damage to the investigator"
    );
    // The fast ability's own effect resolved: Beat Cop dealt its 1 damage to
    // the engaged enemy.
    assert_event!(
        session.events(),
        Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == ATTACKER
    );
}

// -----------------------------------------------------------------------
// A designator the exempt list does not name still provokes.
// -----------------------------------------------------------------------

/// Flashlight's `[action] Spend 1 supply: **Investigate.**` while engaged with
/// a ready enemy **provokes**. It prints a bold action designator, but
/// `glossary/Attack_of_Opportunity.md` names only *"to **fight**, to **evade**,
/// or to activate a **parley** or **resign** ability"* — so an
/// **Investigate** ability provokes exactly as the basic investigate action
/// does. The corpus half of the designator rule (#696): a declared designator
/// is not by itself an exemption.
#[test]
fn activating_an_investigate_designated_ability_while_engaged_provokes_an_aoo() {
    // The bag is only so the investigation has something to draw from once
    // the AoO has resolved and the parked effect runs.
    let session = activate(
        engaged_session(
            seat(1)
                .at(HERE)
                .in_play([(FLASHLIGHT, uses(UseKind::Supplies, 3))]),
            ready_attacker(2),
        ),
        FLASHLIGHT,
        0,
    );

    // No soaker is in play, so the AoO's 2 damage lands on the investigator
    // directly — before the investigation the ability parked behind it.
    assert_eq!(
        session.state().investigators[&INV].damage(),
        2,
        "the **Investigate** ability provoked an AoO: {:?}",
        session.events()
    );
    // The supply was spent first: an AoO is made "immediately after all costs of
    // initiating the action that provoked the attack have been paid".
    assert_eq!(
        card_in_play(&session, FLASHLIGHT)
            .uses
            .get(&UseKind::Supplies)
            .copied(),
        Some(2),
        "Flashlight spent 1 supply as the activation cost, before the AoO"
    );
}

// -----------------------------------------------------------------------
// The activation's AoO can be cancelled, then the ability's effect resumes.
// -----------------------------------------------------------------------

/// Dodge cancels the `AoO` provoked by activating First Aid; the activation then
/// resumes and runs First Aid's heal effect (which itself suspends on its
/// damage-or-horror choice). Proves `resume_activate_ability` runs the parked
/// effect after the `AoO` window closes.
#[test]
fn dodge_cancels_the_activations_aoo_then_the_ability_effect_resumes() {
    // Something for First Aid to heal — *both* harms, so both of its modes stay
    // live and the damage-or-horror choice still prompts: #664 filters a mode
    // with nothing to heal out of the offer, and a lone live mode would
    // auto-resolve before this test could see it.
    let session = engaged_session(
        seat(1)
            .at(HERE)
            .harm(2, 2)
            .hand([DODGE])
            .in_play([(FIRST_AID, uses(UseKind::Supplies, 3))]),
        ready_attacker(2),
    );

    // Activate First Aid → AoO → Dodge is in hand, so the BeforeEnemyAttack
    // cancel window opens.
    // Play Dodge (the single candidate) → cancel the AoO.
    let session = activate(session, FIRST_AID, 0).pick(OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(DODGE),
    });

    // The AoO was cancelled — no damage — and the activation resumed into First
    // Aid's heal choice (the effect ran after the window closed).
    assert_eq!(
        session.state().investigators[&INV].damage(),
        2,
        "the cancelled AoO dealt no damage"
    );
    assert_eq!(
        session.prompt().nature,
        PromptNature::Decision,
        "First Aid's heal choice opened — the parked effect resumed: {:?}",
        session.prompt()
    );

    // Pick the damage branch → 1 damage healed (2 → 1), proving the resumed
    // effect actually resolves.
    let session = session.pick_nth(0);
    let state = session.state();
    assert_eq!(
        state.investigators[&INV].damage(),
        1,
        "First Aid healed 1 damage after the AoO was dodged and the effect resumed"
    );
    assert!(
        state.open_windows().is_empty(),
        "no windows stranded after the dodge + resume + heal cycle: {:?}",
        state.open_windows()
    );
}

// -----------------------------------------------------------------------
// An AoO that defeats the actor mid-activation suppresses the ability effect.
// -----------------------------------------------------------------------

/// If the activation's `AoO` defeats the investigator, the §D re-validation gate
/// suppresses the parked ability effect: First Aid's heal never runs (no heal
/// choice opens), though the supply spent as the activation cost stays spent.
#[test]
fn aoo_that_defeats_the_actor_suppresses_the_ability_effect() {
    // A lethal AoO and no soaker / no Dodge → the actor is defeated before the
    // heal effect can resume.
    let session = activate(
        engaged_session(
            seat(1)
                .at(HERE)
                .harm(2, 0)
                .in_play([(FIRST_AID, uses(UseKind::Supplies, 3))]),
            ready_attacker(50),
        ),
        FIRST_AID,
        0,
    );

    // The heal effect was suppressed by the §D re-validation gate: outcome-
    // independent proof. Had First Aid's heal run, a `Healed` event would have
    // fired and the actor's damage would be 1 lower. Neither happens ⇒ the
    // parked effect was aborted.
    assert_no_event!(session.events(), Event::Healed { .. });
    let state = session.state();
    // Damage is the full lethal AoO (2 initial + 50 from the attacker), NOT
    // reduced by 1 — the heal never subtracted from it.
    assert_eq!(
        state.investigators[&INV].damage(),
        52,
        "First Aid's heal was suppressed: the AoO's full 50 damage stands, \
         un-reduced by the would-be 1-point heal"
    );
    // The actor was defeated by the AoO.
    assert_ne!(
        state.investigators[&INV].status,
        Status::Active,
        "the lethal AoO defeated the actor"
    );
}
