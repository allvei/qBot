use serenity::all::{ChannelId, Context, CreateEmbed, CreateMessage, GetMessages, Http, MessageId, UserId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::{CLEANUP_INTERVAL_SECS, INACTIVITY_TIMEOUT_SECS};

/// Tracks DM messages for automatic cleanup
#[derive(Debug)]
struct UserDmSession {
  channel_id: ChannelId,
  message_ids: Vec<MessageId>,
  last_activity: SystemTime,
  username: String,
}

/// Manages DM message tracking and cleanup
pub struct DmMessageTracker {
  sessions: Arc<RwLock<HashMap<UserId, UserDmSession>>>,
}

impl DmMessageTracker {
  pub fn new() -> Self {
    Self { sessions: Arc::new(RwLock::new(HashMap::new())) }
  }

  /// Track a new DM message
  pub async fn track_message(&self, user_id: UserId, channel_id: ChannelId, message_id: MessageId, username: String) {
    let mut sessions = self.sessions.write().await;

    if let Some(session) = sessions.get_mut(&user_id) {
      session.message_ids.push(message_id);
      session.last_activity = SystemTime::now();
      session.username = username; // Update username in case it changed
    } else {
      sessions.insert(user_id, UserDmSession { channel_id, message_ids: vec![message_id], last_activity: SystemTime::now(), username });
    }
  }

  /// Update activity timestamp for a user (e.g., when they interact with buttons)
  pub async fn update_activity(&self, user_id: UserId) {
    let mut sessions = self.sessions.write().await;

    if let Some(session) = sessions.get_mut(&user_id) {
      session.last_activity = SystemTime::now();
    }
  }

  /// Remove a specific message from tracking
  pub async fn remove_message(&self, user_id: UserId, message_id: MessageId) {
    let mut sessions = self.sessions.write().await;

    if let Some(session) = sessions.get_mut(&user_id) {
      session.message_ids.retain(|&id| id != message_id);

      // If no more messages, remove the session
      if session.message_ids.is_empty() {
        sessions.remove(&user_id);
      }
    }
  }

  /// Send a DM to a user, deleting any previously tracked DMs first.
  /// Also cleans up old untracked bot messages in the DM channel.
  /// Returns Ok(()) on success, Err if the DM could not be sent.
  pub async fn send_dm(&self, ctx: &Context, user_id: UserId, embed: CreateEmbed) -> Result<(), serenity::Error> {
    let user = ctx.http.get_user(user_id).await?;
    let dm_channel = user.create_dm_channel(&ctx.http).await?;
    let username = user.name.clone();

    self.delete_tracked(user_id, &ctx.http).await;
    self.cleanup_old_bot_dms(ctx, dm_channel.id).await;
    let sent = dm_channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await?;
    self.track_message(user_id, dm_channel.id, sent.id, username.clone()).await;

    info!("[DM/{}] Sent and tracked DM", username);
    Ok(())
  }

  /// Delete all tracked messages for a user without removing the session
  async fn delete_tracked(&self, user_id: UserId, http: &Http) {
    let mut sessions = self.sessions.write().await;
    if let Some(session) = sessions.get_mut(&user_id) {
      for message_id in session.message_ids.drain(..) {
        if let Err(e) = http.delete_message(session.channel_id, message_id, None).await {
          warn!("[DM/{}] Failed to delete tracked message {}: {e}", session.username, message_id);
        }
      }
    }
  }

  /// Clean up old bot messages in a DM channel that aren't tracked
  /// (handles messages sent before tracking was added)
  async fn cleanup_old_bot_dms(&self, ctx: &Context, channel_id: ChannelId) {
    let bot_id = ctx.cache.current_user().id;

    let messages = match channel_id.messages(&ctx.http, GetMessages::new().limit(25)).await {
      Ok(msgs) => msgs,
      Err(_) => return,
    };

    for msg in messages {
      if msg.author.id == bot_id {
        // Skip messages we're currently tracking (they were already handled)
        let is_tracked = {
          let sessions = self.sessions.read().await;
          sessions.values().any(|s| s.message_ids.contains(&msg.id))
        };
        if !is_tracked {
          if let Err(e) = msg.delete(&ctx.http).await {
            warn!("[DM] Failed to delete old bot message {}: {e}", msg.id);
          }
        }
      }
    }
  }

  /// Start the cleanup background task
  pub fn start_cleanup_task(self: Arc<Self>, http: Arc<Http>) {
    tokio::spawn(async move {
      let mut interval = tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));

      loop {
        interval.tick().await;
        self.cleanup_inactive_sessions(&http).await;
      }
    });
  }

  /// Clean up messages from inactive sessions
  async fn cleanup_inactive_sessions(&self, http: &Http) {
    let now = SystemTime::now();
    let mut sessions = self.sessions.write().await;
    let mut users_to_remove = Vec::new();

    for (user_id, session) in sessions.iter() {
      if let Ok(elapsed) = now.duration_since(session.last_activity) {
        if elapsed.as_secs() >= INACTIVITY_TIMEOUT_SECS {
          // Delete all tracked messages
          for message_id in &session.message_ids {
            match http.delete_message(session.channel_id, *message_id, None).await {
              Ok(_) => (),
              Err(e) => {
                warn!("[DM/{}] Failed to delete message ID {}: {e}", session.username, message_id);
              }
            }
          }
          users_to_remove.push(*user_id);
        }
      }
    }

    // Remove cleaned up sessions
    for user_id in users_to_remove {
      sessions.remove(&user_id);
    }
  }
}

impl Default for DmMessageTracker {
  fn default() -> Self {
    Self::new()
  }
}
