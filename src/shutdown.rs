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
  util::{timestamp_now, Style},
  Database,
};

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

    // Wait for either SIGINT (Ctrl+C) or SIGTERM
    let sigint = async {
      signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };

    let sigterm = async {
      #[cfg(unix)]
      {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");
        sigterm.recv().await;
      }
      #[cfg(not(unix))]
      {
        // On non-Unix systems, we'll never receive SIGTERM
        std::future::pending::<()>().await;
      }
    };

    // Wait for either signal
    tokio::select! {
        _ = sigint => {
            info!("Shutting down...");
        }
        _ = sigterm => {
            info!("Terminating...");
        }
    }

    // Perform cleanup
    self.cleanup().await;

    // Send shutdown signal to main task
    let _ = shutdown_tx.send(());
  }

  /// Perform cleanup operations before shutdown
  async fn cleanup(&self) {
    // Stop the dashboard update queue
    {
      let mut queue_lock = self.dashboard_queue.lock().await;
      let _ = queue_lock.take(); // Drop the queue, closing the channel
    }

    // Wait for any in-flight batch to finish
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Clean up empty team VCs
    self.cleanup_team_vcs().await;

    // Mark dashboards as offline
    self.mark_dashboards_offline().await;
  }

  /// Clean up empty team voice channels.
  /// Respects `keep_minimum` - preserves at least one empty pair per category when the setting is enabled.
  /// Skips deletion of channels that currently have players in them.
  async fn cleanup_team_vcs(&self) {
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
            empty_pairs.sort_by(|a, b| a.set_index.cmp(&b.set_index));
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
  async fn mark_dashboards_offline(&self) {
    if let Ok(manager_lock) = self.manager.try_lock() {
      info!("Marking all dashboards as offline...");

      // Collect all dashboard update tasks for parallel execution
      let mut tasks = tokio::task::JoinSet::new();

      for server in &manager_lock.qguilds {
        let guild_id = server.id;
        let guild_name = self.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

        for category in &server.categories {
          let offline_embed = CreateEmbed::new().title("🔴 qBot is offline...").description(format!("Shutdown {}", timestamp_now(Style::Relative))).color(0xFF0000);

          let chn_id = category.channels.dashboard;
          let msg_id = category.dashboard_msg;
          let ctg_nm = category.name.clone().unwrap_or_default();
          let guild_name_clone = guild_name.clone();
          let http = self.http.clone();

          tasks.spawn(async move {
            match chn_id.edit_message(&http, msg_id, EditMessage::new().embed(offline_embed).components(vec![])).await {
              Ok(_) => {
                info!("{} Dashboard now offline", log_prefix_category(&guild_name_clone, &ctg_nm));
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
