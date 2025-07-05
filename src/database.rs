// CHECK ME

//Imports
use std::collections::HashMap;

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::models::*;
use tracing::error;

/// Macro to parse configuration values from a HashMap with default values
macro_rules! prscfg {
    ($map:expr, $key:expr, $default:expr) => {
        $map.get($key)
            .unwrap_or(&$default.to_string())
            .parse()
            .unwrap_or($default)
    };
}

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
        info!("[database] Initializing database");
        self.init_config().await?;
        self.init_users().await?;
        self.init_groups().await?;
        Ok(())
    }

    pub async fn new_column(&self, table: &str, columns: Vec<&str>) -> Result<()> {
        info!(
            "[database] Adding columns {:?} to table {} if not present",
            columns, table
        );

        // Get existing columns from PRAGMA table_info
        let existing_cols: Vec<String> = sqlx::query(&format!("PRAGMA table_info({})", table))
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("name").ok())
            .collect();

        for &col in &columns {
            if !existing_cols.contains(&col.to_string()) {
                info!("[database] Creating new column {} for table {}", col, table);
                sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {}", table, col))
                    .execute(&self.pool)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn new_table(&self, table: &str) -> Result<()> {
        info!("[database] Creating new table: {}", table);
        sqlx::query(&format!(
            "CREATE TABLE IF NOT EXISTS {} (id INTEGER PRIMARY KEY)",
            table
        ))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn init_users(&self) -> Result<()> {
        info!("[database] Checking if users have been created in the database");
        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='users'")
                .fetch_all(&self.pool)
                .await?;

        if result.is_empty() {
            self.new_table("users").await?;
            self.new_column("users", vec!["i_discord", "i_steam"])
                .await?;
        }
        Ok(())
    }

    /// Creates a new user in the database.
    ///
    /// Returns a `Result` containing the created user, or an `anyhow::Error` if creation fails.
    pub async fn new_user(&self, i_discord: u64) -> Result<Player> {
        info!("[database] Creating new user with i_discord: {}", i_discord);
        let result = sqlx::query(
            "INSERT INTO users (i_discord, user)
            VALUES (?, ?)
            ON CONFLICT(i_discord) DO UPDATE SET user=excluded.user
            RETURNING id, i_discord, i_steam, user, created_at, updated_at",
        )
        .bind(i_discord as i64)
        .fetch_one(&self.pool)
        .await?;

        let db_player = Player::new(
            result.get::<i64, _>("i_discord") as u64,
            result.get::<i64, _>("i_steam") as u64,
            None,
        );

        Ok(db_player)
    }

    pub async fn get_user(&self, i_discord: u64) -> Result<Player> {
        info!("[database] Getting user with i_discord: {}", i_discord);
        let result = sqlx::query(
            "SELECT id, i_discord, i_steam, user, created_at, updated_at
            FROM users
            WHERE i_discord = ?",
        )
        .bind(i_discord as i64)
        .fetch_one(&self.pool)
        .await?;

        info!(
            "[database] Retrieved user data: id={}, i_discord={}, i_steam={}",
            result.get::<i64, _>("id"),
            result.get::<i64, _>("i_discord"),
            result.get::<i64, _>("i_steam")
        );

        let db_player = Player::new(
            result.get::<i64, _>("i_discord") as u64,
            result.get::<i64, _>("i_steam") as u64,
            None,
        );

        info!(
            "[database] Created new player: i_discord={}, i_steam={}",
            db_player.i_discord,
            db_player.i_steam.unwrap_or(0)
        );

        Ok(db_player)
    }

    pub async fn set_user(&self, i_discord: u64, i_steam: u64) -> Result<Player> {
        info!("[database] Updating user with i_discord: {}", i_discord);
        let result = sqlx::query(
            "UPDATE users
            SET i_steam = ?, updated_at = CURRENT_TIMESTAMP
            WHERE i_discord = ?",
        )
        .bind(i_steam as i64)
        .bind(i_discord as i64)
        .execute(&self.pool)
        .await?;

        let db_player = Player::new(i_discord, i_steam, None);

        Ok(db_player)
    }

    pub async fn init_groups(&self) -> Result<()> {
        info!("[database] Checking if groups have been created in the database");
        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='groups'")
                .fetch_all(&self.pool)
                .await?;

        if result.is_empty() {
            self.new_table("groups").await?;
            self.new_column(
                "groups",
                vec![
                    "dashboard",
                    "chat",
                    "queue",
                    "red",
                    "blu",
                    "session",
                    "session_increment",
                    "session_quota",
                ],
            )
            .await?;
        }
        Ok(())
    }

    /// Creates a new group in the database.
    ///
    /// Returns a `Result` containing the created group, or an `anyhow::Error` if creation fails.
    pub async fn new_group(
        &self,
        dashboard: u64,
        chat: u64,
        queue: u64,
        red: u64,
        blu: u64,
        session_quota: u8,
    ) -> Result<Group> {
        info!("[database] Creating new group with queue: {}", queue);
        let result = sqlx::query(
            "INSERT INTO groups (dashboard, chat, queue, red, blu, session_quota)
            VALUES (?, ?, ?, ?, ?, ?)
            RETURNING dashboard, chat, queue, red, blu, session_quota",
        )
        .bind(dashboard as i64)
        .bind(chat as i64)
        .bind(queue as i64)
        .bind(red as i64)
        .bind(blu as i64)
        .bind(session_quota as i64)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            dashboard:         result.get::<i64, _>("dashboard") as u64,
            chat:              result.get::<i64, _>("chat") as u64,
            queue:             result.get::<i64, _>("queue") as u64,
            teams:             vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blu: result.get::<i64, _>("blu") as u64,
            }],
            session:           Vec::new(),
            session_increment: 0,
            session_quota:     result.get::<i64, _>("session_quota") as u8,
        };

        Ok(group)
    }

    /// Retrieves a group from the database by its queue channel ID.
    ///
    /// Returns a `Result` containing the group, or an `anyhow::Error` if not found.
    pub async fn get_group(&self, queue_id: u64) -> Result<Group> {
        info!("[database] Getting group with queue_id: {}", queue_id);
        let result = sqlx::query(
            "SELECT dashboard, chat, queue, red, blu, session_increment, session_quota
            FROM groups
            WHERE queue = ?",
        )
        .bind(queue_id as i64)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            dashboard:         result.get::<i64, _>("dashboard") as u64,
            chat:              result.get::<i64, _>("chat") as u64,
            queue:             result.get::<i64, _>("queue") as u64,
            teams:             vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blu: result.get::<i64, _>("blu") as u64,
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
        queue_id: u64,
        dashboard: u64,
        chat: u64,
        red: u64,
        blu: u64,
        session_quota: u8,
    ) -> Result<Group> {
        info!("[database] Updating group with queue_id: {}", queue_id);
        let result = sqlx::query(
            "UPDATE groups
            SET dashboard = ?, chat = ?, red = ?, blu = ?, session_quota = ?
            WHERE queue = ?
            RETURNING dashboard, chat, queue, red, blu, session_quota",
        )
        .bind(dashboard as i64)
        .bind(chat as i64)
        .bind(red as i64)
        .bind(blu as i64)
        .bind(queue_id as i64)
        .bind(session_quota as i64)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            dashboard:         result.get::<i64, _>("dashboard") as u64,
            chat:              result.get::<i64, _>("chat") as u64,
            queue:             result.get::<i64, _>("queue") as u64,
            teams:             vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blu: result.get::<i64, _>("blu") as u64,
            }],
            session:           Vec::new(),
            session_increment: result.get::<i64, _>("session_increment") as u16,
            session_quota:     result.get::<i64, _>("session_quota") as u8,
        };

        Ok(group)
    }

    pub async fn init_config(&self) -> Result<()> {
        info!("[database] Checking if config has been created in the database");
        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='config'")
                .fetch_all(&self.pool)
                .await?;

        if result.is_empty() {
            self.new_table("config").await?;
            self.new_column("config", vec!["key", "value", "description"])
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
    pub async fn get_config(&self) -> Result<Config> {
        info!("[database] Getting config from database");
        let rows = sqlx::query_as::<_, ConfigFormat>("SELECT key, value, description FROM config")
            .fetch_all(&self.pool)
            .await?;

        let mut config_map: HashMap<String, String> = HashMap::new();
        for row in rows {
            config_map.insert(row.key, row.value);
        }

        // Use macros to parse configuration values from the HashMap with default values
        Ok(Config {
            i_guild:      prscfg!(config_map, "guild_id", 0),
            i_runner:     prscfg!(config_map, "runner_role_id", 0),
            i_admin:      prscfg!(config_map, "admin_role_id", 0),
            ic_queue:     prscfg!(config_map, "queue_channel_id", 0),
            ic_log:       prscfg!(config_map, "log_channel_id", 0),
            ic_buffer:    prscfg!(config_map, "buffer_channel_id", 0),
            ic_red:       prscfg!(config_map, "red_channel_id", 0),
            ic_blue:      prscfg!(config_map, "blue_channel_id", 0),
            join_timeout: prscfg!(config_map, "confirmation_timeout", 120),
        })
    }

    /// Sets or updates a configuration value in the database.
    /// If the key already exists, its value will be replaced.
    ///
    /// Returns a `Result` indicating success or an `anyhow::Error`.
    ///
    /// * `key` - The key of the configuration item to set.
    /// * `value` - The value to associate with the key.
    pub async fn set_config(&self, key: &str, value: &str) -> Result<()> {
        info!(
            "[database] Setting config key '{}' to value '{}'",
            key, value
        );
        let query_result = sqlx::query("INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await;

        match query_result {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("[database] Failed to set config: {}", e);
                Err(e.into())
            }
        }
    }
}
