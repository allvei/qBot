use anyhow::{Result, anyhow};
use serenity::all::{
    CreateEmbed as CE,
    CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM,
};
use tracing::info;

use crate::player::check_adm;
use crate::{ GREEN, YELLOW, RED };
use crate::models::{CommandContext as CC};
use super::settings::{get_server_settings};
use crate::models::Ephemeral;

/// `/toggledm` - Toggle DM notifications when a game is ready
pub async fn cmd_toggle_dm(cc: &CC<'_>) -> Result<()> {
    let user_id = cc.intax.user.id;

    // Toggle the DM preference
    let new_state = cc.db.users.toggle_pm_hot_alert(user_id).await?;

    let (status_text, status_emoji) = if new_state {
        ("enabled", "🔔")
    } else {
        ("disabled", "🔕")
    };

    let embed = CE::new()
        .title("DM Alerts Updated")
        .description(format!(
            "{status_emoji} DM alerts are now **{status_text}**\n\n\
            You will {a} receive a DM when a game is ready.\n",
            a = if new_state { "now" } else { "no longer" }
        ))
        .color(if new_state { GREEN } else { YELLOW });

    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(embed)).await?;

    Ok(())
}

/// `/prefs` - Open personal settings menu as ephemeral message in current channel
pub async fn cmd_prefs(cc: &CC<'_>) -> Result<()> {
    let user_id = cc.intax.user.id;

    // Get current settings
    let prefs = cc.db.users.get_prefs(user_id).await?;

    // Send ephemeral message in the current channel
    cc.intax.create_response(&cc.ctx.http, Ephemeral::send_prefs(&prefs)).await?;

    info!("Sent settings menu to user {} (ephemeral)", cc.intax.user.name);
    Ok(())
}

/// `/config` - Open server settings menu as ephemeral message (admin only)
pub async fn cmd_config(cc: &CC<'_>) -> Result<()> {
    // Check admin permissions
    if !check_adm(cc).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild_name = cc.ctx.cache.guild(guild_id)
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Server".to_string());

    // Get current server settings
    let settings = get_server_settings(&cc.db, guild_id).await?;

    // Send ephemeral message in the current channel
    cc.intax.create_response(&cc.ctx.http, Ephemeral::send_config(&settings, &guild_name)).await?;

    info!("Sent server settings menu to {} (ephemeral)", cc.intax.user.name);
    Ok(())
}

/// `/editplayer` - Open player settings menu as ephemeral message (admin only)
pub async fn cmd_edit_player(cc: &CC<'_>) -> Result<()> {
    use crate::handlers::settings::{PlayerSettings};
    
    // Check admin permissions
    if !check_adm(cc).await? { return Ok(()); }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Get target user from command options
    let target_user = cc.intax.data.options.iter()
        .find(|o| o.name == "user")
        .and_then(|o| o.value.as_user_id())
        .ok_or_else(|| anyhow!("User option not found"))?;

    // Get player data (ensure user exists in database)
    let player    = cc.db.users.check_user(target_user, None).await?;
    
    // First, try to get player's rank from Discord roles (source of truth)
    use crate::handlers::player::get_user_rank_from_discord_roles;
    let discord_rank = get_user_rank_from_discord_roles(&cc.ctx, &cc.db, guild_id, target_user).await;
    
    // Get guild ELO, using Discord rank if available, otherwise fall back to database
    let guild_elo = match discord_rank {
        Some(discord_guild_rank) => {
            // Convert Discord rank to Rank struct and create GuildElo
            let rank = crate::models::types::Rank {
                guild_id,
                role_id: discord_guild_rank.role_id,
                name: discord_guild_rank.name.clone(),
                elo: discord_guild_rank.elo,
            };
            Ok::<crate::database::repositories::elo::GuildElo, anyhow::Error>(crate::database::repositories::elo::GuildElo {
                elo: rank.elo,
                rank,
                games: 0,
                wins: 0,
            })
        }
        None => {
            // No Discord rank found, fall back to database
            match cc.db.elo.get(target_user, guild_id, &cc.db).await {
                Ok(elo) => Ok(elo),
                Err(e) if e.to_string().contains("Failed to get default rank") => {
                    let error_embed = CE::new()
                        .title("Configuration Error")
                        .description("A default rank has not been set for this server.\n\nPlease configure a default rank in the server settings before editing players.")
                        .color(RED);
                    let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
                    cc.intax.create_response(&cc.ctx.http, response).await?;
                    return Ok(());
                }
                Err(e) => return Err(anyhow::anyhow!("Failed to get player ELO: {}", e)),
            }
        }
    }?;
    
    let username  = cc.ctx.http.get_user(target_user)     .await
        .map(|u| u.name.clone())
        .unwrap_or_else(|_| target_user.to_string());

    let settings  = PlayerSettings {
        user_id:  target_user,
        username,
        steam_id: player.steam_id,
        elo:      guild_elo.elo,
        rank: guild_elo.rank.name.clone(),
        games:    guild_elo.games,
        wins:     guild_elo.wins,
    };

    // Use rank selection dropdown if ranks are available
    let display_settings = crate::handlers::settings_menu::PlayerSettingsDisplay {
        user_id: settings.user_id,
        username: settings.username.clone(),
        steam_id: settings.steam_id,
        elo: settings.elo,
        rank: settings.rank.clone(),
        games: settings.games,
        wins: settings.wins,
    };
    
    let response = match crate::handlers::settings_menu::create_player_settings_with_rank_select(&display_settings, &cc.db, guild_id).await {
        Ok(resp) => resp,
        Err(_) => {
            // Fallback to regular menu if rank selection fails
            Ephemeral::send_edit_player(&settings, target_user)
        }
    };
    cc.intax.create_response(&cc.ctx.http, response).await?;

    info!("Sent player settings menu for {} to {} (ephemeral)", target_user, cc.intax.user.name);
    Ok(())
}
