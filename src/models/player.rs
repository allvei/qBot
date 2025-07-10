// CHECKED

use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use tracing::info;

use crate::models::config::*;
use crate::models::session::Team;
use crate::Session;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    Runner,
    Admin,
}

#[allow(non_snake_case, unreachable_patterns)]
impl Role {
    pub fn id(&self) -> u64 {
        match self {
            Role::Runner => ID_RUNNER,
            Role::Admin => ID_ADMIN,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Role::Runner => "Runner",
            Role::Admin => "Admin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rank {
    Beginner,
    Novice,
    Apprentice,
    Journeyman,
    Master,
    MasterElite,
    Grandmaster,
}

#[allow(non_snake_case, unreachable_patterns)]
impl Rank {
    pub fn id_hardcoded(&self) -> u64 {
        match self {
            Rank::Beginner => ID_EU_BEGINNER,
            Rank::Novice => ID_EU_NOVICE,
            Rank::Apprentice => ID_EU_APPRENTICE,
            Rank::Journeyman => ID_EU_JOURNEYMAN,
            Rank::Master => ID_EU_MASTER,
            Rank::MasterElite => ID_EU_MASTER_ELITE,
            Rank::Grandmaster => ID_EU_GRANDMASTER,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Rank::Beginner => "Beginner",
            Rank::Novice => "Novice",
            Rank::Apprentice => "Apprentice",
            Rank::Journeyman => "Journeyman",
            Rank::Master => "Master",
            Rank::MasterElite => "Master Elite",
            Rank::Grandmaster => "Grandmaster",
        }
    }

    pub fn elo(&self) -> u32 {
        match self {
            Rank::Beginner => 10,
            Rank::Novice => 30,
            Rank::Apprentice => 40,
            Rank::Journeyman => 50,
            Rank::Master => 65,
            Rank::MasterElite => 90,
            Rank::Grandmaster => 95,
        }
    }
}

/// User data structure representing a player in the Discord bot system.
///
/// This struct contains all relevant information about a player including their
/// Discord and Steam identifiers, current session and group affiliations, rank,
/// role, and team assignment. It maintains backreferences to its parent Session
/// and Group for easier navigation between related entities.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    /// Discord user ID of the player
    pub discord_id: u64,
    /// Steam ID64 of the player (optional)
    pub steam_id:   Option<u64>,
    /// Discord guild/server ID where this player belongs
    pub guild_id:   u64,
    /// Historical record of sessions this player has participated in
    pub session:    Vec<Option<Session>>,
    /// Backreference to the current active session ID
    pub session_id: Option<u16>,
    /// Backreference to the current group ID
    pub group_id:   Option<u64>,
    /// Player's skill rank
    pub rank:       Option<Rank>,
    /// Player's preferred role
    pub role:       Option<Role>,
    /// Whether the player is in a buffered state
    pub buffered:   bool,
    /// Current team assignment (Red or Blu)
    pub team:       Option<Team>,
}

impl Player {
    pub fn new(
        discord_id: u64,
        steam_id64: u64,
        guild_id: Option<u64>,
    ) -> Player {
        info!("Creating new player: discord={}, steam={}, guild={:?}", discord_id, steam_id64, guild_id);
        Player {
            guild_id: guild_id.unwrap_or(0),
            discord_id,
            steam_id: Some(steam_id64),
            session: Vec::new(),
            session_id: None,
            group_id: guild_id,
            rank: None,
            role: None,
            buffered: false,
            team: None,
        }
    }

    pub fn set_buffer_status(
        &mut self,
        buffered: bool,
    ) {
        self.buffered = buffered;
    }

    pub fn set_rank(
        &mut self,
        rank: Option<Rank>,
    ) {
        info!("Setting rank for player {}: {:?}", self.discord_id, rank);
        self.rank = rank;
    }

    pub fn set_role(
        &mut self,
        role: Option<Role>,
    ) {
        info!("Setting role for player {}: {:?}", self.discord_id, role);
        self.role = role;
    }

    pub fn set_team(
        &mut self,
        team: Option<Team>,
    ) {
        self.team = team;
    }

    /// Set the current session ID for this player
    pub fn set_session_id(
        &mut self,
        session_id: Option<u16>,
    ) {
        self.session_id = session_id;
        info!("Setting session ID for player {}: {:?}", self.discord_id, session_id);
    }

    /// Set the current group ID for this player
    pub fn set_group_id(
        &mut self,
        group_id: Option<u64>,
    ) {
        self.group_id = group_id;
        info!("Setting group ID for player {}: {:?}", self.discord_id, group_id);
    }

    pub fn by_discord_id(
        players: &[Player],
        discord_id: u64,
    ) -> Option<Player> {
        players.iter().find(|p| p.discord_id == discord_id).cloned()
    }
}
