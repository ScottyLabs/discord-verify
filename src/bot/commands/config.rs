use crate::bot::Error;
use crate::state::AppState;
use redis::AsyncCommands;
use serenity::all::{
    CommandInteraction, Context, CreateCommand, CreateComponent, CreateContainer,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateTextDisplay, Mentionable,
    MessageFlags, RoleId,
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
        None => "Not configured (use `/setverifiedrole`)".to_string(),
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

    // Get role mode
    let role_mode_key = format!("guild:{}:role_mode", guild_id);
    let role_mode: Option<String> = conn.get(&role_mode_key).await?;
    let role_mode = role_mode.unwrap_or_else(|| "none".to_string());

    let mode_description = match role_mode.as_str() {
        "levels" => "* **Levels Mode** (assigning roles based on Undergrad/Graduate status)",
        "classes" => {
            "* **Classes Mode** (assigning roles based on class year, First-Year through Doctoral)"
        }
        "custom" => "* **Custom Mode** (assigning roles based on selected levels and classes)",
        _ => "* **None** (only the verified role is being assigned)",
    };

    // Create components v2 message
    let container = CreateContainer::new(vec![
        CreateComponent::TextDisplay(CreateTextDisplay::new("# Configuration")),
        CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
            "Current verification settings for this server:\n{}\n* **Verified Role:** {}",
            mode_description, role_info
        ))),
        CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
            "Verified Users: {}/{} (total includes bots)\n{}\n\n\
            {} users still need to verify • Use `/setuproles` to change mode",
            verified_count,
            total_members,
            progress_bar,
            total_members.saturating_sub(verified_count)
        ))),
    ]);

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .components(vec![CreateComponent::Container(container)])
            .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
