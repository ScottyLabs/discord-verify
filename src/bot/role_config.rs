use crate::bot::Error;
use redis::AsyncCommands;
use serenity::all::{GuildId, Http, RoleId};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleMode {
    None,
    Levels,
    Classes,
    Custom,
}

impl RoleMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "levels" => Self::Levels,
            "classes" => Self::Classes,
            "custom" => Self::Custom,
            _ => Self::None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::None => "none",
            Self::Levels => "levels",
            Self::Classes => "classes",
            Self::Custom => "custom",
        }
    }
}

/// Configuration for all assignable roles in a guild
#[derive(Debug, Clone)]
pub struct GuildRoleConfig {
    pub guild_id: GuildId,
    pub verified_role: Option<RoleId>,
    pub mode: RoleMode,
    pub level_roles: HashMap<String, RoleId>,
    pub class_roles: HashMap<String, RoleId>,
}

impl GuildRoleConfig {
    /// Load role configuration from Redis for a guild
    pub async fn load(
        redis: &mut redis::aio::ConnectionManager,
        http: &Http,
        guild_id: GuildId,
    ) -> Result<Self, Error> {
        // Get the verified role
        let verified_role_key = format!("guild:{}:role:verified", guild_id);
        let verified_role: Option<String> = redis.get(&verified_role_key).await?;
        let verified_role =
            verified_role.and_then(|s| s.parse::<u64>().ok().map(|id| RoleId::new(id)));

        // Get the role mode
        let role_mode_key = format!("guild:{}:role_mode", guild_id);
        let role_mode: Option<String> = redis.get(&role_mode_key).await?;
        let mode = RoleMode::from_str(&role_mode.unwrap_or_else(|| "none".to_string()));

        // Get level roles
        let mut level_roles = HashMap::new();
        for level in &["Undergrad", "Graduate"] {
            let key = format!("guild:{}:role:level:{}", guild_id, level);
            if let Ok(Some(role_id_str)) = redis.get::<_, Option<String>>(&key).await {
                if let Ok(role_id_u64) = role_id_str.parse::<u64>() {
                    // Verify the role still exists
                    let role_id = RoleId::new(role_id_u64);
                    let roles = guild_id.roles(http).await?;

                    if roles.contains_key(&role_id) {
                        level_roles.insert(level.to_string(), role_id);
                    }
                }
            }
        }

        // Get class roles
        let mut class_roles = HashMap::new();
        for class in &[
            "First-Year",
            "Sophomore",
            "Junior",
            "Senior",
            "Fifth-Year Senior",
            "Masters",
            "Doctoral",
        ] {
            let key = format!("guild:{}:role:class:{}", guild_id, class);
            if let Ok(Some(role_id_str)) = redis.get::<_, Option<String>>(&key).await {
                if let Ok(role_id_u64) = role_id_str.parse::<u64>() {
                    // Verify the role still exists
                    let role_id = RoleId::new(role_id_u64);
                    let roles = guild_id.roles(http).await?;

                    if roles.contains_key(&role_id) {
                        class_roles.insert(class.to_string(), role_id);
                    }
                }
            }
        }

        Ok(Self {
            guild_id,
            verified_role,
            mode,
            level_roles,
            class_roles,
        })
    }

    /// Get the verified role, or return an error if not configured
    pub fn get_verified_role(&self) -> Result<RoleId, Error> {
        self.verified_role.ok_or_else(|| {
            "No verified role configured for this server. Please ask an administrator to run `/setverifiedrole` first."
                .into()
        })
    }

    /// Get the role for a specific level (e.g., "Undergrad" or "Graduate")
    pub fn get_level_role(&self, level: &str) -> Option<RoleId> {
        self.level_roles.get(level).copied()
    }

    /// Get the role for a specific class (e.g., "First-Year", "Sophomore", etc.)
    pub fn get_class_role(&self, class: &str) -> Option<RoleId> {
        self.class_roles.get(class).copied()
    }

    /// Check if level roles should be assigned based on the mode
    pub fn should_assign_level_roles(&self) -> bool {
        matches!(self.mode, RoleMode::Levels | RoleMode::Custom)
    }

    /// Check if class roles should be assigned based on the mode
    pub fn should_assign_class_roles(&self) -> bool {
        matches!(self.mode, RoleMode::Classes | RoleMode::Custom)
    }
}
