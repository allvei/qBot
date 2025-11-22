//! # Server Module
//!
//! This module defines the Server struct and its related functionality.
//! A Server represents a Discord guild with associated groups and games.

use anyhow::{anyhow, Error, Result};
use crate::models::constants::DEFAULT_TIMEOUT;
use serde::{Deserialize, Serialize};
use serenity::all::{
    ButtonStyle, ChannelId as CI, Context, CreateActionRow,
    CreateButton, CreateEmbed, CreateEmbedFooter as CEF,
    CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM,
    CreateMessage as CM, GuildId as GI, Message, MessageId as MI, RoleId as RI,
    UserId as UI,
};
use tracing::{info, warn};

use crate::handlers::player::check_role;
use crate::models::{
    CommandContext, Player, SessionPlayer, Session, SessionStatus, TeamChannel,
};

/// Helper function to calculate mean, median, and standard deviation for team ELOs
fn calculate_stats(elos: &[f64]) -> (f64, f64, f64) {
    if elos.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    // Calculate mean
    let sum: f64 = elos.iter().sum();
    let mean = sum / elos.len() as f64;

    // Calculate median
    let mut sorted = elos.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if sorted.len() % 2 == 0 {
        let mid = sorted.len() / 2;
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    };

    // Calculate standard deviation
    let variance: f64 = elos.iter().map(|&elo| (elo - mean).powi(2)).sum::<f64>() / elos.len() as f64;
    let std_dev = variance.sqrt();

    (mean, median, std_dev)
}


/// Represents a game server with IP and name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameServer {
    pub ip:   String,
    pub name: String,
}

// Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub guild_id:   GI,
    pub guild_name: String,
    pub roles:      Roles,
    pub groups:     Vec<Group>,
}

impl Server {
    pub fn new(
        guild_id:   GI,
        guild_name: String,
        roles:      Roles,
    ) -> Self {
        Self {
            guild_id,
            guild_name,
            roles,
            groups: Vec::new(),
        }
    }

    pub fn add_group(
        &mut self,
        group: Group,
    ) -> Result<()> {
        self.groups.push(group);
        if let Some(group) = self.groups.last_mut() {
            group.create_session();
        }
        Ok(())
    }

    pub fn empty(guild_id: GI, guild_name: String) -> Self {
        Self {
            guild_id,
            guild_name,
            roles: Roles::empty(),
            groups: Vec::new(),
        }
    }

    pub fn get_group(
        &mut self,
        channel_id: CI,
    ) -> Result<&mut Group> {
        match self.groups.iter_mut().find(|group| group.contains_channel(channel_id)) {
            Some(group) => Ok(group),
            None => Err(anyhow!("Group not found")),
        }
    }
}

// Group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub group_id:      u8,
    pub timeout:       u16,
    pub quota:         u8,
    pub dashboard_msg: MI,
    pub channels:      Channels,
    pub sessions:      Vec<Session>,
    pub connect_info:  Option<String>,
}

impl Group {
    pub fn new(
        group_id:      u8,
        quota:         u8,
        timeout:       u16,
        dashboard_msg: MI,
        channels:      Channels,
        games:         Vec<Session>,
    ) -> Self {
        Self {
            group_id,
            quota,
            timeout,
            dashboard_msg,
            channels,
            sessions: games,
            connect_info: None,
        }
    }

    pub fn create_session(&mut self) -> Result<&mut Session> {
        // Check if an inactive session already exists
        if !self.get_inactives().is_empty() {
            return Err(anyhow!("Cannot create new session: inactive session already exists"));
        }

        self.sessions
            .push(Session::new(SessionStatus::Idle, Vec::new()));
        self.sessions.last_mut().ok_or_else(|| anyhow!("Failed to create session"))
    }

    pub fn end_game(&mut self) -> bool {
        if let Some(pos) = self
            .sessions
            .iter()
            .position(|s| s.status == SessionStatus::Idle)
        {
            self.sessions.remove(pos);
            true
        } else {
            false
        }
    }

