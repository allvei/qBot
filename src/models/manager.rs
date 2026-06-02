//! # Manager Module
//!
//! This module defines the Manager struct and its related functionality.
//! The Manager is responsible for managing multiple servers and their categories/games.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{Cache, ChannelId as CI, Context, GuildId as GI, MessageId as MI, UserId as UI};
use std::collections::HashMap;
use std::time::SystemTime;

use crate::models::{Category, QGuild, Roles, SessionStatus};

/// Manages multiple servers and their associated categories/games
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Manager {
  /// Collection of servers managed by this instance
  pub qguilds: Vec<QGuild>,
  /// Tracks active score submissions: (guild_id, category_id, format_id) -> (user_id, start_time)
  pub active_score_submissions: HashMap<(GI, u8, u8), (UI, SystemTime)>,
  /// Tracks active match starts: (guild_id, category_id, format_id) -> (user_id, start_time)
  pub active_match_starts: HashMap<(GI, u8, u8), (UI, SystemTime)>,
  /// Generic interaction lock to prevent duplicate processing of any destructive action
  /// Maps interaction_id -> (action_key, start_time)
  #[serde(skip)]
  pub active_interactions: HashMap<MI, (String, SystemTime)>,
}

impl Manager {
  /// Create a new empty manager
  ///
  /// ### Returns
  /// * A new Manager instance
  pub fn new(guild_id: GI) -> Self {
    Self {
      qguilds: vec![QGuild::new(guild_id, "Unknown".to_string(), Roles::empty())],
      active_score_submissions: HashMap::new(),
      active_match_starts: HashMap::new(),
      active_interactions: HashMap::new(),
    }
  }

  /// Pull server list from Discord cache
  ///
  /// ### Arguments
  /// * `cache` - Discord cache containing guild information
  ///
  /// ### Returns
  /// * A new Manager instance with servers from the cache
  pub fn pull_list(&mut self, cache: &Cache) -> Self {
    let mut qguilds = Vec::new();
    cache.guilds().iter().for_each(|g| {
      let guild_name = cache.guild(*g).map(|guild| guild.name.clone()).unwrap_or_else(|| "Unknown".to_string());
      qguilds.push(QGuild::new(*g, guild_name, Roles::empty()));
    });
    Self { qguilds, active_score_submissions: HashMap::new(), active_match_starts: HashMap::new(), active_interactions: HashMap::new() }
  }

  /// Find a server by its guild ID
  ///
  /// ### Arguments
  /// * `guild_id` - The Discord guild ID to find
  ///
  /// ### Returns
  /// * `Option<&Server>` - The server if found, None otherwise
  pub fn get_qguild(&mut self, guild_id: GI) -> Result<&mut QGuild> {
    let vec_len = self.qguilds.len();
    match self.qguilds.iter_mut().find(|s| s.id == guild_id) {
      Some(server) => Ok(server),
      None => Err(anyhow!("Server not found for guild ID: {}, Vec len: {}", guild_id, vec_len)),
    }
  }

  pub fn get_category_by_channel(&mut self, guild_id: GI, channel_id: CI) -> Result<&mut Category> {
    let server = self.get_qguild(guild_id)?;
    let category = server.categories.iter_mut().find(|g| g.contains_channel(channel_id));
    if let Some(category) = category {
      Ok(category)
    } else {
      Err(anyhow!("No queue category configured for this channel. Please run /setup first."))
    }
  }

  pub fn get_category_by_id(&mut self, guild_id: GI, category_id: u8) -> Result<&mut Category> {
    let server = self.get_qguild(guild_id)?;
    let category = server.categories.iter_mut().find(|g| g.id == category_id);
    if let Some(category) = category {
      Ok(category)
    } else {
      Err(anyhow!("Category {} not found for guild ID: {}", category_id, guild_id.get()))
    }
  }

