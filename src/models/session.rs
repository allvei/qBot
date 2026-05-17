use std::str::FromStr;
use std::time::SystemTime;
use std::collections::HashMap;

use anyhow::{Error, Result};
use egui::RichText;
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId as CI, CreateEmbed as CE, CreateEmbedFooter as CEF, UserId as UI};
use sqlx::FromRow;

use crate::{models::Player};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
  pub status: SessionStatus,
  pub pool: Vec<SessionPlayer>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub ready_at: Option<SystemTime>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub started_at: Option<SystemTime>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub match_ended_at: Option<SystemTime>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub team_channels: Option<TeamChannel>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub pending_team_switch: Option<PendingTeamSwitch>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub last_action_at: Option<SystemTime>,
  #[serde(default)]
  pub score_reported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTeamSwitch {
  pub detected_at: SystemTime,
  pub swapped_teams: HashMap<UI, Team>,
}

impl Session {
  /// Get a player by their Discord ID
  pub fn get_player(&self, user_id: UI) -> Result<Player> {
    match self.pool.iter().find(|p| p.player.user_id == user_id) {
      Some(player) => Ok(player.player.clone()),
      None => Err(anyhow::anyhow!("User not found")),
    }
  }

  /// Add a player to the session
  /// Returns Ok(position) with the player's 1-indexed position in the queue
  pub fn add_ply(&mut self, player: Player, in_vc: bool) -> Result<usize> {
    let mut session_player = SessionPlayer::add(player);
    session_player.in_vc = in_vc;
    self.pool.push(session_player);
    Ok(self.pool.len())
  }

  /// Add a player to the session with their rank, marking them as already in queue VC
  /// Use this when re-adding players who were just moved to the queue channel
  /// Returns Ok(position) with the player's 1-indexed position in the queue
  #[deprecated(since = "0.12.0", note = "Use add_ply() with in_vc=true instead")]
  pub fn add_player_in_vc(&mut self, player: Player) -> Result<usize> {
    self.add_ply(player, true)
  }

  pub fn remove_player(&mut self, user_id: UI) {
    self.pool.retain(|p| p.player.user_id != user_id);
  }

  /// Create a new session
  pub fn new(status: SessionStatus, pool: Vec<SessionPlayer>) -> Self {
    Self { status, pool, ready_at: None, started_at: None, match_ended_at: None, team_channels: None, pending_team_switch: None, last_action_at: None, score_reported: false }
  }

  /// Check if the session is active
  pub fn is_active(&self) -> bool {
    matches!(self.status, SessionStatus::Push | SessionStatus::Live | SessionStatus::Pull)
  }

  /// Check if the session is hot
  pub fn is_hot(&self) -> bool {
    matches!(self.status, SessionStatus::Hot)
  }

  /// Check if the session is idle
  pub fn is_idle(&self) -> bool {
    matches!(self.status, SessionStatus::Idle)
  }

  /// Set the session to idle and clear team assignments
  pub fn idle(&mut self) {
    self.status = SessionStatus::Idle;
    self.ready_at = None;
    self.started_at = None;
    self.match_ended_at = None;
    self.team_channels = None;
    self.pending_team_switch = None;
    // Clear team assignments and VC join tracking when going back to idle
    for player in &mut self.pool {
      player.team = None;
    }
  }

  /// Set the session to hot and record the ready timestamp
  pub fn hot(&mut self) -> CE {
    self.status = SessionStatus::Hot;
    self.ready_at = Some(SystemTime::now());
    // Create an embed message for the game ready notification

    CE::new().title("GAME READY!").description(format!("A match is ready to start with {} players!", self.pool.len())).footer(CEF::new("Awaiting team generation..."))
  }

  /// Set the session to push
  pub fn push(&mut self) {
    self.status = SessionStatus::Push;
  }

  /// Set the session to live and record start time
  pub fn live(&mut self) {
    self.status = SessionStatus::Live;
    self.started_at = Some(SystemTime::now());
  }

  /// Set the session to pull
  pub fn pull(&mut self) {
    self.status = SessionStatus::Pull;
  }

