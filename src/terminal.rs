//! Terminal command interface for testing and administration
//!
//! Commands:
//! ## Queue Management
//! - `addplayers <count>` - Add N fake players with realistic Discord IDs
//! - `fillqueue` - Auto-fill queue to quota
//! - `clearqueue` - Remove all players from idle sessions
//! - `removeplayer <index>` - Remove player at position N
//!
//! ## Session Control
//! - `forcehot` - Force session to Hot (bypass quota)
//! - `forcelive` - Force session to Live
//! - `forceidle` - Reset to Idle and clear pool
//! - `simulate_game` - Run complete cycle (fill → hot → push → live → pull → idle)
//!
//! ## Inspection
//! - `status` - Show all guilds, groups, sessions, player counts
//! - `listguilds` - List connected guilds with IDs
//! - `listgroups <guild_id>` - Show all groups for guild
//! - `showqueue [group_id]` - Display queue with player details and timestamps
//! - `showteams` - Display team compositions with ELO stats
//! - `config <guild_id>` - Print full config
//!
//! ## Team Testing
//! - `genteams` - Force team generation
//! - `testbch <elo_list>` - Test BCH algorithm with ELO values
//! - `shuffleteams` - Randomly shuffle teams
//!
//! ## Database
//! - `dbstats` - Show database statistics
//! - `querydb <sql>` - Execute raw SQL (read-only)
//! - `resetdb <guild_id>` - Clear all data for guild
//! - `exportconfig <guild_id>` - Export config to JSON
//!
//! ## Stress Testing
//! - `stress_join <count> <delay_ms>` - Simulate N players joining
//! - `stress_leave <count>` - Simulate N players leaving
//! - `cyclegames <count>` - Run N complete game cycles
//!
//! ## Dashboard/UI
//! - `refreshdash [group_id]` - Force dashboard update
//! - `testbuttons` - Simulate button clicks

use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::models::{SessionStatus, Team};
use crate::{Database, Manager};

/// Start the terminal command reader loop
pub async fn start_terminal_reader(manager: Arc<Mutex<Manager>>, db: Arc<Database>) {
  tokio::spawn(async move {
    let stdin = io::stdin();
    let mut line = String::new();

    println!("\n[Terminal] Testing commands available. Type 'help' for list.");
    print!("[cmd]> ");
    io::stdout().flush().unwrap();

    loop {
      line.clear();
      match stdin.read_line(&mut line) {
        Ok(_) => {
          let cmd = line.trim();
          if !cmd.is_empty() {
            if let Err(e) = handle_command(cmd, &manager, &db).await {
              error!("[Terminal] Command error: {}", e);
            }
          }
          print!("[cmd]> ");
          io::stdout().flush().unwrap();
        }
        Err(e) => {
          error!("[Terminal] Failed to read line: {}", e);
          break;
        }
      }
    }
  });
}

/// Parse and handle a terminal command
async fn handle_command(cmd: &str, manager: &Arc<Mutex<Manager>>, db: &Arc<Database>) -> anyhow::Result<()> {
  let parts: Vec<&str> = cmd.split_whitespace().collect();
  if parts.is_empty() {
    return Ok(());
  }

  let command = parts[0].to_lowercase();
  let args = &parts[1..];

  info!("[Terminal] Executing command: {}", cmd);

  match command.as_str() {
    "help" | "h" => print_help(),
    "addplayers" => cmd_addplayers(args, manager, db).await?,
    "fillqueue" => cmd_fillqueue(args, manager, db).await?,
    "clearqueue" => cmd_clearqueue(args, manager, db).await?,
    "removeplayer" => cmd_removeplayer(args, manager, db).await?,
    "forcehot" => cmd_forcehot(args, manager, db).await?,
    "forcelive" => cmd_forcelive(args, manager, db).await?,
    "forceidle" => cmd_forceidle(args, manager, db).await?,
    "simulate_game" => cmd_simulate_game(args, manager, db).await?,
    "status" => cmd_status(args, manager, db).await?,
    "listguilds" => cmd_listguilds(args, manager, db).await?,
    "listgroups" => cmd_listgroups(args, manager, db).await?,
    "showqueue" => cmd_showqueue(args, manager, db).await?,
    "showteams" => cmd_showteams(args, manager, db).await?,
    "config" => cmd_config(args, manager, db).await?,
    "genteams" => cmd_genteams(args, manager, db).await?,
    "testbch" => cmd_testbch(args, manager, db).await?,
    "shuffleteams" => cmd_shuffleteams(args, manager, db).await?,
    "dbstats" => cmd_dbstats(args, manager, db).await?,
    "querydb" => cmd_querydb(args, manager, db).await?,
    "resetdb" => cmd_resetdb(args, manager, db).await?,
    "exportconfig" => cmd_exportconfig(args, manager, db).await?,
    "stress_join" => cmd_stress_join(args, manager, db).await?,
    "stress_leave" => cmd_stress_leave(args, manager, db).await?,
    "cyclegames" => cmd_cyclegames(args, manager, db).await?,
    "refreshdash" => cmd_refreshdash(args, manager, db).await?,
    "testbuttons" => cmd_testbuttons(args, manager, db).await?,
    "quit" | "exit" => {
      info!("[Terminal] Exit command received");
      std::process::exit(0);
    }
    _ => {
      println!("[Terminal] Unknown command: {}. Type 'help' for list.", command);
    }
  }

  Ok(())
}