  /// Update category state in the manager
  ///
  /// ### Arguments
  /// * `channel_id` - The channel ID of the category
  /// * `updated_category` - The updated category state
  pub fn update_category(&mut self, server: &mut QGuild, channel_id: CI, updated_category: Category) {
    if let Some(category) = server.categories.iter_mut().find(|g| g.contains_channel(channel_id)) {
      *category = updated_category;
    }
  }

  pub fn cleanup_empty_games(&mut self) {
    for server in &mut self.qguilds {
      for category in &mut server.categories {
        category.formats[0].sessions.retain(|game| !(game.status == SessionStatus::Idle && game.pool.is_empty()));
      }
    }
  }

  /// Check if a score submission is already in progress for a match
  ///
  /// ### Arguments
  /// * `guild_id` - The Discord guild ID
  /// * `category_id` - The category ID
  /// * `format_id` - The format ID
  ///
  /// ### Returns
  /// * `Option<UI>` - The user ID currently submitting, or None if no active submission
  pub fn get_active_score_submission(&self, guild_id: GI, category_id: u8, format_id: u8) -> Option<UI> {
    let key = (guild_id, category_id, format_id);
    self
      .active_score_submissions
      .get(&key)
      .map(|(user_id, start_time)| {
        // Remove stale submissions older than 5 minutes
        if start_time.elapsed().unwrap_or(std::time::Duration::from_secs(0)) > std::time::Duration::from_secs(300) {
          None
        } else {
          Some(*user_id)
        }
      })
      .flatten()
  }

  /// Set a score submission as in progress
  ///
  /// ### Arguments
  /// * `guild_id` - The Discord guild ID
  /// * `category_id` - The category ID
  /// * `format_id` - The format ID
  /// * `user_id` - The user ID starting the submission
  pub fn set_active_score_submission(&mut self, guild_id: GI, category_id: u8, format_id: u8, user_id: UI) {
    let key = (guild_id, category_id, format_id);
    self.active_score_submissions.insert(key, (user_id, SystemTime::now()));
  }

  /// Remove a score submission from active tracking
  ///
  /// ### Arguments
  /// * `guild_id` - The Discord guild ID
  /// * `category_id` - The category ID
  /// * `format_id` - The format ID
  pub fn clear_active_score_submission(&mut self, guild_id: GI, category_id: u8, format_id: u8) {
    let key = (guild_id, category_id, format_id);
    self.active_score_submissions.remove(&key);
  }

  /// Clean up stale score submissions (older than 5 minutes)
  pub fn cleanup_stale_score_submissions(&mut self) {
    let _now = SystemTime::now();
    let timeout = std::time::Duration::from_secs(300);
    self.active_score_submissions.retain(|_, (_, start_time)| start_time.elapsed().unwrap_or(timeout) < timeout);
  }

  /// Check if a match start is already in progress for a match
  ///
  /// ### Arguments
  /// * `guild_id` - The Discord guild ID
  /// * `category_id` - The category ID
  /// * `format_id` - The format ID
  ///
  /// ### Returns
  /// * `Option<UI>` - The user ID currently starting, or None if no active start
  pub fn get_active_match_start(&self, guild_id: GI, category_id: u8, format_id: u8) -> Option<UI> {
    let key = (guild_id, category_id, format_id);
    self
      .active_match_starts
      .get(&key)
      .map(|(user_id, start_time)| {
        // Remove stale starts older than 5 minutes
        if start_time.elapsed().unwrap_or(std::time::Duration::from_secs(0)) > std::time::Duration::from_secs(300) {
          None
        } else {
          Some(*user_id)
        }
      })
      .flatten()
  }

  /// Set a match start as in progress
  ///
  /// ### Arguments
  /// * `guild_id` - The Discord guild ID
  /// * `category_id` - The category ID
  /// * `format_id` - The format ID
  /// * `user_id` - The user ID starting the match
  pub fn set_active_match_start(&mut self, guild_id: GI, category_id: u8, format_id: u8, user_id: UI) {
    let key = (guild_id, category_id, format_id);
    self.active_match_starts.insert(key, (user_id, SystemTime::now()));
  }

