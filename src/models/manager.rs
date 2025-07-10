//! # Manager Module
//!
//! This module defines the Manager struct and its related functionality.
//! The Manager is responsible for managing multiple servers and their groups/sessions.

use serenity::all::Cache;
use tracing::info;

use crate::models::server::Server;
use crate::error::AppResult;

/// Manages multiple servers and their associated groups/sessions
#[derive(Default)]
pub struct Manager {
    /// Collection of servers managed by this instance
    pub servers: Vec<Server>,
}

impl Manager {
    /// Create a new empty manager
    ///
    /// # Returns
    /// * A new Manager instance
    pub fn new() -> Self {
        info!("Creating new Manager instance");
        Self { servers: Vec::new() }
    }

    /// Pull server list from Discord cache
    ///
    /// # Arguments
    /// * `cache` - Discord cache containing guild information
    ///
    /// # Returns
    /// * A new Manager instance with servers from the cache
    pub fn pull_list(
        &mut self,
        cache: &Cache,
    ) -> Self {
        info!("Pulling server list from Discord cache");
        let mut servers = Vec::new();
        cache.guilds().iter().for_each(|g| {
            servers.push(Server::new(*g, None));
        });
        Self { servers }
    }
    
    /// Find a server by its guild ID
    ///
    /// # Arguments
    /// * `guild_id` - The Discord guild ID to find
    ///
    /// # Returns
    /// * `Option<&Server>` - The server if found, None otherwise
    pub fn find_server_by_guild_id(&self, guild_id: u64) -> Option<&Server> {
        self.servers.iter().find(|s| s.guild_id.get() == guild_id)
    }
    
    /// Find a server by its guild ID (mutable)
    ///
    /// # Arguments
    /// * `guild_id` - The Discord guild ID to find
    ///
    /// # Returns
    /// * `Option<&mut Server>` - The mutable server if found, None otherwise
    pub fn find_server_by_guild_id_mut(&mut self, guild_id: u64) -> Option<&mut Server> {
        self.servers.iter_mut().find(|s| s.guild_id.get() == guild_id)
    }
}
