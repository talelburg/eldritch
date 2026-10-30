//! Ward of Protection 01065 end-to-end against the real card registry.
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Fast. Play when you draw a non-weakness treachery card.
//! > Cancel that card's revelation effect. Then, take 1 horror.
//!
//! No rulings: `data/arkhamdb-faq/no-rulings.txt` lists 01065.
//!
//! `glossary/Cancel.md`: *"If the effects of a treachery card are canceled, the
//! card is still regarded as having been drawn, and it is still placed in the
//! encounter discard pile."*
//!
//! The encounter cards drawn, with their text
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//! - Ancient Evils 01166: *"**Revelation** - Place 1 doom on the current
//!   agenda. This effect can cause the current agenda to advance."*
//! - Frozen in Fear 01164: *"**Revelation** - Put Frozen in Fear into play in
//!   your threat area. …"*
//! - Swarm of Rats 01159, an enemy: *"Hunter."*
//!
//! Ward's narrowing to a non-weakness card is reached by a weakness treachery
//! drawn from the player deck, which resolves through the same draw condition:
//! Cover Up 01007, *"**Revelation** - Put Cover Up into play in your
//! threat area, with 3 clues on it. …"*
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    CardCode, EnemyId, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Phase,
};
use game_core::test_support::{enemy, location, seat, BoardLookup as _, Either, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

const WARD: &str = "01065";
/// Agnes Baker: *"\[reaction\] After 1 or more horror is placed on Agnes Baker:
/// Deal 1 damage to an enemy at your location. (Limit once per phase.)"*
const AGNES: &str = "01004";
/// Heirloom of Hyperborea: *"\[reaction\] After you play a [[Spell]] card: Draw
/// 1 card."*
const HEIRLOOM: &str = "01012";
/// Drawn to the Flame: *"Draw the top card of the encounter deck. Then,
/// discover 2 clues at your location."*
const DRAWN_TO_THE_FLAME: &str = "01064";
const ANCIENT_EVILS: &str = "01166";
const FROZEN_IN_FEAR: &str = "01164";
const SWARM_OF_RATS: &str = "01159";
const COVER_UP: &str = "01007";
/// Deck filler, for Heirloom's draw.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

/// Investigator `n` at `HERE` with Ward in hand, 5 resources, and a deck.
fn warded(n: u32) -> Seat {
    seat(n)
        .at(HERE)
        .hand([WARD])
        .resources(5)
        .deck([GUTS, GUTS])
}

/// `seats` at `HERE` at Mythos step 1.4, drawing in seat order, with
/// `encounter` on top of the encounter deck in draw order and The Gathering's
/// first agenda current (Ancient Evils places its doom there). `edit` adds to
/// the board. Rests at the first drawer's encounter-draw prompt.
fn mythos_draw_with(
    seats: Vec<impl Into<Seat<Either>>>,
    encounter: &[&str],
    edit: impl FnOnce(GameStateBuilder) -> GameStateBuilder,
) -> TestSession {
    let ids: Vec<InvestigatorId> = (1..=seats.len())
        .map(|n| InvestigatorId(u32::try_from(n).expect("a handful of seats")))
        .collect();
    let builder = seats
        .into_iter()
        .fold(
            GameStateBuilder::new().with_registries(TEST_REGISTRIES),
            GameStateBuilder::with_seat,
        )
        .with_location(location(HERE))
        .with_seating(ids.clone())
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining(ids);
    edit(builder)
        .with_agenda_deck(["01105"])
        .with_encounter_deck(encounter.iter().copied())
        .session()
}

fn mythos_draw(seats: Vec<impl Into<Seat<Either>>>, encounter: &[&str]) -> TestSession {
    mythos_draw_with(seats, encounter, |builder| builder)
}

fn ward(investigator: InvestigatorId) -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator,
        code: CardCode::new(WARD),
    }
}

