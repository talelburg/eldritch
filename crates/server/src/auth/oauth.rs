//! The `/auth/{provider}/login` and `/auth/{provider}/callback` routes of
//! each configured [`IdentityProvider`], and the allowlist match between
//! them.
//!
//! Login keeps the provider's [`PendingSignIn`] in an encrypted cookie,
//! [`SIGN_IN_COOKIE`], and sends the browser to the provider. The callback
//! clears that cookie whatever happens, checks the `state` the provider sent
//! back against it, exchanges the code for the reported emails, and signs in
//! as the enabled account holding any verified one (ADR 0023). Every refusal
//! is a page, and none writes a session.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{routing, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, PrivateCookieJar, SameSite};
use protocol::{AccountId, SignInMethod};
use serde::{Deserialize, Serialize};
use time::Duration;
use url::Url;

use crate::accounts::{self, Email};
use crate::auth::{
    self,
    provider::{IdentityProvider, PendingSignIn, ProviderError, ReportedEmail},
    SIGN_IN_COOKIE,
};
use crate::AppState;

/// How long a sign-in may sit at the provider before its cookie expires.
const SIGN_IN_MINUTES: i64 = 10;

/// A provider the server offers, with the callback it sends the browser
/// back to.
#[derive(Clone)]
pub(crate) struct ConfiguredProvider {
    provider: Arc<dyn IdentityProvider>,
    /// `{public origin}/auth/{slug}/callback`.
    redirect_uri: Url,
}

impl ConfiguredProvider {
    /// `provider`, calling back to the server at `public_origin`.
    pub(crate) fn new(public_origin: &Url, provider: Arc<dyn IdentityProvider>) -> Self {
        let redirect_uri = public_origin
            .join(&format!("auth/{}/callback", provider.slug()))
            .expect("a slug is a path segment");
        Self {
            provider,
            redirect_uri,
        }
    }

    /// The sign-in screen's link to this provider.
    pub(crate) fn sign_in_method(&self) -> SignInMethod {
        SignInMethod {
            label: format!("Sign in with {}", self.name()),
            url: format!("/auth/{}/login", self.slug()),
        }
    }

    /// The provider's path segment.
    fn slug(&self) -> &'static str {
        self.provider.slug()
    }

    /// The provider's name as a person reads it.
    fn name(&self) -> &'static str {
        self.provider.name()
    }

    /// The URL to send the browser to, asking the provider to call back
    /// here, and the state to keep until it does.
    async fn authorize(&self) -> Result<(Url, PendingSignIn), ProviderError> {
        self.provider.authorize_url(&self.redirect_uri).await
    }

    /// Exchange the `code` the provider called back with for the emails it
    /// reports, finishing the sign-in `pending` started.
    async fn exchange(
        &self,
        code: String,
        pending: PendingSignIn,
    ) -> Result<Vec<ReportedEmail>, ProviderError> {
        self.provider
            .exchange(code, pending, &self.redirect_uri)
            .await
    }
}

/// `router` with the login and callback routes of each of `providers`.
pub(crate) fn routes(
    router: Router<AppState>,
    providers: &[ConfiguredProvider],
) -> Router<AppState> {
    providers.iter().fold(router, |router, configured| {
        let slug = configured.slug();
        let for_login = configured.clone();
        let for_callback = configured.clone();
        router
            .route(
                &format!("/{slug}/login"),
                routing::get(move |jar: PrivateCookieJar| login(for_login.clone(), jar)),
            )
            .route(
                &format!("/{slug}/callback"),
                routing::get(
                    move |state: State<AppState>,
                          sessions: CookieJar,
                          in_flight: PrivateCookieJar,
                          query: Query<Callback>| {
                        callback(for_callback.clone(), state, sessions, in_flight, query)
                    },
                ),
            )
    })
}

/// What the in-flight cookie holds: which provider the sign-in went to, and
/// what that provider's callback needs.
#[derive(Serialize, Deserialize)]
struct InFlight {
    provider: String,
    #[serde(flatten)]
    pending: PendingSignIn,
}

/// `GET /auth/{provider}/login`: set the in-flight cookie and send the
/// browser to the provider, or answer the "try again" page when the provider
/// couldn't be asked where to send it.
async fn login(configured: ConfiguredProvider, jar: PrivateCookieJar) -> Response {
    let (url, pending) = match configured.authorize().await {
        Ok(authorize) => authorize,
        Err(error) => {
            tracing::warn!(%error, provider = configured.slug(), "starting a sign-in failed");
            return try_again(StatusCode::BAD_GATEWAY);
        }
    };
    let in_flight = InFlight {
        provider: configured.slug().to_string(),
        pending,
    };
    let value = serde_json::to_string(&in_flight).expect("an in-flight sign-in serializes");
    // `Lax`, so the cookie goes with the provider's redirect back, which is
    // a cross-site top-level GET.
    let cookie = Cookie::build((SIGN_IN_COOKIE, value))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/auth")
        .max_age(Duration::minutes(SIGN_IN_MINUTES))
        .build();
    (jar.add(cookie), Redirect::to(url.as_str())).into_response()
}

