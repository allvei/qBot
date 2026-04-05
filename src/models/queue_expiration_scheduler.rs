use serenity::all::{Context, GuildId as GI, UserId as UI};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::info;

use crate::models::Manager;
use crate::{Database, Player};

/// Key for identifying a player's timeout task
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct QueueExpirationKey {
  pub guild_id: GI,
  pub category_id: u8,
  pub format_id: u8,
  pub user_id: UI,
}

impl QueueExpirationKey {
  pub fn new(guild_id: GI, category_id: u8, format_id: u8, user_id: UI) -> Self {
    Self {
      guild_id,
      category_id,
      format_id,
      user_id,
    }
  }
}

/// Manages per-player timeout tasks for accurate queue expiry
pub struct QueueExpirationScheduler {
  tasks: HashMap<QueueExpirationKey, JoinHandle<()>>,
  manager: Arc<Mutex<Manager>>,
  db: Arc<Database>,
  ctx: Context,
}

impl QueueExpirationScheduler {
  pub fn new(manager: Arc<Mutex<Manager>>, db: Arc<Database>, ctx: Context) -> Self {
    Self {
      tasks: HashMap::new(),
      manager,
      db,
      ctx,
    }
  }

  /// Schedule a timeout for a player. Cancels any existing timeout for this player.
  pub fn schedule_queue_expiration(
    &mut self,
    guild_id: GI,
    category_id: u8,
    format_id: u8,
    player: Player,
    queue_expiration_minutes: u8,
  ) {
    let key = QueueExpirationKey::new(guild_id, category_id, format_id, player.user_id);
    
    // Cancel existing timeout if any
    if let Some(handle) = self.tasks.remove(&key) {
      handle.abort();
    }

    // Don't schedule if timeout is 0 or below minimum
    if queue_expiration_minutes < crate::models::constants::MIN_QUEUE_EXPIRATION {
      return;
    }

    let duration = Duration::from_secs(queue_expiration_minutes as u64 * 60);
    let manager = self.manager.clone();
    let db = self.db.clone();
    let ctx = self.ctx.clone();
    
    // Capture player data for the async block
    let player_user_id = player.user_id;
    let player_tag = player.tag.clone();

    let handle = tokio::spawn(async move {
      // Wait for the exact timeout duration
      tokio::time::sleep(duration).await;

      // Remove the player from the queue
      let mut mgr = manager.lock().await;
      
      if let Ok(server) = mgr.get_qguild(guild_id) {
        let mut removed = false;
        let mut should_update_dashboard = false;
        
        for category in &mut server.categories {
          let category_clone = category.clone();
          for format in &mut category.formats {
            let format_clone = format.clone();
            for session in &mut format.sessions {
              if let Some(pos) = session.pool.iter().position(|p| p.player.user_id == player_user_id) {
                session.pool.remove(pos);
                removed = true;
                should_update_dashboard = true;
                
                let guild_name = crate::models::constants::guild_name(&ctx, guild_id);
                info!(
                  "{} Timeout {} after {}m",
                  crate::log::log_prefix_format(&guild_name, category_clone.name().as_str(), format_clone.name()),
                  player_tag,
                  queue_expiration_minutes
                );
                
                // Check if hot session dropped below quota
                if session.is_hot() && session.pool.len() < format.quota as usize {
                  session.idle();
                  info!(
                    "{} Hot session dropped below quota after timeout, transitioning back to Idle",
                    crate::log::log_prefix_format(&guild_name, &category_clone.name(), format.name())
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
  pub fn cancel_queue_expiration(&mut self, guild_id: GI, category_id: u8, format_id: u8, user_id: UI) {
    let key = QueueExpirationKey { guild_id, category_id, format_id, user_id };
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
  pub fn reschedule_queue_expiration(
    &mut self,
    guild_id: GI,
    category_id: u8,
    format_id: u8,
    player: Player,
    new_expiration_duration_minutes: u8,
  ) {
    self.schedule_queue_expiration(guild_id, category_id, format_id, player, new_expiration_duration_minutes);
  }
}

/// TypeMapKey for TimeoutScheduler
pub struct QueueExpirationSchedulerKey;
impl serenity::prelude::TypeMapKey for QueueExpirationSchedulerKey {
  type Value = Arc<Mutex<QueueExpirationScheduler>>;
}
