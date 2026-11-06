//! [`ReturnTo`]: the page a sign-in goes back to once it succeeds.
//!
//! The sign-in screen names the page it showed over as a `return_to` query
//! parameter on each method's link. Anything that isn't a path on this site
//! becomes `/`, so the parameter can't send a signed-in browser elsewhere.

use axum::response::Redirect;
use protocol::RETURN_TO_PARAM;
use serde::{Deserialize, Serialize};
use url::Url;

/// The base a raw value is resolved against. Its host is never sent
/// anywhere: it only tells a same-site path from one that leaves the site.
const BASE: &str = "http://x/";

/// A path on this site to go to after a sign-in: a path and query, always
/// relative, so a sign-in through `trunk serve`'s proxy on :3000 lands back
/// on :3000.
///
/// Decoding re-runs the check, so a value read back from a cookie is no
/// more trusted than one read from a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", from = "String")]
pub(crate) struct ReturnTo(String);

impl ReturnTo {
    /// The page `raw` names, or `/` when it names none or names somewhere
    /// off this site.
    ///
    /// `raw` is resolved by the URL parser, which makes the browser's own
    /// normalization decisions: a backslash reads as a slash, tabs and
    /// newlines are stripped, and `//host` names a host. It is kept only
    /// when it starts with `/`, the resolved URL stays on the base's scheme,
    /// host and port, and the path kept reads back as that same URL; a
    /// fragment is dropped.
    pub(crate) fn new(raw: Option<&str>) -> Self {
        raw.and_then(same_site_path)
            .map_or_else(Self::default, Self)
    }

    /// The page a raw request query (`RawQuery`, no `?`) names in its first
    /// [`RETURN_TO_PARAM`].
    pub(crate) fn from_query(query: Option<&str>) -> Self {
        let raw = query.and_then(|query| {
            form_urlencoded::parse(query.as_bytes())
                .find(|(name, _)| name == RETURN_TO_PARAM)
                .map(|(_, value)| value.into_owned())
        });
        Self::new(raw.as_deref())
    }

    /// The path, as a link or a form field carries it.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The redirect to this page.
    pub(crate) fn redirect(&self) -> Redirect {
        Redirect::to(&self.0)
    }
}

impl Default for ReturnTo {
    /// The home page, "Your games".
    fn default() -> Self {
        Self("/".to_string())
    }
}

impl From<String> for ReturnTo {
    fn from(raw: String) -> Self {
        Self::new(Some(&raw))
    }
}

impl From<ReturnTo> for String {
    fn from(return_to: ReturnTo) -> Self {
        return_to.0
    }
}

/// `raw`'s path and query, when it is a path on the base's own site and
/// reads back as the same page.
///
/// The read-back catches a dot segment that normalizes away to leave a path
/// starting `//`, as `/.//evil.com/x` does: on the base's own site as
/// resolved, but a browser reads that path back as the host `evil.com`.
fn same_site_path(raw: &str) -> Option<String> {
    if !raw.starts_with('/') {
        return None;
    }
    let base = Url::parse(BASE).expect("the base is a URL");
    let mut resolved = base.join(raw).ok()?;
    resolved.set_fragment(None);
    let path = match resolved.query() {
        Some(query) => format!("{}?{query}", resolved.path()),
        None => resolved.path().to_string(),
    };
    let same_site = resolved.scheme() == base.scheme()
        && resolved.host() == base.host()
        && resolved.port() == base.port();
    let reads_back = base.join(&path).ok().as_ref() == Some(&resolved);
    (same_site && reads_back).then_some(path)
}
