//! The sign-in screen, shown while the client isn't signed in.
//!
//! It renders at [`ConnStatus::SignedOut`], from the methods the server's
//! `GET /auth/me` listed: each is a link the browser follows to sign in. With
//! none listed it says so, rather than offering a link that leads nowhere.
//! Each link names the page the screen showed over, so a sign-in from a
//! game's link lands back on that game ([`url::sign_in_link`]).

use leptos::prelude::*;

use crate::store::{self, ConnStatus};
use crate::url;

/// The sign-in overlay. Renders only at [`ConnStatus::SignedOut`].
#[component]
pub fn SignInView() -> impl IntoView {
    let store = store::use_store();

    view! {
        {move || {
            let ConnStatus::SignedOut(methods) = store.get().status else {
                return ().into_any();
            };
            let body = if methods.is_empty() {
                view! { <p class="no-sign-in">"No sign-in is available."</p> }.into_any()
            } else {
                methods
                    .into_iter()
                    .map(|method| {
                        let href = current_link(&method.url);
                        view! { <a class="sign-in-method" href=href>{method.label}</a> }
                    })
                    .collect_view()
                    .into_any()
            };
            view! {
                <div class="sign-in-backdrop"></div>
                <section class="sign-in">
                    <p class="sign-in-title">"Sign in"</p>
                    {body}
                </section>
            }
            .into_any()
        }}
    }
}

/// The link to the method at `method_url`, returning to this page.
#[cfg(target_arch = "wasm32")]
fn current_link(method_url: &str) -> String {
    url::current_sign_in_link(method_url)
}

/// Off the browser there is no page to return to, so the link is the
/// method's own.
#[cfg(not(target_arch = "wasm32"))]
fn current_link(method_url: &str) -> String {
    url::sign_in_link(method_url, "/")
}
