use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(FromRow)]
pub struct Session {
    pub id: i64,
    pub session_uuid: String,
    pub status: String,
    pub created_at: String,
    #[sqlx(rename = "accepted_at")]
    pub confirmed_at: Option<String>, // Maps to accepted_at in the database
    pub ended_at: Option<String>,
    pub server_channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Idle,
    Hot,
    Push,
    Live,
    Pull,
}

impl SessionStatus {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Idle => "idle",
            SessionStatus::Hot => "hot",
            SessionStatus::Push => "push",
            SessionStatus::Live => "live",
            SessionStatus::Pull => "pull",
        }
    }
    
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(SessionStatus::Idle),
            "hot" => Some(SessionStatus::Hot),
            "push" => Some(SessionStatus::Push),
            "live" => Some(SessionStatus::Live),
            "pull" => Some(SessionStatus::Pull),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub id: i64,
    pub session_id: i64,
    pub user_id: i64,
    pub team: String,
    pub is_benched: bool,
    pub benched_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Team {
    Red,
    Blu,
}

impl Team {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Team::Red => "RED",
            Team::Blu => "BLU",
        }
    }
    
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "RED" => Some(Team::Red),
            "BLU" => Some(Team::Blu),
            _ => None,
        }
    }
}
