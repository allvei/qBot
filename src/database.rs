use anyhow::Result;
use sqlx::{SqlitePool, Row};
use tracing::{info};
use crate::models::*;
use std::collections::HashMap;

/// Macro to parse configuration values from a HashMap with default values
macro_rules! prscfg {
    ($map:expr, $key:expr, $default:expr) => {
        $map.get($key).unwrap_or(&$default.to_string()).parse().unwrap_or($default)
    };
}

/// Helper macro to create Channels from config map
macro_rules! prsteam {
    ($map:expr, $prefix:expr) => {
        Channels {
            red_id: prscfg!($map, &format!("red_{}_channel_id", $prefix), 0),
            blu_id: prscfg!($map, &format!("blu_{}_channel_id", $prefix), 0),
        }
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
        let db_path_str = database_url.strip_prefix("sqlite:").unwrap_or(database_url);

        if !db_path_str.is_empty() && !db_path_str.contains(":memory:") {
            let db_path = FileManager::normalize_path(db_path_str);
            if !FileManager::file_exists(&db_path) {
                info!("Missing database file, creating: {}", db_path.display());
                FileManager::create_file(&db_path)?;
            }
        }

        let pool = SqlitePool::connect(database_url).await?;
        
        Ok(Database { pool })
    }



    /// Creates a new user in the database.
    /// 
    /// Returns a `Result` containing the created user, or an `anyhow::Error` if creation fails.
    pub async fn create_user(&self, discord_id: u64) -> Result<Player> {
        let result = sqlx::query(
            "INSERT INTO users (discord_id, username)
            VALUES (?, ?)
            ON CONFLICT(discord_id) DO UPDATE SET username=excluded.username
            RETURNING id, discord_id, steam_id64, username, created_at, updated_at"
        )
        .bind(discord_id.to_string())
        .fetch_one(&self.pool)
        .await?;
    
        let db_player = Player::new(result.get::<i64, _>("discord_id") as u64);
        
        Ok(db_player)
    }

    /// Retrieves all configuration values from the database and constructs a `BotConfig` object.
    /// 
    /// This method reads all key-value pairs from the config table and maps them to the
    /// appropriate fields in the BotConfig struct. Default values are used for missing or
    /// malformed configuration entries.
    ///
    /// Returns a `Result` containing the populated `BotConfig` object or an error if the database
    /// query fails.
    pub async fn get_config(&self) -> Result<BotConfig> {
        let rows = sqlx::query_as::<_, Config>("SELECT key, value, description FROM config")
            .fetch_all(&self.pool)
            .await?;

        let mut config_map: HashMap<String, String> = HashMap::new();
        for row in rows {
            config_map.insert(row.key, row.value);
        }

        Ok(BotConfig {
            guild_id:             prscfg!(config_map, "guild_id", 0),
            queue_channel_id:     prscfg!(config_map, "queue_channel_id", 0),
            log_channel_id:       prscfg!(config_map, "log_channel_id", 0),
            queue_quota:          prscfg!(config_map, "queue_size", 8),
            confirmation_timeout: prscfg!(config_map, "confirmation_timeout", 120),
            runner_role_id:       prscfg!(config_map, "runner_role_id", 0),
            admin_role_id:        prscfg!(config_map, "admin_role_id", 0),
            apug:                 prsteam!(config_map, "apug"),
            bpug:                 prsteam!(config_map, "bpug"),
            cpug:                 prsteam!(config_map, "cpug"),
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