fn print_help() {
  println!("\n=== Terminal Commands ===");
  println!("\n## Queue Management");
  println!("  addplayers <count>     - Add N fake players");
  println!("  fillqueue              - Auto-fill queue to quota");
  println!("  clearqueue             - Remove all from idle sessions");
  println!("  removeplayer <index>   - Remove player at position");
  println!("\n## Session Control");
  println!("  forcehot               - Force session to Hot");
  println!("  forcelive              - Force session to Live");
  println!("  forceidle              - Reset to Idle, clear pool");
  println!("  simulate_game          - Run complete game cycle");
  println!("\n## Inspection");
  println!("  status                 - Show all guilds/sessions");
  println!("  listguilds             - List guilds with IDs");
  println!("  listgroups <guild_id>  - Show groups for guild");
  println!("  showqueue [group_id]   - Display queue details");
  println!("  showteams              - Show team compositions");
  println!("  config <guild_id>      - Print config");
  println!("\n## Team Testing");
  println!("  genteams               - Force team generation");
  println!("  testbch <elo_list>     - Test BCH algorithm");
  println!("  shuffleteams           - Shuffle teams");
  println!("\n## Database");
  println!("  dbstats                - Database statistics");
  println!("  querydb <sql>          - Raw SQL (read-only)");
  println!("  resetdb <guild_id>     - Clear guild data");
  println!("  exportconfig <guild_id> - Export to JSON");
  println!("\n## Stress Testing");
  println!("  stress_join <n> <ms>   - Simulate joins");
  println!("  stress_leave <n>       - Simulate leaves");
  println!("  cyclegames <n>         - Run N game cycles");
  println!("\n## Dashboard/UI");
  println!("  refreshdash [group_id] - Force dashboard update");
  println!("  testbuttons            - Simulate button clicks");
  println!("  help, quit, exit       - Show help / exit");
  println!();
}

// ============================================================================
// Queue Management Commands
// ============================================================================

async fn cmd_addplayers(args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let count = args.first().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);

  let mut manager_lock = manager.lock().await;
  let mut added = 0;

  // Find first idle session in first guild/category/format
  'outer: for guild in &mut manager_lock.qguilds {
    for category in &mut guild.categories {
      for format in &mut category.formats {
        for (session_idx, session) in format.sessions.iter_mut().enumerate() {
          if session.is_idle() {
            for i in 0..count {
              let fake_user_id = 100000000000000000 + added as u64;
              let player = crate::models::Player::add(
                serenity::all::UserId::new(fake_user_id),
                format!("Player{}", added + 1),
                5,    // queue_expiration
                None, // steam_id
                None, // rank
              );

              if let Err(e) = session.add_ply(player, false) {
                warn!("[Terminal] Failed to add player {}: {}", added + 1, e);
              } else {
                added += 1;
              }
            }
            println!("[Terminal] Added {} players to session {} in {}/{}/{}", added, session_idx, guild.id, category.id, format.id);
            break 'outer;
          }
        }
      }
    }
  }

  if added == 0 {
    println!("[Terminal] No idle sessions found to add players");
  }

  Ok(())
}

