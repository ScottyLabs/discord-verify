use crate::bot::{Context, Error};
use crate::state::PendingVerification;
use poise::serenity_prelude as serenity;
use uuid::Uuid;

use super::utils::get_verified_role_id;

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

    // Find and assign verified role (configured or default "Verified")
    let mut redis = state.redis.clone();
    let verified_role = get_verified_role_id(http, &mut redis, guild_id).await?;

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
