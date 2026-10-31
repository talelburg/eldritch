//! The Google [`IdentityProvider`]: an `OpenID` Connect client asking for
//! `openid email`, whose code is exchanged for an ID token carrying the
//! address.
//!
//! Google's endpoints and signing keys are discovered from
//! `https://accounts.google.com` on every login and every callback, never at
//! startup. The server then starts while Google is unreachable, and a key
//! Google rotates is picked up by the next sign-in.

use std::borrow::Cow;

use oauth2::reqwest::redirect::Policy;
use oauth2::reqwest::Client as HttpClient;
use oauth2::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
};
use openidconnect::core::{CoreClient, CoreIdTokenClaims, CoreProviderMetadata, CoreResponseType};
use openidconnect::{AuthenticationFlow, IssuerUrl, Nonce, TokenResponse as _};
use url::Url;

use crate::auth::provider::{
    AuthorizeFuture, ExchangeFuture, IdentityProvider, PendingSignIn, ProviderError, ReportedEmail,
};
use crate::config::ProviderCredentials;

/// Google's issuer: discovery reads `/.well-known/openid-configuration`
/// under it, and every ID token must name it.
const ISSUER: &str = "https://accounts.google.com";

/// An `OpenID` Connect client built from Google's discovery document, with
/// its authorization endpoint set and its token and user-info endpoints as
/// discovered.
type DiscoveredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// Sign-in through a Google OAuth client.
pub struct Google {
    client_id: ClientId,
    client_secret: ClientSecret,
    http: HttpClient,
}

impl Google {
    /// The adapter for the client `credentials` names. It makes no request:
    /// discovery waits for the first sign-in.
    ///
    /// # Panics
    ///
    /// The HTTP client's TLS backend failed to start.
    #[must_use]
    pub fn new(credentials: &ProviderCredentials) -> Self {
        // No redirects, which `openidconnect`'s docs call for: a followed
        // redirect is a way to aim the server's requests elsewhere.
        let http = HttpClient::builder()
            .redirect(Policy::none())
            .build()
            .expect("the HTTP client builds");
        Self {
            client_id: ClientId::new(credentials.client_id.clone()),
            client_secret: ClientSecret::new(credentials.client_secret.expose().to_string()),
            http,
        }
    }

    /// The client for one sign-in, from Google's discovery document and
    /// signing keys as they stand now, sending the browser back to
    /// `redirect_uri`.
    async fn discover(&self, redirect_uri: &Url) -> Result<DiscoveredClient, ProviderError> {
        let issuer = IssuerUrl::new(ISSUER.to_string()).expect("Google's issuer parses");
        let metadata = CoreProviderMetadata::discover_async(issuer, &self.http)
            .await
            .map_err(ProviderError::new)?;
        Ok(CoreClient::from_provider_metadata(
            metadata,
            self.client_id.clone(),
            Some(self.client_secret.clone()),
        )
        .set_redirect_uri(RedirectUrl::from_url(redirect_uri.clone())))
    }
}

impl IdentityProvider for Google {
    fn slug(&self) -> &'static str {
        "google"
    }

    fn name(&self) -> &'static str {
        "Google"
    }

    /// Discovers Google's authorization endpoint, and asks for `openid`
    /// (which the client always adds) and `email`, with PKCE and a nonce.
    fn authorize_url<'a>(&'a self, redirect_uri: &'a Url) -> AuthorizeFuture<'a> {
        Box::pin(async move {
            let client = self.discover(redirect_uri).await?;
            let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
            let (url, state, nonce) = client
                .authorize_url(
                    AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                    CsrfToken::new_random,
                    Nonce::new_random,
                )
                .add_scope(Scope::new("email".to_string()))
                .set_pkce_challenge(challenge)
                .url();
            let pending = PendingSignIn::new(state.secret().clone(), verifier.secret().clone())
                .with_nonce(nonce.secret().clone());
            Ok((url, pending))
        })
    }

    /// The address in the ID token, once its signature, issuer, audience,
    /// expiry and nonce are verified against the keys discovered now.
    fn exchange<'a>(
        &'a self,
        code: String,
        pending: PendingSignIn,
        redirect_uri: &'a Url,
    ) -> ExchangeFuture<'a> {
        Box::pin(async move {
            let nonce = pending
                .nonce()
                .ok_or_else(|| ProviderError::new("the sign-in carries no nonce"))?;
            let client = self.discover(redirect_uri).await?;
            let token = client
                .exchange_code(AuthorizationCode::new(code))
                .map_err(ProviderError::new)?
                .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier().to_string()))
                .set_redirect_uri(Cow::Owned(RedirectUrl::from_url(redirect_uri.clone())))
                .request_async(&self.http)
                .await
                .map_err(ProviderError::new)?;
            let id_token = token
                .id_token()
                .ok_or_else(|| ProviderError::new("Google sent no ID token"))?;
            let claims = id_token
                .claims(&client.id_token_verifier(), &Nonce::new(nonce.to_string()))
                .map_err(ProviderError::new)?;
            Ok(reported_emails(claims))
        })
    }
}

/// The address a verified ID token's `claims` carry. Google verified it only
/// when `email_verified` is `true`: a token without the claim reports it
/// unverified.
fn reported_emails(claims: &CoreIdTokenClaims) -> Vec<ReportedEmail> {
    claims
        .email()
        .map(|email| ReportedEmail {
            address: email.as_str().to_string(),
            verified: claims.email_verified() == Some(true),
        })
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    /// ID-token claims as Google sends them for `sam@gmail.com`, with
    /// `email_verified` set to `verified`, or left out when it is `None`.
    fn claims(verified: Option<bool>) -> CoreIdTokenClaims {
        let mut claims = json!({
            "iss": "https://accounts.google.com",
            "aud": "1234-abc.apps.googleusercontent.com",
            "exp": 1_900_000_000,
            "iat": 1_899_999_000,
            "sub": "110169484474386276334",
            "email": "sam@gmail.com",
        });
        if let Some(verified) = verified {
            claims["email_verified"] = Value::Bool(verified);
        }
        serde_json::from_value(claims).expect("the claims deserialize")
    }

    #[test]
    fn a_verified_email_is_reported_verified() {
        assert_eq!(
            reported_emails(&claims(Some(true))),
            vec![ReportedEmail {
                address: "sam@gmail.com".to_string(),
                verified: true,
            }]
        );
    }

    #[test]
    fn an_email_google_did_not_verify_is_reported_unverified() {
        assert_eq!(
            reported_emails(&claims(Some(false))),
            vec![ReportedEmail {
                address: "sam@gmail.com".to_string(),
                verified: false,
            }]
        );
    }

    #[test]
    fn an_email_without_email_verified_is_reported_unverified() {
        assert_eq!(
            reported_emails(&claims(None)),
            vec![ReportedEmail {
                address: "sam@gmail.com".to_string(),
                verified: false,
            }]
        );
    }
}
