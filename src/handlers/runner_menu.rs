use anyhow::Result;
use serenity::all::{
  ButtonStyle as BS, ComponentInteraction as CI, Context, CreateActionRow as CAR, CreateButton as CB, CreateEmbed as CE,
  CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, GuildId as GI, UserId as UI,
};
use std::sync::Arc;
use tracing::info;

use crate::db::Database;
use crate::handlers::player::is_role_component;
use crate::handlers::settings::menu::create_selection_menu;
use crate::models::embeds::Ephemeral as Eph;
use crate::models::{ComponentContext as CC, Role};
use crate::{guild_name, log_prefix_guild, Manager, CYAN};

mod runner_menu_end;
pub use runner_menu_end::{handle_end_without_score, show_end_match_selection, handle_end_match_result};

/// Build the runner menu embed and buttons (shared by show and update)
fn build_runner_menu() -> (CE, Vec<CAR>) {
  let embed = CE::new()
    .title("Runner actions")
    .description(
      "**Queue management:**\n\
       • **Remove** - Remove a player from queue\n\
       • **Clear queue** - Remove all players from queue\n\
       • **Buffer** - Move a player to the front of queue\n\
       • **Fatkid** - Move a player to the end of queue\n\n\
       **Match control:**\n\
       • **Accept** - Bypass VC join requirement for players who need more time\n\
       • **End match** - End match and report the winning team\n\
       • **End without score** - End match without reporting score (backup option)",
    )
    .color(CYAN);

  let buttons = vec![
    CAR::Buttons(vec![
      CB::new("runner_action_remove").label("Remove").style(BS::Primary),
      CB::new("runner_action_clear_queue").label("Clear queue").style(BS::Danger),
      CB::new("runner_action_buffer").label("Buffer").style(BS::Primary),
      CB::new("runner_action_fatkid").label("Fatkid").style(BS::Primary),
    ]),
    CAR::Buttons(vec![
      CB::new("runner_action_accept").label("Force accept").style(BS::Primary),
      CB::new("runner_action_end_match").label("End match").style(BS::Success),
      CB::new("runner_action_end_no_score").label("End without score").style(BS::Danger),
    ]),
  ];

  (embed, buttons)
}

/// Show runner menu as a new ephemeral message (for initial open from dashboard)
pub async fn show_runner_menu(cc: &CC<'_>) -> Result<()> {
  // Defer with ephemeral response before async work
  let response = CIR::Defer(CIRM::new().ephemeral(true));
  cc.component.create_response(&cc.ctx.http, response).await?;

  if !is_role_component(cc, &Role::Runner).await? {
    use serenity::all::CreateInteractionResponseFollowup as CIRF;
    let followup = CIRF::new().content("Only runners can access this menu.").ephemeral(true);
    cc.component.create_followup(&cc.ctx.http, followup).await?;
    return Ok(());
  }

  let (embed, buttons) = build_runner_menu();

  // Use followup since we deferred
  use serenity::all::CreateInteractionResponseFollowup as CIRF;
  let followup = CIRF::new().embed(embed).components(buttons).ephemeral(true);
  cc.component.create_followup(&cc.ctx.http, followup).await?;

  Ok(())
}

/// Update existing ephemeral to show runner menu (for back button)
pub async fn update_runner_menu(cc: &CC<'_>) -> Result<()> {
  if !is_role_component(cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can access this menu.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![]));
    cc.component.create_response(&cc.ctx.http, response).await?;
    return Ok(());
  }

  let (embed, buttons) = build_runner_menu();
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
  cc.component.create_response(&cc.ctx.http, response).await?;

  Ok(())
}

pub async fn handle_runner_action(
  ctx: &Context,
  interaction: &CI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  action: &str,
) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !is_role_component(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
      CAR::Buttons(vec![Eph::back("runner_menu_back")])
    ]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let guild_id = interaction.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;

  match action {
    "accept" => {
      handle_direct_action(ctx, interaction, db, manager, guild_id, action).await
    }
    "end_match" => {
      show_end_match_selection(ctx, interaction, db, manager, guild_id).await
    }
    "end_no_score" => {
      handle_end_without_score(ctx, interaction, db, manager, guild_id).await
    }
    "clear_queue" => {
      handle_clear_queue(ctx, interaction, db, manager, guild_id).await
    }
    "remove" | "buffer" | "fatkid" => {
      show_player_selection(ctx, interaction, db, manager, guild_id, action).await
    }
    _ => Ok(()),
  }
}

