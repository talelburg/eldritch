//! Each of the five investigators, seated with their **default deck** exactly
//! as the picker sends it, plays The Gathering to a resolution.
//!
//! Every walk seats [`WeaknessChoice::Fixed`]'s roster entry, the picker's
//! default, against the production registries (the real corpus and the real
//! scenario modules): act 1 advances on spent clues, act 2 through its
//! round-end clue-spend window, whose reverse spawns the real Ghoul Priest
//! 01116, and the Priest's defeat advances the terminal act to its printed
//! choice. Each investigator lands the kill their own way.
//!
//! Two stand-ins, the same in every walk: a single-token `Numeric(0)` chaos bag
//! (production serves the random Standard bag), and an encounter deck of one
//! Ancient Evils 01166, whose Revelation only places 1 doom, too little to
//! advance the agenda, so round 2's Mythos draw is deterministic. The player deck
//! is the real one, and the fixed `rng` seed `setup()` builds with makes its
//! shuffle deterministic: each walk scripts the windows that shuffle's hand
//! opens. That shuffle leaves every deck's fixed weakness at the same depth,
//! below anything a walk draws, so no walk meets it; seating it is covered by
//! `cards::decks`' own tests and by the server's create test.

use cards::decks::{self, DefaultDeck, WeaknessChoice};
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{self, OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, DiscardPile, EnemyId, GameState,
    InvestigatorId, Phase, SkillKind, Zone,
};
use game_core::test_support::{BoardLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence};
use scenarios::the_gathering;

/// What the server passes to every `apply`: the real corpus and the real
/// scenario modules, with no synthetic test cards composed in. Not
/// `scenarios::TEST_REGISTRIES`, because each walk's claim is that the roster
/// the picker sends plays in production.
const REGISTRIES: Registries = Registries {
    cards: &cards::REGISTRY,
    scenarios: &scenarios::REGISTRY,
};

const INV: InvestigatorId = InvestigatorId(1);
/// The Ghoul Priest, set aside until act 2 (01109)'s reverse spawns it.
const GHOUL_PRIEST: &str = "01116";
const KNIFE: &str = "01086";
const MIND_OVER_MATTER: &str = "01036";
const ELUSIVE: &str = "01050";
const BACKSTAB: &str = "01051";
const WENDYS_AMULET: &str = "01014";

/// The Gathering set up and `deck`'s investigator seated solo with the roster
/// entry the picker sends by default, at the mulligan, which keeps the hand.
fn seated(deck: &DefaultDeck) -> TestSession {
    let mut state = the_gathering::setup();
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
    state.encounter_deck.clear();
    state.encounter_deck.push_back(CardCode::new("01166"));

    let roster = [deck.roster_entry(WeaknessChoice::Fixed)];
    let started = engine::seat_and_open(state, &roster, 0, REGISTRIES);
    let session = TestSession::new(started.state, REGISTRIES).choose_from_hand(&[]);
    // The module doc's claim: the shuffle leaves the fixed weakness deep in the
    // deck, below the one upkeep draw a walk makes.
    let weakness = CardCode::new(deck.weakness);
    let depth = session.state().investigators[&INV]
        .deck
        .iter()
        .position(|c| *c == weakness)
        .expect("the fixed weakness is in the deck");
    assert!(depth > 1, "{} sits at depth {depth}", deck.weakness);
    session
}

/// Where `code` sits in the investigator's hand.
fn hand_index(session: &TestSession, code: &str) -> u8 {
    let index = session.state().investigators[&INV]
        .hand
        .iter()
        .position(|c| c.as_str() == code)
        .unwrap_or_else(|| panic!("{code} is in the hand"));
    u8::try_from(index).expect("a small hand")
}

/// The turn opens on a start-of-turn fast window (Appendix II 2.2) offering a
/// Fast card the shuffle dealt into the hand, `code`, and nothing else. The
/// walk passes it.
fn pass_fast_window_offering(session: TestSession, code: &str) -> TestSession {
    let card = OptionTarget::HandCard {
        investigator: INV,
        hand_index: hand_index(&session, code),
    };
    assert_eq!(
        (session.prompt().options.len(), session.offered_at(&card)),
        (1, 1),
        "a fast window offering {code} alone, got {:?}",
        session.prompt(),
    );
    session.skip()
}

