use serde::{Deserialize, Serialize};
use serenity::all::RoleId;

use crate::models::data::*;



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
    pub fn id_hardcoded(&self) -> RoleId {
        match self {
            Rank::Beginner    => EU_BEGINNER_R_ID    .into(),
            Rank::Novice      => EU_NOVICE_R_ID      .into(),
            Rank::Apprentice  => EU_APPRENTICE_R_ID  .into(),
            Rank::Journeyman  => EU_JOURNEYMAN_R_ID  .into(),
            Rank::Master      => EU_MASTER_R_ID      .into(),
            Rank::MasterElite => EU_MASTER_ELITE_R_ID.into(),
            Rank::Grandmaster => EU_GRANDMASTER_R_ID .into(),
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