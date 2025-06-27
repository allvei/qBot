use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use crate::ChannelGroup;
use crate::Player;

// #[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(FromRow)]
pub struct Session {
    pub id:            Vec<u16>,
    pub channels:      ChannelGroup,
    pub status:        SessionStatus,
    pub players:       Vec<SessionPlayer>,
}

impl Session {
    pub fn new(
        channels: ChannelGroup, 
        status: SessionStatus,
    ) -> Self {
        Self {
            id: Vec::new(),
            players: Vec::new(),
            channels,
            status,
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

    pub fn buff(&mut self, user_id: u64, buffered_by: Player) {
        self.players.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().buff(buffered_by);
    }

    pub fn unbuff(&mut self, user_id: u64) {
        self.players.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().unbuff();
    }


}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player: Player,
    pub team:   Option<Team>,
    pub is_buffered: bool,
    pub buffered_by: Player,
}wd

impl SessionPlayer {
    pub fn buff(&mut self, buffered_by: Player) {
        self.is_buffered = true;
        self.buffered_by = buffered_by;
    }

    pub fn unbuff(&mut self) {
        self.is_buffered = false;
        self.buffered_by = Player::new(0);
    }

    pub fn team(&mut self, team: Team) {
        self.team = Some(team);
    }
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
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

// #[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DivName {
    Newcomer,
    Journey,
}

/// Different session statuses
// #[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Idle, // Waiting for enough players to join
    Hot,  // Waiting for runners to start the session
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}