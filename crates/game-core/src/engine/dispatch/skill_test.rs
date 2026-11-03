//! Skill-test resolution handlers.
//!
//! Contains the full skill-test lifecycle: starting a test
//! ([`start_skill_test`]), the commit-stage entry ([`finish_skill_test`]),
//! the resolution driver ([`advance`]), and all supporting
//! helpers.

use std::collections::BTreeMap;

use card_dsl::card_data::CardKind;
use card_dsl::dsl::{
    Determination, Effect, IntExpr, ResultContribution, ResultController, RevealClause,
    SkillTestKind, Stat, TestOutcome, TokenOutcome, Trigger,
};

use crate::action::InputResponse;
use crate::card_registry::CardRegistry;
use crate::engine::board::{self, NotLeft};
use crate::engine::dispatch::actions::evade::{self, EvadeModification};
use crate::engine::dispatch::actions::fight::{self, FightModification};
use crate::engine::dispatch::actions::investigate::{self, InvestigateModification};
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::{
    choice, coordinator, cursor, emit, hand_pick, prompt, reaction_windows,
};
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::modified_value::{
    self, ContributionSource, ModifiedQuantity, ModifierBreakdown, ModifierTarget, ReadContext,
};
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, PickCount,
};
use crate::engine::{abilities_in_effect, Cx};
use crate::event::{Event, FailureReason};
use crate::scenario;
use crate::state::{
    self, AbilitySource, CardCode, CardInstanceId, ChaosToken, CommittedCard,
    CommittedCardDestination, CommittedIndex, Continuation, DifficultyBasis, DiscardPile,
    EncounterCardFrame, EnemyId, FastWindowKind, GameState, InFlightSkillTest, InvestigatorId,
    LastingEffect, LastingEffectKind, Lifetime, LocationId, PendingResults, RecordedModifier,
    ResolvedTest, RevealedToken, SkillKind, SkillSubstitution, SkillTestFollowUp, SkillTestStep,
    St7Result, Status, SubstitutionPromptFrame, TokenResolution, Zone,
};

/// The one-shot modifier an initiator grants the test it starts: a weapon's
/// *"+N \[combat\] for this attack"* (the
/// [`Fight`](card_dsl::dsl::ActionDesignator::Fight) designator's combat modifier)
/// or Flashlight 01087's *"Your location gets -2 shroud for this
/// investigation"* (the
/// [`Investigate`](card_dsl::dsl::ActionDesignator::Investigate) designator's
/// shroud modifier).
///
/// Derived from the [`SkillTestSpec`] **unstamped**: the test it belongs to
/// has no identity until initiation mints one, and it becomes a
/// [`RecordedModifier`](crate::state::RecordedModifier) with
/// [`Lifetime::SkillTest`](crate::state::Lifetime::SkillTest) there. The
/// delta stays an [`IntExpr`](card_dsl::dsl::IntExpr) — a row is evaluated at
/// read time, not at push time (ADR 0005), which is what makes Esoteric
/// Formula 02254's *"+2 \[willpower\] … for each clue on the attacked
/// enemy"* expressible later.
///
/// One consequence of not evaluating at initiation: an expression the
/// evaluator cannot resolve does not reject the initiating effect: it is
/// skipped at read time under the recorded population's silent-skip policy
/// (loudly in debug — `collect_recorded` `debug_assert`s on it). Nothing
/// can write one today: both DSL modifiers carry literals, and the only
/// other `IntExpr` in reach is a `Count`, which always resolves.
#[derive(Debug, Clone)]
struct InitiatorModifier {
    /// What it modifies — the controller for a combat bonus, the
    /// investigated location for a shroud reduction.
    target: ModifierTarget,
    /// Which quantity of that target.
    stat: Stat,
    /// The signed magnitude, evaluated at every read.
    delta: IntExpr,
}

/// The skill test to start, named by its kind. Each variant carries only what
/// that kind can vary, and [`start_skill_test`] derives the rest from it: the
/// tested skill, the [`SkillTestKind`], the [`DifficultyBasis`], the
/// [`SkillTestFollowUp`], the initiator's one-shot modifier and the reveal
/// clause. A Fight against a shroud, or an Investigate with a Fight
/// follow-up, cannot be written.
#[derive(Debug, Clone)]
pub(in crate::engine) enum SkillTestSpec {
    /// An attack on `enemy`: a Combat test (or the skill the modification
    /// tests instead) against that enemy's modified fight value, dealing
    /// damage on success.
    Fight {
        /// The attacked enemy, whose fight value is the difficulty.
        enemy: EnemyId,
        /// How the attack differs from a basic Fight.
        modification: FightModification,
    },
    /// An evasion attempt on `enemy`: an Agility test (or the skill the
    /// modification tests instead) against that enemy's modified evade value,
    /// evading it on success.
    Evade {
        /// The enemy being evaded, whose evade value is the difficulty.
        enemy: EnemyId,
        /// How the attempt differs from a basic Evade.
        modification: EvadeModification,
    },
    /// An investigation of `location`: an Intellect test against its
    /// modified shroud, discovering a clue on success unless the modification
    /// replaces the discovery.
    Investigate {
        /// The investigated location, whose shroud is the difficulty.
        location: LocationId,
        /// How the investigation differs from a basic Investigate.
        modification: InvestigateModification,
    },
    /// A test of `skill` against a printed difficulty, with no action
    /// follow-up: a Revelation's printed test ([`Effect::SkillTest`]), and
    /// [`perform_skill_test`]'s.
    Plain {
        /// The tested skill.
        skill: SkillKind,
        /// The printed difficulty; a negative one is rejected.
        difficulty: i8,
        /// Resolved at ST.7 on success.
        on_success: Option<Effect>,
        /// Resolved at ST.7 on failure.
        on_fail: Option<Effect>,
        /// The investigator an enclosing
        /// [`Effect::ChooseInvestigator`]
        /// kept, carried into the test so `on_success` / `on_fail` read it
        /// at ST.7 ([`InFlightSkillTest::kept_investigator`]). `None` outside
        /// one.
        kept_investigator: Option<InvestigatorId>,
    },
}

/// Everything [`start_skill_test`] derives from a [`SkillTestSpec`].
struct DerivedSkillTest {
    skill: SkillKind,
    kind: SkillTestKind,
    difficulty_basis: DifficultyBasis,
    follow_up: SkillTestFollowUp,
    on_success: Option<Effect>,
    on_fail: Option<Effect>,
    kept_investigator: Option<InvestigatorId>,
    initiator_modifier: Option<InitiatorModifier>,
    reveal_clause: Option<RevealClause>,
}

impl SkillTestSpec {
    /// Derive the test this spec names, for `investigator` to make.
    fn derive(self, investigator: InvestigatorId) -> DerivedSkillTest {
        match self {
            Self::Fight {
                enemy,
                modification:
                    FightModification {
                        combat_modifier,
                        extra_damage,
                        skill_override,
                        reveal_clause,
                    },
            } => {
                let skill = skill_override.unwrap_or(SkillKind::Combat);
                DerivedSkillTest {
                    skill,
                    kind: SkillTestKind::Fight,
                    difficulty_basis: DifficultyBasis::Fight(enemy),
                    follow_up: SkillTestFollowUp::Fight {
                        enemy,
                        extra_damage,
                    },
                    on_success: None,
                    on_fail: None,
                    kept_investigator: None,
                    // "+N [combat] for this attack" — a row over the
                    // controller's tested skill, scoped to the test about to
                    // start.
                    initiator_modifier: combat_modifier.map(|delta| InitiatorModifier {
                        target: ModifierTarget::Investigator(investigator),
                        stat: modified_value::stat_for_skill(skill),
                        delta,
                    }),
                    reveal_clause,
                }
            }
            Self::Evade {
                enemy,
                modification:
                    EvadeModification {
                        skill_override,
                        reveal_clause,
                        on_success,
                    },
            } => DerivedSkillTest {
                skill: skill_override.unwrap_or(SkillKind::Agility),
                kind: SkillTestKind::Evade,
                // The difficulty *is* the enemy's modified evade value, read at
                // ST.6 — Cold Spring Glen 02244's "Each enemy in Cold Spring
                // Glen gets -1 evade" reaches this test.
                difficulty_basis: DifficultyBasis::Evade(enemy),
                follow_up: SkillTestFollowUp::Evade { enemy },
                on_success,
                on_fail: None,
                kept_investigator: None,
                initiator_modifier: None, // no Evade designator prints a skill modifier
                reveal_clause,
            },
            Self::Investigate {
                location,
                modification:
                    InvestigateModification {
                        shroud_modifier,
                        instead_of_discovery,
                    },
            } => DerivedSkillTest {
                skill: SkillKind::Intellect,
                kind: SkillTestKind::Investigate,
                difficulty_basis: DifficultyBasis::Shroud(location),
                // The discovery *is* the Investigate follow-up, so a
                // replacement for it leaves no follow-up to run.
                follow_up: if instead_of_discovery.is_some() {
                    SkillTestFollowUp::None
                } else {
                    SkillTestFollowUp::Investigate
                },
                on_success: instead_of_discovery,
                on_fail: None,
                kept_investigator: None,
                // "Your location gets -2 shroud for this investigation" — a row
                // over the *location*, scoped to the test about to start.
                initiator_modifier: shroud_modifier.map(|delta| InitiatorModifier {
                    target: ModifierTarget::Location(location),
                    stat: Stat::Shroud,
                    delta,
                }),
                reveal_clause: None,
            },
            Self::Plain {
                skill,
                difficulty,
                on_success,
                on_fail,
                kept_investigator,
            } => DerivedSkillTest {
                skill,
                kind: SkillTestKind::Plain,
                difficulty_basis: DifficultyBasis::Fixed(difficulty),
                follow_up: SkillTestFollowUp::None,
                on_success,
                on_fail,
                kept_investigator,
                initiator_modifier: None,
                reveal_clause: None,
            },
        }
    }
}

/// Start the skill test `spec` names, for `investigator` to make, from
/// `source` (the ability that initiated it, if any).
pub(in crate::engine) fn start_skill_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    spec: SkillTestSpec,
    source: Option<AbilitySource>,
) -> EngineOutcome {
    let DerivedSkillTest {
        skill,
        kind,
        difficulty_basis,
        follow_up,
        on_success,
        on_fail,
        kept_investigator,
        initiator_modifier,
        reveal_clause,
    } = spec.derive(investigator);
    // Validate-first: investigator must exist and be Active; chaos
    // bag must be non-empty so we can draw; difficulty must be non-
    // negative (FFG difficulties are always ≥ 0). Defeated
    // investigators can't take skill tests — they're out of play.
    // A second test cannot overlap an in-flight one.
    let Some(inv) = cx.state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("skill test: investigator {investigator:?} not in state").into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill test: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if cx.state.chaos_bag.tokens.is_empty() {
        return EngineOutcome::Rejected {
            reason: "skill test requires a non-empty chaos bag".into(),
        };
    }
    // A printed difficulty is the one difficulty an initiator supplies as a
    // number, so it is the one that can be malformed. A basis over a
    // location or an enemy reads a live quantity, which the query clamps at
    // zero after every modifier — there is nothing to reject here, and
    // rejecting on the *unmodified* value would refuse a test a card is
    // entitled to make easy.
    if let DifficultyBasis::Fixed(n) = difficulty_basis {
        if n < 0 {
            return EngineOutcome::Rejected {
                reason: format!("skill test: difficulty {n} must be >= 0").into(),
            };
        }
    }
    if cx.state.has_skill_test_in_flight() {
        return EngineOutcome::Rejected {
            reason: "skill test: another skill test is already in flight; only one test \
                     may pause at a commit window at a time"
                .into(),
        };
    }

    // Mutate-second: stash the in-flight record and announce the test.
    // Snapshot the investigator's location for
    // `LocationTarget::TestedLocation` resolution during
    // `Trigger::OnSkillTestResolution` firing. `inv`'s immutable
    // borrow from the validation block above is still live; reading
    // `current_location` here doesn't extend it past this line.
    let tested_location = inv.current_location;
    // Mint this test's identity. Every test-scoped modifier bought
    // while it runs is stamped with it, and its teardown expires exactly the
    // rows carrying it — which is what makes "for *this* skill test" mean a
    // particular test rather than "the next one to come along".
    let id = cx.state.skill_test_ids.mint();
    // Push the SkillTest frame up front (carrying the test's data), so the
    // in-flight test has a home from test start. Safe: all validation precedes
    // this point, and what follows only asks (the Mind-over-Matter
    // substitution prompt, pushed for the loop to surface, or the commit
    // window). During the substitution prompt this frame sits beneath the
    // `SubstitutionPrompt` frame, which top-frame dispatch routes first.
    cx.state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            id,
            investigator,
            skill,
            kind,
            difficulty_basis,
            committed: Vec::new(),
            tested_location,
            follow_up,
            on_fail,
            on_success,
            source,
            kept_investigator,
            continuation: SkillTestStep::PreCommitWindow,
            bonus_attack_damage: 0,
            bonus_clues_discovered: 0,
            resolved: None,
            revealed: None,
            reveal_clause,
            discard_after: None,
        }));
    // The initiating effect's one-shot modifier becomes a row scoped to the
    // test just minted — the weapon's "+N for this attack" and Flashlight's
    // "-2 shroud for this investigation" alike. Recorded rather than folded
    // into a snapshot, so ST.5 and ST.6 both re-derive it (ADR 0005), and
    // stamped here because this is the first moment there is an identity to
    // stamp it with.
    if let Some(modifier) = initiator_modifier {
        cx.state
            .recorded_modifiers
            .push(RecordedModifier::targeting(
                modifier.target,
                investigator,
                modifier.stat,
                modifier.delta,
                Lifetime::SkillTest(id),
                // A row names its origin as an instance, not as an ability
                // source: it is provenance for a modifier, and the per-instance
                // limit-keying it will feed (Fire Axe 02032's *"Limit three
                // times per attack"*) has nothing to key on for a board source.
                // Narrowing here rather than widening `RecordedModifier` keeps
                // the projection at the row's own boundary.
                source.and_then(AbilitySource::instance),
            ));
    }
    // The announced difficulty is the modified difficulty as it stands at
    // ST.1 — a display value for the client, not the number the test
    // resolves against. ST.6 re-reads it (`determine_outcome_step`), so a
    // card that changes it mid-test changes the outcome without changing
    // what was announced.
    //
    // TODO(#1099): the announced skill is the test's skill before the
    // substitution prompt below. When Mind over Matter 01036 substitutes
    // Intellect, `SkillTestStarted` has already announced Combat or Agility.
    cx.events.push(Event::SkillTestStarted {
        investigator,
        skill,
        difficulty: current_difficulty(cx.state, cx.cards),
    });

    // "Use X in place of Y?" (Mind over Matter 01036): if this is a Combat or
    // Agility test and the investigator has a covering round-scoped
    // substitution active, offer the choice BEFORE the commit window — the
    // test type is fixed here (per the card's FAQ). The in-flight record (just
    // created) is the parking; `resume_substitution_choice` rewrites its skill
    // on "yes". Routed via a `SubstitutionPrompt` frame above the `SkillTest`.
    if substitution_covers(cx.state, investigator, skill) {
        // The `drive` loop surfaces the choice from the frame
        // ([`substitution_request`]).
        cx.state
            .continuations
            .push(SubstitutionPromptFrame { investigator });
        return EngineOutcome::Done;
    }
    // No substitution: drive the test. `advance` parks at `AwaitingCommit`,
    // emitting the commit prompt (which propagates up to here).
    advance(cx)
}

