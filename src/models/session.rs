use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::str::FromStr;
use anyhow::Error;
use crate::Channels;
use crate::Player;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(FromRow)]
pub struct Session {
    pub id:            Vec<u8>,
    pub team_channels: Channels,
    pub role_group:    DivName,
    pub status:        SessionStatus,
    pub players:       Vec<SessionPlayer>,
}

impl Session {
    pub fn create() -> Self {
        Self {
            id:            0,
            queue_channel: 0,
            team_channels: Channels::default(),
            role_group:    DivName::Newcomer,
            status:        SessionStatus::Idle,
            players:       Vec::new(),
        }
    }

    pub fn count(&self) -> usize {
        self.players.len()
    }

    pub fn get_members(&self) -> Vec<Player> {
        self.players.iter().map(|m| m.player.clone()).collect()
    }

    pub fn add_member(&mut self, member: SessionPlayer) {
        self.players.push(member);
    }

    pub fn remove_member(&mut self, user_id: u64) {
        self.players.retain(|m| m.player.discord_id != user_id);
    }

    pub fn buffer_member(&mut self, user_id: u64, buffered_by: Player) {
        self.players.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().is_buffered = true;
        self.players.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().buffered_by = buffered_by;
    }

    pub fn unbuffer_member(&mut self, user_id: u64) {
        self.players.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().is_buffered = false;
        self.players.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().buffered_by = Player::new(0);
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player: Player,
    pub team:   Option<Team>,
    pub is_buffered: bool,
    pub buffered_by: Player,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Team {
    Red,
    Blu,
}

impl FromStr for Team {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "RED" => Ok(Team::Red),
            "BLU" => Ok(Team::Blu),
            _ => Err(Error::msg(format!("Unknown Team: {}", s))),
        }
    }
}

pub struct SessionDiv {
    pub name: DivName,
    pub queue_channel_id:   u64,
    pub team_channels:      Channels,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DivName {
    Newcomer,
    Journey,
}

/// Different session statuses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Idle, // Waiting for enough players to join
    Hot,  // Waiting for runners to start the session
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}