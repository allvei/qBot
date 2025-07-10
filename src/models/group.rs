//! # Group Module
//!
//! This module defines the Group struct and its related functionality.
//! A Group represents a collection of sessions and channels for a specific division.

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::models::session::Session;

/// Represents a group within a server, containing sessions and channel information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Unique identifier for the group
    pub id: u64,
    /// Name of the division this group belongs to
    pub div_name: String,
    /// ID of the queue voice channel
    pub queue_channel: u64,
    /// ID of the team A voice channel
    pub team_a_channel: u64,
    /// ID of the team B voice channel
    pub team_b_channel: u64,
    /// Collection of sessions in this group
    pub session: Vec<Session>,
}

impl Group {
    /// Create a new group
    ///
    /// # Arguments
    /// * `id` - Unique identifier for the group
    /// * `div_name` - Name of the division
    /// * `queue_channel` - ID of the queue voice channel
    /// * `team_a_channel` - ID of the team A voice channel
    /// * `team_b_channel` - ID of the team B voice channel
    ///
    /// # Returns
    /// * A new Group instance
    pub fn new(
        id: u64,
        div_name: String,
        queue_channel: u64,
        team_a_channel: u64,
        team_b_channel: u64,
    ) -> Self {
        info!("New group created: {} ({})", div_name, id);
        Self {
            id,
            div_name,
            queue_channel,
            team_a_channel,
            team_b_channel,
            session: vec![Session::new(1, id)],
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
}
