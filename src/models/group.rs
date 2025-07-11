//! # Group Module
//!
//! This module defines the Group struct and its related functionality.
//! A Group represents a collection of sessions and channels for a specific division.

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::models::session::{Session, TeamChannels};

/// Represents a group within a server, containing sessions and channel information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Guild ID this group belongs to
    pub guild_id: u64,
    /// Dashboard channel ID
    pub dashboard: u64,
    /// Chat channel ID
    pub chat: u64,
    /// Queue voice channel ID
    pub queue: u64,
    /// Team channels (red/blue)
    pub teams: Vec<TeamChannels>,
    /// Collection of sessions in this group
    pub session: Vec<Session>,
    /// Session increment counter
    pub session_increment: u16,
    /// Maximum number of sessions allowed
    pub session_quota: u8,
}

impl Group {
    /// Create a new group
    ///
    /// # Arguments
    /// * `guild_id` - Guild ID this group belongs to
    /// * `dashboard` - Dashboard channel ID
    /// * `chat` - Chat channel ID
    /// * `queue` - Queue voice channel ID
    /// * `red` - Red team channel ID
    /// * `blue` - Blue team channel ID
    /// * `session_quota` - Maximum number of sessions allowed
    ///
    /// # Returns
    /// * A new Group instance
    pub fn new(
        guild_id: u64,
        dashboard: u64,
        chat: u64,
        queue: u64,
        red: u64,
        blue: u64,
        session_quota: u8,
    ) -> Self {
        info!("New group created for guild: {}", guild_id);
        Self {
            guild_id,
            dashboard,
            chat,
            queue,
            teams: vec![TeamChannels {
                red,
                blue,
            }],
            session: Vec::new(),
            session_increment: 0,
            session_quota,
        }
    }

    /// Find a session by its ID
    ///
    /// # Arguments
    /// * `id` - The session ID to find
    ///
    /// # Returns
    /// * `Option<&Session>` - The session if found, None otherwise
    pub fn find_session_by_id(&self, id: u16) -> Option<&Session> {
        self.session.iter().find(|s| s.id == id)
    }

    /// Find a session by its ID (mutable)
    ///
    /// # Arguments
    /// * `id` - The session ID to find
    ///
    /// # Returns
    /// * `Option<&mut Session>` - The mutable session if found, None otherwise
    pub fn find_session_by_id_mut(&mut self, id: u16) -> Option<&mut Session> {
        self.session.iter_mut().find(|s| s.id == id)
    }

    /// Creates a new session and adds it to the group
    ///
    /// This increments the session counter and creates a new empty session
    /// with the next available ID.
    ///
    /// # Returns
    /// * Reference to the newly created session
    pub fn create_session(&mut self) -> &Session {
        // Increment the session counter
        self.session_increment += 1;
        
        // Create a new session with the incremented ID
        let new_session = Session::new(self.session_increment, self.guild_id);
        
        // Add the session to the group
        self.session.push(new_session);
        
        // Return a reference to the newly created session
        self.session.last().unwrap()
    }
}
