use anyhow::Result;
use sqlx::{Row, SqlitePool};

#[derive(Clone)]
pub struct GameReadyNotifRepository {
  pool: SqlitePool,
}

impl GameReadyNotifRepository {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  /// Save a notification (insert or replace)
  pub async fn save_notification(&self, channel_id: u64, message_id: u64) -> Result<()> {
    let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;

    sqlx::query(
      "INSERT OR REPLACE INTO game_ready_notifs (channel_id, message_id, created_at)
       VALUES (?, ?, ?)",
    )
    .bind(channel_id as i64)
    .bind(message_id as i64)
    .bind(now)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Delete a specific notification
  pub async fn delete_notification(&self, channel_id: u64, message_id: u64) -> Result<()> {
    sqlx::query(
      "DELETE FROM game_ready_notifs
       WHERE channel_id = ? AND message_id = ?",
    )
    .bind(channel_id as i64)
    .bind(message_id as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Check if a notification exists
  pub async fn notification_exists(&self, channel_id: u64, message_id: u64) -> Result<bool> {
    let result = sqlx::query(
      "SELECT COUNT(*) as count
       FROM game_ready_notifs
       WHERE channel_id = ? AND message_id = ?",
    )
    .bind(channel_id as i64)
    .bind(message_id as i64)
    .fetch_one(&self.pool)
    .await?;

    let count: i64 = result.try_get("count").unwrap_or(0);
    Ok(count > 0)
  }

  /// Delete all notifications (for one-time migration cleanup)
  pub async fn clear_all(&self) -> Result<()> {
    sqlx::query("DELETE FROM game_ready_notifs")
      .execute(&self.pool)
      .await?;

    Ok(())
  }
}
