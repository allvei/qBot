//! # Server Module
//!
//! This module defines the Server struct and its related functionality.
//! A Server represents a Discord guild with associated categories and games.

use std::time::{Instant, SystemTime};
use std::{sync::Arc, time::Duration};

use crate::{
  guild_name, log_prefix_format,
  models::constants::{DEFAULT_ACTIVE_ELO, MAX_QUEUE_EXPIRATION, MIN_QUEUE_EXPIRATION},
  Database as DB, Manager, Rank, GREEN,
};
use anyhow::{anyhow, Error, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId as CI, Context, CreateEmbed, CreateMessage as CM, EditMember, GuildId as GI, MessageId as MI, RoleId as RI, UserId as UI};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::models::{Player, Session, SessionPlayer, SessionStatus, TeamChannel};

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
}

impl Format {
  pub fn new(id: u8, name: String, quota: u8) -> Self {
    Self { id, name, quota, sessions: Vec::new(), connect_info: None }
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
  /// Last dashboard action (user_tag, action_description, timestamp)
  #[serde(skip)]
  pub last_action: Option<(String, String, SystemTime)>,
  /// Bot is restarting - hide join buttons
  #[serde(skip)]
  pub restarting: bool,
  /// Pending VC notification: (message_id, list of user_ids still needing to join)
  #[serde(skip)]
  pub pending_vc_notification: Option<(MI, Vec<UI>)>,
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
      last_action: None,
      restarting: false,
      pending_vc_notification: None,
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
    // Only prevent creation if there's an Idle session (not Hot)
    // Hot sessions can have overflow players that need a new Idle session
    let has_idle = sg.sessions.iter().any(|s| s.is_idle());
    if has_idle {
      return Err(anyhow!("Cannot create new session: idle session already exists"));
    }
    sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
    let sg = self.format_mut(fmt_id).unwrap();
    sg.sessions.last_mut().ok_or_else(|| anyhow!("Failed to create session"))
  }

  pub async fn get_queue(&mut self) -> Result<&mut Session, Error> {
    self.get_queue_fmt(0).await
  }

