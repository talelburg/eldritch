//! Shared helpers for the web crate's browser tests.

use futures::channel::mpsc::{self, UnboundedReceiver};
use game_core::card_registry::CardRegistry;
use game_core::engine::{EngineOutcome, Responder};
use game_core::event::Event;
use game_core::state::investigator::InvestigatorId;
use game_core::state::GameState;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task;
use protocol::lobby::Member;
use protocol::{AccountId, Assignment, ClientMessage, GameId, LogPosition, ServerMessage};
use wasm_bindgen::JsCast as _;
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

/// The text of every element `sel` matches inside `within`, in document order.
pub(crate) fn texts_in(within: &Element, sel: &str) -> Vec<String> {
    let nodes = within.query_selector_all(sel).expect("query");
    (0..nodes.length())
        .filter_map(|i| nodes.item(i))
        .map(|node| node.text_content().unwrap_or_default())
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
    let all = root.query_selector_all(sel).expect("query");
    let el = (0..all.length())
        .filter_map(|i| all.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .find(|el| {
            el.query_selector(name_sel)
                .expect("query")
                .and_then(|n| n.text_content())
                .is_some_and(|text| text == name)
        })
        .unwrap_or_else(|| panic!("a {sel} named {name}"));
    el.dyn_into::<HtmlElement>()
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
