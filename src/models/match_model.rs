use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Match {
    pub id: i64,
    pub match_uuid: String,
    pub red_team_channel_id: Option<String>,
    pub blu_team_channel_id: Option<String>,
    pub server_channel: Option<String>,
    pub status: MatchStatus,
    pub created_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub confirmed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum MatchStatus {
    #[sqlx(rename = "forming")]
    Forming,
    #[sqlx(rename = "confirmed")]
    Confirmed,
    #[sqlx(rename = "in_progress")]
    InProgress,
    #[sqlx(rename = "ended")]
    Ended,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MatchPlayer {
    pub id: i64,
    pub match_id: i64,
    pub user_id: i64,
    pub team: Team,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum Team {
    #[sqlx(rename = "RED")]
    Red,
    #[sqlx(rename = "BLU")]
    Blu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMatch {
    pub match_uuid: String,
    pub red_team_channel_id: Option<String>,
    pub blu_team_channel_id: Option<String>,
    pub server_channel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamAssignment {
    pub red_team: Vec<i64>, // user_ids
    pub blu_team: Vec<i64>, // user_ids
}
