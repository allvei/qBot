//! Graceful shutdown handling for the Discord bot
//!
//! This module handles signal processing and cleanup operations when the bot
//! needs to shut down gracefully, including cleaning up voice channels and
//! marking dashboards as offline.

use serenity::all::{Cache, CreateEmbed, EditMessage, Http};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

use crate::{
  log_prefix_category,
  models::{DashboardUpdateQueue, Manager},
  util::{timestamp_from_unix, timestamp_now, Style},
  Database,
};

/// Exit code used when a restart was requested; the systemd unit treats it as a restart trigger.
pub const RESTART_EXIT_CODE: i32 = 75;

/// Set when a restart (not a stop) was requested, so the process exits with `RESTART_EXIT_CODE`.
pub static RESTART_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Optional reason and expected return time shown on dashboards during a stop or restart.
#[derive(Debug, Default, Clone)]
pub struct ShutdownNotice {
  pub reason: Option<String>,
  /// Unix timestamp (seconds) of when the bot is expected back online.
  pub eta: Option<i64>,
}

impl ShutdownNotice {
  /// Dashboard description lines for the reason and ETA, empty when neither is set.
  fn describe(&self) -> String {
    let mut lines = String::new();
    if let Some(reason) = &self.reason {
      lines.push_str(&format!("\nReason: {reason}"));
    }
    if let Some(eta) = self.eta {
      lines.push_str(&format!("\nExpected back {}", timestamp_from_unix(eta, Style::Relative)));
    }
    lines
  }
}

/// Handles graceful shutdown procedures
pub struct ShutdownHandler {
  manager: Arc<Mutex<Manager>>,
  dashboard_queue: Arc<Mutex<Option<DashboardUpdateQueue>>>,
  cache: Arc<Cache>,
  http: Arc<Http>,
  db: Arc<Database>,
}

impl ShutdownHandler {
  pub fn new(manager: Arc<Mutex<Manager>>, dashboard_queue: Arc<Mutex<Option<DashboardUpdateQueue>>>, cache: Arc<Cache>, http: Arc<Http>, db: Arc<Database>) -> Self {
    Self { manager, dashboard_queue, cache, http, db }
  }

  /// Handle shutdown signals and perform cleanup
  pub async fn handle_signals(&self, shutdown_tx: oneshot::Sender<()>) {
    use tokio::signal;

    // Ctrl+C is ignored on purpose to avoid accidental stops; use `qbot ctl stop` instead
    tokio::spawn(async {
      while signal::ctrl_c().await.is_ok() {
        warn!("Ctrl+C is ignored. Use `qbot ctl stop` to stop the bot.");
      }
    });

    // SIGTERM (e.g. `systemctl stop`) performs a graceful stop
    #[cfg(unix)]
    {
      use tokio::signal::unix::{signal, SignalKind};
      let mut sigterm = signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");
      sigterm.recv().await;
    }
    #[cfg(not(unix))]
    std::future::pending::<()>().await;

    info!("Terminating...");
    self.cleanup(&ShutdownNotice::default()).await;

    // Send shutdown signal to main task
    let _ = shutdown_tx.send(());
  }

  /// Stop the bot: save state, remove empty team VCs and mark dashboards offline
  pub async fn cleanup(&self, notice: &ShutdownNotice) {
    use tokio::time::{sleep, Duration};

    info!("Starting graceful stop sequence...");

    self.save_manager_state().await;
    self.take_dashboard_queue().await;
    sleep(Duration::from_millis(500)).await;

    self.cleanup_team_vcs().await;
    self.mark_dashboards_offline(notice).await;

    info!("Graceful stop complete");
  }

  /// Restart the bot: save state and keep team VCs so they can be re-adopted on boot
  pub async fn restart(&self, notice: &ShutdownNotice) {
    use tokio::time::{sleep, Duration};

    info!("Starting graceful restart sequence...");

    self.save_manager_state().await;
    self.take_dashboard_queue().await;
    sleep(Duration::from_millis(500)).await;

    self.mark_dashboards_restarting(notice).await;
    RESTART_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

    info!("Graceful restart complete");
  }

  /// Stop batching dashboard updates so no further Discord edits race with shutdown
  async fn take_dashboard_queue(&self) {
    let mut queue_lock = self.dashboard_queue.lock().await;
    let _ = queue_lock.take();
  }

  /// Mark all dashboards with a restarting indicator
  pub async fn mark_dashboards_restarting(&self, notice: &ShutdownNotice) {
    if let Ok(mut manager_lock) = self.manager.try_lock() {
      info!("Marking dashboards as restarting...");

      let mut tasks = tokio::task::JoinSet::new();
      let description = format!("Queues and live games will be restored.{}", notice.describe());

      for server in &mut manager_lock.qguilds {
        let guild_id = server.id;
        let guild_name = self.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

        for category in &mut server.categories {
          category.restarting = true;

          let restart_embed = CreateEmbed::new().title("🟠 qBot is restarting...").description(description.clone()).color(0xFFA500);

          let chn_id = category.channels.dashboard;
          let msg_id = category.dashboard_msg;
          let ctg_nm = category.name.clone().unwrap_or_default();
          let guild_name_clone = guild_name.clone();
          let http = self.http.clone();

          tasks.spawn(async move {
            match chn_id.edit_message(&http, msg_id, EditMessage::new().embed(restart_embed)).await {
              Ok(_) => {
                info!("{} Dashboard restarting", log_prefix_category(&guild_name_clone, &ctg_nm));
              }
              Err(e) => {
                warn!("{} Failed to mark dashboard as restarting: {}", log_prefix_category(&guild_name_clone, &ctg_nm), e);
              }
            }
          });
        }
      }

      drop(manager_lock);
      while tasks.join_next().await.is_some() {}
    } else {
      warn!("Could not acquire manager lock to mark dashboards as restarting");
    }
  }

