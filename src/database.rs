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
            "INSERT INTO users (discord_id, steam_id64, username) VALUES (?, ?, ?) RETURNING id, discord_id, steam_id64, username, created_at, updated_at"
        )
        .bind(&user.discord_id)
        .bind(&user.steam_id64)
        .bind(&user.username)
        .fetch_one(&self.pool)
        .await?;

        Ok(User {
            id: row.get("id"),
            discord_id: row.get("discord_id"),
            steam_id64: row.get("steam_id64"),
            username: row.get("username"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn get_user_by_discord_id(&self, discord_id: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT id, discord_id, steam_id64, username, created_at, updated_at FROM users WHERE discord_id = ?"
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
                    "UPDATE users SET username = ?, updated_at = CURRENT_TIMESTAMP WHERE discord_id = ? RETURNING id, discord_id, steam_id64, username, created_at, updated_at"
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
                steam_id64: None,
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
            "INSERT INTO queue_sessions (user_id, queue_type) VALUES (?, ?) RETURNING id, user_id, queue_type, joined_at"
        )
        .bind(user_id)
        .bind(queue_type_str)
        .fetch_one(&self.pool)
        .await?;

        Ok(session)
    }

    pub async fn leave_queue_by_user_id(&self, user_id: i64) -> Result<()> {
        sqlx::query(
            "DELETE FROM queue_sessions WHERE user_id = ?"
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_queue_idle(&self, queue_type: QueueType) -> Result<Vec<(QueueSession, User)>> {
        let queue_type_str: String = queue_type.into();
        let rows = sqlx::query(
            r#"
            SELECT qs.id, qs.user_id, qs.queue_type, qs.joined_at, u.id, u.discord_id, u.steam_id64, u.username, u.created_at as user_created_at, u.updated_at as user_updated_at
            FROM queue_sessions qs
            JOIN users u ON qs.user_id = u.id
            WHERE qs.queue_type = ?
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
            };
            let user = User {
                id: row.get("id"),
                discord_id: row.get("discord_id"),
                steam_id64: row.get("steam_id64"),
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
            "SELECT COUNT(*) as count FROM queue_sessions WHERE queue_type = ?"
        )
        .bind(queue_type_str)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get("count"))
    }

    // Session operations
    pub async fn create_session(&self, red_players: &[User], blu_players: &[User], server: &str) -> Result<Session> {
        let session_uuid = uuid::Uuid::new_v4().to_string();
        
        let mut tx = self.pool.begin().await?;
        
        // Create session
        let session_id = sqlx::query(
            "INSERT INTO sessions (session_uuid, status, server_assignment) VALUES (?, ?, ?)"
        )
        .bind(&session_uuid)
        .bind("hot")
        .bind(server)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        
        // Add players to session
        for player in red_players {
            sqlx::query(
                "INSERT INTO session_players (session_id, user_id, team) VALUES (?, ?, ?)"
            )
            .bind(session_id)
            .bind(player.id)
            .bind("RED")
            .execute(&mut *tx)
            .await?;
        }
        
        for player in blu_players {
            sqlx::query(
                "INSERT INTO session_players (session_id, user_id, team) VALUES (?, ?, ?)"
            )
            .bind(session_id)
            .bind(player.id)
            .bind("BLU")
            .execute(&mut *tx)
            .await?;
        }
        
        tx.commit().await?;
        
        // Return the created session
        let session = Session {
            id: session_id,
            session_uuid,
            status: "hot".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            confirmed_at: None,
            ended_at: None,
            server_assignment: server.to_string(),
        };
        
        Ok(session)
    }

    pub async fn accept_session(&self, session_id: i64) -> Result<()> {
        sqlx::query("UPDATE sessions SET status = 'push', confirmed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn end_session(&self, session_id: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE sessions SET status = 'idle', ended_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_session_players(&self, session_id: i64) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            "SELECT u.discord_id AS discord_id, sp.team AS team FROM session_players sp JOIN users u ON sp.user_id = u.id WHERE sp.session_id = ?"
        )
        .bind(session_id)
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

    pub async fn get_session_by_uuid(&self, session_uuid: &str) -> Result<Session> {
        let s = sqlx::query_as::<_, Session>(
            "SELECT * FROM sessions WHERE session_uuid = ?"
        )
        .bind(session_uuid)
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn get_latest_hot_session(&self) -> Result<Session> {
        let s = sqlx::query_as::<_, Session>(
            "SELECT * FROM sessions WHERE status = 'hot' ORDER BY created_at DESC LIMIT 1"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn get_latest_push_session(&self) -> Result<Session> {
        let s = sqlx::query_as::<_, Session>(
            "SELECT * FROM sessions WHERE status = 'push' ORDER BY created_at DESC LIMIT 1"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
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
            log_channel_id: config_map.get("log_channel_id").unwrap_or(&String::new()).clone(),
            queue_size: config_map.get("queue_size").unwrap_or(&"8".to_string()).parse().unwrap_or(8),
            confirmation_timeout: config_map.get("confirmation_timeout").unwrap_or(&"120".to_string()).parse().unwrap_or(120),
            runner_role_id: config_map.get("runner_role_id").unwrap_or(&String::new()).clone(),
            admin_role_id: config_map.get("admin_role_id").unwrap_or(&String::new()).clone(),
            // Server A channels
            red_a_channel_id: config_map.get("red_a_channel_id").unwrap_or(&String::new()).clone(),
            blu_a_channel_id: config_map.get("blu_a_channel_id").unwrap_or(&String::new()).clone(),
            server_a_channel_id: config_map.get("server_a_channel_id").unwrap_or(&String::new()).clone(),
            // Server B channels
            red_b_channel_id: config_map.get("red_b_channel_id").unwrap_or(&String::new()).clone(),
            blu_b_channel_id: config_map.get("blu_b_channel_id").unwrap_or(&String::new()).clone(),
            server_b_channel_id: config_map.get("server_b_channel_id").unwrap_or(&String::new()).clone(),
            // Server C channels
            red_c_channel_id: config_map.get("red_c_channel_id").unwrap_or(&String::new()).clone(),
            blu_c_channel_id: config_map.get("blu_c_channel_id").unwrap_or(&String::new()).clone(),
            server_c_channel_id: config_map.get("server_c_channel_id").unwrap_or(&String::new()).clone(),
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
