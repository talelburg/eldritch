//! The production server installs the synthetic scenario + card
//! registries (D5), so the real `POST /games` for the toy scenario
//! succeeds and synthetic card codes resolve. Process-isolated: this
//! test binary installs the *real* synthetic registries, distinct from
//! the mock registry other test binaries install via `common`.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::memory_pool;
use game_core::scenario::ScenarioId;
use game_core::state::CardCode;
use scenarios::test_fixtures::synth_cards::SYNTH_TREACHERY_CODE;
use tower::ServiceExt;

#[tokio::test]
async fn install_registries_resolves_synthetic_scenario_and_cards() {
    server::install_registries();

    let scenario = game_core::scenario_registry::current().expect("scenario registry installed");
    let id = ScenarioId::new("synthetic");
    assert!(
        (scenario.module_for)(&id).is_some(),
        "synthetic scenario module must resolve"
    );

    let cards = game_core::card_registry::current().expect("card registry installed");
    let code = CardCode(SYNTH_TREACHERY_CODE.into());
    assert!(
        (cards.metadata_for)(&code).is_some(),
        "synthetic card metadata must resolve"
    );
    assert!(
        (cards.abilities_for)(&code).is_some(),
        "synthetic card abilities must resolve"
    );
}

#[tokio::test]
async fn post_games_creates_synthetic_game_against_installed_registries() {
    server::install_registries();
    let pool = memory_pool().await;
    let app = server::app(server::AppState::new(pool));

    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"scenario_id":"synthetic"}"#))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
}
