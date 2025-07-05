// CHECK ME

use std::str::FromStr;

use anyhow::Error;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::all::{Cache, GuildId};
use sqlx::FromRow;
use tracing::{debug, info};

use crate::models::{Player, Rank};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DivName {
    Newcomer,
    Journey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Idle, // Waiting for enough players to join
    Hot,  // Waiting for runners to start the session
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Default)]
pub struct Manager {
    pub servers: Vec<Server>,
}

impl Manager {
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
        }
    }

    pub fn pull_list(&mut self, cache: &Cache) -> Self {
        let mut servers = Vec::new();
        cache.guilds().iter().for_each(|g| {
            servers.push(Server::new(*g, None));
        });
        Self { servers }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub guild:  GuildId,
    pub groups: Vec<Group>,
}

impl Server {
    pub fn new(guild: GuildId, group: Option<Group>) -> Self {
        info!("New server created for {}", guild);
        Self {
            guild,
            groups: vec![group.unwrap()],
        }
    }

    pub fn groups(&self) -> &Vec<Group> {
        &self.groups
    }

    pub fn groups_mut(&mut self) -> &mut Vec<Group> {
        &mut self.groups
    }

    pub fn find_group_by_queue_channel(&self, channel_id: u64) -> Option<&Group> {
        self.groups.iter().find(|g| g.queue == channel_id)
    }

    pub fn find_group_by_queue_channel_mut(&mut self, channel_id: u64) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.queue == channel_id)
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
    pub session_quota:     u8,
}

impl Group {
    pub fn new(
        dashboard: u64,
        chat: u64,
        queue: u64,
        red: u64,
        blu: u64,
        session_quota: u8,
    ) -> Self {
        info!("New group created for {}", dashboard);
        Self {
            dashboard,
            chat,
            queue,
            teams: vec![TeamChannels { red, blu }],
            session: Vec::new(),
            session_increment: 0,
            session_quota,
        }
    }

    pub fn add_team_channels(&mut self, red: u64, blu: u64) {
        self.teams.push(TeamChannels { red, blu });
    }

    pub fn create_session(&mut self) {
        self.session_increment += 1;
        info!(
            "[model] Creating new session with ID: {}",
            self.session_increment
        );
        self.session.push(Session::new(self.session_increment));
    }

    pub fn end_session(&mut self, session_id: u16) -> bool {
        info!("[model] Attempting to end session with ID: {}", session_id);
        if let Some(pos) = self.session.iter().position(|s| s.id == session_id) {
            self.session.remove(pos);
            info!(
                "[model] Session {} successfully ended and removed",
                session_id
            );
            true
        } else {
            info!(
                "[model] Failed to end session {}: Session not found",
                session_id
            );
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Session {
    pub id:     u16,
    pub status: SessionStatus,
    pub pool:   Vec<SessionPlayer>,
}

impl Session {
    pub fn new(id: u16) -> Self {
        let session = Self {
            id,
            pool: Vec::new(),
            status: SessionStatus::Idle,
        };
        info!("New session started with ID: {}", id);
        session
    }

    pub fn hot(&mut self) {
        // send notif
        info!("[model] Changing session {} status to HOT", self.id);
        self.status = SessionStatus::Hot;
    }

    pub fn push(&mut self) {
        info!("[model] Changing session {} status to PUSH", self.id);
        self.status = SessionStatus::Push;
    }

    pub fn live(&mut self) {
        info!("[model] Changing session {} status to LIVE", self.id);
        self.status = SessionStatus::Live;
    }

    pub fn pull(&mut self) {
        info!("[model] Changing session {} status to PULL", self.id);
        self.status = SessionStatus::Pull;
    }

    pub fn generate_teams(&mut self) {
        info!("[model] Generating teams for session {}", self.id);
        debug!(
            "[model] Cloned {} players for team assignment",
            self.pool.len()
        );
        let mut rng = rand::rng();
        let mut players = self.pool.clone();

        // 1. First add buffered players to genpool (priority)
        let buffered_players = self
            .pool
            .iter()
            .filter(|p| p.buffered.is_some())
            .cloned()
            .collect::<Vec<_>>();

        // 2. Fill remaining slots with non-buffered players
        let remaining_slots = 8 - buffered_players.len(); // Assuming 8 players per match
        let mut non_buffered = self
            .pool
            .iter()
            .filter(|p| p.buffered.is_none())
            .cloned()
            .collect::<Vec<_>>();

        // Take only what we need
        if non_buffered.len() > remaining_slots {
            non_buffered.truncate(remaining_slots);
        }

        // 3. Sort players by ELO in descending order
        players.sort_by(|a, b| {
            let a_elo = a.player.rank.unwrap_or(Rank::Beginner).elo();
            let b_elo = b.player.rank.unwrap_or(Rank::Beginner).elo();

            // Randomize order for players with identical ELO
            if a_elo == b_elo {
                if rng.random::<bool>() {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                b_elo.cmp(&a_elo)
            }
        });

        // 4. Distribute players in snake draft pattern (ABBAABBA)
        let mut team_a = Vec::new();
        let mut team_b = Vec::new();

        for (i, player) in players.iter().enumerate() {
            let mut player_clone = player.clone();

            // Snake draft pattern: 0->A, 1->B, 2->B, 3->A, 4->A, 5->B, 6->B, 7->A
            match i % 4 {
                0 | 3 => {
                    player_clone.team(Team::Red);
                    team_a.push(player_clone);
                }
                1 | 2 => {
                    player_clone.team(Team::Blu);
                    team_b.push(player_clone);
                }
                _ => unreachable!(),
            }
        }

        // 5. Update the original pool with team assignments
        for player in &team_a {
            if let Some(p) = self
                .pool
                .iter_mut()
                .find(|p| p.player.i_discord == player.player.i_discord)
            {
                p.team = Some(Team::Red);
            }
        }

        for player in &team_b {
            if let Some(p) = self
                .pool
                .iter_mut()
                .find(|p| p.player.i_discord == player.player.i_discord)
            {
                p.team = Some(Team::Blu);
            }
        }
    }

    pub fn count(&self) -> usize {
        self.pool.len()
    }

    pub fn get_members(&self) -> Vec<Player> {
        self.pool.iter().map(|m| m.player.clone()).collect()
    }

    pub fn add_player(&mut self, player: &Player) {
        // Clone players for team assignment
        let session_player = SessionPlayer::new(player.clone());
        info!(
            "[model] Player {} added to session {}. Total players: {}",
            player.i_discord,
            self.id,
            self.pool.len()
        );
        self.pool.push(session_player);
    }

    pub fn remove_player(&mut self, player: &SessionPlayer) {
        let before_count = self.pool.len();
        self.pool
            .retain(|p| p.player.i_discord != player.player.i_discord);
        let after_count = self.pool.len();

        if before_count == after_count {
            info!(
                "[model] Player {} not found in session {}",
                player.player.i_discord, self.id
            );
        } else {
            info!(
                "[model] Player {} removed from session {}. Remaining players: {}",
                player.player.i_discord, self.id, after_count
            );
        }
    }

    pub fn buff(&mut self, user_id: u64, buffered: Option<Player>) {
        self.pool
            .iter_mut()
            .find(|m| m.player.i_discord == user_id)
            .unwrap()
            .buff(buffered);
    }

    pub fn unbuff(&mut self, user_id: u64) {
        self.pool
            .iter_mut()
            .find(|m| m.player.i_discord == user_id)
            .unwrap()
            .unbuff();
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player:   Player,
    pub team:     Option<Team>,
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
