use poise::serenity_prelude::all::{GuildId, UserId};
use redis::{Client, aio::ConnectionManager};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{RwLock, mpsc};

use crate::{config::Config, keycloak::KeycloakClient};

#[derive(Clone, Serialize, Deserialize)]
pub struct PendingVerification {
    pub discord_user_id: UserId,
    pub discord_username: String,
    pub guild_id: GuildId,
    pub created_at: i64,
}

#[derive(Clone, Debug)]
pub struct VerificationComplete {
    pub discord_user_id: UserId,
    pub guild_id: GuildId,
    pub keycloak_user_id: String,
}

pub struct AppState {
    pub config: Config,
    pub keycloak: KeycloakClient,
    pub redis: ConnectionManager,
    pub pending_verifications: Arc<RwLock<HashMap<String, PendingVerification>>>,
    pub verification_tx: mpsc::UnboundedSender<VerificationComplete>,
}

impl AppState {
    pub async fn new(
        config: Config,
        verification_tx: mpsc::UnboundedSender<VerificationComplete>,
    ) -> anyhow::Result<Self> {
        let keycloak = KeycloakClient::new(
            &config.keycloak_url,
            &config.keycloak_realm,
            &config.keycloak_admin_client_id,
            &config.keycloak_admin_client_secret,
        )
        .await?;

        let redis_client = Client::open(config.redis_url.clone())?;
        let redis = ConnectionManager::new(redis_client).await?;

        Ok(Self {
            config,
            keycloak,
            redis,
            pending_verifications: Arc::new(RwLock::new(HashMap::new())),
            verification_tx,
        })
    }
}
