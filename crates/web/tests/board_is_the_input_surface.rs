//! The one test that asserts the claim #541 closes on: a whole turn driven from
//! the board alone, with **no** flat input bar behind it to catch what falls
//! through (S6, #206).
//!
//! A miniature server runs the real engine in-process: every `ClientMessage` the
//! UI submits is applied with `game_core::apply` and folded back into the store as
//! an `Applied`, exactly as the transport would. So the assertions are about what
//! a player can see and click, and the prompts are the ones the engine actually
//! emits — including the two option-less `Confirm`s that are byte-identical but
//! for their anchor, which is the collision the retired bar hid (ADR 0011).
//!
//! Registry-dependent (panels read investigator-card capacity), so it lives in its
//! own binary per the `tests/location_card.rs` first-wins-registry precedent.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::CardInPlay;
use game_core::state::{
    CardCode, Continuation, GameState, GameStateBuilder, InvestigationResume, InvestigatorId, Phase,
};
use game_core::test_support::fixtures::test_investigator;
use game_core::{Action, EngineOutcome};
use leptos::prelude::*;
use protocol::{ClientMessage, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::app::Overlays;
use web::board::BoardView;
use web::store::{reduce, ClientState};
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

const INV: InvestigatorId = InvestigatorId(1);
const ROLAND: &str = "01001";
const ROTTING_REMAINS: &str = "01163";

/// A single investigator mid-Investigation with the turn open and one action
/// left. The harness spends that action to reach the open turn menu, after which
/// End turn is the only thing left — the shortest honest route from an open turn
/// into the Mythos encounter draw.
fn open_turn_with_one_action() -> GameState {
    let mut inv = test_investigator(1);
    inv.actions_remaining = 1;
    // A real investigator card, so panel capacity resolves against the real
    // corpus this binary installs — and a real encounter card can be drawn.
    inv.investigator_card = CardInPlay::enter_play(
        CardCode::new(ROLAND),
        game_core::state::CardInstanceId(u32::MAX - 1),
    );
    inv.deck = vec![CardCode::new("01020"), CardCode::new("01021")];
    let mut state = GameStateBuilder::default()
        .with_investigator(inv)
        .with_phase(Phase::Investigation)
        .with_turn_order([INV])
        .with_active_investigator(INV)
        .with_round(1)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(INV)
        // A one-token bag makes the test's outcome deterministic: +1 against
        // Willpower 3 vs difficulty 3 passes, so the flow is stable run to run.
        .with_chaos_bag(game_core::state::ChaosBag::new([
            game_core::state::ChaosToken::Numeric(1),
        ]))
        .build();
    // Rotting Remains 01163: "Revelation - Test [willpower] (3)." Drawing it is
    // what carries the flow from the encounter deck into a skill test, and so to
    // the acknowledge pause — the second of the two identical-on-the-wire
    // `Confirm`s (ADR 0011).
    state.encounter_deck = [CardCode::new(ROTTING_REMAINS)].into_iter().collect();
    // The cosmetic acknowledge pause (#478) is what the result modal renders on.
    state.interactive_acknowledge = true;
    state
}

/// The in-process server: holds the authoritative `GameState`, applies whatever
/// the UI submits, and folds the result back into the store.
struct Harness {
    store: RwSignal<ClientState>,
    rx: mpsc::UnboundedReceiver<ClientMessage>,
    state: Option<GameState>,
}

impl Harness {
    /// Mount the board plus the app's overlays against a fresh store seeded with
    /// `state` and the engine's opening outcome for it.
    async fn mount(state: GameState, outcome: EngineOutcome) -> Self {
        // The real corpus, not the synthetic registry: this flow draws a real
        // encounter card and reads a real investigator card's capacity. Its own
        // binary, per the `tests/location_card.rs` first-wins-registry precedent.
        let _ = game_core::card_registry::install(cards::REGISTRY);
        let store = RwSignal::new(ClientState::default());
        let (tx, rx) = mpsc::unbounded::<ClientMessage>();
        let tx_for_mount: OutboundTx = tx;
        let seed = state.clone();
        leptos::mount::mount_to_body(move || {
            provide_context(store);
            provide_context::<OutboundTx>(tx_for_mount.clone());
            let pending = Signal::derive(move || store.with(web::interaction::pending_options));
            provide_context(web::interaction::PendingOptions(pending));
            let anchor = Signal::derive(move || store.with(web::interaction::confirm_anchor));
            provide_context(web::interaction::ConfirmAnchor(anchor));
            let selected = RwSignal::new(std::collections::BTreeSet::<u32>::new());
            let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
            provide_context(web::interaction::MultiSelect { active, selected });
            view! { <div class="e2e-root"><BoardView/><Overlays/></div> }
        });
        store.update(|s| {
            reduce(
                s,
                ServerMessage::Hello {
                    state: Box::new(seed),
                    outcome,
                    events: Vec::new(),
                },
            );
        });
        leptos::task::tick().await;
        Self {
            store,
            rx,
            state: Some(state),
        }
    }

    /// Drain one submitted frame, apply it to the authoritative state, and fold
    /// the `Applied` back in — the transport's round trip, in-process.
    async fn pump(&mut self) {
        let ClientMessage::Submit { action } = self
            .rx
            .try_recv()
            .expect("the click submitted a frame")
            .clone();
        let result = game_core::apply(
            self.state.take().expect("a state to apply against"),
            Action::Player(action),
        );
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "the board offered something the engine rejected: {:?}",
            result.outcome
        );
        let store = self.store;
        self.state = Some(result.state.clone());
        store.update(|s| {
            reduce(
                s,
                ServerMessage::Applied {
                    state: Box::new(result.state),
                    events: result.events,
                    outcome: result.outcome,
                },
            );
        });
        leptos::task::tick().await;
    }

    fn state(&self) -> &GameState {
        self.state.as_ref().expect("a state")
    }
}

