use leptos::{
    IntoView, component,
    prelude::{ElementChild, Get},
    view,
};
use leptos_router::hooks::use_query_map;

#[component]
pub fn ErrorPage() -> impl IntoView {
    let query = use_query_map();

    let error_content = move || {
        let msg = query.get().get("msg").unwrap_or_default();

        match msg.as_str() {
            "expired" => (
                "Verification Link Expired",
                view! {
                    <div>
                        <p>
                            "This verification link has expired. Please run "
                            <code>"/verify"</code>
                            " in Discord again to get a new link."
                        </p>
                    </div>
                }.into_view()
            ),
            "wrong_account" => (
                "Wrong Discord Account",
                view! {
                    <div>
                        <p>
                            "The Discord account you linked doesn't match the one that requested verification. "
                            "If you need to switch accounts, please unlink the current Discord account in your "
                            <a href="https://idp.scottylabs.org/realms/scottylabs/account" target="_blank" rel="noopener noreferrer">
                                "account settings"
                            </a>
                            " and try again."
                        </p>
                    </div>
                }.into_view()
            ),
            "already_linked" => (
                "Account Already Linked",
                view! {
                    <div>
                        <p>
                            "Your account is already linked to a different Discord account. "
                            "Please unlink it first in your "
                            <a href="https://idp.scottylabs.org/realms/scottylabs/account" target="_blank" rel="noopener noreferrer">
                                "account settings"
                            </a>
                            "."
                        </p>
                    </div>
                }.into_view()
            ),
            "not_linked" => (
                "Discord Account Not Linked",
                view! {
                    <div>
                        <p>
                            "Your Discord account was not successfully linked. "
                            "Please try the verification process again."
                        </p>
                    </div>
                }.into_view()
            ),
            "server_error" => (
                "Server Error",
                view! {
                    <div>
                        <p>
                            "An unexpected error occurred. "
                            "Please try again later or contact an administrator if the problem persists."
                        </p>
                    </div>
                }.into_view()
            ),
            _ => (
                "Unknown Error",
                view! {
                    <div>
                        <p>
                            "An error occurred during verification. "
                            "Please try again or contact an administrator."
                        </p>
                    </div>
                }.into_view()
            )
        }
    };

    view! {
        <article>
            <h1>{move || error_content().0}</h1>
            {move || error_content().1}
            <p><small>"You can close this window and return to Discord."</small></p>
        </article>
    }
}