async fn handle_direct_action(
  _ctx: &Context,
  interaction: &CI,
  _db: &Arc<Database>,
  _manager: &Arc<tokio::sync::Mutex<Manager>>,
  _guild_id: GI,
  action: &str,
) -> Result<()> {
  // Note: accept function expects CommandContext with CommandInteraction
  // We need to defer this response and use a different approach
  let error_embed = CE::new()
    .title(format!("The '{}' action must be triggered via slash command", action))
    .color(0xFFAA00);
  let response = CIR::UpdateMessage(CIRM::new().embed(error_embed).components(vec![
    CAR::Buttons(vec![Eph::back("runner_menu_back")])
  ]));
  interaction.create_response(&_ctx.http, response).await?;
  
  Ok(())
}

async fn show_player_selection(
  ctx: &Context,
  interaction: &CI,
  _db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
  action: &str,
) -> Result<()> {
  let mut mgr = manager.lock().await;
  let server = mgr.get_server(guild_id)?;

  let mut players = Vec::new();

  // Use cache for member nicknames (much faster than API calls)
  // Extract member display names from cache before dropping the reference
  let member_names: std::collections::HashMap<UI, String> = ctx.cache.guild(guild_id)
    .map(|g| {
      g.members.iter()
        .map(|(uid, m)| (*uid, m.display_name().to_string()))
        .collect()
    })
    .unwrap_or_default();

  for category in &server.categories {
    for format in &category.formats {
      for session in &format.sessions {
        for session_player in &session.pool {
          let user_id = session_player.player.user_id;
          
          // Try to get server nickname from extracted cache data, fallback to Discord username
          let display_name = member_names.get(&user_id)
            .cloned()
            .unwrap_or_else(|| session_player.player.tag.clone());
          
          if !players.iter().any(|(_, id)| *id == format!("{}", user_id.get())) {
            players.push((display_name, format!("{}", user_id.get())));
          }
        }
      }
    }
  }

  drop(mgr);

  if players.is_empty() {
    let embed = CE::new().title("No players are currently in any queue").color(0xFFAA00);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
      CAR::Buttons(vec![Eph::back("runner_menu_back")])
    ]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let title = match action {
    "remove" => "Select a player to remove:",
    "buffer" => "Select a player to buffer:",
    "fatkid" => "Select a player to fatkid:",
    _ => "Select a player:",
  };

  let embed = CE::new().title(title).color(CYAN);

  let mut components = Vec::new();
  
  if let Some(menu) = create_selection_menu(&format!("runner_player_{}", action), "Select player", players) {
    components.push(menu);
  }
  components.push(CAR::Buttons(vec![Eph::back("runner_menu_back")]));

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

pub async fn handle_player_selection( ctx: &Context, interaction: &CI, db: &Arc<Database>, manager: &Arc<tokio::sync::Mutex<Manager>>, action: &str, user_id_str: &str ) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !is_role_component(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
      CAR::Buttons(vec![Eph::back("runner_menu_back")])
    ]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let user_id = UI::new(user_id_str.parse::<u64>()?);
  let guild_id = interaction.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;
  let runner_tag = crate::get_user_tag(ctx, interaction.user.id, db).await;

  let mut mgr = manager.lock().await;
  let server = mgr.get_server(guild_id)?;

  // Execute the action directly on the server
  // Track which category index and what action description to set
  let mut last_action_info: Option<(usize, String)> = None;
  // Track if we need to cancel a queue expiration (for remove action)
  
  let result = match action {
    "remove" => {
      // Find and remove the player from all sessions
      let mut found = false;
      for (cat_idx, category) in server.categories.iter_mut().enumerate() {
        for format in &mut category.formats {
          let quota = format.quota as usize;
          for session in &mut format.sessions {
            if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == user_id) {
              let player_tag = session.pool[pos].player.tag.clone();
              session.pool.remove(pos);
              found = true;
              if let Some(scheduler) = ctx.data.read().await.get::<crate::QueueExpirationSchedulerKey>() {
                let mut sched = scheduler.lock().await;
                sched.cancel_queue_expiration(guild_id, category.id, format.id, user_id);
              }
              let guild_name = guild_name(ctx, guild_id);
              let category_name = category.name.as_deref().unwrap_or("Unknown");
              let format_name = &format.name;
              info!("{} Runner removed player {} from queue", crate::log::log_prefix_format(&guild_name, category_name, format_name), player_tag);
              last_action_info = Some((cat_idx, format!("removed {}", player_tag)));
              
              // If this was a Hot session and now below quota, transition back to Idle
              if session.is_hot() && session.pool.len() < quota {
                session.idle();
                info!("{} Hot session dropped below quota after removing player, transitioning back to Idle", crate::log::log_prefix_format(&guild_name, category_name, format_name));
              }
            }
          }
        }
      }
      if found {
        Ok(())
      } else {
        Err(anyhow::anyhow!("Player not found in any queue"))
      }
    }
    "buffer" => {
      // Move player to front of queue
      let mut found = false;
      for (cat_idx, category) in server.categories.iter_mut().enumerate() {
        for format in &mut category.formats {
          for session in &mut format.sessions {
            if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == user_id) {
              let player_tag = session.pool[pos].player.tag.clone();
              let player = session.pool.remove(pos);
              session.pool.insert(0, player);
              found = true;
              let guild_name = guild_name(ctx, guild_id);
              let ctg_nm = category.name.as_deref().unwrap_or("Unknown");
              let fmt_nm = &format.name;
              info!("{} Runner buffered player {} to front of queue", crate::log::log_prefix_format(&guild_name, ctg_nm, fmt_nm), player_tag);
              last_action_info = Some((cat_idx, format!("buffered {}", player_tag)));
            }
          }
        }
      }
      if found {
        Ok(())
      } else {
        Err(anyhow::anyhow!("Player not found in any queue"))
      }
    }
    "fatkid" => {
      // Move player to end of queue
      let mut found = false;
      for (cat_idx, category) in server.categories.iter_mut().enumerate() {
        for format in &mut category.formats {
          for session in &mut format.sessions {
            if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == user_id) {
              let player_tag = session.pool[pos].player.tag.clone();
              let player = session.pool.remove(pos);
              session.pool.push(player);
              found = true;
              let guild_name = guild_name(ctx, guild_id);
              let ctg_nm = category.name.as_deref().unwrap_or("Unknown");
              let fmt_nm = &format.name;
              info!("{} Runner fatkidded player {} to end of queue", crate::log::log_prefix_format(&guild_name, ctg_nm, fmt_nm), player_tag);
              last_action_info = Some((cat_idx, format!("fatkidded {}", player_tag)));
            }
          }
        }
      }
      if found {
        Ok(())
      } else {
        Err(anyhow::anyhow!("Player not found in any queue"))
      }
    }
    _ => Ok(()),
  };
  
  // Set last action after the loops to avoid borrow issues
  if let Some((cat_idx, action_desc)) = last_action_info {
    if let Some(category) = server.categories.get_mut(cat_idx) {
      category.set_last_action(runner_tag.clone(), &action_desc);
    }
  }

  // Update dashboard if action was successful
  let success = result.is_ok();
  drop(mgr);

  if success {
    // Regenerate teams for Hot sessions if buffer/fatkid changed player order
    if action == "buffer" || action == "fatkid" {
      let mut mgr = manager.lock().await;
      let server = mgr.get_server(guild_id)?;
      for category in &mut server.categories {
        // Validate VC status to sync in_queue_vc flags with actual Discord state
        // This ensures the dashboard shows correct VC status after player reordering
        category.validate_vc_status(ctx, guild_id).await;
        
        // Collect format IDs that need team regeneration
        let hot_fmt_ids: Vec<u8> = category.formats.iter()
          .filter(|f| f.sessions.iter().any(|s| s.is_hot() && s.pool.len() >= f.quota as usize))
          .map(|f| f.id)
          .collect();
        
        // Regenerate teams for each hot format
        for fmt_id in hot_fmt_ids {
          category.generate_teams_fmt(fmt_id, ctx, guild_id, Some(db)).await;
        }
      }
      drop(mgr);
    }
    
    // Update dashboard for all affected categories
    let mut mgr = manager.lock().await;
    let server = mgr.get_server(guild_id)?;
    for category in &mut server.categories {
      category.queue_dash_update(ctx, guild_id).await;
    }
    drop(mgr);
  }

  let (title, color) = if let Err(e) = result {
    (format!("Action failed: {}", e), 0xFF0000)
  } else {
    let action_name = match action {
      "remove" => "removed from",
      "buffer" => "buffered to front of",
      "fatkid" => "moved to end of",
      _ => "modified in",
    };
    (format!("Player {} queue", action_name), 0x00FF00)
  };

  let embed = CE::new().title(title).color(color);
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
    CAR::Buttons(vec![Eph::back("runner_menu_back")])
  ]));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

