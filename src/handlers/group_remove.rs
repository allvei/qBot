use anyhow::Result;
use serenity::all::{CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};
use tracing::{info, warn};

use crate::database::repositories::Repository;
use crate::models::{CommandContext as CC, Role, Server};
use super::player::check_role;

/// `/groupremove` - Remove a group from the server
///
/// * `group_id` - The ID of the group to remove (0 = auto-detect from current channel)
pub async fn cmd_group_remove(cc: &CC<'_>, server: &mut Server, group_id: u8) -> Result<()> {
    info!("Processing /groupremove for group_id: {}", group_id);

    // Check admin permissions
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can remove groups!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

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
                    "Removing group {} and deleting all associated channels...\n\nThis may take a moment.",
                    actual_group_id
                ))
                .color(0xffaa00);

            let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            
            // Get user for DM
            let user = cc.intax.user.clone();
            
            // Remove from database
            match cc.db.groups.delete(actual_group_id).await {
                Ok(_) => {
                    info!("[Guild: {}] Group {} removed from database", guild_id, actual_group_id);
                    
                    // Remove from in-memory server
                    server.groups.remove(index);
                    
                    // Get category ID from one of the channels before deleting them
                    let category_id = match channels.dashboard.to_channel(&cc.ctx.http).await {
                        Ok(channel) => {
                            if let Some(guild_channel) = channel.guild() {
                                guild_channel.parent_id
                            } else {
                                None
                            }
                        },
                        Err(e) => {
                            warn!("Failed to get dashboard channel info: {}", e);
                            None
                        }
                    };
                    
                    // Delete Discord channels
                    let mut deleted_channels = Vec::new();
                    let mut failed_channels = Vec::new();
                    
                    // Delete dashboard channel
                    if let Err(e) = channels.dashboard.delete(&cc.ctx.http).await {
                        warn!("Failed to delete dashboard channel: {}", e);
                        failed_channels.push("dashboard");
                    } else {
                        deleted_channels.push("dashboard");
                    }
                    
                    // Delete queue text channel
                    if let Err(e) = channels.queue_chat.delete(&cc.ctx.http).await {
                        warn!("Failed to delete queue text channel: {}", e);
                        failed_channels.push("queue text");
                    } else {
                        deleted_channels.push("queue text");
                    }
                    
                    // Delete queue voice channel
                    if let Err(e) = channels.queue_vc.delete(&cc.ctx.http).await {
                        warn!("Failed to delete queue voice channel: {}", e);
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
                            warn!("Failed to delete category: {}", e);
                            failed_channels.push("category");
                        } else {
                            deleted_channels.push("category");
                            info!("[Guild: {}] Deleted category {}", guild_id, cat_id);
                        }
                    }
                    
                    let mut description = format!("Successfully removed group {}.", actual_group_id);
                    
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
                        .color(0x00ff00);

                    // Try to edit the original response first
                    if let Err(e) = cc.intax.edit_response(&cc.ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(success_embed.clone())
                    ).await {
                        warn!("Failed to edit response (channel may be deleted): {}", e);
                        
                        // If that fails, send a DM to the user
                        if let Err(dm_err) = user.direct_message(&cc.ctx.http, 
                            serenity::all::CreateMessage::new().embed(success_embed)
                        ).await {
                            warn!("Failed to send DM to user: {}", dm_err);
                        } else {
                            info!("Sent group removal confirmation via DM to user {}", user.id);
                        }
                    }
                },
                Err(e) => {
                    warn!("[Guild: {}] Failed to remove group {} from database: {}", guild_id, actual_group_id, e);
                    
                    let error_embed = CE::new()
                        .title("Failed to Remove Group")
                        .description(format!("Error: {}", e))
                        .color(0xff0000);

                    // Edit the loading message with error
                    cc.intax.edit_response(&cc.ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(error_embed)
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
                .description(format!(
                    "{}\n\n{}",
                    error_message,
                    groups_list
                ))
                .color(0xff0000);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }

    Ok(())
}
