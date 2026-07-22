use anyhow::Result;
use serenity::all::{
  ButtonStyle as BS, ComponentInteraction as CIx, Context, CreateActionRow as CAR, CreateButton as CB, CreateEmbed as CE,
  CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, GuildId as GI, UserId as UI,
};
use std::sync::Arc;
use tracing::info;

use crate::{Database, Manager, SessionStatus, Team, CYAN};

/// Handle captain mode button press from runner menu
pub async fn handle_captain_mode(
  ctx: &Context,
  interaction: &CIx,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
) -> Result<()> {
  // Defer response
  let response = CIR::Defer(CIRM::new().ephemeral(true));
  interaction.create_response(&ctx.http, response).await?;

  let mut mgr = manager.lock().await;
  let server = mgr.get_qguild(guild_id)?;

  // Find all Hot sessions across all categories and formats
  let mut hot_formats: Vec<(u8, u8, String)> = Vec::new(); // (category_id, format_id, format_name)

  for category in &server.categories {
    for format in &category.formats {
      if format.sessions.iter().any(|s| s.status == SessionStatus::Hot) {
        hot_formats.push((category.id, format.id, format.name.clone()));
      }
    }
  }

  if hot_formats.is_empty() {
    let followup = serenity::all::CreateInteractionResponseFollowup::new()
      .content("No Hot games found. Captain mode can only be started when a game is ready (Hot status).")
      .ephemeral(true);
    interaction.create_followup(&ctx.http, followup).await?;
    return Ok(());
  }

  // If multiple Hot games, show format selection
  if hot_formats.len() > 1 {
    show_format_selection(ctx, interaction, guild_id, &hot_formats).await?;
  } else {
    // Single Hot game, proceed directly
    let (category_id, format_id, _) = &hot_formats[0];
    start_captain_draft(ctx, interaction, db, manager, guild_id, *category_id, *format_id).await?;
  }

  Ok(())
}

/// Show format selection when multiple Hot games exist
async fn show_format_selection(
  ctx: &Context,
  interaction: &CIx,
  _guild_id: GI,
  hot_formats: &[(u8, u8, String)],
) -> Result<()> {
  let mut buttons = Vec::new();

  for (category_id, format_id, format_name) in hot_formats {
    let custom_id = format!("captain_select_{}_{}", category_id, format_id);
    buttons.push(CB::new(custom_id).label(format_name).style(BS::Primary));
  }

  let embed = CE::new()
    .title("Select Format for Captain Mode")
    .description("Multiple games are ready. Select which format to use for captain mode.")
    .color(CYAN);

  let followup = serenity::all::CreateInteractionResponseFollowup::new()
    .embed(embed)
    .components(vec![CAR::Buttons(buttons)])
    .ephemeral(true);
  interaction.create_followup(&ctx.http, followup).await?;

  Ok(())
}

