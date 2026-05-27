use anyhow::Result;
use sqlx::{Pool, Row, Sqlite};
use std::time::UNIX_EPOCH;
use tracing::{debug, info};

use crate::state_transfer::{BotState, StateTransfer};
use crate::Manager;

#[derive(Clone)]
pub struct StateRepository {
  pool: Pool<Sqlite>,
}

impl StateRepository {
  pub fn new(pool: &Pool<Sqlite>) -> Self {
    Self { pool: pool.clone() }
  }

  pub async fn save_manager(&self, manager: &Manager) -> Result<()> {
    let state = BotState::new(manager.clone());
    self.save(&state).await
  }

  pub async fn load_manager(&self, max_age_secs: u64) -> Result<Option<Manager>> {
    let state_opt: Option<BotState> = self.load().await?;
    if let Some(state) = state_opt {
      if state.age().as_secs() < max_age_secs && state.is_compatible_version() {
        debug!("Loaded bot state from {} seconds ago (version {})", state.age().as_secs(), state.version);
        return Ok(Some(state.manager));
      } else if !state.is_compatible_version() {
        info!("Skipping state restore: version mismatch (saved: {}, current: {})", state.version, env!("CARGO_PKG_VERSION"));
      } else {
        info!("Skipping state restore: too old ({} seconds > {} max)", state.age().as_secs(), max_age_secs);
      }
    }
    Ok(None)
  }
}

#[async_trait::async_trait]
impl StateTransfer for StateRepository {
  async fn save(&self, state: &BotState) -> Result<()> {
    let json = serde_json::to_string(&state.manager)?;
    let version = &state.version;
    let saved_at = state.saved_at.duration_since(UNIX_EPOCH)?.as_secs() as i64;

    sqlx::query("INSERT OR REPLACE INTO bot_state (id, manager_json, saved_at, version) VALUES (1, ?, ?, ?)")
      .bind(&json)
      .bind(saved_at)
      .bind(version)
      .execute(&self.pool)
      .await?;

    debug!("Saved bot state ({} guilds, {} bytes)", state.manager.qguilds.len(), json.len());
    Ok(())
  }

  async fn load(&self) -> Result<Option<BotState>> {
    let row = sqlx::query("SELECT manager_json, saved_at, version FROM bot_state WHERE id = 1").fetch_optional(&self.pool).await?;

    if let Some(row) = row {
      let json: String = row.try_get("manager_json")?;
      let saved_at: i64 = row.try_get("saved_at")?;
      let version: String = row.try_get("version")?;

      let manager: Manager = serde_json::from_str(&json)?;
      let saved_at = UNIX_EPOCH + std::time::Duration::from_secs(saved_at as u64);

      Ok(Some(BotState { version, saved_at, manager }))
    } else {
      Ok(None)
    }
  }

  async fn clear(&self) -> Result<()> {
    sqlx::query("DELETE FROM bot_state WHERE id = 1").execute(&self.pool).await?;
    debug!("Cleared bot state");
    Ok(())
  }

  async fn health_check(&self) -> Result<()> {
    sqlx::query("SELECT 1 FROM bot_state LIMIT 1").fetch_optional(&self.pool).await?;
    Ok(())
  }
}
