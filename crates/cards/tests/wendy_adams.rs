//! Wendy Adams 01005 end-to-end against the real corpus: the reaction that
//! cancels a revealed chaos token for a card from hand, and the elder sign
//! that Wendy's Amulet 01014 turns into an automatic success.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01005):
//!
//! > \[reaction\] When you reveal a chaos token, choose and discard 1 card from
//! > your hand: Cancel that chaos token and return it to the bag. Reveal a new
//! > chaos token. (Limit once per test/ability.)
//! > \[elder_sign\] effect: +0. If Wendy's Amulet is in play, you automatically
//! > succeed instead.
//!
//! Rulings (<https://arkhamdb.com/card/01005>): *"If you use Wendy's ability to
//! 'cancel a chaos token', treat it as if you've never revealed that token, for
//! purposes of effects that trigger if a specific symbol is revealed (e.g.
//! Baseball Bat)."* and *"If a skill test automatically succeeds, the total
//! difficulty of that test is considered 0."*
//!
//! Runs against `cards::TEST_REGISTRIES`, whose scenario registry is empty, so
//! the scenario symbols modify a test by `+0` and initiate nothing.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, DiscardPile, EnemyId,
    GameStateBuilder, InvestigatorId, LocationId, SkillKind, TokenModifiers, Zone,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const WENDY: &str = "01005";
const WENDYS_AMULET: &str = "01014";
const BASEBALL_BAT: &str = "01074";
/// Emergency Cache 01088, *"Gain 3 resources."*: a plain non-weakness card to
/// pay the reaction's cost with.
const EMERGENCY_CACHE: &str = "01088";
/// Guts 01089, `[willpower][willpower]`: *"Max 1 committed per skill test. If
/// this test is successful, draw 1 card."*
const GUTS: &str = "01089";
/// Amnesia 01096, a basic weakness the reaction's cost may not choose.
const AMNESIA: &str = "01096";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

/// A seed whose first two draws from a two-token bag take the first token,
/// then the second: so `[A, B]` reveals `A`, then `B` on the redraw.
const FIRST_THEN_SECOND: u64 = 0;

/// Wendy, seated from her printed card (Willpower 4, Combat 1), at `HERE`.
fn wendy() -> Seat {
    seat(1).as_card(WENDY).at(HERE)
}

/// `wendy`'s open turn at `HERE`, drawing from `bag` under
/// [`FIRST_THEN_SECOND`].
fn board(wendy: Seat, bag: impl IntoIterator<Item = ChaosToken>) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(wendy)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_chaos_bag(ChaosBag::new(bag))
        .with_token_modifiers(TokenModifiers::default())
        .with_rng_seed(FIRST_THEN_SECOND)
        .open_turn(INV)
}

/// Wendy's willpower test against `difficulty`, committing `commit`.
fn willpower_test(
    wendy: Seat,
    bag: impl IntoIterator<Item = ChaosToken>,
    difficulty: i8,
    commit: &[&str],
) -> TestSession {
    TestSession::new(board(wendy, bag).build(), TEST_REGISTRIES)
        .test_skill(INV, SkillKind::Willpower, difficulty)
        .commit(commit)
}

/// Her reaction, offered on her investigator card.
fn wendys_card(session: &TestSession) -> OptionTarget {
    OptionTarget::CardInstance(
        session.state().investigators[&INV]
            .investigator_card
            .instance_id,
    )
}

fn hand_card(hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: INV,
        hand_index,
    }
}

// ---- the reaction ----------------------------------------------------------

/// *"Cancel that chaos token and return it to the bag. Reveal a new chaos
/// token."*: the cancelled token is back in the bag before the redraw, so the
/// redraw can draw the same token. With only the `[auto_fail]` in the bag it
/// is revealed again, and the test fails; the limit keeps the reaction from
/// being offered a second time.
#[test]
fn the_redraw_can_draw_the_same_token() {
    let session = willpower_test(
        wendy().hand([EMERGENCY_CACHE]),
        [ChaosToken::AutoFail],
        4,
        &[],
    );
    let reaction = wendys_card(&session);

    // A lone non-weakness card pays the cost with no prompt.
    let session = session.pick(reaction);

    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::AutoFail,
            ..
        },
        Event::CardDiscarded { code, from: Zone::Hand, .. }
            if *code == CardCode::new(EMERGENCY_CACHE),
        Event::ChaosTokenRevealed {
            token: ChaosToken::AutoFail,
            ..
        },
        Event::SkillTestFailed { .. },
    );
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
}

