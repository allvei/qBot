use anyhow::Result;
use serenity::all::{CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};
use tracing::{info, warn};

use crate::database::repositories::Repository;
use crate::models::{CommandContext as CC, Role, Server};
use super::player::check_role;

/// `/groupremove` - Remove a group from the server
///
/// * `group_id` - The ID of the group to remove
pub async fn cmd_group_remove(cc: &CC<'_>, server: &mut Server, group_id: u8) -> Result<()> {
    info!("Processing /groupremove for group_id: {}", group_id);

    // Check admin permissions
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can remove groups!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");

    // Find the group in the server
    let group_index = server.groups.iter().position(|g| g.group_id == group_id);

    match group_index {
        Some(index) => {
            let group = &server.groups[index];
            let dashboard_channel = group.channels.dashboard;
            
            // Remove from database
            match cc.db.groups.delete(group_id).await {
                Ok(_) => {
                    info!("[Guild: {}] Group {} removed from database", guild_id, group_id);
                    
                    // Remove from in-memory server
                    server.groups.remove(index);
                    
                    let success_embed = CE::new()
                        .title("Group Removed")
                        .description(format!(
                            "Successfully removed group {}.\n\n\
                            **Note:** The Discord channels were not deleted. \
                            You can manually delete them if needed:\n\
                            • Dashboard channel: <#{}>\n\
                            • And associated queue/team channels",
                            group_id,
                            dashboard_channel.get()
                        ))
                        .color(0x00ff00);

                    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
                    cc.intax.create_response(&cc.ctx.http, response).await?;
                },
                Err(e) => {
                    warn!("[Guild: {}] Failed to remove group {} from database: {}", guild_id, group_id, e);
                    
                    let error_embed = CE::new()
                        .title("Failed to Remove Group")
                        .description(format!("Error: {}", e))
                        .color(0xff0000);

                    let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
                    cc.intax.create_response(&cc.ctx.http, response).await?;
                }
            }
        },
        None => {
            // Group not found
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
                    "Group {} was not found.\n\n{}",
                    group_id,
                    groups_list
                ))
                .color(0xff0000);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }

    Ok(())
}
