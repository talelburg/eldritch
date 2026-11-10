//! Shared helpers for the web crate's browser tests.

use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::mpsc::{self, UnboundedReceiver};
use game_core::card_registry::CardRegistry;
use game_core::engine::{EngineOutcome, Responder};
use game_core::event::Event;
use game_core::state::investigator::InvestigatorId;
use game_core::state::GameState;
use js_sys::{Object, Reflect};
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task;
use protocol::live::LiveGame;
use protocol::lobby::Member;
use protocol::{AccountId, Assignment, ClientMessage, GameId, LogPosition, ServerMessage};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast as _, JsValue};
use web::app::{self, MainColumn};
use web::delete_game::DeleteGame;
use web::registry;
use web::store::{self, ClientState, ConnStatus};
use web::transport::OutboundTx;
use web_sys::{Element, HtmlElement, MouseEvent};

/// The last element matching `sel`: the browser tests share one page and
/// nothing unmounts, so a test reads the last match.
pub(crate) fn last_mounted(sel: &str) -> Element {
    let nodes = document()
        .query_selector_all(sel)
        .expect("query_selector_all");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .unwrap_or_else(|| panic!("at least one {sel}"))
}

/// The first element under `within` matching `sel`.
pub(crate) fn one(within: &Element, sel: &str) -> Element {
    within
        .query_selector(sel)
        .expect("query")
        .unwrap_or_else(|| panic!("no {sel} under the element"))
}

/// The text of the first element under `within` matching `sel`.
///
/// Read as `text_content` rather than `inner_html`, which escapes: the
/// apostrophe in "What's Going On?!" arrives as an entity, and a substring test
/// against it would be testing the escaper.
pub(crate) fn text_of(within: &Element, sel: &str) -> String {
    one(within, sel).text_content().unwrap_or_default()
}

/// The text of every element `sel` matches inside `within`, in document order.
pub(crate) fn texts_in(within: &Element, sel: &str) -> Vec<String> {
    let nodes = within.query_selector_all(sel).expect("query");
    (0..nodes.length())
        .filter_map(|i| nodes.item(i))
        .map(|node| node.text_content().unwrap_or_default())
        .collect()
}

/// An option label carrying a card-text symbol token, as Hyperawareness
/// 01034's first ability is labelled.
pub(crate) const INTELLECT_LABEL: &str = "Activate Hyperawareness — +1 [intellect]";

/// Assert `el` shows `prefix` then [`INTELLECT_LABEL`], with the label's
/// `[intellect]` token drawn as the intellect glyph (codepoint `g`) and not
/// printed as the token. `prefix` is whatever `el` renders ahead of the label,
/// such as a log header's marker and investigator name, or `""`.
pub(crate) fn assert_shows_intellect_label(el: &Element, prefix: &str) {
    let glyph = one(el, ".glyph.glyph--intellect");
    assert_eq!(
        glyph.text_content().as_deref(),
        Some("g"),
        "the intellect glyph"
    );
    assert_eq!(
        el.text_content().unwrap_or_default(),
        format!("{prefix}Activate Hyperawareness — +1 g"),
        "the label's text around the glyph is kept, and no literal token"
    );
}

/// `names`, owned, to compare with what [`texts_in`] reads.
pub(crate) fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

/// How many elements under `root` match `sel`.
pub(crate) fn count(root: &Element, sel: &str) -> u32 {
    root.query_selector_all(sel).expect("query").length()
}

/// Whether nothing under `root` matches `sel`.
pub(crate) fn absent(root: &Element, sel: &str) -> bool {
    count(root, sel) == 0
}

