//! The event log, as each account at the table reads it: a card searched into
//! hand is named only to the account holding the investigator who searched.
//! Every other account, viewers included, reads that a card was searched, but
//! not which.
//!
//! `glossary/Search.md`: *"When a player is instructed to search for a card,
//! that player is permitted to look at all of the cards in the searched area
//! without revealing those cards to the other players."*
//!
//! Each game is seated as [`tabs::mount_table`]'s: Tal, [`YOU`], holds Roland
//! Banks and Agnes Baker, Sam holds Daisy Walker, and Alex is a viewer. Each
//! prompt is the engine's own, reached through a test session, and the answer
//! goes through the in-process server, which sends each account its `Applied`.
#![cfg(target_arch = "wasm32")]

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{EngineOutcome, OptionTarget};
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, DeckIndex, DiscardIndex, GameStateBuilder,
    InvestigatorId, LocationId,
};
use game_core::test_support::{self, location, seat, BoardLookup as _, TestSession};
use leptos::prelude::*;
use leptos::task;
use protocol::live::LiveGame;
use protocol::{AccountId, Assignment, LogPosition, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::event_log::EventLogView;
use web_sys::HtmlElement;

use crate::common::{self, BoardHandle, YOU};
use crate::lobby::SAM;
use crate::tabs::{self, AGNES, ALEX, DAISY, ROLAND};

/// The Study 01111, where everyone stands.
const STUDY: LocationId = LocationId(10);

const OLD_BOOK_OF_LORE: &str = "01031";
const RESEARCH_LIBRARIAN: &str = "01032";
const MEDICAL_TEXTS: &str = "01035";
const BEAT_COP: &str = "01018";
const MAGNIFYING_GLASS: &str = "01030";
const EMERGENCY_CACHE: &str = "01088";
const GUTS: &str = "01089";
const SCAVENGING: &str = "01073";
/// Items, for Scavenging to take back.
const FLASHLIGHT: &str = "01087";
const KNIFE: &str = "01086";

/// Roland's deck, top first: Magnifying Glass in the top 3, and two Tomes
/// below them.
const ROLANDS_DECK: [&str; 6] = [
    BEAT_COP,
    MAGNIFYING_GLASS,
    EMERGENCY_CACHE,
    MEDICAL_TEXTS,
    GUTS,
    OLD_BOOK_OF_LORE,
];

/// The table in the Study, Roland holding [`ROLANDS_DECK`] and `rolands_hand`,
/// and Daisy with Old Book of Lore in play.
fn table(rolands_hand: &[&str]) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(STUDY).as_card("01111"))
        .with_seat(
            seat(ROLAND.0)
                .as_card("01001")
                .at(STUDY)
                .deck(ROLANDS_DECK)
                .hand(rolands_hand.iter().copied())
                .resources(5),
        )
        .with_seat(seat(AGNES.0).as_card("01004").at(STUDY).deck([GUTS]))
        .with_seat(
            seat(DAISY.0)
                .as_card("01002")
                .at(STUDY)
                .deck([GUTS])
                .in_play([OLD_BOOK_OF_LORE]),
        )
        .with_seating([ROLAND, DAISY, AGNES])
}

/// Daisy reads Old Book of Lore 01031 and chooses Roland: *"Choose an
/// investigator at your location. That investigator searches the top 3 cards
/// of his or her deck for a card, draws it…"* The session rests at Roland's
/// pick among his top 3.
fn daisy_has_roland_search() -> TestSession {
    let session = table(&[]).open_turn(DAISY).session();
    let book = session.state().in_play(DAISY, OLD_BOOK_OF_LORE);
    let roland = session.state().investigator_card(ROLAND);
    session
        .take(&TurnAction::ActivateAbility {
            investigator: DAISY,
            source: AbilitySource::InPlay(book),
            address: AbilityAddress::Printed(0),
        })
        .pick(roland)
}

/// Roland plays Research Librarian 01032: *"Search your deck for a \[\[Tome\]\]
/// asset and add it to your hand."* The session rests at his pick among his
/// whole deck.
fn roland_plays_research_librarian() -> TestSession {
    let session = table(&[RESEARCH_LIBRARIAN])
        .open_turn(ROLAND)
        .session()
        .take(&TurnAction::PlayCard {
            investigator: ROLAND,
            source: PlaySource::Hand(0),
        });
    let librarian = session.state().card(ROLAND, RESEARCH_LIBRARIAN);
    session.pick(librarian)
}

