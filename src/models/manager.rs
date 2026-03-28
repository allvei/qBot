//! # Manager Module
//!
//! This module defines the Manager struct and its related functionality.
//! The Manager is responsible for managing multiple servers and their categories/games.

use anyhow::{anyhow, Result};
use serenity::all::{Cache, ChannelId as CI, GuildId as GI};

use crate::models::{Category, Roles, Server, SessionStatus};

/// Manages multiple servers and their associated categories/games
#[derive(Default, Clone)]
pub struct Manager {
  /// Collection of servers managed by this instance
  pub servers: Vec<Server>,
}

impl Manager {
  /// Create a new empty manager
  ///
  /// ### Returns
  /// * A new Manager instance
  pub fn new(guild_id: GI) -> Self {
    Self { servers: vec![Server::new(guild_id, "Unknown".to_string(), Roles::empty())] }
  }

  /// Pull server list from Discord cache
  ///
  /// ### Arguments
  /// * `cache` - Discord cache containing guild information
  ///
  /// ### Returns
  /// * A new Manager instance with servers from the cache
  pub fn pull_list(&mut self, cache: &Cache) -> Self {
    let mut servers = Vec::new();
    cache.guilds().iter().for_each(|g| {
      let guild_name = cache.guild(*g).map(|guild| guild.name.clone()).unwrap_or_else(|| "Unknown".to_string());
      servers.push(Server::new(*g, guild_name, Roles::empty()));
    });
    Self { servers }
  }

  /// Find a server by its guild ID
  ///
  /// ### Arguments
  /// * `guild_id` - The Discord guild ID to find
  ///
  /// ### Returns
  /// * `Option<&Server>` - The server if found, None otherwise
  pub fn get_server(&mut self, guild_id: GI) -> Result<&mut Server> {
    match self.servers.iter_mut().find(|s| s.guild_id == guild_id) {
      Some(server) => Ok(server),
      None => Err(anyhow!("Server not found for guild ID: {}", guild_id)),
    }
  }

  pub fn get_category_by_channel(&mut self, guild_id: GI, channel_id: CI) -> Result<&mut Category> {
    let server = self.get_server(guild_id)?;
    let category = server.categories.iter_mut().find(|g| g.contains_channel(channel_id));
    if let Some(category) = category {
      Ok(category)
    } else {
      Err(anyhow!("No queue category configured for this channel. Please run /setup first."))
    }
  }

  pub fn get_category_by_id(&mut self, guild_id: GI, category_id: u8) -> Result<&mut Category> {
    let server = self.get_server(guild_id)?;
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
  pub fn update_category(&mut self, server: &mut Server, channel_id: CI, updated_category: Category) {
    if let Some(category) = server.categories.iter_mut().find(|g| g.contains_channel(channel_id)) {
      *category = updated_category;
    }
  }

  pub fn cleanup_empty_games(&mut self) {
    for server in &mut self.servers {
      for category in &mut server.categories {
        category.formats[0].sessions.retain(|game| !(game.status == SessionStatus::Idle && game.pool.is_empty()));
      }
    }
  }
}
