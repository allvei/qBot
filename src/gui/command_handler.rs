//! Handler for GUI commands

use anyhow::{anyhow, Result};
use serenity::all::{GuildId as GI, UserId as UI};
use tracing::{info, warn};

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

/// Handle a command from the GUI
pub async fn handle_command(command: GuiCommand, manager: &mut Manager, db: &Database) -> Result<()> {
  match command {
    // ── Snapshot ──────────────────────────────────────────────────────────
    GuiCommand::RefreshSnapshot => {
      info!("[GUI] RefreshSnapshot — snapshot updated by periodic task");
      Ok(())
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
      Ok(())
    }

    GuiCommand::RemovePlayer { guild_id, category_id, fmt_id, user_id } => {
      let uid = UI::new(user_id);
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in &mut fmt.sessions {
        if session.pool.iter().any(|p| p.player.user_id == uid) {
          session.remove_player(uid);
          info!("[GUI] RemovePlayer {} from g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
          return Ok(());
        }
      }
      warn!("[GUI] RemovePlayer {} not found in g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
      Ok(())
    }

    GuiCommand::BufferPlayer { guild_id, category_id, fmt_id, user_id } => {
      let uid = UI::new(user_id);
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in fmt.sessions.iter_mut().filter(|s| s.is_idle()) {
        if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == uid) {
          let sp = session.pool.remove(pos);
          session.pool.insert(0, sp);
          info!("[GUI] BufferPlayer {} → front g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
          return Ok(());
        }
      }
      warn!("[GUI] BufferPlayer {} not found in idle sessions", user_id);
      Ok(())
    }

    GuiCommand::FatkidPlayer { guild_id, category_id, fmt_id, user_id } => {
      let uid = UI::new(user_id);
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in fmt.sessions.iter_mut().filter(|s| s.is_idle()) {
        if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == uid) {
          let sp = session.pool.remove(pos);
          session.pool.push(sp);
          info!("[GUI] FatkidPlayer {} → end g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
          return Ok(());
        }
      }
      warn!("[GUI] FatkidPlayer {} not found in idle sessions", user_id);
      Ok(())
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
          return Ok(());
        }
      }
      warn!("[GUI] ReorderQueue {} not found in idle sessions", user_id);
      Ok(())
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
      Ok(())
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
      Ok(())
    }

    GuiCommand::ResetSessionTimer { guild_id, category_id, fmt_id, session_index } => {
      let session = find_session(manager, guild_id, category_id, fmt_id, session_index)?;
      session.ready_at = None;
      info!("[GUI] ResetSessionTimer session {}", session_index);
      Ok(())
    }

    GuiCommand::ForceTeamRegeneration { guild_id, category_id, fmt_id, session_index } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      let quota = fmt.quota as usize;
      let session = fmt.sessions.get_mut(session_index).ok_or_else(|| anyhow!("Session {} not found", session_index))?;
      let pool_len = session.pool.len().min(quota);
      bch_assign_teams(&mut session.pool[..pool_len]);
      info!("[GUI] ForceTeamRegeneration session {} — {} players assigned", session_index, pool_len);
      Ok(())
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
      Ok(())
    }

    GuiCommand::ForceEndGame { guild_id, category_id, fmt_id, session_index } => {
      let session = find_session(manager, guild_id, category_id, fmt_id, session_index)?;
      let players = session.pool.len();
      session.pool.clear();
      session.idle();
      info!("[GUI] ForceEndGame session {} — cleared {} players, reset to Idle", session_index, players);
      Ok(())
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
        return Ok(());
      }
      session.add_ply(player, false)?;
      info!("[GUI] AddPlayer {} added to g={} c={} f={}", user_id, guild_id, category_id, fmt_id);
      Ok(())
    }

    GuiCommand::ForceQuotaMet { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      if let Some(session) = fmt.sessions.iter_mut().find(|s| s.is_idle()) {
        session.hot();
        info!("[GUI] ForceQuotaMet — idle session forced to Hot");
      } else {
        warn!("[GUI] ForceQuotaMet — no idle session found");
      }
      Ok(())
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
      Ok(())
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
      Ok(())
    }

    GuiCommand::SimulateVCTimeout { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in fmt.sessions.iter_mut().filter(|s| s.is_hot()) {
        session.pool.retain(|p| p.in_vc);
        info!("[GUI] SimulateVCTimeout — removed non-VC players from hot session");
      }
      Ok(())
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
      Ok(())
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
      Ok(())
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
      Ok(())
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
      Ok(())
    }

    GuiCommand::ClearPendingTeamSwitches { guild_id, category_id, fmt_id } => {
      let fmt = find_format(manager, guild_id, category_id, fmt_id)?;
      for session in &mut fmt.sessions {
        session.pending_team_switch = None;
      }
      info!("[GUI] ClearPendingTeamSwitches g={} c={} f={}", guild_id, category_id, fmt_id);
      Ok(())
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
            return Ok(());
          }
        }
      }
      warn!("[GUI] FixPlayerVCState {} not found", user_id);
      Ok(())
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
      Ok(())
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
      Ok(())
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
      Ok(())
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
      Ok(())
    }

    // ── Voice Channel (needs Discord API — log only) ───────────────────────
    GuiCommand::MovePlayerToVC { guild_id, user_id, channel_id } => {
      info!("[GUI] MovePlayerToVC u={} → ch={} g={} — requires Discord HTTP, not implemented in GUI handler", user_id, channel_id, guild_id);
      Ok(())
    }
    GuiCommand::KickFromVC { guild_id, user_id } => {
      info!("[GUI] KickFromVC u={} g={} — requires Discord HTTP, not implemented in GUI handler", user_id, guild_id);
      Ok(())
    }
    GuiCommand::SyncVCState { guild_id, category_id } => {
      info!("[GUI] SyncVCState g={} c={} — requires Discord cache, not implemented in GUI handler", guild_id, category_id);
      Ok(())
    }
    GuiCommand::ClearAllTeamVCs { guild_id, category_id } => {
      info!("[GUI] ClearAllTeamVCs g={} c={} — requires Discord HTTP, not implemented in GUI handler", guild_id, category_id);
      Ok(())
    }
    GuiCommand::TestDiscordApi => {
      info!("[GUI] TestDiscordApi — requires Discord HTTP, not implemented in GUI handler");
      Ok(())
    }
    GuiCommand::ToggleDebugMode { guild_id, category_id, enabled } => {
      info!("[GUI] ToggleDebugMode g={} c={} enabled={} — not implemented", guild_id, category_id, enabled);
      Ok(())
    }
  }
}