/// Seed clues for both act thresholds (act 1 = 2, act 2 = 3), as Roland's walk
/// in `the_gathering_resolutions.rs` does — discovering them is covered
/// elsewhere.
fn with_clues_for_both_acts(session: TestSession) -> TestSession {
    session.edit_state(|state| {
        state.investigators.get_mut(&INV).expect("seated").clues = 5;
    })
}

/// Act 1 spends its clues and advances; the reverse builds the board and moves
/// the investigator to the Hallway.
fn advance_act_one(session: TestSession) -> TestSession {
    let session = session.take(&TurnAction::AdvanceAct { investigator: INV });
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().act_index, 1, "act 1 advanced to act 2");
    session
}

/// Ending the round opens act 2's round-end clue-spend window; the spend
/// advances it, and its reverse spawns the Ghoul Priest in the Hallway. The
/// session is left at round 2's Mythos draw.
fn advance_act_two(session: TestSession) -> TestSession {
    let round_end = session.take(&TurnAction::EndTurn);
    assert_eq!(round_end.state().phase, Phase::Upkeep);
    assert_eq!(
        round_end.offered_at(&OptionTarget::Act),
        1,
        "the act-2 round-end window, got {:?}",
        round_end.prompt(),
    );
    let after_spend = round_end.pick(OptionTarget::Act);
    assert_eq!(after_spend.state().act_index, 2, "act 2 advanced to act 3");
    after_spend
}

/// The spawned Priest seeded `left` damage from defeat and engaged with the
/// investigator, as Roland's walk in `the_gathering_resolutions.rs` does.
fn priest_engaged_with(state: &mut GameState, left: u8) {
    let priest = state
        .enemies
        .values_mut()
        .find(|e| e.code.as_str() == GHOUL_PRIEST)
        .expect("act 2's reverse spawned the Ghoul Priest");
    priest.damage = priest.max_health - left;
    priest.engaged_with = Some(INV);
}

/// The spawned Ghoul Priest.
fn the_priest(state: &GameState) -> EnemyId {
    state
        .enemies
        .iter()
        .find(|(_, e)| e.code.as_str() == GHOUL_PRIEST)
        .map(|(id, _)| *id)
        .expect("act 2's reverse spawned the Ghoul Priest")
}

/// The Priest's defeat advanced the terminal act, whose printed choice's first
/// bullet reaches Resolution 1.
fn resolves_on_the_first_bullet(defeated: TestSession) {
    assert_event!(defeated.events(), Event::EnemyDefeated { .. });
    assert_event!(defeated.events(), Event::ActAdvanced { from } if *from == 2);
    assert!(
        defeated.state().ending.is_none(),
        "nothing latches before the pick"
    );
    let result = defeated.pick_nth(0).finish();
    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(1))),
        "act 01110's reverse reaches (→R1)",
    );
}

/// Play Knife from the hand, then fight with its discard ability:
///
/// > [action] Discard Knife: **Fight.** You get +2 [combat] for this attack.
/// > This attack deals +1 damage.
///
/// The session is left at the fight's commit window.
fn fight_with_a_discarded_knife(session: TestSession) -> TestSession {
    let knife = hand_index(&session, KNIFE);
    let armed = session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(knife),
    });
    armed.assert_at_turn_menu(INV);
    let knife = armed.state().in_play(INV, KNIFE);
    let fighting = armed.take(&TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(knife),
        address: AbilityAddress::Printed(1),
    });
    assert_eq!(fighting.rejection(), None, "Knife's discard ability");
    fighting
}

/// Roland Banks 01001. His combat 4 against the Priest's fight 4 lands a basic
/// Fight on the `Numeric(0)` bag. The shuffle deals Mind over Matter 01036, a
/// Fast event (*"Fast. Play only during your turn."*), into his opening hand,
/// so each of his turns opens on a fast window offering it.
#[test]
fn roland_with_his_default_deck_plays_the_gathering_to_a_resolution() {
    let session = pass_fast_window_offering(seated(&decks::ROLAND), MIND_OVER_MATTER);
    let session = with_clues_for_both_acts(session);
    session.assert_at_turn_menu(INV);

    let session = advance_act_two(advance_act_one(session));
    let investigation = pass_fast_window_offering(session.confirm(), MIND_OVER_MATTER);
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    let investigation = investigation.edit_state(|state| priest_engaged_with(state, 1));
    let priest = the_priest(investigation.state());
    let fighting = investigation.take(&TurnAction::Fight {
        investigator: INV,
        enemy: priest,
    });
    let defeated = pass_fast_window_offering(fighting, MIND_OVER_MATTER).commit(&[]);
    let defeated = pass_fast_window_offering(defeated, MIND_OVER_MATTER);
    resolves_on_the_first_bullet(defeated);
}

