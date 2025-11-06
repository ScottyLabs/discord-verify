use crate::bot::{Context, Error};
use crate::state::PendingVerification;
use poise::serenity_prelude::{self as serenity, Mentionable};
use redis::AsyncCommands;
use uuid::Uuid;

#[poise::command(slash_command)]
pub async fn verify(ctx: Context<'_>) -> Result<(), Error> {
    let state = ctx.data();
    let user = ctx.author();

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

    // Generate unique state token
    let state_token = Uuid::new_v4();

    // Store verification request
    let verification = PendingVerification {
        discord_user_id: user.id,
        discord_username: user.name.clone(),
        guild_id,
        created_at: chrono::Utc::now().timestamp(),
    };

    state
        .pending_verifications
        .write()
        .await
        .insert(state_token.to_string(), verification.clone());

    // Also store in Redis with TTL
    let mut conn = state.redis.clone();
    let key = format!("verify:{}", state_token);
    let data = serde_json::to_string(&verification)?;

    redis::cmd("SETEX")
        .arg(&key)
        .arg(600) // 10 minutes
        .arg(&data)
        .query_async::<()>(&mut conn)
        .await?;

    // Create verification link
    let verify_url = format!("{}/verify?state={}", state.config.app_url, state_token);

    // Send ephemeral message
    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "Click the link below to verify your account. This link expires in 10 minutes.\n\n{}",
                verify_url
            ))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Complete the verification process by assigning role and storing mappings
/// Called by the bot task when it receives a verification completion event
pub async fn complete_verification(
    http: &serenity::Http,
    _cache: &serenity::Cache,
    state: &crate::state::AppState,
    discord_user_id: serenity::UserId,
    guild_id: u64,
    keycloak_user_id: String,
) -> Result<(), Error> {
    let guild_id = serenity::GuildId::new(guild_id);
    let member = guild_id.member(http, discord_user_id).await?;

    // Find and assign "Verified" role
    let roles = guild_id.roles(http).await?;
    let verified_role = roles
        .values()
        .find(|r| r.name == "Verified")
        .ok_or("Verified role not found")?
        .id;

    member.add_role(http, verified_role).await?;

    // Store mapping in Redis
    let mut conn = state.redis.clone();
    redis::cmd("SET")
        .arg(format!("discord:{}:keycloak", discord_user_id))
        .arg(&keycloak_user_id)
        .query_async::<()>(&mut conn)
        .await?;

    redis::cmd("SET")
        .arg(format!("keycloak:{}:discord", keycloak_user_id))
        .arg(discord_user_id.to_string())
        .query_async::<()>(&mut conn)
        .await?;

    // DM the user
    let dm_channel = discord_user_id.create_dm_channel(http).await?;
    dm_channel
        .send_message(
            http,
            serenity::CreateMessage::default()
                .content("You have successfully verified your Andrew ID."),
        )
        .await?;

    Ok(())
}

/// Remove verification
#[poise::command(slash_command)]
pub async fn unverify(
    ctx: Context<'_>,
    #[description = "User to unverify (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let state = ctx.data();
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());

    // If targeting another user, require administrator permissions
    if target_user.id != ctx.author().id {
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

        let member = guild_id.member(&ctx.http(), ctx.author().id).await?;
        let guild_roles = guild_id.roles(&ctx.http()).await?;

        // Check if any of the member's roles have administrator permission
        let is_admin = member.roles.iter().any(|role_id| {
            guild_roles
                .get(role_id)
                .map(|role| role.permissions.administrator())
                .unwrap_or(false)
        });

        if !is_admin {
            ctx.send(
                poise::CreateReply::default()
                    .content("You need administrator permissions to unverify other users.")
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    }

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

    // Look up Keycloak user ID from Redis to clean up mappings
    let mut conn = state.redis.clone();
    let redis_key = format!("discord:{}:keycloak", target_user.id);
    let keycloak_user_id: Option<String> = conn.get(&redis_key).await?;

    // Remove Redis mappings
    if let Some(keycloak_id) = keycloak_user_id {
        redis::cmd("DEL")
            .arg(format!("keycloak:{}:discord", keycloak_id))
            .query_async::<()>(&mut conn)
            .await?;
    }

    redis::cmd("DEL")
        .arg(&redis_key)
        .query_async::<()>(&mut conn)
        .await?;

    // Remove "Verified" role
    let member = guild_id.member(&ctx.http(), target_user.id).await?;
    let roles = guild_id.roles(&ctx.http()).await?;

    if let Some(verified_role) = roles.values().find(|r| r.name == "Verified") {
        if member.roles.contains(&verified_role.id) {
            member.remove_role(&ctx.http(), verified_role.id).await?;
        }
    }

    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "Removed verification for {}.",
                target_user.mention()
            ))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

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
                        "{} has not verified their account.",
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
