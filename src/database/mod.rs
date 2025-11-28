pub mod migrations;
pub mod repositories;
pub mod validator;

use anyhow::Result;
use serenity::all::{Context as Ctx, UserId as UI};
use sqlx::SqlitePool;
use tracing::info;

use crate::models::{FileManager, Group, Player, Server};
use migrations::DatabaseMigrations;
use repositories::{ConfigRepository, GroupRepository, UserRepository};

/// Main database interface that orchestrates all repositories
#[derive(Clone)]
pub struct Database {
    pool:       SqlitePool,
    pub users:  UserRepository,
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
        migrations.create_tables().await?;

        // Initialize repositories
        let users  = UserRepository  ::new(pool.clone());
        let groups = GroupRepository ::new(pool.clone());
        let config = ConfigRepository::new(pool.clone());

        Ok(Database { pool, users, groups, config })
    }

    /// Get the underlying connection pool for advanced operations
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // Backward compatibility methods - delegate to repositories

    /// Creates a new user in the database
    pub async fn new_user(&self, user_id: UI, ctx: &Ctx) -> Result<Player> {
        self.users.create_or_update_with_tag(user_id, Some(0), ctx).await
    }

    /// Gets a Player by Discord ID
    pub async fn get_user(&self, user_id: UI, ctx: &Ctx) -> Result<Player> {
        self.users.get_with_tag(user_id, ctx).await
    }

    /// Gets a Player by Discord ID
    pub async fn get_user_with_display_name(&self, user_id: UI, ctx: &Ctx, guild_id: Option<serenity::all::GuildId>) -> Result<Player> {
        self.users.get_user_with_nick(user_id, ctx, guild_id).await
    }

    /// Updates a user's Steam ID
    pub async fn set_player_steam_id(&self, user_id: &UI, steam_id: Option<u64>) -> Result<Player> {
        self.users.update_steam_id(user_id, steam_id).await
    }

    /// Creates a new group
    #[allow(clippy::too_many_arguments)]
    pub async fn new_group(
        &self,
        guild_id:      u64,
        dashboard:     u64,
        chat:          u64,
        queue:         u64,
        dashboard_msg: u64,
        red:           u64,
        blu:           u64,
        quota:         u8,
    ) -> Result<Group> {
        let config = repositories::group::GroupConfig {
            dashboard_channel_id: dashboard,
            chat_channel_id: chat,
            queue_vc_id: queue,
            red_vc_id: red,
            blu_vc_id: blu,
            quota,
        };
        self.groups.create_group(guild_id, dashboard_msg, config).await
    }

    /// Updates a group
    #[allow(clippy::too_many_arguments)]
    pub async fn set_group(
        &self,
        guild_id:      u64,
        queue_id:      u64,
        dashboard:     u64,
        chat:          u64,
        red:           u64,
        blu:           u64,
        quota:         u8,
    ) -> Result<Group> {
        let config = repositories::group::GroupConfig {
            dashboard_channel_id: dashboard,
            chat_channel_id: chat,
            queue_vc_id: queue_id,
            red_vc_id: red,
            blu_vc_id: blu,
            quota,
        };
        self.groups.update_group(guild_id, config).await
    }

    /// Sets a configuration value
    pub async fn set_config(&self, key: &str, value: &str, guild_id: u64) -> Result<()> {
        self.config.set_config(key, value, guild_id).await
    }

    /// Gets configuration map for a guild
    pub async fn get_config_map(&self, guild_id: u64) -> Result<std::collections::HashMap<String, String>> {
        self.config.get_config_map(guild_id).await
    }

    /// Gets configuration for a guild
    pub async fn get_config(&self, guild_id: u64) -> Result<Server> {
        // For now, return a simple Guild with the guild_id
        // The actual configuration is handled through get_config_map
        Ok(Server::empty(serenity::all::GuildId::new(guild_id), "Unknown".to_string()))
    }
}
