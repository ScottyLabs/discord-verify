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
