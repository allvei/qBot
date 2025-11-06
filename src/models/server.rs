//! # Server Module
//!
//! This module defines the Server struct and its related functionality.
//! A Server represents a Discord guild with associated groups and games.

use serde::{Deserialize, Serialize};
use serenity::all::{parse_user_mention, ButtonStyle, Context, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter as CEF, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateMessage as CM, Message};
use serenity::all::{GuildId as GI, RoleId as RI, ChannelId as CI, MessageId as MI, UserId as UI};
use tracing::{info, warn};
use anyhow::{anyhow, Error, Result};

use crate::handlers::player::check_role;
use crate::models::data::*;
use crate::models::game::*;
use crate::models::{CommandContext, ComponentContext};


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
    pub games:         Vec<Games>,
}

impl Group {
    pub fn new(
        group_id:      u8,
        quota:         u8,
        timeout:       u16,
        dashboard_msg: MI,
        channels:      Channels,
        games:         Vec<Games>,
    ) -> Self {
        Self {
            group_id,
            quota,
            timeout,
            dashboard_msg,
            channels,
            games,
        }
    }

    pub fn create_game(&mut self) -> &mut Games {
        info!("Creating new game");
        self.games
            .push(Games::new(GameStatus::Idle, Vec::new()));
        self.games.last_mut().unwrap()
    }

    pub fn end_game(&mut self) -> bool {
        info!("Attempting to end game");
        if let Some(pos) = self
            .games
            .iter()
            .position(|s| s.status == GameStatus::Idle)
        {
            self.games.remove(pos);
            info!("Game successfully ended and removed");
            true
        } else {
            info!("Failed to end game: Game not found");
            false
        }
    }

    pub fn get_games_by_status(
        &mut self,
        status: &GameStatus,
    ) -> Vec<&Games> {
        self.games
            .iter()
            .filter(|s| s.status == *status)
            .collect()
    }

    pub fn get_user_game(
        &mut self,
        user_id: UI,
    ) -> Result<&mut Games> {
        match self.games.iter_mut().find(|s| s.pool.iter().any(|p| p.player.discord_id == user_id)) {
            Some(game) => Ok(game),
            None => Err(anyhow!("User not found in any game")),
        }
    }

    pub fn get_player(
        &mut self,
        user_id: UI,
    ) -> Result<&mut GamePlayer> {
        match self.games.iter_mut().find(|s| s.pool.iter().any(|p| p.player.discord_id == user_id)) {
            Some(game) => Ok(game.pool.iter_mut().find(|p| p.player.discord_id == user_id).unwrap()),
            None => Err(anyhow!("User not found in any game")),
        }
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

    pub async fn is_quota_met(&mut self) -> bool {
        let g = self.get_games_by_status(&GameStatus::Idle);
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
    pub queue:     CI,
    pub queue_vc:  CI,
    pub teams:     Vec<TeamChannel>,
    pub dashboard: CI,
}

impl Channels {
    pub fn new(
        queue:     CI,
        queue_vc:  CI,
        teams:     Vec<TeamChannel>,
        dashboard: CI,
    ) -> Self {
        Self {
            queue,
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
            queue:     CI::new(1),
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
        self.queue == channel_id
            || self.queue_vc  == channel_id
            || self.dashboard == channel_id
            || self.teams.iter().any(|team| team.contains_channel(channel_id))
    }
}

