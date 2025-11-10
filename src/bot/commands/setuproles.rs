use crate::bot::Error;
use crate::state::AppState;
use redis::AsyncCommands;
use serenity::all::{
    ButtonStyle, CommandInteraction, ComponentInteraction, ComponentInteractionDataKind, Context,
    CreateActionRow, CreateButton, CreateCommand, CreateComponent, CreateContainer,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateSelectMenu,
    CreateSelectMenuKind, CreateSelectMenuOption, CreateTextDisplay, Mentionable, MessageFlags,
    RoleId,
};
use std::sync::Arc;

use super::utils::is_admin;

/// Register the setuproles command
pub fn register() -> CreateCommand<'static> {
    CreateCommand::new("setuproles")
        .description("Configure automatic role assignment based on user class and level")
}

/// Handle the setuproles command
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
                .content("You need administrator permissions to configure role assignment.")
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Get current configuration from Redis
    let mut conn = state.redis.clone();
    let role_mode_key = format!("guild:{}:role_mode", guild_id);
    let current_mode: Option<String> = conn.get(&role_mode_key).await?;
    let current_mode = current_mode.unwrap_or_else(|| "none".to_string());

    // Create the mode selection dropdown
    let mode_select = CreateSelectMenu::new(
        "role_mode_select",
        CreateSelectMenuKind::String {
            options: vec![
                CreateSelectMenuOption::new("None", "none")
                    .description("Only assign the verified role")
                    .default_selection(current_mode == "none"),
                CreateSelectMenuOption::new("Levels", "levels")
                    .description("Undergrad and Graduate (2 roles)")
                    .default_selection(current_mode == "levels"),
                CreateSelectMenuOption::new("Classes", "classes")
                    .description("First-Year through Doctoral (7 roles)")
                    .default_selection(current_mode == "classes"),
                CreateSelectMenuOption::new("Custom", "custom")
                    .description("Choose which levels and classes to assign")
                    .default_selection(current_mode == "custom"),
            ]
            .into(),
        },
    )
    .placeholder("Select role assignment mode");

    // Create components v2 message with heading and description in a container
    let container = CreateContainer::new(vec![
        CreateComponent::TextDisplay(CreateTextDisplay::new("# Role Config")),
        CreateComponent::TextDisplay(CreateTextDisplay::new(
            "Configure how roles are automatically assigned to users after verification.",
        )),
        CreateComponent::ActionRow(CreateActionRow::SelectMenu(mode_select)),
    ]);

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .components(vec![CreateComponent::Container(container)])
            .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
    );

    command.create_response(&ctx.http, response).await?;

    Ok(())
}

/// Handle component interactions for role mode selection and role creation
pub async fn handle_component(
    ctx: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
) -> Result<(), Error> {
    let custom_id = interaction.data.custom_id.as_str();

    if custom_id == "role_mode_select" {
        handle_mode_selection(ctx, interaction, state).await
    } else if custom_id == "parent_role_select" {
        handle_parent_role_selection(ctx, interaction, state).await
    } else if custom_id == "custom_roles_multiselect" {
        handle_custom_roles_selection(ctx, interaction, state).await
    } else if custom_id.starts_with("save_roles_button:") {
        handle_save_roles(ctx, interaction, state).await
    } else {
        Ok(())
    }
}

