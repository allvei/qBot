//! # Manager Module
//!
//! This module defines the Manager struct and its related functionality.
//! The Manager is responsible for managing multiple servers and their groups/sessions.

use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serenity::all::{Cache, GuildId};
use tracing::{info, error};

use crate::models::server::Server;
use crate::models::data::Group;
use serenity::all::ChannelId;

/// Manages multiple servers and their associated groups/sessions
#[derive(Default, Clone)]
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

    pub fn get_group(&self, guild_id: GuildId, channel_id: ChannelId) -> Result<&Group> {
        let server = self.find_server_by_guild_id(guild_id.get()).unwrap();
        let group = server.groups.iter().find(|g| g.channel_exists(&channel_id));
        if let Some(group) = group {
            Ok(group)
        } else {
            Err(anyhow!("Group not found for channel ID: {}", channel_id.get()))
        }
    }

    /// Get or create a group for the given channel, maintaining session state
    ///
    /// # Arguments
    /// * `channel_id` - The channel ID to find the group for
    /// * `base_group` - Base group configuration from database
    ///
    /// # Returns
    /// * The group with maintained session state
    pub fn get_or_create_group(&mut self, channel_id: ChannelId, group: &Group) -> &mut Group {
        let guild_id = group.channels.queue.get(); // Use queue channel as guild identifier
        let queue_channel_id = group.channels.queue.get(); // Use the group's queue channel for lookups
        
        // Find or create server
        if self.find_server_by_guild_id(guild_id).is_none() {
            let server = Server::new(serenity::all::GuildId::new(guild_id), Some(group.clone()));
            self.servers.push(server);
        }
        
        // Find server and check if group exists
        let server = self.find_server_by_guild_id_mut(guild_id).unwrap();
        let group_exists = server.find_group_by_queue_channel(queue_channel_id).is_some();
        
        if !group_exists {
            // Create new group if not found
            server.groups.push(group.clone());
        }
        
        // Return the group (either existing or newly created)
        server.find_group_by_queue_channel_mut(queue_channel_id)
            .unwrap_or_else(|| {
                error!("Failed to find group after creation. Queue Channel ID: {}, Guild ID: {}, Original Channel: {}", queue_channel_id, guild_id, channel_id.get());
                panic!("Group should exist after creation but was not found");
            })
    }

    /// Update group state in the manager
    ///
    /// # Arguments
    /// * `channel_id` - The channel ID of the group
    /// * `updated_group` - The updated group state
    pub fn update_group(&mut self, channel_id: ChannelId, updated_group: Group) {
        let guild_id = updated_group.channels.queue.get();
        
        if let Some(server) = self.find_server_by_guild_id_mut(guild_id) {
            if let Some(group) = server.find_group_by_queue_channel_mut(channel_id.get()) {
                *group = updated_group;
            }
        }
    }

    /// Clean up empty sessions across all groups
    pub fn cleanup_empty_sessions(&mut self) {
        for server in &mut self.servers {
            for group in &mut server.groups {
                group.sessions.retain(|session| {
                    !(session.status == crate::models::data::SessionStatus::Idle && session.pool.is_empty())
                });
            }
        }
    }
}
