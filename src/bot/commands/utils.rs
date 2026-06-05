use crate::bot::Error;
use crate::bot::guild_config::GuildConfig;
use serenity::all::{Cache, Context, GuildId, Member, Permissions, RoleId, UserId};

/// Count members with a role from the gateway cache (fast; safe for large guilds).
pub fn count_guild_members_with_role_cached(
    guild_id: GuildId,
    cache: &Cache,
    role_id: RoleId,
) -> (usize, usize) {
    let Some(guild) = guild_id.to_guild_cached(cache) else {
        return (0, 0);
    };

    let total = guild.member_count as usize;
    let verified = guild
        .members
        .iter()
        .filter(|m| m.roles.contains(&role_id))
        .count();

    (verified, total)
}

/// Normalize a Redis string value (migration may have left trailing newlines).
pub fn trim_redis_value(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Helper function to load the guild's configuration
pub async fn load_guild_config(
    http: &serenity::all::Http,
    redis: &mut redis::aio::ConnectionManager,
    guild_id: GuildId,
) -> Result<GuildConfig, Error> {
    GuildConfig::load(redis, http, guild_id).await
}

/// Check if a user has administrator permissions in a guild
pub async fn is_admin(
    ctx: &Context,
    member_option: &Option<Box<Member>>,
    guild_id: GuildId,
    user_id: UserId,
) -> Result<bool, Error> {
    // Get the member from the option or fetch it
    let member = match member_option {
        Some(m) => (**m).clone(),
        None => guild_id.member(&ctx.http, user_id).await?,
    };

    let guild = guild_id
        .to_guild_cached(&ctx.cache)
        .ok_or("Guild not found in cache")?;

    // Check if user is the owner
    if guild.owner_id == user_id {
        return Ok(true);
    }

    // For guild-wide permissions, compute from base roles
    let mut permissions = Permissions::empty();

    // Add @everyone role permissions
    if let Some(everyone_role) = guild.roles.get(&RoleId::new(guild_id.get())) {
        permissions |= everyone_role.permissions;
    }

    // Add member's role permissions
    for role_id in &member.roles {
        if let Some(role) = guild.roles.get(role_id) {
            permissions |= role.permissions;
        }
    }

    Ok(permissions.administrator())
}