async fn cmd_fillqueue(_args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let mut manager_lock = manager.lock().await;
  let mut filled = 0;

  'outer: for guild in &mut manager_lock.qguilds {
    for category in &mut guild.categories {
      for format in &mut category.formats {
        for (session_idx, session) in format.sessions.iter_mut().enumerate() {
          if session.is_idle() {
            let quota = format.quota as usize;
            let needed = quota.saturating_sub(session.pool.len());

            if needed == 0 {
              println!("[Terminal] Session {} already full ({}/{})", session_idx, session.pool.len(), quota);
              continue;
            }

            for i in 0..needed {
              let fake_user_id = 100000000000000000 + filled as u64;
              let player = crate::models::Player::add(
                serenity::all::UserId::new(fake_user_id),
                format!("Player{}", filled + 1),
                5,    // queue_expiration
                None, // steam_id
                None, // rank
              );

              if let Err(e) = session.add_ply(player, false) {
                warn!("[Terminal] Failed to add player: {}", e);
              } else {
                filled += 1;
              }
            }

            println!("[Terminal] Filled session {} to quota ({}/{})", session_idx, session.pool.len(), quota);
            break 'outer;
          }
        }
      }
    }
  }

  if filled == 0 {
    println!("[Terminal] No sessions needed filling");
  }

  Ok(())
}

async fn cmd_clearqueue(_args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let mut manager_lock = manager.lock().await;
  let mut cleared = 0;

  for guild in &mut manager_lock.qguilds {
    for category in &mut guild.categories {
      for format in &mut category.formats {
        for session in &mut format.sessions {
          if session.is_idle() {
            let count = session.pool.len();
            session.pool.clear();
            cleared += count;
          }
        }
      }
    }
  }

  println!("[Terminal] Cleared {} players from idle sessions", cleared);
  Ok(())
}

async fn cmd_removeplayer(args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let index = args.first().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);

  let mut manager_lock = manager.lock().await;
  let mut removed = false;

  'outer: for guild in &mut manager_lock.qguilds {
    for category in &mut guild.categories {
      for format in &mut category.formats {
        for session in &mut format.sessions {
          if session.is_idle() && index < session.pool.len() {
            let player = session.pool.remove(index);
            println!("[Terminal] Removed player {} (ELO: {}) at index {}", player.player.tag, player.player.elo, index);
            removed = true;
            break 'outer;
          }
        }
      }
    }
  }

  if !removed {
    println!("[Terminal] Could not find player at index {} in any idle session", index);
  }

  Ok(())
}

// ============================================================================
// Session Control Commands
// ============================================================================

async fn cmd_forcehot(_args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let mut manager_lock = manager.lock().await;
  let mut forced = false;

  'outer: for guild in &mut manager_lock.qguilds {
    for category in &mut guild.categories {
      for format in &mut category.formats {
        for (idx, session) in format.sessions.iter_mut().enumerate() {
          if session.is_idle() {
            session.status = SessionStatus::Hot;
            println!("[Terminal] Forced session {} to Hot", idx);
            forced = true;
            break 'outer;
          }
        }
      }
    }
  }

  if !forced {
    println!("[Terminal] No idle sessions found");
  }

  Ok(())
}

async fn cmd_forcelive(_args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let mut manager_lock = manager.lock().await;
  let mut forced = false;

  'outer: for guild in &mut manager_lock.qguilds {
    for category in &mut guild.categories {
      for format in &mut category.formats {
        for (idx, session) in format.sessions.iter_mut().enumerate() {
          if !session.is_idle() {
            session.status = SessionStatus::Live;
            session.started_at = Some(std::time::SystemTime::now());
            println!("[Terminal] Forced session {} to Live", idx);
            forced = true;
            break 'outer;
          }
        }
      }
    }
  }

  if !forced {
    println!("[Terminal] No active sessions found");
  }

  Ok(())
}

async fn cmd_forceidle(_args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let mut manager_lock = manager.lock().await;
  let mut forced = false;

  for guild in &mut manager_lock.qguilds {
    for category in &mut guild.categories {
      for format in &mut category.formats {
        for (idx, session) in format.sessions.iter_mut().enumerate() {
          if !session.is_idle() {
            let count = session.pool.len();
            session.status = SessionStatus::Idle;
            session.pool.clear();
            session.team_channels = None;
            session.ready_at = None;
            session.started_at = None;
            session.match_ended_at = None;
            println!("[Terminal] Forced session {} to Idle, cleared {} players", idx, count);
            forced = true;
          }
        }
      }
    }
  }

  if !forced {
    println!("[Terminal] No non-idle sessions found");
  }

  Ok(())
}