/// Register `substitution` for the rest of the round: a later test of one of
/// its `for_skills` by its investigator offers the "use X in place of Y?"
/// choice at initiation. A card-local native (Mind over Matter 01036) reaches
/// it as
/// [`NativeCx::substitute_skill_until_end_of_round`](crate::engine::NativeCx::substitute_skill_until_end_of_round).
/// Stamped [`Lifetime::Round`]: Mind over Matter's is the only substitution in
/// scope, and it lasts *"Until the end of the round"*. No event announces it.
pub(crate) fn substitute_skill_until_end_of_round(cx: &mut Cx, substitution: SkillSubstitution) {
    cx.state.lasting_effects.push(LastingEffect {
        lifetime: Lifetime::Round,
        kind: LastingEffectKind::SkillSubstitution(substitution),
    });
}

/// Send the in-flight test's committed card at `index` back to hand at RR ST.8
/// instead of the discard pile, by setting its
/// [destination](CommittedCard::destination). A card-local native reaches it
/// as
/// [`NativeCx::return_committed_card_to_hand`](crate::engine::NativeCx::return_committed_card_to_hand),
/// with the index its effect was bound to
/// ([`EvalContext::committed_card`]). Nothing moves until ST.8, which announces
/// it.
///
/// A missing test or card is a caller bug: `debug_assert!`, and nothing
/// changes in release.
pub(crate) fn return_committed_card_to_hand(cx: &mut Cx, index: u8) {
    let card = cx
        .state
        .current_skill_test_mut()
        .and_then(|t| t.committed.get_mut(usize::from(index)));
    debug_assert!(
        card.is_some(),
        "return_committed_card_to_hand: no committed card at {index} on an in-flight test",
    );
    if let Some(card) = card {
        card.destination = CommittedCardDestination::Hand;
    }
}

/// Mark the in-flight test to discard `card` from play as it ends, by setting
/// its [`discard_after`](InFlightSkillTest::discard_after). A card-local native
/// reaches it as
/// [`NativeCx::discard_after_this_test`](crate::engine::NativeCx::discard_after_this_test)
/// (Baseball Bat 01074's reveal clause). Nothing moves until the teardown,
/// which announces the discard.
///
/// A missing test is a caller bug: `debug_assert!`, and nothing changes in
/// release.
pub(crate) fn discard_after_this_test(cx: &mut Cx, card: CardInstanceId) {
    let test = cx.state.current_skill_test_mut();
    debug_assert!(
        test.is_some(),
        "discard_after_this_test: no skill test in flight to mark {card:?} on",
    );
    if let Some(test) = test {
        test.discard_after = Some(card);
    }
}

/// Discard the card the test being torn down was marked to discard
/// ([`InFlightSkillTest::discard_after`]), if any, through the leave-play
/// router. Called by [`tear_down_test`] at the ST.8 end and at the
/// target-left-play abandonment, with the frame still on the stack.
///
/// A card already gone from play (an effect discarded it earlier in the test)
/// is left where it went; one that cannot leave play right now stays in play
/// (`glossary/Cannot.md`: *"The word "cannot" is absolute, and cannot be
/// countermanded by other abilities."*). Either way nothing is announced.
fn discard_marked_card(cx: &mut Cx) {
    let Some(card) = cx.state.current_skill_test().and_then(|t| t.discard_after) else {
        return;
    };
    match board::discard_from_play(cx, card) {
        Ok(_) | Err(NotLeft::NotInPlay | NotLeft::Refused) => {}
    }
}

/// Whether `investigator` has an active substitution covering a `skill` test
/// (Mind over Matter 01036: Intellect for Combat/Agility).
fn substitution_covers(state: &GameState, investigator: InvestigatorId, skill: SkillKind) -> bool {
    state.lasting_effects.iter().any(|e| match &e.kind {
        LastingEffectKind::SkillSubstitution(s) => {
            s.investigator == investigator && s.for_skills.contains(&skill)
        }
        _ => false,
    })
}

/// What a [`SubstitutionPromptFrame`] asks: whether to use Intellect in place of
/// the skill of the test beneath it. One of the
/// [prompt table](super::prompt::request)'s helpers.
///
/// A decision, not a selection (ADR 0015): the two options are the
/// alternatives Mind over Matter 01036 offers, not board entities. It is
/// un-anchored because the event has left play by now — the ruling has it
/// played "before the skill test begins" (<https://arkhamdb.com/card/01036>).
///
/// # Panics
///
/// If no skill test is in flight. The frame is pushed above the test it
/// substitutes in, so one always is.
pub(super) fn substitution_request(
    state: &GameState,
    frame: &SubstitutionPromptFrame,
) -> InputRequest {
    let investigator = frame.investigator;
    let skill = state
        .current_skill_test()
        .expect("substitution_request: a substitution prompt sits above its skill test")
        .skill;
    let use_skill = skill_name(SkillKind::Intellect); // sole substitution in scope
    let skill = skill_name(skill);
    choice::effect_request(&choice::decision_prompt(
        format!(
            "{}: use {use_skill} in place of {skill} for this test?",
            choice::investigator_name(state, investigator),
        ),
        vec![format!("Use {use_skill}"), format!("Keep {skill}")],
        None,
    ))
}

/// A skill's name as prompt text prints it.
fn skill_name(skill: SkillKind) -> &'static str {
    match skill {
        SkillKind::Willpower => "Willpower",
        SkillKind::Intellect => "Intellect",
        SkillKind::Combat => "Combat",
        SkillKind::Agility => "Agility",
    }
}

/// Resume the Mind over Matter substitution prompt: `PickSingle(0)`
/// rewrites the in-flight test to an Intellect test (dropping any weapon combat
/// bonus per the ruling "ignore any bonuses to Combat or Agility",
/// <https://arkhamdb.com/card/01036>); `PickSingle(1)`
/// keeps the printed skill (a genuine "may" — a player may decline to fail on
/// purpose). Either way, parks for the `drive` loop, which opens the next window
/// (the ST.1 player window, or the commit prompt on auto-skip).
pub(in crate::engine) fn resume_substitution_choice(
    cx: &mut Cx,
    response: &InputResponse,
) -> EngineOutcome {
    let InputResponse::PickSingle(OptionId(opt)) = response else {
        return EngineOutcome::Rejected {
            reason: "substitution prompt expects PickSingle(0|1)".into(),
        };
    };
    if *opt > 1 {
        return EngineOutcome::Rejected {
            reason: format!("substitution prompt: PickSingle({opt}) out of range (0|1)").into(),
        };
    }
    // Pop the SubstitutionPrompt frame we validated against (it is the top
    // frame, above the SkillTest frame the mutation below reaches).
    cx.state
        .continuations
        .pop_expect::<SubstitutionPromptFrame>();
    if *opt == 0 {
        // Use Intellect: the test becomes an Intellect test (base / icons /
        // bonuses all key off `skill`). A weapon's combat bonus falls away on
        // its own — it is a row over `Stat::Combat`, and an Intellect test
        // never reads one — which is the FAQ's "ignore any bonuses to Combat
        // or Agility" without a second mechanism to keep in step. Bonus damage
        // (`extra_damage` / `bonus_attack_damage`) is separate and untouched.
        let t = cx
            .state
            .current_skill_test_mut()
            .expect("resume_substitution_choice: in-flight test must exist");
        t.skill = SkillKind::Intellect;
    }
    // Park: return `Done` so the `drive` loop's `SkillTest` arm drives the test
    // from its pre-commit cursor — opening the ST.1 player window, then
    // (on auto-skip) the commit prompt reading the now-possibly-rewritten skill,
    // or parking at the window if a Fast play is available. The frame is on top
    // and `resolve_input`'s caller drives it.
    EngineOutcome::Done
}

/// Commit-stage entry to the skill-test resolution driver. Handles the
/// response to an ST.2 commit prompt: a helper's
/// ([`helper_commit_request`], at [`SkillTestStep::AwaitingHelperCommit`]) or
/// the performer's ([`commit_request`], at [`SkillTestStep::AwaitingCommit`]).
/// Map the selected options back to the hand cards the prompt offered,
/// validate the set, and append it to the in-flight test's committed list.
/// Then pre-advance the cursor: past a helper's answer to
/// [`SkillTestStep::NextHelper`] after them, past the performer's to
/// [`SkillTestStep::PreTokenWindow`]. Returns [`EngineOutcome::Done`] so the
/// `drive` loop's `SkillTest` arm runs the remaining steps.
///
/// On invalid input (no in-flight test, an option not offered or named twice,
/// a helper's pick of more than one card, a set over a card's commit limit, or
/// a cursor not at a commit prompt) returns [`EngineOutcome::Rejected`] with no
/// state change and no events pushed — the engine stays paused so the caller
/// can submit a fixed-up response.
pub(super) fn finish_skill_test(cx: &mut Cx, selected: &[OptionId]) -> EngineOutcome {
    let Some(in_flight) = cx.state.current_skill_test() else {
        return EngineOutcome::Rejected {
            reason: "skill-test commit: no in-flight skill test to resume".into(),
        };
    };
    let performer = in_flight.investigator;
    let (committer, next) = match in_flight.continuation {
        SkillTestStep::AwaitingHelperCommit { helper } => {
            // ST.2: a helper *"may commit one card"*. A request's count is not
            // enforced generically, so the one-card limit is checked here.
            if selected.len() > 1 {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "skill-test commit: a helper may commit one card to another \
                         investigator's test, but {} were picked",
                        selected.len(),
                    )
                    .into(),
                };
            }
            (
                helper,
                SkillTestStep::NextHelper {
                    after: Some(helper),
                },
            )
        }
        SkillTestStep::AwaitingCommit => (performer, SkillTestStep::PreTokenWindow),
        other => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "skill-test commit: commit window already closed (continuation {other:?}); \
                     the engine is mid-resolution, not at the commit step",
                )
                .into(),
            };
        }
    };
    // The skill as the commit window sees it: a substitution (Mind over Matter
    // 01036) has already rewritten it by now, and ST.2's icon check matches
    // against the skill actually being tested.
    let skill = in_flight.skill;

    // Map the selection back to the hand cards the prompt offered, and check
    // the set against the commit limits. On Err, state is untouched and the
    // engine stays paused so the client can retry.
    let by = CommitBy {
        committer,
        performer,
    };
    let candidates = commit_candidates(cx.state, cx.cards, by, skill);
    let indices_u8 = match hand_pick::picked(selected, &candidates, "skill-test commit")
        .and_then(|indices| check_commit_limits(cx.state, cx.cards, committer, indices))
    {
        Ok(v) => v,
        Err(rejected) => return rejected,
    };

    // Mutate-second. Move the named cards out of the hand and into limbo (RR
    // glossary "Limbo": a card committed to a skill test "is no longer
    // considered to be in any investigator's hand").
    //
    // Snapshot in the order the player submitted the indices — that is the
    // order the ST.7 result modifiers apply in, and the order ST.8 discards
    // in, so it is the player's to choose. The additions are not tied to it:
    // RR ST.7, *"If there are multiple results to be applied during this
    // step, the investigator performing the test applies those results in
    // the order of his or her choice"*, so the performer picks them one at a
    // time, each named by its card's position in this list. The drain then
    // walks descending, which keeps the still-pending positions stable.
    // Both steps are index-safe: `hand_pick::picked` proved every index unique
    // and one the window offered from this same, so-far-unmutated hand.
    //
    // From here on nothing re-indexes the hand for a committed card, so the
    // ST.2→ST.3 player window is free to mutate it.
    let inv = cx
        .state
        .investigators
        .get_mut(&committer)
        .expect("the prompt offered cards from the committer's hand");
    let picked: Vec<CommittedCard> = indices_u8
        .iter()
        .map(|&i| CommittedCard::new(inv.hand[usize::from(i)].clone(), committer))
        .collect();
    let mut descending = indices_u8;
    descending.sort_unstable_by(|a, b| b.cmp(a));
    for i in descending {
        inv.hand.remove(usize::from(i));
    }

    // Append the committed cards (helpers' first, in queue order, then the
    // performer's) and pre-advance the cursor, then park: return `Done` so the
    // `drive` loop's `SkillTest` arm (dispatch/mod.rs) drives on from there.
    // The frame stays on top and `resolve_input`'s caller drives it
    // (apply_player_action runs `drive` after this returns).
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above");
    t.committed.extend(picked);
    t.continuation = next;
    EngineOutcome::Done
}

/// Resume the cosmetic acknowledgment pause: the player has Confirmed the
/// skill-test result. Validate-first — the in-flight test's cursor must be at
/// [`SkillTestStep::AcknowledgeOutcome`] — then advance it to
/// [`SkillTestStep::ApplyResultModifiers`] and return [`EngineOutcome::Done`] so the
/// caller's `drive` loop runs the ST.7 consequences. Mirrors
/// [`finish_skill_test`]'s park-and-return-`Done` shape; on a bad cursor or no
/// in-flight test it rejects with state and events untouched.
pub(super) fn acknowledge_outcome(cx: &mut Cx) -> EngineOutcome {
    let Some(t) = cx.state.current_skill_test() else {
        return EngineOutcome::Rejected {
            reason: "skill-test acknowledge: no in-flight skill test to resume".into(),
        };
    };
    if !matches!(t.continuation, SkillTestStep::AcknowledgeOutcome) {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill-test acknowledge: not at the acknowledge step (continuation {:?})",
                t.continuation,
            )
            .into(),
        };
    }
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above")
        .continuation = SkillTestStep::ApplyResultModifiers;
    EngineOutcome::Done
}

