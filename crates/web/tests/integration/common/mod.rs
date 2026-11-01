//! Shared helpers for the web crate's browser tests.

use futures::channel::mpsc::{self, UnboundedReceiver};
use game_core::card_registry::CardRegistry;
use game_core::engine::EngineOutcome;
use game_core::state::GameState;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task;
use protocol::{ClientMessage, ServerMessage};
use wasm_bindgen::JsCast as _;
use web::app::{self, MainColumn};
use web::registry;
use web::store::{self, ClientState, ConnStatus};
use web::transport::OutboundTx;
use web_sys::{Element, HtmlElement};

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
    let root = NodeRef::<Div>::new();
    mount_to_body(move || {
        registry::provide_cards(registry);
        provide_context(store);
        app::provide_board_contexts(store);
        provide_context::<OutboundTx>(tx.clone());
        view! { <div class="board-root" node_ref=root><MainColumn/></div> }
    });
    task::tick().await;
    BoardHandle {
        root: root.get_untracked().expect("the board root mounted").into(),
        store,
        rx,
    }
}

/// The client state a `Hello` carrying `state` and `outcome` leaves behind,
/// folded through the transport's own reducer. The status is `Connected`, as
/// the transport sets it when the `Hello` arrives, so the board takes input.
pub(crate) fn hello(state: GameState, outcome: EngineOutcome) -> ClientState {
    let mut client = ClientState {
        status: ConnStatus::Connected,
        ..ClientState::default()
    };
    store::reduce(
        &mut client,
        ServerMessage::Hello {
            state: Box::new(state),
            outcome,
            events: Vec::new(),
        },
    );
    client
}