    pub async fn get_queue(&mut self) -> Result<&mut Session, Error> {
        // Get either Idle or Hot session (both are joinable)
        self.sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Idle || s.status == SessionStatus::Hot)
            .ok_or(anyhow!("No joinable session found"))
    }

    pub fn get_inactives(&self) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|s| !s.is_active())
            .collect()
    }

    pub fn get_actives(&self) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|s| s.is_active())
            .collect()
    }

    pub fn get_sessions_by_status(
        &self,
        status: &SessionStatus,
    ) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|s| s.status == *status)
            .collect()
    }

    /// Get session index (position in Vec) for logging purposes
    /// Returns None if session is not found in the group
    pub fn get_session_index(&self, session: &Session) -> Option<usize> {
        self.sessions.iter().position(|s| std::ptr::eq(s, session))
    }

    pub fn get_games_by_status_mut(
        &mut self,
        status: &SessionStatus,
    ) -> Vec<&mut Session> {
        self.sessions
            .iter_mut()
            .filter(|s| s.status == *status)
            .collect()
    }

    pub async fn get_user_session(
        &mut self,
        user_id: UI,
    ) -> Result<&mut Session> {
        match self.sessions.iter_mut().find(|s| s.pool.iter().any(|p| p.player.discord_id == user_id)) {
            Some(game) => Ok(game),
            None => Err(anyhow!("User not found in any game")),
        }
    }

    pub fn get_player(
        &mut self,
        user_id: UI,
    ) -> Result<&mut SessionPlayer> {
        match self.sessions.iter_mut().find(|s| s.pool.iter().any(|p| p.player.discord_id == user_id)) {
            Some(game) => Ok(game.pool.iter_mut().find(|p| p.player.discord_id == user_id).unwrap()),
            None => Err(anyhow!("User not found in any game")),
        }
    }

    pub async fn hot(&mut self, ctx: &Context, guild_id: Option<serenity::all::GuildId>, db: Option<&crate::Database>, manager: Option<std::sync::Arc<tokio::sync::Mutex<crate::Manager>>>) -> Result<(), Error> {
        // Get session index before calling hot()
        let session_idx = self.sessions
            .iter()
            .position(|s| s.status == SessionStatus::Idle || s.status == SessionStatus::Hot)
            .unwrap_or(0);

        self.get_queue().await?.hot();

        // Log the hot status with session ID
        if let Ok(session) = self.get_queue().await {
            session.log_hot(session_idx);
        }

        // Refresh player ranks from Discord roles before generating teams
        if let (Some(gid), Some(database)) = (guild_id, db) {
            self.refresh_player_ranks(ctx, gid, database).await;
        }

        // Notify requires guild_id for VC validation
        if let Some(gid) = guild_id {
            self.notify(ctx, gid).await;
        } else {
            warn!("Cannot notify: guild_id not provided");
        }
        
        // Generate teams - guild_id is required for dashboard updates
        if let Some(gid) = guild_id {
            self.generate_teams(ctx, gid, db).await;
        } else {
            warn!("Cannot generate teams: guild_id not provided");
        }
        
        // Spawn a targeted deadline timer for this hot session
        if let (Some(gid), Some(mgr)) = (guild_id, manager) {
            let group_id = self.group_id;
            let ctx_clone = ctx.clone();
            
            tokio::spawn(async move {
                use tokio::time::{sleep, Duration};
                
                // Wait for the deadline
                sleep(Duration::from_secs(DEFAULT_TIMEOUT)).await;
                
                // Check if players have joined, remove those who haven't
                let mut manager_lock = mgr.lock().await;
                if let Ok(server) = manager_lock.get_server(gid) {
                    if let Some(group) = server.groups.iter_mut().find(|g| g.group_id == group_id) {
                        if group.check_hot_timeout(&ctx_clone, gid).await {
                            info!("Deadline timer fired: removed timed-out players from group {}", group_id);
                            group.queue_dash_update(&ctx_clone, gid.get()).await;
                        }
                    }
                }
            });
        }
        
        Ok(())
    }

    /// Check hot sessions for timeout and handle accordingly
    /// Returns true if any changes were made that require dashboard update
    pub async fn check_hot_timeout(&mut self, ctx: &Context, guild_id: serenity::all::GuildId) -> bool {
        let mut changes_made = false;
        let quota = self.quota as usize;

        // Find hot sessions that have timed out
        let hot_sessions: Vec<usize> = self.sessions
            .iter()
            .enumerate()
            .filter_map(|(idx, s)| {
                if s.is_hot_timeout(DEFAULT_TIMEOUT) {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();

        for idx in hot_sessions {
            let session = &mut self.sessions[idx];

            // Get players who are not in VC (timed out)
            let timed_out_players: Vec<_> = session.pool
                .iter()
                .take(quota)
                .filter(|p| !p.in_queue_vc)
                .map(|p| p.player.discord_id)
                .collect();

            if timed_out_players.is_empty() {
                // All players joined, no timeout action needed
                continue;
            }

            info!("Removing {} timed-out players from hot session", timed_out_players.len());
            
            // Remove timed out players - retain() preserves order of remaining elements
            session.pool.retain(|p| !timed_out_players.contains(&p.player.discord_id));

            // Check if we still have enough players after removals
            if session.pool.len() >= quota {
                // We have replacements (overflow players took their spots)
                // Re-generate teams with the new first quota players
                info!("Regenerating teams after timeout with {} players", session.pool.len());
                changes_made = true;
            } else {
                // Not enough players left, revert to idle
                info!("Not enough players after timeout, reverting to idle");
                session.idle();
                changes_made = true;
            }
        }
        
        // If changes were made and we still have a hot session with enough players, regenerate teams
        if changes_made {
            if self.sessions.iter().any(|s| s.is_hot() && s.pool.len() >= quota) {
                // Re-generate teams for the hot session
                self.generate_teams(ctx, guild_id, None).await;
            }
        }

        changes_made
    }

    pub async fn push(&mut self, ctx: &Context, guild_id: serenity::all::GuildId) -> Result<(), Error> {
        // Extract channel IDs first to avoid borrowing conflicts
        let red_vc = self.channels.teams[0].red_vc;
        let blu_vc = self.channels.teams[0].blu_vc;

        // Get the hot game (should be the most recent session that's hot)
        let game = self.sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Hot)
            .ok_or(anyhow!("No hot session found for push"))?;

        // Set status to Push and extract player moves
        game.push();
        let player_moves: Vec<(UI, CI, String)> = game.pool
            .iter()
            .filter_map(|player| {
                // Only move players who are actually in the queue VC
                // This prevents disconnecting players who aren't in voice
                if !player.in_queue_vc {
                    return None;
                }

                match player.team {
                    Some(crate::models::Team::Red) => Some((
                        player.player.discord_id,
                        red_vc,
                        player.player.discord_tag.clone().unwrap_or_else(|| "Unknown".to_string())
                    )),
                    Some(crate::models::Team::Blu) => Some((
                        player.player.discord_id,
                        blu_vc,
                        player.player.discord_tag.clone().unwrap_or_else(|| "Unknown".to_string())
                    )),
                    _ => None,
                }
            })
            .collect();

        // Move users to team channels
        for (user_id, channel_id, tag) in player_moves {
            if let Err(e) = self.move_user(guild_id, user_id, channel_id, ctx).await {
                warn!("Failed to move user {}: {}", tag, e);
            }
        }

        // Set game status to Live and extract overflow players
        let session_idx = self.sessions
            .iter()
            .position(|s| s.status == SessionStatus::Push)
            .ok_or(anyhow!("Push session not found"))?;
        let game = &mut self.sessions[session_idx];
        game.live();

        let quota = self.quota as usize;
        let game_pool_len = game.pool.len();

        // Extract overflow players (those beyond quota)
        let overflow_players: Vec<_> = if game_pool_len > quota {
            game.pool.drain(quota..).collect()
        } else {
            Vec::new()
        };

        // Create new idle session for next game
        self.create_session()?;

        // Add overflow players to the new idle session
        // Overflow players are already in join-time order, so just push them
        if !overflow_players.is_empty() {
            let idle_session = self.get_queue().await?;
            for player in overflow_players {
                idle_session.pool.push(player);
            }
        }

        self.queue_dash_update(ctx, guild_id.get()).await;
        Ok(())

    }

    pub async fn pull(&mut self, ctx: &Context, guild_id: serenity::all::GuildId, db: &crate::Database, manager: Option<std::sync::Arc<tokio::sync::Mutex<crate::Manager>>>) -> Result<(), Error> {
        // Extract queue vc channel ID
        let queue_vc = self.channels.queue_vc;

        // Find the active game (Hot or Live status)
        let active_session_idx = self.sessions
            .iter()
            .position(|s| s.status == SessionStatus::Hot || s.status == SessionStatus::Live)
            .ok_or(anyhow!("No active game to pull"))?;
        let game = &mut self.sessions[active_session_idx];

        game.pull();

        // Extract all players to move back to queue
        let players_to_requeue: Vec<Player> = game.pool
            .iter()
            .map(|p| p.player.clone())
            .collect();

        // Move all players back to queue voice channel
        for player in &players_to_requeue {
            let tag = player.discord_tag.as_deref().unwrap_or("Unknown");
            if let Err(e) = self.move_user(guild_id, player.discord_id, queue_vc, ctx).await {
                warn!("Failed to move user {} back to queue: {}", tag, e);
            }
        }

        // Find or create the idle session (queue) and add all players back to it
        let idle_session_idx = match self.sessions.iter().position(|s| s.status == SessionStatus::Idle) {
            Some(idx) => idx,
            None => {
                // No idle session exists (game ended from Hot without push), create one
                info!("No idle session found, creating one for re-queuing players");
                self.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
                self.sessions.len() - 1
            }
        };
        let idle_session = &mut self.sessions[idle_session_idx];

        for player in players_to_requeue {
            if let Some(rank) = player.rank {
                // Use add_player_in_vc since we just moved them to the queue channel
                idle_session.add_player_in_vc(player, rank);
            }
        }

        // Remove the finished session
        self.sessions.retain(|s| s.status != SessionStatus::Pull);

        // Check if the queue now meets quota and transition to Hot if needed
        if self.is_quota() {
            self.hot(ctx, Some(guild_id), Some(db), manager).await?;
        }

        self.queue_dash_update(ctx, guild_id.get()).await;
        Ok(())
    }

    /// Update player ranks from Discord roles for all players in the session
    pub async fn refresh_player_ranks(&mut self, ctx: &Context, guild_id: serenity::all::GuildId, db: &crate::Database) {
        use crate::handlers::player::get_player_rank;

        for session in &mut self.sessions {
            for player in &mut session.pool {
                if let Some(updated_rank) = get_player_rank(ctx, db, guild_id, player.player.discord_id).await {
                    player.player.rank = Some(updated_rank);
                }
            }
        }
    }

    /// Validate and correct in_queue_vc flags against actual Discord voice states
    /// This prevents desync where cached flags don't match reality
    pub async fn validate_vc_status(&mut self, ctx: &Context, guild_id: serenity::all::GuildId) {
        // Get actual voice states from Discord
        let guild = match ctx.cache.guild(guild_id) {
            Some(g) => g,
            None => {
                warn!("[validate_vc_status] Guild {} not in cache", guild_id);
                return;
            }
        };

        let queue_vc_id = self.channels.queue_vc.get();
        
        // Get set of users actually in queue VC
        let users_in_vc: std::collections::HashSet<u64> = guild
            .voice_states
            .iter()
            .filter_map(|(user_id, vs)| {
                if vs.channel_id.map(|c| c.get()) == Some(queue_vc_id) {
                    Some(user_id.get())
                } else {
                    None
                }
            })
            .collect();

        // Update flags for all players in all sessions
        for session in &mut self.sessions {
            for player in &mut session.pool {
                let user_id = player.player.discord_id.get();
                let actual_in_vc = users_in_vc.contains(&user_id);
                
                // Log if we're correcting a desync
                if player.in_queue_vc != actual_in_vc {
                    let username = player.player.discord_tag.as_deref().unwrap_or("Unknown");
                    info!("[validate_vc_status] Correcting VC status for {}: was {}, now {}", 
                        username, player.in_queue_vc, actual_in_vc);
                    player.in_queue_vc = actual_in_vc;
                }
            }
        }
    }

    pub async fn generate_teams(&mut self, ctx: &Context, guild_id: serenity::all::GuildId, db: Option<&crate::Database>) {
        use itertools::Itertools;

        let quota = self.quota as usize;

        // Get the hot game (session was just set to hot before this is called)
        let session_idx = match self.sessions
            .iter()
            .position(|s| s.status == SessionStatus::Hot) {
            Some(idx) => idx,
            None => {
                warn!("No hot session found for team generation");
                return;
            }
        };

        let game = &mut self.sessions[session_idx];

        if game.pool.len() < quota {
            warn!("Not enough players for team generation: {}", game.pool.len());
            return;
        }

        // Extract player ELOs (use config-based ELO if available, otherwise default)
        let mut players_with_elo: Vec<(usize, u32)> = Vec::new();
        for (idx, gp) in game.pool.iter().enumerate() {
            let elo = if let (Some(rank), Some(database)) = (gp.player.rank, db) {
                rank.elo_from_config(database, guild_id.get()).await
            } else if let Some(rank) = gp.player.rank {
                rank.elo()
            } else {
                30 // Default to Novice (30)
            };
            players_with_elo.push((idx, elo));
        }

        // Balance exactly quota players (first N in queue)
        let pool_size = quota.min(game.pool.len());
        let players_to_balance: Vec<(usize, u32)> = players_with_elo.into_iter().take(pool_size).collect();

        // Generate all possible team splits using BCH (deterministic)
        let team_size = pool_size / 2;
        let mut best_split: Option<(Vec<usize>, Vec<usize>)> = None;
        let mut best_score = f64::INFINITY;

        for team_a_indices in (0..pool_size).combinations(team_size) {
            let team_b_indices: Vec<usize> = (0..pool_size)
                .filter(|i| !team_a_indices.contains(i))
                .collect();

            // Get ELOs for each team
            let team_a_elos: Vec<f64> = team_a_indices
                .iter()
                .map(|&i| players_to_balance[i].1 as f64)
                .collect();

            let team_b_elos: Vec<f64> = team_b_indices
                .iter()
                .map(|&i| players_to_balance[i].1 as f64)
                .collect();

            // Calculate statistics for both teams
            let (avg_a, med_a, std_a) = calculate_stats(&team_a_elos);
            let (avg_b, med_b, std_b) = calculate_stats(&team_b_elos);

            // BCH score: weighted sum prioritizing average ELO balance
            // Average is weighted 3x higher because it directly determines team strength
            let score = 3.0 * (avg_a - avg_b).abs() + (med_a - med_b).abs() + (std_a - std_b).abs();

            if score < best_score {
                best_score = score;
                best_split = Some((team_a_indices, team_b_indices));
            }
        }

        if let Some((mut red_indices, mut blu_indices)) = best_split {
            info!("[Session {}] Best team balance found with score: {:.2}", session_idx, best_score);

            // Randomize by swapping players with the same ELO
            // Group players by ELO, then shuffle within each ELO group
            use std::collections::HashMap;
            use rand::seq::SliceRandom;
            let mut rng = rand::rng();
            
            // Create map of ELO -> Vec<indices in players_to_balance>
            let mut elo_groups: HashMap<u32, Vec<usize>> = HashMap::new();
            for i in 0..pool_size {
                let elo = players_to_balance[i].1;
                elo_groups.entry(elo).or_insert_with(Vec::new).push(i);
            }
            
            // Store original team assignments before shuffling
            let original_red = red_indices.clone();
            let original_blu = blu_indices.clone();
            
            // For each ELO group with multiple players, shuffle them across teams
            for (elo, indices) in elo_groups.iter_mut() {
                if indices.len() > 1 {
                    // Count how many of this ELO are on each team (using ORIGINAL assignments)
                    let red_count = indices.iter().filter(|&&i| original_red.contains(&i)).count();
                    let blu_count = indices.len() - red_count;
                    
                    // Shuffle the indices with this ELO
                    indices.shuffle(&mut rng);
                    
                    // Reassign to teams with the same distribution
                    red_indices.retain(|&i| !indices.contains(&i));
                    blu_indices.retain(|&i| !indices.contains(&i));
                    
                    red_indices.extend_from_slice(&indices[..red_count]);
                    blu_indices.extend_from_slice(&indices[red_count..]);
                    
                    info!("[Session {}] Shuffled {} players with ELO {}", session_idx, indices.len(), elo);
                }
            }

            // Assign teams in-place to preserve in_queue_vc and in_queue_vc flags
            // First assign red team
            for &idx in &red_indices {
                let pool_idx = players_to_balance[idx].0;
                game.pool[pool_idx].team(crate::models::Team::Red);
            }

            // Then assign blue team
            for &idx in &blu_indices {
                let pool_idx = players_to_balance[idx].0;
                game.pool[pool_idx].team(crate::models::Team::Blu);
            }
            
            // DO NOT sort the pool by team assignment.
            // The pool MUST remain sorted by join_time to ensure overflow players
            // maintain their proper queue position (first-come-first-serve).
            // Team assignment is stored in each player's team field and used during push/display.
        } else {
            warn!("Failed to generate balanced teams");
        }

        // Update dashboard to show the new teams
        self.queue_dash_update(ctx, guild_id.get()).await;
    }

    pub async fn queue_player(&mut self, player: Player, rank: crate::models::Rank, ctx: &Context, guild_id: Option<serenity::all::GuildId>, db: Option<&crate::Database>, manager: Option<std::sync::Arc<tokio::sync::Mutex<crate::Manager>>>) -> Result<()> {
        self.queue_player_with_vc_status(player, rank, ctx, guild_id, db, manager, false).await
    }

    pub async fn queue_player_with_vc_status(&mut self, player: Player, rank: crate::models::Rank, ctx: &Context, guild_id: Option<serenity::all::GuildId>, db: Option<&crate::Database>, manager: Option<std::sync::Arc<tokio::sync::Mutex<crate::Manager>>>, in_vc: bool) -> Result<()> {
        let session = self.get_queue().await?;
        let user_id = player.discord_id;
        
        if in_vc {
            session.add_player_in_vc(player, rank);
        } else {
            session.add_player(player, rank);
        }

        if self.is_quota() {
            self.hot(ctx, guild_id, db, manager).await?;
        }
        Ok(())
    }

    pub async fn add_player(&mut self, session: &mut Session, player: Player, rank: crate::models::Rank, ctx: &Context, guild_id: serenity::all::GuildId) {
        session.add_player(player, rank);
        self.queue_dash_update(ctx, guild_id.get()).await;
    }

    /// Checks if this group contains the given channel_id in any of its channels
    pub fn contains_channel(
        &self,
        channel_id: CI,
    ) -> bool {
        self.channels.contains_channel(channel_id)
    }

    pub fn is_quota(&self) -> bool {
        let g = self.get_sessions_by_status(&SessionStatus::Idle);
        if g.is_empty() {
            warn!("No idle sessions found when checking quota");
            return false;
        }
        if g.len() > 1 {
            warn!("Multiple idle games found, faulty");
        }
        let l = g[0].pool.len();
        let q = self.quota as usize;
        match l.cmp(&q) {
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => true,
            std::cmp::Ordering::Greater => {
                warn!("Quota met late, more players than quota");
                true
            },
        }
    }

    /// Notifies the queue chat that quota has been met
    /// Only pings players who are NOT in the voice channel yet
    /// Only pings the first 'quota' players, not extras queued for next match
    pub async fn notify(&mut self, ctx: &Context, guild_id: serenity::all::GuildId) {
        // Validate VC status before sending notifications to prevent desync
        // This ensures we only ping players who are actually not in VC
        self.validate_vc_status(ctx, guild_id).await;
        
        let queue_chat = self.channels.queue_chat;
        let mut player_mentions = Vec::new();
        let quota = self.quota as usize;

        if let Some(game) = self.sessions.last() {
            // Only mention players who are NOT in the voice channel
            // AND only the first 'quota' players (not extras queued for next match)
            for player in game.pool.iter().take(quota) {
                if !player.in_queue_vc {
                    player_mentions.push(format!("<@{}>", player.player.discord_id));
                }
            }
        }

        // Only send notification if there are players to ping
        if player_mentions.is_empty() {
            info!("Quota met but all players already in VC - skipping notification");
            return;
        }

        // Use embed for header and raw pings in message content to properly ping users
        let embed = CreateEmbed::new()
            .title("PUG Starting")
            .description("Please join the queue channel!");

        let content = player_mentions.join(" ");
        let msg = CM::new().embed(embed).content(content);
        queue_chat.send_message(&ctx.http, msg).await;
    }

    pub async fn move_user(&self, guild_id: serenity::all::GuildId, user_id: UI, channel_id: CI, ctx: &Context) -> Result<(), Error> {
        let member = guild_id.member(&ctx.http, user_id).await?;
        member.move_to_voice_channel(&ctx.http, channel_id).await?;
        Ok(())
    }
}

// Roles
#[derive(Debug, Clone, Serialize, Deserialize)]
                pub struct Roles {
    pub runner: RI,
    pub admin:  RI,
}

impl Roles {
    pub fn new(
        runner: RI,
        admin: RI,
    ) -> Self {
        Self { runner, admin }
    }
    pub fn empty() -> Self {
        Self {
            runner: RI::new(1),
            admin: RI::new(1),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    Runner,
    Admin,
}

impl Role {
    /// Get config key for this role's Discord role ID
    pub fn config_key(&self) -> &'static str {
        match self {
            Role::Runner => "runner_role",
            Role::Admin  => "admin_role",
        }
    }

    /// Get the Discord role ID from database configuration (legacy single role)
    pub async fn id(&self, db: &crate::Database, guild_id: u64) -> Option<RI> {
        let ids = self.ids(db, guild_id).await;
        ids.first().copied()
    }

    /// Get all Discord role IDs from database configuration (supports multiple roles)
    pub async fn ids(&self, db: &crate::Database, guild_id: u64) -> Vec<RI> {
        if let Ok(Some(value)) = db.config.get_config_value(self.config_key(), guild_id).await {
            // Support comma-separated role IDs
            value.split(',')
                .filter_map(|s| s.trim().parse::<u64>().ok())
                .map(RI::new)
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Role::Runner => "Runner",
            Role::Admin  => "Admin",
        }
    }
}

// Divisons
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Divisons {
    Newcomer,
    Journey,
}

// Channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
    pub queue_chat: CI,
    pub queue_vc:   CI,
    pub teams:      Vec<TeamChannel>,
    pub dashboard:  CI,
}

impl Channels {
    pub fn new(
        queue_chat:     CI,
        queue_vc:  CI,
        teams:     Vec<TeamChannel>,
        dashboard: CI,
    ) -> Self {
        Self {
            queue_chat,
            queue_vc,
            teams,
            dashboard,
        }
    }

    /// Pushs a red and blue channel to the vector
    pub fn add_team_channel_pair(
        &mut self,
        red_vc: CI,
        blu_vc: CI,
    ) {
        self.teams.push(TeamChannel::new(red_vc, blu_vc));
    }

    pub fn empty() -> Self {
        Self {
            queue_chat:     CI::new(1),
            queue_vc:  CI::new(1),
            teams:     Vec::new(),
            dashboard: CI::new(1),
        }
    }

    /// Checks if this Channels struct contains the given channel_id
    /// in any of its channel fields (queue, queue_vc, dashboard, or team channels)
    pub fn contains_channel(
        &self,
        channel_id: CI,
    ) -> bool {
        self.queue_chat == channel_id
            || self.queue_vc  == channel_id
            || self.dashboard == channel_id
            || self.teams.iter().any(|team| team.contains_channel(channel_id))
    }
}

mod tests {
    use super::*;

    #[test]
    fn test_group_meets_quota() {
        let mut group = Group::new(
            1,
            4,
            120,
            MI::new(1),
            Channels::new(
                CI::new(1),
                CI::new(1),
                vec![TeamChannel::new(CI::new(1), CI::new(1))],
                CI::new(1),
            ),
            Vec::new(),
        );

        group.create_session();

        // Add players one by one - each call borrows and immediately drops
        use crate::models::{Rank, Player};
        group.sessions.last_mut().unwrap().add_player(Player::add(UI::new(1), None, None), Rank::Novice);
        group.sessions.last_mut().unwrap().add_player(Player::add(UI::new(2), None, None), Rank::Novice);
        group.sessions.last_mut().unwrap().add_player(Player::add(UI::new(3), None, None), Rank::Novice);

        assert!(!group.is_quota());

        group.sessions.last_mut().unwrap().add_player(Player::add(UI::new(4), None, None), Rank::Novice);

        assert!(group.is_quota());
    }
}