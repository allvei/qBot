use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serenity::all::{GuildId, UserId};

/// Temporary storage for setup configuration state
#[derive(Debug, Clone)]
pub struct SetupConfig {
    pub guild_id: u64,
    pub dashboard_channel: Option<u64>,
    pub dashboard_msg_id: Option<u64>,
    pub queue_channel: Option<u64>,
    pub queue_vc_channel: Option<u64>,
    pub red_channel: Option<u64>,
    pub blue_channel: Option<u64>,
    pub runner_role: Option<u64>,
    pub admin_role: Option<u64>,
}

impl SetupConfig {
    pub fn new(guild_id: u64) -> Self {
        Self {
            guild_id,
            dashboard_channel: None,
            dashboard_msg_id: None,
            queue_channel: None,
            queue_vc_channel: None,
            red_channel: None,
            blue_channel: None,
            runner_role: None,
            admin_role: None,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.dashboard_channel.is_some() &&
        self.queue_channel.is_some() &&
        self.red_channel.is_some() &&
        self.blue_channel.is_some() &&
        self.runner_role.is_some() &&
        self.admin_role.is_some()
    }
}

/// Global setup state manager
pub struct SetupStateManager {
    // Key: (user_id, guild_id), Value: SetupConfig
    states: Arc<Mutex<HashMap<(u64, u64), SetupConfig>>>,
}

impl SetupStateManager {
    pub fn new() -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start_setup(&self, user_id: UserId, guild_id: GuildId) -> SetupConfig {
        let key = (user_id.get(), guild_id.get());
        let config = SetupConfig::new(guild_id.get());
        
        if let Ok(mut states) = self.states.lock() {
            states.insert(key, config.clone());
        }
        
        config
    }

    pub fn get_setup(&self, user_id: UserId, guild_id: GuildId) -> Option<SetupConfig> {
        let key = (user_id.get(), guild_id.get());
        
        if let Ok(states) = self.states.lock() {
            states.get(&key).cloned()
        } else {
            None
        }
    }

    pub fn update_setup<F>(&self, user_id: UserId, guild_id: GuildId, updater: F) -> Option<SetupConfig>
    where
        F: FnOnce(&mut SetupConfig),
    {
        let key = (user_id.get(), guild_id.get());
        
        if let Ok(mut states) = self.states.lock() {
            if let Some(config) = states.get_mut(&key) {
                updater(config);
                Some(config.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn complete_setup(&self, user_id: UserId, guild_id: GuildId) -> Option<SetupConfig> {
        let key = (user_id.get(), guild_id.get());
        
        if let Ok(mut states) = self.states.lock() {
            states.remove(&key)
        } else {
            None
        }
    }

    pub fn cleanup_expired(&self) {
        // In a production system, you'd want to clean up expired entries
        // For now, we'll just clear all entries periodically
        if let Ok(mut states) = self.states.lock() {
            // Only keep the most recent 100 setup states to prevent memory leaks
            if states.len() > 100 {
                states.clear();
            }
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
