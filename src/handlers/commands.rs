use anyhow::{anyhow, Result};
use serenity::all::{CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};
use tracing::info;

use super::settings::get_server_settings;
use crate::models::CommandContext as CC;
use crate::models::Ephemeral;
use crate::player::check_adm;
use crate::{GREEN, RED, YELLOW};

/// `/prefs` - Open personal settings menu as ephemeral message in current channel
pub async fn cmd_prefs(cc: &CC<'_>) -> Result<()> {
  let user_id = cc.intax.user.id;

  // Get current settings
  let prefs = cc.db.users.get_prefs(user_id).await?;

  // Send ephemeral message in the current channel
  cc.intax.create_response(&cc.ctx.http, Ephemeral::send_prefs(&prefs)).await?;

  let user_tag = crate::log::get_user_tag(&cc.ctx, cc.intax.user.id, &cc.db).await;
  info!("Sent settings menu to user {} (ephemeral)", user_tag);
  Ok(())
}

/// `/config` - Open server settings menu as ephemeral message (admin only)
pub async fn cmd_config(cc: &CC<'_>) -> Result<()> {
  // Check admin permissions
  if !check_adm(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
  let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());

  // Get current server settings
  let settings = get_server_settings(&cc.db, guild_id).await?;

  // Send ephemeral message in the current channel
  cc.intax.create_response(&cc.ctx.http, Ephemeral::send_config(&settings, &guild_name)).await?;

  let user_tag = crate::log::get_user_tag(&cc.ctx, cc.intax.user.id, &cc.db).await;
  info!("Sent server settings menu to {} (ephemeral)", user_tag);
  Ok(())
}

