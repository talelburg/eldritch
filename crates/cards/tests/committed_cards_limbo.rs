//! End-to-end coverage for #631: a card committed to a skill test is the same
//! card at ST.2, at ST.5, and at ST.8 — even when the hand it came from is
//! mutated in between.
//!
//! The engine deliberately opens the Rules Reference p.26 player window between
//! ST.2 and ST.3, and that window offers Fast plays out of the very hand the
//! commits came from. Committing therefore moves the cards out of the hand and
//! into limbo on the in-flight record — `Limbo.md`, the Rules Reference
//! glossary entry added by the FAQ:
//!
//! > While the effects of an event or treachery card are being resolved, or
//! > while a skill is committed to a skill test, it is neither in play, in the
//! > discard pile, nor is it in an investigator's hand.
//!
//! Lives at `crates/cards/tests/` because it needs the real corpus: the
//! divergence only reproduces with cards whose printed icons and Fast play
//! gates come from `cards::REGISTRY`.

use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{legal_actions, TurnAction};
use game_core::engine::{EngineOutcome, InputRequest, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, LocationId, Phase, TokenModifiers,
    Zone,
};
use game_core::test_support::{
    drive, test_investigator, test_location, ChoiceResolver, GameStateBuilder,
};
use game_core::{assert_event, assert_event_count};

/// Working a Hunch — `01037`. "Fast. Play only during your turn. / Discover 1
/// clue at your location." Two intellect icons; costs 2 resources.
const WORKING_A_HUNCH: &str = "01037";
/// Deduction — `01039`. "If this skill test is successful while investigating a
/// location, discover 1 additional clue at that location." One intellect icon.
const DEDUCTION: &str = "01039";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Investigation-phase board with one investigator at a 3-clue, shroud-2
/// location, `hand` in hand, `resources` resources (Working a Hunch costs 2),
/// and a single-`Numeric(0)` chaos bag so only the committed icons move the
/// total.
fn board(hand: &[&str], resources: u8) -> (GameState, InvestigatorId, LocationId) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(20);

    let mut loc = test_location(20, "Study");
    loc.clues = 3;
    loc.shroud = 2;

    let mut inv = test_investigator(1);
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    inv.resources = resources;
    inv.current_location = Some(loc_id);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(loc)
        .with_investigator(inv)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, inv_id, loc_id)
}

/// Resolver for the divergence scenario: answer the ST.2 commit prompt with
/// `commit` (translated to hand indices at resolve time), then play `play` at
/// the first Fast window that opens **after** it — the ST.2→ST.3 window — and
/// Skip every other window, including the ST.1 one. Declining ST.1 is what
/// makes the play land in the window under test.
///
/// Records the option labels of every Fast window it sees **after** the commit
/// window closed, so a test can assert on what the ST.2→ST.3 window offered.
struct CommitThenPlay {
    commit: Vec<CardCode>,
    play: Option<CardCode>,
    committed: bool,
    windows_after_commit: Vec<Vec<String>>,
}

impl CommitThenPlay {
    fn new(commit: &[&str], play: Option<&str>) -> Self {
        Self {
            commit: commit.iter().map(|c| CardCode::new(*c)).collect(),
            play: play.map(CardCode::new),
            committed: false,
            windows_after_commit: Vec::new(),
        }
    }

    /// The labels the ST.2→ST.3 window offered — the first window to open after
    /// the commit window closed. Panics if no such window opened, so a test
    /// cannot pass by never reaching the window it means to assert on.
    fn st2_window(&self) -> &[String] {
        self.windows_after_commit
            .first()
            .expect("no Fast window opened after the commit window")
    }
}

