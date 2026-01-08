use anyhow::{Result, anyhow};
use serenity::all::{
    CreateEmbed                      as CE,
    CreateInteractionResponse        as CIR,
    CreateInteractionResponseMessage as CIRM,
    EditInteractionResponse          as EIR,
    GuildId                          as GI,
    Permissions,
};
use serenity::builder::EditRole as ER;
use tracing::{error, info, warn};

use crate::{ADMIN, GREEN, ORANGE, RED, RUNNER, Rank, Server};
use crate::admin::create_group_channels;
use crate::models::{CommandContext as CC, Role};
use crate::repositories::Repository;
use super::player::{check_role, create_rank_roles};
use super::settings::{build_settings_embed, build_settings_buttons, build_server_settings_embed, build_server_settings_buttons, get_server_settings};

/// Helper: Create a Discord role with error handling
async fn create_role_with_error(cc: &CC<'_>, guild_id: GI, name: &str, color: u32) -> Result<Option<serenity::all::Role>> {
    match guild_id.create_role(&cc.ctx.http,
        ER::new().name(name).colour(color).permissions(Permissions::empty())
    ).await {
        Ok(role) => Ok(Some(role)),
        Err(e) => {
            let error_embed = CE::new().title(format!("Failed to Create {name} Role"))
                .description(format!("Error: {e}")).color(RED);

            cc.intax.edit_response(&cc.ctx.http, EIR::new().embed(error_embed)).await?;
            Ok(None)
        }
    }
}

/// `/roleadd` - Create runner and admin roles for the bot
pub async fn cmd_role_add(cc: &CC<'_>) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    let loading_embed = CE::new()
        .title("Creating Roles")
        .description("Creating Runner and Admin roles...")
        .color(ORANGE);

    let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Create Runner role
    let runner_role = match create_role_with_error(cc, guild_id, "PUG Runner", RUNNER as u32).await? {
        Some(role) => role,
        None => return Ok(()),
    };

    // Create Admin role
    let admin_role = match create_role_with_error(cc, guild_id, "PUG Admin", ADMIN as u32).await? {
        Some(role) => role,
        None => return Ok(()),
    };

    // Save to database
    if let Err(e) = cc.db.config.set_config("runner_role", &runner_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save runner_role config: {e}");
    }
    if let Err(e) = cc.db.config.set_config("admin_role", &admin_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {e}");
    }

    // Create rank roles
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
    info!("[{}] Creating rank roles", guild_name);
    if let Err(e) = create_rank_roles(cc.ctx, &cc.db, guild_id).await {
        warn!("[{}] Failed to create rank roles: {}", guild_name, e);
    }

    let success_embed = CE::new()
        .title("Roles Created!")
        .description(format!(
            "Successfully created bot roles:\n\n\
            • Runner Role: <@&{}>\n\
            • Admin Role: <@&{}>\n\
            • Rank Roles: Created\n\n\
            **Note:** Assign these roles to users who should manage PUGs.",
            runner_role.id,
            admin_role.id
        ))
        .color(GREEN);

    cc.intax.edit_response(&cc.ctx.http,
        EIR::new().embed(success_embed)
    ).await?;

    Ok(())
}

