//! # Server Module
//!
//! This module defines the Server struct and its related functionality.
//! A Server represents a Discord guild with associated categories and games.

use std::time::{Instant, SystemTime};
use std::sync::Arc;

use crate::{
  guild_name, log_prefix_format,
  models::constants::DEFAULT_ACTIVE_ELO,
  Database as DB, Manager, Rank, GREEN,
};
use anyhow::{anyhow, Error, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId as CI, Context, CreateEmbed, CreateMessage as CM, EditMember, GuildId as GI, MessageId as MI, RoleId as RI, UserId as UI};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::models::{Player, Session, SessionPlayer, SessionStatus, TeamChannel};

// Voice channel movement configuration to prevent Discord client bugs
const VC_MOVE_BATCH_SIZE: usize = 4; // Number of users to move in parallel per batch
const VC_MOVE_BATCH_DELAY_MS: u64 = 50; // Delay between batches in milliseconds

/// Context parameters for queue operations
pub struct QueueContext<'a> {
  pub ctx: &'a Context,
  pub guild_id: Option<GI>,
  pub db: Option<&'a DB>,
  pub manager: Option<Arc<Mutex<Manager>>>,
}

impl<'a> QueueContext<'a> {
  pub fn new(ctx: &'a Context, guild_id: Option<GI>, db: Option<&'a DB>, manager: Option<Arc<Mutex<Manager>>>) -> Self {
    Self { ctx, guild_id, db, manager }
  }
}

/// Helper function to calculate mean, median, and standard deviation for team ELOs
fn calculate_stats(elos: &[f64]) -> (f64, f64, f64) {
  if elos.is_empty() {
    return (0.0, 0.0, 0.0);
  }

  // Calculate mean
  let sum: f64 = elos.iter().sum();
  let mean = sum / elos.len() as f64;

  // Calculate median
  let mut sorted = elos.to_vec();
  sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
  let median = if sorted.len().is_multiple_of(2) {
    let mid = sorted.len() / 2;
    (sorted[mid - 1] + sorted[mid]) / 2.0
  } else {
    sorted[sorted.len() / 2]
  };

  // Calculate standard deviation
  let variance: f64 = elos.iter().map(|&elo| (elo - mean).powi(2)).sum::<f64>() / elos.len() as f64;
  let std_dev = variance.sqrt();

  (mean, median, std_dev)
}

/// Represents a game server with IP and name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameServer {
  pub ip: String,
  pub name: String,
}

// Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QGuild {
  pub id: GI,
  pub name: String,
  pub roles: Roles,
  pub categories: Vec<Category>,
}

impl QGuild {
  pub fn new(guild_id: GI, guild_name: String, roles: Roles) -> Self {
    Self { id: guild_id, name: guild_name, roles, categories: Vec::new() }
  }

  pub fn add_category(&mut self, category: Category) -> Result<()> {
    self.categories.push(category);
    if let Some(category) = self.categories.last_mut() {
      // Create an idle session for every format
      for format in &mut category.formats {
        if format.sessions.is_empty() {
          format.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
        }
      }
    }
    Ok(())
  }

  pub fn has_categories(&self) -> bool {
    !self.categories.is_empty()
  }

  pub fn empty(guild_id: GI, guild_name: String) -> Self {
    Self { id: guild_id, name: guild_name, roles: Roles::empty(), categories: Vec::new() }
  }

  pub fn get_category(&mut self, channel_id: CI) -> Result<&mut Category> {
    match self.categories.iter_mut().find(|category| category.contains_channel(channel_id)) {
      Some(category) => Ok(category),
      None => Err(anyhow!("Category not found")),
    }
  }

  /// Check if active ELO is enabled for this server
  pub async fn is_active_elo_enabled(&self, db: &DB) -> Result<bool> {
    Ok(db.config.get_active_elo(self.id).await.unwrap_or(DEFAULT_ACTIVE_ELO))
  }
}

/// Team balancing method for generating teams
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TeamBalanceMethod {
  #[default]
  Bch,
  Average,
}

impl TeamBalanceMethod {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Bch => "BCH",
      Self::Average => "Average",
    }
  }

  pub fn parse(s: &str) -> Self {
    match s.to_lowercase().as_str() {
      "average" => Self::Average,
      _ => Self::Bch,
    }
  }
}

impl std::fmt::Display for TeamBalanceMethod {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

/// When to create team voice channels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TeamVcCreatePolicy {
  /// Create when the first player joins the queue
  OnFirstJoin,
  /// Create when the game goes hot (quota met)
  #[default]
  OnHot,
  /// Create when runners start the game (push)
  OnGameStart,
}

impl TeamVcCreatePolicy {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::OnFirstJoin => "First player joins",
      Self::OnHot => "Game goes hot",
      Self::OnGameStart => "Runners start game",
    }
  }

  pub fn parse(s: &str) -> Self {
    match s {
      "on_first_join" => Self::OnFirstJoin,
      "on_hot" => Self::OnHot,
      "on_game_start" => Self::OnGameStart,
      _ => Self::default(),
    }
  }

  pub fn to_db_str(&self) -> &'static str {
    match self {
      Self::OnFirstJoin => "on_first_join",
      Self::OnHot => "on_hot",
      Self::OnGameStart => "on_game_start",
    }
  }
}

impl std::fmt::Display for TeamVcCreatePolicy {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

/// When to destroy team voice channels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TeamVcDestroyPolicy {
  /// Destroy when the last player leaves the queue
  OnLastLeave,
  /// Destroy after players are moved back to queue VC (after pull)
  #[default]
  AfterPull,
  /// Destroy after a timeout post-game if no new game starts
  AfterExpiration,
}

impl TeamVcDestroyPolicy {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::OnLastLeave => "Last player leaves",
      Self::AfterPull => "After game ends",
      Self::AfterExpiration => "After post-game expiration",
    }
  }

  pub fn parse(s: &str) -> Self {
    match s {
      "on_last_leave" => Self::OnLastLeave,
      "after_pull" => Self::AfterPull,
      "after_expiration" => Self::AfterExpiration,
      _ => Self::default(),
    }
  }

  pub fn to_db_str(&self) -> &'static str {
    match self {
      Self::OnLastLeave => "on_last_leave",
      Self::AfterPull => "after_pull",
      Self::AfterExpiration => "after_expiration",
    }
  }
}

impl std::fmt::Display for TeamVcDestroyPolicy {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

/// Settings controlling dynamic team voice channel lifecycle
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TeamVcSettings {
  pub create_policy: TeamVcCreatePolicy,
  pub destroy_policy: TeamVcDestroyPolicy,
  /// Always keep at least 1 set of team channels; create more as needed
  pub keep_minimum: bool,
}

impl Default for TeamVcSettings {
  fn default() -> Self {
    Self { create_policy: TeamVcCreatePolicy::default(), destroy_policy: TeamVcDestroyPolicy::default(), keep_minimum: true }
  }
}

// Format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Format {
  pub id: u8,
  pub name: String,
  pub quota: u8,
  pub sessions: Vec<Session>,
  pub connect_info: Option<String>,
  /// Active captain draft state for this format
  #[serde(skip)]
  pub captain_draft: Option<CaptainDraft>,
}

/// Captain draft state for manual team picking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptainDraft {
  /// The two captains (highest ELO players)
  pub captains: (serenity::all::UserId, serenity::all::UserId),
  /// Which captain's turn it is (0 = Red/first, 1 = Blue/second)
  pub current_turn: usize,
  /// Pick order (ABBAAB pattern)
  pub pick_order: Vec<usize>,
  /// Current pick index in the order
  pub current_pick_index: usize,
  /// Channel ID where the draft embed is displayed
  pub draft_channel_id: serenity::all::ChannelId,
  /// Message ID of the draft embed
  pub draft_message_id: serenity::all::MessageId,
}

impl Format {
  pub fn new(id: u8, name: String, quota: u8) -> Self {
    Self { id, name, quota, sessions: Vec::new(), connect_info: None, captain_draft: None }
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn contains_user(&self, user_id: UI) -> bool {
    self.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.user_id == user_id))
  }

  pub fn get_player(&self, user_id: UI) -> Result<Player> {
    self.sessions.get_player(user_id)
  }

  /// Pack waiting players so no queue holds more than `quota` players.
  ///
  /// Queue order is preserved: players from later Idle sessions are pulled forward to fill
  /// earlier ones and everyone past `quota` spills into the next Idle session (the following
  /// game). Emptied Idle sessions are dropped, keeping a single open queue. Hot/Push/Live/Pull
  /// sessions are left untouched.
  ///
  /// Returns true when the layout changed.
  pub fn pack_idle_sessions(&mut self) -> bool {
    let quota = (self.quota as usize).max(1);
    let idle_idxs: Vec<usize> = self.sessions.iter().enumerate().filter(|(_, s)| s.is_idle()).map(|(i, _)| i).collect();
    if idle_idxs.is_empty() {
      return false;
    }

    let layout = |sessions: &[Session]| -> Vec<Vec<UI>> {
      sessions.iter().filter(|s| s.is_idle()).map(|s| s.pool.iter().map(|p| p.player.user_id).collect()).collect()
    };
    let before = layout(&self.sessions);

    // Collect every waiting player in queue order, then hand them back out in quota-sized chunks
    let mut waiting: Vec<SessionPlayer> = Vec::new();
    for &i in &idle_idxs {
      waiting.append(&mut self.sessions[i].pool);
    }
    let mut waiting = waiting.into_iter().peekable();

    for &i in &idle_idxs {
      self.sessions[i].pool = waiting.by_ref().take(quota).collect();
    }
    while waiting.peek().is_some() {
      let pool: Vec<SessionPlayer> = waiting.by_ref().take(quota).collect();
      self.sessions.push(Session::new(SessionStatus::Idle, pool));
    }

    // Drop the sessions that were emptied, keeping one open queue to join
    let has_open_queue = self.sessions.iter().any(|s| s.is_idle() && !s.pool.is_empty() && s.pool.len() < quota);
    let mut keep_empty = !has_open_queue;
    self.sessions.retain(|s| {
      if !(s.is_idle() && s.pool.is_empty()) {
        return true;
      }
      let keep = keep_empty;
      keep_empty = false;
      keep
    });

    layout(&self.sessions) != before
  }
}

trait FindPlayer {
  fn get_player(&self, user_id: UI) -> Result<Player>;
}

impl FindPlayer for Vec<Session> {
  fn get_player(&self, user_id: UI) -> Result<Player> {
    self.iter().find(|session| session.get_player(user_id).is_ok()).unwrap().get_player(user_id)
  }
}

// Category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
  pub guild_id: GI,
  pub guild_name: Option<String>,
  pub id: u8,
  pub name: Option<String>,
  pub confirm_time: u16,
  pub dashboard_msg: MI,
  pub channels: Channels,
  pub formats: Vec<Format>,
  pub team_balance_method: TeamBalanceMethod,
  pub team_vc_settings: TeamVcSettings,
  pub dm_alert_enabled: bool,
  pub dm_alert_threshold: u8,
  pub dm_alert_users: Vec<UI>,
  /// Track recently freed team channels to avoid immediate recreation
  pub recently_freed_teams: Vec<TeamChannel>,
  /// Require score reporting when ending matches via dashboard
  pub require_score_report: bool,
  /// Whether this category records match results and processes dynamic ELO
  pub enable_competitive: bool,
  /// Last dashboard action (user_tag, action_description, timestamp)
  #[serde(skip)]
  pub last_action: Option<(String, String, SystemTime)>,
  /// Bot is restarting - hide join buttons
  #[serde(skip)]
  pub restarting: bool,
  /// Pending VC notification message_id (database-backed)
  #[serde(skip)]
  pub pending_vc_notification: Option<MI>,
  /// Pending users for VC notification (in-memory only, for editing message content)
  #[serde(skip)]
  pub pending_users: Vec<UI>,
  /// Last ping time per user (for cooldown tracking)
  #[serde(skip)]
  pub last_ping_time: Option<SystemTime>,
}

impl Category {
  pub fn new(
    guild_id: GI,
    guild_name: Option<String>,
    category_id: u8,
    name: Option<String>,
    quota: u8,
    confirm_time: u16,
    dashboard_msg: MI,
    channels: Channels,
    games: Vec<Session>,
  ) -> Self {
    let default_name = name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Category {}", category_id));
    let mut sg = Format::new(0, default_name, quota);
    sg.sessions = games;

    Self {
      guild_id,
      guild_name,
      id: category_id,
      name,
      confirm_time,
      dashboard_msg,
      channels,
      formats: vec![sg],
      team_balance_method: TeamBalanceMethod::default(),
      team_vc_settings: TeamVcSettings::default(),
      dm_alert_enabled: false,
      dm_alert_threshold: 0,
      dm_alert_users: Vec::new(),
      recently_freed_teams: Vec::new(),
      require_score_report: false,
      enable_competitive: true,
      last_action: None,
      restarting: false,
      pending_vc_notification: None,
      pending_users: Vec::new(),
      last_ping_time: None,
    }
  }

