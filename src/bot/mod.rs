mod commands;

use crate::state::{AppState, VerificationComplete};
use serenity::Client;
use serenity::all::{
    Context, CreateInteractionResponse, CreateInteractionResponseMessage, EventHandler,
    GatewayIntents, Interaction,
};
use serenity::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

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

                // Register global slash commands
                if let Err(e) = commands::register_commands(ctx).await {
                    tracing::error!("Failed to register commands: {}", e);
                } else {
                    tracing::info!("Successfully registered slash commands");
                }
            }
            serenity::all::FullEvent::InteractionCreate { interaction, .. } => {
                if let Interaction::Command(command) = interaction {
                    let result = match command.data.name.as_str() {
                        "verify" => commands::verify::handle(ctx, command, &self.state).await,
                        "unverify" => commands::unverify::handle(ctx, command, &self.state).await,
                        "userinfo" => commands::userinfo::handle(ctx, command, &self.state).await,
                        "setverifiedrole" => {
                            commands::setverifiedrole::handle(ctx, command, &self.state).await
                        }
                        "config" => commands::config::handle(ctx, command, &self.state).await,
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
            }
            _ => {}
        }
    }
}

pub async fn run(
    state: Arc<AppState>,
    mut verification_rx: mpsc::UnboundedReceiver<VerificationComplete>,
) -> Result<(), Error> {
    let token = state.config.discord_token.clone().parse()?;
    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS;

    // Clone state for completion handler
    let completion_state = state.clone();

    let mut client = Client::builder(token, intents)
        .event_handler(Handler {
            state: state.clone(),
        })
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

    client.start().await?;

    Ok(())
}