#[test]
fn drawing_a_treachery_offers_ward_which_cancels_its_revelation_then_deals_1_horror() {
    let drawn = mythos_draw(vec![warded(1)], &[ANCIENT_EVILS]).confirm();
    assert_eq!(drawn.offered_at(&ward(INV)), 1, "Ward is offered");
    assert_eq!(drawn.state().agenda_doom, 0, "the revelation waits on it");

    let s = drawn.pick(ward(INV));
    s.assert_at_turn_menu(INV);
    let state = s.state();
    assert_eq!(state.agenda_doom, 0, "the revelation was cancelled");
    assert_eq!(
        state.encounter_discard,
        vec![CardCode::new(ANCIENT_EVILS)],
        "the cancelled treachery is still discarded",
    );
    let inv = &state.investigators[&INV];
    assert_eq!(inv.horror(), 1, "then, take 1 horror");
    assert_eq!(inv.resources, 4, "Ward costs 1");
    assert_eq!(inv.discard, vec![CardCode::new(WARD)]);
}

#[test]
fn declining_ward_lets_the_revelation_resolve() {
    let s = mythos_draw(vec![warded(1)], &[ANCIENT_EVILS])
        .confirm()
        .skip();

    s.assert_at_turn_menu(INV);
    let state = s.state();
    assert_eq!(state.agenda_doom, 1, "Ancient Evils placed its doom");
    assert_eq!(state.encounter_discard, vec![CardCode::new(ANCIENT_EVILS)]);
    let inv = &state.investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(WARD)], "Ward stays in hand");
    assert_eq!(inv.horror(), 0);
}

/// A cancelled persistent treachery never puts itself into play, so it is
/// discarded like a one-shot one rather than left in no zone.
#[test]
fn a_cancelled_persistent_treachery_is_discarded_not_put_into_play() {
    let s = mythos_draw(vec![warded(1)], &[FROZEN_IN_FEAR])
        .confirm()
        .pick(ward(INV));

    s.assert_at_turn_menu(INV);
    let state = s.state();
    assert!(
        state.investigators[&INV].threat_area.is_empty(),
        "Frozen in Fear is not in the threat area",
    );
    assert_eq!(state.encounter_discard, vec![CardCode::new(FROZEN_IN_FEAR)]);
    assert_no_event!(s.events(), Event::CardEnteredThreatArea { .. });
}

/// *"…a non-weakness **treachery** card"*: an enemy is not one.
#[test]
fn drawing_an_enemy_does_not_offer_ward() {
    let s = mythos_draw(vec![warded(1)], &[SWARM_OF_RATS]).confirm();

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].hand,
        vec![CardCode::new(WARD)],
        "Ward was never played",
    );
    assert_eq!(s.state().enemies.len(), 1, "the enemy spawned");
}

/// *"…a **non-weakness** treachery card"*: Cover Up 01007, a treachery
/// weakness drawn from the player deck, opens the same draw condition as an
/// encounter-deck treachery, and Ward's filter turns it away. Cover Up's
/// *"**Revelation** - Put Cover Up into play in your threat area, with 3 clues
/// on it."* resolves uncancelled.
#[test]
fn drawing_a_weakness_treachery_does_not_offer_ward() {
    let s = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(warded(1).actions(3).deck([COVER_UP, GUTS]))
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(WARD)], "Ward was never played");
    assert!(
        inv.threat_area.iter().any(|c| c.code.as_str() == COVER_UP),
        "its Revelation resolved",
    );
}

/// *"…when **you** draw"*: another investigator's draw does not offer it.
#[test]
fn another_investigators_draw_does_not_offer_ward() {
    let s = mythos_draw(
        vec![seat(1).at(HERE).deck([GUTS]), warded(2)],
        &[ANCIENT_EVILS, ANCIENT_EVILS],
    )
    .confirm();

    // Investigator 1's Ancient Evils resolved with no window, and investigator
    // 2 is now prompted for their own draw.
    s.assert_at_encounter_draw();
    assert_eq!(s.state().agenda_doom, 1);
    assert_eq!(
        s.state().investigators[&INV2].hand,
        vec![CardCode::new(WARD)]
    );
}