/// Handle mode selection and show role creation confirmation interface
async fn handle_mode_selection(
    ctx: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
) -> Result<(), Error> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Ok(()),
    };

    // Get the selected mode
    let selected_mode = match &interaction.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => {
            values.first().map(|s| s.as_str()).unwrap_or("none")
        }
        _ => "none",
    };

    // For "none" mode, just save it and show confirmation
    if selected_mode == "none" {
        let mut conn = state.redis.clone();
        let role_mode_key = format!("guild:{}:role_mode", guild_id);
        redis::cmd("SET")
            .arg(&role_mode_key)
            .arg("none")
            .query_async::<()>(&mut conn)
            .await?;

        let container = CreateContainer::new(vec![
            CreateComponent::TextDisplay(CreateTextDisplay::new("# Mode Updated")),
            CreateComponent::TextDisplay(CreateTextDisplay::new(
                "Role assignment mode has been set to **None**.\n\n\
                Only the verified role will be assigned to users after verification.",
            )),
        ]);

        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .components(vec![CreateComponent::Container(container)])
                .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
        );

        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Create a new session for this mode
    {
        let mut sessions = state.setuproles_sessions.write().await;
        sessions.insert(
            (guild_id, interaction.user.id),
            crate::state::SetupRolesSession::new(selected_mode.to_string()),
        );
    }

    // Get all guild roles and find bot's highest role for filtering
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let bot_user_id = ctx.cache.current_user().id;
    let bot_member = guild.id.member(&ctx.http, bot_user_id).await?;
    let bot_highest_position = bot_member
        .roles
        .iter()
        .filter_map(|role_id| guild.roles.get(role_id))
        .map(|role| role.position)
        .max()
        .unwrap_or(0);

    // Filter roles to those below bot's highest role
    let everyone_role_id = RoleId::from(guild_id.get());
    let mut available_roles: Vec<_> = guild
        .roles
        .iter()
        .filter(|role| {
            role.position < bot_highest_position && !role.managed() && role.id != everyone_role_id // Exclude @everyone
        })
        .collect();

    // Sort by position (highest first)
    available_roles.sort_by(|a, b| b.position.cmp(&a.position));

    // Create role select menu
    let role_select = CreateSelectMenu::new(
        "parent_role_select",
        CreateSelectMenuKind::Role {
            default_roles: None,
        },
    )
    .placeholder("Select role to create new roles under");

    // Determine what roles will be created
    let (mode_name, mode_description, roles_to_create) = match selected_mode {
        "levels" => (
            "Levels Mode",
            "The following roles will be created under your selected role:\n\n",
            vec!["Undergrad", "Graduate"],
        ),
        "classes" => (
            "Classes Mode",
            "The following roles will be created under your selected role:\n\n",
            vec![
                "First-Year",
                "Sophomore",
                "Junior",
                "Senior",
                "Fifth-Year Senior",
                "Masters",
                "Doctoral",
            ],
        ),
        "custom" => {
            // For custom mode, show a multiselect instead
            return handle_custom_mode_selection(ctx, interaction, available_roles).await;
        }
        _ => return Ok(()),
    };

    // Create roles list text
    let roles_list = roles_to_create
        .iter()
        .map(|name| format!("* {}", name))
        .collect::<Vec<_>>()
        .join("\n");

    // Create save button with mode data embedded
    let save_button = CreateButton::new(format!("save_roles_button:{}", selected_mode))
        .label("Save")
        .style(ButtonStyle::Primary);

    let container = CreateContainer::new(vec![
        CreateComponent::TextDisplay(CreateTextDisplay::new(format!("# {}", mode_name))),
        CreateComponent::TextDisplay(CreateTextDisplay::new(mode_description)),
        CreateComponent::TextDisplay(CreateTextDisplay::new(roles_list)),
        CreateComponent::ActionRow(CreateActionRow::SelectMenu(role_select)),
        CreateComponent::ActionRow(CreateActionRow::Buttons(vec![save_button].into())),
    ]);

    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .components(vec![CreateComponent::Container(container)])
            .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handle custom mode selection and show multiselect for role choices
