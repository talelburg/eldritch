//! The server serves the client bundle from its dist dir as a router
//! fallback, with `index.html` as the SPA fallback (D3). Uses a
//! committed fixture dir because the real `crates/web/dist/` is
//! gitignored (Trunk output) and absent in CI/fresh checkouts.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

const FIXTURE_DIST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dist");

async fn app_with_fixture_dist() -> axum::Router {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");
    server::db::MIGRATOR.run(&pool).await.expect("migrate");
    server::app(server::AppState::new_with_dist(pool, FIXTURE_DIST.into()))
}

async fn body_string(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn root_serves_index_html() {
    let app = app_with_fixture_dist().await;
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        body_string(response).await.contains("eldritch-test-bundle"),
        "GET / serves the bundle's index.html"
    );
}

#[tokio::test]
async fn unknown_route_falls_back_to_index_html() {
    let app = app_with_fixture_dist().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/game/abc/some-spa-route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        body_string(response).await.contains("eldritch-test-bundle"),
        "an unmatched route falls back to index.html (SPA routing)"
    );
}
