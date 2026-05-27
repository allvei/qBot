use anyhow::Result;
use sqlx::SqlitePool;

use crate::{ansi::*, cinfo, QGuild};

#[derive(Clone)]
pub struct GuildRepository {
  pool: SqlitePool,
}

impl GuildRepository {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  pub async fn add(guild: &QGuild) -> Result<()> {
    cinfo!("{GREEN}Adding a new guild to the database: {} {}", guild.name, guild.id);
    Ok(())
  }

  pub async fn remove(guild: &QGuild) -> Result<()> {
    cinfo!("{RED}Removing a guild from the database: {} {}", guild.name, guild.id);
    Ok(())
  }

  /// Update the guild name for a category
  pub async fn update_name(&self, guild: &QGuild) -> Result<()> {
    sqlx::query("UPDATE guilds SET name = ? WHERE guild_id = ?").bind(guild.name.clone()).bind(guild.id.get() as i64).execute(&self.pool).await?;
    Ok(())
  }

  pub async fn exists(&self, guild_id: &serenity::all::GuildId) -> Result<bool> {
    match sqlx::query("SELECT * FROM guilds WHERE guild_id = ?").bind(guild_id.get() as i64).execute(&self.pool).await {
      Ok(_) => Ok(true),
      Err(e) => Err(anyhow::anyhow!("{e}")),
    }
  }
}
