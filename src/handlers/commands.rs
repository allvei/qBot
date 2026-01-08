use anyhow::{Result, anyhow};
use serenity::all::{
    CreateEmbed                      as CE,
    CreateInteractionResponse        as CIR,
    CreateInteractionResponseMessage as CIRM,
};
use tracing::info;

use crate::GREEN;
use crate::models::{CommandContext as CC, Role};
use super::player::check_role;
use super::settings::{build_settings_embed, build_settings_buttons, build_server_settings_embed, build_server_settings_buttons, get_server_settings};

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
