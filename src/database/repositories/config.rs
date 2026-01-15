use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use sqlx::{Row, SqlitePool};
use tracing::error;
use serenity::all::GuildId as GI;

use super::Repository;
use crate::models::ConfigFormat;

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

    pub async fn set_config(&self, key: &str, value: &str, guild_id: GI) -> Result<()> {
        let query_result = sqlx::query("INSERT OR REPLACE INTO config (guild, key, value) VALUES (?, ?, ?)")
        .bind(guild_id.get() as i64)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await;

        match query_result {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to set config: {e}");
                Err(e.into())
            }
        }
    }

    pub async fn get_config_item(&self, column: &str, guild_id: GI) -> Result<Option<String>> {
        let result = sqlx::query("SELECT ? FROM config WHERE guild_id = ?")
            .bind(column)
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result.and_then(|row| row.get::<Option<String>, _>(column)))
    }

    pub async fn delete_config(&self, key: &str, guild_id: GI) -> Result<()> {
        sqlx::query("DELETE FROM config WHERE guild_id = ? AND key = ?")
            .bind(guild_id.get() as i64)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_all_for_guild(&self, guild_id: GI) -> Result<Vec<ConfigFormat>> {
        let rows = sqlx::query_as::<_, ConfigFormat>("SELECT key, value, description FROM config WHERE guild_id = ?")
        .bind(guild_id.get() as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}

#[async_trait]
impl Repository<ConfigFormat, (GI, String)> for ConfigRepository {
    async fn create(&self, _config : &ConfigFormat) -> Result<ConfigFormat> {
        // This implementation assumes guild_id is passed separately
        // In a real implementation, you might want to modify ConfigFormat to include guild_id
        Err(anyhow::anyhow!("Use set_config method instead"))
    }

    async fn get_by_id(&self, (guild_id, key): (GI, String)) -> Result<ConfigFormat> {
        let result = sqlx::query_as::<_, ConfigFormat>("SELECT key, value, description FROM config WHERE guild_id = ? AND key = ?")
        .bind(guild_id.get() as i64)
        .bind(&key)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    async fn update(&self, _config: &ConfigFormat) -> Result<ConfigFormat> {
        // This would need guild_id to be part of ConfigFormat or passed separately
        Err(anyhow::anyhow!("Use set_config method instead"))
    }

    async fn delete(&self, (guild_id, key): (GI, String)) -> Result<()> {
        self.delete_config(&key, guild_id).await
    }
}
