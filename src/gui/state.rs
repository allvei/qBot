//! Shared state between GUI and tokio thread

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, oneshot, RwLock};

use crate::{Database, Manager};
use crate::gui::commands::GuiCommand;

/// Shared state accessible from both GUI (main thread) and tokio thread
pub struct GuiSharedState {
    /// Manager containing all bot state
    pub manager: Arc<Mutex<Manager>>,
    /// Database connection
    pub db: Arc<Database>,
    /// Ring buffer for GUI log viewer (max 1000 lines)
    pub log_buffer: Arc<Mutex<VecDeque<String>>>,
    /// Command sender from GUI to bot (tokio::sync::mpsc)
    pub cmd_tx: mpsc::Sender<GuiCommand>,
    /// Shutdown signal sender (optional, consumed on shutdown)
    pub shutdown_tx: Option<oneshot::Sender<()>>,
    /// Latest snapshot of Manager state for GUI (updated periodically)
    pub latest_manager: Arc<RwLock<Option<Manager>>>,
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
            cmd_tx,
            shutdown_tx: Some(shutdown_tx),
            latest_manager: Arc::new(RwLock::new(None)),
        }
    }
}
