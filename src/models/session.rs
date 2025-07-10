//! # Session Module
//!
//! This module defines the Session struct and its related functionality.
//! A Session represents a game session with players, teams, and status.

use rand::prelude::*;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tracing::{debug, info};

use crate::error::{AppError, AppResult};
use crate::models::player::{Player, Rank};
use crate::models::common::Team;
use crate::models::group::Group;

/// Type alias for Session ID
pub type SessionId = u16;

/// Division names for grouping sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DivName {
    /// New players division
    Newcomer,
    /// Experienced players division
    Journey,
}

/// Session status representing the current state of a session
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    /// Waiting for enough players to join
    Idle,
    /// Waiting for runners to start the session
    Hot,
    /// Moving players to the team channels
    Push,
    /// Game is active
    Live,
    /// Moving players back to the queue
    Pull,
}

/// Represents a team channel configuration with voice channels for each team
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannels {
    /// Red team voice channel ID
    pub red: u64,
    /// Blue team voice channel ID
    pub blue: u64,
}



/// Represents a game session within a group.
///
/// A Session manages a collection of players (the pool) and tracks the current status
/// of the game. It maintains a backreference to its parent Group via the group_id field.
/// Sessions progress through various states (Idle, Hot, Push, Live, Pull) as players
/// join, teams are formed, and the game proceeds.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Session {
    /// Unique identifier for this session
    pub id: u16,
    /// ID of the owning group (backreference)
    pub group_id: u64,
    /// Current status of the session (Idle, Hot, Push, Live, Pull)
    pub status: SessionStatus,
    /// Collection of players currently in this session
    pub pool: Vec<Player>,
}

impl Session {
    pub fn new(id: u16, group_id: u64) -> Self {
        let session = Self {
            id,
            group_id,
            pool: Vec::new(),
            status: SessionStatus::Idle,
        };
        info!("New session started with ID: {}", id);
        session
    }

    /// Change session status to Hot
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub fn hot(&mut self) -> crate::error::AppResult<()> {
        info!("Changing session {} status to HOT", self.id);
        self.status = SessionStatus::Hot;
        Ok(())
    }
    
    /// Get the group ID this session belongs to
    ///
    /// # Returns
    /// * `u64` - The group ID
    pub fn group_id(&self) -> u64 {
        self.group_id
    }
    
    /// Update the group ID this session belongs to
    ///
    /// # Arguments
    /// * `group_id` - The new group ID
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub fn set_group_id(&mut self, group_id: u64) -> crate::error::AppResult<()> {
        info!("Updating session {} group ID to {}", self.id, group_id);
        self.group_id = group_id;
        Ok(())
    }

    /// Change session status to Push
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub fn push(&mut self) -> crate::error::AppResult<()> {
        info!("Changing session {} status to PUSH", self.id);
        self.status = SessionStatus::Push;
        Ok(())
    }

    /// Change session status to Live
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub fn live(&mut self) -> crate::error::AppResult<()> {
        info!("Changing session {} status to LIVE", self.id);
        self.status = SessionStatus::Live;
        Ok(())
    }

    /// Change session status to Pull
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub fn pull(&mut self) -> crate::error::AppResult<()> {
        info!("Changing session {} status to PULL", self.id);
        self.status = SessionStatus::Pull;
        Ok(())
    }

