mod commands;

use crate::state::{AppState, VerificationComplete};
use poise::serenity_prelude as serenity;
use std::sync::Arc;
use tokio::sync::mpsc;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Arc<AppState>, Error>;

pub async fn run(
    state: Arc<AppState>,
    mut verification_rx: mpsc::UnboundedReceiver<VerificationComplete>,
) -> Result<(), Error> {
    let token = state.config.discord_token.clone();
    let intents = serenity::GatewayIntents::GUILDS | serenity::GatewayIntents::GUILD_MEMBERS;

    // Clone state for setup closure and completion handler
    let setup_state = state.clone();
    let completion_state = state.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::verify(),
                commands::unverify(),
                commands::userinfo(),
                commands::setverifiedrole(),
            ],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(setup_state)
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
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

            if let Err(e) = commands::complete_verification(
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
                if let Ok(dm_channel) = user_id.create_dm_channel(&http).await {
                    let error_message = format!(
                        "Verification failed: {}\n\nPlease contact a server administrator for assistance.",
                        e
                    );

                    if let Err(dm_err) = dm_channel
                        .send_message(
                            &http,
                            serenity::CreateMessage::default().content(error_message),
                        )
                        .await
                    {
                        tracing::error!("Failed to send error DM to user {}: {}", user_id, dm_err);
                    }
                } else {
                    tracing::error!("Failed to create DM channel with user {}", user_id);
                }
            }
        }
    });

    client.start().await?;

    Ok(())
}
