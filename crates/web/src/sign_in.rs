//! The sign-in screen, shown while the client isn't signed in.
//!
//! It renders at [`ConnStatus::SignedOut`], from the methods the server's
//! `GET /auth/me` listed: each is a link the browser follows to sign in. With
//! none listed it says so, rather than offering a link that leads nowhere.
//! The saved game id survives, so the page a sign-in lands back on resumes
//! the same game.

use leptos::prelude::*;

use crate::store::{self, ConnStatus};

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
                        view! {
                            <a class="sign-in-method" href=method.url>{method.label}</a>
                        }
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
