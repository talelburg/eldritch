//! The GitHub adapter against a fake GitHub on a loopback port, answering
//! the token exchange and `GET /user/emails` as GitHub does.

use std::collections::HashMap;

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Form, Json, Router};
use serde_json::json;
use server::auth::github::GitHub;
use server::auth::provider::{IdentityProvider, ReportedEmail};
use server::config::{ProviderCredentials, Secret};
use tokio::net::TcpListener;
use url::Url;

/// The fake's token exchange: a token for `the-code`, sent with the app's
/// credentials and a PKCE verifier.
async fn token(Form(form): Form<HashMap<String, String>>) -> Response {
    let sent = |name: &str| form.get(name).map(String::as_str);
    if sent("code") != Some("the-code")
        || sent("client_id") != Some("Iv1.abc")
        || sent("client_secret") != Some("hunter2")
        || sent("code_verifier").is_none()
    {
        return Json(json!({ "error": "bad_verification_code" })).into_response();
    }
    Json(json!({ "access_token": "gho_t", "token_type": "bearer", "scope": "user:email" }))
        .into_response()
}

/// The fake's `GET /user/emails`. Like GitHub, it refuses a request without
/// a `User-Agent`.
async fn emails(headers: HeaderMap) -> Response {
    if !headers.contains_key(header::USER_AGENT) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if headers
        .get(header::AUTHORIZATION)
        .map(HeaderValue::as_bytes)
        != Some(b"Bearer gho_t")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!([
        { "email": "sam@work.com", "primary": true, "verified": true, "visibility": "public" },
        { "email": "sam@gmail.com", "primary": false, "verified": true, "visibility": null },
        { "email": "sam@old.com", "primary": false, "verified": false, "visibility": null },
    ]))
    .into_response()
}

/// The adapter, pointed at a fake GitHub serving both the web and the API.
async fn github() -> GitHub {
    let fake = Router::new()
        .route("/login/oauth/access_token", routing::post(token))
        .route("/user/emails", routing::get(emails));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = Url::parse(&format!("http://{}", listener.local_addr().expect("addr"))).unwrap();
    tokio::spawn(async move { axum::serve(listener, fake).await.expect("serve") });
    let credentials = ProviderCredentials {
        client_id: "Iv1.abc".to_string(),
        client_secret: Secret::new("hunter2".to_string()),
    };
    GitHub::with_base_urls(&credentials, &base, &base)
}

/// The callback the tests register.
fn redirect_uri() -> Url {
    Url::parse("http://localhost:3000/auth/github/callback").unwrap()
}

#[tokio::test]
async fn github_asks_for_the_email_scope_with_pkce_and_the_state() {
    let (url, pending) = github().await.authorize_url(&redirect_uri());

    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(url.path(), "/login/oauth/authorize");
    assert_eq!(query["client_id"], "Iv1.abc");
    assert_eq!(query["scope"], "user:email");
    assert_eq!(query["state"], pending.state());
    assert_eq!(query["redirect_uri"], redirect_uri().as_str());
    assert_eq!(query["code_challenge_method"], "S256");
}

#[tokio::test]
async fn github_reports_every_address_with_its_own_verified_flag() {
    let github = github().await;
    let (_, pending) = github.authorize_url(&redirect_uri());

    let reported = github
        .exchange("the-code".to_string(), pending, &redirect_uri())
        .await
        .expect("the exchange succeeds");

    let email = |address: &str, verified| ReportedEmail {
        address: address.to_string(),
        verified,
    };
    assert_eq!(
        reported,
        vec![
            email("sam@work.com", true),
            email("sam@gmail.com", true),
            email("sam@old.com", false),
        ]
    );
}

#[tokio::test]
async fn a_code_github_refuses_fails_the_exchange() {
    let github = github().await;
    let (_, pending) = github.authorize_url(&redirect_uri());

    let exchanged = github
        .exchange("not-the-code".to_string(), pending, &redirect_uri())
        .await;

    assert!(exchanged.is_err());
}