impl ChoiceResolver for CommitThenPlay {
    fn next(&mut self, request: &InputRequest, state: &GameState) -> InputResponse {
        // The commit window is the only `pick_multiple` prompt these flows
        // reach; answer it once, with the requested codes' hand indices.
        if request.prompt.starts_with("Commit cards to the") {
            assert!(!self.committed, "the commit window opened twice");
            self.committed = true;
            let inv = &state.investigators[&InvestigatorId(1)];
            let mut used = vec![false; inv.hand.len()];
            let selected = self
                .commit
                .iter()
                .map(|code| {
                    let i = inv
                        .hand
                        .iter()
                        .enumerate()
                        .find_map(|(i, c)| (!used[i] && c == code).then_some(i))
                        .unwrap_or_else(|| panic!("{code} not in hand {:?}", inv.hand));
                    used[i] = true;
                    OptionId(u32::try_from(i).expect("fits"))
                })
                .collect();
            return InputResponse::PickMultiple { selected };
        }

        // Decline the ST.1 window untouched — the play under test has to land
        // *after* the commit, in the ST.2→ST.3 window.
        if !self.committed {
            return InputResponse::Skip;
        }
        self.windows_after_commit
            .push(request.options.iter().map(|o| o.label.clone()).collect());
        let wanted = self.play.as_ref().map(|c| format!("Play {c}"));
        let offered = wanted.and_then(|w| request.options.iter().find(|o| o.label == w));
        match offered {
            Some(opt) => {
                self.play = None;
                InputResponse::PickSingle(opt.id)
            }
            None => InputResponse::Skip,
        }
    }
}

impl ChoiceResolver for &mut CommitThenPlay {
    fn next(&mut self, request: &InputRequest, state: &GameState) -> InputResponse {
        (**self).next(request, state)
    }
}

/// Take the Investigate action off the open-turn menu, committing `commit` at
/// ST.2 and playing `play` in a Fast window if one offers it.
fn investigate(
    state: GameState,
    inv_id: InvestigatorId,
    commit: &[&str],
    play: Option<&str>,
) -> (game_core::ApplyResult, CommitThenPlay) {
    let want = TurnAction::Investigate {
        investigator: inv_id,
    };
    let idx = legal_actions(&state)
        .iter()
        .position(|a| *a == want)
        .unwrap_or_else(|| panic!("Investigate not legal; offered {:?}", legal_actions(&state)));
    let mut resolver = CommitThenPlay::new(commit, play);
    let result = drive(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).expect("fits"))),
        }),
        &mut resolver,
    );
    (result, resolver)
}

/// The drive ends where a resolver-driven drive always ends for a completed
/// turn action: back at the open-turn action menu, with the skill test fully
/// torn down.
fn assert_turn_resolved(r: &game_core::ApplyResult) {
    match &r.outcome {
        EngineOutcome::AwaitingInput { request, .. } => {
            assert_eq!(request.prompt, "Choose an action");
        }
        other => panic!("expected the open-turn menu, got {other:?}"),
    }
    assert!(
        !r.state.has_skill_test_in_flight(),
        "SkillTest frame torn down"
    );
}

#[test]
fn a_fast_play_in_the_st2_window_does_not_move_the_committed_card() {
    // #631's divergence scenario. Hand is [Working a Hunch, Deduction]; commit
    // Deduction (hand index 1), then play Working a Hunch in the ST.2→ST.3
    // window. Under the old hand-index binding, index 1 pointed past the end of
    // the one-card hand at ST.5 and panicked.
    let (state, inv_id, loc_id) = board(&[WORKING_A_HUNCH, DEDUCTION], 2);
    let (r, _resolver) = investigate(state, inv_id, &[DEDUCTION], Some(WORKING_A_HUNCH));

    assert_turn_resolved(&r);

    // ST.5 counted Deduction's single intellect icon: base intellect 3 + 1 = 4
    // vs shroud 2 → success with margin 2. Without the icon the margin is 1, so
    // this pins the icon sum to the committed card rather than to whatever the
    // stale index happened to reach.
    assert_event!(
        r.events,
        Event::SkillTestSucceeded { investigator, margin: 2, .. } if *investigator == inv_id
    );

    // ST.8 discarded Deduction. Working a Hunch is in the discard too — but as
    // a played event (RR Appendix I), not as a committed card.
    let inv = &r.state.investigators[&inv_id];
    assert!(inv.hand.is_empty(), "hand = {:?}", inv.hand);
    assert!(
        inv.discard.contains(&CardCode::new(DEDUCTION)),
        "committed Deduction discarded at ST.8; discard = {:?}",
        inv.discard,
    );
    assert_event!(
        r.events,
        Event::CardDiscarded { investigator, code, from: Zone::Hand }
            if *investigator == inv_id && code.as_str() == DEDUCTION
    );

    // Three clues off the location: 1 for Working a Hunch's OnPlay, 1 for the
    // successful Investigate, 1 more for Deduction's OnSkillTestResolution.
    assert_eq!(r.state.locations[&loc_id].clues, 0);
    assert_eq!(inv.clues, 3);
}

