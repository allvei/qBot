// CHECK ME

use anyhow::{anyhow, Result};
use serenity::all::{
    ChannelId as CI, ChannelType, ComponentInteraction, ComponentInteractionDataKind,
    Context, CreateActionRow, CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, CreateMessage, CreateSelectMenu,
    CreateSelectMenuKind, CreateSelectMenuOption, GuildId, PartialGuild, RoleId, UserId,
};
use tracing::{error, info};

use crate::DEFAULT_QUOTA;
use crate::handlers::player::check_role;
use crate::models::{CommandContext as CC, Role, Server, SETUP_STATE};

/// `/config`
///
/// * `key`   - The key to modify.
/// * `value` - The value to set for the key.
pub async fn cmd_config(cc: &CC<'_>, key: String, value: Option<String,>,) -> Result<()> {
    info!("Processing config: key={}, value={:?}", key, value);
    if !check_role(cc, &Role::Admin).await? {
        info!("User is not an admin");
        let response = CIR::Message(CIRM::new().content("Only game admins can modify the config!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    if let Some(val,) = value {
        info!("Setting config {} = {}", key, val);

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

/// `/cmd_init_group`
pub async fn cmd_init_group(cc: &CC<'_>, guild: &mut Server) -> Result<()> {
    info!("Processing cmd_init_group command");
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
        .title("✅ Dashboard Created")
        .description(format!(
            "Dashboard message created in <#{}>\n\n\
            Now let's configure the remaining channels for **{}**.\n\n\
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
        Ok(mention[3..mention.len() - 1].to_string().parse::<u64>().unwrap())
    } else if mention.starts_with("<@") && mention.ends_with('>') {
        Ok(mention[2..mention.len() - 1].to_string().parse::<u64>().unwrap())
    } else {
        // Assume it's already a raw user ID
        Ok(mention.parse::<u64>().unwrap())
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
    let group = guild.get_group(channel).unwrap();
    
    // Create and send dashboard
    group.dash_publish(cc.ctx, channel).await?;
    
    cc.reply("✅ Dashboard created/updated successfully!").await?;
    
    Ok(())
}

/// `/setup`
///
/// Sets up the bot for a guild using an interactive DM flow
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

    // Acknowledge the command
    let response = CIR::Message(CIRM::new()
        .content("🚀 Starting guild setup! Check your DMs for configuration steps.")
        .ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Start the DM flow
    start_setup_flow(cc.ctx, guild_id, user_id, &cc.db).await?;
    
    Ok(())
}

/// Starts the interactive setup flow via DMs
async fn start_setup_flow(ctx: &Context, guild_id: GuildId, user_id: UserId, _db: &std::sync::Arc<crate::Database>) -> Result<()> {
    // Initialize setup state
    SETUP_STATE.start_setup(user_id, guild_id);
    
    // Create DM channel with user
    let dm_channel = user_id.create_dm_channel(&ctx.http).await?;
    
    // Get guild information
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    
    // Send welcome message
    let welcome_embed = CE::new()
        .title("🛠️ Guild Setup Wizard")
        .description(format!(
            "Welcome to the setup wizard for **{}**!\n\n\
            I'll guide you through configuring the bot step by step.\n\
            You'll select channels and roles using dropdown menus.\n\n\
            **Step 1/6: Dashboard Channel**\n\
            Select the channel where the queue dashboard will be displayed:",
            guild.name
        ))
        .color(0x00ff00);

    // Get text channels for dropdown
    let channels = get_text_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "dashboard");
    
    let select_menu = CreateSelectMenu::new("setup_dashboard", CreateSelectMenuKind::String { options: channel_options })
        .placeholder("Select dashboard channel...")
        .max_values(1);
    
    let action_row = CreateActionRow::SelectMenu(select_menu);
    
    dm_channel.send_message(&ctx.http, 
        CreateMessage::new()
            .embed(welcome_embed)
            .components(vec![action_row])
    ).await?;
    
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
pub async fn handle_setup_interaction(ctx: &Context, interaction: &ComponentInteraction, db: &std::sync::Arc<crate::Database>) -> Result<()> {
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
        ButtonType::SetupRed       => handle_red_selection(          ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupBlue      => handle_blue_selection(         ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupRunner    => handle_runner_selection(       ctx, interaction, channel_or_role_id).await?,
        ButtonType::SetupAdmin     => handle_admin_selection(        ctx, interaction, channel_or_role_id, db).await?,
        
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
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild    = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id  = interaction.user.id;
    
    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.dashboard_channel = Some(channel_id);
    });
    
    let embed = CE::new()
        .title("✅ Dashboard Channel Selected")
        .description(format!(
            "Dashboard channel: <#{}>\n\n\
            **Step 2/6: Queue Channel**\n\
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
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild    = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id  = interaction.user.id;
    
    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_channel = Some(channel_id);
    });
    
    let embed = CE::new()
        .title("✅ Queue Channel Selected")
        .description(format!(
            "Queue channel: <#{}>\n\n\
            **Step 3/6: Red Team Voice Channel**\n\
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
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;
    
    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.red_channel = Some(channel_id);
    });
    
    let embed = CE::new()
        .title("✅ Red Team Channel Selected")
        .description(format!(
            "Red team channel: <#{}>\n\n\
            **Step 4/6: Blue Team Voice Channel**\n\
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
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;
    
    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.blue_channel = Some(channel_id);
    });
    
    let embed = CE::new()
        .title("✅ Blue Team Channel Selected")
        .description(format!(
            "Blue team channel: <#{}>\n\n\
            **Step 5/6: Runner Role**\n\
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
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;
    
    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.runner_role = Some(role_id);
    });
    
    let embed = CE::new()
        .title("✅ Runner Role Selected")
        .description(format!(
            "Runner role: <@&{}>\n\n\
            **Step 6/6: Admin Role**\n\
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
async fn handle_admin_selection(ctx: &Context, interaction: &ComponentInteraction, role_id: u64, db: &std::sync::Arc<crate::Database>) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let user_id = interaction.user.id;
    
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
    
    // Create the group configuration in database with properly configured values
    // Note: queue_channel is a text channel, used for both chat and voice (simplified setup)
    match db.groups.create_group(
        guild_id.get(),
        dashboard_channel,
        queue_channel, // queue text channel (used for chat)
        queue_channel, // same channel used as voice channel (simplified setup)
        dashboard_msg_id, // actual dashboard message ID
        red_channel,
        blue_channel,
        10, // default game quota
    ).await {
        Ok(_) => {
            // Clean up setup state
            SETUP_STATE.complete_setup(user_id, guild_id);
            
            let success_embed = CE::new()
                .title("🎉 Setup Complete!")
                .description(format!(
                    "Guild configuration has been saved successfully!\n\n\
                    **Configuration Summary:**\n\
                    • Dashboard: <#{}>\n\
                    • Queue: <#{}>\n\
                    • Red Team: <#{}>\n\
                    • Blue Team: <#{}>\n\
                    • Runner Role: <@&{}>\n\
                    • Admin Role: <@&{}>\n\n\
                    You can now use `/cmd_init_group` in the dashboard channel to create the queue interface!",
                    dashboard_channel, queue_channel, red_channel, blue_channel, runner_role, admin_role
                ))
                .color(0x00ff00);
            
            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(success_embed)
                    .components(vec![]) // Remove components
            );
            
            interaction.create_response(&ctx.http, response).await?;
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Setup Failed")
                .description(format!("Failed to save configuration: {}", e))
                .color(0xff0000);
            
            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(error_embed)
                    .components(vec![])
            );
            
            interaction.create_response(&ctx.http, response).await?;
        }
    }
    
    Ok(())
}

// ==================== INIT GROUP HANDLERS ====================

/// Handles queue channel selection for init_group
async fn handle_init_queue_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild    = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id  = interaction.user.id;
    
    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_channel = Some(channel_id);
    });
    
    let embed = CE::new()
        .title("✅ Queue Text Channel Selected")
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
    
    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );
    
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles queue voice channel selection for init_group
async fn handle_init_queue_vc_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild    = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id  = interaction.user.id;
    
    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_vc_channel = Some(channel_id);
    });
    
    let embed = CE::new()
        .title("✅ Queue Voice Channel Selected")
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
    
    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );
    
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles red team channel selection for init_group
async fn handle_init_red_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;
    
    // Store the selection in setup state
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.red_channel = Some(channel_id);
    });
    
    let embed = CE::new()
        .title("✅ Red Team Channel Selected")
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
    
    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );
    
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles blue team channel selection and completes init_group setup
async fn handle_init_blue_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64, db: &std::sync::Arc<crate::Database>) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let user_id = interaction.user.id;
    
    // Store the final selection and retrieve complete config
    let config = SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.blue_channel = Some(channel_id);
    });
    
    // Validate all required fields are present
    let config = match config {
        Some(cfg) if cfg.dashboard_channel.is_some() 
                  && cfg.dashboard_msg_id.is_some()
                  && cfg.queue_channel.is_some() 
                  && cfg.queue_vc_channel.is_some()
                  && cfg.red_channel.is_some() 
                  && cfg.blue_channel.is_some() => cfg,
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
    let dashboard_msg_id  = config.dashboard_msg_id.unwrap();
    let queue_channel     = config.queue_channel.unwrap();
    let queue_vc_channel  = config.queue_vc_channel.unwrap();
    let red_channel       = config.red_channel.unwrap();
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
            if let Err(e) = temp_group.dash_update(ctx).await {
                error!("Failed to update dashboard message: {}", e);
            }
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("❌ Setup Failed")
                .description(format!("Failed to create group configuration: {}", e))
                .color(0xff0000);
            
            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(error_embed)
                    .components(vec![])
            );
            
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    }
    
    // Clean up setup state
    SETUP_STATE.complete_setup(user_id, guild_id);
    
    let success_embed = CE::new()
        .title("🎉 Group Setup Complete!")
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