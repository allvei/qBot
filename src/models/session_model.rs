use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Session {
    pub id: i64,
    pub session_uuid: String,
    pub status: String,
    pub created_at: String,
    pub confirmed_at: Option<String>,
    pub ended_at: Option<String>,
    pub server_assignment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Waiting,
    Hot,
    Pushing,
    Playing,
    Pulling,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Waiting => "waiting",
            SessionStatus::Hot => "hot",
            SessionStatus::Pushing => "pushing",
            SessionStatus::Playing => "playing",
            SessionStatus::Pulling => "pulling",
        }
    }
    
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "waiting" => Some(SessionStatus::Waiting),
            "hot" => Some(SessionStatus::Hot),
            "pushing" => Some(SessionStatus::Pushing),
            "playing" => Some(SessionStatus::Playing),
            "pulling" => Some(SessionStatus::Pulling),
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
    pub fn as_str(&self) -> &'static str {
        match self {
            Team::Red => "RED",
            Team::Blu => "BLU",
        }
    }
    
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "RED" => Some(Team::Red),
            "BLU" => Some(Team::Blu),
            _ => None,
        }
    }
}
