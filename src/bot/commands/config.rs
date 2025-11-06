use crate::bot::{Context, Error};
use poise::serenity_prelude::{self as serenity, Mentionable};
use redis::AsyncCommands;

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

/// Show server verification configuration and statistics
#[poise::command(slash_command)]
pub async fn config(ctx: Context<'_>) -> Result<(), Error> {
    let state = ctx.data();

    // Get guild_id from context
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content("This command can only be used in a server.")
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Check if user has administrator permissions
    if !is_admin(&ctx, guild_id, ctx.author().id).await? {
        ctx.send(
            poise::CreateReply::default()
                .content("You need administrator permissions to view server configuration.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    // Get configured verified role
    let mut conn = state.redis.clone();
    let redis_key = format!("guild:{}:verified_role", guild_id);
    let role_id_str: Option<String> = conn.get(&redis_key).await?;

    let role_info = match role_id_str {
        Some(id_str) => {
            if let Ok(role_id_u64) = id_str.parse::<u64>() {
                let role_id = serenity::RoleId::new(role_id_u64);
                let roles = guild_id.roles(&ctx.http()).await?;

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
    let pattern = format!("discord:*:keycloak");
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut conn)
        .await?;
    let verified_count = keys.len();

    // Get total member count from cache
    let total_members = {
        let cache = ctx.cache();
        let guild_cache = guild_id
            .to_guild_cached(&cache)
            .ok_or("Guild not found in cache")?;
        guild_cache.member_count as usize
    };

    let progress_bar = generate_progress_bar(verified_count, total_members, 20);

    let embed = serenity::CreateEmbed::new()
        .title("Server Configuration")
        .field("Verified Role", role_info, false)
        .field(
            "Verification Statistics",
            format!(
                "Verified Users: {}/{}\n{}",
                verified_count, total_members, progress_bar
            ),
            false,
        )
        .color(serenity::Color::BLUE)
        .footer(serenity::CreateEmbedFooter::new(format!(
            "{} users still need to verify",
            total_members.saturating_sub(verified_count)
        )));

    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true))
        .await?;

    Ok(())
}
