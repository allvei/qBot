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

  pub async fn add(&self, guild: &QGuild) -> Result<()> {
    cinfo!("{GREEN}Adding a new guild to the database: {} {}", guild.name, guild.id);
    sqlx::query("INSERT OR IGNORE INTO guilds (guild_id, name) VALUES (?, ?)").bind(guild.id.get() as i64).bind(&guild.name).execute(&self.pool).await?;
    Ok(())
  }

  pub async fn remove(&self, guild: &QGuild) -> Result<()> {
    cinfo!("{RED}Removing a guild from the database: {} {}", guild.name, guild.id);
    sqlx::query("DELETE FROM guilds WHERE guild_id = ?").bind(guild.id.get() as i64).execute(&self.pool).await?;
    Ok(())
  }

  /// Update the guild name for a category
  pub async fn update_name(&self, guild: &QGuild) -> Result<()> {
    sqlx::query("UPDATE guilds SET name = ? WHERE guild_id = ?").bind(guild.name.clone()).bind(guild.id.get() as i64).execute(&self.pool).await?;
    Ok(())
  }

  /// Returns nick if set, otherwise the Discord name, or None if the guild is not in the DB.
  pub async fn get_display_name(&self, guild_id: serenity::all::GuildId) -> Result<Option<String>> {
    let name: Option<String> =
      sqlx::query_scalar("SELECT COALESCE(nick, name) FROM guilds WHERE guild_id = ?").bind(guild_id.get() as i64).fetch_optional(&self.pool).await?;
    Ok(name)
  }

  pub async fn exists(&self, guild_id: &serenity::all::GuildId) -> Result<bool> {
    let row = sqlx::query("SELECT 1 FROM guilds WHERE guild_id = ?").bind(guild_id.get() as i64).fetch_optional(&self.pool).await?;
    Ok(row.is_some())
  }
}
