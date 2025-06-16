use anyhow::Result;
use sqlx::{SqlitePool, Row};
use crate::models::*;
use std::collections::HashMap;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = SqlitePool::connect(database_url).await?;
        
        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;
        
        Ok(Self { pool })
    }

    // User operations
    pub async fn create_user(&self, user: CreateUser) -> Result<User> {
        let row = sqlx::query(
            "INSERT INTO users (discord_id, steam_id, username) VALUES (?, ?, ?) RETURNING id, discord_id, steam_id, username, created_at, updated_at"
        )
        .bind(&user.discord_id)
        .bind(&user.steam_id)
        .bind(&user.username)
        .fetch_one(&self.pool)
        .await?;

        Ok(User {
            id: row.get("id"),
            discord_id: row.get("discord_id"),
            steam_id: row.get("steam_id"),
            username: row.get("username"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn get_user_by_discord_id(&self, discord_id: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT id, discord_id, steam_id, username, created_at, updated_at FROM users WHERE discord_id = ?"
        )
        .bind(discord_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(user)
    }

    pub async fn get_or_create_user(&self, discord_id: &str, username: &str) -> Result<User> {
        if let Some(user) = self.get_user_by_discord_id(discord_id).await? {
            // Update username if it changed
            if user.username != username {
                let updated_user = sqlx::query_as::<_, User>(
                    "UPDATE users SET username = ?, updated_at = CURRENT_TIMESTAMP WHERE discord_id = ? RETURNING id, discord_id, steam_id, username, created_at, updated_at"
                )
                .bind(username)
                .bind(discord_id)
                .fetch_one(&self.pool)
                .await?;
                return Ok(updated_user);
            }
            Ok(user)
        } else {
            self.create_user(CreateUser {
                discord_id: discord_id.to_string(),
                steam_id: None,
                username: username.to_string(),
            }).await
        }
    }

    // Queue operations
    pub async fn join_queue(&self, user_id: i64, queue_type: QueueType) -> Result<QueueSession> {
        // Remove any existing queue sessions for this user
        self.leave_queue_by_user_id(user_id).await?;

        let queue_type_str: String = queue_type.into();
        let session = sqlx::query_as::<_, QueueSession>(
            "INSERT INTO queue_sessions (user_id, queue_type, status) VALUES (?, ?, 'waiting') RETURNING id, user_id, queue_type, joined_at, status"
        )
        .bind(user_id)
        .bind(queue_type_str)
        .fetch_one(&self.pool)
        .await?;

        Ok(session)
    }

    pub async fn leave_queue_by_user_id(&self, user_id: i64) -> Result<()> {
        sqlx::query(
            "DELETE FROM queue_sessions WHERE user_id = ? AND status = 'waiting'"
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_queue_waiting(&self, queue_type: QueueType) -> Result<Vec<(QueueSession, User)>> {
        let queue_type_str: String = queue_type.into();
        let rows = sqlx::query(
            r#"
            SELECT qs.id, qs.user_id, qs.queue_type, qs.joined_at, qs.status, u.id, u.discord_id, u.steam_id, u.username, u.created_at as user_created_at, u.updated_at as user_updated_at
            FROM queue_sessions qs
            JOIN users u ON qs.user_id = u.id
            WHERE qs.queue_type = ? AND qs.status = 'waiting'
            ORDER BY qs.joined_at ASC
            "#,
        )
        .bind(queue_type_str)
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let session = QueueSession {
                id: row.get("id"),
                user_id: row.get("user_id"),
                queue_type: row.get("queue_type"),
                joined_at: row.get("joined_at"),
                status: QueueStatus::Waiting,
            };
            let user = User {
                id: row.get("id"),
                discord_id: row.get("discord_id"),
                steam_id: row.get("steam_id"),
                username: row.get("username"),
                created_at: row.get("user_created_at"),
                updated_at: row.get("user_updated_at"),
            };
            result.push((session, user));
        }

        Ok(result)
    }

    pub async fn get_queue_count(&self, queue_type: QueueType) -> Result<i64> {
        let queue_type_str: String = queue_type.into();
        let row = sqlx::query(
            "SELECT COUNT(*) as count FROM queue_sessions WHERE queue_type = ? AND status = 'waiting'"
        )
        .bind(queue_type_str)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get("count"))
    }

    // Match operations
    pub async fn create_match(&self, create_match: CreateMatch) -> Result<Match> {
        let match_row = sqlx::query_as::<_, Match>(
            "INSERT INTO matches (match_uuid, red_team_channel_id, blu_team_channel_id, server_channel) VALUES (?, ?, ?, ?) RETURNING id, match_uuid, red_team_channel_id, blu_team_channel_id, server_channel, status, created_at, confirmed_at, ended_at, confirmed_by"
        )
        .bind(&create_match.match_uuid)
        .bind(&create_match.red_team_channel_id)
        .bind(&create_match.blu_team_channel_id)
        .bind(&create_match.server_channel)
        .fetch_one(&self.pool)
        .await?;

        Ok(match_row)
    }

    pub async fn add_players_to_match(&self, match_id: i64, team_assignment: TeamAssignment) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        for user_id in team_assignment.red_team {
            sqlx::query(
                "INSERT INTO match_players (match_id, user_id, team) VALUES (?, ?, 'RED')"
            )
            .bind(match_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

            // Update queue status
            sqlx::query(
                "UPDATE queue_sessions SET status = 'in_match' WHERE user_id = ?"
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }

        for user_id in team_assignment.blu_team {
            sqlx::query(
                "INSERT INTO match_players (match_id, user_id, team) VALUES (?, ?, 'BLU')"
            )
            .bind(match_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

            // Update queue status
            sqlx::query(
                "UPDATE queue_sessions SET status = 'in_match' WHERE user_id = ?"
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn confirm_match(&self, match_id: i64, confirmed_by: &str) -> Result<()> {
        sqlx::query(
            "UPDATE matches SET status = 'confirmed', confirmed_at = CURRENT_TIMESTAMP, confirmed_by = ? WHERE id = ?"
        )
        .bind(confirmed_by)
        .bind(match_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn end_match(&self, match_id: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Update match status
        sqlx::query(
            "UPDATE matches SET status = 'ended', ended_at = CURRENT_TIMESTAMP WHERE id = ?"
        )
        .bind(match_id)
        .execute(&mut *tx)
        .await?;

        // Remove players from queue sessions
        sqlx::query(
            "DELETE FROM queue_sessions WHERE user_id IN (SELECT user_id FROM match_players WHERE match_id = ?)"
        )
        .bind(match_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_match_players(&self, match_id: i64) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            "SELECT u.discord_id AS discord_id, mp.team AS team FROM match_players mp JOIN users u ON mp.user_id = u.id WHERE mp.match_id = ?"
        )
        .bind(match_id)
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let discord_id: String = row.get("discord_id");
            let team: String = row.get("team");
            result.push((discord_id, team));
        }
        Ok(result)
    }

    pub async fn get_match_by_uuid(&self, match_uuid: &str) -> Result<Match> {
        let m = sqlx::query_as::<_, Match>(
            "SELECT * FROM matches WHERE match_uuid = ?"
        )
        .bind(match_uuid)
        .fetch_one(&self.pool)
        .await?;
        Ok(m)
    }

    pub async fn get_latest_forming_match(&self) -> Result<Match> {
        let m = sqlx::query_as::<_, Match>(
            "SELECT * FROM matches WHERE status = 'forming' ORDER BY created_at DESC LIMIT 1"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(m)
    }

    pub async fn get_latest_confirmed_match(&self) -> Result<Match> {
        let m = sqlx::query_as::<_, Match>(
            "SELECT * FROM matches WHERE status = 'confirmed' ORDER BY confirmed_at DESC LIMIT 1"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(m)
    }

    // Config operations
    pub async fn get_config(&self) -> Result<BotConfig> {
        let rows = sqlx::query_as::<_, Config>("SELECT key, value, description FROM config")
            .fetch_all(&self.pool)
            .await?;

        let mut config_map: HashMap<String, String> = HashMap::new();
        for row in rows {
            config_map.insert(row.key, row.value);
        }

        Ok(BotConfig {
            guild_id: config_map.get("guild_id").unwrap_or(&String::new()).clone(),
            queue_channel_id: config_map.get("queue_channel_id").unwrap_or(&String::new()).clone(),
            red_channel_id: config_map.get("red_channel_id").unwrap_or(&String::new()).clone(),
            blu_channel_id: config_map.get("blu_channel_id").unwrap_or(&String::new()).clone(),
            server_a_channel_id: config_map.get("server_a_channel_id").unwrap_or(&String::new()).clone(),
            server_b_channel_id: config_map.get("server_b_channel_id").unwrap_or(&String::new()).clone(),
            server_c_channel_id: config_map.get("server_c_channel_id").unwrap_or(&String::new()).clone(),
            log_channel_id: config_map.get("log_channel_id").unwrap_or(&String::new()).clone(),
            queue_size: config_map.get("queue_size").unwrap_or(&"8".to_string()).parse().unwrap_or(8),
            confirmation_timeout: config_map.get("confirmation_timeout").unwrap_or(&"120".to_string()).parse().unwrap_or(120),
            runner_role_id: config_map.get("runner_role_id").unwrap_or(&String::new()).clone(),
            admin_role_id: config_map.get("admin_role_id").unwrap_or(&String::new()).clone(),
        })
    }

    pub async fn set_config(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)"
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
