//! Signing in through an identity provider, at the app's HTTP boundary: a
//! fake provider reports the emails, and the tests follow the browser from
//! `/auth/fake/login` through `/auth/fake/callback`.

use std::sync::Arc;

use axum::http::{header, HeaderMap, StatusCode};
use axum::Router;
use protocol::{SignInMethod, SignInRequired, SignedIn};
use server::accounts::{self, Email, EntryRef};
use server::auth;
use server::auth::github::GitHub;
use server::auth::google::Google;
use server::auth::provider::IdentityProvider;
use server::config::{ProviderCredentials, Secret};
use server::AppState;
use sqlx::SqlitePool;
use url::Url;

use crate::common::{self, FakeProvider, FAILING_CODE, FAKE_ORIGIN, FAKE_STATE};

/// The pool, with Sam on the allowlist as `sam@gmail.com`.
async fn pool_with_sam() -> SqlitePool {
    let pool = common::memory_pool().await;
    accounts::add(&pool, "Sam", &["sam@gmail.com".to_string()])
        .await
        .expect("add Sam");
    pool
}

/// Start a sign-in on `app` as the browser does: the `state` the provider is
/// sent, and the in-flight cookie as a `Cookie` header value.
async fn start(app: &Router) -> (String, String) {
    let (status, headers, _) = common::get(app, "/auth/fake/login", None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = Url::parse(location(&headers)).expect("an absolute provider URL");
    let state = location
        .query_pairs()
        .find(|(name, _)| name == "state")
        .expect("the provider is sent a state")
        .1
        .into_owned();
    (state, common::cookie_pair(&headers, auth::SIGN_IN_COOKIE))
}

/// The response's `Location`.
fn location(headers: &HeaderMap) -> &str {
    headers
        .get(header::LOCATION)
        .expect("a Location")
        .to_str()
        .expect("an ASCII Location")
}

/// Assert the response clears the in-flight cookie, at the path it was set on.
fn assert_in_flight_cleared(headers: &HeaderMap) {
    let cleared = common::set_cookie(headers, auth::SIGN_IN_COOKIE).expect("a removal");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    assert!(cleared.contains("Path=/auth"), "{cleared}");
}

/// Start a sign-in on `app` and come back from the provider with `code`.
async fn sign_in(app: &Router, code: &str) -> (StatusCode, HeaderMap, String) {
    let (state, cookie) = start(app).await;
    common::get(
        app,
        &format!("/auth/fake/callback?code={code}&state={state}"),
        Some(&cookie),
    )
    .await
}

#[tokio::test]
async fn signed_out_me_lists_each_provider_before_the_dev_picker() {
    let app = common::provider_app(
        common::memory_pool().await,
        FakeProvider::reporting(&[]),
        true,
    );

    let (status, _, body) = common::get(&app, "/auth/me", None).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let required: SignInRequired = serde_json::from_str(&body).expect("a method list");
    assert_eq!(
        required.methods,
        vec![
            SignInMethod {
                label: "Sign in with Fake".to_string(),
                url: "/auth/fake/login".to_string(),
            },
            SignInMethod {
                label: "Dev sign-in".to_string(),
                url: "/auth/dev".to_string(),
            },
        ]
    );
}

#[tokio::test]
async fn signed_out_me_lists_github_then_google() {
    let credentials = ProviderCredentials {
        client_id: "client-id".to_string(),
        client_secret: Secret::new("client-secret".to_string()),
    };
    let providers: Vec<Arc<dyn IdentityProvider>> = vec![
        Arc::new(GitHub::new(&credentials)),
        Arc::new(Google::new(&credentials)),
    ];
    let app = server::app(
        AppState::new(common::memory_pool().await, common::REGISTRIES)
            .with_identity_providers(&Url::parse(FAKE_ORIGIN).expect("a URL"), providers),
    );

    let (status, _, body) = common::get(&app, "/auth/me", None).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let required: SignInRequired = serde_json::from_str(&body).expect("a method list");
    assert_eq!(
        required.methods,
        vec![
            SignInMethod {
                label: "Sign in with GitHub".to_string(),
                url: "/auth/github/login".to_string(),
            },
            SignInMethod {
                label: "Sign in with Google".to_string(),
                url: "/auth/google/login".to_string(),
            },
        ]
    );
}

#[tokio::test]
async fn login_sends_the_browser_to_the_provider_with_the_configured_callback() {
    let app = common::provider_app(
        common::memory_pool().await,
        FakeProvider::reporting(&[]),
        false,
    );

    let (status, headers, _) = common::get(&app, "/auth/fake/login", None).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = Url::parse(location(&headers)).expect("an absolute provider URL");
    assert_eq!(location.host_str(), Some("fake.example"));
    assert!(
        location
            .query_pairs()
            .any(|(name, value)| name == "redirect_uri"
                && value == format!("{FAKE_ORIGIN}/auth/fake/callback")),
        "{location}"
    );
    let cookie = common::set_cookie(&headers, auth::SIGN_IN_COOKIE).expect("in-flight cookie");
    for attribute in [
        "HttpOnly",
        "Secure",
        "SameSite=Lax",
        "Path=/auth",
        "Max-Age=600",
    ] {
        assert!(cookie.contains(attribute), "{attribute}: {cookie}");
    }
    assert!(!cookie.contains(FAKE_STATE), "encrypted: {cookie}");
}

#[tokio::test]
async fn a_login_the_provider_cannot_start_gets_the_try_again_page_and_no_cookie() {
    let app = common::provider_app(
        common::memory_pool().await,
        FakeProvider::unreachable(),
        false,
    );

    let (status, headers, page) = common::get(&app, "/auth/fake/login", None).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(page.contains("Try again"), "{page}");
    assert_eq!(common::set_cookie(&headers, auth::SIGN_IN_COOKIE), None);
}

/// Sam's first verified address isn't on the list; the second is.
#[tokio::test]
async fn a_verified_address_that_is_not_the_first_signs_in_as_its_account() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam@work.com", true), ("Sam@Gmail.com", true)]),
        false,
    );

    let (status, headers, _) = sign_in(&app, "a-code").await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location(&headers), "/");
    assert_eq!(common::session_count(&pool).await, 1);
    let cookie = common::session_cookie(&headers);
    let (status, _, body) = common::get(&app, "/auth/me", Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
    let signed_in: SignedIn = serde_json::from_str(&body).expect("an account");
    assert_eq!(signed_in.display_name, "Sam");
}

