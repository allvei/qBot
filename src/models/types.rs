use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serenity::all::{
  CommandInteraction, ComponentInteraction, Context, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, GuildId as GI, RoleId as RI, UserId as UI,
};
use sqlx::prelude::FromRow;
use sqlx::Row;
use tokio::sync::Mutex;
use tracing::warn;

use crate::db::Database;
use crate::guild_name;
use crate::models::{Manager as GameManager, Role};
use crate::RED;

// ============================================================================
// Command Contexts
// ============================================================================

#[derive(Clone)]
pub struct CommandContext<'a> {
  pub ctx: &'a Context,
  pub intax: &'a CommandInteraction,
  pub db: Arc<Database>,
  pub manager: &'a Arc<Mutex<GameManager>>,
}

#[derive(Clone)]
pub struct ComponentContext<'a> {
  pub ctx: &'a Context,
  pub component: &'a ComponentInteraction,
  pub db: Arc<Database>,
  pub manager: &'a Arc<Mutex<GameManager>>,
}

impl CommandContext<'_> {
  /// Create a standard message response
  pub async fn create_response(&self, response: CIR) -> Result<(), anyhow::Error> {
    self.intax.create_response(&self.ctx.http, response).await?;
    Ok(())
  }

  /// Reply with a standard message
  pub async fn reply(&self, message: &str) -> Result<(), anyhow::Error> {
    let response = CIR::Message(CIRM::new().content(message));
    self.create_response(response).await
  }

  /// Reply with an ephemeral message
  pub async fn reply_ephemeral(&self, message: &str) -> Result<(), anyhow::Error> {
    let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
    self.create_response(response).await
  }

  /// Reply with an embed
  pub async fn reply_embed(&self, embed: serenity::all::CreateEmbed) -> Result<(), anyhow::Error> {
    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    self.create_response(response).await
  }

  /// Reply with an error embed
  pub async fn reply_error(&self, title: &str, description: &str) -> Result<(), anyhow::Error> {
    let embed = self.create_error_embed(title, description);
    self.reply_embed(embed).await
  }

  /// Create a standardized error embed
  pub fn create_error_embed(&self, title: &str, description: &str) -> serenity::all::CreateEmbed {
    serenity::all::CreateEmbed::new().title(title).description(description).color(RED)
  }

  /// Get guild ID with error handling
  pub fn guild_id(&self) -> Result<serenity::all::GuildId, anyhow::Error> {
    self.intax.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))
  }

  /// Get guild name with fallback
  pub fn guild_name(&self) -> String {
    self.intax.guild_id.map(|gid| guild_name(self.ctx, gid)).unwrap_or_else(|| "Unknown".to_string())
  }

  /// Check if user has specific permissions
  pub async fn check_permissions(&self, permission_type: PermissionType) -> Result<bool, anyhow::Error> {
    let guild_id = self.guild_id()?;
    let member = self.ctx.http.get_member(guild_id, self.intax.user.id).await?;

    match permission_type {
      PermissionType::Admin => {
        // Check if user has administrator permissions
        Ok(member.permissions(self.ctx.cache.clone()).map(|p| p.administrator()).unwrap_or(false))
      }
      PermissionType::Moderator => {
        // Check specific role or permissions
        // This would need to be implemented based on your bot's role system
        Ok(false)
      }
    }
  }
}

#[derive(Debug)]
pub enum PermissionType {
  Admin,
  Moderator,
}

