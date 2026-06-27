use anyhow::Result;
use serenity::all::{
  ButtonStyle as BS, ComponentInteraction as CI, Context, CreateActionRow as CAR, CreateButton as CB, CreateEmbed as CE, CreateInteractionResponse as CIR,
  CreateInteractionResponseMessage as CIRM, GuildId as GI,
};
use std::sync::Arc;
use tracing::{error, info};

use crate::db::Database;
use crate::handlers::player::is_role_component;
use crate::models::embeds::Ephemeral as Eph;
use crate::models::{ComponentContext as CC, Role};
use crate::{guild_name, log_prefix_category, Manager, BLUE, RED};

/// Handle "End without score" action from runner menu
/// Finds the runner's active match and ends it without requiring score reporting
pub async fn handle_end_without_score(ctx: &Context, interaction: &CI, db: &Arc<Database>, manager: &Arc<tokio::sync::Mutex<Manager>>, guild_id: GI) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !is_role_component(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![Eph::back("runner_menu_back")])]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let user_id = interaction.user.id;

  // Find the runner's active match
  let (found_match, guild_name_str, category_name, format_name) = {
    let mut mgr = manager.lock().await;
    let server = mgr.get_qguild(guild_id)?;

    let mut found_match = None;

    // Strategy 1: Find the only active match in any category
    let mut active_matches = Vec::new();
    for (cat_idx, category) in server.categories.iter().enumerate() {
      for (fmt_idx, format) in category.formats.iter().enumerate() {
        for session in &format.sessions {
          if session.is_active() {
            active_matches.push((cat_idx, fmt_idx, category.id, format.id));
          }
        }
      }
    }

    if active_matches.len() == 1 {
      // Only one active match - use it
      found_match = Some(active_matches[0]);
    } else if active_matches.len() > 1 {
      // Strategy 2: Check if runner is in any team VC
      if let Some(guild) = ctx.cache.guild(guild_id) {
        if let Some(voice_state) = guild.voice_states.get(&user_id) {
          if let Some(channel_id) = voice_state.channel_id {
            // Check which category this channel belongs to
            for (cat_idx, category) in server.categories.iter().enumerate() {
              if category.channels.teams.iter().any(|tc| tc.red_vc == channel_id || tc.blu_vc == channel_id) {
                // Found the category - now find the active session
                for (fmt_idx, format) in category.formats.iter().enumerate() {
                  if format.sessions.iter().any(|s| s.is_active()) {
                    found_match = Some((cat_idx, fmt_idx, category.id, format.id));
                    break;
                  }
                }
                break;
              }
            }
          }
        }
      }
    }

    let (cat_idx, fmt_idx, _category_id, _format_id) = match found_match {
      Some(m) => m,
      None => {
        let embed = CE::new()
          .title("No active match found")
          .description("Could not find an active match to end. Either there are no active matches, or you need to be in a team voice channel when multiple matches are running.")
          .color(0xFFAA00);
        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![Eph::back("runner_menu_back")])]));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }
    };

    let category = &server.categories[cat_idx];
    let guild_name_str = guild_name(ctx, guild_id);
    let category_name = category.name.as_deref().unwrap_or("Unknown").to_string();
    let format_name = category.formats[fmt_idx].name.clone();

    (found_match, guild_name_str, category_name, format_name)
  };

  let (_cat_idx, _fmt_idx, _category_id, format_id) = found_match.unwrap();

  info!("{} Runner {} used 'End without score'", log_prefix_category(&guild_name_str, &category_name), interaction.user.tag());

  // Defer the response since we're about to do async work
  interaction.create_response(&ctx.http, CIR::Defer(CIRM::new().ephemeral(true))).await?;

  // End the match using the category's pull method
  {
    let mut mgr = manager.lock().await;
    let server = mgr.get_qguild(guild_id)?;
    let (cat_idx, _fmt_idx, _category_id, _format_id) = found_match.unwrap();
    let category = &mut server.categories[cat_idx];

    match category.pull_fmt(format_id, ctx, guild_id, db, Some(manager.clone())).await {
      Ok(_) => {
        info!("{} Match ended without score report", log_prefix_category(&guild_name_str, &category_name));

        category.queue_dash_update(ctx, guild_id).await;

        let embed = CE::new().title("Match ended").description(format!("Ended {} match without reporting score.", format_name)).color(0x00FF00);

        interaction.edit_response(&ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;
      }
      Err(e) => {
        error!("Failed to end match: {e}");

        let embed = CE::new().title("Failed to end match").description(format!("Error: {}", e)).color(0xFF0000);

        interaction.edit_response(&ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;
      }
    }
  }

  Ok(())
}

/// Show end match selection with RED WON / DRAW / BLU WON buttons
pub async fn show_end_match_selection(ctx: &Context, interaction: &CI, db: &Arc<Database>, manager: &Arc<tokio::sync::Mutex<Manager>>, guild_id: GI) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !is_role_component(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![Eph::back("runner_menu_back")])]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let user_id = interaction.user.id;

  // Find the runner's active match (same logic as end_without_score)
  let found_match = {
    let mut mgr = manager.lock().await;
    let server = mgr.get_qguild(guild_id)?;

    let mut found_match = None;

    // Strategy 1: Find the only active match in any category
    let mut active_matches = Vec::new();
    for (cat_idx, category) in server.categories.iter().enumerate() {
      for (fmt_idx, format) in category.formats.iter().enumerate() {
        for session in &format.sessions {
          if session.is_active() {
            active_matches.push((cat_idx, fmt_idx, category.id, format.id));
          }
        }
      }
    }

    if active_matches.len() == 1 {
      found_match = Some(active_matches[0]);
    } else if active_matches.len() > 1 {
      // Strategy 2: Check if runner is in any team VC
      if let Some(guild) = ctx.cache.guild(guild_id) {
        if let Some(voice_state) = guild.voice_states.get(&user_id) {
          if let Some(channel_id) = voice_state.channel_id {
            for (cat_idx, category) in server.categories.iter().enumerate() {
              if category.channels.teams.iter().any(|tc| tc.red_vc == channel_id || tc.blu_vc == channel_id) {
                for (fmt_idx, format) in category.formats.iter().enumerate() {
                  if format.sessions.iter().any(|s| s.is_active()) {
                    found_match = Some((cat_idx, fmt_idx, category.id, format.id));
                    break;
                  }
                }
                break;
              }
            }
          }
        }
      }
    }

    found_match
  };

  let (cat_idx, _fmt_idx, category_id, format_id) = match found_match {
    Some(m) => m,
    None => {
      let embed = CE::new()
        .title("No active match found")
        .description("Could not find an active match to end. Either there are no active matches, or you need to be in a team voice channel when multiple matches are running.")
        .color(0xFFAA00);
      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![Eph::back("runner_menu_back")])]));
      interaction.create_response(&ctx.http, response).await?;
      return Ok(());
    }
  };

  // Get format name for display
  let format_name = {
    let mut mgr = manager.lock().await;
    let server = mgr.get_qguild(guild_id)?;
    server.categories[cat_idx].formats.iter().find(|f| f.id == format_id).map(|f| f.name.clone()).unwrap_or_else(|| "Match".to_string())
  };

  let embed = CE::new().title(format!("End {} - Select winner", format_name)).description("Choose the winning team to end the match:").color(0x00AAFF);

  let buttons = vec![
    CAR::Buttons(vec![
      CB::new(format!("runner_end_blu_{}_{}", category_id, format_id)).label("BLU WON").style(BS::Primary),
      CB::new(format!("runner_end_draw_{}_{}", category_id, format_id)).label("DRAW").style(BS::Secondary),
      CB::new(format!("runner_end_red_{}_{}", category_id, format_id)).label("RED WON").style(BS::Danger),
    ]),
    CAR::Buttons(vec![Eph::back("runner_menu_back")]),
  ];

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