/// The query a provider calls back with: a `code` and the `state`, or an
/// `error` when the person didn't approve.
#[derive(Deserialize)]
struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// `GET /auth/{provider}/callback`: finish the sign-in, setting the session
/// cookie and going to `/`, or answer a refusal page.
async fn callback(
    configured: ConfiguredProvider,
    State(state): State<AppState>,
    sessions: CookieJar,
    in_flight_jar: PrivateCookieJar,
    Query(callback): Query<Callback>,
) -> Response {
    let in_flight = in_flight_jar
        .get(SIGN_IN_COOKIE)
        .and_then(|cookie| serde_json::from_str::<InFlight>(cookie.value()).ok());
    let in_flight_jar = in_flight_jar.remove(Cookie::build(SIGN_IN_COOKIE).path("/auth"));
    let account = match signed_in_account(&configured, &state, callback, in_flight).await {
        Ok(account) => account,
        Err(refusal) => return (in_flight_jar, refusal).into_response(),
    };
    match auth::create_session(&state.db, account).await {
        // Relative, as the dev picker's is, so a sign-in through `trunk
        // serve`'s proxy lands back on :3000.
        Ok(session_id) => (
            in_flight_jar,
            sessions.add(auth::session_cookie(session_id)),
            Redirect::to("/"),
        )
            .into_response(),
        Err(error) => (
            in_flight_jar,
            auth::internal_error("starting a session", &error),
        )
            .into_response(),
    }
}

/// The account `callback` signs in as, or the page refusing it.
async fn signed_in_account(
    configured: &ConfiguredProvider,
    state: &AppState,
    callback: Callback,
    in_flight: Option<InFlight>,
) -> Result<AccountId, Response> {
    if callback.error.is_some() {
        return Err(try_again(StatusCode::FORBIDDEN));
    }
    let pending = in_flight
        .filter(|in_flight| in_flight.provider == configured.slug())
        .map(|in_flight| in_flight.pending)
        .filter(|pending| callback.state.as_deref() == Some(pending.state()))
        .ok_or_else(|| try_again(StatusCode::BAD_REQUEST))?;
    let code = callback
        .code
        .ok_or_else(|| try_again(StatusCode::BAD_REQUEST))?;
    let reported = configured.exchange(code, pending).await.map_err(|error| {
        tracing::warn!(%error, provider = configured.slug(), "sign-in exchange failed");
        try_again(StatusCode::BAD_GATEWAY)
    })?;

    let (verified, unverified): (Vec<ReportedEmail>, Vec<ReportedEmail>) =
        reported.into_iter().partition(|email| email.verified);
    let mut checked = Vec::new();
    let mut unusable = Vec::new();
    for email in verified {
        match Email::parse(&email.address) {
            Ok(parsed) => checked.push(parsed),
            Err(_) => unusable.push(email),
        }
    }
    match accounts::enabled_by_email(&state.db, &checked).await {
        Ok(Some(account)) => Ok(account),
        Ok(None) => Err(ask_the_host(
            configured.name(),
            &checked,
            &unusable,
            &unverified,
        )),
        Err(error) => Err(auth::internal_error("matching a sign-in", &error).into_response()),
    }
}

/// The page for a sign-in that didn't complete: cancelled at the provider,
/// stale, forged, or failed at the provider.
fn try_again(status: StatusCode) -> Response {
    let page = auth::page(
        "Sign-in didn't complete",
        r#"<p>Sign-in didn't complete. <a href="/">Try again</a>.</p>"#,
    );
    (status, Html(page)).into_response()
}

/// The page for a sign-in none of whose verified addresses is on an enabled
/// allowlist entry: every address `provider` reported, and what to do. The
/// verified ones are split into those `checked` against the allowlist and
/// those `unusable` as an email address.
fn ask_the_host(
    provider: &str,
    checked: &[Email],
    unusable: &[ReportedEmail],
    unverified: &[ReportedEmail],
) -> Response {
    let provider = auth::escape(provider);
    let mut body = String::new();
    if !checked.is_empty() {
        let _ = write!(
            body,
            "<p>{provider} reported these verified addresses, and none of them can sign in here:</p>{}",
            list(checked.iter().map(Email::as_str))
        );
    } else if unusable.is_empty() {
        let _ = write!(body, "<p>{provider} reported no verified address.</p>");
    }
    if !unusable.is_empty() {
        let _ = write!(
            body,
            "<p>{provider} verified these, but they aren't email addresses, so they weren't checked:</p>{}",
            list(unusable.iter().map(|email| email.address.as_str()))
        );
    }
    if !unverified.is_empty() {
        let _ = write!(
            body,
            "<p>These are not verified by {provider}, so they weren't checked:</p>{}",
            list(unverified.iter().map(|email| email.address.as_str()))
        );
    }
    body.push_str("<p>Ask the host to add one of your addresses to the allowlist.</p>");
    (
        StatusCode::FORBIDDEN,
        Html(auth::page("Not on the allowlist", &body)),
    )
        .into_response()
}

/// `items` as an HTML list, escaped.
fn list<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let mut list = items.fold(String::from("<ul>"), |mut list, item| {
        let _ = write!(list, "<li>{}</li>", auth::escape(item));
        list
    });
    list.push_str("</ul>");
    list
}