/// Run the token half of a skill test (RR ST.3–ST.4): reveal the chaos token
/// (ST.3), whose resolve step ([`resolve_reveal`]) pushes the revealed
/// symbol's unconditional [`immediate`](card_dsl::dsl::TokenOutcome::immediate)
/// effect (ST.4).
///
/// **No total, margin, or verdict is computed here.** ST.5 (*"all active card
/// abilities that are modifying the investigator's skill value"* — active *at
/// ST.5*) and ST.6 run one driver step later, at
/// [`DetermineOutcome`](SkillTestStep::DetermineOutcome), because the ST.4
/// effects this step pushes can change the board the ST.5 sum reads: a
/// `[tablet]` in The Gathering deals 1 damage when a Ghoul is present, that
/// damage can be soaked onto a Beat Cop 01018 (*"You get +1 \[combat\]."*,
/// printed health 2), and a Beat Cop discarded at ST.4 must not still be
/// contributing its `+1` at ST.5. What this step *does* write down is
/// the token's own contribution, as a [`RecordedModifier`] scoped to this
/// test — see [`record_token_contribution`] — so the ST.5 total is one query
/// rather than a query plus an integer the driver adds afterwards.
///
/// **Both steps are skipped outright when the test's determination is already
/// known.** `glossary/Automatic_Failure_Success.md`:
///
/// > If it is known that an investigator automatically succeeds or fails at a
/// > skill test before Step 3 ("Reveal chaos token") occurs, that step is
/// > skipped, along with Step 4. No chaos token(s) are revealed from the chaos
/// > bag, and the investigator immediately moves to Step 5. All other steps of
/// > the skill test resolve as normal.
/// >
/// > If a chaos token effect causes an investigator to automatically succeed or
/// > fail at a skill test, continue with Steps 3 and 4, as normal.
///
/// The two clauses are one question asked at two moments, not two mechanisms
/// (ADR 0007): this step asks
/// [`test_determination`](crate::engine::modified_value::test_determination)
/// *before* the draw, so a row latched at ST.1/ST.2 (Three Aces 06199 —
/// *"that test automatically succeeds <i>(do not reveal chaos tokens from the
/// chaos bag)</i>"*) skips the draw, while the `[auto_fail]` token's own row —
/// written by [`record_token_contribution`] below, after the reveal — has
/// nothing left to skip. Nothing is recorded for the skip: there is no
/// `EngineRecord` for a chaos draw, so a skip that is a pure function of game
/// state replays identically.
///
/// **This is the draw step; the token's effect on the test is a step of its
/// own.** The token is drawn and announced here, and then the
/// [`ChaosTokenRevealed`](TimingEvent::ChaosTokenRevealed) condition is
/// emitted in tail position. Its resolve step, [`resolve_reveal`], does the
/// rest of ST.3 and pushes ST.4 — after its `when` cell, where a cancel (Wendy
/// Adams 01005) skips it and [`Effect::RevealChaosToken`] sets the cursor back
/// here for a fresh draw. A test whose determination is known skips before the
/// draw, so it opens no `when` cell.
///
/// The token is drawn once per pass; the cursor is pre-advanced to
/// `DetermineOutcome` before the emit, so a soak window suspending inside the
/// ST.4 effects resumes there and re-draws nothing. As it comes out it is
/// substituted, once, by any [`Effect::TreatRevealedToken`] on the performer's
/// cards ([`counted_token`]): The Necronomicon 01009 counts an `[elder_sign]`
/// as an `[auto_fail]`. Every later read — the resolution, the chaos symbol
/// effect at ST.4 and ST.7, and the reveal clause — takes the counted token, so
/// a substituted elder sign never initiates the investigator's elder-sign
/// ability. [`ChaosTokenRevealed`](Event::ChaosTokenRevealed) names the token
/// drawn with the counted token's resolution; the drawn token, the counted one
/// and that resolution ride the timing event to the resolve step, which keeps
/// them on [`revealed`](InFlightSkillTest::revealed). The skip above, and a
/// cancelled token, leave it `None`. The outcome's result-conditional parts
/// (Cultist 01104's `on_fail` horror) are not kept: the ST.7
/// [`ApplyResults`](SkillTestStep::ApplyResults) step looks the outcome up
/// again from the counted token and picks one part by the (by then known)
/// result.
///
/// A scenario symbol's modifier is an [`IntExpr`]. The recorded row keeps it
/// unevaluated, so ST.5 reads it live; the `i8` that
/// [`ChaosTokenRevealed`](Event::ChaosTokenRevealed) announces is the same
/// expression evaluated once, here, at reveal — a display value, as
/// `SkillTestStarted`'s announced difficulty is. The two agree for every
/// `IntExpr::Lit` a scenario returns today. A live expression, as The
/// Gathering's `[skull]` would be once it counts Ghouls at ST.5, could drift
/// between reveal and ST.5, and the ST.5 read is the one the test resolves
/// against.
fn run_resolution(cx: &mut Cx, investigator: InvestigatorId) {
    // Known already? Then ST.3 and ST.4 do not happen at all: no draw, no
    // `ChaosTokenRevealed`, no symbol effects — straight to ST.5. Every other
    // step of the test still runs, so the cursor advances exactly as the
    // drawing path leaves it.
    if modified_value::test_determination(cx.state, ReadContext::from_state(cx.state)).is_some() {
        cx.state
            .current_skill_test_mut()
            .expect("run_resolution: the SkillTest frame must exist")
            .continuation = SkillTestStep::DetermineOutcome;
        return;
    }

    // ST.3 reveal the chaos token, and look up the chaos symbol effect it
    // initiates for the announcement.
    let token_idx = cx.state.rng.next_index(cx.state.chaos_bag.tokens.len());
    let token = cx.state.chaos_bag.tokens[token_idx];
    // From here on the test reads the token it counts as, never the drawn one.
    let counts_as = counted_token(cx, token, investigator);
    let printed = state::resolve_token(counts_as, &cx.state.token_modifiers);
    let (outcome, ctx) = chaos_symbol_effect(cx, counts_as, investigator);
    let resolution = match (&outcome, counts_as) {
        // The elder sign keeps its own resolution label.
        (_, ChaosToken::ElderSign) | (None, _) => printed,
        // A scenario symbol's modifier is evaluated once for the
        // announcement only; the row below stays the unevaluated
        // expression. An unexpressible condition contributes nothing to
        // the fold, so it announces `0`.
        (Some(o), _) => TokenResolution::Modifier(
            evaluator::eval_int_expr(cx.state, cx.cards, &ctx, &o.modifier).unwrap_or(0),
        ),
    };
    cx.events
        .push(Event::ChaosTokenRevealed { token, resolution });

    // Pre-advance to DetermineOutcome BEFORE emitting, so the test resumes
    // there once the reveal's sequence has drained (ADR 0003) and never
    // re-draws the token. A `when`-cell redraw sets it back to `Resolving`.
    cx.state
        .current_skill_test_mut()
        .expect("run_resolution: the SkillTest frame must exist")
        .continuation = SkillTestStep::DetermineOutcome;
    // Queued in tail position: the driver's loop sees the coordinator on top
    // and yields to it, so the outcome (always a queued `Done`) has nothing
    // to carry.
    #[expect(
        clippy::let_underscore_must_use,
        reason = "this fn returns `()` and the driver loop owns the suspension channel: it yields \
                  to the queued coordinator, so the outcome (always `Done`) has nothing to carry"
    )]
    let _ = emit::queue_event(
        cx,
        &TimingEvent::ChaosTokenRevealed {
            investigator,
            token: RevealedToken {
                token,
                counts_as,
                resolution,
            },
        },
    );
}

/// The resolve step of the [`ChaosTokenRevealed`](TimingEvent::ChaosTokenRevealed)
/// condition: what Skill Test Timing step 3 does with a token once it is out,
/// and step 4's symbol effect. Reached only when no `when`-cell ability
/// cancelled the token, so a cancelled token writes nothing and every symbol
/// trigger misses it — Wendy Adams 01005's ruling
/// (<https://arkhamdb.com/card/01005>): *"treat it as if you've never revealed
/// that token, for purposes of effects that trigger if a specific symbol is
/// revealed"*.
///
/// In order: the token's contribution becomes a recorded row (or the
/// automatic failure an `[auto_fail]` latches), the frame records the token
/// as [`revealed`](InFlightSkillTest::revealed), the ST.4 immediate effect is
/// pushed, and the initiating card's reveal clause is pushed last so it
/// resolves first. The chaos symbol effect is looked up again here from the
/// counted token, off the board as the `when` cell left it. Nothing else is
/// stashed: the outcome's result-conditional parts (Cultist horror, an elder
/// sign's "If you succeed") are looked up again from `revealed` at ST.7, once
/// the result that picks between them is known.
///
/// The test's cursor is already at `DetermineOutcome`, and the pushed effects
/// sit above the coordinator, so a soak suspend inside them resumes into the
/// reveal's `at` cell and then the test's ST.5, never re-drawing the token.
pub(super) fn resolve_reveal(cx: &mut Cx, investigator: InvestigatorId, revealed: RevealedToken) {
    let RevealedToken {
        counts_as,
        resolution,
        ..
    } = revealed;
    let (outcome, ctx) = chaos_symbol_effect(cx, counts_as, investigator);
    record_token_contribution(
        cx,
        investigator,
        resolution,
        outcome.as_ref(),
        ctx.source_instance(),
    );
    cx.state
        .current_skill_test_mut()
        .expect("resolve_reveal: the SkillTest frame must exist")
        .revealed = Some(revealed);

    // ST.4 push the outcome's immediate part (Tablet damage when Ghouls are
    // present; an `Effect::Deal` routes through the interactive soak and may
    // suspend). If pushed, it becomes the top frame and the loop resumes at
    // DetermineOutcome.
    if let Some(effect) = outcome.as_ref().and_then(|o| o.immediate.as_ref()) {
        evaluator::push_effect(cx, effect, ctx);
    }

    // ST.3 the initiating card's reveal clause, pushed last so it resolves
    // first: Shrivelling 01060's ruling (https://arkhamdb.com/card/01060),
    // *"The "If a symbol is revealed..." effect triggers during Step 3 of the
    // Skill Test Timing."*, ahead of the ST.4 symbol effect above.
    let t = cx
        .state
        .current_skill_test()
        .expect("resolve_reveal: the SkillTest frame must exist");
    if let (Some(clause), Some(symbol)) = (&t.reveal_clause, counts_as.symbol()) {
        if clause.fires_on(symbol) {
            let effect = clause.effect.clone();
            let ctx = EvalContext::for_controller_with_optional_source(investigator, t.source);
            evaluator::push_effect(cx, &effect, ctx);
        }
    }
}

/// [`Effect::RevealChaosToken`]: set the skill test in flight back to its draw
/// step, so once the cancelled reveal's sequence is abandoned the driver draws
/// a new token and opens a fresh `when` cell for it — Wendy Adams 01005's
/// *"Reveal a new chaos token."* The cancelled token needs no returning: a draw
/// indexes the bag and never removes from it.
///
/// Rejected with no test in flight. A redraw after the token has already
/// resolved is a malformed card, not a game state: the effect belongs in the
/// reveal's `when` cell, before [`resolve_reveal`] runs.
pub(in crate::engine) fn reveal_new_token(cx: &mut Cx) -> EngineOutcome {
    let Some(test) = cx.state.current_skill_test_mut() else {
        return EngineOutcome::Rejected {
            reason: "RevealChaosToken resolved with no skill test in flight: there is no test \
                     to reveal a chaos token for."
                .into(),
        };
    };
    debug_assert!(
        test.revealed.is_none(),
        "Effect::RevealChaosToken after the token resolved — a card revealing a new token \
         outside the reveal's `when` cell"
    );
    test.continuation = SkillTestStep::Resolving;
    EngineOutcome::Done
}

/// The token a `drawn` chaos token counts as when `investigator` reveals it:
/// the `to` of the first in-effect
/// [`Effect::TreatRevealedToken`] on `investigator`'s own cards whose `from`
/// is the drawn symbol, or `drawn` itself when none applies.
///
/// The Necronomicon 01009: *"Treat each \[elder_sign\] you reveal on a chaos
/// token as a \[auto_fail\]."* — *"you reveal"*, so the constant is read off
/// the performer's cards ([`constants_of`](abilities_in_effect::constants_of):
/// investigator card, play area, threat area) and no one else's. A numeric
/// token shows no symbol, so nothing substitutes it. One substitution, not a
/// chain: the counted symbol is not looked up again.
fn counted_token(cx: &Cx, drawn: ChaosToken, investigator: InvestigatorId) -> ChaosToken {
    let Some(symbol) = drawn.symbol() else {
        return drawn;
    };
    abilities_in_effect::constants_of(cx.state, cx.cards, investigator)
        .into_iter()
        .find_map(|(_, ability)| match ability.effect {
            Effect::TreatRevealedToken { from, to } if from == symbol => Some(ChaosToken::from(to)),
            _ => None,
        })
        .unwrap_or(drawn)
}

/// The chaos symbol effect a revealed `token` initiates, with the context its
/// parts evaluate under — `None` for a token that initiates none.
///
/// A scenario symbol's is the scenario reference card's, through the
/// scenario module's [`resolve_symbol`](scenario::ScenarioModule::resolve_symbol)
/// hook, and runs with the controller-only context: the reference card is not
/// a card in play to source it from. The elder sign's is the performer's
/// investigator card's [`elder_sign_outcome`](modified_value::elder_sign_outcome),
/// sourced to that card, so whatever it latches names it.
///
/// Called at ST.3 by [`run_resolution`] and again at ST.7 by
/// [`collect_results_step`] and [`apply_result`], so each step reads the outcome as the board
/// stands when it uses it (ADR 0005) rather than a copy parked at reveal.
fn chaos_symbol_effect(
    cx: &Cx,
    token: ChaosToken,
    investigator: InvestigatorId,
) -> (Option<TokenOutcome>, EvalContext) {
    let (outcome, source) = match token {
        ChaosToken::Skull | ChaosToken::Cultist | ChaosToken::Tablet | ChaosToken::ElderThing => (
            scenario::resolve_symbol_token(cx.scenario, cx.state, token, investigator),
            None,
        ),
        ChaosToken::ElderSign => (
            modified_value::elder_sign_outcome(cx.state, cx.cards, investigator),
            cx.state
                .investigators
                .get(&investigator)
                .map(|inv| AbilitySource::InPlay(inv.investigator_card.instance_id)),
        ),
        _ => (None, None),
    };
    (
        outcome,
        EvalContext::for_controller_with_optional_source(investigator, source),
    )
}

