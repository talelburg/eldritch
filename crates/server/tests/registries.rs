//! The production server runs on the real `scenarios` + `cards` registries
//! (C7a): `POST /games` for The Gathering succeeds and real card codes
//! resolve. This binary hands the server the *production* pairing,
//! `server::REGISTRIES`, where the other test binaries pass the mock pairing
//! from `common`.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use game_core::scenario::ScenarioId;
use game_core::state::CardCode;
use scenarios::the_gathering::ID as GATHERING_SCENARIO_ID;
use server::AppState;
use tower::ServiceExt as _;

/// A real card with both corpus metadata and a hand-written `abilities()`
/// impl — Dr. Milan Christopher 01033.
const REAL_CARD: &str = "01033";

#[test]
fn the_production_registries_resolve_the_gathering_and_real_cards() {
    let scenario = server::REGISTRIES.scenarios;
    let id = ScenarioId::new(GATHERING_SCENARIO_ID);
    assert!(
        (scenario.module_for)(&id).is_some(),
        "The Gathering scenario module must resolve"
    );

    let cards = server::REGISTRIES.cards;
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
async fn post_games_creates_the_gathering_against_the_production_registries() {
    let pool = common::memory_pool().await;
    let app = server::app(AppState::new(pool, server::REGISTRIES));

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
