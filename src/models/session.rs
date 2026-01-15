use std::str::FromStr;
use std::time::SystemTime;

use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use serenity::all::{
    ChannelId as CI, CreateEmbed as CE, CreateEmbedFooter as CEF, UserId as UI,
};
use sqlx::FromRow;

use crate::{DEFAULT_TIMEOUT, models::Player};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub status:     SessionStatus,
    pub pool:       Vec<SessionPlayer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_at:   Option<SystemTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<SystemTime>,
}

impl Session {
    /// Get a player by their Discord ID
    pub fn get_player(&self, user_id: UI) -> Result<Player> {
        match self.pool.iter().find(|p| p.player.user_id == user_id) {
            Some(player) => Ok(player.player.clone()),
            None => Err(anyhow::anyhow!("User not found")),
        }
    }

    /// Add a player to the session with their rank
    pub fn add_player(&mut self, player: Player) {
        let session_player = SessionPlayer::add(player);
        self.pool.push(session_player);
    }

    /// Add a player to the session with their rank, marking them as already in queue VC
    /// Use this when re-adding players who were just moved to the queue channel
    pub fn add_player_in_vc(&mut self, player: Player) {
        let mut session_player = SessionPlayer::add(player);
        session_player.in_queue_vc = true;
        self.pool.push(session_player);
    }

    pub fn remove_player(&mut self, user_id: UI) {
        self.pool.retain(|p| p.player.user_id != user_id);
    }

    /// Create a new session
    pub fn new(status: SessionStatus, pool: Vec<SessionPlayer>) -> Self {
        Self {
            status,
            pool,
            ready_at: None,
            started_at: None,
        }
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

    /// Set the session to idle and clear team assignments
    pub fn idle(&mut self) {
        self.status     = SessionStatus::Idle;
        self.ready_at   = None;
        self.started_at = None;
        // Clear team assignments and VC join tracking when going back to idle
        for player in &mut self.pool {
            player.team = None;
        }
    }

    /// Set the session to hot and record the ready timestamp
    pub fn hot(&mut self) -> CE {
        self.status   = SessionStatus::Hot;
        self.ready_at = Some(SystemTime::now());
        // Create an embed message for the game ready notification

        CE::new().title("GAME READY!")
                 .description(format!("A match is ready to start with {} players!", self.pool.len()))
                 .footer(CEF::new("Awaiting team generation..."))
    }

    /// Set the session to push
    pub fn push(&mut self) {self.status = SessionStatus::Push;}

    /// Set the session to live and record start time
    pub fn live(&mut self) {
        self.status = SessionStatus::Live;
        self.started_at = Some(SystemTime::now());
    }

    /// Set the session to pull
    pub fn pull(&mut self) {self.status = SessionStatus::Pull;}

    /// Create an empty session
    pub fn empty() -> Self {
        Self {
            status:     SessionStatus::Idle,
            pool:       Vec::new(),
            ready_at:   None,
            started_at: None,
        }
    }

    /// Check if this Hot session has timed out (players didn't join VC in time)
    pub fn is_hot_timeout(&self, timeout_seconds: u64) -> bool {
        if !self.is_hot() {
            return false;
        }

        if let Some(ready_at) = self.ready_at {
            if let Ok(elapsed) = SystemTime::now().duration_since(ready_at) {
                return elapsed.as_secs() >= timeout_seconds;
            }
        }
        false
    }

    /// Get seconds remaining until timeout (returns 0 if timed out or not hot)
    pub fn seconds_until_timeout(&self, timeout_seconds: u64) -> u64 {
        if !self.is_hot() {
            return 0;
        }

        if let Some(ready_at) = self.ready_at {
            if let Ok(elapsed) = SystemTime::now().duration_since(ready_at) {
                let elapsed_secs = elapsed.as_secs();
                if elapsed_secs < timeout_seconds {
                    return timeout_seconds - elapsed_secs;
                }
            }
        }
        0
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Idle, // Waiting for enough players to join
    Hot,  // Waiting for runners to start the game
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player:       Player,
    pub team:         Option<Team>,
    pub in_queue_vc:  bool,
    pub in_queue_cmd: bool,
    pub joined_at:    SystemTime,
    pub timeout:      u8,
}

impl SessionPlayer {
    pub fn add(player: Player) -> Self {
        Self {
            player,
            team:         None,
            in_queue_vc:  false,
            in_queue_cmd: false,
            joined_at:    SystemTime::now(),
            timeout:      DEFAULT_TIMEOUT,
        }
    }

    pub fn team(&mut self, team: Team) {
        self.team = Some(team);
    }

    pub fn in_queue(&self) -> bool {
        self.in_queue_vc || self.in_queue_cmd
    }
}

#[allow(dead_code)]
trait Quota {
    fn less(&self,  quota: usize) -> bool;
    fn equal(&self, quota: usize) -> bool;
    fn more(&self,  quota: usize) -> bool;
}

impl Quota for Vec<SessionPlayer> {
    fn less(&self,  quota: usize) -> bool {self.len() <  quota}
    fn equal(&self, quota: usize) -> bool {self.len() == quota}
    fn more(&self,  quota: usize) -> bool {self.len() >  quota}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannel {
    pub red_vc: CI,
    pub blu_vc: CI,
}

impl TeamChannel {
    pub fn new(red_vc: CI, blu_vc: CI) -> Self {
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
    pub fn contains_channel(&self, channel_id: CI) -> bool {
        self.red_vc == channel_id || self.blu_vc == channel_id
    }
}

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
            _            => Err(Error::msg(format!("Unknown : {s}"))),
        }
    }
}

