use anyhow::Result;
use serenity::all::GuildId as GI;
use sqlx::{Row, SqlitePool};

#[derive(Clone)]
pub struct CaptainDraftRepository {
  pool: SqlitePool,
}

impl CaptainDraftRepository {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  /// Save a captain draft embed record
  pub async fn save_draft(&self, guild_id: GI, category_id: u8, format_id: u8, channel_id: u64, message_id: u64) -> Result<()> {
    let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;

    sqlx::query(
      "INSERT INTO captain_drafts (guild_id, category_id, format_id, channel_id, message_id, created_at)
       VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(guild_id.get() as i64)
    .bind(category_id as i64)
    .bind(format_id as i64)
    .bind(channel_id as i64)
    .bind(message_id as i64)
    .bind(now)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Delete a specific captain draft record
  pub async fn delete_draft(&self, guild_id: GI, category_id: u8, format_id: u8) -> Result<()> {
    sqlx::query(
      "DELETE FROM captain_drafts
       WHERE guild_id = ? AND category_id = ? AND format_id = ?",
    )
    .bind(guild_id.get() as i64)
    .bind(category_id as i64)
    .bind(format_id as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Get all captain draft records for a guild (for cleanup on startup)
  pub async fn get_all_for_guild(&self, guild_id: GI) -> Result<Vec<(u64, u64)>> {
    let rows = sqlx::query(
      "SELECT channel_id, message_id
       FROM captain_drafts
       WHERE guild_id = ?",
    )
    .bind(guild_id.get() as i64)
    .fetch_all(&self.pool)
    .await?;

    let results = rows
      .into_iter()
      .filter_map(|row| {
        let channel_id: i64 = row.try_get("channel_id").ok()?;
        let message_id: i64 = row.try_get("message_id").ok()?;
        Some((channel_id as u64, message_id as u64))
      })
      .collect();

    Ok(results)
  }

  /// Check if a captain draft exists for a specific format
  pub async fn draft_exists(&self, guild_id: GI, category_id: u8, format_id: u8) -> Result<bool> {
    let result = sqlx::query(
      "SELECT COUNT(*) as count
       FROM captain_drafts
       WHERE guild_id = ? AND category_id = ? AND format_id = ?",
    )
    .bind(guild_id.get() as i64)
    .bind(category_id as i64)
    .bind(format_id as i64)
    .fetch_one(&self.pool)
    .await?;

    let count: i64 = result.try_get("count").unwrap_or(0);
    Ok(count > 0)
  }

  /// Delete all captain draft records for a guild (for startup cleanup)
  pub async fn delete_all_for_guild(&self, guild_id: GI) -> Result<()> {
    sqlx::query(
      "DELETE FROM captain_drafts
       WHERE guild_id = ?",
    )
    .bind(guild_id.get() as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }
}
