use std::sync::Arc;

use anyhow::{Error, anyhow};
use serde::{Deserialize, Serialize};
use serenity::all::{
    CommandInteraction, ComponentInteraction, Context,
    CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM,
    RoleId, UserId,
};
use sqlx::Decode;
use sqlx::prelude::{FromRow, Type};
use tokio::sync::Mutex;

use crate::DEFAULT_RANK;
use crate::database::Database;
use crate::models::{
    Manager as GameManager, Role,
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
    pub manager:   &'a Arc<Mutex<GameManager>>,
}

impl CommandContext<'_> {
    pub async fn reply(&self, message: &str) -> Result<(), anyhow::Error> {
        let response = CIR::Message(CIRM::new().content(message));
        self.intax.create_response(&self.ctx.http, response).await?;
        Ok(())
    }

    pub async fn reply_embed(&self, embed: serenity::all::CreateEmbed) -> Result<(), anyhow::Error> {
        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        self.intax.create_response(&self.ctx.http, response).await?;
        Ok(())
    }

    pub async fn reply_error(&self, title: &str, description: &str) -> Result<(), anyhow::Error> {
        let embed = serenity::all::CreateEmbed::new()
            .title(title)
            .description(description)
            .color(0xff0000);
        self.reply_embed(embed).await
    }

    pub fn guild_id(&self) -> Result<serenity::all::GuildId, anyhow::Error> {
        self.intax.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))
    }

    pub fn guild_name(&self) -> String {
        self.intax.guild_id
            .and_then(|gid| self.ctx.cache.guild(gid).map(|g| g.name.clone()))
            .unwrap_or_else(|| "Unknown".to_string())
    }
}

impl ComponentContext<'_> {
    pub async fn reply(&self, message: &str) -> Result<(), anyhow::Error> {
        let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
        self.component.create_response(&self.ctx.http, response).await?;
        Ok(())
    }

    pub async fn acknowledge(&self) -> Result<(), anyhow::Error> {
        let response = CIR::Acknowledge;
        self.component.create_response(&self.ctx.http, response).await?;
        Ok(())
    }

    pub async fn defer_update(&self) -> Result<(), anyhow::Error> {
        let response = CIR::UpdateMessage(CIRM::new());
        self.component.create_response(&self.ctx.http, response).await?;
        Ok(())
    }

    pub fn guild_id(&self) -> Result<serenity::all::GuildId, anyhow::Error> {
        self.component.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))
    }

    pub fn guild_name(&self) -> String {
        self.component.guild_id
            .and_then(|gid| self.ctx.cache.guild(gid).map(|g| g.name.clone()))
            .unwrap_or_else(|| "Unknown".to_string())
    }
}

/// Player ELO rating (0-100 scale)
pub type Elo = u16;

// ============================================================================
// Player
// ============================================================================

/// User data structure representing a player in the system
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    pub user_id:  UserId,
    pub tag:      String,
    pub steam_id: Option<u64>,
    pub rank:     Rank,
    pub elo:      Elo,
    pub role:     Option<Role>,
}

impl Player {
    pub fn default(user_id: UserId, tag: String, steam_id: Option<u64>) -> Player {
        Player {
            user_id,
            tag,
            steam_id,
            rank: DEFAULT_RANK,
            elo:  DEFAULT_RANK.default_rank_elo(),
            role: None,
        }
    }

    pub fn add(user_id: UserId, tag: String, steam_id: Option<u64>, rank: Rank) -> Player {
        Player {
            user_id,
            tag,
            steam_id,
            rank,
            elo:  rank.default_rank_elo(),
            role: None,
        }
    }

    pub fn set_steam(&mut self, steam_id: Option<u64>) {
        self.steam_id = steam_id;
    }

    pub fn set_rank(&mut self, rank: Rank) {
        self.rank = rank;
    }

    pub fn set_elo(&mut self, elo: Elo) {
        self.elo = elo;
    }

