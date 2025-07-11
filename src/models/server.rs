//! # Server Module
//!
//! This module defines the Server struct and its related functionality.
//! A Server represents a Discord guild with associated groups and sessions.

use serde::{Deserialize, Serialize};
use serenity::all::GuildId;
use tracing::info;

use crate::models::group::Group;
use crate::error::AppResult;

/// Represents a game server with IP and name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameServer {
    /// IP address of the game server
    pub ip: String,
    /// Name of the game server
    pub name: String,
}

/// Represents a Discord server (guild) with associated groups
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    /// Discord Guild ID
    pub guild_id: GuildId,
    /// Collection of groups in this server
    pub groups: Vec<Group>,
}

impl Server {
    /// Create a new server
    ///
    /// # Arguments
    /// * `guild_id` - Discord Guild ID
    /// * `group` - Optional initial group
    ///
    /// # Returns
    /// * A new Server instance
    pub fn new(
        guild_id: GuildId,
        group: Option<Group>,
    ) -> Self {
        info!("New server created for {}", guild_id);
        Self {
            guild_id,
            groups: if let Some(g) = group { vec![g] } else { Vec::new() },
        }
    }

    /// Get a reference to all groups in the server
    ///
    /// # Returns
    /// * Reference to the vector of groups
    pub fn groups(&self) -> &Vec<Group> {
        &self.groups
    }

    /// Get a mutable reference to all groups in the server
    ///
    /// # Returns
    /// * Mutable reference to the vector of groups
    pub fn groups_mut(&mut self) -> &mut Vec<Group> {
        &mut self.groups
    }

    /// Find a group by its queue channel ID
    ///
    /// # Arguments
    /// * `channel_id` - The queue channel ID to find
    ///
    /// # Returns
    /// * `Option<&Group>` - The group if found, None otherwise
    pub fn find_group_by_queue_channel(
        &self,
        channel_id: u64,
    ) -> Option<&Group> {
        self.groups.iter().find(|g| g.queue == channel_id)
    }

    /// Find a group by its queue channel ID (mutable)
    ///
    /// # Arguments
    /// * `channel_id` - The queue channel ID to find
    ///
    /// # Returns
    /// * `Option<&mut Group>` - The mutable group if found, None otherwise
    pub fn find_group_by_queue_channel_mut(
        &mut self,
        channel_id: u64,
    ) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.queue == channel_id)
    }
}
