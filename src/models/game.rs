use std::str::FromStr;

use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId as CI, CreateEmbed as CE, CreateEmbedFooter as CEF, UserId};
use sqlx::FromRow;
use tracing::info;

use crate::models::Player;

// Game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Games {
    pub status: GameStatus,
    pub pool: Vec<GamePlayer>,   
}

impl Games {
    pub fn get_user(&self, discord_id: UserId) -> Result<Player> {
        match self.pool.iter().find(|p| p.player.discord_id == discord_id) {
            Some(player) => Ok(player.player),
            None => Err(anyhow::anyhow!("User not found")),
        }
    }

    pub fn add_player(&mut self, discord_id: UserId) {
        let player = GamePlayer::add(discord_id);
        self.pool.push(player);
    }

    pub fn new(
        status: GameStatus,
        pool: Vec<GamePlayer>,
    ) -> Self {
        Self { status, pool }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, GameStatus::Push | GameStatus::Live | GameStatus::Pull)
    }

    pub fn is_hot(&self) -> bool {
        matches!(self.status, GameStatus::Hot)
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.status, GameStatus::Idle)
    }

    pub fn idle(&mut self) {
        self.status = GameStatus::Idle;
    }

    pub fn hot(&mut self) -> CE {
        info!("Game is HOT with {} players", self.player_count());
        self.status = GameStatus::Hot;
        // Create an embed message for the game ready notification
        let embed = CE::new()
            .title("GAME READY!")
            .description(format!("A match is ready to start with {} players!", self.player_count()))
            .footer(CEF::new("Awaiting team generation..."));
        embed
    }

    pub fn push(&mut self) {
        self.status = GameStatus::Push;
    }

    pub fn live(&mut self) {
        self.status = GameStatus::Live;
    }

    pub fn pull(&mut self) {
        self.status = GameStatus::Pull;
    }

    pub fn player_count(&self) -> usize {
        self.pool.len()
    }
    
    pub fn empty() -> Self {
        Self {
            status: GameStatus::Idle,
            pool: Vec::new(),
        }
    }
}

// GameStatus
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum GameStatus {
    Idle, // Waiting for enough players to join
    Hot,  // Waiting for runners to start the game
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}

// GamePlayer
#[derive(Debug, Clone, Copy, FromRow, Serialize, Deserialize)]
pub struct GamePlayer {
    pub player:       Player,
    pub team:         Option<Team>,
    pub is_buffered:  bool,
    pub in_queue_vc:  bool,
    pub in_queue_cmd: bool,
}

impl GamePlayer {
    pub fn add(discord_id: UserId) -> Self {
        let player = Player::add(discord_id, None);
        Self {
            player,
            team:         None,
            is_buffered:  false,
            in_queue_vc:  false,
            in_queue_cmd: false,
        }
    }

    pub fn buff(&mut self) {
        self.is_buffered = true;
    }

    pub fn unbuff(&mut self) {
        self.is_buffered = false;
    }

    pub fn team(
        &mut self,
        team: Team,
    ) {
        self.team = Some(team);
    }

    pub fn in_queue(&self) -> bool {
        self.in_queue_vc || self.in_queue_cmd
    }
}

// Teams
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannel {
    pub red_vc: CI,
    pub blu_vc: CI,
}

impl TeamChannel {
    pub fn new(
        red_vc: CI,
        blu_vc: CI,
    ) -> Self {
        Self { red_vc, blu_vc }
    }

    pub fn empty() -> Self {
        Self {
            red_vc: CI::new(1),
            blu_vc: CI::new(1),
        }
    }

    /// Checks if this TeamChannel contains the given channel_id
    /// in either red_vc or blu_vc
    pub fn contains_channel(
        &self,
        channel_id: CI,
    ) -> bool {
        self.red_vc == channel_id || self.blu_vc == channel_id
    }
}

// Team
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Team {
    Unassigned,
    Red,
    Blu,
}

impl FromStr for Team {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "UNASSIGNED" => Ok(Team::Unassigned),
            "RED"        => Ok(Team::Red),
            "BLU"        => Ok(Team::Blu),
            _            => Err(Error::msg(format!("Unknown : {}", s))),
        }
    }
}

