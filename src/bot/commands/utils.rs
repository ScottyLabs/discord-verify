use crate::bot::{Context, Error};
use poise::serenity_prelude as serenity;
use redis::AsyncCommands;

/// Helper function to get the configured verified role for a guild.
/// Returns an error if no role is configured or if the configured role no longer exists.
pub async fn get_verified_role_id(
    http: &serenity::Http,
    redis: &mut redis::aio::ConnectionManager,
    guild_id: serenity::GuildId,
) -> Result<serenity::RoleId, Error> {
    // Try to get configured role from Redis
    let redis_key = format!("guild:{}:verified_role", guild_id);
    let role_id_str: Option<String> = redis.get(&redis_key).await?;

    let role_id_str = role_id_str.ok_or_else(|| {
        "No verified role configured for this server. Please ask an administrator to run /setverifiedrole first."
    })?;

    // Parse the stored role ID
    let role_id_u64 = role_id_str
        .parse::<u64>()
        .map_err(|_| "Invalid role ID stored in configuration")?;
    let role_id = serenity::RoleId::new(role_id_u64);

    // Verify the role still exists
    let roles = guild_id.roles(http).await?;
    if !roles.contains_key(&role_id) {
        return Err(
            "The configured verified role no longer exists. Please ask an administrator to run /setverifiedrole again."
                .into(),
        );
    }

    Ok(role_id)
}

/// Check if a user has administrator permissions in a guild
pub async fn is_admin(
    ctx: &Context<'_>,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> Result<bool, Error> {
    let member = guild_id.member(&ctx.http(), user_id).await?;

    let cache = ctx.cache();
    let guild = guild_id
        .to_guild_cached(&cache)
        .ok_or("Guild not found in cache")?;

    // Check if user is the owner
    if guild.owner_id == user_id {
        return Ok(true);
    }

    // For guild-wide permissions, compute from base roles
    let mut permissions = serenity::Permissions::empty();

    // Add @everyone role permissions
    if let Some(everyone_role) = guild.roles.get(&serenity::RoleId::new(guild_id.get())) {
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
