use anyhow::Result;
use serenity::all::{
    ComponentInteraction, Context, CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, UserId as UI,
};
use std::sync::Arc;
use tracing::info;

use crate::Database;
use crate::handlers::settings::{nav_player_settings, PlayerSettings};

/// Handle confirmation/cancellation of ELO changes that would change rank
pub async fn handle_elo_change_confirmation(
    ctx: &Context,
    interaction: &ComponentInteraction,
    db: &Arc<Database>,
    manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
    let custom_id = &interaction.data.custom_id;
    let guild_id = interaction.guild_id.expect("Guild ID not found");

    if custom_id.starts_with("cancel_elo_change_") {
        // User cancelled - just go back to settings menu
        let target_user_id: u64 = custom_id
            .strip_prefix("cancel_elo_change_")
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("Invalid cancel button ID format"))?;
        
        let target_uid = UI::new(target_user_id);
        
        // Refresh the settings menu
        let player = db.users.check_user(target_uid, None).await?;
        let guild_elo = db.elo.get(target_uid, guild_id, db).await?;
        let username = ctx.http.get_user(target_uid).await
            .map(|u| u.name.clone())
            .unwrap_or_else(|_| target_user_id.to_string());

        let settings = PlayerSettings {
            user_id:  target_uid,
            username,
            steam_id: player.steam_id,
            elo:      guild_elo.elo,
            rank: guild_elo.rank.name.clone(),
            games:    guild_elo.games,
            wins:     guild_elo.wins,
        };

        let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
        interaction.create_response(&ctx.http, response).await?;
        
        let user_tag = crate::log::get_user_tag(&ctx, target_uid, &db).await;
        info!("Admin cancelled ELO change for user {}", user_tag);
        return Ok(());
    }

    if custom_id.starts_with("confirm_elo_change_") {
        // Parse target user ID and new ELO from button ID
        let parts: Vec<&str> = custom_id.strip_prefix("confirm_elo_change_")
            .ok_or_else(|| anyhow::anyhow!("Invalid confirm button ID format"))?
            .split('_')
            .collect();
        
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Invalid confirm button ID format"));
        }
        
        let target_user_id: u64 = parts[0].parse()?;
        let new_elo: u16 = parts[1].parse()?;
        let target_uid = UI::new(target_user_id);
        
        // Get current and new ranks
        let guild_elo = db.elo.get(target_uid, guild_id, db).await?;
        let old_rank = guild_elo.rank.clone();
        let new_rank = crate::models::types::Rank::from_elo(db, guild_id, new_elo).await?;
        
        // Update ELO and rank in database
        db.elo.set(target_uid, guild_id, new_elo, new_rank.clone()).await?;
        
        // Update Discord roles
        if let Ok(member) = guild_id.member(&ctx.http, target_uid).await {
            // Remove old rank role
            if member.roles.contains(&old_rank.role_id) {
                if let Err(e) = member.remove_role(&ctx.http, old_rank.role_id).await {
                    info!("Failed to remove old rank role {} from user {}: {}", old_rank.role_id, target_uid, e);
                } else {
                    let user_tag = crate::log::get_user_tag(&ctx, target_uid, &db).await;
                    info!("Removed rank role {} from user {}", old_rank.name, user_tag);
                }
            }
            
            // Add new rank role
            if !member.roles.contains(&new_rank.role_id) {
                if let Err(e) = member.add_role(&ctx.http, new_rank.role_id).await {
                    info!("Failed to add new rank role {} to user {}: {}", new_rank.role_id, target_uid, e);
                } else {
                    let user_tag = crate::log::get_user_tag(&ctx, target_uid, &db).await;
                    info!("Added rank role {} to user {}", new_rank.name, user_tag);
                }
            }
        }
        
        info!("Updated ELO for {} from {} to {} and changed rank from {} to {}", 
              target_uid, guild_elo.elo, new_elo, old_rank.name, new_rank.name);

        // Update dashboards where this player is queued
        {
            let mut manager_lock = manager.lock().await;
            if let Ok(server) = manager_lock.get_server(guild_id) {
                for group in &server.groups {
                    let player_in_queue = group.subgroups[0].sessions.iter().any(|session| {
                        session.pool.iter().any(|p| p.player.user_id == target_uid)
                    });
                    
                    if player_in_queue {
                        info!("Player {} ELO changed, updating dashboard for group {}", target_uid, group.group_id);
                        group.queue_dash_update(ctx, guild_id).await;
                    }
                }
            }
        }

        // Show success message and refresh settings menu
        let player = db.users.check_user(target_uid, None).await?;
        let updated_guild_elo = db.elo.get(target_uid, guild_id, db).await?;
        let username = ctx.http.get_user(target_uid).await
            .map(|u| u.name.clone())
            .unwrap_or_else(|_| target_user_id.to_string());

        let settings = PlayerSettings {
            user_id:  target_uid,
            username: username.clone(),
            steam_id: player.steam_id,
            elo:      updated_guild_elo.elo,
            rank: updated_guild_elo.rank.name.clone(),
            games:    updated_guild_elo.games,
            wins:     updated_guild_elo.wins,
        };

        let success_embed = CE::new()
            .title("ELO and Rank Updated")
            .description(format!(
                "Successfully updated **{}'s** profile:\n\n\
                **ELO:** {} → **{}**\n\
                **Rank:** {} → **{}**\n\
                **Discord Role:** Updated",
                username,
                guild_elo.elo, new_elo,
                old_rank.name, new_rank.name
            ))
            .color(0x00FF00);

        let (embed, components) = nav_player_settings(&settings, db, guild_id).await;

        let response = CIR::UpdateMessage(
            CIRM::new().embeds(vec![success_embed, embed]).components(components)
        );
        interaction.create_response(&ctx.http, response).await?;
        
        return Ok(());
    }

    Ok(())
}
