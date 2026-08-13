//! A card that has commenced being played and is not yet placed rides the frame
//! driving its play (#604, #565 — see
//! `docs/adr/0002-in-progress-play-lives-on-its-frame.md`).
//!
//! Rules Reference **Appendix I: Initiation Sequence**, steps 3–4:
//!
//! > 3. The card commences being played, or the effects of the ability attempt
//! >    to initiate.
//! > 4. The effects of the ability (if not canceled in step 3) complete their
//! >    initiation, and resolve. The card is regarded as played (**and placed in
//! >    play, or in its owner's discard pile if it's an event**), and the ability
//! >    is considered resolved simultaneously with the completion of this step.
//!
//! Between those two points the card is in no zone at all, and **plays nest**: a
//! non-fast play provokes an attack of opportunity while it is itself mid-play,
//! and a Fast event played to cancel that attack is a second play running inside
//! the first. Every test here drives that interleaving through the public
//! [`apply`] API with the real corpus installed.
//!
//! ## Verified card text (`ArkhamDB`, 2026-08-13)
//!
//! **Dynamite Blast 01024** (Event, cost 5, not Fast): "Choose either your
//! location or a connecting location. Deal 3 damage to each enemy and to each
//! investigator at the chosen location." FAQ: "When you play Dynamite Blast while
//! engaged with enemies, first you spend an action and pay the cost, then each
//! engaged enemy makes an attack of opportunity against you, and then the effects
//! of the card resolve -- but only if you're still alive."
//!
//! **Dodge 01023** (Event, cost 1, Fast): "Fast. Play when an enemy attacks an
//! investigator at your location. Cancel that attack." FAQ: "Dodge can cancel any
//! type of enemy attack: a normal attack during the Enemy phase, **an attack of
//! opportunity**, or a Retaliate attack."
//!
//! **Machete 01020** (Asset, cost 3, Hand slot, not Fast) and **Knife 01086**
//! (Asset, cost 1, Hand slot, not Fast) are the two-asset hand used for the
//! stale-index cases; neither has an `OnPlay` ability, so the play's only
//! observable is where the card lands.

use game_core::engine::EngineOutcome;
use game_core::state::{CardCode, EnemyId, InvestigatorId, LocationId, Phase, Status};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{apply, Action, InputResponse, OptionId, PlayerAction, TurnAction};

const DYNAMITE: &str = "01024";
const DODGE: &str = "01023";
const MACHETE: &str = "01020";
const KNIFE: &str = "01086";
const INV: InvestigatorId = InvestigatorId(1);
const ATTACKER: EnemyId = EnemyId(100);
const LOC_A: LocationId = LocationId(10);
const LOC_B: LocationId = LocationId(11);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

fn pick(state: game_core::GameState, option: u32) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(option)),
        }),
    )
}

/// The controller at `LOC_A` (connected to `LOC_B`) holding `hand`, with
/// `resources` to spend, engaged by one ready enemy — so a non-fast play provokes
/// an attack of opportunity. The attacker survives a Dynamite Blast (9 health) and
/// deals 2 damage, so nothing but the play under test is in flight.
fn board(hand: &[&str], resources: u8) -> game_core::GameState {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LOC_A);
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    inv.resources = resources;
    // "Skids" O'Toole (01003, 8 health / 6 sanity): a real code so `max_health()`
    // reads from the installed registry.
    inv.investigator_card.code = CardCode::new("01003");

    let mut loc_a = test_location(10, "Cellar");
    loc_a.connections = vec![LOC_B];
    let mut loc_b = test_location(11, "Hallway");
    loc_b.connections = vec![LOC_A];

    let mut attacker = test_enemy(100, "Ghoul");
    attacker.current_location = Some(LOC_A);
    attacker.engaged_with = Some(INV);
    attacker.max_health = 9;
    attacker.attack_damage = 2;
    attacker.attack_horror = 0;

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_enemy(attacker)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build()
}

// -----------------------------------------------------------------------
// #604 — a nested play must not erase the play it nests inside.
// -----------------------------------------------------------------------

/// Dynamite Blast is mid-play when Dodge is played to cancel the attack of
/// opportunity it provoked. Both are events, so both belong in the discard pile
/// once their effects resolve (RR Appendix I step 4).
///
/// Before #604 the two plays shared one global slot: Dodge's play overwrote
/// Dynamite Blast's entry, and Dynamite Blast's own disposal then found the slot
/// empty — the card resolved its effect and ended up in no zone at all, with no
/// panic, rejection, or event to show for it.
#[test]
fn dodging_the_aoo_of_a_non_fast_event_does_not_erase_it() {
    // Play Dynamite Blast (hand_index 0). Non-fast → action spent, cost paid,
    // card commences being played (leaves hand), then the AoO resolves.
    let r = take_turn_action(
        board(&[DYNAMITE, DODGE], 6),
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        },
    );
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "the AoO's before-attack cancel window suspends, offering Dodge: {:?}",
        r.outcome
    );
    assert_eq!(
        r.state.play_in_progress().map(|(_, c)| c.clone()),
        Some(CardCode::new(DYNAMITE)),
        "Dynamite Blast is mid-play (commenced, not yet placed)",
    );

    // Play Dodge from hand to cancel the attack of opportunity. Its own play
    // nests inside Dynamite Blast's, on a frame above it.
    let r = pick(r.state, 0);

    assert_eq!(
        r.state.investigators[&INV].damage(),
        0,
        "the AoO was cancelled by Dodge",
    );

    // Drive the rest of the play: Dynamite Blast's location choice, if it
    // suspended. LOC_A + LOC_B are both candidates.
    let r = if matches!(r.outcome, EngineOutcome::AwaitingInput { .. }) {
        pick(r.state, 0)
    } else {
        r
    };

    let discard = &r.state.investigators[&INV].discard;
    let hand = &r.state.investigators[&INV].hand;
    assert!(
        discard.contains(&CardCode::new(DODGE)),
        "Dodge was played and discarded; discard={discard:?} hand={hand:?}",
    );
    assert!(
        discard.contains(&CardCode::new(DYNAMITE)),
        "RR Appendix I step 4: Dynamite Blast must be placed in its owner's \
         discard pile once its effect resolves. discard={discard:?} hand={hand:?}",
    );
    assert!(
        r.state.play_in_progress().is_none(),
        "both plays completed — no card left mid-play",
    );
}