#[tokio::test]
async fn the_callback_clears_the_in_flight_cookie() {
    let app = common::provider_app(pool_with_sam().await, FakeProvider::reporting(&[]), false);

    let (_, headers, _) = sign_in(&app, "a-code").await;

    assert_in_flight_cleared(&headers);
}

#[tokio::test]
async fn an_unverified_address_is_refused_even_on_the_list() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam@gmail.com", false)]),
        false,
    );

    let (status, _, page) = sign_in(&app, "a-code").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(page.contains("sam@gmail.com"), "{page}");
    assert!(page.contains("not verified"), "{page}");
    assert_eq!(common::session_count(&pool).await, 0);
}

#[tokio::test]
async fn an_address_not_on_the_list_gets_the_ask_the_host_page() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("alex@work.com", true), ("alex@<b>home</b>.com", true)]),
        false,
    );

    let (status, _, page) = sign_in(&app, "a-code").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(page.contains("Fake"), "names the provider: {page}");
    assert!(page.contains("alex@work.com"), "{page}");
    assert!(
        page.contains("alex@&lt;b&gt;home&lt;/b&gt;.com"),
        "escaped: {page}"
    );
    assert!(page.contains("Ask the host"), "{page}");
    assert_eq!(common::session_count(&pool).await, 0);
}

/// A verified address that isn't an email address is listed as unusable, and
/// the page doesn't claim the provider verified nothing.
#[tokio::test]
async fn a_verified_address_that_cannot_be_an_email_is_listed_as_unusable() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam-at-gmail", true)]),
        false,
    );

    let (status, _, page) = sign_in(&app, "a-code").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(page.contains("sam-at-gmail"), "{page}");
    assert!(page.contains("aren't email addresses"), "{page}");
    assert!(!page.contains("no verified address"), "{page}");
    assert_eq!(common::session_count(&pool).await, 0);
}

#[tokio::test]
async fn an_address_on_a_disabled_entry_gets_the_ask_the_host_page() {
    let pool = pool_with_sam().await;
    let sam = EntryRef::Email(Email::parse("sam@gmail.com").expect("an email"));
    accounts::remove(&pool, &sam).await.expect("disable Sam");
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam@gmail.com", true)]),
        false,
    );

    let (status, _, page) = sign_in(&app, "a-code").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(page.contains("sam@gmail.com"), "{page}");
    assert!(page.contains("Ask the host"), "{page}");
    assert_eq!(common::session_count(&pool).await, 0);
}

#[tokio::test]
async fn a_callback_without_the_in_flight_cookie_gets_the_try_again_page() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam@gmail.com", true)]),
        false,
    );

    let (status, _, page) = common::get(
        &app,
        &format!("/auth/fake/callback?code=a-code&state={FAKE_STATE}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("Try again"), "{page}");
    assert_eq!(common::session_count(&pool).await, 0);
}

#[tokio::test]
async fn a_callback_whose_state_does_not_match_gets_the_try_again_page() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam@gmail.com", true)]),
        false,
    );
    let (_, cookie) = start(&app).await;

    for query in ["code=a-code&state=forged", "code=a-code"] {
        let (status, headers, page) =
            common::get(&app, &format!("/auth/fake/callback?{query}"), Some(&cookie)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert!(page.contains("Try again"), "{query}: {page}");
        assert_in_flight_cleared(&headers);
    }
    assert_eq!(common::session_count(&pool).await, 0);
}

#[tokio::test]
async fn a_callback_with_a_forged_cookie_gets_the_try_again_page() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam@gmail.com", true)]),
        false,
    );
    let forged = format!(
        r#"{}={{"provider":"fake","state":"{FAKE_STATE}","pkce_verifier":"x"}}"#,
        auth::SIGN_IN_COOKIE
    );

    let (status, _, _) = common::get(
        &app,
        &format!("/auth/fake/callback?code=a-code&state={FAKE_STATE}"),
        Some(&forged),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(common::session_count(&pool).await, 0);
}

#[tokio::test]
async fn a_sign_in_cancelled_at_the_provider_gets_the_try_again_page() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam@gmail.com", true)]),
        false,
    );
    let (state, cookie) = start(&app).await;

    let (status, headers, page) = common::get(
        &app,
        &format!("/auth/fake/callback?error=access_denied&state={state}"),
        Some(&cookie),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(page.contains("Try again"), "{page}");
    assert!(page.contains(r#"href="/""#), "links home: {page}");
    assert_in_flight_cleared(&headers);
    assert_eq!(common::session_count(&pool).await, 0);
}

#[tokio::test]
async fn an_exchange_that_fails_at_the_provider_gets_the_try_again_page() {
    let pool = pool_with_sam().await;
    let app = common::provider_app(
        pool.clone(),
        FakeProvider::reporting(&[("sam@gmail.com", true)]),
        false,
    );

    let (status, headers, page) = sign_in(&app, FAILING_CODE).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(page.contains("Try again"), "{page}");
    assert_in_flight_cleared(&headers);
    assert_eq!(common::session_count(&pool).await, 0);
}