  /// Get format by index, defaulting to format 0
  pub fn format(&self, idx: u8) -> Option<&Format> {
    self.formats.iter().find(|sg| sg.id == idx)
  }

  /// Get mutable format by index, defaulting to format 0
  pub fn format_mut(&mut self, idx: u8) -> Option<&mut Format> {
    self.formats.iter_mut().find(|sg| sg.id == idx)
  }

  // --- Backward-compatible accessors delegating to format 0 ---

  /// Sessions of the default format (format 0)
  pub fn sessions(&self) -> &Vec<Session> {
    &self.formats[0].sessions
  }

  /// Mutable sessions of the default format (format 0)
  pub fn sessions_mut(&mut self) -> &mut Vec<Session> {
    &mut self.formats[0].sessions
  }

  /// Returns true if the player exists in any session across all formats
  pub fn contains_player(&self, user_id: UI) -> bool {
    self.formats.iter().any(|format| format.sessions.iter().any(|session| session.pool.iter().any(|player| player.player.user_id == user_id)))
  }

  /// Applies the closure to every occurrence of the player across all sessions.
  /// Returns true if the player was found in at least one session.
  pub fn for_each_player_mut<F>(&mut self, user_id: UI, mut f: F) -> bool
  where
    F: FnMut(&mut SessionPlayer),
  {
    let mut found = false;

    for format in &mut self.formats {
      for session in &mut format.sessions {
        if let Some(session_player) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
          f(session_player);
          found = true;
        }
      }
    }

