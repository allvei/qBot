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
    CommandContext, ComponentContext, FileManager, GamePlayer, Session, SessionStatus, TeamChannel,
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
    ) {
        self.groups.push(group);
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

    pub fn create_game(&mut self) -> &mut Session {
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

    pub fn get_queue(&mut self) -> &mut Session {
        self.sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Idle)
            .unwrap()
    }

    pub fn get_games_by_status(
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

    pub fn get_user_game(
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
    ) -> Result<&mut GamePlayer> {
        match self.sessions.iter_mut().find(|s| s.pool.iter().any(|p| p.player.discord_id == user_id)) {
            Some(game) => Ok(game.pool.iter_mut().find(|p| p.player.discord_id == user_id).unwrap()),
            None => Err(anyhow!("User not found in any game")),
        }
    }

    pub async fn hot(&mut self, ctx: &Context) {
        self.get_queue().hot();
        self.notify(ctx).await;
        self.generate_teams(ctx).await;
    }

    pub async fn generate_teams(&mut self, ctx: &Context) {
        use itertools::Itertools;
        
        info!("Generating balanced teams using BCH algorithm");
        
        // Get the current game (should be hot or idle with enough players)
        let game = self.get_queue();
        
        // Need at least 8 players for team generation
        if game.pool.len() < 8 {
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
        
        // We'll balance exactly 8 players (first 8 in queue)
        let pool_size = 8.min(game.pool.len());
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

    pub async fn queue_player(&mut self, user_id: UI, ctx: &Context) {
        self.get_queue().add_player(user_id);
        if self.is_quota() {
            self.hot(ctx).await;
        }
        self.dash_update(ctx).await;
    }

    pub async fn add_player(&mut self, session: &mut Session, user_id: UI, ctx: &Context) {
        session.add_player(user_id);
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
        let g = self.get_games_by_status(&SessionStatus::Idle);
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
    pub async fn notify(&self, ctx: &Context) {
        let queue_chat = self.channels.queue_chat;
        let mut player_mentions = Vec::new();
        if let Some(game) = self.sessions.last() {
            for player in &game.pool {
                player_mentions.push(format!("<@{}>", player.player.discord_id));
            }
        }
        
        let embed = CreateEmbed::new()
            .title("Quota Met")
            .description(format!(
                "PUG is ready, please join the queue channel!\n\n{}",
                player_mentions.join("\n")
            ));
        let msg = CM::new().embed(embed);
        queue_chat.send_message(&ctx.http, msg).await;
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

