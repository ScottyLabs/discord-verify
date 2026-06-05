use crate::bot::Error;
use crate::state::{AppState, PendingVerification};
use redis::AsyncCommands;
use serenity::all::{
    CommandInteraction, Context, CreateCommand, CreateEmbed, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateMessage, GuildId, Mentionable, RoleId, UserId,
};
use std::sync::Arc;
use uuid::Uuid;

use super::utils::{load_guild_config, trim_redis_value};

use std::collections::HashSet;

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
    let existing_keycloak_id = trim_redis_value(conn.get(&redis_key).await?);

    if let Some(keycloak_user_id) = existing_keycloak_id {
        // User is already verified globally, complete verification in this server
        complete_verification(
            &ctx.http,
            &ctx.cache,
            state,
            user.id,
            guild_id.get(),
            keycloak_user_id,
            true,
        )
        .await?;

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

/// Formats a Vec of role ids to be a comma separated string with <@&__________>
fn format_roles(roles: Vec<RoleId>) -> String {
    let roles_mentions: Vec<String> = roles
        .iter()
        .map(|role_id| format!("<@&{}>", role_id))
        .collect();
    if roles_mentions.is_empty() {
        "None".to_string()
    } else {
        roles_mentions.join(", ")
    }
}

/// Complete the verification process by assigning role and storing mappings
/// Called by the bot task when it receives a verification completion event.
/// `send_dm` controls whether the user receives a DM on success: pass false
/// for background jobs like reverify to avoid spamming users.
pub async fn complete_verification(
    http: &serenity::all::Http,
    _cache: &serenity::all::Cache,
    state: &AppState,
    discord_user_id: UserId,
    guild_id: u64,
    keycloak_user_id: String,
    send_dm: bool,
) -> Result<(), Error> {
    let guild_id = GuildId::new(guild_id);

    let mut verification_issues = Vec::new();

    // Load the guild's role configuration
    let mut redis = state.redis.clone();
    let guild_config = load_guild_config(http, &mut redis, guild_id).await?;

    // Track roles that were added and removed for logging
    let mut added_roles = Vec::new();
    let mut removed_roles = Vec::new();

    // Remove unverified role if configured
    let unverified_redis_key = format!("guild:{}:role:unverified", guild_id);
    if let Ok(Some(unverified_role_str)) =
        redis.get::<_, Option<String>>(&unverified_redis_key).await
        && let Ok(unverified_role_id) = unverified_role_str.parse::<u64>()
    {
        let unverified_role = serenity::all::RoleId::new(unverified_role_id);
        // Ensure the role is removed even if the cache is stale
        if let Err(e) = http
            .remove_member_role(guild_id, discord_user_id, unverified_role, None)
            .await
        {
            tracing::warn!("Failed to remove unverified role: {}", e);
            verification_issues.push(format!("Failed to remove unverified role: {}", e));
        } else {
            removed_roles.push(unverified_role);
        }
    }

    // Find existing level/class roles to remove first
    let managed_roles: HashSet<serenity::all::RoleId> = guild_config
        .level_roles
        .values()
        .chain(guild_config.class_roles.values())
        .copied()
        .collect();

    // Re-fetch member to ensure fresh role state
    let member = http.get_member(guild_id, discord_user_id).await?;

    // Find all managed roles currently on the member
    let roles_to_remove: Vec<serenity::all::RoleId> = member
        .roles
        .iter()
        .filter(|role_id| managed_roles.contains(role_id))
        .copied()
        .collect();

    // Remove verification managed roles from member
    for role_id in roles_to_remove {
        if let Err(e) = http
            .remove_member_role(guild_id, discord_user_id, role_id, None)
            .await
        {
            tracing::warn!(
                "Failed to remove managed role {} from user {}: {}",
                role_id,
                discord_user_id,
                e
            );
            verification_issues.push(format!("Failed to remove managed role {}: {}", role_id, e));
        } else {
            tracing::info!(
                "Removed managed role {} from user {}",
                role_id,
                discord_user_id
            );
            removed_roles.push(role_id);
        }
    }

    // Re-fetch member to ensure fresh role state
    let member = http.get_member(guild_id, discord_user_id).await?;

    // Assign verified role
    let verified_role = guild_config.get_verified_role()?;
    if let Err(e) = member.add_role(http, verified_role, None).await {
        verification_issues.push(format!("Failed to assign verified role: {}", e));
    } else {
        added_roles.push(verified_role);
    }

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
                verification_issues.push(format!("Failed to assign level role {}: {}", level, e));
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
                verification_issues.push(format!("Failed to assign class role {}: {}", class, e));
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

        let embed = CreateEmbed::new()
            .title("User Verified")
            .color(0xA6E3A1) // Green
            .field("User", format!("{}", discord_user_id.mention()), false)
            .field("Roles Added", format_roles(added_roles), false)
            .field("Roles Removed", format_roles(removed_roles), false)
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

    // Only DM the user if requested (skipped during reverify to avoid spam)
    if send_dm
        && let Err(e) = discord_user_id
            .direct_message(
                http,
                CreateMessage::new().content("You have successfully verified your Andrew ID."),
            )
            .await
    {
        tracing::warn!(
            "Failed to send verification DM to user {}: {}",
            discord_user_id,
            e
        );
    }

    // Send logs of any verification issue to the log channel
    if !verification_issues.is_empty()
        && let Some(channel_id) = guild_config.get_log_channel()
    {
        let issue_text = verification_issues.join("\n");

        let truncated_issue_text: String = if issue_text.chars().count() > 1024 {
            issue_text.chars().take(1021).collect::<String>() + "..."
        } else {
            issue_text
        };

        let embed = CreateEmbed::new()
            .title("Reverification Warning")
            .color(0xF9E2AF) // Yellow
            .field("User", format!("{}", discord_user_id.mention()), false)
            .field("Issues", truncated_issue_text, false)
            .timestamp(chrono::Utc::now());

        let _ = http
            .send_message(
                channel_id.into(),
                Vec::new(),
                &CreateMessage::new().embed(embed),
            )
            .await;
    }

    Ok(())
}
