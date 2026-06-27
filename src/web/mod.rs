mod api;
mod auth;

use crate::frontend::app;
use crate::state::AppState;
use axum::{
    Router,
    error_handling::HandleErrorLayer,
    extract::FromRequestParts,
    http::{Uri, request::Parts},
    response::IntoResponse,
    routing::get,
};
use axum_oidc::{
    AdditionalClaims, EmptyAdditionalClaims, OidcAuthLayer, OidcClient, OidcLoginLayer,
    OidcSession,
    error::MiddlewareError,
    handle_oidc_redirect,
    openidconnect::{ClientId, ClientSecret, CsrfToken, IssuerUrl, Scope, core::CoreGenderClaim},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use leptos::{config::get_configuration, prelude::provide_context};
use leptos_axum::{LeptosRoutes, generate_route_list};
use serde::Serialize;
use std::sync::Arc;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tower_sessions::{
    Expiry, MemoryStore, Session, SessionManagerLayer,
    cookie::{SameSite, time::Duration},
};

struct SessionWrapper(Session);

impl<S: Send + Sync> FromRequestParts<S> for SessionWrapper {
    type Rejection = <Session as FromRequestParts<S>>::Rejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(Session::from_request_parts(parts, state).await?))
    }
}

impl<AC: AdditionalClaims> axum_oidc::Session<AC> for SessionWrapper {
    type Error = tower_sessions::session::Error;

    async fn get(&self) -> Result<OidcSession<AC, CoreGenderClaim>, Self::Error> {
        Ok(self.0.get("axum-oidc").await?.unwrap_or_default())
    }

    async fn set(&mut self, value: OidcSession<AC, CoreGenderClaim>) -> Result<(), Self::Error> {
        self.0.insert("axum-oidc", value).await?;
        Ok(())
    }
}

/// OAuth2 state payload carrying the return_to and a CSRF token
#[derive(Serialize)]
struct RelayState<'a> {
    return_to: &'a str,
    csrf: uuid::Uuid,
}

/// Build the base64url state with a random csrf to guard against login CSRF
fn relay_state(return_to: &str) -> String {
    let state = RelayState {
        return_to,
        csrf: uuid::Uuid::new_v4(),
    };
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&state).expect("serialize relay state"))
}

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
        .layer(OidcLoginLayer::<EmptyAdditionalClaims, SessionWrapper>::new());

    // Initialize OIDC client
    let scopes = vec![
        Scope::new("openid".into()),
        Scope::new("email".into()),
        Scope::new("profile".into()),
    ];

    let issuer_url = IssuerUrl::new(format!(
        "{}/realms/{}",
        state.config.keycloak_url, state.config.keycloak_realm
    ))
    .expect("valid IssuerUrl");

    // State carries /auth/callback
    let auth_return_to = format!("{}/auth/callback", state.config.app_url);
    let oidc_client = OidcClient::<EmptyAdditionalClaims>::builder()
        .with_default_http_client()
        .with_redirect_url(
            Uri::try_from(state.config.oauth_relay_url.clone()).expect("valid OAUTH_RELAY_URL"),
        )
        .with_client_id(ClientId::new(state.config.keycloak_oidc_client_id.clone()))
        .with_client_secret(ClientSecret::new(
            state.config.keycloak_oidc_client_secret.clone(),
        ))
        .with_scopes(scopes)
        .with_state_generator(move || CsrfToken::new(relay_state(&auth_return_to)))
        .discover(issuer_url)
        .await
        .map_err(|e| anyhow::anyhow!("oidc discovery failed: {e}"))?
        .build();

    tracing::info!("OIDC discovery completed successfully");

    let oidc_auth_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async move {
            tracing::error!("OIDC Auth Layer error: {:?}", e);
            tracing::error!("Error details: {}", e);
            e.into_response()
        }))
        .layer(OidcAuthLayer::<EmptyAdditionalClaims, SessionWrapper>::new(
            oidc_client,
        ));

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
        .route("/api/health", get(api::health))
        .route("/api/verify-status/{state}", get(api::verify_status))
        .route(
            "/auth/callback",
            get(handle_oidc_redirect::<EmptyAdditionalClaims, SessionWrapper>),
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

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}