  /// Detect if players have switched teams based on current VC positions
  /// Returns true if a valid swap is detected (one player from each team swapped)
  pub fn detect_team_switch(&mut self, ctx: &serenity::all::Context, guild_id: serenity::all::GuildId) -> bool {
    use tracing::debug;

    // Only track switches during Live games
    if self.status != SessionStatus::Live {
      return false;
    }

    let Some(team_channels) = &self.team_channels else {
      return false;
    };

    let red_vc = team_channels.red_vc;
    let blu_vc = team_channels.blu_vc;

    // Get current VC positions from Discord
    let Some(guild) = ctx.cache.guild(guild_id) else {
      return false;
    };

    let mut current_vc_positions = HashMap::new();
    for player in &self.pool {
      if let Some(voice_state) = guild.voice_states.get(&player.player.user_id) {
        if let Some(channel_id) = voice_state.channel_id {
          if channel_id == red_vc || channel_id == blu_vc {
            current_vc_positions.insert(player.player.user_id, channel_id);
          }
        }
      }
    }

    // Detect switches: find players whose VC doesn't match their assigned team
    let mut swapped_teams = HashMap::new();
    for player in &self.pool {
      let Some(assigned_team) = player.team else { continue };
      let Some(&current_vc) = current_vc_positions.get(&player.player.user_id) else { continue };

      let expected_vc = match assigned_team {
        Team::Red => red_vc,
        Team::Blu => blu_vc,
        Team::Unassigned => continue,
      };

      if current_vc != expected_vc {
        // Player is in wrong VC - they switched
        let new_team = if current_vc == red_vc { Team::Red } else { Team::Blu };
        swapped_teams.insert(player.player.user_id, new_team);
      }
    }

    // Valid switch requires exactly 2 players (one from each team swapping)
    if swapped_teams.len() != 2 {
      return false;
    }

    // Verify it's a cross-team swap (one Red->Blu, one Blu->Red)
    let teams: Vec<_> = swapped_teams.values().collect();
    if teams.len() == 2 && teams[0] != teams[1] {
      debug!("Detected valid team switch: {} players swapped teams", swapped_teams.len());
      self.pending_team_switch = Some(PendingTeamSwitch {
        detected_at: SystemTime::now(),
        swapped_teams,
      });
      return true;
    }

    false
  }

  /// Check if pending team switch has been stable for 2+ minutes and commit it
  pub fn validate_and_commit_team_switch(&mut self, ctx: &serenity::all::Context, guild_id: serenity::all::GuildId) -> bool {
    use tracing::{info, debug};

    let Some(pending) = &self.pending_team_switch else {
      return false;
    };

    // Check if 2 minutes have elapsed
    let Ok(elapsed) = SystemTime::now().duration_since(pending.detected_at) else {
      return false;
    };

    if elapsed.as_secs() < 120 {
      return false;
    }

    // Verify the switch is still valid (players are still in swapped positions)
    let Some(team_channels) = &self.team_channels else {
      self.pending_team_switch = None;
      return false;
    };

    let red_vc = team_channels.red_vc;
    let blu_vc = team_channels.blu_vc;

    let Some(guild) = ctx.cache.guild(guild_id) else {
      return false;
    };

    // Verify each swapped player is still in their new team's VC
    for (&user_id, &new_team) in &pending.swapped_teams {
      let expected_vc = match new_team {
        Team::Red => red_vc,
        Team::Blu => blu_vc,
        Team::Unassigned => continue,
      };

      let still_in_correct_vc = guild
        .voice_states
        .get(&user_id)
        .and_then(|vs| vs.channel_id)
        .map(|ch| ch == expected_vc)
        .unwrap_or(false);

      if !still_in_correct_vc {
        debug!("Team switch invalidated - player {} not in expected VC", user_id);
        self.pending_team_switch = None;
        return false;
      }
    }

    // Switch is still valid after 2 minutes - commit it to memory
    for (&user_id, &new_team) in &pending.swapped_teams {
      if let Some(player) = self.pool.iter_mut().find(|p| p.player.user_id == user_id) {
        let old_team = player.team;
        player.team = Some(new_team);
        info!("Committed team switch: {} moved from {:?} to {:?}", player.player.tag, old_team, new_team);
      }
    }

    self.pending_team_switch = None;
    true
  }

  /// Create an empty session
  pub fn empty() -> Self {
    Self { status: SessionStatus::Idle, pool: Vec::new(), ready_at: None, started_at: None, match_ended_at: None, team_channels: None, pending_team_switch: None, last_action_at: None, score_reported: false }
  }

  /// Check if this Hot session has timed out (players didn't join VC in time)
  pub fn is_hot_confirm_time(&self, confirm_time_seconds: u64) -> bool {
    if !self.is_hot() {
      return false;
    }

    // Use match_ended_at if available (post-game scenario), otherwise ready_at
    let base_time = self.match_ended_at.or(self.ready_at);

    if let Some(base_time) = base_time {
      if let Ok(elapsed) = SystemTime::now().duration_since(base_time) {
        return elapsed.as_secs() >= confirm_time_seconds;
      }
    }
    false
  }

