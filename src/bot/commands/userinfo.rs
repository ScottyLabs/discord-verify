use crate::bot::Error;
use crate::state::AppState;
use redis::AsyncCommands;
use serenity::all::{
    Colour, CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, Mentionable,
    ResolvedOption, ResolvedValue,
};
use std::sync::Arc;

/// Register the userinfo command
pub fn register() -> CreateCommand<'static> {
    CreateCommand::new("userinfo")
        .description("Display user information for a verified Discord user")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::User,
                "user",
                "User to get info for (defaults to you)",
            )
            .required(false),
        )
}

/// Handle the userinfo command
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

    // Look up Keycloak user ID from Redis
    let mut conn = state.redis.clone();
    let redis_key = format!("discord:{}:keycloak", target_user.id);

    let keycloak_user_id: Option<String> = conn.get(&redis_key).await?;
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

    // Fetch user info from Keycloak
    let keycloak_user = match state.keycloak.get_user(&keycloak_user_id).await {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("Failed to fetch Keycloak user info: {}", e);
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("Failed to fetch user information from Keycloak.")
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
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

    let embed = CreateEmbed::new()
        .title(format!("User Information for {}", target_user.name))
        .field("Andrew ID", username, false)
        .field("Full Name", full_name, false)
        .field("Email", email, false)
        .colour(Colour::BLUE);

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .embed(embed)
            .ephemeral(true),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
