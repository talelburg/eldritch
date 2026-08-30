//! #771 integration: a **set-aside asset** enters play *at* a location, under
//! no investigator's control, against the real `cards::REGISTRY`.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-08-30)
//!
//! **The Barrier (01109)**, `back_text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > The barrier blocking passage into the parlor has vanished. Reveal the
//! > Parlor.
//! > Put the set-aside Lita Chantler into play in the Parlor.
//! > Spawn the set-aside Ghoul Priest in the Hallway.
//!
//! **Lita Chantler (01117)**, `text` verbatim: an `Ally` asset, health 3 /
//! sanity 3, whose whole printed body is a grant —
//!
//! > While you control Lita Chantler, she gains:
//! > "Each investigator at your location gets +1 \[combat\].
//! > \[reaction\] When an investigator at your location successfully attacks a
//! > \[\[Monster\]\] enemy: That investigator deals +1 damage."
//!
//! ## The zone
//!
//! `data/rules-reference/rules/glossary/In_Play_and_Out_of_Play.md`: *"The
//! current act, the current agenda, each location in the play area, and each
//! encounter card in a investigator's threat area **or at a location**, are all
//! considered in play."* — so a card put into play in the Parlor is in play
//! with no controller, and belongs in the location's own `cards_at_location`
//! zone rather than in anybody's `cards_in_play`.
//!
//! ## Why she cannot soak
//!
//! `data/rules-reference/rules/glossary/Asset_Cards.md`: *"When an investigator
//! is dealt damage or horror, that investigator may assign some or all of that
//! damage or horror to eligible asset cards **he or she controls**"* —
//! uncontrolled is not eligible, so her printed 3/3 soak capacity is simply not
//! offered while nobody controls her. Asserted below through the real
//! distribution path.
//!
//! ## What is *not* here
//!
//! Lita's **Parley** — the ability the Parlor grants her once she is on the
//! board — is #772's, and lives in `lita_parley.rs`. This file drives
//! `put_set_aside_card_into_play` directly for the entry path, and asserts
//! 01109b's own contract — printed order, and preconditions checked up front —
//! separately.
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::action::EngineRecord;
use game_core::card_registry;
use game_core::event::Event;
use game_core::state::{
    Act, CardCode, ChaosToken, GameState, InvestigatorId, LocationId, Phase, SkillKind,
};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{
    put_set_aside_card_into_play, Action, Cx, EngineOutcome, EvalContext, ModifiedQuantity,
    ModifierTarget, ReadContext,
};

/// Lita Chantler — the set-aside `Ally` asset act 01109b puts into play.
const LITA: &str = "01117";
/// The Ghoul Priest — the set-aside enemy the same reverse spawns.
const GHOUL_PRIEST: &str = "01116";
/// The Hallway — where the Priest spawns.
const HALLWAY: &str = "01112";
/// The Parlor — where Lita is put into play.
const PARLOR: &str = "01115";

const HALLWAY_ID: LocationId = LocationId(1);
const PARLOR_ID: LocationId = LocationId(2);
const INV: InvestigatorId = InvestigatorId(1);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(cards::REGISTRY);
}

/// The act-2 board: an investigator in the Hallway, the Parlor in play but
/// unrevealed (the barrier is still up), and both set-aside cards in the zone.
/// Act 01109 is current, with a successor so its advance is non-terminal.
fn board() -> GameState {
    let mut hallway = test_location(1, "Hallway");
    hallway.code = CardCode::new(HALLWAY);
    let mut parlor = test_location(2, "Parlor");
    parlor.code = CardCode::new(PARLOR);
    parlor.revealed = false;

    let mut inv = test_investigator(1);
    // A real investigator code, so `max_health()` reads from the installed
    // corpus. Skids O'Toole (01003, 8/6).
    inv.investigator_card.code = CardCode::new("01003");
    inv.current_location = Some(HALLWAY_ID);

    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator(inv)
        .with_location(hallway)
        .with_location(parlor)
        .build();
    state.set_aside_cards = vec![CardCode::new(GHOUL_PRIEST), CardCode::new(LITA)];
    state.act_deck = vec![
        Act {
            code: CardCode::new("01109"),
            clue_threshold: 3,
        },
        Act {
            code: CardCode::new("01110"),
            clue_threshold: 0,
        },
    ];
    state.act_index = 0;
    state
}

