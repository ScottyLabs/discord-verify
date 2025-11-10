use crate::bot::Error;
use crate::state::AppState;
use serenity::all::{
    CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseMessage, Mentionable, ResolvedOption,
    ResolvedValue,
};
use std::sync::Arc;

use super::utils::is_admin;

/// Register the setverifiedrole command
pub fn register() -> CreateCommand<'static> {
    CreateCommand::new("setverifiedrole")
        .description("Set the verified role for this server")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Role,
                "role",
                "The role to assign when users verify",
            )
            .required(true),
        )
}

/// Handle the setverifiedrole command
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
                .content("You need administrator permissions to configure the verified role.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Get the role from command options
    let options = command.data.options();
    let role = match options.first() {
        Some(ResolvedOption {
            value: ResolvedValue::Role(r),
            ..
        }) => r,
        _ => {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("Role parameter is required.")
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    // Check if bot can actually assign this role
    let bot_user_id = ctx.cache.current_user().id;
    let bot_member = guild_id.member(&ctx.http, bot_user_id).await?;
    let guild_roles = guild_id.roles(&ctx.http).await?;

    // Find bot's highest role position
    let bot_top_role = bot_member
        .roles
        .iter()
        .filter_map(|role_id| guild_roles.get(role_id))
        .max_by_key(|role| role.position);

    let bot_position = bot_top_role.map(|r| r.position).unwrap_or(0);
    let target_role_position = guild_roles.get(&role.id).map(|r| r.position).unwrap_or(0);

    if bot_position <= target_role_position {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content(format!(
                    "I cannot assign {}. My highest role is at position {}, but this role is at position {}.\n\
                    Please move my role higher than {} in the server settings.",
                    role.mention(),
                    bot_position,
                    target_role_position,
                    role.mention()
                ))
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Store the role ID in Redis
    let mut conn = state.redis.clone();
    let redis_key = format!("guild:{}:role:verified", guild_id);

    redis::cmd("SET")
        .arg(&redis_key)
        .arg(role.id.to_string())
        .query_async::<()>(&mut conn)
        .await?;

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(format!(
                "Verified role has been set to {}. Users who verify will now receive this role.",
                role.mention()
            ))
            .ephemeral(true),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
