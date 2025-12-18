use crate::bot::Error;
use crate::state::AppState;
use redis::AsyncCommands;
use serenity::all::{
    CommandInteraction, Context, CreateCommand, CreateComponent, CreateContainer,
    CreateContainerComponent, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateSeparator, CreateTextDisplay, Mentionable, MessageFlags,
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

    // Load guild role configuration
    let mut conn = state.redis.clone();
    let guild_config = super::utils::load_guild_config(&ctx.http, &mut conn, guild_id).await?;

    // Get guild roles for verified and unverified lookups
    let roles = guild_id.roles(&ctx.http).await?;

    // Format verified role info
    let verified_role_info = match guild_config.verified_role {
        Some(role_id) => {
            if let Some(role) = roles.get(&role_id) {
                format!("{} (position: {})", role.mention(), role.position)
            } else {
                "Role deleted".to_string()
            }
        }
        None => "Not configured (use `/setverifiedrole`)".to_string(),
    };

    // Format unverified role info
    let unverified_redis_key = format!("guild:{}:role:unverified", guild_id);
    let unverified_role_info: String =
        if let Ok(Some(role_id_str)) = conn.get::<_, Option<String>>(&unverified_redis_key).await {
            if let Ok(role_id_u64) = role_id_str.parse::<u64>() {
                let role_id = serenity::all::RoleId::new(role_id_u64);
                if let Some(role) = roles.get(&role_id) {
                    format!("{} (position: {})", role.mention(), role.position)
                } else {
                    "Role deleted".to_string()
                }
            } else {
                "Not configured (use `/setunverifiedrole`)".to_string()
            }
        } else {
            "Not configured (use `/setunverifiedrole`)".to_string()
        };

    // Format log channel info
    let log_channel_info = match guild_config.log_channel {
        Some(channel_id) => {
            // Check if channel still exists
            if ctx.http.get_channel(channel_id.into()).await.is_ok() {
                format!("<#{}>", channel_id)
            } else {
                "Channel deleted".to_string()
            }
        }
        None => "Not configured (use `/setlogchannel`)".to_string(),
    };

    // Format mode description
    let mode_description = match guild_config.mode {
        crate::bot::guild_config::RoleMode::Levels => {
            "* **Levels Mode** (assigning roles based on Undergrad/Graduate status)"
        }
        crate::bot::guild_config::RoleMode::Classes => {
            "* **Classes Mode** (assigning roles based on class year, First-Year through Doctoral)"
        }
        crate::bot::guild_config::RoleMode::Custom => {
            "* **Custom Mode** (assigning roles based on selected levels and classes)"
        }
        crate::bot::guild_config::RoleMode::None => {
            "* **None** (only the verified role is being assigned)"
        }
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

    // Create components v2 message
    let container = CreateContainer::new(vec![
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new("# Configuration")),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
            "Current verification settings for this server:\n{}\n* **Verified Role:** {}\n* **Unverified Role:** {}\n* **Log Channel:** {}",
            mode_description, verified_role_info, unverified_role_info, log_channel_info
        ))),
        CreateContainerComponent::Separator(CreateSeparator::new(true)),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
            "Verified Users: {}/{} (total includes bots)\n{}",
            verified_count, total_members, progress_bar
        ))),
        CreateContainerComponent::Separator(CreateSeparator::new(true)),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
            "{} users still need to verify • Use `/setuproles` to change mode",
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