/// The controller is defeated by the attack of opportunity their own Dynamite
/// Blast provoked, so elimination runs while the event is mid-play (RR Appendix I
/// step 3 → 4).
///
/// RR p.10 (Elimination), step 1: an eliminated investigator's owned cards are
/// removed from the game. A mid-play card is in no zone, so the hand/deck/discard
/// drain cannot reach it — elimination sweeps it off the frame carrying it
/// instead. Before that sweep the card was flushed into the *already drained*
/// discard pile of a `Killed` investigator.
#[test]
fn defeated_by_its_own_aoo_the_mid_play_event_is_removed_not_discarded() {
    let mut state = board(&[DYNAMITE], 6); // no Dodge — the AoO must land
    state
        .enemies
        .get_mut(&ATTACKER)
        .expect("attacker present")
        .attack_damage = 8; // Skids O'Toole is 8 health

    let r = take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        },
    );

    let inv = &r.state.investigators[&INV];
    assert_ne!(
        inv.status,
        Status::Active,
        "the AoO defeated the controller"
    );
    assert!(
        inv.removed_from_game.contains(&CardCode::new(DYNAMITE)),
        "an eliminated investigator's cards are removed from the game; \
         removed={:?} discard={:?}",
        inv.removed_from_game,
        inv.discard,
    );
    assert_eq!(
        inv.removed_from_game
            .iter()
            .filter(|c| **c == CardCode::new(DYNAMITE))
            .count(),
        1,
        "removed exactly once — elimination's sweep and the suppressed resume's \
         own placement must not both fire",
    );
    assert!(
        !inv.discard.contains(&CardCode::new(DYNAMITE)),
        "nothing may be placed in the drained discard pile of a dead investigator",
    );
    assert!(
        r.state.play_in_progress().is_none(),
        "the sweep took the card off its frame",
    );
}

// -----------------------------------------------------------------------
// #565 — a play parked across the AoO must survive a hand-shifting reaction.
// -----------------------------------------------------------------------

/// A non-fast **asset** play parks across the attack-of-opportunity loop, and the
/// Dodge played in that loop leaves hand from under it. The asset that enters
/// play must be the one that was announced.
///
/// Before #565 the parked play carried a raw `hand_index`: with hand
/// `[Dodge, Machete, Knife]`, Machete is announced at index 1, Dodge's play
/// shifts the hand to `[Machete, Knife]`, and disposal took index 1 — putting
/// **Knife** into play while Machete stayed in hand, with `CardPlayed`, the cost
/// payment, and the slot check all naming Machete.
#[test]
fn a_hand_shifting_reaction_does_not_swap_the_asset_that_enters_play() {
    // Machete 3 + Dodge 1 = 4 resources.
    let r = take_turn_action(
        board(&[DODGE, MACHETE, KNIFE], 4),
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 1,
        },
    );
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "the AoO's before-attack cancel window suspends, offering Dodge: {:?}",
        r.outcome
    );
    assert_eq!(
        r.state.play_in_progress().map(|(_, c)| c.clone()),
        Some(CardCode::new(MACHETE)),
        "the announced asset rides its frame across the suspension",
    );

    // Dodge the attack of opportunity — this is what shifts the hand.
    let r = pick(r.state, 0);
    assert_eq!(
        r.state.investigators[&INV].damage(),
        0,
        "the AoO was cancelled by Dodge",
    );

    let inv = &r.state.investigators[&INV];
    let in_play: Vec<&str> = inv.cards_in_play.iter().map(|c| c.code.as_str()).collect();
    assert_eq!(
        in_play,
        vec![MACHETE],
        "the announced asset entered play; hand={:?}",
        inv.hand,
    );
    assert_eq!(
        inv.hand,
        vec![CardCode::new(KNIFE)],
        "the unplayed card stayed in hand",
    );
    assert_eq!(inv.discard, vec![CardCode::new(DODGE)], "Dodge discarded");
}

/// The same shift, with nothing left in hand behind the reaction: hand
/// `[Dodge, Machete]`, Machete announced at index 1, Dodge played mid-`AoO`
/// leaves a one-card hand. The stale index then addressed slot 1 of a 1-element
/// `Vec` — a `Vec::remove` panic, which `apply`'s Rejected-only rollback does not
/// cover.
#[test]
fn a_hand_shifting_reaction_does_not_panic_on_a_short_hand() {
    let r = take_turn_action(
        board(&[DODGE, MACHETE], 4),
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 1,
        },
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));

    let r = pick(r.state, 0);

    let inv = &r.state.investigators[&INV];
    assert!(
        inv.cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MACHETE)),
        "Machete entered play from its frame, with no hand slot to address",
    );
    assert!(inv.hand.is_empty(), "hand={:?}", inv.hand);
}
