use redis::{AsyncCommands, Client, aio::ConnectionManager};
use serde::{Deserialize, Serialize};
use serenity::all::{GuildId, Http, RoleId, UserId};
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

#[derive(Clone, Debug)]
pub struct SetupRolesSession {
    pub mode: String,
    pub parent_role: Option<RoleId>,
    pub custom_roles: Vec<String>,
}

impl SetupRolesSession {
    pub fn new(mode: String) -> Self {
        Self {
            mode,
            parent_role: None,
            custom_roles: Vec::new(),
        }
    }

    /// Update the parent role selection
    pub fn set_parent_role(&mut self, role_id: RoleId) {
        self.parent_role = Some(role_id);
    }

    /// Update the custom roles selection
    pub fn set_custom_roles(&mut self, roles: Vec<String>) {
        self.custom_roles = roles;
    }

    /// Validate that the session has all required data to proceed
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.parent_role.is_none() {
            return Err("Please select a parent role before saving.");
        }

        if self.mode == "custom" && self.custom_roles.is_empty() {
            return Err("Please select at least one role to create for custom mode.");
        }

        Ok(())
    }

    /// Determine which roles to create based on the mode and selections
    pub fn get_roles_to_create(&self) -> Vec<(String, String)> {
        match self.mode.as_str() {
            "levels" => vec![
                ("Undergrad".to_string(), "undergrad".to_string()),
                ("Graduate".to_string(), "graduate".to_string()),
            ],
            "classes" => vec![
                ("First-Year".to_string(), "first-year".to_string()),
                ("Sophomore".to_string(), "sophomore".to_string()),
                ("Junior".to_string(), "junior".to_string()),
                ("Senior".to_string(), "senior".to_string()),
                ("Fifth-Year Senior".to_string(), "fifth-year".to_string()),
                ("Masters".to_string(), "masters".to_string()),
                ("Doctoral".to_string(), "doctoral".to_string()),
            ],
            "custom" => {
                // Map the selections to (display_name, redis_key)
                self.custom_roles
                    .iter()
                    .filter_map(|s| {
                        let parts: Vec<&str> = s.split(':').collect();
                        if parts.len() == 2 {
                            let display_name = match parts[1] {
                                "undergrad" => "Undergrad".to_string(),
                                "graduate" => "Graduate".to_string(),
                                "first-year" => "First-Year".to_string(),
                                "sophomore" => "Sophomore".to_string(),
                                "junior" => "Junior".to_string(),
                                "senior" => "Senior".to_string(),
                                "fifth-year" => "Fifth-Year Senior".to_string(),
                                "masters" => "Masters".to_string(),
                                "doctoral" => "Doctoral".to_string(),
                                _ => return None,
                            };
                            Some((display_name, s.clone()))
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            _ => vec![],
        }
    }

    /// Create the roles in Discord and save configuration to Redis
    pub async fn save_and_create_roles(
        &self,
        http: &Http,
        guild_id: GuildId,
        redis: &mut ConnectionManager,
    ) -> Result<Vec<(String, RoleId)>, Box<dyn std::error::Error + Send + Sync>> {
        let parent_role_id = self.parent_role.ok_or("Parent role not set")?;

        // Get guild and parent role info
        let guild = guild_id.to_partial_guild(http).await?;
        let parent_role = guild
            .roles
            .get(&parent_role_id)
            .ok_or("Selected parent role not found")?;

        // Get the roles to create
        let roles_to_create = self.get_roles_to_create();
        let parent_position = parent_role.position;
        let mut created_roles = Vec::new();

        // Create each role
        for (role_name, role_key) in &roles_to_create {
            let new_role = guild_id
                .create_role(
                    http,
                    serenity::all::EditRole::new()
                        .name(role_name.as_str())
                        .position((parent_position - 1).max(0) as i16),
                )
                .await?;

            created_roles.push((role_key.clone(), new_role.id));

            // Store role ID in Redis
            let redis_key = format!("guild:{}:role:{}", guild_id, role_key);
            let _: () = redis.set(&redis_key, new_role.id.get()).await?;
        }

        // Save mode in Redis
        let role_mode_key = format!("guild:{}:role_mode", guild_id);
        let _: () = redis.set(&role_mode_key, self.mode.as_str()).await?;

        Ok(created_roles)
    }
}

pub struct AppState {
    pub config: Config,
    pub keycloak: KeycloakClient,
    pub redis: ConnectionManager,
    pub verification_tx: mpsc::UnboundedSender<VerificationComplete>,
    pub pending_verifications: Arc<RwLock<HashMap<String, PendingVerification>>>,
    pub setuproles_sessions: Arc<RwLock<HashMap<(GuildId, UserId), SetupRolesSession>>>,
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
            verification_tx,
            pending_verifications: Arc::new(RwLock::new(HashMap::new())),
            setuproles_sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}