  /// Remove a match start from active tracking
  ///
  /// ### Arguments
  /// * `guild_id` - The Discord guild ID
  /// * `category_id` - The category ID
  /// * `format_id` - The format ID
  pub fn clear_active_match_start(&mut self, guild_id: GI, category_id: u8, format_id: u8) {
    let key = (guild_id, category_id, format_id);
    self.active_match_starts.remove(&key);
  }

  /// Queue dashboard updates for every category that contains the player.
  /// Returns the number of dashboards that were queued for update.
  pub async fn queue_dash_updates_for_player(&mut self, ctx: &Context, guild_id: GI, user_id: UI) -> usize {
    match self.get_qguild(guild_id) {
      Ok(server) => {
        let mut updated = 0;
        for category in &mut server.categories {
          if category.contains_player(user_id) {
            category.queue_dash_update(ctx, guild_id).await;
            updated += 1;
          }
        }
        updated
      }
      Err(_) => 0,
    }
  }

  /// Validate and cleanup restored state after restart
  /// Removes stale data and validates that Discord entities still exist
  pub async fn validate_and_cleanup(&mut self, cache: &Cache) {
    use std::time::Duration;
    use tracing::{debug, warn};

    let session_max_age = Duration::from_secs(3600);

    self.qguilds.retain(|guild| {
      let exists = cache.guild(guild.id).is_some();
      if !exists {
        warn!("Removing guild {} from state: bot is no longer in this guild", guild.id);
      }
      exists
    });

    for guild in &mut self.qguilds {
      let guild_id = guild.id;
      let guild_cache = cache.guild(guild_id);

      for category in &mut guild.categories {
        let dashboard_exists = guild_cache.as_ref().and_then(|g| g.channels.get(&category.channels.dashboard)).is_some();

        if !dashboard_exists {
          warn!("Dashboard channel {} missing for category {} in guild {}", category.channels.dashboard, category.id, guild_id);
        }

        category.channels.teams.retain(|tc| {
          let red_exists = guild_cache.as_ref().and_then(|g| g.channels.get(&tc.red_vc)).is_some();
          let blu_exists = guild_cache.as_ref().and_then(|g| g.channels.get(&tc.blu_vc)).is_some();

          if !red_exists || !blu_exists {
            debug!("Removing stale team VC pair (red: {}, blu: {}) from state", tc.red_vc, tc.blu_vc);
            false
          } else {
            true
          }
        });

        for format in &mut category.formats {
          format.sessions.retain(|session| {
            if let Some(started_at) = session.started_at {
              let age = started_at.elapsed().unwrap_or_default();
              if age > session_max_age {
                warn!("Removing stale session from state (age: {}s, status: {:?})", age.as_secs(), session.status);
                return false;
              }
            }
            true
          });
        }
      }
    }

    self.cleanup_stale_score_submissions();

    debug!("State validation complete: {} guilds, {} categories", self.qguilds.len(), self.qguilds.iter().map(|g| g.categories.len()).sum::<usize>());
  }

  /// Count active sessions across all guilds
  pub fn count_active_sessions(&self) -> usize {
    self.qguilds.iter().map(|guild| guild.categories.iter().map(|cat| cat.formats.iter().map(|fmt| fmt.sessions.iter().filter(|s| s.is_active()).count()).sum::<usize>()).sum::<usize>()).sum()
  }

  /// Check if there are any sessions in progress
  pub fn has_active_sessions(&self) -> bool {
    self.count_active_sessions() > 0
  }

