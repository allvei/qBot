
use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::{anyhow, Result};
use serenity::all::{
    ChannelId as CI, ChannelType, ComponentInteraction, ComponentInteractionDataKind,
    Context, CreateActionRow, CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, CreateMessage, CreateSelectMenu,
    CreateSelectMenuKind, CreateSelectMenuOption, GuildId, PartialGuild, RoleId, UserId,
};
use tracing::{error, info, warn};

use crate::DEFAULT_QUOTA;
use crate::handlers::player::{check_role, create_rank_roles, validate_rank_roles, validate_system_roles};
use crate::models::{CommandContext as CC, Role, Server, SETUP_STATE};

/// `/config`
///
/// * `key`   - The key to modify.
/// * `value` - The value to set for the key.
pub async fn cmd_config(cc: &CC<'_>, key: String, value: Option<String,>,) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only game admins can modify the config!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    if let Some(val,) = value {
        cc.db.get_config(cc.intax.guild_id.expect("Guild ID not found").get()).await?;

        let embed = CE::new().title("Config Updated").description(format!("Set `{}` = `{}`", key, val));

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        let config = match cc.db.get_config(cc.intax.guild_id.expect("Guild ID not found").get()).await {
            Ok(cfg) => cfg,
            Err(e) => {
                let err_embed = CE::new().title("Failed to Load Config").description(format!("Error: {e}\nPlease create a config using `/config`."));
                let response = CIR::Message(CIRM::new().embed(err_embed).ephemeral(true));
                cc.intax.create_response(&cc.ctx.http, response).await?;
                return Ok(());
            }
        };

        let config_text = format!(
            "**Current Configuration:**\n\
                                  guild: `{}`\n\
                                  roles: `{}`\n\
                                  groups: `{}`",
        config.guild_id, config.roles.runner, config.roles.admin
        );

        let embed = CE::new().title("Bot Configuration").description(config_text);

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;
    }

    Ok(())
}

