use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use crate::models::Channels;



/// Configuration key-value pair struct.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ConfigFormat {
    pub key:         String,
    pub value:       String,
    pub description: Option<String>,
}

/// Bot configuration struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub queue_channel_id:     u64,
    pub log_channel_id:       u64,
    pub queue_quota:          u8,
    pub confirmation_timeout: u64,
    pub runner_role_id:       u64,
    pub admin_role_id:        u64,
    pub queue_channel:        u64,
    pub buffer_channel:       u64,
    pub red_channel:          u64,
    pub blue_channel:         u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            queue_channel_id:     0,
            log_channel_id:       0,
            queue_quota:          8,
            confirmation_timeout: 120,
            runner_role_id:       0,
            admin_role_id:        0,
            queue_channel:        0,
            buffer_channel:       0,
            red_channel:          0,
            blue_channel:         0,
        }
    }
}
