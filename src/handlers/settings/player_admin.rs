use crate::handlers::settings::menu::{build_player_settings_menu, PlayerSettingsDisplay};
use crate::handlers::settings::utils::{create_paragraph_input_with_value, create_short_input_opt, create_value_input_sh, create_value_input_sh_cap, send_modal_error_response};
use crate::{get_modal_input, get_player_settings};
use crate::{get_user_tag, guild_name, log_prefix_guild, Database, RED};
use anyhow::{anyhow, Result};
use serenity::all::{
  ActionRowComponent as ARC, ButtonStyle as BS, ComponentInteraction as CI, ComponentInteractionDataKind as CIDK, Context, CreateActionRow as CAR, CreateButton as CB,
  CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateModal as CM, GuildId as GI, ModalInteraction as MI, RoleId as RI,
  UserId as UI,
};
use std::sync::Arc;
use tracing::{info, warn};

/// Player settings structure for admin editing
pub struct PlayerSettings {
  pub user_id: UI,
  pub username: String,
  pub steam_id: Option<u64>,
  pub elo: u16,
  pub dynamic_elo: Option<u16>,
  pub rank: String,
  pub games: u32,
  pub wins: u32,
}

impl PlayerSettings {
  pub fn to_display(&self) -> PlayerSettingsDisplay {
    PlayerSettingsDisplay {
      user_id: self.user_id,
      username: self.username.clone(),
      steam_id: self.steam_id,
      elo: self.elo,
      dynamic_elo: self.dynamic_elo,
      rank: self.rank.clone(),
      games: self.games,
      wins: self.wins,
    }
  }
}

