//! The seam every identity provider sits behind: [`IdentityProvider`].
//!
//! An adapter sends the browser to the provider, and turns the code the
//! provider sends back into the email addresses it reports, each flagged
//! verified or not. Which account those addresses sign in as is decided
//! above the seam, in `auth::oauth`, so no adapter matches against the
//! allowlist.

use std::error::Error;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use url::Url;

/// A provider a friend signs in through, such as GitHub.
///
/// `main` builds the real adapters from the configuration, and tests pass
/// fakes, through [`AppState::with_identity_providers`](crate::AppState::with_identity_providers).
pub trait IdentityProvider: Send + Sync {
    /// The provider's path segment: its routes are `/auth/{slug}/login` and
    /// `/auth/{slug}/callback`.
    fn slug(&self) -> &'static str;

    /// The provider's name as a person reads it, such as `GitHub`.
    fn name(&self) -> &'static str;

    /// The URL to send the browser to, asking the provider to send it back to
    /// `redirect_uri`, and the state to keep until it does.
    fn authorize_url(&self, redirect_uri: &Url) -> (Url, PendingSignIn);

    /// Exchange the `code` the provider sent back for every email address it
    /// reports. `pending` is what [`authorize_url`](Self::authorize_url)
    /// returned for this sign-in, and `redirect_uri` is the one it was given.
    fn exchange<'a>(
        &'a self,
        code: String,
        pending: PendingSignIn,
        redirect_uri: &'a Url,
    ) -> ExchangeFuture<'a>;
}

/// What [`IdentityProvider::exchange`] returns.
pub type ExchangeFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<ReportedEmail>, ExchangeError>> + Send + 'a>>;

/// A sign-in sent to the provider and not yet back: what the callback needs
/// to check and finish it. It is kept in an encrypted cookie, never in the
/// database.
#[derive(Clone, Serialize, Deserialize)]
pub struct PendingSignIn {
    state: String,
    pkce_verifier: String,
}

impl PendingSignIn {
    /// A sign-in whose callback must carry `state`, and whose code is
    /// exchanged with the PKCE `pkce_verifier`.
    #[must_use]
    pub fn new(state: String, pkce_verifier: String) -> Self {
        Self {
            state,
            pkce_verifier,
        }
    }

    /// The `state` the provider was sent, which the callback must carry back.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// The PKCE verifier whose challenge the provider was sent.
    #[must_use]
    pub fn pkce_verifier(&self) -> &str {
        &self.pkce_verifier
    }
}

/// An email address a provider reports for the person signing in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportedEmail {
    /// The address, as the provider spells it.
    pub address: String,
    /// Whether the provider verified the person holds it. Only a verified
    /// address is looked up on the allowlist.
    pub verified: bool,
}

/// The exchange or the email fetch failed at the provider.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ExchangeError(Box<dyn Error + Send + Sync>);

impl ExchangeError {
    /// The exchange failed with `error`.
    pub fn new(error: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self(error.into())
    }
}
