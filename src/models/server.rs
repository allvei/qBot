//! # Server Module
//!
//! This module defines the Server struct and its related functionality.
//! A Server represents a Discord guild with associated categories and games.

use std::{sync::Arc, time::Duration};
use std::time::SystemTime;

use tokio::sync::Mutex;
use anyhow::{anyhow, Error, Result};
use crate::{Database as DB, GREEN, Manager, Rank, models::constants::{DEFAULT_ACTIVE_ELO, MAX_TIMEOUT, MIN_TIMEOUT}, guild_name, log_prefix_format};
use serde::{Deserialize, Serialize};
use serenity::{all::{
    ChannelId as CI, Context, CreateEmbed,
    CreateMessage as CM, GuildId as GI, MessageId as MI, RoleId as RI,
    UserId as UI,
}, };
use tracing::{debug, info, warn};

use crate::models::{
    Player, SessionPlayer, Session, SessionStatus, TeamChannel,
};

/// Context parameters for queue operations
pub struct QueueContext<'a> {
    pub ctx: &'a Context,
    pub guild_id: Option<GI>,
    pub db: Option<&'a DB>,
    pub manager: Option<Arc<Mutex<Manager>>>,
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
    pub ip:   String,
    pub name: String,
}

// Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub guild_id:   GI,
    pub guild_name: String,
    pub roles:      Roles,
    pub categories:     Vec<Category>,
}

impl Server {
    pub fn new(guild_id: GI, guild_name: String, roles: Roles) -> Self {
        Self {
            guild_id,
            guild_name,
            roles,
            categories: Vec::new(),
        }
    }

    pub fn add_category(&mut self, category: Category) -> Result<()> {
        self.categories.push(category);
        if let Some(category) = self.categories.last_mut() {
            // Create an idle session for every format
            for sg in &mut category.formats {
                if sg.sessions.is_empty() {
                    sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
                }
            }
        }
        Ok(())
    }

    pub fn empty(guild_id: GI, guild_name: String) -> Self {
        Self {
            guild_id,
            guild_name,
            roles: Roles::empty(),
            categories: Vec::new(),
        }
    }

    pub fn get_category(&mut self, channel_id: CI) -> Result<&mut Category> {
        match self.categories.iter_mut().find(|category| category.contains_channel(channel_id)) {
            Some(category) => Ok(category),
            None => Err(anyhow!("Category not found")),
        }
    }

    /// Check if active ELO is enabled for this server
    pub async fn is_active_elo_enabled(&self, db: &DB) -> Result<bool> {
        Ok(db.config.get_active_elo(self.guild_id).await.unwrap_or(DEFAULT_ACTIVE_ELO))
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
            Self::Bch =>     "BCH",
            Self::Average => "Average",
        }
    }

    pub fn from_str(s: &str) -> Self {
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
            Self::OnHot       => "Game goes hot",
            Self::OnGameStart => "Runners start game",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "on_first_join" => Self::OnFirstJoin,
            "on_hot"        => Self::OnHot,
            "on_game_start" => Self::OnGameStart,
            _ => Self::default(),
        }
    }

    pub fn to_db_str(&self) -> &'static str {
        match self {
            Self::OnFirstJoin => "on_first_join",
            Self::OnHot       => "on_hot",
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
    AfterTimeout,
}

impl TeamVcDestroyPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OnLastLeave => "Last player leaves",
            Self::AfterPull   => "After game ends",
            Self::AfterTimeout => "After post-game timeout",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "on_last_leave"  => Self::OnLastLeave,
            "after_pull"     => Self::AfterPull,
            "after_timeout"  => Self::AfterTimeout,
            _ => Self::default(),
        }
    }

    pub fn to_db_str(&self) -> &'static str {
        match self {
            Self::OnLastLeave  => "on_last_leave",
            Self::AfterPull    => "after_pull",
            Self::AfterTimeout => "after_timeout",
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
    pub create_policy:  TeamVcCreatePolicy,
    pub destroy_policy: TeamVcDestroyPolicy,
    /// Always keep at least 1 set of team channels; create more as needed
    pub keep_minimum:   bool,
}

impl Default for TeamVcSettings {
    fn default() -> Self {
        Self {
            create_policy:  TeamVcCreatePolicy::default(),
            destroy_policy: TeamVcDestroyPolicy::default(),
            keep_minimum:   true,
        }
    }
}

// Format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Format {
    pub id:           u8,
    pub name:         String,
    pub quota:        u8,
    pub sessions:     Vec<Session>,
    pub connect_info: Option<String>,
}

impl Format {
    pub fn new(id: u8, name: String, quota: u8) -> Self {
        Self {
            id,
            name,
            quota,
            sessions: Vec::new(),
            connect_info: None,
        }
    }

    pub fn display_name(&self) -> &str {
        &self.name
    }
}

// Category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub guild_id:            GI,
    pub guild_name:          Option<String>,
    pub category_id:            u8,
    pub name:                Option<String>,
    pub timeout:             u16,
    pub dashboard_msg:       MI,
    pub channels:            Channels,
    pub formats:           Vec<Format>,
    pub team_balance_method: TeamBalanceMethod,
    pub team_vc_settings:    TeamVcSettings,
    pub dm_alert_enabled:    bool,
    pub dm_alert_threshold:  u8,
    pub dm_alert_users:      Vec<UI>,
    /// Track recently freed team channels to avoid immediate recreation
    pub recently_freed_teams: Vec<TeamChannel>,
}

impl Category {
    pub fn new(
        guild_id:      GI,
        guild_name:    Option<String>,
        category_id:      u8,
        name:          Option<String>,
        quota:         u8,
        timeout:       u16,
        dashboard_msg: MI,
        channels:      Channels,
        games:         Vec<Session>,
    ) -> Self {
        let default_name = name.clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("Category {}", category_id));
        let mut sg = Format::new(0, default_name, quota);
        sg.sessions = games;

