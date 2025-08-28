// CHECKED

use serde::{
    Deserialize,
    Serialize,
};
use serenity::all::{Context, RoleId, UserId};
use sqlx::prelude::FromRow;
use tracing::info;

use crate::models::data::*;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    Runner,
    Admin,
}

#[allow(non_snake_case, unreachable_patterns)]
impl Role {
    pub fn id(&self) -> RoleId {
        match self {
            Role::Runner => RUNNER_R_ID.into(),
            Role::Admin  => ADMIN_R_ID.into(),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Role::Runner => "Runner",
            Role::Admin  => "Admin",
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
    pub fn id_hardcoded(&self) -> UserId {
        match self {
            Rank::Beginner    => EU_BEGINNER_R_ID.into(),
            Rank::Novice      => EU_NOVICE_R_ID.into(),
            Rank::Apprentice  => EU_APPRENTICE_R_ID.into(),
            Rank::Journeyman  => EU_JOURNEYMAN_R_ID.into(),
            Rank::Master      => EU_MASTER_R_ID.into(),
            Rank::MasterElite => EU_MASTER_ELITE_R_ID.into(),
            Rank::Grandmaster => EU_GRANDMASTER_R_ID.into(),
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, FromRow)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    pub discord_id: UserId,
    pub steam_id:   Option<u64>,
    pub rank:       Option<Rank>,
    pub role:       Option<Role>,
}

impl Player {
    pub fn construct(discord_id: UserId, steam_id: Option<u64>) -> Player {
        Player {
            discord_id,
            steam_id,
            rank: None,
            role: None,
        }
    }

    pub fn set_rank(&mut self, rank: Option<Rank>) {
        info!("Setting rank for player {}: {:?}", self.discord_id, rank);
        self.rank = rank;
    }

    pub fn set_role(&mut self, role: Option<Role>) {
        info!("Setting role for player {}: {:?}", self.discord_id, role);
        self.role = role;
    }

    pub async fn get_name(&self, ctx: &Context) -> String {
        let name = &ctx.http.get_user(self.discord_id).await.unwrap();
        name.display_name().to_string()
    }
}
