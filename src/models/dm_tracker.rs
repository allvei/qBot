use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use serenity::all::{ChannelId, MessageId, UserId, Http};
use tokio::sync::RwLock;
use tracing::{info, warn};

const CLEANUP_INTERVAL_SECS: u64 = 60; // Check every minute
const INACTIVITY_TIMEOUT_SECS: u64 = 600; // 10 minutes

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
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Track a new DM message
    pub async fn track_message(&self, user_id: UserId, channel_id: ChannelId, message_id: MessageId, username: String) {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(&user_id) {
            session.message_ids.push(message_id);
            session.last_activity = SystemTime::now();
            session.username = username; // Update username in case it changed
        } else {
            sessions.insert(user_id, UserDmSession {
                channel_id,
                message_ids: vec![message_id],
                last_activity: SystemTime::now(),
                username,
            });
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
                    info!("Cleaning up {} DM messages for user {} (inactive for {} seconds)",
                          session.message_ids.len(), session.username, elapsed.as_secs());

                    // Delete all tracked messages
                    for message_id in &session.message_ids {
                        match http.delete_message(session.channel_id, *message_id, None).await {
                            Ok(_) => {
                                info!("Deleted DM message {} from user {}", message_id, session.username);
                            }
                            Err(e) => {
                                warn!("Failed to delete DM message {} from user {}: {}", message_id, session.username, e);
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