async fn handle_clear_queue(
  ctx: &Context,
  interaction: &CI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !is_role_component(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
      CAR::Buttons(vec![Eph::back("runner_menu_back")])
    ]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let runner_tag = crate::get_user_tag(ctx, interaction.user.id, db).await;

  let mut mgr = manager.lock().await;
  let server = mgr.get_server(guild_id)?;

  let mut removed_count = 0;

  // Remove all players from all idle sessions only
  for category in &mut server.categories {
    for format in &mut category.formats {
      for session in &mut format.sessions {
        if session.is_idle() {
          removed_count += session.pool.len();
          session.pool.clear();
        }
      }
    }
    if removed_count > 0 {
      category.set_last_action(runner_tag.clone(), "cleared the queue");
    }
  }

  drop(mgr);

  // Update dashboard for all categories
  if removed_count > 0 {
    let mut mgr = manager.lock().await;
    let server = mgr.get_server(guild_id)?;
    for category in &mut server.categories {
      category.queue_dash_update(ctx, guild_id).await;
    }
    drop(mgr);
  }

  let guild_name = guild_name(ctx, guild_id);
  info!("{} Runner cleared queue ({} players)", log_prefix_guild(&guild_name), removed_count);

  let title = if removed_count > 0 {
    format!("Cleared queue: removed {} player{}", removed_count, if removed_count == 1 { "" } else { "s" })
  } else {
    "Queue was already empty".to_string()
  };

  let embed = CE::new().title(title).color(0x00FF00);
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
    CAR::Buttons(vec![Eph::back("runner_menu_back")])
  ]));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

