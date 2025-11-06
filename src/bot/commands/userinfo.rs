use crate::bot::{Context, Error};
use poise::serenity_prelude::{self as serenity, Mentionable};
use redis::AsyncCommands;

/// Display Keycloak user information for a verified Discord user
#[poise::command(slash_command)]
pub async fn userinfo(
    ctx: Context<'_>,
    #[description = "User to get info for (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let state = ctx.data();
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());

    // Look up Keycloak user ID from Redis
    let mut conn = state.redis.clone();
    let redis_key = format!("discord:{}:keycloak", target_user.id);

    let keycloak_user_id: Option<String> = conn.get(&redis_key).await?;
    let keycloak_user_id = match keycloak_user_id {
        Some(id) => id,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!(
                        "{} is not verified.",
                        target_user.mention()
                    ))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Fetch user info from Keycloak
    let keycloak_user = match state.keycloak.get_user(&keycloak_user_id).await {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("Failed to fetch Keycloak user info: {}", e);
            ctx.send(
                poise::CreateReply::default()
                    .content("Failed to fetch user information from Keycloak.")
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Build response with user information
    let username = keycloak_user
        .username
        .unwrap_or_else(|| "Unknown".to_string());
    let email = keycloak_user
        .email
        .unwrap_or_else(|| "Not provided".to_string());
    let first_name = keycloak_user.first_name.unwrap_or_else(|| "".to_string());
    let last_name = keycloak_user.last_name.unwrap_or_else(|| "".to_string());
    let full_name = if !first_name.is_empty() || !last_name.is_empty() {
        format!("{} {}", first_name, last_name).trim().to_string()
    } else {
        "Not provided".to_string()
    };

    ctx.send(
        poise::CreateReply::default()
            .embed(
                serenity::CreateEmbed::new()
                    .title(format!("User Information for {}", target_user.name))
                    .field("Discord User", target_user.mention().to_string(), true)
                    .field("Andrew ID", username, true)
                    .field("Full Name", full_name, false)
                    .field("Email", email, false)
                    .color(serenity::Color::BLUE),
            )
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