/// Willpower 4 against 4. The `[auto_fail]` would fail it; cancelled, it
/// contributes nothing, and the `0` revealed in its place passes the test. The
/// cost discards the hand card she chose, and the new token's `when` cell
/// offers the reaction no more: the test runs back to the turn menu.
#[test]
fn the_reaction_discards_the_chosen_card_cancels_the_token_and_reveals_a_new_one() {
    let session = willpower_test(
        wendy().hand([EMERGENCY_CACHE, GUTS]),
        [ChaosToken::AutoFail, ChaosToken::Numeric(0)],
        4,
        &[],
    );
    let reaction = wendys_card(&session);
    assert_eq!(session.offered_at(&reaction), 1, "offered on her reveal");

    let at_cost = session.pick(reaction);
    assert_eq!(at_cost.offered_at(&hand_card(0)), 1);
    assert_eq!(at_cost.offered_at(&hand_card(1)), 1);
    let session = at_cost.pick(hand_card(1));

    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::AutoFail,
            ..
        },
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(INV) }
            if *code == CardCode::new(GUTS),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Numeric(0),
            ..
        },
        Event::SkillTestSucceeded { margin: 0, .. },
    );
    assert_no_event!(session.events(), Event::SkillTestFailed { .. });
    let wendy = &session.state().investigators[&INV];
    assert_eq!(wendy.hand, vec![CardCode::new(EMERGENCY_CACHE)]);
    assert_eq!(wendy.discard, vec![CardCode::new(GUTS)]);
}

/// The Bat's ruling, through Wendy's: a cancelled `[skull]` is treated as never
/// revealed, so the Bat's *"If a `[skull]` or `[auto_fail]` symbol is revealed
/// during this attack, discard Baseball Bat"* never fires. Combat 1 + 2 = 3
/// against fight 3: the `0` drawn in the skull's place passes the attack.
#[test]
fn a_cancelled_skull_does_not_discard_baseball_bat() {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 3;
    let state = board(
        wendy().hand([EMERGENCY_CACHE]).in_play([BASEBALL_BAT]),
        [ChaosToken::Skull, ChaosToken::Numeric(0)],
    )
    .with_enemy_engaged(enemy, INV)
    .build();
    let bat = state.in_play(INV, BASEBALL_BAT);
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(bat),
            address: AbilityAddress::Printed(0),
        })
        .commit(&[]);
    // A lone discardable card is paid with no prompt.
    let reaction = wendys_card(&session);
    let session = session.pick(reaction);

    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            ..
        },
        Event::ChaosTokenRevealed {
            token: ChaosToken::Numeric(0),
            ..
        },
        Event::SkillTestSucceeded { .. },
    );
    assert_no_event!(
        session.events(),
        Event::CardDiscarded { code, .. } if *code == CardCode::new(BASEBALL_BAT)
    );
    assert!(session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .any(|c| c.code == CardCode::new(BASEBALL_BAT)));
}

/// Dark Memory 01013's ruling (<https://arkhamdb.com/card/01013>): *"You cannot
/// choose to discard a Weakness card from your hand, neither for exceeding the
/// max number of cards in hand, nor for card effects that make you 'choose and
/// discard'."* A hand of only a weakness can't pay, so no reaction is offered
/// and the test resolves its `[auto_fail]` untouched.
#[test]
fn no_reaction_is_offered_with_a_hand_of_only_weaknesses() {
    let session = willpower_test(wendy().hand([AMNESIA]), [ChaosToken::AutoFail], 4, &[]);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_eq!(
        session.state().investigators[&INV].hand,
        vec![CardCode::new(AMNESIA)]
    );
}

// ---- the elder sign --------------------------------------------------------

/// Without the Amulet the elder sign is `+0`: willpower 4 passes 4 by 0 and
/// fails 5 by 1.
#[test]
fn without_the_amulet_the_elder_sign_is_plus_zero() {
    let passes = willpower_test(wendy(), [ChaosToken::ElderSign], 4, &[]);
    passes.assert_at_turn_menu(INV);
    assert_event!(passes.events(), Event::SkillTestSucceeded { margin: 0, .. });

    let fails = willpower_test(wendy(), [ChaosToken::ElderSign], 5, &[]);
    fails.assert_at_turn_menu(INV);
    assert_event!(fails.events(), Event::SkillTestFailed { by: 1, .. });
}

/// With the Amulet in play the elder sign succeeds automatically: willpower 4
/// plus Guts' two icons is 6 against 9, yet the test succeeds. The test still
/// takes place, so the committed Guts resolves its *"If this test is
/// successful, draw 1 card."*
#[test]
fn with_the_amulet_in_play_the_elder_sign_automatically_succeeds() {
    let session = willpower_test(
        wendy()
            .hand([GUTS])
            .deck([EMERGENCY_CACHE])
            .in_play([WENDYS_AMULET]),
        [ChaosToken::ElderSign],
        9,
        &[GUTS],
    );

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_no_event!(session.events(), Event::SkillTestFailed { .. });
    assert_eq!(
        session.state().investigators[&INV].hand,
        vec![CardCode::new(EMERGENCY_CACHE)],
        "Guts drew a card on the success"
    );
}

/// A cancelled elder sign is never revealed, so the Amulet's automatic success
/// never latches: the `0` revealed in its place fails willpower 4 against 9.
#[test]
fn a_cancelled_elder_sign_does_not_automatically_succeed() {
    let session = willpower_test(
        wendy().hand([EMERGENCY_CACHE]).in_play([WENDYS_AMULET]),
        [ChaosToken::ElderSign, ChaosToken::Numeric(0)],
        9,
        &[],
    );
    let reaction = wendys_card(&session);
    let session = session.pick(reaction);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { by: 5, .. });
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
}
