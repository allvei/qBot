use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
    pub cid_queue:            u64,
    pub cid_log:              u64,
    pub queue_quota:          u8,
    pub confirmation_timeout: u64,
    pub id_runner:            u64,
    pub id_admin:             u64,
    pub cid_buffer:           u64,
    pub cid_red:              u64,
    pub cid_blue:             u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cid_queue:     0,
            cid_log:       0,
            queue_quota:          8,
            confirmation_timeout: 120,
            id_runner:       0,
            id_admin:        0,
            cid_queue:        0,
            cid_buffer:       0,
            cid_red:          0,
            cid_blue:         0,
        }
    }
}