  /// Wait for active sessions to complete with timeout
  async fn wait_for_active_sessions(&self, timeout: std::time::Duration) {
    use std::time::Instant;
    use tokio::time::{sleep, Duration};

    let start = Instant::now();

    loop {
      let (has_active, count) = {
        let manager = self.manager.lock().await;
        let count = manager.count_active_sessions();
        (count > 0, count)
      };

      if !has_active {
        info!("All sessions completed");
        break;
      }

      if start.elapsed() > timeout {
        warn!("Timeout waiting for sessions to complete ({} still active)", count);
        break;
      }

      info!("Waiting for {} active session(s) to complete... ({}s elapsed)", count, start.elapsed().as_secs());
      sleep(Duration::from_secs(10)).await;
    }
  }

  /// Save current manager state to database
  pub async fn save_manager_state(&self) {
    let manager = self.manager.lock().await;
    match self.db.state.save_manager(&manager).await {
      Ok(_) => {
        let session_count = manager.count_active_sessions();
        info!("Saved manager state ({} guilds, {} active sessions)", manager.qguilds.len(), session_count);
      }
      Err(e) => {
        warn!("Failed to save manager state: {}", e);
      }
    }
  }

  /// Clean up empty team voice channels.
  /// Respects `keep_minimum` - preserves at least one empty pair per category when the setting is enabled.
  /// Skips deletion of channels that currently have players in them.
  pub async fn cleanup_team_vcs(&self) {
    if let Ok(mut manager_lock) = self.manager.try_lock() {
      let mut deleted_count = 0;

      for server in &mut manager_lock.qguilds {
        let guild_id = server.id;
        let guild_name = self.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

        // Collect voice channel members for this guild
        let vc_members: std::collections::HashSet<u64> =
          self.cache.guild(guild_id).map(|g| g.voice_states.values().filter_map(|vs| vs.channel_id.map(|c| c.get())).collect()).unwrap_or_default();

        for category in &mut server.categories {
          let category_name = category.name();
          let keep_minimum = category.team_vc_settings.keep_minimum;

          // Partition into occupied and empty
          let mut occupied_pairs = Vec::new();
          let mut empty_pairs = Vec::new();

          for tc in &category.channels.teams {
            let red_has_players = vc_members.contains(&tc.red_vc.get());
            let blu_has_players = vc_members.contains(&tc.blu_vc.get());

            if red_has_players || blu_has_players {
              occupied_pairs.push(tc.clone());
            } else {
              empty_pairs.push(tc.clone());
            }
          }

          // If keep_minimum is enabled, preserve one empty pair (lowest set_index)
          if keep_minimum && !empty_pairs.is_empty() {
            empty_pairs.sort_by_key(|a| a.set_index);
            occupied_pairs.push(empty_pairs.remove(0));
          }

          // Delete remaining empty pairs
          for tc in &empty_pairs {
            let _ = tc.red_vc.delete(&self.http).await;
            let _ = tc.blu_vc.delete(&self.http).await;
            let _ = self.db.teams.remove_team(guild_id, tc.red_vc, tc.blu_vc, &guild_name, &category_name).await;
            deleted_count += 1;
          }

          category.channels.teams = occupied_pairs;
        }
      }

      if deleted_count > 0 {
        info!("Cleaned up {} empty team VC pairs on shutdown", deleted_count);
      }
    } else {
      warn!("Could not acquire manager lock for team VC cleanup");
    }
  }

  /// Mark all dashboards as offline before shutting down
  async fn mark_dashboards_offline(&self, notice: &ShutdownNotice) {
    if let Ok(manager_lock) = self.manager.try_lock() {
      info!("Marking all dashboards as offline...");

      // Collect all dashboard update tasks for parallel execution
      let mut tasks = tokio::task::JoinSet::new();
      let description = format!("Shutdown {}{}", timestamp_now(Style::Relative), notice.describe());

      for server in &manager_lock.qguilds {
        let guild_id = server.id;
        let guild_name = self.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

        for category in &server.categories {
          let offline_embed = CreateEmbed::new().title("🔴 qBot is offline...").description(description.clone()).color(0xFF0000);

          let chn_id = category.channels.dashboard;
          let msg_id = category.dashboard_msg;
          let ctg_nm = category.name.clone().unwrap_or_default();
          let guild_name_clone = guild_name.clone();
          let http = self.http.clone();

          tasks.spawn(async move {
            match chn_id.edit_message(&http, msg_id, EditMessage::new().embed(offline_embed).components(vec![])).await {
              Ok(_) => {
                info!("{} Dashboard offline", log_prefix_category(&guild_name_clone, &ctg_nm));
              }
              Err(e) => {
                warn!("{} Failed to update dashboard: {}", log_prefix_category(&guild_name_clone, &ctg_nm), e);
              }
            }
          });
        }
      }

      // Run all updates in parallel
      drop(manager_lock);
      while tasks.join_next().await.is_some() {}
    } else {
      warn!("Could not acquire manager lock for graceful shutdown");
    }
  }
}