/// Record the revealed token's contribution to this test's ST.5 total as a
/// [`RecordedModifier`] over the tested skill, scoped to this test.
///
/// ADR 0005 names *"the revealed chaos token's ±N"* among the lifetimes that
/// are recorded rather than derived from where a card sits: there is nothing
/// on the board to sweep for it, and it expires with the test. Recording it
/// is what lets the whole ST.5 total — base, every additive row, the clamp —
/// be a single fold with the clamp genuinely last, instead of a query whose
/// result the driver then adds to and clamps itself.
///
/// The three resolutions:
///
/// - [`Modifier(n)`](TokenResolution::Modifier) records `n` as a literal,
///   including `0`, so the breakdown always names the token — unless the
///   token is a scenario symbol, whose `outcome`'s modifier is recorded
///   **unevaluated** instead (`n` is only its value at reveal).
/// - [`ElderSign`](TokenResolution::ElderSign) records the modifier of the
///   investigator card's elder-sign `outcome` **unevaluated**. Roland Banks 01001's
///   *"\[elder_sign\] effect: +1 for each clue on your location."*
///   (`data/arkhamdb-snapshot/pack/core/core.json`) counts the clues on his
///   location when ST.5 reads the row, not the ones that were there when the
///   token came out — the ST.4 window sits between the two. An investigator
///   with no elder-sign ability records no row. The row is attributed to the
///   investigator card, so the ST.5 breakdown names whose ability it is.
/// - [`AutoFail`](TokenResolution::AutoFail) records a **determination**
///   rather than a modifier: it carries no ±N, and what it does instead —
///   *"the investigator's total skill value for that test is considered
///   0"* (`glossary/Automatic_Failure_Success.md`) — is the fold's stage-5
///   substitution, read through
///   [`test_determination`](crate::engine::modified_value::test_determination).
///   Recording it here rather than special-casing the token at ST.6 is what
///   makes a card-latched automatic failure
///   ([`Effect::AutoResolve`](card_dsl::dsl::Effect::AutoResolve)) the same one
///   rule and not a second one.
///
/// `source` is the instance the chaos symbol effect is sourced to: the
/// investigator card for an elder sign, `None` for every other token.
fn record_token_contribution(
    cx: &mut Cx,
    investigator: InvestigatorId,
    resolution: TokenResolution,
    outcome: Option<&TokenOutcome>,
    source: Option<CardInstanceId>,
) {
    let (test_id, skill) = {
        let t = cx
            .state
            .current_skill_test()
            .expect("record_token_contribution: the SkillTest frame must exist");
        (t.id, t.skill)
    };
    let delta = match resolution {
        TokenResolution::Modifier(n) => {
            outcome.map_or(IntExpr::Lit(n), |outcome| outcome.modifier.clone())
        }
        TokenResolution::ElderSign => {
            let Some(outcome) = outcome else {
                return;
            };
            outcome.modifier.clone()
        }
        TokenResolution::AutoFail => {
            cx.state
                .recorded_modifiers
                .push(RecordedModifier::determination(
                    investigator,
                    Determination::AutomaticFailure,
                    Lifetime::SkillTest(test_id),
                    source,
                ));
            return;
        }
    };
    cx.state.recorded_modifiers.push(RecordedModifier::new(
        investigator,
        modified_value::stat_for_skill(skill),
        delta,
        Lifetime::SkillTest(test_id),
        source,
    ));
}

/// Read the determination stashed on the in-flight test at the
/// `DetermineOutcome` step (RR ST.5–ST.6). Every later driver step reads the
/// outcome here instead of threading `succeeded`/`failed_by` through cursor
/// payloads; the `.expect` is the structural witness that the test is past
/// `DetermineOutcome` (the cursor never reaches these steps before it runs).
fn resolved(cx: &Cx) -> ResolvedTest {
    cx.state
        .current_skill_test()
        .expect("resolved: the SkillTest frame must persist")
        .resolved
        .expect("resolved: set at the DetermineOutcome step (ST.5–ST.6) for every later step")
}

/// RR ST.7 head — the [`ApplyResultModifiers`](SkillTestStep::ApplyResultModifiers)
/// step. Collect the committed cards' [result
/// modifiers](ResultContribution::Modifies) whose outcome matches the resolved
/// test (Vicious Blow 01025's `BoostAttackDamage`, Deduction 01039's
/// `DiscoverAdditionalClues`), combine them into one
/// [`Effect::Seq`](card_dsl::dsl::Effect::Seq), and `push_effect` it for the
/// drive loop (push nothing if none matches). Pre-advances the cursor to
/// [`ApplyFollowUp`](SkillTestStep::ApplyFollowUp) **before** the push, so a
/// suspending effect would resume past this step.
///
/// A modifier changes a consequence the test already produces, so it runs
/// before `ApplyFollowUp` produces it, and outside the results the performer
/// orders (**Result modifier** in `GLOSSARY.md`). Both in-scope effects are
/// non-suspending accumulator writes, so the loop drives the `Seq` and
/// re-dispatches this `SkillTest` at `ApplyFollowUp`, which reads the
/// now-populated accumulators.
///
/// The combined `Seq` runs for the performer, so each modifier's
/// [`ResultController`] is not read: every modifier declares
/// [`Performer`](ResultController::Performer) ([`card_dsl::dsl::modifies_result`]),
/// and a debug build asserts it, so a modifier declaring
/// [`Committer`](ResultController::Committer) can't silently run for the
/// performer.
fn apply_result_modifiers_step(
    cx: &mut Cx,
    investigator: InvestigatorId,
    committed: &[CommittedCard],
) {
    let succeeded = resolved(cx).succeeded;
    let effects: Vec<Effect> =
        collect_result_contributions(cx.cards, committed, succeeded, ResultContribution::Modifies)
            .into_iter()
            .map(|(index, effect, controller)| {
                debug_assert_eq!(
                    controller,
                    ResultController::Performer,
                    "the result modifier on committed card {index:?} declares {controller:?}, \
                     but modifiers run for the performer",
                );
                effect
            })
            .collect();
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = SkillTestStep::ApplyFollowUp;
    if !effects.is_empty() {
        let seq = Effect::Seq(effects);
        evaluator::push_effect(cx, &seq, EvalContext::for_controller(investigator));
    }
}

/// RR ST.7 part 1 — apply the action-specific [`SkillTestFollowUp`].
///
/// On **success** the follow-up runs: the Investigate follow-up pushes its
/// `discover_clue` effect and the Evade follow-up its `EnemyEvaded` timing
/// event (each yielding to the drive loop), while Fight / None mutate
/// synchronously. On **failure** the follow-up is skipped, except a Fight's:
/// its failure path deals the attack's damage to the investigator engaged
/// with the attacked enemy, if that is someone other than the fighter
/// (`glossary/Fight_Action.md`), and that deal pushes its frame the way the
/// Investigate discovery does. The cursor is pre-advanced to
/// [`CollectResults`](SkillTestStep::CollectResults) **before** any push,
/// so a suspending discovery (Cover Up 01007's before-discover interrupt) or
/// evasion (Pickpocketing 01046's after-evade reaction window) resumes at
/// `CollectResults` rather than re-running the follow-up. (The
/// "after you successfully investigate" timing point already fired at the
/// preceding [`DetermineOutcome`](SkillTestStep::DetermineOutcome) step,
/// before this discovery — RR ST.6 success precedes the ST.7 consequence.)
///
/// When the Investigate or Evade follow-up, or a failed Fight's redirected
/// damage, pushes its frame the `advance` loop's top-frame check yields on the
/// next iteration (that frame is now `last()`); otherwise nothing is pushed and
/// the loop `continue`s on this frame straight into `CollectResults`.
///
/// The Fight follow-up evaluates its `extra_damage` here, on either outcome,
/// and an expression the evaluator can't answer comes back as the rejection
/// the driver returns, rolling the apply back. Any other outcome but `Done` is
/// returned the same way.
fn apply_follow_up_step(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let follow_up = cx
        .state
        .current_skill_test()
        .expect("apply_follow_up_step: the SkillTest frame must exist")
        .follow_up
        .clone();

    // Pre-advance BEFORE any push, so a suspending discovery resumes past here.
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above")
        .continuation = SkillTestStep::CollectResults;

    // Each basic action owns its after-test step; the test procedure only
    // dispatches by kind and outcome.
    match (resolved(cx).succeeded, follow_up) {
        (true, SkillTestFollowUp::Investigate) => investigate::after_test(cx, investigator),
        (
            true,
            SkillTestFollowUp::Fight {
                enemy,
                extra_damage,
            },
        ) => {
            if let Err(rejection) =
                fight::after_test(cx, investigator, enemy, extra_damage.as_ref())
            {
                return rejection;
            }
        }
        (
            false,
            SkillTestFollowUp::Fight {
                enemy,
                extra_damage,
            },
        ) => return fight::after_failed_test(cx, investigator, enemy, extra_damage.as_ref()),
        (true, SkillTestFollowUp::Evade { enemy }) => evade::after_test(cx, investigator, enemy),
        (_, SkillTestFollowUp::None)
        | (false, SkillTestFollowUp::Investigate | SkillTestFollowUp::Evade { .. }) => {}
    }
    EngineOutcome::Done
}

/// RR ST.5–ST.6 — the [`DetermineOutcome`](SkillTestStep::DetermineOutcome)
/// step. Sum the **modified skill value** (ST.5) and compare it against the
/// difficulty (ST.6) — [`verdict`] — stashing the result on
/// [`InFlightSkillTest::resolved`](crate::state::InFlightSkillTest::resolved),
/// then fire the general `SkillTestResolved` timing point for **every** test and
/// both outcomes (the `{ Investigate, Success }` narrowing is Obscuring Fog 01168
/// forced with Dr. Milan 01033 reaction; one `queue_event` so forced precedes
/// reaction — RR p.2).
///
/// The verdict announced here is **provisional**: the condition's own resolve
/// step ([`resolve_verdict`]) re-totals after the `when` cell, logs the
/// [`SkillTestSucceeded`](crate::event::Event::SkillTestSucceeded) /
/// [`SkillTestFailed`](crate::event::Event::SkillTestFailed), and sends the test
/// back here if a `when`-cell modifier turned it into the other verdict (Lucky!
/// 01080).
///
/// The sum happens **here** rather than at
/// [`Resolving`](SkillTestStep::Resolving) because
/// `Appendix_II_Timing_and_Gameplay.md` orders ST.4 (*"Apply chaos symbol
/// effect(s)"*) before ST.5, and ST.5 reads *"all active card abilities that
/// are modifying the investigator's skill value"* — active **at ST.5**. An
/// ST.4 effect that discards a modifying asset (a `[tablet]`'s damage soaked
/// onto a lethally-damaged Beat Cop 01018) must therefore have taken its `+1`
/// away by the time this runs.
///
/// The forced/reaction scans (and an empty candidate set) decide whether any
/// window actually opens, so a test no card listens to costs nothing. Returns
/// the `queue_event` outcome so the caller yields if a 2+ forced run suspends.
/// Pre-advances the cursor to [`AcknowledgeOutcome`](SkillTestStep::AcknowledgeOutcome)
/// **before** the emit, so a suspending reaction window resumes past this step.
fn determine_outcome_step(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let provisional = verdict(cx.state, cx.cards);
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("determine_outcome_step: the SkillTest frame must persist");
    t.resolved = Some(provisional);
    t.continuation = SkillTestStep::AcknowledgeOutcome;
    let kind = t.kind;
    emit::queue_event(
        cx,
        &TimingEvent::SkillTestResolved {
            investigator,
            kind,
            outcome: outcome_of(provisional),
        },
    )
}

/// RR ST.5–ST.6 read off the board as it stands: the in-flight test's modified
/// skill value, compared against its current difficulty, with any
/// determination applied. A pure read, so the resolve step of
/// `SkillTestResolved` can re-total ([`resolve_verdict`]) and an open `when`
/// cell can ask whether the verdict it was opened for still holds
/// (`reaction_windows`).
///
/// # Panics
///
/// With no skill test in flight — every caller is inside one.
pub(crate) fn verdict(state: &GameState, cards: &CardRegistry) -> ResolvedTest {
    let t = state
        .current_skill_test()
        .expect("verdict: a skill test must be in flight");
    // ST.6's determination, resolved once for the whole test across both
    // quantities it can substitute — automatic failure beating automatic
    // success whichever was latched first (ADR 0007). The `[auto_fail]`
    // token wrote its row at ST.3 like any other latch; nothing here knows
    // a token was involved.
    let determination = modified_value::test_determination(state, ReadContext::DuringTest(t.kind));
    // The total difficulty, read the same way as the total skill value: off
    // the board as it stands now, not as it stood at ST.1. An enemy's
    // modified fight value *is* the difficulty of a Fight action, so a card
    // that changed it after initiation has changed this test.
    let difficulty = current_difficulty(state, cards);

    // ST.5 modified skill value — read off the board as the ST.4 symbol effects
    // left it, with the revealed token's own row already among the modifiers
    // it folds and the clamp applied once at the end.
    let total = sum_skill_value(state, cards, t.investigator, t.skill, t.kind, &t.committed);
    // Both quantities above already carry the ST.6 substitution, applied as
    // the last stage of the fold that produced them:
    // `glossary/Automatic_Failure_Success.md` — *"If a skill test
    // automatically fails, the investigator's total skill value for that
    // test is considered 0"*, *"If a skill test automatically succeeds, the
    // total difficulty of that test is considered 0"*. So the margin is a
    // real one either way: skill value 0 against difficulty 3 fails **by
    // 3**, which is what an "if you fail by 2 or more" clause on the same
    // test needs to read.
    let margin = total.saturating_sub(difficulty);
    // The verdict itself is the determination's where there is one, and the
    // comparison's otherwise. The comparison carries no "…and it wasn't an
    // auto-fail" term, which would name the *token*; the determination says
    // so, and says it for a card-latched failure as readily as for the
    // token's.
    //
    // The failure arm is not merely a restatement of the substituted
    // comparison. At every difficulty of 1 or more it agrees with it — skill
    // value 0 loses — but a difficulty of **0** is reachable (a printed
    // `Fixed(0)`, or Flashlight 01087's *"-2 shroud"* on a shroud-2
    // location), and there 0 against 0 compares as a success. The official
    // FAQ states the verdict directly, so the substitution is how the
    // **margin** comes out right rather than how the outcome is decided:
    // *"No. No matter what, if you automatically fail a test, you have
    // failed the test, regardless of how your skill value and the difficulty
    // compare."* (`data/official-faq/Frequently_Asked_Questions.md`.)
    // `perform_skill_test_autofail_at_difficulty_zero_still_fails` is the
    // guard.
    //
    // The success arm *is* a restatement — the difficulty is substituted to
    // 0 and a total is clamped at 0, so the comparison already passes — and
    // is spelled out anyway so the verdict reads off the determination
    // rather than off a coincidence of two clamps.
    let (succeeded, fail_reason) = match determination {
        Some(Determination::AutomaticFailure) => (false, FailureReason::AutoFail),
        Some(Determination::AutomaticSuccess) => (true, FailureReason::Total),
        None => (margin >= 0, FailureReason::Total),
    };
    ResolvedTest {
        succeeded,
        margin,
        fail_reason,
        total,
    }
}