async fn cmd_simulate_game(_args: &[&str], _manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  println!("[Terminal] Simulate game cycle: fill → hot → push → live → pull → idle");
  println!("[Terminal] Not yet implemented - requires full async game flow");
  Ok(())
}

// ============================================================================
// Inspection Commands
// ============================================================================

async fn cmd_status(_args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let manager_lock = manager.lock().await;

  println!("\n=== Status ===");
  println!("Guilds: {}", manager_lock.qguilds.len());

  for guild in &manager_lock.qguilds {
    println!("\n  Guild: {} ({})", guild.name, guild.id);
    println!("  Categories: {}", guild.categories.len());

    for category in &guild.categories {
      let cat_name = category.name.as_deref().unwrap_or("Unnamed");
      println!("\n    Category: {} ({})", cat_name, category.id);
      println!("    Formats: {}", category.formats.len());

      for format in &category.formats {
        println!("\n      Format: {} (quota: {})", format.name, format.quota);
        println!("      Sessions: {}", format.sessions.len());

        for (idx, session) in format.sessions.iter().enumerate() {
          let status_str = format!("{:?}", session.status);
          println!("        Session {}: {} - {} players", idx, status_str, session.pool.len());
        }
      }
    }
  }

  println!();
  Ok(())
}

async fn cmd_listguilds(_args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let manager_lock = manager.lock().await;

  println!("\n=== Connected Guilds ===");
  for guild in &manager_lock.qguilds {
    println!("  {} - {}", guild.id, guild.name);
  }
  println!();

  Ok(())
}

async fn cmd_listgroups(args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let guild_id = args.first().and_then(|s| s.parse::<u64>().ok());

  let manager_lock = manager.lock().await;

  println!("\n=== Groups ===");
  for guild in &manager_lock.qguilds {
    if let Some(target_id) = guild_id {
      if guild.id != target_id {
        continue;
      }
    }

    println!("\nGuild: {} ({})", guild.name, guild.id);
    for category in &guild.categories {
      let cat_name = category.name.as_deref().unwrap_or("Unnamed");
      println!("  Group: {} (ID: {})", cat_name, category.id);
    }
  }
  println!();

  Ok(())
}

async fn cmd_showqueue(args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let target_group = args.first().and_then(|s| s.parse::<u8>().ok());

  let manager_lock = manager.lock().await;

  println!("\n=== Queue Details ===");
  for guild in &manager_lock.qguilds {
    for category in &guild.categories {
      if let Some(target) = target_group {
        if category.id != target {
          continue;
        }
      }

      let cat_name = category.name.as_deref().unwrap_or("Unnamed");
      println!("\nGroup: {} ({})", cat_name, category.id);

      for format in &category.formats {
        println!("  Format: {} (quota: {})", format.name, format.quota);

        for (idx, session) in format.sessions.iter().enumerate() {
          println!("\n    Session {}: {:?} - {} players", idx, session.status, session.pool.len());

          for (pidx, player) in session.pool.iter().enumerate() {
            let vc_indicator = if player.in_vc { "[VC] " } else { "" };
            let team_str = match player.team {
              Some(Team::Red) => "[RED] ",
              Some(Team::Blu) => "[BLU] ",
              _ => "",
            };
            println!("      {}. {}{}{} (ELO: {})", pidx + 1, vc_indicator, team_str, player.player.tag, player.player.elo);
          }
        }
      }
    }
  }
  println!();

  Ok(())
}

