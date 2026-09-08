//! The `/health` readiness probe answers 200 when the pooled database
//! is reachable, exercising the `SqlitePool` wired into Axum app state.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use server::db::MIGRATOR;
use server::AppState;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt as _;

#[tokio::test]
async fn health_returns_ok_when_database_reachable() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");
    MIGRATOR.run(&pool).await.expect("run migrations");

    let app = server::app(AppState::new(pool));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router handles the request");

    assert_eq!(response.status(), StatusCode::OK);
}
