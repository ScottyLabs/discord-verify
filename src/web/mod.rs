mod api;
mod auth;

use crate::frontend::app;
use crate::state::AppState;
use axum::{
    Router, error_handling::HandleErrorLayer, http::Uri, response::IntoResponse, routing::get,
};
use axum_oidc::{
    EmptyAdditionalClaims, OidcAuthLayer, OidcClient, OidcLoginLayer, error::MiddlewareError,
    handle_oidc_redirect,
};
use leptos::{config::get_configuration, prelude::provide_context};
use leptos_axum::{LeptosRoutes, generate_route_list};
use std::sync::Arc;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tower_sessions::{
    Expiry, MemoryStore, SessionManagerLayer,
    cookie::{SameSite, time::Duration},
};

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

pub async fn serve(state: Arc<AppState>) -> anyhow::Result<()> {
    // Set up session management
    let session_store = MemoryStore::default();
    let session_service = ServiceBuilder::new().layer(
        SessionManagerLayer::new(session_store)
            .with_secure(false)
            .with_same_site(SameSite::Lax)
            .with_expiry(Expiry::OnInactivity(Duration::minutes(10))),
    );

    let oidc_login_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async move {
            tracing::error!("OIDC Login Layer error: {:?}", e);
            tracing::error!("Error details: {}", e);
            e.into_response()
        }))
        .layer(OidcLoginLayer::<EmptyAdditionalClaims>::new());

    // Initialize OIDC client
    let oidc_client = OidcClient::<EmptyAdditionalClaims>::builder()
        .with_default_http_client()
        .with_redirect_url(
            Uri::try_from(format!("{}/auth/callback", state.config.app_url))
                .expect("valid APP_URL"),
        )
        .with_client_id(state.config.keycloak_oidc_client_id.clone())
        .with_client_secret(state.config.keycloak_oidc_client_secret.clone())
        .with_scopes(["openid", "email", "profile"].into_iter())
        .discover(format!(
            "{}/realms/{}",
            state.config.keycloak_url, state.config.keycloak_realm
        ))
        .await?
        .build();

    let oidc_auth_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async move {
            tracing::error!("OIDC Auth Layer error: {:?}", e);
            tracing::error!("Error details: {}", e);
            e.into_response()
        }))
        .layer(OidcAuthLayer::new(oidc_client));

    // Leptos configuration
    let conf = get_configuration(Some("Cargo.toml")).unwrap();
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(app::App);

    // Build router
    let app = Router::new()
        // Protected routes
        .route("/verify", get(auth::verify_start))
        .route("/link-callback", get(auth::link_callback))
        .layer(oidc_login_service)
        // Public routes
        .route("/health", get(api::health))
        .route("/api/verify-status/{state}", get(api::verify_status))
        .route(
            "/auth/callback",
            get(handle_oidc_redirect::<EmptyAdditionalClaims>),
        )
        .layer(oidc_auth_service)
        .layer(session_service)
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone())
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            move || provide_context(state.clone()),
            app::App,
        )
        .fallback_service(ServeDir::new("target/site"))
        .with_state(leptos_options);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("Listening on http://0.0.0.0:3000");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}
