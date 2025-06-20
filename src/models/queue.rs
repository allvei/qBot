use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct QueueSession {
    pub id: i64,
    pub user_id: i64,
    #[sqlx(rename = "channel_id")]
    pub queue_type: String, // Legacy name, maps to channel_id in database
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueueType {
    Newcomer,
    Journey,
    Default,
}

impl From<String> for QueueType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "newcomer" => QueueType::Newcomer,
            "journey" => QueueType::Journey,
            _ => QueueType::Default,
        }
    }
}

impl From<QueueType> for String {
    fn from(qt: QueueType) -> Self {
        match qt {
            QueueType::Newcomer => "newcomer".to_string(),
            QueueType::Journey => "journey".to_string(),
            QueueType::Default => "default".to_string(),
        }
    }
}
