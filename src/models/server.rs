//! # Server Module
//!
//! This module defines the Server struct and its related functionality.
//! A Server represents a Discord guild with associated groups and games.

use anyhow::{anyhow, Error, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{
    parse_user_mention, ButtonStyle, ChannelId as CI, Context, CreateActionRow,
    CreateButton, CreateEmbed, CreateEmbedFooter as CEF,
    CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM,
    CreateMessage as CM, GuildId as GI, Message, MessageId as MI, RoleId as RI,
    UserId as UI,
};
use tracing::{info, warn};

use crate::handlers::player::check_role;
use crate::models::{
    CommandContext, ComponentContext, FileManager, SessionPlayer, Session, SessionStatus, TeamChannel,
    ADMIN_R_ID, BLU_VC_ID, CHAT_TC_ID, DASHBOARD_TC_ID, QUEUE_TC_ID, RED_VC_ID, RUNNER_R_ID,
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
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
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
    pub ip: String,
    pub name: String,
}

// Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub guild_id: GI,
    pub roles:    Roles,
    pub groups:   Vec<Group>,
}

impl Server {
    pub fn new(
        guild_id: GI,
        roles: Roles,
    ) -> Self {
        Self {
            guild_id,
            roles,
            groups: Vec::new(),
        }
    }

    pub fn add_group(
        &mut self,
        group: Group,
    ) -> Result<()> {
        self.groups.push(group);
        self.groups.last_mut().unwrap().create_session();
        Ok(())
    }

    pub fn empty(guild_id: GI) -> Self {
        Self {
            guild_id,
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
        }
    }

    pub fn create_session(&mut self) -> &mut Session {
        info!("Creating new game");
        self.sessions
            .push(Session::new(SessionStatus::Idle, Vec::new()));
        self.sessions.last_mut().unwrap()
    }

    pub fn end_game(&mut self) -> bool {
        info!("Attempting to end game");
        if let Some(pos) = self
            .sessions
            .iter()
            .position(|s| s.status == SessionStatus::Idle)
        {
            self.sessions.remove(pos);
            info!("Game successfully ended and removed");
            true
        } else {
            info!("Failed to end game: Game not found");
            false
        }
    }

    pub async fn get_queue(&mut self) -> Result<&mut Session, Error> {
        self.sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Idle)
            .ok_or(anyhow!("No idle session found"))
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

    pub async fn hot(&mut self, ctx: &Context) -> Result<(), Error> {
        self.get_queue().await?.hot();
        self.notify(ctx).await;
        self.generate_teams(ctx).await;
        Ok(())
    }

    pub async fn push(&mut self, ctx: &Context) -> Result<(), Error> {
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
        let player_moves: Vec<(UI, CI)> = game.pool
            .iter()
            .filter_map(|player| {
                match player.team {
                    Some(crate::models::Team::Red) => Some((player.player.discord_id, red_vc)),
                    Some(crate::models::Team::Blu) => Some((player.player.discord_id, blu_vc)),
                    _ => None,
                }
            })
            .collect();
        
        // Drop the mutable borrow and move users to team channels
        for (user_id, channel_id) in player_moves {
            if let Err(e) = self.move_user(user_id, channel_id, ctx).await {
                warn!("Failed to move user {}: {}", user_id, e);
            }
        }
        
        // Set game status to Live
        let game = self.sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Push)
            .ok_or(anyhow!("Push session not found"))?;
        game.live();
        info!("Match is now LIVE with {} players", game.pool.len());
        
        // Create new idle session for next game now that current session is live
        info!("Creating new idle session for next game");
        self.create_session();
        
