// CHECKED

use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use tracing::info;

use crate::models::config::*;

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
          Role::Admin  => ID_ADMIN,
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

impl Rank {
    pub fn id(&self) -> u64 {
        match self {
            Rank::Beginner    => ID_EU_BEGINNER,
            Rank::Novice      => ID_EU_NOVICE,
            Rank::Apprentice  => ID_EU_APPRENTICE,
            Rank::Journeyman  => ID_EU_JOURNEYMAN,
            Rank::Master      => ID_EU_MASTER,
            Rank::MasterElite => ID_EU_MASTER_ELITE,
            Rank::Grandmaster => ID_EU_GRANDMASTER,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Rank::Beginner    => "Beginner",
            Rank::Novice      => "Novice",
            Rank::Apprentice  => "Apprentice",
            Rank::Journeyman  => "Journeyman",
            Rank::Master      => "Master",
            Rank::MasterElite => "Master Elite",
            Rank::Grandmaster => "Grandmaster",
        }
    }

    pub fn elo(&self) -> u32 {
        match self {
            Rank::Beginner    => 10,
            Rank::Novice      => 30,
            Rank::Apprentice  => 40,
            Rank::Journeyman  => 50,
            Rank::Master      => 65,
            Rank::MasterElite => 90,
            Rank::Grandmaster => 95,
        }
    }
}

/// User data structure representing a player in the system
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    pub i_guild:   u64,
    pub i_discord: u64,
    pub i_steam:   Option<u64>,
    pub rank:      Option<Rank>,
    pub role:      Option<Role>,
}

impl Player {
    pub fn new(i_discord: u64, i_steam64: u64, i_guild: Option<u64>) -> Player {
        info!("[player] Creating new player: discord={}, steam={}, guild={:?}", i_discord, i_steam64, i_guild);
        Player {
            i_guild: i_guild.unwrap_or(0),
            i_discord,
            i_steam: Some(i_steam64),
            rank: None,
            role: None,
        }
    }

    pub fn set_rank(&mut self, rank: Option<Rank>) {
        info!("[player] Setting rank for player {}: {:?}", self.i_discord, rank);
        self.rank = rank;
    }

    pub fn set_role(&mut self, role: Option<Role>) {
        info!("[player] Setting role for player {}: {:?}", self.i_discord, role);
        self.role = role;
    }
}