/// Run `f` against a `Cx` over `state`, returning the outcome and the events it
/// pushed. The natives under test are `NativeEffectFn`s, which take a `Cx`.
fn with_cx(
    state: &mut GameState,
    f: impl FnOnce(&mut Cx) -> EngineOutcome,
) -> (EngineOutcome, Vec<Event>) {
    let mut events = Vec::new();
    let outcome = f(&mut Cx {
        state,
        events: &mut events,
    });
    (outcome, events)
}

/// 01109's reverse, as the registry resolves it for the forced dispatch.
fn reverse(state: &mut GameState) -> (EngineOutcome, Vec<Event>) {
    let reg = card_registry::current().expect("registry installed");
    let native =
        (reg.native_effect_for)("01109:reverse").expect("01109:reverse is registered by 01109");
    let ctx = EvalContext::for_controller(INV);
    with_cx(state, |cx| native(cx, &ctx))
}

// ---- the entry path --------------------------------------------------

/// *"Put the set-aside Lita Chantler into play in the Parlor."* She leaves the
/// set-aside zone, lands in the Parlor's `cards_at_location`, and gets a minted
/// instance id announced on the event stream.
#[test]
fn a_set_aside_asset_enters_play_at_the_named_location() {
    let mut state = board();
    let (outcome, events) = with_cx(&mut state, |cx| {
        put_set_aside_card_into_play(cx, LITA, Some(PARLOR))
    });

    assert_eq!(outcome, EngineOutcome::Done, "the asset entered play");
    assert_eq!(
        state.set_aside_cards,
        vec![CardCode::new(GHOUL_PRIEST)],
        "Lita left the set-aside zone; the Priest is untouched",
    );
    let at_parlor = &state.locations[&PARLOR_ID].cards_at_location;
    assert_eq!(at_parlor.len(), 1, "one card at the Parlor");
    assert_eq!(at_parlor[0].code.as_str(), LITA);
    assert_eq!(
        at_parlor[0].accumulated_damage, 0,
        "she enters play undamaged",
    );
    let instance_id = at_parlor[0].instance_id;
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::CardPutIntoPlayAtLocation { location, code, instance_id: i }
                if *location == PARLOR_ID && code.as_str() == LITA && *i == instance_id
        )),
        "the entry is announced, got {events:?}",
    );
}

/// She is in play but under **nobody's** control: not in any investigator's
/// `cards_in_play`, and not reachable through `controlled_card_instances`,
/// which is the iterator every "cards at your seat" walk shares.
#[test]
fn a_card_put_into_play_at_a_location_is_controlled_by_no_investigator() {
    let mut state = board();
    let (outcome, _) = with_cx(&mut state, |cx| {
        put_set_aside_card_into_play(cx, LITA, Some(PARLOR))
    });
    assert_eq!(outcome, EngineOutcome::Done);

    for inv in state.investigators.values() {
        assert!(
            inv.cards_in_play.is_empty(),
            "she is in nobody's play area, got {:?}",
            inv.cards_in_play,
        );
        assert!(
            !inv.controlled_card_instances()
                .any(|c| c.code.as_str() == LITA),
            "she is in no investigator's controlled instances",
        );
    }
}

/// An asset with no location named has nowhere to be put into play, and the
/// call rejects without touching the zone (validate-first).
#[test]
fn a_set_aside_asset_without_a_location_is_rejected() {
    let mut state = board();
    let (outcome, events) = with_cx(&mut state, |cx| {
        put_set_aside_card_into_play(cx, LITA, None)
    });

    assert!(
        matches!(&outcome, EngineOutcome::Rejected { reason } if reason.contains("needs a location")),
        "an asset with no target location must reject, got {outcome:?}",
    );
    assert!(
        state.set_aside_cards.contains(&CardCode::new(LITA)),
        "she stays set aside when the entry rejects",
    );
    assert!(events.is_empty(), "no event on reject, got {events:?}");
}

