//! The GitHub [`IdentityProvider`]: an OAuth app asking for `user:email`,
//! whose code is exchanged for a token that reads `GET /user/emails` once.

use std::borrow::Cow;

use oauth2::basic::BasicClient;
use oauth2::reqwest::header::{ACCEPT, USER_AGENT};
use oauth2::reqwest::redirect::Policy;
use oauth2::reqwest::{Client as HttpClient, Response};
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse as _,
    TokenUrl,
};
use serde::Deserialize;
use url::Url;

use crate::auth::provider::{
    ExchangeError, ExchangeFuture, IdentityProvider, PendingSignIn, ReportedEmail,
};
use crate::config::ProviderCredentials;

/// Where GitHub serves its sign-in pages and token exchange.
const WEB: &str = "https://github.com/";
/// Where GitHub serves its REST API.
const API: &str = "https://api.github.com/";

/// An OAuth client with its authorization and token endpoints set.
type OAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// Sign-in through a GitHub OAuth app.
pub struct GitHub {
    client: OAuthClient,
    http: HttpClient,
    emails_url: Url,
}

impl GitHub {
    /// The adapter for the app `credentials` names, on github.com.
    ///
    /// # Panics
    ///
    /// The HTTP client's TLS backend failed to start.
    #[must_use]
    pub fn new(credentials: &ProviderCredentials) -> Self {
        let web = Url::parse(WEB).expect("GitHub's web URL parses");
        let api = Url::parse(API).expect("GitHub's API URL parses");
        Self::with_base_urls(credentials, &web, &api)
    }

    /// The adapter with GitHub's web pages under `web` and its REST API
    /// under `api`, each ending in `/`: a test points both at a fake.
    ///
    /// # Panics
    ///
    /// The HTTP client's TLS backend failed to start.
    #[must_use]
    pub fn with_base_urls(credentials: &ProviderCredentials, web: &Url, api: &Url) -> Self {
        let endpoint = |base: &Url, path: &str| base.join(path).expect("a relative path joins");
        let client = BasicClient::new(ClientId::new(credentials.client_id.clone()))
            .set_client_secret(ClientSecret::new(
                credentials.client_secret.expose().to_string(),
            ))
            .set_auth_uri(AuthUrl::from_url(endpoint(web, "login/oauth/authorize")))
            .set_token_uri(TokenUrl::from_url(endpoint(
                web,
                "login/oauth/access_token",
            )))
            .set_auth_type(AuthType::RequestBody);
        // No redirects, which both `oauth2`'s docs and GitHub's token
        // endpoint call for: a followed redirect is a way to aim the server's
        // requests elsewhere.
        let http = HttpClient::builder()
            .redirect(Policy::none())
            .build()
            .expect("the HTTP client builds");
        Self {
            client,
            http,
            emails_url: endpoint(api, "user/emails"),
        }
    }
}

/// One entry of GitHub's `GET /user/emails`.
#[derive(Deserialize)]
struct GitHubEmail {
    email: String,
    verified: bool,
}

impl IdentityProvider for GitHub {
    fn slug(&self) -> &'static str {
        "github"
    }

    fn name(&self) -> &'static str {
        "GitHub"
    }

    fn authorize_url(&self, redirect_uri: &Url) -> (Url, PendingSignIn) {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state) = self
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("user:email".to_string()))
            .set_pkce_challenge(challenge)
            .set_redirect_uri(Cow::Owned(RedirectUrl::from_url(redirect_uri.clone())))
            .url();
        (
            url,
            PendingSignIn::new(state.secret().clone(), verifier.secret().clone()),
        )
    }

    /// Every address on the account, each with its own `verified` flag, and
    /// not only the primary one. The token is used for this one request and
    /// dropped.
    fn exchange<'a>(
        &'a self,
        code: String,
        pending: PendingSignIn,
        redirect_uri: &'a Url,
    ) -> ExchangeFuture<'a> {
        Box::pin(async move {
            let token = self
                .client
                .exchange_code(AuthorizationCode::new(code))
                .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier().to_string()))
                .set_redirect_uri(Cow::Owned(RedirectUrl::from_url(redirect_uri.clone())))
                .request_async(&self.http)
                .await
                .map_err(ExchangeError::new)?;
            // GitHub's API refuses a request without a `User-Agent`.
            let body = self
                .http
                .get(self.emails_url.clone())
                .bearer_auth(token.access_token().secret())
                .header(USER_AGENT, "eldritch")
                .header(ACCEPT, "application/vnd.github+json")
                .send()
                .await
                .and_then(Response::error_for_status)
                .map_err(ExchangeError::new)?
                .bytes()
                .await
                .map_err(ExchangeError::new)?;
            let emails: Vec<GitHubEmail> =
                serde_json::from_slice(&body).map_err(ExchangeError::new)?;
            Ok(emails
                .into_iter()
                .map(|email| ReportedEmail {
                    address: email.email,
                    verified: email.verified,
                })
                .collect())
        })
    }
}
