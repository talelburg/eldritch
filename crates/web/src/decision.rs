//! The decision modal (#856): a choice among alternatives **printed on one
//! card** presents itself the moment it arises, instead of waiting behind a
//! second click on the card the player has just committed to.
//!
//! The engine names the prompt's *nature* and this module maps that nature to a
//! presentation — ADR 0011's rule (*"the client never decides where a control
//! belongs — it reads the anchor the engine attached"*) one layer in, recorded in
//! ADR 0015. Nothing here infers the nature from the prompt's shape.
//!
//! It is the **sole** surface for a decision: the source card keeps its glow, the
//! provenance signal, but opens no context menu while the modal is up, and the
//! prompt banner renders neither the prompt text nor the branches. Offering one
//! choice on two surfaces is the double-render defect #541 fixed for anchored
//! options.
//!
//! [`live_decision`] is pure and native-tested; the view's submit is wasm-only.

use game_core::state::{AdvanceDeck, GameState};
use game_core::{ChoiceOption, EngineOutcome, OptionTarget, PromptNature};
use leptos::prelude::*;

use crate::act_agenda::{deck_face, name_and_text_src, Face};
use crate::store::ClientState;

/// The card a decision came from, as the modal names it: the printed name and
/// the printed text of the **face the board is showing** — an advancing act or
/// agenda shows the reverse the player has just flipped to, a card instance its
/// front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionSource {
    /// The card's printed name, or its raw code when no metadata resolves.
    pub name: String,
    /// The card's printed text on that face, if it prints any.
    pub text: Option<String>,
}

/// A live decision prompt: the engine's prompt text, the branches it offers, and
/// the card they are printed on.
///
/// `source` is `None` when the prompt is un-anchored — First Aid 01019 spending
/// its *last* supply is discarded during cost payment, so #845 degrades the
/// anchor rather than pointing at a card in the discard pile. The modal still
/// carries the branches; it just cannot name where they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub prompt: String,
    pub options: Vec<ChoiceOption>,
    pub source: Option<DecisionSource>,
}

/// The live decision prompt, or `None` when the live prompt is a selection (or
/// there is none). Reads the nature the engine attached — never the prompt's
/// shape. Pure.
#[must_use]
pub fn live_decision(state: &ClientState) -> Option<Decision> {
    let Some(EngineOutcome::AwaitingInput { request, .. }) = &state.outcome else {
        return None;
    };
    if request.nature != PromptNature::Decision {
        return None;
    }
    Some(Decision {
        prompt: request.prompt.clone(),
        options: request.options.clone(),
        source: state
            .game
            .as_ref()
            .zip(request.target.as_ref())
            .and_then(|(game, target)| decision_source(game, target)),
    })
}

/// True iff the decision modal is up. Public because the prompt banner and the
/// board's cards consult it — the modal is the sole surface, so the rule lives
/// here, with the view it governs, rather than being re-derived by each of them
/// (the pattern [`crate::skill_test_result::modal_is_live`] already set). Pure.
#[must_use]
pub fn modal_is_live(state: &ClientState) -> bool {
    live_decision(state).is_some()
}

/// Context newtype carrying "a decision modal is up" as a signal, so a board
/// card reads it without prop-drilling — the same idiom as
/// [`PendingOptions`](crate::interaction::PendingOptions). A distinct type so it
/// cannot collide with another `Signal<bool>` context.
#[derive(Clone)]
pub struct DecisionLive(pub Signal<bool>);

/// True iff a live decision is taking the board's context menus away. The modal
/// is the sole surface for a decision, so the card it came from keeps its glow
/// but opens no menu; every other card is behind the scrim anyway.
///
/// Absent context reads as `false` — a view mounted without [`DecisionLive`]
/// (an isolated component test) behaves exactly as it did before #856.
#[must_use]
pub fn menus_are_suppressed() -> bool {
    use_context::<DecisionLive>().is_some_and(|d| d.0.get())
}