/// Daisy Walker 01002. Her combat 2 fails a basic Fight against the Priest's
/// fight 4 on the `Numeric(0)` bag, so she fights with a discarded Knife, which
/// the shuffle deals into her opening hand: combat 2 + 2 meets fight 4, and
/// its 1 + 1 damage defeats the Priest.
#[test]
fn daisy_with_her_default_deck_plays_the_gathering_to_a_resolution() {
    let session = with_clues_for_both_acts(seated(&decks::DAISY));
    session.assert_at_turn_menu(INV);

    let session = advance_act_two(advance_act_one(session));
    let investigation = session.confirm();
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    let investigation = investigation.edit_state(|state| priest_engaged_with(state, 2));
    let defeated = fight_with_a_discarded_knife(investigation).commit(&[]);
    assert_event!(defeated.events(), Event::EnemyDamaged { amount: 2, .. });
    resolves_on_the_first_bullet(defeated);
}

/// "Skids" O'Toole 01003. His combat 3 fails a basic Fight against the Priest's
/// fight 4 on the `Numeric(0)` bag, so he fights with a discarded Knife, which
/// the shuffle deals into his opening hand: combat 3 + 2 beats fight 4. Each of
/// his turns opens on a fast window offering his own *"\[fast\] During your turn,
/// spend 2 resources: You may take an additional action this turn."*, which the
/// walk passes.
#[test]
fn skids_with_his_default_deck_plays_the_gathering_to_a_resolution() {
    let session = pass_his_ability(seated(&decks::SKIDS));
    let session = with_clues_for_both_acts(session);
    session.assert_at_turn_menu(INV);

    let session = advance_act_two(advance_act_one(session));
    let investigation = pass_his_ability(session.confirm());
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    // His ability keeps the fight's player windows open, before his commit and
    // after it, as in his own card tests: pass them.
    let investigation = investigation.edit_state(|state| priest_engaged_with(state, 2));
    let fighting = pass_his_ability(fight_with_a_discarded_knife(investigation));
    let defeated = pass_his_ability(fighting.commit(&[]));
    assert_event!(defeated.events(), Event::EnemyDamaged { amount: 2, .. });
    resolves_on_the_first_bullet(defeated);
}

/// Skids's own zero-action ability, offered on his investigator card; the walk passes
/// it.
fn pass_his_ability(session: TestSession) -> TestSession {
    let skids = session.state().investigators[&INV].card_anchor();
    assert_eq!(
        session.offered_at(&skids),
        1,
        "his ability is offered, got {:?}",
        session.prompt(),
    );
    session.skip()
}

/// Agnes Baker 01004. The engaged Priest attacks in the Enemy phase, and the
/// horror it places offers her own *"\[reaction\] After 1 or more horror is
/// placed on Agnes Baker: Deal 1 damage to an enemy at your location."*, whose
/// 1 damage defeats it.
#[test]
fn agnes_with_her_default_deck_plays_the_gathering_to_a_resolution() {
    let session = with_clues_for_both_acts(seated(&decks::AGNES));
    session.assert_at_turn_menu(INV);

    let session = advance_act_two(advance_act_one(session));
    let investigation = session.confirm();
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    let investigation = investigation.edit_state(|state| priest_engaged_with(state, 1));
    let attacked = investigation.take(&TurnAction::EndTurn);
    let agnes = attacked.state().investigators[&INV].card_anchor();
    assert_eq!(
        attacked.offered_at(&agnes),
        1,
        "the Priest's horror offers Agnes's reaction, got {:?}",
        attacked.prompt(),
    );
    resolves_on_the_first_bullet(attacked.pick(agnes));
}

