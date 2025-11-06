use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub discord_token: String,
    pub keycloak_url: String,
    pub keycloak_realm: String,
    pub keycloak_oidc_client_id: String,
    pub keycloak_oidc_client_secret: String,
    pub keycloak_admin_client_id: String,
    pub keycloak_admin_client_secret: String,
    pub app_url: String,
    pub redis_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            discord_token: dotenvy::var("DISCORD_TOKEN").context("DISCORD_TOKEN must be set")?,
            keycloak_url: dotenvy::var("KEYCLOAK_URL").context("KEYCLOAK_URL must be set")?,
            keycloak_realm: dotenvy::var("KEYCLOAK_REALM").context("KEYCLOAK_REALM must be set")?,
            keycloak_oidc_client_id: dotenvy::var("KEYCLOAK_OIDC_CLIENT_ID")
                .context("KEYCLOAK_OIDC_CLIENT_ID must be set")?,
            keycloak_oidc_client_secret: dotenvy::var("KEYCLOAK_OIDC_CLIENT_SECRET")
                .context("KEYCLOAK_OIDC_CLIENT_SECRET must be set")?,
            keycloak_admin_client_id: dotenvy::var("KEYCLOAK_ADMIN_CLIENT_ID")
                .context("KEYCLOAK_ADMIN_CLIENT_ID must be set")?,
            keycloak_admin_client_secret: dotenvy::var("KEYCLOAK_ADMIN_CLIENT_SECRET")
                .context("KEYCLOAK_ADMIN_CLIENT_SECRET must be set")?,
            app_url: dotenvy::var("APP_URL").context("APP_URL must be set")?,
            redis_url: dotenvy::var("REDIS_URL").context("REDIS_URL must be set")?,
        })
    }
}
