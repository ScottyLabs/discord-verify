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

        // Get the current mode to determine what roles exist
        let current_mode_key = format!("guild:{}:role_mode", guild_id);
        let current_mode: Option<String> = redis.get(&current_mode_key).await?;
        let current_mode = current_mode.unwrap_or_else(|| "none".to_string());

        // Get roles in the old mode and new mode
        let current_roles = self
            .get_current_roles(&current_mode, redis, guild_id)
            .await?;
        let desired_roles = self.get_roles_to_create();

        let current_role_keys: std::collections::HashSet<_> =
            current_roles.iter().map(|(key, _)| key.as_str()).collect();
        let desired_role_keys: std::collections::HashSet<_> =
            desired_roles.iter().map(|(_, key)| key.as_str()).collect();

        // Roles that exist in both current and desired
        let roles_to_keep: Vec<_> = current_roles
            .iter()
            .filter(|(key, _)| desired_role_keys.contains(key.as_str()))
            .cloned()
            .collect();

        // Roles that exist in current but not desired
        let roles_to_delete: Vec<_> = current_roles
            .iter()
            .filter(|(key, _)| !desired_role_keys.contains(key.as_str()))
            .cloned()
            .collect();

        // Roles that exist in desired but not current
        let roles_to_create: Vec<_> = desired_roles
            .iter()
            .filter(|(_, key)| !current_role_keys.contains(key.as_str()))
            .cloned()
            .collect();

        // Delete old roles and their Redis keys
        for (role_key, role_id) in &roles_to_delete {
            if let Err(e) = guild_id.delete_role(http, *role_id, None).await {
                tracing::warn!("Failed to delete role {}: {}", role_id, e);
            }

            let redis_key = format!("guild:{}:role:{}", guild_id, role_key);
            let _: () = redis.del(&redis_key).await?;
        }

        let parent_position = parent_role.position;
        let mut all_roles = Vec::new();

        // Create new roles
        for (role_name, role_key) in &roles_to_create {
            let new_role = guild_id
                .create_role(
                    http,
                    serenity::all::EditRole::new()
                        .name(role_name.as_str())
                        .position((parent_position - 1).max(0) as i16),
                )
                .await?;

            all_roles.push((role_key.clone(), new_role.id));

            // Store role ID in Redis
            let redis_key = format!("guild:{}:role:{}", guild_id, role_key);
            let _: () = redis.set(&redis_key, new_role.id.get()).await?;
        }

        // Update positions for kept roles (move them under parent role)
        for (role_key, role_id) in &roles_to_keep {
            // Update the role's position
            if let Err(e) = guild_id
                .edit_role(
                    http,
                    *role_id,
                    serenity::all::EditRole::new().position((parent_position - 1).max(0) as i16),
                )
                .await
            {
                eprintln!(
                    "Warning: Failed to update position for role {}: {}",
                    role_key, e
                );
            }

            all_roles.push((role_key.clone(), *role_id));
        }

        // Save mode in Redis
        let role_mode_key = format!("guild:{}:role_mode", guild_id);
        let _: () = redis.set(&role_mode_key, self.mode.as_str()).await?;

        Ok(all_roles)
    }

    /// Get currently configured roles based on the old mode
    async fn get_current_roles(
        &self,
        current_mode: &str,
        redis: &mut ConnectionManager,
        guild_id: GuildId,
    ) -> Result<Vec<(String, RoleId)>, Box<dyn std::error::Error + Send + Sync>> {
        let mut current_roles = Vec::new();

        match current_mode {
            "levels" => {
                for level in &["undergrad", "graduate"] {
                    let key = format!("guild:{}:role:{}", guild_id, level);
                    if let Ok(Some(role_id_str)) = redis.get::<_, Option<String>>(&key).await {
                        if let Ok(role_id_u64) = role_id_str.parse::<u64>() {
                            current_roles.push((level.to_string(), RoleId::new(role_id_u64)));
                        }
                    }
                }
            }
            "classes" => {
                for class in &[
                    "first-year",
                    "sophomore",
                    "junior",
                    "senior",
                    "fifth-year",
                    "masters",
                    "doctoral",
                ] {
                    let key = format!("guild:{}:role:{}", guild_id, class);
                    if let Ok(Some(role_id_str)) = redis.get::<_, Option<String>>(&key).await {
                        if let Ok(role_id_u64) = role_id_str.parse::<u64>() {
                            current_roles.push((class.to_string(), RoleId::new(role_id_u64)));
                        }
                    }
                }
            }
            "custom" => {
                // For custom mode, check all possible roles
                let all_possible = vec![
                    "undergrad",
                    "graduate",
                    "first-year",
                    "sophomore",
                    "junior",
                    "senior",
                    "fifth-year",
                    "masters",
                    "doctoral",
                ];
                for role_key in all_possible {
                    let key = format!("guild:{}:role:{}", guild_id, role_key);
                    if let Ok(Some(role_id_str)) = redis.get::<_, Option<String>>(&key).await {
                        if let Ok(role_id_u64) = role_id_str.parse::<u64>() {
                            current_roles.push((role_key.to_string(), RoleId::new(role_id_u64)));
                        }
                    }
                }
            }
            _ => {} // "none" or unknown mode has no roles
        }

        Ok(current_roles)
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
