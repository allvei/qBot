// CHECK ME

//Imports
use std::collections::HashMap;

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::info;
use tracing::error;

use crate::models::*;
use crate::models::group::Group;
use crate::models::session::TeamChannels;
use crate::models::player::Player;

/// `Database` struct provides an interface for interacting with the SQLite database.
/// Manages the database connection pool and provides methods for various data operations.
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Creates a new `Database` instance and initializes the connection pool.
    ///
    /// Returns a `Result` containing the `Database` instance or an `anyhow::Error` if the connection fails.
    ///
    /// * `database_url` - The URL of the SQLite database to connect to.
    pub async fn new(database_url: &str) -> Result<Self> {
        // Get the database path
        let db_path_str = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
        // Check if the database file exists, create it if it doesn't
        if !db_path_str.is_empty() && !db_path_str.contains(":memory:") {
            let db_path = FileManager::normalize_path(db_path_str);
            if !FileManager::file_exists(&db_path) {
                info!("Missing database file, creating: {}", db_path.display());
                FileManager::create_file(&db_path)?;
            }
        }
        // Initialize the database connection pool
        let pool = SqlitePool::connect(database_url).await?;

        let db = Database { pool };
        db.init_db().await?;

        Ok(db)
    }

    pub async fn init_db(&self) -> Result<()> {
        info!("Initializing database");
        self.init_config_table().await?;
        self.init_user_table().await?;
        self.init_group_table().await?;
        Ok(())
    }

    pub async fn new_column(
        &self,
        table: &str,
        columns: Vec<&str>,
    ) -> Result<()> {
        info!("Adding columns {:?} to table {} if not present", columns, table);

        // Get existing columns from PRAGMA table_info
        let existing_cols: Vec<String> = sqlx::query(&format!("PRAGMA table_info({})", table))
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("name").ok())
            .collect();

        for &col in &columns {
            if !existing_cols.contains(&col.to_string()) {
                info!("Creating new column {} for table {}", col, table);
                sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {}", table, col)).execute(&self.pool).await?;
            }
        }
        Ok(())
    }

    pub async fn new_table(
        &self,
        table: &str,
    ) -> Result<()> {
        info!("Creating new table: {}", table);
        sqlx::query(&format!("CREATE TABLE IF NOT EXISTS {} (id INTEGER PRIMARY KEY)", table)).execute(&self.pool).await?;

        Ok(())
    }

    pub async fn init_user_table(&self) -> Result<()> {
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='users'").fetch_all(&self.pool).await?;

        if result.is_empty() {
            info!("Users table not found, creating...");
            self.new_table("users").await?;
            self.new_column("users", vec!["discord_id", "steam_id"]).await?;
        }
        Ok(())
    }

    /// Creates a new user in the database.
    ///
    /// Returns a `Result` containing the created user, or an `anyhow::Error` if creation fails.
    pub async fn new_user(
        &self,
        discord_id: u64,
    ) -> Result<Player> {
        info!("Creating new user with discord_id: {}", discord_id);
        let result = sqlx::query(
            "INSERT INTO users (discord_id, user)
            VALUES (?, ?)
            ON CONFLICT(discord_id) DO UPDATE SET user=excluded.user
            RETURNING id, discord_id, steam_id, user, created_at, updated_at",
        )
        .bind(discord_id as i64)
        .fetch_one(&self.pool)
        .await?;

        let db_player = Player::new(result.get::<i64, _>("discord_id") as u64, result.get::<i64, _>("steam_id") as u64, None);

        Ok(db_player)
    }

    pub async fn get_user(
        &self,
        discord_id: u64,
    ) -> Result<Player> {
        info!("Getting user with discord_id: {}", discord_id);
        let result = sqlx::query(
            "SELECT id, discord_id, steam_id, user, created_at, updated_at
            FROM users
            WHERE discord_id = ?",
        )
        .bind(discord_id as i64)
        .fetch_one(&self.pool)
        .await?;

        info!(
            "Retrieved user data: id={}, discord_id={}, steam_id={}",
            result.get::<i64, _>("id"),
            result.get::<i64, _>("discord_id"),
            result.get::<i64, _>("steam_id")
        );

        let db_player = Player::new(result.get::<i64, _>("discord_id") as u64, result.get::<i64, _>("steam_id") as u64, None);

        info!("Created new player: discord_id={}, steam_id={}", db_player.discord_id, db_player.steam_id.unwrap_or(0));

        Ok(db_player)
    }

    pub async fn set_user(
        &self,
        discord_id: u64,
        steam_id: u64,
    ) -> Result<Player> {
        info!("Updating user with discord_id: {}", discord_id);
        let _result = sqlx::query(
            "UPDATE users
            SET steam_id = ?, updated_at = CURRENT_TIMESTAMP
            WHERE discord_id = ?",
        )
        .bind(steam_id as i64)
        .bind(discord_id as i64)
        .execute(&self.pool)
        .await?;

        let db_player = Player::new(discord_id, steam_id, None);

        Ok(db_player)
    }

    pub async fn init_group_table(&self) -> Result<()> {
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='groups'").fetch_all(&self.pool).await?;

        if result.is_empty() {
            info!("Groups table not found, creating...");
            self.new_table("groups").await?;
            self.new_column("groups", vec!["guild", "dashboard", "chat", "queue", "red", "blue", "session", "session_increment", "session_quota"])
                .await?;
        }
        Ok(())
    }

    /// Creates a new group in the database.
    ///
    /// Returns a `Result` containing the created group, or an `anyhow::Error` if creation fails.
    pub async fn new_group(
        &self,
        guild_id: u64,
        dashboard: u64,
        chat: u64,
        queue: u64,
        red: u64,
        blue: u64,
        session_quota: u8,
    ) -> Result<Group> {
        info!("Creating new group with queue: {}", queue);
        let result = sqlx::query(
            "INSERT INTO groups (guild_id, dashboard, chat, queue, red, blue, session_quota)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING guild_id, dashboard, chat, queue, red, blue, session_quota",
        )
        .bind(guild_id as i64)
        .bind(dashboard as i64)
        .bind(chat as i64)
        .bind(queue as i64)
        .bind(red as i64)
        .bind(blue as i64)
        .bind(session_quota as i64)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            guild_id,
            dashboard: result.get::<i64, _>("dashboard") as u64,
            chat: result.get::<i64, _>("chat") as u64,
            queue: result.get::<i64, _>("queue") as u64,
            teams: vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blue: result.get::<i64, _>("blue") as u64,
            }],
            session: Vec::new(),
            session_increment: 0,
            session_quota: result.get::<i64, _>("session_quota") as u8,
        };

        Ok(group)
    }

    /// Retrieves a group from the database by its queue channel ID.
    ///
    /// Returns a `Result` containing the group, or an `anyhow::Error` if not found.
    pub async fn get_group(
        &self,
        queue_id: u64,
    ) -> Result<Group> {
        info!("Getting group with queue_id: {}", queue_id);
        let result = sqlx::query(
            "SELECT dashboard, chat, queue, red, blue, session_increment, session_quota
            FROM groups
            WHERE queue = ?",
        )
        .bind(queue_id as i64)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            guild_id:          result.get::<i64, _>("guild") as u64,
            dashboard:         result.get::<i64, _>("dashboard") as u64,
            chat:              result.get::<i64, _>("chat") as u64,
            queue:             result.get::<i64, _>("queue") as u64,
            teams:             vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blue: result.get::<i64, _>("blue") as u64,
            }],
            session:           Vec::new(),
            session_increment: result.get::<i64, _>("session_increment") as u16,
            session_quota:     result.get::<i64, _>("session_quota") as u8,
        };

        Ok(group)
    }

    /// Updates a group in the database.
    ///
    /// Returns a `Result` containing the updated group, or an `anyhow::Error` if update fails.
    pub async fn set_group(
        &self,
        guild_id: u64,
        queue_id: u64,
        dashboard: u64,
        chat: u64,
        red: u64,
        blue: u64,
        session_quota: u8,
    ) -> Result<Group> {
        info!("Updating group with queue_id: {}", queue_id);
        let result = sqlx::query(
            "UPDATE groups
            SET dashboard = ?, chat = ?, red = ?, blue = ?, session_quota = ?
            WHERE queue = ?
            RETURNING guild_id, dashboard, chat, queue, red, blue, session_quota",
        )
        .bind(guild_id as i64)
        .bind(dashboard as i64)
        .bind(chat as i64)
        .bind(red as i64)
        .bind(blue as i64)
        .bind(queue_id as i64)
        .bind(session_quota as i64)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            guild_id:          result.get::<i64, _>("guild") as u64,
            dashboard:         result.get::<i64, _>("dashboard") as u64,
            chat:              result.get::<i64, _>("chat") as u64,
            queue:             result.get::<i64, _>("queue") as u64,
            teams:             vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blue: result.get::<i64, _>("blue") as u64,
            }],
            session:           Vec::new(),
            session_increment: result.get::<i64, _>("session_increment") as u16,
            session_quota:     result.get::<i64, _>("session_quota") as u8,
        };

        Ok(group)
    }

    pub async fn init_config_table(&self) -> Result<()> {
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='config'").fetch_all(&self.pool).await?;

        if result.is_empty() {
            info!("Config table not found, creating...");
            sqlx::query(
                "CREATE TABLE config (
                    guild    INTEGER NOT NULL UNIQUE,
                    key      TEXT NOT NULL,
                    value    TEXT,
                    description TEXT,
                    PRIMARY KEY(guild, key)
                )",
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Retrieves all configuration values from the database and constructs a `Config` object.
    ///
    /// This method reads all key-value pairs from the config table and maps them to the
    /// appropriate fields in the BotConfig struct. Default values are used for missing or
    /// malformed configuration entries.
    ///
    /// Returns a `Result` containing the populated `BotConfig` object or an error if the database
    /// query fails.
    pub async fn get_config(&self) -> Result<Config, anyhow::Error> {
        info!("Getting config from database");
        let rows = sqlx::query_as::<_, ConfigFormat>("SELECT key, value, description FROM config").fetch_all(&self.pool).await?;

        if rows.is_empty() {
            error!("No config values found in database");
            return Err(anyhow::anyhow!("No configuration values found in database"));
        }

        let mut config_map: HashMap<String, String> = HashMap::new();
        for row in rows {
            config_map.insert(row.key, row.value);
        }

        let config = Config::new(
            config_map.get("guild_id").unwrap().parse().unwrap(),
            config_map.get("runner_role_id").unwrap().parse().unwrap(),
            config_map.get("admin_role_id").unwrap().parse().unwrap(),
            config_map.get("queue_channel_id").unwrap().parse().unwrap(),
            config_map.get("log_channel_id").unwrap().parse().unwrap(),
            config_map.get("buffer_channel_id").unwrap().parse().unwrap(),
            config_map.get("red_channel_id").unwrap().parse().unwrap(),
            config_map.get("blue_channel_id").unwrap().parse().unwrap(),
        );

        Ok(config)
    }

    /// Sets or updates a configuration value in the database.
    /// If the key already exists, its value will be replaced.
    ///
    /// Returns a `Result` indicating success or an `anyhow::Error`.
    ///
    /// * `key` - The key of the configuration item to set.
    /// * `value` - The value to associate with the key.
    pub async fn set_config(
        &self,
        key: &str,
        value: &str,
    ) -> Result<()> {
        info!("Setting config key '{}' to value '{}'", key, value);
        let query_result = sqlx::query("INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)").bind(key).bind(value).execute(&self.pool).await;

        match query_result {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to set config: {}", e);
                Err(e.into())
            }
        }
    }
}
