//! # Session Module
//!
//! This module defines the Session struct and its related functionality.
//! A Session represents a game session with players, teams, and status.

use rand::prelude::*;
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, GuildId, UserId};
use sqlx::FromRow;
use tracing::{debug, info};

use crate::discord::commands::CommandResponse;
use crate::discord::utils::{move_user_to_channel, send_response};
use crate::models::common::Team;
use crate::models::group::Group;
use crate::models::player::{Player, Rank};

/// Calculate statistical metrics for a set of ELO values
///
/// # Arguments
/// * `elos` - A slice of ELO values
///
/// # Returns
/// * `(f64, f64, f64)` - (mean, median, standard deviation)
fn calculate_stats(elos: &[u32]) -> (f64, f64, f64) {
    if elos.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    // Calculate mean
    let sum: u32 = elos.iter().sum();
    let mean = sum as f64 / elos.len() as f64;

    // Calculate median
    let mut sorted_elos = elos.to_vec();
    sorted_elos.sort_unstable();

    let median = if elos.len() % 2 == 0 {
        let mid = elos.len() / 2;
        (sorted_elos[mid - 1] as f64 + sorted_elos[mid] as f64) / 2.0
    } else {
        sorted_elos[elos.len() / 2] as f64
    };

    // Calculate standard deviation
    let variance = elos
        .iter()
        .map(|&x| {
            let diff = mean - (x as f64);
            diff * diff
        })
        .sum::<f64>()
        / elos.len() as f64;

    let std_dev = variance.sqrt();

    (mean, median, std_dev)
}

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
    pub red:  u64,
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
    pub id:       u16,
    /// ID of the owning group (backreference)
    pub group_id: u64,
    /// Current status of the session (Idle, Hot, Push, Live, Pull)
    pub status:   SessionStatus,
    /// Collection of players currently in this session
    pub pool:     Vec<Player>,
}

impl Session {
    pub fn new(
        id: u16,
        group_id: u64,
    ) -> Self {
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
    /// This method performs the following actions:
    /// 1. Updates the session status to Hot
    /// 2. Notifies players that the session is ready to start
    /// 3. Prepares team assignments if not already done
    ///
    /// # Arguments
    /// * `ctx` - Discord context for API access
    /// * `group` - Reference to the parent group containing this session
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub async fn hot<'a>(
        &mut self,
        ctx: &serenity::prelude::Context,
        group: &'a Group,
    ) -> crate::error::AppResult<()> {
        info!("Changing session {} status to HOT", self.id);

        // Update status
        self.status = SessionStatus::Hot;

        // Generate teams if not already done
        if self.pool.iter().any(|p| p.team.is_none()) {
            self.generate_teams()?;
        }

        // Notify players in the dashboard channel
        let dashboard_id = ChannelId::new(group.dashboard);
        let red_players: Vec<_> = self.pool.iter().filter(|p| p.team == Some(Team::Red)).map(|p| format!("<@{}>", p.discord_id)).collect();

        let blue_players: Vec<_> = self.pool.iter().filter(|p| p.team == Some(Team::Blue)).map(|p| format!("<@{}>", p.discord_id)).collect();

        let notification = CommandResponse::Embed {
            title:       format!("Session {} is HOT!", self.id),
            description: format!(
                "The session is ready to start!\n\n\
                **Red Team:** {}\n\
                **Blue Team:** {}\n\n\
                Please prepare to be moved to your team channels.",
                red_players.join(", "),
                blue_players.join(", ")
            ),
            color:       Some((255, 165, 0)), // Orange color for HOT status
        };

        send_response(ctx, dashboard_id, notification).await?;

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
    pub fn set_group_id(
        &mut self,
        group_id: u64,
    ) -> crate::error::AppResult<()> {
        info!("Updating session {} group ID to {}", self.id, group_id);
        self.group_id = group_id;
        Ok(())
    }

    /// Change session status to Push and move players to their team channels
    ///
    /// This method performs the following actions:
    /// 1. Updates the session status to Push
    /// 2. Moves players from the queue channel to their assigned team channels
    /// 3. Notifies players that the match is starting
    ///
    /// # Arguments
    /// * `ctx` - Discord context for API access
    /// * `group` - Reference to the parent group containing this session
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub async fn push<'a>(
        &mut self,
        ctx: &serenity::prelude::Context,
        group: &'a Group,
    ) -> crate::error::AppResult<()> {
        info!("Changing session {} status to PUSH", self.id);

        // Update status
        self.status = SessionStatus::Push;

        // Ensure teams are assigned
        if self.pool.iter().any(|p| p.team.is_none()) {
            self.generate_teams()?;
        }

        // Get guild ID
        let guild_id = GuildId::new(group.guild_id);

        // Find the team channels
        if group.teams.is_empty() {
            return Err(crate::error::AppError::SessionError(format!("No team channels configured for group {}", group.guild_id)));
        }

        // Use the first team channels configuration
        let team_channels = &group.teams[0];
        let red_channel = ChannelId::new(team_channels.red);
        let blue_channel = ChannelId::new(team_channels.blue);

        // Move players to their team channels
        for player in &self.pool {
            let user_id = UserId::new(player.discord_id);

            match player.team {
                Some(Team::Red) => {
                    move_user_to_channel(ctx, guild_id, user_id, red_channel).await?;
                }
                Some(Team::Blue) => {
                    move_user_to_channel(ctx, guild_id, user_id, blue_channel).await?;
                }
                None => {
                    // Skip players without team assignment
                    info!("Player {} has no team assignment, skipping move", player.discord_id);
                }
            }
        }

        // Notify in dashboard channel
        let dashboard_id = ChannelId::new(group.dashboard);
        let notification = CommandResponse::Embed {
            title:       format!("Session {} is starting!", self.id),
            description: "Players have been moved to their team channels. The match is now starting!".to_string(),
            color:       Some((0, 255, 0)), // Green color for PUSH status
        };

        send_response(ctx, dashboard_id, notification).await?;

        Ok(())
    }

