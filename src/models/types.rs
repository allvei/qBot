use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serenity::all::{
    CommandInteraction, ComponentInteraction, Context,
    CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM,
    RoleId, UserId,
};
use sqlx::prelude::FromRow;
use tokio::sync::Mutex;

use crate::database::Database;
use crate::models::{
    Manager as GameManager, Role,
    EU_APPRENTICE_R_ID, EU_BEGINNER_R_ID, EU_GRANDMASTER_R_ID, EU_JOURNEYMAN_R_ID,
    EU_MASTER_ELITE_R_ID, EU_MASTER_R_ID, EU_NOVICE_R_ID,
};

// ============================================================================
// Command Contexts
// ============================================================================

#[derive(Clone)]
pub struct CommandContext<'a> {
    pub ctx:     &'a Context,
    pub intax:   &'a CommandInteraction,
    pub db:      Arc<Database>,
    pub manager: &'a Arc<Mutex<GameManager>>,
}

#[derive(Clone)]
pub struct ComponentContext<'a> {
    pub ctx:       &'a Context,
    pub component: &'a ComponentInteraction,
    pub db:        Arc<Database>,
}

impl CommandContext<'_> {
    pub async fn create_bot_reply(&self, message: &str) -> Result<(), anyhow::Error> {
        let response = CIR::Message(
            CIRM::new().content(message).ephemeral(true)
        );
        self.intax.create_response(&self.ctx.http, response).await?;
        Ok(())
    }
}

impl ComponentContext<'_> {
    pub async fn create_bot_reply(&self, message: &str) -> Result<(), anyhow::Error> {
        let response = CIR::Message(
            CIRM::new().content(message).ephemeral(true)
        );
        self.component.create_response(&self.ctx.http, response).await?;
        Ok(())
    }
}

// ============================================================================
// Player
// ============================================================================

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
    pub fn add(discord_id: UserId, steam_id: Option<u64>) -> Player {
        Player {
            discord_id,
            steam_id,
            rank:     None,
            role:     None,
        }
    }

    pub fn set_steam(&mut self, steam_id: Option<u64>) {
        self.steam_id = steam_id;
    }

    pub fn set_rank(&mut self, rank: Option<Rank>) {
        self.rank = rank;
    }

    pub fn set_role(&mut self, role: Option<Role>) {
        self.role = role;
    }

    pub async fn get_name(&self, ctx: &Context) -> String {
        let name = &ctx.http.get_user(self.discord_id).await.unwrap();
        name.display_name().to_string()
    }
}

// ============================================================================
// Rank
// ============================================================================

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