/// Handle player settings rank selection dropdown
pub async fn handle_player_settings_rank_select(ctx: &Context, interaction: &CI, db: &Arc<Database>, manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>) -> Result<()> {
  let custom_id = &interaction.data.custom_id;
  let user_tag = get_user_tag(ctx, interaction.user.id, db).await;
  // Extract user_id from custom_id (format: player_settings_rank_select_<user_id>)
  let target_user_id: u64 = custom_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid select ID format: {}", custom_id))?;

  let target_uid = UI::new(target_user_id);
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let guild_name = guild_name(ctx, guild_id);
  let target_tag = get_user_tag(ctx, target_uid, db).await;
  // Get the selected role ID from the select menu
  let selected_role_id_str = match &interaction.data.kind {
    CIDK::StringSelect { values } => values.first().ok_or_else(|| anyhow::anyhow!("No rank selected"))?.clone(),
    _ => return Err(anyhow!("Invalid interaction type")),
  };

  let selected_role_id: u64 = selected_role_id_str.parse().map_err(|_| anyhow::anyhow!("Invalid role ID: {}", selected_role_id_str))?;
  let role_id = RI::new(selected_role_id);

  // Get current player data
  let player = db.players.check_user(target_uid, None).await?;
  let guild_elo = db.elo.get(target_uid, guild_id, db).await?;

  // Get the new rank from the selected role ID
  let new_rank = match db.ranks.rank_from_role_id(guild_id, role_id).await {
    Ok(rank) => crate::models::types::Rank { guild_id, role_id: rank.role_id, name: rank.name, elo: rank.elo },
    Err(e) => {
      warn!("Failed to find rank for role ID {}: {}", selected_role_id, e);

      // Send error message to user
      let error_embed = CE::new()
        .title("Rank Not Found")
        .description(format!("The rank for role <@&{}> was not found in the database. Please ensure ranks are properly configured in server settings.", selected_role_id))
        .color(RED);
      let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
      return Ok(());
    }
  };

  let elo_ranks_linked = db.config.get_elo_ranks_linked(guild_id).await?;

  if elo_ranks_linked {
    // Linked: update both rank and ELO
    db.elo.set(target_uid, guild_id, new_rank.elo, new_rank.clone()).await?;

    // Validate ELO against player's Discord rank (if they have one)
    use crate::handlers::player::get_user_rank_from_discord_roles;
    if let Some(discord_rank_info) = get_user_rank_from_discord_roles(ctx, db, guild_id, target_uid).await {
      let discord_rank = crate::models::types::Rank { guild_id, role_id: discord_rank_info.role_id, name: discord_rank_info.name.clone(), elo: discord_rank_info.elo };

      // Validate and normalize the manually set rank's ELO
      if let Ok((normalized_elo, was_normalized)) = db.elo.validate_and_normalize_elo(target_uid, guild_id, &discord_rank, db).await {
        if was_normalized {
          info!("Admin set rank {} (ELO {}) for {}, but normalized to {} based on Discord rank {}", new_rank.name, new_rank.elo, target_tag, normalized_elo, discord_rank.name);
        }
      }
    }
  } else {
    // Independent: update rank only, keep existing ELO
    db.elo.set(target_uid, guild_id, guild_elo.elo, new_rank.clone()).await?;
  }

  if guild_elo.rank.name != new_rank.name {
    info!(
      "{} Updated rank for {}: {} -> {}{}",
      log_prefix_guild(&guild_name),
      target_tag,
      guild_elo.rank.name,
      new_rank.name,
      if elo_ranks_linked { "" } else { " (ELO unchanged, independent)" }
    );
  }

  // Update Discord roles
  if let Ok(member) = guild_id.member(&ctx.http, target_uid).await {
    // Remove old rank role
    if member.roles.contains(&guild_elo.rank.role_id) {
      if let Err(e) = member.remove_role(&ctx.http, guild_elo.rank.role_id).await {
        info!("Failed to remove old rank role {} from {}: {}", guild_elo.rank.name, target_tag, e);
      } else {
        info!("Removed rank role {} from {}", guild_elo.rank.name, target_tag);
      }
    }

    // Add new rank role
    if !member.roles.contains(&new_rank.role_id) {
      if let Err(e) = member.add_role(&ctx.http, new_rank.role_id).await {
        info!("Failed to add new rank role {} to {}: {}", new_rank.name, target_tag, e);
      } else {
        info!("Added rank role {} to {}", new_rank.name, target_tag);
      }
    }
  }

  // Update in-memory player data and dashboards where this player is queued
  {
    let mut manager_lock = manager.lock().await;
    if let Ok(server) = manager_lock.get_qguild(guild_id) {
      let mut found_in_queue = false;
      for category in &mut server.categories {
        // Update in-memory player rank for all sessions
        for session in &mut category.formats[0].sessions {
          if let Some(session_player) = session.pool.iter_mut().find(|p| p.player.user_id == target_uid) {
            session_player.player.rank = Some(new_rank.clone());
            // ELO is updated based on elo_ranks_linked setting
            if elo_ranks_linked {
              session_player.player.elo = new_rank.elo;
            }
          }
        }

        // Check if player is in any session in this category
        let player_in_queue = category.formats[0].sessions.iter().any(|session| session.pool.iter().any(|p| p.player.user_id == target_uid));

        if player_in_queue {
          found_in_queue = true;
          category.queue_dash_update(ctx, guild_id).await;
          info!("[{}] Player {} rank changed, dashboard updated for {}", guild_name, target_tag, category.name());
        }
      }
      if !found_in_queue {
        info!("Player {} rank changed but not found in any queue", target_tag);
      }
    } else {
      warn!("[{}] Failed to get server when checking if player {} is queued", guild_name, target_tag);
    }
  }

  // Refresh the settings menu
  let username = match ctx.http.get_user(target_uid).await {
    Ok(u) => u.name.clone(),
    Err(_) => target_user_id.to_string(),
  };

  let updated_guild_elo = db.elo.get(target_uid, guild_id, db).await?;
  let settings = PlayerSettings {
    user_id: target_uid,
    username,
    steam_id: player.steam_id,
    elo: updated_guild_elo.elo,
    dynamic_elo: updated_guild_elo.dynamic_elo,
    rank: updated_guild_elo.rank.name.clone(),
    games: updated_guild_elo.games,
    wins: updated_guild_elo.wins,
  };

  let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
} // <--- Added closing brace here

