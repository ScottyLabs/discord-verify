use crate::bot::{Context, Error};
use poise::serenity_prelude::{self as serenity, Mentionable};

use super::utils::is_admin;

/// Set the verified role for this server
#[poise::command(slash_command)]
pub async fn setverifiedrole(
    ctx: Context<'_>,
    #[description = "The role to assign when users verify"] role: serenity::Role,
) -> Result<(), Error> {
    let state = ctx.data();

    // Get guild_id from context
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content("This command can only be used in a server.")
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Check if user has administrator permissions
    if !is_admin(&ctx, guild_id, ctx.author().id).await? {
        ctx.send(
            poise::CreateReply::default()
                .content("You need administrator permissions to configure the verified role.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    // Check if bot can actually assign this role
    let bot_user_id = ctx.cache().current_user().id;
    let bot_member = guild_id.member(&ctx.http(), bot_user_id).await?;
    let guild_roles = guild_id.roles(&ctx.http()).await?;

    // Find bot's highest role position
    let bot_top_role = bot_member
        .roles
        .iter()
        .filter_map(|role_id| guild_roles.get(role_id))
        .max_by_key(|role| role.position);

    let bot_position = bot_top_role.map(|r| r.position).unwrap_or(0);
    let target_role_position = guild_roles.get(&role.id).map(|r| r.position).unwrap_or(0);

    if bot_position <= target_role_position {
        ctx.send(
            poise::CreateReply::default()
                .content(format!(
                    "I cannot assign {}. My highest role is at position {}, but this role is at position {}.\n\
                    Please move my role higher than {} in the server settings.",
                    role.mention(),
                    bot_position,
                    target_role_position,
                    role.mention()
                ))
                .ephemeral(true),
        )
        .await?;

        return Ok(());
    }

    // Store the role ID in Redis
    let mut conn = state.redis.clone();
    let redis_key = format!("guild:{}:verified_role", guild_id);

    redis::cmd("SET")
        .arg(&redis_key)
        .arg(role.id.to_string())
        .query_async::<()>(&mut conn)
        .await?;

    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "Verified role has been set to {}. Users who verify will now receive this role.",
                role.mention()
            ))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
