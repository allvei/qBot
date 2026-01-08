
use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::{anyhow, Result};
use serenity::all::{
    ChannelId as CI, ChannelType, ComponentInteraction as CX, ComponentInteractionDataKind as CXD,
    Context, CreateActionRow as CAR, CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, CreateMessage, CreateSelectMenu as CSM,
    CreateSelectMenuKind as CSMK, CreateSelectMenuOption as CSMO, GuildId as GI, PartialGuild as PG, RoleId as RI, UserId as UI,
};
use tracing::{error, info, warn};

use crate::{ADMIN, CYAN, DEFAULT_QUOTA, Database, ELO_MAX, ELO_MIN, GRAY, GREEN, Manager, ORANGE, RED, RUNNER};
use crate::commands::{parse_rank_name};
use crate::database::repositories::Repository;
use crate::handlers::player::{check_role, create_rank_roles, validate_rank_roles, validate_system_roles};
use crate::models::{CommandContext as CC, Role, Server, SETUP_STATE};

/// `/config`
///
/// * `key`   - The key to modify.
/// * `value` - The value to set for the key.
pub async fn cmd_config(cc: &CC<'_>, key: String, value: Option<String,>,) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    if let Some(val,) = value {
        cc.db.get_config(cc.intax.guild_id.expect("Guild ID not found").get()).await?;
        let embed    = CE::new().title("Config Updated").description(format!("Set `{key}` = `{val}`"));
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

        let config_text = format!("**Current Configuration:**\n\
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
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found").get();

    // If no parameters, show current role configuration
    if role_type.is_empty() && role.is_none() {
        let runner_role = cc.db.config.get_config_value("runner_role", guild_id).await?;
        let admin_role = cc.db.config.get_config_value("admin_role", guild_id).await?;

        let role_text = format!(
            "**Current Role Configuration:**\n\
             Runner Role: {}\n\
             Admin Role: {}",
            runner_role.map(|r| format!("<@&{r}>")).unwrap_or_else(|| "Not set".to_string()),
            admin_role.map(|r| format!("<@&{r}>")).unwrap_or_else(|| "Not set".to_string())
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
            .color(GREEN);

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        // Show current value for this role type
        let current_role = cc.db.config.get_config_value(role_key, guild_id).await?;

        let embed = CE::new()
            .title(format!("{role_type} Role"))
            .description(format!(
                "Current {} role: {}",
                role_type.to_lowercase(),
                current_role.map(|r| format!("<@&{r}>")).unwrap_or_else(|| "Not set".to_string())
            ));

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
    }

    Ok(())
}

/// `/grouplink` - Interactive flow to link existing channels to a group
pub async fn cmd_group_link(cc: &CC<'_>, _server: &mut Server) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let user_id = cc.intax.user.id;

    // Initialize setup state
    SETUP_STATE.start_setup(user_id, guild_id);

    // Start the channel selection flow
    start_grouplink_flow(cc).await?;

    Ok(())
}

/// Starts the grouplink flow - Step 1: Dashboard channel
async fn start_grouplink_flow(cc: &CC<'_>) -> Result<()> {
    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let guild = guild_id.to_partial_guild(&cc.ctx.http).await?;

    let welcome_embed = CE::new()
        .title("Link Existing Channels")
        .description(format!(
            "Let's link your existing channels to create a PUG group for **{}**.\n\n\
            **Step 1/5: Dashboard Channel**\n\
            Select the text channel where the dashboard will be displayed:",
            guild.name
        ))
        .color(GREEN);

    let channels = get_text_channels(&guild, cc.ctx).await?;
    let channel_options = create_channel_options(&channels, "grouplink_dashboard");

    let select_menu = CSM::new("grouplink_dashboard", CSMK::String { options: channel_options })
        .placeholder("Select dashboard channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::Message(
        CIRM::new()
            .embed(welcome_embed)
            .components(vec![action_row])
            .ephemeral(true)
    );

    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/groupadd` - Creates a new category with all necessary channels
pub async fn cmd_group_add(cc: &CC<'_>, server: &mut Server) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

    // Check if runner and admin roles are configured
    let missing_system_roles = match validate_system_roles(cc.ctx, &cc.db, guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            let error_embed = CE::new()
                .title("Error")
                .description(format!("Failed to check roles: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Check if rank roles are configured
    let missing_rank_roles = match validate_rank_roles(cc.ctx, &cc.db, guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            let error_embed = CE::new()
                .title("Error")
                .description(format!("Failed to check rank roles: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // If roles are missing, start role setup flow first
    if !missing_system_roles.is_empty() || !missing_rank_roles.is_empty() {
        let user_id = cc.intax.user.id;
        SETUP_STATE.start_setup(user_id, guild_id);

        let mut description = String::from("Before creating a group, we need to set up roles.\n\n");

        if !missing_system_roles.is_empty() {
            description.push_str(&format!("**Missing System Roles:** {}\n", missing_system_roles.join(", ")));
        }
        if !missing_rank_roles.is_empty() {
            description.push_str(&format!("**Missing Rank Roles:** {}\n", missing_rank_roles.join(", ")));
        }

        description.push_str("\nLet's create these roles now, then we'll proceed with group creation.");

        let embed = CE::new()
            .title("Role Setup Required")
            .description(description)
            .color(ORANGE);

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;

        // Create the roles
        info!("[{}] Creating missing roles for groupadd flow", guild_name);

        // Create runner and admin roles if missing
        if !missing_system_roles.is_empty() {
            use serenity::all::Permissions;
            use serenity::builder::EditRole;

            if missing_system_roles.contains(&"PUG Runner".to_string()) {
                match guild_id.create_role(&cc.ctx.http,
                    EditRole::new()
                        .name("PUG Runner")
                        .colour(RUNNER)
                        .permissions(Permissions::empty())
                ).await {
                    Ok(role) => {
                        if let Err(e) = cc.db.config.set_config("runner_role", &role.id.to_string(), guild_id.get()).await {
                            warn!("Failed to save runner_role config: {e}");
                        }
                        info!("[{}] Created PUG Runner role", guild_name);
                    },
                    Err(e) => {
                        error!("[{}] Failed to create PUG Runner role: {}", guild_name, e);
                    }
                }
            }

            if missing_system_roles.contains(&"PUG Admin".to_string()) {
                match guild_id.create_role(&cc.ctx.http,
                    EditRole::new()
                        .name("PUG Admin")
                        .colour(ADMIN)
                        .permissions(Permissions::empty())
                ).await {
                    Ok(role) => {
                        if let Err(e) = cc.db.config.set_config("admin_role", &role.id.to_string(), guild_id.get()).await {
                            warn!("Failed to save admin_role config: {e}");
                        }
                        info!("[{}] Created PUG Admin role", guild_name);
                    },
                    Err(e) => {
                        error!("[{}] Failed to create PUG Admin role: {}", guild_name, e);
                    }
                }
            }
        }

        // Create rank roles if missing
        if !missing_rank_roles.is_empty() {
            if let Err(e) = create_rank_roles(cc.ctx, &cc.db, guild_id).await {
                warn!("[{}] Failed to create rank roles: {}", guild_name, e);
            } else {
                info!("[{}] Created rank roles", guild_name);
            }
        }

        // Update the message to show roles were created and now proceeding
        let success_embed = CE::new()
            .title("Roles Created!")
            .description("All required roles have been created.\n\nNow creating group channels...")
            .color(GREEN);

        cc.intax.edit_response(&cc.ctx.http,
            serenity::all::EditInteractionResponse::new().embed(success_embed)
        ).await?;

        // Small delay to let the user see the message
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    } else {
        // Roles exist, send initial "creating channels" message
        let loading_embed = CE::new()
            .title("Creating Group Channels")
            .description("Creating a new category with all necessary channels...\nThis may take a moment.")
            .color(ORANGE);

        let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
    }

    // Create the category and channels
    match create_group_channels(cc.ctx, guild_id).await {
        Ok((category_id, dashboard_channel, queue_channel, queue_vc_channel, red_channel, blue_channel)) => {
            // Create a temporary Group in memory to publish the dashboard
            use crate::models::{Group, Channels, TeamChannel};
            use serenity::all::MessageId;

            let mut temp_group = Group {
                group_id: 0, // Will be assigned by DB
                name: None,
                quota: crate::DEFAULT_QUOTA,
                timeout: crate::DEFAULT_TIMEOUT,
                dashboard_msg: MessageId::new(1), // Temporary, will be replaced
                channels: Channels {
                    queue_chat: queue_channel,
                    queue_vc: queue_vc_channel,
                    teams: vec![TeamChannel {
                        red_vc: red_channel,
                        blu_vc: blue_channel,
                    }],
                    dashboard: dashboard_channel,
                },
                sessions: vec![],
                connect_info: None,
            };

            // Publish the dashboard to get the actual message ID
            match temp_group.dash_publish(cc.ctx, dashboard_channel, &cc.db, guild_id.get()).await {
                Ok(_) => {
                    let dashboard_msg_id = temp_group.dashboard_msg.get();
                    info!("[{}] Dashboard message created with ID {}", guild_name, dashboard_msg_id);

                    // Now create the group in the database with the actual dashboard message ID
                    let group_config = crate::database::repositories::group::GroupConfig {
                        dashboard_channel_id: dashboard_channel.get(),
                        chat_channel_id: queue_channel.get(),
                        queue_vc_id: queue_vc_channel.get(),
                        red_vc_id: red_channel.get(),
                        blu_vc_id: blue_channel.get(),
                        quota: crate::DEFAULT_QUOTA,
                    };
                    match cc.db.groups.create_group(
                        guild_id.get(),
                        dashboard_msg_id,
                        group_config,
                    ).await {
                        Ok(db_group) => {
                            info!("[{}] Group {} saved to database", guild_name, db_group.group_id);

                            // Add group to in-memory server and create initial session
                            if let Err(e) = server.add_group(db_group.clone()) {
                                error!("Failed to add group to server: {e}");
                            }

                            let success_embed = CE::new()
                                .title("Group Created!")
                                .description(format!(
                                    "New PUG group is ready!\n\n\
                                    **Configuration:**\n\
                                    • Dashboard: <#{}>\n\
                                    • Queue Text: <#{}>\n\
                                    • Queue Voice: <#{}>\n\
                                    • Red Team: <#{}>\n\
                                    • Blue Team: <#{}>\n\
                                    • Category: <#{}>",
                                    dashboard_channel.get(),
                                    queue_channel.get(),
                                    queue_vc_channel.get(),
                                    red_channel.get(),
                                    blue_channel.get(),
                                    category_id.get()
                                ))
                                .color(GREEN);

                            cc.intax.edit_response(&cc.ctx.http,
                                serenity::all::EditInteractionResponse::new().embed(success_embed)
                            ).await?;
                        },
                        Err(e) => {
                            // Database save failed - clean up everything
                            info!("[{}] Database save failed, cleaning up channels and dashboard", guild_name);
                            let _ = dashboard_channel.delete_message(&cc.ctx.http, dashboard_msg_id).await;
                            let _ = dashboard_channel.delete(&cc.ctx.http).await;
                            let _ = queue_channel.delete(&cc.ctx.http).await;
                            let _ = queue_vc_channel.delete(&cc.ctx.http).await;
                            let _ = red_channel.delete(&cc.ctx.http).await;
                            let _ = blue_channel.delete(&cc.ctx.http).await;
                            let _ = category_id.delete(&cc.ctx.http).await;

                            let error_embed = CE::new()
                                .title("Failed to Save Group")
                                .description(format!("Failed to save group to database: {e}\n\nChannels were cleaned up."))
                                .color(RED);

                            cc.intax.edit_response(&cc.ctx.http,
                                serenity::all::EditInteractionResponse::new().embed(error_embed)
                            ).await?;
                        }
                    }
                },
                Err(e) => {
                    // Dashboard creation failed - clean up the created channels
                    info!("[{}] Dashboard creation failed, cleaning up channels", guild_name);
                    let _ = dashboard_channel.delete(&cc.ctx.http).await;
                    let _ = queue_channel.delete(&cc.ctx.http).await;
                    let _ = queue_vc_channel.delete(&cc.ctx.http).await;
                    let _ = red_channel.delete(&cc.ctx.http).await;
                    let _ = blue_channel.delete(&cc.ctx.http).await;
                    let _ = category_id.delete(&cc.ctx.http).await;

                    let error_embed = CE::new()
                        .title("Dashboard Creation Failed")
                        .description(format!("Failed to create dashboard: {e}\n\nChannels were cleaned up."))
                        .color(RED);

                    cc.intax.edit_response(&cc.ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(error_embed)
                    ).await?;
                }
            }
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("Channel Creation Failed")
                .description(format!("Failed to create channels: {e}\n\nMake sure the bot has proper permissions."))
                .color(RED);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
        }
    }

    Ok(())
}

/// Creates a category and all necessary group channels
/// Flow: Create category -> Create dashboard -> Test message send -> Create other channels
/// If dashboard message send fails, cleanup and abort
pub async fn create_group_channels(ctx: &Context, guild_id: GI) -> Result<(CI, CI, CI, CI, CI, CI)> {
    use serenity::all::{CreateChannel, CreateEmbed, CreateMessage, PermissionOverwrite, PermissionOverwriteType, Permissions};

    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

    // Get bot's user ID and find bot's integration role "qBot"
    let bot_user_id = ctx.cache.current_user().id;
    let bot_role = guild.roles.values()
        .find(|r| r.name == "qBot" && r.managed)
        .map(|r| r.id);

    // Step 1: Create category
    let category = match guild_id.create_channel(&ctx.http,
        CreateChannel::new("PUG Queue")
            .kind(ChannelType::Category)
    ).await {
        Ok(cat) => cat,
        Err(e) => {
            error!("[{}] Failed to create category: {}", guild_name, e);
            return Err(anyhow!("Failed to create category: {e}"));
        }
    };

    let category_id = category.id;

    // Step 2: Create dashboard text channel with proper permissions
    let mut permissions = vec![
        // Deny @everyone from sending messages and creating threads
        PermissionOverwrite {
            allow: Permissions::empty(),
            deny: Permissions::SEND_MESSAGES | Permissions::CREATE_PUBLIC_THREADS | Permissions::CREATE_PRIVATE_THREADS,
            kind: PermissionOverwriteType::Role(guild_id.everyone_role()),
        },
        // Allow bot user explicitly
        PermissionOverwrite {
            allow: Permissions::SEND_MESSAGES | Permissions::VIEW_CHANNEL | Permissions::EMBED_LINKS,
            deny: Permissions::empty(),
            kind: PermissionOverwriteType::Member(bot_user_id),
        }
    ];

    // Add bot's integration role if found
    if let Some(role_id) = bot_role {
        permissions.push(PermissionOverwrite {
            allow: Permissions::SEND_MESSAGES | Permissions::VIEW_CHANNEL | Permissions::EMBED_LINKS,
            deny: Permissions::empty(),
            kind: PermissionOverwriteType::Role(role_id),
        });
    }

    let dashboard_channel = match guild_id.create_channel(&ctx.http,
        CreateChannel::new("dashboard")
            .kind(ChannelType::Text)
            .category(category_id)
            .topic("PUG queue dashboard - use buttons to join/leave")
            .permissions(permissions)
    ).await {
        Ok(ch) => ch,
        Err(e) => {
            error!("[{}] Failed to create dashboard channel: {}", guild_name, e);
            // Clean up category
            let _ = category_id.delete(&ctx.http).await;
            return Err(anyhow!("Failed to create dashboard channel: {e}"));
        }
    };

    // Step 3: Test dashboard message send - CRITICAL STEP
    let test_embed = CreateEmbed::new()
        .title("PUG Dashboard")
        .description("Setting up queue system...")
        .color(ORANGE);

    let test_msg = dashboard_channel.id.send_message(&ctx.http,
        CreateMessage::new().embed(test_embed)
    ).await;

    if let Err(e) = test_msg {
        error!("[{}] Failed to send dashboard message: {}", guild_name, e);
        // Clean up dashboard channel and category
        info!("[{}] Cleaning up dashboard channel and category", guild_name);
        let _ = dashboard_channel.id.delete(&ctx.http).await;
        let _ = category_id.delete(&ctx.http).await;
        return Err(anyhow!("Failed to send dashboard message (bot may lack permissions): {e}"));
    }

    // Delete the test message - we'll create the real one later
    if let Ok(msg) = test_msg {
        let _ = dashboard_channel.id.delete_message(&ctx.http, msg.id).await;
    }

    info!("[{}] Dashboard channel verified, creating remaining channels", guild_name);

    // Step 4: Create remaining channels (only if dashboard works)
    let queue_channel = match guild_id.create_channel(&ctx.http,
        CreateChannel::new("pug-add-up")
            .kind(ChannelType::Text)
            .category(category_id)
            .topic("Queue discussion and commands")
    ).await {
        Ok(ch) => ch,
        Err(e) => {
            error!("[{}] Failed to create queue text channel: {}", guild_name, e);
            let _ = dashboard_channel.id.delete(&ctx.http).await;
            let _ = category_id.delete(&ctx.http).await;
            return Err(anyhow!("Failed to create queue text channel: {e}"));
        }
    };

    let queue_vc_channel = match guild_id.create_channel(&ctx.http,
        CreateChannel::new("Queue")
            .kind(ChannelType::Voice)
            .category(category_id)
    ).await {
        Ok(ch) => ch,
        Err(e) => {
            error!("[{}] Failed to create queue voice channel: {}", guild_name, e);
            let _ = queue_channel.id.delete(&ctx.http).await;
            let _ = dashboard_channel.id.delete(&ctx.http).await;
            let _ = category_id.delete(&ctx.http).await;
            return Err(anyhow!("Failed to create queue voice channel: {e}"));
        }
    };

    let red_channel = match guild_id.create_channel(&ctx.http,
        CreateChannel::new("🔴 RED")
            .kind(ChannelType::Voice)
            .category(category_id)
    ).await {
        Ok(ch) => ch,
        Err(e) => {
            error!("[{}] Failed to create red team channel: {}", guild_name, e);
            let _ = queue_vc_channel.id.delete(&ctx.http).await;
            let _ = queue_channel.id.delete(&ctx.http).await;
            let _ = dashboard_channel.id.delete(&ctx.http).await;
            let _ = category_id.delete(&ctx.http).await;
            return Err(anyhow!("Failed to create red team channel: {e}"));
        }
    };

    let blue_channel = match guild_id.create_channel(&ctx.http,
        CreateChannel::new("🔵 BLU")
            .kind(ChannelType::Voice)
            .category(category_id)
    ).await {
        Ok(ch) => ch,
        Err(e) => {
            error!("[{}] Failed to create blue team channel: {}", guild_name, e);
            let _ = red_channel.id.delete(&ctx.http).await;
            let _ = queue_vc_channel.id.delete(&ctx.http).await;
            let _ = queue_channel.id.delete(&ctx.http).await;
            let _ = dashboard_channel.id.delete(&ctx.http).await;
            let _ = category_id.delete(&ctx.http).await;
            return Err(anyhow!("Failed to create blue team channel: {e}"));
        }
    };

    info!("[{}] Successfully created all group channels", guild_name);

    Ok((
        category_id,
        dashboard_channel.id,
        queue_channel.id,
        queue_vc_channel.id,
        red_channel.id,
        blue_channel.id,
    ))
}

/// `/dashboard`
///
/// Creates or updates the dashboard in the current channel
pub async fn cmd_dashboard(cc: &CC<'_>, guild: &mut Server) -> Result<()> {
    if !check_role(cc, &Role::Runner).await? && !check_role(cc, &Role::Admin).await? {
        return Ok(());
    }

    let channel = cc.intax.channel_id;
    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("This command must be used in a server"))?;
    let group = guild.get_group(channel)?;

    // Create and send dashboard
    group.dash_publish(cc.ctx, channel, &cc.db, guild_id.get()).await?;

    cc.reply("Dashboard created/updated successfully!").await?;

    Ok(())
}

/// `/setup`
///
/// Sets up the bot for a guild using an interactive ephemeral message flow
pub async fn cmd_setup(cc: &CC<'_>) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id: GI = cc.intax.guild_id.expect("Guild ID not found");
    let user_id:  UI  = cc.intax.user.id;

    // Start the setup flow with ephemeral message
    start_setup_flow(cc, guild_id, user_id).await?;

    Ok(())
}

/// Starts the interactive setup flow with ephemeral messages
async fn start_setup_flow(cc: &CC<'_>, guild_id: GI, user_id: UI) -> Result<()> {
    // Initialize setup state
    SETUP_STATE.start_setup(user_id, guild_id);

    // Get guild information
    let guild = guild_id.to_partial_guild(&cc.ctx.http).await?;

    // Send welcome message as ephemeral reply
    let welcome_embed = CE::new()
        .title("Guild Setup Wizard")
        .description(format!(
            "Welcome to the setup wizard for **{}**!\n\n\
            I'll guide you through configuring the bot step by step.\n\
            You'll select channels and roles using dropdown menus.\n\n\
            **Before continuing**, make sure you have created:\n\
            • A text channel for the dashboard\n\
            • A text channel for queue commands\n\
            • A voice channel for the queue\n\
            • Voice channels for Red and Blue teams\n\n\
            **Step 1/7: Dashboard Channel**\n\
            Select the channel where the queue dashboard will be displayed:",
            guild.name
        ))
        .color(GREEN);

    // Get text channels for dropdown
    let channels = get_text_channels(&guild, cc.ctx).await?;
    let channel_options = create_channel_options(&channels, "dashboard");

    let select_menu = CSM::new("setup_dashboard", CSMK::String { options: channel_options })
        .placeholder("Select dashboard channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

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
async fn get_text_channels(guild: &PG, ctx: &Context) -> Result<Vec<(CI, String)>> {
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
async fn get_voice_channels(guild: &PG, ctx: &Context) -> Result<Vec<(CI, String)>> {
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
async fn get_guild_roles(guild: &PG) -> Result<Vec<(RI, String)>> {
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
fn create_channel_options(channels: &[(CI, String)], prefix: &str) -> Vec<CSMO> {
    channels.iter()
        .take(25) // Discord limit
        .map(|(id, name)| {
            CSMO::new(name.clone(), format!("{prefix}_{}", id.get()))
                .description(format!("Channel ID: {}", id.get()))
        })
        .collect()
}

/// Creates role select options for dropdown
fn create_role_options(roles: &[(RI, String)], prefix: &str) -> Vec<CSMO> {
    roles.iter()
        .take(25) // Discord limit
        .map(|(id, name)| {
            CSMO::new(name.clone(), format!("{prefix}_{}", id.get()))
                .description(format!("Role ID: {}", id.get()))
        })
        .collect()
}

/// Handles setup interaction responses
pub async fn handle_setup_interaction(ctx: &Context, interaction: &CX, db: &Arc<Database>, manager: &Arc<Mutex<Manager>>) -> Result<()> {
    use crate::models::ButtonType;

    let button_type = ButtonType::parse(&interaction.data.custom_id);

    // Extract selected value from dropdown
    let selected_values = match &interaction.data.kind {
        CXD::StringSelect { values } => values,
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
        ButtonType::InitBlue       => handle_init_blue_selection(    ctx, interaction, channel_or_role_id).await?,
        ButtonType::InitRunner     => handle_init_runner_selection(  ctx, interaction, channel_or_role_id).await?,
        ButtonType::InitAdmin      => handle_init_admin_selection(   ctx, interaction, channel_or_role_id, db, manager).await?,

        // GroupLink flow
        ButtonType::GroupLinkDashboard => handle_grouplink_dashboard_selection(ctx, interaction, channel_or_role_id).await?,
        ButtonType::GroupLinkQueue     => handle_grouplink_queue_selection(    ctx, interaction, channel_or_role_id).await?,
        ButtonType::GroupLinkQueueVc   => handle_grouplink_queuevc_selection(  ctx, interaction, channel_or_role_id).await?,
        ButtonType::GroupLinkRed       => handle_grouplink_red_selection(      ctx, interaction, channel_or_role_id).await?,
        ButtonType::GroupLinkBlue      => handle_grouplink_blue_selection(     ctx, interaction, channel_or_role_id, db, manager).await?,

        // Unknown button types are ignored
        _ => {}
    }

    Ok(())
}

/// Handles dashboard channel selection
async fn handle_dashboard_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
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
            "Dashboard channel: <#{channel_id}>\n\n\
            **Step 2/7: Queue Text Channel**\n\
            Select the text channel where players will use queue commands:",
        ))
        .color(GREEN);

    let channels = get_text_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "queue");

    let select_menu = CSM::new("setup_queue", CSMK::String { options: channel_options })
        .placeholder("Select queue channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles queue channel selection
async fn handle_queue_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
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
            "Queue text channel: <#{channel_id}>\n\n\
            **Step 3/7: Queue Voice Channel**\n\
            Select the voice channel where players will wait in queue:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "queuevc");

    let select_menu = CSM::new("setup_queuevc", CSMK::String { options: channel_options })
        .placeholder("Select queue voice channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles queue voice channel selection
async fn handle_queue_vc_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
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
            "Queue voice channel: <#{channel_id}>\n\n\
            **Step 4/7: Red Team Voice Channel**\n\
            Select the voice channel for the Red team:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "red");

    let select_menu = CSM::new("setup_red", CSMK::String { options: channel_options })
        .placeholder("Select red team voice channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles red team channel selection
async fn handle_red_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
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
            "Red team channel: <#{channel_id}>\n\n\
            **Step 5/7: Blue Team Voice Channel**\n\
            Select the voice channel for the Blue team:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "blue");

    let select_menu = CSM::new("setup_blue", CSMK::String { options: channel_options })
        .placeholder("Select blue team voice channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles blue team channel selection
async fn handle_blue_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
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
            "Blue team channel: <#{channel_id}>\n\n\
            **Step 6/7: Runner Role**\n\
            Select the role that can manage PUG games:",
        ))
        .color(GREEN);

    let roles = get_guild_roles(&guild).await?;
    let role_options = create_role_options(&roles, "runner");

    let select_menu = CSM::new("setup_runner", CSMK::String { options: role_options })
        .placeholder("Select runner role...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles runner role selection
async fn handle_runner_selection(ctx: &Context, interaction: &CX, role_id: u64) -> Result<()> {
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
            "Runner role: <@&{role_id}>\n\n\
            **Step 7/7: Admin Role**\n\
            Select the role that can configure the bot:"
        ))
        .color(GREEN);

    let roles = get_guild_roles(&guild).await?;
    let role_options = create_role_options(&roles, "admin");

    let select_menu = CSM::new("setup_admin", CSMK::String { options: role_options })
        .placeholder("Select admin role...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(
        CIRM::new()
            .embed(embed)
            .components(vec![action_row])
    );

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles admin role selection and completes setup
async fn handle_admin_selection(ctx: &Context, interaction: &CX, role_id: u64, db: &Arc<Database>, manager: &Arc<Mutex<Manager>>) -> Result<()> {
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
                .title("Setup Error")
                .description("Configuration is incomplete. Please restart the setup process.")
                .color(RED);

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
    let queue_channel     = config.queue_channel    .unwrap();
    let queue_vc_channel  = config.queue_vc_channel .unwrap();
    let red_channel       = config.red_channel      .unwrap();
    let blue_channel      = config.blue_channel     .unwrap();
    let runner_role       = config.runner_role      .unwrap();
    let admin_role        = role_id;

    // Create the initial dashboard message
    let dashboard_channel_id = CI::new(dashboard_channel);
    let initial_embed = CE::new()
        .title("PUG Queue Dashboard")
        .description("Queue is empty. Be the first to join!")
        .color(CYAN);

    let dashboard_message = match dashboard_channel_id.send_message(&ctx.http, CreateMessage::new().embed(initial_embed)).await {
        Ok(msg) => msg,
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create dashboard message: {e}"))
                .color(RED);

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
        warn!("Failed to save runner_role config: {e}");
    }
    if let Err(e) = db.config.set_config("admin_role", &admin_role.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {e}");
    }

    // Create/validate rank roles
    let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
    info!("[{}] Creating/validating rank roles", guild_name);
    if let Err(e) = crate::handlers::player::create_rank_roles(ctx, db, guild_id).await {
        warn!("[{}] Failed to create rank roles: {}", guild_name, e);
    }

    // Create the group configuration in database
    let group_config = crate::database::repositories::group::GroupConfig {
        dashboard_channel_id: dashboard_channel,
        chat_channel_id: queue_channel,
        queue_vc_id: queue_vc_channel,
        red_vc_id: red_channel,
        blu_vc_id: blue_channel,
        quota: crate::DEFAULT_QUOTA,
    };
    match db.groups.create_group(
        guild_id.get(),
        dashboard_msg_id,
        group_config,
    ).await {
        Ok(_) => {
            info!("[{}] Group configuration saved to database", guild_name);

            // Load group into manager and immediately update dashboard
            if let Err(e) = finalize_group_setup(ctx, db, manager, guild_id, dashboard_msg_id).await {
                warn!("[{}] Failed to finalize group setup: {}", guild_name, e);
            } else {
                SETUP_STATE.complete_setup(user_id, guild_id);
            }

            let success_embed = CE::new()
                .title("Setup Complete!")
                .description(format!(
                    "Your PUG bot is now fully configured and ready to use!\n\n\
                    **Configuration Summary:**\n\
                    • Dashboard: <#{dashboard_channel}>\n\
                    • Queue Text: <#{queue_channel}>\n\
                    • Queue Voice: <#{queue_vc_channel}>\n\
                    • Red Team: <#{red_channel}>\n\
                    • Blue Team: <#{blue_channel}>\n\
                    • Runner Role: <@&{runner_role}>\n\
                    • Admin Role: <@&{admin_role}>\n\
                    • Rank Roles: Created\n\n\
                    **The dashboard is ready!** Players can now:\n\
                    • Click \"Join\" to queue up or \"Leave\" to exit the queue\n\
                    • Join the queue voice channel to auto-queue\n\n\
                    Runners can use the dashboard buttons to manage matches.",
                ))
                .color(GREEN);

            let response = CIR::UpdateMessage(CIRM::new().embed(success_embed).components(vec![]));

            interaction.create_response(&ctx.http, response).await?;
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to save configuration: {e}"))
                .color(RED);

            let response = CIR::UpdateMessage(CIRM::new().embed(error_embed).components(vec![]));

            interaction.create_response(&ctx.http, response).await?;
        }
    }

    Ok(())
}

// ==================== INIT GROUP HANDLERS ====================

/// Handles queue channel selection for init_group
async fn handle_init_queue_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
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
            "Queue text channel: <#{channel_id}>\n\n\
            **Step 3/5: Queue Voice Channel**\n\
            Select the voice channel players will join for the queue:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "init_queuevc");

    let select_menu = CSM::new("init_queuevc", CSMK::String { options: channel_options })
        .placeholder("Select queue voice channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles queue voice channel selection for init_group
async fn handle_init_queue_vc_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
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
            "Queue voice channel: <#{channel_id}>\n\n\
            **Step 4/5: Red Team Voice Channel**\n\
            Select the voice channel for the Red team:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "init_red");

    let select_menu = CSM::new("init_red", CSMK::String { options: channel_options })
        .placeholder("Select red team voice channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles red team channel selection for init_group
async fn handle_init_red_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
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
            "Red team channel: <#{channel_id}>\n\n\
            **Step 5/5: Blue Team Voice Channel**\n\
            Select the voice channel for the Blue team:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "init_blue");

    let select_menu = CSM::new("init_blue", CSMK::String { options: channel_options })
        .placeholder("Select blue team voice channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles blue team channel selection for init_group flow (created channels)
async fn handle_init_blue_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Err(anyhow!("Guild ID not found - setup must be run in a server"))
    };
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;

    // Store the selection
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.blue_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Blue Team Channel Selected")
        .description(format!(
            "Blue team channel: <#{channel_id}>\n\n\
            **Step 2/2: Admin Role**\n\
            Select the role for bot administrators:",
        ))
        .color(GREEN);

    let roles = get_guild_roles(&guild).await?;
    let role_options = create_role_options(&roles, "init_admin");

    let select_menu = CSM::new("init_admin", CSMK::String { options: role_options })
        .placeholder("Select admin role...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles runner role selection for init_group flow
async fn handle_init_runner_selection(ctx: &Context, interaction: &CX, role_id: u64) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Err(anyhow!("Guild ID not found - setup must be run in a server"))
    };
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let user_id = interaction.user.id;

    // Store the selection
    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.runner_role = Some(role_id);
    });

    let embed = CE::new()
        .title("Runner Role Selected")
        .description(format!(
            "Runner role: <@&{role_id}>\n\n\
            **Step 2/2: Admin Role**\n\
            Select the role for bot administrators:",
        ))
        .color(GREEN);

    let roles = get_guild_roles(&guild).await?;
    let role_options = create_role_options(&roles, "init_admin");

    let select_menu = CSM::new("init_admin", CSMK::String { options: role_options })
        .placeholder("Select admin role...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));

    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles admin role selection and completes init_group setup
async fn handle_init_admin_selection(ctx: &Context, interaction: &CX, role_id: u64, db: &std::sync::Arc<crate::Database>, manager: &Arc<Mutex<crate::models::Manager>>) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Err(anyhow!("Guild ID not found - setup must be run in a server"))
    };
    let user_id = interaction.user.id;

    // Store the admin role selection
    let config = SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.admin_role = Some(role_id);
    });

    // Validate all required fields are present
    let config = match config {
        Some(cfg) if cfg.dashboard_channel.is_some()
                  && cfg.dashboard_msg_id .is_some()
                  && cfg.queue_channel    .is_some()
                  && cfg.queue_vc_channel .is_some()
                  && cfg.red_channel      .is_some()
                  && cfg.blue_channel     .is_some()
                  && cfg.runner_role      .is_some()
                  && cfg.admin_role       .is_some() => cfg,
        _ => {
            let error_embed = CE::new()
                .title("Setup Error")
                .description("Configuration is incomplete. Please restart the setup process.")
                .color(RED);

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
    let blue_channel      = config.blue_channel     .unwrap();
    let runner_role       = config.runner_role      .unwrap();
    let admin_role        = role_id;

    // Save role configurations to database
    if let Err(e) = db.config.set_config("runner_role", &runner_role.to_string(), guild_id.get()).await {
        warn!("Failed to save runner_role config: {e}");
    }
    if let Err(e) = db.config.set_config("admin_role", &admin_role.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {e}");
    }

    // Create/validate rank roles
    let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
    info!("[{}] Creating/validating rank roles", guild_name);
    if let Err(e) = create_rank_roles(ctx, db, guild_id).await {
        warn!("[{}] Failed to create rank roles: {}", guild_name, e);
    }

    // Create the group configuration in database with actual dashboard message ID
    let group_config = crate::database::repositories::group::GroupConfig {
        dashboard_channel_id: dashboard_channel,
        chat_channel_id: queue_channel,
        queue_vc_id: queue_vc_channel,
        red_vc_id: red_channel,
        blu_vc_id: blue_channel,
        quota: DEFAULT_QUOTA,
    };
    match db.groups.create_group(
        guild_id.get(),
        dashboard_msg_id, // Real dashboard message ID from step 1
        group_config,
    ).await {
        Ok(_) => {
            info!("[{}] Group configuration saved to database", guild_name);

            // Load group into manager and immediately update dashboard
            if let Err(e) = finalize_group_setup(ctx, db, manager, guild_id, dashboard_msg_id).await {
                warn!("[{}] Failed to finalize group setup: {}", guild_name, e);
            } else {
                info!("[{}] Group setup finalized successfully", guild_name);
            }
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create group configuration: {e}"))
                .color(RED);

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
            • Dashboard: <#{dashboard_channel}>\n\
            • Queue Text: <#{queue_channel}>\n\
            • Queue Voice: <#{queue_vc_channel}>\n\
            • Red Team: <#{red_channel}>\n\
            • Blue Team: <#{blue_channel}>\n\n\
            The dashboard has been initialized in <#{dashboard_channel}> with the interactive queue interface!",
        ))
        .color(GREEN);

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
    if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");

    // Check for missing system roles (Runner and Admin)
    let missing_system_roles = match validate_system_roles(cc.ctx, &cc.db, guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            let error_embed = CE::new()
                .title("Error")
                .description(format!("Failed to check system roles: {e}"))
                .color(RED);

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
                .title("Error")
                .description(format!("Failed to check rank roles: {e}"))
                .color(RED);

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
            .color(GREEN);

        let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        // Build description for missing roles
        let mut description = String::new();

        if !missing_system_roles.is_empty() {
            let system_list = missing_system_roles.join(", ");
            description.push_str(&format!(
                "**Missing System Roles:**\n{system_list}\n\n\
                 System roles should be created manually and assigned appropriate permissions.\n\n",
            ));
        }

        if !missing_rank_roles.is_empty() {
            let rank_list = missing_rank_roles.join(", ");
            description.push_str(&format!(
                "**Missing Rank Roles:**\n{rank_list}\n\n\
                Would you like me to create these rank roles automatically?\n\n\
                 Note: The roles will be created but you may need to adjust their permissions and position in the role hierarchy.",
            ));
        }

        let embed = CE::new()
            .title("Missing Roles")
            .description(description)
            .color(ORANGE);

        // Only add create button if there are rank roles to create
        if !missing_rank_roles.is_empty() {
            use serenity::all::{CreateButton, ButtonStyle};
            let yes_button = CreateButton::new("create_rank_roles_yes")
                .label("Create Rank Roles")
                .style(ButtonStyle::Success);

            let no_button = CreateButton::new("create_rank_roles_no")
                .label("Cancel")
                .style(ButtonStyle::Secondary);

            let buttons = CAR::Buttons(vec![yes_button, no_button]);

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
pub async fn handle_create_rank_roles(ctx: &Context, db: &crate::Database, interaction: &CX, create: bool) -> Result<()> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Err(anyhow!("Guild ID not found - this command must be run in a server"))
    };

    if !create {
        // User cancelled
        let cancel_embed = CE::new()
            .title("Cancelled")
            .description("Rank role creation was cancelled.")
            .color(GRAY);

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
                .title("Error")
                .description(format!("Failed to create rank roles: {e}"))
                .color(RED);

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
            .title("No Roles Created")
            .description("All rank roles already exist in this server.")
            .color(CYAN);

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
                "Successfully created the following rank roles:\n**{created_list}**\n\n\
                You may want to:\n\
                • Adjust role positions in Server Settings\n\
                • Configure role permissions\n\
                • Assign roles to existing members",
            ))
            .color(GREEN);

        let response = CIR::UpdateMessage(
            CIRM::new()
                .embed(success_embed)
                .components(vec![])
        );

        interaction.create_response(&ctx.http, response).await?;
    }

    Ok(())
}

/// `/quotaset` - Set the queue quota for the current group
///
/// * `quota` - The new quota value (number of players required to start a game)
pub async fn cmd_set_quota(cc: &CC<'_>, quota: i64) -> Result<()> {
    if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    // Validate quota range
    if !(2..=100).contains(&quota) {
        let error_embed = CE::new()
            .title("Invalid Quota")
            .description("Quota must be between 2 and 100 players.")
            .color(RED);

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
                .title("Server Not Found")
                .description(format!("Server not configured: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    let group = match server.get_group(cc.intax.channel_id) {
        Ok(g) => g,
        Err(e) => {
            let error_embed = CE::new()
                .title("Group Not Found")
                .description(format!("No queue group found in this channel: {e}"))
                .color(RED);

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
            let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
            info!("[{}] Updated quota from {} to {}", guild_name, old_quota, quota);

            let success_embed = CE::new()
                .title("Quota Updated")
                .description(format!(
                    "Queue quota has been changed from **{old_quota}** to **{quota}** players.\n\n\
                    The queue will now require {quota} players before a game can start.",
                ))
                .color(GREEN);

            let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;

            // Update the dashboard to reflect the new quota
            group.queue_dash_update(cc.ctx, cc.intax.guild_id.unwrap().get()).await;
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("Failed to Update Quota")
                .description(format!("Failed to save quota to database: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }

    Ok(())
}

/// `/connectadd` - Set server connection info for the current group
///
/// * `connect_info` - The server connect command (e.g., "connect 1.1.1.1:1234; password 1234")
pub async fn cmd_add_connect(cc: &CC<'_>, connect_info: String) -> Result<()> {
    if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");

    // Get the group from the current channel
    let mut manager = cc.manager.lock().await;
    let server = match manager.get_server(guild_id) {
        Ok(s) => s,
        Err(e) => {
            let error_embed = CE::new()
                .title("Server Not Found")
                .description(format!("Server not configured: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    let group = match server.get_group(cc.intax.channel_id) {
        Ok(g) => g,
        Err(e) => {
            let error_embed = CE::new()
                .title("Group Not Found")
                .description(format!("No queue group found in this channel: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Update the connect info in the group
    group.connect_info = Some(connect_info.clone());

    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
    info!("[{}] Set server connect info for group {}", guild_name, group.group_id);

    let success_embed = CE::new()
        .title("Server Connect Info Updated")
        .description(format!(
            "Server connection command has been set:\n\n```{connect_info}```\n\n\
            This will now appear on the dashboard when players are ready to join.",
        ))
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Update the dashboard to show the new connect info
    group.queue_dash_update(cc.ctx, cc.intax.guild_id.unwrap().get()).await;

    Ok(())
}

/// Helper function to finalize group setup by loading it into manager and immediately updating dashboard
async fn finalize_group_setup(ctx: &Context, db: &Arc<Database>, manager: &Arc<Mutex<Manager>>, guild_id: GI, dashboard_msg_id: u64) -> Result<()> {
    let guild_name = ctx.cache.guild(guild_id)
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    // Load the group from database
    match db.groups.get_groups_for_guild(guild_id.get()).await {
        Ok(groups) if !groups.is_empty() => {
            let new_group = groups.into_iter()
                .find(|g| g.dashboard_msg.get() == dashboard_msg_id)
                .ok_or_else(|| anyhow!("Could not find newly created group"))?;

            use crate::models::Server;
            let mut mgr = manager.lock().await;

            // Ensure server exists in manager
            if mgr.get_server(guild_id).is_err() {
                let server = Server::empty(guild_id, guild_name.clone());
                mgr.servers.push(server);
            }

            let server = mgr.get_server(guild_id)?;
            server.add_group(new_group)?;

            // Get the newly added group and immediately update its dashboard
            let group = server.groups.last_mut()
                .ok_or_else(|| anyhow!("Failed to get newly added group"))?;

            // Use dash_update for immediate synchronous update instead of queued async update
            if let Err(e) = group.dash_update(ctx).await {
                warn!("[{}] Failed to update dashboard: {}", guild_name, e);
            } else {
                info!("[{}] Group added to manager and dashboard updated successfully", guild_name);
            }

            Ok(())
        },
        Ok(_) => {
            Err(anyhow!("[{}] No groups found after creation", guild_name))
        },
        Err(e) => {
            Err(anyhow!("[{}] Failed to load groups from database: {}", guild_name, e))
        }
    }
}

/// Handles grouplink dashboard channel selection - Step 1
async fn handle_grouplink_dashboard_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.dashboard_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Dashboard Channel Selected")
        .description(format!(
            "Dashboard: <#{channel_id}>\n\n\
            **Step 2/5: Queue Text Channel**\n\
            Select the text channel for queue commands:",
        ))
        .color(GREEN);

    let channels = get_text_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "grouplink_queue");

    let select_menu = CSM::new("grouplink_queue", CSMK::String { options: channel_options })
        .placeholder("Select queue text channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles grouplink queue text channel selection - Step 2
async fn handle_grouplink_queue_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Queue Text Channel Selected")
        .description(format!(
            "Queue text: <#{channel_id}>\n\n\
            **Step 3/5: Queue Voice Channel**\n\
            Select the voice channel where players wait:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "grouplink_queuevc");

    let select_menu = CSM::new("grouplink_queuevc", CSMK::String { options: channel_options })
        .placeholder("Select queue voice channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles grouplink queue voice channel selection - Step 3
async fn handle_grouplink_queuevc_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.queue_vc_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Queue Voice Channel Selected")
        .description(format!(
            "Queue voice: <#{channel_id}>\n\n\
            **Step 4/5: Red Team Voice Channel**\n\
            Select the Red team voice channel:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "grouplink_red");

    let select_menu = CSM::new("grouplink_red", CSMK::String { options: channel_options })
        .placeholder("Select red team channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles grouplink red team channel selection - Step 4
async fn handle_grouplink_red_selection(ctx: &Context, interaction: &CX, channel_id: u64) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;

    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.red_channel = Some(channel_id);
    });

    let embed = CE::new()
        .title("Red Team Channel Selected")
        .description(format!(
            "Red team: <#{channel_id}>\n\n\
            **Step 5/5: Blue Team Voice Channel**\n\
            Select the Blue team voice channel:",
        ))
        .color(GREEN);

    let channels = get_voice_channels(&guild, ctx).await?;
    let channel_options = create_channel_options(&channels, "grouplink_blue");

    let select_menu = CSM::new("grouplink_blue", CSMK::String { options: channel_options })
        .placeholder("Select blue team channel...")
        .max_values(1);

    let action_row = CAR::SelectMenu(select_menu);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![action_row]));
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Handles grouplink blue team channel selection - Step 5 (final step, creates the group)
async fn handle_grouplink_blue_selection(ctx: &Context, interaction: &CX, channel_id: u64, db: &std::sync::Arc<crate::Database>, manager: &Arc<Mutex<crate::models::Manager>>) -> Result<()> {
    let user_id = interaction.user.id;
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

    SETUP_STATE.update_setup(user_id, guild_id, |config| {
        config.blue_channel = Some(channel_id);
    });

    // Get complete configuration
    let config = SETUP_STATE.get_setup(user_id, guild_id)
        .ok_or_else(|| anyhow!("Setup configuration not found"))?;

    let dashboard_channel = CI::new(config.dashboard_channel.ok_or_else(|| anyhow!("Dashboard channel not set"))?);
    let queue_channel = CI::new(config.queue_channel.ok_or_else(|| anyhow!("Queue channel not set"))?);
    let queue_vc_channel = CI::new(config.queue_vc_channel.ok_or_else(|| anyhow!("Queue VC channel not set"))?);
    let red_channel = CI::new(config.red_channel.ok_or_else(|| anyhow!("Red channel not set"))?);
    let blue_channel = CI::new(config.blue_channel.ok_or_else(|| anyhow!("Blue channel not set"))?);

    // Send "creating group" message
    let loading_embed = CE::new()
        .title("Creating Group")
        .description("Linking channels and creating PUG group...\n\nCleaning up any old configurations...")
        .color(ORANGE);

    let response = CIR::UpdateMessage(CIRM::new().embed(loading_embed).components(vec![]));
    interaction.create_response(&ctx.http, response).await?;

    // Check and clean up old groups that use these channels
    let mut mgr = manager.lock().await;
    let server_opt = mgr.servers.iter_mut().find(|s| s.guild_id == guild_id);

    if let Some(server) = server_opt {
        let mut groups_to_remove = Vec::new();

        for (idx, group) in server.groups.iter().enumerate() {
            if group.channels.dashboard == dashboard_channel ||
               group.channels.queue_chat == queue_channel ||
               group.channels.queue_vc == queue_vc_channel ||
               group.channels.teams.iter().any(|t| t.red_vc == red_channel || t.blu_vc == blue_channel) {
                groups_to_remove.push((idx, group.group_id));
            }
        }

        // Remove old configurations from database and memory
        for (idx, group_id) in groups_to_remove.iter().rev() {
            info!("[{}] Removing old group {} configuration", guild_name, group_id);
            if let Err(e) = db.groups.delete(*group_id).await {
                warn!("[{}] Failed to delete old group {}: {}", guild_name, group_id, e);
            }
            server.groups.remove(*idx);
        }
    }
    drop(mgr);

    // Create temporary group and publish dashboard
    use crate::models::{Group, Channels, TeamChannel};
    use serenity::all::MessageId;

    let mut temp_group = Group {
        group_id: 0,
        name: None,
        quota: crate::DEFAULT_QUOTA,
        timeout: crate::DEFAULT_TIMEOUT,
        dashboard_msg: MessageId::new(1),
        channels: Channels {
            queue_chat: queue_channel,
            queue_vc: queue_vc_channel,
            teams: vec![TeamChannel {
                red_vc: red_channel,
                blu_vc: blue_channel,
            }],
            dashboard: dashboard_channel,
        },
        sessions: vec![],
        connect_info: None,
    };

    // Publish dashboard to get message ID
    match temp_group.dash_publish(ctx, dashboard_channel, db, guild_id.get()).await {
        Ok(_) => {
            let dashboard_msg_id = temp_group.dashboard_msg.get();

            // Save to database
            let group_config = crate::database::repositories::group::GroupConfig {
                dashboard_channel_id: dashboard_channel.get(),
                chat_channel_id: queue_channel.get(),
                queue_vc_id: queue_vc_channel.get(),
                red_vc_id: red_channel.get(),
                blu_vc_id: blue_channel.get(),
                quota: crate::DEFAULT_QUOTA,
            };
            match db.groups.create_group(
                guild_id.get(),
                dashboard_msg_id,
                group_config,
            ).await {
                Ok(db_group) => {
                    info!("[{}] Group {} created via grouplink", guild_name, db_group.group_id);

                    // Add to manager
                    let mut mgr = manager.lock().await;
                    if let Ok(server) = mgr.get_server(guild_id) {
                        if let Err(e) = server.add_group(db_group.clone()) {
                            error!("[{}] Failed to add group: {}", guild_name, e);
                        }
                    }
                    drop(mgr);

                    // Clean up setup state
                    SETUP_STATE.complete_setup(user_id, guild_id);

                    let success_embed = CE::new()
                        .title("Group Created!")
                        .description(format!(
                            "Successfully linked existing channels!\n\n\
                            **Configuration:**\n\
                            • Dashboard: <#{}>\n\
                            • Queue Text: <#{}>\n\
                            • Queue Voice: <#{}>\n\
                            • Red Team: <#{}>\n\
                            • Blue Team: <#{}>\n\n\
                            The PUG queue is now ready to use!",
                            dashboard_channel.get(),
                            queue_channel.get(),
                            queue_vc_channel.get(),
                            red_channel.get(),
                            blue_channel.get()
                        ))
                        .color(GREEN);

                    interaction.edit_response(&ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(success_embed)
                    ).await?;
                },
                Err(e) => {
                    // Delete dashboard message on failure
                    let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;

                    let error_embed = CE::new()
                        .title("Failed to Save Group")
                        .description(format!("Error saving to database: {e}"))
                        .color(RED);

                    interaction.edit_response(&ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(error_embed)
                    ).await?;
                }
            }
        },
        Err(e) => {
            let error_embed = CE::new()
                .title("Dashboard Creation Failed")
                .description(format!("Failed to create dashboard: {e}"))
                .color(RED);

            interaction.edit_response(&ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
        }
    }

    Ok(())
}

/// `/ranksetelo` - Set custom ELO value for a rank
///
/// * `rank_role` - The rank name or role mention/ID
/// * `elo` - The ELO value to set for this rank
pub async fn cmd_rank_set_elo(cc: &CC<'_>, rank_role: String, elo: i64) -> Result<()> {
    // Check admin permissions
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

    // Validate ELO range (1-100)
    if elo < 1 {
        let error_embed = CE::new()
            .title("Invalid ELO Value")
            .description("ELO must be above 0")
            .color(RED);
        let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Try to parse as rank name first
    let rank = parse_rank_name(&rank_role).expect("Invalid rank name");
    // Verify this rank has a role configured
    let config_key = format!("rank_{}_role", rank.name().to_lowercase().replace(" ", "_"));
    match cc.db.config.get_config_value(&config_key, guild_id.get()).await {
        Ok(Some(_)) => rank,
        _ => {
            error!("[{}] - Invalid rank name: {}", guild_name, rank_role);
            let error_embed = CE::new()
                .title("Rank Not Configured")
                .description(format!("Rank '{rank_role}' is not configured.\nUse `/check_ranks` to set up rank roles first."))
                .color(RED);
            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };
    let config_key = format!("rank_{}_elo", rank.name().to_lowercase().replace(" ", "_"));

    // Store ELO value in config
    cc.db.config.set_config(&config_key, &elo.to_string(), guild_id.get()).await?;

    // Get role ID for display in success message
    let role_config_key = format!("rank_{}_role", rank.name().to_lowercase().replace(" ", "_"));
    let role_id = cc.db.config.get_config_value(&role_config_key, guild_id.get()).await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let success_embed = CE::new()
        .title("Rank ELO Updated")
        .description(format!(
            "**{}** rank (<@&{}>) ELO set to **{}**\n\n\
            *Note: This will take effect for new team generations.*",
            rank.name(),
            role_id,
            elo
        ))
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    info!("[{}] - {} = {}", guild_name, rank.name(), elo);

    Ok(())
}

/// `/setplayerelo` - Set ELO value for a specific player
///
/// * `user` - The Discord user (mention or ID)
/// * `elo` - The ELO value to set for this player (0-100, or -1 to clear)
pub async fn cmd_set_player_elo(cc: &CC<'_>, user: serenity::all::User, elo: i64) -> Result<()> {
    info!("DEBUG: cmd_set_player_elo called for user {} with elo {}", user.tag(), elo);
    
    // Check admin permissions
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let user_id = user.id;

    // Validate ELO range
    if elo != -1 && (elo < ELO_MIN as i64 || elo > ELO_MAX as i64) {
        let error_embed = CE::new()
            .title("Invalid ELO Value")
            .description(format!("ELO must be between {ELO_MIN} and {ELO_MAX}, or -1 to clear ELO"))
            .color(RED);
        let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Set guild-specific ELO value
    let elo_value = elo as u16;
    info!("DEBUG: Setting guild ELO value {} for user {} in guild {}", elo_value, user_id, guild_id);
    cc.db.elos.update_elo(user_id, guild_id.get(), elo_value).await?;

    // Get updated player info with guild-specific ELO
    let guild_elo = cc.db.elos.get(user_id, guild_id.get()).await?;
    info!("DEBUG: After update - Player ELO: {}, Division: {}", guild_elo.elo, guild_elo.division.name());

    let success_embed = CE::new()
        .title("Player ELO Updated")
        .description(format!(
            "**{}**'s ELO set to **{}**\nCurrent division: **{}**",
            user.tag(),
            guild_elo.elo,
            guild_elo.division.name()
        ))
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/getplayerelo` - View ELO and rank information for a player
///
/// * `user` - The Discord user (mention or ID, optional - defaults to command user)
pub async fn cmd_get_player_elo(cc: &CC<'_>, user: Option<serenity::all::User>) -> Result<()> {
    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let user_id = user.as_ref().map(|u| u.id).unwrap_or(cc.intax.user.id);
    let is_self = user_id == cc.intax.user.id;

    if !is_self && !check_role(cc, &Role::Admin).await? {
        return Ok(());
    }

    // Get guild-specific ELO data
    let guild_elo = cc.db.elos.get(user_id, guild_id.get()).await?;
    
    // Get base player info for steam_id
    let player = match cc.db.users.get(user_id).await {
        Ok(p) => p,
        Err(_) => {
            let error_embed = CE::new()
                .title("Player Not Found")
                .description(format!("<@{}> is not in the database.", user_id))
                .color(RED);
            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    info!("DEBUG: User {} - Guild ELO: {}, Division: {}, Games: {}, Wins: {}", 
          user_id, guild_elo.elo, guild_elo.division.name(), guild_elo.games, guild_elo.wins);

    // Get user info - if no user provided, we can't continue
    let user_info = user.ok_or_else(|| {
        let _error_embed = CE::new()
            .title("User Required")
            .description("You must specify a user to view their ELO information, or use the command on yourself.")
            .color(RED);
        anyhow::anyhow!("User not provided")
    })?;

    // Create embed with player info
    let mut embed = CE::new()
        .title(format!("{}'s ELO Information", user_info.tag()))
        .color(CYAN);

    // ELO information
    embed = embed.field("ELO Rating", format!("**{}** / 100", guild_elo.elo), true);

    // Division information
    embed = embed.field("Division", format!("**{}**", guild_elo.division.name()), true);

    // Stats
    let win_rate = if guild_elo.games > 0 {
        format!("{:.1}%", (guild_elo.wins as f64 / guild_elo.games as f64) * 100.0)
    } else {
        "N/A".to_string()
    };
    embed = embed.field("Games", format!("**{}** ({} wins, {} win rate)", guild_elo.games, guild_elo.wins, win_rate), false);

    // Additional info
    embed = embed.field("Discord ID", format!("`{}`", user_id), false)
        .field("Steam ID", player.steam_id.map(|id| format!("`{id}`")).unwrap_or_else(|| "*Not linked*".to_string()), false);

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/enableactiveelo` - Enable automatic ELO adjustments from match results
pub async fn cmd_enable_active_elo(cc: &CC<'_>) -> Result<()> {
    // Check admin permissions
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    
    // Enable active ELO in config
    cc.db.config.set_config("active_elo_enabled", "true", guild_id.get()).await?;

    let success_embed = CE::new()
        .title("Active ELO Enabled")
        .description("Automatic ELO adjustments from match results are now **enabled**.\n\n*Note: This requires webhooks and game server API to be configured to actually work.*")
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/disableactiveelo` - Disable automatic ELO adjustments from match results
pub async fn cmd_disable_active_elo(cc: &CC<'_>) -> Result<()> {
    // Check admin permissions
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    
    // Disable active ELO in config
    cc.db.config.set_config("active_elo_enabled", "false", guild_id.get()).await?;

    let success_embed = CE::new()
        .title("Active ELO Disabled")
        .description("Automatic ELO adjustments from match results are now **disabled**.")
        .color(ORANGE);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/activeelostatus` - Check if automatic ELO adjustments are enabled
pub async fn cmd_active_elo_status(cc: &CC<'_>) -> Result<()> {
    // Check admin permissions
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    
    // Check current status
    let is_enabled = match cc.db.config.get_config_value("active_elo_enabled", guild_id.get()).await {
        Ok(Some(value)) => value.parse::<bool>().unwrap_or(crate::ACTIVE_ELO_ENABLED_BY_DEFAULT),
        Ok(None) => crate::ACTIVE_ELO_ENABLED_BY_DEFAULT,
        Err(_) => crate::ACTIVE_ELO_ENABLED_BY_DEFAULT,
    };

    let status_embed = CE::new()
        .title("Active ELO Status")
        .description(format!(
            "Automatic ELO adjustments are currently **{}**\n\n\
            When enabled, this feature will:\n\
            • Receive match results from game server API\n\
            • Automatically adjust player ELO based on wins/losses\n\
            • Update player ranks based on new ELO values\n\n\
            *Note: Webhooks and game server integration required for full functionality.*",
            if is_enabled { "✅ ENABLED" } else { "❌ DISABLED" }
        ))
        .color(if is_enabled { GREEN } else { RED });

    let response = CIR::Message(CIRM::new().embed(status_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/setplayersteam` - Set Steam ID for a specific player
pub async fn cmd_set_player_steam(cc: &CC<'_>, user: serenity::all::User, steam_id: u64) -> Result<()> {
    // Check admin permissions
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let user_id = user.id;
    let steam_id_value = if steam_id == 0 { None } else { Some(steam_id) };

    // Update Steam ID in database
    cc.db.users.update_steam_id(&user_id, steam_id_value).await?;

    let success_embed = CE::new()
        .title("Steam ID Updated")
        .description(format!(
            "**{}**'s Steam ID set to **{}**",
            user.tag(),
            if let Some(sid) = steam_id_value {
                format!("`{}`", sid)
            } else {
                "Cleared".to_string()
            }
        ))
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

// RUNNER COMMANDS

/// `/buffer`
///
/// * `user_id` - The user ID to buffer.
/// * `server` - The server (already has manager lock held by caller)
pub async fn cmd_buffer(cc: &CC<'_>, server: &mut Server, user_id: UI) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

    info!("[{}] Getting group from channel {}", guild_name, cc.intax.channel_id);
    // Get the group from the current channel
    let group = match server.get_group(cc.intax.channel_id) {
        Ok(g) => g,
        Err(e) => {
            warn!("[{}] No group found in channel {}: {}", guild_name, cc.intax.channel_id, e);
            let error_embed = CE::new()
                .title("Group Not Found")
                .description(format!("No queue group found in this channel: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    info!("[{}] Finding session for user {}", guild_name, user_id);
    // Find the session containing the player
    let session = match group.get_user_session(user_id).await {
        Ok(s) => s,
        Err(e) => {
            warn!("[{}] User {} not found in any session: {}", guild_name, user_id, e);
            let error_embed = CE::new()
                .title("Player Not Found")
                .description(format!("<@{user_id}> is not in any queue."))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Find the player's index in the pool
    let player_idx = match session.pool.iter().position(|p| p.player.user_id == user_id) {
        Some(idx) => idx,
        None => {
            error!("[{}] Player {} not found in pool despite being in session", guild_name, user_id);
            let error_embed = CE::new()
                .title("Player Not Found")
                .description(format!("<@{user_id}> is not in the queue pool."))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Remove the player from their current position
    let player = session.pool.remove(player_idx);

    // Insert the player at the front of the queue (index 0)
    session.pool.insert(0, player);

    let is_hot = session.is_hot();

    // If session is hot, regenerate teams with new order
    if is_hot {
        group.generate_teams(cc.ctx, guild_id, Some(&cc.db)).await;
    }

    group.queue_dash_update(cc.ctx, guild_id.get()).await;

    let success_embed = CE::new()
        .title("Player Buffered")
        .description(format!("<@{user_id}> moved to front of queue."))
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/fatkid`
///
/// * `user_id` - The user ID to fatkid (move to end of queue).
/// * `server` - The server (already has manager lock held by caller)
pub async fn cmd_fatkid(cc: &CC<'_>, server: &mut Server, user_id: UI) -> Result<()> {
    if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

    info!("[{}] Getting group from channel {}", guild_name, cc.intax.channel_id);
    // Get the group from the current channel
    let group = match server.get_group(cc.intax.channel_id) {
        Ok(g) => g,
        Err(e) => {
            warn!("[{}] No group found in channel {}: {}", guild_name, cc.intax.channel_id, e);
            let error_embed = CE::new()
                .title("Group Not Found")
                .description(format!("No queue group found in this channel: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    info!("[{}] Finding session for user {}", guild_name, user_id);
    // Find the session containing the player
    let session = match group.get_user_session(user_id).await {
        Ok(s) => s,
        Err(e) => {
            warn!("[{}] User {} not found in any session: {}", guild_name, user_id, e);
            let error_embed = CE::new()
                .title("Player Not Found")
                .description(format!("<@{user_id}> is not in any queue."))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Find the player's index in the pool
    let player_idx = match session.pool.iter().position(|p| p.player.user_id == user_id) {
        Some(idx) => idx,
        None => {
            error!("[{}] Player {} not found in pool despite being in session", guild_name, user_id);
            let error_embed = CE::new()
                .title("Player Not Found")
                .description(format!("<@{user_id}> is not in the queue pool."))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Remove the player from their current position
    let player = session.pool.remove(player_idx);

    // Insert the player at the end of the queue
    session.pool.push(player);

    let is_hot = session.is_hot();

    // If session is hot, regenerate teams with new order
    if is_hot {
        group.generate_teams(cc.ctx, guild_id, Some(&cc.db)).await;
    }

    group.queue_dash_update(cc.ctx, guild_id.get()).await;

    let success_embed = CE::new()
        .title("Player Fatkidded")
        .description(format!("<@{user_id}> moved to end of queue."))
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/clear` - Clear all players from the queue
pub async fn cmd_clear_queue(cc: &CC<'_>, server: &mut Server) -> Result<()> {
    if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");

    // Get the group from the current channel
    let group = match server.get_group(cc.intax.channel_id) {
        Ok(g) => g,
        Err(e) => {
            let error_embed = CE::new()
                .title("Group Not Found")
                .description(format!("No queue group found in this channel: {e}"))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Get the idle session and clear it
    let player_count = match group.get_queue().await {
        Ok(session) => {
            let count = session.pool.len();
            session.pool.clear();
            count
        },
        Err(_) => 0
    };

    // Update the dashboard
    group.queue_dash_update(cc.ctx, guild_id.get()).await;

    let success_embed = CE::new()
        .title("Queue Cleared")
        .description(format!("Removed {player_count} player(s) from the queue."))
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}