/// Start captain draft for a specific format
pub async fn start_captain_draft(
  ctx: &Context,
  interaction: &CIx,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
  category_id: u8,
  format_id: u8,
) -> Result<()> {
  // First, extract needed data with lock held
  let (captain_red, captain_blu, sorted_players, dashboard_channel, pick_order) = {
    let mut mgr = manager.lock().await;
    let _server = mgr.get_qguild(guild_id)?;
    let category = mgr.get_category_by_id(guild_id, category_id)?;
    let format = category.format(format_id).ok_or_else(|| anyhow::anyhow!("Format not found"))?;

    // Find the Hot session
    let hot_session = format
      .sessions
      .iter()
      .find(|s| s.status == SessionStatus::Hot)
      .ok_or_else(|| anyhow::anyhow!("No Hot session found"))?;

    // Sort players by ELO to find captains
    let mut sorted_players = hot_session.pool.clone();
    sorted_players.sort_by(|a, b| b.player.elo.cmp(&a.player.elo));

    if sorted_players.len() < 2 {
      drop(mgr);
      let followup = serenity::all::CreateInteractionResponseFollowup::new()
        .content("Need at least 2 players for captain mode.")
        .ephemeral(true);
      interaction.create_followup(&ctx.http, followup).await?;
      return Ok(());
    }

    // Get the two highest ELO players as captains
    let captain_red = sorted_players[0].player.user_id;
    let captain_blu = sorted_players[1].player.user_id;

    info!(
      "Starting captain mode: Red={}, Blu={}, Total players={}",
      captain_red, captain_blu, sorted_players.len()
    );

    // Get dashboard channel before mutable borrow
    let dashboard_channel = category.channels.dashboard;

    // Clear existing team assignments from BCH
    let format_mut = category.format_mut(format_id).unwrap();
    for session in &mut format_mut.sessions {
      if session.status == SessionStatus::Hot {
        for player in &mut session.pool {
          player.team = None;
        }
      }
    }

    // Create ABBAAB pick order
    let pick_order = vec![0, 1, 1, 0, 0, 1]; // Red, Blu, Blu, Red, Red, Blu

    (captain_red, captain_blu, sorted_players, dashboard_channel, pick_order)
  };

  // Create draft embed with player buttons (no lock held)
  let draft_embed = create_draft_embed(&sorted_players, captain_red, captain_blu, 0, &pick_order, 0).await?;

  let message = dashboard_channel
    .send_message(&ctx.http, draft_embed)
    .await?;

  let draft_message_id = message.id;

  // Save draft state to database (no lock held)
  db.captain_drafts
    .save_draft(guild_id, category_id, format_id, dashboard_channel.get(), draft_message_id.get())
    .await?;

  // Re-acquire lock to update in-memory state
  let mut mgr = manager.lock().await;
  let category = mgr.get_category_by_id(guild_id, category_id)?;
  let format_mut = category.format_mut(format_id).unwrap();

  use crate::models::server::CaptainDraft;
  format_mut.captain_draft = Some(CaptainDraft {
    captains: (captain_red, captain_blu),
    current_turn: 0,
    pick_order: pick_order.clone(),
    current_pick_index: 0,
    draft_channel_id: dashboard_channel,
    draft_message_id,
  });

  // Update dashboard (still holding lock)
  category.queue_dash_update(ctx, guild_id).await;

  drop(mgr);

  // Send followup message (no lock held)
  let followup = serenity::all::CreateInteractionResponseFollowup::new()
    .content(format!(
      "Captain mode started! Captains: <@{}> (Red) and <@{}> (Blue)",
      captain_red, captain_blu
    ))
    .ephemeral(true);
  interaction.create_followup(&ctx.http, followup).await?;

  Ok(())
}

/// Create the draft embed with player buttons
async fn create_draft_embed(
  players: &[crate::models::SessionPlayer],
  captain_red: UI,
  captain_blu: UI,
  current_turn: usize,
  pick_order: &[usize],
  current_pick_index: usize,
) -> Result<serenity::all::CreateMessage> {
  let captain_name = if current_turn == 0 { "Red" } else { "Blue" };
  let captain_id = if current_turn == 0 { captain_red } else { captain_blu };

  let mut description = format!(
    "**Captain Mode - Draft Phase**\n\n\
     **Red Captain:** <@{}>\n\
     **Blue Captain:** <@{}>\n\n\
     **Current Turn:** {} Captain (<@{}>)\n\
     **Pick {}/{}**\n\n\
     **Available Players:**",
    captain_red, captain_blu, captain_name, captain_id, current_pick_index + 1, pick_order.len()
  );

  // Create player buttons in 2 rows
  let mut buttons_row1 = Vec::new();
  let mut buttons_row2 = Vec::new();

  let available_players: Vec<_> = players
    .iter()
    .filter(|p| p.player.user_id != captain_red && p.player.user_id != captain_blu && p.team.is_none())
    .collect();

  for (i, player) in available_players.iter().enumerate() {
    let custom_id = format!("captain_pick_{}_{}", player.player.user_id, current_turn);
    let button = CB::new(custom_id).label(&player.player.tag).style(BS::Primary);

    if i < 5 {
      buttons_row1.push(button);
    } else {
      buttons_row2.push(button);
    }

    description.push_str(&format!("\n• {}", player.player.tag));
  }

  let mut action_rows = Vec::new();
  if !buttons_row1.is_empty() {
    action_rows.push(CAR::Buttons(buttons_row1));
  }
  if !buttons_row2.is_empty() {
    action_rows.push(CAR::Buttons(buttons_row2));
  }

  // Add cancel button
  action_rows.push(CAR::Buttons(vec![CB::new("captain_cancel").label("Cancel Draft").style(BS::Danger)]));

  let embed = CE::new()
    .title("Captain Draft")
    .description(description)
    .color(CYAN);

  Ok(serenity::all::CreateMessage::new().embed(embed).components(action_rows))
}

