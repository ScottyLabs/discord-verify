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
    pub custom_roles: Vec<String>,
}

impl SetupRolesSession {
    pub fn new(mode: String) -> Self {
        Self {
            mode,
            custom_roles: Vec::new(),
        }
    }

    /// Update the custom roles selection
    pub fn set_custom_roles(&mut self, roles: Vec<String>) {
        self.custom_roles = roles;
    }

    /// Validate that the session has all required data to proceed
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.mode == "custom" && self.custom_roles.is_empty() {
            return Err("Please select at least one role to create for custom mode.");
        }

        Ok(())
    }

    /// Determine which roles to create based on the mode and selections
    pub fn get_roles_to_create(&self) -> Vec<(String, String)> {
        match self.mode.as_str() {
            "levels" => vec![
                ("Undergrad".to_string(), "level:Undergrad".to_string()),
                ("Graduate".to_string(), "level:Graduate".to_string()),
            ],
            "classes" => vec![
                ("First-Year".to_string(), "class:First-Year".to_string()),
                ("Sophomore".to_string(), "class:Sophomore".to_string()),
                ("Junior".to_string(), "class:Junior".to_string()),
                ("Senior".to_string(), "class:Senior".to_string()),
                (
                    "Fifth-Year Senior".to_string(),
                    "class:Fifth-Year Senior".to_string(),
                ),
                ("Masters".to_string(), "class:Masters".to_string()),
                ("Doctoral".to_string(), "class:Doctoral".to_string()),
            ],
            "custom" => {
                // Map the selections to (display_name, redis_key_suffix)
                self.custom_roles
                    .iter()
                    .filter_map(|s| {
                        let parts: Vec<&str> = s.split(':').collect();
                        if parts.len() == 2 {
                            let (display_name, redis_suffix) = match parts[1] {
                                "undergrad" => ("Undergrad", "level:Undergrad"),
                                "graduate" => ("Graduate", "level:Graduate"),
                                "first-year" => ("First-Year", "class:First-Year"),
                                "sophomore" => ("Sophomore", "class:Sophomore"),
                                "junior" => ("Junior", "class:Junior"),
                                "senior" => ("Senior", "class:Senior"),
                                "fifth-year" => ("Fifth-Year Senior", "class:Fifth-Year Senior"),
                                "masters" => ("Masters", "class:Masters"),
                                "doctoral" => ("Doctoral", "class:Doctoral"),
                                _ => return None,
                            };
                            Some((display_name.to_string(), redis_suffix.to_string()))
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
        let guild = guild_id.to_partial_guild(http).await?;

        // Get the current mode to determine what roles exist
        let current_mode_key = format!("guild:{}:role_mode", guild_id);
        let current_mode: Option<String> = redis.get(&current_mode_key).await?;
        let current_mode = current_mode.unwrap_or_else(|| "none".to_string());

        // Get roles in the old mode and new mode
        let current_roles = self
            .get_current_roles(&current_mode, redis, guild_id)
            .await?;
        let desired_roles = self.get_roles_to_create();

        // Create a map from role_key to display_name for looking up names
        let role_key_to_name: std::collections::HashMap<_, _> = desired_roles
            .iter()
            .map(|(name, key)| (key.as_str(), name.as_str()))
            .collect();

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
            // Check if the role still exists in the guild before trying to delete it
            if guild.roles.contains_key(role_id) {
                if let Err(e) = guild_id.delete_role(http, *role_id, None).await {
                    tracing::warn!("Failed to delete role {}: {}", role_id, e);
                }
            } else {
                tracing::debug!(
                    "Role {} (ID: {}) already deleted from Discord, cleaning up Redis key",
                    role_key,
                    role_id
                );
            }

            // Clean up the Redis key regardless of whether the Discord role exists
            let redis_key = format!("guild:{}:role:{}", guild_id, role_key);
            let _: () = redis.del(&redis_key).await?;
        }

        let mut all_roles = Vec::new();

        // Create all new roles
        for (role_name, role_key) in &roles_to_create {
            // Check if a role with this name already exists in the guild
            let existing_role = guild.roles.iter().find(|r| r.name == *role_name);

            let role_id = if let Some(existing) = existing_role {
                tracing::info!(
                    "Role '{}' already exists in Discord (ID: {}), reusing it",
                    role_name,
                    existing.id
                );
                existing.id
            } else {
                // Create the role
                let new_role = guild_id
                    .create_role(
                        http,
                        serenity::all::EditRole::new().name(role_name.as_str()),
                    )
                    .await?;
                new_role.id
            };

            all_roles.push((role_key.clone(), role_id));

            // Store role ID in Redis
            let redis_key = format!("guild:{}:role:{}", guild_id, role_key);
            let _: () = redis.set(&redis_key, role_id.get()).await?;
        }

        // Add kept roles to the list
        for (role_key, role_id) in &roles_to_keep {
            if !guild.roles.contains_key(role_id) {
                // Role was manually deleted from Discord but still in Redis, recreate it
                tracing::info!(
                    "Role {} (ID: {}) was deleted from Discord, recreating it",
                    role_key,
                    role_id
                );

                // Get the display name for this role
                let display_name = role_key_to_name
                    .get(role_key.as_str())
                    .ok_or(format!("Missing display name for role key: {}", role_key))?;

                // Create the role
                let new_role = guild_id
                    .create_role(http, serenity::all::EditRole::new().name(*display_name))
                    .await?;

                // Update Redis with the new role ID
                let redis_key = format!("guild:{}:role:{}", guild_id, role_key);
                let _: () = redis.set(&redis_key, new_role.id.get()).await?;

                all_roles.push((role_key.clone(), new_role.id));
            } else {
                // Role exists, just add it to the list
                all_roles.push((role_key.clone(), *role_id));
            }
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
                for level_name in &["Undergrad", "Graduate"] {
                    let key = format!("guild:{}:role:level:{}", guild_id, level_name);
                    if let Ok(Some(role_id_str)) = redis.get::<_, Option<String>>(&key).await
                        && let Ok(role_id_u64) = role_id_str.parse::<u64>()
                    {
                        current_roles
                            .push((format!("level:{}", level_name), RoleId::new(role_id_u64)));
                    }
                }
            }
            "classes" => {
                for class_name in &[
                    "First-Year",
                    "Sophomore",
                    "Junior",
                    "Senior",
                    "Fifth-Year Senior",
                    "Masters",
                    "Doctoral",
                ] {
                    let key = format!("guild:{}:role:class:{}", guild_id, class_name);
                    if let Ok(Some(role_id_str)) = redis.get::<_, Option<String>>(&key).await
                        && let Ok(role_id_u64) = role_id_str.parse::<u64>()
                    {
                        current_roles
                            .push((format!("class:{}", class_name), RoleId::new(role_id_u64)));
                    }
                }
            }
            "custom" => {
                // For custom mode, check all possible roles
                let all_possible = vec![
                    ("level:Undergrad", "Undergrad"),
                    ("level:Graduate", "Graduate"),
                    ("class:First-Year", "First-Year"),
                    ("class:Sophomore", "Sophomore"),
                    ("class:Junior", "Junior"),
                    ("class:Senior", "Senior"),
                    ("class:Fifth-Year Senior", "Fifth-Year Senior"),
                    ("class:Masters", "Masters"),
                    ("class:Doctoral", "Doctoral"),
                ];
                for (redis_suffix, _name) in all_possible {
                    let key = format!("guild:{}:role:{}", guild_id, redis_suffix);
                    if let Ok(Some(role_id_str)) = redis.get::<_, Option<String>>(&key).await
                        && let Ok(role_id_u64) = role_id_str.parse::<u64>()
                    {
                        current_roles.push((redis_suffix.to_string(), RoleId::new(role_id_u64)));
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
