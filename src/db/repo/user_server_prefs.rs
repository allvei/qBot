use anyhow::Result;
use serenity::all::{GuildId as GI, UserId as UI};
use sqlx::{Row, SqlitePool};

/// Repository for per-server, per-user preferences
#[derive(Clone)]
pub struct UserServerPrefsRepository {
  pool: SqlitePool,
}

impl UserServerPrefsRepository {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  /// Get per-server preference for a user, returns None if not set (use server default)
  async fn get_pref(&self, user_id: UI, guild_id: GI, column: &str) -> Result<Option<bool>> {
    let query = format!("SELECT {column} FROM user_server_prefs WHERE user_id = ? AND guild_id = ?");
    let row = sqlx::query(&query)
      .bind(user_id.get() as i64)
      .bind(guild_id.get() as i64)
      .fetch_optional(&self.pool)
      .await?;

    Ok(row.and_then(|row| row.try_get::<Option<i64>, _>(column).ok().flatten().map(|val| val != 0)))
  }

  /// Set per-server preference for a user (None = use server default)
  async fn set_pref(&self, user_id: UI, guild_id: GI, column: &str, value: Option<bool>) -> Result<()> {
    let query = format!(
      "INSERT INTO user_server_prefs (user_id, guild_id, {column}) VALUES (?, ?, ?) \
       ON CONFLICT(user_id, guild_id) DO UPDATE SET {column} = excluded.{column}"
    );
    
    let value_i64 = value.map(|v| v as i64);
    sqlx::query(&query)
      .bind(user_id.get() as i64)
      .bind(guild_id.get() as i64)
      .bind(value_i64)
      .execute(&self.pool)
      .await?;
    Ok(())
  }

  /// Get vc_auto_join preference for a user in a specific server
  pub async fn get_vc_auto_join(&self, user_id: UI, guild_id: GI) -> Result<Option<bool>> {
    self.get_pref(user_id, guild_id, "vc_auto_join").await
  }

  /// Set vc_auto_join preference for a user in a specific server
  pub async fn set_vc_auto_join(&self, user_id: UI, guild_id: GI, value: Option<bool>) -> Result<()> {
    self.set_pref(user_id, guild_id, "vc_auto_join", value).await
  }

  /// Get vc_auto_leave preference for a user in a specific server
  pub async fn get_vc_auto_leave(&self, user_id: UI, guild_id: GI) -> Result<Option<bool>> {
    self.get_pref(user_id, guild_id, "vc_auto_leave").await
  }

  /// Set vc_auto_leave preference for a user in a specific server
  pub async fn set_vc_auto_leave(&self, user_id: UI, guild_id: GI, value: Option<bool>) -> Result<()> {
    self.set_pref(user_id, guild_id, "vc_auto_leave", value).await
  }

  /// Get vc_leave_queue preference for a user in a specific server
  pub async fn get_vc_leave_queue(&self, user_id: UI, guild_id: GI) -> Result<Option<bool>> {
    self.get_pref(user_id, guild_id, "vc_leave_queue").await
  }

  /// Set vc_leave_queue preference for a user in a specific server
  pub async fn set_vc_leave_queue(&self, user_id: UI, guild_id: GI, value: Option<bool>) -> Result<()> {
    self.set_pref(user_id, guild_id, "vc_leave_queue", value).await
  }

  /// Get all VC preferences for a user in a specific server
  /// Returns (vc_auto_join, vc_auto_leave, vc_leave_queue) where None = use server default
  pub async fn get_all_vc_prefs(&self, user_id: UI, guild_id: GI) -> Result<(Option<bool>, Option<bool>, Option<bool>)> {
    let row = sqlx::query("SELECT vc_auto_join, vc_auto_leave, vc_leave_queue FROM user_server_prefs WHERE user_id = ? AND guild_id = ?")
      .bind(user_id.get() as i64)
      .bind(guild_id.get() as i64)
      .fetch_optional(&self.pool)
      .await?;

    match row {
      Some(row) => {
        let vc_auto_join = row.try_get::<Option<i64>, _>("vc_auto_join").ok().flatten().map(|v| v != 0);
        let vc_auto_leave = row.try_get::<Option<i64>, _>("vc_auto_leave").ok().flatten().map(|v| v != 0);
        let vc_leave_queue = row.try_get::<Option<i64>, _>("vc_leave_queue").ok().flatten().map(|v| v != 0);
        Ok((vc_auto_join, vc_auto_leave, vc_leave_queue))
      }
      None => Ok((None, None, None)),
    }
  }

  /// Set all VC preferences for a user in a specific server (None = use server default)
  pub async fn set_all_vc_prefs(&self, user_id: UI, guild_id: GI, vc_auto_join: Option<bool>, vc_auto_leave: Option<bool>, vc_leave_queue: Option<bool>) -> Result<()> {
    sqlx::query(
      "INSERT INTO user_server_prefs (user_id, guild_id, vc_auto_join, vc_auto_leave, vc_leave_queue) 
       VALUES (?, ?, ?, ?, ?) 
       ON CONFLICT(user_id, guild_id) DO UPDATE SET 
         vc_auto_join = excluded.vc_auto_join,
         vc_auto_leave = excluded.vc_auto_leave,
         vc_leave_queue = excluded.vc_leave_queue"
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .bind(vc_auto_join.map(|v| v as i64))
    .bind(vc_auto_leave.map(|v| v as i64))
    .bind(vc_leave_queue.map(|v| v as i64))
    .execute(&self.pool)
    .await?;
    Ok(())
  }

  /// Delete all preferences for a user in a specific server (revert to server defaults)
  pub async fn delete_prefs(&self, user_id: UI, guild_id: GI) -> Result<()> {
    sqlx::query("DELETE FROM user_server_prefs WHERE user_id = ? AND guild_id = ?")
      .bind(user_id.get() as i64)
      .bind(guild_id.get() as i64)
      .execute(&self.pool)
      .await?;
    Ok(())
  }

  /// Get ping_notification_enabled preference for a user in a specific server
  /// Returns None if not set (user hasn't opted out or opted in yet)
  /// Returns Some(true) if user has opted in to ping notifications
  /// Returns Some(false) if user has opted out of ping notifications
  pub async fn get_ping_notification_enabled(&self, user_id: UI, guild_id: GI) -> Result<Option<bool>> {
    self.get_pref(user_id, guild_id, "ping_notification_enabled").await
  }

  /// Set ping_notification_enabled preference for a user in a specific server
  /// None = user hasn't made a choice yet (default behavior applies)
  /// Some(true) = user has opted in to ping notifications
  /// Some(false) = user has opted out of ping notifications
  pub async fn set_ping_notification_enabled(&self, user_id: UI, guild_id: GI, value: Option<bool>) -> Result<()> {
    self.set_pref(user_id, guild_id, "ping_notification_enabled", value).await
  }
}