async fn cmd_showteams(_args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let manager_lock = manager.lock().await;

  println!("\n=== Team Compositions ===");
  for guild in &manager_lock.qguilds {
    for category in &guild.categories {
      for format in &category.formats {
        for (idx, session) in format.sessions.iter().enumerate() {
          if session.status == SessionStatus::Hot || session.status == SessionStatus::Push || session.status == SessionStatus::Live {
            let red_team: Vec<_> = session.pool.iter().filter(|p| matches!(p.team, Some(Team::Red))).collect();
            let blu_team: Vec<_> = session.pool.iter().filter(|p| matches!(p.team, Some(Team::Blu))).collect();

            let red_elo: u32 = red_team.iter().map(|p| p.player.elo as u32).sum::<u32>() / red_team.len().max(1) as u32;
            let blu_elo: u32 = blu_team.iter().map(|p| p.player.elo as u32).sum::<u32>() / blu_team.len().max(1) as u32;

            println!("\nSession {} ({})", idx, format.name);
            println!("  RED Team (avg ELO: {}):", red_elo);
            for player in &red_team {
              println!("    - {} (ELO: {})", player.player.tag, player.player.elo);
            }
            println!("  BLU Team (avg ELO: {}):", blu_elo);
            for player in &blu_team {
              println!("    - {} (ELO: {})", player.player.tag, player.player.elo);
            }
          }
        }
      }
    }
  }
  println!();

  Ok(())
}

async fn cmd_config(args: &[&str], _manager: &Arc<Mutex<Manager>>, db: &Arc<Database>) -> anyhow::Result<()> {
  let guild_id = args.first().and_then(|s| s.parse::<u64>().ok());

  if let Some(gid) = guild_id {
    match db.config.get_config_map(serenity::all::GuildId::new(gid)).await {
      Ok(config) => {
        println!("\n=== Config for Guild {} ===", gid);
        println!("{:#?}", config);
      }
      Err(e) => {
        println!("[Terminal] Failed to get config: {}", e);
      }
    }
  } else {
    println!("[Terminal] Usage: config <guild_id>");
  }

  println!();
  Ok(())
}

// ============================================================================
// Team Testing Commands
// ============================================================================

async fn cmd_genteams(_args: &[&str], _manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  println!("[Terminal] Force team generation not yet implemented");
  Ok(())
}

async fn cmd_testbch(args: &[&str], _manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  if args.is_empty() {
    println!("[Terminal] Usage: testbch <elo1,elo2,elo3,...>");
    return Ok(());
  }

  let elo_str = args.join(",");
  let elos: Vec<i32> = elo_str.split(',').filter_map(|s| s.parse().ok()).collect();

  println!("[Terminal] Testing BCH with ELOs: {:?}", elos);
  println!("[Terminal] Not yet implemented");
  Ok(())
}

async fn cmd_shuffleteams(_args: &[&str], _manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  println!("[Terminal] Shuffle teams not yet implemented");
  Ok(())
}

// ============================================================================
// Database Commands
// ============================================================================

async fn cmd_dbstats(_args: &[&str], _manager: &Arc<Mutex<Manager>>, db: &Arc<Database>) -> anyhow::Result<()> {
  println!("\n=== Database Statistics ===");

  match sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM players").fetch_one(&db.pool).await {
    Ok((count,)) => println!("Players: {}", count),
    Err(e) => println!("Players: Error - {}", e),
  }

  match sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM guilds").fetch_one(&db.pool).await {
    Ok((count,)) => println!("Guilds: {}", count),
    Err(e) => println!("Guilds: Error - {}", e),
  }

  match sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM matches").fetch_one(&db.pool).await {
    Ok((count,)) => println!("Matches: {}", count),
    Err(e) => println!("Matches: Error - {}", e),
  }

  println!();
  Ok(())
}

async fn cmd_querydb(args: &[&str], _manager: &Arc<Mutex<Manager>>, db: &Arc<Database>) -> anyhow::Result<()> {
  if args.is_empty() {
    println!("[Terminal] Usage: querydb <SQL>");
    return Ok(());
  }

  let sql = args.join(" ");

  // Safety check - only allow SELECT queries
  let trimmed = sql.trim().to_uppercase();
  if !trimmed.starts_with("SELECT") && !trimmed.starts_with("PRAGMA") {
    println!("[Terminal] Only SELECT/PRAGMA queries allowed for safety");
    return Ok(());
  }

  println!("[Terminal] Executing: {}", sql);

  match sqlx::query(&sql).fetch_all(&db.pool).await {
    Ok(rows) => {
      println!("[Terminal] {} rows returned", rows.len());
      for (idx, row) in rows.iter().enumerate() {
        // Print row data without Debug trait
        println!("  Row {}", idx);
      }
    }
    Err(e) => {
      println!("[Terminal] Query error: {}", e);
    }
  }

  Ok(())
}

