use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct QueueSession {
    pub id: i64,
    pub user_id: i64,
    pub queue_type: String,
    pub joined_at: DateTime<Utc>,
    pub status: QueueStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum QueueStatus {
    #[sqlx(rename = "waiting")]
    Waiting,
    #[sqlx(rename = "in_match")]
    InMatch,
    #[sqlx(rename = "benched")]
    Benched,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueueType {
    Nowbie,
    Journey,
    Default,
}

impl From<String> for QueueType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "nowbie" => QueueType::Nowbie,
            "journey" => QueueType::Journey,
            _ => QueueType::Default,
        }
    }
}

impl From<QueueType> for String {
    fn from(qt: QueueType) -> Self {
        match qt {
            QueueType::Nowbie => "nowbie".to_string(),
            QueueType::Journey => "journey".to_string(),
            QueueType::Default => "default".to_string(),
        }
    }
}