/// `/roleremove` - Remove runner and admin role configuration
pub async fn cmd_role_remove(cc: &CC<'_>, role_type: String) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    let role_key = match role_type.to_lowercase().as_str() {
        "runner"       => "runner_role",
        "admin"        => "admin_role",
        "both" | "all" => {
            // Remove both
            cc.db.config.delete_config("runner_role", guild_id.get()).await?;
            cc.db.config.delete_config("admin_role", guild_id.get()).await?;

            let success_embed = CE::new()
                .title("Roles Removed")
                .description("Removed both Runner and Admin role configurations.\n\n\
                    **Note:** The Discord roles themselves were not deleted.")
                .color(GREEN);

            let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
        _ => {
            let response = CIR::Message(CIRM::new()
                .content("Invalid role type. Use `runner`, `admin`, or `both`")
                .ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Remove single role
    cc.db.config.delete_config(role_key, guild_id.get()).await?;

    let success_embed = CE::new()
        .title("Role Removed")
        .description(format!(
            "Removed {role_type} role configuration.\n\n\
            **Note:** The Discord role itself was not deleted."
        ))
        .color(GREEN);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// Parse rank name to Rank enum
pub fn parse_rank_name(rank_name: &str) -> Result<Rank> {
    match rank_name.to_lowercase().as_str() {
        "beginner"                            => Ok(Rank::Beginner),
        "newcomer"                            => Ok(Rank::Newcomer),
        "novice"                              => Ok(Rank::Novice),
        "apprentice"                          => Ok(Rank::Apprentice),
        "journeyman" | "jman"                 => Ok(Rank::Journeyman),
        "expert"                              => Ok(Rank::Expert),
        "master"                              => Ok(Rank::Master),
        "masterelite" | "master elite" | "me" => Ok(Rank::MasterElite),
        "grandmaster" | "gm"                  => Ok(Rank::Grandmaster),
        _ => Err(anyhow!("Invalid rank name")),
    }
}

/// `/setupadd` - Creates both roles and a new group with channels
pub async fn cmd_setup_add(cc: &CC<'_>, server: &mut Server) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

    let loading_embed = CE::new()
        .title("Setting Up PUG Bot")
        .description("Creating roles and group channels...\nThis may take a moment.")
        .color(ORANGE);

    let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Step 1: Create Runner role
    let runner_role = match guild_id.create_role(&cc.ctx.http,
        ER::new().name("PUG Runner").colour(RUNNER).permissions(Permissions::empty())
    ).await {
        Ok(role) => role,
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create Runner role: {e}"))
                .color(RED);

            cc.intax.edit_response(&cc.ctx.http,
                EIR::new().embed(error_embed)
            ).await?;
            return Ok(());
        }
    };

    // Step 2: Create Admin role
    let admin_role = match guild_id.create_role(&cc.ctx.http,
        ER::new().name("PUG Admin").colour(ADMIN).permissions(Permissions::empty())
    ).await {
        Ok(role) => role,
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create Admin role: {e}"))
                .color(RED);

            cc.intax.edit_response(&cc.ctx.http,
                EIR::new().embed(error_embed)
            ).await?;
            return Ok(());
        }
    };

    // Step 3: Save roles to database
    if let Err(e) = cc.db.config.set_config("runner_role", &runner_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save runner_role config: {e}");
    }
    if let Err(e) = cc.db.config.set_config("admin_role", &admin_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {e}");
    }

    // Step 4: Create rank roles
    info!("[{}] Creating rank roles", guild_name);
    if let Err(e) = create_rank_roles(cc.ctx, &cc.db, guild_id).await {
        warn!("[{}] Failed to create rank roles: {}", guild_name, e);
    }

    // Step 5: Create group channels
    let (category_id, dashboard_channel, queue_channel, queue_vc_channel, red_channel, blue_channel) =
        match create_group_channels(cc.ctx, guild_id).await {
            Ok(channels) => channels,
            Err(e) => {
                let error_embed = CE::new()
                    .title("Setup Failed")
                    .description(format!("Failed to create channels: {e}\n\nRoles were created successfully."))
                    .color(RED);

                cc.intax.edit_response(&cc.ctx.http,
                    EIR::new().embed(error_embed)
                ).await?;
                return Ok(());
            }
        };

    // Step 6: Create temporary Group and publish dashboard
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

    // Publish the dashboard to get message ID
    match temp_group.dash_publish(cc.ctx, dashboard_channel, &cc.db, guild_id.get()).await {
        Ok(_) => {
            let dashboard_msg_id = temp_group.dashboard_msg.get();

            // Step 7: Save group to database
            let group_config = crate::database::repositories::group::GroupConfig {
                dashboard_channel_id: dashboard_channel.get(),
                chat_channel_id:      queue_channel    .get(),
                queue_vc_id:          queue_vc_channel .get(),
                red_vc_id:            red_channel      .get(),
                blu_vc_id:            blue_channel     .get(),
                quota:                crate::DEFAULT_QUOTA,
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
                        .title("Setup Complete!")
                        .description(format!(
                            "PUG bot is now fully configured!\n\n\
                            **Roles Created:**\n\
                            • Runner: <@&{}>\n\
                            • Admin: <@&{}>\n\
                            • Rank Roles: Created\n\n\
                            **Group Created:**\n\
                            • Dashboard: <#{}>\n\
                            • Queue Text: <#{}>\n\
                            • Queue Voice: <#{}>\n\
                            • Red Team: <#{}>\n\
                            • Blue Team: <#{}>\n\
                            • Category: <#{}>\n\n\
                            **Ready to use!** Players can join the queue now.",
                            runner_role.id,
                            admin_role.id,
                            dashboard_channel.get(),
                            queue_channel    .get(),
                            queue_vc_channel .get(),
                            red_channel      .get(),
                            blue_channel     .get(),
                            category_id      .get()
                        ))
                        .color(GREEN);

                    cc.intax.edit_response(&cc.ctx.http,
                        EIR::new().embed(success_embed)
                    ).await?;
                },
                Err(e) => {
                    // Database save failed - clean up everything
                    info!("[{}] Database save failed, cleaning up channels and dashboard", guild_name);
                    let _ = dashboard_channel.delete_message(&cc.ctx.http, dashboard_msg_id).await;
                    let _ = dashboard_channel.delete(&cc.ctx.http).await;
                    let _ = queue_channel    .delete(&cc.ctx.http).await;
                    let _ = queue_vc_channel .delete(&cc.ctx.http).await;
                    let _ = red_channel      .delete(&cc.ctx.http).await;
                    let _ = blue_channel     .delete(&cc.ctx.http).await;
                    let _ = category_id      .delete(&cc.ctx.http).await;

                    let error_embed = CE::new()
                        .title("Setup Failed")
                        .description(format!("Failed to save group to database: {e}\n\nChannels were cleaned up. Roles remain."))
                        .color(RED);

                    cc.intax.edit_response(&cc.ctx.http,
                        EIR::new().embed(error_embed)
                    ).await?;
                }
            }
        },
        Err(e) => {
            // Dashboard creation failed - clean up the created channels
            info!("[{}] Dashboard creation failed, cleaning up channels", guild_name);
            let _ = dashboard_channel.delete(&cc.ctx.http).await;
            let _ = queue_channel    .delete(&cc.ctx.http).await;
            let _ = queue_vc_channel .delete(&cc.ctx.http).await;
            let _ = red_channel      .delete(&cc.ctx.http).await;
            let _ = blue_channel     .delete(&cc.ctx.http).await;
            let _ = category_id      .delete(&cc.ctx.http).await;

            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create dashboard: {e}\n\nChannels were cleaned up. Roles remain."))
                .color(RED);

            cc.intax.edit_response(&cc.ctx.http,
                EIR::new().embed(error_embed)
            ).await?;
        }
    }

    Ok(())
}

/// `/setuplink` - Links existing roles and channels
pub async fn cmd_setup_link(cc: &CC<'_>) -> Result<()> {
        if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let embed = CE::new()
        .title("Link Existing Configuration")
        .description(
            "To link existing roles and channels, use these commands:\n\n\
            **Link Roles:**\n\
            `/rolelink runner_role:@Runner admin_role:@Admin`\n\n\
            **Link Group Channels:**\n\
            `/grouplink` (run in the dashboard channel)\n\n\
            Or create new ones with:\n\
            • `/roleadd` - Create new roles\n\
            • `/groupadd` - Create new group channels"
        )
        .color(RUNNER);

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/groupremove` - Remove a group from the server
///
/// * `group_id` - The ID of the group to remove (0 = auto-detect from current channel)
pub async fn cmd_group_remove(cc: &CC<'_>, server: &mut Server, group_id: u8) -> Result<()> {
    if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let channel_id = cc.intax.channel_id;

    // Determine which group to remove
    let group_index = if group_id == 0 {
        // Auto-detect group from current channel
        server.groups.iter().position(|g| g.contains_channel(channel_id))
    } else {
        // Use provided group_id
        server.groups.iter().position(|g| g.group_id == group_id)
    };

    match group_index {
        Some(index) => {
            let group = &server.groups[index];
            let actual_group_id = group.group_id;
            let channels = group.channels.clone();

            // Send response immediately before deleting channels
            let loading_embed = CE::new()
                .title("Removing Group")
                .description(format!(
                    "Removing group {actual_group_id} and deleting all associated channels...\n\nThis may take a moment.",
                ))
                .color(ORANGE);

            let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;

            // Get user for DM
            let user = cc.intax.user.clone();

            // Remove from database
            match cc.db.groups.delete(actual_group_id).await {
                Ok(_) => {

                    // Remove from in-memory server
                    server.groups.remove(index);

                    // Get category ID from one of the channels before deleting them
                    let category_id = match channels.dashboard.to_channel(&cc.ctx.http).await {
                        Ok(channel) => {
                            if let Some(guild_channel) = channel.guild() {
                                guild_channel.parent_id
                            } else { None }
                        },
                        Err(e) => {
                            warn!("Failed to get dashboard channel info: {e}");
                            None
                        }
                    };

                    // Delete Discord channels
                    let mut deleted_channels = Vec::new();
                    let mut failed_channels  = Vec::new();

                    // Delete dashboard channel
                    if let Err(e) = channels.dashboard.delete(&cc.ctx.http).await {
                        warn!("Failed to delete dashboard channel: {e}");
                        failed_channels.push("dashboard");
                    } else {
                        deleted_channels.push("dashboard");
                    }

                    // Delete queue text channel
                    if let Err(e) = channels.queue_chat.delete(&cc.ctx.http).await {
                        warn!("Failed to delete queue text channel: {e}");
                        failed_channels.push("queue text");
                    } else {
                        deleted_channels.push("queue text");
                    }

                    // Delete queue voice channel
                    if let Err(e) = channels.queue_vc.delete(&cc.ctx.http).await {
                        warn!("Failed to delete queue voice channel: {e}");
                        failed_channels.push("queue voice");
                    } else {
                        deleted_channels.push("queue voice");
                    }

                    // Delete team voice channels
                    for (i, team) in channels.teams.iter().enumerate() {
                        if let Err(e) = team.red_vc.delete(&cc.ctx.http).await {
                            warn!("Failed to delete red team channel {}: {}", i, e);
                            failed_channels.push("red team");
                        } else {
                            deleted_channels.push("red team");
                        }

                        if let Err(e) = team.blu_vc.delete(&cc.ctx.http).await {
                            warn!("Failed to delete blue team channel {}: {}", i, e);
                            failed_channels.push("blue team");
                        } else {
                            deleted_channels.push("blue team");
                        }
                    }

                    // Delete the category after all channels are deleted
                    if let Some(cat_id) = category_id {
                        if let Err(e) = cat_id.delete(&cc.ctx.http).await {
                            warn!("Failed to delete category: {e}");
                            failed_channels.push("category");
                        } else {
                            deleted_channels.push("category");

                        }
                    }

                    let mut description = format!("Successfully removed group {actual_group_id}.");

                    if !deleted_channels.is_empty() {
                        description.push_str(&format!(
                            "\n\n**Deleted {} channel{}:**\n• {}",
                            deleted_channels.len(),
                            if deleted_channels.len() == 1 { "" } else { "s" },
                            deleted_channels.join("\n• ")
                        ));
                    }

                    if !failed_channels.is_empty() {
                        description.push_str(&format!(
                            "\n\n**Failed to delete {} channel{}:**\n• {}",
                            failed_channels.len(),
                            if failed_channels.len() == 1 { "" } else { "s" },
                            failed_channels.join("\n• ")
                        ));
                    }

                    let success_embed = CE::new()
                        .title("Group Removed")
                        .description(description)
                        .color(GREEN);

                    // Try to edit the original response first
                    if let Err(e) = cc.intax.edit_response(&cc.ctx.http,
                        EIR::new().embed(success_embed.clone())
                    ).await {
                        warn!("Failed to edit response (channel may be deleted): {e}");

                        // If that fails, send a DM to the user
                        if let Err(dm_err) = user.direct_message(&cc.ctx.http,
                            serenity::all::CreateMessage::new().embed(success_embed)
                        ).await {warn!("Failed to send DM to user: {}", dm_err);
                        } else {
                            info!("Sent group removal confirmation via DM to user {}", user.id);
                        }
                    }
                },
                Err(e) => {
                    warn!("[Guild: {}] Failed to remove group {} from database: {}", guild_id, actual_group_id, e);

                    let error_embed = CE::new()
                        .title("Failed to Remove Group")
                        .description(format!("Error: {e}"))
                        .color(RED);

                    // Edit the loading message with error
                    cc.intax.edit_response(&cc.ctx.http,
                        EIR::new().embed(error_embed)
                    ).await?;
                }
            }
        },
        None => {
            // Group not found
            let error_message = if group_id == 0 {
                "No group found for this channel. Use this command in a channel that belongs to a group."
            } else {
                "Group not found with the specified ID."
            };

            let groups_list = if server.groups.is_empty() {
                "No groups configured for this server.".to_string()
            } else {
                let mut list = String::from("Available groups:\n");
                for g in &server.groups {
                    list.push_str(&format!("• Group {} (Dashboard: <#{}>)\n", g.group_id, g.channels.dashboard.get()));
                }
                list
            };

            let error_embed = CE::new()
                .title("Group Not Found")
                .description(format!("{error_message}\n\n{groups_list}",))
                .color(RED);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }

    Ok(())
}

/// `/toggledm` - Toggle DM notifications when a game is ready
pub async fn cmd_toggle_dm(cc: &CC<'_>) -> Result<()> {
    let user_id = cc.intax.user.id;

    // Toggle the DM preference
    let new_state = cc.db.users.toggle_dm_enabled(user_id).await?;

    let (status_text, status_emoji) = if new_state {
        ("enabled", "🔔")
    } else {
        ("disabled", "🔕")
    };

    let embed = CE::new()
        .title("DM Notifications Updated")
        .description(format!(
            "{status_emoji} DM notifications are now **{status_text}**\n\n\
            You will {a} receive a DM when a game is ready.\n",
            a = if new_state { "now" } else { "no longer" }
        ))
        .color(if new_state { GREEN } else { 0xff9900 });

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/settings` - Open personal settings menu as ephemeral message in current channel
pub async fn cmd_settings(cc: &CC<'_>) -> Result<()> {
    let user_id = cc.intax.user.id;

    // Get current settings
    let settings = cc.db.users.get_settings(user_id).await?;

    // Use helper functions from settings module to build embed and buttons
    let embed   = build_settings_embed(&settings);
    let buttons = build_settings_buttons(&settings);

    // Send ephemeral message in the current channel
    let response = CIR::Message(
        CIRM::new()
            .embed(embed)
            .components(buttons)
            .ephemeral(true)
    );
    cc.intax.create_response(&cc.ctx.http, response).await?;

    info!("Sent settings menu to user {} (ephemeral)", cc.intax.user.name);
    Ok(())
}

/// `/serversettings` - Open server settings menu as ephemeral message (admin only)
pub async fn cmd_server_settings(cc: &CC<'_>) -> Result<()> {
    // Check admin permissions
    if !check_role(cc, &Role::Admin).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild_name = cc.ctx.cache.guild(guild_id)
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Server".to_string());

    // Get current server settings
    let settings = get_server_settings(&cc.db, guild_id.get()).await?;

    // Build embed and buttons
    let embed   = build_server_settings_embed(&settings, &guild_name);
    let buttons = build_server_settings_buttons(&settings, &guild_name);

    // Send ephemeral message in the current channel
    let response = CIR::Message(
        CIRM::new()
            .embed(embed)
            .components(buttons)
            .ephemeral(true)
    );
    cc.intax.create_response(&cc.ctx.http, response).await?;

    info!("Sent server settings menu to {} (ephemeral)", cc.intax.user.name);
    Ok(())
}

/// `/groupsettings` - Open group settings menu as ephemeral message (runner only)
pub async fn cmd_group_settings(cc: &CC<'_>) -> Result<()> {
    use crate::handlers::{build_group_settings_embed, build_group_settings_buttons, build_group_selector, GroupSettings};
    
    // Check runner permissions
    if !check_role(cc, &Role::Runner).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let channel_id = cc.intax.channel_id;

    // Get the server and try to find a group
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

    // Try to get group from current channel, or show selector if not in a group channel
    let group = match server.get_group(channel_id) {
        Ok(g) => g,
        Err(_) => {
            // Not in a group channel - check how many groups exist
            let groups = &server.groups;
            
            if groups.is_empty() {
                let error_embed = CE::new()
                    .title("No Groups Configured")
                    .description("No queue groups have been set up for this server.")
                    .color(RED);

                let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
                cc.intax.create_response(&cc.ctx.http, response).await?;
                return Ok(());
            }
            
            if groups.len() == 1 {
                // Auto-select the only group
                &groups[0]
            } else {
                // Multiple groups - show selector
                let selector_embed = CE::new()
                    .title("Select a Group")
                    .description("Choose a group to configure:")
                    .color(0x5865F2);

                let selector = build_group_selector(groups);
                
                let response = CIR::Message(
                    CIRM::new()
                        .embed(selector_embed)
                        .components(vec![selector])
                        .ephemeral(true)
                );
                cc.intax.create_response(&cc.ctx.http, response).await?;
                
                info!("Sent group selector to {} (ephemeral)", cc.intax.user.name);
                return Ok(());
            }
        }
    };

    let settings = GroupSettings {
        group_id:     group.group_id,
        name:         group.name.clone(),
        quota:        group.quota,
        timeout:      group.timeout,
        connect_info: group.connect_info.clone(),
    };
    drop(manager);

    // Build embed and buttons
    let embed   = build_group_settings_embed(&settings);
    let buttons = build_group_settings_buttons(settings.group_id);

    // Send ephemeral message in the current channel
    let response = CIR::Message(
        CIRM::new()
            .embed(embed)
            .components(buttons)
            .ephemeral(true)
    );
    cc.intax.create_response(&cc.ctx.http, response).await?;

    info!("Sent group settings menu to {} (ephemeral)", cc.intax.user.name);
    Ok(())
}

/// `/editplayer` - Open player settings menu as ephemeral message (admin only)
pub async fn cmd_player_settings(cc: &CC<'_>) -> Result<()> {
    use crate::handlers::settings::{PlayerSettings, build_player_settings_embed, build_player_settings_buttons};
    
    // Check admin permissions
    if !check_role(cc, &Role::Admin).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Get target user from command options
    let target_user = cc.intax.data.options.iter()
        .find(|o| o.name == "user")
        .and_then(|o| o.value.as_user_id())
        .ok_or_else(|| anyhow!("User option not found"))?;

    // Get player data
    let player = cc.db.users.get(target_user).await?;
    let guild_elo = cc.db.elos.get(target_user, guild_id.get()).await?;
    let username = cc.ctx.http.get_user(target_user).await
        .map(|u| u.name.clone())
        .unwrap_or_else(|_| target_user.to_string());

    let settings = PlayerSettings {
        user_id:  target_user,
        username,
        steam_id: player.steam_id,
        elo:      guild_elo.elo,
        division: guild_elo.division.name().to_string(),
        games:    guild_elo.games,
        wins:     guild_elo.wins,
    };

    let embed = build_player_settings_embed(&settings);
    let buttons = build_player_settings_buttons(target_user);

    let response = CIR::Message(
        CIRM::new()
            .embed(embed)
            .components(buttons)
            .ephemeral(true)
    );
    cc.intax.create_response(&cc.ctx.http, response).await?;

    info!("Sent player settings menu for {} to {} (ephemeral)", target_user, cc.intax.user.name);
    Ok(())
}
