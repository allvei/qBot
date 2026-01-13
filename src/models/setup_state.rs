use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serenity::all::{GuildId as GI, UserId};

/// Temporary storage for setup configuration state
#[derive(Debug, Clone)]
pub struct SetupConfig {
    pub guild_id:          GI,
    pub dashboard_channel: Option<u64>,
    pub dashboard_msg_id:  Option<u64>,
    pub queue_channel:     Option<u64>,
    pub queue_vc_channel:  Option<u64>,
    pub red_channel:       Option<u64>,
    pub blue_channel:      Option<u64>,
    pub runner_role:       Option<u64>,
    pub admin_role:        Option<u64>,
}

#[derive(Debug, Clone)]
struct SetupEntry {
    config: SetupConfig,
    created_at: Instant,
}

impl SetupConfig {
    pub fn new(guild_id: GI) -> Self {
        Self {
            guild_id,
            dashboard_channel: None,
            dashboard_msg_id:  None,
            queue_channel:     None,
            queue_vc_channel:  None,
            red_channel:       None,
            blue_channel:      None,
            runner_role:       None,
            admin_role:        None,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.dashboard_channel.is_some() &&
        self.queue_channel    .is_some() &&
        self.queue_vc_channel .is_some() &&
        self.red_channel      .is_some() &&
        self.blue_channel     .is_some()
    }
}

/// Manages temporary setup configurations for users
#[derive(Debug)]
pub struct SetupStateManager {
    states: Mutex<HashMap<(UserId, GI), SetupEntry>>,
}

impl SetupStateManager {
    pub fn new() -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
        }
    }

    pub fn start_setup(&self, user_id: UserId, guild_id: GI) -> SetupConfig {
        let key    = (user_id, guild_id);
        let config = SetupConfig::new(guild_id);

        if let Ok(mut states) = self.states.lock() {
            let entry = SetupEntry {
                config: config.clone(),
                created_at: Instant::now(),
            };
            states.insert(key, entry);

            // Cleanup expired entries while we have the lock
            Self::cleanup_expired_internal(&mut states);
        }

        config
    }

    pub fn get_setup(&self, user_id: UserId, guild_id: GI) -> Option<SetupConfig> {
        if let Ok(states) = self.states.lock() {
            states.get(&(user_id, guild_id)).map(|entry| entry.config.clone())
        } else {
            None
        }
    }

    pub fn update_setup<F>(&self, user_id: UserId, guild_id: GI, updater: F) -> Option<SetupConfig>
    where
        F: FnOnce(&mut SetupConfig),
    {
        let key = (user_id, guild_id);

        if let Ok(mut states) = self.states.lock() {
            if let Some(entry) = states.get_mut(&key) {
                updater(&mut entry.config);
                Some(entry.config.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn complete_setup(&self, user_id: UserId, guild_id: GI) -> Option<SetupConfig> {
        let key = (user_id, guild_id);

        if let Ok(mut states) = self.states.lock() {
            states.remove(&key).map(|entry| entry.config)
        } else {
            None
        }
    }

    /// Internal cleanup that assumes lock is already held
    fn cleanup_expired_internal(states: &mut HashMap<(UserId, GI), SetupEntry>) {
        const EXPIRY_DURATION: Duration = Duration::from_secs(30 * 60); // 30 minutes
        let now = Instant::now();

        states.retain(|_, entry| {
            now.duration_since(entry.created_at) < EXPIRY_DURATION
        });
    }

    /// Public cleanup method that acquires lock
    pub fn cleanup_expired(&self) {
        if let Ok(mut states) = self.states.lock() {
            Self::cleanup_expired_internal(&mut states);
        }
    }
}

impl Default for SetupStateManager {
    fn default() -> Self {
        Self::new()
    }
}

// Global instance
lazy_static::lazy_static! {
    pub static ref SETUP_STATE: SetupStateManager = SetupStateManager::new();
}
