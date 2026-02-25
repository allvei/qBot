//! Graceful shutdown handling for the Discord bot
//! 
//! This module handles signal processing and cleanup operations when the bot
//! needs to shut down gracefully, including cleaning up voice channels and
//! marking dashboards as offline.

use serenity::all::{Cache, Http, CreateEmbed, EditMessage};
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tracing::{info, warn};

use crate::{
    log_prefix_category, models::{DashboardUpdateQueue, Manager}, util::{Style, now}
};

/// Handles graceful shutdown procedures
pub struct ShutdownHandler {
    manager: Arc<Mutex<Manager>>,
    dashboard_queue: Arc<Mutex<Option<DashboardUpdateQueue>>>,
    cache: Arc<Cache>,
    http: Arc<Http>,
}

impl ShutdownHandler {
    pub fn new(
        manager: Arc<Mutex<Manager>>,
        dashboard_queue: Arc<Mutex<Option<DashboardUpdateQueue>>>,
        cache: Arc<Cache>,
        http: Arc<Http>,
    ) -> Self {
        Self {
            manager,
            dashboard_queue,
            cache,
            http,
        }
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
                info!("Received Ctrl+C, shutting down gracefully...");
            }
            _ = sigterm => {
                info!("Received SIGTERM, shutting down gracefully...");
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
    
    /// Clean up empty team voice channels
    async fn cleanup_team_vcs(&self) {
        if let Ok(mut manager_lock) = self.manager.try_lock() {
            for server in &mut manager_lock.servers {
                let guild_id = server.guild_id;
                
                // Collect voice channel members for this guild
                let vc_members: std::collections::HashSet<u64> =
                    self.cache.guild(guild_id)
                        .map(|g| g.voice_states.values()
                            .filter_map(|vs| vs.channel_id.map(|c| c.get()))
                            .collect())
                        .unwrap_or_default();
                
                for category in &mut server.categories {
                    let mut kept = Vec::new();
                    for tc in &category.channels.teams {
                        let red_empty = !vc_members.contains(&tc.red_vc.get());
                        let blu_empty = !vc_members.contains(&tc.blu_vc.get());
                        
                        if red_empty && blu_empty {
                            let _ = tc.red_vc.delete(&self.http).await;
                            let _ = tc.blu_vc.delete(&self.http).await;
                        } else {
                            kept.push(tc.clone());
                        }
                    }
                    category.channels.teams = kept;
                }
            }
        } else {
            warn!("Could not acquire manager lock for team VC cleanup");
        }
    }
    
    /// Mark all dashboards as offline before shutting down
    async fn mark_dashboards_offline(&self) {
        if let Ok(mut manager_lock) = self.manager.try_lock() {
            info!("Marking all dashboards as offline...");
            
            for server in &mut manager_lock.servers {
                let gld_id = server.guild_id;
                let gld_nm = self.cache.guild(gld_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                
                for category in &mut server.categories {
                    let offline_embed = CreateEmbed::new()
                        .title("🔴 qBot is offline...")
                        .color(0xFF0000) // Red color
                        .footer(serenity::all::CreateEmbedFooter::new(format!(
                            "Shutdown at {}", now(Style::Relative)
                        )));
                    
                    let chn_id = category.channels.dashboard;
                    let msg_id = category.dashboard_msg;
                    let ctg_nm = category.name.as_ref().unwrap();
                    
                    match chn_id.edit_message(
                        &self.http, 
                        msg_id, 
                        EditMessage::new().embed(offline_embed).components(vec![])
                    ).await {
                        Ok(_) => {
                            info!("{} Dashboard now offline", log_prefix_category(gld_nm.as_str(), ctg_nm.as_str()));
                        }
                        Err(e) => {
                            warn!("{} Failed to update dashboard: {}", log_prefix_category(gld_nm.as_str(), ctg_nm.as_str()), e);
                        }
                    }
                }
            }
        } else {
            warn!("Could not acquire manager lock for graceful shutdown");
        }
    }
}