  /// Try to acquire a lock for a destructive interaction
  /// Returns true if lock was acquired, false if already in progress
  ///
  /// ### Arguments
  /// * `interaction_id` - The Discord interaction ID (unique per interaction)
  /// * `action_key` - A descriptive key for the action (e.g., "cancel_match_0_0")
  ///
  /// ### Returns
  /// * `true` if lock was acquired, `false` if action is already in progress
  pub fn try_lock_interaction(&mut self, interaction_id: MI, action_key: String) -> bool {
    use tracing::{debug, warn};

    // Check if this exact interaction is already being processed
    if let Some((existing_action, start_time)) = self.active_interactions.get(&interaction_id) {
      let elapsed = start_time.elapsed().unwrap_or(std::time::Duration::from_secs(0)).as_secs();
      warn!("Duplicate interaction detected: {} (existing: {}, age: {}s)", action_key, existing_action, elapsed);
      return false;
    }

    // Clean up stale interactions (older than 5 minutes)
    let timeout = std::time::Duration::from_secs(300);
    let before_count = self.active_interactions.len();
    self.active_interactions.retain(|id, (action, start_time)| {
      let age = start_time.elapsed().unwrap_or(timeout);
      if age >= timeout {
        warn!("Cleaned up stale interaction lock: {} (interaction_id: {}, age: {}s)", action, id, age.as_secs());
        false
      } else {
        true
      }
    });
    let cleaned = before_count - self.active_interactions.len();
    if cleaned > 0 {
      info!("Cleaned up {} stale interaction locks", cleaned);
    }

    // Acquire the lock
    debug!("Acquired interaction lock: {} (interaction_id: {})", action_key, interaction_id);
    self.active_interactions.insert(interaction_id, (action_key, SystemTime::now()));
    true
  }

  /// Release a lock for a completed interaction
  ///
  /// ### Arguments
  /// * `interaction_id` - The Discord interaction ID to unlock
  pub fn unlock_interaction(&mut self, interaction_id: MI) {
    use tracing::debug;

    if let Some((action_key, start_time)) = self.active_interactions.remove(&interaction_id) {
      let duration = start_time.elapsed().unwrap_or(std::time::Duration::from_secs(0));
      debug!("Released interaction lock: {} (interaction_id: {}, held for: {}ms)", action_key, interaction_id, duration.as_millis());
    } else {
      use tracing::warn;
      warn!("Attempted to unlock non-existent interaction: {}", interaction_id);
    }
  }

  /// Check if an interaction is currently locked
  ///
  /// ### Arguments
  /// * `interaction_id` - The Discord interaction ID to check
  ///
  /// ### Returns
  /// * `true` if locked, `false` if available
  pub fn is_interaction_locked(&self, interaction_id: MI) -> bool {
    self.active_interactions.contains_key(&interaction_id)
  }

  /// Get diagnostic information about active interaction locks
  ///
  /// ### Returns
  /// * Count of active locks and list of actions with their ages
  pub fn get_interaction_lock_stats(&self) -> (usize, Vec<(String, u64)>) {
    let count = self.active_interactions.len();
    let mut locks: Vec<(String, u64)> = self
      .active_interactions
      .values()
      .map(|(action, start_time)| {
        let age_secs = start_time.elapsed().unwrap_or(std::time::Duration::from_secs(0)).as_secs();
        (action.clone(), age_secs)
      })
      .collect();
    locks.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by age descending
    (count, locks)
  }

  /// Manually cleanup stale interaction locks (for periodic cleanup tasks)
  ///
  /// ### Returns
  /// * Number of stale locks removed
  pub fn cleanup_stale_interaction_locks(&mut self) -> usize {
    use tracing::warn;

    let timeout = std::time::Duration::from_secs(300);
    let before_count = self.active_interactions.len();
    
    self.active_interactions.retain(|id, (action, start_time)| {
      let age = start_time.elapsed().unwrap_or(timeout);
      if age >= timeout {
        warn!("Periodic cleanup: removed stale interaction lock: {} (interaction_id: {}, age: {}s)", action, id, age.as_secs());
        false
      } else {
        true
      }
    });
    
    before_count - self.active_interactions.len()
  }
}
