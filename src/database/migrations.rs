use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::info;

/// Database migration system for managing schema changes
pub struct DatabaseMigrations {
    pool: SqlitePool,
}

impl DatabaseMigrations {
    pub fn new(pool: &SqlitePool) -> Self {
        Self {
            pool: pool.clone(),
        }
    }

    /// Run all migrations in order
    pub async fn run_all(&self) -> Result<()> {
        info!("Running database migrations");
        
        self.create_config_table().await?;
        self.create_users_table().await?;
        self.create_groups_table().await?;
        self.create_teams_table().await?;
        
        info!("All migrations completed successfully");
        Ok(())
    }

    /// Check if table exists
    async fn table_exists(&self, table_name: &str) -> Result<bool> {
        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name=?"
        )
        .bind(table_name)
        .fetch_all(&self.pool)
        .await?;
        
        Ok(!result.is_empty())
    }

    /// Check if column exists in table
    async fn column_exists(&self, table_name: &str, column_name: &str) -> Result<bool> {
        let existing_cols: Vec<String> = sqlx::query(&format!("PRAGMA table_info({})", table_name))
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("name").ok())
            .collect();
        
        Ok(existing_cols.contains(&column_name.to_string()))
    }

    /// Create config table with proper schema
    async fn create_config_table(&self) -> Result<()> {
        if !self.table_exists("config").await? {
            info!("Config table not found, creating...");
            sqlx::query(
                "CREATE TABLE config (
                    guild    INTEGER NOT NULL,
                    key      TEXT NOT NULL,
                    value    TEXT,
                    description TEXT,
                    PRIMARY KEY(guild, key)
                )"
            )
            .execute(&self.pool)
            .await?;
        } else if !self.column_exists("config", "guild").await? {
            info!("Adding missing guild column to config table");
            sqlx::query("ALTER TABLE config ADD COLUMN guild INTEGER NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Create users table with proper schema and constraints
    async fn create_users_table(&self) -> Result<()> {
        if !self.table_exists("users").await? {
            info!("Users table not found, creating...");
            sqlx::query(
                "CREATE TABLE users (
                    id INTEGER PRIMARY KEY,
                    discord_id INTEGER NOT NULL UNIQUE,
                    steam_id INTEGER
                )"
            )
            .execute(&self.pool)
            .await?;
        } else {
            // Verify schema integrity
            let has_unique_constraint = self.check_unique_constraint("users", "discord_id").await?;
            let has_discord_id = self.column_exists("users", "discord_id").await?;
            let has_steam_id = self.column_exists("users", "steam_id").await?;
            
            if !has_discord_id || !has_steam_id || !has_unique_constraint {
                info!("Users table schema is incomplete, recreating with proper schema...");
                
                // Backup existing data if any
                let backup_data = if has_discord_id {
                    sqlx::query("SELECT discord_id, steam_id FROM users")
                        .fetch_all(&self.pool)
                        .await
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                
                // Drop and recreate table
                sqlx::query("DROP TABLE users").execute(&self.pool).await?;
                sqlx::query(
                    "CREATE TABLE users (
                        id INTEGER PRIMARY KEY,
                        discord_id INTEGER NOT NULL UNIQUE,
                        steam_id INTEGER
                    )"
                )
                .execute(&self.pool)
                .await?;
                
                // Restore data if we had any
                for row in backup_data {
                    let discord_id: i64 = row.get("discord_id");
                    let steam_id: Option<i64> = row.try_get("steam_id").ok();
                    sqlx::query("INSERT OR IGNORE INTO users (discord_id, steam_id) VALUES (?, ?)")
                        .bind(discord_id)
                        .bind(steam_id)
                        .execute(&self.pool)
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// Create groups table with proper schema
    async fn create_groups_table(&self) -> Result<()> {
        if !self.table_exists("groups").await? {
            info!("Groups table not found, creating...");
            sqlx::query(
                "CREATE TABLE groups (
                    id INTEGER PRIMARY KEY,
                    group_id INTEGER DEFAULT 0,
                    timeout INTEGER DEFAULT 120,
                    guild_id INTEGER NOT NULL,
                    dashboard INTEGER NOT NULL,
                    chat INTEGER NOT NULL,
                    queue INTEGER NOT NULL,
                    dashboard_msg_id INTEGER DEFAULT 0,
                    red INTEGER NOT NULL,
                    blu INTEGER NOT NULL,
                    session INTEGER DEFAULT 0,
                    session_increment INTEGER DEFAULT 0,
                    session_quota INTEGER DEFAULT 10
                )"
            )
            .execute(&self.pool)
            .await?;
        } else {
            // Check if essential columns exist
            let has_guild_id = self.column_exists("groups", "guild_id").await?;
            
            if !has_guild_id {
                info!("Groups table schema is incorrect, recreating...");
                sqlx::query("DROP TABLE groups").execute(&self.pool).await?;
                sqlx::query(
                    "CREATE TABLE groups (
                        id INTEGER PRIMARY KEY,
                        group_id INTEGER DEFAULT 0,
                        timeout INTEGER DEFAULT 120,
                        guild_id INTEGER NOT NULL,
                        dashboard INTEGER NOT NULL,
                        chat INTEGER NOT NULL,
                        queue INTEGER NOT NULL,
                        dashboard_msg_id INTEGER DEFAULT 0,
                        red INTEGER NOT NULL,
                        blu INTEGER NOT NULL,
                        session INTEGER DEFAULT 0,
                        session_increment INTEGER DEFAULT 0,
                        session_quota INTEGER DEFAULT 10
                    )"
                )
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn create_teams_table(&self) -> Result<()> {
        if !self.table_exists("teams").await? {
            info!("Teams table not found, creating...");
            sqlx::query(
                "CREATE TABLE teams (
                    id INTEGER PRIMARY KEY,
                    guild_id INTEGER NOT NULL,
                    group_id INTEGER NOT NULL,
                    red INTEGER NOT NULL,
                    blu INTEGER NOT NULL
                )"
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Check if a unique constraint exists on a column
    async fn check_unique_constraint(&self, table: &str, _column: &str) -> Result<bool> {
        let index_info = sqlx::query(&format!("PRAGMA index_list({})", table))
            .fetch_all(&self.pool)
            .await?;
        
        let has_unique = index_info.iter().any(|row| {
            if let Ok(unique) = row.try_get::<i64, _>("unique") {
                unique == 1
            } else {
                false
            }
        });
        
        Ok(has_unique)
    }

    /// Validate database schema integrity
    pub async fn validate_schema(&self) -> Result<()> {
        info!("Validating database schema integrity");
        
        // Validate config table
        self.validate_config_schema().await?;
        
        // Validate users table
        self.validate_users_schema().await?;
        
        // Validate groups table
        self.validate_groups_schema().await?;
        
        info!("Database schema validation completed successfully");
        Ok(())
    }

    /// Validate config table schema
    async fn validate_config_schema(&self) -> Result<()> {
        let required_columns = vec!["guild", "key", "value", "description"];
        self.validate_table_columns("config", &required_columns).await?;
        Ok(())
    }

    /// Validate users table schema
    async fn validate_users_schema(&self) -> Result<()> {
        let required_columns = vec!["id", "discord_id", "steam_id"];
        self.validate_table_columns("users", &required_columns).await?;
        Ok(())
    }

    /// Validate groups table schema
    async fn validate_groups_schema(&self) -> Result<()> {
        let required_columns = vec![
            "id", "group_id", "timeout", "guild_id", "dashboard", 
            "chat", "queue", "dashboard_msg_id", "red", "blu", 
            "session", "session_increment", "session_quota"
        ];
        self.validate_table_columns("groups", &required_columns).await?;
        Ok(())
    }

    /// Validate that a table has all required columns
    async fn validate_table_columns(&self, table_name: &str, required_columns: &[&str]) -> Result<()> {
        let existing_cols: Vec<String> = sqlx::query(&format!("PRAGMA table_info({})", table_name))
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("name").ok())
            .collect();
        
        for required_col in required_columns {
            if !existing_cols.contains(&required_col.to_string()) {
                return Err(anyhow::anyhow!(
                    "Missing required column '{}' in table '{}'", 
                    required_col, 
                    table_name
                ));
            }
        }
        
        info!("Table '{}' has all required columns", table_name);
        Ok(())
    }

    /// Validate that essential group records exist
    pub async fn validate_group_entries(&self, required_guild_ids: &[u64]) -> Result<()> {
        info!("Validating group entries for {} guilds", required_guild_ids.len());
        
        for guild_id in required_guild_ids {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM groups WHERE guild_id = ?"
            )
            .bind(*guild_id as i64)
            .fetch_one(&self.pool)
            .await?;
            
            if count == 0 {
                return Err(anyhow::anyhow!(
                    "No group configuration found for guild_id: {}. Please create a group record.", 
                    guild_id
                ));
            }
            
            info!("Guild {} has {} group(s) configured", guild_id, count);
        }
        
        Ok(())
    }

    /// Create a default group entry for a guild if none exists
    pub async fn ensure_default_group(&self, guild_id: u64) -> Result<()> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM groups WHERE guild_id = ?"
        )
        .bind(guild_id as i64)
        .fetch_one(&self.pool)
        .await?;
        
        if count == 0 {
            info!("Creating default group for guild_id: {}", guild_id);
            sqlx::query(
                "INSERT INTO groups (group_id, guild_id, dashboard, chat, queue, red, blu) 
                 VALUES (1, ?, 1, 1, 1, 1, 1)"
            )
            .bind(guild_id as i64)
            .execute(&self.pool)
            .await?;
            
            info!("Default group created for guild_id: {} (requires manual configuration)", guild_id);
        }
        
        Ok(())
    }
}
