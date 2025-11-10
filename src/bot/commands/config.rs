use crate::bot::Error;
use crate::state::AppState;
use redis::AsyncCommands;
use serenity::all::{
    Colour, CommandInteraction, Context, CreateCommand, CreateEmbed, CreateEmbedFooter,
    CreateInteractionResponse, CreateInteractionResponseMessage, Mentionable, RoleId,
};
use std::sync::Arc;

use super::utils::is_admin;

/// Generate ASCII progress bar
fn generate_progress_bar(current: usize, total: usize, width: usize) -> String {
    if total == 0 {
        return format!("[{}] 0%", " ".repeat(width));
    }

    let percentage = (current as f64 / total as f64 * 100.0).round() as usize;
    let filled = (current as f64 / total as f64 * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    format!(
        "[{}{}] {}%",
        "█".repeat(filled),
        "░".repeat(empty),
        percentage
    )
}

/// Register the config command
pub fn register() -> CreateCommand<'static> {
    CreateCommand::new("config")
        .description("Show server verification configuration and statistics")
}

/// Handle the config command
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
                .content("You need administrator permissions to view server configuration.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Get configured verified role
    let mut conn = state.redis.clone();
    let redis_key = format!("guild:{}:role:verified", guild_id);
    let role_id_str: Option<String> = conn.get(&redis_key).await?;

    let role_info = match role_id_str {
        Some(id_str) => {
            if let Ok(role_id_u64) = id_str.parse::<u64>() {
                let role_id = RoleId::new(role_id_u64);
                let roles = guild_id.roles(&ctx.http).await?;

                if let Some(role) = roles.get(&role_id) {
                    format!("{} (position: {})", role.mention(), role.position)
                } else {
                    "Role deleted".to_string()
                }
            } else {
                "Invalid configuration".to_string()
            }
        }
        None => "Not configured (use /setverifiedrole)".to_string(),
    };

    // Count verified users
    let pattern = "discord:*:keycloak".to_string();
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut conn)
        .await?;
    let verified_count = keys.len();

    // Get total member count from cache
    let total_members = {
        let guild_cache = guild_id
            .to_guild_cached(&ctx.cache)
            .ok_or("Guild not found in cache")?;
        guild_cache.member_count as usize
    };

    let progress_bar = generate_progress_bar(verified_count, total_members, 20);

    let embed = CreateEmbed::new()
        .title("Server Configuration")
        .field("Verified Role", role_info, false)
        .field(
            "Statistics",
            format!(
                "Verified Users: {}/{} (total includes bots)\n{}",
                verified_count, total_members, progress_bar
            ),
            false,
        )
        .colour(Colour::BLUE)
        .footer(CreateEmbedFooter::new(format!(
            "{} users still need to verify",
            total_members.saturating_sub(verified_count)
        )));

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .embed(embed)
            .ephemeral(true),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