    /// Update rank based on ELO using configurable values
    pub async fn update_rank_from_elo(&mut self, db: &Database, guild_id: u64) {
            self.rank = Rank::from_elo(self.elo, db, guild_id).await;
    }

    /// Update rank based on ELO using default values (fallback)
    pub fn update_rank_from_elo_default(&mut self) {
        self.rank = Rank::from_elo_default(self.elo);
    }

    pub fn set_role(&mut self, role: Option<Role>) {
        self.role = role;
    }

    pub async fn get_name(&self, ctx: &Context) -> String {
        let name = &ctx.http.get_user(self.user_id).await.unwrap();
        name.display_name().to_string()
    }
}

// ============================================================================
// Rank
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rank {
    Beginner,
    Newcomer,
    Novice,
    Apprentice,
    Journeyman,
    Expert,
    Master,
    MasterElite,
    Grandmaster,
}

#[allow(non_snake_case, unreachable_patterns)]
impl Rank {
    /// Get config key for this rank's role IDs
    pub fn config_key(&self) -> &'static str {
        match self {
            Rank::Beginner    => "rank_beginner_roles",
            Rank::Newcomer    => "rank_newcomer_roles",
            Rank::Novice      => "rank_novice_roles",
            Rank::Apprentice  => "rank_apprentice_roles",
            Rank::Journeyman  => "rank_journeyman_roles",
            Rank::Expert      => "rank_expert_roles",
            Rank::Master      => "rank_master_roles",
            Rank::MasterElite => "rank_master_elite_roles",
            Rank::Grandmaster => "rank_grandmaster_roles",
        }
    }

    /// Get all Discord role IDs that map to this rank from config
    /// Format: comma-separated role IDs, e.g., "1234567890,9876543210"
    /// Returns empty vector if no config is set (roles need to be created)
    pub async fn role_ids(&self, db: &Database, guild_id: u64) -> Vec<RoleId> {
        if let Ok(Some(value)) = db.config.get_config_value(self.config_key(), guild_id).await {
            value.split(',')
                .filter_map(|s| s.trim().parse::<u64>().ok())
                .map(RoleId::new)
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Rank::Beginner    => "Beginner",
            Rank::Newcomer    => "Newcomer",
            Rank::Novice      => "Novice",
            Rank::Apprentice  => "Apprentice",
            Rank::Journeyman  => "Journeyman",
            Rank::Expert      => "Expert",
            Rank::Master      => "Master",
            Rank::MasterElite => "Master Elite",
            Rank::Grandmaster => "Grandmaster",
        }
    }

    pub fn elo(&self) -> u32 {
        match self {
            Rank::Beginner    => 10,
            Rank::Newcomer    => 30,
            Rank::Novice      => 40,
            Rank::Apprentice  => 50,
            Rank::Journeyman  => 65,
            Rank::Expert      => 75,
            Rank::Master      => 85,
            Rank::MasterElite => 90,
            Rank::Grandmaster => 95,
        }
    }

    /// Get default ELO for this rank (renamed from elo for clarity)
    pub fn default_rank_elo(&self) -> Elo {
        match self {
            Rank::Beginner    => 10,
            Rank::Newcomer    => 30,
            Rank::Novice      => 40,
            Rank::Apprentice  => 50,
            Rank::Journeyman  => 65,
            Rank::Expert      => 75,
            Rank::Master      => 85,
            Rank::MasterElite => 90,
            Rank::Grandmaster => 95,
        }
    }

    /// Get the next higher rank (for ELO comparison)
    pub fn next_rank(&self) -> Option<Rank> {
        match self {
            Rank::Beginner    => Some(Rank::Newcomer),
            Rank::Newcomer    => Some(Rank::Novice),
            Rank::Novice      => Some(Rank::Apprentice),
            Rank::Apprentice  => Some(Rank::Journeyman),
            Rank::Journeyman  => Some(Rank::Expert),
            Rank::Expert      => Some(Rank::Master),
            Rank::Master      => Some(Rank::MasterElite),
            Rank::MasterElite => Some(Rank::Grandmaster),
            Rank::Grandmaster => None,
        }
    }

    /// Determine rank from ELO value using simple comparison with config
    pub async fn from_elo(elo: Elo, db: &Database, guild_id: u64) -> Rank {
        // Check each rank in order - if ELO >= rank_elo and < next_rank_elo, that's the rank
        for rank in [
            Rank::Beginner,
            Rank::Newcomer,
            Rank::Novice,
            Rank::Apprentice,
            Rank::Journeyman,
            Rank::Expert,
            Rank::Master,
            Rank::MasterElite,
        ] {
            let current_elo = rank.elo_from_config(db, guild_id).await as Elo;
            let next_elo = match rank.next_rank() {
                Some(next_rank) => next_rank.elo_from_config(db, guild_id).await as Elo,
                None => 101, // Grandmaster max is 100, use 101 as upper bound
            };
            
            if elo >= current_elo && elo < next_elo {
                return rank;
            }
        }
        
        // If ELO is >= Grandmaster (95) or above max (100), return Grandmaster
        Rank::Grandmaster
    }

    /// Determine rank from ELO value using default values (fallback)
    pub fn from_elo_default(elo: Elo) -> Rank {
        // Check each rank in order - if ELO >= rank_elo and < next_rank_elo, that's the rank
        for rank in [
            Rank::Beginner,
            Rank::Newcomer,
            Rank::Novice,
            Rank::Apprentice,
            Rank::Journeyman,
            Rank::Expert,
            Rank::Master,
            Rank::MasterElite,
        ] {
            let current_elo = rank.default_rank_elo();
            let next_elo = rank.next_rank().map(|r| r.default_rank_elo()).unwrap_or(101); // Grandmaster max is 100, use 101 as upper bound
            
            if elo >= current_elo && elo < next_elo {
                return rank;
            }
        }
        
        // If ELO is >= Grandmaster (95) or above max (100), return Grandmaster
        Rank::Grandmaster
    }

    /// Get ELO value from config, falling back to default if not set
    pub async fn elo_from_config(&self, db: &Database, guild_id: u64) -> u32 {
        let config_key = format!("rank_{}_elo", self.name().to_lowercase().replace(" ", "_"));

        if let Ok(Some(value)) = db.config.get_config_value(&config_key, guild_id).await {
            if let Ok(elo) = value.parse::<u32>() {
                return elo;
            }
        }

        // Fall back to default ELO
        self.elo()
    }

    /// Convert a Discord RoleId to a Rank enum using guild config
    /// Supports multiple Discord roles mapping to the same rank (EU/NA/Retired variants)
    pub async fn from_role_id(role_id: RoleId, db: &Database, guild_id: u64) -> Result<Rank, Error> {
        // Check each rank's role IDs
        for rank in [
            Rank::Beginner,
            Rank::Newcomer,
            Rank::Novice,
            Rank::Apprentice,
            Rank::Journeyman,
            Rank::Expert,
            Rank::Master,
            Rank::MasterElite,
            Rank::Grandmaster,
        ] {
            if rank.role_ids(db, guild_id).await.contains(&role_id) {
                return Ok(rank);
            }
        }

        Err(anyhow!("Role ID not found in any rank"))
    }

    /// Get all rank role IDs from config (including all regional and retired variants)
    pub async fn all_role_ids(db: &Database, guild_id: u64) -> Vec<RoleId> {
        let mut all_ids = Vec::new();
        for rank in [
            Rank::Beginner,
            Rank::Newcomer,
            Rank::Novice,
            Rank::Apprentice,
            Rank::Journeyman,
            Rank::Expert,
            Rank::Master,
            Rank::MasterElite,
            Rank::Grandmaster,
        ] {
            all_ids.extend(rank.role_ids(db, guild_id).await);
        }
        all_ids
    }
}
