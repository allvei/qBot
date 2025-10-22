use std::str::FromStr;

use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId as CI, UserId};
use sqlx::FromRow;

use crate::models::Player;



// Session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub status: SessionStatus,
    pub pool: Vec<SessionPlayer>,   
}

impl Session {
    pub fn get_user(&self, discord_id: UserId) -> Result<Player> {
        match self.pool.iter().find(|p| p.player.discord_id == discord_id) {
            Some(player) => Ok(player.player),
            None => Err(anyhow::anyhow!("User not found")),
        }
    }

    pub fn add_player(&mut self, discord_id: UserId, steam_id: Option<u64>) {
        let player = SessionPlayer::add(discord_id, steam_id);
        self.pool.push(player);
    }

    pub fn new(
        status: SessionStatus,
        pool: Vec<SessionPlayer>,
    ) -> Self {
        Self { status, pool }
    }

    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    pub fn empty() -> Self {
        Self {
            status: SessionStatus::Idle,
            pool: Vec::new(),
        }
    }
}

// SessionStatus
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Idle, // Waiting for enough players to join
    Hot,  // Waiting for runners to start the session
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}

impl SessionStatus {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            SessionStatus::Push | SessionStatus::Live | SessionStatus::Pull
        )
    }
}

// SessionPlayer
#[derive(Debug, Clone, Copy, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player:       Player,
    pub team:         Option<Team>,
    pub is_buffered:  bool,
    pub in_queue_vc:  bool,
    pub in_queue_cmd: bool,
}

impl SessionPlayer {
    pub fn add(discord_id: UserId, steam_id: Option<u64>) -> Self {
        let player = Player::add(discord_id, steam_id);
        Self {
            player:       player,
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