/// The [`TestOutcome`] a verdict announces.
pub(crate) fn outcome_of(resolved: ResolvedTest) -> TestOutcome {
    if resolved.succeeded {
        TestOutcome::Success
    } else {
        TestOutcome::Failure
    }
}

/// The resolve step of `SkillTestResolved`: **re-total**, then either
/// confirm the verdict the condition announced or void it.
///
/// Lucky! 01080 prints *"Fast. Play when you would fail a skill test. Get +2 to
/// your skill value for that test."*, so a `when`-cell modifier can change the
/// total after the provisional verdict was announced. The fresh result is
/// always written back to the frame, margin included, so an `after`-cell margin
/// narrowing ("Look what I found!" 01079's *"by 2 or less"*) reads the
/// re-totalled margin even when the verdict stands.
///
/// - **The verdict stands:** log the
///   [`SkillTestSucceeded`](crate::event::Event::SkillTestSucceeded) /
///   [`SkillTestFailed`](crate::event::Event::SkillTestFailed), after the ST.4
///   symbol effects and the `when` cell, and let the sequence run on to its `at`
///   and `after` cells.
/// - **The verdict flipped:** void the rest of this sequence — the stale
///   verdict's `at` and `after` cells never run, and nothing is logged for it —
///   and rewind the cursor to
///   [`DetermineOutcome`](SkillTestStep::DetermineOutcome), which announces the
///   fresh verdict in a sequence of its own. The revealed token and its recorded
///   row stay, so ST.3 and ST.4 do not run again (the 01080 ruling's *"If you
///   reveal an \[auto_fail\] auto-fail token, your total skill value is
///   considered to be 0, overriding any modifiers"* holds on the re-total for the
///   same reason, <https://arkhamdb.com/card/01080>).
///
/// `resolved` stays `Some` across the rewind, so the past-ST.6 guard on
/// target-left-play abandonment does not reopen.
pub(super) fn resolve_verdict(
    cx: &mut Cx,
    investigator: InvestigatorId,
    announced: TestOutcome,
) -> EngineOutcome {
    let fresh = verdict(cx.state, cx.cards);
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("resolve_verdict: the SkillTest frame must persist under its condition");
    t.resolved = Some(fresh);
    let skill = t.skill;
    if outcome_of(fresh) != announced {
        t.continuation = SkillTestStep::DetermineOutcome;
        coordinator::void_remaining_sequence(cx);
        return EngineOutcome::Done;
    }
    if fresh.succeeded {
        cx.events.push(Event::SkillTestSucceeded {
            investigator,
            skill,
            margin: fresh.margin,
        });
    } else {
        cx.events.push(Event::SkillTestFailed {
            investigator,
            skill,
            reason: fresh.fail_reason,
            // `failed_by` is a non-negative margin as a `u8`; the event field
            // is `i8`. Realistic margins are tiny, so the conversion never
            // saturates.
            by: i8::try_from(fresh.failed_by().unwrap_or(0)).unwrap_or(i8::MAX),
        });
    }
    EngineOutcome::Done
}

/// RR ST.7 — the [`CollectResults`](SkillTestStep::CollectResults) step. Settle
/// which **ST.7 results** (`GLOSSARY.md`) are pending, once, now that the
/// follow-up has resolved, and set the cursor to
/// [`ApplyResults`](SkillTestStep::ApplyResults) with them:
///
/// - the card result, when the test carries an effect for its outcome
///   (`on_success` on a pass, `on_fail` on a failure);
/// - the token result, when the revealed token's outcome
///   ([`chaos_symbol_effect`]) has a part for the test's outcome. No token was
///   revealed when a determination skipped the draw, so there is none;
/// - one addition per committed card's [result
///   addition](ResultContribution::Adds) whose outcome matches.
///
/// The set is not recomputed: nothing during ST.7 changes what decides
/// membership. Each entry's effect is looked up again when it is applied.
fn collect_results_step(cx: &mut Cx, investigator: InvestigatorId, committed: &[CommittedCard]) {
    let succeeded = resolved(cx).succeeded;
    let (card, token) = {
        let t = cx
            .state
            .current_skill_test()
            .expect("collect_results_step: the SkillTest frame must persist");
        let card = if succeeded {
            t.on_success.is_some()
        } else {
            t.on_fail.is_some()
        };
        (card, t.revealed.map(|revealed| revealed.counts_as))
    };
    let token = token.is_some_and(|token| {
        chaos_symbol_effect(cx, token, investigator)
            .0
            .is_some_and(|outcome| token_result_part(&outcome, succeeded).is_some())
    });
    let additions =
        collect_result_contributions(cx.cards, committed, succeeded, ResultContribution::Adds)
            .len();
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = SkillTestStep::ApplyResults {
        pending: PendingResults::new(card, token, additions),
    };
}

/// RR ST.7 — the [`ApplyResults`](SkillTestStep::ApplyResults) step. Take the
/// next pending ST.7 result, pre-advance the cursor to `ApplyResults` with the
/// rest **before** pushing its effect, so a result that suspends (a sanity soak
/// on an `Effect::Deal`) resumes at the next entry, and apply it
/// ([`apply_result`]). The push (if any) makes an Effect frame the new top, and
/// the `advance` loop yields to drive it. With nothing pending, advance to
/// [`PostRetaliate`](SkillTestStep::PostRetaliate).
///
/// RR ST.7: *"If there are multiple results to be applied during this step,
/// the investigator performing the test applies those results in the order of
/// his or her choice."* So while two or more results are pending, this step
/// applies none: it parks the cursor at
/// [`AwaitingResultOrder`](SkillTestStep::AwaitingResultOrder) and suspends on
/// the "which next" prompt ([`result_order_request`]), which
/// [`resume_result_order`] answers. With one pending, it applies without
/// asking.
fn apply_results_step(
    cx: &mut Cx,
    investigator: InvestigatorId,
    committed: &[CommittedCard],
    pending: PendingResults,
) -> Option<EngineOutcome> {
    if pending.iter().nth(1).is_some() {
        cx.state
            .current_skill_test_mut()
            .expect("the SkillTest frame must persist across driver steps")
            .continuation = SkillTestStep::AwaitingResultOrder { pending };
        return Some(prompt::suspend(cx.state, cx.cards));
    }
    let next = pending.iter().next();
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = match next {
        Some(result) => SkillTestStep::ApplyResults {
            pending: pending.without(result),
        },
        None => SkillTestStep::PostRetaliate,
    };
    if let Some(result) = next {
        apply_result(cx, investigator, committed, result);
    }
    None
}

/// What a skill test at its result-order pick asks (the
/// [`AwaitingResultOrder`](SkillTestStep::AwaitingResultOrder) step): which
/// pending ST.7 result applies next, one option per
/// [pending](PendingResults::iter) result, in that order. One of the
/// [prompt table](super::prompt::request)'s helpers; the performer answers it.
///
/// A `PickSingle` [`Selection`](crate::engine::PromptNature::Selection), not
/// skippable, like the multiple-attacks order pick. Each option anchors to the
/// board entity its result comes from, when there is one:
///
/// - the card result, to the source of the test when it is still on the board
///   (Frozen in Fear 01164 in a threat area). A Revelation in limbo (Rotting
///   Remains 01163) has none, so the option is label-only, named after the
///   encounter card resolving beneath the test;
/// - the elder sign's token result, to the performer's investigator card,
///   which prints it. A scenario token's (the Cultist's horror) is label-only:
///   the scenario reference card is not on the board;
/// - a committed card's result addition, to that card's position in the
///   committed list ([`OptionTarget::CommittedCard`]): a committed card has no
///   instance id during the test, and the position keeps two copies apart.
///
/// # Panics
///
/// If no skill test is in flight at that step.
pub(super) fn result_order_request(state: &GameState, cards: &CardRegistry) -> InputRequest {
    let test = state
        .current_skill_test()
        .expect("result_order_request: in-flight test must exist");
    let SkillTestStep::AwaitingResultOrder { pending } = test.continuation else {
        unreachable!("result_order_request: the prompt table asks only at AwaitingResultOrder")
    };
    let options = pending
        .iter()
        .zip(0u32..)
        .map(|(result, i)| {
            let (label, target) = result_option(state, cards, test, result);
            ChoiceOption::new(OptionId(i), label).maybe_at(target)
        })
        .collect();
    InputRequest::pick_single(
        format!(
            "{}: choose which result of the skill test to apply next.",
            choice::investigator_name(state, test.investigator),
        ),
        options,
    )
}

/// The label and anchor of the option offering `result` in the
/// [`result_order_request`] (see there for the anchors).
fn result_option(
    state: &GameState,
    cards: &CardRegistry,
    test: &InFlightSkillTest,
    result: St7Result,
) -> (String, Option<OptionTarget>) {
    match result {
        St7Result::Card => {
            let target = test
                .source
                .and_then(|source| OptionTarget::for_live_source(source, state));
            let name = match &target {
                Some(target) => Some(choice::anchor_label(state, cards, target)),
                None => resolving_encounter_card(state).map(|code| choice::card_name(cards, code)),
            };
            let label = name.map_or_else(
                || "Result of the test".to_owned(),
                |name| format!("Result of {name}"),
            );
            (label, target)
        }
        St7Result::Token => {
            let token = test.revealed.map(|revealed| revealed.counts_as);
            let target = match token {
                Some(ChaosToken::ElderSign) => state
                    .investigators
                    .get(&test.investigator)
                    .map(|inv| OptionTarget::CardInstance(inv.investigator_card.instance_id)),
                _ => None,
            };
            let label = format!(
                "Result of the {} token",
                token.map_or("chaos", token_symbol_name)
            );
            (label, target)
        }
        St7Result::Addition { index } => {
            // Map the addition to the committed card that prints it.
            let card = test.resolved.and_then(|resolved| {
                collect_result_contributions(
                    cards,
                    &test.committed,
                    resolved.succeeded,
                    ResultContribution::Adds,
                )
                .get(index.to_usize())
                .map(|&(card, _, _)| card)
            });
            let target = card.map(|index| OptionTarget::CommittedCard { index });
            let label = target.as_ref().map_or_else(
                || "Result of a committed card".to_owned(),
                |target| format!("Result of {}", choice::anchor_label(state, cards, target)),
            );
            (label, target)
        }
    }
}

/// The encounter card whose Revelation started the in-flight test: the card
/// on the [`EncounterCard`](Continuation::EncounterCard) frame beneath the
/// test, past the frames its Revelation resolves in (the draw's timing event,
/// and the effect walk that pushed the test). `None` when another kind of frame
/// comes first, such as a card being played, so a test some other card started
/// is never named after a Revelation still resolving further down.
fn resolving_encounter_card(state: &GameState) -> Option<&CardCode> {
    state
        .continuations
        .iter()
        .rev()
        .skip_while(|frame| !matches!(frame, Continuation::SkillTest(_)))
        .skip(1)
        .find(|frame| {
            !matches!(
                frame,
                Continuation::Effect(_) | Continuation::EffectBase(_) | Continuation::EmitEvent(_)
            )
        })
        .and_then(|frame| match frame {
            Continuation::EncounterCard(EncounterCardFrame { card, .. }) => card.as_ref(),
            _ => None,
        })
}

/// A chaos symbol's name as a card prints it, for a prompt label.
fn token_symbol_name(token: ChaosToken) -> &'static str {
    match token {
        ChaosToken::Skull => "Skull",
        ChaosToken::Cultist => "Cultist",
        ChaosToken::Tablet => "Tablet",
        ChaosToken::ElderThing => "Elder Thing",
        ChaosToken::ElderSign => "Elder Sign",
        ChaosToken::AutoFail => "Auto-fail",
        _ => "chaos",
    }
}

/// Resume the result-order pick: the performer named the pending ST.7 result
/// to apply next. Validate-first: the cursor must be at
/// [`AwaitingResultOrder`](SkillTestStep::AwaitingResultOrder) and the answer a
/// `PickSingle` naming one of the options [`result_order_request`] offered;
/// anything else is rejected with the prompt left up for a retry. A valid pick
/// pre-advances the cursor to [`ApplyResults`](SkillTestStep::ApplyResults)
/// without the picked result, **before** pushing its effect (as
/// [`apply_results_step`] does), and returns `Done`: the drive loop resolves
/// the effect and re-enters [`advance`], which asks again while two or more
/// results remain.
pub(super) fn resume_result_order(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let Some(t) = cx.state.current_skill_test() else {
        return EngineOutcome::Rejected {
            reason: "skill-test result order: no in-flight skill test to resume".into(),
        };
    };
    let SkillTestStep::AwaitingResultOrder { pending } = t.continuation else {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill-test result order: not at the result-order pick (continuation {:?})",
                t.continuation,
            )
            .into(),
        };
    };
    let InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: the result-order pick expects InputResponse::PickSingle, got \
                 {response:?}"
            )
            .into(),
        };
    };
    let Some(result) = usize::try_from(*i).ok().and_then(|i| pending.iter().nth(i)) else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: result-order option {i} out of range (0..{})",
                pending.iter().count(),
            )
            .into(),
        };
    };
    let (investigator, committed) = (t.investigator, t.committed.clone());
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above")
        .continuation = SkillTestStep::ApplyResults {
        pending: pending.without(result),
    };
    apply_result(cx, investigator, &committed, result);
    EngineOutcome::Done
}

