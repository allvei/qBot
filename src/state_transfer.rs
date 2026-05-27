//! State transfer abstraction for graceful restarts and hot reload
//!
//! This module provides an abstraction layer for transferring bot state between instances.
//! Currently uses database persistence, but designed to easily support IPC for hot reload.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

use crate::Manager;

const STATE_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_STATE_AGE_SECS: u64 = 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotState {
  pub version: String,
  pub saved_at: SystemTime,
  pub manager: Manager,
}

impl BotState {
  pub fn new(manager: Manager) -> Self {
    Self { version: STATE_VERSION.to_string(), saved_at: SystemTime::now(), manager }
  }

  pub fn age(&self) -> Duration {
    self.saved_at.elapsed().unwrap_or(Duration::from_secs(0))
  }

  pub fn is_recent(&self) -> bool {
    self.age().as_secs() < MAX_STATE_AGE_SECS
  }

  pub fn is_compatible_version(&self) -> bool {
    self.version == STATE_VERSION
  }
}

#[async_trait::async_trait]
pub trait StateTransfer: Send + Sync {
  async fn save(&self, state: &BotState) -> Result<()>;
  async fn load(&self) -> Result<Option<BotState>>;
  async fn clear(&self) -> Result<()>;
  async fn health_check(&self) -> Result<()>;
}

#[derive(Clone, Copy, Debug)]
pub enum StateTransferMethod {
  Database,
  #[allow(dead_code)]
  Ipc,
}

impl StateTransferMethod {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Database => "database",
      Self::Ipc => "ipc",
    }
  }
}
