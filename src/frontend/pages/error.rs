use leptos::{
    IntoView, component,
    prelude::{ElementChild, Get},
    view,
};
use leptos_router::hooks::use_query_map;

#[component]
pub fn ErrorPage() -> impl IntoView {
    let query = use_query_map();
    let error_msg = move || {
        query
            .get()
            .get("msg")
            .and_then(|msg| match msg.as_str() {
                "expired" => Some((
                    "Verification Link Expired",
                    "This verification link has expired. Please run /verify in Discord again to get a new link."
                )),
                "wrong_account" => Some((
                    "Wrong Discord Account",
                    "The Discord account you linked doesn't match the one that requested verification. Please ensure you're logging in with the correct Discord account."
                )),
                "already_linked" => Some((
                    "Account Already Linked",
                    "Your Keycloak account is already linked to a different Discord account. Please unlink it first in your Keycloak account settings, or contact an administrator."
                )),
                "not_linked" => Some((
                    "Discord Account Not Linked",
                    "Your Discord account was not successfully linked. Please try the verification process again."
                )),
                "server_error" => Some((
                    "Server Error",
                    "An unexpected error occurred. Please try again later or contact an administrator if the problem persists."
                )),
                _ => Some((
                    "Unknown Error",
                    "An error occurred during verification. Please try again or contact an administrator."
                ))
            })
            .unwrap_or((
                "Error",
                "An error occurred during verification. Please try again."
            ))
    };

    view! {
        <article>
            <h1>{move || error_msg().0}</h1>
            <p>{move || error_msg().1}</p>
            <p><small>"You can close this window and return to Discord."</small></p>
        </article>
    }
}