/// Roland, with Scavenging 01073 in play and two Items in his discard pile,
/// investigates a shroud-1 location and succeeds by 2: *"After you
/// successfully investigate by 2 or more, exhaust Scavenging: Choose an
/// \[\[Item\]\] card in your discard pile and add it to your hand."* The
/// session rests at his choice among the Items in his discard pile.
fn roland_scavenges() -> TestSession {
    let session = GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(STUDY).shroud(1).clues(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_seat(
            seat(ROLAND.0)
                .as_card("01001")
                .at(STUDY)
                .in_play([SCAVENGING])
                .discard([FLASHLIGHT, GUTS, KNIFE]),
        )
        .with_seat(seat(AGNES.0).as_card("01004").at(STUDY))
        .with_seat(seat(DAISY.0).as_card("01002").at(STUDY))
        .with_seating([ROLAND, DAISY, AGNES])
        .open_turn(ROLAND)
        .session();
    let scavenging = session.state().card(ROLAND, SCAVENGING);
    session
        .take(&TurnAction::Investigate {
            investigator: ROLAND,
        })
        .commit(&[])
        .pick(scavenging)
}

/// One account's client: its board, and its event log over the same store.
struct Client {
    board: BoardHandle,
    log: HtmlElement,
}

impl Client {
    /// `you`'s client at the prompt `session` rests at. The log is the
    /// app's own drawer, mounted beside the board over the board's store.
    async fn at(you: AccountId, session: &TestSession) -> Self {
        let board = tabs::mount_as(you, session).await;
        let log = document()
            .create_element("div")
            .expect("create_element")
            .dyn_into::<HtmlElement>()
            .expect("HtmlElement");
        document()
            .body()
            .expect("body")
            .append_child(&log)
            .expect("append_child");
        let store = board.store;
        mount_to(log.clone(), move || {
            provide_context(store);
            view! { <EventLogView/> }
        })
        .forget();
        task::tick().await;
        Self { board, log }
    }

    /// The text of the event log: every batch's header and event line,
    /// whether or not the drawer is open.
    fn log_text(&self) -> String {
        common::text_of(&self.log, ".event-log .log-scroll")
    }
}

/// The table's three accounts, each with its client at the prompt a test
/// session rests at, and the in-process server holding the same game.
struct Table {
    live: LiveGame,
    tal: Client,
    sam: Client,
    alex: Client,
}

impl Table {
    /// Mount the table as each of Tal, Sam and Alex, at the prompt `session`
    /// rests at.
    async fn at(session: &TestSession) -> Self {
        let outcome = test_support::awaiting_request(session.prompt().clone());
        let live = LiveGame::new(
            session.state().clone(),
            outcome,
            Vec::new(),
            LogPosition(0),
            Assignment::from_iter([(ROLAND, YOU), (AGNES, YOU), (DAISY, SAM)]),
            YOU,
            cards::TEST_REGISTRIES,
        );
        Self {
            live,
            tal: Client::at(YOU, session).await,
            sam: Client::at(SAM, session).await,
            alex: Client::at(ALEX, session).await,
        }
    }

    /// `investigator`, held by `account`, picks the live prompt's option
    /// anchored to `target`. The server commits it, and each account's client
    /// receives the `Applied` it is sent.
    async fn pick(
        &mut self,
        account: AccountId,
        investigator: InvestigatorId,
        target: OptionTarget,
    ) {
        let EngineOutcome::AwaitingInput { request, .. } = self.live.outcome() else {
            panic!("the game awaits no prompt");
        };
        let option = request
            .options
            .iter()
            .find(|option| option.target.as_ref() == Some(&target))
            .unwrap_or_else(|| panic!("no option anchored to {target:?}"))
            .id;
        let action = PlayerAction::ResolveInput {
            investigator,
            response: InputResponse::PickSingle(option),
        };
        let accepted = self
            .live
            .submit(account, self.live.prompt(), action)
            .unwrap_or_else(|refusal| panic!("the server refuses the pick: {refusal}"));
        let committed = self.live.commit(accepted);
        for (you, client) in self.each() {
            common::arrives(
                &client.board,
                ServerMessage::Applied(committed.applied_for(you)),
            )
            .await;
        }
    }

    /// Every account's client receives `message`.
    async fn each_receives(&self, message: ServerMessage) {
        for (_, client) in self.each() {
            common::arrives(&client.board, message.clone()).await;
        }
    }

    fn each(&self) -> [(AccountId, &Client); 3] {
        [(YOU, &self.tal), (SAM, &self.sam), (ALEX, &self.alex)]
    }
}

/// The anchor of the card `index` from the top of Roland's deck.
fn rolands_deck_card(index: usize) -> OptionTarget {
    OptionTarget::DeckCard {
        investigator: ROLAND,
        index: DeckIndex::new(index),
    }
}

/// Assert `client`'s log says Roland searched a card into hand without naming
/// it: no name, no code.
fn assert_hides(client: &Client, who: &str, name: &str, code: &str) {
    let log = client.log_text();
    assert!(
        log.contains("Roland Banks: a card"),
        "{who}: the header says Roland took a card: {log}"
    );
    assert!(
        log.contains("CardSearchedToHand { investigator: InvestigatorId(1), code: <hidden> }"),
        "{who}: the event line keeps its shape with the code masked: {log}"
    );
    assert!(
        !log.contains(name) && !log.contains(code),
        "{who}: the log names neither {name} nor {code}: {log}"
    );
}

/// Tal holds Roland, who drew Magnifying Glass, so his log names it in the
/// header and the event line. Sam's, whose Daisy read the book, and Alex's
/// name neither.
#[wasm_bindgen_test]
async fn old_book_of_lore_names_the_card_only_to_the_searchers_holder() {
    let mut table = Table::at(&daisy_has_roland_search()).await;

    table.pick(YOU, ROLAND, rolands_deck_card(1)).await;

    let tal = table.tal.log_text();
    assert!(
        tal.contains("Roland Banks: Magnifying Glass"),
        "Tal's header names the card: {tal}"
    );
    assert!(
        tal.contains(&format!("code: CardCode(\"{MAGNIFYING_GLASS}\")")),
        "Tal's event line names its code: {tal}"
    );
    assert_hides(&table.sam, "Sam", "Magnifying Glass", MAGNIFYING_GLASS);
    assert_hides(&table.alex, "Alex", "Magnifying Glass", MAGNIFYING_GLASS);
}

/// The log follows who holds Roland now: once Tal reassigns him to Sam, Sam's
/// log names Magnifying Glass, which is in the hand Sam now sees, and Tal's
/// no longer does.
#[wasm_bindgen_test]
async fn a_reassignment_moves_the_searched_card_to_the_new_holders_log() {
    let mut table = Table::at(&daisy_has_roland_search()).await;
    table.pick(YOU, ROLAND, rolands_deck_card(1)).await;

    table
        .each_receives(ServerMessage::Assigned {
            by: YOU,
            investigator: ROLAND,
            assigned: Assignment::from_iter([(ROLAND, SAM), (AGNES, YOU), (DAISY, SAM)]),
        })
        .await;

    let sam = table.sam.log_text();
    assert!(
        sam.contains("Roland Banks: Magnifying Glass") && sam.contains(MAGNIFYING_GLASS),
        "Sam now holds Roland, so his log names the card: {sam}"
    );
    assert_hides(&table.tal, "Tal", "Magnifying Glass", MAGNIFYING_GLASS);
    assert_hides(&table.alex, "Alex", "Magnifying Glass", MAGNIFYING_GLASS);
}

/// Research Librarian searches Roland's whole deck, which is hidden the same
/// way: Tal's log names the Tome he took, and Sam's and Alex's don't.
#[wasm_bindgen_test]
async fn a_searched_deck_names_the_card_only_to_the_searchers_holder() {
    let mut table = Table::at(&roland_plays_research_librarian()).await;

    table.pick(YOU, ROLAND, rolands_deck_card(3)).await;

    let tal = table.tal.log_text();
    assert!(
        tal.contains("Roland Banks: Medical Texts") && tal.contains(MEDICAL_TEXTS),
        "Tal's log names the Tome: {tal}"
    );
    assert_hides(&table.sam, "Sam", "Medical Texts", MEDICAL_TEXTS);
    assert_hides(&table.alex, "Alex", "Medical Texts", MEDICAL_TEXTS);
}

/// A discard pile is open information (`glossary/Discard_Piles.md`: *"Each
/// discard pile is open information"*), so a Scavenging pick from one keeps
/// naming its card in every account's log.
#[wasm_bindgen_test]
async fn a_pick_from_a_discard_pile_is_named_to_everyone() {
    let mut table = Table::at(&roland_scavenges()).await;

    table
        .pick(
            YOU,
            ROLAND,
            OptionTarget::DiscardCard {
                investigator: ROLAND,
                index: DiscardIndex::new(0),
            },
        )
        .await;

    for (who, client) in [
        ("Tal", &table.tal),
        ("Sam", &table.sam),
        ("Alex", &table.alex),
    ] {
        let log = client.log_text();
        assert!(
            log.contains("Roland Banks: Flashlight"),
            "{who}'s log names the Item: {log}"
        );
    }
}
