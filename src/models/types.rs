use std::sync::Arc;

use anyhow::{Error, anyhow};
use serde::{Deserialize, Serialize};
use serenity::all::{
    CommandInteraction, ComponentInteraction, Context,
    CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM,
    RoleId, UserId,
};
use sqlx::prelude::{FromRow};
use tokio::sync::Mutex;

use crate::{RED, DEFAULT_RANK};
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
            .color(RED);
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

/// User data structure representing a player in a server.
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
    /// Get position index (0-8) for this rank
    pub fn position(&self) -> u8 {
        match self {
            Rank::Beginner    => 0,
            Rank::Newcomer    => 1,
            Rank::Novice      => 2,
            Rank::Apprentice  => 3,
            Rank::Journeyman  => 4,
            Rank::Expert      => 5,
            Rank::Master      => 6,
            Rank::MasterElite => 7,
            Rank::Grandmaster => 8,
        }
    }

    /// Create rank from position index
    pub fn from_position(position: u8) -> Option<Rank> {
        match position {
            0 => Some(Rank::Beginner),
            1 => Some(Rank::Newcomer),
            2 => Some(Rank::Novice),
            3 => Some(Rank::Apprentice),
            4 => Some(Rank::Journeyman),
            5 => Some(Rank::Expert),
            6 => Some(Rank::Master),
            7 => Some(Rank::MasterElite),
            8 => Some(Rank::Grandmaster),
            _ => None,
        }
    }

    /// Get all Discord role IDs that map to this rank from ranks table
    pub async fn role_ids(&self, db: &Database, guild_id: u64) -> Vec<RoleId> {
        if let Ok(Some(guild_rank)) = db.ranks.get_rank(guild_id, self.position()).await {
            guild_rank.role_ids
        } else {
            Vec::new()
        }
    }

    /// Get default name for this rank (fallback when DB not available)
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

    /// Get configurable name from DB, falling back to default
    pub async fn name_from_db(&self, db: &Database, guild_id: u64) -> String {
        if let Ok(Some(guild_rank)) = db.ranks.get_rank(guild_id, self.position()).await {
            guild_rank.name
        } else {
            self.name().to_string()
        }
    }

    pub fn from_name(name: &str) -> Rank {
        match name.to_lowercase().as_str() {
            "beginner"     => Rank::Beginner,
            "newcomer"     => Rank::Newcomer,
            "novice"       => Rank::Novice,
            "apprentice"   => Rank::Apprentice,
            "journeyman"   => Rank::Journeyman,
            "expert"       => Rank::Expert,
            "master"       => Rank::Master,
            "master elite" => Rank::MasterElite,
            "grandmaster"  => Rank::Grandmaster,
            _              => Rank::Journeyman, // Default fallback
        }
    }

    /// Get default ELO for this rank (fallback when DB not available)
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

    /// Determine rank from ELO value using guild-configured values
    pub async fn from_elo(elo: Elo, db: &Database, guild_id: u64) -> Rank {
        if let Ok(Some(guild_rank)) = db.ranks.rank_from_elo(guild_id, elo).await {
            Rank::from_position(guild_rank.position).unwrap_or(Rank::Journeyman)
        } else {
            Rank::from_elo_default(elo)
        }
    }

    /// Determine rank from ELO value using default values (fallback)
    pub fn from_elo_default(elo: Elo) -> Rank {
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
            let next_elo = rank.next_rank().map(|r| r.default_rank_elo()).unwrap_or(101);
            
            if elo >= current_elo && elo < next_elo {
                return rank;
            }
        }
        Rank::Grandmaster
    }

    /// Get ELO value from DB, falling back to default if not set
    pub async fn elo_from_db(&self, db: &Database, guild_id: u64) -> Elo {
        if let Ok(Some(guild_rank)) = db.ranks.get_rank(guild_id, self.position()).await {
            guild_rank.elo
        } else {
            self.default_rank_elo()
        }
    }

    /// Convert a Discord RoleId to a Rank enum using guild ranks table
    pub async fn from_role_id(role_id: RoleId, db: &Database, guild_id: u64) -> Result<Rank, Error> {
        if let Ok(Some(guild_rank)) = db.ranks.rank_from_role_id(guild_id, role_id).await {
            if let Some(rank) = Rank::from_position(guild_rank.position) {
                return Ok(rank);
            }
        }
        Err(anyhow!("Role ID not found in any rank"))
    }

    /// Get all rank role IDs from DB
    pub async fn all_role_ids(db: &Database, guild_id: u64) -> Vec<RoleId> {
        db.ranks.all_role_ids(guild_id).await.unwrap_or_default()
    }

    /// Load all rank-to-role mappings for a guild in a single pass.
    pub async fn load_rank_mappings(db: &Database, guild_id: u64) -> Vec<(Rank, Vec<RoleId>)> {
        let guild_ranks = db.ranks.get_or_init_ranks(guild_id).await.unwrap_or_default();
        guild_ranks.into_iter()
            .filter_map(|gr| {
                Rank::from_position(gr.position).map(|rank| (rank, gr.role_ids))
            })
            .collect()
    }

    /// Find rank from role ID using pre-loaded mappings (no DB calls)
    pub fn from_role_id_cached(role_id: RoleId, mappings: &[(Rank, Vec<RoleId>)]) -> Option<Rank> {
        for (rank, role_ids) in mappings {
            if role_ids.contains(&role_id) {
                return Some(*rank);
            }
        }
        None
    }
}
