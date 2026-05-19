use anyhow::Result;
use serenity::all::{GuildId as GI, UserId as UI};
use sqlx::{Row, SqlitePool};

#[derive(Clone)]
pub struct FatkidRepository {
  pool: SqlitePool,
}

impl FatkidRepository {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  /// Get fatkid immunity info for a player
  /// Returns (immunity_level, last_fatkidded_timestamp)
  pub async fn get_immunity(&self, user_id: UI, guild_id: GI) -> Result<(u32, Option<i64>)> {
    let result = sqlx::query(
      "SELECT immunity_level, last_fatkidded_at
             FROM fatkids
             WHERE guild_id = ? AND user_id = ?",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .fetch_optional(&self.pool)
    .await?;

    match result {
      Some(row) => {
        let level: i64 = row.try_get("immunity_level").unwrap_or(0);
        let last_fatkidded: Option<i64> = row.try_get("last_fatkidded_at").ok().flatten();
        Ok((level as u32, last_fatkidded))
      }
      None => Ok((0, None)),
    }
  }

  /// Record a fatkid event - increment immunity level and update timestamp
  pub async fn record_fatkid(&self, user_id: UI, guild_id: GI) -> Result<()> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;

    sqlx::query(
      "INSERT INTO fatkids (guild_id, user_id, immunity_level, last_fatkidded_at)
             VALUES (?, ?, 1, ?)
             ON CONFLICT(guild_id, user_id) DO UPDATE SET 
                immunity_level = immunity_level + 1,
                last_fatkidded_at = excluded.last_fatkidded_at",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .bind(now)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Reset fatkid immunity level (called when immunity expires)
  pub async fn reset_immunity(&self, user_id: UI, guild_id: GI) -> Result<()> {
    sqlx::query(
      "UPDATE fatkids 
             SET immunity_level = 0
             WHERE guild_id = ? AND user_id = ?",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }
}