/// Wendy Adams 01005. Her combat 1 fails a basic Fight against the Priest's
/// fight 4 on the `Numeric(0)` bag, and a Knife's +2 still falls short. She
/// lands the kill with Backstab 01051 (*"<b>Fight.</b> This attack uses
/// \[agility\] instead of \[combat\]. This attack deals +2 damage."*), played from
/// her discard pile under Wendy's Amulet 01014, *"You may play the topmost event
/// in your discard pile as if it were in your hand."* Backstab reaches the
/// discard pile through her own reaction's cost:
///
/// > \[reaction\] When you reveal a chaos token, choose and discard 1 card from
/// > your hand: Cancel that chaos token and return it to the bag. Reveal a new
/// > chaos token. (Limit once per test/ability.)
///
/// and, played with the Amulet in play, leaves for the bottom of her deck under
/// the Amulet's *"<b>Forced</b> - After you play an event or discard an event
/// from play: Place it on the bottom of your deck instead of in your discard
/// pile."* The shuffle deals neither card by then, so the walk seeds both at
/// the front of her hand. It does deal Elusive 01050 (*"Fast. Play only during
/// your turn."*) into her opening hand, so each of her turns opens on a fast
/// window offering it.
#[test]
fn wendy_with_her_default_deck_plays_the_gathering_to_a_resolution() {
    let session = seated(&decks::WENDY);
    let session = with_clues_for_both_acts(session).edit_state(|state| {
        to_front_of_hand(state, BACKSTAB);
        to_front_of_hand(state, WENDYS_AMULET);
    });
    session.assert_at_turn_menu(INV);
    let session = advance_act_one(session);

    // --- Round 1, with no enemy yet: the Amulet into play, from her 5 starting
    // resources.
    let session = session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    session.assert_at_turn_menu(INV);
    let amulet = session.state().in_play(INV, WENDYS_AMULET);

    // An investigation, whose reveal offers her reaction: she takes it and pays
    // its cost with Backstab, now at the front of her hand. The `0` revealed in
    // the cancelled one's place investigates the Hallway's shroud 1 with
    // intellect 3.
    let at_reveal = session
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[]);
    let reaction = wendys_card(&at_reveal);
    assert_eq!(
        at_reveal.offered_at(&reaction),
        1,
        "her reveal offers the reaction, got {:?}",
        at_reveal.prompt(),
    );
    let investigated = at_reveal.pick(reaction).pick(OptionTarget::HandCard {
        investigator: INV,
        hand_index: 0,
    });
    investigated.assert_at_turn_menu(INV);
    assert_event_sequence!(
        investigated.events(),
        Event::ChaosTokenRevealed { .. },
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(INV) }
            if *code == CardCode::new(BACKSTAB),
        Event::ChaosTokenRevealed { .. },
        Event::SkillTestSucceeded { .. },
    );

    // Act 2's reverse reveals the Parlor, a location Elusive can move her to,
    // so from round 2 on her turn opens on a fast window offering it.
    let session = advance_act_two(investigated);
    let investigation = pass_fast_window_offering(session.confirm(), ELUSIVE);
    investigation.assert_at_turn_menu(INV);
    assert_eq!(investigation.state().round, 2);

    // --- Backstab from the top of her discard pile, offered on the Amulet:
    // agility 4 + Numeric(0) against the Priest's fight 4 succeeds, and its 3
    // damage defeats the Priest. The reveal offers her reaction again, a new
    // test's: she passes it.
    let investigation = investigation.edit_state(|state| priest_engaged_with(state, 3));
    let fighting = investigation.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::TopOfDiscardPile { permission: amulet },
    });
    let at_reveal = fighting.commit(&[]);
    assert_eq!(
        at_reveal.offered_at(&wendys_card(&at_reveal)),
        1,
        "her reveal offers the reaction, got {:?}",
        at_reveal.prompt(),
    );
    let defeated = at_reveal.skip();
    assert_event!(
        defeated.events(),
        Event::SkillTestStarted {
            skill: SkillKind::Agility,
            difficulty: 4,
            ..
        }
    );
    assert_event!(defeated.events(), Event::EnemyDamaged { amount: 3, .. });
    resolves_on_the_first_bullet(defeated);
}

/// Move one copy of `code` from wherever the shuffle left it, deck or hand, to
/// the front of Wendy's hand, so the walk need not depend on what was dealt.
fn to_front_of_hand(state: &mut GameState, code: &str) {
    let code = CardCode::new(code);
    let wendy = state.investigators.get_mut(&INV).expect("Wendy seated");
    if let Some(i) = wendy.hand.iter().position(|c| *c == code) {
        wendy.hand.remove(i);
    } else {
        let i = wendy
            .deck
            .iter()
            .position(|c| *c == code)
            .expect("the default deck holds the card");
        wendy.deck.remove(i);
    }
    wendy.hand.insert(0, code);
}

/// Her reaction, offered on her investigator card.
fn wendys_card(session: &TestSession) -> OptionTarget {
    session.state().investigators[&INV].card_anchor()
}