/// `/roles`
///
/// Manage runner and admin roles for the guild
/// * `role_type` - The role type to manage ("runner" or "admin")
/// * `role` - The Discord role mention/ID to assign
pub async fn cmd_roles(cc: &CC<'_>, role_type: String, role: Option<String>) -> Result<()> {
    // Check admin permissions
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can manage roles!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found").get();

    // If no parameters, show current role configuration
    if role_type.is_empty() && role.is_none() {
        let runner_role = cc.db.config.get_config_value("runner_role", guild_id).await?;
        let admin_role = cc.db.config.get_config_value("admin_role", guild_id).await?;

        let role_text = format!(
            "**Current Role Configuration:**\n\
             Runner Role: {}\n\
             Admin Role: {}",
            runner_role.map(|r| format!("<@&{}>", r)).unwrap_or_else(|| "Not set".to_string()),
            admin_role.map(|r| format!("<@&{}>", r)).unwrap_or_else(|| "Not set".to_string())
        );

        let embed = CE::new()
            .title("Role Configuration")
            .description(role_text);

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Validate role type
    let role_key = match role_type.to_lowercase().as_str() {
        "runner" => "runner_role",
        "admin" => "admin_role",
        "" => {
            let response = CIR::Message(CIRM::new()
                .content("Please specify role type: `runner` or `admin`")
                .ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
        _ => {
            let response = CIR::Message(CIRM::new()
                .content("Invalid role type. Use `runner` or `admin`")
                .ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // If role is provided, set it
    if let Some(role_value) = role {
        // Parse role ID from mention format <@&123456> or raw ID
        let role_id = if role_value.starts_with("<@&") && role_value.ends_with('>') {
            role_value[3..role_value.len()-1].to_string()
        } else {
            role_value
        };

        // Save to database
        cc.db.config.set_config(role_key, &role_id, guild_id).await?;

        let embed = CE::new()
            .title("Role Updated")
            .description(format!(
                "Set {} role to <@&{}>",
                role_type.to_lowercase(),
                role_id
            ))
            .color(0x00ff00);

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        // Show current value for this role type
        let current_role = cc.db.config.get_config_value(role_key, guild_id).await?;

        let embed = CE::new()
            .title(format!("{} Role", role_type))
            .description(format!(
                "Current {} role: {}",
                role_type.to_lowercase(),
                current_role.map(|r| format!("<@&{}>", r)).unwrap_or_else(|| "Not set".to_string())
            ));

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
    }

    Ok(())
}

/// `/cmd_init_group`
pub async fn cmd_init_group(cc: &CC<'_>, guild: &mut Server) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can set up the dashboard!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let user_id = cc.intax.user.id;
    let dashboard_channel = cc.intax.channel_id;

    // Initialize setup state with dashboard channel pre-filled
    SETUP_STATE.start_setup(user_id, guild_id);
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.dashboard_channel = Some(dashboard_channel.get());
    });

    // Start ephemeral setup flow
    start_init_group_flow(cc, dashboard_channel).await?;

    Ok(())
}

/// Starts the interactive init_group setup flow with ephemeral messages
async fn start_init_group_flow(cc: &CC<'_>, dashboard_channel: CI) -> Result<()> {
    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let user_id = cc.intax.user.id;
    let guild = guild_id.to_partial_guild(&cc.ctx.http).await?;

    // Step 1: Create dashboard message immediately with "loading..." placeholder
    let loading_embed = CE::new()
        .title("🎮 PUG Queue Dashboard")
        .description("⏳ Setting up queue system...")
        .color(0xffaa00);

    let dashboard_msg = match dashboard_channel.send_message(&cc.ctx.http,
        CreateMessage::new().embed(loading_embed)
    ).await {
        Ok(msg) => msg,
        Err(e) => {
            let error_response = CIR::Message(
                CIRM::new()
                    .content(format!("❌ Failed to create dashboard message: {}", e))
                    .ephemeral(true)
            );
            cc.intax.create_response(&cc.ctx.http, error_response).await?;
            return Ok(());
        }
    };

    // Store dashboard message ID in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.dashboard_msg_id = Some(dashboard_msg.id.get());
    });

    // Send welcome message with next step
    let welcome_embed = CE::new()
        .title("Dashboard Created")
        .description(format!(
            "Dashboard message created in <#{}>\n\n\
            Now let's configure the remaining channels for **{}**.\n\n\
            💡 **Make sure these channels exist:**\n\
            • Queue text channel (for commands)\n\
            • Queue voice channel (where players wait)\n\
            • Red team voice channel\n\
            • Blue team voice channel\n\n\
            **Step 2/5: Queue Text Channel**\n\
            Select the text channel where players will use queue commands:",
            dashboard_channel.get(), guild.name
        ))
        .color(0x00ff00);

    // Get text channels for dropdown
    let channels = get_text_channels(&guild, cc.ctx).await?;
    let channel_options = create_channel_options(&channels, "init_queue");

    let select_menu = CreateSelectMenu::new("init_queue", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select queue channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::Message(
        CIRM::new()
            .embed(welcome_embed)
            .components(vec![action_row])
            .ephemeral(true)
    );

    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// Parses a user mention to get the Discord ID.
///
/// * `mention` - The user mention to parse.
fn parse_user_mention(mention: &str,) -> Result<u64> {
    // Parse Discord user mention format: <@!123456789> or <@123456789>
    let mention =
        mention
            .trim();
    if mention.starts_with("<@!") && mention.ends_with('>') {
        mention[3..mention.len() - 1].to_string().parse::<u64>()
            .map_err(|_| anyhow!("Invalid user ID in mention"))
    } else if mention.starts_with("<@") && mention.ends_with('>') {
        mention[2..mention.len() - 1].to_string().parse::<u64>()
            .map_err(|_| anyhow!("Invalid user ID in mention"))
    } else {
        // Assume it's already a raw user ID
        mention.parse::<u64>()
            .map_err(|_| anyhow!("Invalid user ID format"))
    }
}

/// `/dashboard`
///
/// Creates or updates the dashboard in the current channel
pub async fn cmd_dashboard(cc: &CC<'_>, guild: &mut Server) -> Result<()> {
    info!("Processing dashboard command");

    // Check permissions - only runners/admins can create dashboard
    if !check_role(cc, &Role::Runner).await? && !check_role(cc, &Role::Admin).await? {
        cc.reply("Only runners and admins can create the dashboard!").await?;
        return Ok(());
    }

    let channel = cc.intax.channel_id;
    let group = guild.get_group(channel)?;

    // Create and send dashboard
    group.dash_publish(cc.ctx, channel).await?;

    cc.reply("Dashboard created/updated successfully!").await?;

    Ok(())
}

/// `/setup`
///
/// Sets up the bot for a guild using an interactive ephemeral message flow
pub async fn cmd_setup(cc: &CC<'_>) -> Result<()> {
    info!("Processing setup command");

    // Check permissions - only admins can run setup
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can run the setup command!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id: GuildId = cc.intax.guild_id.expect("Guild ID not found");
    let user_id:  UserId  = cc.intax.user.id;

    // Start the setup flow with ephemeral message
    start_setup_flow(cc, guild_id, user_id).await?;

    Ok(())
}

/// Starts the interactive setup flow with ephemeral messages
async fn start_setup_flow(cc: &CC<'_>, guild_id: GuildId, user_id: UserId) -> Result<()> {
    // Initialize setup state
    SETUP_STATE.start_setup(user_id, guild_id);

    // Get guild information
    let guild = guild_id.to_partial_guild(&cc.ctx.http).await?;

    // Send welcome message as ephemeral reply
    let welcome_embed = CE::new()
        .title("🛠️ Guild Setup Wizard")
        .description(format!(
            "Welcome to the setup wizard for **{}**!\n\n\
            I'll guide you through configuring the bot step by step.\n\
            You'll select channels and roles using dropdown menus.\n\n\
            💡 **Before continuing**, make sure you have created:\n\
            • A text channel for the dashboard\n\
            • A text channel for queue commands\n\
            • A voice channel for the queue\n\
            • Voice channels for Red and Blue teams\n\n\
            **Step 1/7: Dashboard Channel**\n\
            Select the channel where the queue dashboard will be displayed:",
            guild.name
        ))
        .color(0x00ff00);

    // Get text channels for dropdown
    let channels = get_text_channels(&guild, cc.ctx).await?;
    let channel_options = create_channel_options(&channels, "dashboard");

    let select_menu = CreateSelectMenu::new("setup_dashboard", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select dashboard channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::Message(
        CIRM::new()
            .embed(welcome_embed)
            .components(vec![action_row])
            .ephemeral(true)
    );

    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// Gets text channels from a guild
async fn get_text_channels(guild: &PartialGuild, ctx: &Context) -> Result<Vec<(CI, String)>> {
    let mut channels = Vec::new();

    let guild_channels = guild.channels(&ctx.http).await?;
    for (channel_id, channel) in guild_channels {
        if channel.kind == ChannelType::Text {
            channels.push((channel_id, channel.name.clone()));
        }
    }

    // Sort by name for better UX
    channels.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(channels)
}

/// Gets voice channels from a guild
async fn get_voice_channels(guild: &PartialGuild, ctx: &Context) -> Result<Vec<(CI, String)>> {
    let mut channels = Vec::new();

    let guild_channels = guild.channels(&ctx.http).await?;
    for (channel_id, channel) in guild_channels {
        if channel.kind == ChannelType::Voice {
            channels.push((channel_id, channel.name.clone()));
        }
    }

    // Sort by name for better UX
    channels.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(channels)
}

/// Gets roles from a guild (excluding @everyone)
async fn get_guild_roles(guild: &PartialGuild) -> Result<Vec<(RoleId, String)>> {
    let mut roles = Vec::new();

    for (role_id, role) in &guild.roles {
        // Skip @everyone role
        if role.name != "@everyone" {
            roles.push((*role_id, role.name.clone()));
        }
    }

    // Sort by name for better UX
    roles.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(roles)
}

/// Creates channel select options for dropdown
fn create_channel_options(channels: &[(CI, String)], prefix: &str) -> Vec<CreateSelectMenuOption> {
    channels.iter()
        .take(25) // Discord limit
        .map(|(id, name)| {
            CreateSelectMenuOption::new(name.clone(), format!("{}_{}", prefix, id.get()))
                .description(format!("Channel ID: {}", id.get()))
        })
        .collect()
}

/// Creates role select options for dropdown
fn create_role_options(roles: &[(RoleId, String)], prefix: &str) -> Vec<CreateSelectMenuOption> {
    roles.iter()
        .take(25) // Discord limit
        .map(|(id, name)| {
            CreateSelectMenuOption::new(name.clone(), format!("{}_{}", prefix, id.get()))
                .description(format!("Role ID: {}", id.get()))
        })
        .collect()
}

/// Handles setup interaction responses
pub async fn handle_setup_interaction(ctx: &Context, interaction: &ComponentInteraction, db: &std::sync::Arc<crate::Database>, manager: &Arc<Mutex<crate::models::Manager>>) -> Result<()> {
    use crate::models::ButtonType;

    let button_type = ButtonType::parse(&interaction.data.custom_id);

    // Extract selected value from dropdown
    let selected_values = match &interaction.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values,
        _ => return Ok(()),
    };

    if selected_values.is_empty() {
        return Ok(());
    }

    let selected_value = &selected_values[0];
    let value_parts: Vec<&str> = selected_value.split('_').collect();
    if value_parts.len() < 2 {
        return Ok(());
    }

    // The ID is always the last part after splitting (e.g., "init_queue_123" -> "123")
    let channel_or_role_id: u64 = value_parts.last()
        .ok_or_else(|| anyhow!("No ID found in selected value"))?
        .parse()?;

    // Route based on button type
    match button_type {
        // Setup flow
        ButtonType::SetupDashboard => handle_dashboard_selection(    ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupQueue     => handle_queue_selection(        ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupQueueVc   => handle_queue_vc_selection(     ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupRed       => handle_red_selection(          ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupBlue      => handle_blue_selection(         ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupRunner    => handle_runner_selection(       ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupAdmin     => handle_admin_selection(        ctx, interaction, channel_or_role_id, db, manager).await?,

        // Init flow
        ButtonType::InitQueue      => handle_init_queue_selection(   ctx, interaction, channel_or_role_id).await?,
        ButtonType::InitQueueVc    => handle_init_queue_vc_selection(ctx, interaction, channel_or_role_id).await?,
        ButtonType::InitRed        => handle_init_red_selection(     ctx, interaction, channel_or_role_id).await?,
        ButtonType::InitBlue       => handle_init_blue_selection(    ctx, interaction, channel_or_role_id, db).await?,

        // Unknown button types are ignored
        _ => {}
    }

    Ok(())
}

/// Handles dashboard channel selection
async fn handle_dashboard_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.dashboard_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Dashboard Channel Selected")
        .description(format!(
            "Dashboard channel: <#{}>\n\n\
            **Step 2/7: Queue Text Channel**\n\
            Select the text channel where players will use queue commands:",
            channel_id
        ))
        .color(0x00ff00);

    let channels = get_text_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "queue");

    let select_menu = CreateSelectMenu::new("setup_queue", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select queue channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles queue channel selection
async fn handle_queue_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Queue Text Channel Selected")
        .description(format!(
            "Queue text channel: <#{}>\n\n\
            **Step 3/7: Queue Voice Channel**\n\
            Select the voice channel where players will wait in queue:",
            channel_id
        ))
        .color(0x00ff00);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "queuevc");

    let select_menu = CreateSelectMenu::new("setup_queuevc", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select queue voice channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles queue voice channel selection
async fn handle_queue_vc_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_vc_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Queue Voice Channel Selected")
        .description(format!(
            "Queue voice channel: <#{}>\n\n\
            **Step 4/7: Red Team Voice Channel**\n\
            Select the voice channel for the Red team:",
            channel_id
        ))
        .color(0x00ff00);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "red");

    let select_menu = CreateSelectMenu::new("setup_red", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select red team voice channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles red team channel selection
async fn handle_red_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.red_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Red Team Channel Selected")
        .description(format!(
            "Red team channel: <#{}>\n\n\
            **Step 5/7: Blue Team Voice Channel**\n\
            Select the voice channel for the Blue team:",
            channel_id
        ))
        .color(0x00ff00);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "blue");

    let select_menu = CreateSelectMenu::new("setup_blue", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select blue team voice channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles blue team channel selection
async fn handle_blue_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.blue_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Blue Team Channel Selected")
        .description(format!(
            "Blue team channel: <#{}>\n\n\
            **Step 6/7: Runner Role**\n\
            Select the role that can manage PUG games:",
            channel_id
        ))
        .color(0x00ff00);

    let roles = get_guild_roles(&guild).await?;
    let role_options = create_role_options(&roles, "runner");

    let select_menu = CreateSelectMenu::new("setup_runner", CreateSelectMenuKind::String { options: role_options })
        .placeholder("Select runner role...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles runner role selection
async fn handle_runner_selection(ctx: &Context, interaction: &ComponentInteraction, role_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.runner_role = Some(role_id);
    });

    let embed = CE::new()
        .title("Runner Role Selected")
        .description(format!(
            "Runner role: <@&{}>\n\n\
            **Step 7/7: Admin Role**\n\
            Select the role that can configure the bot:",
            role_id
        ))
        .color(0x00ff00);

    let roles = get_guild_roles(&guild).await?;
    let role_options = create_role_options(&roles, "admin");

    let select_menu = CreateSelectMenu::new("setup_admin", CreateSelectMenuKind::String { options: role_options })
        .placeholder("Select admin role...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles admin role selection and completes setup
async fn handle_admin_selection(ctx: &Context, interaction: &ComponentInteraction, role_id: u64, db: &std::sync::Arc<crate::Database>, manager: &Arc<Mutex<crate::models::Manager>>) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Store the final selection and retrieve complete config
    let config = SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.admin_role = Some(role_id);
    });

    let config = match config {
        Some(cfg) if cfg.is_complete() => cfg,
        _ => {
            let error_embed = CE::new()
                .title("❌ Setup Error")
                .description("Configuration is incomplete. Please restart the setup process.")
                .color(0xff0000);

            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(error_embed)
                    .components(vec![])
            );

            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    let dashboard_channel = config.dashboard_channel.unwrap();
    let queue_channel = config.queue_channel.unwrap();
    let queue_vc_channel = config.queue_vc_channel.unwrap();
    let red_channel = config.red_channel.unwrap();
    let blue_channel = config.blue_channel.unwrap();
    let runner_role = config.runner_role.unwrap();
    let admin_role = role_id;

    // Create the initial dashboard message
    let dashboard_channel_id = CI::new(dashboard_channel);
    let initial_embed = CE::new()
        .title("🎮 PUG Queue Dashboard")
        .description("Queue is empty. Be the first to join!")
        .color(0x00aaff);

    let dashboard_message = match dashboard_channel_id.send_message(&ctx.http, CreateMessage::new().embed(initial_embed)).await {
        Ok(msg) => msg,
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Setup Failed")
                .description(format!("Failed to create dashboard message: {}", e))
                .color(0xff0000);

            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(error_embed)
                    .components(vec![])
            );

            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    let dashboard_msg_id = dashboard_message.id.get();

    // Save role configurations to database
    if let Err(e) = db.config.set_config("runner_role", &runner_role.to_string(), guild_id.get()).await {
        warn!("Failed to save runner_role config: {}", e);
    }
    if let Err(e) = db.config.set_config("admin_role", &admin_role.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {}", e);
    }

    // Create/validate rank roles
    info!("Creating/validating rank roles for guild {}", guild_id);
    if let Err(e) = crate::handlers::player::create_rank_roles(ctx, db, guild_id).await {
        warn!("Failed to create rank roles: {}", e);
    }

    // Create the group configuration in database
    match db.groups.create_group(
        guild_id.get(),
        dashboard_channel,
        queue_channel,
        queue_vc_channel,
        dashboard_msg_id,
        red_channel,
        blue_channel,
        crate::DEFAULT_QUOTA,
    ).await {
        Ok(_) => {
            info!("Group configuration saved to database");

            match db.groups.get_groups_for_guild(guild_id.get()).await {
                Ok(groups) if !groups.is_empty() => {
                    let new_group = groups.into_iter()
                        .find(|g| g.dashboard_msg.get() == dashboard_msg_id)
                        .ok_or_else(|| anyhow!("Could not find newly created group"))?;

                    use crate::models::Server;
                    let mut mgr = manager.lock().await;

                    if mgr.get_server(guild_id).is_err() {
                        let guild_name = ctx.cache.guild(guild_id)
                            .map(|g| g.name.clone())
                            .unwrap_or_else(|| "Unknown".to_string());
                        let server = Server::empty(guild_id, guild_name);
                        mgr.servers.push(server);
                    }

                    let server = mgr.get_server(guild_id)?;
                    server.groups.push(new_group);

                    let group = server.groups.last_mut().ok_or_else(|| anyhow!("Failed to get newly added group"))?;
                    group.queue_dash_update(ctx, guild_id.get()).await;

                    info!("Group added to in-memory manager and dashboard updated");
                },
                Ok(_) => {
                    warn!("No groups found after creation");
                },
                Err(e) => {
                    warn!("Failed to load groups from database: {}", e);
                }
            }

            SETUP_STATE.complete_setup(user_id, guild_id);

            let success_embed = CE::new()
                .title("🎉 Setup Complete!")
                .description(format!(
                    "Your PUG bot is now fully configured and ready to use!\n\n\
                    **Configuration Summary:**\n\
                    • Dashboard: <#{}>\n\
                    • Queue Text: <#{}>\n\
                    • Queue Voice: <#{}>\n\
                    • Red Team: <#{}>\n\
                    • Blue Team: <#{}>\n\
                    • Runner Role: <@&{}>\n\
                    • Admin Role: <@&{}>\n\
                    • Rank Roles: Created\n\n\
                    **The dashboard is ready!** Players can now:\n\
                    • Click \"Join/Leave\" to queue up\n\
                    • Join the queue voice channel to auto-queue\n\n\
                    Runners can use the dashboard buttons to manage matches.",
                    dashboard_channel, queue_channel, queue_vc_channel, red_channel, blue_channel, runner_role, admin_role
                ))
                .color(0x00ff00);

            let response = CIR::UpdateMessage(CIRM::new().embed(success_embed).components(vec![]));

            interaction.create_response(&ctx.http, response).await?;
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Setup Failed")
                .description(format!("Failed to save configuration: {}", e))
                .color(0xff0000);

            let response = CIR::UpdateMessage(CIRM::new().embed(error_embed).components(vec![]));

            interaction.create_response(&ctx.http, response).await?;
        }
    }

    Ok(())
}

// ==================== INIT GROUP HANDLERS ====================

/// Handles queue channel selection for init_group
async fn handle_init_queue_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Err(anyhow!("Guild ID not found - setup must be run in a server"))
    };
    let guild    = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id  = interaction.user.id;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Queue Text Channel Selected")
        .description(format!(
            "Queue text channel: <#{}>\n\n\
            **Step 3/5: Queue Voice Channel**\n\
            Select the voice channel players will join for the queue:",
            channel_id
        ))
        .color(0x00ff00);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "init_queuevc");

    let select_menu = CreateSelectMenu::new("init_queuevc", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select queue voice channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles queue voice channel selection for init_group
async fn handle_init_queue_vc_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None     => return Err(anyhow!("Guild ID not found - setup must be run in a server"))
    };
    let guild    = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id  = interaction.user.id;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_vc_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Queue Voice Channel Selected")
        .description(format!(
            "Queue voice channel: <#{}>\n\n\
            **Step 4/5: Red Team Voice Channel**\n\
            Select the voice channel for the Red team:",
            channel_id
        ))
        .color(0x00ff00);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "init_red");

    let select_menu = CreateSelectMenu::new("init_red", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select red team voice channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles red team channel selection for init_group
async fn handle_init_red_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Err(anyhow!("Guild ID not found - setup must be run in a server"))
    };
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;

    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.red_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Red Team Channel Selected")
        .description(format!(
            "Red team channel: <#{}>\n\n\
            **Step 5/5: Blue Team Voice Channel**\n\
            Select the voice channel for the Blue team:",
            channel_id
        ))
        .color(0x00ff00);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "init_blue");

    let select_menu = CreateSelectMenu::new("init_blue", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select blue team voice channel...")
        .max_values(1);

    let action_row = CreateActionRow::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles blue team channel selection and completes init_group setup
async fn handle_init_blue_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64, db: &std::sync::Arc<crate::Database>) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Err(anyhow!("Guild ID not found - setup must be run in a server"))
    };
    let user_id = interaction.user.id;

    // Store the final selection and retrieve complete config
    let config = SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.blue_channel = Some(channel_id);
    });

    // Validate all required fields are present
    let config = match config {
        Some(cfg) if cfg.dashboard_channel.is_some()
                  && cfg.dashboard_msg_id .is_some()
                  && cfg.queue_channel    .is_some()
                  && cfg.queue_vc_channel .is_some()
                  && cfg.red_channel      .is_some()
                  && cfg.blue_channel     .is_some() => cfg,
        _ => {
            let error_embed = CE::new()
                .title("❌ Setup Error")
                .description("Configuration is incomplete. Please restart the setup process.")
                .color(0xff0000);

            let response = CIR::UpdateMessage(CIRM::new().embed(error_embed).components(vec![]));

            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    let dashboard_channel = config.dashboard_channel.unwrap();
    let dashboard_msg_id  = config.dashboard_msg_id .unwrap();
    let queue_channel     = config.queue_channel    .unwrap();
    let queue_vc_channel  = config.queue_vc_channel .unwrap();
    let red_channel       = config.red_channel      .unwrap();
    let blue_channel      = channel_id;

    // Create the group configuration in database with actual dashboard message ID
    match db.groups.create_group(
        guild_id.get(),
        dashboard_channel,
        queue_channel,
        queue_vc_channel,
        dashboard_msg_id, // Real dashboard message ID from step 1
        red_channel,
        blue_channel,
        DEFAULT_QUOTA,
    ).await {
        Ok(group) => {
            // Update the dashboard message with the actual dashboard content
            use crate::models::{Group, Channels, TeamChannel};
            use serenity::all::{ChannelId as CI2, MessageId as MI};

            let mut temp_group = Group::new(
                group.group_id,
                DEFAULT_QUOTA,
                120,
                MI::new(dashboard_msg_id),
                Channels::new(
                    CI2::new(queue_channel),
                    CI2::new(queue_vc_channel),
                    vec![TeamChannel::new(CI2::new(red_channel), CI2::new(blue_channel))],
                    CI2::new(dashboard_channel),
                ),
                Vec::new(),
            );

            // Update the dashboard message to show the proper dashboard UI
            temp_group.queue_dash_update(ctx, guild_id.get()).await;
        },
        Err(e) => {
            let error_embed = CE::new().title("❌ Setup Failed").description(format!("Failed to create group configuration: {}", e)).color(0xff0000);

            let response = CIR::UpdateMessage(CIRM::new().embed(error_embed).components(vec![]));

            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    }

    SETUP_STATE.complete_setup(user_id, guild_id);

    let success_embed = CE::new()
        .title("Group Setup Complete!")
        .description(format!(
            "Group configuration has been saved successfully!\n\n\
            **Configuration Summary:**\n\
            • Dashboard: <#{}>\n\
            • Queue Text: <#{}>\n\
            • Queue Voice: <#{}>\n\
            • Red Team: <#{}>\n\
            • Blue Team: <#{}>\n\n\
            The dashboard has been initialized in <#{}> with the interactive queue interface!",
            dashboard_channel, queue_channel, queue_vc_channel, red_channel, blue_channel, dashboard_channel
        ))
        .color(0x00ff00);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(success_embed)
            .components(vec![]) // Remove components
    );

    interaction.create_response(&ctx.http, response).await?;

    Ok(())
}

/// `/check_ranks` - Check and offer to create missing rank roles
pub async fn cmd_check_ranks(cc: &CC<'_>) -> Result<()> {
    info!("Processing check_ranks command");

    // Check admin permissions
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can check roles!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");

    // Check for missing system roles (Runner and Admin)
    let missing_system_roles = match validate_system_roles(cc.ctx, &cc.db, guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Error")
                .description(format!("Failed to check system roles: {}", e))
                .color(0xff0000);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Check for missing rank roles
    let missing_rank_roles = match validate_rank_roles(cc.ctx, &cc.db, guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Error")
                .description(format!("Failed to check rank roles: {}", e))
                .color(0xff0000);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Build response based on what's missing
    if missing_system_roles.is_empty() && missing_rank_roles.is_empty() {
        // All roles exist
        let success_embed = CE::new()
            .title("All Roles Configured")
            .description("All system roles (Runner, Admin) and rank roles are properly configured in this server!")
            .color(0x00ff00);

        let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        // Build description for missing roles
        let mut description = String::new();

        if !missing_system_roles.is_empty() {
            let system_list = missing_system_roles.join(", ");
            description.push_str(&format!(
                "**Missing System Roles:**\n{}\n\n\
                ⚠️ System roles should be created manually and assigned appropriate permissions.\n\n",
                system_list
            ));
        }

        if !missing_rank_roles.is_empty() {
            let rank_list = missing_rank_roles.join(", ");
            description.push_str(&format!(
                "**Missing Rank Roles:**\n{}\n\n\
                Would you like me to create these rank roles automatically?\n\n\
                ⚠️ Note: The roles will be created but you may need to adjust their permissions and position in the role hierarchy.",
                rank_list
            ));
        }

        let embed = CE::new()
            .title("⚠️ Missing Roles")
            .description(description)
            .color(0xffaa00);

        // Only add create button if there are rank roles to create
        if !missing_rank_roles.is_empty() {
            use serenity::all::{CreateButton, ButtonStyle};
            let yes_button = CreateButton::new("create_rank_roles_yes")
                .label("Create Rank Roles")
                .style(ButtonStyle::Success);

            let no_button = CreateButton::new("create_rank_roles_no")
                .label("Cancel")
                .style(ButtonStyle::Secondary);

            let buttons = CreateActionRow::Buttons(vec![yes_button, no_button]);

            let response = CIR::Message(
                CIRM::new()
                    .embed(embed)
                    .components(vec![buttons])
                    .ephemeral(true)
            );

            cc.intax.create_response(&cc.ctx.http, response).await?;
        } else {
            // No rank roles to create, just show the message
            let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }

    Ok(())
}

/// Handle rank role creation confirmation button
pub async fn handle_create_rank_roles(ctx: &Context, db: &crate::Database, interaction: &ComponentInteraction, create: bool) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Err(anyhow!("Guild ID not found - this command must be run in a server"))
    };

    if !create {
        // User cancelled
        let cancel_embed = CE::new()
            .title("❌ Cancelled")
            .description("Rank role creation was cancelled.")
            .color(0x999999);

        let response = CIR::UpdateMessage(
            CIRM::new()
                .embed(cancel_embed)
                .components(vec![])
        );

        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Create the missing roles
    let created_roles = match create_rank_roles(ctx, db, guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Error")
                .description(format!("Failed to create rank roles: {}", e))
                .color(0xff0000);

            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(error_embed)
                    .components(vec![])
            );

            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };

    if created_roles.is_empty() {
        let embed = CE::new()
            .title("ℹ️ No Roles Created")
            .description("All rank roles already exist in this server.")
            .color(0x00aaff);

        let response = CIR::UpdateMessage(
            CIRM::new()
                .embed(embed)
                .components(vec![])
        );

        interaction.create_response(&ctx.http, response).await?;
    } else {
        let created_list = created_roles.join(", ");
        let success_embed = CE::new()
            .title("Rank Roles Created")
            .description(format!(
                "Successfully created the following rank roles:\n**{}**\n\n\
                💡 You may want to:\n\
                • Adjust role positions in Server Settings\n\
                • Configure role permissions\n\
                • Assign roles to existing members",
                created_list
            ))
            .color(0x00ff00);

        let response = CIR::UpdateMessage(
            CIRM::new()
                .embed(success_embed)
                .components(vec![])
        );

        interaction.create_response(&ctx.http, response).await?;
    }

    Ok(())
}

/// `/setquota` - Set the queue quota for the current group
///
/// * `quota` - The new quota value (number of players required to start a game)
pub async fn cmd_set_quota(cc: &CC<'_>, quota: i64) -> Result<()> {
    info!("Processing /setquota quota: {}", quota);

    // Check admin permissions
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can modify the queue quota!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Validate quota range
    if !(2..=100).contains(&quota) {
        let error_embed = CE::new()
            .title("❌ Invalid Quota")
            .description("Quota must be between 2 and 100 players.")
            .color(0xff0000);

        let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");

    // Get the group from the current channel
    let mut manager = cc.manager.lock().await;
    let server = match manager.get_server(guild_id) {
        Ok(s) => s,
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Server Not Found")
                .description(format!("Server not configured: {}", e))
                .color(0xff0000);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    let group = match server.get_group(cc.intax.channel_id) {
        Ok(g) => g,
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Group Not Found")
                .description(format!("No queue group found in this channel: {}", e))
                .color(0xff0000);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    let old_quota = group.quota;

    // Update the quota in the group
    group.quota = quota as u8;

    // Update the quota in the database
    match cc.db.set_group(
        guild_id.get(),
        group.channels.queue_vc.get(),
        group.channels.dashboard.get(),
        group.channels.queue_chat.get(),
        group.channels.teams[0].red_vc.get(),
        group.channels.teams[0].blu_vc.get(),
        quota as u8,
    ).await {
        Ok(_) => {
            info!("Updated quota from {} to {} for guild {}", old_quota, quota, guild_id);

            let success_embed = CE::new()
                .title("Quota Updated")
                .description(format!(
                    "Queue quota has been changed from **{}** to **{}** players.\n\n\
                    The queue will now require {} players before a game can start.",
                    old_quota, quota, quota
                ))
                .color(0x00ff00);

            let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;

            // Update the dashboard to reflect the new quota
            group.queue_dash_update(cc.ctx, cc.intax.guild_id.unwrap().get()).await;
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Failed to Update Quota")
                .description(format!("Failed to save quota to database: {}", e))
                .color(0xff0000);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }

    Ok(())
}