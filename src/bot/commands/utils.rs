use crate::bot::Error;
use redis::AsyncCommands;
use serenity::all::{Context, GuildId, Member, Permissions, RoleId, UserId};

/// Helper function to get the configured verified role for a guild.
/// Returns an error if no role is configured or if the configured role no longer exists.
pub async fn get_verified_role_id(
    http: &serenity::all::Http,
    redis: &mut redis::aio::ConnectionManager,
    guild_id: GuildId,
) -> Result<RoleId, Error> {
    // Try to get configured role from Redis
    let redis_key = format!("guild:{}:role:verified", guild_id);
    let role_id_str: Option<String> = redis.get(&redis_key).await?;

    let role_id_str = role_id_str.ok_or_else(|| {
        "No verified role configured for this server. Please ask an administrator to run /setverifiedrole first."
    })?;

    // Parse the stored role ID
    let role_id_u64 = role_id_str
        .parse::<u64>()
        .map_err(|_| "Invalid role ID stored in configuration")?;
    let role_id = RoleId::new(role_id_u64);

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