  /// Get seconds remaining until timeout (returns 0 if timed out or not hot)
  pub fn seconds_until_confirm_expiration(&self, confirm_time_seconds: u64) -> u64 {
    if !self.is_hot() {
      return 0;
    }

    // Use match_ended_at if available (post-game scenario), otherwise ready_at
    let base_time = self.match_ended_at.or(self.ready_at);

    if let Some(base_time) = base_time {
      if let Ok(elapsed) = SystemTime::now().duration_since(base_time) {
        let elapsed_secs = elapsed.as_secs();
        if elapsed_secs < confirm_time_seconds {
          return confirm_time_seconds - elapsed_secs;
        }
      }
    }
    0
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
  Idle, // Waiting for enough players to join
  Hot,  // Waiting for runners to start the game
  Push, // Moving players to the team channels
  Live, // Game is active
  Pull, // Moving players back to the queue
}

impl SessionStatus {
  pub fn icon(&self) -> egui::RichText {
    use egui_phosphor::regular::*;
    let icon = match self {
      SessionStatus::Idle => MOON,
      SessionStatus::Hot => FIRE_SIMPLE,
      SessionStatus::Push => ARROW_BEND_DOWN_RIGHT,
      SessionStatus::Live => PLAY,
      SessionStatus::Pull => ARROW_BEND_DOWN_LEFT,
    };
    egui::RichText::new(icon).family(egui::FontFamily::Name("phosphor".into()))
  }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
  pub player: Player,
  pub team: Option<Team>,
  pub in_vc: bool,
  pub in_queue: bool,
  pub joined_at: SystemTime,
  pub queue_expiration: u8,
  pub vc_leave_grace_until: Option<SystemTime>,
}

impl SessionPlayer {
  pub fn add(player: Player) -> Self {
    Self { player: player.clone(),
      team: None,
      in_vc: false,
      in_queue: false,
      joined_at: SystemTime::now(),
      queue_expiration: player.queue_expiration,
      vc_leave_grace_until: None }
  }

  pub fn set_team(&mut self, team: Team) {
    self.team = Some(team);
  }

  pub fn vc_on(&mut self) {
    self.in_vc = true;
  }

  pub fn vc_off(&mut self) {
    self.in_vc = false;
  }

  pub fn vc_icon(&self) -> egui::RichText {
    use egui_phosphor::regular::*;
    let icon = match self.in_vc {
      true => HEADSET,
      false => MOON,
    };
    RichText::new(icon).family(egui::FontFamily::Name("phosphor".into()))
  }
}

#[allow(dead_code)]
trait Quota {
  fn less(&self, quota: usize) -> bool;
  fn equal(&self, quota: usize) -> bool;
  fn more(&self, quota: usize) -> bool;
}

impl Quota for Vec<SessionPlayer> {
  fn less(&self, quota: usize) -> bool {
    self.len() < quota
  }
  fn equal(&self, quota: usize) -> bool {
    self.len() == quota
  }
  fn more(&self, quota: usize) -> bool {
    self.len() > quota
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannel {
  pub red_vc: CI,
  pub blu_vc: CI,
  pub set_index: u32,
  pub session_id: Option<String>,
}

impl TeamChannel {
  pub fn new(red_vc: CI, blu_vc: CI, set_index: u32) -> Self {
    Self { red_vc, blu_vc, set_index, session_id: None }
  }

  pub fn with_session(red_vc: CI, blu_vc: CI, set_index: u32, session_id: String) -> Self {
    Self { red_vc, blu_vc, set_index, session_id: Some(session_id) }
  }

  pub fn empty() -> Self {
    Self { red_vc: CI::new(1), blu_vc: CI::new(1), set_index: 0, session_id: None }
  }

  /// Checks if this TeamChannel contains the given channel_id
  /// in either red_vc or blu_vc
  pub fn contains_channel(&self, channel_id: CI) -> bool {
    self.red_vc == channel_id || self.blu_vc == channel_id
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Team {
  Unassigned,
  Red,
  Blu,
}

impl FromStr for Team {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "UNASSIGNED" => Ok(Team::Unassigned),
      "RED" => Ok(Team::Red),
      "BLU" => Ok(Team::Blu),
      _ => Err(Error::msg(format!("Unknown : {s}"))),
    }
  }
}

/// Process match result with ELO calculations
/// This is a shared function used by both dashboard and runner menu to ensure consistency
///
/// # Arguments
/// * `db` - Database connection
/// * `guild_id` - Guild ID
/// * `category_id` - Category ID
/// * `session_players` - Session player data for ELO processing
/// * `result` - Match result ("red", "draw", or "blu")
///
/// # Returns
/// * `Ok(Some(changes))` - ELO changes applied (if dynamic ELO enabled)
/// * `Ok(None)` - No ELO changes (dynamic ELO disabled or no match found)
/// * `Err(e)` if database operations fail
pub async fn process_match_result_with_elo(
  db: std::sync::Arc<crate::db::Database>,
  guild_id: serenity::all::GuildId,
  category_id: u8,
  session_players: &[SessionPlayer],
  result: &str,
  ctx: &serenity::prelude::Context,
) -> Result<Option<Vec<crate::models::dynamic_elo::EloChange>>> {
  use tracing::error;

  // Get latest match ID from database
  if let Some(match_id) = db.matches.get_latest_match_id(guild_id, category_id as i64).await? {
    // Update match result in database
    if let Err(e) = db.matches.update_match_result(match_id, result).await {
      error!("Failed to update match result in database: {e}");
    }

    // Apply dynamic ELO changes if enabled
    return match crate::models::dynamic_elo::process_match_elo(
      &db, guild_id, match_id, session_players, result, ctx,
    ).await {
      Ok(Some(changes)) => {
        tracing::info!("Dynamic ELO applied: {} players updated", changes.len());
        Ok(Some(changes))
      }
      Ok(None) => Ok(None), // Dynamic ELO not enabled
      Err(e) => {
        error!("Failed to process dynamic ELO: {e}");
        Err(e.into())
      }
    };
  }

  Ok(None)
}
