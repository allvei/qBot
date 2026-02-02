use std::collections::HashMap;

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use serenity::all::{GuildId as GI, RoleId};

#[derive(Clone)]
pub struct ConfigRepository {
    pool: SqlitePool,
}

impl ConfigRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_config_map(&self, guild_id: GI) -> Result<HashMap<String, String>> {
        let row = sqlx::query("SELECT runner_id, admin_id, active_elo, default_rank FROM config WHERE guild_id = ?")
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        let mut config_map = HashMap::new();
        
        if let Some(row) = row {
            // Add runner_id if present
            if let Ok(Some(runner_id)) = row.try_get::<Option<i64>, _>("runner_id") {
                config_map.insert("runner_id".to_string(), runner_id.to_string());
            }
            
            // Add admin_id if present
            if let Ok(Some(admin_id)) = row.try_get::<Option<i64>, _>("admin_id") {
                config_map.insert("admin_id".to_string(), admin_id.to_string());
            }
            
            // Add active_elo if present
            if let Ok(Some(active_elo)) = row.try_get::<Option<i64>, _>("active_elo") {
                config_map.insert("active_elo".to_string(), active_elo.to_string());
            }
            
            // Add default_rank if present
            if let Ok(Some(default_rank)) = row.try_get::<Option<i64>, _>("default_rank") {
                config_map.insert("default_rank".to_string(), default_rank.to_string());
            }
        }

        Ok(config_map)
    }

    /// DEPRECATED: This method uses dynamic SQL with arbitrary column names that don't match the actual schema.
    /// The config table has columns: runner_id, admin_id, active_elo, default_rank (all INTEGER)
    /// But code uses this with keys like "runner_role", "admin_role", "active_elo_enabled" which don't exist.
    /// 
    /// TODO: Replace all usages with proper column-specific methods:
    /// - Use get_runner_id(), set_runner_id() for runner_id column
    /// - Use get_admin_id(), set_admin_id() for admin_id column  
    /// - Use get_active_elo(), set_active_elo() for active_elo column
    /// - Use get_default_rank_role_id(), set_default_rank_role_id() for default_rank column
    #[deprecated(note = "Use column-specific methods instead. This method doesn't match actual schema.")]
    pub async fn set_config(&self, key: &str, value: &str, guild_id: GI) -> Result<()> {
        // Build dynamic query for setting specific columns
        let query = format!("INSERT INTO config (guild_id, {}) VALUES (?, ?) ON CONFLICT(guild_id) DO UPDATE SET {} = excluded.{}", key, key, key);
        sqlx::query(&query)
        .bind(guild_id.get() as i64)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// DEPRECATED: This method uses dynamic SQL with arbitrary column names that don't match the actual schema.
    /// See set_config() deprecation note for details.
    #[deprecated(note = "Use column-specific methods instead. This method doesn't match actual schema.")]
    pub async fn get_config_item(&self, column: &str, guild_id: GI) -> Result<Option<String>> {
        // Build dynamic query - cannot use bind for column names
        let query = format!("SELECT {} FROM config WHERE guild_id = ?", column);
        let result = sqlx::query(&query)
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result.and_then(|row| {
            // Try to get as i64 first (for INTEGER columns), then as String
            if let Ok(val) = row.try_get::<i64, _>(column) {
                Some(val.to_string())
            } else {
                row.get::<Option<String>, _>(column)
            }
        }))
    }

    /// DEPRECATED: This method assumes a key-value schema that doesn't exist.
    /// The config table doesn't have a 'key' column.
    #[deprecated(note = "Config table doesn't have key-value structure. Use column-specific methods.")]
    pub async fn delete_config(&self, _key: &str, _guild_id: GI) -> Result<()> {
        // This method cannot work with the current schema
        Err(anyhow::anyhow!("delete_config is deprecated and doesn't match schema. Set column to NULL instead."))
    }

    /// Get default_rank as Discord role ID
    pub async fn get_default_rank_role_id(&self, guild_id: GI) -> Result<Option<RoleId>> {
        let row = sqlx::query("SELECT default_rank FROM config WHERE guild_id = ?")
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.and_then(|row| {
            row.try_get::<Option<i64>, _>("default_rank")
                .ok()
                .flatten()
                .map(|id| RoleId::new(id as u64))
        }))
    }

    /// Set default_rank as Discord role ID
    pub async fn set_default_rank_role_id(&self, guild_id: GI, role_id: RoleId) -> Result<()> {
        sqlx::query("INSERT INTO config (guild_id, default_rank) VALUES (?, ?) ON CONFLICT(guild_id) DO UPDATE SET default_rank = excluded.default_rank")
            .bind(guild_id.get() as i64)
            .bind(role_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // TODO: Add these column-specific methods:
    // pub async fn get_runner_id(&self, guild_id: GI) -> Result<Option<UserId>>
    // pub async fn set_runner_id(&self, guild_id: GI, user_id: UserId) -> Result<()>
    // pub async fn get_admin_id(&self, guild_id: GI) -> Result<Option<UserId>>
    // pub async fn set_admin_id(&self, guild_id: GI, user_id: UserId) -> Result<()>
    // pub async fn get_active_elo(&self, guild_id: GI) -> Result<bool>
    // pub async fn set_active_elo(&self, guild_id: GI, enabled: bool) -> Result<()>
}