/// Resolve a decision's anchor to the card it names. `None` for an anchor that
/// is not a card the modal can print — no decision site raises one today, and a
/// modal with no source header is a better failure than a fabricated one.
fn decision_source(game: &GameState, target: &OptionTarget) -> Option<DecisionSource> {
    let (code, face) = match target {
        OptionTarget::Act => (
            game.act_deck.get(game.act_index)?.code.clone(),
            deck_face(game, AdvanceDeck::Act),
        ),
        OptionTarget::Agenda => (
            game.agenda_deck.get(game.agenda_index)?.code.clone(),
            deck_face(game, AdvanceDeck::Agenda),
        ),
        OptionTarget::CardInstance(instance_id) => (
            game.investigators
                .values()
                .flat_map(game_core::state::Investigator::controlled_card_instances)
                .find(|card| card.instance_id == *instance_id)?
                .code
                .clone(),
            Face::Front,
        ),
        _ => return None,
    };
    let (name, text) = name_and_text_src(&code, face);
    Some(DecisionSource { name, text })
}

/// The decision modal. Renders nothing unless a decision prompt is live.
///
/// A scrim over the board and a centred panel carrying the source card's name and
/// printed text, the engine's prompt, and one button per branch. **Dismissible
/// only by choosing a branch** — every exit submits engine input, so a backdrop
/// click or an Escape key would choose on the player's behalf; neither is wired,
/// deliberately, the same rule the skill-test result modal follows.
///
/// **Draggable** (#857, [`crate::drag`]): a player choosing a First Aid mode can
/// park the modal aside — the backdrop fades once it has been moved — and read
/// who is at their location before answering. Moving it is not answering it, and
/// a press that lands on a branch button starts no drag.
#[component]
pub fn DecisionView() -> impl IntoView {
    let store = crate::store::use_store();
    // The prompt's fingerprint: which applied batch a live modal is up for. The
    // batch count is what tells one prompt from the next when liveness does not
    // — one decision answered straight into another (#857).
    let drag = crate::drag::Drag::per_prompt(move || {
        let st = store.get();
        modal_is_live(&st).then_some(st.log.len())
    });
    view! {
        {move || {
            let st = store.get();
            let Some(decision) = live_decision(&st) else {
                return ().into_any();
            };
            let source = decision.source.map(|s| {
                let text = s
                    .text
                    .map(|t| crate::card::render_segments(crate::card::parse_card_text(&t)));
                view! {
                    <div class="decision-source">{s.name}</div>
                    <div class="decision-printed card-text">{text}</div>
                }
            });
            let branches: Vec<_> = decision
                .options
                .into_iter()
                .map(|opt| {
                    let ChoiceOption { id, label, .. } = opt;
                    let header = label.clone();
                    view! {
                        <button
                            class="decision-branch"
                            on:click={
                                #[cfg(target_arch = "wasm32")]
                                {
                                    move |_| {
                                        crate::controls::submit(
                                            game_core::InputResponse::PickSingle(id),
                                            header.clone(),
                                        );
                                    }
                                }
                                #[cfg(not(target_arch = "wasm32"))]
                                {
                                    let _ = (id, header);
                                    |_| ()
                                }
                            }
                        >
                            {label}
                        </button>
                    }
                })
                .collect();
            view! {
                // No `on:click` on the backdrop: every exit is engine input.
                <div class="decision-backdrop" style=move || drag.scrim_style()></div>
                <section
                    class="decision-modal"
                    style=move || drag.transform_style()
                    on:pointerdown=move |ev| drag.down(&ev)
                    on:pointermove=move |ev| drag.movement(&ev)
                    on:pointerup=move |ev| drag.up(&ev)
                >
                    {source}
                    <p class="decision-prompt">{decision.prompt}</p>
                    <div class="decision-branches">{branches}</div>
                </section>
            }
            .into_any()
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::{
        Act, Agenda, CardCode, CardInPlay, CardInstanceId, GameStateBuilder, InvestigatorId,
        UseKind,
    };
    use game_core::test_support::fixtures::test_investigator;
    use game_core::{InputRequest, OptionId};

    const FIRST_AID: &str = "01019";
    const AGENDA_1: &str = "01105";
    const ACT_3: &str = "01110";
    const KIT: CardInstanceId = CardInstanceId(7);

    /// The continuation the engine pushes while `deck`'s advance fires its
    /// reverse — the step a decision printed on that reverse arises from.
    fn advancing(deck: AdvanceDeck, code: CardCode) -> game_core::state::Continuation {
        game_core::state::Continuation::AdvanceReverse {
            deck,
            from: 0,
            leaving_code: code,
            step: game_core::state::AdvanceStep::FireReverse,
            trigger: game_core::state::AdvanceTrigger::Forced,
        }
    }

    fn install_registry() {
        // Idempotent (`OnceLock`, first-wins) and safe in the web lib test
        // binary, which has no competing installer.
        let _ = game_core::card_registry::install(cards::REGISTRY);
    }

    fn branches() -> Vec<ChoiceOption> {
        vec![
            ChoiceOption::new(OptionId(0), "Heal 1 damage"),
            ChoiceOption::new(OptionId(1), "Heal 1 horror"),
        ]
    }

    /// A client state whose live prompt is `request`, over `game`.
    fn awaiting(game: Option<GameState>, request: InputRequest) -> ClientState {
        ClientState {
            game,
            outcome: Some(game_core::test_support::fixtures::awaiting_request(request)),
            ..Default::default()
        }
    }

    /// First Aid in play, so a `CardInstance` anchor resolves.
    fn board_with_first_aid() -> GameState {
        let mut inv = test_investigator(1);
        let mut kit = CardInPlay::enter_play(CardCode::new(FIRST_AID), KIT);
        kit.uses.insert(UseKind::Supplies, 3);
        inv.cards_in_play.push(kit);
        GameStateBuilder::new().with_investigator(inv).build()
    }

    #[test]
    fn a_selection_prompt_is_not_a_decision() {
        let state = awaiting(
            None,
            InputRequest::pick_single("Choose an enemy", branches()),
        );
        assert!(live_decision(&state).is_none());
        assert!(!modal_is_live(&state));
    }

    #[test]
    fn no_prompt_is_not_a_decision() {
        assert!(!modal_is_live(&ClientState::default()));
        assert!(!modal_is_live(&ClientState {
            outcome: Some(EngineOutcome::Done),
            ..Default::default()
        }));
    }

    #[test]
    fn a_decision_names_the_card_instance_it_is_printed_on() {
        install_registry();
        let state = awaiting(
            Some(board_with_first_aid()),
            InputRequest::pick_single("Choose one", branches())
                .at(OptionTarget::CardInstance(KIT))
                .deciding(),
        );
        let d = live_decision(&state).expect("a live decision");
        let source = d.source.expect("First Aid is in play, so it is named");
        assert_eq!(source.name, "First Aid");
        assert!(
            source
                .text
                .as_deref()
                .is_some_and(|t| t.contains("Heal 1 damage or horror")),
            "the modal carries the card's printed text: {:?}",
            source.text,
        );
        assert_eq!(d.options.len(), 2, "every branch reaches the modal");
    }

    /// The advancing agenda shows the **reverse** — the face the player has just
    /// flipped to and the one the choice is printed on, not the front's doom
    /// track.
    #[test]
    fn an_advancing_agenda_is_named_by_its_reverse() {
        install_registry();
        let mut game = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        game.agenda_deck = vec![Agenda {
            code: CardCode::new(AGENDA_1),
            doom_threshold: 3,
        }];
        game.agenda_index = 0;
        game.continuations
            .push(advancing(AdvanceDeck::Agenda, CardCode::new(AGENDA_1)));
        let state = awaiting(
            Some(game),
            InputRequest::pick_single("Choose one", branches())
                .at(OptionTarget::Agenda)
                .deciding(),
        );
        let source = live_decision(&state)
            .expect("a live decision")
            .source
            .expect("the agenda is named");
        assert_eq!(
            source.name.trim(),
            "A Lapse in Time",
            "the reverse's own name, not the front's",
        );
        assert!(
            source
                .text
                .as_deref()
                .is_some_and(|t| t.contains("must decide")),
            "the reverse's printed choice, not the front: {:?}",
            source.text,
        );
    }

    /// #845 reaching the modal: the anchor degraded to `None` because First Aid
    /// was discarded paying its own cost. The branches still arrive — an
    /// unanswerable prompt is a deadlock, not a cosmetic slip (ADR 0011).
    #[test]
    fn a_decision_with_no_live_source_still_carries_its_branches() {
        install_registry();
        let state = awaiting(
            Some(board_with_first_aid()),
            InputRequest::pick_single("Choose one", branches()).deciding(),
        );
        let d = live_decision(&state).expect("a live decision");
        assert!(d.source.is_none(), "un-anchored: nothing to name");
        assert_eq!(d.options.len(), 2);
    }

    /// An anchor pointing at a card that is no longer findable names nothing
    /// rather than fabricating a source.
    #[test]
    fn an_unresolvable_anchor_names_nothing() {
        install_registry();
        let state = awaiting(
            Some(board_with_first_aid()),
            InputRequest::pick_single("Choose one", branches())
                .at(OptionTarget::CardInstance(CardInstanceId(999)))
                .deciding(),
        );
        assert!(live_decision(&state)
            .expect("a live decision")
            .source
            .is_none());
    }

    /// The two modals cannot collide: the skill-test result modal requires the
    /// live prompt to be the un-anchored acknowledge `Confirm`, and a decision is
    /// a `PickSingle`. Mutually exclusive by construction (ADR 0015).
    #[test]
    fn the_decision_and_skill_test_modals_are_mutually_exclusive() {
        let decision = awaiting(
            None,
            InputRequest::pick_single("Choose one", branches()).deciding(),
        );
        assert!(modal_is_live(&decision));
        assert!(
            !crate::skill_test_result::modal_is_live(&decision),
            "a decision is a PickSingle, so the result modal stands down",
        );

        let mut ack = ClientState {
            last_skill_test_difficulty: Some(3),
            last_revealed_token: Some((
                game_core::state::ChaosToken::Numeric(0),
                game_core::state::TokenResolution::Modifier(0),
            )),
            last_skill_test_result: Some(game_core::Event::SkillTestSucceeded {
                investigator: InvestigatorId(1),
                skill: game_core::state::SkillKind::Willpower,
                margin: 0,
            }),
            ..awaiting(None, InputRequest::confirm("Acknowledge"))
        };
        ack.status = crate::store::ConnStatus::Connected;
        assert!(crate::skill_test_result::modal_is_live(&ack));
        assert!(
            !modal_is_live(&ack),
            "the acknowledge pause is a Confirm, so the decision modal stands down",
        );
    }

    /// Act 3, What Have You Done? 01110 — the decision fires from the reverse
    /// the player has just flipped to, so the modal must name *that* face:
    /// `back_name` "Defending the Home" and the two printed bullets, not the
    /// front's Objective. The advance sits at `FireReverse`, which is the only
    /// step an act decision can arise from.
    #[test]
    fn an_advancing_act_is_named_and_quoted_by_its_reverse() {
        install_registry();
        let mut game = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        game.act_deck = vec![Act {
            code: CardCode::new(ACT_3),
            clue_threshold: 2,
        }];
        game.act_index = 0;
        game.continuations
            .push(advancing(AdvanceDeck::Act, CardCode::new(ACT_3)));
        let state = awaiting(
            Some(game),
            InputRequest::pick_single("Choose one", branches())
                .at(OptionTarget::Act)
                .deciding(),
        );
        let source = live_decision(&state)
            .expect("a live decision")
            .source
            .expect("the act is named");
        assert_eq!(
            source.name.trim(),
            "Defending the Home",
            "the reverse's own name, not the front's",
        );
        let text = source.text.expect("the reverse prints the choice");
        assert!(
            text.contains("The lead investigator must decide (choose one):")
                && text.contains("Burn it down!"),
            "the modal quotes the reverse's printed choice: {text:?}",
        );
    }

    /// The control for the test above: with no advance in flight the act shows
    /// its front, so the face selection is doing real work rather than always
    /// reaching for the reverse.
    #[test]
    fn an_act_anchor_resolves_to_the_current_act() {
        install_registry();
        let mut game = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        game.act_deck = vec![Act {
            code: CardCode::new("01110"),
            clue_threshold: 2,
        }];
        game.act_index = 0;
        let state = awaiting(
            Some(game),
            InputRequest::pick_single("Choose one", branches())
                .at(OptionTarget::Act)
                .deciding(),
        );
        let source = live_decision(&state)
            .expect("a live decision")
            .source
            .expect("the act is named");
        assert_eq!(source.name, "What Have You Done?");
    }
}
