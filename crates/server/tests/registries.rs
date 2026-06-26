//! The production server installs the real `scenarios` + `cards`
//! registries (C7a): `POST /games` for The Gathering succeeds and real
//! card codes resolve. Process-isolated: this test binary installs the
//! *production* registries via `server::install_registries()`, distinct
//! from the mock registry other test binaries install via `common`.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::memory_pool;
use game_core::scenario::ScenarioId;
use game_core::state::CardCode;
use scenarios::the_gathering::ID as GATHERING_SCENARIO_ID;
use tower::ServiceExt;

/// A real card with both corpus metadata and a hand-written `abilities()`
/// impl — Dr. Milan Christopher 01033.
const REAL_CARD: &str = "01033";

#[test]
fn install_registries_resolves_the_gathering_and_real_cards() {
    server::install_registries();

    let scenario = game_core::scenario_registry::current().expect("scenario registry installed");
    let id = ScenarioId::new(GATHERING_SCENARIO_ID);
    assert!(
        (scenario.module_for)(&id).is_some(),
        "The Gathering scenario module must resolve"
    );

    let cards = game_core::card_registry::current().expect("card registry installed");
    let code = CardCode(REAL_CARD.into());
    assert!(
        (cards.metadata_for)(&code).is_some(),
        "real card metadata must resolve from cards::REGISTRY"
    );
    assert!(
        (cards.abilities_for)(&code).is_some(),
        "real card abilities must resolve from cards::REGISTRY"
    );
}

#[tokio::test]
async fn post_games_creates_the_gathering_against_installed_registries() {
    server::install_registries();
    let pool = memory_pool().await;
    let app = server::app(server::AppState::new(pool));

    // Seat Roland Banks (01001) — a real investigator from the production
    // card registry.
    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"scenario_id":"{GATHERING_SCENARIO_ID}","roster":[{{"investigator":"01001","deck":[]}}]}}"#
        )))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
}
