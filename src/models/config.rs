use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Config {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub guild_id: String,
    pub queue_channel_id: String,
    pub log_channel_id: String,
    pub queue_size: u32,
    pub confirmation_timeout: u64,
    pub runner_role_id: String,
    pub admin_role_id: String,
    // Server A channels
    pub red_a_channel_id: String,
    pub blu_a_channel_id: String,
    pub server_a_channel_id: String,
    // Server B channels
    pub red_b_channel_id: String,
    pub blu_b_channel_id: String,
    pub server_b_channel_id: String,
    // Server C channels
    pub red_c_channel_id: String,
    pub blu_c_channel_id: String,
    pub server_c_channel_id: String,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            guild_id: String::new(),
            queue_channel_id: String::new(),
            log_channel_id: String::new(),
            queue_size: 8,
            confirmation_timeout: 120,
            runner_role_id: String::new(),
            admin_role_id: String::new(),
            red_a_channel_id: String::new(),
            blu_a_channel_id: String::new(),
            server_a_channel_id: String::new(),
            red_b_channel_id: String::new(),
            blu_b_channel_id: String::new(),
            server_b_channel_id: String::new(),
            red_c_channel_id: String::new(),
            blu_c_channel_id: String::new(),
            server_c_channel_id: String::new(),
        }
    }
}
