use serenity::all::{Context, GuildId as GI, UserId as UI};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::info;

use crate::models::Manager;
use crate::Database;

/// Key for identifying a player's timeout task
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct TimeoutKey {
  pub guild_id: GI,
  pub user_id: UI,
}

/// Manages per-player timeout tasks for accurate queue expiry
pub struct TimeoutScheduler {
  /// Active timeout tasks, keyed by (guild_id, user_id)
  tasks: HashMap<TimeoutKey, JoinHandle<()>>,
  /// Reference to manager for removing players
  manager: Arc<Mutex<Manager>>,
  /// Reference to database
  db: Arc<Database>,
  /// Serenity context for API calls
  ctx: Context,
}

impl TimeoutScheduler {
  pub fn new(manager: Arc<Mutex<Manager>>, db: Arc<Database>, ctx: Context) -> Self {
    Self {
      tasks: HashMap::new(),
      manager,
      db,
      ctx,
    }
  }

  /// Schedule a timeout for a player. Cancels any existing timeout for this player.
  pub fn schedule_timeout(
    &mut self,
    guild_id: GI,
    user_id: UI,
    timeout_minutes: u8,
    player_tag: String,
    category_name: String,
    format_name: String,
  ) {
    let key = TimeoutKey { guild_id, user_id };
    
    // Cancel existing timeout if any
    if let Some(handle) = self.tasks.remove(&key) {
      handle.abort();
    }

    // Don't schedule if timeout is 0 or below minimum
    if timeout_minutes < crate::models::constants::MIN_TIMEOUT {
      return;
    }

    let duration = Duration::from_secs(timeout_minutes as u64 * 60);
    let manager = self.manager.clone();
    let db = self.db.clone();
    let ctx = self.ctx.clone();

    let handle = tokio::spawn(async move {
      // Wait for the exact timeout duration
      tokio::time::sleep(duration).await;

      // Remove the player from the queue
      let mut mgr = manager.lock().await;
      
      if let Ok(server) = mgr.get_server(guild_id) {
        let mut removed = false;
        let mut should_update_dashboard = false;
        
        for category in &mut server.categories {
          for format in &mut category.formats {
            for session in &mut format.sessions {
              if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == user_id) {
                session.pool.remove(pos);
                removed = true;
                should_update_dashboard = true;
                
                let gld_nm = crate::models::constants::guild_name(&ctx, guild_id);
                info!(
                  "{} Timeout {} after {}m",
                  crate::log::log_prefix_format(&gld_nm, &category_name, &format_name),
                  player_tag,
                  timeout_minutes
                );
                
                // Check if hot session dropped below quota
                if session.is_hot() && session.pool.len() < format.quota as usize {
                  session.idle();
                  info!(
                    "{} Hot session dropped below quota after timeout, transitioning back to Idle",
                    crate::log::log_prefix_format(&gld_nm, &category_name, &format_name)
                  );
                }
                break;
              }
            }
          }
          
          if should_update_dashboard {
            category.queue_dash_update(&ctx, guild_id).await;
          }
        }
        
        if !removed {
          // Player was already removed (left queue, game started, etc.)
          // This is normal - the task just didn't get cancelled in time
        }
      }
    });

    self.tasks.insert(key, handle);
  }

  /// Cancel a player's timeout (e.g., when they leave the queue or game starts)
  pub fn cancel_timeout(&mut self, guild_id: GI, user_id: UI) {
    let key = TimeoutKey { guild_id, user_id };
    if let Some(handle) = self.tasks.remove(&key) {
      handle.abort();
    }
  }

  /// Cancel all timeouts for a guild (e.g., when a game starts)
  pub fn cancel_all_for_guild(&mut self, guild_id: GI) {
    let keys_to_remove: Vec<_> = self.tasks.keys()
      .filter(|k| k.guild_id == guild_id)
      .cloned()
      .collect();
    
    for key in keys_to_remove {
      if let Some(handle) = self.tasks.remove(&key) {
        handle.abort();
      }
    }
  }

  /// Reschedule a timeout with a new duration (e.g., when player changes their timeout setting)
  pub fn reschedule_timeout(
    &mut self,
    guild_id: GI,
    user_id: UI,
    new_timeout_minutes: u8,
    player_tag: String,
    category_name: String,
    format_name: String,
  ) {
    self.schedule_timeout(guild_id, user_id, new_timeout_minutes, player_tag, category_name, format_name);
  }
}

/// TypeMapKey for TimeoutScheduler
pub struct TimeoutSchedulerKey;
impl serenity::prelude::TypeMapKey for TimeoutSchedulerKey {
  type Value = Arc<Mutex<TimeoutScheduler>>;
}
