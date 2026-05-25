//! # Manager Module
//!
//! This module defines the Manager struct and its related functionality.
//! The Manager is responsible for managing multiple servers and their categories/games.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{Cache, ChannelId as CI, Context, GuildId as GI, UserId as UI};
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
}

impl Manager {
  /// Create a new empty manager
  ///
  /// ### Returns
  /// * A new Manager instance
  pub fn new(guild_id: GI) -> Self {
    Self { qguilds: vec![QGuild::new(guild_id, "Unknown".to_string(), Roles::empty())], active_score_submissions: HashMap::new() }
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
    Self { qguilds, active_score_submissions: HashMap::new() }
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
    let now = SystemTime::now();
    let timeout = std::time::Duration::from_secs(300);
    self.active_score_submissions.retain(|_, (_, start_time)| start_time.elapsed().unwrap_or(timeout) < timeout);
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
}
