//! `ForcedTriggerPoint::RoundEnded`: an agenda's `OnEvent(RoundEnded)`
//! Forced ability fires at the end of the round (step 4.6).

use std::sync::OnceLock;

use card_dsl::dsl::{
    deal_horror, forced_on_event, native, Ability, EventPattern, EventTiming, InvestigatorTarget,
};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::state::{Agenda, CardCode, InvestigatorId};
use game_core::test_support::{
    fire_forced_on_round_end, metadata_for_test_inv, test_investigator, GameStateBuilder,
};
use game_core::{Cx, EngineOutcome, EvalContext};

const AGENDA: &str = "TEST-AGENDA";

/// A second round-end forced source: a threat-area card with an
/// `OnEvent(RoundEnded)` forced ability dealing 1 horror to its controller.
/// Mirrors the real two-source collision (agenda 01107's doom + Dissonant
/// Voices 01165's discard) so the lead orders both at round end (#213).
const DISSONANT: &str = "TEST-DISSONANT";

/// Returns metadata for `TEST_INV` so capacity reads work when this registry
/// is installed. All other codes return `None`.
fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    metadata_for_test_inv(code)
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == AGENDA {
        Some(vec![forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::At,
            native("test:set-doom"),
        )])
    } else if code.as_str() == DISSONANT {
        Some(vec![forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::At,
            deal_horror(InvestigatorTarget::You, 1u8),
        )])
    } else {
        None
    }
}

fn set_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    cx.state.agenda_doom = 5;
    EngineOutcome::Done
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == "test:set-doom").then_some(set_doom as NativeEffectFn)
}

fn install() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: mock_native_for,
            native_eligibility_for: |_| None,
        });
    });
}

#[test]
fn round_ended_fires_agenda_forced_ability() {
    install();
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(AGENDA),
        doom_threshold: 10,
        resolution: None,
    }];
    state.agenda_index = 0;
    let mut events = Vec::new();
    let outcome = fire_forced_on_round_end(&mut state, &mut events);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 5, "RoundEnded fired the agenda ability");
}

#[test]
fn two_round_end_forced_suspend_then_resume_the_upkeep_tail() {
    // #213 end-to-end through `apply`: two simultaneous round-end forced
    // abilities (agenda doom + a threat-area discard-shape) make `upkeep_phase_end`
    // open the forced run and suspend for the lead's ordering. Resolving both
    // closes the run, whose `UpkeepAfterRoundEnded` continuation resumes the
    // upkeep tail — the Upkeep→Mythos transition — that a terminal close would
    // have dropped.
    use game_core::action::InputResponse;
    use game_core::engine::enumerate::legal_actions;
    use game_core::engine::OptionId;
    use game_core::state::{CardInPlay, CardInstanceId, LocationId, Phase};
    use game_core::test_support::test_location;
    use game_core::{apply, Action, PlayerAction, TurnAction};

    install();

    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.actions_remaining = 0;
    // Non-empty deck so the upkeep 4.4 draw doesn't fire a deckout horror
    // penalty and muddy the horror assertion.
    inv.deck = vec![CardCode::new("filler-card")];
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new(DISSONANT),
        CardInstanceId(1),
    ));

    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        // Mid-Investigation invariant (slice 1a): the EndTurn cascade pops the
        // InvestigationPhase anchor at investigation_phase_end.
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame the
        // EndTurn cascade pops before advancing past Investigation.
        .with_investigator_turn(InvestigatorId(1))
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(AGENDA),
        doom_threshold: 10,
        resolution: None,
    }];
    state.agenda_index = 0;

    // EndTurn drives Investigation → Enemy → Upkeep and reaches step 4.6,
    // where the two round-end forced abilities fire simultaneously. The
    // forced run suspends for the lead's ordering choice rather than
    // transitioning to Mythos. Submitted via the enumeration round-trip
    // (the typed `PlayerAction::EndTurn` removed in 2b, #447).
    let end_turn = {
        let idx = legal_actions(&state)
            .iter()
            .position(|a| a == &TurnAction::EndTurn)
            .expect("EndTurn must be a legal open-turn action");
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        })
    };
    let paused = apply(state, end_turn);
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "two round-end forced must present the lead a choice; got {:?}",
        paused.outcome,
    );
    assert_eq!(
        paused.state.phase,
        Phase::Upkeep,
        "no Upkeep→Mythos transition until both forced resolve",
    );
    assert_eq!(
        paused.state.agenda_doom, 0,
        "no forced resolves until ordered"
    );
    assert_eq!(paused.state.investigators[&InvestigatorId(1)].horror(), 0);

    // Resolve the first ordered forced; the second is still pending.
    let after_first = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(
        matches!(after_first.outcome, EngineOutcome::AwaitingInput { .. }),
        "second round-end forced still pending; got {:?}",
        after_first.outcome,
    );
    assert_eq!(
        after_first.state.phase,
        Phase::Upkeep,
        "still mid-forced-run; no transition yet",
    );

    // Resolve the second: the forced run closes, its UpkeepAfterRoundEnded
    // continuation runs the upkeep tail (no act round-end window here) and
    // transitions Upkeep → Mythos.
    let done = apply(
        after_first.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    // The upkeep tail runs through to Mythos, which pauses at the step-1.4
    // encounter-draw prompt (AwaitingInput) after placing its +1 doom.
    assert!(matches!(done.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        done.state.agenda_doom, 6,
        "agenda round-end forced resolved (native set 5), then Mythos entry \
         placed its +1 doom — the upkeep tail ran through to the next phase",
    );
    assert_eq!(
        done.state.investigators[&InvestigatorId(1)].horror(),
        1,
        "threat-area round-end forced resolved",
    );
    assert_eq!(
        done.state.phase,
        Phase::Mythos,
        "upkeep tail resumed: Upkeep → Mythos after both forced resolved",
    );
    assert_eq!(done.state.round, 1, "round bumped (0 → 1) on Mythos entry");
}