impl ComponentContext<'_> {
  /// Create a standard component response
  pub async fn create_response(&self, response: CIR) -> Result<(), anyhow::Error> {
    self.component.create_response(&self.ctx.http, response).await?;
    Ok(())
  }

  /// Reply with an ephemeral message
  pub async fn reply_ephemeral(&self, message: &str) -> Result<(), anyhow::Error> {
    let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
    self.create_response(response).await
  }

  /// Reply with an embed
  pub async fn reply_embed(&self, embed: serenity::all::CreateEmbed) -> Result<(), anyhow::Error> {
    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    self.create_response(response).await
  }

  /// Reply with an error embed
  pub async fn reply_error(&self, title: &str, description: &str) -> Result<(), anyhow::Error> {
    let embed = self.create_error_embed(title, description);
    self.reply_embed(embed).await
  }

  /// Create a standardized error embed
  pub fn create_error_embed(&self, title: &str, description: &str) -> serenity::all::CreateEmbed {
    serenity::all::CreateEmbed::new().title(title).description(description).color(RED)
  }

  /// Get guild ID with error handling
  pub fn guild_id(&self) -> Result<serenity::all::GuildId, anyhow::Error> {
    self.component.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))
  }

  /// Get guild name with fallback
  pub fn guild_name(&self) -> String {
    self.component.guild_id.map(|gid| guild_name(self.ctx, gid)).unwrap_or_else(|| "Unknown".to_string())
  }

  /// Check if user has specific permissions
  pub async fn check_permissions(&self, permission_type: PermissionType) -> Result<bool, anyhow::Error> {
    let guild_id = self.guild_id()?;
    let member = self.ctx.http.get_member(guild_id, self.component.user.id).await?;

    match permission_type {
      PermissionType::Admin => {
        // Check if user has administrator permissions
        Ok(member.permissions(self.ctx.cache.clone()).map(|p| p.administrator()).unwrap_or(false))
      }
      PermissionType::Moderator => {
        // Check specific role or permissions
        // This would need to be implemented based on your bot's role system
        Ok(false)
      }
    }
  }

  /// Acknowledge the component interaction
  pub async fn reply_acknowledge(&self) -> Result<(), anyhow::Error> {
    self.create_response(CIR::Acknowledge).await
  }

  pub async fn reply_defer(&self) -> Result<(), anyhow::Error> {
    let response = CIR::Defer(CIRM::new());
    self.create_response(response).await
  }

  pub async fn reply_defer_ephemeral(&self) -> Result<(), anyhow::Error> {
    let response = CIR::Defer(CIRM::new().ephemeral(true));
    self.create_response(response).await
  }

  pub async fn reply_update_message(&self) -> Result<(), anyhow::Error> {
    let response = CIR::UpdateMessage(CIRM::new());
    self.create_response(response).await
  }

  /// Try to acquire a lock for this interaction to prevent duplicate processing.
  /// Use this at the start of any destructive action.
  ///
  /// ### Arguments
  /// * `action_key` - A descriptive key for the action (e.g., "cancel_match_0_0")
  ///
  /// ### Returns
  /// * `Ok(true)` if lock was acquired and action should proceed
  /// * `Ok(false)` if already being processed (silently acknowledge and return)
  pub async fn try_lock_interaction(&self, action_key: &str) -> Result<bool, anyhow::Error> {
    use tracing::debug;

    let mut mgr = self.manager.lock().await;
    let interaction_id = self.component.message.id;
    let acquired = mgr.try_lock_interaction(interaction_id, action_key.to_string());
    
    if !acquired {
      debug!("Interaction already in progress, silently acknowledging: {} (user: {})", action_key, self.component.user.tag());
    }
    
    Ok(acquired)
  }

  /// Release the lock for this interaction after completion or error.
  /// Always call this in a finally block or at the end of processing.
  pub async fn unlock_interaction(&self) {
    let mut mgr = self.manager.lock().await;
    let interaction_id = self.component.message.id;
    mgr.unlock_interaction(interaction_id);
  }
}

/// Player ELO rating (0+ scale)
pub type Elo = u16;

// ============================================================================
// Player
// ============================================================================

/// User data structure representing a player in a server.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
  pub user_id: UI,
  pub tag: String,
  pub queue_expiration: u8,
  pub steam_id: Option<u64>,
  pub rank: Option<Rank>,
  pub elo: Elo,
  pub dynamic_elo: Option<u16>,
  pub role: Option<Role>,
}

impl Player {
  pub fn add(user_id: UI, tag: String, queue_expiration: u8, steam_id: Option<u64>, rank: Option<Rank>) -> Player {
    let elo = rank.as_ref().map_or(50, |r| r.elo); // Default ELO if no rank
    Player { user_id, tag, queue_expiration, steam_id, rank, elo, dynamic_elo: None, role: None }
  }

  pub fn set_steam(&mut self, steam_id: Option<u64>) {
    self.steam_id = steam_id;
  }

  pub fn set_rank(&mut self, rank: Rank) {
    self.elo = rank.elo;
    self.rank = Some(rank);
  }

  pub fn set_elo(&mut self, elo: Elo) {
    self.elo = elo;
  }