        self.dash_update(ctx).await;
        Ok(())

    }

    pub async fn pull(&mut self, ctx: &Context) -> Result<(), Error> {
        // Extract queue vc channel ID
        let queue_vc = self.channels.queue_vc;
        
        // Find the active game (Hot or Live status)
        let game = self.sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Hot || s.status == SessionStatus::Live)
            .ok_or(anyhow!("No active game to pull"))?;
        
        game.pull();
        
        // Extract all players to move back to queue
        let player_ids: Vec<UI> = game.pool
            .iter()
            .map(|player| player.player.discord_id)
            .collect();
        
        // Move all players back to queue voice channel
        for user_id in player_ids {
            if let Err(e) = self.move_user(user_id, queue_vc, ctx).await {
                warn!("Failed to move user {} back to queue: {}", user_id, e);
            }
        }
        
        // Clear the pool and reset to Idle
        let game = self.sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Pull)
            .ok_or(anyhow!("Game state changed unexpectedly"))?;
        
        game.pool.clear();
        game.idle();
        info!("Match ended, players returned to queue");
        
        self.dash_update(ctx).await;
        Ok(())
    }

    pub async fn generate_teams(&mut self, ctx: &Context) {
        use itertools::Itertools;
        
        info!("Generating balanced teams using BCH algorithm");
        
        let quota = self.quota as usize;
        
        // Get the hot game (session was just set to hot before this is called)
        let game = self.sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Hot)
            .expect("No hot session found for team generation");
        
        if game.pool.len() < quota {
            warn!("Not enough players for team generation: {}", game.pool.len());
            return;
        }
        
        // Extract player ELOs (use default ELO if rank is None)
        let players_with_elo: Vec<(usize, u32)> = game.pool
            .iter()
            .enumerate()
            .map(|(idx, gp)| {
                let elo = gp.player.rank.map(|r| r.elo()).unwrap_or(30); // Default to Novice (30)
                (idx, elo)
            })
            .collect();
        
        // Balance exactly quota players (first N in queue)
        let pool_size = quota.min(game.pool.len());
        let players_to_balance: Vec<(usize, u32)> = players_with_elo.into_iter().take(pool_size).collect();
        
        // Generate all possible team splits (C(8,4) = 70 combinations)
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
            
            // BCH score: sum of absolute differences
            let score = (avg_a - avg_b).abs() + (med_a - med_b).abs() + (std_a - std_b).abs();
            
            if score < best_score {
                best_score = score;
                best_split = Some((team_a_indices, team_b_indices));
            }
        }
        
        if let Some((red_indices, blu_indices)) = best_split {
            info!("Best team balance found with score: {:.2}", best_score);
            
            // Assign teams based on the best split
            let mut new_pool = Vec::new();
            
            // First add red team players
            for &idx in &red_indices {
                let mut player = game.pool[players_to_balance[idx].0];
                player.team(crate::models::Team::Red);
                new_pool.push(player);
            }
            
            // Then add blue team players
            for &idx in &blu_indices {
                let mut player = game.pool[players_to_balance[idx].0];
                player.team(crate::models::Team::Blu);
                new_pool.push(player);
            }
            
            // Add remaining players (if more than 8)
            for i in pool_size..game.pool.len() {
                new_pool.push(game.pool[i]);
            }
            
            // Update the pool with balanced teams
            game.pool = new_pool;
            
            info!("Teams generated and assigned successfully");
        } else {
            warn!("Failed to generate balanced teams");
        }
        
        // Update dashboard to show the new teams
        self.dash_update(ctx).await.ok();
    }

    pub async fn queue_player(&mut self, user_id: UI, rank: crate::models::Rank, ctx: &Context) {
        self.get_queue().await.unwrap().add_player(user_id, rank);
        if self.is_quota() {
            self.hot(ctx).await;
        }
    }

    pub async fn add_player(&mut self, session: &mut Session, user_id: UI, rank: crate::models::Rank, ctx: &Context) {
        session.add_player(user_id, rank);
        self.dash_update(ctx).await;
    }

    /// Checks if this group contains the given channel_id in any of its channels
    pub fn contains_channel(
        &self,
        channel_id: CI,
    ) -> bool {
        self.channels.contains_channel(channel_id)
    }

    /// `/buffer`
    ///
    /// * `user_mention` - The user mention to buffer.
    pub async fn cmd_buffer(cc: &CommandContext<'_>,user_mention: &str,) -> Result<()> {
        info!("Processing buffer command for user mention: {}", user_mention);
        let user_id = parse_user_mention(user_mention).unwrap();
        if !check_role(cc, &Role::Admin).await? {
            let response = CIR::Message(CIRM::new().content("Only admins can buffer players!").ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
        let mut manager    = cc.manager.lock().await;
        let server         = manager.get_server(cc.intax.guild_id.unwrap()).unwrap();
        let group          = server .get_group(cc.intax.channel_id).unwrap();
        let game_player    = group  .get_player(user_id).unwrap();
        game_player.buff();
        Ok(())
    }

    pub fn is_quota(&self) -> bool {
        let g = self.get_sessions_by_status(&SessionStatus::Idle);
        if g.len() > 1 {
            warn!("Multiple idle games found, faulty");
        }
        let l = g[0].pool.len();
        let q = self.quota as usize;
        match l.cmp(&q) {
            std::cmp::Ordering::Less => {
                false
            },
            std::cmp::Ordering::Equal => {
                true
            },
            std::cmp::Ordering::Greater => {
                warn!("Quota met late, more players than quota");
                true
            },
        }
    }

    /// Notifies the queue chat that quota has been met
    /// Only pings players who are NOT in the voice channel yet
    pub async fn notify(&self, ctx: &Context) {
        let queue_chat = self.channels.queue_chat;
        let mut player_mentions = Vec::new();
        
        if let Some(game) = self.sessions.last() {
            // Only mention players who are NOT in the voice channel
            for player in &game.pool {
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

    pub async fn move_user(&self, user_id: UI, channel_id: CI, ctx: &Context) -> Result<(), Error> {
        let guilds   = ctx.cache.guilds();
        let guild_id = guilds.first().ok_or(anyhow!("No guild found"))?;
        let member   = guild_id.member(&ctx.http, user_id).await?;
        member.move_to_voice_channel(&ctx.http, channel_id).await?;
        Ok(())
    }
}

// Roles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roles {
    pub runner: RI,
    pub admin: RI,
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
    pub fn id(&self) -> RI {
        match self {
            Role::Runner => RUNNER_R_ID.into(),
            Role::Admin  => ADMIN_R_ID .into(),
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
    pub queue_chat:     CI,
    pub queue_vc:  CI,
    pub teams:     Vec<TeamChannel>,
    pub dashboard: CI,
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
        use crate::models::Rank;
        group.sessions.last_mut().unwrap().add_player(UI::new(1), Rank::Novice);
        group.sessions.last_mut().unwrap().add_player(UI::new(2), Rank::Novice);
        group.sessions.last_mut().unwrap().add_player(UI::new(3), Rank::Novice);
        
        assert!(!group.is_quota());
        
        group.sessions.last_mut().unwrap().add_player(UI::new(4), Rank::Novice);
        
        assert!(group.is_quota());
    }
}