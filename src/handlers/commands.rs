use anyhow::{Result, anyhow};
use serenity::all::{
    CreateEmbed as CE,
};
use tracing::info;

use crate::player::check_adm;
use crate::{ GREEN, YELLOW };
use crate::models::{CommandContext as CC};
use super::settings::{get_server_settings};
use crate::models::Ephemeral;

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

    // Get player data
    let player    = cc.db.users.get(target_user)          .await?;
    let guild_elo = cc.db.elos .get(target_user, guild_id).await?;
    let username  = cc.ctx.http.get_user(target_user)     .await
        .map(|u| u.name.clone())
        .unwrap_or_else(|_| target_user.to_string());

    let settings  = PlayerSettings {
        user_id:  target_user,
        username,
        steam_id: player.steam_id,
        elo:      guild_elo.elo,
        division: guild_elo.division.name().to_string(),
        games:    guild_elo.games,
        wins:     guild_elo.wins,
    };

    cc.intax.create_response(&cc.ctx.http, Ephemeral::send_edit_player(&settings, target_user)).await?;

    info!("Sent player settings menu for {} to {} (ephemeral)", target_user, cc.intax.user.name);
    Ok(())
}
