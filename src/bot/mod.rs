mod commands;
pub mod guild_config;

use crate::state::{AppState, ReverifyJob, VerificationComplete};
use redis::AsyncCommands;
use serenity::Client;
use serenity::all::{
    Context, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
    EventHandler, GatewayIntents, Interaction,
};
use serenity::async_trait;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio_stream::{self as stream, StreamExt};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub struct Handler {
    pub state: Arc<AppState>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn dispatch(&self, ctx: &Context, event: &serenity::all::FullEvent) {
        match event {
            serenity::all::FullEvent::Ready { data_about_bot, .. } => {
                tracing::info!("{} is connected!", data_about_bot.user.name);

                // Reset reverify flag in case bot restarted mid-job
                self.state
                    .reverify_in_progress
                    .store(false, Ordering::SeqCst);

                // Register global slash commands
                if let Err(e) = commands::register_commands(ctx).await {
                    tracing::error!("Failed to register commands: {}", e);
                } else {
                    tracing::info!("Successfully registered slash commands");
                }
            }
            serenity::all::FullEvent::GuildMemberAddition { new_member, .. } => {
                // Auto-assign unverified role if configured
                let guild_id = new_member.guild_id;
                let mut conn = self.state.redis.clone();
                let redis_key = format!("guild:{}:role:unverified", guild_id);

                if let Ok(Some(role_id_str)) = conn.get::<_, Option<String>>(&redis_key).await
                    && let Ok(role_id_u64) = role_id_str.parse::<u64>()
                {
                    let role_id = serenity::all::RoleId::new(role_id_u64);

                    if let Err(e) = new_member.add_role(&ctx.http, role_id, None).await {
                        tracing::warn!(
                            "Failed to assign unverified role {} to user {} in guild {}: {}",
                            role_id,
                            new_member.user.id,
                            guild_id,
                            e
                        );
                    } else {
                        tracing::info!(
                            "Assigned unverified role {} to user {} in guild {}",
                            role_id,
                            new_member.user.id,
                            guild_id
                        );
                    }
                }
            }
            serenity::all::FullEvent::InteractionCreate { interaction, .. } => {
                match interaction {
                    Interaction::Command(command) => {
                        let result = match command.data.name.as_str() {
                            "verify" => commands::verify::handle(ctx, command, &self.state).await,
                            "unverify" => {
                                commands::unverify::handle(ctx, command, &self.state).await
                            }
                            "userinfo" => {
                                commands::userinfo::handle(ctx, command, &self.state).await
                            }
                            "setverifiedrole" => {
                                commands::setverifiedrole::handle(ctx, command, &self.state).await
                            }
                            "setunverifiedrole" => {
                                commands::setunverifiedrole::handle(ctx, command, &self.state).await
                            }
                            "setlogchannel" => {
                                commands::setlogchannel::handle(ctx, command, &self.state).await
                            }
                            "setuproles" => {
                                commands::setuproles::handle(ctx, command, &self.state).await
                            }
                            "config" => commands::config::handle(ctx, command, &self.state).await,
                            "reverify" => {
                                commands::reverify::handle(ctx, command, &self.state).await
                            }
                            _ => {
                                tracing::warn!("Unknown command: {}", command.data.name);
                                Ok(())
                            }
                        };

                        if let Err(e) = result {
                            tracing::error!("Error handling command {}: {}", command.data.name, e);

                            // Try to send an error message to the user
                            let error_response = CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("An error occurred: {}", e))
                                    .ephemeral(true),
                            );

                            if let Err(respond_err) =
                                command.create_response(&ctx.http, error_response).await
                            {
                                tracing::error!("Failed to send error response: {}", respond_err);
                            }
                        }
                    }
                    Interaction::Component(component) => {
                        let result =
                            commands::setuproles::handle_component(ctx, component, &self.state)
                                .await;

                        if let Err(e) = result {
                            tracing::error!("Error handling component interaction: {}", e);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

pub async fn run(
    state: Arc<AppState>,
    mut verification_rx: mpsc::UnboundedReceiver<VerificationComplete>,
    mut reverify_rx: mpsc::UnboundedReceiver<ReverifyJob>,
) -> Result<(), Error> {
    let token = state.config.discord_token.clone().parse()?;
    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS;

    // Clone state for completion handler
    let completion_state = state.clone();

    let mut client = Client::builder(token, intents)
        .event_handler(Arc::new(Handler {
            state: state.clone(),
        }))
        .await?;

    // Get the http client and cache before starting
    let http = client.http.clone();
    let cache = client.cache.clone();

    // Spawn task to handle verification completions
    tokio::spawn(async move {
        while let Some(completion) = verification_rx.recv().await {
            tracing::info!(
                "Processing verification completion for Discord user {} in guild {}",
                completion.discord_user_id,
                completion.guild_id
            );

            if let Err(e) = commands::verify::complete_verification(
                &http,
                &cache,
                &completion_state,
                completion.discord_user_id,
                completion.guild_id.get(),
                completion.keycloak_user_id,
                true, // send DM — this is a direct user action
            )
            .await
            {
                tracing::error!("Failed to complete verification: {}", e);

                // Send error message to user via DM
                let user_id = completion.discord_user_id;
                let error_message = format!(
                    "Verification failed: {}\n\nPlease contact a server administrator for assistance.",
                    e
                );

                if let Err(dm_err) = user_id
                    .direct_message(
                        &http,
                        serenity::all::CreateMessage::new().content(error_message),
                    )
                    .await
                {
                    tracing::error!("Failed to send error DM to user {}: {}", user_id, dm_err);
                }
            }
        }
    });

    // Spawn task to handle reverify batches
    let reverify_http = client.http.clone();
    let reverify_cache = client.cache.clone();
    let reverify_state = state.clone();
    tokio::spawn(async move {
        while let Some(job) = reverify_rx.recv().await {
            tracing::info!(
                "Processing reverify batch {}/{} for guild {} ({} users)",
                job.batch_index,
                job.total_batches,
                job.guild_id,
                job.users.len(),
            );

            // Process all users in the batch with a 500ms delay between each
            let results = stream::iter(&job.users)
                .then(|user| async {
                    let result = commands::verify::complete_verification(
                        &reverify_http,
                        &reverify_cache,
                        &reverify_state,
                        user.discord_user_id,
                        user.guild_id.get(),
                        user.keycloak_user_id.clone(),
                        false, // do not DM users during reverify to avoid spam
                    )
                    .await;
                    // Sleep between users to stay well under Discord's rate limit
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    result
                })
                .collect::<Vec<_>>()
                .await;

            let succeeded = results.iter().filter(|r| r.is_ok()).count();
            let failed = results.iter().filter(|r| r.is_err()).count();

            // Post progress update to log channel if configured
            if let Some(channel_id) = job.log_channel {
                let is_last_batch = job.batch_index == job.total_batches;
                let message = if is_last_batch {
                    format!(
                        "Reverification complete! Processed all **{}** users.\nSucceeded: {} | Failed: {}",
                        job.total_users, succeeded, failed
                    )
                } else {
                    format!(
                        "Reverify batch {}/{} done: {} succeeded, {} failed. ({} total users)",
                        job.batch_index, job.total_batches, succeeded, failed, job.total_users
                    )
                };

                if let Err(e) = reverify_http
                    .send_message(
                        channel_id.into(),
                        Vec::new(),
                        &CreateMessage::new().content(message),
                    )
                    .await
                {
                    tracing::warn!("Failed to send reverify progress to log channel: {}", e);
                }

                // Clear the in-progress flag once the last batch is done
                if is_last_batch {
                    reverify_state
                        .reverify_in_progress
                        .store(false, Ordering::SeqCst);
                }
            } else if job.batch_index == job.total_batches {
                // No log channel — still clear the flag when done
                reverify_state
                    .reverify_in_progress
                    .store(false, Ordering::SeqCst);
            }
        }
    });

    client.start().await?;

    Ok(())
}
