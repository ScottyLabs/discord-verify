use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub mod bot;
pub mod config;
pub mod error;
pub mod frontend;
pub mod keycloak;
pub mod state;
pub mod web;

use config::Config;
use state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration from environment
    let config = Config::from_env()?;

    tracing_subscriber::registry()
        .with(EnvFilter::new(std::env::var("RUST_LOG").unwrap_or_else(
            |_| "axum_oidc=debug,tower_sessions=debug,tower_http=debug,discord_verify=debug".into(),
        )))
        .with(tracing_subscriber::fmt::layer())
        .try_init()?;

    tracing::info!("Configuration loaded successfully");

    // Create channel for verification completion events
    let (verification_tx, verification_rx) = mpsc::unbounded_channel();

    // Initialize shared state
    let app_state = Arc::new(AppState::new(config, verification_tx).await?);
    tracing::info!("App state created successfully");

    // Spawn Discord bot in background
    let bot_state = app_state.clone();
    tokio::spawn(async move {
        tracing::info!("Starting Discord bot...");

        if let Err(e) = bot::run(bot_state, verification_rx).await {
            tracing::error!("Discord bot error: {}", e);
        }
    });

    // Start web server
    tracing::info!("Starting web server...");
    web::serve(app_state).await?;

    Ok(())
}
