// CHECK ME

use anyhow::Result;
use serenity::all::*;
use tracing::{info, error};
use crate::models::command::CommandContext as CC;
use serenity::builder::{
    CreateEmbed as CE, 
    CreateInteractionResponse as CIR, 
    CreateInteractionResponseMessage as CIRM
};

use crate::handlers::player::check_role;
use crate::handlers::dashboard;
use crate::models::player::Role;
use crate::models::setup_state::SETUP_STATE;

/// `/buffer`
///
/// * `user_mention` - The user mention to buffer.
pub async fn cmd_buffer(cc: &CC<'_>,user_mention: String,) -> Result<()> {
    info!("Processing buffer command for user mention: {}", user_mention);
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can buffer players!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }
    // TODO: Actually buffer the player
    Ok(())
}

/// `/config`
///
/// * `key`         - The key to modify.
/// * `value`       - The value to set for the key.
pub async fn cmd_config(cc: &CC<'_>, key: String, value: Option<String,>,) -> Result<()> {
    info!("Processing config: key={}, value={:?}", key, value);
    if !check_role(cc, &Role::Admin).await? {
        info!("User is not an admin");
        let response = CIR::Message(CIRM::new().content("Only session admins can modify the config!").ephemeral(true));
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

/// `/init_dashboard`
pub async fn cmd_init_dashboard(cc: &CC<'_>,) -> Result<()> {
    info!("Processing init_dashboard command");
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can set up the dashboard!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }
    
    let channel_id = cc.intax.channel_id;
    
    // Get the group from database using channel_id since there can be multiple groups per guild
    let group = match cc.db.get_group_by_channel(channel_id).await {
        Ok(group) => group,
        Err(e) => {
            let response = CIR::Message(CIRM::new().content(format!("Failed to get group for this channel: {}", e)).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };
    
    match group.init_dashboard(cc.ctx, &cc.db, channel_id).await {
        Ok(true) => {
            let response = CIR::Message(CIRM::new().content("Dashboard setup complete!").ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        },
        Ok(false) => {
            let response = CIR::Message(CIRM::new().content("Failed to set up dashboard: channel ID mismatch.").ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        },
        Err(e) => {
            let response = CIR::Message(CIRM::new().content(format!("Failed to set up dashboard: {}", e)).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }
    
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
pub async fn cmd_dashboard(cc: &CC<'_>) -> Result<()> {
    info!("Processing dashboard command");
    
    // Check permissions - only runners/admins can create dashboard
    if !check_role(cc, &Role::Runner).await? && !check_role(cc, &Role::Admin).await? {
        cc.create_bot_reply("Only runners and admins can create the dashboard!").await?;
        return Ok(());
    }
    
    let channel = cc.intax.channel_id;
    let base_group = cc.db.get_group_by_channel(channel).await?;
    
    // Get current group state
    let group_data = {
        let mut manager = match cc.manager.lock() {
            Ok(manager) => manager,
            Err(poisoned) => {
                error!("Manager mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        };
        let group = manager.get_or_create_group(channel, base_group);
        group.clone()
    };
    
    // Create and send dashboard
    dashboard::update_dashboard(&group_data, &cc.ctx, channel.get()).await?;
    
    cc.create_bot_reply("✅ Dashboard created/updated successfully!").await?;
    
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

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let user_id = cc.intax.user.id;

    // Acknowledge the command
    let response = CIR::Message(CIRM::new()
        .content("🚀 Starting guild setup! Check your DMs for configuration steps.")
        .ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Start the DM flow
    start_setup_flow(&cc.ctx, guild_id, user_id, &cc.db).await?;
    
    Ok(())
}

/// Starts the interactive setup flow via DMs
async fn start_setup_flow(ctx: &Context, guild_id: GuildId, user_id: UserId, db: &std::sync::Arc<crate::Database>) -> Result<()> {
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
    let channels = get_text_channels(&guild).await?;
    let channel_options = create_channel_options(&channels, "dashboard");
    
    let select_menu = CreateSelectMenu::new("setup_dashboard", channel_options)
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
async fn get_text_channels(guild: &PartialGuild) -> Result<Vec<(ChannelId, String)>> {
    let mut channels = Vec::new();
    
    for (channel_id, channel) in &guild.channels {
        if let Channel::Guild(guild_channel) = channel {
            if guild_channel.kind == ChannelType::Text {
                channels.push((*channel_id, guild_channel.name.clone()));
            }
        }
    }
    
    // Sort by name for better UX
    channels.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(channels)
}

/// Gets voice channels from a guild
async fn get_voice_channels(guild: &PartialGuild) -> Result<Vec<(ChannelId, String)>> {
    let mut channels = Vec::new();
    
    for (channel_id, channel) in &guild.channels {
        if let Channel::Guild(guild_channel) = channel {
            if guild_channel.kind == ChannelType::Voice {
                channels.push((*channel_id, guild_channel.name.clone()));
            }
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
fn create_channel_options(channels: &[(ChannelId, String)], prefix: &str) -> Vec<CreateSelectMenuOption> {
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
    let custom_id = &interaction.data.custom_id;
    
    if !custom_id.starts_with("setup_") {
        return Ok(());
    }
    
    // Parse the setup step and selected value
    let parts: Vec<&str> = custom_id.split('_').collect();
    if parts.len() < 2 {
        return Ok(());
    }
    
    let step = parts[1];
    let selected_values = &interaction.data.values;
    
    if selected_values.is_empty() {
        return Ok(());
    }
    
    let selected_value = &selected_values[0];
    let value_parts: Vec<&str> = selected_value.split('_').collect();
    if value_parts.len() < 2 {
        return Ok(());
    }
    
    let channel_or_role_id: u64 = value_parts[1].parse()?;
    
    match step {
        "dashboard" => handle_dashboard_selection(ctx, interaction, channel_or_role_id).await?,
        "queue" => handle_queue_selection(ctx, interaction, channel_or_role_id).await?,
        "red" => handle_red_selection(ctx, interaction, channel_or_role_id).await?,
        "blue" => handle_blue_selection(ctx, interaction, channel_or_role_id).await?,
        "runner" => handle_runner_selection(ctx, interaction, channel_or_role_id).await?,
        "admin" => handle_admin_selection(ctx, interaction, channel_or_role_id, db).await?,
        _ => {}
    }
    
    Ok(())
}

/// Handles dashboard channel selection
async fn handle_dashboard_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;
    
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
    
    let channels = get_text_channels(&guild).await?;
    let channel_options = create_channel_options(&channels, "queue");
    
    let select_menu = CreateSelectMenu::new("setup_queue", channel_options)
        .placeholder("Select queue channel...")
        .max_values(1);
    
    let action_row = CreateActionRow::SelectMenu(select_menu);
    
    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .embed(embed)
            .components(vec![action_row])
    );
    
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles queue channel selection
async fn handle_queue_selection(ctx: &Context, interaction: &ComponentInteraction, channel_id: u64) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;
    
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
    
    let channels = get_voice_channels(&guild).await?;
    let channel_options = create_channel_options(&channels, "red");
    
    let select_menu = CreateSelectMenu::new("setup_red", channel_options)
        .placeholder("Select red team voice channel...")
        .max_values(1);
    
    let action_row = CreateActionRow::SelectMenu(select_menu);
    
    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
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
    
    let channels = get_voice_channels(&guild).await?;
    let channel_options = create_channel_options(&channels, "blue");
    
    let select_menu = CreateSelectMenu::new("setup_blue", channel_options)
        .placeholder("Select blue team voice channel...")
        .max_values(1);
    
    let action_row = CreateActionRow::SelectMenu(select_menu);
    
    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
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
            Select the role that can manage PUG sessions:",
            channel_id
        ))
        .color(0x00ff00);
    
    let roles = get_guild_roles(&guild).await?;
    let role_options = create_role_options(&roles, "runner");
    
    let select_menu = CreateSelectMenu::new("setup_runner", role_options)
        .placeholder("Select runner role...")
        .max_values(1);
    
    let action_row = CreateActionRow::SelectMenu(select_menu);
    
    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
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
    
    let select_menu = CreateSelectMenu::new("setup_admin", role_options)
        .placeholder("Select admin role...")
        .max_values(1);
    
    let action_row = CreateActionRow::SelectMenu(select_menu);
    
    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
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
            
            let response = CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
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
    
    // Create the group configuration in database
    match db.groups.create_group(
        guild_id.get(),
        dashboard_channel,
        0, // chat channel (not used)
        queue_channel,
        0, // dashboard message ID (will be set later)
        red_channel,
        blue_channel,
        10, // default session quota
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
                    You can now use `/init_dashboard` in the dashboard channel to create the queue interface!",
                    dashboard_channel, queue_channel, red_channel, blue_channel, runner_role, admin_role
                ))
                .color(0x00ff00);
            
            let response = CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
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
            
            let response = CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .embed(error_embed)
                    .components(vec![])
            );
            
            interaction.create_response(&ctx.http, response).await?;
        }
    }
    
    Ok(())
}