/// `/migrate` - Bulk-assign ELO to all members with a given role (admin only)
pub async fn cmd_migrate(cc: &CC<'_>) -> Result<()> {
  if !check_adm(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

  // Parse options
  let role_id = cc.intax.data.options.iter().find(|o| o.name == "role").and_then(|o| o.value.as_role_id()).ok_or_else(|| anyhow!("Role option not found"))?;

  let elo = cc.intax.data.options.iter().find(|o| o.name == "elo").and_then(|o| o.value.as_i64()).ok_or_else(|| anyhow!("ELO option not found"))? as u16;

  // Resolve rank for this ELO
  let rank = match crate::Rank::from_elo(&cc.db, guild_id, elo).await {
    Ok(r) => r,
    Err(_) => {
      let embed = CE::new().title("Migration Failed").description(format!("No rank configured for ELO {}. Set up ranks first.", elo)).color(RED);
      cc.reply_embed(embed).await?;
      return Ok(());
    }
  };

  // Defer response since fetching members can take a while
  cc.intax.create_response(&cc.ctx.http, CIR::Defer(CIRM::new().ephemeral(true))).await?;

  // Fetch guild members in chunks (Discord API returns max 1000 per call)
  // Requires GUILD_MEMBERS privileged intent
  let mut all_members = Vec::new();
  let mut last_id = None;
  loop {
    let members = match guild_id.members(&cc.ctx.http, Some(1000), last_id).await {
      Ok(m) => m,
      Err(e) => {
        let embed = CE::new()
          .title("Migration Failed")
          .description(format!("Failed to fetch guild members: {}\n\nEnsure the bot has the **Server Members** privileged intent enabled.", e))
          .color(RED);
        cc.intax.edit_response(&cc.ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;
        return Ok(());
      }
    };
    if members.is_empty() {
      break;
    }
    last_id = members.last().map(|m| m.user.id);
    all_members.extend(members);
    if all_members.len() >= 100_000 {
      break;
    } // safety cap
  }

  // Filter to members with the target role
  let matching: Vec<_> = all_members.iter().filter(|m| m.roles.contains(&role_id)).collect();

  if matching.is_empty() {
    let embed = CE::new().title("Migration Complete").description(format!("No members found with <@&{}>.", role_id)).color(YELLOW);
    cc.intax.edit_response(&cc.ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;
    return Ok(());
  }

  let total = matching.len();
  let user_ids: Vec<_> = matching.iter().map(|m| m.user.id).collect();

  // Batch ensure all users exist, then batch set ELO (2 queries instead of 2*N)
  if let Err(e) = cc.db.users.batch_ensure(&user_ids).await {
    let embed = CE::new().title("Migration Failed").description(format!("Failed to create user records: {}", e)).color(RED);
    cc.intax.edit_response(&cc.ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;
    return Ok(());
  }

  let success = match cc.db.elo.batch_set(guild_id, elo, &rank, &user_ids).await {
    Ok(n) => n,
    Err(e) => {
      let embed = CE::new().title("Migration Failed").description(format!("Failed to set ELO: {}", e)).color(RED);
      cc.intax.edit_response(&cc.ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;
      return Ok(());
    }
  };

  let desc = format!("Assigned **{} ELO** (rank: {}) to **{}** member(s) with <@&{}>.", elo, rank.name, success, role_id);

  let embed = CE::new().title("Migration Complete").description(desc).color(GREEN);
  cc.intax.edit_response(&cc.ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;

  info!("[migrate] Assigned ELO {} to {}/{} members with role {} in guild {}", elo, success, total, role_id, guild_id);
  Ok(())
}

/// `/edit` - Open player settings menu as ephemeral message (admin only)
pub async fn cmd_edit_player(cc: &CC<'_>) -> Result<()> {
  use crate::handlers::settings::PlayerSettings;

  // Check admin permissions
  if !check_adm(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

  // Get target user from command options
  let target_user = cc.intax.data.options.iter().find(|o| o.name == "user").and_then(|o| o.value.as_user_id()).ok_or_else(|| anyhow!("User option not found"))?;

  // Get player data (ensure user exists in database)
  let player = cc.db.users.check_user(target_user, None).await?;

  // First, try to get player's rank from Discord roles (source of truth)
  use crate::handlers::player::get_user_rank_from_discord_roles;
  let discord_rank = get_user_rank_from_discord_roles(&cc.ctx, &cc.db, guild_id, target_user).await;

  // Get guild ELO from database (this has the actual ELO, games, wins)
  let mut guild_elo: crate::db::repo::elo::GuildElo = match cc.db.elo.get(target_user, guild_id, &cc.db).await {
    Ok(elo) => elo,
    Err(e) if e.to_string().contains("Failed to get default rank") => {
      let error_embed = CE::new()
        .title("Configuration Error")
        .description("A default rank has not been set for this server.\n\nPlease configure a default rank in the server settings before editing players.")
        .color(RED);
      cc.reply_embed(error_embed).await?;
      return Ok(());
    }
    Err(e) => return Err(anyhow::anyhow!("Failed to get player ELO: {}", e)),
  };

  // If Discord rank differs from database rank, use Discord rank but keep database ELO/games/wins
  if let Some(discord_guild_rank) = discord_rank {
    let discord_rank = crate::models::types::Rank { guild_id, role_id: discord_guild_rank.role_id, name: discord_guild_rank.name.clone(), elo: discord_guild_rank.elo };

    // Override rank info but keep ELO/games/wins from database
    guild_elo.rank = discord_rank;
  }

  let username = cc.ctx.http.get_user(target_user).await.map(|u| u.name.clone()).unwrap_or_else(|_| target_user.to_string());

  let settings = PlayerSettings {
    user_id: target_user,
    username,
    steam_id: player.steam_id,
    elo: guild_elo.elo,
    rank: guild_elo.rank.name.clone(),
    games: guild_elo.games,
    wins: guild_elo.wins,
  };

  let (embed, components) = crate::handlers::settings::nav_player_settings(&settings, &cc.db, guild_id).await;
  let response = CIR::Message(CIRM::new().embed(embed).components(components).ephemeral(true));
  cc.intax.create_response(&cc.ctx.http, response).await?;

  crate::log::log_command_usage(&cc.ctx, &cc.intax, &cc.db, "edit", Some(target_user), None).await;
  Ok(())
}