/// Push one ST.7 result's effect, looked up now, with the test's margin bound
/// as every ST.7 result has it:
/// [`Quantity::SkillTestSucceededBy`](card_dsl::dsl::Quantity::SkillTestSucceededBy)
/// on a pass, `Quantity::SkillTestFailedBy` on a failure (Grasping Hands 01162,
/// Rotting Remains 01163 scale by it).
///
/// - [`St7Result::Card`]: the test's `on_success` / `on_fail` (Frozen in Fear
///   01164's self-discard, Crypt Chill 01167's discard choice), in a context
///   rebuilt from the parked source, so `Effect::DiscardSelf` finds itself
///   across the suspend/resume boundary and a `ChooseOne` in either branch
///   keeps the anchor the activation gave it. The parked
///   [`kept_investigator`](InFlightSkillTest::kept_investigator) is put back
///   too, so *"that investigator"* in either branch is the one picked before
///   the test. Nothing else from the starting context crosses: the rebuilt
///   context is fresh, so a `Chosen` pick, a discovery, an enemy attack or a
///   Revelation binding made before the test is gone by construction, and so
///   is any `ForEach` binding. Only this arm restores the kept pick; the token
///   and addition arms build their own contexts without it.
/// - [`St7Result::Token`]: the revealed token's part, looked up again through
///   [`chaos_symbol_effect`] from the token on
///   [`revealed`](InFlightSkillTest::revealed), the way the committed cards'
///   abilities are looked up: an elder sign from the investigator card, sourced
///   to it; a scenario symbol from the scenario's symbol hook, controller-only.
/// - [`St7Result::Addition`]: the committed card's added effect, with the
///   position of the card it is printed on, so the effect can name its own card
///   (Opportunist 01053 returns itself to hand).
///
/// An entry whose effect is gone when it is applied pushes nothing.
fn apply_result(
    cx: &mut Cx,
    investigator: InvestigatorId,
    committed: &[CommittedCard],
    result: St7Result,
) {
    let resolved_test = resolved(cx);
    let pushed = match result {
        St7Result::Card => {
            let (effect, source, kept_investigator) = {
                let t = cx
                    .state
                    .current_skill_test()
                    .expect("apply_result: the SkillTest frame must persist");
                let effect = if resolved_test.succeeded {
                    &t.on_success
                } else {
                    &t.on_fail
                };
                (effect.clone(), t.source, t.kept_investigator)
            };
            effect.map(|effect| {
                let mut ctx =
                    EvalContext::for_controller_with_optional_source(investigator, source);
                if let Some(kept) = kept_investigator {
                    ctx.set_kept_investigator(kept);
                }
                (effect, ctx)
            })
        }
        St7Result::Token => cx
            .state
            .current_skill_test()
            .expect("apply_result: the SkillTest frame must persist")
            .revealed
            .and_then(|revealed| {
                let (outcome, ctx) = chaos_symbol_effect(cx, revealed.counts_as, investigator);
                outcome
                    .and_then(|outcome| token_result_part(&outcome, resolved_test.succeeded))
                    .map(|effect| (effect, ctx))
            }),
        St7Result::Addition { index } => collect_result_contributions(
            cx.cards,
            committed,
            resolved_test.succeeded,
            ResultContribution::Adds,
        )
        .into_iter()
        .nth(index.to_usize())
        .map(|(card_index, effect, controller)| {
            // Overpower 01091's ruling (https://arkhamdb.com/card/01091): *"If
            // you commit this card to another player's skill test, and the test
            // is successful, you draw a card (not the other player)."*
            let controller = match controller {
                ResultController::Committer => committed[card_index.to_usize()].committer,
                ResultController::Performer => investigator,
            };
            let mut ctx = EvalContext::for_controller(controller);
            ctx.set_committed_card(card_index.0);
            (effect, ctx)
        }),
    };
    if let Some((effect, mut ctx)) = pushed {
        ctx.bind_resolved(&resolved_test);
        evaluator::push_effect(cx, &effect, ctx);
    }
}

/// The revealed token's result-conditional part for the test's outcome:
/// `on_success` on a pass, `on_fail` on a failure (Cultist 01104's horror).
fn token_result_part(outcome: &TokenOutcome, succeeded: bool) -> Option<Effect> {
    if succeeded {
        outcome.on_success.clone()
    } else {
        outcome.on_fail.clone()
    }
}

/// The next helper of the in-flight test's [commit
/// queue](SkillTestStep::NextHelper) after `after` (`None`: the first), or
/// `None` when no helper is left.
///
/// A helper is an Active investigator other than the performer, at the
/// performer's location, holding a candidate ([`helper_candidates`]) now. The
/// walk is in player order, so with `after` it resumes past them.
fn next_helper(
    state: &GameState,
    cards: &CardRegistry,
    after: Option<InvestigatorId>,
) -> Option<InvestigatorId> {
    let test = state.current_skill_test()?;
    let performer = test.investigator;
    let location = state.investigators.get(&performer)?.current_location?;
    let colocated = cursor::active_investigators_at(state, location);
    let start = after.map_or(0, |after| {
        colocated
            .iter()
            .position(|id| *id == after)
            .map_or(colocated.len(), |i| i + 1)
    });
    colocated[start..]
        .iter()
        .copied()
        .filter(|id| *id != performer)
        .find(|&helper| !helper_candidates(state, cards, helper).is_empty())
}

/// The hand indices of `helper`'s cards they may commit to the in-flight test,
/// which another investigator performs: the helper prompt's candidates, and
/// what [`next_helper`] reads to skip a helper with none.
fn helper_candidates(state: &GameState, cards: &CardRegistry, helper: InvestigatorId) -> Vec<u8> {
    let Some(test) = state.current_skill_test() else {
        return Vec::new();
    };
    commit_candidates(
        state,
        cards,
        CommitBy {
            committer: helper,
            performer: test.investigator,
        },
        test.skill,
    )
}

/// What a helper of the in-flight test is asked (the
/// [`AwaitingHelperCommit`](SkillTestStep::AwaitingHelperCommit) step): commit
/// at most one of their candidates ([`helper_candidates`]) to the performer's
/// test, or none. Appendix II, ST.2: *"Each other investigator at the same
/// location as the investigator performing the skill test may commit one card
/// with an appropriate skill icon to this test."* The text names the
/// performer's test, its difficulty, and the commits made so far. The answer
/// resumes via `finish_skill_test`, which rejects a pick of more than one. One
/// of the [prompt table](super::prompt::request)'s helpers.
pub(super) fn helper_commit_request(
    state: &GameState,
    cards: &CardRegistry,
    helper: InvestigatorId,
) -> InputRequest {
    let test = state
        .current_skill_test()
        .expect("helper_commit_request: in-flight test must exist");
    let candidates = helper_candidates(state, cards, helper);
    let ask = format!(
        "{}: commit up to one card to {}'s {} test (difficulty {}).",
        choice::investigator_name(state, helper),
        choice::investigator_name(state, test.investigator),
        skill_name(test.skill),
        current_difficulty(state, cards),
    );
    commit_prompt(state, cards, helper, &ask, &candidates, PickCount::up_to(1))
}

/// What a skill test at the performer's commit window asks (the
/// `AwaitingCommit` step), after every helper has answered. The test parks
/// there for the performer's commit; a Resolution frame pushed *above* it is a
/// mid-test window. The commit `ResolveInput` resumes via `finish_skill_test`.
/// One of the [prompt table](super::prompt::request)'s helpers.
///
/// It offers only the cards that may be committed ([`commit_candidates`]), so
/// a card ST.2 forbids is never offered rather than rejected once chosen. A
/// skill's "Max N committed per skill test" limits a set rather than a card, so
/// the count admits every offered card and the limit is checked on resume.
///
/// The text names each card committed so far and its committer, since nothing
/// else shows the performer what help has come. TODO(#1322): the
/// committed-card surface replaces that listing.
pub(super) fn commit_request(state: &GameState, cards: &CardRegistry) -> InputRequest {
    let test = state
        .current_skill_test()
        .expect("commit_request: in-flight test must exist");
    let (investigator, skill) = (test.investigator, test.skill);
    let candidates = commit_candidates(
        state,
        cards,
        CommitBy {
            committer: investigator,
            performer: investigator,
        },
        skill,
    );
    // The difficulty as it stands at the commit window — what a player
    // decides how many cards to commit against. ST.6 re-reads it.
    let ask = format!(
        "Commit cards to the {} test (difficulty {}).",
        skill_name(skill),
        current_difficulty(state, cards),
    );
    let count = PickCount::up_to(candidates.len());
    commit_prompt(state, cards, investigator, &ask, &candidates, count)
}

/// A commit prompt to `committer`, offering their hand cards at `candidates`:
/// `ask`, the commits made so far ([`committed_so_far`]), and how to commit
/// nothing. The shared body of [`helper_commit_request`] and
/// [`commit_request`].
fn commit_prompt(
    state: &GameState,
    cards: &CardRegistry,
    committer: InvestigatorId,
    ask: &str,
    candidates: &[u8],
    count: PickCount,
) -> InputRequest {
    let test = state
        .current_skill_test()
        .expect("commit_prompt: in-flight test must exist");
    InputRequest::pick_multiple(
        format!(
            "{ask}{} An empty selection commits no cards.",
            committed_so_far(state, cards, test),
        ),
        hand_pick::options(state, cards, committer, candidates),
        count,
    )
}

/// A commit prompt's listing of the cards committed to `test` so far, each with
/// its committer: `" Committed so far: Unexpected Courage (Daisy Walker)."`,
/// or empty when nothing is committed, so a solo prompt reads as it always
/// has.
fn committed_so_far(state: &GameState, cards: &CardRegistry, test: &InFlightSkillTest) -> String {
    if test.committed.is_empty() {
        return String::new();
    }
    let listed: Vec<String> = test
        .committed
        .iter()
        .map(|card| {
            format!(
                "{} ({})",
                choice::card_name(cards, &card.code),
                choice::investigator_name(state, card.committer),
            )
        })
        .collect();
    format!(" Committed so far: {}.", listed.join(", "))
}

/// What a skill test at its result pause asks (the `AcknowledgeOutcome` step,
/// under `interactive_acknowledge`): a cosmetic `Confirm`. One of the
/// [prompt table](super::prompt::request)'s helpers.
pub(super) fn acknowledge_request() -> InputRequest {
    InputRequest::confirm("Acknowledge the skill-test result.")
}

/// Open one of the RR p.26 skill-test framework player windows and return
/// `open_fast_window`'s outcome directly. Pre-advances the cursor to `next`
/// **before** opening (the suspend/resume invariant), so a resume — whether the
/// auto-skip inline `run_fast_continuation -> advance` or a wait-then-close —
/// picks up at `next`, not by re-opening this window.
///
/// The caller must **return** this outcome, never fall through: `open_fast_window`
/// returns `Done` both on auto-skip (continuation already ran) and when it parks
/// a pure-Fast window on top (an empty `FastWindow` gate the `advance` loop does
/// not re-dispatch — falling through would emit the next step's prompt *over* the
/// parked window).
fn open_skill_test_player_window(
    cx: &mut Cx,
    next: SkillTestStep,
    before_token: bool,
) -> EngineOutcome {
    cx.state
        .current_skill_test_mut()
        .expect("open_skill_test_player_window: the SkillTest frame must exist")
        .continuation = next;
    reaction_windows::open_fast_window(cx, FastWindowKind::SkillTest { before_token })
}