/// A location that isn't in play is refused the same way an enemy's spawn
/// location is, and again mutates nothing.
#[test]
fn a_set_aside_asset_named_at_a_location_not_in_play_is_rejected() {
    let mut state = board();
    let (outcome, _) = with_cx(&mut state, |cx| {
        put_set_aside_card_into_play(cx, LITA, Some("01113")) // the Attic, still set aside
    });

    assert!(
        matches!(&outcome, EngineOutcome::Rejected { reason } if reason.contains("not in play")),
        "a location that isn't in play must reject, got {outcome:?}",
    );
    assert!(
        state.set_aside_cards.contains(&CardCode::new(LITA)),
        "she stays set aside when the entry rejects",
    );
    assert!(
        state.locations[&PARLOR_ID].cards_at_location.is_empty(),
        "no card was put into play anywhere",
    );
}

// ---- the acceptance clauses ------------------------------------------

/// `glossary/Asset_Cards.md` limits assignment to assets an investigator
/// *"controls"*, so an uncontrolled Lita is not an eligible soaker even for an
/// investigator standing on top of her: Grasping Hands' 2 damage is not
/// contested, so no per-point prompt opens and every point lands on the
/// investigator.
#[test]
fn an_uncontrolled_asset_cannot_soak_damage_for_a_colocated_investigator() {
    let mut state = board();
    // Put her into play where the investigator is standing — the strongest
    // form of the claim: co-location is not control.
    let (outcome, _) = with_cx(&mut state, |cx| {
        put_set_aside_card_into_play(cx, LITA, Some(HALLWAY))
    });
    assert_eq!(outcome, EngineOutcome::Done);

    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage.
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(-2)];
    state.encounter_deck.push_back(CardCode::new("01162")); // Grasping Hands
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    // No pick_single is scripted: a contested point would prompt, and the drive
    // would fail for want of a response.
    let result = drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: INV }),
        resolver,
    );

    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "no distribution prompt opened — nothing was eligible to soak",
    );
    assert_eq!(
        result.state.investigators[&INV].damage(),
        2,
        "both points landed on the investigator",
    );
    assert_eq!(
        result.state.locations[&HALLWAY_ID].cards_at_location[0].accumulated_damage, 0,
        "an uncontrolled asset takes none of it",
    );
}

/// She is nonetheless *in play* and swept: her location-scoped grant is not
/// implemented until #772, but the collection she sits in is read by the
/// modifier query, which is what makes the zone load-bearing rather than
/// decorative. Read here as the printed baseline — no modifier reaches the
/// investigator's combat through her yet.
#[test]
fn a_card_at_a_location_is_swept_by_the_modifier_query() {
    let mut state = board();
    let (outcome, _) = with_cx(&mut state, |cx| {
        put_set_aside_card_into_play(cx, LITA, Some(HALLWAY))
    });
    assert_eq!(outcome, EngineOutcome::Done);

    let combat = game_core::modified_value(
        &state,
        card_registry::current(),
        ModifierTarget::Investigator(INV),
        ModifiedQuantity::Skill(SkillKind::Combat),
        ReadContext::OutsideTest,
    );
    assert_eq!(
        combat.contributions.len(),
        0,
        "01117's buffs are a grant conditioned on control (#772), so nothing \
         contributes yet; the sweep merely does not choke on her",
    );
}

// ---- 01109b's reverse ------------------------------------------------

/// The reverse resolves **in the order the card prints**: reveal the Parlor,
/// then spawn the Priest in the Hallway. Read off the event stream, which is
/// the only place the order is observable.
#[test]
fn the_reverse_resolves_in_printed_order() {
    let mut state = board();
    let (outcome, events) = reverse(&mut state);
    assert_eq!(outcome, EngineOutcome::Done, "got {outcome:?}");

    let reveal = events
        .iter()
        .position(
            |e| matches!(e, Event::LocationRevealed { location, .. } if *location == PARLOR_ID),
        )
        .expect("the Parlor was revealed");
    let spawn = events
        .iter()
        .position(|e| matches!(e, Event::EnemySpawned { .. }))
        .expect("the Ghoul Priest spawned");
    assert!(
        reveal < spawn,
        "printed order is reveal-then-spawn; events were {events:?}",
    );
    assert!(state.locations[&PARLOR_ID].revealed);
    assert!(state
        .enemies
        .values()
        .any(|e| e.code.as_str() == GHOUL_PRIEST));
}

