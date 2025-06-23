use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use crate::models::Channels;



/// Configuration key-value pair struct.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Config {
    pub key:         String,
    pub value:       String,
    pub description: Option<String>,
}

/// Bot configuration struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub guild_id:             u64,
    pub queue_channel_id:     u64,
    pub log_channel_id:       u64,
    pub queue_quota:          u8,
    pub confirmation_timeout: u64,
    pub runner_role_id:       u64,
    pub admin_role_id:        u64,
    pub apug:                 Channels,
    pub bpug:                 Channels,
    pub cpug:                 Channels,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            guild_id:             0,
            queue_channel_id:     0,
            log_channel_id:       0,
            queue_quota:          8,
            confirmation_timeout: 120,
            runner_role_id:       0,
            admin_role_id:        0,
            apug:                 Channels::new(0, 0, 0, 0),
            bpug:                 Channels::new(0, 0, 0, 0),
            cpug:                 Channels::new(0, 0, 0, 0),
        }
    }
}