/// The name of each card in the card modal's own row that has a verb button,
/// with the button's text, in order. The "placed so far" row and the collapsed
/// cards are not the modal's own row, and a card shown there without a verb is
/// not offered.
pub(crate) fn offered(root: &Element) -> Vec<(String, String)> {
    let cards = root
        .query_selector_all(".card-modal > .card-modal-row > .card-modal-card")
        .expect("query");
    (0..cards.length())
        .filter_map(|i| cards.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .filter_map(|card| {
            let verb = card.query_selector(".card-modal-verb").expect("query")?;
            Some((
                text_of(&card, ".card-name"),
                verb.text_content().unwrap_or_default(),
            ))
        })
        .collect()
}

/// Inject `style.css` once per browser session, so measurements are taken
/// against the stylesheet the app ships rather than an unstyled DOM.
///
/// Every browser test shares one page, so the stylesheet stays in force for
/// every test that runs after this one. A test that measures layout calls this
/// itself rather than depend on whether another test ran first.
pub(crate) fn inject_style() {
    let doc = document();
    if doc
        .query_selector("style[data-app-css]")
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    let style = doc.create_element("style").expect("create style");
    style.set_attribute("data-app-css", "1").expect("attr");
    style.set_text_content(Some(include_str!("../../../style.css")));
    doc.head()
        .expect("head")
        .append_child(&style)
        .expect("append");
}

/// The computed value of `el`'s CSS `property`.
pub(crate) fn computed(el: &Element, property: &str) -> String {
    window()
        .get_computed_style(el)
        .expect("computed style")
        .expect("a style declaration")
        .get_property_value(property)
        .expect("the property")
}

/// Point at `host` and let the popover it carries render.
///
/// CSS `:hover` can't be driven headless, so the popover is signal-driven and
/// this dispatches the `mouseenter` that opens it, then ticks.
pub(crate) async fn hover(host: &Element) {
    pointer(host, "mouseenter").await;
}

/// Move the pointer off `host`, closing its popover.
pub(crate) async fn unhover(host: &Element) {
    pointer(host, "mouseleave").await;
}

/// Dispatch the mouse event `kind` on `host`, and let the page react.
async fn pointer(host: &Element, kind: &str) {
    let event = MouseEvent::new(kind).expect("a mouse event");
    host.dispatch_event(&event)
        .expect("dispatch the mouse event");
    task::tick().await;
}

/// Hover `host` and return the text of the popover that opens.
pub(crate) async fn hovered_popover(host: &Element) -> String {
    hover(host).await;
    popover_text(host).expect("hovering it opens its popover")
}

/// Click the investigator tab under `root` named `name`, and let the board
/// react.
pub(crate) async fn click_tab(root: &Element, name: &str) {
    click_named(root, ".inv-tab", ".tab-name", name);
    task::tick().await;
}

/// Click the one `sel` under `root` whose `name_sel` reads `name`.
pub(crate) fn click_named(root: &Element, sel: &str, name_sel: &str, name: &str) {
    click(&named(root, sel, name_sel, name));
}

/// The `sel` under `root` whose `name_sel` reads `name`.
pub(crate) fn named(root: &Element, sel: &str, name_sel: &str, name: &str) -> Element {
    let all = root.query_selector_all(sel).expect("query");
    (0..all.length())
        .filter_map(|i| all.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .find(|el| {
            el.query_selector(name_sel)
                .expect("query")
                .and_then(|n| n.text_content())
                .is_some_and(|text| text == name)
        })
        .unwrap_or_else(|| panic!("a {sel} named {name}"))
}

/// Click `el`. The page reacts on the next tick, which the caller awaits.
pub(crate) fn click(el: &Element) {
    el.clone()
        .dyn_into::<HtmlElement>()
        .expect("an HTML element")
        .click();
}

/// The text of the open popover inside `host`, `None` while none is open.
pub(crate) fn popover_text(host: &Element) -> Option<String> {
    host.query_selector(".popover")
        .expect("query")
        .map(|el| el.text_content().unwrap_or_default())
}

/// Mount `view` into a container of its own over a fresh store; return the
/// store and the container. The DOM accumulates across tests in one browser
/// session, so every assertion is scoped to this container.
pub(crate) fn mount<V: IntoView + 'static>(
    view: impl FnOnce() -> V + 'static,
) -> (RwSignal<ClientState>, HtmlElement) {
    let container = document()
        .create_element("div")
        .expect("create_element")
        .dyn_into::<HtmlElement>()
        .expect("HtmlElement");
    document()
        .body()
        .expect("body")
        .append_child(&container)
        .expect("append_child");

    let store = RwSignal::new(ClientState::default());
    mount_to(container.clone(), move || {
        provide_context(store);
        view()
    })
    .forget();
    (store, container)
}

/// A board mounted by [`mount_board`].
pub(crate) struct BoardHandle {
    /// The `.board-root` this mount put the board and its overlays in. Query
    /// from here: the page is shared, so a document-wide query can match an
    /// earlier test's board.
    pub(crate) root: Element,
    /// The store the board renders from, for a test that feeds it frames.
    pub(crate) store: RwSignal<ClientState>,
    /// Every frame the board submitted, in order.
    pub(crate) rx: UnboundedReceiver<ClientMessage>,
    /// Every game a confirmed Delete deleted, in order: the recorder this
    /// mount provides as the [`DeleteGame`] context, in place of the wasm
    /// build's `DELETE /games/{id}`.
    pub(crate) deleted: UnboundedReceiver<GameId>,
}

impl BoardHandle {
    /// The lockout wrapper this mount rendered: the board and its in-game
    /// modals sit in it, and it carries `inert` at every status but
    /// `Connected`.
    pub(crate) fn lock(&self) -> Element {
        self.root
            .query_selector(".board-lock")
            .expect("query")
            .expect("the board and its in-game modals sit in a .board-lock wrapper")
    }
}

/// Assert `el` has no `inert` ancestor, so it takes clicks while the board is
/// locked. `what` names it in the failure.
pub(crate) fn assert_live(el: &Element, what: &str) {
    assert!(
        el.closest("[inert]").expect("closest").is_none(),
        "{what} must sit outside the lockout wrapper, so it takes clicks"
    );
}

/// Every connection status, listed rather than sampled, for a test whose claim
/// is about *every* status, or every status but one. `SignedOut` lists no
/// sign-in method.
pub(crate) fn every_status() -> Vec<ConnStatus> {
    let all = vec![
        ConnStatus::Connecting,
        ConnStatus::Connected,
        ConnStatus::Reconnecting,
        ConnStatus::Restarting,
        ConnStatus::Failed,
        ConnStatus::Home,
        ConnStatus::InLobby,
        ConnStatus::VersionMismatch,
        ConnStatus::SignedOut(vec![]),
        ConnStatus::Unresumable,
    ];
    // Exhaustive, so a variant added later doesn't compile until it is
    // listed above.
    for status in &all {
        match status {
            ConnStatus::Connecting
            | ConnStatus::Connected
            | ConnStatus::Reconnecting
            | ConnStatus::Restarting
            | ConnStatus::Failed
            | ConnStatus::Home
            | ConnStatus::InLobby
            | ConnStatus::VersionMismatch
            | ConnStatus::SignedOut(_)
            | ConnStatus::Unresumable => {}
        }
    }
    all
}

/// Mount [`MainColumn`], the composition `App` mounts, rendering `client`
/// against `registry`, and tick once so render effects have flushed. The board
/// sits inside the real lockout wrapper, so it is `inert` unless `client` is
/// `Connected`.
///
/// The contexts come from `App`'s own [`app::provide_board_contexts`], so a board
/// test can't miss one the app provides.
pub(crate) async fn mount_board(
    client: ClientState,
    registry: &'static CardRegistry,
) -> BoardHandle {
    let store = RwSignal::new(client);
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let (delete_tx, deleted) = mpsc::unbounded::<GameId>();
    let root = NodeRef::<Div>::new();
    mount_to_body(move || {
        registry::provide_cards(registry);
        provide_context(store);
        app::provide_board_contexts(store);
        provide_context::<OutboundTx>(tx.clone());
        let delete_tx = delete_tx.clone();
        provide_context(DeleteGame(Callback::new(move |game_id| {
            let _ = delete_tx.unbounded_send(game_id);
        })));
        view! { <div class="board-root" node_ref=root><MainColumn/></div> }
    });
    task::tick().await;
    BoardHandle {
        root: root.get_untracked().expect("the board root mounted").into(),
        store,
        rx,
        deleted,
    }
}

/// The account a browser test's client is signed in as.
pub(crate) const YOU: AccountId = AccountId(1);

/// [`YOU`] as a member of the game, which every `Hello` here names as its
/// creator and only member.
pub(crate) fn you_member() -> Member {
    Member {
        account: YOU,
        display_name: "You".to_string(),
    }
}

/// Every investigator `game` seats or `outcome` asks, held by [`YOU`].
pub(crate) fn you_hold(game: Option<&GameState>, outcome: Option<&EngineOutcome>) -> Assignment {
    let mut investigators: Vec<InvestigatorId> = game
        .iter()
        .flat_map(|game| game.investigators.keys().copied())
        .collect();
    if let Some(EngineOutcome::AwaitingInput { request, .. }) = outcome {
        if let Responder::Investigator(investigator) | Responder::Lead(investigator) =
            request.responder
        {
            investigators.push(investigator);
        }
        investigators.extend(request.holders());
    }
    Assignment::all_to(YOU, investigators)
}

/// Sign `client` in as [`YOU`], holding every investigator its game seats or
/// its prompt asks: what a `Hello` seeds, for a test that sets the prompt
/// directly. Call it once the prompt is in place.
pub(crate) fn you_hold_every_investigator(client: &mut ClientState) {
    client.you = Some(YOU);
    client.assigned = you_hold(client.game.as_ref(), client.outcome.as_ref());
}

/// The client state a `Hello` carrying `state` and `outcome` leaves behind,
/// folded through the transport's own reducer, signed in as [`YOU`] holding
/// every investigator. The status is `Connected`, as the transport sets it when
/// the `Hello` arrives, so the board takes input.
pub(crate) fn hello(state: GameState, outcome: EngineOutcome) -> ClientState {
    let mut client = ClientState {
        status: ConnStatus::Connected,
        ..ClientState::default()
    };
    let assigned = you_hold(Some(&state), Some(&outcome));
    store::reduce(
        &mut client,
        ServerMessage::Hello {
            build: protocol::build_id().to_string(),
            you: YOU,
            creator: YOU,
            members: vec![you_member()],
            assigned,
            prompt: LogPosition(0),
            state: Box::new(state),
            outcome,
            events: Vec::new(),
        },
    );
    client
}

/// The text frame of a `Hello` from build `build`, carrying `state`,
/// `outcome` and setup's `events`: what the transport hands
/// [`store::receive`].
pub(crate) fn hello_frame(
    build: &str,
    state: GameState,
    outcome: EngineOutcome,
    events: Vec<Event>,
) -> String {
    let assigned = you_hold(Some(&state), Some(&outcome));
    serde_json::to_string(&ServerMessage::Hello {
        build: build.to_string(),
        you: YOU,
        creator: YOU,
        members: vec![you_member()],
        assigned,
        prompt: LogPosition(0),
        state: Box::new(state),
        outcome,
        events,
    })
    .expect("a Hello serializes")
}

/// Feed `handle`'s store `message`, as the transport would, and let the page
/// react.
pub(crate) async fn arrives(handle: &BoardHandle, message: ServerMessage) {
    let frame = serde_json::to_string(&message).expect("a ServerMessage serializes");
    handle.store.update(|s| {
        store::receive(s, &frame);
    });
    task::tick().await;
}

/// A build id that is not this client's.
pub(crate) fn another_build() -> String {
    format!("not-{}", protocol::build_id())
}

/// The in-process server: holds the authoritative live game, submits whatever
/// the UI sends to it, and folds the result back into the store.
pub(crate) struct Harness {
    pub(crate) board: BoardHandle,
    pub(crate) live: LiveGame,
}

impl Harness {
    /// Mount the board plus the app's overlays against a fresh store seeded with
    /// `state` and the engine's opening outcome for it.
    pub(crate) async fn mount(state: GameState, outcome: EngineOutcome) -> Self {
        // The real corpus, not the synthetic registry: the engine runs the
        // real cards, and the board renders their faces.
        let board = mount_board(hello(state.clone(), outcome.clone()), &cards::TEST_REGISTRY).await;
        Self {
            board,
            live: LiveGame::new(
                state.clone(),
                outcome.clone(),
                Vec::new(),
                LogPosition::default(),
                you_hold(Some(&state), Some(&outcome)),
                YOU,
                cards::TEST_REGISTRIES,
            ),
        }
    }

    /// Drain one submitted frame, submit it to the live game and commit it, and
    /// fold the `Applied` back in — the transport's round trip, in-process.
    pub(crate) async fn pump(&mut self) {
        let ClientMessage::Submit { prompt, action } = self
            .board
            .rx
            .try_recv()
            .expect("the click submitted a frame")
            .clone()
        else {
            panic!("the click submitted a frame other than a Submit");
        };
        let accepted = self
            .live
            .submit(YOU, prompt, action)
            .unwrap_or_else(|refusal| {
                panic!("the board offered something the server refuses: {refusal}")
            });
        let applied = self.live.commit(accepted).applied_for(YOU);
        let store = self.board.store;
        store.update(|s| store::reduce(s, ServerMessage::Applied(applied)));
        task::tick().await;
    }

    pub(crate) fn state(&self) -> &GameState {
        self.live.state()
    }

    pub(crate) fn find(&self, sel: &str) -> HtmlElement {
        self.board
            .root
            .query_selector(sel)
            .expect("query")
            .and_then(|n| n.dyn_into::<HtmlElement>().ok())
            .unwrap_or_else(|| panic!("no `{sel}` on the board"))
    }

    /// True iff nothing under the mounted root matches `sel`.
    pub(crate) fn absent(&self, sel: &str) -> bool {
        absent(&self.board.root, sel)
    }
}

/// The words every Leptos "signal read outside a reactive tracking context"
/// warning carries.
const UNTRACKED_READ_WARNING: &str = "outside a reactive tracking context";

/// A recorder standing in for the global `console.warn` until it is dropped,
/// which puts the original back.
///
/// Leptos raises its debug-build diagnostics through `console.warn`, so a test
/// that holds one sees every warning the views it mounts log.
pub(crate) struct WarnCapture {
    console: Object,
    original: JsValue,
    messages: Rc<RefCell<Vec<String>>>,
    _recorder: Closure<dyn Fn(JsValue)>,
}

impl WarnCapture {
    /// Swap the recorder in for `console.warn`.
    pub(crate) fn start() -> Self {
        let console: Object = Reflect::get(&js_sys::global(), &"console".into())
            .expect("a global console")
            .unchecked_into();
        let original = Reflect::get(&console, &"warn".into()).expect("console.warn exists");
        let messages = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&messages);
        let recorder = Closure::<dyn Fn(JsValue)>::new(move |message: JsValue| {
            sink.borrow_mut().push(
                message
                    .as_string()
                    .unwrap_or_else(|| format!("{message:?}")),
            );
        });
        Reflect::set(&console, &"warn".into(), recorder.as_ref()).expect("replace console.warn");
        Self {
            console,
            original,
            messages,
            _recorder: recorder,
        }
    }

    /// Every message recorded so far that reports a signal read outside a
    /// reactive tracking context, cut to its first sentence: where the read
    /// is, and where the signal was defined.
    pub(crate) fn untracked_reads(&self) -> Vec<String> {
        self.messages
            .borrow()
            .iter()
            .filter_map(|m| {
                let end = m.find(UNTRACKED_READ_WARNING)?;
                Some(m[..end].trim_end().to_owned())
            })
            .collect()
    }
}

impl Drop for WarnCapture {
    fn drop(&mut self) {
        let _ = Reflect::set(&self.console, &"warn".into(), &self.original);
    }
}