    found
  }

  /// Quota of the default format (format 0)
  pub fn quota(&self) -> u8 {
    self.formats[0].quota
  }

  /// Connect info of the default format (format 0)
  pub fn connect_info(&self) -> Option<&str> {
    self.formats[0].connect_info.as_deref()
  }

  /// Set connect info on the default format (format 0)
  pub fn set_connect_info(&mut self, info: Option<String>) {
    self.formats[0].connect_info = info;
  }

  /// Set quota on the default format (format 0)
  pub fn set_quota(&mut self, quota: u8) {
    self.formats[0].quota = quota;
  }

  /// Add a new format. Returns error if max (3) reached.
  /// Automatically creates an idle session for the new format.
  pub fn add_format(&mut self, name: String, quota: u8) -> Result<&Format> {
    if self.formats.len() >= 3 {
      return Err(anyhow!("Maximum of 3 formats per category"));
    }
    let id = self.next_format_id();
    let mut sg = Format::new(id, name, quota);
    sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
    self.formats.push(sg);
    Ok(self.formats.last().unwrap())
  }

  /// Remove a format by ID. Cannot remove format 0 (default).
  pub fn remove_format(&mut self, id: u8) -> Result<()> {
    if id == 0 {
      return Err(anyhow!("Cannot remove the default format"));
    }
    let idx = self.formats.iter().position(|sg| sg.id == id).ok_or_else(|| anyhow!("Format {} not found", id))?;
    self.formats.remove(idx);
    Ok(())
  }

  /// Get the next available format ID
  fn next_format_id(&self) -> u8 {
    (0..=255).find(|id| !self.formats.iter().any(|sg| sg.id == *id)).unwrap_or(0)
  }

  /// Get display name for the category (name or "Category {id}")
  pub fn name(&self) -> String {
    self.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Category {}", self.id))
  }

  pub fn create_session(&mut self) -> Result<&mut Session> {
    self.create_session_format(0)
  }

  pub fn create_session_format(&mut self, fmt_id: u8) -> Result<&mut Session> {
    let sg = self.format_mut(fmt_id).ok_or_else(|| anyhow!("Format {} not found", fmt_id))?;
    // Only prevent creation if there's an Idle session with room left; a full queue needs
    // another session so the extra players form the next game
    let quota = sg.quota as usize;
    let has_open_queue = sg.sessions.iter().any(|s| s.is_idle() && s.pool.len() < quota);
    if has_open_queue {
      return Err(anyhow!("Cannot create new session: open idle session already exists"));
    }
    sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
    let sg = self.format_mut(fmt_id).unwrap();
    sg.sessions.last_mut().ok_or_else(|| anyhow!("Failed to create session"))
  }

  pub async fn get_queue(&mut self) -> Result<&mut Session, Error> {
    self.get_queue_fmt(0).await
  }

  /// The queue new players join: the first Idle/Hot session that is still under quota.
  /// Opens a new queue (next game) when every joinable session is already full, so a queue
  /// never grows past the quota.
  pub async fn get_queue_fmt(&mut self, fmt_id: u8) -> Result<&mut Session, Error> {
    let sg = self.format_mut(fmt_id).ok_or_else(|| anyhow!("Format {} not found", fmt_id))?;
    let quota = sg.quota as usize;
    debug!(
      "get_queue_fmt: fmt_id={}, total sessions={}, session pools: {:?}",
      fmt_id,
      sg.sessions.len(),
      sg.sessions.iter().map(|s| format!("{:?}({})", s.status, s.pool.len())).collect::<Vec<_>>()
    );
    match sg.sessions.iter().position(|s| (s.is_idle() || s.is_hot()) && s.pool.len() < quota) {
      Some(idx) => Ok(&mut sg.sessions[idx]),
      None => {
        info!("All queues in format {} are full, opening a new one for the next game", fmt_id);
        sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
        sg.sessions.last_mut().ok_or_else(|| anyhow!("Failed to open a new queue in format {}", fmt_id))
      }
    }
  }

  pub fn get_inactives(&self) -> Vec<&Session> {
    self.formats[0].sessions.iter().filter(|s| !s.is_active()).collect()
  }

  pub fn get_actives(&self) -> Vec<&Session> {
    self.formats[0].sessions.iter().filter(|s| s.is_active()).collect()
  }

  /// Delete any orphaned dynamic VCs left under the category from a previous bot run.
  /// Only deletes channel pairs that are empty; pairs with users are kept intact.
  pub async fn clean_orphaned_vcs(&mut self, ctx: &Context, db: &DB) {
    use serenity::all::ChannelType;

    let category_id = self.channels.category;
    if category_id.get() <= 1 {
      return;
    }

    let guild = match ctx.cache.guild(self.guild_id) {
      Some(g) => g.clone(),
      None => return,
    };

    // Clean up orphaned database entries first (teams where channels no longer exist)
    let existing_channel_ids: Vec<CI> = guild.channels.values().filter(|c| c.kind == ChannelType::Voice).map(|c| c.id).collect();

    if let Ok(orphaned_db_teams) = db.teams.get_orphaned_teams(self.guild_id, &existing_channel_ids).await {
      if !orphaned_db_teams.is_empty() {
        info!("[{}] Cleaning up {} orphaned database team entries", guild.name, orphaned_db_teams.len());
        for (red_vc, blu_vc) in orphaned_db_teams {
          if let Err(e) = db.teams.remove_team(self.guild_id, red_vc, blu_vc, &guild.name, &self.name()).await {
            warn!("Failed to remove orphaned team from database: {}", e);
          }
        }
      }
    }

    let mut surviving_teams = Vec::new();
    let mut deleted_count = 0usize;

    for team in &self.channels.teams {
      let red_exists = guild.channels.contains_key(&team.red_vc);
      let blu_exists = guild.channels.contains_key(&team.blu_vc);

      if !red_exists && !blu_exists {
        // Both channels are gone - DB already cleaned up by get_orphaned_teams above
        continue;
      }

      // Check if users are currently in either channel
      let red_occupied = guild.voice_states.values().any(|vs| vs.channel_id == Some(team.red_vc));
      let blu_occupied = guild.voice_states.values().any(|vs| vs.channel_id == Some(team.blu_vc));
      let has_users = red_occupied || blu_occupied;

      if has_users {
        info!("[{}] Keeping team channel pair with active users: set {}", guild.name, team.set_index);
        surviving_teams.push(team.clone());
        continue;
      }

      // No users - safe to delete the pair
      if red_exists {
        if let Err(e) = team.red_vc.delete(&ctx.http).await {
          if !e.to_string().contains("Unknown channel") {
            warn!("[{}] Failed to delete RED team VC {}: {}", guild.name, team.red_vc, e);
            surviving_teams.push(team.clone());
            continue;
          }
        }
      }
      if blu_exists {
        if let Err(e) = team.blu_vc.delete(&ctx.http).await {
          if !e.to_string().contains("Unknown channel") {
            warn!("[{}] Failed to delete BLU team VC {}: {}", guild.name, team.blu_vc, e);
            surviving_teams.push(team.clone());
            continue;
          }
        }
      }

      if let Err(e) = db.teams.remove_team(self.guild_id, team.red_vc, team.blu_vc, &guild.name, &self.name()).await {
        warn!("[{}] Failed to remove team pair {} from database: {}", guild.name, team.set_index, e);
      }

      deleted_count += 1;
    }

    self.channels.teams = surviving_teams;

    if deleted_count > 0 {
      info!("[{}] Cleaned up {} empty team VC pairs on startup", guild.name, deleted_count);
    }
  }

  pub fn get_seshs_by_status(&self, status: &SessionStatus) -> Vec<&Session> {
    self.formats[0].sessions.iter().filter(|s| s.status == *status).collect()
  }

  pub fn get_seshs_by_status_fmt(&self, fmt_id: u8, status: &SessionStatus) -> Vec<&Session> {
    self.format(fmt_id).map(|sg| sg.sessions.iter().filter(|s| s.status == *status).collect()).unwrap_or_default()
  }

  pub fn get_seshs_by_status_fmt_mut(&mut self, fmt_id: u8, status: &SessionStatus) -> Vec<&mut Session> {
    self.format_mut(fmt_id).map(|sg| sg.sessions.iter_mut().filter(|s| s.status == *status).collect()).unwrap_or_default()
  }

  /// Get session index (position in Vec) for logging purposes
  pub fn get_session_index(&self, session: &Session) -> Option<usize> {
    self.formats[0].sessions.iter().position(|s| std::ptr::eq(s, session))
  }

  pub fn get_games_by_status_mut(&mut self, status: &SessionStatus) -> Vec<&mut Session> {
    self.formats[0].sessions.iter_mut().filter(|s| s.status == *status).collect()
  }

  pub async fn get_user_sesh(&mut self, user_id: UI) -> Result<&mut Session> {
    for sg in &mut self.formats {
      if let Some(game) = sg.sessions.iter_mut().find(|s| s.pool.iter().any(|p| p.player.user_id == user_id)) {
        return Ok(game);
      }
    }
    Err(anyhow!("User not found in any game"))
  }

  /// Check if user is in a session within a specific format
  pub fn get_user_sesh_fmt(&mut self, fmt_id: u8, user_id: UI) -> Result<&mut Session> {
    let sg = self.format_mut(fmt_id).ok_or_else(|| anyhow!("Format {} not found", fmt_id))?;
    sg.sessions.iter_mut().find(|s| s.pool.iter().any(|p| p.player.user_id == user_id)).ok_or_else(|| anyhow!("User not found in format {}", fmt_id))
  }

  /// Get the format name for the format containing this user
  pub fn get_user_fmt_name(&self, user_id: UI) -> String {
    match self.formats.iter().find(|sg| sg.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.user_id == user_id))) {
      Some(fmt_nm) => fmt_nm.name.clone(),
      None => "-".to_string(),
    }
  }

  /// Check if user is in any session across all formats
  pub fn is_user_in_session(&self, user_id: UI) -> bool {
    self.formats.iter().any(|sg| sg.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.user_id == user_id)))
  }

  /// Check if user is in a specific format's sessions
  pub fn is_user_in_fmt(&self, fmt_id: u8, user_id: UI) -> bool {
    self.format(fmt_id).unwrap().contains_user(user_id)
  }

  pub fn is_user_in_other_fmts(&self, fmt_id: u8, user_id: UI) -> bool {
    self.formats.iter().any(|f| f.id != fmt_id && f.contains_user(user_id))
  }

  /// Check if a user is currently in the queue voice channel (cache-only, no HTTP call).
  pub fn is_user_in_queue_vc(&self, cache: &serenity::all::Cache, user_id: UI) -> bool {
    cache
      .guild(self.guild_id)
      .map(|g| g.voice_states.get(&user_id).and_then(|vs| vs.channel_id) == Some(self.channels.queue_vc))
      .unwrap_or(false)
  }

  pub fn get_session_player(&mut self, user_id: UI) -> Result<&mut SessionPlayer> {
    for format in &mut self.formats {
      for session in &mut format.sessions {
        if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
          return Ok(player);
        }
      }
    }
    Err(anyhow!("Player not found in any session"))
  }

  pub fn get_player(&mut self, user_id: UI) -> Result<Player> {
    match self.get_session_player(user_id) {
      Ok(session_player) => Ok(session_player.player.clone()),
      Err(e) => Err(e),
    }
  }

  pub async fn hot(&mut self, ctx: &Context, guild_id: Option<GI>, db: Option<&DB>, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    self.hot_fmt(0, ctx, guild_id, db, manager, false).await
  }

  pub async fn hot_fmt(&mut self, format_id: u8, ctx: &Context, guild_id: Option<GI>, db: Option<&DB>, manager: Option<Arc<Mutex<Manager>>>, post_game: bool) -> Result<(), Error> {
    info!("hot_fmt: starting format {} (post_game={})", format_id, post_game);
    // Pick the first full queue; other sessions may be waiting below quota for the next game
    let sg = self.format_mut(format_id).ok_or_else(|| anyhow!("Format {} not found", format_id))?;
    let quota = sg.quota as usize;
    let full_idx = sg
      .sessions
      .iter()
      .position(|s| s.is_idle() && s.pool.len() >= quota)
      .or_else(|| sg.sessions.iter().position(|s| s.is_hot() && s.pool.len() >= quota));
    let Some(full_idx) = full_idx else {
      // No queue has enough players, don't transition to Hot
      return Ok(());
    };
    let session = &mut sg.sessions[full_idx];

    // Check if session is already Hot to prevent duplicate notifications (race condition)
    let was_already_hot = session.is_hot();

    let _ = session.hot();
    info!("hot_fmt: format {} transitioned session to Hot ({} players)", format_id, session.pool.len());

    // Create team VCs if policy is OnHot
    if self.team_vc_settings.create_policy == TeamVcCreatePolicy::OnHot {
      if let Some(gid) = guild_id {
        if let Some(db) = db {
          if let Err(e) = self.ensure_team_vcs(ctx, gid, db).await {
            warn!("Failed to ensure team VCs on hot: {e}");
          }
        }
      }
    }

    // Refresh player ranks from Discord roles before generating teams
    if let (Some(gid), Some(database)) = (guild_id, db) {
      self.reload_player_ranks(ctx, gid, database).await;
    }

    // Only notify if session wasn't already Hot (prevents duplicate notifications from race condition)
    if !was_already_hot {
      // Notify requires guild_id for VC validation
      if let Some(gid) = guild_id {
        self.notify_fmt(format_id, ctx, gid, db, post_game).await;
      } else {
        warn!("Cannot notify: guild_id not provided");
      }
    } else {
      debug!("Skipping notification - session was already Hot (race condition prevented)");
    }

    // Generate teams - guild_id is required for dashboard updates
    if let Some(gid) = guild_id {
      self.generate_teams_fmt(format_id, ctx, gid, db).await;
    } else {
      warn!("Cannot generate teams: guild_id not provided");
    }

    // Spawn a targeted deadline timer for this hot session
    if let (Some(guild_id), Some(mgr)) = (guild_id, manager) {
      let category_id = self.id;
      let confirm_time = self.confirm_time;
      let ctx_clone = ctx.clone();

      // Get post-game timeout before spawning task
      let post_game_confirm_time = if let Some(database) = db { database.config.get_post_game_confirm_time(guild_id).await.ok() } else { None };

      tokio::spawn(async move {
        use tokio::time::{sleep, Duration};

        // Wait for the deadline (use category's configured timeout)
        sleep(Duration::from_secs(confirm_time as u64)).await;

        // Check if players have joined, remove those who haven't
        let mgr_for_rebalance = mgr.clone();
        let mut manager_lock = mgr.lock().await;
        if let Ok(server) = manager_lock.get_qguild(guild_id) {
          if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
            if category.check_hot_confirm_time(&ctx_clone, guild_id, post_game_confirm_time, Some(mgr_for_rebalance)).await {
              info!("Deadline timer fired: removed timed-out players from category {}", category_id);
              category.queue_dash_update(&ctx_clone, guild_id).await;
            }
          }
        }
      });
    }

    Ok(())
  }

  /// Keep the queues of a format within quota and start every queue that is full.
  ///
  /// Waiting players are packed into quota-sized queues (extras forming the next game) and
  /// each full queue is transitioned to Hot. Call this after any queue mutation - joins,
  /// leaves, removals, timeouts and match ends - so a queue never shows more players than
  /// the quota and a filled queue never sits idle.
  pub async fn rebalance_fmt(
    &mut self,
    fmt_id: u8,
    ctx: &Context,
    guild_id: Option<GI>,
    db: Option<&DB>,
    manager: Option<Arc<Mutex<Manager>>>,
    post_game: bool,
  ) -> Result<(), Error> {
    let full_queues = {
      let Some(sg) = self.format_mut(fmt_id) else {
        return Ok(());
      };
      if sg.pack_idle_sessions() {
        let layout: Vec<String> = sg.sessions.iter().map(|s| format!("{:?}({})", s.status, s.pool.len())).collect();
        info!("rebalance_fmt: packed format {} queues into {}", fmt_id, layout.join(", "));
      }
      let quota = sg.quota as usize;
      sg.sessions.iter().filter(|s| s.is_idle() && s.pool.len() >= quota).count()
    };
    for _ in 0..full_queues {
      self.hot_fmt(fmt_id, ctx, guild_id, db, manager.clone(), post_game).await?;
    }

    Ok(())
  }

  /// [`Self::rebalance`] as a boxed future.
  ///
  /// The hot deadline timer is spawned, so its future must be `Send`; boxing breaks the
  /// `hot_fmt -> check_hot_confirm_time -> rebalance -> hot_fmt` auto-trait inference cycle.
  fn rebalance_boxed<'a>(
    &'a mut self,
    ctx: &'a Context,
    guild_id: Option<GI>,
    db: Option<&'a DB>,
    manager: Option<Arc<Mutex<Manager>>>,
  ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>> {
    Box::pin(self.rebalance(ctx, guild_id, db, manager))
  }

  /// Rebalance every format of the category
  pub async fn rebalance(&mut self, ctx: &Context, guild_id: Option<GI>, db: Option<&DB>, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    let fmt_ids: Vec<u8> = self.formats.iter().map(|sg| sg.id).collect();
    for fmt_id in fmt_ids {
      self.rebalance_fmt(fmt_id, ctx, guild_id, db, manager.clone(), false).await?;
    }
    Ok(())
  }

  /// Check hot sessions for timeout and handle accordingly
  /// Returns true if any changes were made that require dashboard update
  pub async fn check_hot_confirm_time(&mut self, ctx: &Context, guild_id: GI, post_game_confirm_time: Option<u16>, manager: Option<Arc<Mutex<Manager>>>) -> bool {
    let mut changes_made = false;

    // Sync VC status with actual Discord state before making timeout decisions
    self.verify_vc(ctx, guild_id).await;

    debug!("check_hot_confirm_time: category confirm_time={}, post_game_confirm_time={:?}", self.confirm_time, post_game_confirm_time);

    // Check hot sessions across all formats
    for sg in &mut self.formats {
      let quota = sg.quota as usize;

      // Find hot sessions that have timed out
      let hot_sessions: Vec<usize> = sg
        .sessions
        .iter()
        .enumerate()
        .filter_map(|(idx, s)| {
          // Use post-game timeout if this is a post-game scenario and post_game_confirm_time is provided
          let confirm_time_seconds =
            if s.match_ended_at.is_some() { post_game_confirm_time.map(|t| t as u64).unwrap_or(self.confirm_time as u64) } else { self.confirm_time as u64 };

          debug!("Session {}: is_hot={}, match_ended={:?}, using confirm_time={}", idx, s.is_hot(), s.match_ended_at.is_some(), confirm_time_seconds);

          if s.is_hot_confirm_time(confirm_time_seconds) {
            Some(idx)
          } else {
            None
          }
        })
        .collect();

      for idx in hot_sessions {
        let session = &mut sg.sessions[idx];

        // Get players who are not in VC (timed out)
        let timed_out_players: Vec<_> = session.pool.iter().take(quota).filter(|p| !p.in_vc).collect();

        if timed_out_players.is_empty() {
          continue;
        }

        // Create list of player names for logging
        let player_names: Vec<String> = timed_out_players.iter().map(|p| p.player.tag.clone()).collect();

        let guild_name = guild_name(ctx, guild_id);
        let full_prefix = log_prefix_format(&guild_name, self.name.as_deref().unwrap_or("unknown"), &sg.name);

        info!("{} Removing {} timed-out players: {}", full_prefix, timed_out_players.len(), player_names.join(", "));

        // Remove timed out players - retain() preserves order of remaining elements
        let timed_out_ids: Vec<_> = timed_out_players.iter().map(|p| p.player.user_id).collect();
        session.pool.retain(|p| !timed_out_ids.contains(&p.player.user_id));

        // Check if we still have enough players after removals
        if session.pool.len() >= quota {
          info!("{} Regenerating teams after confirm time with {} players", full_prefix, session.pool.len());
          changes_made = true;
        } else {
          info!("{} Not enough players after confirm time, reverting to idle", full_prefix);
          session.idle();
          changes_made = true;
        }
      }
    }

    // If changes were made, regenerate teams for each format that still has a hot session
    if changes_made {
      let hot_fmt_ids: Vec<u8> = self.formats.iter().filter(|sg| sg.sessions.iter().any(|s| s.is_hot() && s.pool.len() >= sg.quota as usize)).map(|sg| sg.id).collect();
      for fmt_id in hot_fmt_ids {
        self.generate_teams_fmt(fmt_id, ctx, guild_id, None).await;
      }

      // Removals can leave a reverted queue short while players wait in the next one
      if let Err(e) = self.rebalance_boxed(ctx, Some(guild_id), None, manager).await {
        warn!("Failed to rebalance queues after confirm time: {e}");
      }
    }

    changes_made
  }

  pub async fn push(&mut self, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    self.push_fmt(0, ctx, guild_id, db, manager).await
  }

  pub async fn push_fmt(&mut self, format_id: u8, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    info!("push_fmt: starting format {}", format_id);
    // Clear any pending VC notifications since the game is starting
    self.clear_ready_notif(ctx, Some(db)).await;

    // Ensure a free team VC pair exists (creates one if needed)
    self.ensure_team_vcs(ctx, guild_id, db).await?;

    // Now find the free pair (recently freed pairs are available for reuse)
    let occupied_teams: Vec<TeamChannel> = self.actively_occupied_teams();

    let team_pair = self
      .channels
      .teams
      .iter()
      .find(|t| !occupied_teams.iter().any(|o| o.red_vc == t.red_vc && o.blu_vc == t.blu_vc))
      .cloned()
      .ok_or_else(|| anyhow!("No free team VC pair available after ensure"))?;

    let red_vc = team_pair.red_vc;
    let blu_vc = team_pair.blu_vc;

    // Get the hot/push game in the target format and collect player IDs for timeout cancellation
    // Note: Session may already be Push if called from dash_start (to prevent race conditions)
    let player_ids_for_queue_expiration: Vec<UI> = {
      let format = self.format(format_id).ok_or_else(|| anyhow!("Format {} not found for push", format_id))?;
      let game = format
        .sessions
        .iter()
        .find(|s| s.status == SessionStatus::Hot || s.status == SessionStatus::Push)
        .ok_or(anyhow!("No hot/push session found for push in format {}", format_id))?;
      game.pool.iter().map(|p| p.player.user_id).collect()
    };

    // Cancel timeouts for all players in this game (game is starting) in the background
    let category_id = self.id;
    let ctx_clone = ctx.clone();
    tokio::spawn(async move {
      use crate::models::QueueExpirationSchedulerKey;
      if let Some(scheduler) = ctx_clone.data.read().await.get::<QueueExpirationSchedulerKey>() {
        let mut sched = scheduler.lock().await;
        for user_id in player_ids_for_queue_expiration {
          sched.cancel_queue_expiration(guild_id, category_id, format_id, user_id);
        }
      }
    });

    // Now get mutable reference for the rest of the operation
    let sg = self.format_mut(format_id).ok_or_else(|| anyhow!("Format {} not found for push", format_id))?;
    let game = sg
      .sessions
      .iter_mut()
      .find(|s| s.status == SessionStatus::Hot || s.status == SessionStatus::Push)
      .ok_or(anyhow!("No hot/push session found for push in format {}", format_id))?;

    // Store the team channels on the session
    game.team_channels = Some(team_pair);

    // Set status to Push if not already (may already be Push from dash_start)
    if game.status != SessionStatus::Push {
      game.push();
    }

    let player_moves: Vec<(UI, CI, String)> = game
      .pool
      .iter()
      .filter_map(|player| {
        if !player.in_vc {
          return None;
        }

        match player.team {
          Some(crate::models::Team::Red) => Some((player.player.user_id, red_vc, player.player.tag.clone())),
          Some(crate::models::Team::Blu) => Some((player.player.user_id, blu_vc, player.player.tag.clone())),
          _ => None,
        }
      })
      .collect();

    // Move users to team channels in parallel using EditMember to avoid a per-user member fetch
    let _start_time = Instant::now();
    let _player_count = player_moves.len();
    let move_tasks: Vec<_> = player_moves
      .into_iter()
      .map(|(user_id, channel_id, tag)| {
        let http = ctx.http.clone();
        tokio::spawn(async move {
          let edit = EditMember::new().voice_channel(channel_id);
          let move_call = http.edit_member(guild_id, user_id, &edit, Some("Moving to team VC"));
          // A hung Discord HTTP call here would otherwise block push_fmt forever, holding
          // the category dispatch lock and freezing every other interaction on this category.
          let result = match tokio::time::timeout(std::time::Duration::from_secs(10), move_call).await {
            Ok(r) => r,
            Err(_) => Err(serenity::Error::Other("timed out after 10s")),
          };
          if let Err(ref e) = result {
            warn!("Failed to move user {}: {}", tag, e);
          }
          (tag, result)
        })
      })
      .collect();

    let mut moved_tags = Vec::new();
    for task in move_tasks {
      match task.await {
        Ok((tag, Ok(_))) => moved_tags.push(tag),
        Ok((tag, Err(e))) => warn!("Failed to move user {}: {}", tag, e),
        Err(e) => warn!("Move task panicked: {}", e),
      }
    }
    info!("Moved {} player(s) to team channels: {}", moved_tags.len(), moved_tags.join(", "));

    // Set game status to Live and extract overflow players
    // Capture category_id before mutably borrowing self
    let category_id = self.id;
    
    let sg = self.format_mut(format_id).unwrap();
    let quota = sg.quota as usize;
    let session_idx = sg.sessions.iter().position(|s| s.status == SessionStatus::Push).ok_or(anyhow!("Push session not found in format {}", format_id))?;
    let game = &mut sg.sessions[session_idx];
    game.status = SessionStatus::Live;
    game.started_at = Some(std::time::SystemTime::now());

    // Schedule dashboard update when cancel window expires (5 minutes)
    if let Some(mgr) = manager.clone() {
      let ctx_clone = ctx.clone();
      tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
        let mut manager_lock = mgr.lock().await;
        if let Ok(server) = manager_lock.get_qguild(guild_id) {
          if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
            category.queue_dash_update(&ctx_clone, guild_id).await;
            debug!("Dashboard updated after cancel window expired (5 minutes)");
          }
        }
      });
    }

    let game_pool_len = game.pool.len();

    // Extract overflow players (those beyond quota)
    let overflow_players: Vec<_> = if game_pool_len > quota { game.pool.drain(quota..).collect() } else { Vec::new() };
    
    // Backup the queue order AFTER removing overflow players (for potential cancellation)
    // This ensures pre_match_pool only contains players actually in this game
    game.pre_match_pool = Some(game.pool.clone());

    // Create new idle session for next game in this format (only if one doesn't exist)
    let has_idle = self.format(format_id).map(|sg| sg.sessions.iter().any(|s| s.is_idle())).unwrap_or(false);
    if !has_idle {
      self.create_session_format(format_id)?;
    }

    // Add overflow players to the idle session
    if !overflow_players.is_empty() {
      let overflow_count = overflow_players.len();
      let idle_session = self.get_queue_fmt(format_id).await?;
      for player in overflow_players {
        idle_session.pool.push(player);
      }
      info!("Moved {} overflow players to idle session in format {}", overflow_count, format_id);
    }

    // Clear recently freed teams since we're now using team channels
    self.recently_freed_teams.clear();

    // Clean up excess free team VCs (e.g., higher-numbered sets when a lower one is now in use)
    self.cleanup_team_vcs(ctx, true).await;

    // Pack the remaining queue and fire the next game if it is already full (concurrent games)
    self.rebalance_fmt(format_id, ctx, Some(guild_id), Some(db), manager, false).await?;

    self.queue_dash_update(ctx, guild_id).await;
    info!("push_fmt: completed format {}", format_id);
    Ok(())
  }

  /// Collect team channel pairs occupied by active sessions (excludes recently freed)
  /// Use this when looking for a free pair to reuse
  pub fn actively_occupied_teams(&self) -> Vec<TeamChannel> {
    self.formats.iter().flat_map(|sg| sg.sessions.iter()).filter(|s| s.is_active()).filter_map(|s| s.team_channels.clone()).collect()
  }

  /// Collect all occupied teams including recently freed (for cleanup - prevents deleting reserved pairs)
  pub fn all_occupied_teams(&self) -> Vec<TeamChannel> {
    let mut occupied = self.actively_occupied_teams();
    occupied.extend(self.recently_freed_teams.clone());
    occupied
  }

  /// Returns true if any users are currently connected to the given voice channel in Discord.
  pub fn has_players_in_vc(&self, ctx: &Context, channel_id: CI) -> bool {
    ctx.cache.guild(self.guild_id).map(|g| g.voice_states.values().any(|vs| vs.channel_id == Some(channel_id))).unwrap_or(false)
  }

  /// Returns true if any users are currently in either channel of a team pair.
  pub fn has_players_in_team(&self, ctx: &Context, team: &TeamChannel) -> bool {
    self.has_players_in_vc(ctx, team.red_vc) || self.has_players_in_vc(ctx, team.blu_vc)
  }

  /// Ensure at least one free team VC pair exists under the category.
  /// Called at the lifecycle point determined by `team_vc_settings.create_policy`.
  /// Returns the newly created pair, or None if a free pair already exists.
  pub async fn ensure_team_vcs(&mut self, ctx: &Context, guild_id: GI, db: &crate::Database) -> Result<Option<TeamChannel>, Error> {
    use serenity::all::{ChannelType, CreateChannel};

    info!("ensure_team_vcs: checking team VC availability for category {}", self.name.as_deref().unwrap_or("Unknown"));
    // Validate that team channels actually exist in Discord, removing any that were deleted
    let http_timeout = std::time::Duration::from_secs(10);
    let mut teams_to_remove = Vec::new();
    for tc in &self.channels.teams {
      // On timeout, assume the channel still exists rather than risk deleting a valid pair
      // due to a transient Discord API stall.
      let red_exists = tokio::time::timeout(http_timeout, ctx.http.get_channel(tc.red_vc)).await.map(|r| r.is_ok()).unwrap_or(true);
      let blu_exists = tokio::time::timeout(http_timeout, ctx.http.get_channel(tc.blu_vc)).await.map(|r| r.is_ok()).unwrap_or(true);
      if !red_exists || !blu_exists {
        warn!("Team channel pair #{} no longer exists in Discord (red: {}, blu: {}), removing from list", tc.set_index, red_exists, blu_exists);
        teams_to_remove.push(tc.clone());
      }
    }
    for tc in teams_to_remove {
      self.channels.teams.retain(|t| t.red_vc != tc.red_vc && t.blu_vc != tc.blu_vc);
      // Also remove from database
      let guild_name = crate::models::constants::guild_name(ctx, guild_id);
      let category_name = self.name.as_deref().unwrap_or("Unknown");
      if let Err(e) = db.teams.remove_team(guild_id, tc.red_vc, tc.blu_vc, &guild_name, category_name).await {
        warn!("Failed to remove deleted team channels from database: {}", e);
      }
    }

    // Check which team pairs are currently in active use (recently freed pairs are available for reuse)
    let occupied: Vec<TeamChannel> = self.actively_occupied_teams();

    // Check if there's already a free pair
    let has_free = self.channels.teams.iter().any(|t| !occupied.iter().any(|o| o.red_vc == t.red_vc && o.blu_vc == t.blu_vc));

    if has_free {
      info!("Found an empty set of team channels.");
      return Ok(None);
    } else {
      info!("No empty set of team channels found, creating a new set.");
    }

    // Create a new pair
    // Resolve the parent category: use channels.category if valid, otherwise
    // look up the queue VC's parent category from Discord
    let category = {
      let cat = self.channels.category;
      if let Some(ch) = ctx.cache.channel(cat) {
        if ch.kind == ChannelType::Category {
          cat
        } else if let Some(parent) = ch.parent_id {
          parent
        } else {
          // channels.category is not a category and has no parent - try queue VC
          ctx.cache.channel(self.channels.queue_vc).and_then(|qvc| qvc.parent_id).ok_or_else(|| anyhow!("No valid category found for team VC creation"))?
        }
      } else {
        // Not in cache - try queue VC's parent
        ctx.cache.channel(self.channels.queue_vc).and_then(|qvc| qvc.parent_id).ok_or_else(|| anyhow!("No valid category found for team VC creation"))?
      }
    };
    // Update stored category if it was wrong
    if category != self.channels.category {
      let new_name = ctx.cache.channel(category).map(|c| c.name.clone()).unwrap_or_else(|| category.to_string());
      let old_name = ctx.cache.channel(self.channels.category).map(|c| c.name.clone()).unwrap_or_else(|| self.channels.category.to_string());
      info!("Resolved team VC category to {} (was {})", new_name, old_name);
      self.channels.category = category;
    }

    let pair_num = self.channels.teams.len() + 1;

    // Create both team channels in parallel
    let _start_time = Instant::now();
    let (blu_result, red_result) = tokio::join!(
      tokio::time::timeout(http_timeout, guild_id.create_channel(&ctx.http, CreateChannel::new(format!("🔵 BLU #{}", pair_num)).kind(ChannelType::Voice).category(category))),
      tokio::time::timeout(http_timeout, guild_id.create_channel(&ctx.http, CreateChannel::new(format!("🔴 RED #{}", pair_num)).kind(ChannelType::Voice).category(category)))
    );

    let blu_ch = blu_result.map_err(|_| anyhow!("Timed out creating BLU VC"))?.map_err(|e| anyhow!("Failed to create BLU VC: {e}"))?;
    let red_ch = red_result.map_err(|_| anyhow!("Timed out creating RED VC"))?.map_err(|e| anyhow!("Failed to create RED VC: {e}"))?;
    info!("Created team channels #{}", pair_num);

    let pair = TeamChannel::new(red_ch.id, blu_ch.id, pair_num as u32);
    self.channels.teams.push(pair.clone());

    // Persist to database
    if let Err(e) = db.teams.add_team(guild_id, self.id, red_ch.id, blu_ch.id, pair_num as u32, None).await {
      warn!("Failed to persist team channels to database: {}", e);
    }

    // Log with user-friendly message
    let guild_name = crate::models::constants::guild_name(ctx, guild_id);
    let category_name = self.name.as_deref().unwrap_or("Unknown");
    let prefix = crate::log::log_prefix_category(&guild_name, category_name);

    info!("{} Added set {} of team channels to database.", prefix, pair_num);
    info!("ensure_team_vcs: completed for category {}", self.name.as_deref().unwrap_or("Unknown"));

    Ok(Some(pair))
  }

  /// Remove unused team VC pairs.
  /// When `force` is true, all free pairs are deleted (used by destroy policy triggers).
  /// When `force` is false, `keep_minimum` is respected (preserving at least one free pair).
  pub async fn cleanup_team_vcs(&mut self, ctx: &Context, force: bool) {
    // Collect occupied pairs from active sessions across all formats
    let session_occupied: Vec<TeamChannel> = self.all_occupied_teams();

    // Also check Discord voice states - any pair with actual users is occupied
    let discord_occupied: Vec<TeamChannel> = self.channels.teams.iter().filter(|tc| self.has_players_in_team(ctx, tc)).cloned().collect();

    // Merge both occupied sets
    let mut occupied = session_occupied;
    for tc in &discord_occupied {
      if !occupied.iter().any(|o| o.red_vc == tc.red_vc && o.blu_vc == tc.blu_vc) {
        occupied.push(tc.clone());
      }
    }

    // Partition into occupied and free
    let (keep, mut removable): (Vec<_>, Vec<_>) = self.channels.teams.iter().cloned().partition(|t| occupied.iter().any(|o| o.red_vc == t.red_vc && o.blu_vc == t.blu_vc));

    // Sort removable by set_index descending so higher-numbered sets are deleted first
    removable.sort_by(|a, b| b.set_index.cmp(&a.set_index));

    // If keep_minimum and not forced, preserve one free pair (the lowest-numbered one survives)
    let min_free = if !force && self.team_vc_settings.keep_minimum && keep.is_empty() { 1 } else { 0 };
    let to_delete_count = removable.len().saturating_sub(min_free);
    let to_delete: Vec<TeamChannel> = removable.drain(..to_delete_count).collect();

    // Delete all team VCs in parallel
    let _start_time = Instant::now();
    let delete_count = to_delete.len() * 2; // Each pair has RED + BLU
    let delete_tasks: Vec<_> = to_delete
      .iter()
      .flat_map(|tc| {
        let http = ctx.http.clone();
        let red_vc = tc.red_vc;
        let blu_vc = tc.blu_vc;
        let set_idx = tc.set_index;
        let red_name = ctx.cache.channel(red_vc).map(|c| c.name.clone()).unwrap_or_else(|| red_vc.to_string());
        let blu_name = ctx.cache.channel(blu_vc).map(|c| c.name.clone()).unwrap_or_else(|| blu_vc.to_string());
        vec![
          tokio::spawn(async move { (red_vc, red_vc.delete(&http).await, "RED", red_name, set_idx) }),
          tokio::spawn({
            let http = ctx.http.clone();
            async move { (blu_vc, blu_vc.delete(&http).await, "BLU", blu_name, set_idx) }
          }),
        ]
      })
      .collect();

    for task in delete_tasks {
      if let Ok((_, result, team, name, set_idx)) = task.await {
        if let Err(e) = result {
          let hint = if e.to_string().contains("Missing access") { "(Missing \"Manage channels\" permissions)" } else { "" };
          warn!("Failed to delete {} VC #{} ({}): {}{}", team, set_idx, name, e, hint);
        } else {
          info!("Deleted {} team VC #{} ({})", team, set_idx, name);
        }
      }
    }
    if delete_count > 0 {
      info!("Deleted {} team channels", delete_count);
    }

    // Rebuild teams list: occupied + remaining free
    let mut new_teams = keep;
    new_teams.extend(removable);
    self.channels.teams = new_teams;
  }

  /// Reconcile team VCs after a setting change.
  /// Creates VCs if keep_minimum is on and none exist, or cleans up if keep_minimum
  /// was turned off and no active games need them.
  pub async fn reconcile_team_vcs(&mut self, ctx: &Context, guild_id: GI, db: &DB) {
    let has_active = self.formats.iter().any(|sg| sg.sessions.iter().any(|s| s.is_active()));

    if self.team_vc_settings.keep_minimum && self.channels.teams.is_empty() && !has_active {
      // keep_minimum is on but no VCs exist - create a pair
      if let Err(e) = self.ensure_team_vcs(ctx, guild_id, db).await {
        warn!("Failed to create team VCs after setting change: {e}");
      }
    } else if !has_active {
      // No active games - clean up excess VCs (respects keep_minimum internally)
      self.cleanup_team_vcs(ctx, false).await;
    }
  }

  /// Called after a player leaves the queue. If the destroy policy is OnLastLeave
  /// and no idle sessions have players, clean up team VCs.
  pub async fn check_team_vc_cleanup_on_leave(&mut self, ctx: &Context) {
    if self.team_vc_settings.destroy_policy != TeamVcDestroyPolicy::OnLastLeave {
      return;
    }

    // Check if all idle sessions are empty (no queued players)
    let all_idle_empty = self.formats.iter().all(|sg| sg.sessions.iter().filter(|s| s.is_idle()).all(|s| s.pool.is_empty()));

    // Also check there are no active games
    let no_active = !self.formats.iter().any(|sg| sg.sessions.iter().any(|s| s.is_active()));

    if all_idle_empty && no_active {
      self.cleanup_team_vcs(ctx, false).await;
    }
  }

  /// Check if a channel is one of this category's team VCs
  pub fn is_team_vc(&self, channel_id: CI) -> bool {
    self.channels.teams.iter().any(|t| t.contains_channel(channel_id))
  }

  /// When a player leaves a team VC, check if both team VCs for any active
  /// session are now empty. If so, auto-end the game via pull.
  pub async fn check_team_vc_empty_auto_end(&mut self, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) {
    let guild = match ctx.cache.guild(guild_id) {
      Some(g) => g.clone(),
      None => return,
    };

    // Collect (format_id, session_index) of live sessions whose team VCs are empty
    let mut to_pull: Vec<u8> = Vec::new();

    for sg in &self.formats {
      for session in &sg.sessions {
        // Only auto-end Live or Push sessions — Pull is already being torn down by pull_fmt,
        // and triggering it again would race against the in-progress teardown.
        if !matches!(session.status, SessionStatus::Live | SessionStatus::Push) {
          continue;
        }
        // Skip sessions that are already being ended via a dashboard end-match
        // button. The dashboard handler runs pull_fmt on a cloned category
        // without holding the manager lock; marking score_reported here (in the
        // manager's copy) prevents this auto-end from racing with it and
        // creating a duplicate match-ready notification.
        if session.score_reported {
          continue;
        }
        let tc = match &session.team_channels {
          Some(tc) => tc,
          None => continue,
        };

        let red_count = guild.voice_states.values().filter(|vs| vs.channel_id == Some(tc.red_vc)).count();
        let blu_count = guild.voice_states.values().filter(|vs| vs.channel_id == Some(tc.blu_vc)).count();

        if red_count == 0 && blu_count == 0 {
          info!("All players left team VCs for format {}, auto-ending game", sg.id);
          to_pull.push(sg.id);
        }
      }
    }

    for fmt_id in to_pull {
      if let Err(e) = self.pull_fmt(fmt_id, None, ctx, guild_id, db, manager.clone()).await {
        warn!("Failed to auto-end game in format {}: {}", fmt_id, e);
      }
    }
  }

  pub async fn pull(&mut self, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    self.pull_fmt(0, None, ctx, guild_id, db, manager).await
  }

  /// Move a batch of users to a target voice channel, in parallel batches with a short delay
  /// between batches to avoid Discord client bugs when many users move at once.
  /// Returns the set of user IDs that were successfully moved (or already present).
  pub async fn move_users_to_vc(ctx: &Context, guild_id: GI, target_vc: CI, users: &[UI], tag_map: &std::collections::HashMap<UI, String>, reason: &str) -> std::collections::HashSet<UI> {
    let mut successfully_moved = std::collections::HashSet::new();

    for (batch_idx, batch) in users.chunks(VC_MOVE_BATCH_SIZE).enumerate() {
      // Add delay between batches (except before first batch)
      if batch_idx > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(VC_MOVE_BATCH_DELAY_MS)).await;
      }

      // Move this batch in parallel
      let move_tasks: Vec<_> = batch
        .iter()
        .map(|&user_id| {
          let http = ctx.http.clone();
          let gid = guild_id;
          let cache = ctx.cache.clone();
          let tag = tag_map.get(&user_id).cloned().unwrap_or_else(|| user_id.to_string());
          let reason = reason.to_string();
          tokio::spawn(async move {
            // Check if already in the target VC
            if let Some(guild) = cache.guild(gid) {
              if let Some(vs) = guild.voice_states.get(&user_id) {
                if vs.channel_id == Some(target_vc) {
                  info!("{} is already in target VC", tag);
                  return (user_id, true);
                }
              }
            }

            let edit = EditMember::new().voice_channel(target_vc);
            let move_call = http.edit_member(gid, user_id, &edit, Some(reason.as_str()));
            match tokio::time::timeout(std::time::Duration::from_secs(10), move_call).await {
              Ok(Ok(_)) => {
                info!("Moved {} to target VC", tag);
                (user_id, true)
              }
              Ok(Err(e)) => {
                warn!("Failed to move {} to target VC: {}", tag, e);
                (user_id, false)
              }
              Err(_) => {
                // A hung Discord HTTP call here would otherwise block the caller forever,
                // holding the category dispatch lock and freezing every other interaction
                // on this category (joins, other end-match clicks, etc).
                warn!("Timed out moving {} to target VC after 10s", tag);
                (user_id, false)
              }
            }
          })
        })
        .collect();

      // Wait for this batch to complete
      for task in move_tasks {
        if let Ok((user_id, success)) = task.await {
          if success {
            successfully_moved.insert(user_id);
          }
        }
      }
    }

    successfully_moved
  }

  /// Release a team VC pair after a game ends or is cancelled, following the category's
  /// configured destroy policy. If `quota_will_be_met` is true and the policy is AfterPull,
  /// the pair is kept in `recently_freed_teams` for immediate reuse instead of being torn down.
  pub async fn release_team_channel_pair(&mut self, ctx: &Context, guild_id: GI, manager: Option<Arc<Mutex<Manager>>>, team_channels: TeamChannel, quota_will_be_met: bool) {
    match self.team_vc_settings.destroy_policy {
      TeamVcDestroyPolicy::AfterPull => {
        if quota_will_be_met {
          self.recently_freed_teams.push(team_channels);
          debug!("Added team channels to recently_freed_teams for immediate reuse");
        } else {
          self.cleanup_team_vcs(ctx, true).await;
        }
      }
      TeamVcDestroyPolicy::AfterExpiration => {
        // Spawn a timer that cleans up team VCs if no new game starts
        if let Some(mgr) = manager.clone() {
          let category_id = self.id;
          let post_game_timeout_secs = self.confirm_time as u64;
          let ctx_clone = ctx.clone();

          tokio::spawn(async move {
            use tokio::time::{sleep, Duration};
            sleep(Duration::from_secs(post_game_timeout_secs)).await;

            let mut manager_lock = mgr.lock().await;
            if let Ok(server) = manager_lock.get_qguild(guild_id) {
              if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
                // Only clean up if no active games are running
                let has_active = category.formats.iter().any(|sg| sg.sessions.iter().any(|s| s.is_active()));
                if !has_active {
                  category.cleanup_team_vcs(&ctx_clone, true).await;
                }
              }
            }
          });
        }
      }
      _ => {} // OnLastLeave handled elsewhere
    }
  }

  /// `session_key` disambiguates which session to end when multiple sessions in the same
  /// format are active concurrently. It should be the `red_vc` channel ID of the target
  /// session's team channels. Pass `None` to fall back to the first Live (or Hot) session
  /// found, which preserves single-session behavior.
  pub async fn pull_fmt(&mut self, fmt_id: u8, session_key: Option<u64>, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    // Clear any pending VC notifications since the game is ending
    self.clear_ready_notif(ctx, Some(db)).await;

    // Extract queue vc channel ID
    let queue_vc = self.channels.queue_vc;

    // Find the active game to end - prefer Live sessions over Hot (Live games should be ended first)
    let sg = self.format_mut(fmt_id).ok_or_else(|| anyhow!("Format {} not found for pull", fmt_id))?;
    let active_session_idx = if let Some(key) = session_key {
      sg.sessions
        .iter()
        .position(|s| s.team_channels.as_ref().map(|tc| tc.red_vc.get()) == Some(key))
        .ok_or_else(|| anyhow!("Target session (key {}) not found for pull in format {}", key, fmt_id))?
    } else {
      sg.sessions
        .iter()
        .position(|s| s.status == SessionStatus::Live)
        .or_else(|| sg.sessions.iter().position(|s| s.status == SessionStatus::Hot))
        .ok_or(anyhow!("No active game to pull in format {}", fmt_id))?
    };
    // Capture a status summary before taking the mutable borrow on the target session.
    let session_summaries: Vec<String> = sg.sessions.iter().map(|s| format!("{:?}({})", s.status, s.pool.len())).collect();
    let game = &mut sg.sessions[active_session_idx];

    // Determine if this is a post-game scenario (game was Live, not just Hot)
    let post_game = game.status == SessionStatus::Live;
    let old_status = game.status;
    let game_pool_len = game.pool.len();

    info!(
      "pull_fmt: ending format {} session idx {} (status {:?}, {} players); current sessions: {}",
      fmt_id,
      active_session_idx,
      old_status,
      game_pool_len,
      session_summaries.join(", ")
    );

    game.pull();

    // Extract all players to move back to queue
    let mut players_to_requeue: Vec<Player> = game.pool.iter().map(|p| p.player.clone()).collect();

    // Shuffle the requeue order for variety
    {
      use rand::seq::SliceRandom;
      let mut rng = rand::rng();
      players_to_requeue.shuffle(&mut rng);
    } // RNG dropped here before async operations

    // Move everyone from team VCs back to queue (not just players)
    let guild = match ctx.cache.guild(guild_id) {
      Some(g) => g.clone(),
      None => return Ok(()),
    };

    // Collect all users in team VCs
    let mut users_to_move: Vec<UI> = Vec::new();

    // Add players from the game
    for player in &players_to_requeue {
      users_to_move.push(player.user_id);
    }

    // Add any other users in THIS game's team VCs (spectators, etc.) - they get moved to VC but NOT added to queue
    // Only scan the ending game's team channels, not all team channels (avoids pulling players from concurrent games)
    let mut spectators_to_move: Vec<UI> = Vec::new();
    if let Some(tc) = &game.team_channels {
      for vc_id in [tc.red_vc, tc.blu_vc] {
        let users_in_vc: Vec<_> = guild.voice_states.iter().filter(|(_, vs)| vs.channel_id == Some(vc_id)).map(|(uid, _)| *uid).collect();
        for user_id in users_in_vc {
          if !users_to_move.contains(&user_id) {
            spectators_to_move.push(user_id);
          }
        }
      }
    }

    // Combine players and spectators for VC move
    users_to_move.extend(spectators_to_move.iter().cloned());
    let player_ids: std::collections::HashSet<_> = players_to_requeue.iter().map(|p| p.user_id).collect();

    // Build tag lookup for readable log messages
    let tag_map: std::collections::HashMap<UI, String> = players_to_requeue.iter().map(|p| (p.user_id, p.tag.clone())).collect();

    let successfully_moved = Self::move_users_to_vc(ctx, guild_id, queue_vc, &users_to_move, &tag_map, "Moving user to queue VC").await;

    // Log spectators moved (they go to VC but not queue)
    let spectators_moved: Vec<_> = spectators_to_move.iter().filter(|uid| successfully_moved.contains(uid)).collect();
    if !spectators_moved.is_empty() {
      info!("Moved {} spectators to queue VC", spectators_moved.len());
    }

    // Check if quota will be met after re-queuing players to avoid unnecessary VC deletion/recreation
    // Only count actual players, not spectators
    let players_successfully_moved = successfully_moved.iter().filter(|uid| player_ids.contains(uid)).count();
    let quota_will_be_met = {
      let mut projected_count = 0;
      if let Some(idle_idx) = self.format_mut(fmt_id).unwrap().sessions.iter().position(|s| s.status == SessionStatus::Idle) {
        // Count existing players in idle session
        projected_count += self.formats[fmt_id as usize].sessions[idle_idx].pool.len();
      }
      // Add players who will be re-queued (those successfully moved) - only actual players
      projected_count += players_successfully_moved;
      projected_count >= self.quota() as usize
    };

    // Clear team_channels from the pulled session so cleanup/reuse sees the pair as free
    let team_channels = {
      let sg = self.format_mut(fmt_id).unwrap();
      sg.sessions[active_session_idx].team_channels.take()
    };

    if let Some(team_channels) = team_channels {
      self.release_team_channel_pair(ctx, guild_id, manager.clone(), team_channels, quota_will_be_met).await;
    }

    // Get quota before mutable borrows
    let quota = {
      let sg = self.format(fmt_id).unwrap();
      sg.quota as usize
    };

    // Filter to only players who were successfully moved
    let players_to_add: Vec<Player> = players_to_requeue
      .into_iter()
      .filter(|p| {
        if successfully_moved.contains(&p.user_id) {
          true
        } else {
          info!("Not re-queueing {} - they left voice before match ended", p.tag);
          false
        }
      })
      .collect();

    // Find or create the idle session (queue) and get current size
    // If pulling a Hot session (not post-game) and an Idle session already exists, create a new one
    // to avoid mixing players from the ended game with players waiting for the next game
    let (idle_session_idx, current_queue_size) = {
      let sg = self.format_mut(fmt_id).unwrap();
      let has_existing_idle = sg.sessions.iter().any(|s| s.status == SessionStatus::Idle);
      let idle_session_idx = if !post_game && has_existing_idle {
        // Pulling a Hot session with existing Idle - create new Idle for these players
        info!("Pulling hot session with existing Idle, creating new Idle session for re-queuing players in format {}", fmt_id + 1);
        sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
        sg.sessions.len() - 1
      } else {
        match sg.sessions.iter().position(|s| s.status == SessionStatus::Idle) {
          Some(idx) => idx,
          None => {
            // No idle session exists (game ended from Hot without push), create one
            info!("No idle session found, creating one for re-queuing players in format {}", fmt_id + 1);
            sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
            sg.sessions.len() - 1
          }
        }
      };
      let current_size = sg.sessions[idle_session_idx].pool.len();
      (idle_session_idx, current_size)
    };

    // Apply fatkid immunity if re-adding all players would exceed quota
    let total_after_readd = current_queue_size + players_to_add.len();

    if total_after_readd > quota {
      // Need to apply fatkid immunity - select who gets added
      let available_slots = quota.saturating_sub(current_queue_size);
      let (selected_players, fatkidded_players) = Self::select_players_with_fatkid_immunity(players_to_add, available_slots, guild_id, db).await?;

      // Record fatkid events for players who were not selected
      for player in &fatkidded_players {
        info!("Fatkidding {} - queue would exceed quota", player.tag);
        if let Err(e) = db.fatkids.record_fatkid(player.user_id, guild_id).await {
          warn!("Failed to record fatkid for {}: {}", player.tag, e);
        }
      }

      // Add selected players to queue first, then fatkidded players at the end
      let sg = self.format_mut(fmt_id).unwrap();
      let idle_session = &mut sg.sessions[idle_session_idx];
      idle_session.match_ended_at = Some(std::time::SystemTime::now());
      for player in selected_players {
        idle_session.add_player_in_vc(player);
      }
      // Add fatkidded players to the end of the queue (not removed, just moved to back)
      for player in fatkidded_players {
        idle_session.add_player_in_vc(player);
      }
    } else {
      // Queue has space for everyone, add them all
      let sg = self.format_mut(fmt_id).unwrap();
      let idle_session = &mut sg.sessions[idle_session_idx];
      idle_session.match_ended_at = Some(std::time::SystemTime::now());
      for player in players_to_add {
        idle_session.add_player_in_vc(player);
      }
    }

    // Remove the finished session
    let sg = self.format_mut(fmt_id).unwrap();
    let queue_size: usize = sg.sessions.iter().filter(|s| s.is_idle()).map(|s| s.pool.len()).sum();
    sg.sessions.retain(|s| s.status != SessionStatus::Pull);

    info!(
      "pull_fmt: removed pulled session from format {}; idle queue size is {}, remaining sessions: {}",
      fmt_id,
      queue_size,
      sg.sessions.iter().map(|s| format!("{:?}({})", s.status, s.pool.len())).collect::<Vec<_>>().join(", ")
    );

    // Pack the re-queued players into quota-sized queues (extras wait for the next game)
    // and transition every full queue to Hot
    let queue_filled = {
      let sg = self.format(fmt_id).ok_or_else(|| anyhow!("Format {} not found", fmt_id))?;
      sg.sessions.iter().filter(|s| s.is_idle()).map(|s| s.pool.len()).sum::<usize>() >= sg.quota as usize
    };
    if queue_filled {
      info!("pull_fmt: format {} meets quota after re-queue, transitioning to Hot", fmt_id);
      self.rebalance_fmt(fmt_id, ctx, Some(guild_id), Some(db), manager, true).await?;
    } else if post_game {
      // If this is post-game but quota isn't met, still notify players who are waiting
      // This is for the case where some players finished a game but not enough to start a new one
      self.notify_fmt(fmt_id, ctx, guild_id, Some(db), true).await; // true = post-game
    }

    self.queue_dash_update(ctx, guild_id).await;
    Ok(())
  }

  /// Select players for queue with fatkid immunity consideration
  /// Returns (selected_players, fatkidded_players)
  ///
  /// Selection priority:
  /// 1. Immune players sorted by immunity_level descending (most-fatkidded get priority)
  /// 2. Non-immune players sorted by immunity_level ascending (least-fatkidded get priority)
  async fn select_players_with_fatkid_immunity(players: Vec<Player>, available_slots: usize, guild_id: GI, db: &DB) -> Result<(Vec<Player>, Vec<Player>)> {
    use crate::models::fatkid_immunity;

    let mut players_with_immunity: Vec<(Player, fatkid_immunity::PlayerImmunityInfo)> = Vec::new();

    for player in &players {
      let info = fatkid_immunity::get_player_immunity_info(db, player.user_id, guild_id).await?;
      players_with_immunity.push((player.clone(), info));
    }

    // Log immunity status for each player
    for (player, info) in &players_with_immunity {
      debug!("  Fatkid immunity: {} immune={} level={}", player.tag, info.has_immunity, info.immunity_level);
    }

    // Separate into immune and non-immune groups
    let mut immune: Vec<(&Player, u32)> = players_with_immunity.iter().filter(|(_, info)| info.has_immunity).map(|(p, info)| (p, info.immunity_level)).collect();

    let mut non_immune: Vec<(&Player, u32)> = players_with_immunity.iter().filter(|(_, info)| !info.has_immunity).map(|(p, info)| (p, info.immunity_level)).collect();

    // Sort immune by level descending: players fatkidded most get priority for slots
    immune.sort_by(|a, b| b.1.cmp(&a.1));
    // Sort non-immune by level ascending: least-fatkidded get priority for remaining slots
    non_immune.sort_by_key(|(_, level)| *level);

    // Fill slots: immune first, then non-immune
    let mut selected_players: Vec<Player> = Vec::with_capacity(available_slots);

    for (player, _) in &immune {
      if selected_players.len() >= available_slots {
        break;
      }
      selected_players.push((*player).clone());
    }

    for (player, _) in &non_immune {
      if selected_players.len() >= available_slots {
        break;
      }
      selected_players.push((*player).clone());
    }

    info!(
      "  Fatkid selection: {}/{} immune, {} slots → {} selected, {} fatkidded",
      immune.len(),
      players_with_immunity.len(),
      available_slots,
      selected_players.len(),
      players_with_immunity.len() - selected_players.len()
    );

    // Determine fatkidded players (preserve original order)
    let selected_ids: std::collections::HashSet<_> = selected_players.iter().map(|p| p.user_id).collect();
    let fatkidded_players: Vec<Player> = players.into_iter().filter(|p| !selected_ids.contains(&p.user_id)).collect();

    Ok((selected_players, fatkidded_players))
  }

  /// Update player ranks from Discord roles for all players in the session
  pub async fn reload_player_ranks(&mut self, _ctx: &Context, guild_id: GI, db: &DB) {
    use crate::handlers::player::get_player_rank;

    for sg in &mut self.formats {
      for session in &mut sg.sessions {
        for player in &mut session.pool {
          if let Some(updated_rank) = get_player_rank(db, guild_id, player.player.user_id).await {
            player.player.rank = Some(updated_rank);
          }
        }
      }
    }
  }

  /// Validate and correct in_queue_vc flags against actual Discord voice states
  /// This prevents desync where cached flags don't match realitycd
  pub async fn verify_vc(&mut self, ctx: &Context, guild_id: GI) {
    // Get actual voice states from Discord
    let guild = match ctx.cache.guild(guild_id) {
      Some(g) => g,
      None => {
        warn!("Guild {} not in cache", guild_id);
        return;
      }
    };

    let queue_vc_id = self.channels.queue_vc.get();

    // Get set of users actually in queue VC
    let users_in_queue_vc: std::collections::HashSet<u64> =
      guild.voice_states.iter().filter_map(|(user_id, vs)| if vs.channel_id.map(|c| c.get()) == Some(queue_vc_id) { Some(user_id.get()) } else { None }).collect();

    // Update flags for all players in all sessions across all formats
    let mut corrected: Vec<String> = Vec::new();
    for sg in &mut self.formats {
      for session in &mut sg.sessions {
        // For active sessions with team channels (Push/Live), also check if players are in their team VCs.
        // Note: team_channels is only ever set once a session leaves Hot (see push_fmt), so this must
        // not be gated on is_hot() or team-VC players get falsely marked as missing.
        let team_vc_ids = session.team_channels.as_ref().map(|tc| (tc.red_vc.get(), tc.blu_vc.get()));

        for player in &mut session.pool {
          let user_id = player.player.user_id.get();
          
          // Player is in VC if they're in queue VC OR in their team VC (if session is Hot)
          let actual_in_vc = if let Some((red_vc, blu_vc)) = team_vc_ids {
            let in_team_vc = guild.voice_states.iter().any(|(uid, vs)| {
              uid.get() == user_id && (vs.channel_id.map(|c| c.get()) == Some(red_vc) || vs.channel_id.map(|c| c.get()) == Some(blu_vc))
            });
            users_in_queue_vc.contains(&user_id) || in_team_vc
          } else {
            users_in_queue_vc.contains(&user_id)
          };

          if player.in_vc != actual_in_vc {
            corrected.push(player.player.tag.clone());
            let old_value = player.in_vc;
            player.in_vc = actual_in_vc;
          }
        }
      }
    }
  }

  pub async fn generate_teams(&mut self, ctx: &Context, guild_id: GI, db: Option<&DB>) {
    self.generate_teams_fmt(0, ctx, guild_id, db).await;
  }

  pub async fn generate_teams_fmt(&mut self, fmt_id: u8, ctx: &Context, guild_id: GI, _db: Option<&DB>) {
    use itertools::Itertools;

    let sg = match self.format(fmt_id) {
      Some(sg) => sg,
      None => {
        warn!("Format {} not found for team generation", fmt_id);
        return;
      }
    };
    let quota = sg.quota as usize;

    // Get the hot game (session was just set to hot before this is called)
    let Some(session_idx) = sg.sessions.iter().position(|s| s.status == SessionStatus::Hot) else {
      warn!("No hot session found for team generation in format {}", fmt_id);
      return;
    };

    let sg = self.format_mut(fmt_id).unwrap();
    let game = &mut sg.sessions[session_idx];

    if game.pool.len() < quota {
      warn!("Not enough players for team generation: {}", game.pool.len());
      return;
    }

    // Extract player ELOs (use player's ELO or rank default)
    let mut players_with_elo: Vec<(usize, u32)> = Vec::new();
    for (idx, gp) in game.pool.iter().enumerate() {
      let elo = gp.player.elo as u32;
      players_with_elo.push((idx, elo));
    }

    // Balance exactly quota players (first N in queue)
    let pool_size = quota.min(game.pool.len());
    let players_to_balance: Vec<(usize, u32)> = players_with_elo.into_iter().take(pool_size).collect();

    // Score all possible team splits using BCH, then pick randomly from the top few
    // so that each shuffle produces a different but still well-balanced result.
    let team_size = pool_size / 2;
    let mut all_splits: Vec<(f64, Vec<usize>, Vec<usize>)> = Vec::new();

    for team_a_indices in (0..pool_size).combinations(team_size) {
      let team_b_indices: Vec<usize> = (0..pool_size).filter(|i| !team_a_indices.contains(i)).collect();

      // Get ratings for each team
      let team_a_elos: Vec<f64> = team_a_indices.iter().map(|&i| players_to_balance[i].1 as f64).collect();
      let team_b_elos: Vec<f64> = team_b_indices.iter().map(|&i| players_to_balance[i].1 as f64).collect();

      // Calculate statistics for both teams
      let (avg_a, med_a, std_a) = calculate_stats(&team_a_elos);
      let (avg_b, med_b, std_b) = calculate_stats(&team_b_elos);

      // BCH score: weighted sum prioritising average balance
      let score = 3.0 * (avg_a - avg_b).abs() + (med_a - med_b).abs() + (std_a - std_b).abs();

      all_splits.push((score, team_a_indices, team_b_indices));
    }

    if !all_splits.is_empty() {
      use rand::RngExt;

      // Sort best-first, then pick randomly from the top 5 to introduce variety
      all_splits.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
      let top_n = 5.min(all_splits.len());
      let mut rng = rand::rng();
      let chosen = rng.random_range(0..top_n);
      let (_, mut red_indices, mut blu_indices) = all_splits.remove(chosen);

      // Randomly flip the colour assignment for additional variety
      if rng.random_bool(0.5) {
        std::mem::swap(&mut red_indices, &mut blu_indices);
      }

      // Clear all team assignments first, then assign new ones
      // This ensures players pushed outside quota don't keep stale team assignments
      for player in game.pool.iter_mut() {
        player.team = None;
      }

      // Assign teams in-place to preserve in_queue_vc flag
      for &idx in &red_indices {
        let pool_idx = players_to_balance[idx].0;
        game.pool[pool_idx].set_team(crate::models::Team::Red);
      }

      for &idx in &blu_indices {
        let pool_idx = players_to_balance[idx].0;
        game.pool[pool_idx].set_team(crate::models::Team::Blu);
      }
    } else {
      warn!("Failed to generate balanced teams");
    }

    // Update dashboard to show the new teams
    self.queue_dash_update(ctx, guild_id).await;
  }

  pub async fn queue_player(&mut self, player: Player, rank: Rank, ctx: &Context, guild_id: Option<GI>, db: Option<&DB>, manager: Option<Arc<Mutex<Manager>>>) -> Result<()> {
    let queue_ctx = QueueContext { ctx, guild_id, db, manager };
    self.queue_player_fmt(player, rank, queue_ctx, false).await
  }

  pub async fn queue_player_fmt(&mut self, player: Player, _rank: Rank, queue_ctx: QueueContext<'_>, in_vc: bool) -> Result<()> {
    let was_empty = self.get_queue().await?.pool.is_empty();
    let session = self.get_queue().await?;

    let user_id = player.user_id;
    let _ply_tg = player.tag.clone();
    let player_queue_expiration = player.queue_expiration;
    let db = queue_ctx.db.unwrap();
    let _usr_prefs = db.players.get_prefs(user_id).await?;

    session.add_ply(player.clone(), in_vc)?;

    // Schedule timeout for this player
    if let Some(guild_id) = queue_ctx.guild_id {
      self.set_player_rejoin_expiration(queue_ctx.ctx, guild_id, player, player_queue_expiration).await;
    }

    // Create team VCs on first join if policy requires it
    if was_empty && self.team_vc_settings.create_policy == TeamVcCreatePolicy::OnFirstJoin {
      if let Some(gid) = queue_ctx.guild_id {
        if let Some(db) = queue_ctx.db {
          if let Err(e) = self.ensure_team_vcs(queue_ctx.ctx, gid, db).await {
            warn!("Failed to ensure team VCs on first join: {e}");
          }
        }
      }
    }

    self.rebalance_fmt(0, queue_ctx.ctx, queue_ctx.guild_id, queue_ctx.db, queue_ctx.manager, false).await?;
    Ok(())
  }

  pub async fn queue_player_with_vc_status_fmt(&mut self, fmt_id: u8, player: Player, _rank: Rank, queue_ctx: QueueContext<'_>, in_vc: bool) -> Result<()> {
    let session = self.get_queue_fmt(fmt_id).await?;
    let was_empty = session.pool.is_empty();
    let was_idle = session.is_idle();
    let was_hot = session.is_hot();

    let user_id = player.user_id;
    let _player_tag = player.tag.clone();
    let player_queue_expiration = player.queue_expiration;
    let db = queue_ctx.db.unwrap();
    let _user_prefs = db.players.get_prefs(user_id).await?;

    // Handle ping role assignment and DB consistency checks
    if let Some(guild_id) = queue_ctx.guild_id {
      let ctx = queue_ctx.ctx;
      let ping_role_str = db.config.get_ping_role_id(guild_id).await?;
      
      if let Some(ref role_str) = ping_role_str {
        if let Ok(role_id) = role_str.parse::<u64>() {
          let role_id = serenity::all::RoleId::new(role_id);
          
          // Get current DB preference
          let db_ping_enabled = db.user_server_prefs.get_ping_notification_enabled(user_id, guild_id).await.unwrap_or(None);
          
          // Get member from cache first to avoid HTTP calls on every join.
          // Unconditional HTTP member fetches on concurrent joins burst the rate limiter,
          // causing serenity to globally block all HTTP calls (including reply_acknowledge()).
          let member_cached = ctx.cache.guild(guild_id).and_then(|g| g.members.get(&user_id).cloned());
          if let Some(member) = member_cached {
            let has_role = member.roles.contains(&role_id);
            
            // Handle consistency checks and role assignment
            match (has_role, db_ping_enabled) {
              // User has role but DB shows 0 (opted out) - remove role
              (true, Some(false)) => {
                let _ = member.remove_role(&ctx.http, role_id).await;
                debug!("Removed ping role from user {} (DB shows opted out)", user_id);
              }
              // User doesn't have role and DB shows 1 (opted in) - add role
              (false, Some(true)) => {
                let _ = member.add_role(&ctx.http, role_id).await;
                debug!("Added ping role to user {} (DB shows opted in)", user_id);
              }
              // User doesn't have role and DB is NULL - give role and set DB to 1
              (false, None) => {
                let _ = member.add_role(&ctx.http, role_id).await;
                let _ = db.user_server_prefs.set_ping_notification_enabled(user_id, guild_id, Some(true)).await;
                debug!("Added ping role to user {} and set DB to opted in (was NULL)", user_id);
              }
              // User has role and DB is NULL - set DB to 1 (consistency)
              (true, None) => {
                let _ = db.user_server_prefs.set_ping_notification_enabled(user_id, guild_id, Some(true)).await;
                debug!("Set DB to opted in for user {} (has role, was NULL)", user_id);
              }
              // Other cases are consistent, no action needed
              _ => {}
            }
          }
        }
      }
    }

    let pool_before = session.pool.len();
    let position = session.add_ply(player.clone(), in_vc)?;
    info!(
      "queue_player_with_vc_status_fmt: user {} (id={} in_vc={} exp={}m) added to {:?} session (was_idle={} was_hot={} was_empty={}) at position {}/{} (pool before: {} after: {})",
      player.tag,
      player.user_id,
      in_vc,
      player_queue_expiration,
      session.status,
      was_idle,
      was_hot,
      was_empty,
      position,
      session.pool.len(),
      pool_before,
      session.pool.len()
    );

    // Schedule timeout for this player
    if let Some(guild_id) = queue_ctx.guild_id {
      self.set_player_rejoin_expiration(queue_ctx.ctx, guild_id, player, player_queue_expiration).await;
    }

    // Create team VCs on first join if policy requires it
    if was_empty && self.team_vc_settings.create_policy == TeamVcCreatePolicy::OnFirstJoin {
      if let Some(gid) = queue_ctx.guild_id {
        if let Some(db) = queue_ctx.db {
          if let Err(e) = self.ensure_team_vcs(queue_ctx.ctx, gid, db).await {
            warn!("Failed to ensure team VCs on first join: {e}");
          }
        }
      }
    }

    // Keep queues within quota (extras open the next game) and fire any queue that just filled
    self.rebalance_fmt(fmt_id, queue_ctx.ctx, queue_ctx.guild_id, queue_ctx.db, queue_ctx.manager, false).await?;

    Ok(())
  }

  /// Update queue VC name to show current count
  /// Filters out existing " n/n" pattern to avoid stacking
  ///
  /// Discord has a strict rate limit of 2 channel name changes per 10 minutes.
  /// To avoid hitting this limit, we parse the current name and skip updates if:
  /// - The displayed count hasn't changed
  ///   This prevents rate limit issues while keeping the name accurate.
  pub async fn update_queue_vc_name(&self, ctx: &Context, _guild_id: GI) {
    use serenity::all::EditChannel;

    let queue_vc = self.channels.queue_vc;

    // Get current queue count from idle sessions
    let current_count: usize = self.formats[0].sessions.iter().filter(|s| s.is_idle()).map(|s| s.pool.len()).sum();

    // Get current channel name

    let current_name = match queue_vc.name(&ctx.http).await {
      Ok(name) => name,
      Err(e) => {
        warn!("Failed to get channel name: {}", e);
        return;
      }
    };

    // Parse existing count from " n/n" pattern to check if update is needed
    let (base_name, displayed_count) = if let Some(idx) = current_name.rfind(' ') {
      let potential_suffix = &current_name[idx + 1..];
      // Check if it matches "n/n" pattern
      if potential_suffix.contains('/') {
        // Extract the first number (current count)
        let parts: Vec<&str> = potential_suffix.split('/').collect();
        if parts.len() == 2 {
          if let Ok(count) = parts[0].parse::<usize>() {
            (&current_name[..idx], Some(count))
          } else {
            (&current_name[..], None)
          }
        } else {
          (&current_name[..], None)
        }
      } else {
        (&current_name[..], None)
      }
    } else {
      (&current_name[..], None)
    };

    // Check if displayed count matches current count
    if let Some(displayed) = displayed_count {
      if displayed == current_count {
        return;
      }
    }

    // Build new name with count
    let new_name = format!("{} {}/{}", base_name, current_count, self.formats[0].quota);

    // Update the channel name
    if new_name != current_name {
      match ctx.http.edit_channel(queue_vc, &EditChannel::new().name(&new_name), Some("Update queue count")).await {
        Ok(_) => {}
        Err(e) => warn!("Failed to update channel name: {}", e),
      }
    }
  }

  pub async fn add_player(&mut self, session: &mut Session, player: Player, _rank: Rank, queue_ctx: &QueueContext<'_>, guild_id: GI) -> Result<()> {
    let user_id = player.user_id;
    let _player_tag = player.tag.clone();
    let player_queue_expiration = player.queue_expiration;

    session.add_ply(player.clone(), false)?;
    let db = queue_ctx.db.unwrap();
    let _user_prefs = db.players.get_prefs(user_id).await?;

    // Schedule timeout for this player
    self.set_player_rejoin_expiration(queue_ctx.ctx, guild_id, player, player_queue_expiration).await;

    self.queue_dash_update(queue_ctx.ctx, guild_id).await;
    Ok(())
  }

  /// Schedule a timeout task for a player
  pub async fn set_player_rejoin_expiration(&self, ctx: &Context, guild_id: GI, player: Player, rejoin_expiration_minutes: u8) {
    use crate::models::QueueExpirationSchedulerKey;

    if let Some(scheduler) = ctx.data.read().await.get::<QueueExpirationSchedulerKey>() {
      let mut sched = scheduler.lock().await;
      sched.schedule_queue_expiration(guild_id, self.id, self.formats[0].id, player, rejoin_expiration_minutes);
    }
  }

  /// Cancel a player's timeout task
  pub async fn cancel_player_rejoin_expiration(&self, ctx: &Context, guild_id: GI, format_id: u8, user_id: UI) {
    use crate::models::QueueExpirationSchedulerKey;

    if let Some(scheduler) = ctx.data.read().await.get::<QueueExpirationSchedulerKey>() {
      let mut sched = scheduler.lock().await;
      sched.cancel_queue_expiration(guild_id, self.id, format_id, user_id);
    }
  }

  /// Checks if this category contains the given channel_id in any of its channels
  pub fn contains_channel(&self, channel_id: CI) -> bool {
    self.channels.contains_channel(channel_id)
  }

  pub fn is_quota(&self) -> bool {
    self.is_quota_fmt(0)
  }

  /// Whether any queue of the format is full and ready to start
  pub fn is_quota_fmt(&self, fmt_id: u8) -> bool {
    let Some(sg) = self.format(fmt_id) else {
      return false;
    };
    sg.sessions.iter().any(|s| s.is_idle() && s.pool.len() >= sg.quota as usize)
  }

  /// Notifies the queue chat that quota has been met
  /// Pings ALL players in the first 'quota' players, not just those missing from VC
  /// Only pings the first 'quota' players, not extras queued for next match
  /// Also sends DMs to players who have pm_hot_alert=true
  ///
  /// If `post_game` is true, only pings players who are NOT in voice chat (to avoid pinging players who just finished)
  pub async fn notify(&mut self, ctx: &Context, guild_id: GI, db: Option<&DB>, post_game: bool) {
    self.notify_fmt(0, ctx, guild_id, db, post_game).await;
  }

  /// Notify players in a specific format that the game is ready
  /// Also sends DMs to players who have pm_hot_alert=true
  ///
  /// If `post_game` is true, only pings players who are NOT in voice chat (to avoid pinging players who just finished)
  pub async fn notify_fmt(&mut self, fmt_id: u8, ctx: &Context, guild_id: GI, db: Option<&DB>, post_game: bool) {
    // Validate VC status before sending notifications to prevent desync
    self.verify_vc(ctx, guild_id).await;
    let mut player_mentions = Vec::new();
    let mut players_to_dm = Vec::new();

    let Some(format) = self.format(fmt_id) else {
      warn!("Format {} not found when trying to notify players", fmt_id);
      return;
    };

    let quota = format.quota as usize;

    // Get the HOT session specifically, not the last session
    // This ensures we notify the correct players when quota is met
    if let Some(hot_session) = format.sessions.iter().find(|s| s.status == SessionStatus::Hot) {
      // Ping players based on post_game flag
      for player in hot_session.pool.iter().take(quota) {
        // In post-game scenarios, only ping players NOT in queue VC
        if post_game {
          // Check if player is specifically in the queue VC
          let in_queue_vc = if let Some(guild) = ctx.cache.guild(guild_id) {
            guild.voice_states.get(&player.player.user_id).and_then(|vs| vs.channel_id).map(|ch_id| ch_id == self.channels.queue_vc).unwrap_or(false)
          } else {
            false
          };

          // Only ping if NOT in queue VC
          if !in_queue_vc {
            player_mentions.push(format!("<@{}>", player.player.user_id));
            players_to_dm.push(player.player.user_id);
          }
        } else {
          // Normal pre-game behavior: only ping players NOT already in queue VC
          let in_queue_vc = if let Some(guild) = ctx.cache.guild(guild_id) {
            guild.voice_states.get(&player.player.user_id).and_then(|vs| vs.channel_id).map(|ch_id| ch_id == self.channels.queue_vc).unwrap_or(false)
          } else {
            false
          };

          // Only ping if NOT in queue VC
          if !in_queue_vc {
            player_mentions.push(format!("<@{}>", player.player.user_id));
            players_to_dm.push(player.player.user_id);
          }
        }
      }
    } else {
      warn!("No hot session found in format {} when trying to notify players", fmt_id);
      return;
    }

    // Only send notification if there are actually players to notify
    if !player_mentions.is_empty() {
      let guild_name = guild_name(ctx, guild_id);
      let fmt_name = &format.name;
      let full_prefix = log_prefix_format(&guild_name, self.name.as_deref().unwrap_or("unknown"), fmt_name);

      info!("{} Quota met - notifying all {} players in match", full_prefix, player_mentions.len());

      // Use embed for header and raw pings in message content to properly ping users
      let embed = CreateEmbed::new().title("PUG starting").description("Please join the queue channel!");

      let content = player_mentions.join(" ");
      let msg = CM::new().embed(embed).content(content);
      let dashboard = self.channels.dashboard;
      match tokio::time::timeout(tokio::time::Duration::from_secs(10), dashboard.send_message(&ctx.http, msg)).await {
        Ok(Ok(sent)) => {
        // Save notification to database
        if let Some(db) = db {
          let _ = db.game_ready_notifs.save_notification(dashboard.get(), sent.id.get()).await;
        }
        // Store message_id in-memory
        self.pending_vc_notification = Some(sent.id);
        // Store pending users in-memory
        self.pending_users = players_to_dm.clone();
        
        info!("{} Match ready notification created", full_prefix);

        // Delete the message after confirm expiry duration
        let http = ctx.http.clone();
        let channel_id = dashboard;
        let message_id = sent.id;
        let confirm_time = self.confirm_time;
        let log_prefix = full_prefix.clone();
        tokio::spawn(async move {
          tokio::time::sleep(tokio::time::Duration::from_secs(confirm_time as u64)).await;
          match channel_id.delete_message(&http, message_id).await {
            Ok(_) => {
              debug!("{} Match ready notification auto-deleted after timeout (msg_id: {})", log_prefix, message_id);
            }
            Err(e) => {
              debug!("{} Match ready notification already deleted or not found (msg_id: {}): {}", log_prefix, message_id, e);
            }
          }
        });
      }
      Ok(Err(e)) => warn!("{} Failed to send match ready notification: {}", full_prefix, e),
      Err(_) => warn!("{} Timed out sending match ready notification", full_prefix),
    }
  }

    // Send DMs to users who have pm_hot_alert=true in the background so end-match/queueing isn't blocked.
    if let Some(database) = db {
      let dm_tracker = ctx.data.read().await.get::<crate::models::DmTrackerKey>().cloned();

      if let Some(tracker) = dm_tracker {
        let ctx = ctx.clone();
        let database = database.clone();
        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "the server".to_string());

        for user_id in players_to_dm {
          let ctx = ctx.clone();
          let database = database.clone();
          let tracker = tracker.clone();
          let guild_name = guild_name.clone();

          tokio::spawn(async move {
            match database.players.get_pm_hot_alert(user_id).await {
              Ok(true) => {
                let dm_embed = CreateEmbed::new()
                  .title("PUG ready!")
                  .description(format!("A game is ready in **{}**!\nPlease join the queue channel.", guild_name))
                  .footer(serenity::all::CreateEmbedFooter::new("Don't want to be messaged directly? Press the button below"))
                  .color(GREEN);

                let disable_button = serenity::all::CreateButton::new("disable_dm_notifications").label("Disable DM notifications").style(serenity::all::ButtonStyle::Secondary);
                let components = vec![serenity::all::CreateActionRow::Buttons(vec![disable_button])];

                match tokio::time::timeout(
                  std::time::Duration::from_secs(10),
                  tracker.send_dm(&ctx, user_id, dm_embed, components)
                ).await {
                  Ok(Ok(_)) => {}
                  Ok(Err(e)) => warn!("Failed to send DM to user {}: {}", user_id, e),
                  Err(_) => warn!("Timed out sending DM to user {}", user_id),
                }
              }
              Ok(false) => {}
              Err(e) => warn!("Failed to check DM status for user {}: {}", user_id, e),
            }
          });
        }
      } else {
        warn!("DM tracker not available for hot alert DMs");
      }
    }
  }

  pub async fn move_user(&self, guild_id: GI, user_id: UI, channel_id: CI, ctx: &Context) -> Result<(), Error> {
    let member = guild_id.member(&ctx.http, user_id).await?;
    member.move_to_voice_channel(&ctx.http, channel_id).await?;
    Ok(())
  }

  /// Called when a player joins the queue VC. Updates/deletes the pending notification if applicable.
  pub async fn on_player_joined_vc(&mut self, ctx: &Context, user_id: UI, db: Option<&DB>) {
    if let Some(msg_id) = self.pending_vc_notification {
      // Check if notification exists in database before proceeding
      if let Some(db) = db {
        if !db.game_ready_notifs.notification_exists(self.channels.dashboard.get(), msg_id.get()).await.unwrap_or(false) {
          // Notification doesn't exist in database, clear in-memory state
          self.pending_vc_notification = None;
          self.pending_users.clear();
          return;
        }
      }
      
      // Check if this user was in the pending list
      if let Some(pos) = self.pending_users.iter().position(|&u| u == user_id) {
        self.pending_users.remove(pos);
        
        let user_tag = ctx.cache.user(user_id).map(|u| u.tag()).unwrap_or_else(|| user_id.to_string());

        let dashboard = self.channels.dashboard;

        if self.pending_users.is_empty() {
          self.clear_ready_notif(ctx, db).await;
        } else {
          // Edit the message to show remaining players
          let remaining_mentions: Vec<String> = self.pending_users.iter().map(|u| format!("<@{}>", u)).collect();
          let embed = CreateEmbed::new().title("PUG starting").description("Please join the queue channel!");
          let content = remaining_mentions.join(" ");

          let edit = serenity::all::EditMessage::new().embed(embed).content(content);
          match tokio::time::timeout(tokio::time::Duration::from_secs(10), dashboard.edit_message(&ctx.http, msg_id, edit)).await {
            Ok(Ok(_)) => {
              debug!("Match ready notification updated - {} joined, {} remaining (msg_id: {})", user_tag, self.pending_users.len(), msg_id);
            }
            Ok(Err(e)) => {
              warn!("Failed to edit match ready notification (msg_id: {}): {}", msg_id, e);
            }
            Err(_) => {
              warn!("Timed out editing match ready notification (msg_id: {})", msg_id);
            }
          }
        }
      }
    }
  }

  /// Clear the pending VC notification (e.g., when game starts or ends)
  pub async fn clear_ready_notif(&mut self, ctx: &Context, db: Option<&DB>) {
    if let Some(msg_id) = self.pending_vc_notification.take() {
      match tokio::time::timeout(tokio::time::Duration::from_secs(10), self.channels.dashboard.delete_message(&ctx.http, msg_id)).await {
        Ok(Ok(_)) => {
          info!("Match ready notification cleared - game starting (msg_id: {}, {} players still pending)", msg_id, self.pending_users.len());
        }
        Ok(Err(e)) => {
          debug!("Match ready notification already deleted (msg_id: {}): {}", msg_id, e);
        }
        Err(_) => {
          warn!("Timed out deleting match ready notification (msg_id: {})", msg_id);
        }
      }
      // Delete from database
      if let Some(db) = db {
        let _ = db.game_ready_notifs.delete_notification(self.channels.dashboard.get(), msg_id.get()).await;
      }
      // Clear in-memory fields
      self.pending_users.clear();
    }
  }
}

