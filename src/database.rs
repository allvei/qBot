// CHECK ME

//Imports
use std::collections::HashMap;

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::models::*;

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

        Ok(Database { pool })
    }

    /// Creates a new user in the database.
    ///
    /// Returns a `Result` containing the created user, or an `anyhow::Error` if creation fails.
    pub async fn new_user(&self, discord_id: u64) -> Result<Player> {
        let result = sqlx::query(
            "INSERT INTO users (discord_id, user)
            VALUES (?, ?)
            ON CONFLICT(discord_id) DO UPDATE SET user=excluded.user
            RETURNING id, discord_id, steam_id64, user, created_at, updated_at",
        )
        .bind(discord_id.to_string())
        .fetch_one(&self.pool)
        .await?;

        let db_player = Player::new(result.get::<i64, _>("discord_id") as u64, None);

        Ok(db_player)
    }

    pub async fn get_user(&self, discord_id: u64) -> Result<Player> {
        let result = sqlx::query(
            "SELECT id, discord_id, steam_id64, user, created_at, updated_at
            FROM users
            WHERE discord_id = ?",
        )
        .bind(discord_id.to_string())
        .fetch_one(&self.pool)
        .await?;

        let db_player = Player::new(result.get::<i64, _>("discord_id") as u64, None);

        Ok(db_player)
    }

    pub async fn set_user(&self, discord_id: u64, steam_id64: u64) -> Result<Player> {
        let result = sqlx::query(
            "UPDATE users
            SET steam_id64 = ?, updated_at = CURRENT_TIMESTAMP
            WHERE discord_id = ?",
        )
        .bind(steam_id64.to_string())
        .bind(discord_id.to_string())
        .execute(&self.pool)
        .await?;

        let db_player = Player::new(discord_id, Some(steam_id64));

        Ok(db_player)
    }

    /// Creates a new group in the database.
    ///
    /// Returns a `Result` containing the created group, or an `anyhow::Error` if creation fails.
    pub async fn new_group(&self, dashboard: u64, chat: u64, queue: u64, red: u64, blu: u64) -> Result<Group> {
        let result = sqlx::query(
            "INSERT INTO groups (dashboard, chat, queue, red, blu)
            VALUES (?, ?, ?, ?, ?)
            RETURNING id, dashboard, chat, queue, red, blu",
        )
        .bind(dashboard)
        .bind(chat)
        .bind(queue)
        .bind(red)
        .bind(blu)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            dashboard: result.get::<i64, _>("dashboard") as u64,
            chat: result.get::<i64, _>("chat") as u64,
            queue: result.get::<i64, _>("queue") as u64,
            teams: vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blu: result.get::<i64, _>("blu") as u64,
            }],
            session: Vec::new(),
            session_increment: 0,
        };

        Ok(group)
    }

    /// Retrieves a group from the database by its queue channel ID.
    ///
    /// Returns a `Result` containing the group, or an `anyhow::Error` if not found.
    pub async fn get_group(&self, queue_id: u64) -> Result<Group> {
        let result = sqlx::query(
            "SELECT dashboard, chat, queue, red, blu
            FROM groups
            WHERE queue = ?",
        )
        .bind(queue_id as i64)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            dashboard: result.get::<i64, _>("dashboard") as u64,
            chat: result.get::<i64, _>("chat") as u64,
            queue: result.get::<i64, _>("queue") as u64,
            teams: vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blu: result.get::<i64, _>("blu") as u64,
            }],
            session: Vec::new(),
            session_increment: 0,
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
    ) -> Result<Group> {
        let result = sqlx::query(
            "UPDATE groups
            SET dashboard = ?, chat = ?, red = ?, blu = ?
            WHERE queue = ?
            RETURNING dashboard, chat, queue, red, blu",
        )
        .bind(dashboard as i64)
        .bind(chat as i64)
        .bind(red as i64)
        .bind(blu as i64)
        .bind(queue_id as i64)
        .fetch_one(&self.pool)
        .await?;

        let group = Group {
            dashboard: result.get::<i64, _>("dashboard") as u64,
            chat: result.get::<i64, _>("chat") as u64,
            queue: result.get::<i64, _>("queue") as u64,
            teams: vec![TeamChannels {
                red: result.get::<i64, _>("red") as u64,
                blu: result.get::<i64, _>("blu") as u64,
            }],
            session: Vec::new(),
            session_increment: 0,
        };

        Ok(group)
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
        let rows = sqlx::query_as::<_, ConfigFormat>("SELECT key, value, description FROM config")
            .fetch_all(&self.pool)
            .await?;

        let mut config_map: HashMap<String, String> = HashMap::new();
        for row in rows {
            config_map.insert(row.key, row.value);
        }

        // Use macros to parse configuration values from the HashMap with default values
        Ok(Config {
            ic_queue: prscfg!(config_map, "queue_channel_id", 0),
            ic_log: prscfg!(config_map, "log_channel_id", 0),
            quota: prscfg!(config_map, "queue_size", 8),
            join_timeout: prscfg!(config_map, "confirmation_timeout", 120),
            i_runner: prscfg!(config_map, "runner_role_id", 0),
            i_admin: prscfg!(config_map, "admin_role_id", 0),
            ic_buffer: prscfg!(config_map, "buffer_channel_id", 0),
            ic_red: prscfg!(config_map, "red_channel_id", 0),
            ic_blue: prscfg!(config_map, "blue_channel_id", 0),
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
        sqlx::query("INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