    /// Change session status to Live and mark the match as in progress
    ///
    /// This method performs the following actions:
    /// 1. Updates the session status to Live
    /// 2. Notifies players that the match is now live
    /// 3. Updates any necessary match tracking information
    ///
    /// # Arguments
    /// * `ctx` - Discord context for API access
    /// * `group` - Reference to the parent group containing this session
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub async fn live<'a>(
        &mut self,
        ctx: &serenity::prelude::Context,
        group: &'a Group,
    ) -> crate::error::AppResult<()> {
        info!("Changing session {} status to LIVE", self.id);

        // Update status
        self.status = SessionStatus::Live;

        // Notify in dashboard channel
        let dashboard_id = ChannelId::new(group.dashboard);
        let notification = CommandResponse::Embed {
            title:       format!("Session {} is now LIVE!", self.id),
            description: "The match is now in progress. Good luck and have fun!".to_string(),
            color:       Some((0, 0, 255)), // Blue color for LIVE status
        };

        send_response(ctx, dashboard_id, notification).await?;

        // Notify in chat channel
        let chat_id = ChannelId::new(group.chat);
        let chat_message = CommandResponse::Text(format!("@here Session {} is now LIVE! The match is in progress.", self.id));

        send_response(ctx, chat_id, chat_message).await?;

        Ok(())
    }

    /// Change session status to Pull, move players back to queue, and reset team assignments
    ///
    /// This method performs the following actions:
    /// 1. Updates the session status to Pull
    /// 2. Moves players from team channels back to the queue channel
    /// 3. Resets team assignments for all players
    /// 4. Notifies players that the match has ended
    ///
    /// # Arguments
    /// * `ctx` - Discord context for API access
    /// * `group` - Reference to the parent group containing this session
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub async fn pull<'a>(
        &mut self,
        ctx: &serenity::prelude::Context,
        group: &'a Group,
    ) -> crate::error::AppResult<()> {
        info!("Changing session {} status to PULL", self.id);

        // Update status
        self.status = SessionStatus::Pull;

        // Get guild ID and queue channel
        let guild_id = GuildId::new(group.guild_id);
        let queue_channel = ChannelId::new(group.queue);

        // Move all players back to queue channel and reset team assignments
        for player in &mut self.pool {
            let user_id = UserId::new(player.discord_id);

            // Move player to queue channel
            move_user_to_channel(ctx, guild_id, user_id, queue_channel).await?;

            // Reset team assignment
            player.set_team(None);
        }

        // Notify in dashboard channel
        let dashboard_id = ChannelId::new(group.dashboard);
        let notification = CommandResponse::Embed {
            title:       format!("Session {} has ended", self.id),
            description: "The match has ended. Players have been moved back to the queue channel.".to_string(),
            color:       Some((128, 128, 128)), // Gray color for PULL status
        };

        send_response(ctx, dashboard_id, notification).await?;

        // Notify in chat channel
        let chat_id = ChannelId::new(group.chat);
        let chat_message = CommandResponse::Text(format!("Session {} has ended. Thanks for playing!", self.id));

        send_response(ctx, chat_id, chat_message).await?;

        Ok(())
    }

    /// Generate balanced teams for the session using the Balanced Composite Heuristic (BCH) algorithm
    ///
    /// BCH evaluates all possible team splits and selects the one with the lowest combined score
    /// of differences in average ELO, median ELO, and standard deviation of ELO between teams.
    ///
    /// # Returns
    /// * `AppResult<()>` - Success or failure with error context
    pub fn generate_teams(&mut self) -> crate::error::AppResult<()> {
        use itertools::Itertools;
        use std::collections::HashSet;

        info!("Generating teams for session {} using BCH algorithm", self.id);
        debug!("Evaluating {} players for team assignment", self.pool.len());

        if self.pool.is_empty() {
            return Err(crate::error::AppError::SessionError(format!("Cannot generate teams for session {} with no players", self.id)));
        }

        // Ensure we have an even number of players
        if self.pool.len() % 2 != 0 {
            return Err(crate::error::AppError::SessionError(format!(
                "Cannot generate balanced teams with odd number of players: {}",
                self.pool.len()
            )));
        }

        // 1. First add buffered players to genpool (priority)
        let buffered_players = self.pool.iter().filter(|p| p.buffered).cloned().collect::<Vec<_>>();

        // 2. Fill remaining slots with non-buffered players
        let team_size = self.pool.len() / 2; // Each team should have half the players
        let remaining_slots = (team_size * 2) - buffered_players.len();
        let mut non_buffered = self.pool.iter().filter(|p| !p.buffered).cloned().collect::<Vec<_>>();

        // Take only what we need
        if non_buffered.len() > remaining_slots {
            non_buffered.truncate(remaining_slots);
        }

        // Combine buffered and non-buffered players
        let mut players = Vec::new();
        players.extend(buffered_players);
        players.extend(non_buffered);

        // If we don't have enough players for two teams, return error
        if players.len() < 2 {
            return Err(crate::error::AppError::SessionError(format!("Not enough players to form teams: {}", players.len())));
        }

        // Get player ELO values
        let player_elos: Vec<(usize, u32)> = players.iter().enumerate().map(|(idx, p)| (idx, p.rank.unwrap_or(Rank::Beginner).elo())).collect();

        // For small player counts, we can evaluate all combinations
        // For larger counts, we might need to limit or sample
        let half_size = players.len() / 2;
        let max_combinations = 10000; // Limit combinations to evaluate for performance

        // Track best team split
        let mut best_score = f64::MAX;
        let mut best_team_a_indices = HashSet::new();

        // Generate all possible combinations of player indices for team A
        // Team B will be the complement of these indices
        let combinations = (0..players.len()).combinations(half_size);

        // Count combinations for logging
        let total_combinations = combinations.clone().count();
        info!("Evaluating {} possible team combinations", total_combinations);

        // Limit combinations if there are too many
        let combinations_to_evaluate = if total_combinations > max_combinations {
            debug!("Limiting to {} combinations for performance", max_combinations);
            combinations.take(max_combinations)
        } else {
            combinations.take(total_combinations)
        };

        // Evaluate each possible team split
        for team_a_indices in combinations_to_evaluate {
            let team_a_indices_set: HashSet<usize> = team_a_indices.iter().cloned().collect();

            // Calculate team A stats
            let team_a_elos: Vec<u32> = team_a_indices_set.iter().map(|&idx| player_elos[idx].1).collect();

            // Calculate team B stats (all players not in team A)
            let team_b_elos: Vec<u32> = (0..players.len()).filter(|idx| !team_a_indices_set.contains(idx)).map(|idx| player_elos[idx].1).collect();

            // Calculate mean, median, and standard deviation for both teams
            let (avg_a, med_a, std_a) = calculate_stats(&team_a_elos);
            let (avg_b, med_b, std_b) = calculate_stats(&team_b_elos);

            // Calculate the score (lower is better)
            let score = (avg_a - avg_b).abs() + (med_a - med_b).abs() + (std_a - std_b).abs();

            // Update best team if this is better
            if score < best_score {
                best_score = score;
                best_team_a_indices = team_a_indices_set;
            }
        }

        // Create the teams based on the best split found
        let mut team_a = Vec::new();
        let mut team_b = Vec::new();

        for (idx, player) in players.iter().enumerate() {
            let mut player_clone = player.clone();

            if best_team_a_indices.contains(&idx) {
                player_clone.set_team(Some(Team::Red));
                team_a.push(player_clone);
            } else {
                player_clone.set_team(Some(Team::Blue));
                team_b.push(player_clone);
            }
        }

        info!("BCH algorithm selected optimal team split with score: {}", best_score);

        // Update the original players in the pool with their team assignments
        for player in &mut self.pool {
            if let Some(team_player) = team_a.iter().find(|p| p.discord_id == player.discord_id) {
                player.set_team(team_player.team);
            } else if let Some(team_player) = team_b.iter().find(|p| p.discord_id == player.discord_id) {
                player.set_team(team_player.team);
            }
        }

        info!("Teams generated for session {}: Team A: {} players, Team B: {} players", self.id, team_a.len(), team_b.len());

        Ok(())
    }

    /// Get all players in the session
    ///
    /// # Returns
    /// * `Vec<Player>` - A vector containing clones of all players in the session
    pub fn get_members(&self) -> Vec<Player> {
        self.pool.to_vec()
    }

    pub fn add_player(
        &mut self,
        player: &Player,
    ) -> crate::error::AppResult<()> {
        if self.pool.iter().any(|p| p.discord_id == player.discord_id) {
            return Err(crate::error::AppError::SessionError(format!("Player {} is already in the session", player.discord_id)));
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
            if group.queue == channel_id {
                return group.session.iter().find(|s| s.status == SessionStatus::Idle);
            }
        }
        None
    }
}
