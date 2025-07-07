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

/// User data structure representing a player in the system
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    pub guild_id:   u64,
    pub discord_id: u64,
    pub steam_id:   Option<u64>,
    pub rank:       Option<Rank>,
    pub role:       Option<Role>,
}

impl Player {
    pub fn new(discord_id: u64, steam_id64: u64, guild_id: Option<u64>) -> Player {
        info!("Creating new player: discord={}, steam={}, guild={:?}", discord_id, steam_id64, guild_id);
        Player { guild_id: guild_id.unwrap_or(0),
                 discord_id,
                 steam_id: Some(steam_id64),
                 rank: None,
                 role: None }
    }

    pub fn set_rank(&mut self, rank: Option<Rank>) {
        info!("Setting rank for player {}: {:?}", self.discord_id, rank);
        self.rank = rank;
    }

    pub fn set_role(&mut self, role: Option<Role>) {
        info!("Setting role for player {}: {:?}", self.discord_id, role);
        self.role = role;
    }
}
