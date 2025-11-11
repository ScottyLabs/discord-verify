use crate::bot::Error;
use crate::state::AppState;
use serenity::all::{
    CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseMessage, Mentionable, ResolvedOption,
    ResolvedValue,
};
use std::sync::Arc;

use super::utils::is_admin;

/// Register the setlogchannel command
pub fn register() -> CreateCommand<'static> {
    CreateCommand::new("setlogchannel")
        .description("Set the logging channel for verification events")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "channel",
                "The channel where verification logs will be sent",
            )
            .required(true),
        )
}

/// Handle the setlogchannel command
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
                .content("You need administrator permissions to configure the log channel.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Get the channel from command options
    let options = command.data.options();
    let channel = match options.first() {
        Some(ResolvedOption {
            value: ResolvedValue::Channel(c),
            ..
        }) => c,
        _ => {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("Channel parameter is required.")
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    let channel_id = channel.id();

    // Validate the channel exists and we can send messages to it
    let full_channel = match ctx.http.get_channel(channel_id).await {
        Ok(ch) => ch,
        Err(_) => {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("Unable to access that channel. Please make sure the bot has permission to view it.")
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    // Check if it's a guild text channel
    if !matches!(
        full_channel,
        serenity::all::Channel::Guild(ref gc) if matches!(
                        gc.base.kind,
            serenity::all::ChannelType::Text | serenity::all::ChannelType::News
        )
    ) {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("The log channel must be a text or news channel.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Check if the bot has permission to send messages in the channel
    if let serenity::all::Channel::Guild(gc) = &full_channel {
        let bot_user_id = ctx.cache.current_user().id;
        let bot_member = guild_id.member(&ctx.http, bot_user_id).await?;

        // Get bot permissions in this channel (scope to drop guild reference before await)
        let has_permission = {
            let guild = guild_id
                .to_guild_cached(&ctx.cache)
                .ok_or("Guild not in cache")?;

            let permissions = guild.user_permissions_in(gc, &bot_member);
            permissions.send_messages()
        };

        if !has_permission {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(format!(
                        "I don't have permission to send messages in {}. Please update my permissions for that channel.",
                        channel_id.mention()
                    ))
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    }

    // Store the channel ID in Redis
    let mut conn = state.redis.clone();
    let redis_key = format!("guild:{}:log_channel", guild_id);

    redis::cmd("SET")
        .arg(&redis_key)
        .arg(channel_id.to_string())
        .query_async::<()>(&mut conn)
        .await?;

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(format!(
                "Log channel has been set to {}. Verification and unverification events will be logged there.",
                channel_id.mention()
            ))
            .ephemeral(true),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
