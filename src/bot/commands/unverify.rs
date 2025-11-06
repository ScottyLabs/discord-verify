use crate::bot::{Context, Error};
use poise::serenity_prelude::{self as serenity, Mentionable};
use redis::AsyncCommands;

use super::utils::{get_verified_role_id, is_admin};

/// Remove verification for a user
#[poise::command(slash_command)]
pub async fn unverify(
    ctx: Context<'_>,
    #[description = "User to unverify (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let state = ctx.data();
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());

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

    // If targeting another user, require administrator permissions
    if target_user.id != ctx.author().id {
        if !is_admin(&ctx, guild_id, ctx.author().id).await? {
            ctx.send(
                poise::CreateReply::default()
                    .content("You need administrator permissions to unverify other users.")
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Look up Keycloak user ID from Redis
    let mut conn = state.redis.clone();
    let redis_key = format!("discord:{}:keycloak", target_user.id);
    let keycloak_user_id: Option<String> = conn.get(&redis_key).await?;

    // Check if user is actually verified
    let keycloak_user_id = match keycloak_user_id {
        Some(id) => id,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("{} is not verified.", target_user.mention()))
                    .ephemeral(true),
            )
            .await?;

            return Ok(());
        }
    };

    // Remove Redis mappings
    redis::cmd("DEL")
        .arg(format!("keycloak:{}:discord", keycloak_user_id))
        .query_async::<()>(&mut conn)
        .await?;

    redis::cmd("DEL")
        .arg(&redis_key)
        .query_async::<()>(&mut conn)
        .await?;

    redis::cmd("DEL")
        .arg(format!("discord:{}:verified_at", target_user.id))
        .query_async::<()>(&mut conn)
        .await?;

    // Remove verified role
    let member = guild_id.member(&ctx.http(), target_user.id).await?;

    if let Ok(verified_role) = get_verified_role_id(&ctx.http(), &mut conn, guild_id).await {
        if member.roles.contains(&verified_role) {
            member.remove_role(&ctx.http(), verified_role).await?;
        }
    }

    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "Removed verification for {}.",
                target_user.mention()
            ))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
