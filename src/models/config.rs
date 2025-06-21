use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use SessionChannel

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Config {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub guild_id: u64,
    pub queue_channel_id: u64,
    pub log_channel_id: u64,
    pub queue_quota: u8,
    pub confirmation_timeout: u64,
    pub runner_role_id: u64,
    pub admin_role_id: u64,
    pub a: TeamChannel,
    pub b: TeamChannel,
    pub c: TeamChannel,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            guild_id: String::new(),
            queue_channel_id: String::new(),
            log_channel_id: String::new(),
            queue_quota: 8,
            confirmation_timeout: 120,
            runner_role_id,
            admin_role_id,
            a: TeamChannel,
            b: TeamChannel,
            c: TeamChannel,
        }
    }
}
