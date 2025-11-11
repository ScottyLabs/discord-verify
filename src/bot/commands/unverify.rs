use crate::bot::Error;
use crate::state::AppState;
use redis::AsyncCommands;
use serenity::all::{
    CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
    Mentionable, ResolvedOption, ResolvedValue,
};
use std::sync::Arc;

use super::utils::{is_admin, load_guild_config};

/// Register the unverify command
pub fn register() -> CreateCommand<'static> {
    CreateCommand::new("unverify")
        .description("Remove verification for a user")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::User,
                "user",
                "User to unverify (defaults to you)",
            )
            .required(false),
        )
}

/// Handle the unverify command
pub async fn handle(
    ctx: &Context,
    command: &CommandInteraction,
    state: &Arc<AppState>,
) -> Result<(), Error> {
    let user = &command.user;

    // Get the target user from options, or default to command user
    let target_user = if let Some(ResolvedOption {
        value: ResolvedValue::User(u, _),
        ..
    }) = command.data.options().first()
    {
        u
    } else {
        user
    };

    // Get guild_id from context
    let guild_id = match command.guild_id {
        Some(id) => id,
        None => {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("This command can only be used in a server.")
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    // If targeting another user, require administrator permissions
    if target_user.id != user.id && !is_admin(ctx, &command.member, guild_id, user.id).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("You need administrator permissions to unverify other users.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Look up Keycloak user ID from Redis
    let mut conn = state.redis.clone();
    let redis_key = format!("discord:{}:keycloak", target_user.id);
    let keycloak_user_id: Option<String> = conn.get(&redis_key).await?;

    // Check if user is actually verified
    let keycloak_user_id = match keycloak_user_id {
        Some(id) => id,
        None => {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(format!("{} is not verified.", target_user.mention()))
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
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

    // Remove verified role and track removed roles for logging
    let member = guild_id.member(&ctx.http, target_user.id).await?;
    let mut removed_roles = Vec::new();

    if let Ok(guild_config) = load_guild_config(&ctx.http, &mut conn, guild_id).await {
        if let Ok(verified_role) = guild_config.get_verified_role()
            && member.roles.contains(&verified_role)
        {
            member.remove_role(&ctx.http, verified_role, None).await?;
            removed_roles.push(verified_role);
        }

        // Also remove level and class roles if present
        for role_id in member.roles.iter() {
            if guild_config.level_roles.values().any(|r| r == role_id)
                || guild_config.class_roles.values().any(|r| r == role_id)
            {
                if let Err(e) = member.remove_role(&ctx.http, *role_id, None).await {
                    tracing::warn!("Failed to remove role {}: {}", role_id, e);
                } else {
                    removed_roles.push(*role_id);
                }
            }
        }

        // Log to log channel if configured
        if let Some(channel_id) = guild_config.get_log_channel() {
            // Format roles list
            let roles_mentions: Vec<String> = removed_roles
                .iter()
                .map(|role_id| format!("<@&{}>", role_id))
                .collect();
            let roles_text = if roles_mentions.is_empty() {
                "None".to_string()
            } else {
                roles_mentions.join(", ")
            };

            let embed = CreateEmbed::new()
                .title("User Unverified")
                .color(0xF38BA8) // Red
                .field("User", target_user.mention().to_string(), false)
                .field("Roles Removed", roles_text, false)
                .timestamp(chrono::Utc::now());

            if let Err(e) = ctx
                .http
                .send_message(
                    channel_id.into(),
                    Vec::new(),
                    &CreateMessage::new().embed(embed),
                )
                .await
            {
                tracing::warn!(
                    "Failed to send unverification log to channel {}: {}",
                    channel_id,
                    e
                );
            }
        }
    }

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(format!(
                "Removed verification for {}.",
                target_user.mention()
            ))
            .ephemeral(true),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
