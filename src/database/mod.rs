pub mod migrations;
pub mod repositories;
pub mod validator;

use anyhow::Result;
use serenity::all::{ChannelId, UserId};
use sqlx::SqlitePool;
use tracing::info;

use crate::models::*;
use crate::models::data::Guild;
use repositories::{UserRepository, GroupRepository, ConfigRepository};
use migrations::DatabaseMigrations;

/// Main database interface that orchestrates all repositories
#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
    pub users: UserRepository,
    pub groups: GroupRepository,
    pub config: ConfigRepository,
}

impl Database {
    /// Creates a new Database instance and initializes all repositories
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
        
        // Run migrations
        let migrations = DatabaseMigrations::new(&pool);
        migrations.run_all().await?;
        
        // Initialize repositories
        let users = UserRepository::new(pool.clone());
        let groups = GroupRepository::new(pool.clone());
        let config = ConfigRepository::new(pool.clone());
        
        info!("Database connection established");
        Ok(Database { pool, users, groups, config })
    }
    
    /// Get the underlying connection pool for advanced operations
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // Backward compatibility methods - delegate to repositories
    
    /// Creates a new user in the database (backward compatibility)
    pub async fn new_user(&self, discord_id: UserId) -> Result<Player> {
        self.users.create_or_update(discord_id, Some(0)).await
    }

    /// Gets a user by Discord ID (backward compatibility)
    pub async fn get_user(&self, discord_id: UserId) -> Result<Player> {
        self.users.get_by_discord_id(discord_id).await
    }

    /// Updates a user's Steam ID (backward compatibility)
    pub async fn set_user(&self, discord_id: UserId, steam_id: Option<u64>) -> Result<Player> {
        self.users.update_steam_id(discord_id, steam_id).await
    }

    /// Creates a new group (backward compatibility)
    pub async fn new_group(
        &self,
        guild_id:         u64,
        dashboard:        u64,
        chat:             u64,
        queue:            u64,
        dashboard_msg_id: u64,
        red:              u64,
        blu:              u64,
        session_quota:    u8,
    ) -> Result<Group> {
        self.groups.create_group(guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, session_quota).await
    }

    /// Gets a group by channel ID (backward compatibility)
    pub async fn get_group_by_channel(&self, channel_id: ChannelId) -> Result<Group> {
        self.groups.get_by_channel(channel_id).await
    }

    /// Updates a group (backward compatibility)
    pub async fn set_group(
        &self,
        guild_id:      u64,
        queue_id:      u64,
        dashboard:     u64,
        chat:          u64,
        red:           u64,
        blu:           u64,
        session_quota: u8,
    ) -> Result<Group> {
        self.groups.update_group(guild_id, queue_id, dashboard, chat, red, blu, session_quota).await
    }

    /// Sets a configuration value (backward compatibility)
    pub async fn set_config(&self, key: &str, value: &str, guild_id: u64) -> Result<()> {
        self.config.set_config(key, value, guild_id).await
    }

    /// Gets configuration map for a guild (backward compatibility)
    pub async fn get_config_map(&self, guild_id: u64) -> Result<std::collections::HashMap<String, String>> {
        self.config.get_config_map(guild_id).await
    }

    /// Gets configuration for a guild (backward compatibility)
    pub async fn get_config(&self, guild_id: u64) -> Result<Guild> {
        // For now, return a simple Guild with the guild_id
        // The actual configuration is handled through get_config_map
        Ok(Guild::empty(serenity::all::GuildId::new(guild_id)))
    }
}
