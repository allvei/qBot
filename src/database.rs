// CHECK ME

//Imports
use std::collections::HashMap;

use anyhow::Result;
use sqlx::{SqlitePool, Row};
use tracing::{info};

use crate::models::*;

/// Macro to parse configuration values from a HashMap with default values
macro_rules! prscfg {
    ($map:expr, $key:expr, $default:expr) => {
        $map.get($key).unwrap_or(&$default.to_string()).parse().unwrap_or($default)
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
    pub async fn create_user(&self, discord_id: u64) -> Result<Player> {
        let result = sqlx::query(
            "INSERT INTO users (discord_id, user)
            VALUES (?, ?)
            ON CONFLICT(discord_id) DO UPDATE SET user=excluded.user
            RETURNING id, discord_id, steam_id64, user, created_at, updated_at"
        )
        .bind(discord_id.to_string())
        .fetch_one(&self.pool)
        .await?;
    
        let db_player = Player::new(result.get::<i64, _>("discord_id") as u64);
        
        Ok(db_player)
    }

    /// Retrieves all configuration values from the database and constructs a `Config` object.
    /// Retrieves all configuration values from the database and constructs a `Config` object.
    /// 
    /// This method reads all key-value pairs from the config table and maps them to the
    /// appropriate fields in the BotConfig struct. Default values are used for missing or
    /// malformed configuration entries.
    ///
    /// Returns a `Result` containing the populated `BotConfig` object or an error if the database
    /// query fails.
    pub async fn pull(&self) -> Result<Config> {
        let rows = sqlx::query_as::<_, ConfigFormat>("SELECT key, value, description FROM config")
            .fetch_all(&self.pool)
            .await?;

        let mut config_map: HashMap<String, String> = HashMap::new();
        for row in rows {
            config_map.insert(row.key, row.value);
        }

        // Use macros to parse configuration values from the HashMap with default values
        Ok(Config {
            cid_queue:            prscfg!(config_map, "queue_channel_id",     0),
            cid_log:              prscfg!(config_map, "log_channel_id",       0),
            queue_quota:          prscfg!(config_map, "queue_size",           8),
            confirmation_timeout: prscfg!(config_map, "confirmation_timeout", 120),
            id_runner:            prscfg!(config_map, "runner_role_id",       0),
            id_admin:             prscfg!(config_map, "admin_role_id",        0),
            cid_buffer:           prscfg!(config_map, "buffer_channel_id",    0),
            cid_red:              prscfg!(config_map, "red_channel_id",       0),
            cid_blue:             prscfg!(config_map, "blue_channel_id",      0),
        })
    }
    
    /// Get the configuration settings
    pub async fn get_config(&self) -> Result<Config> {
        self.pull().await
    }
    
    /// Get a session by its UUID
    pub async fn get_session_by_uuid(&self, uuid: &str) -> Result<Session> {
        // For now, return a dummy session
        // In a real implementation, this would query the database
        let mut session = Session::new();
        session.id = uuid.as_bytes().iter().map(|&b| b as u16).collect();
        Ok(session)
    }
    
    /// Get the latest hot session
    pub async fn get_latest_hot_session(&self) -> Result<Session> {
        // For now, return a dummy session with "hot" status
        // In a real implementation, this would query the database
        let mut session = Session::new();
        session.status = SessionStatus::Hot;
        Ok(session)
    }
    
    /// Get the latest push session
    pub async fn get_latest_push_session(&self) -> Result<Session> {
        // For now, return a dummy session with "ongoing" status
        // In a real implementation, this would query the database
        let mut session = Session::new();
        session.status = SessionStatus::Live;
        Ok(session)
    }
    
    /// Accept a session
    pub async fn accept_session(&self, session_id: String) -> Result<()> {
        // In a real implementation, this would update the session status in the database
        info!("Accepting session with ID: {}", session_id);
        Ok(())
    }
    
    /// End a session
    pub async fn end_session(&self, session_id: String) -> Result<()> {
        // In a real implementation, this would update the session status in the database
        info!("Ending session with ID: {}", session_id);
        Ok(())
    }

    pub async fn pull_group(&self) -> Result<Group> {
        // This is a placeholder implementation
        // In a real implementation, this would fetch group data from the database
        Ok(Group::new(0, 0, 0, 0, 0))
    }

    pub async fn init(&self) -> Result<()> {
        sqlx::query(
            "INSERT INTO config (key, value) VALUES (?, ?)"
        )
        .bind("queue_channel_id")
        .bind("0")
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a player by Discord ID or create one if it doesn't exist
    pub async fn get_or_create_player(&self, discord_id: u64) -> Result<Player> {
        // For now, simply create a new player instance
        // In a real implementation, this would check the database first
        Ok(Player::new(discord_id))
    }

    /// Get the active group with its session
    pub async fn get_group(&self) -> Result<Group> {
        // For now, return a dummy group
        // In a real implementation, this would query the database
        Ok(Group::new(0, 0, 0, 0, 0))
    }

    /// Update a group in the database
    pub async fn update_group(&self, group: &Group) -> Result<()> {
        // For now, just log the update
        // In a real implementation, this would update the group in the database
        info!("Updating group with session");
        Ok(())
    }

    /// Get the group with an idle session
    pub async fn get_group_idle(&self) -> Result<Group> {
        self.get_group().await
    }

    /// Create a new session
    pub async fn create_session(&self, players: Vec<Player>) -> Result<Session> {
        // Create a new session with the provided players
        let mut session = Session::new();
        for player in players {
            session.add_player(player);
        }
        Ok(session)
    }

    /// Remove a player from the group's session by their Discord user ID
    pub async fn leave_group_by_user_id(&self, group: &mut Group, user_id: u64) -> Result<bool> {
        // Remove player from session
        group.session.remove_player(user_id);
        info!("Player {} left group session", user_id);
        Ok(true)
    }

    /// Get session players
    pub async fn get_session_players(&self, session_id: &str) -> Result<Vec<SessionPlayer>> {
        // In a real implementation, this would fetch from the database
        // For now, return an empty vector
        Ok(Vec::new())
    }

    /// Sets or updates a configuration value in the database.
    /// If the key already exists, its value will be replaced.
    /// 
    /// Returns a `Result` indicating success or an `anyhow::Error`.
    /// 
    /// * `key` - The key of the configuration item to set.
    /// * `value` - The value to associate with the key.
    pub async fn push(&self, key: &str, value: &str) -> Result<()> {
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
