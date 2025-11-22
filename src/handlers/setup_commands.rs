use anyhow::{anyhow, Result};
use serenity::all::{
    CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, GuildId, Permissions,
};
use serenity::builder::EditRole;
use tracing::{error, info, warn};

use crate::models::{CommandContext as CC, Role, Server};
use super::player::{check_role, create_rank_roles};
use super::admin::create_group_channels;

/// `/setupadd` - Creates both roles and a new group with channels
pub async fn cmd_setup_add(cc: &CC<'_>, server: &mut Server) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can run setup!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
    
    let loading_embed = CE::new()
        .title("Setting Up PUG Bot")
        .description("Creating roles and group channels...\nThis may take a moment.")
        .color(0xffaa00);

    let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Step 1: Create Runner role
    let runner_role = match guild_id.create_role(&cc.ctx.http, 
        EditRole::new()
            .name("PUG Runner")
            .colour(0x3498db)
            .permissions(Permissions::empty())
    ).await {
        Ok(role) => role,
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create Runner role: {}", e))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
            return Ok(());
        }
    };

    // Step 2: Create Admin role
    let admin_role = match guild_id.create_role(&cc.ctx.http,
        EditRole::new()
            .name("PUG Admin")
            .colour(0xe74c3c)
            .permissions(Permissions::empty())
    ).await {
        Ok(role) => role,
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create Admin role: {}", e))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
            return Ok(());
        }
    };

    // Step 3: Save roles to database
    if let Err(e) = cc.db.config.set_config("runner_role", &runner_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save runner_role config: {}", e);
    }
    if let Err(e) = cc.db.config.set_config("admin_role", &admin_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {}", e);
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
                    .description(format!("Failed to create channels: {}\n\nRoles were created successfully.", e))
                    .color(0xff0000);

                cc.intax.edit_response(&cc.ctx.http,
                    serenity::all::EditInteractionResponse::new().embed(error_embed)
                ).await?;
                return Ok(());
            }
        };

    // Step 6: Create temporary Group and publish dashboard
    use crate::models::{Group, Channels, TeamChannel};
    use serenity::all::MessageId;
    
    let mut temp_group = Group {
        group_id: 0,
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
    match temp_group.dash_publish(cc.ctx, dashboard_channel).await {
        Ok(_) => {
            let dashboard_msg_id = temp_group.dashboard_msg.get();
            
            // Step 7: Save group to database
            match cc.db.groups.create_group(
                guild_id.get(),
                dashboard_channel.get(),
                queue_channel.get(),
                queue_vc_channel.get(),
                dashboard_msg_id,
                red_channel.get(),
                blue_channel.get(),
                crate::DEFAULT_QUOTA,
            ).await {
                Ok(db_group) => {
                    info!("[{}] Group {} saved to database", guild_name, db_group.group_id);

                    // Add group to in-memory server and create initial session
                    if let Err(e) = server.add_group(db_group.clone()) {
                        error!("Failed to add group to server: {}", e);
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
                            queue_channel.get(), 
                            queue_vc_channel.get(), 
                            red_channel.get(), 
                            blue_channel.get(),
                            category_id.get()
                        ))
                        .color(0x00ff00);

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
                        .title("Setup Failed")
                        .description(format!("Failed to save group to database: {}\n\nChannels were cleaned up. Roles remain.", e))
                        .color(0xff0000);

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
                .title("Setup Failed")
                .description(format!("Failed to create dashboard: {}\n\nChannels were cleaned up. Roles remain.", e))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
        }
    }

    Ok(())
}

/// `/setuplink` - Links existing roles and channels
pub async fn cmd_setup_link(cc: &CC<'_>) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can run setup!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

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
        .color(0x3498db);

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}
