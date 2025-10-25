//! # Manager Module
//!
//! This module defines the Manager struct and its related functionality.
//! The Manager is responsible for managing multiple servers and their groups/games.

use anyhow::{anyhow, Result};
use serenity::all::{Cache, GuildId};
use tracing::info;

use crate::models::game::*;
use crate::models::server::*;
use serenity::all::ChannelId;

/// Manages multiple servers and their associated groups/games
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
    pub fn new(guild_id: GuildId) -> Self {
        Self { servers: vec![Server::new(guild_id, Roles::empty())] }
    }

    /// Pull server list from Discord cache
    ///
    /// ### Arguments
    /// * `cache` - Discord cache containing guild information
    ///
    /// ### Returns
    /// * A new Manager instance with servers from the cache
    pub fn pull_list(
        &mut self,
        cache: &Cache,
    ) -> Self {
        let mut servers = Vec::new();
        cache.guilds().iter().for_each(|g| {
            servers.push(Server::new(*g, Roles::empty()));
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
    pub fn get_server(&mut self, guild_id: GuildId) -> Result<&mut Server> {
        match self.servers.iter_mut().find(|s| s.guild_id == guild_id) {
            Some(server) => Ok(server),
            None         => Err(anyhow!("Server not found for guild ID: {}", guild_id)),
        }
    }

    pub fn get_group(&mut self, guild_id: GuildId, channel_id: ChannelId) -> Result<&mut Group> {
        let server = self.get_server(guild_id).unwrap();
        let group = server.groups.iter_mut().find(|g| g.contains_channel(channel_id));
        if let Some(group) = group {
            Ok(group)
        } else {
            Err(anyhow!("Group not found for channel ID: {}", channel_id.get()))
        }
    }

    /// Update group state in the manager
    ///
    /// ### Arguments
    /// * `channel_id` - The channel ID of the group
    /// * `updated_group` - The updated group state
    pub fn update_group(&mut self, server: &mut Server, channel_id: ChannelId, updated_group: Group) {        
        if let Some(group) = server.groups.iter_mut().find(|g| g.contains_channel(channel_id)) {
            *group = updated_group;
        }
    }

    /// Clean up empty games across all groups
    pub fn cleanup_empty_games(&mut self) {
        for server in &mut self.servers {
            for group in &mut server.groups {
                group.games.retain(|game| {
                    !(game.status == GameStatus::Idle && game.pool.is_empty())
                });
            }
        }
    }
}
