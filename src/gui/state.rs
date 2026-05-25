//! Shared state between GUI and tokio thread

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};

use crate::gui::commands::GuiCommand;
use crate::models::Player;
use crate::{Database, Manager};
use serenity::all::{Context, GuildId};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Persistent GUI settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiSettings {
  pub theme: ThemeChoice,
  pub font_size: f32,
  pub log_buffer_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
  Dark,
  Light,
}

impl Default for GuiSettings {
  fn default() -> Self {
    Self {
      theme: ThemeChoice::Dark,
      font_size: 14.0,
      log_buffer_size: 1000,
    }
  }
}

impl GuiSettings {
  pub fn load() -> Self {
    // Try to load from file, fallback to defaults
    std::fs::read_to_string("gui_settings.json")
      .ok()
      .and_then(|s| serde_json::from_str(&s).ok())
      .unwrap_or_default()
  }

  pub fn save(&self) {
    if let Ok(json) = serde_json::to_string_pretty(self) {
      let _ = std::fs::write("gui_settings.json", json);
    }
  }
}

/// Shared state accessible from both GUI (main thread) and tokio thread
pub struct GuiSharedState {
  /// Manager containing all bot state
  pub manager: Arc<Mutex<Manager>>,
  /// Database connection
  pub db: Arc<Database>,
  /// Ring buffer for GUI log viewer (max 1000 lines)
  pub log_buffer: Arc<Mutex<VecDeque<String>>>,
  /// Command sender from GUI to bot (tokio::sync::mpsc) - optional so it can be dropped on shutdown
  pub cmd_tx: Arc<Mutex<Option<mpsc::Sender<GuiCommand>>>>,
  /// Shutdown signal sender (optional, consumed on shutdown)
  pub shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
  /// Latest snapshot of Manager state for GUI (updated periodically)
  pub latest_manager: Arc<RwLock<Option<Manager>>>,
  /// Latest user search results (updated by tokio thread)
  pub user_search_results: Arc<RwLock<Vec<Player>>>,
  /// Per-guild ELO data for the selected user (updated by tokio thread)
  pub user_guild_data: Arc<RwLock<Vec<(u64, crate::db::repo::GuildElo)>>>,
  /// Discord context (optional, set when bot is ready)
  pub ctx: Option<Arc<Context>>,
  /// Guild names mapped by ID
  pub guilds: HashMap<GuildId, String>,
  /// GUI settings (theme, font size, etc.)
  pub gui_settings: Arc<RwLock<GuiSettings>>,
  /// Cached guild config values for live editing (guild_id -> column -> value)
  pub guild_config_cache: Arc<RwLock<HashMap<u64, HashMap<String, String>>>>,
}

impl GuiSharedState {
  pub fn new(
    manager: Arc<Mutex<Manager>>,
    db: Arc<Database>,
    log_buffer: Arc<Mutex<VecDeque<String>>>,
    cmd_tx: mpsc::Sender<GuiCommand>,
    shutdown_tx: oneshot::Sender<()>,
  ) -> Self {
    Self {
      manager,
      db,
      log_buffer,
      cmd_tx: Arc::new(Mutex::new(Some(cmd_tx))),
      shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
      latest_manager: Arc::new(RwLock::new(None)),
      user_search_results: Arc::new(RwLock::new(Vec::new())),
      user_guild_data: Arc::new(RwLock::new(Vec::new())),
      ctx: None,
      guilds: HashMap::new(),
      gui_settings: Arc::new(RwLock::new(GuiSettings::load())),
      guild_config_cache: Arc::new(RwLock::new(HashMap::new())),
    }
  }

  /// Send a command to the bot thread. Silently ignores errors (channel closed).
  pub fn send_cmd(&self, cmd: GuiCommand) {
    if let Ok(tx_lock) = self.cmd_tx.try_lock() {
      if let Some(tx) = tx_lock.as_ref() {
        let _ = tx.try_send(cmd);
      }
    }
  }
}

/// Inner state wrapper for panels that need access to ctx and guilds
pub struct GuiState {
  pub ctx: Option<Arc<Context>>,
  pub db: Option<Arc<Database>>,
  pub guilds: HashMap<GuildId, String>,
}

impl GuiState {
  pub fn from_shared(shared: &GuiSharedState) -> Self {
    Self { ctx: shared.ctx.clone(), db: Some(shared.db.clone()), guilds: shared.guilds.clone() }
  }
}
