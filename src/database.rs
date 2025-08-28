// CHECK ME

//Imports
use std::collections::HashMap;

use anyhow::Result;
use sqlx::{ Row, SqlitePool };
use tracing::info;

use crate::models::*;
use tracing::error;

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
        info!("Database connection established");
        Ok(db)
    }

    pub async fn init_db(&self) -> Result<()> {
        info!("Initializing database");
        self.init_config_table().await?;
        self.init_user_table().await?;
        self.init_group_table().await?;
        Ok(())
    }

    pub async fn new_column(&self, table: &str, columns: Vec<&str>) -> Result<()> {
        info!("Adding columns {:?} to table {} if not present", columns, table);

        // Get existing columns from PRAGMA table_info
        let existing_cols: Vec<String> = sqlx
            ::query(&format!("PRAGMA table_info({})", table))
            .fetch_all(&self.pool).await?
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

    pub async fn new_table(&self, table: &str) -> Result<()> {
        info!("Creating new table: {}", table);
        sqlx::query(&format!("CREATE TABLE IF NOT EXISTS {} (id INTEGER PRIMARY KEY)", table)).execute(&self.pool).await?;

        Ok(())
    }

    pub async fn init_user_table(&self) -> Result<()> {
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='users'").fetch_all(&self.pool).await?;

        if result.is_empty() {
            info!("Users table not found, creating...");
            sqlx::query("CREATE TABLE users (
                id INTEGER PRIMARY KEY,
                discord_id INTEGER NOT NULL UNIQUE,
                steam_id INTEGER
            )").execute(&self.pool).await?;
        } else {
            // Check if the table has the correct schema including UNIQUE constraint
            let index_info = sqlx::query("PRAGMA index_list(users)")
                .fetch_all(&self.pool).await?;
            
            let has_unique_discord_id = index_info.iter().any(|row| {
                if let Ok(unique) = row.try_get::<i64, _>("unique") {
                    unique == 1
                } else {
                    false
                }
            });
            
            let existing_cols: Vec<String> = sqlx::query("PRAGMA table_info(users)")
                .fetch_all(&self.pool).await?
                .into_iter()
                .filter_map(|row| row.try_get::<String, _>("name").ok())
                .collect();
            
            let has_discord_id = existing_cols.contains(&"discord_id".to_string());
            let has_steam_id = existing_cols.contains(&"steam_id".to_string());
            
            if !has_discord_id || !has_steam_id || !has_unique_discord_id {
                info!("Users table schema is incomplete, recreating with proper schema...");
                
                // Backup existing data if any
                let backup_data = if has_discord_id {
                    sqlx::query("SELECT discord_id, steam_id FROM users")
                        .fetch_all(&self.pool).await.unwrap_or_default()
                } else {
                    Vec::new()
                };
                
                // Drop and recreate table
                sqlx::query("DROP TABLE users").execute(&self.pool).await?;
                sqlx::query("CREATE TABLE users (
                    id INTEGER PRIMARY KEY,
                    discord_id INTEGER NOT NULL UNIQUE,
                    steam_id INTEGER
                )").execute(&self.pool).await?;
                
                // Restore data if we had any
                for row in backup_data {
                    let discord_id: i64 = row.get("discord_id");
                    let steam_id: Option<i64> = row.try_get("steam_id").ok();
                    sqlx::query("INSERT OR IGNORE INTO users (discord_id, steam_id) VALUES (?, ?)")
                        .bind(discord_id)
                        .bind(steam_id)
                        .execute(&self.pool).await?;
                }
            }
        }
        Ok(())
    }


    /// Creates a new user in the database.
    ///
    /// Returns a `Result` containing the created user, or an `anyhow::Error` if creation fails.
    pub async fn new_user(&self, discord_id: u64) -> Result<Player> {
        info!("Creating new user with discord_id: {}", discord_id);
        let result = sqlx
            ::query("INSERT INTO users (discord_id, steam_id)
            VALUES (?, ?)
            ON CONFLICT(discord_id) DO UPDATE SET steam_id=excluded.steam_id
            RETURNING id, discord_id, steam_id")
            .bind(discord_id as i64)
            .bind(0i64) // Default steam_id
            .fetch_one(&self.pool).await?;

        let steam_id = result.get::<Option<i64>, _>("steam_id").map(|id| id as u64);
        let db_player = Player::construct(result.get::<i64, _>("discord_id") as u64, steam_id);

        Ok(db_player)
    }

    pub async fn get_user(&self, discord_id: u64) -> Result<Player> {
        // info!("Getting user with discord_id: {}", discord_id);
        let result = match sqlx
            ::query("SELECT id, discord_id, steam_id
            FROM users
            WHERE discord_id = ?")
            .bind(discord_id as i64)
            .fetch_one(&self.pool).await {
                Ok(result) => result,
                Err(e) => return Err(e.into()),
            };

        // info!("Retrieved user data: id={}, discord_id={}, steam_id={:?}", 
        //       result.get::<i64, _>("id"),
        //       result.get::<i64, _>("discord_id"),
        //       result.get::<Option<i64>, _>("steam_id"));

        let steam_id = result.get::<Option<i64>, _>("steam_id").map(|id| id as u64);
        let db_player = Player::construct(result.get::<i64, _>("discord_id") as u64, steam_id);
        // info!("Successfully loaded user!");

        Ok(db_player)
    }

    pub async fn set_user(&self, discord_id: u64, steam_id: Option<u64>) -> Result<Player> {
        info!("Updating user with discord_id: {}", discord_id);
        let _result = sqlx
            ::query("UPDATE users
            SET steam_id = ?
            WHERE discord_id = ?")
            .bind(steam_id.map(|id| id as i64))
            .bind(discord_id as i64)
            .execute(&self.pool).await?;

        let db_player = Player::construct(discord_id, steam_id);

        Ok(db_player)
    }

    pub async fn init_group_table(&self) -> Result<()> {
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='groups'").fetch_all(&self.pool).await?;

        if result.is_empty() {
            info!("Groups table not found, creating...");
            sqlx::query("CREATE TABLE groups (
                id INTEGER PRIMARY KEY,
                guild_id INTEGER NOT NULL,
                dashboard INTEGER NOT NULL,
                chat INTEGER NOT NULL,
                queue INTEGER NOT NULL,
                red INTEGER NOT NULL,
                blu INTEGER NOT NULL,
                session INTEGER DEFAULT 0,
                session_increment INTEGER DEFAULT 0,
                session_quota INTEGER DEFAULT 10
            )").execute(&self.pool).await?;
        } else {
            // Check if the table has the correct schema
            let existing_cols: Vec<String> = sqlx::query("PRAGMA table_info(groups)")
                .fetch_all(&self.pool).await?
                .into_iter()
                .filter_map(|row| row.try_get::<String, _>("name").ok())
                .collect();
            
            if !existing_cols.contains(&"guild_id".to_string()) {
                info!("Groups table schema is incorrect, recreating...");
                sqlx::query("DROP TABLE groups").execute(&self.pool).await?;
                sqlx::query("CREATE TABLE groups (
                    id INTEGER PRIMARY KEY,
                    guild_id INTEGER NOT NULL,
                    dashboard INTEGER NOT NULL,
                    chat INTEGER NOT NULL,
                    queue INTEGER NOT NULL,
                    red INTEGER NOT NULL,
                    blu INTEGER NOT NULL,
                    session INTEGER DEFAULT 0,
                    session_increment INTEGER DEFAULT 0,
                    session_quota INTEGER DEFAULT 10
                )").execute(&self.pool).await?;
            }
        }
        Ok(())
    }

    /// Creates a new group in the database.
    ///
    /// Returns a `Result` containing the created group, or an `anyhow::Error` if creation fails.
    pub async fn new_group(&self, guild_id: u64, dashboard: u64, chat: u64, queue: u64, red: u64, blu: u64, session_quota: u8) -> Result<Group> {
        info!("Creating new group with queue: {}", queue);
        let result = sqlx
            ::query("INSERT INTO groups (guild_id, dashboard, chat, queue, red, blu, session_quota)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING guild_id, dashboard, chat, queue, red, blu, session_quota")
            .bind(guild_id as i64)
            .bind(dashboard as i64)
            .bind(chat as i64)
            .bind(queue as i64)
            .bind(red as i64)
            .bind(blu as i64)
            .bind(session_quota as i64)
            .fetch_one(&self.pool).await?;

        let group = Group {
            guild_id,
            dashboard_id:  result.get::<i64, _>("dashboard") as u64,
            queue_chat_id: result.get::<i64, _>("chat") as u64,
            queue_id:      result.get::<i64, _>("queue") as u64,
            teams: vec![TeamChannels {
                red_vc_id: result.get::<i64, _>("red") as u64,
                blu_vc_id: result.get::<i64, _>("blu") as u64,
            }],
            sessions: Vec::new(),
            session_increment: 0,
            quota: result.get::<i64, _>("session_quota") as u8,
        };

        Ok(group)
    }

    /// Retrieves a group from the database by any channel ID associated with it.
    ///
    /// Returns a `Result` containing the group, or an `anyhow::Error` if not found.
    pub async fn get_group_by_channel(&self, channel_id: u64) -> Result<Group> {
        info!("Looking for group with channel_id: {}", channel_id);
        let result = sqlx
            ::query("SELECT guild_id, dashboard, chat, queue, red, blu, session_increment, session_quota
            FROM groups
            WHERE dashboard = ? OR chat = ? OR queue = ? OR red = ? OR blu = ?")
            .bind(channel_id as i64)
            .bind(channel_id as i64)
            .bind(channel_id as i64)
            .bind(channel_id as i64)
            .bind(channel_id as i64)
            .fetch_one(&self.pool).await?;

        let group = Group {
            guild_id:      result.get::<i64, _>("guild_id")  as u64,
            dashboard_id:  result.get::<i64, _>("dashboard") as u64,
            queue_chat_id: result.get::<i64, _>("chat")      as u64,
            queue_id:      result.get::<i64, _>("queue")     as u64,
            teams: vec![TeamChannels {
                red_vc_id: result.get::<i64, _>("red") as u64,
                blu_vc_id: result.get::<i64, _>("blu") as u64,
            }],
            sessions: Vec::new(),
            session_increment: result.get::<i64, _>("session_increment") as u16,
            quota:             result.get::<i64, _>("session_quota")     as u8,
        };

        Ok(group)
    }

    /// Updates a group in the database.
    ///
    /// Returns a `Result` containing the updated group, or an `anyhow::Error` if update fails.
    pub async fn set_group(&self, guild_id: u64, queue_id: u64, dashboard: u64, chat: u64, red: u64, blu: u64, session_quota: u8) -> Result<Group> {
        info!("Updating group with queue_id: {}", queue_id);
        let result = sqlx
            ::query("UPDATE groups
            SET dashboard = ?, chat = ?, red = ?, blu = ?, session_quota = ?
            WHERE queue = ?
            RETURNING guild_id, dashboard, chat, queue, red, blu, session_quota")
            .bind(guild_id as i64)
            .bind(dashboard as i64)
            .bind(chat as i64)
            .bind(red as i64)
            .bind(blu as i64)
            .bind(queue_id as i64)
            .bind(session_quota as i64)
            .fetch_one(&self.pool).await?;

        let group = Group {
            guild_id:      result.get::<i64, _>("guild") as u64,
            dashboard_id:  result.get::<i64, _>("dashboard") as u64,
            queue_chat_id: result.get::<i64, _>("chat") as u64,
            queue_id:      result.get::<i64, _>("queue") as u64,
            teams: vec![TeamChannels {
                red_vc_id: result.get::<i64, _>("red") as u64,
                blu_vc_id: result.get::<i64, _>("blu") as u64,
            }],
            sessions: Vec::new(),
            session_increment: result.get::<i64, _>("session_increment") as u16,
            quota: result.get::<i64, _>("session_quota") as u8,
        };

        Ok(group)
    }

    pub async fn init_config_table(&self) -> Result<()> {
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='config'").fetch_all(&self.pool).await?;

        if result.is_empty() {
            info!("Config table not found, creating...");
            sqlx
                ::query("CREATE TABLE config (
                    guild    INTEGER NOT NULL,
                    key      TEXT NOT NULL,
                    value    TEXT,
                    description TEXT,
                    PRIMARY KEY(guild, key)
                )")
                .execute(&self.pool).await?;
        } else {
            // Check if guild column exists, add it if missing
            let existing_cols: Vec<String> = sqlx::query("PRAGMA table_info(config)")
                .fetch_all(&self.pool).await?
                .into_iter()
                .filter_map(|row| row.try_get::<String, _>("name").ok())
                .collect();
            
            if !existing_cols.contains(&"guild".to_string()) {
                info!("Adding missing guild column to config table");
                sqlx::query("ALTER TABLE config ADD COLUMN guild INTEGER NOT NULL DEFAULT 0")
                    .execute(&self.pool).await?;
            }
        }
        Ok(())
    }

    /// Retrieves all configuration values from the database and constructs a `Config` object.
    ///
    /// This method reads all key-value pairs from the config table and maps them to the
    /// appropriate fields in the BotConfig struct. Default values are used for missing or
    /// malformed configuration entries.
    ///
    /// If no configuration exists for the specified guild, a new default configuration will be created.
    ///
    /// Returns a `Result` containing the populated `BotConfig` object or an error if the database
    /// query fails.
    pub async fn get_config(&self, guild_id: u64) -> Result<GroupConfig, anyhow::Error> {
        // Ensure all required columns exist
        let existing_cols: Vec<String> = sqlx::query("PRAGMA table_info(config)")
            .fetch_all(&self.pool).await?
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("name").ok())
            .collect();
        
        let required_columns = vec!["guild", "key", "value", "description"];
        for col in &required_columns {
            if !existing_cols.contains(&col.to_string()) {
                error!("Missing required column '{}' in config table", col);
                return Err(anyhow::anyhow!("Config table missing required column: {}", col));
            }
        }
        
        let rows = sqlx
            ::query_as::<_, ConfigFormat>("SELECT key, value, description FROM config WHERE guild = ?")
            .bind(guild_id as i64)
            .fetch_all(&self.pool).await?;
        
        // If no config found, create default config
        if rows.is_empty() {
            info!("No config found for guild {}, creating default configuration", guild_id);

            // Create default config values
            let default_config = GroupConfig::empty(guild_id, 0);

            // Save default config to database
            self.set_config("runner_r_id",     &default_config.runner_r_id    .to_string(), guild_id).await?;
            self.set_config("admin_r_id",      &default_config.admin_r_id     .to_string(), guild_id).await?;
            self.set_config("dashboard_tc_id", &default_config.dashboard_tc_id.to_string(), guild_id).await?;
            self.set_config("queue_chat_id",   &default_config.queue_tc_id    .to_string(), guild_id).await?;
            self.set_config("queue_vc_id",     &default_config.queue_vc_id    .to_string(), guild_id).await?;
            self.set_config("log_tc_id",       &default_config.log_tc_id      .to_string(), guild_id).await?;
            self.set_config("red_vc_id",       &default_config.red_vc_id      .to_string(), guild_id).await?;
            self.set_config("blue_tc_id",      &default_config.blu_vc_id      .to_string(), guild_id).await?;

            return Ok(default_config);
        }

        // Create config map
        let mut config_map: HashMap<String, String> = HashMap::new();
        for row in rows {
            if let Some(value) = row.value {
                config_map.insert(row.key, value);
            }
        }

        // Create config from map
        let config = GroupConfig::new(
            guild_id,
            0,
            config_map.get("runner_r_id")    .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            config_map.get("admin_r_id")     .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            config_map.get("dashboard_tc_id").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            config_map.get("queue_chat_id")  .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            config_map.get("queue_vc_id")    .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            config_map.get("log_tc_id")      .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            config_map.get("red_vc_id")      .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            config_map.get("blue_tc_id")     .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0)
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
    pub async fn set_config(&self, key: &str, value: &str, guild_id: u64) -> Result<()> {
        info!("Setting config key '{}' to value '{}' for guild {}", key, value, guild_id);
        let query_result = sqlx
            ::query("INSERT OR REPLACE INTO config (guild, key, value) VALUES (?, ?, ?)")
            .bind(guild_id as i64)
            .bind(key)
            .bind(value)
            .execute(&self.pool).await;

        match query_result {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to set config: {}", e);
                Err(e.into())
            }
        }
    }
}
