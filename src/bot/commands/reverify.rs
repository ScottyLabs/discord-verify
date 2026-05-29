use crate::bot::Error;
use crate::state::{AppState, ReverifyJob, VerificationComplete};
use redis::AsyncCommands;
use serenity::all::{
    CommandInteraction, Context, CreateCommand, CreateMessage, EditInteractionResponse, Permissions,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::utils::{is_admin, load_guild_config};

/// Batch size for reverification to avoid Discord rate limits
const REVERIFY_BATCH_SIZE: usize = 50;

/// Register the reverify command
pub fn register() -> CreateCommand<'static> {
    CreateCommand::new("reverify")
        .description("Re-run verification for all verified users to sync roles (admin only)")
        .default_member_permissions(Permissions::ADMINISTRATOR)
}

/// Handle the reverify command
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
            command.defer_ephemeral(&ctx.http).await?;
            command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new()
                        .content("This command can only be used in a server."),
                )
                .await?;
            return Ok(());
        }
    };

    // Check if user has administrator permissions
    if !is_admin(ctx, &command.member, guild_id, user.id).await? {
        command.defer_ephemeral(&ctx.http).await?;
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new()
                    .content("You need administrator permissions to run reverification."),
            )
            .await?;
        return Ok(());
    }

    // Reject if a reverify is already running
    if state
        .reverify_in_progress
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        command.defer_ephemeral(&ctx.http).await?;
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(
                    "A reverification is already in progress. Please wait for it to finish.",
                ),
            )
            .await?;
        return Ok(());
    }

    // Defer immediately now that we're past the quick checks
    command.defer_ephemeral(&ctx.http).await?;

    // Load guild config to get log channel
    let mut conn = state.redis.clone();
    let guild_config = load_guild_config(&ctx.http, &mut conn, guild_id).await?;
    let log_channel = guild_config.get_log_channel();

    // Scan Redis for all verified users: keys are "discord:{user_id}:keycloak"
    let pattern = "discord:*:keycloak".to_string();
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut conn)
        .await?;

    if keys.is_empty() {
        // Clear the flag since we're not actually starting a job
        state.reverify_in_progress.store(false, Ordering::SeqCst);
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("No verified users found in this server."),
            )
            .await?;
        return Ok(());
    }

    // Build VerificationComplete entries for all verified users
    let mut users = Vec::new();
    for key in &keys {
        // Key format: "discord:{user_id}:keycloak"
        let parts: Vec<&str> = key.split(':').collect();
        if parts.len() != 3 {
            continue;
        }

        let Ok(user_id_u64) = parts[1].parse::<u64>() else {
            continue;
        };

        let keycloak_id: Option<String> = conn.get(key).await.unwrap_or(None);
        let Some(keycloak_user_id) = keycloak_id else {
            continue;
        };

        // Skip the guild membership check here — complete_verification will
        // fail gracefully if the user isn't in the guild
        users.push(VerificationComplete {
            discord_user_id: serenity::all::UserId::new(user_id_u64),
            guild_id,
            keycloak_user_id,
        });
    }

    let total_users = users.len();
    if total_users == 0 {
        // Clear the flag since we're not actually starting a job
        state.reverify_in_progress.store(false, Ordering::SeqCst);
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("No verified members found in this guild."),
            )
            .await?;
        return Ok(());
    }

    // Split into batches of REVERIFY_BATCH_SIZE and send over the channel
    let batches: Vec<Vec<VerificationComplete>> = users
        .chunks(REVERIFY_BATCH_SIZE)
        .map(|c| c.to_vec())
        .collect();
    let total_batches = batches.len();

    for (i, batch) in batches.into_iter().enumerate() {
        let job = ReverifyJob {
            guild_id,
            users: batch,
            log_channel,
            total_users,
            batch_index: i + 1,
            total_batches,
        };

        if let Err(e) = state.reverify_tx.send(job) {
            tracing::error!("Failed to send reverify job batch {}: {}", i + 1, e);
        }
    }

    // Send start message to log channel if configured
    if let Some(channel_id) = log_channel
        && let Err(e) = ctx
            .http
            .send_message(
                channel_id.into(),
                Vec::new(),
                &CreateMessage::new().content(format!(
                    "Starting reverification for **{}** users across **{}** batches of up to {}.",
                    total_users, total_batches, REVERIFY_BATCH_SIZE
                )),
            )
            .await
    {
        tracing::warn!(
            "Failed to send reverify start message to log channel: {}",
            e
        );
    }

    // Respond to the interaction
    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content(format!(
                "Starting reverification for **{}** users across **{}** batches of up to {}.\n{}",
                total_users,
                total_batches,
                REVERIFY_BATCH_SIZE,
                match log_channel {
                    Some(ch) => format!("Progress updates will be posted to <#{}>.", ch),
                    None =>
                        "Configure a log channel with `/setlogchannel` to receive progress updates."
                            .to_string(),
                }
            )),
        )
        .await?;

    Ok(())
}
