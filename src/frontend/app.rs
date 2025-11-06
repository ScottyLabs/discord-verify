use crate::frontend::pages::{error::ErrorPage, success::SuccessPage};
use leptos::{IntoView, component, prelude::ElementChild, view};
use leptos_router::{
    StaticSegment,
    components::{Route, Router, Routes},
};

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <main>
                <Routes fallback=|| "Page not found".into_view()>
                    <Route path=StaticSegment("/success") view=SuccessPage/>
                    <Route path=StaticSegment("/error") view=ErrorPage/>
                </Routes>
            </main>
        </Router>
    }
}