/// Line 2 — *"Put the set-aside Lita Chantler into play in the Parlor."* — puts
/// her into play there under nobody's control (#772). She waited on the
/// granting hook: until the Parlor's *"While Lita Chantler is not controlled by
/// a player, she gains: \[action\] **Parley.** …"* existed she would have
/// arrived inert, which is a worse board than not having her.
#[test]
fn the_reverse_puts_lita_into_play_in_the_parlor() {
    let mut state = board();
    let (outcome, _) = reverse(&mut state);
    assert_eq!(outcome, EngineOutcome::Done);

    assert!(
        state.set_aside_cards.is_empty(),
        "both set-aside cards left the zone, got {:?}",
        state.set_aside_cards,
    );
    let at_parlor = &state.locations[&PARLOR_ID].cards_at_location;
    assert_eq!(at_parlor.len(), 1, "one card at the Parlor");
    assert_eq!(at_parlor[0].code.as_str(), LITA);
    assert_eq!(
        at_parlor[0].owner, None,
        "she is scenario-owned; the reverse gives nobody control of her",
    );
}

/// The reverse checks **Lita's** precondition up front too: a board where she
/// is no longer set aside rejects before the reveal, rather than half-resolving.
#[test]
fn the_reverse_rejects_when_lita_is_not_set_aside() {
    let mut state = board();
    state.set_aside_cards.retain(|c| c.as_str() != LITA);

    let (outcome, events) = reverse(&mut state);
    assert!(
        matches!(&outcome, EngineOutcome::Rejected { reason } if reason.contains("Lita")),
        "a missing Lita must reject, got {outcome:?}",
    );
    assert!(
        !state.locations[&PARLOR_ID].revealed,
        "the Parlor must not have been revealed by a reverse that cannot finish",
    );
    assert!(events.is_empty(), "no event on reject, got {events:?}");
}

/// The preconditions are checked **up front**, not implied by the ordering:
/// with the Hallway not in play the spawn cannot happen, and the reveal that
/// now comes first must not have run. Without the up-front check this leaves
/// the barrier lifted (#774 makes `revealed` its gate) on a board the reverse
/// never finished.
#[test]
fn the_reverse_checks_its_preconditions_before_revealing() {
    let mut state = board();
    state.locations.remove(&HALLWAY_ID);

    let (outcome, events) = reverse(&mut state);
    assert!(
        matches!(&outcome, EngineOutcome::Rejected { reason } if reason.contains("Hallway")),
        "a missing Hallway must reject, got {outcome:?}",
    );
    assert!(
        !state.locations[&PARLOR_ID].revealed,
        "the Parlor must not have been revealed by a reverse that cannot finish",
    );
    assert!(events.is_empty(), "no event on reject, got {events:?}");
    assert_eq!(
        state.set_aside_cards.len(),
        2,
        "nothing left the set-aside zone",
    );
}

/// The same, from the other precondition: a Priest that is not set aside (a
/// resumed campaign state, or a second advance) rejects before the reveal.
#[test]
fn the_reverse_rejects_when_the_priest_is_not_set_aside() {
    let mut state = board();
    state.set_aside_cards.retain(|c| c.as_str() != GHOUL_PRIEST);

    let (outcome, _) = reverse(&mut state);
    assert!(
        matches!(&outcome, EngineOutcome::Rejected { reason } if reason.contains("not set aside")),
        "a Priest that is not set aside must reject, got {outcome:?}",
    );
    assert!(
        !state.locations[&PARLOR_ID].revealed,
        "the Parlor must not have been revealed",
    );
}