/// The cancel names the draw's own condition, so the horror's sequences run
/// normally above it and Agnes's reaction is offered for Ward's horror.
#[test]
fn agnes_reaction_is_offered_for_wards_horror() {
    let agnes = seat(1)
        .as_card(AGNES)
        .at(HERE)
        .hand([WARD])
        .resources(5)
        .deck([GUTS]);
    let s = mythos_draw_with(vec![agnes], &[ANCIENT_EVILS], |builder| {
        // 3 health, so Agnes's 1 damage does not defeat it.
        builder.with_enemy_engaged(enemy(ENEMY).health(3), INV)
    })
    .confirm()
    .pick(ward(INV));

    let agnes_card = s.state().investigators[&INV].card_anchor();
    assert_eq!(s.offered_at(&agnes_card), 1, "her reaction is offered");
    let s = s.pick(agnes_card);

    s.assert_at_turn_menu(INV);
    let state = s.state();
    assert_eq!(state.enemies[&ENEMY].damage, 1, "Agnes dealt 1 damage");
    assert_eq!(state.investigators[&INV].horror(), 1);
    assert_eq!(state.agenda_doom, 0, "the revelation stays cancelled");
    assert_eq!(state.encounter_discard, vec![CardCode::new(ANCIENT_EVILS)]);
}

/// Ward is a Spell played from a reaction window, and that play path announces
/// the completed play too, so Heirloom is offered once Ward has resolved in
/// full — its horror taken and the card in the discard pile.
#[test]
fn heirloom_is_offered_after_ward_resolves() {
    let s = mythos_draw(vec![warded(1).in_play([HEIRLOOM])], &[ANCIENT_EVILS])
        .confirm()
        .pick(ward(INV));

    let heirloom = s.state().card(INV, HEIRLOOM);
    assert_eq!(s.offered_at(&heirloom), 1, "Heirloom is offered");
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.horror(), 1, "Ward's horror is already taken");
    assert_eq!(inv.discard, vec![CardCode::new(WARD)]);

    let s = s.pick(heirloom);
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].hand,
        vec![CardCode::new(GUTS)],
        "Heirloom drew 1 card",
    );
    assert_eq!(s.state().agenda_doom, 0, "the revelation stays cancelled");
}

/// A card effect's draw is a draw too: Drawn to the Flame's encounter card
/// offers Ward, and the play goes on to discover its 2 clues.
#[test]
fn ward_cancels_the_treachery_drawn_by_drawn_to_the_flame() {
    let drawn = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE).clues(3))
        .with_seat(
            seat(1)
                .at(HERE)
                .hand([DRAWN_TO_THE_FLAME, WARD])
                .resources(5),
        )
        .open_turn(INV)
        .with_agenda_deck(["01105"])
        .with_encounter_deck([ANCIENT_EVILS])
        .session()
        .take(&TurnAction::PlayCard {
            investigator: INV,
            source: PlaySource::Hand(0),
        });
    assert_eq!(drawn.offered_at(&ward(INV)), 1, "Ward is offered");
    let s = drawn.pick(ward(INV));

    s.assert_at_turn_menu(INV);
    let state = s.state();
    assert_eq!(state.agenda_doom, 0, "the revelation was cancelled");
    assert_eq!(state.encounter_discard, vec![CardCode::new(ANCIENT_EVILS)]);
    let inv = &state.investigators[&INV];
    assert_eq!(inv.horror(), 1);
    assert_eq!(inv.clues, 2, "Drawn to the Flame still discovers 2 clues");
    assert_event!(
        s.events(),
        Event::CluePlaced { investigator, count: 2 } if *investigator == INV
    );
}