// Roles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roles {
  pub runner: RI,
  pub admin: RI,
}

impl Roles {
  pub fn new(runner: RI, admin: RI) -> Self {
    Self { runner, admin }
  }
  pub fn empty() -> Self {
    Self { runner: RI::new(1), admin: RI::new(1) }
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
  Runner,
  Admin,
}

impl Role {
  /// Get config key for this role's Discord role ID
  pub fn config_key(&self) -> &'static str {
    match self {
      Role::Runner => "runner_role",
      Role::Admin => "admin_role",
    }
  }

  /// Get the Discord role ID from database configuration (legacy single role)
  pub async fn id(&self, db: &DB, guild_id: GI) -> Option<RI> {
    let ids = self.ids(db, guild_id).await;
    ids.first().copied()
  }

  /// Get all Discord role IDs from database configuration (supports multiple roles)
  pub async fn ids(&self, db: &DB, guild_id: GI) -> Vec<RI> {
    match self {
      Role::Runner => {
        if let Ok(Some(role_id)) = db.config.get_runner_role_id(guild_id).await {
          vec![role_id]
        } else {
          Vec::new()
        }
      }
      Role::Admin => {
        if let Ok(Some(role_id)) = db.config.get_admin_role_id(guild_id).await {
          vec![role_id]
        } else {
          Vec::new()
        }
      }
    }
  }