    /// Generate balanced teams for the session using a snake draft pattern
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub fn generate_teams(&mut self) -> crate::error::AppResult<()> {
        info!("Generating teams for session {}", self.id);
        debug!("Cloned {} players for team assignment", self.pool.len());
        
        if self.pool.is_empty() {
            return Err(crate::error::AppError::SessionError(
                format!("Cannot generate teams for session {} with no players", self.id)
            ));
        }
        
        let mut rng = rand::rngs::ThreadRng::default();
        let mut players = self.pool.clone();

        // 1. First add buffered players to genpool (priority)
        let buffered_players = self.pool.iter().filter(|p| p.buffered).cloned().collect::<Vec<_>>();

        // 2. Fill remaining slots with non-buffered players
        let remaining_slots = 8 - buffered_players.len(); // Assuming 8 players per match
        let mut non_buffered = self.pool.iter().filter(|p| !p.buffered).cloned().collect::<Vec<_>>();

        // Take only what we need
        if non_buffered.len() > remaining_slots {
            non_buffered.truncate(remaining_slots);
        }

        // 3. Sort players by ELO in descending order
        players.sort_by(|a, b| {
            let a_elo = a.rank.unwrap_or(Rank::Beginner).elo();
            let b_elo = b.rank.unwrap_or(Rank::Beginner).elo();

            // Randomize order for players with identical ELO
            if a_elo == b_elo {
                if rng.random::<bool>() {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                b_elo.cmp(&a_elo)
            }
        });

        // 4. Distribute players in snake draft pattern (ABBAABBA)
        let mut team_a = Vec::new();
        let mut team_b = Vec::new();

        for (i, player) in players.iter().enumerate() {
            let mut player_clone = player.clone();

            // Snake draft pattern: 0->A, 1->B, 2->B, 3->A, 4->A, 5->B, 6->B, 7->A
            match i % 4 {
                0 | 3 => {
                    player_clone.set_team(Some(Team::Red));
                    team_a.push(player_clone);
                }
                _ => {
                    player_clone.set_team(Some(Team::Blue));
                    team_b.push(player_clone);
                }
            }
        }

        // Update the original players in the pool with their team assignments
        for player in &mut self.pool {
            if let Some(team_player) = team_a.iter().find(|p| p.discord_id == player.discord_id) {
                player.set_team(team_player.team);
            } else if let Some(team_player) = team_b.iter().find(|p| p.discord_id == player.discord_id) {
                player.set_team(team_player.team);
            }
        }

        info!("Teams generated for session {}: Team A: {} players, Team B: {} players", 
            self.id, team_a.len(), team_b.len());
            
        Ok(())
    }

    /// Get all players in the session
    ///
    /// # Returns
    /// * `Vec<Player>` - A vector containing clones of all players in the session
    pub fn get_members(&self) -> Vec<Player> {
        self.pool.iter().cloned().collect()
    }

    pub fn add_player(
        &mut self,
        player: &Player,
    ) -> crate::error::AppResult<()> {
        if self.pool.iter().any(|p| p.discord_id == player.discord_id) {
            return Err(crate::error::AppError::SessionError(
                format!("Player {} is already in the session", player.discord_id)
            ));
        }
        
        // Clone player and set backreferences
        let mut session_player = player.clone();
        session_player.session_id = Some(self.id);
        session_player.group_id = Some(self.group_id);
        
        info!("Adding player {} to session {}", player.discord_id, self.id);
        self.pool.push(session_player);
        Ok(())
    }

    /// Remove a player from the session by their Discord ID
    ///
    /// # Arguments
    /// * `discord_id` - The Discord ID of the player to remove
    ///
    /// # Returns
    /// * `Ok(())` if the player was successfully removed
    /// * `Err(AppError::SessionError)` if the player was not found in the session
    pub fn remove_player(
        &mut self,
        discord_id: u64,
    ) -> crate::error::AppResult<()> {
        if let Some(pos) = self.pool.iter().position(|p| p.discord_id == discord_id) {
            self.pool.remove(pos);
            info!("Player {} removed from session {}. Remaining players: {}", discord_id, self.id, self.pool.len());
            Ok(())
        } else {
            let error_msg = format!("Player {} not found in session {}", discord_id, self.id);
            info!("{}", error_msg);
            Err(crate::error::AppError::SessionError(error_msg))
        }
    }

    pub fn get_idle_session_by_vc<'a>(
        &self,
        channel_id: u64,
        groups: &'a [Group],
    ) -> Option<&'a Session> {
        for group in groups {
            if group.queue_channel == channel_id {
                return group.session.iter().find(|s| s.status == SessionStatus::Idle);
            }
        }
        None
    }
}


