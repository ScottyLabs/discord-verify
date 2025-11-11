use crate::bot::Error;
use crate::state::{AppState, PendingVerification};
use redis::AsyncCommands;
use serenity::all::{
    CommandInteraction, Context, CreateCommand, CreateEmbed, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateMessage, GuildId, Mentionable, UserId,
};
use std::sync::Arc;
use uuid::Uuid;

use super::utils::load_guild_config;

/// Register the verify command
pub fn register() -> CreateCommand<'static> {
    CreateCommand::new("verify").description("Verify your Andrew ID")
}

/// Handle the verify command
pub async fn handle(
    ctx: &Context,
    command: &CommandInteraction,
    state: &Arc<AppState>,
) -> Result<(), Error> {
    let user = &command.user;

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

    // Check if user is already verified globally
    let mut conn = state.redis.clone();
    let redis_key = format!("discord:{}:keycloak", user.id);
    let existing_keycloak_id: Option<String> = conn.get(&redis_key).await?;

    if existing_keycloak_id.is_some() {
        // Just assign role in this server
        let role_config = load_guild_config(&ctx.http, &mut conn, guild_id).await?;
        let verified_role = role_config.get_verified_role()?;
        let member = guild_id.member(&ctx.http, user.id).await?;
        member.add_role(&ctx.http, verified_role, None).await?;

        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("You are already verified. The verified role has been assigned to you in this server.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Generate unique state token
    let state_token = Uuid::new_v4();

    // Store verification request
    let verification = PendingVerification {
        discord_user_id: user.id,
        discord_username: user.name.to_string(),
        guild_id,
        created_at: chrono::Utc::now().timestamp(),
    };

    state
        .pending_verifications
        .write()
        .await
        .insert(state_token.to_string(), verification.clone());

    // Also store in Redis with TTL
    let mut conn = state.redis.clone();
    let key = format!("verify:{}", state_token);
    let data = serde_json::to_string(&verification)?;

    redis::cmd("SETEX")
        .arg(&key)
        .arg(600) // 10 minutes
        .arg(&data)
        .query_async::<()>(&mut conn)
        .await?;

    // Create verification link
    let verify_url = format!("{}/verify?state={}", state.config.app_url, state_token);

    // Send ephemeral message
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(format!(
                "Click the link below to verify your account. This link expires in 10 minutes.\n\n{}",
                verify_url
            ))
            .ephemeral(true),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}

/// Complete the verification process by assigning role and storing mappings
/// Called by the bot task when it receives a verification completion event
pub async fn complete_verification(
    http: &serenity::all::Http,
    _cache: &serenity::all::Cache,
    state: &AppState,
    discord_user_id: UserId,
    guild_id: u64,
    keycloak_user_id: String,
) -> Result<(), Error> {
    let guild_id = GuildId::new(guild_id);
    let member = guild_id.member(http, discord_user_id).await?;

    // Load the guild's role configuration
    let mut redis = state.redis.clone();
    let guild_config = load_guild_config(http, &mut redis, guild_id).await?;

    // Track roles that were added for logging
    let mut added_roles = Vec::new();

    // Assign verified role
    let verified_role = guild_config.get_verified_role()?;
    member.add_role(http, verified_role, None).await?;
    added_roles.push(verified_role);

    // Fetch Keycloak user to get attributes
    let keycloak_user = state.keycloak.get_user(&keycloak_user_id).await?;

    // Assign additional roles based on mode and user attributes
    if let Some(attrs) = keycloak_user.attributes.as_ref() {
        // Try to assign level-based role
        if guild_config.should_assign_level_roles()
            && let Some(level_values) = attrs.get("level")
            && let Some(level) = level_values.first()
            && let Some(level_role) = guild_config.get_level_role(level)
        {
            if let Err(e) = member.add_role(http, level_role, None).await {
                tracing::warn!("Failed to assign level role {}: {}", level, e);
            } else {
                added_roles.push(level_role);
            }
        }

        // Try to assign class-based role
        if guild_config.should_assign_class_roles()
            && let Some(class_values) = attrs.get("class")
            && let Some(class) = class_values.first()
            && let Some(class_role) = guild_config.get_class_role(class)
        {
            if let Err(e) = member.add_role(http, class_role, None).await {
                tracing::warn!("Failed to assign class role {}: {}", class, e);
            } else {
                added_roles.push(class_role);
            }
        }
    }

    // Store mapping in Redis
    let mut conn = state.redis.clone();
    let timestamp = chrono::Utc::now().timestamp();

    redis::cmd("SET")
        .arg(format!("discord:{}:verified_at", discord_user_id))
        .arg(timestamp.to_string())
        .query_async::<()>(&mut conn)
        .await?;

    redis::cmd("SET")
        .arg(format!("discord:{}:keycloak", discord_user_id))
        .arg(&keycloak_user_id)
        .query_async::<()>(&mut conn)
        .await?;

    redis::cmd("SET")
        .arg(format!("keycloak:{}:discord", keycloak_user_id))
        .arg(discord_user_id.to_string())
        .query_async::<()>(&mut conn)
        .await?;

    // Log to log channel if configured
    if let Some(channel_id) = guild_config.get_log_channel() {
        // Format roles list
        let roles_mentions: Vec<String> = added_roles
            .iter()
            .map(|role_id| format!("<@&{}>", role_id))
            .collect();
        let roles_text = if roles_mentions.is_empty() {
            "None".to_string()
        } else {
            roles_mentions.join(", ")
        };

        let embed = CreateEmbed::new()
            .title("User Verified")
            .color(0xA6E3A1) // Green
            .field("User", format!("{}", discord_user_id.mention()), false)
            .field("Roles Added", roles_text, false)
            .timestamp(chrono::Utc::now());

        if let Err(e) = http
            .send_message(
                channel_id.into(),
                Vec::new(),
                &CreateMessage::new().embed(embed),
            )
            .await
        {
            tracing::warn!(
                "Failed to send verification log to channel {}: {}",
                channel_id,
                e
            );
        }
    }

    // DM the user
    discord_user_id
        .direct_message(
            http,
            CreateMessage::new().content("You have successfully verified your Andrew ID."),
        )
        .await?;

    Ok(())
}
