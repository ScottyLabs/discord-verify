use leptos::{IntoView, component, prelude::ElementChild, view};

#[component]
pub fn SuccessPage() -> impl IntoView {
    view! {
        <article>
            <p>"Your Andrew ID has been successfully linked to Discord."</p>
            <p><small>"You can now close this window."</small></p>
        </article>
    }
}