/// Handle a captain picking a player
pub async fn handle_captain_pick(
  ctx: &Context,
  interaction: &CIx,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
  picked_user_id: UI,
  turn: usize,
) -> Result<()> {
  let mut mgr = manager.lock().await;
  let server = mgr.get_qguild(guild_id)?;

  // Find the active captain draft
  let (category_id, format_id, draft_state) = find_active_draft(server)?;

  // Verify the picker is the current captain
  let current_captain = if draft_state.current_turn == 0 {
    draft_state.captains.0
  } else {
    draft_state.captains.1
  };

  if interaction.user.id != current_captain {
    // Check if user is admin (edge case)
    let is_admin = crate::handlers::player::is_role_component(
      &crate::models::ComponentContext { ctx, component: interaction, db: db.clone(), manager },
      &crate::models::Role::Admin,
    )
    .await?;

    if !is_admin {
      let response = CIR::UpdateMessage(CIRM::new().content("It's not your turn to pick!").ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
      return Ok(());
    }
  }

  // Assign the picked player to the appropriate team
  let team = if turn == 0 { Team::Red } else { Team::Blu };

  let category = mgr.get_category_by_id(guild_id, category_id)?;
  let format_mut = category.format_mut(format_id).unwrap();

  for session in &mut format_mut.sessions {
    if session.status == SessionStatus::Hot {
      for player in &mut session.pool {
        if player.player.user_id == picked_user_id {
          player.team = Some(team);
          break;
        }
      }
    }
  }

  // Update draft state
  if let Some(draft) = &mut format_mut.captain_draft {
    draft.current_pick_index += 1;

    // Check if draft is complete
    if draft.current_pick_index >= draft.pick_order.len() {
      // Draft complete, show start button
      complete_draft(ctx, interaction, db, manager, guild_id, category_id, format_id, draft).await?;
      return Ok(());
    }

    // Move to next turn
    draft.current_turn = draft.pick_order[draft.current_pick_index];
  }

  // Update the draft embed
  update_draft_embed(ctx, db, manager, guild_id, category_id, format_id).await?;

  // Update dashboard
  drop(mgr);
  let mut mgr = manager.lock().await;
  if let Ok(server) = mgr.get_qguild(guild_id) {
    if let Some(category) = server.categories.iter_mut().find(|c| c.id == category_id) {
      category.queue_dash_update(ctx, guild_id).await;
    }
  }

  let response = CIR::UpdateMessage(CIRM::new().content("Player picked!").ephemeral(true));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

/// Handle draft cancellation
pub async fn handle_captain_cancel(
  ctx: &Context,
  interaction: &CIx,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
) -> Result<()> {
  let mut mgr = manager.lock().await;
  let server = mgr.get_qguild(guild_id)?;

  // Find the active captain draft
  let (category_id, format_id, draft_state) = find_active_draft(server)?;

  // Delete draft message
  draft_state
    .draft_channel_id
    .delete_message(&ctx.http, draft_state.draft_message_id)
    .await?;

  // Clear draft state
  let category = mgr.get_category_by_id(guild_id, category_id)?;
  let format_mut = category.format_mut(format_id).unwrap();
  format_mut.captain_draft = None;

  // Clear database record
  db.captain_drafts.delete_draft(guild_id, category_id, format_id).await?;

  // Re-run BCH to restore automatic teams
  for session in &mut format_mut.sessions {
    if session.status == SessionStatus::Hot {
      crate::gui::command_handler::bch_assign_teams(&mut session.pool);
    }
  }

  // Update dashboard
  drop(mgr);
  let mut mgr = manager.lock().await;
  if let Ok(server) = mgr.get_qguild(guild_id) {
    if let Some(category) = server.categories.iter_mut().find(|c| c.id == category_id) {
      category.queue_dash_update(ctx, guild_id).await;
    }
  }

  let response = CIR::UpdateMessage(CIRM::new().content("Draft cancelled. Teams have been rebalanced automatically.").ephemeral(true));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

/// Handle starting the game after draft completion
pub async fn handle_captain_start(
  ctx: &Context,
  interaction: &CIx,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
) -> Result<()> {
  let mut mgr = manager.lock().await;
  let server = mgr.get_qguild(guild_id)?;

  // Find the active captain draft
  let (category_id, format_id, draft_state) = find_active_draft(server)?;

  // Delete draft message
  draft_state
    .draft_channel_id
    .delete_message(&ctx.http, draft_state.draft_message_id)
    .await?;

  // Clear draft state
  let category = mgr.get_category_by_id(guild_id, category_id)?;
  let format_mut = category.format_mut(format_id).unwrap();
  format_mut.captain_draft = None;

  // Clear database record
  db.captain_drafts.delete_draft(guild_id, category_id, format_id).await?;

  // Proceed with normal game start logic - transition to Live
  let category = mgr.get_category_by_id(guild_id, category_id)?;
  let format_mut = category.format_mut(format_id).unwrap();

  for session in &mut format_mut.sessions {
    if session.status == SessionStatus::Hot {
      session.status = SessionStatus::Live;
      break;
    }
  }

  drop(mgr);

  // Update dashboard
  let mut mgr = manager.lock().await;
  if let Ok(server) = mgr.get_qguild(guild_id) {
    if let Some(category) = server.categories.iter_mut().find(|c| c.id == category_id) {
      category.queue_dash_update(ctx, guild_id).await;
    }
  }

  let response = CIR::UpdateMessage(CIRM::new().content("Game started!").ephemeral(true));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

/// Find the active captain draft across all categories and formats
fn find_active_draft(server: &crate::models::QGuild) -> Result<(u8, u8, crate::models::server::CaptainDraft)> {
  for category in &server.categories {
    for format in &category.formats {
      if let Some(draft) = &format.captain_draft {
        return Ok((category.id, format.id, draft.clone()));
      }
    }
  }
  Err(anyhow::anyhow!("No active captain draft found"))
}

/// Update the draft embed after a pick
async fn update_draft_embed(
  ctx: &Context,
  _db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
  category_id: u8,
  format_id: u8,
) -> Result<()> {
  let mut mgr = manager.lock().await;
  let _server = mgr.get_qguild(guild_id)?;
  let category = mgr.get_category_by_id(guild_id, category_id)?;
  let format = category.format(format_id).unwrap();

  let draft_state = format.captain_draft.as_ref().ok_or_else(|| anyhow::anyhow!("No active draft"))?;

  // Get current session players
  let hot_session = format
    .sessions
    .iter()
    .find(|s| s.status == SessionStatus::Hot)
    .ok_or_else(|| anyhow::anyhow!("No Hot session"))?;

  let _new_embed = create_draft_embed(
    &hot_session.pool,
    draft_state.captains.0,
    draft_state.captains.1,
    draft_state.current_turn,
    &draft_state.pick_order,
    draft_state.current_pick_index,
  )
  .await?;

  draft_state
    .draft_channel_id
    .edit_message(&ctx.http, draft_state.draft_message_id, create_draft_embed_edit(
      &hot_session.pool,
      draft_state.captains.0,
      draft_state.captains.1,
      draft_state.current_turn,
      &draft_state.pick_order,
      draft_state.current_pick_index,
    ).await?)
    .await?;

  Ok(())
}

/// Create the draft embed edit for updating an existing message
async fn create_draft_embed_edit(
  players: &[crate::models::SessionPlayer],
  captain_red: UI,
  captain_blu: UI,
  current_turn: usize,
  pick_order: &[usize],
  current_pick_index: usize,
) -> Result<serenity::all::EditMessage> {
  let captain_name = if current_turn == 0 { "Red" } else { "Blue" };
  let captain_id = if current_turn == 0 { captain_red } else { captain_blu };

  let mut description = format!(
    "**Captain Mode - Draft Phase**\n\n\
     **Red Captain:** <@{}>\n\
     **Blue Captain:** <@{}>\n\n\
     **Current Turn:** {} Captain (<@{}>)\n\
     **Pick {}/{}**\n\n\
     **Available Players:**",
    captain_red, captain_blu, captain_name, captain_id, current_pick_index + 1, pick_order.len()
  );

  // Create player buttons in 2 rows
  let mut buttons_row1 = Vec::new();
  let mut buttons_row2 = Vec::new();

  let available_players: Vec<_> = players
    .iter()
    .filter(|p| p.player.user_id != captain_red && p.player.user_id != captain_blu && p.team.is_none())
    .collect();

  for (i, player) in available_players.iter().enumerate() {
    let custom_id = format!("captain_pick_{}_{}", player.player.user_id, current_turn);
    let button = CB::new(custom_id).label(&player.player.tag).style(BS::Primary);

    if i < 5 {
      buttons_row1.push(button);
    } else {
      buttons_row2.push(button);
    }

    description.push_str(&format!("\n• {}", player.player.tag));
  }

  let mut action_rows = Vec::new();
  if !buttons_row1.is_empty() {
    action_rows.push(CAR::Buttons(buttons_row1));
  }
  if !buttons_row2.is_empty() {
    action_rows.push(CAR::Buttons(buttons_row2));
  }

  // Add cancel button
  action_rows.push(CAR::Buttons(vec![CB::new("captain_cancel").label("Cancel Draft").style(BS::Danger)]));

  let embed = CE::new()
    .title("Captain Draft")
    .description(description)
    .color(CYAN);

  Ok(serenity::all::EditMessage::new().embed(embed).components(action_rows))
}

/// Complete the draft and show start button
async fn complete_draft(
  ctx: &Context,
  interaction: &CIx,
  _db: &Arc<Database>,
  _manager: &Arc<tokio::sync::Mutex<Manager>>,
  _guild_id: GI,
  _category_id: u8,
  _format_id: u8,
  draft_state: &crate::models::server::CaptainDraft,
) -> Result<()> {
  let embed = CE::new()
    .title("Captain Draft Complete")
    .description(
      format!(
        "**Teams have been assigned!**\n\n\
         **Red Captain:** <@{}>\n\
         **Blue Captain:** <@{}>\n\n\
         Press **Start Game** to begin the match.",
        draft_state.captains.0, draft_state.captains.1
      ),
    )
    .color(CYAN);

  let buttons = vec![CAR::Buttons(vec![
    CB::new("captain_start").label("Start Game").style(BS::Success),
    CB::new("captain_cancel").label("Cancel").style(BS::Danger),
  ])];

  let new_embed = serenity::all::EditMessage::new().embed(embed).components(buttons);

  draft_state
    .draft_channel_id
    .edit_message(&ctx.http, draft_state.draft_message_id, new_embed)
    .await?;

  let response = CIR::UpdateMessage(CIRM::new().content("Draft complete! Teams are ready."));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}