/// Walk the skill-test resolution sequence from the current
/// [`SkillTestStep`] onward, suspending if a reaction window
/// queues mid-step.
///
/// Each loop iteration starts by checking for a queued reaction
/// window: if one is pending, the driver opens it and returns
/// [`AwaitingInput`](crate::engine::EngineOutcome::AwaitingInput). The window's
/// close path ([`close_reaction_window`]) re-enters this driver on
/// resume.
///
/// Fully frame-driven: every effect a step would run is
/// `push_effect`'d for the global `drive` loop rather than run synchronously.
/// When a step pushes an effect (or a sub-step queues a reaction window), the
/// pushed frame becomes `last()`; the top-of-loop check below sees a
/// non-`SkillTest` top frame and returns `Done`, ceding to `drive`, which
/// resolves the sub-frame and re-dispatches this `SkillTest` at the
/// pre-advanced cursor. Each step pre-advances the cursor **before** pushing, so
/// a suspending effect never re-runs its step.
///
/// Step → next-continuation mapping (RR p.26 ST order):
///
/// - [`Resolving`](SkillTestStep::Resolving) → ST.3 token reveal + ST.4
///   immediate symbol effects; advance to
///   [`DetermineOutcome`](SkillTestStep::DetermineOutcome).
/// - [`DetermineOutcome`](SkillTestStep::DetermineOutcome) → ST.5 modified skill
///   value + ST.6 comparison, then the `SkillTestResolved` timing point (every
///   test/outcome), whose resolve step re-totals and logs the success/failure
///   event, before the ST.7 consequences; advance to
///   [`AcknowledgeOutcome`](SkillTestStep::AcknowledgeOutcome), or back here
///   if the re-total flipped the verdict.
/// - [`ApplyResultModifiers`](SkillTestStep::ApplyResultModifiers) → ST.7 head
///   (committed cards' result modifiers); advance to
///   [`ApplyFollowUp`](SkillTestStep::ApplyFollowUp).
/// - [`ApplyFollowUp`](SkillTestStep::ApplyFollowUp) → ST.7 action follow-up
///   (the clue discovery); advance to
///   [`CollectResults`](SkillTestStep::CollectResults).
/// - [`CollectResults`](SkillTestStep::CollectResults) → settle the pending
///   ST.7 results (card, token, additions); advance to
///   [`ApplyResults`](SkillTestStep::ApplyResults).
/// - [`ApplyResults`](SkillTestStep::ApplyResults) → apply one pending ST.7
///   result per visit; advance to
///   [`PostRetaliate`](SkillTestStep::PostRetaliate) once none is left. With
///   two or more ordered results pending, park at
///   [`AwaitingResultOrder`](SkillTestStep::AwaitingResultOrder) instead.
/// - [`AwaitingResultOrder`](SkillTestStep::AwaitingResultOrder) → suspend on
///   the "which next" pick; `resume_result_order` applies the picked result
///   and sets `ApplyResults` with the rest.
/// - [`PostRetaliate`](SkillTestStep::PostRetaliate) → fire a Retaliate attack
///   (failed Fight vs ready retaliate enemy); advance to
///   [`PostOnResolution`](SkillTestStep::PostOnResolution).
/// - [`PostOnResolution`](SkillTestStep::PostOnResolution) → ST.8: discard
///   committed cards, emit [`SkillTestEnded`](crate::event::Event::SkillTestEnded),
///   drain pending modifiers, tear down the frame, return `Done`.
///
/// [`close_reaction_window`]: super::reaction_windows::close_reaction_window
#[expect(
    clippy::too_many_lines,
    reason = "one arm per `SkillTestStep`, RR ST.3–ST.8, in the fixed step order"
)]
pub(super) fn advance(cx: &mut Cx) -> EngineOutcome {
    loop {
        // A sub-frame queued by the previous step sits *above* this `SkillTest`
        // frame — a reaction window, or a pushed on_success/on_fail effect.
        // Whenever the top frame is not this `SkillTest`, yield to
        // the `drive` loop: it drives the sub-frame, and its completion
        // re-dispatches this `SkillTest` at the pre-advanced cursor. A forced run
        // *below* this frame (reentrancy: two Frozen in Fear copies) is never
        // `last()` while this driver runs, so "not the SkillTest on top" simply
        // means "a sub-frame above me" — no `rposition` self-location.
        if !matches!(
            cx.state.continuations.top(),
            Some(Continuation::SkillTest(_))
        ) {
            return EngineOutcome::Done;
        }

        let (continuation, investigator, committed, difficulty_basis, past_st6) = {
            let in_flight = cx.state.current_skill_test().unwrap_or_else(|| {
                unreachable!(
                    "advance: the SkillTest frame must exist while driver is active; \
                     state-corruption invariant violation"
                )
            });
            (
                in_flight.continuation,
                in_flight.investigator,
                in_flight.committed.clone(),
                in_flight.difficulty_basis,
                in_flight.resolved.is_some(),
            )
        };

        // RR p.10 Elimination step 1 removed every card this investigator owns
        // from the game — including the cards they committed to this test,
        // which it held in limbo. Abandon the test rather than resolve it on
        // behalf of someone who has left the scenario. Mirrors
        // `drive_attack_loop`'s early-break (`combat.rs`), which drops
        // remaining attackers on the same signal.
        //
        // This is the single choke point: both the `drive` loop's `SkillTest`
        // arm and the `finish_skill_test` commit hop reach the driver here.
        if cx
            .state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.status != Status::Active)
        {
            return tear_down_test(cx, investigator, &committed, TestEnd::PerformerEliminated);
        }

        // The test's difficulty *is* a quantity of some entity on the
        // board ([`DifficultyBasis`]), and that entity has left play — a
        // 1-health enemy killed by Beat Cop 01018 at the ST.2 player window
        // while the attack against it is in flight. There is no test left to
        // resolve, so abandon it. See `DifficultyBasis`, which records the
        // decision and says plainly that the vendored sources do not settle it.
        //
        // Bounded by `past_st6`: past ST.6 the verdict is already set, and the
        // entity leaving play is ordinary — the Fight follow-up's own damage is
        // what defeats the enemy. Only a test that has not yet been compared
        // can still be abandoned.
        if !past_st6 && difficulty_target_left_play(cx.state, difficulty_basis) {
            return tear_down_test(cx, investigator, &committed, TestEnd::PerformerRemains);
        }

        match continuation {
            SkillTestStep::PreCommitWindow => {
                // RR p.26 player window after ST.1.
                return open_skill_test_player_window(
                    cx,
                    SkillTestStep::NextHelper { after: None },
                    false,
                );
            }
            SkillTestStep::NextHelper { after } => {
                // RR ST.2's commit queue, worked out after the ST.1→ST.2
                // window has closed: ask the next helper, or the performer
                // once none is left.
                cx.state
                    .current_skill_test_mut()
                    .expect("the SkillTest frame must persist across driver steps")
                    .continuation = match next_helper(cx.state, cx.cards, after) {
                    Some(helper) => SkillTestStep::AwaitingHelperCommit { helper },
                    None => SkillTestStep::AwaitingCommit,
                };
            }
            SkillTestStep::AwaitingHelperCommit { .. }
            | SkillTestStep::AwaitingCommit
            | SkillTestStep::AwaitingResultOrder { .. } => {
                // The frame's `awaiting()`: emit a helper's or the
                // performer's commit prompt, or the outstanding ST.7 "which
                // next" pick. (See
                // `emit_commit_window` for the propagation rationale.)
                return prompt::suspend(cx.state, cx.cards);
            }
            SkillTestStep::PreTokenWindow => {
                // RR p.26 player window after ST.2.
                return open_skill_test_player_window(cx, SkillTestStep::Resolving, true);
            }
            SkillTestStep::Resolving => {
                // RR ST.3 (reveal the chaos token) — skipped outright, with
                // ST.4, when the test's determination is already known.
                // Draws and announces the token, pre-advances to
                // DetermineOutcome and queues the `ChaosTokenRevealed`
                // condition, whose resolve step records the token and pushes
                // the ST.4 symbol effect: the loop yields to it, resuming at
                // DetermineOutcome — or back here, if a `when`-cell ability
                // cancelled the token and revealed a new one.
                run_resolution(cx, investigator);
            }
            SkillTestStep::DetermineOutcome => {
                // RR ST.5 (sum the modified skill value, off the board the ST.4
                // effects left behind) + ST.6 (compare, set the provisional
                // outcome), then the general SkillTestResolved timing point
                // (every test/outcome) — BEFORE the ST.7 consequences. Its resolve
                // step logs the verdict, or sends the test back here when a
                // `when`-cell modifier flipped it. Pre-advances to
                // AcknowledgeOutcome; yields if a 2+ forced run suspends.
                //
                // A token was revealed and resolved, or the result was known
                // before ST.3. Neither means a `when`-cell cancel left the test
                // with no token and nothing set the cursor back to draw again —
                // a malformed card (a cancel with no `RevealChaosToken`).
                debug_assert!(
                    cx.state
                        .current_skill_test()
                        .is_some_and(|t| t.revealed.is_some())
                        || modified_value::test_determination(
                            cx.state,
                            ReadContext::from_state(cx.state)
                        )
                        .is_some(),
                    "a skill test reached ST.5 with its chaos token cancelled and no new token \
                     revealed"
                );
                let outcome = determine_outcome_step(cx, investigator);
                if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                    return outcome;
                }
            }
            SkillTestStep::AcknowledgeOutcome => {
                // RR-neutral cosmetic pause. The result events were emitted
                // by SkillTestResolved's resolve step; if interactive acknowledgment is enabled,
                // suspend with a Confirm so the player registers the result before
                // the ST.7 consequence resolves. The cursor stays at
                // AcknowledgeOutcome across the suspension — `acknowledge_outcome`
                // advances it on the Confirm resume (the AwaitingCommit /
                // finish_skill_test handshake). When off, advance straight to
                // ApplyResultModifiers so non-interactive drives are unchanged.
                if cx.state.interactive_acknowledge {
                    return prompt::suspend(cx.state, cx.cards);
                }
                cx.state
                    .current_skill_test_mut()
                    .expect("the SkillTest frame must persist across driver steps")
                    .continuation = SkillTestStep::ApplyResultModifiers;
            }
            SkillTestStep::ApplyResultModifiers => {
                // RR ST.7 head. Pre-advances the cursor to ApplyFollowUp, then
                // pushes the committed cards' matching result modifiers
                // (Vicious Blow's BoostAttackDamage) as one Seq. The loop
                // drives them (non-suspending accumulator writes in scope) and
                // re-dispatches at ApplyFollowUp, which reads the
                // now-populated accumulator.
                apply_result_modifiers_step(cx, investigator, &committed);
            }
            SkillTestStep::ApplyFollowUp => {
                // RR ST.7 part 1. Pre-advances the cursor to CollectResults,
                // then applies the action follow-up (success-only, but for a
                // failed Fight's redirected damage). The Investigate follow-up
                // pushes discover_clue, the Evade one its EnemyEvaded
                // coordinator and a failed Fight its redirected deal → the loop
                // check at the top of the next iteration sees that frame on top
                // and yields; a successful Fight and None mutate synchronously (a Fight that
                // defeats an enemy queues a reaction window — also a non-SkillTest
                // top frame, also yields).
                let outcome = apply_follow_up_step(cx, investigator);
                if !matches!(outcome, EngineOutcome::Done) {
                    return outcome;
                }
            }
            SkillTestStep::CollectResults => {
                collect_results_step(cx, investigator, &committed);
            }
            SkillTestStep::ApplyResults { pending } => {
                // RR ST.7 results, one per visit. With two or more the
                // performer orders still pending, suspends on the "which next"
                // pick instead, which `resume_result_order` answers.
                if let Some(outcome) = apply_results_step(cx, investigator, &committed, pending) {
                    return outcome;
                }
            }
            SkillTestStep::PostRetaliate => {
                // Retaliate fires here — after ST.7, before the ST.8 teardown
                // below. `glossary/Retaliate.md` says only *"after applying
                // all results for that skill test"*; the official FAQ places
                // it inside the test rather than after it, enumerating
                // retaliate among the effects that *"trigger during Steps 6 &
                // 7 of a failed skill test"*
                // (`data/official-faq/Frequently_Asked_Questions.md`, on
                // Neither Rain nor Snow). So the committed-card discard and
                // the token's return to the bag — both at PostOnResolution —
                // must not precede the retaliate attack.
                //
                // Advance the cursor first: a retaliate that suspends on its
                // cancel/soak window resumes here at PostOnResolution (the retaliate
                // already happened; only its window is being resolved).
                let succeeded = resolved(cx).succeeded;
                cx.state
                    .current_skill_test_mut()
                    .expect("the SkillTest frame must persist across driver steps")
                    .continuation = SkillTestStep::PostOnResolution;
                let outcome = fight::fire_retaliate_if_any(cx, investigator, succeeded);
                if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                    return outcome; // parked on the retaliate's window; resume via advance
                }
            }
            SkillTestStep::PostOnResolution => {
                // RR ST.8 teardown. The skill-test-outcome timing point
                // (`SkillTestResolved`; "after you successfully investigate" =
                // Obscuring Fog forced + Dr. Milan reaction) already fired at
                // the DetermineOutcome step via `queue_event` —
                // forced-before-reaction.
                //
                // Encounter-treachery disposal is not the skill-test
                // driver's concern: a treachery whose Revelation
                // suspended into this test parks an `EncounterCard` frame
                // beneath it, which the framework disposes of at the
                // `resolve_input` chokepoint once this test completes.
                //
                // Teardown tail. Once the test is torn down, `Done` cedes to the
                // `drive` loop, which re-dispatches whatever it was nested
                // within. A suspending `EndOfTurn` forced stranded `end_turn`
                // before rotation and flagged the `InvestigatorTurn { ending:
                // true }` frame beneath (a single Frozen in Fear, or a 2+ forced
                // run); the loop's `InvestigatorTurn { ending: true }` arm runs
                // `resume_end_turn` once this returns — no reach-down here (the
                // same path a 2+ forced run takes).
                return tear_down_test(cx, investigator, &committed, TestEnd::PerformerRemains);
            }
        }
    }
}

/// Expire every [`RecordedModifier`](crate::state::RecordedModifier) bought for
/// the test being torn down.
///
/// Called from [`tear_down_test`], however the test ended, with the
/// `SkillTest` frame still on the stack, since the id it keys on lives there.
/// A missing frame is a teardown bug that would leak test-scoped rows into the
/// next test — the failure a test's identity exists to prevent — so it trips
/// a `debug_assert` rather than quietly doing nothing.
fn expire_this_tests_modifiers(cx: &mut Cx) {
    let ending = cx.state.current_skill_test().map(|t| t.id);
    debug_assert!(
        ending.is_some(),
        "skill-test teardown: no SkillTest frame to expire test-scoped modifiers against",
    );
    if let Some(id) = ending {
        cx.state.expire_modifiers_for_test(id);
    }
}

/// Whether the entity whose quantity `basis` reads has left play, so the test
/// resting on it has nothing left to be a test *of*.
///
/// [`Fixed`](DifficultyBasis::Fixed) reads a number printed on a card rather
/// than a board entity, so it can never go absent.
fn difficulty_target_left_play(state: &GameState, basis: DifficultyBasis) -> bool {
    match basis {
        DifficultyBasis::Fixed(_) => false,
        DifficultyBasis::Shroud(id) => !state.locations.contains_key(&id),
        DifficultyBasis::Fight(id) | DifficultyBasis::Evade(id) => !state.enemies.contains_key(&id),
    }
}

/// Tear down the in-flight skill test, and pop its frame. Three callers: RR
/// ST.8's normal end, at [`PostOnResolution`](SkillTestStep::PostOnResolution),
/// and the two abandoned cases in [`advance`]'s loop preamble, where the
/// performer was eliminated mid-resolution or the test's difficulty target left
/// play.
///
/// `committed` is the cards still in limbo on the test, each discarded to its
/// owner's pile: *"Discard all cards that were committed to this skill test"*
/// (Appendix II, ST.8). The test's [`TestEnd`] decides whether the card the
/// test was marked to discard ([`InFlightSkillTest::discard_after`]) goes with
/// them.
///
/// [`Event::SkillTestEnded`] still fires either way: the test *is* over, and it
/// is the documented "test is fully over" signal downstream listeners key on.
fn tear_down_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    committed: &[CommittedCard],
    end: TestEnd,
) -> EngineOutcome {
    discard_committed_cards(cx, committed);
    if end == TestEnd::PerformerRemains {
        // The marked card goes with them: the symbol that marked it was
        // revealed during this test, abandoned or not.
        discard_marked_card(cx);
    }
    cx.events.push(Event::SkillTestEnded { investigator });
    // ModifierScope::ThisSkillTest contributions expire with the test, however
    // it ended — the abandoned case is exactly the one where a leak would be
    // invisible. Keyed on this test's identity, as in the normal
    // teardown.
    expire_this_tests_modifiers(cx);
    let taken = cx.state.take_skill_test();
    debug_assert!(
        taken.is_some(),
        "skill-test teardown: no SkillTest frame on the continuation stack",
    );
    EngineOutcome::Done
}

/// How a skill test came to [tear down](tear_down_test), which decides what
/// its teardown discards beyond the committed cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestEnd {
    /// The performer is still in the scenario: RR ST.8's normal end, or a test
    /// abandoned because its difficulty target left play. The cards are still
    /// their owners', so dropping them silently would delete cards off the
    /// board; they go to the discard pile exactly as an ST.8 teardown would
    /// send them, and so does the card the test was marked to discard.
    PerformerRemains,
    /// The performer was eliminated mid-test. Rules Reference p.10 Elimination
    /// step 1 (*"The cards he or she controls in play and all of the cards in
    /// his or her out-of-play areas (such as hand, deck, discard pile) are
    /// removed from the game"*) has already taken the performer's own
    /// committed cards off the test
    /// ([`Continuation::take_committed_cards`]), so what is left is other
    /// committers', still theirs and discarded to their piles. The marked card
    /// is in play under the performer, so the same step removed it from the
    /// game and it is not discarded here.
    PerformerEliminated,
}

