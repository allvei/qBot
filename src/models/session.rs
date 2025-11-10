use std::str::FromStr;
use std::time::SystemTime;

use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{
    ChannelId as CI, CreateEmbed as CE, CreateEmbedFooter as CEF, UserId,
};
use sqlx::FromRow;
use tracing::info;

use crate::models::Player;

// Game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub status: SessionStatus,
    pub pool: Vec<SessionPlayer>,   
}

impl Session {
    /// Get a player by their Discord ID
    pub fn get_user(&self, discord_id: UserId) -> Result<Player> {
        match self.pool.iter().find(|p| p.player.discord_id == discord_id) {
            Some(player) => Ok(player.player),
            None => Err(anyhow::anyhow!("User not found")),
        }
    }

    /// Add a player to the session
    pub fn add_player(&mut self, discord_id: UserId) {
        let player = SessionPlayer::add(discord_id);
        self.pool.push(player);
        self.sort_by_join_time();
    }

    pub fn remove_player(&mut self, discord_id: UserId) {
        self.pool.retain(|p| p.player.discord_id != discord_id);
    }
    
    /// Sort players by join time (first-come-first-serve)
    pub fn sort_by_join_time(&mut self) {
        self.pool.sort_by_key(|p| p.joined_at);
    }

    /// Create a new session
    pub fn new(
        status: SessionStatus,
        pool: Vec<SessionPlayer>,
    ) -> Self {
        Self { status, pool }
    }

    /// Check if the session is active
    pub fn is_active(&self) -> bool {
        matches!(self.status, SessionStatus::Push | SessionStatus::Live | SessionStatus::Pull)
    }

    /// Check if the session is hot
    pub fn is_hot(&self) -> bool {
        matches!(self.status, SessionStatus::Hot)
    }

    /// Check if the session is idle
    pub fn is_idle(&self) -> bool {
        matches!(self.status, SessionStatus::Idle)
    }

    /// Set the session to idle
    pub fn idle(&mut self) {
        self.status = SessionStatus::Idle;
    }

    /// Set the session to hot
    pub fn hot(&mut self) -> CE {
        info!("Game is HOT with {} players", self.pool.len());
        self.status = SessionStatus::Hot;
        // Create an embed message for the game ready notification
        
        CE::new().title("GAME READY!")
                 .description(format!("A match is ready to start with {} players!", self.pool.len()))
                 .footer(CEF::new("Awaiting team generation..."))
    }

    /// Set the session to push
    pub fn push(&mut self) {
        self.status = SessionStatus::Push;
    }

    /// Set the session to live
    pub fn live(&mut self) {
        self.status = SessionStatus::Live;
    }

    /// Set the session to pull
    pub fn pull(&mut self) {
        self.status = SessionStatus::Pull;
    }
    
    /// Create an empty session
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
    Hot,  // Waiting for runners to start the game
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}

// SessionPlayer
#[derive(Debug, Clone, Copy, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player:       Player,
    pub team:         Option<Team>,
    pub is_buffered:  bool,
    pub in_queue_vc:  bool,
    pub in_queue_cmd: bool,
    #[serde(with = "systemtime_serde")]
    pub joined_at:    SystemTime,
}

// Serde serialization for SystemTime
mod systemtime_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH).map_err(serde::ser::Error::custom)?;
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + std::time::Duration::from_secs(secs))
    }
}

impl SessionPlayer {
    pub fn add(discord_id: UserId) -> Self {
        let player = Player::add(discord_id, None);
        Self {
            player,
            team:         None,
            is_buffered:  false,
            in_queue_vc:  false,
            in_queue_cmd: false,
            joined_at:    SystemTime::now(),
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

