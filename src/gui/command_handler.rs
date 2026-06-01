//! Handler for GUI commands

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serenity::all::{GuildId as GI, UserId as UI};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::gui::commands::GuiCommand;
use crate::models::{Player, Session, SessionPlayer, SessionStatus, Team};
use crate::{Database, Manager};

// ── Navigation helpers ────────────────────────────────────────────────────────

fn find_format(manager: &mut Manager, guild_id: u64, category_id: u8, fmt_id: u8) -> Result<&mut crate::models::Format> {
  let guild = manager.get_qguild(GI::new(guild_id))?;
  let cat = guild.categories.iter_mut().find(|c| c.id == category_id).ok_or_else(|| anyhow!("Category {} not found", category_id))?;
  cat.formats.iter_mut().find(|f| f.id == fmt_id).ok_or_else(|| anyhow!("Format {} not found", fmt_id))
}

fn find_session(manager: &mut Manager, guild_id: u64, category_id: u8, fmt_id: u8, session_index: usize) -> Result<&mut Session> {
  let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
  fmt.sessions.get_mut(session_index).ok_or_else(|| anyhow!("Session index {} out of range", session_index))
}

// ── BCH team assignment (no Discord needed) ───────────────────────────────────
//
// Classic BCH pattern for N players sorted by ELO descending:
//   index (0-based): 0→Red, 1→Blu, 2→Blu, 3→Red, 4→Red, 5→Blu, …
fn bch_assign_teams(players: &mut [SessionPlayer]) {
  // Sort descending by ELO
  players.sort_by(|a, b| b.player.elo.cmp(&a.player.elo));
  for (i, sp) in players.iter_mut().enumerate() {
    sp.team = Some(match (i / 2) % 2 == 0 {
      true => {
        if i % 2 == 0 {
          Team::Red
        } else {
          Team::Blu
        }
      }
      false => {
        if i % 2 == 0 {
          Team::Blu
        } else {
          Team::Red
        }
      }
    });
  }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Handle a command from the GUI.
/// Returns the affected guild ID for state-mutating commands, or None for read-only commands.
pub async fn handle_command(
  command: GuiCommand,
  manager: &mut Manager,
  db: &Database,
  user_search_results: Arc<RwLock<Vec<Player>>>,
  user_guild_data: Arc<RwLock<Vec<(u64, crate::db::repo::GuildElo)>>>,
  guild_config_cache: Arc<RwLock<std::collections::HashMap<u64, std::collections::HashMap<String, String>>>>,
  ctx: Option<Arc<serenity::all::Context>>,
) -> Result<Option<u64>> {
  let affected_guild = command.guild_id();
  let result: Result<Option<u64>, anyhow::Error> = match command {
    // ── Snapshot ──────────────────────────────────────────────────────────
    GuiCommand::RefreshSnapshot => {
      info!("[GUI] RefreshSnapshot — snapshot updated by periodic task");
      Ok(None)
    }

    // ── Queue Management ──────────────────────────────────────────────────
    GuiCommand::ClearQueue { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      let mut cleared = 0usize;
      for session in fmt.sessions.iter_mut().filter(|s| s.is_idle()) {
        cleared += session.pool.len();
        session.pool.clear();
      }
      info!("[GUI] ClearQueue g={} c={} f={} — removed {} players", guild_id, category_id, fmt_id, cleared);
      Ok(None)
    }

    GuiCommand::RemovePlayer { guild_id, category_id, fmt_id, user_id } => {
      let uid = UI::new(user_id);
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in &mut fmt.sessions {
        if session.pool.iter().any(|p| p.player.user_id == uid) {
          session.remove_player(uid);
          info!("[GUI] RemovePlayer {} from g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
          return Ok(None);
        }
      }
      warn!("[GUI] RemovePlayer {} not found in g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
      Ok(None)
    }

    GuiCommand::DeletePlayerFromDb { guild_id, user_id } => {
      let uid = UI::new(user_id);
      let guild = manager.get_qguild(GI::new(guild_id))?;

      // Remove player from all queues in the guild first
      let mut removed_count = 0;
      for cat in &mut guild.categories {
        for fmt in &mut cat.formats {
          for session in &mut fmt.sessions {
            if session.pool.iter().any(|p| p.player.user_id == uid) {
              session.remove_player(uid);
              removed_count += 1;
            }
          }
        }
      }

      // Delete guild-specific ELO record
      match db.elo.delete_for_guild(uid, GI::new(guild_id)).await {
        Ok(_) => {
          info!("[GUI] DeletePlayerFromDb {} g={} — removed from {} queue(s) and deleted guild ELO record", user_id, guild_id, removed_count);
        }
        Err(e) => {
          warn!("[GUI] DeletePlayerFromDb {} g={} — removed from {} queue(s) but failed to delete guild ELO record: {}", user_id, guild_id, removed_count, e);
        }
      }
      Ok(None)
    }

    GuiCommand::BufferPlayer { guild_id, category_id, fmt_id, user_id } => {
      let uid = UI::new(user_id);
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in fmt.sessions.iter_mut().filter(|s| s.is_idle()) {
        if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == uid) {
          let sp = session.pool.remove(pos);
          session.pool.insert(0, sp);
          info!("[GUI] BufferPlayer {} → front g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
          return Ok(None);
        }
      }
      warn!("[GUI] BufferPlayer {} not found in idle sessions", user_id);
      Ok(None)
    }

    GuiCommand::FatkidPlayer { guild_id, category_id, fmt_id, user_id } => {
      let uid = UI::new(user_id);
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in fmt.sessions.iter_mut().filter(|s| s.is_idle()) {
        if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == uid) {
          let sp = session.pool.remove(pos);
          session.pool.push(sp);
          info!("[GUI] FatkidPlayer {} → end g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
          return Ok(None);
        }
      }
      warn!("[GUI] FatkidPlayer {} not found in idle sessions", user_id);
      Ok(None)
    }

    GuiCommand::ReorderQueue { guild_id, category_id, fmt_id, user_id, new_position } => {
      let uid = UI::new(user_id);
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in fmt.sessions.iter_mut().filter(|s| s.is_idle()) {
        if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == uid) {
          let sp = session.pool.remove(pos);
          let insert_at = new_position.min(session.pool.len());
          session.pool.insert(insert_at, sp);
          info!("[GUI] ReorderQueue {} → pos {} g={} c={} f={}", user_id, insert_at, guild_id, category_id, fmt_id);
          return Ok(None);
        }
      }
      warn!("[GUI] ReorderQueue {} not found in idle sessions", user_id);
      Ok(None)
    }

    GuiCommand::MovePlayerBetweenSessions { guild_id, category_id, fmt_id, user_id, from_session, to_session } => {
      let uid = UI::new(user_id);
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      if from_session == to_session {
        return Err(anyhow!("from_session == to_session"));
      }
      let sp = {
        let src = fmt.sessions.get_mut(from_session).ok_or_else(|| anyhow!("from_session {} out of range", from_session))?;
        let pos = src.pool.iter().position(|p| p.player.user_id == uid).ok_or_else(|| anyhow!("Player {} not in session {}", user_id, from_session))?;
        src.pool.remove(pos)
      };
      let dst = fmt.sessions.get_mut(to_session).ok_or_else(|| anyhow!("to_session {} out of range", to_session))?;
      dst.pool.push(sp);
      info!("[GUI] MovePlayer {} from session {} to {}", user_id, from_session, to_session);
      Ok(None)
    }

    GuiCommand::ForceSessionState { guild_id, category_id, fmt_id, session_index, new_state } => {
      let session = find_session(manager, guild_id, category_id, fmt_id, session_index)?;
      match new_state {
        SessionStatus::Idle => session.idle(),
        SessionStatus::Hot => {
          session.hot();
        }
        SessionStatus::Push => session.push(),
        SessionStatus::Live => session.live(),
        SessionStatus::Pull => session.pull(),
      }
      info!("[GUI] ForceSessionState session {} → {:?}", session_index, new_state);
      Ok(None)
    }

    GuiCommand::ResetSessionTimer { guild_id, category_id, fmt_id, session_index } => {
      let session = find_session(manager, guild_id, category_id, fmt_id, session_index)?;
      session.ready_at = None;
      info!("[GUI] ResetSessionTimer session {}", session_index);
      Ok(None)
    }

    GuiCommand::ForceTeamRegeneration { guild_id, category_id, fmt_id, session_index } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      let quota = fmt.quota as usize;
      let session = fmt.sessions.get_mut(session_index).ok_or_else(|| anyhow!("Session {} not found", session_index))?;
      let pool_len = session.pool.len().min(quota);
      bch_assign_teams(&mut session.pool[..pool_len]);
      info!("[GUI] ForceTeamRegeneration session {} — {} players assigned", session_index, pool_len);
      Ok(None)
    }

    GuiCommand::SwapTeams { guild_id, category_id, fmt_id, session_index } => {
      let session = find_session(manager, guild_id, category_id, fmt_id, session_index)?;
      for sp in &mut session.pool {
        sp.team = match sp.team {
          Some(Team::Red) => Some(Team::Blu),
          Some(Team::Blu) => Some(Team::Red),
          other => other,
        };
      }
      info!("[GUI] SwapTeams session {}", session_index);
      Ok(None)
    }

    GuiCommand::ForceEndGame { guild_id, category_id, fmt_id, session_index } => {
      let session = find_session(manager, guild_id, category_id, fmt_id, session_index)?;
      let players = session.pool.len();
      session.pool.clear();
      session.idle();
      info!("[GUI] ForceEndGame session {} — cleared {} players, reset to Idle", session_index, players);
      Ok(None)
    }

    GuiCommand::AddPlayer { guild_id, category_id, fmt_id, user_id } => {
      let uid = UI::new(user_id);
      // Try to load from DB (no Discord context available); fall back to placeholder
      let player = match db.players.check_user(uid, None).await {
        Ok(p) => p,
        Err(_) => Player::add(uid, format!("User#{}", user_id), crate::DEFAULT_QUEUE_EXPIRATION, None, None),
      };
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      let session = fmt.sessions.iter_mut().find(|s| s.is_idle()).ok_or_else(|| anyhow!("No idle session to add player to"))?;
      if session.pool.iter().any(|p| p.player.user_id == uid) {
        warn!("[GUI] AddPlayer {} already in session", user_id);
        return Ok(None);
      }
      session.add_ply(player, false)?;
      info!("[GUI] AddPlayer {} added to g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
      Ok(None)
    }

    GuiCommand::ForceQuotaMet { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      if let Some(session) = fmt.sessions.iter_mut().find(|s| s.is_idle()) {
        session.hot();
        info!("[GUI] ForceQuotaMet — idle session forced to Hot");
      } else {
        warn!("[GUI] ForceQuotaMet — no idle session found");
      }
      Ok(None)
    }

    // ── Testing ───────────────────────────────────────────────────────────
    GuiCommand::AddDummyPlayers { guild_id, category_id, fmt_id, count, role_id: _ } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      let session = fmt.sessions.iter_mut().find(|s| s.is_idle()).ok_or_else(|| anyhow!("No idle session to add dummy players to"))?;
      for i in 0..count {
        let fake_uid = UI::new(900_000_000_000_000_000 + i as u64);
        let elo: u16 = 50 + (i as u16 * 7) % 150;
        let player = Player::add(fake_uid, format!("Dummy{}", i + 1), crate::DEFAULT_QUEUE_EXPIRATION, None, None);
        let mut sp = SessionPlayer::add(player);
        sp.player.elo = elo;
        session.pool.push(sp);
      }
      info!("[GUI] AddDummyPlayers — added {} to g={} c={} f={}", count, guild_id, category_id, fmt_id);
      Ok(None)
    }

    GuiCommand::SimulateGameFlow { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      let quota = fmt.quota as usize;
      let session = fmt.sessions.iter_mut().find(|s| s.is_idle()).ok_or_else(|| anyhow!("No idle session for SimulateGameFlow"))?;
      // Fill to quota with dummies if needed
      let need = quota.saturating_sub(session.pool.len());
      for i in 0..need {
        let fake_uid = UI::new(910_000_000_000_000_000 + i as u64);
        let player = Player::add(fake_uid, format!("Sim{}", i + 1), crate::DEFAULT_QUEUE_EXPIRATION, None, None);
        session.pool.push(SessionPlayer::add(player));
      }
      // Hot → assign teams → Live
      session.hot();
      let pool_len = session.pool.len().min(quota);
      bch_assign_teams(&mut session.pool[..pool_len]);
      session.live();
      info!("[GUI] SimulateGameFlow — session is now Live with {} players", session.pool.len());
      Ok(None)
    }

    GuiCommand::SimulateVCTimeout { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in fmt.sessions.iter_mut().filter(|s| s.is_hot()) {
        session.pool.retain(|p| p.in_vc);
        info!("[GUI] SimulateVCTimeout — removed non-VC players from hot session");
      }
      Ok(None)
    }

    GuiCommand::TriggerConcurrentGames { guild_id, category_id, fmt_id, count } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      let quota = fmt.quota as usize;
      for game_i in 0..count {
        let mut new_session = Session::new(SessionStatus::Idle, Vec::new());
        for p_i in 0..quota {
          let fake_uid = UI::new(920_000_000_000_000_000 + (game_i * quota + p_i) as u64);
          let player = Player::add(fake_uid, format!("Conc{}_{}", game_i, p_i), crate::DEFAULT_QUEUE_EXPIRATION, None, None);
          new_session.pool.push(SessionPlayer::add(player));
        }
        new_session.hot();
        bch_assign_teams(&mut new_session.pool);
        new_session.live();
        fmt.sessions.push(new_session);
      }
      info!("[GUI] TriggerConcurrentGames — created {} live sessions", count);
      Ok(None)
    }

    GuiCommand::TestBalanceMethods { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      let quota = fmt.quota as usize;
      if let Some(session) = fmt.sessions.iter().find(|s| !s.pool.is_empty()) {
        let mut players: Vec<_> = session.pool.iter().take(quota).map(|p| (p.player.tag.clone(), p.player.elo)).collect();
        players.sort_by(|a, b| b.1.cmp(&a.1));
        info!("[GUI] TestBalanceMethods — top {} players by ELO:", players.len());
        for (tag, elo) in &players {
          info!("  {} — {}", tag, elo);
        }
        // BCH split
        let (red, blu): (Vec<_>, Vec<_>) = players.iter().enumerate().partition(|(i, _)| (i / 2) % 2 == 0 && i % 2 == 0 || (i / 2) % 2 != 0 && i % 2 != 0);
        info!("[GUI] Red: {:?}", red.iter().map(|(_, p)| p.0.as_str()).collect::<Vec<_>>());
        info!("[GUI] Blu: {:?}", blu.iter().map(|(_, p)| p.0.as_str()).collect::<Vec<_>>());
      }
      Ok(None)
    }

    // ── Recovery ──────────────────────────────────────────────────────────
    GuiCommand::ResetCategoryState { guild_id, category_id } => {
      let guild = manager.get_qguild(GI::new(guild_id))?;
      let cat = guild.categories.iter_mut().find(|c| c.id == category_id).ok_or_else(|| anyhow!("Category {} not found", category_id))?;
      for fmt in &mut cat.formats {
        fmt.sessions.clear();
        fmt.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
      }
      info!("[GUI] ResetCategoryState g={} c={} — all sessions cleared", guild_id, category_id);
      Ok(None)
    }

    GuiCommand::RemoveOrphanedSessions { guild_id, category_id } => {
      let guild = manager.get_qguild(GI::new(guild_id))?;
      let cat = guild.categories.iter_mut().find(|c| c.id == category_id).ok_or_else(|| anyhow!("Category {} not found", category_id))?;
      let mut removed = 0usize;
      for fmt in &mut cat.formats {
        let before = fmt.sessions.len();
        fmt.sessions.retain(|s| !(s.is_idle() && s.pool.is_empty()));
        removed += before - fmt.sessions.len();
        // Ensure at least one idle session
        if fmt.sessions.is_empty() {
          fmt.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
        }
      }
      info!("[GUI] RemoveOrphanedSessions g={} c={} — removed {}", guild_id, category_id, removed);
      Ok(None)
    }

    GuiCommand::ClearPendingTeamSwitches { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in &mut fmt.sessions {
        session.pending_team_switch = None;
      }
      info!("[GUI] ClearPendingTeamSwitches g={} c={} f={}", guild_id, category_id, fmt_id);
      Ok(None)
    }

    GuiCommand::FixPlayerVCState { guild_id, category_id, user_id } => {
      let uid = UI::new(user_id);
      let guild = manager.get_qguild(GI::new(guild_id))?;
      let cat = guild.categories.iter_mut().find(|c| c.id == category_id).ok_or_else(|| anyhow!("Category {} not found", category_id))?;
      for fmt in &mut cat.formats {
        for session in &mut fmt.sessions {
          if let Some(sp) = session.pool.iter_mut().find(|p| p.player.user_id == uid) {
            sp.vc_off();
            info!("[GUI] FixPlayerVCState {} — in_vc cleared", user_id);
            return Ok(None);
          }
        }
      }
      warn!("[GUI] FixPlayerVCState {} not found", user_id);
      Ok(None)
    }

    GuiCommand::ResetVoiceStateTracking { guild_id } => {
      let guild = manager.get_qguild(GI::new(guild_id))?;
      for cat in &mut guild.categories {
        for fmt in &mut cat.formats {
          for session in &mut fmt.sessions {
            for sp in &mut session.pool {
              sp.vc_off();
            }
          }
        }
      }
      info!("[GUI] ResetVoiceStateTracking g={} — all in_vc cleared", guild_id);
      Ok(None)
    }

    GuiCommand::RecoverFromDatabase { guild_id, category_id } => {
      use crate::db::repo::CategoryRepository;
      let repo = CategoryRepository::new(db.pool().clone());
      let categories = repo.get_categories_for_guild(GI::new(guild_id)).await?;
      if let Some(cat) = categories.into_iter().find(|c| c.id == category_id) {
        let guild = manager.get_qguild(GI::new(guild_id))?;
        if let Some(existing) = guild.categories.iter_mut().find(|c| c.id == category_id) {
          // Only restore formats/sessions from DB; keep runtime state for fields not in DB
          for fmt in &cat.formats {
            if !existing.formats.iter().any(|f| f.id == fmt.id) {
              existing.formats.push(fmt.clone());
            }
          }
          info!("[GUI] RecoverFromDatabase g={} c={} — merged from DB", guild_id, category_id);
        } else {
          guild.add_category(cat)?;
          info!("[GUI] RecoverFromDatabase g={} c={} — added from DB", guild_id, category_id);
        }
      } else {
        warn!("[GUI] RecoverFromDatabase — category {} not found in DB", category_id);
      }
      Ok(None)
    }

    // ── Debugging ─────────────────────────────────────────────────────────
    GuiCommand::DumpStateToLog { guild_id } => {
      match manager.get_qguild(GI::new(guild_id)) {
        Ok(guild) => {
          info!("[GUI] DumpState guild '{}' ({}):", guild.name, guild_id);
          for cat in &guild.categories {
            info!("  category {} '{:?}':", cat.id, cat.name);
            for fmt in &cat.formats {
              info!("    format {} '{}' quota={}:", fmt.id, fmt.name, fmt.quota);
              for (i, s) in fmt.sessions.iter().enumerate() {
                info!("      session {} {:?} players={}:", i, s.status, s.pool.len());
                for sp in &s.pool {
                  info!("        {} elo={} vc={} team={:?}", sp.player.tag, sp.player.elo, sp.in_vc, sp.team);
                }
              }
            }
          }
        }
        Err(e) => warn!("[GUI] DumpState guild {} not found: {}", guild_id, e),
      }
      Ok(None)
    }

    GuiCommand::ViewSessionDetails { guild_id, category_id, fmt_id, session_index } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      match fmt.sessions.get(session_index) {
        Some(s) => {
          info!("[GUI] Session {} — status={:?} players={}:", session_index, s.status, s.pool.len());
          for sp in &s.pool {
            info!("  {} elo={} team={:?} vc={} in_queue={}", sp.player.tag, sp.player.elo, sp.team, sp.in_vc, sp.in_queue);
          }
        }
        None => warn!("[GUI] ViewSessionDetails — session {} not found", session_index),
      }
      Ok(None)
    }

    // ── User Management ────────────────────────────────────────────────
    GuiCommand::QueryUsers { search_term } => {
      match db.players.search(&search_term, 50).await {
        Ok(results) => {
          let count = results.len();
          let mut lock = user_search_results.write().await;
          *lock = results;
          info!("[GUI] QueryUsers '{}' — {} result(s)", search_term, count);
        }
        Err(e) => {
          warn!("[GUI] QueryUsers '{}' failed: {}", search_term, e);
          let mut lock = user_search_results.write().await;
          lock.clear();
        }
      }
      Ok(None)
    }

    GuiCommand::UpdateUserTag { user_id, tag } => {
      let uid = UI::new(user_id);
      match db.players.update_discord_tag(uid, &tag).await {
        Ok(_) => info!("[GUI] UpdateUserTag {} → '{}'", user_id, tag),
        Err(e) => warn!("[GUI] UpdateUserTag {} failed: {}", user_id, e),
      }
      Ok(None)
    }

    GuiCommand::UpdateUserSteamId { user_id, steam_id } => {
      let uid = UI::new(user_id);
      match db.players.update_steam_id(&uid, steam_id).await {
        Ok(_) => info!("[GUI] UpdateUserSteamId {} → {:?}", user_id, steam_id),
        Err(e) => warn!("[GUI] UpdateUserSteamId {} failed: {}", user_id, e),
      }
      Ok(None)
    }

    GuiCommand::UpdateUserQueueExpiration { user_id, queue_expiration } => {
      let uid = UI::new(user_id);
      match db.players.update_prefs_field(uid, "queue_expiration", queue_expiration as i64).await {
        Ok(_) => info!("[GUI] UpdateUserQueueExpiration {} → {}", user_id, queue_expiration),
        Err(e) => warn!("[GUI] UpdateUserQueueExpiration {} failed: {}", user_id, e),
      }
      Ok(None)
    }

    GuiCommand::GetUserGuildData { user_id } => {
      let uid = UI::new(user_id);
      match db.elo.get_all_for_user(uid).await {
        Ok(results) => {
          let count = results.len();
          let mut gd_lock = user_guild_data.write().await;
          *gd_lock = results;
          info!("[GUI] GetUserGuildData {} — {} guild(s)", user_id, count);
        }
        Err(e) => {
          warn!("[GUI] GetUserGuildData {} failed: {}", user_id, e);
          let mut gd_lock = user_guild_data.write().await;
          gd_lock.clear();
        }
      }
      Ok(None)
    }

    GuiCommand::UpdateUserElo { user_id, guild_id, elo } => {
      let uid = UI::new(user_id);
      let gid = GI::new(guild_id);
      match db.elo.update_elo(uid, gid, elo, db).await {
        Ok(_) => info!("[GUI] UpdateUserElo {} g={} → {}", user_id, guild_id, elo),
        Err(e) => warn!("[GUI] UpdateUserElo {} g={} failed: {}", user_id, guild_id, e),
      }
      Ok(None)
    }

    GuiCommand::UpdateUserDynamicElo { user_id, guild_id, dynamic_elo } => {
      let uid = UI::new(user_id);
      let gid = GI::new(guild_id);
      match db.elo.set_dynamic_elo(uid, gid, dynamic_elo).await {
        Ok(_) => info!("[GUI] UpdateUserDynamicElo {} g={} → {:?}", user_id, guild_id, dynamic_elo),
        Err(e) => warn!("[GUI] UpdateUserDynamicElo {} g={} failed: {}", user_id, guild_id, e),
      }
      Ok(None)
    }

    // ── Config Management ─────────────────────────────────────────────────
    GuiCommand::LoadGuildConfig { guild_id } => {
      let gid = GI::new(guild_id);
      match db.config.get_config_map(gid).await {
        Ok(config_map) => {
          info!("[GUI] LoadGuildConfig g={} — loaded {} values", guild_id, config_map.len());
          // Update the cache
          if let Ok(mut cache) = guild_config_cache.try_write() {
            cache.insert(guild_id, config_map);
          }
          Ok(Some(guild_id))
        }
        Err(e) => {
          warn!("[GUI] LoadGuildConfig g={} failed: {}", guild_id, e);
          Ok(None)
        }
      }
    }

    GuiCommand::UpdateGuildConfigBool { guild_id, column, value } => {
      let gid = GI::new(guild_id);
      match db.config.set_bool(gid, &column, value).await {
        Ok(_) => info!("[GUI] UpdateGuildConfigBool g={} {}={}", guild_id, column, value),
        Err(e) => warn!("[GUI] UpdateGuildConfigBool g={} {} failed: {}", guild_id, column, e),
      }
      Ok(Some(guild_id))
    }

    GuiCommand::UpdateGuildConfigInt { guild_id, column, value } => {
      let gid = GI::new(guild_id);
      match db.config.set_int(gid, &column, value).await {
        Ok(_) => info!("[GUI] UpdateGuildConfigInt g={} {}={}", guild_id, column, value),
        Err(e) => warn!("[GUI] UpdateGuildConfigInt g={} {} failed: {}", guild_id, column, e),
      }
      Ok(Some(guild_id))
    }

    GuiCommand::UpdateGuildConfigText { guild_id, column, value } => {
      let gid = GI::new(guild_id);
      match db.config.set_text(gid, &column, &value).await {
        Ok(_) => info!("[GUI] UpdateGuildConfigText g={} {}={}", guild_id, column, value),
        Err(e) => warn!("[GUI] UpdateGuildConfigText g={} {} failed: {}", guild_id, column, e),
      }
      Ok(Some(guild_id))
    }

    // ── System Messages ─────────────────────────────────────────────────────
    GuiCommand::SendSystemMessage { guild_id, message } => {
      if let Some(ctx) = ctx {
        if let Some(guild_id) = guild_id {
          match crate::services::send_system_message(&ctx, db, GI::new(guild_id), &message).await {
            Ok(_) => info!("[GUI] System message sent to guild {}", guild_id),
            Err(e) => error!("[GUI] Failed to send system message to guild {}: {}", guild_id, e),
          }
        } else {
          match crate::services::broadcast_system_message(&ctx, db, &message).await {
            Ok(results) => {
              let success_count = results.iter().filter(|(_, r)| r.is_ok()).count();
              let error_count = results.iter().filter(|(_, r)| r.is_err()).count();
              info!("[GUI] Broadcast system message: {} success, {} failed", success_count, error_count);
            }
            Err(e) => error!("[GUI] Failed to broadcast system message: {}", e),
          }
        }
      } else {
        error!("[GUI] Cannot send system message: Discord context not available");
      }
      Ok(None)
    }

    GuiCommand::ValidateSystemMessageChannels => {
      if let Some(ctx) = ctx {
        let errors = crate::services::validate_system_message_channels(&ctx, db).await;
        if errors.is_empty() {
          info!("[GUI] All system message channels validated successfully");
        } else {
          error!("[GUI] Found {} guild(s) with invalid system message channels", errors.len());
          for (guild_id, guild_name, error) in errors {
            error!("[GUI] [{}] {}: {}", guild_id, guild_name, error);
          }
        }
      } else {
        error!("[GUI] Cannot validate system message channels: Discord context not available");
      }
      Ok(None)
    }

    GuiCommand::SendCommunityUpdate { guild_id, message } => {
      if let Some(ctx) = ctx {
        if let Some(guild_id) = guild_id {
          let result = crate::services::send_community_update(&ctx, db, guild_id.into(), &message).await;
          match result {
            Ok(_) => info!("[GUI] Community update sent to guild {}", guild_id),
            Err(e) => error!("[GUI] Failed to send community update to guild {}: {}", guild_id, e),
          }
        } else {
          match crate::services::broadcast_community_update(&ctx, db, &message).await {
            Ok(results) => {
              let success_count = results.iter().filter(|(_, r)| r.is_ok()).count();
              let fail_count = results.len() - success_count;
              info!("[GUI] Community update broadcast: {} succeeded, {} failed", success_count, fail_count);
              for (guild_id, result) in results {
                if let Err(e) = result {
                  error!("[GUI] Failed to send community update to guild {}: {}", guild_id, e);
                }
              }
            }
            Err(e) => {
              error!("[GUI] Failed to broadcast community update: {}", e);
            }
          }
        }
      } else {
        error!("[GUI] Cannot send community update: Discord context not available");
      }
      Ok(None)
    }

    GuiCommand::ValidateCommunityUpdatesChannels => {
      if let Some(ctx) = ctx {
        let errors = crate::services::validate_community_updates_channels(&ctx, db).await;
        if errors.is_empty() {
          info!("[GUI] All community updates channels validated successfully");
        } else {
          error!("[GUI] Found {} guild(s) with invalid community updates channels", errors.len());
          for (guild_id, guild_name, error) in errors {
            error!("[GUI] [{}] {}: {}", guild_id, guild_name, error);
          }
        }
      } else {
        error!("[GUI] Cannot validate community updates channels: Discord context not available");
      }
      Ok(None)
    }

    // ── Voice Channel (needs Discord API — log only) ───────────────────────
    GuiCommand::MovePlayerToVC { guild_id, user_id, channel_id } => {
      info!("[GUI] MovePlayerToVC u={} → ch={} g={} — requires Discord HTTP, not implemented in GUI handler", user_id, channel_id, guild_id);
      Ok(None)
    }
    GuiCommand::KickFromVC { guild_id, user_id } => {
      info!("[GUI] KickFromVC u={} g={} — requires Discord HTTP, not implemented in GUI handler", user_id, guild_id);
      Ok(None)
    }
    GuiCommand::SyncVCState { guild_id, category_id } => {
      info!("[GUI] SyncVCState g={} c={} — requires Discord cache, not implemented in GUI handler", guild_id, category_id);
      Ok(None)
    }
    GuiCommand::ClearAllTeamVCs { guild_id, category_id } => {
      info!("[GUI] ClearAllTeamVCs g={} c={} — requires Discord HTTP, not implemented in GUI handler", guild_id, category_id);
      Ok(None)
    }
    GuiCommand::TestDiscordApi => {
      info!("[GUI] TestDiscordApi — requires Discord HTTP, not implemented in GUI handler");
      Ok(None)
    }
    GuiCommand::ToggleDebugMode { guild_id, category_id, enabled } => {
      info!("[GUI] ToggleDebugMode g={} c={} enabled={} — not implemented", guild_id, category_id, enabled);
      Ok(None)
    }
    GuiCommand::GracefulRestart => {
      info!("[GUI] GracefulRestart — handled by application.rs command task");
      Ok(None)
    }
    GuiCommand::GracefulShutdown => {
      info!("[GUI] GracefulShutdown — handled by application.rs command task");
      Ok(None)
    }
  };
  result?;
  Ok(affected_guild)
}