  /// Save a Discord role ID to the database configuration
  pub async fn save_id(&self, db: &DB, guild_id: GI, role_id: RI) -> anyhow::Result<()> {
    match self {
      Role::Runner => db.config.set_runner_role_id(guild_id, role_id).await,
      Role::Admin => db.config.set_admin_role_id(guild_id, role_id).await,
    }
  }

  pub fn name(&self) -> &'static str {
    match self {
      Role::Runner => "Runner",
      Role::Admin => "Admin",
    }
  }
}

// Channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
  pub category: CI,
  pub queue_chat: CI,
  pub queue_vc: CI,
  pub ping_channel: CI,
  pub teams: Vec<TeamChannel>,
  pub dashboard: CI,
}

impl Channels {
  pub fn new(category: CI, queue_chat: CI, queue_vc: CI, ping_channel: CI, teams: Vec<TeamChannel>, dashboard: CI) -> Self {
    Self { category, queue_chat, queue_vc, ping_channel, teams, dashboard }
  }

  /// Pushs a red and blue channel to the vector
  pub fn add_team_channel_pair(&mut self, red_vc: CI, blu_vc: CI) {
    let set_index = self.teams.len() as u32 + 1;
    self.teams.push(TeamChannel::new(red_vc, blu_vc, set_index));
  }

  pub fn empty() -> Self {
    Self { category: CI::new(1), queue_chat: CI::new(1), queue_vc: CI::new(1), ping_channel: CI::new(1), teams: Vec::new(), dashboard: CI::new(1) }
  }

  /// Checks if this struct contains the given channel_id
  pub fn contains_channel(&self, channel_id: CI) -> bool {
    self.queue_chat == channel_id
      || self.queue_vc == channel_id
      || self.ping_channel == channel_id
      || self.dashboard == channel_id
      || self.teams.iter().any(|team| team.contains_channel(channel_id))
  }

  /// Returns all known static channel IDs (category, chat, queue, dashboard, team VCs)
  pub fn known_channel_ids(&self) -> Vec<CI> {
    let mut ids = vec![self.category, self.queue_chat, self.queue_vc, self.ping_channel, self.dashboard];
    for team in &self.teams {
      ids.push(team.red_vc);
      ids.push(team.blu_vc);
    }
    ids
  }
}
