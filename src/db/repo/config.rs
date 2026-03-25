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
        let row = sqlx::query(
            "SELECT r.role_id 
             FROM config c 
             JOIN ranks r ON c.default_rank = r.id 
             WHERE c.guild_id = ?"
        )
        .bind(guild_id.get() as i64)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|row| {
            row.try_get::<Option<i64>, _>("role_id")
                .ok()
                .flatten()
                .map(|id| RoleId::new(id as u64))
        }))
    }

    /// Set default_rank as Discord role ID (stores ranks.id internally)
    pub async fn set_default_rank_role_id(&self, guild_id: GI, role_id: RoleId) -> Result<()> {
        // Find the rank ID from the role ID
        let rank_id: i64 = sqlx::query_scalar(
            "SELECT id FROM ranks WHERE guild_id = ? AND role_id = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(role_id.get() as i64)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to find rank for role ID {}: {}", role_id, e))?;
        
        sqlx::query("INSERT INTO config (guild_id, default_rank) VALUES (?, ?) ON CONFLICT(guild_id) DO UPDATE SET default_rank = excluded.default_rank")
            .bind(guild_id.get() as i64)
            .bind(rank_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get runner_id as Discord role ID
    pub async fn get_runner_role_id(&self, guild_id: GI) -> Result<Option<RoleId>> {
        let row = sqlx::query("SELECT runner_id FROM config WHERE guild_id = ?")
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.and_then(|row| {
            row.try_get::<Option<i64>, _>("runner_id")
                .ok()
                .flatten()
                .map(|id| RoleId::new(id as u64))
        }))
    }

    /// Set runner_id as Discord role ID
    pub async fn set_runner_role_id(&self, guild_id: GI, role_id: RoleId) -> Result<()> {
        sqlx::query("INSERT INTO config (guild_id, runner_id) VALUES (?, ?) ON CONFLICT(guild_id) DO UPDATE SET runner_id = excluded.runner_id")
            .bind(guild_id.get() as i64)
            .bind(role_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get admin_id as Discord role ID
    pub async fn get_admin_role_id(&self, guild_id: GI) -> Result<Option<RoleId>> {
        let row = sqlx::query("SELECT admin_id FROM config WHERE guild_id = ?")
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.and_then(|row| {
            row.try_get::<Option<i64>, _>("admin_id")
                .ok()
                .flatten()
                .map(|id| RoleId::new(id as u64))
        }))
    }

    /// Set admin_id as Discord role ID
    pub async fn set_admin_role_id(&self, guild_id: GI, role_id: RoleId) -> Result<()> {
        sqlx::query("INSERT INTO config (guild_id, admin_id) VALUES (?, ?) ON CONFLICT(guild_id) DO UPDATE SET admin_id = excluded.admin_id")
            .bind(guild_id.get() as i64)
            .bind(role_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get a boolean config column by name.
    /// `default` is returned when the row or column is NULL.
    pub async fn get_bool(&self, guild_id: GI, column: &str, default: bool) -> Result<bool> {
        let query = format!("SELECT {column} FROM config WHERE guild_id = ?");
        let row = sqlx::query(&query)
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.and_then(|row| {
            row.try_get::<Option<i64>, _>(column)
                .ok()
                .flatten()
                .map(|val| val != 0)
        }).unwrap_or(default))
    }

    /// Set a boolean config column by name.
    pub async fn set_bool(&self, guild_id: GI, column: &str, value: bool) -> Result<()> {
        let query = format!(
            "INSERT INTO config (guild_id, {column}) VALUES (?, ?) \
             ON CONFLICT(guild_id) DO UPDATE SET {column} = excluded.{column}"
        );
        sqlx::query(&query)
            .bind(guild_id.get() as i64)
            .bind(value as i32)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get active_elo setting
    pub async fn get_active_elo(&self, guild_id: GI) -> Result<bool> {
        self.get_bool(guild_id, "active_elo", false).await
    }

    /// Set active_elo setting
    pub async fn set_active_elo(&self, guild_id: GI, enabled: bool) -> Result<()> {
        self.set_bool(guild_id, "active_elo", enabled).await
    }

    /// Get elo_ranks_linked setting (default: true = ELO and ranks are coupled)
    pub async fn get_elo_ranks_linked(&self, guild_id: GI) -> Result<bool> {
        self.get_bool(guild_id, "elo_ranks_linked", true).await
    }

    /// Set elo_ranks_linked setting
    pub async fn set_elo_ranks_linked(&self, guild_id: GI, linked: bool) -> Result<()> {
        self.set_bool(guild_id, "elo_ranks_linked", linked).await
    }

    /// Get post_game_confirm_time setting (in seconds)
    pub async fn get_post_game_confirm_time(&self, guild_id: GI) -> Result<u16> {
        let query = "SELECT post_game_confirm_time FROM config WHERE guild_id = ?";
        let row = sqlx::query(query)
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.and_then(|row| {
            row.try_get::<Option<i64>, _>("post_game_confirm_time")
                .ok()
                .flatten()
                .map(|val| val as u16)
        }).unwrap_or(crate::DEFAULT_POST_GAME_CONFIRM_TIME))
    }

    /// Set post_game_confirm_time setting (in seconds)
    pub async fn set_post_game_confirm_time(&self, guild_id: GI, confirm_time: u16) -> Result<()> {
        let query = "INSERT INTO config (guild_id, post_game_confirm_time) VALUES (?, ?) ON CONFLICT(guild_id) DO UPDATE SET post_game_confirm_time = excluded.post_game_confirm_time";
        sqlx::query(query)
            .bind(guild_id.get() as i64)
            .bind(confirm_time as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
