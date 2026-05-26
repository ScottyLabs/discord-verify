use crate::bot::Error;
use crate::state::{AppState, ReverifyJob, VerificationComplete};
use redis::AsyncCommands;
use serenity::all::{
    CommandInteraction, Context, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseMessage,
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
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("This command can only be used in a server.")
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    // Check if user has administrator permissions
    if !is_admin(ctx, &command.member, guild_id, user.id).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("You need administrator permissions to run reverification.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Reject if a reverify is already running
    if state
        .reverify_in_progress
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("A reverification is already in progress. Please wait for it to finish.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

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

        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("No verified users found in this server.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Build VerificationComplete entries for all verified users in this guild
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

        // Only include users who are actually members of this guild
        let discord_user_id = serenity::all::UserId::new(user_id_u64);
        if guild_id.member(&ctx.http, discord_user_id).await.is_err() {
            continue;
        }

        users.push(VerificationComplete {
            discord_user_id,
            guild_id,
            keycloak_user_id,
        });
    }

    let total_users = users.len();
    if total_users == 0 {
        // Clear the flag since we're not actually starting a job
        state.reverify_in_progress.store(false, Ordering::SeqCst);

        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("No verified members found in this guild.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
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

    // Progress updates go to the log channel
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(format!(
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
            ))
            .ephemeral(true),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
