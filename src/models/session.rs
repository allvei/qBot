// CHECK ME

use std::str::FromStr;

use anyhow::Error;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tracing::info;

use crate::Player;

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

pub struct PugManager {
    pugmanager: Vec<Group>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub dashboard:         u64,
    pub chat:              u64,
    pub queue:             u64,
    pub teams:             Vec<TeamChannels>,
    pub session:           Vec<Session>,
    pub session_increment: u16,
}

impl Group {
    pub fn new(dashboard: u64, chat: u64, queue: u64, red: u64, blu: u64) -> Self {
        info!("New group created for {}", dashboard);
        Self {
            dashboard,
            chat,
            queue,
            teams:             vec![TeamChannels { red, blu }],
            session:           Vec::new(),
            session_increment: 0,
        }
    }

    pub fn add_team_channels(&mut self, red: u64, blu: u64) {
        self.teams.push(TeamChannels { red, blu });
    }

    pub fn create_session(&mut self) {
        self.session_increment += 1;
        self.session.push(Session::new(session_increment));
    }

    pub fn end_session(&mut self, session_id: &[u16]) -> bool {
        if let Some(pos) = self.session.iter().position(|s| s.id == session_id) {
            self.session.remove(pos);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Session {
    pub id:                u16,
    pub status:            SessionStatus,
    pub pool:           Vec<SessionPlayer>,
}

impl Session {
    pub fn new(id: u16) -> Self {
        let session = Self {
            id,
            pool: Vec::new(),
            status: SessionStatus::Idle,
        };
        info!("New session started with ID: {}", self.id);
        session
    }

    pub fn hot(&mut self) {
        // send notif
        self.status = SessionStatus::Hot;
    }

    pub fn push(&mut self) {
        self.status = SessionStatus::Push;
    }

    pub fn live(&mut self) {
        self.status = SessionStatus::Live;
    }

    pub fn pull(&mut self) {
        self.status = SessionStatus::Pull;
    }

    pub fn generate_teams(&mut self) {
        // Create genpool
        let genpool = Vec::<SessionPlayer>::new();
        // 1. Get the first n=quota_size players who are buffered using first-in first-out method from the pool vector
        // 2. Add the buffered players to the genpool vector
        // 3. Now pull the remaining players from the pool vector and fill the genpool vector up until n=quota_size
        // 4. Now we have a genpool with n=quota_size players
        // 5. Generate teams using an algorithm that balances the teams based on the player's elo.
        //   - Sort players by ELO rating in descending order
        //   - Add randomness for players with identical ELO values
        //   - Distribute players using a snake draft pattern (ABBAABBA) to ensure balance
        //   - Team A gets highest rated player, Team B gets 2nd and 3rd, Team A gets 4th and 5th, etc.
        //   - This creates teams with similar average skill levels
        //   - Finally, assign players to appropriate positions based on their preferences if available
        // 

    }

    pub fn count(&self) -> usize {
        self.pool.len()
    }

    pub fn get_members(&self) -> Vec<Player> {
        self.pool.iter().map(|m| m.player.clone()).collect()
    }

    pub fn add_player(&mut self, player: Player) {
        let session_player = SessionPlayer {
            player,
            team: None,
            buffered: None,
        };
        info!("Added player {} to session {}", player.discord_id, self.id);
        self.pool.push(session_player);
    }

    pub fn remove_player(&mut self, player: &SessionPlayer) {
        self.pool.retain(|p| p.player.discord_id != player.player.discord_id);
    }

    pub fn buff(&mut self, user_id: u64, buffered: Option<Player>) {
        self.pool.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().buff(buffered);
    }

    pub fn unbuff(&mut self, user_id: u64) {
        self.pool.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().unbuff();
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player: Player,
    pub team:   Option<Team>,
    pub buffered: Option<Player>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannels {
    pub red: u64,
    pub blu: u64,
}