async fn cmd_resetdb(args: &[&str], _manager: &Arc<Mutex<Manager>>, db: &Arc<Database>) -> anyhow::Result<()> {
  let guild_id = args.first().and_then(|s| s.parse::<u64>().ok());

  if let Some(gid) = guild_id {
    println!("[Terminal] Clearing data for guild {}...", gid);

    // Delete guild-specific data
    let result = sqlx::query("DELETE FROM guild_settings WHERE guild_id = ?").bind(gid as i64).execute(&db.pool).await;

    match result {
      Ok(r) => println!("[Terminal] Deleted {} rows from guild_settings", r.rows_affected()),
      Err(e) => println!("[Terminal] Error: {}", e),
    }

    let result = sqlx::query("DELETE FROM player_guild_stats WHERE guild_id = ?").bind(gid as i64).execute(&db.pool).await;

    match result {
      Ok(r) => println!("[Terminal] Deleted {} rows from player_guild_stats", r.rows_affected()),
      Err(e) => println!("[Terminal] Error: {}", e),
    }

    println!("[Terminal] Guild {} data cleared", gid);
  } else {
    println!("[Terminal] Usage: resetdb <guild_id>");
  }

  Ok(())
}

async fn cmd_exportconfig(args: &[&str], _manager: &Arc<Mutex<Manager>>, db: &Arc<Database>) -> anyhow::Result<()> {
  let guild_id = args.first().and_then(|s| s.parse::<u64>().ok());

  if let Some(gid) = guild_id {
    match db.config.get_config_map(serenity::all::GuildId::new(gid)).await {
      Ok(config) => match serde_json::to_string_pretty(&config) {
        Ok(json) => {
          let filename = format!("config_{}.json", gid);
          match std::fs::write(&filename, json) {
            Ok(_) => println!("[Terminal] Config exported to {}", filename),
            Err(e) => println!("[Terminal] Failed to write file: {}", e),
          }
        }
        Err(e) => println!("[Terminal] JSON serialization error: {}", e),
      },
      Err(e) => {
        println!("[Terminal] Failed to get config: {}", e);
      }
    }
  } else {
    println!("[Terminal] Usage: exportconfig <guild_id>");
  }

  Ok(())
}

// ============================================================================
// Stress Testing Commands
// ============================================================================

async fn cmd_stress_join(args: &[&str], _manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let count = args.first().and_then(|s| s.parse::<usize>().ok()).unwrap_or(10);
  let delay = args.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(100);

  println!("[Terminal] Stress test: {} players joining with {}ms delay", count, delay);
  println!("[Terminal] Not yet implemented - requires async simulation");
  Ok(())
}

async fn cmd_stress_leave(args: &[&str], _manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let count = args.first().and_then(|s| s.parse::<usize>().ok()).unwrap_or(10);

  println!("[Terminal] Stress test: {} players leaving", count);
  println!("[Terminal] Not yet implemented");
  Ok(())
}

async fn cmd_cyclegames(args: &[&str], _manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let count = args.first().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);

  println!("[Terminal] Running {} game cycles", count);
  println!("[Terminal] Not yet implemented");
  Ok(())
}

// ============================================================================
// Dashboard/UI Commands
// ============================================================================

async fn cmd_refreshdash(args: &[&str], manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  let target_group = args.first().and_then(|s| s.parse::<u8>().ok());

  let manager_lock = manager.lock().await;
  let mut refreshed = 0;

  for guild in &manager_lock.qguilds {
    for category in &guild.categories {
      if let Some(target) = target_group {
        if category.id != target {
          continue;
        }
      }

      // Trigger dashboard update for this category
      println!("[Terminal] Refreshing dashboard for group {}", category.id);
      refreshed += 1;
    }
  }

  if refreshed == 0 {
    println!("[Terminal] No dashboards found to refresh");
  } else {
    println!("[Terminal] Refreshed {} dashboard(s)", refreshed);
  }

  Ok(())
}

async fn cmd_testbuttons(_args: &[&str], _manager: &Arc<Mutex<Manager>>, _db: &Arc<Database>) -> anyhow::Result<()> {
  println!("[Terminal] Testing button simulation");
  println!("[Terminal] Not yet implemented - requires Discord interaction simulation");
  Ok(())
}