  /// Update rank based on ELO using configurable values
  pub async fn update_rank_from_elo(&mut self, db: &Database, guild_id: GI) {
    match Rank::from_elo(db, guild_id, self.elo).await {
      Ok(rank) => {
        self.elo = rank.elo;
        self.rank = Some(rank);
      }
      Err(e) => warn!("Failed to update rank from ELO: {}", e),
    }
  }

  pub fn set_role(&mut self, role: Option<Role>) {
    self.role = role;
  }

  pub async fn get_name(&self, ctx: &Context) -> String {
    let name = &ctx.http.get_user(self.user_id).await.unwrap();
    name.display_name().to_string()
  }
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq)]
pub struct Rank {
  pub guild_id: GI,
  pub role_id: RI,
  pub name: String,
  pub elo: u16,
}

impl Rank {
  pub async fn get_user_rank(db: &Database, guild_id: GI, user_id: UI) -> Result<Self, anyhow::Error> {
    let row = sqlx::query(
      "SELECT r.guild_id, r.name, r.elo, r.role_id 
             FROM elo e 
             JOIN ranks r ON e.rank = r.role_id 
             WHERE e.guild_id = ? AND e.user_id = ?",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .fetch_one(&db.pool)
    .await?;

    Ok(Self { guild_id: GI::new(row.get::<i64, _>(0) as u64), name: row.get(1), elo: row.get(2), role_id: RI::new(row.get::<i64, _>(3) as u64) })
  }

  pub async fn get_guild_default(db: &Database, guild_id: GI) -> Result<Rank, anyhow::Error> {
    let row = sqlx::query(
      "SELECT r.elo, r.name, r.role_id 
             FROM config c 
             JOIN ranks r ON c.default_rank = r.id 
             WHERE c.guild_id = ?",
    )
    .bind(guild_id.get() as i64)
    .fetch_optional(&db.pool)
    .await?;

    match row {
      Some(row) => {
        let elo: u16 = row.get("elo");
        let name: String = row.get("name");
        let role_id: RI = RI::new(row.get::<i64, _>("role_id") as u64);
        Ok(Rank { guild_id, name, elo, role_id })
      }
      None => Self::lowest(db, guild_id).await,
    }
  }

  pub fn get_rank_elo(&self) -> u16 {
    self.elo
  }

  /// Get rank by name from database
  pub async fn from_name(db: &Database, guild_id: GI, name: &str) -> Result<Rank, anyhow::Error> {
    let row = sqlx::query(
      "SELECT role_id, name, elo 
             FROM ranks 
             WHERE guild_id = ? AND LOWER(name) = LOWER(?)",
    )
    .bind(guild_id.get() as i64)
    .bind(name)
    .fetch_one(&db.pool)
    .await?;

    Ok(Rank { guild_id, role_id: RI::new(row.get::<i64, _>("role_id") as u64), name: row.get("name"), elo: row.get("elo") })
  }

  pub async fn lowest(db: &Database, guild_id: GI) -> Result<Rank, anyhow::Error> {
    let row = sqlx::query(
      "SELECT role_id, name, elo 
             FROM ranks 
             WHERE guild_id = ?
             ORDER BY elo ASC 
             LIMIT 1",
    )
    .bind(guild_id.get() as i64)
    .fetch_optional(&db.pool)
    .await?;

    match row {
      Some(row) => Ok(Rank { guild_id, role_id: RI::new(row.get::<i64, _>("role_id") as u64), name: row.get("name"), elo: row.get("elo") }),
      None => Err(anyhow::anyhow!("No ranks configured for this guild")),
    }
  }

  pub async fn from_elo(db: &Database, guild_id: GI, elo: u16) -> Result<Rank, anyhow::Error> {
    let rows = sqlx::query(
      "SELECT role_id, name, elo 
             FROM ranks 
             WHERE guild_id = ? AND elo <= ?
             ORDER BY elo DESC 
             LIMIT 1",
    )
    .bind(guild_id.get() as i64)
    .bind(elo as i64)
    .fetch_optional(&db.pool)
    .await?;

    match rows {
      Some(row) => Ok(Rank { guild_id, role_id: RI::new(row.get::<i64, _>("role_id") as u64), name: row.get("name"), elo: row.get("elo") }),
      None => Err(anyhow::anyhow!("No rank found for ELO {}", elo)),
    }
  }
}
