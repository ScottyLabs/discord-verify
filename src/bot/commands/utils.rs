use crate::bot::Error;
use crate::bot::role_config::GuildRoleConfig;
use serenity::all::{Context, GuildId, Member, Permissions, RoleId, UserId};

/// Helper function to load the guild's role configuration
pub async fn load_guild_role_config(
    http: &serenity::all::Http,
    redis: &mut redis::aio::ConnectionManager,
    guild_id: GuildId,
) -> Result<GuildRoleConfig, Error> {
    GuildRoleConfig::load(redis, http, guild_id).await
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