async fn handle_custom_mode_selection(
    ctx: &Context,
    interaction: &ComponentInteraction,
    _available_roles: Vec<&serenity::all::Role>,
) -> Result<(), Error> {
    // Create multiselect for custom role selection
    let all_possible_roles = vec![
        ("Undergrad", "level:undergrad"),
        ("Graduate", "level:graduate"),
        ("First-Year", "class:first-year"),
        ("Sophomore", "class:sophomore"),
        ("Junior", "class:junior"),
        ("Senior", "class:senior"),
        ("Fifth-Year Senior", "class:fifth-year"),
        ("Masters", "class:masters"),
        ("Doctoral", "class:doctoral"),
    ];

    let role_options: Vec<_> = all_possible_roles
        .into_iter()
        .map(|(name, value)| CreateSelectMenuOption::new(name, value))
        .collect();

    let custom_role_select = CreateSelectMenu::new(
        "custom_roles_multiselect",
        CreateSelectMenuKind::String {
            options: role_options.into(),
        },
    )
    .min_values(1)
    .max_values(9)
    .placeholder("Select which roles to create");

    let role_select = CreateSelectMenu::new(
        "parent_role_select",
        CreateSelectMenuKind::Role {
            default_roles: None,
        },
    )
    .placeholder("Select role to create new roles under");

    let save_button = CreateButton::new("save_roles_button:custom")
        .label("Save")
        .style(ButtonStyle::Primary);

    let container = CreateContainer::new(vec![
        CreateComponent::TextDisplay(CreateTextDisplay::new("# Custom Mode")),
        CreateComponent::TextDisplay(CreateTextDisplay::new(
            "Select any combination of level-based and class-based roles.",
        )),
        CreateComponent::ActionRow(CreateActionRow::SelectMenu(role_select)),
        CreateComponent::ActionRow(CreateActionRow::SelectMenu(custom_role_select)),
        CreateComponent::ActionRow(CreateActionRow::Buttons(vec![save_button].into())),
    ]);

    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .components(vec![CreateComponent::Container(container)])
            .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handle parent role selection and store it in the session
async fn handle_parent_role_selection(
    ctx: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
) -> Result<(), Error> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Ok(()),
    };

    // Get selected role
    let selected_role = match &interaction.data.kind {
        ComponentInteractionDataKind::RoleSelect { values } => values.first().copied(),
        _ => None,
    };

    if let Some(role_id) = selected_role {
        // Update the session with the parent role
        let mut sessions = state.setuproles_sessions.write().await;
        if let Some(session) = sessions.get_mut(&(guild_id, interaction.user.id)) {
            session.set_parent_role(role_id);
        }

        // Acknowledge without updating message
        interaction
            .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
            .await?;
    }

    Ok(())
}

/// Handle custom roles multiselect and store selections in the session
async fn handle_custom_roles_selection(
    ctx: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
) -> Result<(), Error> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Ok(()),
    };

    // Get selected role types
    let selected_roles = match &interaction.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values.to_vec(),
        _ => vec![],
    };

    // Update the session with the custom roles selection
    let mut sessions = state.setuproles_sessions.write().await;
    if let Some(session) = sessions.get_mut(&(guild_id, interaction.user.id)) {
        session.set_custom_roles(selected_roles);
    }

    // Acknowledge without updating message
    interaction
        .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
        .await?;

    Ok(())
}

/// Handle save button, which creates the roles and saves configuration
async fn handle_save_roles(
    ctx: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
) -> Result<(), Error> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Ok(()),
    };

    // Get the session data
    let session = {
        let sessions = state.setuproles_sessions.read().await;
        sessions.get(&(guild_id, interaction.user.id)).cloned()
    };

    let session = match session {
        Some(s) => s,
        None => {
            // No session found, shouldn't get here
            let container =
                CreateContainer::new(vec![CreateComponent::TextDisplay(CreateTextDisplay::new(
                    "# Error\n\nSession expired. Please run `/setuproles` again.",
                ))]);

            let response = CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .components(vec![CreateComponent::Container(container)])
                    .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
            );

            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    // Validate the session has all required data
    if let Err(error_msg) = session.validate() {
        let container = CreateContainer::new(vec![CreateComponent::TextDisplay(
            CreateTextDisplay::new(format!("# Error\n\n{}", error_msg)),
        )]);

        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .components(vec![CreateComponent::Container(container)])
                .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
        );

        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Create the roles
    let mut conn = state.redis.clone();
    let created_roles = match session
        .save_and_create_roles(&ctx.http, guild_id, &mut conn)
        .await
    {
        Ok(roles) => roles,
        Err(e) => {
            let container = CreateContainer::new(vec![CreateComponent::TextDisplay(
                CreateTextDisplay::new(format!("# Error\n\n{}", e)),
            )]);

            let response = CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .components(vec![CreateComponent::Container(container)])
                    .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
            );

            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    // Clean up the session from memory
    {
        let mut sessions = state.setuproles_sessions.write().await;
        sessions.remove(&(guild_id, interaction.user.id));
    }

    // Show success message
    let roles_list = created_roles
        .iter()
        .map(|(_, role_id)| format!("* {}", role_id.mention()))
        .collect::<Vec<_>>()
        .join("\n");

    let container = CreateContainer::new(vec![
        CreateComponent::TextDisplay(CreateTextDisplay::new("# Roles Created Successfully")),
        CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
            "The following roles have been created and configured:\n{}\n\
            Users will now automatically receive these roles when they verify.",
            roles_list
        ))),
    ]);

    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .components(vec![CreateComponent::Container(container)])
            .flags(MessageFlags::EPHEMERAL | MessageFlags::IS_COMPONENTS_V2),
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}
