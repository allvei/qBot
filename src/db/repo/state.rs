use anyhow::Result;
use sqlx::{Pool, Row, Sqlite};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::state_transfer::{BotState, StateTransfer};
use crate::Manager;

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(5);
const AUTOSAVE_HEARTBEAT: Duration = Duration::from_secs(300);

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

  async fn write_json(&self, json: &str, version: &str, saved_at: SystemTime) -> Result<()> {
    let saved_at = saved_at.duration_since(UNIX_EPOCH)?.as_secs() as i64;
    sqlx::query("INSERT OR REPLACE INTO bot_state (id, manager_json, saved_at, version) VALUES (1, ?, ?, ?)")
      .bind(json)
      .bind(saved_at)
      .bind(version)
      .execute(&self.pool)
      .await?;
    Ok(())
  }

  /// Continuously persist queue/session state so a crash or kill loses at most `AUTOSAVE_INTERVAL`.
  /// Writes only when the serialized state changed, plus a heartbeat so `saved_at` stays inside
  /// the restore age limit while the queue is idle. Must be started after state restoration,
  /// otherwise the not-yet-restored (empty) manager would overwrite the saved state.
  pub async fn run_autosave(self, manager: Arc<Mutex<Manager>>) {
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + AUTOSAVE_INTERVAL, AUTOSAVE_INTERVAL);
    let mut last: Option<(u64, Instant)> = None;

    loop {
      ticker.tick().await;

      let json = {
        let Ok(manager) = manager.try_lock() else { continue };
        match serde_json::to_string(&*manager) {
          Ok(json) => json,
          Err(e) => {
            warn!("Autosave: failed to serialize manager state: {}", e);
            continue;
          }
        }
      };

      let mut hasher = DefaultHasher::new();
      json.hash(&mut hasher);
      let hash = hasher.finish();

      if matches!(last, Some((h, at)) if h == hash && at.elapsed() < AUTOSAVE_HEARTBEAT) {
        continue;
      }

      match self.write_json(&json, env!("CARGO_PKG_VERSION"), SystemTime::now()).await {
        Ok(_) => {
          debug!("Autosaved bot state ({} bytes)", json.len());
          last = Some((hash, Instant::now()));
        }
        Err(e) => warn!("Autosave: failed to write manager state: {}", e),
      }
    }
  }
}

#[async_trait::async_trait]
impl StateTransfer for StateRepository {
  async fn save(&self, state: &BotState) -> Result<()> {
    let json = serde_json::to_string(&state.manager)?;
    self.write_json(&json, &state.version, state.saved_at).await?;
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