        Self {
            guild_id,
            guild_name,
            category_id,
            name,
            timeout,
            dashboard_msg,
            channels,
            formats: vec![sg],
            team_balance_method: TeamBalanceMethod::default(),
            team_vc_settings: TeamVcSettings::default(),
            dm_alert_enabled: false,
            dm_alert_threshold: 0,
            dm_alert_users: Vec::new(),
            recently_freed_teams: Vec::new(),
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
        let idx = self.formats.iter().position(|sg| sg.id == id)
            .ok_or_else(|| anyhow!("Format {} not found", id))?;
        self.formats.remove(idx);
        Ok(())
    }

    /// Get the next available format ID
    fn next_format_id(&self) -> u8 {
        (0..=255).find(|id| !self.formats.iter().any(|sg| sg.id == *id))
            .unwrap_or(0)
    }

    /// Get display name for the category (name or "Category {id}")
    pub fn display_name(&self) -> String {
        self.name.clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("Category {}", self.category_id))
    }

    pub fn create_session(&mut self) -> Result<&mut Session> {
        self.create_session_sg(0)
    }

    pub fn create_session_sg(&mut self, sg_id: u8) -> Result<&mut Session> {
        let sg = self.format_mut(sg_id)
            .ok_or_else(|| anyhow!("Format {} not found", sg_id))?;
        let has_inactive = sg.sessions.iter().any(|s| !s.is_active());
        if has_inactive {
            return Err(anyhow!("Cannot create new session: inactive session already exists"));
        }
        sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
        let sg = self.format_mut(sg_id).unwrap();
        sg.sessions.last_mut().ok_or_else(|| anyhow!("Failed to create session"))
    }

    pub fn end_game(&mut self) -> bool {
        self.end_game_sg(0)
    }

    pub fn end_game_sg(&mut self, sg_id: u8) -> bool {
        if let Some(sg) = self.format_mut(sg_id) {
            if let Some(pos) = sg.sessions.iter().position(|s| s.status == SessionStatus::Idle) {
                sg.sessions.remove(pos);
                return true;
            }
        }
        false
    }

    pub async fn get_queue(&mut self) -> Result<&mut Session, Error> {
        self.get_queue_sg(0).await
    }

    pub async fn get_queue_sg(&mut self, sg_id: u8) -> Result<&mut Session, Error> {
        let sg = self.format_mut(sg_id)
            .ok_or_else(|| anyhow!("Format {} not found", sg_id))?;
        sg.sessions.iter_mut()
            .find(|s| s.status == SessionStatus::Idle || s.status == SessionStatus::Hot)
            .ok_or(anyhow!("No joinable session found in format {}", sg_id))
    }

    pub fn get_inactives(&self) -> Vec<&Session> {
        self.formats[0].sessions.iter()
            .filter(|s| !s.is_active())
            .collect()
    }

    pub fn get_actives(&self) -> Vec<&Session> {
        self.formats[0].sessions.iter()
            .filter(|s| s.is_active())
            .collect()
    }

    /// Delete any orphaned dynamic VCs left under the category from a previous bot run.
    /// Also cleans up stale team VC pairs from `channels.teams`.
    /// Only deletes channels that are tracked in the database as team channels.
    pub async fn cleanup_orphaned_vcs(&mut self, ctx: &Context, db: &DB) {
        use serenity::all::ChannelType;

        let category_id = self.channels.category;
        if category_id.get() <= 1 {
            return;
        }

        // Static channel IDs that should never be deleted (excludes team VCs)
        let static_ids = vec![
            self.channels.category,
            self.channels.queue_chat,
            self.channels.queue_vc,
            self.channels.dashboard,
        ];

        let guild = match ctx.cache.guild(self.guild_id) {
            Some(g) => g.clone(),
            None => return,
        };

        // Delete team VCs from previous run (they are ephemeral)
        // Only attempt deletion if the channel still exists in the cache to avoid slow API calls
        for tc in &self.channels.teams {
            for vc_id in [tc.red_vc, tc.blu_vc] {
                if guild.channels.contains_key(&vc_id) {
                    if let Err(e) = vc_id.delete(&ctx.http).await {
                        warn!("[{}] Failed to delete team VC {}: {}", guild.name, vc_id, e);
                    } else {
                        info!("[{}] Cleaned up team VC from previous run: {}", guild.name, vc_id);
                    }
                }
            }
        }
        self.channels.teams.clear();

        // Load tracked team channels from database
        let tracked_teams = match db.teams.get_teams_for_category(self.guild_id, self.category_id).await {
            Ok(teams) => teams,
            Err(e) => {
                warn!("Failed to load tracked team channels from database: {}", e);
                return;
            }
        };

        // Create a set of all tracked team channel IDs
        let mut tracked_channel_ids = std::collections::HashSet::new();
        for (red_vc, blu_vc) in &tracked_teams {
            tracked_channel_ids.insert(*red_vc);
            tracked_channel_ids.insert(*blu_vc);
        }

        // Only delete voice channels that are tracked in the database as team channels
        for (_id, channel) in &guild.channels {
            if channel.kind != ChannelType::Voice {
                continue;
            }
            if channel.parent_id != Some(category_id) {
                continue;
            }
            if static_ids.contains(&channel.id) {
                continue;
            }
            // IMPORTANT: Only delete if it's tracked in the database as a team channel
            if !tracked_channel_ids.contains(&channel.id) {
                info!("[{}] Skipping untracked VC: {} (not in database)", guild.name, channel.name);
                continue;
            }

            info!("[{}] Cleaning up tracked team VC: {}", guild.name, channel.name);
            if let Err(e) = channel.id.delete(&ctx.http).await {
                warn!("Failed to delete tracked team VC {} ({}): {}", channel.name, channel.id, e);
            } else {
                // Remove from database after successful deletion
                if let Err(e) = db.teams.remove_team(self.guild_id, channel.id, CI::new(0)).await {
                    warn!("Failed to remove team channel from database: {}", e);
                }
            }
        }
    }

    pub fn get_sessions_by_status(&self, status: &SessionStatus) -> Vec<&Session> {
        self.formats[0].sessions.iter()
            .filter(|s| s.status == *status)
            .collect()
    }

    pub fn get_sessions_by_status_sg(&self, sg_id: u8, status: &SessionStatus) -> Vec<&Session> {
        self.format(sg_id)
            .map(|sg| sg.sessions.iter().filter(|s| s.status == *status).collect())
            .unwrap_or_default()
    }

    /// Get session index (position in Vec) for logging purposes
    pub fn get_session_index(&self, session: &Session) -> Option<usize> {
        self.formats[0].sessions.iter().position(|s| std::ptr::eq(s, session))
    }

    pub fn get_games_by_status_mut(&mut self, status: &SessionStatus) -> Vec<&mut Session> {
        self.formats[0].sessions.iter_mut()
            .filter(|s| s.status == *status)
            .collect()
    }

    pub async fn get_user_session(&mut self, user_id: UI) -> Result<&mut Session> {
        for sg in &mut self.formats {
            if let Some(game) = sg.sessions.iter_mut().find(|s| s.pool.iter().any(|p| p.player.user_id == user_id)) {
                return Ok(game);
            }
        }
        Err(anyhow!("User not found in any game"))
    }

    /// Check if user is in a session within a specific format
    pub fn get_user_session_sg(&mut self, sg_id: u8, user_id: UI) -> Result<&mut Session> {
        let sg = self.format_mut(sg_id)
            .ok_or_else(|| anyhow!("Format {} not found", sg_id))?;
        sg.sessions.iter_mut()
            .find(|s| s.pool.iter().any(|p| p.player.user_id == user_id))
            .ok_or_else(|| anyhow!("User not found in format {}", sg_id))
    }

    /// Get the format name for the format containing this user
    pub fn get_user_sg_name(&self, user_id: UI) -> Option<String> {
        self.formats.iter()
            .find(|sg| sg.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.user_id == user_id)))
            .map(|sg| sg.name.clone())
    }

    /// Check if user is in any session across all formats
    pub fn is_user_in_any_session(&self, user_id: UI) -> bool {
        self.formats.iter().any(|sg|
            sg.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.user_id == user_id))
        )
    }

    /// Check if user is in a specific format's sessions
    pub fn is_user_in_sg(&self, sg_id: u8, user_id: UI) -> bool {
        self.format(sg_id)
            .map(|sg| sg.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.user_id == user_id)))
            .unwrap_or(false)
    }

    pub fn get_player(&mut self, user_id: UI) -> Result<&mut SessionPlayer> {
        for sg in &mut self.formats {
            for session in &mut sg.sessions {
                if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
                    return Ok(player);
                }
            }
        }
        Err(anyhow!("User not found in any game"))
    }

    pub async fn hot(&mut self, ctx: &Context, guild_id: Option<GI>, db: Option<&DB>, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
        self.hot_sg(0, ctx, guild_id, db, manager).await
    }

    pub async fn hot_sg(&mut self, sg_id: u8, ctx: &Context, guild_id: Option<GI>, db: Option<&DB>, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
        let _ = self.get_queue_sg(sg_id).await?.hot();

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
            self.refresh_player_ranks(ctx, gid, database).await;
        }

        // Notify requires guild_id for VC validation
        if let Some(gid) = guild_id {
            self.notify(ctx, gid, db).await;
        } else {
            warn!("Cannot notify: guild_id not provided");
        }

        // Generate teams - guild_id is required for dashboard updates
        if let Some(gid) = guild_id {
            self.generate_teams_sg(sg_id, ctx, gid, db).await;
        } else {
            warn!("Cannot generate teams: guild_id not provided");
        }

        // Spawn a targeted deadline timer for this hot session
        if let (Some(guild_id), Some(mgr)) = (guild_id, manager) {
            let category_id = self.category_id;
            let timeout = self.timeout;
            let ctx_clone = ctx.clone();
            
            // Get post-game timeout before spawning task
            let post_game_timeout = if let Some(database) = db {
                database.config.get_post_game_timeout(guild_id).await.ok()
            } else {
                None
            };

            tokio::spawn(async move {
                use tokio::time::{sleep, Duration};

                // Wait for the deadline (use category's configured timeout)
                sleep(Duration::from_secs(timeout as u64)).await;

                // Check if players have joined, remove those who haven't
                let mut manager_lock = mgr.lock().await;
                if let Ok(server) = manager_lock.get_server(guild_id) {
                    if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
                        if category.check_hot_timeout(&ctx_clone, guild_id, post_game_timeout).await {
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
    pub async fn check_hot_timeout(&mut self, ctx: &Context, guild_id: GI, post_game_timeout: Option<u16>) -> bool {
        let mut changes_made = false;

        // Check hot sessions across all formats
        for sg in &mut self.formats {
            let quota = sg.quota as usize;

            // Find hot sessions that have timed out
            let hot_sessions: Vec<usize> = sg.sessions.iter().enumerate()
                .filter_map(|(idx, s)| {
                    // Use post-game timeout if this is a post-game scenario and post_game_timeout is provided
                    let timeout_seconds = if s.match_ended_at.is_some() {
                        post_game_timeout.map(|t| t as u64).unwrap_or(self.timeout as u64)
                    } else {
                        self.timeout as u64
                    };
                    
                    if s.is_hot_timeout(timeout_seconds) {
                        Some(idx)
                    } else {
                        None
                    }
                })
                .collect();

            for idx in hot_sessions {
                let session = &mut sg.sessions[idx];

                // Get players who are not in VC (timed out)
                let timed_out_players: Vec<_> = session.pool.iter().take(quota)
                    .filter(|p| !p.in_queue_vc)
                    .collect();

                if timed_out_players.is_empty() {continue;}

                // Create list of player names for logging
                let player_names: Vec<String> = timed_out_players.iter()
                    .map(|p| p.player.tag.clone())
                    .collect();

                let guild_name = guild_name(ctx, guild_id);
                let full_prefix = log_prefix_format(
                    &guild_name, 
                    self.name.as_deref().unwrap_or("unknown"), 
                    &sg.name
                );
                
                info!("{} Removing {} timed-out players: {}", 
                    full_prefix, timed_out_players.len(), player_names.join(", "));

                // Remove timed out players - retain() preserves order of remaining elements
                let timed_out_ids: Vec<_> = timed_out_players.iter().map(|p| p.player.user_id).collect();
                session.pool.retain(|p| !timed_out_ids.contains(&p.player.user_id));

                // Check if we still have enough players after removals
                if session.pool.len() >= quota {
                    info!("{} Regenerating teams after timeout with {} players", full_prefix, session.pool.len());
                    changes_made = true;
                } else {
                    info!("{} Not enough players after timeout, reverting to idle", full_prefix);
                    session.idle();
                    changes_made = true;
                }
            }
        }

        // If changes were made, regenerate teams for each format that still has a hot session
        if changes_made {
            let hot_sg_ids: Vec<u8> = self.formats.iter()
                .filter(|sg| sg.sessions.iter().any(|s| s.is_hot() && s.pool.len() >= sg.quota as usize))
                .map(|sg| sg.id)
                .collect();
            for sg_id in hot_sg_ids {
                self.generate_teams_sg(sg_id, ctx, guild_id, None).await;
            }
        }

        changes_made
    }

    /// Check idle sessions for timeout timeouts and handle accordingly
    /// Returns true if any changes were made that require dashboard update
    pub async fn check_timeout(&mut self, db: &DB, ctx: &Context, guild_id: GI) -> bool {
        let mut changes_made = false;

        // Check idle sessions across all formats (not hot/push/live)
        for sg in &mut self.formats {
            for session in sg.sessions.iter_mut() {
                if !session.is_idle() {
                    continue;
                }

                let mut players_to_remove = Vec::new();

                for player in &session.pool {
                    let timeout = player.timeout;

                    // Clamp to valid range (EXPIRY_MIN to EXPIRY_MAX)
                    let expiry_mins = timeout.clamp(MIN_TIMEOUT, MAX_TIMEOUT);

                    // Skip if timeout is disabled (below EXPIRY_MIN)
                    if expiry_mins < MIN_TIMEOUT {
                        continue;
                    }

                    // Check if player has exceeded their timeout time
                    if let Ok(elapsed) = SystemTime::now().duration_since(player.joined_at) {
                        if elapsed.as_secs() >= Duration::from_mins(expiry_mins as u64).as_secs() {
                            info!("Auto-removing player {} after {} seconds (limit: {})",
                                player.player.tag,
                                elapsed.as_secs(),
                                expiry_mins
                            );
                            players_to_remove.push(player.player.user_id);
                        }
                    }
                }

                if !players_to_remove.is_empty() {
                    // Remove the timed-out players
                    for user_id in &players_to_remove {
                        session.remove_player(*user_id);
                        
                        // Optionally: disconnect from VC if vc_kick is enabled
                        if let Ok(settings) = db.users.get_prefs(*user_id).await {
                            if settings.vc_auto_leave {
                                if let Ok(member) = guild_id.member(&ctx.http, *user_id).await {
                                    if let Err(e) = member.disconnect_from_voice(&ctx.http).await {
                                        warn!("Failed to disconnect timeoutd player from VC: {e}");
                                    }
                                }
                            }
                        }
                    }
                    changes_made = true;
                    info!("Timeoutd {} player(s) from queue", players_to_remove.len());
                }
            }
        }

        changes_made
    }

    pub async fn push(&mut self, ctx: &Context, guild_id: GI, db: &DB) -> Result<(), Error> {
        self.push_sg(0, ctx, guild_id, db).await
    }

    pub async fn push_sg(&mut self, sg_id: u8, ctx: &Context, guild_id: GI, db: &DB) -> Result<(), Error> {
        // Ensure a free team VC pair exists (creates one if needed)
        self.ensure_team_vcs(ctx, guild_id, db).await?;

        // Now find the free pair
        let occupied_teams: Vec<TeamChannel> = self.all_occupied_teams();

        let team_pair = self.channels.teams.iter().find(|t| {
            !occupied_teams.iter().any(|o| o.red_vc == t.red_vc && o.blu_vc == t.blu_vc)
        }).cloned().ok_or_else(|| anyhow!("No free team VC pair available after ensure"))?;

        let red_vc = team_pair.red_vc;
        let blu_vc = team_pair.blu_vc;

        // Get the hot game in the target format
        let sg = self.format_mut(sg_id)
            .ok_or_else(|| anyhow!("Format {} not found for push", sg_id))?;
        let game = sg.sessions.iter_mut()
            .find(|s| s.status == SessionStatus::Hot)
            .ok_or(anyhow!("No hot session found for push in format {}", sg_id))?;

        // Store the team channels on the session
        game.team_channels = Some(team_pair);

        // Set status to Push and extract player moves
        game.push();
        let player_moves: Vec<(UI, CI, String)> = game.pool.iter()
            .filter_map(|player| {
                if !player.in_queue_vc {
                    return None;
                }

                match player.team {
                    Some(crate::models::Team::Red) => Some((
                        player.player.user_id,
                        red_vc,
                        player.player.tag.clone()
                    )),
                    Some(crate::models::Team::Blu) => Some((
                        player.player.user_id,
                        blu_vc,
                        player.player.tag.clone()
                    )),
                    _ => None,
                }
            })
            .collect();

        // Move users to team channels
        for (user_id, channel_id, tag) in player_moves {
            if let Err(e) = self.move_user(guild_id, user_id, channel_id, ctx).await {
                warn!("Failed to move user {}: {}", tag, e);
            }
        }

        // Set game status to Live and extract overflow players
        let sg = self.format_mut(sg_id).unwrap();
        let quota = sg.quota as usize;
        let session_idx = sg.sessions.iter()
            .position(|s| s.status == SessionStatus::Push)
            .ok_or(anyhow!("Push session not found in format {}", sg_id))?;
        let game = &mut sg.sessions[session_idx];
        game.live();

        let game_pool_len = game.pool.len();

        // Extract overflow players (those beyond quota)
        let overflow_players: Vec<_> = if game_pool_len > quota {
            game.pool.drain(quota..).collect()
        } else {
            Vec::new()
        };

        // Create new idle session for next game in this format
        self.create_session_sg(sg_id)?;

        // Add overflow players to the new idle session
        if !overflow_players.is_empty() {
            let idle_session = self.get_queue_sg(sg_id).await?;
            for player in overflow_players {
                idle_session.pool.push(player);
            }
        }

        // Clear recently freed teams since we're now using team channels
        self.recently_freed_teams.clear();
        
        self.queue_dash_update(ctx, guild_id).await;
        Ok(())
    }

    /// Collect all team channel pairs occupied by active sessions across all formats
    pub fn all_occupied_teams(&self) -> Vec<TeamChannel> {
        let mut occupied: Vec<TeamChannel> = self.formats.iter()
            .flat_map(|sg| sg.sessions.iter())
            .filter(|s| s.is_active())
            .filter_map(|s| s.team_channels.clone())
            .collect();
        
        // Also include recently freed teams to prevent immediate recreation
        occupied.extend(self.recently_freed_teams.clone());
        occupied
    }

    /// Ensure at least one free team VC pair exists under the category.
    /// Called at the lifecycle point determined by `team_vc_settings.create_policy`.
    /// Returns the newly created pair, or None if a free pair already exists.
    pub async fn ensure_team_vcs(&mut self, ctx: &Context, guild_id: GI, db: &crate::Database) -> Result<Option<TeamChannel>, Error> {
        use serenity::all::{CreateChannel, ChannelType};

        // Check which team pairs are currently occupied by active sessions across all formats
        let occupied: Vec<TeamChannel> = self.all_occupied_teams();

        // Check if there's already a free pair
        let has_free = self.channels.teams.iter().any(|t| {
            !occupied.iter().any(|o| o.red_vc == t.red_vc && o.blu_vc == t.blu_vc)
        });

        if has_free {
            return Ok(None);
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
                    ctx.cache.channel(self.channels.queue_vc)
                        .and_then(|qvc| qvc.parent_id)
                        .ok_or_else(|| anyhow!("No valid category found for team VC creation"))?
                }
            } else {
                // Not in cache - try queue VC's parent
                ctx.cache.channel(self.channels.queue_vc)
                    .and_then(|qvc| qvc.parent_id)
                    .ok_or_else(|| anyhow!("No valid category found for team VC creation"))?
            }
        };
        // Update stored category if it was wrong
        if category != self.channels.category {
            info!("Resolved team VC category to {} (was {})", category, self.channels.category);
            self.channels.category = category;
        }

        let pair_num = self.channels.teams.len() + 1;

        let red_ch = guild_id.create_channel(&ctx.http,
            CreateChannel::new(format!("🔴 RED #{}", pair_num))
                .kind(ChannelType::Voice)
                .category(category)
        ).await.map_err(|e| anyhow!("Failed to create RED VC: {e}"))?;

        let blu_ch = guild_id.create_channel(&ctx.http,
            CreateChannel::new(format!("🔵 BLU #{}", pair_num))
                .kind(ChannelType::Voice)
                .category(category)
        ).await.map_err(|e| anyhow!("Failed to create BLU VC: {e}"))?;

        let pair = TeamChannel::new(red_ch.id, blu_ch.id);
        self.channels.teams.push(pair.clone());
        
        // Persist to database
        if let Err(e) = db.teams.add_team(guild_id, self.category_id, red_ch.id, blu_ch.id).await {
            warn!("Failed to persist team channels to database: {}", e);
        }
        
        info!("Created set {} of team VCs", pair_num);

        Ok(Some(pair))
    }

    /// Remove unused team VC pairs.
    /// When `force` is true, all free pairs are deleted (used by destroy policy triggers).
    /// When `force` is false, `keep_minimum` is respected (preserving at least one free pair).
    pub async fn cleanup_team_vcs(&mut self, ctx: &Context, force: bool) {
        // Collect occupied pairs from active sessions across all formats
        let occupied: Vec<TeamChannel> = self.all_occupied_teams();

        // Partition into occupied and free
        let (keep, mut removable): (Vec<_>, Vec<_>) = self.channels.teams.iter().cloned()
            .partition(|t| occupied.iter().any(|o| o.red_vc == t.red_vc && o.blu_vc == t.blu_vc));

        // If keep_minimum and not forced, preserve one free pair
        let min_free = if !force && self.team_vc_settings.keep_minimum && keep.is_empty() { 1 } else { 0 };
        let to_delete_count = removable.len().saturating_sub(min_free);
        let to_delete: Vec<TeamChannel> = removable.drain(..to_delete_count).collect();

        for tc in &to_delete {
            info!("Deleting unused team VCs: RED={} BLU={}", tc.red_vc, tc.blu_vc);
            if let Err(e) = tc.red_vc.delete(&ctx.http).await {
                let hint = if e.to_string().contains("Missing Access") { "(Missing *Manage Channels* permissions)" } else { "" };
                warn!("Failed to delete RED VC {}:{}{}", tc.red_vc, e, hint);
            }
            if let Err(e) = tc.blu_vc.delete(&ctx.http).await {
                let hint = if e.to_string().contains("Missing Access") { "(Missing *Manage Channels* permissions)" } else { "" };
                warn!("Failed to delete BLU VC {}:{}{}", tc.blu_vc, e, hint);
            }
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
        let has_active = self.formats.iter().any(|sg|
            sg.sessions.iter().any(|s| s.is_active())
        );

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
        let all_idle_empty = self.formats.iter().all(|sg|
            sg.sessions.iter()
                .filter(|s| s.is_idle())
                .all(|s| s.pool.is_empty())
        );

        // Also check there are no active games
        let no_active = !self.formats.iter().any(|sg|
            sg.sessions.iter().any(|s| s.is_active())
        );

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
    pub async fn check_team_vc_empty_auto_end(
        &mut self,
        ctx: &Context,
        guild_id: GI,
        db: &DB,
        manager: Option<Arc<Mutex<Manager>>>,
    ) {
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

                let red_count = guild.voice_states.values()
                    .filter(|vs| vs.channel_id == Some(tc.red_vc))
                    .count();
                let blu_count = guild.voice_states.values()
                    .filter(|vs| vs.channel_id == Some(tc.blu_vc))
                    .count();

                if red_count == 0 && blu_count == 0 {
                    info!("All players left team VCs (RED={} BLU={}) for format {}, auto-ending game",
                        tc.red_vc, tc.blu_vc, sg.id);
                    to_pull.push(sg.id);
                }
            }
        }

        for sg_id in to_pull {
            if let Err(e) = self.pull_sg(sg_id, ctx, guild_id, db, manager.clone()).await {
                warn!("Failed to auto-end game in format {}: {}", sg_id, e);
            }
        }
    }

    pub async fn pull(&mut self, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
        self.pull_sg(0, ctx, guild_id, db, manager).await
    }

    pub async fn pull_sg(&mut self, sg_id: u8, ctx: &Context, guild_id: GI, db: &DB, manager: Option<Arc<Mutex<Manager>>>) -> Result<(), Error> {
        // Extract queue vc channel ID
        let queue_vc = self.channels.queue_vc;

        // Find the active game (Hot or Live status) in the target format
        let sg = self.format_mut(sg_id)
            .ok_or_else(|| anyhow!("Format {} not found for pull", sg_id))?;
        let active_session_idx = sg.sessions.iter()
            .position(|s| s.status == SessionStatus::Hot || s.status == SessionStatus::Live)
            .ok_or(anyhow!("No active game to pull in format {}", sg_id))?;
        let game = &mut sg.sessions[active_session_idx];

        game.pull();

        // Extract all players to move back to queue
        let mut players_to_requeue: Vec<Player> = game.pool.iter()
            .map(|p| p.player.clone())
            .collect();

        // Shuffle the requeue order for variety
        {
            use rand::seq::SliceRandom;
            let mut rng = rand::rng();
            players_to_requeue.shuffle(&mut rng);
        } // RNG dropped here before async operations

        // Move all players back to queue voice channel and track who successfully moved
        let mut successfully_moved = std::collections::HashSet::new();
        for player in &players_to_requeue {
            let tag = &player.tag;
            
            // Check if player is already in the queue VC
            let already_in_queue = if let Some(guild) = ctx.cache.guild(guild_id) {
                guild.voice_states.get(&player.user_id)
                    .map(|vs| vs.channel_id == Some(queue_vc))
                    .unwrap_or(false)
            } else {
                false
            };
            
            if already_in_queue {
                // Player is already in queue VC, consider them successfully moved
                successfully_moved.insert(player.user_id);
                debug!("Player {} already in queue VC, marking as successfully moved", tag);
            } else if let Err(e) = self.move_user(guild_id, player.user_id, queue_vc, ctx).await {
                warn!("Failed to move {} back to queue: {}", tag, e);
            } else {
                successfully_moved.insert(player.user_id);
            }
        }

        // Check if quota will be met after re-queuing players to avoid unnecessary VC deletion/recreation
        let quota_will_be_met = {
            let mut projected_count = 0;
            if let Some(idle_idx) = self.format_mut(sg_id).unwrap().sessions.iter().position(|s| s.status == SessionStatus::Idle) {
                // Count existing players in idle session
                projected_count += self.formats[sg_id as usize].sessions[idle_idx].pool.len();
            }
            // Add players who will be re-queued (those successfully moved)
            projected_count += successfully_moved.len();
            projected_count >= self.quota() as usize
        };

        // Handle team channels based on whether we'll cleanup or not
        let skip_cleanup = quota_will_be_met && self.team_vc_settings.destroy_policy == TeamVcDestroyPolicy::AfterPull;
        
        if skip_cleanup {
            // Add team channels to recently_freed_teams to prevent immediate recreation
            let sg = self.format_mut(sg_id).unwrap();
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
            let sg = self.format_mut(sg_id).unwrap();
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
            },
            TeamVcDestroyPolicy::AfterTimeout => {
                // Spawn a timer that cleans up team VCs if no new game starts
                if let Some(mgr) = manager.clone() {
                    let category_id     = self.category_id;
                    let timeout_secs = self.timeout as u64;
                    let ctx_clone    = ctx.clone();

                    tokio::spawn(async move {
                        use tokio::time::{sleep, Duration};
                        sleep(Duration::from_secs(timeout_secs)).await;

                        let mut manager_lock = mgr.lock().await;
                        if let Ok(server) = manager_lock.get_server(guild_id) {
                            if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
                                // Only clean up if no active games are running
                                let has_active = category.formats.iter().any(|sg|
                                    sg.sessions.iter().any(|s| s.is_active())
                                );
                                if !has_active {
                                    category.cleanup_team_vcs(&ctx_clone, true).await;
                                }
                            }
                        }
                    });
                }
            },
            _ => {} // OnLastLeave handled elsewhere
        }

        // Find or create the idle session (queue) and add all players back to it
        let sg = self.format_mut(sg_id).unwrap();
        let idle_session_idx = match sg.sessions.iter().position(|s| s.status == SessionStatus::Idle) {
            Some(idx) => idx,
            None => {
                // No idle session exists (game ended from Hot without push), create one
                info!("No idle session found, creating one for re-queuing players in format {}", sg_id);
                sg.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
                sg.sessions.len() - 1
            }
        };
        let idle_session = &mut sg.sessions[idle_session_idx];
        
        // Set match_ended_at to track when the match ended for confirm timer base
        idle_session.match_ended_at = Some(std::time::SystemTime::now());

        // Only re-add players who were successfully moved (i.e., were still in voice)
        for player in players_to_requeue {
            let _rank = player.rank.clone();
            if successfully_moved.contains(&player.user_id) {
                // Player was successfully moved back to queue VC
                idle_session.add_player_in_vc(player);
            } else {
                // Player was not in voice, don't re-add them
                let tag = player.tag;
                info!("Not re-queueing {} - they left voice before match ended", tag);
            }
        }

        // Remove the finished session
        let sg = self.format_mut(sg_id).unwrap();
        sg.sessions.retain(|s| s.status != SessionStatus::Pull);

        // Check if the queue now meets quota and transition to Hot if needed
        if self.is_quota_sg(sg_id) {
            self.hot_sg(sg_id, ctx, Some(guild_id), Some(db), manager).await?;
        }

        self.queue_dash_update(ctx, guild_id).await;
        Ok(())
    }

    /// Update player ranks from Discord roles for all players in the session
    pub async fn refresh_player_ranks(&mut self, _ctx: &Context, guild_id: GI, db: &DB) {
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
    pub async fn validate_vc_status(&mut self, ctx: &Context, guild_id: GI) {
        // Get actual voice states from Discord
        let guild = match ctx.cache.guild(guild_id) {
            Some(g) => g,
            None    => {
                warn!("[validate_vc_status] Guild {} not in cache", guild_id);
                return;
            }
        };

        let queue_vc_id = self.channels.queue_vc.get();

        // Get set of users actually in queue VC
        let users_in_vc: std::collections::HashSet<u64> = guild.voice_states.iter()
            .filter_map(|(user_id, vs)| {
                if vs.channel_id.map(|c| c.get()) == Some(queue_vc_id) {
                    Some(user_id.get())
                } else {None}
            })
            .collect();

        // Update flags for all players in all sessions across all formats
        let mut corrected: Vec<String> = Vec::new();
        for sg in &mut self.formats {
            for session in &mut sg.sessions {
                for player in &mut session.pool {
                    let user_id      = player.player.user_id.get();
                    let actual_in_vc = users_in_vc.contains(&user_id);

                    if player.in_queue_vc != actual_in_vc {
                        corrected.push(player.player.tag.clone());
                        player.in_queue_vc = actual_in_vc;
                    }
                }
            }
        }
    }

    pub async fn generate_teams(&mut self, ctx: &Context, guild_id: GI, db: Option<&DB>) {
        self.generate_teams_sg(0, ctx, guild_id, db).await;
    }

    pub async fn generate_teams_sg(&mut self, sg_id: u8, ctx: &Context, guild_id: GI, _db: Option<&DB>) {
        use itertools::Itertools;

        let sg = match self.format(sg_id) {
            Some(sg) => sg,
            None => { warn!("Format {} not found for team generation", sg_id); return; }
        };
        let quota = sg.quota as usize;

        // Get the hot game (session was just set to hot before this is called)
        let Some(session_idx) = sg.sessions.iter()
            .position(|s| s.status == SessionStatus::Hot) else {
            warn!("No hot session found for team generation in format {}", sg_id);
            return;
        };

        let sg = self.format_mut(sg_id).unwrap();
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
            let team_b_indices: Vec<usize> = (0..pool_size)
                .filter(|i| !team_a_indices.contains(i))
                .collect();

            // Get ELOs for each team
            let team_a_elos: Vec<f64> = team_a_indices.iter()
                .map(|&i| players_to_balance[i].1 as f64)
                .collect();

            let team_b_elos: Vec<f64> = team_b_indices.iter()
                .map(|&i| players_to_balance[i].1 as f64)
                .collect();

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
            use std::collections::HashMap;
            use rand::seq::SliceRandom;

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
            for (_elo, indices) in &mut elo_categories {
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

            // Assign teams in-place to preserve in_queue_vc and in_queue_vc flags
            // First assign red team
            for &idx in &red_indices {
                let pool_idx = players_to_balance[idx].0;
                game.pool[pool_idx].team(crate::models::Team::Red);
            }

            // Then assign blue team
            for &idx in &blu_indices {
                let pool_idx = players_to_balance[idx].0;
                game.pool[pool_idx].team(crate::models::Team::Blu);
            }
        } else {
            warn!("Failed to generate balanced teams");
        }

        // Update dashboard to show the new teams
        self.queue_dash_update(ctx, guild_id).await;
    }

    pub async fn queue_player(
        &mut self,
        player: Player,
        rank: Rank,
        ctx: &Context,
        guild_id: Option<GI>,
        db: Option<&DB>,
        manager: Option<Arc<Mutex<Manager>>>,
    ) -> Result<()> {
        let queue_ctx = QueueContext { ctx, guild_id, db, manager };
        self.queue_player_with_vc_status(player, rank, queue_ctx, false).await
    }

    pub async fn queue_player_sg(
        &mut self,
        sg_id: u8,
        player: Player,
        rank: Rank,
        ctx: &Context,
        guild_id: Option<GI>,
        db: Option<&DB>,
        manager: Option<Arc<Mutex<Manager>>>,
    ) -> Result<()> {
        let queue_ctx = QueueContext { ctx, guild_id, db, manager };
        self.queue_player_with_vc_status_sg(sg_id, player, rank, queue_ctx, false).await
    }

    pub async fn queue_player_with_vc_status(&mut self, player: Player, _rank: Rank, queue_ctx: QueueContext<'_>, in_vc: bool) -> Result<()> {
        let was_empty = self.get_queue().await?.pool.is_empty();
        let session = self.get_queue().await?;

        if in_vc {session.add_player_in_vc(player);}
        else {session.add_player(player);}

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

    pub async fn queue_player_with_vc_status_sg(&mut self, sg_id: u8, player: Player, _rank: Rank, queue_ctx: QueueContext<'_>, in_vc: bool) -> Result<()> {
        let was_empty = self.get_queue_sg(sg_id).await?.pool.is_empty();
        let session = self.get_queue_sg(sg_id).await?;

        if in_vc {session.add_player_in_vc(player);}
        else {session.add_player(player);}

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

        if self.is_quota_sg(sg_id) {
            self.hot_sg(sg_id, queue_ctx.ctx, queue_ctx.guild_id, queue_ctx.db, queue_ctx.manager).await?;
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
        let current_count = self.formats[0].sessions.iter()
            .find(|s| s.status == SessionStatus::Idle)
            .map(|s| s.pool.len())
            .unwrap_or(0);

        // Get current channel name

        let current_name = match queue_vc.name(&ctx.http).await {
            Ok(name) => {

                name
            },
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
                Ok(_) => {},
                Err(e) => warn!("[UPDATE_VC_NAME] Failed to update channel name: {}", e),
            }
        }

    }

    pub async fn add_player(&mut self, session: &mut Session, player: Player, _rank: Rank, ctx: &Context, guild_id: GI) {
        session.add_player(player);
        self.queue_dash_update(ctx, guild_id).await;
    }

    /// Checks if this category contains the given channel_id in any of its channels
    pub fn contains_channel(&self, channel_id: CI) -> bool {
        self.channels.contains_channel(channel_id)
    }

    pub fn is_quota(&self) -> bool {
        self.is_quota_sg(0)
    }

    pub fn is_quota_sg(&self, sg_id: u8) -> bool {
        let g = self.get_sessions_by_status_sg(sg_id, &SessionStatus::Idle);
        if g.is_empty() {
            warn!("No idle sessions found when checking quota for format {}", sg_id);
            return false;
        }
        if g.len() > 1 {
            warn!("Multiple idle games found in format {}, faulty", sg_id);
        }
        let l = g[0].pool.len();
        let q = self.format(sg_id).map(|sg| sg.quota as usize).unwrap_or(0);
        match l.cmp(&q) {
            std::cmp::Ordering::Less    => false,
            std::cmp::Ordering::Equal   => true,
            std::cmp::Ordering::Greater => {
                warn!("Quota met late, more players than quota in format {}", sg_id);
                true
            },
        }
    }

    /// Notifies the queue chat that quota has been met
    /// Pings ALL players in the first 'quota' players, not just those missing from VC
    /// Only pings the first 'quota' players, not extras queued for next match
    /// Also sends DMs to players who have pm_hot_alert=true
    pub async fn notify(&mut self, ctx: &Context, guild_id: GI, db: Option<&DB>) {
        // Validate VC status before sending notifications to prevent desync
        self.validate_vc_status(ctx, guild_id).await;

        let queue_chat = self.channels.queue_chat;
        let mut player_mentions = Vec::new();
        let mut players_to_dm = Vec::new();
        let quota = self.formats[0].quota as usize;

        // Get the HOT session specifically, not the last session
        // This ensures we notify the correct players when quota is met
        if let Some(hot_session) = self.formats[0].sessions.iter()
            .find(|s| s.status == SessionStatus::Hot) {
            // Ping ALL players in the first 'quota' positions (not extras queued for next match)
            // This ensures everyone in the match gets notified, not just those missing from VC
            for player in hot_session.pool.iter().take(quota) {
                player_mentions.push(format!("<@{}>", player.player.user_id));
                players_to_dm.push(player.player.user_id);
            }
        } else {
            warn!("No hot session found when trying to notify players");
            return;
        }

        // Only log notification if there are actually players to notify
        if !player_mentions.is_empty() {
            let guild_name = guild_name(ctx, guild_id);
            let sg_name = &self.formats[0].name;
            let full_prefix = log_prefix_format(
                &guild_name, 
                self.name.as_deref().unwrap_or("unknown"), 
                sg_name
            );
            
            info!("{} Quota met - notifying all {} players in match", full_prefix, player_mentions.len());
        }

        // Use embed for header and raw pings in message content to properly ping users
        let embed = CreateEmbed::new()
            .title("PUG Starting")
            .description("Please join the queue channel!");

        let content = player_mentions.join(" ");
        let msg = CM::new().embed(embed).content(content);
        if let Ok(sent) = queue_chat.send_message(&ctx.http, msg).await {
            let http = ctx.http.clone();
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                let _ = sent.delete(&http).await;
            });
        }

        // Send DMs to users who have pm_hot_alert=true
        if let Some(database) = db {
            let dm_tracker = ctx.data.read().await.get::<crate::models::DmTrackerKey>().cloned();

            for user_id in players_to_dm {
                // Check if user has DM notifications enabled
                match database.users.get_pm_hot_alert(user_id).await {
                    Ok(true) => {
                        let dm_embed = CreateEmbed::new()
                            .title("PUG Ready!")
                            .description(format!(
                                "A game is ready in **{}**!\nPlease join the queue channel.",
                                ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "the server".to_string())
                            ))
                            .color(GREEN);

                        if let Some(ref tracker) = dm_tracker {
                            if let Err(e) = tracker.send_dm(ctx, user_id, dm_embed).await {
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
}

// Roles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roles {
    pub runner: RI,
    pub admin:  RI,
}

impl Roles {
    pub fn new(runner: RI, admin: RI) -> Self {
        Self { runner, admin }
    }
    pub fn empty() -> Self {
        Self {
            runner: RI::new(1),
            admin: RI::new(1),
        }
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
            Role::Admin  => "admin_role",
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
            },
            Role::Admin => {
                if let Ok(Some(role_id)) = db.config.get_admin_role_id(guild_id).await {
                    vec![role_id]
                } else {
                    Vec::new()
                }
            },
        }
    }

    /// Save a Discord role ID to the database configuration
    pub async fn save_id(&self, db: &DB, guild_id: GI, role_id: RI) -> anyhow::Result<()> {
        match self {
            Role::Runner => db.config.set_runner_role_id(guild_id, role_id).await,
            Role::Admin  => db.config.set_admin_role_id(guild_id, role_id).await,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Role::Runner => "Runner",
            Role::Admin  => "Admin",
        }
    }
}

// Channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
    pub category:   CI,
    pub queue_chat: CI,
    pub queue_vc:   CI,
    pub teams:      Vec<TeamChannel>,
    pub dashboard:  CI,
}

impl Channels {
    pub fn new(category: CI, queue_chat: CI, queue_vc: CI, teams: Vec<TeamChannel>, dashboard: CI) -> Self {
        Self {
            category,
            queue_chat,
            queue_vc,
            teams,
            dashboard,
        }
    }

    /// Pushs a red and blue channel to the vector
    pub fn add_team_channel_pair(&mut self, red_vc: CI, blu_vc: CI) {
        self.teams.push(TeamChannel::new(red_vc, blu_vc));
    }

    pub fn empty() -> Self {
        Self {
            category:   CI::new(1),
            queue_chat: CI::new(1),
            queue_vc:   CI::new(1),
            teams:      Vec::new(),
            dashboard:  CI::new(1),
        }
    }

    /// Checks if this struct contains the given channel_id
    pub fn contains_channel(&self, channel_id: CI) -> bool {
        self.queue_chat == channel_id
            || self.queue_vc  == channel_id
            || self.dashboard == channel_id
            || self.teams.iter().any(|team| team.contains_channel(channel_id))
    }

    /// Returns all known static channel IDs (category, chat, queue, dashboard, team VCs)
    pub fn known_channel_ids(&self) -> Vec<CI> {
        let mut ids = vec![self.category, self.queue_chat, self.queue_vc, self.dashboard];
        for team in &self.teams {
            ids.push(team.red_vc);
            ids.push(team.blu_vc);
        }
        ids
    }
}