pub async fn handle_remove_all(
  ctx: &Context,
  interaction: &CI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !is_role_component(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
      CAR::Buttons(vec![Eph::back("runner_menu_back")])
    ]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let guild_id = interaction.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;

  let mut mgr = manager.lock().await;
  let server = mgr.get_server(guild_id)?;

  let mut removed_count = 0;

  // Remove all players from all sessions
  for category in &mut server.categories {
    for format in &mut category.formats {
      for session in &mut format.sessions {
        removed_count += session.pool.len();
        session.pool.clear();
      }
    }
  }

  drop(mgr);

  // Update dashboard for all categories
  if removed_count > 0 {
    let mut mgr = manager.lock().await;
    let server = mgr.get_server(guild_id)?;
    for category in &mut server.categories {
      category.queue_dash_update(ctx, guild_id).await;
    }
    drop(mgr);
  }

  let guild_name = guild_name(ctx, guild_id);
  info!("{} Runner removed all players from queue ({} players)", log_prefix_guild(&guild_name), removed_count);

  let title = if removed_count > 0 {
    format!("Removed all {} player{} from queue", removed_count, if removed_count == 1 { "" } else { "s" })
  } else {
    "No players were in queue".to_string()
  };

  let embed = CE::new().title(title).color(0x00FF00);
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
    CAR::Buttons(vec![Eph::back("runner_menu_back")])
  ]));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}
