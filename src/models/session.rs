// Standard Library
use std::str::FromStr;

// External modules
use anyhow::Error;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tracing::info;

// Local modules
use crate::Player;


// -----
// Enums
// -----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DivName {
    Newcomer,
    Journey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Idle, // Waiting for enough players to join
    Hot,  // Waiting for runners to start the session
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Team {
    Red,
    Blu,
}


// --------------------------
// Enum trait implementations
// --------------------------

impl FromStr for Team {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "RED" => Ok(Team::Red),
            "BLU" => Ok(Team::Blu),
            _ => Err(Error::msg(format!("Unknown : {}", s))),
        }
    }
}


// -------
// Structs
// -------

pub struct PugManager {
    pugmanager: Vec<Group>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub dashboard:     u64,
    pub chat:          u64,
    pub queue:         u64,
    pub teams:         Vec<TeamChannels>,
    pub session:       Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Session {
    pub id:            Vec<u16>,
    pub status:        SessionStatus,
    pub players:       Vec<SessionPlayer>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player: Player,
    pub team:   Option<Team>,
    pub buffered: Option<Player>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannels {
    pub red: u64,
    pub blu: u64,
}


// ---------------
// Implementations
// ---------------

impl PugManager {
    pub fn new(group: Group) -> Self {
        Self {
            pugmanager: vec![group],
        }
    }

    pub fn groups(&self) -> &Vec<Group> {
        &self.pugmanager
    }

    pub fn groups_mut(&mut self) -> &mut Vec<Group> {
        &mut self.pugmanager
    }

    pub fn find_group_by_queue_channel(&self, channel_id: u64) -> Option<&Group> {
        self.pugmanager.iter().find(|g| g.queue == channel_id)
    }

    pub fn find_group_by_queue_channel_mut(&mut self, channel_id: u64) -> Option<&mut Group> {
        self.pugmanager.iter_mut().find(|g| g.queue == channel_id)
    }
}

impl Group {
    pub fn new(dashboard: u64, chat: u64, queue: u64, red: u64, blu: u64) -> Self {
        info!("New group created for {}", dashboard);
        Self {
            dashboard,
            chat,
            queue,
            teams: vec![TeamChannels { red, blu }],
            session: Session::new() }
    }
    pub fn add_team_channels(&mut self, red: u64, blu: u64) {
        self.teams.push(TeamChannels { red, blu });
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            id: Vec::new(),
            players: Vec::<SessionPlayer>::new(),
            status: SessionStatus::Idle,
        }
    }

    pub fn count(&self) -> usize {
        self.players.len()
    }

    pub fn get_members(&self) -> Vec<Player> {
        self.players.iter().map(|m| m.player.clone()).collect()
    }

    pub fn add_player(&mut self, player: Player) {
        let session_player = SessionPlayer {
            player,
            team: None,
            buffered: None,
        };
        self.players.push(session_player);
    }

    pub fn remove_player(&mut self, user_id: u64) {
        self.players.retain(|p| p.player.discord_id != user_id);
    }

    pub fn buff(&mut self, user_id: u64, buffered: Option<Player>) {
        self.players.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().buff(buffered);
    }

    pub fn unbuff(&mut self, user_id: u64) {
        self.players.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().unbuff();
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionPlayer {
    pub fn new(player: Player) -> Self {
        Self {
            player,
            team: None,
            buffered: None,
        }
    }
    pub fn buff(&mut self, buffered: Option<Player>) {
        self.buffered = buffered;
    }

    pub fn unbuff(&mut self) {
        self.buffered = None;
    }

    pub fn team(&mut self, team: Team) {
        self.team = Some(team);
    }
}
