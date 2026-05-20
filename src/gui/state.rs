//! Shared state between GUI and tokio thread

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};

use crate::gui::commands::GuiCommand;
use crate::models::Player;
use crate::{Database, Manager};

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
