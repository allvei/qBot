use anyhow::Result;
use serenity::all::{
  ButtonStyle as BS, ComponentInteraction as CI, Context, CreateActionRow as CAR, CreateButton as CB, CreateEmbed as CE,
  CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, GuildId as GI, UserId as UI,
};
use std::sync::Arc;
use tracing::info;

use crate::db::Database;
use crate::handlers::player::check_component_role;
use crate::handlers::settings::menu::create_selection_menu;
use crate::models::embeds::Ephemeral as Eph;
use crate::models::{ComponentContext as CC, Role};
use crate::{guild_name, log_prefix_guild, Manager, CYAN};

pub async fn show_runner_menu(cc: &CC<'_>) -> Result<()> {
  if !check_component_role(cc, &Role::Runner).await? {
    cc.reply("Only runners can access this menu.").await?;
    return Ok(());
  }

  let embed = CE::new()
    .title("Runner actions")
    .description(
      "**Queue management:**\n\
       • **Remove** - Remove players or a player from queue\n\
       • **Buffer** - Move a player to the front of queue\n\
       • **Fatkid** - Move a player to the end of queue\n\n\
       **Match control:**\n\
       • **Accept** - Bypass VC join requirement for players who need more time",
    )
    .color(CYAN);

  let buttons = vec![
    CAR::Buttons(vec![
      CB::new("runner_action_remove").label("Remove player").style(BS::Primary),
      CB::new("runner_action_buffer").label("Buffer player").style(BS::Primary),
      CB::new("runner_action_fatkid").label("Fatkid player").style(BS::Primary),
      CB::new("runner_action_accept").label("Force accept").style(BS::Primary),
    ]),
  ];

  let response = CIR::Message(CIRM::new().embed(embed).components(buttons).ephemeral(true));
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

  if !check_component_role(&cc, &Role::Runner).await? {
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

  // Get guild to fetch member nicknames
  let guild = ctx.http.get_guild(guild_id).await.ok();

  for category in &server.categories {
    for format in &category.formats {
      for session in &format.sessions {
        for session_player in &session.pool {
          let user_id = session_player.player.user_id;
          
          // Try to get server nickname, fallback to Discord username
          let display_name = if let Some(ref _g) = guild {
            if let Ok(member) = ctx.http.get_member(guild_id, user_id).await {
              member.display_name().to_string()
            } else {
              session_player.player.tag.clone()
            }
          } else {
            session_player.player.tag.clone()
          };
          
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
  
  // Add "Remove all" button for remove action
  if action == "remove" {
    components.push(CAR::Buttons(vec![
      CB::new("runner_action_remove_all").label("Remove all players").style(BS::Primary)
    ]));
  }
  
  if let Some(menu) = create_selection_menu(&format!("runner_player_{}", action), "Select player", players) {
    components.push(menu);
  }
  components.push(CAR::Buttons(vec![Eph::back("runner_menu_back")]));

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

pub async fn handle_player_selection(
  ctx: &Context,
  interaction: &CI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  action: &str,
  user_id_str: &str,
) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !check_component_role(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
      CAR::Buttons(vec![Eph::back("runner_menu_back")])
    ]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let user_id = UI::new(user_id_str.parse::<u64>()?);
  let guild_id = interaction.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;

  let mut mgr = manager.lock().await;
  let server = mgr.get_server(guild_id)?;

  // Execute the action directly on the server
  let result = match action {
    "remove" => {
      // Find and remove the player from all sessions
      let mut found = false;
      for category in &mut server.categories {
        for format in &mut category.formats {
          for session in &mut format.sessions {
            if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == user_id) {
              let player_tag = session.pool[pos].player.tag.clone();
              session.pool.remove(pos);
              found = true;
              let gld_nm = guild_name(ctx, guild_id);
              let ctg_nm = category.name.as_deref().unwrap_or("Unknown");
              let fmt_nm = &format.name;
              info!("{} Runner removed player {} from queue", crate::log::log_prefix_format(&gld_nm, ctg_nm, fmt_nm), player_tag);
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
      for category in &mut server.categories {
        for format in &mut category.formats {
          for session in &mut format.sessions {
            if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == user_id) {
              let player_tag = session.pool[pos].player.tag.clone();
              let player = session.pool.remove(pos);
              session.pool.insert(0, player);
              found = true;
              let gld_nm = guild_name(ctx, guild_id);
              let ctg_nm = category.name.as_deref().unwrap_or("Unknown");
              let fmt_nm = &format.name;
              info!("{} Runner buffered player {} to front of queue", crate::log::log_prefix_format(&gld_nm, ctg_nm, fmt_nm), player_tag);
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
      for category in &mut server.categories {
        for format in &mut category.formats {
          for session in &mut format.sessions {
            if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == user_id) {
              let player_tag = session.pool[pos].player.tag.clone();
              let player = session.pool.remove(pos);
              session.pool.push(player);
              found = true;
              let gld_nm = guild_name(ctx, guild_id);
              let ctg_nm = category.name.as_deref().unwrap_or("Unknown");
              let fmt_nm = &format.name;
              info!("{} Runner fatkidded player {} to end of queue", crate::log::log_prefix_format(&gld_nm, ctg_nm, fmt_nm), player_tag);
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

  // Update dashboard if action was successful
  let success = result.is_ok();
  drop(mgr);

  if success {
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

pub async fn handle_remove_all(
  ctx: &Context,
  interaction: &CI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !check_component_role(&cc, &Role::Runner).await? {
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

  let gld_nm = guild_name(ctx, guild_id);
  info!("{} Runner removed all players from queue ({} players)", log_prefix_guild(&gld_nm), removed_count);

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