/// Handle end match result button click (runner_end_red/draw/blu_{category_id}_{format_id})
pub async fn handle_end_match_result(ctx: &Context, interaction: &CI, db: &Arc<Database>, manager: &Arc<tokio::sync::Mutex<Manager>>) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !is_role_component(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![Eph::back("runner_menu_back")])]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let guild_id = interaction.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;

  // Parse result and IDs from custom_id (format: runner_end_{result}_{category_id}_{format_id})
  let custom_id = &interaction.data.custom_id;
  let parts: Vec<&str> = custom_id.split('_').collect();
  let result = parts.get(2).unwrap_or(&"");
  let category_id = parts.get(3).and_then(|s| s.parse::<u8>().ok());
  let format_id = parts.get(4).and_then(|s| s.parse::<u8>().ok());

  if !matches!(*result, "red" | "draw" | "blu") || category_id.is_none() || format_id.is_none() {
    let embed = CE::new().title("Invalid action").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![Eph::back("runner_menu_back")])]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let category_id = category_id.unwrap();
  let format_id = format_id.unwrap();

  // Check if someone is already submitting a score for this match
  {
    let mgr = manager.lock().await;
    if let Some(submitting_user_id) = mgr.get_active_score_submission(guild_id, category_id, format_id) {
      if submitting_user_id != interaction.user.id {
        // Get the username of the user currently submitting
        let submitting_user_tag = crate::log::get_user_tag(ctx, submitting_user_id, db).await;
        let embed = CE::new().title("Match already being reported").description(format!("{} started reporting this match already", submitting_user_tag)).color(0xFFAA00);
        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![Eph::back("runner_menu_back")])]));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }
    }
    drop(mgr);
  }

  // Mark this user as submitting
  {
    let mut mgr = manager.lock().await;
    mgr.set_active_score_submission(guild_id, category_id, format_id, interaction.user.id);
    drop(mgr);
  }

  let (guild_name_str, category_name, format_name) = {
    let mut mgr = manager.lock().await;
    let server = mgr.get_qguild(guild_id)?;
    let category = server.categories.iter().find(|c| c.id == category_id).ok_or_else(|| anyhow::anyhow!("Category not found"))?;
    let format = category.formats.iter().find(|f| f.id == format_id).ok_or_else(|| anyhow::anyhow!("Format not found"))?;
    (guild_name(ctx, guild_id), category.name.as_deref().unwrap_or("Unknown").to_string(), format.name.clone())
  };

  let result_text = match *result {
    "red" => "RED team victory",
    "draw" => "Draw",
    "blu" => "BLU team victory",
    _ => "Result",
  };

  info!("{} Runner {} ended match with result: {}", log_prefix_category(&guild_name_str, &category_name), interaction.user.tag(), result_text);

  // Capture data needed for ELO processing and mark score as reported, then release the lock.
  let session_players = {
    let mut mgr = manager.lock().await;
    let server = mgr.get_qguild(guild_id)?;
    let category = server.categories.iter_mut().find(|c| c.id == category_id).ok_or_else(|| anyhow::anyhow!("Category not found"))?;

    let players: Vec<crate::models::session::SessionPlayer> =
      category.formats.iter().find(|f| f.id == format_id).and_then(|f| f.sessions.iter().find(|s| s.is_active())).map(|s| s.pool.clone()).unwrap_or_default();

    if let Some(session) = category.formats.iter_mut().find(|f| f.id == format_id).and_then(|f| f.sessions.iter_mut().find(|s| s.is_active())) {
      session.score_reported = true;
    }

    players
    // mgr lock released here
  };

  // Process ELO outside the manager lock (only needs db + ctx)
  let elo_changes = match crate::models::session::process_match_result_with_elo(db.clone(), guild_id, category_id, &session_players, result, ctx).await {
    Ok(changes) => changes,
    Err(e) => {
      error!("{} Failed to process match result with ELO: {e}", log_prefix_category(&guild_name_str, &category_name));
      None
    }
  };

  // Apply ELO changes to in-memory session and call pull_fmt, then release the lock.
  let (pull_result, chat_embed_data) = {
    let mut mgr = manager.lock().await;
    let server = mgr.get_qguild(guild_id)?;
    let category = server.categories.iter_mut().find(|c| c.id == category_id).ok_or_else(|| anyhow::anyhow!("Category not found"))?;

    if let Some(changes) = elo_changes {
      if let Some(session) = category.formats.iter_mut().find(|f| f.id == format_id).and_then(|f| f.sessions.iter_mut().find(|s| s.is_active())) {
        for change in &changes {
          if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == change.user_id) {
            player.player.elo = change.new_elo;
          }
        }
        info!("{} Updated {} players' ELO in session memory", log_prefix_category(&guild_name_str, &category_name), changes.len());
      }
    }

    // Capture match data for chat embed before pull_fmt clears the session
    let queue_chat = category.channels.queue_chat;
    let embed_data = {
      let active_session = category
        .formats
        .iter()
        .find(|f| f.id == format_id)
        .and_then(|f| f.sessions.iter().find(|s| s.status == crate::models::SessionStatus::Live).or_else(|| f.sessions.iter().find(|s| s.status == crate::models::SessionStatus::Hot)));
      if let Some(session) = active_session {
        let quota = category.formats.iter().find(|f| f.id == format_id).map(|f| f.quota as usize).unwrap_or(0);
        let (team_red, team_blu) = crate::models::dashboard::get_sorted_teams_pub(&session.pool, quota);
        let duration = session.started_at.and_then(|started| std::time::SystemTime::now().duration_since(started).ok()).map(|d| d.as_secs());
        let started_at = session.started_at;
        let session_id = session.team_channels.as_ref().and_then(|tc| tc.session_id.clone());
        Some((queue_chat, team_red, team_blu, duration, started_at, session_id))
      } else {
        None
      }
    };

    // pull_fmt takes &mut self (category) and may lock manager internally via the
    // AfterExpiration spawn — pass None here so it doesn't try to re-lock manager.
    let result = category.pull_fmt(format_id, ctx, guild_id, db, None).await;
    if result.is_ok() {
      category.queue_dash_update(ctx, guild_id).await;
    }
    (result, embed_data)
    // mgr lock released here
  };

  // Clear the active score submission now that the lock is free
  {
    let mut mgr = manager.lock().await;
    mgr.clear_active_score_submission(guild_id, category_id, format_id);
  }

  // Record match to database (after manager lock is released)
  let match_id = if let Some((_, team_red, team_blu, _, started_at, session_id)) = chat_embed_data.as_ref() {
    crate::models::Category::record_match_to_database(db, guild_id, category_id, format_id, *started_at, session_id.clone(), team_red, team_blu)
      .await
      .ok()
      .flatten()
  } else {
    None
  };

  match pull_result {
    Ok(_) => {
      info!("{} Match ended with {}", log_prefix_category(&guild_name_str, &category_name), result_text);

      let result_color = match *result {
        "red" => RED,
        "blu" => BLUE,
        _ => 0x888888,
      };

      // Post match result embed to queue chat
      if let Some((queue_chat, team_red, team_blu, duration, _, _)) = chat_embed_data {
        let hide_elo = db.config.get_bool(guild_id, "hide_elo", false).await.unwrap_or(false);
        let dynamic_elo_active = db.config.get_active_elo(guild_id).await.unwrap_or(false);

        let mut chat_embed = CE::new().title(format!("{} - {}", format_name, result_text)).color(result_color);

        if let Some(secs) = duration {
          chat_embed = chat_embed.field("Duration", format!("{}m {}s", secs / 60, secs % 60), true);
        }

        chat_embed = crate::models::dashboard::build_team_fields(chat_embed, team_red, team_blu, hide_elo, dynamic_elo_active, db, guild_id).await;
        let footer_text = match match_id {
          Some(id) => format!("Logged by {} · Game #{}", interaction.user.tag(), id),
          None => format!("Logged by {}", interaction.user.tag()),
        };
        chat_embed = chat_embed.footer(serenity::all::CreateEmbedFooter::new(footer_text));

        let _ = queue_chat.send_message(&ctx.http, serenity::all::CreateMessage::new().embed(chat_embed)).await;
      }

      let description = match match_id {
        Some(id) => format!("**{}** - {} (Game #{})", format_name, result_text, id),
        None => format!("**{}** - {}", format_name, result_text),
      };
      let embed = CE::new().title("Match ended").description(description).color(result_color);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![]));
      interaction.create_response(&ctx.http, response).await?;
    }
    Err(e) => {
      error!("Failed to end match: {e}");

      let embed = CE::new().title("Failed to end match").description(format!("Error: {}", e)).color(0xFF0000);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![Eph::back("runner_menu_back")])]));
      interaction.create_response(&ctx.http, response).await?;
    }
  }

  Ok(())
}