fn root() -> web_sys::Element {
    let roots = document().query_selector_all(".e2e-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("an .e2e-root")
}

fn find(sel: &str) -> web_sys::HtmlElement {
    root()
        .query_selector(sel)
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .unwrap_or_else(|| panic!("no `{sel}` on the board"))
}

fn is_disabled(sel: &str) -> bool {
    find(sel)
        .dyn_ref::<web_sys::HtmlButtonElement>()
        .expect("a button")
        .disabled()
}

#[wasm_bindgen_test]
async fn a_whole_turn_is_driven_from_the_board_alone() {
    // Seed by spending the one action through the engine: what comes back is the
    // engine's own open-turn menu, so nothing about the anchors is reconstructed
    // in the test.
    let seeded = game_core::test_support::resolver::take_turn_action(
        open_turn_with_one_action(),
        &game_core::TurnAction::Resource { investigator: INV },
    );
    let mut h = Harness::mount(seeded.state, seeded.outcome).await;

    // 1. The open turn. The banner stays silent — the menu is anchored to the
    //    turn control, so its "Choose an action" text is suppressed structurally
    //    (ADR 0011) rather than by matching the string.
    assert!(
        root()
            .query_selector(".prompt-banner")
            .expect("query")
            .is_none(),
        "the open-turn menu must not put 'Choose an action' in the banner"
    );
    assert!(
        !is_disabled(".turn-control"),
        "End turn is live on the panel"
    );

    // 2. End turn, clicked on the investigator's own panel. The round-ending turn
    //    cascades through Upkeep into Mythos and parks at the step-1.4 draw.
    find(".turn-control").click();
    h.pump().await;
    assert_eq!(h.state().phase, Phase::Mythos);

    // 3. The encounter draw is a button on a visible encounter deck, not a
    //    context-free "Confirm". End turn is still present, greyed out — the
    //    panel's shape does not change between phases.
    assert!(
        is_disabled(".turn-control"),
        "off-turn, End turn is visible but dead rather than gone"
    );
    assert!(!is_disabled(".encounter-draw"), "the Mythos draw is live");
    find(".encounter-draw").click();
    h.pump().await;

    // 4. Rotting Remains' revelation opens a Willpower test. Its commit prompt has
    //    no board home, so the banner — the floor — carries it. Commit nothing.
    find(".prompt-banner .confirm").click();
    h.pump().await;

    // 5. The result lands in a modal carrying the acknowledge pause's only
    //    Confirm. This is the prompt the flat bar rendered identically to the
    //    encounter draw, which is why the collision went unnoticed until now.
    find(".skill-test-result .str-confirm").click();
    h.pump().await;

    // 6. Nothing was ever stranded, and no flat bar existed to catch it if it had
    //    been. Asserted against the app's real overlay composition.
    assert!(
        document()
            .query_selector(".action-bar")
            .expect("query")
            .is_none(),
        "the sticky action bar is deleted (#541); it must not creep back via a merge"
    );
}