  pub async fn get_queue_fmt(&mut self, fmt_id: u8) -> Result<&mut Session, Error> {
    debug!("get_queue_fmt: fmt_id={}, total formats={}", fmt_id, self.formats.len());
    let sg = self.format_mut(fmt_id).ok_or_else(|| anyhow!("Format {} not found", fmt_id))?;
    debug!("get_queue_fmt: format found, total sessions={}, session statuses: {:?}", sg.sessions.len(), sg.sessions.iter().map(|s| format!("{:?}", s.status)).collect::<Vec<_>>());
    sg.sessions.iter_mut().find(|s| s.status == SessionStatus::Idle || s.status == SessionStatus::Hot).ok_or(anyhow!("No joinable session found in format {}", fmt_id))
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
          if !e.to_string().contains("Unknown Channel") {
            warn!("[{}] Failed to delete RED team VC {}: {}", guild.name, team.red_vc, e);
            surviving_teams.push(team.clone());
            continue;
          }
        }
      }
      if blu_exists {
        if let Err(e) = team.blu_vc.delete(&ctx.http).await {
          if !e.to_string().contains("Unknown Channel") {
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

  /// Check if a user is currently in the queue voice channel
  pub async fn is_user_in_queue_vc(&self, http: &serenity::all::Http, user_id: UI) -> bool {
    match self.guild_id.get_user_voice_state(http, user_id).await {
      Ok(voice_state) => voice_state.channel_id == Some(self.channels.queue_vc),
      Err(_) => false,
    }
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
    // Verify session has enough players before transitioning to Hot
    let quota = self.format(format_id).ok_or_else(|| anyhow!("Format {} not found", format_id))?.quota as usize;
    let session = self.get_queue_fmt(format_id).await?;

    if session.pool.len() < quota {
      // Not enough players, don't transition to Hot
      return Ok(());
    }

    let _ = session.hot();

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

    // Notify requires guild_id for VC validation
    if let Some(gid) = guild_id {
      self.notify_fmt(format_id, ctx, gid, db, post_game).await;
    } else {
      warn!("Cannot notify: guild_id not provided");
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
        let mut manager_lock = mgr.lock().await;
        if let Ok(server) = manager_lock.get_qguild(guild_id) {
          if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
            if category.check_hot_confirm_time(&ctx_clone, guild_id, post_game_confirm_time).await {
              info!("Deadline timer fired: removed timed-out players from category {}", category_id);
              category.queue_dash_update(&ctx_clone, guild_id).await;
            }
          }
        }
      });
    }

    Ok(())
  }

  /// Check hot sessions for timeout and handle accordingly
  /// Returns true if any changes were made that require dashboard update
  pub async fn check_hot_confirm_time(&mut self, ctx: &Context, guild_id: GI, post_game_confirm_time: Option<u16>) -> bool {
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
    }

    changes_made
  }

  /// Check idle sessions for timeout timeouts and handle accordingly
  /// Returns true if any changes were made that require dashboard update
  pub async fn check_confirm_time(&mut self, db: &DB, ctx: &Context, guild_id: GI) -> bool {
    let mut changes_made = false;

    // Collect all active game players first (outside the format loop)
    let mut active_game_players = std::collections::HashSet::new();
    let mut player_tags = std::collections::HashMap::new();

    for fmt in &self.formats {
      for session in &fmt.sessions {
        if session.status == SessionStatus::Live || session.status == SessionStatus::Hot {
          for player in &session.pool {
            active_game_players.insert(player.player.user_id);
            player_tags.insert(player.player.user_id, (player.player.tag.clone(), fmt.name.clone()));
          }
        }
      }
    }

    // Check idle sessions across all formats (not hot/push/live)
    for fmt in &mut self.formats {
      for session in fmt.sessions.iter_mut() {
        if !session.is_idle() {
          continue;
        }

        let mut players_to_remove = Vec::new();

        // First, check for expired VC leave grace periods
        for player in &session.pool {
          if let Some(grace_until) = player.vc_leave_grace_until {
            let now = SystemTime::now();
            if now >= grace_until {
              // Calculate how long the grace period actually was
              let elapsed = if let Ok(duration) = now.duration_since(grace_until) { format!("{}s ago", duration.as_secs()) } else { "unknown".to_string() };
              info!("VC leave grace period expired for {} (expired {}), removing from queue", player.player.tag, elapsed);
              players_to_remove.push(player.player.user_id);
            }
          }
        }

        // Then check regular timeouts

        for player in &session.pool {
          let queue_expiration = player.queue_expiration;

          // Clamp to valid range (EXPIRY_MIN to EXPIRY_MAX)
          let expiry_mins = queue_expiration.clamp(MIN_QUEUE_EXPIRATION, MAX_QUEUE_EXPIRATION);

          // Skip if timeout is disabled (below EXPIRY_MIN)
          if expiry_mins < MIN_QUEUE_EXPIRATION {
            continue;
          }

          // Check if player has exceeded their timeout time
          if let Ok(elapsed) = SystemTime::now().duration_since(player.joined_at) {
            if elapsed.as_secs() >= Duration::from_mins(expiry_mins as u64).as_secs() {
              let guild_name = crate::models::constants::guild_name(ctx, guild_id);
              let ctg_nm = self.name.as_deref().unwrap_or("Unknown");
              let fmt_nm = &fmt.name;
              info!("{} Timeout {} after {}m", crate::log::log_prefix_format(&guild_name, ctg_nm, fmt_nm), player.player.tag, (elapsed.as_secs() as f64 / 60.0).round());
              players_to_remove.push(player.player.user_id);
            }
          }
        }

        if !players_to_remove.is_empty() {
          // Remove the timed-out players
          for user_id in &players_to_remove {
            if let Some((_tag, _fmt_name)) = player_tags.get(user_id) {
              // Player is in an active game, don't remove them
            } else {
              // Remove the player
              session.remove_player(*user_id);

              // Optionally: disconnect from VC if vc_kick is enabled
              if let Ok(settings) = db.players.get_prefs(*user_id).await {
                if settings.vc_auto_leave {
                  if let Ok(member) = guild_id.member(&ctx.http, *user_id).await {
                    if let Err(e) = member.disconnect_from_voice(&ctx.http).await {
                      warn!("Failed to disconnect timed-out player from VC: {e}");
                    }
                  }
                }
              }
            }
          }
        }
        changes_made = true;
      }
    }

    changes_made
  }

  pub async fn push(&mut self, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    self.push_fmt(0, ctx, guild_id, db, manager).await
  }

  pub async fn push_fmt(&mut self, format_id: u8, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
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

    // Get the hot game in the target format and collect player IDs for timeout cancellation
    let player_ids_for_queue_expiration: Vec<UI> = {
      let format = self.format(format_id).ok_or_else(|| anyhow!("Format {} not found for push", format_id))?;
      let game = format.sessions.iter().find(|s| s.status == SessionStatus::Hot).ok_or(anyhow!("No hot session found for push in format {}", format_id))?;
      game.pool.iter().map(|p| p.player.user_id).collect()
    };

    // Cancel timeouts for all players in this game (game is starting)
    for user_id in player_ids_for_queue_expiration {
      self.cancel_player_rejoin_expiration(ctx, guild_id, format_id, user_id).await;
    }

    // Now get mutable reference for the rest of the operation
    let sg = self.format_mut(format_id).ok_or_else(|| anyhow!("Format {} not found for push", format_id))?;
    let game = sg.sessions.iter_mut().find(|s| s.status == SessionStatus::Hot).ok_or(anyhow!("No hot session found for push in format {}", format_id))?;

    // Store the team channels on the session
    game.team_channels = Some(team_pair);

    // Set status to Push and extract player moves
    game.push();

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

    // Move users to team channels in parallel
    let _start_time = Instant::now();
    let _player_count = player_moves.len();
    let move_tasks: Vec<_> = player_moves
      .into_iter()
      .map(|(user_id, channel_id, tag)| {
        let http = ctx.http.clone();
        tokio::spawn(async move {
          let result = async {
            let member = guild_id.member(&http, user_id).await?;
            member.move_to_voice_channel(&http, channel_id).await
          }
          .await;
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
    let sg = self.format_mut(format_id).unwrap();
    let quota = sg.quota as usize;
    let session_idx = sg.sessions.iter().position(|s| s.status == SessionStatus::Push).ok_or(anyhow!("Push session not found in format {}", format_id))?;
    let game = &mut sg.sessions[session_idx];
    game.live();

    let game_pool_len = game.pool.len();

    // Extract overflow players (those beyond quota)
    let overflow_players: Vec<_> = if game_pool_len > quota { game.pool.drain(quota..).collect() } else { Vec::new() };

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

    // Check if the new idle session already has enough players for another game (concurrent games)
    if self.is_quota_fmt(format_id) {
      info!("Overflow players met quota for format {} - firing next game", format_id);
      self.hot_fmt(format_id, ctx, Some(guild_id), Some(db), manager, false).await?;
    }

    self.queue_dash_update(ctx, guild_id).await;
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

    // Validate that team channels actually exist in Discord, removing any that were deleted
    let mut teams_to_remove = Vec::new();
    for tc in &self.channels.teams {
      let red_exists = ctx.http.get_channel(tc.red_vc).await.is_ok();
      let blu_exists = ctx.http.get_channel(tc.blu_vc).await.is_ok();
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
      guild_id.create_channel(&ctx.http, CreateChannel::new(format!("🔵 BLU #{}", pair_num)).kind(ChannelType::Voice).category(category)),
      guild_id.create_channel(&ctx.http, CreateChannel::new(format!("🔴 RED #{}", pair_num)).kind(ChannelType::Voice).category(category))
    );

    let blu_ch = blu_result.map_err(|e| anyhow!("Failed to create BLU VC: {e}"))?;
    let red_ch = red_result.map_err(|e| anyhow!("Failed to create RED VC: {e}"))?;
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
          let hint = if e.to_string().contains("Missing Access") { "(Missing \"Manage Channels\" permissions)" } else { "" };
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
        if !session.is_active() {
          continue;
        }
        let tc = match &session.team_channels {
          Some(tc) => tc,
          None => continue,
        };

        let red_count = guild.voice_states.values().filter(|vs| vs.channel_id == Some(tc.red_vc)).count();
        let blu_count = guild.voice_states.values().filter(|vs| vs.channel_id == Some(tc.blu_vc)).count();

        if red_count == 0 && blu_count == 0 {
          info!("All players left team VCs (RED={} BLU={}) for format {}, auto-ending game", tc.red_vc, tc.blu_vc, sg.id);
          to_pull.push(sg.id);
        }
      }
    }

    for fmt_id in to_pull {
      if let Err(e) = self.pull_fmt(fmt_id, ctx, guild_id, db, manager.clone()).await {
        warn!("Failed to auto-end game in format {}: {}", fmt_id, e);
      }
    }
  }

  pub async fn pull(&mut self, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    self.pull_fmt(0, ctx, guild_id, db, manager).await
  }

  pub async fn pull_fmt(&mut self, fmt_id: u8, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
    // Extract queue vc channel ID
    let queue_vc = self.channels.queue_vc;

    // Find the active game to end - prefer Live sessions over Hot (Live games should be ended first)
    let sg = self.format_mut(fmt_id).ok_or_else(|| anyhow!("Format {} not found for pull", fmt_id))?;
    let active_session_idx = sg
      .sessions
      .iter()
      .position(|s| s.status == SessionStatus::Live)
      .or_else(|| sg.sessions.iter().position(|s| s.status == SessionStatus::Hot))
      .ok_or(anyhow!("No active game to pull in format {}", fmt_id))?;
    let game = &mut sg.sessions[active_session_idx];

    // Determine if this is a post-game scenario (game was Live, not just Hot)
    let post_game = game.status == SessionStatus::Live;

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

    // Move all users to queue VC in parallel
    let move_tasks: Vec<_> = users_to_move
      .into_iter()
      .map(|user_id| {
        let http = ctx.http.clone();
        let gid = guild_id;
        let qvc = queue_vc;
        let cache = ctx.cache.clone();
        let tag = tag_map.get(&user_id).cloned().unwrap_or_else(|| user_id.to_string());
        tokio::spawn(async move {
          // Check if already in queue VC
          if let Some(guild) = cache.guild(gid) {
            if let Some(vs) = guild.voice_states.get(&user_id) {
              if vs.channel_id == Some(qvc) {
                info!("User {} already in queue VC", tag);
                return (user_id, true);
              }
            }
          }

          match http.edit_member(gid, user_id, &EditMember::new().voice_channel(qvc), Some("Moving user back to queue VC after game end")).await {
            Ok(_) => {
              info!("Moved user {} back to queue VC after game end", tag);
              (user_id, true)
            }
            Err(e) => {
              warn!("Failed to move user {} back to queue VC: {}", tag, e);
              (user_id, false)
            }
          }
        })
      })
      .collect();

    let mut successfully_moved = std::collections::HashSet::new();
    for task in move_tasks {
      if let Ok((user_id, success)) = task.await {
        if success {
          successfully_moved.insert(user_id);
        }
      }
    }

    // Log spectators moved (they go to VC but not queue)
    let spectators_moved: Vec<_> = spectators_to_move.iter().filter(|uid| successfully_moved.contains(uid)).collect();
    if !spectators_moved.is_empty() {
      info!("Moved {} spectators to queue VC (not added to queue)", spectators_moved.len());
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

    // Handle team channels based on whether we'll cleanup or not
    let skip_cleanup = quota_will_be_met && self.team_vc_settings.destroy_policy == TeamVcDestroyPolicy::AfterPull;

    if skip_cleanup {
      // Add team channels to recently_freed_teams to prevent immediate recreation
      let sg = self.format_mut(fmt_id).unwrap();
      let team_channels = sg.sessions[active_session_idx].team_channels.clone();
      // Clear from pulled session first
      sg.sessions[active_session_idx].team_channels = None;
      // Then add to recently freed teams
      if let Some(team_channels) = team_channels {
        self.recently_freed_teams.push(team_channels);
        debug!("Added team channels to recently_freed_teams for immediate reuse");
      }
    } else {
      // Clear team_channels from the pulled session so cleanup sees them as free
      let sg = self.format_mut(fmt_id).unwrap();
      sg.sessions[active_session_idx].team_channels = None;
    }

    // Clean up team VCs based on destroy policy, but avoid cleanup if quota will be met and policy is AfterPull
    match self.team_vc_settings.destroy_policy {
      TeamVcDestroyPolicy::AfterPull => {
        // Only clean up if quota won't be immediately met again (to avoid delete/recreate cycle)
        if !quota_will_be_met {
          self.cleanup_team_vcs(ctx, true).await;
        } else {
          debug!("Skipping team VC cleanup - quota will be met again, avoiding unnecessary delete/recreate");
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
        info!("Pulling Hot session with existing Idle, creating new Idle session for re-queuing players in format {}", fmt_id + 1);
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
    sg.sessions.retain(|s| s.status != SessionStatus::Pull);

    // Check if the queue now meets quota and transition to Hot if needed
    if self.is_quota_fmt(fmt_id) {
      self.hot_fmt(fmt_id, ctx, Some(guild_id), Some(db), manager, true).await?;
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
  /// This prevents desync where cached flags don't match reality
  pub async fn verify_vc(&mut self, ctx: &Context, guild_id: GI) {
    // Get actual voice states from Discord
    let guild = match ctx.cache.guild(guild_id) {
      Some(g) => g,
      None => {
        warn!("[validate_vc_status] Guild {} not in cache", guild_id);
        return;
      }
    };

    let queue_vc_id = self.channels.queue_vc.get();

    // Get set of users actually in queue VC
    let users_in_vc: std::collections::HashSet<u64> =
      guild.voice_states.iter().filter_map(|(user_id, vs)| if vs.channel_id.map(|c| c.get()) == Some(queue_vc_id) { Some(user_id.get()) } else { None }).collect();

    // Update flags for all players in all sessions across all formats
    let mut corrected: Vec<String> = Vec::new();
    for sg in &mut self.formats {
      for session in &mut sg.sessions {
        for player in &mut session.pool {
          let user_id = player.player.user_id.get();
          let actual_in_vc = users_in_vc.contains(&user_id);

          if player.in_vc != actual_in_vc {
            corrected.push(player.player.tag.clone());
            let old_value = player.in_vc;
            player.in_vc = actual_in_vc;
            info!("[validate_vc_status] Corrected in_queue_vc for player {} (was {}, now {})", player.player.tag, old_value, actual_in_vc);
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

    // Generate all possible team splits using BCH (deterministic)
    let team_size = pool_size / 2;
    let mut best_split: Option<(Vec<usize>, Vec<usize>)> = None;
    let mut best_score = f64::INFINITY;

    for team_a_indices in (0..pool_size).combinations(team_size) {
      let team_b_indices: Vec<usize> = (0..pool_size).filter(|i| !team_a_indices.contains(i)).collect();

      // Get ELOs for each team
      let team_a_elos: Vec<f64> = team_a_indices.iter().map(|&i| players_to_balance[i].1 as f64).collect();
      let team_b_elos: Vec<f64> = team_b_indices.iter().map(|&i| players_to_balance[i].1 as f64).collect();

      // Calculate statistics for both teams
      let (avg_a, med_a, std_a) = calculate_stats(&team_a_elos);
      let (avg_b, med_b, std_b) = calculate_stats(&team_b_elos);

      // BCH score: weighted sum prioritizing average ELO balance
      // Average is weighted 3x higher because it directly determines team strength
      let score = 3.0 * (avg_a - avg_b).abs() + (med_a - med_b).abs() + (std_a - std_b).abs();

      if score < best_score {
        best_score = score;
        best_split = Some((team_a_indices, team_b_indices));
      }
    }

    if let Some((mut red_indices, mut blu_indices)) = best_split {
      use rand::seq::SliceRandom;
      use std::collections::HashMap;

      // Randomize by swapping players with the same ELO
      // Category players by ELO, then shuffle within each ELO category
      let mut rng = rand::rng();

      // Create map of ELO -> Vec<indices in players_to_balance>
      let mut elo_categories: HashMap<u32, Vec<usize>> = HashMap::new();
      for (i, player) in players_to_balance.iter().enumerate().take(pool_size) {
        let elo = player.1;
        elo_categories.entry(elo).or_default().push(i);
      }

      // Store original team assignments before shuffling
      let original_red = red_indices.clone();
      let original_blu = blu_indices.clone();

      // For each ELO category with multiple players, shuffle them across teams
      for indices in elo_categories.values_mut() {
        if indices.len() > 1 {
          // Count how many of this ELO are on each team (using ORIGINAL assignments)
          let red_count = indices.iter().filter(|&&i| original_red.contains(&i)).count();
          let _blu_count = indices.iter().filter(|&&i| original_blu.contains(&i)).count();

          // Shuffle the indices with this ELO
          indices.shuffle(&mut rng);

          // Reassign to teams with the same distribution
          red_indices.retain(|&i| !indices.contains(&i));
          blu_indices.retain(|&i| !indices.contains(&i));

          red_indices.extend_from_slice(&indices[..red_count]);
          blu_indices.extend_from_slice(&indices[red_count..]);
        }
      }

      // Clear all team assignments first, then assign new ones
      // This ensures players pushed outside quota don't keep stale team assignments
      for player in game.pool.iter_mut() {
        player.team = None;
      }

      // Assign teams in-place to preserve in_queue_vc flag
      // First assign red team
      for &idx in &red_indices {
        let pool_idx = players_to_balance[idx].0;
        game.pool[pool_idx].set_team(crate::models::Team::Red);
      }

      // Then assign blue team
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

    if self.is_quota() {
      self.hot(queue_ctx.ctx, queue_ctx.guild_id, queue_ctx.db, queue_ctx.manager).await?;
    }
    Ok(())
  }

  pub async fn queue_player_with_vc_status_fmt(&mut self, fmt_id: u8, player: Player, _rank: Rank, queue_ctx: QueueContext<'_>, in_vc: bool) -> Result<()> {
    debug!("queue_player_with_vc_status_fmt: user_id={}, tag={}, fmt_id={}, in_vc={}", player.user_id, player.tag, fmt_id, in_vc);
    let session = self.get_queue_fmt(fmt_id).await?;
    debug!("queue_player_with_vc_status_fmt: got session, current pool size={}", session.pool.len());
    let was_empty = session.pool.is_empty();
    let was_idle = session.is_idle();
    let was_hot = session.is_hot();

    let user_id = player.user_id;
    let _player_tag = player.tag.clone();
    let player_queue_expiration = player.queue_expiration;
    let db = queue_ctx.db.unwrap();
    let _user_prefs = db.players.get_prefs(user_id).await?;
    debug!("queue_player_with_vc_status_fmt: user_prefs loaded, calling add_ply");

    // Handle ping role assignment and DB consistency checks
    if let Some(guild_id) = queue_ctx.guild_id {
      let ctx = queue_ctx.ctx;
      let ping_role_str = db.config.get_ping_role_id(guild_id).await?;
      
      if let Some(ref role_str) = ping_role_str {
        if let Ok(role_id) = role_str.parse::<u64>() {
          let role_id = serenity::all::RoleId::new(role_id);
          
          // Get current DB preference
          let db_ping_enabled = db.user_server_prefs.get_ping_notification_enabled(user_id, guild_id).await.unwrap_or(None);
          
          // Get member to check current role status
          if let Ok(member) = guild_id.member(&ctx.http, user_id).await {
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

    session.add_ply(player.clone(), in_vc)?;
    debug!("queue_player_with_vc_status_fmt: add_ply succeeded, new pool size={}", session.pool.len());

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

    // Clone manager early to avoid move issues
    let manager_for_hot = queue_ctx.manager.clone();
    let manager_for_overflow = queue_ctx.manager.clone();

    // Only check quota if session was Idle - Hot sessions already met quota
    if was_idle && self.is_quota_fmt(fmt_id) {
      self.hot_fmt(fmt_id, queue_ctx.ctx, queue_ctx.guild_id, queue_ctx.db, manager_for_hot, false).await?;
    }

    // Handle overflow in Hot sessions - create new Idle session for 2nd game
    if was_hot {
      let quota = self.format(fmt_id).map(|sg| sg.quota as usize).unwrap_or(8);
      
      // Check if Hot session has overflow
      let has_overflow = {
        let hot_sessions = self.get_seshs_by_status_fmt(fmt_id, &SessionStatus::Hot);
        !hot_sessions.is_empty() && hot_sessions[0].pool.len() > quota
      };
      
      if has_overflow {
        // Try to create new Idle session
        match self.create_session_format(fmt_id) {
          Ok(_) => {
            // Move overflow players to new Idle session
            let mut hot_sessions_mut = self.get_seshs_by_status_fmt_mut(fmt_id, &SessionStatus::Hot);
            if !hot_sessions_mut.is_empty() {
              let overflow_count = hot_sessions_mut[0].pool.len() - quota;
              let overflow_players: Vec<_> = hot_sessions_mut[0].pool.drain(quota..).collect();
              drop(hot_sessions_mut); // Release borrow before next mutable borrow
              
              // Get the newly created Idle session (last in the vector)
              let idle_session = self.format_mut(fmt_id).and_then(|sg| sg.sessions.last_mut()).ok_or_else(|| anyhow!("Failed to get new Idle session"))?;
              for overflow_player in overflow_players {
                idle_session.pool.push(overflow_player);
              }
              
              info!("Created new Idle session and moved {} overflow players from Hot session in format {}", overflow_count, fmt_id);
              
              // Check if the new Idle session now meets quota for a 2nd simultaneous game
              if self.is_quota_fmt(fmt_id) {
                info!("New Idle session meets quota, transitioning to Hot for 2nd simultaneous game");
                self.hot_fmt(fmt_id, queue_ctx.ctx, queue_ctx.guild_id, queue_ctx.db, manager_for_overflow, false).await?;
              }
            }
          }
          Err(_) => {
            // Idle session already exists, overflow players stay in Hot session
            // This is expected and not an error
          }
        }
      }
    }

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
    let current_count = self.formats[0].sessions.iter().find(|s| s.status == SessionStatus::Idle).map(|s| s.pool.len()).unwrap_or(0);

    // Get current channel name

    let current_name = match queue_vc.name(&ctx.http).await {
      Ok(name) => name,
      Err(e) => {
        warn!("[UPDATE_VC_NAME] Failed to get channel name: {}", e);
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
        Err(e) => warn!("[UPDATE_VC_NAME] Failed to update channel name: {}", e),
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

  pub fn is_quota_fmt(&self, fmt_id: u8) -> bool {
    let g = self.get_seshs_by_status_fmt(fmt_id, &SessionStatus::Idle);
    if g.is_empty() {
      warn!("No idle sessions found when checking quota for format {}", fmt_id);
      return false;
    }
    // Check all idle sessions - any one meeting quota should return true for concurrent games
    let q = self.format(fmt_id).map(|sg| sg.quota as usize).unwrap_or(0);
    for session in g {
      let l = session.pool.len();
      match l.cmp(&q) {
        std::cmp::Ordering::Equal => return true,
        std::cmp::Ordering::Greater => {
          warn!("Quota met late, more players than quota in format {}", fmt_id);
          return true;
        }
        std::cmp::Ordering::Less => continue,
      }
    }
    false
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
      let embed = CreateEmbed::new().title("PUG Starting").description("Please join the queue channel!");

      let content = player_mentions.join(" ");
      let msg = CM::new().embed(embed).content(content);
      let dashboard = self.channels.dashboard;
      if let Ok(sent) = dashboard.send_message(&ctx.http, msg).await {
        // Store pending notification for tracking
        self.pending_vc_notification = Some((sent.id, players_to_dm.clone()));

        // Delete the message after confirm expiry duration
        let http = ctx.http.clone();
        let channel_id = dashboard;
        let message_id = sent.id;
        let confirm_time = self.confirm_time;
        tokio::spawn(async move {
          tokio::time::sleep(tokio::time::Duration::from_secs(confirm_time as u64)).await;
          let _ = channel_id.delete_message(&http, message_id).await;
        });
      }
    }

    // Send DMs to users who have pm_hot_alert=true
    if let Some(database) = db {
      let dm_tracker = ctx.data.read().await.get::<crate::models::DmTrackerKey>().cloned();

      for user_id in players_to_dm {
        // Check if user has DM notifications enabled
        match database.players.get_pm_hot_alert(user_id).await {
          Ok(true) => {
            let dm_embed = CreateEmbed::new()
              .title("PUG Ready!")
              .description(format!(
                "A game is ready in **{}**!\nPlease join the queue channel.",
                ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "the server".to_string())
              ))
              .footer(serenity::all::CreateEmbedFooter::new("Don't want to be messaged directly? Press the button below"))
              .color(GREEN);

            let disable_button = serenity::all::CreateButton::new("disable_dm_notifications").label("Disable DM notifications").style(serenity::all::ButtonStyle::Secondary);

            let components = vec![serenity::all::CreateActionRow::Buttons(vec![disable_button])];

            if let Some(ref tracker) = dm_tracker {
              if let Err(e) = tracker.send_dm(ctx, user_id, dm_embed, components).await {
                warn!("Failed to send DM to user {}: {}", user_id, e);
              }
            } else {
              warn!("DM tracker not available for hot alert DM to {}", user_id);
            }
          }
          Ok(false) => {
            // User has DMs disabled, skip
          }
          Err(e) => {
            warn!("Failed to check DM status for user {}: {}", user_id, e);
          }
        }
      }
    }
  }

  pub async fn move_user(&self, guild_id: GI, user_id: UI, channel_id: CI, ctx: &Context) -> Result<(), Error> {
    let member = guild_id.member(&ctx.http, user_id).await?;
    member.move_to_voice_channel(&ctx.http, channel_id).await?;
    Ok(())
  }

  /// Called when a player joins the queue VC. Updates/deletes the pending notification if applicable.
  pub async fn on_player_joined_vc(&mut self, ctx: &Context, user_id: UI) {
    if let Some((msg_id, ref mut pending_users)) = self.pending_vc_notification {
      // Check if this user was in the pending list
      if let Some(pos) = pending_users.iter().position(|&u| u == user_id) {
        pending_users.remove(pos);

        let dashboard = self.channels.dashboard;

        if pending_users.is_empty() {
          // All players have joined - delete the notification
          if let Err(e) = dashboard.delete_message(&ctx.http, msg_id).await {
            warn!("Failed to delete VC notification message: {}", e);
          }
          self.pending_vc_notification = None;
        } else {
          // Edit the message to show remaining players
          let remaining_mentions: Vec<String> = pending_users.iter().map(|u| format!("<@{}>", u)).collect();
          let embed = CreateEmbed::new().title("PUG Starting").description("Please join the queue channel!");
          let content = remaining_mentions.join(" ");

          let edit = serenity::all::EditMessage::new().embed(embed).content(content);
          if let Err(e) = dashboard.edit_message(&ctx.http, msg_id, edit).await {
            warn!("Failed to edit VC notification message: {}", e);
          }
        }
      }
    }
  }

  /// Clear the pending VC notification (e.g., when game starts or ends)
  pub async fn clear_vc_notification(&mut self, ctx: &Context) {
    if let Some((msg_id, _)) = self.pending_vc_notification.take() {
      let _ = self.channels.dashboard.delete_message(&ctx.http, msg_id).await;
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