#[test]
fn the_st2_window_does_not_offer_a_committed_card_as_a_play() {
    // Working a Hunch has two intellect icons, so it is both committable and a
    // legal Fast play. Hold two copies and 4 resources — enough to play either —
    // then commit one. The ST.2→ST.3 window must offer the *other* copy and only
    // the other copy: at ST.2 the committed one left the hand for the test (RR
    // glossary, "Limbo"), so it is not available to be played.
    let (state, inv_id, _) = board(&[WORKING_A_HUNCH, WORKING_A_HUNCH], 4);
    let (r, resolver) = investigate(state, inv_id, &[WORKING_A_HUNCH], None);

    assert_turn_resolved(&r);
    assert_eq!(
        resolver.st2_window(),
        [format!("Play {WORKING_A_HUNCH}")],
        "exactly the uncommitted copy is offered",
    );
}

#[test]
fn only_the_committed_copy_of_a_duplicated_card_is_discarded() {
    // A hand holds `CardCode`s, so two copies of one card are the same value and
    // "the wrong copy was discarded" is not observable from the end state — this
    // is the guard that exactly *one* copy is consumed. The falsifiable
    // duplicate case is the sibling test below, which commits both.
    //
    // Two copies of Deduction in hand, one committed — and a Working a Hunch
    // played in the ST.2→ST.3 window so the surviving copy's hand position is
    // not the one the commit named. Exactly one copy reaches the discard pile at
    // ST.8; the other stays in hand.
    let (state, inv_id, _) = board(&[WORKING_A_HUNCH, DEDUCTION, DEDUCTION], 2);
    let (r, _resolver) = investigate(state, inv_id, &[DEDUCTION], Some(WORKING_A_HUNCH));

    assert_turn_resolved(&r);
    let inv = &r.state.investigators[&inv_id];
    assert_eq!(
        inv.hand,
        vec![CardCode::new(DEDUCTION)],
        "one copy retained"
    );
    assert_eq!(
        inv.discard,
        vec![CardCode::new(WORKING_A_HUNCH), CardCode::new(DEDUCTION)],
        "the played event, then the one committed copy",
    );
    // One for the played event, one for the committed copy — not two Deductions.
    assert_event_count!(r.events, 2, Event::CardDiscarded { .. });
}

#[test]
fn committing_both_copies_of_a_card_discards_both_after_a_hand_shift() {
    // Both Deductions committed, then Working a Hunch played in the ST.2→ST.3
    // window. Under the old hand-index binding the descending-order ST.8 removal
    // ran off the end of the two-card hand and left one copy behind.
    let (state, inv_id, _) = board(&[WORKING_A_HUNCH, DEDUCTION, DEDUCTION], 2);
    let (r, _resolver) = investigate(
        state,
        inv_id,
        &[DEDUCTION, DEDUCTION],
        Some(WORKING_A_HUNCH),
    );

    assert_turn_resolved(&r);
    let inv = &r.state.investigators[&inv_id];
    assert!(inv.hand.is_empty(), "hand = {:?}", inv.hand);
    assert_eq!(
        inv.discard,
        vec![
            CardCode::new(WORKING_A_HUNCH),
            CardCode::new(DEDUCTION),
            CardCode::new(DEDUCTION),
        ],
    );
}
