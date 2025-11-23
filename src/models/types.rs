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

// ============================================================================
// Player
// ============================================================================

/// User data structure representing a player in the system
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    pub discord_id:  UserId,
    pub discord_tag: Option<String>,
    pub steam_id:    Option<u64>,
    pub rank:        Option<Rank>,
    pub role:        Option<Role>,
}

impl Player {
    pub fn add(discord_id: UserId, discord_tag: Option<String>, steam_id: Option<u64>) -> Player {
        Player {
            discord_id,
            discord_tag,
            steam_id,
            rank: None,
            role: None,
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
    pub async fn from_role_id(role_id: RoleId, db: &Database, guild_id: u64) -> Option<Rank> {
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
                return Some(rank);
            }
        }
        None
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
