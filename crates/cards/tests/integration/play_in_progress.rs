//! A card that has commenced being played and is not yet placed rides the frame
//! driving its play (see `docs/adr/0002-in-progress-play-lives-on-its-frame.md`).
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
//! [`apply`] API against the real corpus.
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

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId, Status};
use game_core::test_support::{enemy, location, seat, TestSession};

const DYNAMITE: &str = "01024";
const DODGE: &str = "01023";
const MACHETE: &str = "01020";
const KNIFE: &str = "01086";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const HALLWAY: LocationId = LocationId(11);

fn play(session: TestSession, hand_index: u8) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(hand_index),
    })
}

/// Dodge as the before-attack cancel window offers it.
fn dodge() -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(DODGE),
    }
}

/// The controller's turn at `HERE` (connected to `HALLWAY`), holding `hand`,
/// with `resources` to spend, engaged by one ready enemy — so a non-fast play
/// provokes an attack of opportunity.
fn board(hand: &[&str], resources: u8) -> TestSession {
    board_attacked_for(hand, resources, 2)
}

/// [`board`] with an attacker dealing `attack_damage`. The attacker survives a
/// Dynamite Blast (9 health) and deals no horror, so nothing but the play under
/// test is in flight.
fn board_attacked_for(hand: &[&str], resources: u8, attack_damage: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .hand(hand.iter().copied())
                .resources(resources),
        )
        .with_location(location(HERE).connections([HALLWAY]))
        .with_location(location(HALLWAY).connections([HERE]))
        .with_enemy_engaged(
            enemy(100)
                .health(9)
                .attack_damage(attack_damage)
                .attack_horror(0),
            INV,
        )
        .open_turn(INV)
        .session()
}

// -----------------------------------------------------------------------
// A nested play must not erase the play it nests inside.
// -----------------------------------------------------------------------

/// Regression for #604: a nested play does not erase the play it nests inside.
///
/// Dynamite Blast is mid-play when Dodge is played to cancel the attack of
/// opportunity it provoked. Both are events, so both belong in the discard pile
/// once their effects resolve (RR Appendix I step 4). Each play rides its own
/// frame, so Dodge's play leaves Dynamite Blast's in place, and Dynamite Blast
/// reaches the discard pile rather than ending up in no zone at all.
#[test]
fn dodging_the_aoo_of_a_non_fast_event_does_not_erase_it() {
    // Play Dynamite Blast (hand_index 0). Non-fast → action spent, cost paid,
    // card commences being played (leaves hand), then the AoO resolves.
    let s = play(board(&[DYNAMITE, DODGE], 6), 0);
    assert!(
        s.prompt().skippable,
        "the AoO's before-attack cancel window suspends, offering Dodge: {:?}",
        s.prompt()
    );
    assert_eq!(
        s.state().play_in_progress().map(|(_, c)| c.clone()),
        Some(CardCode::new(DYNAMITE)),
        "Dynamite Blast is mid-play (commenced, not yet placed)",
    );

    // Play Dodge from hand to cancel the attack of opportunity. Its own play
    // nests inside Dynamite Blast's, on a frame above it.
    let s = s.pick(dodge());

    assert_eq!(
        s.state().investigators[&INV].damage(),
        0,
        "the AoO was cancelled by Dodge",
    );

    // Drive the rest of the play: Dynamite Blast's location choice. HERE and
    // HALLWAY are both candidates; blast your own.
    let s = s.pick(OptionTarget::Location(HERE));
    s.assert_at_turn_menu(INV);

    let discard = &s.state().investigators[&INV].discard;
    let hand = &s.state().investigators[&INV].hand;
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
        s.state().play_in_progress().is_none(),
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
/// discard pile of a `Defeated` investigator.
#[test]
fn defeated_by_its_own_aoo_the_mid_play_event_is_removed_not_discarded() {
    // No Dodge — the AoO must land; the test investigator has 8 health.
    let s = play(board_attacked_for(&[DYNAMITE], 6, 8), 0);

    let inv = &s.state().investigators[&INV];
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
        s.state().play_in_progress().is_none(),
        "the sweep took the card off its frame",
    );
}

// -----------------------------------------------------------------------
// A play parked across the AoO must survive a hand-shifting reaction.
// -----------------------------------------------------------------------

/// Regression for #565: the asset that enters play is the one that was
/// announced, even when a reaction shifts the hand under the parked play.
///
/// A non-fast **asset** play parks across the attack-of-opportunity loop, and the
/// Dodge played in that loop leaves hand from under it. With hand
/// `[Dodge, Machete, Knife]`, Machete is announced at index 1 and Dodge's play
/// shifts the hand to `[Machete, Knife]`. The parked play names Machete rather
/// than an index, so Machete enters play, not **Knife** at index 1.
#[test]
fn a_hand_shifting_reaction_does_not_swap_the_asset_that_enters_play() {
    // Machete 3 + Dodge 1 = 4 resources.
    let s = play(board(&[DODGE, MACHETE, KNIFE], 4), 1);
    assert!(
        s.prompt().skippable,
        "the AoO's before-attack cancel window suspends, offering Dodge: {:?}",
        s.prompt()
    );
    assert_eq!(
        s.state().play_in_progress().map(|(_, c)| c.clone()),
        Some(CardCode::new(MACHETE)),
        "the announced asset rides its frame across the suspension",
    );

    // Dodge the attack of opportunity — this is what shifts the hand.
    let s = s.pick(dodge());
    assert_eq!(
        s.state().investigators[&INV].damage(),
        0,
        "the AoO was cancelled by Dodge",
    );

    let inv = &s.state().investigators[&INV];
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
    let s = play(board(&[DODGE, MACHETE], 4), 1);
    assert!(s.prompt().skippable);

    let s = s.pick(dodge());

    let inv = &s.state().investigators[&INV];
    assert!(
        inv.cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MACHETE)),
        "Machete entered play from its frame, with no hand slot to address",
    );
    assert!(inv.hand.is_empty(), "hand={:?}", inv.hand);
}