/// Handle player settings button interactions
pub async fn handle_player_settings_button(ctx: &Context, interaction: &CI, db: &Arc<Database>) -> Result<()> {
  let button_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;

  // Extract user_id from button custom_id (format: player_settings_edit_<action>_<user_id>)
  let target_user_id: u64 = button_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid button ID format: {}", button_id))?;

  let target_uid = UI::new(target_user_id);
  let target_tag = crate::log::get_user_tag(ctx, target_uid, db).await;
  let action = button_id.trim_end_matches(&format!("_{}", target_user_id)).replace("player_settings_", "");
  info!("[Player Settings] {} pressed {} on {}", user_tag, action, target_tag);

  // Get current player data (ensure user exists)
  let player = db.players.check_user(target_uid, None).await?;
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let guild_elo = db.elo.get(target_uid, guild_id, db).await?;

  if button_id.starts_with("player_settings_edit_steam_") {
    let modal = CM::new(format!("player_settings_modal_steam_{target_user_id}"), "Edit Steam ID").components(vec![create_short_input_opt(
      "Steam ID (64-bit)",
      "steam_id",
      "e.g., 76561198012345678",
      &player.steam_id.map(|id| id.to_string()).unwrap_or_default(),
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("player_settings_edit_elo_") {
    let modal = CM::new(format!("player_settings_modal_elo_{target_user_id}"), "Edit ELO").components(vec![create_value_input_sh_cap(
      "ELO",
      "elo",
      "e.g., 50",
      &guild_elo.elo.to_string(),
      1,
      3,
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("player_settings_edit_dynamic_elo_") {
    let modal = CM::new(format!("player_settings_modal_dynamic_elo_{target_user_id}"), "Edit Dynamic ELO").components(vec![create_value_input_sh_cap(
      "Dynamic ELO",
      "dynamic_elo",
      "e.g., 1500",
      &guild_elo.dynamic_elo.map(|e| e.to_string()).unwrap_or_default(),
      1,
      5,
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("player_settings_edit_rank_") {
    let modal = CM::new(format!("player_settings_modal_rank_{target_user_id}"), "Edit rank").components(vec![create_value_input_sh(
      "Rank",
      "rank",
      "e.g., Gold, Silver, Bronze",
      &guild_elo.rank.name,
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("player_settings_edit_alerts_") {
    // Get target user's current alert settings
    let user_settings = db.players.get_prefs(target_uid).await?;

    let modal = CM::new(format!("player_settings_modal_alerts_{target_user_id}"), "Edit player alerts").components(vec![
      create_short_input_opt("HEX color", "join_alert_color", "e.g., 3447003 or FF5733", &format!("{:06X}", user_settings.join_alert_color)),
      create_paragraph_input_with_value("Join alert message", "join_alert", "e.g., Kafri: defense", &user_settings.join_alert_desc.unwrap_or_default()),
      create_short_input_opt("Join alert footer", "join_alert_footer", "e.g., Good luck!", &user_settings.join_alert_footer.unwrap_or_default()),
      create_paragraph_input_with_value("Leave alert message", "leave_alert", "e.g., See you next time!", &user_settings.leave_alert_desc.unwrap_or_default()),
    ]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else {
    warn!("Unknown player settings button: {}", button_id);
  }

  Ok(())
}

/// Handle player settings modal submissions
pub async fn handle_player_settings_modal(ctx: &Context, interaction: &MI, db: &Arc<Database>, manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let modal_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;

  // Extract user_id from modal custom_id (format: player_settings_modal_<action>_<user_id>)
  let target_user_id: u64 = modal_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

  let target_uid = UI::new(target_user_id);
  let target_tag = crate::log::get_user_tag(ctx, target_uid, db).await;
  let action = modal_id.trim_end_matches(&format!("_{}", target_user_id)).replace("player_settings_modal_", "");
  info!("[Player Settings] {} submitted {} for {}", user_tag, action, target_tag);

  if modal_id.starts_with("player_settings_modal_steam_") {
    let steam_str = get_modal_input!(interaction);

    let steam_id: Option<u64> = if steam_str.trim().is_empty() {
      None
    } else {
      match steam_str.trim().parse::<u64>() {
        Ok(id) => Some(id),
        Err(_) => {
          send_modal_error_response(interaction, ctx, "Invalid Steam ID. Must be a 64-bit number.").await;
          return Ok(());
        }
      }
    };

    db.players.update_steam_id(&target_uid, steam_id).await?;

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("player_settings_modal_elo_") {
    let elo_str = get_modal_input!(interaction);

    let elo: u16 = match elo_str.trim().parse() {
      Ok(e) => e,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid ELO. Must be a valid number.").await;
        return Ok(());
      }
    };

    // Get current rank and calculate new rank from ELO
    let guild_elo = db.elo.get(target_uid, guild_id, db).await?;
    let old_rank = guild_elo.rank.clone();
    let elo_ranks_linked = db.config.get_elo_ranks_linked(guild_id).await?;

    if elo_ranks_linked {
      let new_rank = crate::models::types::Rank::from_elo(db, guild_id, elo).await?;

      // Check if this ELO change would cause a rank change
      if old_rank.role_id != new_rank.role_id {
        // Rank would change - show confirmation prompt
        let username = ctx.http.get_user(target_uid).await.map(|u| u.name.clone()).unwrap_or_else(|_| target_user_id.to_string());

        let confirm_embed = CE::new()
          .title("Rank Change Required")
          .description(format!(
            "Setting **{}'s** ELO to **{}** will change their rank:\n\n\
                        **Current:** {} (ELO {})\n\
                        **New:** {} (ELO {})\n\n\
                        This will update their Discord role from <@&{}> to <@&{}>.\n\n\
                        Do you want to continue?",
            username, elo, old_rank.name, guild_elo.elo, new_rank.name, elo, old_rank.role_id, new_rank.role_id
          ))
          .color(0xFFA500);

        let confirm_buttons = vec![CAR::Buttons(vec![
          CB::new(format!("confirm_elo_change_{}_{}", target_user_id, elo)).label("Confirm").style(BS::Success),
          CB::new(format!("cancel_elo_change_{}", target_user_id)).label("Cancel").style(BS::Danger),
        ])];

        let response = CIR::UpdateMessage(CIRM::new().embed(confirm_embed).components(confirm_buttons));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }

      // No rank change - proceed with ELO update (rank stays the same)
      db.elo.set(target_uid, guild_id, elo, new_rank.clone()).await?;
      info!("Updated ELO for {} to {} (rank: {})", target_tag, elo, new_rank.name);
    } else {
      // ELO-Rank independent: update ELO only, keep existing rank
      db.elo.set(target_uid, guild_id, elo, old_rank.clone()).await?;
      info!("Updated ELO for {} to {} (rank unchanged: {}, ELO-Rank independent)", target_tag, elo, old_rank.name);
    }

    // Update in-memory player data and dashboards where this player is queued
    {
      let mut manager_lock = manager.lock().await;
      let mut found_in_queue = false;

      if let Ok(server) = manager_lock.get_qguild(guild_id) {
        for category in &mut server.categories {
          if category.for_each_player_mut(target_uid, |session_player| {
            session_player.player.elo = elo;
          }) {
            found_in_queue = true;
          }
        }
      } else {
        let guild_name = crate::models::constants::guild_name(ctx, guild_id);
        warn!("[{}] Failed to get server when checking if player {} is queued", guild_name, target_tag);
      }

      let queued = manager_lock.queue_dash_updates_for_player(ctx, guild_id, target_uid).await;
      if found_in_queue {
        info!("Player {} ELO changed, queued {} dashboard update(s)", target_tag, queued);
      } else {
        info!("Player {} ELO changed but not found in any queue", target_tag);
      }
    }

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("player_settings_modal_dynamic_elo_") {
    let dynamic_elo_str = get_modal_input!(interaction);

    let dynamic_elo: Option<u16> = if dynamic_elo_str.trim().is_empty() {
      None
    } else {
      match dynamic_elo_str.trim().parse() {
        Ok(e) => Some(e),
        _ => {
          send_modal_error_response(interaction, ctx, "Invalid Dynamic ELO. Must be a valid number.").await;
          return Ok(());
        }
      }
    };

    db.elo.set_dynamic_elo(target_uid, guild_id, dynamic_elo).await?;
    info!("Updated Dynamic ELO for {} to {:?}", target_tag, dynamic_elo);

    // Update in-memory player data and dashboards where this player is queued
    {
      let mut manager_lock = manager.lock().await;
      let mut found_in_queue = false;

      if let Ok(server) = manager_lock.get_qguild(guild_id) {
        for category in &mut server.categories {
          if category.for_each_player_mut(target_uid, |session_player| {
            session_player.player.dynamic_elo = dynamic_elo;
          }) {
            found_in_queue = true;
          }
        }
      } else {
        let guild_name = crate::models::constants::guild_name(ctx, guild_id);
        warn!("[{}] Failed to get server when checking if player {} is queued", guild_name, target_tag);
      }

      let queued = manager_lock.queue_dash_updates_for_player(ctx, guild_id, target_uid).await;
    }

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("player_settings_modal_rank_") {
    let rank_str = get_modal_input!(interaction);

    let new_rank = crate::models::types::Rank::from_name(db, guild_id, rank_str.trim()).await?;

    // Get current data
    let guild_elo = db.elo.get(target_uid, guild_id, db).await?;
    let old_rank = guild_elo.rank;
    let elo_ranks_linked = db.config.get_elo_ranks_linked(guild_id).await?;

    if elo_ranks_linked {
      // Linked: update both rank and ELO to the new rank's base
      db.elo.set(target_uid, guild_id, new_rank.elo, new_rank.clone()).await?;
    } else {
      // Independent: update rank only, keep existing ELO
      db.elo.set(target_uid, guild_id, guild_elo.elo, new_rank.clone()).await?;
    }

    if old_rank.name != new_rank.name {
      info!("Updated rank for {}: {} -> {}{}", target_tag, old_rank.name, new_rank.name, if elo_ranks_linked { "" } else { " (ELO unchanged, independent)" });
    }

    // Update Discord roles
    if let Ok(member) = guild_id.member(&ctx.http, target_uid).await {
      // Remove old rank role
      if member.roles.contains(&old_rank.role_id) {
        if let Err(e) = member.remove_role(&ctx.http, old_rank.role_id).await {
          info!("Failed to remove old rank role {} from {}: {}", old_rank.name, target_tag, e);
        } else {
          info!("Removed rank role {} from {}", old_rank.name, target_tag);
        }
      }

      // Add new rank role
      if !member.roles.contains(&new_rank.role_id) {
        if let Err(e) = member.add_role(&ctx.http, new_rank.role_id).await {
          info!("Failed to add new rank role {} to {}: {}", new_rank.name, target_tag, e);
        } else {
          info!("Added rank role {} to {}", new_rank.name, target_tag);
        }
      }
    }

    // Update in-memory player data and dashboards where this player is queued
    {
      let mut manager_lock = manager.lock().await;
      let mut found_in_queue = false;

      if let Ok(server) = manager_lock.get_qguild(guild_id) {
        for category in &mut server.categories {
          if category.for_each_player_mut(target_uid, |session_player| {
            session_player.player.rank = Some(new_rank.clone());
            if elo_ranks_linked {
              session_player.player.elo = new_rank.elo;
            }
          }) {
            found_in_queue = true;
          }
        }
      } else {
        warn!("Failed to get server when checking if player {} is queued", target_tag);
      }

      let queued = manager_lock.queue_dash_updates_for_player(ctx, guild_id, target_uid).await;
      if found_in_queue {
        info!("Player {} rank changed, queued {} dashboard update(s)", target_tag, queued);
      } else {
        info!("Player {} rank changed but not found in any queue", target_tag);
      }
    }

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("player_settings_modal_alerts_") {
    // Extract values from modal components
    let mut user_settings = db.players.get_prefs(target_uid).await?;

    for (idx, action_row) in interaction.data.components.iter().enumerate() {
      if let Some(ARC::InputText(input)) = action_row.components.first() {
        if let Some(value) = &input.value {
          let trimmed = value.trim();
          match idx {
            0 => {
              // Color field
              if !trimmed.is_empty() {
                let hex_str = trimmed.trim_start_matches('#');
                if let Ok(color) = u32::from_str_radix(hex_str, 16) {
                  if (0..=0xFFFFFF).contains(&color) {
                    user_settings.join_alert_color = color;
                  }
                }
              }
            }
            1 => user_settings.join_alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
            2 => user_settings.join_alert_footer = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
            3 => user_settings.leave_alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
            _ => {}
          }
        }
      }
    }

    // Update target user's settings
    db.players.update_prefs(target_uid, &user_settings).await?;

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;

    info!("[Player Settings] Updated alerts for {}", target_tag);
  } else {
    warn!("Unknown player settings modal: {}", modal_id);
  }

  Ok(())
}

/// Build player settings embed and components (with rank dropdown when ranks exist)
pub async fn nav_player_settings(settings: &PlayerSettings, db: &Arc<Database>, guild_id: GI) -> (CE, Vec<CAR>) {
  build_player_settings_menu(&settings.to_display(), db, guild_id).await
}