/// Who is committing, to whose test: the two differ once another investigator
/// at the same location may commit.
#[derive(Debug, Clone, Copy)]
struct CommitBy {
    /// The investigator whose hand the cards come from.
    committer: InvestigatorId,
    /// The investigator performing the test.
    performer: InvestigatorId,
}

/// The hand indices of the committer's cards that may be committed to a test
/// of `skill`, in hand order: the commit window's candidates. Each passes
/// [`may_commit`], and its title still has a copy left of its commit cap
/// ([`remaining_allowance`]), counting every committer's cards so far.
fn commit_candidates(
    state: &GameState,
    cards: &CardRegistry,
    by: CommitBy,
    skill: SkillKind,
) -> Vec<u8> {
    hand_pick::candidates(state, by.committer, |code| {
        may_commit(cards, code, by, skill) && remaining_allowance(state, cards, code) != Some(0)
    })
}

/// How many more copies of `code`'s title may be committed to the in-flight
/// test, under its printed "Max N committed per skill test" (`CardKind::Skill`'s
/// `commit_limit`, Guts 01089), or `None` if the card has no cap or no registry
/// entry.
///
/// The cap counts every committer, and keys on the title rather than the code.
/// `glossary/Limits_and_Maximums.md`: a limit *"imposes a maximum across all
/// copies of a card (by title) for all players."*
///
/// The one reading of the cap: the commit window's candidates
/// ([`commit_candidates`]) and the resume check ([`check_commit_limits`]) both
/// take it from here, so the two can't disagree.
fn remaining_allowance(state: &GameState, cards: &CardRegistry, code: &CardCode) -> Option<u8> {
    let meta = (cards.metadata_for)(code)?;
    let CardKind::Skill {
        commit_limit: Some(limit),
        ..
    } = meta.kind
    else {
        return None;
    };
    let committed = state.current_skill_test().map_or(0, |test| {
        test.committed
            .iter()
            .filter(|card| (cards.metadata_for)(&card.code).is_some_and(|m| m.name == meta.name))
            .count()
    });
    Some(limit.saturating_sub(u8::try_from(committed).unwrap_or(u8::MAX)))
}

/// Whether the card `code` may be committed to a test of `skill`, on its own.
///
/// RR Appendix II ST.2: *"An appropriate skill icon is either one that matches
/// the skill being tested, or a wild icon. […] Cards that lack an appropriate
/// skill icon may not be committed to a skill test."* Without this, commit is a
/// free discard outlet for any card in hand — a weakness included.
///
/// `skill` is the test's skill *as it stands at the commit window*, so a
/// substitution that already rewrote it (Mind over Matter 01036 makes the test
/// an Intellect test) is what the icons are matched against.
///
/// A card with no registry entry is unconstrained, matching
/// [`check_commit_limits`] and `push_committed_icons`: engine-only tests run
/// against a registry that knows no real cards.
fn may_commit(cards: &CardRegistry, code: &CardCode, by: CommitBy, skill: SkillKind) -> bool {
    let Some(meta) = (cards.metadata_for)(code) else {
        return true;
    };
    // Opportunist 01053: *"Commit only to a skill test you are performing."*
    let own_test_only = matches!(
        meta.kind,
        CardKind::Skill {
            commit_only_to_own_test: true,
            ..
        }
    );
    meta.skill_icons().appropriate_for(skill) && !(own_test_only && by.committer != by.performer)
}

/// Check a commit of the `committer`'s hand cards at `indices` against each
/// title's [`remaining_allowance`], and return `indices` if it holds. A card
/// with no cap, or no registry entry, is unconstrained. The window offers
/// every copy of a title with any allowance left, so a pick of more copies
/// than are left is rejected here.
fn check_commit_limits(
    state: &GameState,
    cards: &CardRegistry,
    committer: InvestigatorId,
    indices: Vec<u8>,
) -> Result<Vec<u8>, EngineOutcome> {
    let hand = &state.investigators[&committer].hand;
    // Copies picked per title, with a code to look the allowance up by.
    let mut picked: BTreeMap<&str, (&CardCode, u8)> = BTreeMap::new();
    for &i in &indices {
        let code = &hand[usize::from(i)];
        let Some(meta) = (cards.metadata_for)(code) else {
            continue;
        };
        picked.entry(meta.name.as_str()).or_insert((code, 0)).1 += 1;
    }
    for (title, (code, count)) in picked {
        if let Some(left) = remaining_allowance(state, cards, code) {
            if count > left {
                return Err(EngineOutcome::Rejected {
                    reason: format!(
                        "skill-test commit: {left} more {title} ({code}) may be committed to \
                         this skill test, but {count} were picked"
                    )
                    .into(),
                });
            }
        }
    }
    Ok(indices)
}

/// The in-flight test's **modified difficulty**, recalculated from the
/// board at this read: the modified shroud of the location being
/// investigated, or the modified fight or evade value of the enemy being
/// attacked or evaded, clamped once at zero after every modifier.
///
/// The `i8` face of the query, for the callers that announce or compare a
/// difficulty; [`DifficultyBasis`](crate::state::DifficultyBasis) carries
/// the rule that makes it a query at all, and the ST.6 check that sets it.
///
/// `0` with no test in flight. Saturates into `i8` for the absurd case;
/// realistic difficulties are 0–6.
fn current_difficulty(state: &GameState, cards: &CardRegistry) -> i8 {
    i8::try_from(
        modified_value::modified_value(
            state,
            cards,
            ModifierTarget::Test,
            ModifiedQuantity::Difficulty,
            ReadContext::from_state(state),
        )
        .total(),
    )
    .unwrap_or(i8::MAX)
}

/// RR ST.5's **total** modified skill value: the investigator's
/// [modified value](modified_value) for `skill` — base plus every
/// modifier the board carries and every recorded row scoped to this
/// test, recalculated here rather than banked earlier — with the one
/// contribution that is a property of this test rather than of the
/// board, the committed cards' matching and wild icons, pushed into the
/// same breakdown. A weapon's one-shot *"+N \[combat\] for this attack"*
/// is a recorded row the query itself folds in, and so is the
/// revealed token's ±N and the elder sign's bonus.
///
/// **Returned clamped**, because nothing is left to add: the icons go
/// into the breakdown rather than onto its result, so the clamp is last
/// and `Modifiers.md`'s worked example comes out right — base 4 with a
/// −8 token and a +2 is −2 → 0, not 0 + 2 → 2.
///
/// Cards the registry can't address contribute 0 — the same
/// silent-skip policy the sweep uses.
fn sum_skill_value(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
    committed: &[CommittedCard],
) -> i8 {
    assert!(
        state.investigators.contains_key(&investigator),
        "sum_skill_value: investigator {investigator:?} disappeared while test was in flight; \
         this is a state-corruption invariant violation"
    );
    let mut breakdown = modified_value::modified_value(
        state,
        cards,
        ModifierTarget::Investigator(investigator),
        ModifiedQuantity::Skill(skill),
        ReadContext::DuringTest(kind),
    );
    push_committed_icons(cards, &mut breakdown, committed, skill);
    i8::try_from(
        breakdown
            .total()
            .clamp(i32::from(i8::MIN), i32::from(i8::MAX)),
    )
    .expect("clamped into i8's range on the line above")
}

/// Push the skill-icon contribution of the `committed` cards (RR ST.5) onto
/// a breakdown: each card adds its matching-skill icons plus its wild icons,
/// attributed to the card that was committed. Cards whose code isn't in the
/// registry contribute nothing.
///
/// They are contributions rather than a number added to the fold's result so
/// that the clamp stays last (see [`sum_skill_value`]) and so the breakdown
/// can say which committed card bought which point.
///
/// Reads the codes the commit hop moved into limbo, **not** the hand — the
/// ST.2→ST.3 player window sits between the commit and this sum and is free to
/// mutate the hand.
fn push_committed_icons(
    cards: &CardRegistry,
    out: &mut ModifierBreakdown,
    committed: &[CommittedCard],
    skill: SkillKind,
) {
    for CommittedCard { code, .. } in committed {
        let Some(meta) = (cards.metadata_for)(code) else {
            continue;
        };
        let icons = meta.skill_icons();
        let delta =
            i8::try_from(icons.matching(skill).saturating_add(icons.wild)).unwrap_or(i8::MAX);
        out.push(
            ContributionSource::Card {
                code: code.clone(),
                instance: None,
            },
            delta,
        );
    }
}

/// RR ST.8: *"Discard all cards that were committed to this skill test."* Move
/// the committed cards out of limbo and into each card's owner's discard pile
/// (its [committer](CommittedCard::committer)'s), emitting
/// [`Event::CardDiscarded`] for each. Per the [`Event::SkillTestEnded`] docs,
/// these discards precede the `SkillTestEnded` cleanup marker.
///
/// A card whose own success effect overrode its
/// [destination](CommittedCard::destination) goes back to hand instead,
/// emitting [`Event::CommittedCardReturnedToHand`] in place of the discard
/// (Opportunist 01053: *"return Opportunist to your hand after this test
/// instead of discarding it."*). Each card goes in commit order.
///
/// The cards come off the in-flight record, not out of the hand — the commit
/// hop already removed them, so nothing here can index a hand position
/// that a Fast play in the ST.2→ST.3 window has since shifted. The emitted
/// `from` is still [`Zone::Hand`]: limbo is not a zone the event vocabulary
/// models, and the hand is where the cards came from.
fn discard_committed_cards(cx: &mut Cx, committed: &[CommittedCard]) {
    for CommittedCard {
        code,
        committer,
        destination,
    } in committed
    {
        let investigator = *committer;
        // Unreachable: an investigator is never removed from the map, and
        // elimination takes an eliminated committer's cards off the test.
        // Total rather than `expect` so a future path can't panic a
        // production apply (a panic escapes `apply_via`'s rollback) — the card
        // then simply stays in limbo and dies with the frame.
        let Some(inv) = cx.state.investigators.get_mut(&investigator) else {
            debug_assert!(
                false,
                "discard_committed_cards: committer {investigator:?} vanished mid-test; \
                 this is a state-corruption invariant violation",
            );
            continue;
        };
        match destination {
            CommittedCardDestination::Discard => {
                inv.discard.push(code.clone());
                cx.events.push(Event::CardDiscarded {
                    code: code.clone(),
                    from: Zone::Hand,
                    to: DiscardPile::Investigator(investigator),
                });
            }
            CommittedCardDestination::Hand => {
                inv.hand.push(code.clone());
                cx.events.push(Event::CommittedCardReturnedToHand {
                    investigator,
                    code: code.clone(),
                });
            }
        }
    }
}

/// Collect, in committed-card order, the effects of every committed card's
/// [`Trigger::OnSkillTestResolution`] ability that makes the given
/// `contribution` and matches the resolved outcome.
///
/// The one collector behind both ST.7 steps, so they can't drift in how they
/// gate or source effects.
/// [`ApplyResultModifiers`](SkillTestStep::ApplyResultModifiers) combines the
/// [`Modifies`](ResultContribution::Modifies) list into one `Seq`;
/// [`CollectResults`](SkillTestStep::CollectResults) counts the
/// [`Adds`](ResultContribution::Adds) list as pending ST.7 results, and
/// [`ApplyResults`](SkillTestStep::ApplyResults) indexes into it, pushing one
/// effect per visit, in the order the performer picks, so they
/// cursor-sequence (no LIFO). At collection time the committed cards are in
/// limbo on the in-flight record (discard happens at teardown), which also
/// still holds the tested location, so [`LocationTarget::TestedLocation`]
/// resolves cleanly.
///
/// Each effect comes paired with its card's position in `committed`, which
/// `ApplyResults` binds for the effect to name its own card, and with the
/// ability's [`ResultController`], which `ApplyResults` reads for whose "you"
/// an addition is. Modifiers are evaluated for the performer, the controller
/// every `modifies_result` declares.
///
/// No abilities for the codes → empty list: engine-only tests that don't touch
/// card data never reach a committed card's ability. Silent skip mirrors
/// `constant_skill_modifier`'s behavior.
fn collect_result_contributions(
    cards: &CardRegistry,
    committed: &[CommittedCard],
    succeeded: bool,
    contribution: ResultContribution,
) -> Vec<(CommittedIndex, Effect, ResultController)> {
    let outcome_now = if succeeded {
        TestOutcome::Success
    } else {
        TestOutcome::Failure
    };

    let mut effects = Vec::new();
    for (index, CommittedCard { code, .. }) in committed.iter().enumerate() {
        // Committed cards come out of a hand, which `hand_pick::picked`
        // already indexes by `u8`.
        let index =
            CommittedIndex(u8::try_from(index).expect("committed cards are hand-indexed by u8"));
        let Some(abilities) = (cards.abilities_for)(code) else {
            continue;
        };
        for ability in abilities {
            let Trigger::OnSkillTestResolution {
                outcome,
                contributes,
                controller,
            } = ability.trigger
            else {
                continue;
            };
            if outcome != outcome_now || contributes != contribution {
                continue;
            }
            effects.push((index, ability.effect, controller));
        }
    }
    effects
}

/// Start a plain skill test directly, the synthetic entry point behind
/// [`TestSession::test_skill`](crate::test_support::TestSession::test_skill).
///
/// Opens the commit window with no action-specific follow-up. The
/// after-resolution trigger window is downstream.
pub(crate) fn perform_skill_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) -> EngineOutcome {
    start_skill_test(
        cx,
        investigator,
        SkillTestSpec::Plain {
            skill,
            difficulty,
            on_success: None,
            on_fail: None,
            kept_investigator: None,
        },
        None,
    )
}

pub(super) fn peril_check(
    _cx: &mut Cx,
    _code: &CardCode,
    _investigator: InvestigatorId,
    _is_peril: bool,
) {
    // TODO(#139): if `is_peril`, install a temporary
    //   restriction on `_cx.state` such that other investigators cannot
    //   (a) play cards, (b) trigger abilities, or (c) commit to the
    //   drawing investigator's skill tests until this card's
    //   resolution completes.
}

#[cfg(test)]
mod tests;
